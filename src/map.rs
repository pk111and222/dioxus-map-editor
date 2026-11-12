//! Map-specific intents on ordinary objects and host-accepted anchor commands.
use crate::{
    command::Command,
    document::*,
    editor::{create_object, EditorTools},
    error::{MapError, MapResult},
    geometry::*,
    scene,
};
use serde::{Deserialize, Serialize};
/// Configure the shared scatter tool with any registered symbol, including plugin
/// resources. Trees, mountains and other entities use the same placement pipeline.
pub fn apply_symbol(
    tools: &mut EditorTools,
    registry: &crate::registry::Registry,
    symbol: ResourceRef,
    role: String,
) -> MapResult<()> {
    if registry.stamp(&symbol).is_none() {
        return Err(MapError::Missing(symbol.key));
    }
    tools.tool = crate::editor::EditorTool::Scatter;
    tools.brush = None;
    tools.stamp = symbol;
    tools.paint = Paint {
        role,
        ..Default::default()
    };
    Ok(())
}
/// Replace selected stamp resources while preserving positions, sizes, node labels
/// and other extensions. Submit the returned commands as one undoable transaction.
/// Mixed non-stamp, missing, hidden or locked selections reject the whole edit.
pub fn replace_symbols(
    s: &MapSnapshot,
    ids: &std::collections::BTreeSet<ObjectId>,
    symbol: ResourceRef,
    registry: &crate::registry::Registry,
) -> MapResult<Vec<Command>> {
    if registry.stamp(&symbol).is_none() {
        return Err(MapError::Missing(symbol.key));
    }
    let mut commands = Vec::new();
    for id in ids {
        let (index, original) = s
            .document
            .objects
            .iter()
            .enumerate()
            .find(|(_, o)| &o.id == id)
            .ok_or_else(|| MapError::Missing(id.to_string()))?;
        if !scene::editable(s, original) {
            return Err(MapError::ReadOnly);
        }
        let mut object = original.clone();
        let Geometry::Stamp { stamp, .. } = &mut object.geometry else {
            return Err(MapError::Invalid(
                "symbol replacement requires stamp objects".into(),
            ));
        };
        *stamp = symbol.clone();
        commands.push(Command::PutObject { object, index });
    }
    Ok(commands)
}
/// Versioned ordinary node appearance, stored in a namespaced stamp extension.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeLabel {
    /// Extension format, independent of document schema.
    pub version: u32,
    /// Ordinary map node name; bound nodes use host names instead.
    pub name: String,
    /// World-space label offset.
    pub offset: Point,
}
/// Read an ordinary node's supported label metadata.
pub fn node_label(o: &MapObject) -> Option<NodeLabel> {
    serde_json::from_value(o.extensions.get("map:node")?.clone())
        .ok()
        .filter(|n: &NodeLabel| {
            n.version == 1
                && n.offset.is_finite()
                && n.name.len() <= 16384
                && !n.name.chars().any(char::is_control)
        })
}
/// Update an ordinary node label without replacing other extensions.
pub fn set_node_label(o: &mut MapObject, name: String, offset: Point) -> MapResult<()> {
    if name.len() > 16384 || name.chars().any(char::is_control) || !offset.is_finite() {
        return Err(MapError::Invalid("node label".into()));
    }
    o.extensions.insert(
        "map:node".into(),
        serde_json::to_value(NodeLabel {
            version: 1,
            name,
            offset,
        })?,
    );
    Ok(())
}
/// Namespaced map preset implemented with sampled brushes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapPreset {
    /// Land region.
    Land,
    /// Water region.
    Water,
    /// Lake region.
    Lake,
    /// Mountain region.
    Mountain,
    /// Forest region.
    Forest,
    /// Region boundary without fill.
    Boundary,
    /// Smooth water course.
    River,
    /// Textured road ribbon.
    Road,
    /// Narrow textured walking trail ribbon.
    Trail,
}
impl MapPreset {
    /// Stable role key for style resolution and line patterns.
    pub fn role(self) -> &'static str {
        match self {
            Self::Land => "land",
            Self::Water => "water",
            Self::Lake => "lake",
            Self::Mountain => "mountain",
            Self::Forest => "forest",
            Self::Boundary => "boundary",
            Self::River => "river",
            Self::Road => "road",
            Self::Trail => "trail",
        }
    }
    /// Configure drag-to-draw tools, retaining all existing geometry.
    pub fn apply(self, t: &mut EditorTools) {
        t.tool = crate::editor::EditorTool::Brush;
        t.brush = None;
        t.closed = !matches!(self, Self::River | Self::Road | Self::Trail);
        t.smooth = matches!(self, Self::River | Self::Lake | Self::Road);
        t.paint = Paint {
            role: self.role().into(),
            width: match self {
                Self::River => Some(12.),
                Self::Road => Some(10.),
                Self::Trail => Some(6.),
                _ => None,
            },
            ..Paint::default()
        };
    }
}
/// Deterministic scatter settings. Results are stored as normal stamp objects.
#[derive(Clone, Debug, PartialEq)]
pub struct ScatterSettings {
    /// World distance between planting centers.
    pub spacing: f64,
    /// Probability of accepting a candidate, 0..=1.
    pub density: f64,
    /// Size interval.
    pub size: [f64; 2],
    /// Rotation interval, radians.
    pub rotation: [f64; 2],
    /// Maximum perpendicular displacement from the sampled route.
    pub spread: f64,
    /// Seed; no random work occurs during load or rendering.
    pub seed: u64,
}
impl Default for ScatterSettings {
    fn default() -> Self {
        Self {
            spacing: 28.,
            density: 1.,
            size: [0.65, 1.2],
            rotation: [-0.25, 0.25],
            spread: 12.,
            seed: 1,
        }
    }
}
impl ScatterSettings {
    /// Resize footprint and symbols together, retaining density and size variation.
    /// Hosts can still set spread and size separately for independent controls.
    pub fn resize(&mut self, spread: f64) -> MapResult<()> {
        if !spread.is_finite() || !(0.0..=10000.).contains(&spread) {
            return Err(MapError::Invalid("scatter radius".into()));
        }
        let ratio = (spread + 12.) / (self.spread + 12.);
        let mut next = self.clone();
        next.spread = spread;
        next.size = [self.size[0] * ratio, self.size[1] * ratio];
        next.spacing = (self.spacing * ratio).max(1.);
        next.validate()?;
        *self = next;
        Ok(())
    }
    /// Validate work and numeric limits before generating candidates.
    pub fn validate(&self) -> MapResult<()> {
        if !self.spacing.is_finite()
            || self.spacing < 1.
            || !self.density.is_finite()
            || !(0.0..=1.).contains(&self.density)
            || !self.spread.is_finite()
            || self.spread < 0.
            || self.spread > 10000.
            || self
                .size
                .iter()
                .any(|v| !v.is_finite() || *v <= 0. || *v > 100.)
            || self.size[0] > self.size[1]
            || self.rotation.iter().any(|v| !v.is_finite())
            || self.rotation[0] > self.rotation[1]
        {
            return Err(MapError::Invalid("scatter settings".into()));
        }
        Ok(())
    }
}
fn random(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (*seed >> 11) as f64 / ((1_u64 << 53) as f64)
}
/// Generate one atomic scatter batch, bounded by remaining object and command budgets.
/// Route resampling carries distance across segments, independent of event frequency.
pub fn scatter(
    s: &MapSnapshot,
    revision: u64,
    t: &EditorTools,
    route: &[Point],
    settings: &ScatterSettings,
    limits: &Limits,
) -> MapResult<Vec<Command>> {
    settings.validate()?;
    if route.is_empty() || route.iter().any(|p| !p.is_finite()) {
        return Err(MapError::Invalid("scatter route".into()));
    }
    let budget = limits
        .max_commands
        .min(limits.max_objects.saturating_sub(s.document.objects.len()));
    let mut candidates = vec![route[0]];
    let mut carry = 0.;
    for segment in route.windows(2) {
        let a = segment[0];
        let b = segment[1];
        let length = a.distance(b);
        if !length.is_finite() {
            return Err(MapError::Invalid("scatter route extent".into()));
        }
        let mut distance = settings.spacing - carry;
        while distance <= length {
            if candidates.len() >= limits.max_object_nodes {
                return Err(MapError::Budget("scatter candidates".into()));
            }
            let f = distance / length;
            candidates.push(Point::new(a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f));
            distance += settings.spacing;
        }
        carry = (carry + length) % settings.spacing;
    }
    let mut seed = settings.seed;
    let mut result = Vec::new();
    let mut number = revision;
    for p in candidates {
        if random(&mut seed) > settings.density {
            continue;
        }
        if result.len() >= budget {
            return Err(MapError::Budget("scatter objects or commands".into()));
        }
        let mut command = create_object(
            s,
            number,
            t,
            Geometry::Stamp {
                stamp: t.stamp.clone(),
                transform: Transform {
                    translation: Point::new(
                        p.x + (random(&mut seed) * 2. - 1.) * settings.spread,
                        p.y + (random(&mut seed) * 2. - 1.) * settings.spread,
                    ),
                    scale: settings.size[0]
                        + random(&mut seed) * (settings.size[1] - settings.size[0]),
                    rotation: settings.rotation[0]
                        + random(&mut seed) * (settings.rotation[1] - settings.rotation[0]),
                },
            },
        )?;
        if let Command::PutObject { object, index } = &mut command {
            loop {
                number = number.checked_add(1).ok_or(MapError::Exhausted)?;
                let id = ObjectId(format!("scatter-{number}"));
                if !s.document.objects.iter().any(|o| o.id == id) {
                    object.id = id;
                    break;
                }
            }
            *index = s.document.objects.len() + result.len();
            object.extensions.insert(
                "map:scatter".into(),
                serde_json::json!({"version":1,"seed":settings.seed}),
            );
        }
        result.push(command);
    }
    Ok(result)
}
/// Create an independent named node as a normal stamp with versioned label data.
pub fn create_node(
    s: &MapSnapshot,
    revision: u64,
    t: &EditorTools,
    p: Point,
    name: String,
) -> MapResult<Vec<Command>> {
    let mut c = create_object(
        s,
        revision,
        t,
        Geometry::Stamp {
            stamp: t.stamp.clone(),
            transform: Transform {
                translation: p,
                ..Transform::default()
            },
        },
    )?;
    if let Command::PutObject { object, .. } = &mut c {
        set_node_label(object, name, Point::new(18., 8.))?;
    }
    Ok(vec![c])
}
/// Host candidate state; no copied authoritative name/position enters BoundMarker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorStatus {
    /// Available in this map.
    Located,
    /// Host place has no coordinates.
    Unlocated,
    /// Parent or coordinate system does not match this map.
    Invalid,
}
/// Inspect host candidates, including unlocated and invalid places.
pub fn anchor_status(s: &MapSnapshot, a: &ExternalAnchor) -> AnchorStatus {
    if a.parent_id.as_deref() != Some(&s.document.id.0)
        || a.position
            .as_ref()
            .is_some_and(|p| p.canvas != s.document.id)
    {
        AnchorStatus::Invalid
    } else if a.position.is_some() {
        AnchorStatus::Located
    } else {
        AnchorStatus::Unlocated
    }
}
/// Default appearance for projected host places.
pub fn default_appearance() -> MarkerAppearance {
    MarkerAppearance {
        icon: ResourceRef {
            key: "builtin:town".into(),
            version: 1,
        },
        scale: 1.,
        label_offset: Point::new(18., 8.),
    }
}
/// Return one visible marker per located host place, with ephemeral identity when
/// it has no document appearance. Projection is never serialized or put in history.
pub fn projected_markers(s: &MapSnapshot) -> Vec<MapObject> {
    let Some(layer) = s.document.layers.first() else {
        return vec![];
    };
    s.anchors
        .values()
        .filter(|a| {
            anchor_status(s, a) == AnchorStatus::Located
                && !s.document.objects.iter().any(|o| {
                    matches!(&o.geometry,
                Geometry::BoundMarker { anchor_id, .. } if anchor_id == &a.id)
                })
        })
        .map(|a| {
            let mut id = ObjectId(format!("anchor:{}", a.id));
            while s.document.objects.iter().any(|o| o.id == id) {
                id.0.push(':');
            }
            MapObject {
                id,
                layer_id: layer.id.clone(),
                geometry: Geometry::BoundMarker {
                    anchor_id: a.id.clone(),
                    appearance: default_appearance(),
                },
                paint: Paint {
                    role: "node".into(),
                    ..Paint::default()
                },
                extensions: Extensions::new(),
            }
        })
        .collect()
}
/// Place/move one host node with appearance and coordinate in the same transaction.
pub fn place_anchor(
    s: &MapSnapshot,
    revision: u64,
    t: &EditorTools,
    id: &AnchorId,
    point: Point,
) -> MapResult<Vec<Command>> {
    let a = s
        .anchors
        .get(id)
        .ok_or_else(|| MapError::Missing(id.to_string()))?;
    if anchor_status(s, a) == AnchorStatus::Invalid {
        return Err(MapError::Invalid("anchor parent or map".into()));
    }
    let mut result = vec![];
    if !s
        .document
        .objects
        .iter()
        .any(|o| matches!(&o.geometry,Geometry::BoundMarker{anchor_id,..}if anchor_id==id))
    {
        result.push(create_object(
            s,
            revision,
            t,
            Geometry::BoundMarker {
                anchor_id: id.clone(),
                appearance: default_appearance(),
            },
        )?);
    }
    result.push(Command::MoveAnchor {
        id: id.clone(),
        point: Some(point),
    });
    Ok(result)
}
/// Edit only a host marker's map-owned appearance, including unlocated candidates.
/// Default projections materialize on their original first layer.
pub fn set_anchor_appearance(
    s: &MapSnapshot,
    revision: u64,
    id: &AnchorId,
    appearance: MarkerAppearance,
) -> MapResult<Vec<Command>> {
    let a = s
        .anchors
        .get(id)
        .ok_or_else(|| MapError::Missing(id.to_string()))?;
    if anchor_status(s, a) == AnchorStatus::Invalid {
        return Err(MapError::Invalid("anchor parent or map".into()));
    }
    let existing = s.document.objects.iter().find(|o| {
        matches!(&o.geometry,
        Geometry::BoundMarker { anchor_id, .. } if anchor_id == id)
    });
    let mut object = if let Some(o) = existing {
        o.clone()
    } else {
        MapObject {
            id: crate::editor::next_object_id(s, revision)?,
            layer_id: s.document.layers.first().ok_or(MapError::Stale)?.id.clone(),
            geometry: Geometry::BoundMarker {
                anchor_id: id.clone(),
                appearance: default_appearance(),
            },
            paint: Paint {
                role: "node".into(),
                ..Paint::default()
            },
            extensions: Extensions::new(),
        }
    };
    object.geometry = Geometry::BoundMarker {
        anchor_id: id.clone(),
        appearance,
    };
    Ok(vec![Command::PutObject {
        object,
        index: s.document.objects.len(),
    }])
}
/// Creation-only snapping: no persistent connection is created. Moving/deleting a
/// node later never changes an existing road. Only first/last road clicks snap.
pub fn snap_endpoint(s: &MapSnapshot, p: Point, scale: f64) -> Point {
    let mut nearest = p;
    let mut distance = 12. / scale;
    for o in s
        .document
        .objects
        .iter()
        .filter(|o| scene::visible(s, o) && node_label(o).is_some())
    {
        if let Some(t) = scene::transform(&o.geometry) {
            let d = t.translation.distance(p);
            if d < distance {
                distance = d;
                nearest = t.translation;
            }
        }
    }
    let projected = projected_markers(s);
    for o in s
        .document
        .objects
        .iter()
        .chain(projected.iter())
        .filter(|o| scene::visible(s, o))
    {
        let Geometry::BoundMarker { anchor_id, .. } = &o.geometry else {
            continue;
        };
        let Some(a) = s
            .anchors
            .get(anchor_id)
            .filter(|a| anchor_status(s, a) == AnchorStatus::Located)
        else {
            continue;
        };
        if let Some(pos) = &a.position {
            let d = pos.point.distance(p);
            if d < distance {
                distance = d;
                nearest = pos.point;
            }
        }
    }
    nearest
}
