//! Renderer-free tools and edit intents; persistent state remains in MapSession.
use crate::{
    command::Command,
    document::*,
    error::{MapError, MapResult},
    geometry::*,
    registry::Registry,
    scene,
    session::SessionStamp,
};
use std::collections::BTreeSet;
/// Tool mode. View and selection never enter document history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorTool {
    /// Click, Shift-toggle, marquee, drag selection.
    #[default]
    Select,
    /// Sampled freehand stroke.
    Brush,
    /// Click to extend an immediately committed polyline/region.
    Path,
    /// Single-line text overlay.
    Text,
    /// Repeated symbol placement.
    Stamp,
    /// Viewport navigation.
    Pan,
    /// Independent named map node.
    MapNode,
    /// Place or move a host-owned place selected in the candidate panel.
    Anchor,
    /// One deterministic planting gesture committed as normal stamp objects.
    Scatter,
}
/// Controlled editor preferences, selection and active layer.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorTools {
    /// Current tool.
    pub tool: EditorTool,
    /// Optional registered generator when the generic Brush tool is active.
    pub brush: Option<ResourceRef>,
    /// Host-owned custom brush options; never serialized into the document.
    pub brush_parameters: serde_json::Value,
    /// Selected stable IDs.
    pub selection: BTreeSet<ObjectId>,
    /// Target layer for newly created objects.
    pub layer: LayerId,
    /// Current brush/path/stamp paint.
    pub paint: Paint,
    /// World-space text size, separate from line width.
    pub text_size: f64,
    /// Placement symbol.
    pub stamp: ResourceRef,
    /// New paths use smooth interpolation.
    pub smooth: bool,
    /// New paths are closed regions.
    pub closed: bool,
    /// Natural outlines and contour alignment for new closed regions.
    pub region: crate::region::RegionSettings,
    /// New ordinary map node name.
    pub node_name: String,
    /// Candidate host identity for the anchor placement tool.
    pub anchor: Option<AnchorId>,
    /// Planting parameters; not persistent document data.
    pub scatter: crate::map::ScatterSettings,
}
impl Default for EditorTools {
    fn default() -> Self {
        Self {
            tool: EditorTool::Select,
            brush: None,
            brush_parameters: serde_json::Value::Null,
            selection: BTreeSet::new(),
            layer: "base".into(),
            paint: Paint {
                color: Some("#294c47".into()),
                width: Some(2.5),
                ..Paint::default()
            },
            text_size: 18.,
            stamp: ResourceRef {
                key: "builtin:diamond".into(),
                version: 1,
            },
            smooth: false,
            closed: false,
            region: Default::default(),
            node_name: "New place".into(),
            anchor: None,
            scatter: crate::map::ScatterSettings::default(),
        }
    }
}
impl EditorTools {
    /// Reject drawing on missing, hidden or locked layers before creating a preview.
    /// Hosts retain the selected layer and can offer an explicit unlock/show action.
    pub fn drawing_layer(&self, s: &MapSnapshot) -> MapResult<()> {
        let layer = s
            .document
            .layers
            .iter()
            .find(|l| l.id == self.layer)
            .ok_or_else(|| MapError::Missing(self.layer.to_string()))?;
        if layer.locked {
            return Err(MapError::ReadOnly);
        }
        if !layer.visible {
            return Err(MapError::Invalid("drawing layer is hidden".into()));
        }
        Ok(())
    }
    /// Remove stale, hidden and locked selections after undo, reload or layer edits.
    pub fn reconcile(&mut self, s: &MapSnapshot) {
        self.selection.retain(|id| {
            s.document
                .objects
                .iter()
                .any(|o| &o.id == id && scene::editable(s, o))
        });
        if !s.document.layers.iter().any(|l| l.id == self.layer) {
            if let Some(layer) = s.document.layers.first() {
                self.layer = layer.id.clone();
            }
        }
    }
}
/// Nonpersistent multi-click path draft.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathDraft {
    /// World-space endpoints; at most the document's per-object budget.
    pub nodes: Vec<PathNode>,
    /// Session identity at the first click.
    pub stamp: Option<SessionStamp>,
    /// Already committed path extended by subsequent clicks.
    pub object: Option<ObjectId>,
}
impl PathDraft {
    /// Commit a valid click path immediately; subsequent points replace the same
    /// object and each replacement is independently undoable. Before the minimum
    /// node count is reached, only the point handles are transient.
    pub fn command(
        &self,
        s: &MapSnapshot,
        revision: u64,
        tools: &EditorTools,
    ) -> MapResult<Option<Command>> {
        self.command_with_limits(s, revision, tools, &Limits::default())
    }
    /// Generate a click edit using the host's geometry budget.
    pub fn command_with_limits(
        &self,
        s: &MapSnapshot,
        revision: u64,
        tools: &EditorTools,
        limits: &Limits,
    ) -> MapResult<Option<Command>> {
        if self.nodes.len() < if tools.closed { 3 } else { 2 } {
            return Ok(None);
        }
        let decorated = tools.closed
            && (tools.region.roughness > 0.
                || tools.region.alignment != crate::region::EdgeAlignment::Free);
        let nodes = if tools.closed {
            crate::region::region_nodes(
                s,
                &self.nodes,
                tools.smooth,
                &tools.region,
                self.object.as_ref(),
                limits,
            )?
        } else {
            self.nodes.clone()
        };
        let geometry = Geometry::Path {
            nodes,
            closed: tools.closed,
            smooth: tools.smooth && !decorated,
            transform: Transform::default(),
        };
        if let Some(id) = &self.object {
            let (index, original) = s
                .document
                .objects
                .iter()
                .enumerate()
                .find(|(_, o)| &o.id == id)
                .ok_or(MapError::Stale)?;
            let mut object = original.clone();
            object.geometry = geometry;
            Ok(Some(Command::PutObject { object, index }))
        } else {
            create_object(s, revision, tools, geometry).map(Some)
        }
    }
    /// Add a point if sufficiently far from the last in screen pixels. Double
    /// click's repeated point is ignored before ending the draft.
    pub fn append(
        &mut self,
        p: Point,
        scale: f64,
        stamp: SessionStamp,
        limit: usize,
    ) -> MapResult<()> {
        if !p.is_finite() {
            return Err(MapError::Invalid("path point".into()));
        }
        if self.stamp.as_ref().is_some_and(|s| s != &stamp) {
            return Err(MapError::Stale);
        }
        if self
            .nodes
            .last()
            .is_some_and(|n| n.position.distance(p) * scale < 2.)
        {
            return Ok(());
        }
        if self.nodes.len() >= limit {
            return Err(MapError::Budget("path nodes".into()));
        }
        self.stamp = Some(stamp);
        self.nodes.push(PathNode {
            position: p,
            incoming: None,
            outgoing: None,
        });
        Ok(())
    }
    /// Clear only UI state.
    pub fn cancel(&mut self) {
        self.nodes.clear();
        self.stamp = None;
        self.object = None;
    }
}

