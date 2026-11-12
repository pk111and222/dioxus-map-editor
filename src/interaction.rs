//! Bounded, renderer-independent gesture prototype. No history, storage or IO.

use crate::geometry::{Point, Viewport};
use std::fmt::Write;

/// Maximum samples in one prototype stroke.
pub const MAX_STROKE_POINTS: usize = 8192;

/// Tools available in the stage-one experiment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    /// Freehand polyline preview.
    #[default]
    Brush,
    /// Translate the viewport.
    Pan,
}

/// Why a gesture was discarded. Never commits partial geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelReason {
    /// Escape pressed within the active canvas.
    Escape,
    /// Native pointercancel.
    PointerCancel,
    /// The pointer exited the full-window overlay.
    WindowBoundary,
    /// Window blur, hidden tab, or focus left the active canvas.
    FocusLost,
    /// Scrolling, resizing, or host layout invalidation during the gesture.
    LayoutChanged,
    /// A later move reports no pressed button (release was missed).
    LostRelease,
    /// Tool switched or host explicitly reset the prototype.
    Reset,
}

/// A completed ephemeral stroke with a cached SVG path.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    /// World-space samples; Y points upwards.
    pub points: Vec<Point>,
    /// Cached path for the static scene.
    pub path: String,
}

impl Stroke {
    /// Cache a finite sequence as SVG. Used by the deterministic load generator.
    pub fn from_points(points: Vec<Point>) -> Option<Self> {
        if points.is_empty() || points.iter().any(|p| !p.is_finite()) {
            return None;
        }
        let mut path = String::new();
        for (index, point) in points.iter().enumerate() {
            append_point(&mut path, *point, index == 0);
        }
        Some(Self { points, path })
    }
}

#[derive(Debug)]
struct Gesture {
    pointer: i32,
    tool: Tool,
    start_view: Viewport,
    previous: Point,
    points: Vec<Point>,
    path: String,
}

/// Terminal result; a UI appends a stroke only for `Stroke`.
#[derive(Debug, PartialEq)]
pub enum GestureResult {
    /// No active matching pointer or a pan completed.
    None,
    /// One completed stroke.
    Stroke(Stroke),
    /// Preview discarded; a pan restores its initial transform.
    Cancelled(CancelReason),
    /// Sample budget reached; this stroke completed safely.
    BudgetReached(Stroke),
}

/// Minimal single-pointer interaction state, independent of any UI renderer.
#[derive(Debug, Default)]
pub struct Interaction {
    active: Option<Gesture>,
}

impl Interaction {
    /// Whether a gesture owns this canvas.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Current owning pointer, if any.
    pub fn pointer_id(&self) -> Option<i32> {
        self.active.as_ref().map(|g| g.pointer)
    }

    /// Borrow the incremental preview path without copying committed content.
    pub fn preview(&self) -> &str {
        self.active.as_ref().map_or("", |g| g.path.as_str())
    }
    /// Borrow the active world samples for a tool-specific transient preview.
    pub fn preview_points(&self) -> &[Point] {
        self.active
            .as_ref()
            .map(|g| g.points.as_slice())
            .unwrap_or_default()
    }

    /// Number of preview nodes.
    pub fn preview_nodes(&self) -> usize {
        self.active.as_ref().map_or(0, |g| g.points.len())
    }

    /// Start a primary/middle-button gesture using canvas-local coordinates.
    /// A second pointer cannot replace an existing gesture.
    pub fn begin(&mut self, pointer: i32, tool: Tool, local: Point, view: Viewport) -> bool {
        if self.is_active() || !local.is_finite() {
            return false;
        }
        let mut gesture = Gesture {
            pointer,
            tool,
            start_view: view,
            previous: local,
            points: Vec::new(),
            path: String::new(),
        };
        if tool == Tool::Brush {
            let world = view.screen_to_world(local);
            if !world.is_finite() {
                return false;
            }
            gesture.points.push(world);
            append_point(&mut gesture.path, world, true);
        }
        self.active = Some(gesture);
        true
    }

    /// Update only the owning pointer. Missing buttons cancel before sampling.
    /// Samples closer than 0.75 screen pixels are filtered.
    pub fn update(
        &mut self,
        pointer: i32,
        local: Point,
        button_held: bool,
        view: &mut Viewport,
    ) -> GestureResult {
        if self.pointer_id() != Some(pointer) {
            return GestureResult::None;
        }
        if !button_held {
            return self.cancel(CancelReason::LostRelease, view);
        }
        if !local.is_finite() {
            return GestureResult::None;
        }
        if let Some(g) = self.active.as_mut() {
            match g.tool {
                Tool::Brush => {
                    if local.distance(g.previous) < 0.75 {
                        return GestureResult::None;
                    }
                    let point = view.screen_to_world(local);
                    if !point.is_finite() {
                        return GestureResult::None;
                    }
                    g.points.push(point);
                    append_point(&mut g.path, point, false);
                }
                Tool::Pan => {
                    let _ = view.pan(Point::new(local.x - g.previous.x, local.y - g.previous.y));
                }
            }
            g.previous = local;
            if g.points.len() >= MAX_STROKE_POINTS {
                if let GestureResult::Stroke(stroke) = self.finish(pointer, local, view) {
                    return GestureResult::BudgetReached(stroke);
                }
            }
        }
        GestureResult::None
    }

    /// Complete exactly once, retaining the release endpoint. Foreign releases
    /// leave the owning gesture intact. Clicks render as a tiny round segment.
    pub fn finish(&mut self, pointer: i32, local: Point, view: &mut Viewport) -> GestureResult {
        if self.pointer_id() != Some(pointer) {
            return GestureResult::None;
        }
        let Some(mut g) = self.active.take() else {
            return GestureResult::None;
        };
        if g.tool == Tool::Pan {
            if local.is_finite() {
                let _ = view.pan(Point::new(local.x - g.previous.x, local.y - g.previous.y));
            }
            return GestureResult::None;
        }
        if local.is_finite()
            && local.distance(g.previous) > 0.0
            && g.points.len() < MAX_STROKE_POINTS
        {
            let point = view.screen_to_world(local);
            if point.is_finite() {
                g.points.push(point);
                append_point(&mut g.path, point, false);
            }
        }
        if g.points.len() == 1 {
            let point = g.points[0];
            append_point(&mut g.path, Point::new(point.x + 0.001, point.y), false);
        }
        GestureResult::Stroke(Stroke {
            points: g.points,
            path: g.path,
        })
    }

    /// Discard preview; restore the viewport if an unfinished pan was cancelled.
    pub fn cancel(&mut self, reason: CancelReason, view: &mut Viewport) -> GestureResult {
        let Some(g) = self.active.take() else {
            return GestureResult::None;
        };
        if g.tool == Tool::Pan {
            *view = g.start_view;
        }
        GestureResult::Cancelled(reason)
    }
}

fn append_point(path: &mut String, point: Point, first: bool) {
    // Writing into String is infallible; no user-controlled markup is accepted.
    let _ = write!(
        path,
        "{} {:.3} {:.3} ",
        if first { 'M' } else { 'L' },
        point.x,
        point.y
    );
}
