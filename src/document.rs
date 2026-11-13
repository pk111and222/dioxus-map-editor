//! Versioned persistent content, host anchor snapshots and shared validation.
use crate::error::{MapError, MapResult};
use crate::geometry::{PathNode, Point, Transform, WorldRect};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Initial independently versioned document format. No legacy format is implied.
pub const SCHEMA_VERSION: u32 = 1;
/// Namespaced opaque values, preserved through edits, undo and serialization.
pub type Extensions = BTreeMap<String, Value>;

macro_rules! id_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
id_type!(MapId, "Opaque map identity provided by the host.");
id_type!(
    ObjectId,
    "Stable object identity, independent of list order."
);
id_type!(
    LayerId,
    "Stable layer identity, independent of its display name."
);
id_type!(
    AnchorId,
    "Opaque host anchor identity; never a copied place name."
);

/// A stable resource key and separately versioned definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRef {
    /// Namespaced key, e.g. builtin:simple.
    pub key: String,
    /// Version of this resource's definition.
    pub version: u32,
}
impl Default for ResourceRef {
    fn default() -> Self {
        Self {
            key: "builtin:simple".into(),
            version: 1,
        }
    }
}

/// A layer's persistent display and edit policy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    /// Stable layer identity.
    pub id: LayerId,
    /// User-facing name, not an identifier.
    pub name: String,
    /// Hidden layers remain persisted.
    pub visible: bool,
    /// Locked layers reject object and associated anchor edits.
    pub locked: bool,
    /// Provider-owned fields.
    #[serde(default)]
    pub extensions: Extensions,
}
impl Layer {
    /// Construct an empty visible, unlocked layer.
    pub fn new(id: impl Into<LayerId>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            visible: true,
            locked: false,
            extensions: Extensions::new(),
        }
    }
}

/// Explicit colors use validated #RRGGBB / #RRGGBBAA, never arbitrary CSS.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Paint {
    /// Semantic role, e.g. ink, river, label.
    pub role: String,
    /// Stroke override.
    pub color: Option<String>,
    /// Fill override.
    pub fill: Option<String>,
    /// Width in world units.
    pub width: Option<f64>,
    /// Alpha from zero to one.
    pub opacity: Option<f64>,
}
impl Default for Paint {
    fn default() -> Self {
        Self {
            role: "ink".into(),
            color: None,
            fill: None,
            width: None,
            opacity: None,
        }
    }
}

/// Bound marker appearance, deliberately without a world position or place name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkerAppearance {
    /// Resource used for the marker icon.
    pub icon: ResourceRef,
    /// Appearance-only scale.
    pub scale: f64,
    /// Label offset from the host's position.
    pub label_offset: Point,
}

/// First-party geometry. Unknown kinds are protected by the loader, not discarded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Geometry {
    /// Freehand polyline; persistent samples only, no cached SVG.
    Stroke {
        /// Local samples.
        points: Vec<Point>,
        /// Object transform.
        transform: Transform,
    },
    /// Editable vector path.
    Path {
        /// Endpoint and optional controls.
        nodes: Vec<PathNode>,
        /// Whether the last point connects to the first.
        closed: bool,
        /// Whether to use smooth interpolation.
        smooth: bool,
        /// Object transform.
        transform: Transform,
    },
    /// Registered vector symbol.
    Stamp {
        /// Versioned symbol key.
        stamp: ResourceRef,
        /// Placement transform.
        transform: Transform,
    },
    /// Single-line label.
    Text {
        /// Text content, never raw SVG markup.
        text: String,
        /// Placement transform.
        transform: Transform,
    },
    /// Versioned plugin object. Unknown providers retain payload and fallback bounds.
    Custom {
        /// Namespaced codec key and payload version.
        object_type: ResourceRef,
        /// Provider-owned JSON, preserved even when its plugin is absent.
        payload: Box<Value>,
        /// Local fallback rectangle; known codecs must agree with this value.
        bounds: WorldRect,
        /// Standard placement, rotation and scale.
        transform: Transform,
    },
    /// Reference to a host place; coordinates and name live only in the anchor.
    BoundMarker {
        /// Host identity.
        anchor_id: AnchorId,
        /// Display-only settings.
        appearance: MarkerAppearance,
    },
}