/// Generate a freehand region, textured route ribbon, or ordinary stroke.
/// Preview and commit share this generator; only the final geometry is saved.
pub fn brush_commands(
    s: &MapSnapshot,
    revision: u64,
    tools: &EditorTools,
    samples: &[Point],
    width: f64,
    limits: &Limits,
) -> MapResult<Vec<Command>> {
    if samples.is_empty()
        || samples.iter().any(|p| !p.is_finite())
        || !width.is_finite()
        || width <= 0.
        || samples.len() > limits.max_object_nodes
    {
        return Err(MapError::Invalid("brush samples or width".into()));
    }
    let ribbon = matches!(tools.paint.role.as_str(), "river" | "road" | "trail");
    let mut points = simplify(samples, (width * 0.035).clamp(0.2, 1.));
    if ribbon && points.len() > 2 {
        let nodes = scene::smooth_nodes(&points, false);
        let mut curved = vec![points[0]];
        for pair in nodes.windows(2) {
            let a = pair[0].position;
            let b = pair[1].position;
            let c = pair[0].outgoing.unwrap_or(a);
            let d = pair[1].incoming.unwrap_or(b);
            let steps = ((a.distance(b) / 3.).ceil() as usize).clamp(2, 128);
            if curved.len().saturating_add(steps).saturating_mul(2) > limits.max_object_nodes {
                return Err(MapError::Budget("brush outline".into()));
            }
            for step in 1..=steps {
                let t = step as f64 / steps as f64;
                let u = 1. - t;
                curved.push(Point::new(
                    u * u * u * a.x + 3. * u * u * t * c.x + 3. * u * t * t * d.x + t * t * t * b.x,
                    u * u * u * a.y + 3. * u * u * t * c.y + 3. * u * t * t * d.y + t * t * t * b.y,
                ));
            }
        }
        points = curved;
    }
    let outline = if ribbon || (tools.closed && points.len() < 3) {
        let half = width / 2.;
        if points.len() < 2 || points.first() == points.last() && points.len() == 2 {
            (0..24)
                .map(|i| {
                    let a = i as f64 * std::f64::consts::TAU / 24.;
                    Point::new(points[0].x + half * a.cos(), points[0].y + half * a.sin())
                })
                .collect::<Vec<_>>()
        } else {
            let mut left = Vec::new();
            let mut right = Vec::new();
            let total: f64 = points.windows(2).map(|p| p[0].distance(p[1])).sum();
            let mut distance = 0.;
            for (i, p) in points.iter().enumerate() {
                if i > 0 {
                    distance += points[i - 1].distance(*p);
                }
                let a = points[i.saturating_sub(1)];
                let b = points[(i + 1).min(points.len() - 1)];
                let length = a.distance(b).max(0.001);
                let taper = if tools.paint.role == "river" {
                    0.45 + 0.55 * distance / total.max(0.001)
                } else {
                    1.
                };
                let radius = half * taper;
                let normal = Point::new(
                    -(b.y - a.y) / length * radius,
                    (b.x - a.x) / length * radius,
                );
                left.push(Point::new(p.x + normal.x, p.y + normal.y));
                right.push(Point::new(p.x - normal.x, p.y - normal.y));
            }
            let last = points[points.len() - 1];
            let end_angle =
                (last.y - points[points.len() - 2].y).atan2(last.x - points[points.len() - 2].x);
            for step in 1..=8 {
                let a = end_angle + std::f64::consts::FRAC_PI_2
                    - step as f64 * std::f64::consts::PI / 8.;
                left.push(Point::new(last.x + half * a.cos(), last.y + half * a.sin()));
            }
            left.extend(right.into_iter().rev());
            let start = points[0];
            let start_angle = (points[1].y - start.y).atan2(points[1].x - start.x);
            let radius = if tools.paint.role == "river" {
                half * 0.45
            } else {
                half
            };
            for step in 1..=8 {
                let a = start_angle
                    - std::f64::consts::FRAC_PI_2
                    - step as f64 * std::f64::consts::PI / 8.;
                left.push(Point::new(
                    start.x + radius * a.cos(),
                    start.y + radius * a.sin(),
                ));
            }
            left
        }
    } else {
        points
    };
    let decorated = tools.closed
        && !ribbon
        && (tools.region.roughness > 0.
            || tools.region.alignment != crate::region::EdgeAlignment::Free);
    let geometry = if ribbon || tools.closed || tools.tool == EditorTool::Path {
        let nodes = outline
            .into_iter()
            .map(|position| PathNode {
                position,
                incoming: None,
                outgoing: None,
            })
            .collect::<Vec<_>>();
        let nodes = if tools.closed && !ribbon {
            crate::region::region_nodes(s, &nodes, tools.smooth, &tools.region, None, limits)?
        } else {
            nodes
        };
        Geometry::Path {
            nodes,
            closed: ribbon || tools.closed,
            smooth: tools.smooth && !ribbon && !decorated,
            transform: Transform::default(),
        }
    } else {
        Geometry::Stroke {
            points: outline,
            transform: Transform::default(),
        }
    };
    let mut command = create_object(s, revision, tools, geometry)?;
    if let Command::PutObject { object, .. } = &mut command {
        if ribbon {
            object
                .extensions
                .insert("map:ribbon".into(), serde_json::json!({"version":1}));
            object.paint.fill = object.paint.color.clone();
            object.paint.width = Some(0.65);
        }
    }
    // The caller commits atomically; reject expanded outlines before showing them.
    let nodes = match &command {
        Command::PutObject { object, .. } => match &object.geometry {
            Geometry::Path { nodes, .. } => nodes.len(),
            Geometry::Stroke { points, .. } => points.len(),
            _ => 0,
        },
        _ => 0,
    };
    if nodes > limits.max_object_nodes {
        return Err(MapError::Budget("brush outline".into()));
    }
    Ok(vec![command])
}
/// First available generated ID, checking loaded data and history revisions.
pub fn next_object_id(s: &MapSnapshot, revision: u64) -> MapResult<ObjectId> {
    let used = used_ids(s);
    next_free_id(&used, "object", revision).map(|(id, _)| id)
}
/// Existing object IDs, collected once so allocation stays linear in document size.
pub(crate) fn used_ids(s: &MapSnapshot) -> std::collections::HashSet<&str> {
    s.document.objects.iter().map(|o| o.id.0.as_str()).collect()
}
/// First `{prefix}-{n}` after `after` that is not in `used`; also returns `n`.
pub(crate) fn next_free_id(
    used: &std::collections::HashSet<&str>,
    prefix: &str,
    after: u64,
) -> MapResult<(ObjectId, u64)> {
    let mut n = after;
    loop {
        n = n.checked_add(1).ok_or(MapError::Exhausted)?;
        let id = format!("{prefix}-{n}");
        if !used.contains(id.as_str()) {
            return Ok((ObjectId(id), n));
        }
    }
}
/// New ordinary object on the controlled active layer.
pub fn create_object(
    s: &MapSnapshot,
    revision: u64,
    tools: &EditorTools,
    geometry: Geometry,
) -> MapResult<Command> {
    let id = next_object_id(s, revision)?;
    Ok(object_with_id(s, tools, id, geometry))
}
/// Ordinary object with a caller-allocated ID (see `create_object`).
pub(crate) fn object_with_id(
    s: &MapSnapshot,
    tools: &EditorTools,
    id: ObjectId,
    geometry: Geometry,
) -> Command {
    let mut paint = tools.paint.clone();
    if matches!(geometry, Geometry::Text { .. }) {
        paint.role = "label".into();
        paint.width = Some(tools.text_size);
    }
    Command::PutObject {
        object: MapObject {
            id,
            layer_id: tools.layer.clone(),
            geometry,
            paint,
            extensions: Extensions::new(),
        },
        index: s.document.objects.len(),
    }
}
/// Deterministic Ramer-Douglas-Peucker, iterative to bound stack usage. Keep endpoints.
pub fn simplify(points: &[Point], tolerance: f64) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((first, last)) = stack.pop() {
        let a = points[first];
        let b = points[last];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let len = dx * dx + dy * dy;
        let mut best = tolerance;
        let mut index = None;
        for (i, p) in points.iter().enumerate().take(last).skip(first + 1) {
            let t = if len == 0. {
                0.
            } else {
                ((p.x - a.x) * dx + (p.y - a.y) * dy) / len
            }
            .clamp(0., 1.);
            let distance = p.distance(Point::new(a.x + t * dx, a.y + t * dy));
            if distance > best {
                best = distance;
                index = Some(i);
            }
        }
        if let Some(i) = index {
            keep[i] = true;
            stack.push((first, i));
            stack.push((i, last));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}
/// Commit one selection transform atomically, without modifying local geometry.
pub fn transform_selection(
    s: &MapSnapshot,
    ids: &BTreeSet<ObjectId>,
    pivot: Point,
    delta: Point,
    scale: f64,
    rotation: f64,
) -> MapResult<Vec<Command>> {
    if !pivot.is_finite()
        || !delta.is_finite()
        || !scale.is_finite()
        || scale <= 0.
        || !rotation.is_finite()
    {
        return Err(MapError::Invalid("selection transform".into()));
    }
    let operation = Transform {
        translation: Point::default(),
        scale,
        rotation,
    };
    let mut commands = Vec::new();
    for (id, index) in ids.iter().filter_map(|id| {
        s.document
            .objects
            .iter()
            .position(|o| &o.id == id)
            .map(|i| (id, i))
    }) {
        let mut o = s.document.objects[index].clone();
        if !scene::editable(s, &o) {
            return Err(MapError::Invalid(format!("uneditable {id}")));
        }
        if let Geometry::BoundMarker { anchor_id, .. } = &o.geometry {
            if scale != 1. || rotation != 0. {
                return Err(MapError::Invalid(
                    "host markers support position moves only".into(),
                ));
            }
            let anchor = s
                .anchors
                .get(anchor_id)
                .and_then(|a| a.position.as_ref())
                .filter(|p| p.canvas == s.document.id)
                .ok_or_else(|| MapError::Missing(anchor_id.to_string()))?;
            commands.push(Command::MoveAnchor {
                id: anchor_id.clone(),
                point: Some(Point::new(
                    anchor.point.x + delta.x,
                    anchor.point.y + delta.y,
                )),
            });
            continue;
        }
        let Some(mut t) = scene::transform(&o.geometry) else {
            continue;
        };
        let relative = operation
            .apply(Point::new(
                t.translation.x - pivot.x,
                t.translation.y - pivot.y,
            ))
            .map_err(|e| MapError::Invalid(e.to_string()))?;
        t.translation = Point::new(
            pivot.x + relative.x + delta.x,
            pivot.y + relative.y + delta.y,
        );
        t.scale *= scale;
        t.rotation += rotation;
        scene::set_transform(&mut o.geometry, t);
        commands.push(Command::PutObject { object: o, index });
    }
    Ok(commands)
}
/// Remove selected objects as one command batch.
pub fn delete_selection(ids: &BTreeSet<ObjectId>) -> Vec<Command> {
    ids.iter().cloned().map(Command::RemoveObject).collect()
}
/// Selection by intersection with the marquee. Hidden/locked layers are excluded.
pub fn marquee(s: &MapSnapshot, r: &Registry, rect: WorldRect) -> BTreeSet<ObjectId> {
    s.document
        .objects
        .iter()
        .filter(|o| scene::editable(s, o))
        .filter(|o| {
            scene::object_bounds(o, s, r).is_some_and(|b| {
                b.min.x <= rect.min.x + rect.width
                    && b.min.x + b.width >= rect.min.x
                    && b.min.y <= rect.min.y + rect.height
                    && b.min.y + b.height >= rect.min.y
            })
        })
        .map(|o| o.id.clone())
        .collect()
}
/// Selected aggregate bounds.
pub fn selection_bounds(
    s: &MapSnapshot,
    r: &Registry,
    ids: &BTreeSet<ObjectId>,
) -> Option<WorldRect> {
    let mut result: Option<WorldRect> = None;
    for o in s.document.objects.iter().filter(|o| ids.contains(&o.id)) {
        if let Some(b) = scene::object_bounds(o, s, r) {
            result = Some(match result {
                None => b,
                Some(a) => {
                    let min = Point::new(a.min.x.min(b.min.x), a.min.y.min(b.min.y));
                    WorldRect {
                        min,
                        width: (a.min.x + a.width).max(b.min.x + b.width) - min.x,
                        height: (a.min.y + a.height).max(b.min.y + b.height) - min.y,
                    }
                }
            });
        }
    }
    result
}
/// Move one path endpoint or handle in world coordinates. Endpoint moves preserve
/// relative handles. None `handle` moves endpoint; false incoming, true outgoing.
pub fn move_node(
    o: &MapObject,
    index: usize,
    handle: Option<bool>,
    world: Point,
) -> MapResult<MapObject> {
    let mut next = o.clone();
    let Geometry::Path {
        nodes, transform, ..
    } = &mut next.geometry
    else {
        return Err(MapError::Invalid("path required".into()));
    };
    let p = scene::inverse(*transform, world);
    let n = nodes
        .get_mut(index)
        .ok_or_else(|| MapError::Missing("path node".into()))?;
    match handle {
        Some(false) => n.incoming = Some(p),
        Some(true) => n.outgoing = Some(p),
        None => {
            let d = Point::new(p.x - n.position.x, p.y - n.position.y);
            n.position = p;
            for h in [&mut n.incoming, &mut n.outgoing].into_iter().flatten() {
                h.x += d.x;
                h.y += d.y;
            }
        }
    }
    Ok(next)
}
/// Fit world bounds into a finite pixel size with padding. No content change.
pub fn fit_view(bounds: WorldRect, width: f64, height: f64) -> MapResult<Viewport> {
    let mut view = Viewport::default();
    let scale = ((width - 48.) / bounds.width)
        .min((height - 48.) / bounds.height)
        .clamp(Viewport::MIN_SCALE, Viewport::MAX_SCALE);
    if !width.is_finite()
        || !height.is_finite()
        || width <= 48.
        || height <= 48.
        || !bounds.is_valid()
    {
        return Err(MapError::Invalid("fit viewport".into()));
    }
    view.zoom_at(Point::default(), scale)
        .map_err(|e| MapError::Invalid(e.to_string()))?;
    let center = Point::new(
        bounds.min.x + bounds.width / 2.,
        bounds.min.y + bounds.height / 2.,
    );
    let current = view.world_to_screen(center);
    view.pan(Point::new(width / 2. - current.x, height / 2. - current.y))
        .map_err(|e| MapError::Invalid(e.to_string()))?;
    Ok(view)
}
