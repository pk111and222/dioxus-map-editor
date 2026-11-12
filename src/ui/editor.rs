//! Controlled canvas and default editor. No renderer, filesystem, or global hooks.
use crate::{
    command::Command,
    document::*,
    editor::*,
    error::{MapError, MapResult},
    geometry::*,
    interaction::{CancelReason, GestureResult, Interaction, Tool},
    registry::Registry,
    scene,
    session::*,
};
use dioxus::{html::input_data::MouseButton, prelude::*};
use std::{
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
static INSTANCE: AtomicU64 = AtomicU64::new(1);
/// Default editor UI CSS, embedded in the published crate.
pub const EDITOR_CSS: &str = include_str!("../../assets/editor.css");

#[derive(Clone, Copy, Debug, PartialEq)]
enum DragKind {
    Move,
    Scale,
    Rotate,
    Marquee,
    Node(usize, Option<bool>),
}
#[derive(Clone, Debug)]
struct Drag {
    pointer: i32,
    kind: DragKind,
    start: Point,
    current: Point,
    pivot: Point,
    stamp: SessionStamp,
    original: Vec<MapObject>,
    shift: bool,
}
#[derive(Clone, Debug)]
struct TextDraft {
    id: u64,
    text: String,
    position: Point,
    object: Option<ObjectId>,
    stamp: SessionStamp,
    composing: bool,
}
/// Transient text state for host-owned input controls; never persisted until commit.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasTextDraft {
    /// Stable identity for keying a native input during composition.
    pub id: u64,
    /// Current input value.
    pub text: String,
    /// World position.
    pub position: Point,
    /// Whether an IME composition is active.
    pub composing: bool,
}
/// Default text-overlay labels. Hosts can replace the entire overlay instead.
#[derive(Clone, Debug, PartialEq)]
pub struct TextEditorLabels {
    /// Accessible input name.
    pub input: String,
    /// Commit button caption.
    pub commit: String,
    /// Cancel button caption.
    pub cancel: String,
}
impl Default for TextEditorLabels {
    fn default() -> Self {
        Self {
            input: "Canvas text".into(),
            commit: "Apply".into(),
            cancel: "Cancel".into(),
        }
    }
}
/// Rust ref-style control handle. Use only while its canvas is mounted.
/// Methods use the same session validation, history and host transaction boundary.
#[derive(Clone, Copy, PartialEq)]
pub struct MapCanvasHandle {
    canvas: Canvas,
}
impl MapCanvasHandle {
    /// Authoritative session for custom menus, history and persistence.
    pub fn session(self) -> Signal<MapSession> {
        self.canvas.session
    }
    /// Host-editable tool preferences and selection.
    pub fn tools(self) -> Signal<EditorTools> {
        self.canvas.tools
    }
    /// Host-editable style and symbol catalogue.
    pub fn registry(self) -> Signal<Registry> {
        self.canvas.registry
    }
    /// Host-editable viewport, independent from saved content.
    pub fn viewport(self) -> Signal<Viewport> {
        self.canvas.view
    }
    /// Read reactive text state from a host component.
    pub fn text_draft(self) -> Option<CanvasTextDraft> {
        self.canvas.text.read().as_ref().map(|d| CanvasTextDraft {
            id: d.id,
            text: d.text.clone(),
            position: d.position,
            composing: d.composing,
        })
    }
    /// Update a draft from a custom input, without rewriting the native input value.
    pub fn set_text(mut self, value: String) {
        if let Some(d) = self.canvas.text.write().as_mut() {
            d.text = value;
        }
    }
    /// Notify composition boundaries; commit is ignored while composing.
    pub fn set_composing(mut self, value: bool) {
        if let Some(d) = self.canvas.text.write().as_mut() {
            d.composing = value;
        }
    }
    /// Submit the current text draft.
    pub fn commit_text(self) {
        self.canvas.commit_text();
    }
    /// Discard active pointer, path and text drafts.
    pub fn cancel(self) {
        self.canvas.cancel();
    }
    /// End a multi-click path; valid nodes are already committed.
    pub fn finish_path(self) {
        self.canvas.finish_path();
    }
    /// Undo through the same host transaction used by canvas edits.
    pub fn undo(self) {
        self.canvas.history(false);
    }
    /// Redo through the same host transaction used by canvas edits.
    pub fn redo(self) {
        self.canvas.history(true);
    }
    /// Submit host-created commands atomically.
    pub fn execute(self, commands: Vec<Command>) {
        self.canvas.execute(commands);
    }
    /// Restore keyboard focus explicitly after a host action.
    pub fn focus(self) {
        self.canvas.focus();
    }
    /// Fit the finite document frame to the mounted canvas without changing content.
    pub fn fit_frame(self) {
        let c = self.canvas;
        let element = c.mounted.peek().clone();
        if let Some(element) = element {
            spawn(async move {
                if let Ok(rect) = element.get_client_rect().await {
                    if rect.size.width <= 48. || rect.size.height <= 48. {
                        return;
                    }
                    let frame = c.session.peek().snapshot().document.frame;
                    if let Ok(next) = fit_view(frame, rect.size.width, rect.size.height) {
                        let mut view = c.view;
                        view.set(next);
                    }
                }
            });
        }
    }
}
/// Pointer boundary policy, independent of renderer and document storage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasInteractionOptions {
    /// Clip the scene to the export frame and constrain new drawing samples.
    pub constrain_to_frame: bool,
    /// Preserve a held gesture across pointer exits. Focus loss still cancels.
    pub resume_on_reentry: bool,
    /// Canvas-scoped undo/redo shortcuts and temporary Space pan.
    pub keyboard_shortcuts: bool,
    /// Require Control for wheel zoom; unmodified wheel scrolls the host.
    pub ctrl_wheel_zoom: bool,
}
impl Default for CanvasInteractionOptions {
    fn default() -> Self {
        Self {
            constrain_to_frame: true,
            resume_on_reentry: true,
            keyboard_shortcuts: true,
            ctrl_wheel_zoom: true,
        }
    }
}
#[derive(Clone, Copy, PartialEq)]
struct Canvas {
    session: Signal<MapSession>,
    tools: Signal<EditorTools>,
    registry: Signal<Registry>,
    view: Signal<Viewport>,
    interaction: Signal<Interaction>,
    drag: Signal<Option<Drag>>,
    text: Signal<Option<TextDraft>>,
    path: Signal<PathDraft>,
    origin: Signal<Point>,
    button: Signal<MouseButton>,
    active: Signal<bool>,
    ticket: Signal<Option<GestureTicket>>,
    message: Signal<String>,
    node: Signal<Option<usize>>,
    mounted: Signal<Option<Rc<MountedData>>>,
    hover: Signal<Option<Point>>,
    space: Signal<bool>,
    host: Option<Callback<Vec<crate::command::AnchorPatch>, MapResult<()>>>,
    activate: EventHandler<AnchorId>,
    options: CanvasInteractionOptions,
    onerror: EventHandler<MapError>,
}
struct HostPolicy(Option<Callback<Vec<crate::command::AnchorPatch>, MapResult<()>>>);
impl AnchorTransaction for HostPolicy {
    fn accept(&mut self, patches: &[crate::command::AnchorPatch]) -> MapResult<()> {
        if patches.is_empty() {
            Ok(())
        } else if let Some(callback) = self.0 {
            callback.call(patches.to_vec())
        } else {
            Err(MapError::HostRejected("no host anchor adapter".into()))
        }
    }
}
impl Canvas {
    fn constrain(self, local: Point) -> Point {
        if !self.options.constrain_to_frame {
            return local;
        }
        let view = self.view.peek();
        let p = view.screen_to_world(local);
        let f = self.session.peek().snapshot().document.frame;
        view.world_to_screen(Point::new(
            p.x.clamp(f.min.x, f.min.x + f.width),
            p.y.clamp(f.min.y, f.min.y + f.height),
        ))
    }
    fn outside_frame(self, local: Point) -> bool {
        self.options.constrain_to_frame && self.constrain(local).distance(local) > 0.001
    }
    fn focus(self) {
        let element = self.mounted.peek().clone();
        if let Some(element) = element {
            spawn(async move {
                let _ = element.set_focus(true).await;
            });
        }
    }