/// Persistent object with a stable identity and layer reference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapObject {
    /// Stable identity.
    pub id: ObjectId,
    /// Owning layer.
    pub layer_id: LayerId,
    /// Editable geometry.
    pub geometry: Geometry,
    /// Semantic paint and overrides.
    pub paint: Paint,
    /// Namespaced fields not interpreted by core.
    #[serde(default)]
    pub extensions: Extensions,
}

/// Persistent map content. UI tools, viewport, history and caches are excluded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapDocument {
    /// Document schema, independent of the crate and resource versions.
    pub schema_version: u32,
    /// Host-provided stable map identity.
    pub id: MapId,
    /// Finite export rectangle.
    pub frame: WorldRect,
    /// Semantic style resource.
    pub style: ResourceRef,
    /// Bottom-to-top layer order.
    pub layers: Vec<Layer>,
    /// Object order within each layer is its order in this sequence.
    pub objects: Vec<MapObject>,
    /// Opaque provider values, round-tripped unchanged.
    #[serde(default)]
    pub extensions: Extensions,
}
impl MapDocument {
    /// Empty content. Merely opening this document does not require writing it.
    pub fn new(id: impl Into<MapId>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            id: id.into(),
            frame: WorldRect::default(),
            style: ResourceRef::default(),
            layers: vec![Layer::new("base", "Base")],
            objects: Vec::new(),
            extensions: Extensions::new(),
        }
    }

    /// Validate model and encoded size at the same boundary used for commands.
    pub fn validate(&self, limits: &Limits) -> MapResult<()> {
        if self.schema_version != SCHEMA_VERSION {
            return invalid("unsupported schema");
        }
        valid_id(&self.id.0)?;
        if !self.frame.is_valid() {
            return invalid("frame");
        }
        resource(&self.style)?;
        extensions(&self.extensions)?;
        if self.layers.is_empty() {
            return invalid("at least one layer required");
        }
        if self.layers.len() > limits.max_layers || self.objects.len() > limits.max_objects {
            return Err(MapError::Budget("layers or objects".into()));
        }
        let mut layers = BTreeSet::new();
        for layer in &self.layers {
            valid_id(&layer.id.0)?;
            text(&layer.name, limits)?;
            extensions(&layer.extensions)?;
            if !layers.insert(&layer.id) {
                return Err(MapError::Duplicate(layer.id.to_string()));
            }
        }
        let mut ids = BTreeSet::new();
        let mut bound = BTreeSet::new();
        let mut nodes = 0usize;
        for object in &self.objects {
            valid_id(&object.id.0)?;
            if !ids.insert(&object.id) {
                return Err(MapError::Duplicate(object.id.to_string()));
            }
            if !layers.contains(&object.layer_id) {
                return Err(MapError::Missing(object.layer_id.to_string()));
            }
            extensions(&object.extensions)?;
            paint(&object.paint)?;
            let count = match &object.geometry {
                Geometry::Stroke { points, transform } => {
                    if points.is_empty() {
                        return invalid("empty stroke");
                    }
                    transform_points(*transform, points.iter().copied())?;
                    points.len()
                }
                Geometry::Path {
                    nodes,
                    closed,
                    transform,
                    ..
                } => {
                    if nodes.len() < if *closed { 3 } else { 2 } {
                        return invalid("path nodes");
                    }
                    let points = nodes.iter().flat_map(|n| {
                        [Some(n.position), n.incoming, n.outgoing]
                            .into_iter()
                            .flatten()
                    });
                    transform_points(*transform, points)?;
                    nodes
                        .iter()
                        .map(|n| {
                            1 + usize::from(n.incoming.is_some())
                                + usize::from(n.outgoing.is_some())
                        })
                        .sum()
                }
                Geometry::Stamp { stamp, transform } => {
                    resource(stamp)?;
                    transform_points(*transform, [Point::default()].into_iter())?;
                    0
                }
                Geometry::Text {
                    text: value,
                    transform,
                } => {
                    text(value, limits)?;
                    if value.contains(['\n', '\r']) {
                        return invalid("single-line text required");
                    }
                    transform_points(*transform, [Point::default()].into_iter())?;
                    0
                }
                Geometry::Custom {
                    object_type,
                    bounds,
                    transform,
                    ..
                } => {
                    resource(object_type)?;
                    if !bounds.is_valid() {
                        return invalid("custom object bounds");
                    }
                    transform_points(*transform, crate::scene::corners(*bounds).into_iter())?;
                    0
                }
                Geometry::BoundMarker {
                    anchor_id,
                    appearance,
                } => {
                    valid_id(&anchor_id.0)?;
                    if !bound.insert(anchor_id) {
                        return Err(MapError::Duplicate(format!("bound anchor {anchor_id}")));
                    }
                    resource(&appearance.icon)?;
                    if !appearance.scale.is_finite()
                        || appearance.scale <= 0.0
                        || !appearance.label_offset.is_finite()
                    {
                        return invalid("marker appearance");
                    }
                    // Missing anchors are allowed: unresolved references must survive reload.
                    0
                }
            };
            if count > limits.max_object_nodes {
                return Err(MapError::Budget("object nodes".into()));
            }
            nodes = nodes.checked_add(count).ok_or(MapError::Exhausted)?;
            if nodes > limits.max_nodes {
                return Err(MapError::Budget("document nodes".into()));
            }
        }
        if serde_json::to_vec(self)?.len() > limits.max_bytes {
            return Err(MapError::Budget("encoded document".into()));
        }
        Ok(())
    }
}

