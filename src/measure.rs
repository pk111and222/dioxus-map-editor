//! Renderer-free measurements for host annotations: map scale, distances, bearings,
//! path lengths and position lookups. Nothing here edits a document; hosts render the
//! results (labels, rulers, constraint warnings) through their own overlay or panel.
use crate::{
    document::{AnchorId, Extensions, Geometry, MapObject, MapSnapshot, ObjectId},
    error::{MapError, MapResult},
    geometry::{PathNode, Point},
    scene,
};
use serde::{Deserialize, Serialize};

/// Document extension key used by [`MapScale::read`] / [`MapScale::command`].
pub const SCALE_EXTENSION: &str = "dme:scale";

/// Real-world length of one world unit. Stored as a document extension so it
/// round-trips through saves, undo and older builds without a schema change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapScale {
    /// Real distance represented by one world unit (must be finite and positive).
    pub units_per_world: f64,
    /// Display unit, e.g. "km", "mi" or "里". Content, never translated by the crate.
    pub unit: String,
}
impl MapScale {
    /// Validate the numeric factor and unit text.
    pub fn validate(&self) -> MapResult<()> {
        if !self.units_per_world.is_finite() || self.units_per_world <= 0. {
            return Err(MapError::Invalid("map scale".into()));
        }
        if self.unit.len() > 64 || self.unit.chars().any(char::is_control) {
            return Err(MapError::Invalid("map scale unit".into()));
        }
        Ok(())
    }
    /// Read the scale from document extensions; `None` when unset or malformed.
    pub fn read(extensions: &Extensions) -> Option<Self> {
        let scale: Self = serde_json::from_value(extensions.get(SCALE_EXTENSION)?.clone()).ok()?;
        scale.validate().ok().map(|()| scale)
    }
    /// Undoable command that sets (`Some`) or clears (`None`) the document scale.
    pub fn command(scale: Option<&Self>) -> MapResult<crate::command::Command> {
        let value = match scale {
            Some(scale) => {
                scale.validate()?;
                Some(serde_json::to_value(scale)?)
            }
            None => None,
        };
        Ok(crate::command::Command::SetExtension {
            key: SCALE_EXTENSION.into(),
            value,
        })
    }
    /// Convert a world distance to real units.
    pub fn to_real(&self, world: f64) -> f64 {
        world * self.units_per_world
    }
    /// Convert a real distance to world units.
    pub fn to_world(&self, real: f64) -> f64 {
        real / self.units_per_world
    }
}

/// Straight-line world distance.
pub fn distance(a: Point, b: Point) -> f64 {
    a.distance(b)
}

/// Compass bearing from `a` to `b` in degrees: 0 = north (+Y), 90 = east (+X),
/// clockwise, in `[0, 360)`. `None` when the points coincide or are not finite.
pub fn bearing(a: Point, b: Point) -> Option<f64> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    if !dx.is_finite() || !dy.is_finite() || dx.hypot(dy) < f64::EPSILON {
        return None;
    }
    Some(dx.atan2(dy).to_degrees().rem_euclid(360.))
}

/// One of the eight compass directions, for "A lies north-east of B" style checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compass {
    /// 337.5°–22.5°
    North,
    /// 22.5°–67.5°
    NorthEast,
    /// 67.5°–112.5°
    East,
    /// 112.5°–157.5°
    SouthEast,
    /// 157.5°–202.5°
    South,
    /// 202.5°–247.5°
    SouthWest,
    /// 247.5°–292.5°
    West,
    /// 292.5°–337.5°
    NorthWest,
}
impl Compass {
    const ALL: [Self; 8] = [
        Self::North,
        Self::NorthEast,
        Self::East,
        Self::SouthEast,
        Self::South,
        Self::SouthWest,
        Self::West,
        Self::NorthWest,
    ];
    /// Nearest compass direction for a bearing in degrees.
    pub fn from_bearing(degrees: f64) -> Option<Self> {
        if !degrees.is_finite() {
            return None;
        }
        let sector = ((degrees.rem_euclid(360.) + 22.5) / 45.).floor() as usize % 8;
        Some(Self::ALL[sector])
    }
    /// Centre bearing of this direction in degrees.
    pub fn degrees(self) -> f64 {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0) as f64 * 45.
    }
}

