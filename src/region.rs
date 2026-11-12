//! Deterministic region outlines, contour snapping and exact shared cubic borders.
//! Settings affect new geometry only; no random work or live constraint runs on load.
use crate::{
    document::*,
    error::{MapError, MapResult},
    geometry::*,
    scene,
};

/// How a new region meets visible, closed path objects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeAlignment {
    /// Preserve the authored outline.
    #[default]
    Free,
    /// Project nearby vertices onto existing contours.
    Snap,
    /// Also copy the shorter contour arc between consecutive snapped vertices.
    /// Cubic controls are copied exactly, including partial first/last segments.
    Shared,
}
/// Host-owned preferences for newly drawn regions; not part of document schema.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionSettings {
    /// Natural-edge displacement in world units; zero retains the authored edge.
    pub roughness: f64,
    /// Approximate distance between natural-edge samples in world units.
    pub detail: f64,
    /// Stable random seed, shared by preview and commit.
    pub seed: u64,
    /// Contour alignment mode.
    pub alignment: EdgeAlignment,
    /// Maximum vertex-to-contour distance in world units.
    pub snap_distance: f64,
    /// Optional contour object; None searches all visible closed paths.
    pub target: Option<ObjectId>,
}
impl Default for RegionSettings {
    fn default() -> Self {
        Self {
            roughness: 0.,
            detail: 12.,
            seed: 1,
            alignment: EdgeAlignment::Free,
            snap_distance: 12.,
            target: None,
        }
    }
}
impl RegionSettings {
    /// Reject nonfinite settings and unbounded geometry expansion.
    pub fn validate(&self) -> MapResult<()> {
        if !self.roughness.is_finite()
            || !(0.0..=1000.).contains(&self.roughness)
            || !self.detail.is_finite()
            || !(1.0..=10000.).contains(&self.detail)
            || !self.snap_distance.is_finite()
            || !(0.0..=10000.).contains(&self.snap_distance)
        {
            return Err(MapError::Invalid("region edge settings".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Curve([Point; 4]);
fn lerp(a: Point, b: Point, t: f64) -> Point {
    Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}
impl Curve {
    fn at(self, t: f64) -> Point {
        let [a, b, c, d] = self.0;
        lerp(
            lerp(lerp(a, b, t), lerp(b, c, t), t),
            lerp(lerp(b, c, t), lerp(c, d, t), t),
            t,
        )
    }
    fn split(self, t: f64) -> (Self, Self) {
        let [a, b, c, d] = self.0;
        let ab = lerp(a, b, t);
        let bc = lerp(b, c, t);
        let cd = lerp(c, d, t);
        let abc = lerp(ab, bc, t);
        let bcd = lerp(bc, cd, t);
        let p = lerp(abc, bcd, t);
        (Self([a, ab, abc, p]), Self([p, bcd, cd, d]))
    }
    fn slice(self, start: f64, end: f64) -> Self {
        if start >= 1. || end <= start {
            return Self([self.at(start); 4]);
        }
        if start <= 0. {
            return self.split(end).0;
        }
        let right = self.split(start).1;
        right.split(((end - start) / (1. - start)).clamp(0., 1.)).0
    }
    fn reverse(self) -> Self {
        let [a, b, c, d] = self.0;
        Self([d, c, b, a])
    }
    fn length(self) -> f64 {
        (1..=24)
            .map(|i| {
                self.at((i - 1) as f64 / 24.)
                    .distance(self.at(i as f64 / 24.))
            })
            .sum()
    }
}
fn curves(nodes: &[PathNode]) -> Vec<Curve> {
    (0..nodes.len())
        .map(|i| {
            let a = &nodes[i];
            let b = &nodes[(i + 1) % nodes.len()];
            let curved = a.outgoing.is_some() || b.incoming.is_some();
            Curve([
                a.position,
                a.outgoing.unwrap_or_else(|| {
                    if curved {
                        a.position
                    } else {
                        lerp(a.position, b.position, 1. / 3.)
                    }
                }),
                b.incoming.unwrap_or_else(|| {
                    if curved {
                        b.position
                    } else {
                        lerp(a.position, b.position, 2. / 3.)
                    }
                }),
                b.position,
            ])
        })
        .collect()
}
struct Contour {
    id: ObjectId,
    curves: Vec<Curve>,
}
#[derive(Clone, Copy)]
struct Hit {
    contour: usize,
    segment: usize,
    t: f64,
    point: Point,
}
fn contours(
    s: &MapSnapshot,
    settings: &RegionSettings,
    exclude: Option<&ObjectId>,
) -> Vec<Contour> {
    if settings.alignment == EdgeAlignment::Free {
        return vec![];
    }
    s.document
        .objects
        .iter()
        .filter_map(|o| {
            if Some(&o.id) == exclude
                || settings.target.as_ref().is_some_and(|id| id != &o.id)
                || o.extensions.contains_key("map:ribbon")
                || !s
                    .document
                    .layers
                    .iter()
                    .any(|l| l.id == o.layer_id && l.visible)
            {
                return None;
            }
            let Geometry::Path {
                nodes,
                closed: true,
                smooth,
                transform,
            } = &o.geometry
            else {
                return None;
            };
            let nodes = scene::path_nodes(nodes, *smooth, true)
                .into_iter()
                .map(|mut n| {
                    n.position = transform.apply(n.position).ok()?;
                    n.incoming = n.incoming.map(|p| transform.apply(p)).transpose().ok()?;
                    n.outgoing = n.outgoing.map(|p| transform.apply(p)).transpose().ok()?;
                    Some(n)
                })
                .collect::<Option<Vec<_>>>()?;
            Some(Contour {
                id: o.id.clone(),
                curves: curves(&nodes),
            })
        })
        .collect()
}
fn nearest(contours: &[Contour], p: Point, distance: f64) -> Option<Hit> {
    let mut best = distance;
    let mut hit = None;
    for (ci, c) in contours.iter().enumerate() {
        for (si, curve) in c.curves.iter().enumerate() {
            // The control hull bounds the cubic; skip distant curves before sampling.
            let min_x = curve.0.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
            let max_x = curve
                .0
                .iter()
                .map(|p| p.x)
                .fold(f64::NEG_INFINITY, f64::max);
            let min_y = curve.0.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
            let max_y = curve
                .0
                .iter()
                .map(|p| p.y)
                .fold(f64::NEG_INFINITY, f64::max);
            if p.x < min_x - best || p.x > max_x + best || p.y < min_y - best || p.y > max_y + best
            {
                continue;
            }
            // Project onto a bounded polyline, then refine the nearest cubic interval.
            let mut candidate = (f64::INFINITY, 0.);
            for i in 0..24 {
                let a = curve.at(i as f64 / 24.);
                let b = curve.at((i + 1) as f64 / 24.);
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let u = if dx == 0. && dy == 0. {
                    0.
                } else {
                    ((p.x - a.x) * dx + (p.y - a.y) * dy) / (dx * dx + dy * dy)
                }
                .clamp(0., 1.);
                let d = p.distance(lerp(a, b, u));
                if d < candidate.0 {
                    candidate = (d, (i as f64 + u) / 24.);
                }
            }
            let mut lo = (candidate.1 - 1. / 24.).max(0.);
            let mut hi = (candidate.1 + 1. / 24.).min(1.);
            for _ in 0..28 {
                let a = lo + (hi - lo) / 3.;
                let b = hi - (hi - lo) / 3.;
                if p.distance(curve.at(a)) < p.distance(curve.at(b)) {
                    hi = b;
                } else {
                    lo = a;
                }
            }
            let mut t = (lo + hi) / 2.;
            for end in [0., 1.] {
                if p.distance(curve.at(end)) <= p.distance(curve.at(t)) {
                    t = end;
                }
            }
            let point = curve.at(t);
            let d = p.distance(point);
            if d <= best {
                best = d;
                hit = Some(Hit {
                    contour: ci,
                    segment: si,
                    t,
                    point,
                });
            }
        }
    }
    hit
}
fn forward(c: &Contour, a: Hit, b: Hit) -> Vec<Curve> {
    if a.segment == b.segment && a.t <= b.t {
        return vec![c.curves[a.segment].slice(a.t, b.t)];
    }
    let mut result = vec![c.curves[a.segment].slice(a.t, 1.)];
    let mut i = (a.segment + 1) % c.curves.len();
    while i != b.segment {
        result.push(c.curves[i]);
        i = (i + 1) % c.curves.len();
    }
    result.push(c.curves[b.segment].slice(0., b.t));
    result.retain(|curve| curve.length() > 1e-10);
    result
}
fn shared(c: &Contour, a: Hit, b: Hit) -> Vec<Curve> {
    let forward = forward(c, a, b);
    let reverse = forward_arc_reverse(c, a, b);
    if forward.iter().map(|c| c.length()).sum::<f64>()
        <= reverse.iter().map(|c| c.length()).sum::<f64>()
    {
        forward
    } else {
        reverse
    }
}
fn forward_arc_reverse(c: &Contour, a: Hit, b: Hit) -> Vec<Curve> {
    forward(c, b, a)
        .into_iter()
        .rev()
        .map(Curve::reverse)
        .collect()
}
fn noise(seed: u64, i: usize) -> f64 {
    let mut x = seed.wrapping_add((i as u64).wrapping_mul(0x9e3779b97f4a7c15));
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((x ^ (x >> 31)) >> 11) as f64 / ((1_u64 << 53) as f64) * 2. - 1.
}

/// Build a closed outline for brush or click input. Shared edges retain exact cubic
/// geometry, even when the source is transformed. Hidden paths and route ribbons
/// are ignored; locked visible regions can be read as guides. `exclude` prevents
/// a click draft from snapping to its own already committed object.
pub fn region_nodes(
    s: &MapSnapshot,
    nodes: &[PathNode],
    smooth: bool,
    settings: &RegionSettings,
    exclude: Option<&ObjectId>,
    limits: &Limits,
) -> MapResult<Vec<PathNode>> {
    settings.validate()?;
    if nodes.len() < 3
        || nodes.len() > limits.max_object_nodes
        || nodes.iter().any(|n| {
            !n.position.is_finite()
                || n.incoming.is_some_and(|p| !p.is_finite())
                || n.outgoing.is_some_and(|p| !p.is_finite())
        })
    {
        return Err(MapError::Invalid("region nodes".into()));
    }
    if settings.roughness == 0. && settings.alignment == EdgeAlignment::Free {
        return Ok(nodes.to_vec());
    }
    let contours = contours(s, settings, exclude);
    let hits: Vec<_> = nodes
        .iter()
        .map(|n| nearest(&contours, n.position, settings.snap_distance))
        .collect();
    let mut authored = nodes.to_vec();
    for (n, hit) in authored.iter_mut().zip(&hits) {
        if let Some(hit) = hit {
            let delta = Point::new(hit.point.x - n.position.x, hit.point.y - n.position.y);
            n.position = hit.point;
            n.incoming = n.incoming.map(|p| Point::new(p.x + delta.x, p.y + delta.y));
            n.outgoing = n.outgoing.map(|p| Point::new(p.x + delta.x, p.y + delta.y));
        }
    }
    let authored = scene::path_nodes(&authored, smooth, true);
    let mut output = Vec::new();
    for (i, curve) in curves(&authored).into_iter().enumerate() {
        let next = (i + 1) % nodes.len();
        let segment = if let (EdgeAlignment::Shared, Some(a), Some(b)) =
            (settings.alignment, hits[i], hits[next])
        {
            if a.contour == b.contour && a.point.distance(b.point) > 0.001 {
                Some(shared(&contours[a.contour], a, b))
            } else {
                None
            }
        } else {
            None
        };
        let segment = if let Some(segment) = segment {
            segment
        } else if settings.roughness > 0. {
            let steps = (curve.length() / settings.detail).ceil().max(2.) as usize;
            if steps > limits.max_object_nodes.saturating_sub(output.len()) / 3 {
                return Err(MapError::Budget("natural region edge".into()));
            }
            let mut points = Vec::new();
            for j in 0..=steps {
                let t = j as f64 / steps as f64;
                let p = curve.at(t);
                let a = curve.at((t - 0.001).max(0.));
                let b = curve.at((t + 0.001).min(1.));
                let len = a.distance(b).max(0.001);
                let offset = if j == 0 || j == steps {
                    0.
                } else {
                    noise(settings.seed.wrapping_add(i as u64), j)
                        * settings.roughness
                        * (std::f64::consts::PI * t).sin()
                };
                points.push(Point::new(
                    p.x - (b.y - a.y) / len * offset,
                    p.y + (b.x - a.x) / len * offset,
                ));
            }
            scene::smooth_nodes(&points, false)
                .windows(2)
                .map(|n| {
                    Curve([
                        n[0].position,
                        n[0].outgoing.unwrap_or(n[0].position),
                        n[1].incoming.unwrap_or(n[1].position),
                        n[1].position,
                    ])
                })
                .collect()
        } else {
            vec![curve]
        };
        if output.len().saturating_add(segment.len()).saturating_mul(3) > limits.max_object_nodes {
            return Err(MapError::Budget("region edge nodes".into()));
        }
        output.extend(segment);
    }
    let result = (0..output.len())
        .map(|i| PathNode {
            position: output[i].0[0],
            incoming: Some(output[(i + output.len() - 1) % output.len()].0[2]),
            outgoing: Some(output[i].0[1]),
        })
        .collect();
    Ok(result)
}

/// Find a nearby region contour point for a host-owned snap indicator.
pub fn snap_point(
    s: &MapSnapshot,
    p: Point,
    settings: &RegionSettings,
    exclude: Option<&ObjectId>,
) -> Option<(ObjectId, Point)> {
    if settings.validate().is_err() {
        return None;
    }
    let contours = contours(s, settings, exclude);
    nearest(&contours, p, settings.snap_distance).map(|h| (contours[h.contour].id.clone(), h.point))
}