/// Validation/resource budgets, applied before parsing and after every transaction.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum commands in a transaction/gesture preview.
    pub max_commands: usize,
    /// Maximum encoded document or storage envelope size in bytes.
    pub max_bytes: usize,
    /// Maximum layer count.
    pub max_layers: usize,
    /// Maximum persistent object count.
    pub max_objects: usize,
    /// Maximum endpoints plus control points across the document.
    pub max_nodes: usize,
    /// Maximum nodes in one object.
    pub max_object_nodes: usize,
    /// Maximum UTF-8 bytes per text or label.
    pub max_text_bytes: usize,
    /// Maximum host anchor snapshot count.
    pub max_anchors: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_commands: 1024,
            max_bytes: 16 * 1024 * 1024,
            max_layers: 256,
            max_objects: 10_000,
            max_nodes: 100_000,
            max_object_nodes: 8192,
            max_text_bytes: 16_384,
            max_anchors: 10_000,
        }
    }
}

/// Position fields that can participate in host-accepted mixed transactions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorPosition {
    /// World X/Y.
    pub point: Point,
    /// Optional host altitude; never filled with a synthetic zero.
    pub z: Option<f64>,
    /// Map in whose coordinate system this position was authored.
    pub canvas: MapId,
}

/// Read-only host snapshot. Names/metadata are never part of position patches.
#[derive(Clone, Debug, PartialEq)]
pub struct ExternalAnchor {
    /// Stable host identity.
    pub id: AnchorId,
    /// Host name, refreshed independently from document history.
    pub name: String,
    /// Current host parent; commands cannot change it.
    pub parent_id: Option<String>,
    /// Optional location, retained even if it belongs to a different map.
    pub position: Option<AnchorPosition>,
    /// Host metadata, not serialized by document storage.
    pub metadata: Extensions,
}