/// Smallest absolute difference between two bearings, in `[0, 180]`.
pub fn bearing_difference(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.);
    d.min(360. - d)
}

/// Length of a polyline through `points` (world units).
pub fn polyline_length(points: &[Point]) -> f64 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// Length of a cubic path, flattening each curved segment into `steps` chords
/// (clamped to 1..=256). Straight segments are exact.
pub fn path_length(nodes: &[PathNode], closed: bool, steps: usize) -> f64 {
    let steps = steps.clamp(1, 256);
    let n = nodes.len();
    if n < 2 {
        return 0.;
    }
    let mut total = 0.;
    for i in 1..n + usize::from(closed) {
        let a = &nodes[(i - 1) % n];
        let b = &nodes[i % n];
        if a.outgoing.is_none() && b.incoming.is_none() {
            total += a.position.distance(b.position);
            continue;
        }
        let (p0, p3) = (a.position, b.position);
        let p1 = a.outgoing.unwrap_or(p0);
        let p2 = b.incoming.unwrap_or(p3);
        let mut previous = p0;
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let u = 1. - t;
            let p = Point::new(
                u * u * u * p0.x + 3. * u * u * t * p1.x + 3. * u * t * t * p2.x + t * t * t * p3.x,
                u * u * u * p0.y + 3. * u * u * t * p1.y + 3. * u * t * t * p2.y + t * t * t * p3.y,
            );
            total += previous.distance(p);
            previous = p;
        }
    }
    total
}

/// World length of a stroke or path object (transform scale applied); `None` for
/// objects without a linear shape. Uses the same smoothing as rendering.
pub fn object_length(o: &MapObject) -> Option<f64> {
    let (nodes, closed, transform) = match &o.geometry {
        Geometry::Stroke { points, transform } => {
            (scene::smooth_nodes(points, false), false, *transform)
        }
        Geometry::Path {
            nodes,
            closed,
            smooth,
            transform,
        } => (
            scene::path_nodes(nodes, *smooth, *closed),
            *closed,
            *transform,
        ),
        _ => return None,
    };
    transform
        .is_valid()
        .then(|| path_length(&nodes, closed, 16) * transform.scale)
}

/// World position of an object or bound host place, as drawn on this map.
/// Host anchors resolve through their snapshot position; ordinary objects use their
/// transform origin. `None` for unknown IDs, unlocated anchors or another map's anchor.
pub fn object_position(s: &MapSnapshot, id: &ObjectId) -> Option<Point> {
    let o = s.document.objects.iter().find(|o| &o.id == id)?;
    match &o.geometry {
        Geometry::BoundMarker { anchor_id, .. } => anchor_position(s, anchor_id),
        g => scene::transform(g).map(|t| t.translation),
    }
}

/// Position of a host anchor on this map, if it is located here.
pub fn anchor_position(s: &MapSnapshot, id: &AnchorId) -> Option<Point> {
    s.anchors
        .get(id)?
        .position
        .as_ref()
        .filter(|p| p.canvas == s.document.id)
        .map(|p| p.point)
}

/// Distance and direction between two located points, with optional real units.
#[derive(Clone, Debug, PartialEq)]
pub struct Measurement {
    /// World-space distance.
    pub world: f64,
    /// Real distance when the document has a [`MapScale`].
    pub real: Option<f64>,
    /// Bearing from the first to the second point, if they differ.
    pub bearing: Option<f64>,
    /// Nearest compass direction of `bearing`.
    pub compass: Option<Compass>,
}

/// Measure from `a` to `b` using the document's scale when present.
pub fn measure(s: &MapSnapshot, a: Point, b: Point) -> Measurement {
    let world = a.distance(b);
    let bearing = bearing(a, b);
    Measurement {
        world,
        real: MapScale::read(&s.document.extensions).map(|scale| scale.to_real(world)),
        bearing,
        compass: bearing.and_then(Compass::from_bearing),
    }
}