    fn report_error(mut self, error: MapError) {
        self.message.set(error.to_string());
        self.onerror.call(error);
    }
    fn error(mut self, result: MapResult<CommitOutcome>) {
        if let Err(error) = &result {
            self.onerror.call(error.clone());
        }
        self.message.set(match result {
            Ok(CommitOutcome::NoOp) => "无内容变更".into(),
            Ok(CommitOutcome::Committed(r)) => format!("已提交 revision {r}"),
            Err(e) => e.to_string(),
        });
    }
    fn execute(mut self, commands: Vec<Command>) {
        let result = self
            .session
            .write()
            .execute_with_host(commands, &mut HostPolicy(self.host));
        self.error(result);
    }
    fn cancel_pointer(mut self) {
        self.hover.set(None);
        let mut view = *self.view.peek();
        self.interaction
            .write()
            .cancel(CancelReason::Escape, &mut view);
        if view != *self.view.peek() {
            self.view.set(view);
        }
        self.drag.set(None);
        self.active.set(false);
        if let Some(t) = self.ticket.write().take() {
            let _ = self.session.write().cancel_gesture(&t);
        }
        self.path.write().cancel();
    }
    fn cancel(mut self) {
        self.cancel_pointer();
        self.text.set(None);
    }
    fn local(self, event: &PointerEvent) -> Point {
        let p = event.client_coordinates();
        let origin = *self.origin.peek();
        Point::new(p.x - origin.x, p.y - origin.y)
    }
    fn commit(mut self, commands: MapResult<Vec<Command>>) {
        let result = match commands {
            Err(e) => {
                if let Some(t) = self.ticket.write().take() {
                    let _ = self.session.write().cancel_gesture(&t);
                }
                Err(e)
            }
            Ok(commands) => {
                if let Some(t) = self.ticket.write().take() {
                    let mut session = self.session.write();
                    session
                        .update_gesture(&t, commands)
                        .and_then(|()| session.commit_gesture(&t, &mut HostPolicy(self.host)))
                        .inspect_err(|_| {
                            let _ = session.cancel_gesture(&t);
                        })
                } else {
                    self.session
                        .write()
                        .execute_with_host(commands, &mut HostPolicy(self.host))
                }
            }
        };
        self.error(result);
        self.active.set(false);
        self.drag.set(None);
        self.focus();
    }
    fn begin_content(mut self) -> bool {
        let result = self.session.write().begin_gesture();
        match result {
            Ok(t) => {
                self.ticket.set(Some(t));
                true
            }
            Err(e) => {
                self.report_error(e);
                false
            }
        }
    }
    fn result(mut self, result: GestureResult) {
        match result {
            GestureResult::Stroke(s) | GestureResult::BudgetReached(s) => {
                if self.tools.peek().tool == EditorTool::Scatter {
                    let commands = {
                        let session = self.session.peek();
                        crate::map::scatter(
                            session.snapshot(),
                            session.stamp().revision,
                            &self.tools.peek(),
                            &s.points,
                            &self.tools.peek().scatter,
                            &session.limits(),
                        )
                    };
                    self.commit(commands);
                    return;
                }
                if let Some(key) = self.tools.peek().brush.clone() {
                    let commands = {
                        let session = self.session.peek();
                        let registry = self.registry.peek();
                        let tools = self.tools.peek();
                        registry
                            .brush(&key)
                            .ok_or_else(|| MapError::Missing(key.key.clone()))
                            .and_then(|brush| {
                                brush.commands(crate::plugin::BrushInput {
                                    snapshot: session.snapshot(),
                                    revision: session.stamp().revision,
                                    tools: &tools,
                                    points: &s.points,
                                    limits: &session.limits(),
                                })
                            })
                    };
                    self.commit(commands);
                    return;
                }
                self.path.write().cancel();
                let commands = {
                    let session = self.session.peek();
                    let tools = self.tools.peek();
                    let mut samples = s.points.clone();
                    if matches!(tools.paint.role.as_str(), "road" | "trail") {
                        for index in [0, samples.len() - 1] {
                            samples[index] = crate::map::snap_endpoint(
                                session.snapshot(),
                                samples[index],
                                self.view.peek().scale(),
                            );
                        }
                    }
                    let width = self
                        .registry
                        .peek()
                        .paint(&session.snapshot().document.style, &tools.paint)
                        .width;
                    brush_commands(
                        session.snapshot(),
                        session.stamp().revision,
                        &tools,
                        &samples,
                        width,
                        &session.limits(),
                    )
                };
                self.commit(commands);
            }
            GestureResult::Cancelled(_) => self.cancel(),
            GestureResult::None => {
                if !self.interaction.peek().is_active() && self.drag.peek().is_none() {
                    self.active.set(false);
                }
            }
        }
    }
    fn move_pointer(mut self, event: PointerEvent) {
        if !*self.active.peek() {
            let p = event.element_coordinates();
            let local = Point::new(p.x, p.y);
            self.hover
                .set((!self.outside_frame(local)).then(|| self.view.peek().screen_to_world(local)));
            return;
        }
        if let Some(d) = self.drag.peek().as_ref() {
            if d.pointer != event.pointer_id() {
                return;
            }
        }
        if !event.held_buttons().contains(*self.button.peek()) {
            self.cancel();
            return;
        }
        let p = self.local(&event);
        if !p.is_finite() {
            return;
        }
        self.hover
            .set((!self.outside_frame(p)).then(|| self.view.peek().screen_to_world(p)));
        if matches!(
            self.tools.peek().tool,
            EditorTool::Brush | EditorTool::Scatter | EditorTool::Path
        ) && self.ticket.peek().is_some()
            && self.outside_frame(p)
        {
            // Keep the gesture ticket, but do not append off-map samples.
            return;
        }
        if self.drag.peek().is_some() {
            let world = self.view.peek().screen_to_world(p);
            if let Some(d) = self.drag.write().as_mut() {
                d.current = world;
            }
            return;
        }
        let p = if matches!(
            self.tools.peek().tool,
            EditorTool::Brush | EditorTool::Scatter | EditorTool::Path
        ) && self.ticket.peek().is_some()
        {
            self.constrain(p)
        } else {
            p
        };
        let mut view = *self.view.peek();
        let result = self
            .interaction
            .write()
            .update(event.pointer_id(), p, true, &mut view);
        if view != *self.view.peek() {
            self.view.set(view);
        }
        self.result(result);
    }
    fn finish(mut self, event: PointerEvent) {
        if event.trigger_button() != Some(*self.button.peek()) {
            return;
        }
        if let Some(d) = self.drag.peek().as_ref() {
            if d.pointer != event.pointer_id() {
                return;
            }
        }
        let local = self.local(&event);
        let local = if matches!(
            self.tools.peek().tool,
            EditorTool::Brush | EditorTool::Scatter | EditorTool::Path
        ) && self.ticket.peek().is_some()
        {
            self.constrain(local)
        } else {
            local
        };
        let drag = { self.drag.peek().clone() };
        if let Some(mut d) = drag {
            d.current = self.view.peek().screen_to_world(local);
            let commands = {
                let session = self.session.peek();
                session.check_stamp(&d.stamp).and_then(|()| match d.kind {
                    DragKind::Marquee => Ok(vec![]),
                    DragKind::Node(index, handle) => {
                        let o = d.original.first().ok_or(MapError::Stale)?;
                        move_node(o, index, handle, d.current)
                            .map(|object| vec![Command::PutObject { object, index: 0 }])
                    }
                    _ => {
                        let (delta, scale, rotation) = drag_transform(&d);
                        transform_selection(
                            session.snapshot(),
                            &self.tools.peek().selection,
                            d.pivot,
                            delta,
                            scale,
                            rotation,
                        )
                    }
                })
            };
            if d.kind == DragKind::Marquee {
                let rect = drag_rect(&d);
                let ids = marquee(self.session.peek().snapshot(), &self.registry.peek(), rect);
                if d.shift {
                    self.tools.write().selection.extend(ids);
                } else {
                    self.tools.write().selection = ids;
                }
                self.active.set(false);
                self.drag.set(None);
                self.focus();
            } else {
                self.commit(commands);
            }
            return;
        }
        let mut view = *self.view.peek();
        let result = self
            .interaction
            .write()
            .finish(event.pointer_id(), local, &mut view);
        if view != *self.view.peek() {
            self.view.set(view);
        }
        self.result(result);
    }
    fn down(mut self, event: PointerEvent) {
        if !event.is_primary()
            || !matches!(
                event.trigger_button(),
                Some(MouseButton::Primary | MouseButton::Auxiliary)
            )
            || *self.active.peek()
        {
            return;
        }
        event.prevent_default();
        let p = event.element_coordinates();
        let client = event.client_coordinates();
        let local = Point::new(p.x, p.y);
        self.origin.set(Point::new(client.x - p.x, client.y - p.y));
        self.button
            .set(event.trigger_button().unwrap_or(MouseButton::Primary));
        let world = self.view.peek().screen_to_world(local);
        if !world.is_finite() {
            return;
        }
        let tool = if event.trigger_button() == Some(MouseButton::Auxiliary) || *self.space.peek() {
            EditorTool::Pan
        } else {
            self.tools.peek().tool
        };
        if tool != EditorTool::Pan && tool != EditorTool::Select && self.outside_frame(local) {
            return;
        }
        let local = if tool != EditorTool::Pan && tool != EditorTool::Select {
            self.constrain(local)
        } else {
            local
        };
        let world = self.view.peek().screen_to_world(local);
        if !matches!(tool, EditorTool::Select | EditorTool::Pan) {
            let allowed = {
                self.tools
                    .peek()
                    .drawing_layer(self.session.peek().snapshot())
            };
            if let Err(error) = allowed {
                self.report_error(error);
                return;
            }
        }
        if tool == EditorTool::Path {
            self.append_path(world);
            self.focus();
            return;
        }
        if matches!(
            tool,
            EditorTool::Pan | EditorTool::Brush | EditorTool::Scatter
        ) {
            if tool != EditorTool::Pan && !self.begin_content() {
                return;
            }
            let accepted = self.interaction.write().begin(
                event.pointer_id(),
                if tool == EditorTool::Pan {
                    Tool::Pan
                } else {
                    Tool::Brush
                },
                local,
                *self.view.peek(),
            );
            self.active.set(accepted);
            return;
        }
        if self.session.peek().mode() != SessionMode::Editable
            || matches!(self.session.peek().save_state(), SaveState::Conflict(_))
        {
            self.report_error(MapError::ReadOnly);
            return;
        }
        if tool != EditorTool::Text {
            self.focus();
        }
        match tool {
            EditorTool::MapNode => {
                let result = {
                    let s = self.session.peek();
                    crate::map::create_node(
                        s.snapshot(),
                        s.stamp().revision,
                        &self.tools.peek(),
                        world,
                        self.tools.peek().node_name.clone(),
                    )
                };
                match result {
                    Ok(c) => self.execute(c),
                    Err(e) => self.report_error(e),
                }
            }
            EditorTool::Anchor => {
                let result = {
                    let s = self.session.peek();
                    self.tools
                        .peek()
                        .anchor
                        .as_ref()
                        .ok_or_else(|| MapError::Missing("select a host candidate".into()))
                        .and_then(|id| {
                            crate::map::place_anchor(
                                s.snapshot(),
                                s.stamp().revision,
                                &self.tools.peek(),
                                id,
                                world,
                            )
                        })
                };
                match result {
                    Ok(c) => self.execute(c),
                    Err(e) => self.report_error(e),
                }
            }
            EditorTool::Text => {
                let session = self.session.peek();
                let hit = scene::hit_test(
                    session.snapshot(),
                    &self.registry.peek(),
                    world,
                    self.view.peek().scale(),
                );
                let found = hit
                    .and_then(|id| {
                        session
                            .snapshot()
                            .document
                            .objects
                            .iter()
                            .find(|o| o.id == id)
                    })
                    .and_then(|o| {
                        if let Geometry::Text { text, transform } = &o.geometry {
                            Some((o.id.clone(), text.clone(), transform.translation))
                        } else {
                            None
                        }
                    });
                let (object, text, position) = found
                    .map(|(id, text, p)| (Some(id), text, p))
                    .unwrap_or((None, String::new(), world));
                self.text.set(Some(TextDraft {
                    id: INSTANCE.fetch_add(1, Ordering::Relaxed),
                    text,
                    position,
                    object,
                    stamp: session.stamp(),
                    composing: false,
                }));
            }
            EditorTool::Stamp => {
                let command = {
                    let s = self.session.peek();
                    create_object(
                        s.snapshot(),
                        s.stamp().revision,
                        &self.tools.peek(),
                        Geometry::Stamp {
                            stamp: self.tools.peek().stamp.clone(),
                            transform: Transform {
                                translation: world,
                                ..Transform::default()
                            },
                        },
                    )
                };
                match command {
                    Ok(command) => self.execute(vec![command]),
                    Err(e) => self.report_error(e),
                };
            }
            EditorTool::Select => {
                let (kind, hit, original, pivot, stamp) = {
                    let s = self.session.peek();
                    let snapshot = s.snapshot();
                    let registry = self.registry.peek();
                    let tools = self.tools.peek();
                    let bounds = selection_bounds(snapshot, &registry, &tools.selection);
                    let hit = scene::hit_test(snapshot, &registry, world, self.view.peek().scale());
                    let selected: Vec<_> = snapshot
                        .document
                        .objects
                        .iter()
                        .filter(|o| tools.selection.contains(&o.id))
                        .cloned()
                        .collect();
                    let mut kind = None;
                    if let Some(b) = bounds.filter(|_| {
                        selected
                            .iter()
                            .all(|o| scene::transform(&o.geometry).is_some())
                    }) {
                        let screen = self.view.peek().world_to_screen(world);
                        let scale_handle = self
                            .view
                            .peek()
                            .world_to_screen(Point::new(b.min.x + b.width, b.min.y + b.height));
                        let rotate_handle = self.view.peek().world_to_screen(Point::new(
                            b.min.x + b.width / 2.,
                            b.min.y + b.height + 26. / self.view.peek().scale(),
                        ));
                        if screen.distance(scale_handle) < 9. {
                            kind = Some(DragKind::Scale);
                        } else if screen.distance(rotate_handle) < 9. {
                            kind = Some(DragKind::Rotate);
                        }
                    }
                    if kind.is_none() && selected.len() == 1 {
                        if let Geometry::Path {
                            nodes,
                            transform,
                            smooth,
                            closed,
                        } = &selected[0].geometry
                        {
                            let nodes = scene::path_nodes(nodes, *smooth, *closed);
                            for (i, n) in nodes.iter().enumerate() {
                                for (p, h) in [
                                    (Some(n.position), None),
                                    (n.incoming, Some(false)),
                                    (n.outgoing, Some(true)),
                                ] {
                                    if let Some(p) = p {
                                        if transform.apply(p).is_ok_and(|p| {
                                            p.distance(world) * self.view.peek().scale() < 7.
                                        }) {
                                            kind = Some(DragKind::Node(i, h));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let pivot = bounds
                        .map(|b| Point::new(b.min.x + b.width / 2., b.min.y + b.height / 2.))
                        .unwrap_or(world);
                    (kind, hit, selected, pivot, s.stamp())
                };
                let shift = event.modifiers().shift();
                let mut kind = kind;
                if kind.is_none() {
                    if let Some(id) = hit {
                        let mut tools = self.tools.write();
                        if shift {
                            if !tools.selection.insert(id.clone()) {
                                tools.selection.remove(&id);
                                self.node.set(None);
                                return;
                            }
                        } else if !tools.selection.contains(&id) {
                            tools.selection.clear();
                            tools.selection.insert(id);
                        }
                        kind = Some(DragKind::Move);
                    } else {
                        if !shift {
                            self.tools.write().selection.clear();
                        }
                        kind = Some(DragKind::Marquee);
                    }
                }
                let kind = kind.unwrap_or(DragKind::Marquee);
                if let DragKind::Node(i, _) = kind {
                    self.node.set(Some(i));
                } else {
                    self.node.set(None);
                }
                let original = if kind == DragKind::Move {
                    let s = self.session.peek();
                    s.snapshot()
                        .document
                        .objects
                        .iter()
                        .filter(|o| self.tools.peek().selection.contains(&o.id))
                        .cloned()
                        .collect()
                } else {
                    original
                };
                if kind != DragKind::Marquee && !self.begin_content() {
                    return;
                }
                self.drag.set(Some(Drag {
                    pointer: event.pointer_id(),
                    kind,
                    start: world,
                    current: world,
                    pivot,
                    stamp,
                    original,
                    shift,
                }));
                self.active.set(true);
            }
            _ => {}
        }
    }
    fn append_path(mut self, world: Point) {
        let tools = self.tools.peek().clone();
        let session = self.session.peek();
        let mut path = self.path.peek().clone();
        if path
            .stamp
            .as_ref()
            .is_some_and(|stamp| stamp != &session.stamp())
        {
            path.cancel();
        }
        let result = path
            .append(
                world,
                self.view.peek().scale(),
                session.stamp(),
                session.limits().max_object_nodes,
            )
            .and_then(|()| {
                path.command_with_limits(
                    session.snapshot(),
                    session.stamp().revision,
                    &tools,
                    &session.limits(),
                )
            });
        drop(session);
        match result {
            Ok(Some(command)) => {
                let id = if let Command::PutObject { object, .. } = &command {
                    Some(object.id.clone())
                } else {
                    None
                };
                let result = self
                    .session
                    .write()
                    .execute_with_host(vec![command], &mut HostPolicy(self.host));
                if result.is_ok() {
                    path.object = id;
                    path.stamp = Some(self.session.peek().stamp());
                    self.path.set(path);
                }
                self.error(result);
            }
            Ok(None) => self.path.set(path),
            Err(e) => self.report_error(e),
        }
    }
    fn finish_path(mut self) {
        // All valid nodes are already committed. Ending only starts a new path.
        self.path.write().cancel();
    }
    fn history(mut self, redo: bool) {
        self.cancel();
        let result = if redo {
            self.session.write().redo(&mut HostPolicy(self.host))
        } else {
            self.session.write().undo(&mut HostPolicy(self.host))
        };
        self.error(result);
        let snapshot = self.session.peek().snapshot().clone();
        self.tools.write().reconcile(&snapshot);
        self.focus();
    }
    fn key_down(mut self, e: KeyboardEvent) {
        if e.is_composing() || self.text.peek().is_some() {
            return;
        }
        let modifiers = e.modifiers();
        if self.options.keyboard_shortcuts {
            if e.code() == Code::Space {
                e.prevent_default();
                e.stop_propagation();
                self.space.set(true);
                return;
            }
            if (modifiers.ctrl() || modifiers.meta()) && !modifiers.alt() && e.code() == Code::KeyZ
            {
                e.prevent_default();
                e.stop_propagation();
                self.history(modifiers.shift());
                return;
            }
        }
        match e.key() {
            Key::Escape => {
                e.prevent_default();
                self.cancel();
            }
            Key::Enter if self.tools.peek().tool == EditorTool::Path => {
                e.prevent_default();
                self.finish_path();
            }
            Key::Delete | Key::Backspace => {
                e.prevent_default();
                self.execute(delete_selection(&self.tools.peek().selection));
            }
            _ => {}
        }
    }
    fn key_up(mut self, e: KeyboardEvent) {
        if e.code() == Code::Space {
            e.prevent_default();
            self.space.set(false);
        }
    }
    fn commit_text(mut self) {
        let draft = self.text.peek().clone();
        let Some(d) = draft else { return };
        if d.composing {
            return;
        }
        if d.text.trim().is_empty() {
            self.text.set(None);
            return;
        }
        let command = {
            let s = self.session.peek();
            s.check_stamp(&d.stamp).and_then(|()| {
                if let Some(id) = d.object {
                    let mut o = s
                        .snapshot()
                        .document
                        .objects
                        .iter()
                        .find(|o| o.id == id)
                        .cloned()
                        .ok_or(MapError::Stale)?;
                    if let Geometry::Text { text, .. } = &mut o.geometry {
                        *text = d.text;
                    }
                    Ok(Command::PutObject {
                        object: o,
                        index: 0,
                    })
                } else {
                    create_object(
                        s.snapshot(),
                        s.stamp().revision,
                        &self.tools.peek(),
                        Geometry::Text {
                            text: d.text,
                            transform: Transform {
                                translation: d.position,
                                ..Transform::default()
                            },
                        },
                    )
                }
            })
        };
        match command {
            Ok(c) => {
                self.execute(vec![c]);
                self.text.set(None);
            }
            Err(e) => self.report_error(e),
        }
    }
}
fn drag_rect(d: &Drag) -> WorldRect {
    WorldRect {
        min: Point::new(d.start.x.min(d.current.x), d.start.y.min(d.current.y)),
        width: (d.start.x - d.current.x).abs().max(0.001),
        height: (d.start.y - d.current.y).abs().max(0.001),
    }
}
fn drag_transform(d: &Drag) -> (Point, f64, f64) {
    match d.kind {
        DragKind::Move => (
            Point::new(d.current.x - d.start.x, d.current.y - d.start.y),
            1.,
            0.,
        ),
        DragKind::Scale => (
            Point::default(),
            (d.current.distance(d.pivot) / d.start.distance(d.pivot).max(0.01)).clamp(0.02, 100.),
            0.,
        ),
        DragKind::Rotate => (
            Point::default(),
            1.,
            (d.current.y - d.pivot.y).atan2(d.current.x - d.pivot.x)
                - (d.start.y - d.pivot.y).atan2(d.start.x - d.pivot.x),
        ),
        _ => (Point::default(), 1., 0.),
    }
}
fn drag_svg(d: &Drag) -> String {
    let (delta, scale, rotation) = drag_transform(d);
    format!(
        "translate({} {}) translate({} {}) rotate({}) scale({}) translate({} {})",
        delta.x,
        delta.y,
        d.pivot.x,
        d.pivot.y,
        rotation.to_degrees(),
        scale,
        -d.pivot.x,
        -d.pivot.y
    )
}

/// Controlled canvas: host owns one session, resource registry, tools and view.
/// Hosts increment invalidation on blur, ancestor scrolling and layout changes.
/// `host` must atomically accept all anchor patches or return an error without
/// mutating host state. Without it, anchor coordinate edits are rejected.
/// `onactivate` requests host navigation; document names and positions stay external.
#[component]
pub fn MapCanvas(
    session: Signal<MapSession>,
    tools: Signal<EditorTools>,
    registry: Signal<Registry>,
    view: Signal<Viewport>,
    #[props(default="地图画布".into())] label: String,
    #[props(default)] invalidation: u64,
    #[props(default)] host: Option<Callback<Vec<crate::command::AnchorPatch>, MapResult<()>>>,
    #[props(default)] onactivate: EventHandler<AnchorId>,
    #[props(default)] onready: EventHandler<MapCanvasHandle>,
    /// Typed failures for host-owned translated notices, independent of built-in status.
    #[props(default)]
    onerror: EventHandler<MapError>,
    #[props(default)] text_labels: TextEditorLabels,
    #[props(default)] text_overlay: Option<Callback<MapCanvasHandle, Element>>,
    #[props(default)] interaction_options: CanvasInteractionOptions,
    #[props(default = true)] show_controls: bool,
    #[props(default = true)] show_status: bool,
    /// Passive host-owned overlay; root must use pointer-events:none. Interactive
    /// controls belong outside the SVG and submit commands through the handle.
    #[props(default)]
    overlay: Option<Callback<MapCanvasHandle, Element>>,
    /// Refit the frame after container resize; disabled for hosts preserving a view.
    #[props(default)]
    fit_on_resize: bool,
) -> Element {
    let mut c = Canvas {
        session,
        tools,
        registry,
        view,
        interaction: use_signal(Interaction::default),
        drag: use_signal(|| None),
        text: use_signal(|| None),
        path: use_signal(PathDraft::default),
        origin: use_signal(Point::default),
        button: use_signal(|| MouseButton::Primary),
        active: use_signal(|| false),
        ticket: use_signal(|| None),
        message: use_signal(|| "选择、绘制或放置对象；Esc 取消".into()),
        node: use_signal(|| None),
        mounted: use_signal(|| None),
        hover: use_signal(|| None),
        space: use_signal(|| false),
        host,
        activate: onactivate,
        options: interaction_options,
        onerror,
    };
    let prefix = use_hook(|| format!("dme-editor-{}", INSTANCE.fetch_add(1, Ordering::Relaxed)));
    use_effect(move || onready.call(MapCanvasHandle { canvas: c }));
    use_effect(move || {
        let registry = c.registry.read().clone();
        let result = c.session.write().set_registry(registry);
        if let Err(error) = result {
            c.report_error(error);
        }
    });
    let mut mounted = use_signal(|| None::<Rc<MountedData>>);
    let mut size = use_signal(|| (800., 440.));
    use_effect(use_reactive!(|(invalidation)| {
        let _ = invalidation;
        c.space.set(false);
        c.cancel_pointer();
    }));
    use_effect(move || {
        let session = c.session.read();
        let mut tools = c.tools.peek().clone();
        tools.reconcile(session.snapshot());
        if tools != *c.tools.peek() {
            let mut signal = c.tools;
            signal.set(tools);
        }
        let mut path = c.path;
        if path
            .peek()
            .stamp
            .as_ref()
            .is_some_and(|s| s != &session.stamp())
        {
            path.write().cancel();
        }
    });
    let mut previous_tool = use_signal(|| {
        let t = tools.peek();
        (
            t.tool,
            t.paint.role.clone(),
            t.closed,
            t.smooth,
            t.brush.clone(),
        )
    });
    use_effect(move || {
        let t = tools.read();
        let tool = (
            t.tool,
            t.paint.role.clone(),
            t.closed,
            t.smooth,
            t.brush.clone(),
        );
        if tool != *previous_tool.peek() {
            c.cancel();
            previous_tool.set(tool);
        }
    });
    use_effect(move || {
        let session = c.session.read();
        let stale = c
            .ticket
            .peek()
            .as_ref()
            .is_some_and(|t| !session.is_gesture_current(t));
        if stale {
            drop(session);
            c.cancel();
        }
    });
    let active = (c.active)();
    let text = c.text.read().clone();
    let tool = tools.read().tool;
    rsx! {
           style{{EDITOR_CSS}}
           section{class:"dme-editor-canvas", "aria-label":label.clone(),
               if show_controls {div{class:"dme-viewbar",
                   span{"{label}"}
                   button{onclick:move |_|{c.cancel();let mut view=c.view;view.set(Viewport::default());},"回原点"}
                   button{onclick:move |_|{c.cancel();let s=c.session.peek();let bounds=scene::content_bounds(s.snapshot(),&c.registry.peek()).unwrap_or(s.snapshot().document.frame);let (w,h)=*size.peek();match fit_view(bounds,w,h){Ok(v)=>{let mut view=c.view;view.set(v);},Err(e)=>{let mut message=c.message;message.set(e.to_string());}}},"适应内容"}

               }}
               div{class:"dme-editor-surface",onblur:move|_|{if !*c.active.peek(){c.space.set(false);}},tabindex:"0",onkeydown:move|e|c.key_down(e),onkeyup:move|e|c.key_up(e),onmounted:move|e|{let mut target=c.mounted;target.set(Some(e.data()));mounted.set(Some(e.data()));spawn(async move{if let Some(element)=mounted.peek().clone(){if let Ok(rect)=element.get_client_rect().await{size.set((rect.size.width,rect.size.height));}}});},onresize:move|_|{c.cancel_pointer();if fit_on_resize{MapCanvasHandle{canvas:c}.fit_frame();}spawn(async move{if let Some(element)=mounted.peek().clone(){if let Ok(rect)=element.get_client_rect().await{size.set((rect.size.width,rect.size.height));}}});},
                   svg{class:"dme-editor-svg",tabindex:"0",role:"img","aria-label":label.clone(),"data-tool":format!("{tool:?}"),style:if *c.space.read(){"cursor:grab"}else{""},
                       onpointerdown:move|e|c.down(e),onpointermove:move|e|c.move_pointer(e),onpointerup:move|e|c.finish(e),onpointercancel:move|_|c.cancel(),onpointerleave:move|_|{let mut hover=c.hover;hover.set(None);},
                       ondoubleclick:move|e|{if c.tools.peek().tool==EditorTool::Path {e.prevent_default();c.finish_path();}else{let s=c.session.peek();if let Some(id)=c.tools.peek().selection.iter().next(){if let Some(o)=s.snapshot().document.objects.iter().find(|o|&o.id==id){if let Geometry::BoundMarker{anchor_id,..}=&o.geometry{c.activate.call(anchor_id.clone());}}}}},
                       onblur:move|_|{if !*c.active.peek(){c.space.set(false);}},
                       onwheel:move|e|{if c.options.ctrl_wheel_zoom && !e.modifiers().ctrl(){return;}e.prevent_default();if *c.active.peek()||c.text.peek().is_some(){return;}let p=e.element_coordinates();let delta=e.delta().strip_units().y;let delta=match e.delta(){dioxus::html::geometry::WheelDelta::Lines(_)=>delta*16.,dioxus::html::geometry::WheelDelta::Pages(_)=>delta*400.,_=>delta};let mut view=c.view;let _=view.write().zoom_at(Point::new(p.x,p.y),(-delta.clamp(-200.,200.)*0.002).exp());},
                       CanvasScene{c,prefix:prefix.clone()}
                       CanvasPreview{c,prefix:prefix.clone()}
                   }
                   if let Some(render)=overlay { {render.call(MapCanvasHandle{canvas:c})} }
                   if let Some(overlay)=text_overlay { {overlay.call(MapCanvasHandle{canvas:c})} } else if let Some(draft)=text { {let p=view.peek().world_to_screen(draft.position);rsx!{
                       div{class:"dme-text-editor",style:"left:{p.x}px;top:{p.y-28.}px",
                           input{onkeyup:move|e|e.stop_propagation(),key:"{draft.id}",initial_value:draft.text,"aria-label":text_labels.input.clone(),onmounted:move|e|async move{let _=e.set_focus(true).await;},oninput:move|e|{let mut text=c.text;if let Some(d)=text.write().as_mut(){d.text=e.value();};},oncompositionstart:move|_|{let mut text=c.text;if let Some(d)=text.write().as_mut(){d.composing=true;};},oncompositionend:move|_|{let mut text=c.text;if let Some(d)=text.write().as_mut(){d.composing=false;};},onkeydown:move|e|{e.stop_propagation();if e.is_composing()||c.text.peek().as_ref().is_some_and(|d|d.composing){return;}
    if e.key()==Key::Enter{e.prevent_default();c.commit_text();}else if e.key()==Key::Escape{e.prevent_default();c.cancel();}}}
                           button{onclick:move|_|c.commit_text(),"{text_labels.commit}"}button{onclick:move|_|c.cancel(),"{text_labels.cancel}"}
                       }
                   }}}
               }
               if show_status {CanvasStatus{c}}
               if active {div{class:"dme-capture",style:if *c.space.read(){"cursor:grabbing"}else{""},tabindex:"0","aria-label":format!("{label}活动手势"),onmounted:move|e|async move{let _=e.set_focus(true).await;},onpointermove:move|e|c.move_pointer(e),onpointerup:move|e|c.finish(e),onpointercancel:move|_|c.cancel(),onpointerleave:move|_|{if !c.options.resume_on_reentry{c.cancel();}},onblur:move|_|{if *c.active.peek(){c.space.set(false);c.cancel();}},onkeydown:move|e|c.key_down(e),onkeyup:move|e|c.key_up(e),onwheel:move|e|e.prevent_default(),oncontextmenu:move|e|e.prevent_default()}}
           }
       }
}
#[component]
fn CanvasScene(c: Canvas, prefix: String) -> Element {
    let scene_prefix = prefix.clone();
    let cache = use_memo(move || {
        let s = c.session.read();
        let r = c.registry.read();
        Rc::new(
            scene::build_scene(s.snapshot(), &r, &scene_prefix)
                .into_iter()
                .map(Rc::new)
                .collect::<Vec<_>>(),
        )
    });
    let defs = scene::definitions(&c.registry.read(), &prefix);
    let f = c.session.read().snapshot().document.frame;
    rsx! {defs{dangerous_inner_html:defs} defs{clipPath{id:format!("{prefix}-frame"),clip_path_units:"userSpaceOnUse",rect{x:f.min.x,y:f.min.y,width:f.width,height:f.height}}} SceneContent{c,objects:cache(),prefix:prefix.clone()}}
}
#[component]
fn SceneContent(c: Canvas, objects: Rc<Vec<Rc<scene::SceneObject>>>, prefix: String) -> Element {
    let view = (c.view)();
    let origin = view.origin();
    let scale = view.scale();
    let tools = c.tools.read();
    let selected = &tools.selection;
    let d = c.drag.read();
    let transform = d.as_ref().map(drag_svg).unwrap_or_default();
    let moving = d
        .as_ref()
        .is_some_and(|d| matches!(d.kind, DragKind::Move | DragKind::Scale | DragKind::Rotate));
    let node = d
        .as_ref()
        .is_some_and(|d| matches!(d.kind, DragKind::Node(..)));
    let snapshot = c.session.read();
    let frame = snapshot.snapshot().document.frame;
    let bg = c
        .registry
        .read()
        .style(&snapshot.snapshot().document.style)
        .background
        .clone();
    let clip = c
        .options
        .constrain_to_frame
        .then(|| format!("url(#{prefix}-frame)"));
    rsx! {g{transform:"translate({origin.x} {origin.y}) scale({scale} -{scale})",clip_path:clip,
        rect{x:frame.min.x,y:frame.min.y,width:frame.width,height:frame.height,fill:bg,stroke:"#b4bcb1",stroke_width:1./scale}
        for object in objects.iter() {
            g{key:"{object.id}",transform:if moving&&selected.contains(&object.id){transform.clone()}else{String::new()},
                if !(node&&selected.contains(&object.id)){ObjectFragment{object:object.clone()}}
            }
        }
        g{dangerous_inner_html:scene::paper_svg(snapshot.snapshot(),&c.registry.read(),&prefix)}
    }}
}
#[component]
fn ObjectFragment(object: Rc<scene::SceneObject>) -> Element {
    rsx! {g{"data-object":object.id.to_string(),dangerous_inner_html:object.svg.clone()}}
}
#[component]
fn CanvasPreview(c: Canvas, prefix: String) -> Element {
    let view = (c.view)();
    let origin = view.origin();
    let scale = view.scale();
    let interaction = c.interaction.read();
    let brush = scene::path_data(
        &scene::smooth_nodes(interaction.preview_points(), false),
        false,
    );
    let draft = c.path.read();
    let tools = c.tools.read();
    let r = c.registry.read();
    let s = c.session.read();
    let paint = r.paint(&s.snapshot().document.style, &tools.paint);
    let scattering = tools.tool == EditorTool::Scatter;
    let custom_brush = matches!(tools.tool, EditorTool::Brush | EditorTool::Path);
    let custom_preview: Vec<(ObjectId, String)> = if custom_brush {
        (if let Some(key) = &tools.brush {
            r.brush(key).and_then(|brush| {
                brush
                    .commands(crate::plugin::BrushInput {
                        snapshot: s.snapshot(),
                        revision: s.stamp().revision,
                        tools: &tools,
                        points: interaction.preview_points(),
                        limits: &s.limits(),
                    })
                    .ok()
            })
        } else if !interaction.preview_points().is_empty() {
            let mut samples = interaction.preview_points().to_vec();
            if matches!(tools.paint.role.as_str(), "road" | "trail") {
                for index in [0, samples.len() - 1] {
                    samples[index] = crate::map::snap_endpoint(s.snapshot(), samples[index], scale);
                }
            }
            brush_commands(
                s.snapshot(),
                s.stamp().revision,
                &tools,
                &samples,
                paint.width,
                &s.limits(),
            )
            .ok()
        } else {
            None
        })
        .map(|commands| {
            commands
                .into_iter()
                .filter_map(|c| {
                    if let Command::PutObject { object, .. } = c {
                        Some((
                            object.id.clone(),
                            scene::object_svg(&object, s.snapshot(), &r, &prefix),
                        ))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
    } else {
        Vec::new()
    };
    let scatter_preview: Vec<(ObjectId, String)> =
        if scattering && !interaction.preview_points().is_empty() {
            crate::map::scatter(
                s.snapshot(),
                s.stamp().revision,
                &tools,
                interaction.preview_points(),
                &tools.scatter,
                &s.limits(),
            )
            .map(|commands| {
                commands
                    .into_iter()
                    .filter_map(|command| {
                        if let Command::PutObject { object, .. } = command {
                            Some((
                                object.id.clone(),
                                scene::object_svg(&object, s.snapshot(), &r, &prefix),
                            ))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
        } else {
            Vec::new()
        };
    let ghost = if matches!(tools.tool, EditorTool::Stamp | EditorTool::MapNode) {
        (*c.hover.read()).map(|p| {
            let o = MapObject {
                id: "preview".into(),
                layer_id: tools.layer.clone(),
                geometry: Geometry::Stamp {
                    stamp: tools.stamp.clone(),
                    transform: Transform {
                        translation: p,
                        ..Transform::default()
                    },
                },
                paint: tools.paint.clone(),
                extensions: Extensions::new(),
            };
            scene::object_svg(&o, s.snapshot(), &r, &prefix)
        })
    } else {
        None
    };
    let draft_path = scene::path_data(
        &scene::path_nodes(&draft.nodes, tools.smooth, tools.closed),
        tools.closed,
    );
    let edge_snap = if tools.closed && matches!(tools.tool, EditorTool::Brush | EditorTool::Path) {
        (*c.hover.read()).and_then(|p| {
            crate::region::snap_point(s.snapshot(), p, &tools.region, draft.object.as_ref())
        })
    } else {
        None
    };
    let drag = c.drag.read();
    let bounds = selection_bounds(s.snapshot(), &r, &tools.selection);
    let moving = drag
        .as_ref()
        .is_some_and(|d| matches!(d.kind, DragKind::Move | DragKind::Scale | DragKind::Rotate));
    let preview_transform = if moving {
        drag.as_ref().map(drag_svg).unwrap_or_default()
    } else {
        String::new()
    };
    let node_preview = drag.as_ref().and_then(|d| {
        if let DragKind::Node(i, h) = d.kind {
            d.original
                .first()
                .and_then(|o| move_node(o, i, h, d.current).ok())
        } else {
            None
        }
    });
    let selected_object = if tools.selection.len() == 1 {
        s.snapshot()
            .document
            .objects
            .iter()
            .find(|o| tools.selection.contains(&o.id))
    } else {
        None
    };
    let handles = selected_object.and_then(|o| {
        if let Geometry::Path {
            nodes,
            transform,
            smooth,
            closed,
        } = &o.geometry
        {
            Some((scene::path_nodes(nodes, *smooth, *closed), *transform))
        } else {
            None
        }
    });
    let clip = c
        .options
        .constrain_to_frame
        .then(|| format!("url(#{prefix}-frame)"));
    rsx! {g{class:"dme-edit-preview",transform:"translate({origin.x} {origin.y}) scale({scale} -{scale})",clip_path:clip,
        if custom_brush {g{class:"dme-plugin-brush-preview",for (id,svg) in custom_preview {g{key:"{id}",dangerous_inner_html:svg}}}}else if scattering {g{class:"dme-scatter-preview",opacity:0.85,for (id,svg) in scatter_preview {g{key:"{id}",dangerous_inner_html:svg}}}}else{path{d:brush,fill:"none",stroke:paint.color.clone(),stroke_width:paint.width,opacity:paint.opacity,stroke_linecap:"round",stroke_linejoin:"round"}}
        if let Some(p)=*c.hover.read(){if matches!(tools.tool,EditorTool::Scatter|EditorTool::Brush){circle{class:"dme-brush-cursor",cx:p.x,cy:p.y,r:if scattering{tools.scatter.spread+12.*tools.scatter.size[1]}else{paint.width/2.},fill:"#7f926910",stroke:"#73815d",stroke_width:1./scale,stroke_dasharray:format!("{} {}",4./scale,3./scale)}}}
        if let Some(svg)=ghost{g{class:"dme-stamp-ghost",opacity:0.55,dangerous_inner_html:svg}}
        if draft.object.is_none(){path{d:draft_path,fill:if tools.closed{paint.fill.clone().unwrap_or_else(||"none".into())}else{"none".into()},stroke:paint.color,stroke_width:paint.width,opacity:paint.opacity}}
        for n in &draft.nodes {circle{cx:n.position.x,cy:n.position.y,r:4./scale,fill:"#287a70"}}
        if let Some((_,p))=edge_snap {circle{class:"dme-edge-snap",cx:p.x,cy:p.y,r:6./scale,fill:"#faf8ee",stroke:"#b98540",stroke_width:2./scale}}
        if let Some(o)=node_preview {g{dangerous_inner_html:scene::object_svg(&o,s.snapshot(),&r,"preview")}}
        g{transform:preview_transform,
            if let Some(b)=bounds {rect{class:"dme-selection",x:b.min.x,y:b.min.y,width:b.width,height:b.height,fill:"none",stroke:"#197d79",stroke_width:1./scale,stroke_dasharray:format!("{} {}",4./scale,3./scale)}
                if selected_object.is_some_and(|o|!matches!(o.geometry,Geometry::BoundMarker{..})) {
                rect{x:b.min.x+b.width-4./scale,y:b.min.y+b.height-4./scale,width:8./scale,height:8./scale,fill:"white",stroke:"#197d79",stroke_width:1./scale}
                line{x1:b.min.x+b.width/2.,y1:b.min.y+b.height,x2:b.min.x+b.width/2.,y2:b.min.y+b.height+26./scale,stroke:"#197d79",stroke_width:1./scale}
                circle{cx:b.min.x+b.width/2.,cy:b.min.y+b.height+26./scale,r:5./scale,fill:"white",stroke:"#197d79",stroke_width:1./scale}
                }
            }
            if let Some((nodes,t))=handles {for (i,n) in nodes.iter().enumerate(){ {let p=t.apply(n.position).unwrap_or_default();rsx!{
                for h in [n.incoming,n.outgoing].into_iter().flatten(){ {let h=t.apply(h).unwrap_or_default();rsx!{line{x1:p.x,y1:p.y,x2:h.x,y2:h.y,stroke:"#d88b44",stroke_width:1./scale}circle{cx:h.x,cy:h.y,r:3.5/scale,fill:"#ffe1a9",stroke:"#b77629",stroke_width:1./scale}}} }
                circle{cx:p.x,cy:p.y,r:4.5/scale,fill:if *c.node.read()==Some(i){"#197d79"}else{"white"},stroke:"#197d79",stroke_width:1./scale}
            }}}}
        }
        if let Some(d)=drag.as_ref().filter(|d|d.kind==DragKind::Marquee){ {let b=drag_rect(d);rsx!{rect{x:b.min.x,y:b.min.y,width:b.width,height:b.height,fill:"#197d7920",stroke:"#197d79",stroke_width:1./scale}}} }
    }}
}
#[component]
fn CanvasStatus(c: Canvas) -> Element {
    let s = c.session.read();
    let message = (c.message)();
    let count = s.snapshot().document.objects.len();
    let revision = s.stamp().revision;
    let state = format!("{:?}", s.save_state());
    let selected = c.tools.read().selection.len();
    let zoom = c.view.read().scale() * 100.;
    rsx! {div{class:"dme-editor-status","data-count":count,"data-revision":revision,"data-dirty":s.is_dirty(),"data-state":state.clone(),span{"{zoom:.0}% · 对象 {count} · 选择 {selected} · revision {revision}"}span{"{state}"}span{role:"status","{message}"}}}
}

/// Default editor chrome around a controlled MapCanvas. Host owns session and IO.
#[component]
pub fn MapEditor(
    session: Signal<MapSession>,
    #[props(default="地图编辑器".into())] label: String,
    #[props(default)] invalidation: u64,
    #[props(default)] host: Option<Callback<Vec<crate::command::AnchorPatch>, MapResult<()>>>,
    #[props(default)] onactivate: EventHandler<AnchorId>,
    #[props(default)] onready: EventHandler<MapCanvasHandle>,
    /// Typed failures for host-owned translated notices, independent of built-in status.
    #[props(default)]
    onerror: EventHandler<MapError>,
    #[props(default)] text_labels: TextEditorLabels,
    #[props(default)] text_overlay: Option<Callback<MapCanvasHandle, Element>>,
    /// Replace the entire property panel, including text controls, using the public handle.
    #[props(default)]
    properties: Option<Callback<MapCanvasHandle, Element>>,
    #[props(default)] interaction_options: CanvasInteractionOptions,
) -> Element {
    let tools = use_signal(EditorTools::default);
    let registry = use_signal(Registry::default);
    let view = use_signal(Viewport::default);
    let mut ready = use_signal(|| None::<MapCanvasHandle>);
    rsx! {style{{EDITOR_CSS}}section{class:"dme-editor","aria-label":label.clone(),
        EditorToolbar{session,tools,registry,host}
        MapTools{session,tools,host,onactivate}
        div{class:"dme-editor-body",MapCanvas{session,tools,registry,view,label:label.clone(),invalidation,host,onactivate,onready:move|canvas|{ready.set(Some(canvas));onready.call(canvas);},onerror,text_labels,text_overlay,interaction_options}
            if let Some(render)=properties {if let Some(canvas)=ready(){{render.call(canvas)}}}else{EditorProperties{session,tools,registry}}
        }
    }}
}
fn apply(
    mut session: Signal<MapSession>,
    mut message: Signal<String>,
    commands: MapResult<Vec<Command>>,
) {
    let result = commands.and_then(|c| session.write().execute(c));
    message.set(match result {
        Ok(r) => format!("{r:?}"),
        Err(e) => e.to_string(),
    });
}
fn patch_selection(
    s: &MapSnapshot,
    tools: &EditorTools,
    edit: impl Fn(&mut MapObject),
) -> Vec<Command> {
    s.document
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| tools.selection.contains(&o.id))
        .map(|(index, o)| {
            let mut object = o.clone();
            edit(&mut object);
            Command::PutObject { object, index }
        })
        .collect()
}
/// Reusable toolbar for hosts using MapCanvas directly.
#[component]
pub fn EditorToolbar(
    mut session: Signal<MapSession>,
    mut tools: Signal<EditorTools>,
    registry: Signal<Registry>,
    #[props(default)] host: Option<Callback<Vec<crate::command::AnchorPatch>, MapResult<()>>>,
) -> Element {
    let mut message = use_signal(String::new);
    let current = tools.read().tool;
    let history = session.read().history();
    let selected = !tools.read().selection.is_empty();
    rsx! {div{class:"dme-editor-toolbar","aria-label":"绘图工具栏",
        for (tool,label) in [(EditorTool::Select,"选择"),(EditorTool::Brush,"笔刷"),(EditorTool::Path,"路径"),(EditorTool::Text,"文字"),(EditorTool::Stamp,"图章"),(EditorTool::Pan,"抓手")] {
            button{r#type:"button","aria-pressed":current==tool,onclick:move|_|tools.write().tool=tool,"{label}"}
        }
        span{class:"dme-toolbar-separator"}
        button{disabled:history.undo==0,onclick:move|_|{let result=session.write().undo(&mut HostPolicy(host));message.set(format!("{result:?}"));},"撤销"}
        button{disabled:history.redo==0,onclick:move|_|{let result=session.write().redo(&mut HostPolicy(host));message.set(format!("{result:?}"));},"重做"}
        button{disabled:!selected,onclick:move|_|{let commands=delete_selection(&tools.peek().selection);apply(session,message,Ok(commands));},"删除选择"}
        label{"地图风格" select{"aria-label":"地图风格",value:session.read().snapshot().document.style.key.clone(),onchange:move|e|{let resource=registry.peek().styles().iter().find(|s|s.resource.key==e.value()).map(|s|s.resource.clone());if let Some(resource)=resource{apply(session,message,Ok(vec![Command::SetStyle(resource)]));}},for style in registry.read().styles(){option{value:style.resource.key.clone(),"{style.name}"}}}}
        span{class:"dme-toolbar-message",role:"status","{message}"}
    }}
}
/// Default properties and layer controls; each content edit is one command batch.
#[component]
pub fn EditorProperties(
    session: Signal<MapSession>,
    mut tools: Signal<EditorTools>,
    registry: Signal<Registry>,
) -> Element {
    let message = use_signal(String::new);
    let mut new_name = use_signal(|| "新图层".to_string());
    let mut pending_delete = use_signal(|| None::<LayerId>);
    let mut node_index = use_signal(|| 0_usize);
    let snapshot = session.read();
    let s = snapshot.snapshot();
    let selected = s
        .document
        .objects
        .iter()
        .find(|o| tools.read().selection.contains(&o.id))
        .cloned();
    let color = tools
        .read()
        .paint
        .color
        .clone()
        .unwrap_or_else(|| "#294c47".into());
    let width = tools.read().paint.width.unwrap_or(2.5);
    let opacity = tools.read().paint.opacity.unwrap_or(1.);
    let text_size = tools.read().text_size;
    let layers = s.document.layers.clone();
    let style_missing = !registry.read().has_style(&s.document.style);
    let stamps_missing=s.document.objects.iter().filter(|o|matches!(&o.geometry,Geometry::Stamp{stamp,..} if registry.read().stamp(stamp).is_none())).count();
    drop(snapshot);
    rsx! {aside{class:"dme-editor-properties","aria-label":"绘图属性",
        h3{"绘制属性"}
        label{"颜色" input{r#type:"color","aria-label":"笔刷颜色",value:color,oninput:move|e|{let value=e.value();tools.write().paint.color=Some(value.clone());let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|o.paint.color=Some(value.clone()));apply(session,message,Ok(commands));}}}
        div{class:"dme-palette",for color in ["#294c47","#594c3c","#347da2","#c87445","#88854f","#b3536b"] {button{title:color,"aria-label":format!("颜色 {color}"),style:"background:{color}",onclick:move|_|{tools.write().paint.color=Some(color.into());let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|o.paint.color=Some(color.into()));apply(session,message,Ok(commands));}}}}
        label{"线宽" input{r#type:"number",min:"0.1",max:"100",step:"0.5","aria-label":"笔刷宽度",value:width,onchange:move|e|{if let Ok(value)=e.value().parse::<f64>(){if value.is_finite()&&value>0.{tools.write().paint.width=Some(value);let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|if !matches!(o.geometry,Geometry::Text{..}){o.paint.width=Some(value)});apply(session,message,Ok(commands));}}}}}
        label{"透明度" input{r#type:"number",min:"0",max:"1",step:"0.1","aria-label":"笔刷透明度",value:opacity,onchange:move|e|{if let Ok(value)=e.value().parse::<f64>(){if value.is_finite()&&(0.0..=1.0).contains(&value){tools.write().paint.opacity=Some(value);let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|o.paint.opacity=Some(value));apply(session,message,Ok(commands));}}}}}
        label{"字号" input{r#type:"number",min:"1",max:"200","aria-label":"文字字号",value:text_size,onchange:move|e|{if let Ok(value)=e.value().parse::<f64>(){if value.is_finite()&&value>0.{tools.write().text_size=value;let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|if matches!(o.geometry,Geometry::Text{..}){o.paint.width=Some(value)});apply(session,message,Ok(commands));}}}}}
        label{input{r#type:"checkbox",checked:tools.read().smooth,onchange:move|e|{let value=e.checked();tools.write().smooth=value;let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|if let Geometry::Path{smooth,..}=&mut o.geometry{*smooth=value});apply(session,message,Ok(commands));}}"平滑路径"}
        label{input{r#type:"checkbox",checked:tools.read().closed,onchange:move|e|{let value=e.checked();tools.write().closed=value;let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|if let Geometry::Path{closed,..}=&mut o.geometry{*closed=value});apply(session,message,Ok(commands));}}"闭合区域"}
        label{input{r#type:"checkbox",checked:tools.read().paint.fill.is_some(),onchange:move|e|{let fill=e.checked().then(||tools.peek().paint.color.clone().unwrap_or_else(||"#d7e6dd".into()));tools.write().paint.fill=fill.clone();let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|o.paint.fill=fill.clone());apply(session,message,Ok(commands));}}"填充区域"}
        label{"图章" select{"aria-label":"图章类型",value:tools.read().stamp.key.clone(),onchange:move|e|{if let Some(stamp)=registry.peek().stamps().iter().find(|s|s.resource.key==e.value()){tools.write().stamp=stamp.resource.clone();}},for stamp in registry.read().stamps(){option{value:stamp.resource.key.clone(),"{stamp.name}"}}}}
        if style_missing||stamps_missing>0 {p{class:"dme-notice","部分资源缺失，已显示替代外观。原始键与数据保留，注册相同版本后恢复。"}}
        if let Some(object)=selected {h3{"选择属性"}p{class:"dme-object-id","{object.id}"}
            if let Some(t)=scene::transform(&object.geometry) {
                for (field,label,value) in [(0,"对象 X",t.translation.x),(1,"对象 Y",t.translation.y),(2,"对象缩放",t.scale),(3,"对象角度",t.rotation.to_degrees())] {
                    label{"{label}" input{r#type:"number",step:"0.1","aria-label":label,value,
                        onchange:move|e|{if let Ok(value)=e.value().parse::<f64>(){let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|{if let Some(mut t)=scene::transform(&o.geometry){match field{0=>t.translation.x=value,1=>t.translation.y=value,2=>t.scale=value,_=>t.rotation=value.to_radians()}scene::set_transform(&mut o.geometry,t);}});apply(session,message,Ok(commands));}}
                    }}
                }
            }
            label{"对象图层" select{"aria-label":"对象图层",value:object.layer_id.to_string(),onchange:move|e|{let id=LayerId(e.value());let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|o.layer_id=id.clone());apply(session,message,Ok(commands));},for layer in &layers {option{value:layer.id.to_string(),"{layer.name}"}}}}
            label{"语义颜色" button{onclick:move|_|{tools.write().paint=Paint::default();let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|o.paint=Paint{role:o.paint.role.clone(),..Paint::default()});apply(session,message,Ok(commands));},"跟随地图风格"}}
            if let Geometry::Path{nodes,closed,..}=&object.geometry { {let count=nodes.len();let closed=*closed;rsx!{
                label{"节点编号" input{r#type:"number",min:"0",max:count.saturating_sub(1),"aria-label":"节点编号",value:node_index(),onchange:move|e|{if let Ok(i)=e.value().parse::<usize>(){node_index.set(i.min(count.saturating_sub(1)));}}}}
                div{class:"dme-property-actions",
                    button{onclick:move|_|{let i=node_index();let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|if let Geometry::Path{nodes,..}=&mut o.geometry{if let Some(a)=nodes.get(i){let b=nodes.get(i+1).or_else(||nodes.first()).unwrap_or(a);let p=Point::new((a.position.x+b.position.x)/2.,(a.position.y+b.position.y)/2.);nodes.insert(i+1,PathNode{position:p,incoming:None,outgoing:None});}});apply(session,message,Ok(commands));},"追加节点"}
                    button{disabled:count<=if closed{3}else{2},onclick:move|_|{let i=node_index();let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|if let Geometry::Path{nodes,..}=&mut o.geometry{if i<nodes.len(){nodes.remove(i);}});apply(session,message,Ok(commands));node_index.set(i.saturating_sub(1));},"删除节点"}
                    button{onclick:move|_|{let i=node_index();let commands=patch_selection(session.peek().snapshot(),&tools.peek(),|o|if let Geometry::Path{nodes,..}=&mut o.geometry{if let Some(n)=nodes.get_mut(i){n.incoming=None;n.outgoing=None;}});apply(session,message,Ok(commands));},"清除控制点"}
                }
            }}}
        }
        h3{"画框"}
        for (field,label,value) in [(0,"画框 X",session.read().snapshot().document.frame.min.x),(1,"画框 Y",session.read().snapshot().document.frame.min.y),(2,"画框宽度",session.read().snapshot().document.frame.width),(3,"画框高度",session.read().snapshot().document.frame.height)] {
            label{"{label}" input{r#type:"number","aria-label":label,value,onchange:move|e|{if let Ok(value)=e.value().parse::<f64>() {let mut frame=session.peek().snapshot().document.frame;match field{0=>frame.min.x=value,1=>frame.min.y=value,2=>frame.width=value,_=>frame.height=value}apply(session,message,Ok(vec![Command::SetFrame(frame)]));}}}}
        }
        h3{"图层"}
        div{class:"dme-layer-list",for (index,layer) in layers.iter().enumerate(){ {let id=layer.id.clone();let show=layer.clone();let lock=layer.clone();let rename=layer.clone();let up=layer.id.clone();let down=layer.id.clone();let remove=layer.id.clone();let count=layers.len();rsx!{
            div{class:"dme-layer-row",button{"aria-pressed":tools.read().layer==id,onclick:move|_|tools.write().layer=id.clone(),"{layer.name}"}
                input{"aria-label":format!("图层名称 {}",layer.id),value:layer.name.clone(),onchange:move|e|{let mut layer=rename.clone();layer.name=e.value();apply(session,message,Ok(vec![Command::PutLayer{layer,index}]));}}
                button{"aria-label":format!("显隐图层 {}",layer.id),onclick:move|_|{let mut layer=show.clone();layer.visible= !layer.visible;apply(session,message,Ok(vec![Command::PutLayer{layer,index}]));},if layer.visible{"隐藏"}else{"显示"}}
                button{"aria-label":format!("锁定图层 {}",layer.id),onclick:move|_|{let mut layer=lock.clone();layer.locked= !layer.locked;apply(session,message,Ok(vec![Command::PutLayer{layer,index}]));},if layer.locked{"解锁"}else{"锁定"}}
                button{disabled:index+1>=count,"aria-label":format!("上移图层 {}",layer.id),onclick:move|_|apply(session,message,Ok(vec![reorder_layer(session,&up,index+1)])),"↑"}
                button{disabled:index==0,"aria-label":format!("下移图层 {}",layer.id),onclick:move|_|apply(session,message,Ok(vec![reorder_layer(session,&down,index-1)])),"↓"}
                button{disabled:count<=1,"aria-label":format!("删除图层 {}",layer.id),onclick:move|_|{let nonempty=session.peek().snapshot().document.objects.iter().any(|o|o.layer_id==remove);if nonempty{pending_delete.set(Some(remove.clone()));}else{apply(session,message,Ok(vec![Command::RemoveLayer{id:remove.clone(),delete_objects:false}]));}},"删除"}
            }
        }}}}
        if let Some(id)=pending_delete() {div{class:"dme-notice",role:"alert",p{"删除此图层及其全部对象？可撤销。"}button{onclick:move|_|{apply(session,message,Ok(vec![Command::RemoveLayer{id:id.clone(),delete_objects:true}]));pending_delete.set(None);},"确认删除图层及对象"}button{onclick:move|_|pending_delete.set(None),"取消删除图层"}}}
        input{"aria-label":"新图层名称",value:new_name(),oninput:move|e|new_name.set(e.value())}
        button{onclick:move|_|{let s=session.peek();let mut n=s.stamp().revision;let id=loop{n=n.saturating_add(1);let id=LayerId(format!("layer-{n}"));if !s.snapshot().document.layers.iter().any(|l|l.id==id){break id;}};let index=s.snapshot().document.layers.len();drop(s);apply(session,message,Ok(vec![Command::PutLayer{layer:Layer::new(id.clone(),new_name()),index}]));tools.write().layer=id;},"新增图层"}
        p{class:"dme-notice",role:"status","{message}"}
        p{class:"dme-help","选择后拖动移动；角手柄等比缩放，圆手柄旋转。Shift 多选，空白处拖框；路径节点和控制点可拖动。Enter 结束路径，Esc 取消。"}
    }}
}

fn reorder_layer(session: Signal<MapSession>, id: &LayerId, index: usize) -> Command {
    let mut ids: Vec<_> = session
        .peek()
        .snapshot()
        .document
        .layers
        .iter()
        .map(|l| l.id.clone())
        .collect();
    if let Some(from) = ids.iter().position(|v| v == id) {
        let id = ids.remove(from);
        ids.insert(index.min(ids.len()), id);
    }
    Command::ReorderLayers(ids)
}

/// Optional map preset and host candidate UI; all intents also exist in `map`.
#[component]
pub fn MapTools(
    mut session: Signal<MapSession>,
    mut tools: Signal<EditorTools>,
    host: Option<Callback<Vec<crate::command::AnchorPatch>, MapResult<()>>>,
    onactivate: EventHandler<AnchorId>,
) -> Element {
    let mut message = use_signal(String::new);
    let mut category = use_signal(|| "绿植".to_string());
    let selected = {
        let s = session.read();
        s.snapshot()
            .document
            .objects
            .iter()
            .find(|o| tools.read().selection.contains(&o.id))
            .cloned()
    };
    let anchors = session
        .read()
        .snapshot()
        .anchors
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let settings = tools.read().scatter.clone();
    let mut use_host = move |commands: MapResult<Vec<Command>>| {
        let result =
            commands.and_then(|c| session.write().execute_with_host(c, &mut HostPolicy(host)));
        message.set(format!("{result:?}"));
    };
    rsx! {details{class:"dme-map-tools",open:true,summary{"地图绘制工具"},
            div{class:"dme-editor-toolbar",
                button{"aria-pressed":tools.read().tool==EditorTool::MapNode,onclick:move|_|{let mut t=tools.write();t.tool=EditorTool::MapNode;t.stamp=ResourceRef{key:"builtin:town".into(),version:1};t.paint=Paint{role:"node".into(),..Paint::default()};},"地点节点"}
                label{"节点名称" input{"aria-label":"新节点名称",value:tools.read().node_name.clone(),oninput:move|e|tools.write().node_name=e.value()}}
                for (preset,label) in [(crate::map::MapPreset::Land,"陆地"),(crate::map::MapPreset::Water,"水域"),(crate::map::MapPreset::Lake,"湖泊"),(crate::map::MapPreset::Mountain,"山地区域"),(crate::map::MapPreset::Forest,"森林区域"),(crate::map::MapPreset::Boundary,"边界"),(crate::map::MapPreset::River,"河流"),(crate::map::MapPreset::Road,"道路"),(crate::map::MapPreset::Trail,"小径")] {button{onclick:move|_|preset.apply(&mut tools.write()),"{label}"}}
                select{"aria-label":"地图图章分类",value:category(),onchange:move|e|category.set(e.value()),option{"绿植"}option{"地形"}option{"城镇"}}
                for (key,name,group) in [("builtin:tree","树木","绿植"),("builtin:forest","森林","绿植"),("builtin:mountain","山体","地形"),("builtin:town","城镇","城镇")] {if category()==group{button{onclick:move|_|{let mut t=tools.write();t.stamp=ResourceRef{key:key.into(),version:1};t.tool=EditorTool::Stamp;t.paint=Paint{role:if group=="绿植"{"vegetation"}else{"ink"}.into(),..Paint::default()};},"{name}"}}}
                button{"aria-pressed":tools.read().tool==EditorTool::Scatter,onclick:move|_|{let mut t=tools.write();t.tool=EditorTool::Scatter;t.stamp=ResourceRef{key:"builtin:tree".into(),version:1};t.paint=Paint{role:"vegetation".into(),..Paint::default()};},"连续种植"}
            }
            div{class:"dme-editor-toolbar",
                for (field,label,value) in [(0,"种植间距",settings.spacing),(1,"种植密度",settings.density),(2,"最小大小",settings.size[0]),(3,"最大大小",settings.size[1]),(4,"旋转范围",settings.rotation[1].to_degrees()),(5,"散布宽度",settings.spread)] {
                    label{"{label}" input{r#type:"number",step:"0.1","aria-label":label,value,onchange:move|e|{if let Ok(v)=e.value().parse::<f64>() {let mut t=tools.write();match field{0=>t.scatter.spacing=v,1=>t.scatter.density=v,2=>t.scatter.size[0]=v,3=>t.scatter.size[1]=v,4=>t.scatter.rotation=[-v.to_radians(),v.to_radians()],_=>{let _=t.scatter.resize(v);}}}}}}
                }
            }
            if let Some(object)=selected {if let Some(label)=crate::map::node_label(&object){
                div{class:"dme-editor-toolbar",
                    select{"aria-label":"地点图标",value:match &object.geometry{Geometry::Stamp{stamp,..}=>stamp.key.clone(),_=>String::new()},onchange:move|e|{let commands={let s=session.peek();s.snapshot().document.objects.iter().filter(|o|tools.peek().selection.contains(&o.id)&&crate::map::node_label(o).is_some()).cloned().map(|mut o|{if let Geometry::Stamp{stamp,..}=&mut o.geometry{stamp.key=e.value();stamp.version=1;}Command::PutObject{object:o,index:0}}).collect()};use_host(Ok(commands));},for (key,name) in [("builtin:town","城镇"),("builtin:mountain","山体"),("builtin:tree","树木"),("builtin:flag","旗帜")] {option{value:key,"{name}"}}}
                    label{"地点名称" input{"aria-label":"地点名称",value:label.name,onchange:move|e|{let mut object=object.clone();let offset=crate::map::node_label(&object).map(|l|l.offset).unwrap_or_default();let result=crate::map::set_node_label(&mut object,e.value(),offset).map(|()|vec![Command::PutObject{object,index:0}]);use_host(result);}}}
                    for (axis,name,value) in [(0,"标签偏移 X",label.offset.x),(1,"标签偏移 Y",label.offset.y)] {input{r#type:"number","aria-label":name,value,onchange:move|e|{if let Ok(v)=e.value().parse::<f64>(){let commands={let s=session.peek();s.snapshot().document.objects.iter().filter(|o|tools.peek().selection.contains(&o.id)).cloned().filter_map(|mut o|{let l=crate::map::node_label(&o)?;let mut offset=l.offset;if axis==0{offset.x=v}else{offset.y=v}Some(crate::map::set_node_label(&mut o,l.name,offset).map(|()|Command::PutObject{object:o,index:0}))}).collect::<MapResult<Vec<_>>>()};use_host(commands);}}}}
                }
            }}
            if !anchors.is_empty(){div{class:"dme-anchor-panel",h3{"宿主地点候选"}
                for a in anchors { {let status=crate::map::anchor_status(session.read().snapshot(),&a);let id=a.id.clone();let unlocate=a.id.clone();let nav=a.id.clone();let appearance_id=a.id.clone();let appearance= session.read().snapshot().document.objects.iter().find_map(|o|match &o.geometry{Geometry::BoundMarker{anchor_id,appearance}if anchor_id==&a.id=>Some(appearance.clone()),_=>None}).unwrap_or_else(crate::map::default_appearance);rsx!{
                    div{class:"dme-anchor-row",span{"{a.name} · {status:?}"}
                        button{disabled:status==crate::map::AnchorStatus::Invalid,"aria-label":format!("放置地点 {}",a.id),onclick:move|_|{tools.write().anchor=Some(id.clone());tools.write().tool=EditorTool::Anchor;},"放置 / 重新定位"}
                        button{disabled:status!=crate::map::AnchorStatus::Located,"aria-label":format!("解除定位 {}",a.id),onclick:move|_|use_host(Ok(vec![Command::MoveAnchor{id:unlocate.clone(),point:None}])) ,"解除定位"}
                        button{onclick:move|_|onactivate.call(nav.clone()),"打开地点"}
                        {let id=a.id.clone();rsx!{select{disabled:status==crate::map::AnchorStatus::Invalid,"aria-label":format!("宿主图标 {}",a.id),value:appearance.icon.key.clone(),onchange:move|e|{let commands={let s=session.peek();let mut appearance=s.snapshot().document.objects.iter().find_map(|o|match &o.geometry{Geometry::BoundMarker{anchor_id,appearance}if anchor_id==&id=>Some(appearance.clone()),_=>None}).unwrap_or_else(crate::map::default_appearance);appearance.icon=ResourceRef{key:e.value(),version:1};crate::map::set_anchor_appearance(s.snapshot(),s.stamp().revision,&id,appearance)};use_host(commands);},for (key,name) in [("builtin:town","城镇"),("builtin:mountain","山体"),("builtin:tree","树木"),("builtin:flag","旗帜")] {option{value:key,"{name}"}}}}}
                        for (axis,name,value) in [(0,"宿主标签 X",appearance.label_offset.x),(1,"宿主标签 Y",appearance.label_offset.y),(2,"宿主图标比例",appearance.scale)] { {let id=appearance_id.clone();rsx!{input{r#type:"number","aria-label":format!("{name} {}",a.id),value,onchange:move|e|{if let Ok(v)=e.value().parse::<f64>(){let command={let s=session.peek();let mut appearance=s.snapshot().document.objects.iter().find_map(|o|match &o.geometry{Geometry::BoundMarker{anchor_id,appearance}if anchor_id==&id=>Some(appearance.clone()),_=>None}).unwrap_or_else(crate::map::default_appearance);
    match axis{0=>appearance.label_offset.x=v,1=>appearance.label_offset.y=v,_=>appearance.scale=v}
    crate::map::set_anchor_appearance(s.snapshot(),s.stamp().revision,&id,appearance)};use_host(command);}}}}}}
                    }
                }}}
            }}
            p{class:"dme-help",role:"status","{message}"}
            p{class:"dme-help","默认投影位于首图层，通过候选列表放置后可选中拖动。道路首尾在创建时吸附地点；无持续连接。区域按住拖绘松手生效，点击路径逐点提交。连续种植拖动一次对应一次撤销。"}
        }}
}