/// Validated document and host snapshot. Only the session can mutate its copy.
#[derive(Clone, Debug, PartialEq)]
pub struct MapSnapshot {
    /// Persistent content.
    pub document: MapDocument,
    /// Host-owned data visible to this session.
    pub anchors: BTreeMap<AnchorId, ExternalAnchor>,
}
impl MapSnapshot {
    /// Document without host bindings, using the same session path as hosts.
    pub fn standalone(document: MapDocument) -> Self {
        Self {
            document,
            anchors: BTreeMap::new(),
        }
    }
    /// Validate the document and host snapshot without requiring every binding to resolve.
    pub fn validate(&self, limits: &Limits) -> MapResult<()> {
        self.document.validate(limits)?;
        if self.anchors.len() > limits.max_anchors {
            return Err(MapError::Budget("anchors".into()));
        }
        for (id, anchor) in &self.anchors {
            valid_id(&id.0)?;
            if id != &anchor.id {
                return invalid("anchor map key differs from ID");
            }
            text(&anchor.name, limits)?;
            if let Some(parent) = &anchor.parent_id {
                valid_id(parent)?;
            }
            if let Some(position) = &anchor.position {
                validate_position(position)?;
            }
            extensions(&anchor.metadata)?;
        }
        Ok(())
    }
}

pub(crate) fn validate_position(position: &AnchorPosition) -> MapResult<()> {
    valid_id(&position.canvas.0)?;
    if !position.point.is_finite() || position.z.is_some_and(|z| !z.is_finite()) {
        return invalid("anchor position");
    }
    Ok(())
}
pub(crate) fn valid_id(value: &str) -> MapResult<()> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return invalid("stable ID");
    }
    Ok(())
}
fn text(value: &str, limits: &Limits) -> MapResult<()> {
    if value.len() > limits.max_text_bytes {
        return Err(MapError::Budget("text".into()));
    }
    if value.contains('\0') {
        return invalid("NUL text");
    }
    Ok(())
}
fn resource(value: &ResourceRef) -> MapResult<()> {
    namespace(&value.key)?;
    if value.version == 0 {
        return invalid("resource version");
    }
    Ok(())
}
fn namespace(value: &str) -> MapResult<()> {
    valid_id(value)?;
    if !value
        .split_once(':')
        .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty())
    {
        return invalid("namespaced key");
    }
    Ok(())
}
fn extensions(values: &Extensions) -> MapResult<()> {
    for key in values.keys() {
        namespace(key)?;
    }
    Ok(())
}
fn paint(value: &Paint) -> MapResult<()> {
    valid_id(&value.role)?;
    for c in [&value.color, &value.fill].into_iter().flatten() {
        if !c.starts_with('#')
            || ![7, 9].contains(&c.len())
            || !c[1..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return invalid("paint color");
        }
    }
    if value.width.is_some_and(|x| !x.is_finite() || x <= 0.0)
        || value
            .opacity
            .is_some_and(|x| !x.is_finite() || !(0.0..=1.0).contains(&x))
    {
        return invalid("paint numeric value");
    }
    Ok(())
}
/// Largest accepted absolute world coordinate. Far below `f64::MAX`, so smoothing,
/// bounds and SVG output (which add and scale coordinates) stay finite.
pub const MAX_COORDINATE: f64 = 1e12;
fn transform_points(transform: Transform, points: impl Iterator<Item = Point>) -> MapResult<()> {
    for point in points {
        let world = transform
            .apply(point)
            .map_err(|e| MapError::Invalid(e.to_string()))?;
        if [point.x, point.y, world.x, world.y]
            .iter()
            .any(|v| v.abs() > MAX_COORDINATE)
        {
            return invalid("coordinate extent");
        }
    }
    Ok(())
}
pub(crate) fn invalid<T>(message: &str) -> MapResult<T> {
    Err(MapError::Invalid(message.into()))
}
