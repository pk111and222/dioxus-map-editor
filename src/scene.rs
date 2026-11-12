//! Shared geometry, hit testing, cached scene fragments and safe SVG generation.
use crate::{document::*, geometry::*, registry::*};
use std::fmt::Write;

/// Escape text or a quoted XML attribute. Never accept raw user markup.
pub fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
/// Stable safe SVG identifier for any resource key and version.
pub fn resource_id(prefix: &str, r: &ResourceRef) -> String {
    let prefix: String = prefix
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
                (b as char).to_string()
            } else {
                format!("_{b:02x}")
            }
        })
        .collect();
    let mut id = format!("dme-{prefix}-r{}-", r.version);
    for b in r.key.bytes() {
        let _ = write!(id, "{b:02x}");
    }
    id
}
/// Apply inverse object transform.
pub fn inverse(t: Transform, p: Point) -> Point {
    let x = (p.x - t.translation.x) / t.scale;
    let y = (p.y - t.translation.y) / t.scale;
    let (s, c) = t.rotation.sin_cos();
    Point::new(x * c + y * s, -x * s + y * c)
}
/// Borrow the ordinary object's transform (bound markers are host-controlled).
pub fn transform(g: &Geometry) -> Option<Transform> {
    match g {
        Geometry::Stroke { transform, .. }
        | Geometry::Path { transform, .. }
        | Geometry::Text { transform, .. }
        | Geometry::Stamp { transform, .. }
        | Geometry::Custom { transform, .. } => Some(*transform),
        _ => None,
    }
}
/// Change an ordinary object's transform, preserving its geometry.
pub fn set_transform(g: &mut Geometry, t: Transform) {
    match g {
        Geometry::Stroke { transform, .. }
        | Geometry::Path { transform, .. }
        | Geometry::Text { transform, .. }
        | Geometry::Stamp { transform, .. }
        | Geometry::Custom { transform, .. } => *transform = t,
        _ => {}
    }
}
/// Deterministic Catmull-Rom controls used for both display and SVG output.
pub fn smooth_nodes(points: &[Point], closed: bool) -> Vec<PathNode> {
    let len = points.len();
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let before = points[if i == 0 {
                if closed {
                    len - 1
                } else {
                    0
                }
            } else {
                i - 1
            }];
            let after = points[if i + 1 == len {
                if closed {
                    0
                } else {
                    i
                }
            } else {
                i + 1
            }];
            let delta = Point::new((after.x - before.x) / 6., (after.y - before.y) / 6.);
            PathNode {
                position: *p,
                incoming: Some(Point::new(p.x - delta.x, p.y - delta.y)),
                outgoing: Some(Point::new(p.x + delta.x, p.y + delta.y)),
            }
        })
        .collect()
}
/// SVG path in world/local Y-up coordinates, with optional cubic controls.
pub fn path_data(nodes: &[PathNode], closed: bool) -> String {
    let mut out = String::new();
    let Some(first) = nodes.first() else {
        return out;
    };
    let _ = write!(out, "M {} {} ", first.position.x, first.position.y);
    for i in 1..nodes.len() + usize::from(closed) {
        let a = &nodes[(i - 1) % nodes.len()];
        let b = &nodes[i % nodes.len()];
        if a.outgoing.is_some() || b.incoming.is_some() {
            let c = a.outgoing.unwrap_or(a.position);
            let d = b.incoming.unwrap_or(b.position);
            let _ = write!(
                out,
                "C {} {} {} {} {} {} ",
                c.x, c.y, d.x, d.y, b.position.x, b.position.y
            );
        } else {
            let _ = write!(out, "L {} {} ", b.position.x, b.position.y);
        }
    }
    if nodes.len() == 1 {
        let _ = write!(out, "l 0.001 0 ");
    }
    if closed {
        out.push('Z');
    }
    out
}
/// Get explicit nodes; smooth paths retain manual handles where supplied.
pub fn path_nodes(nodes: &[PathNode], smooth: bool, closed: bool) -> Vec<PathNode> {
    let automatic = smooth_nodes(
        &nodes.iter().map(|n| n.position).collect::<Vec<_>>(),
        closed,
    );
    nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            if smooth {
                PathNode {
                    position: n.position,
                    incoming: n.incoming.or(automatic[i].incoming),
                    outgoing: n.outgoing.or(automatic[i].outgoing),
                }
            } else {
                n.clone()
            }
        })
        .collect()
}
fn geometry_path(g: &Geometry) -> Option<(Vec<PathNode>, bool)> {
    match g {
        Geometry::Stroke { points, .. } => Some((smooth_nodes(points, false), false)),
        Geometry::Path {
            nodes,
            closed,
            smooth,
            ..
        } => Some((path_nodes(nodes, *smooth, *closed), *closed)),
        _ => None,
    }
}
/// Bound of transformed points; a cubic's control hull conservatively bounds it.
fn bounds(points: impl Iterator<Item = Point>) -> Option<WorldRect> {
    let mut min = Point::new(f64::INFINITY, f64::INFINITY);
    let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in points {
        if !p.is_finite() {
            return None;
        }
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    if !min.is_finite() {
        return None;
    }
    Some(WorldRect {
        min,
        width: (max.x - min.x).max(0.001),
        height: (max.y - min.y).max(0.001),
    })
}
/// Approximate label bounds use a deterministic font metric. Actual text is SVG text.
pub fn object_bounds(o: &MapObject, s: &MapSnapshot, r: &Registry) -> Option<WorldRect> {
    let t = transform(&o.geometry).unwrap_or_default();
    let points = if let Some((nodes, _)) = geometry_path(&o.geometry) {
        nodes
            .into_iter()
            .flat_map(|n| {
                [Some(n.position), n.incoming, n.outgoing]
                    .into_iter()
                    .flatten()
            })
            .collect()
    } else {
        let rect = match &o.geometry {
            Geometry::Text { text, .. } => {
                let size = r.paint(&s.document.style, &o.paint).width;
                WorldRect {
                    min: Point::new(0., -size * 0.25),
                    width: (text
                        .chars()
                        .map(|c| if c.is_ascii() { 0.62 } else { 1.0 })
                        .sum::<f64>()
                        * size)
                        .max(size * 0.5),
                    height: size * 1.25,
                }
            }
            Geometry::Custom { bounds, .. } => *bounds,
            Geometry::Stamp { stamp, .. } => {
                let symbol = r.stamp(stamp).map(|s| s.bounds).unwrap_or(WorldRect {
                    min: Point::new(-14., -14.),
                    width: 28.,
                    height: 28.,
                });
                if let Some(label) = crate::map::node_label(o) {
                    WorldRect {
                        min: Point::new(
                            symbol.min.x.min(label.offset.x),
                            symbol.min.y.min(label.offset.y - 4.),
                        ),
                        width: (symbol.min.x + symbol.width)
                            .max(label.offset.x + label.name.chars().count() as f64 * 14.)
                            - symbol.min.x.min(label.offset.x),
                        height: (symbol.min.y + symbol.height).max(label.offset.y + 16.)
                            - symbol.min.y.min(label.offset.y - 4.),
                    }
                } else {
                    symbol
                }
            }
            Geometry::BoundMarker {
                anchor_id,
                appearance,
            } => {
                let anchor = s.anchors.get(anchor_id)?;
                if crate::map::anchor_status(s, anchor) != crate::map::AnchorStatus::Located {
                    return None;
                }
                let a = anchor.position.as_ref()?;
                let symbol = r
                    .stamp(&appearance.icon)
                    .map(|v| v.bounds)
                    .unwrap_or(WorldRect {
                        min: Point::new(-14., -14.),
                        width: 28.,
                        height: 28.,
                    });
                let left = symbol.min.x.min(appearance.label_offset.x);
                let bottom = symbol.min.y.min(appearance.label_offset.y - 4.);
                return Some(WorldRect {
                    min: Point::new(
                        a.point.x + left * appearance.scale,
                        a.point.y + bottom * appearance.scale,
                    ),
                    width: ((symbol.min.x + symbol.width)
                        .max(appearance.label_offset.x + anchor.name.chars().count() as f64 * 14.)
                        - left)
                        * appearance.scale,
                    height: ((symbol.min.y + symbol.height).max(appearance.label_offset.y + 16.)
                        - bottom)
                        * appearance.scale,
                });
            }
            _ => return None,
        };
        corners(rect).to_vec()
    };
    bounds(points.into_iter().filter_map(|p| t.apply(p).ok()))
}
/// Four corners, bottom-left clockwise.
pub fn corners(r: WorldRect) -> [Point; 4] {
    [
        r.min,
        Point::new(r.min.x + r.width, r.min.y),
        Point::new(r.min.x + r.width, r.min.y + r.height),
        Point::new(r.min.x, r.min.y + r.height),
    ]
}
/// Union of object bounds, excluding hidden layers.
pub fn content_bounds(s: &MapSnapshot, r: &Registry) -> Option<WorldRect> {
    let projected = crate::map::projected_markers(s);
    bounds(
        s.document
            .objects
            .iter()
            .chain(projected.iter())
            .filter(|o| visible(s, o))
            .filter_map(|o| object_bounds(o, s, r))
            .flat_map(corners),
    )
}
/// Layer visibility, independent from its edit lock.
pub fn visible(s: &MapSnapshot, o: &MapObject) -> bool {
    s.document
        .layers
        .iter()
        .any(|l| l.id == o.layer_id && l.visible)
}
/// Hidden/locked objects cannot be selected or edited by pointer tools.
pub fn editable(s: &MapSnapshot, o: &MapObject) -> bool {
    s.document
        .layers
        .iter()
        .any(|l| l.id == o.layer_id && l.visible && !l.locked)
        && (transform(&o.geometry).is_some() || matches!(o.geometry, Geometry::BoundMarker { .. }))
}
fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let u = if dx == 0. && dy == 0. {
        0.
    } else {
        ((p.x - a.x) * dx + (p.y - a.y) * dy) / (dx * dx + dy * dy)
    }
    .clamp(0., 1.);
    p.distance(Point::new(a.x + u * dx, a.y + u * dy))
}
fn flatten(nodes: &[PathNode], closed: bool) -> Vec<Point> {
    let Some(first) = nodes.first() else {
        return vec![];
    };
    let mut points = vec![first.position];
    for i in 1..nodes.len() + usize::from(closed) {
        let a = &nodes[(i - 1) % nodes.len()];
        let b = &nodes[i % nodes.len()];
        if a.outgoing.is_none() && b.incoming.is_none() {
            points.push(b.position);
            continue;
        }
        let c = a.outgoing.unwrap_or(a.position);
        let d = b.incoming.unwrap_or(b.position);
        subdivide(a.position, c, d, b.position, 0, &mut points);
    }
    points
}
fn subdivide(a: Point, b: Point, c: Point, d: Point, depth: u8, out: &mut Vec<Point>) {
    if depth >= 10 || segment_distance(b, a, d).max(segment_distance(c, a, d)) < 0.02 {
        out.push(d);
        return;
    }
    let mid = |a: Point, b: Point| Point::new((a.x + b.x) / 2., (a.y + b.y) / 2.);
    let ab = mid(a, b);
    let bc = mid(b, c);
    let cd = mid(c, d);
    let abc = mid(ab, bc);
    let bcd = mid(bc, cd);
    let center = mid(abc, bcd);
    subdivide(a, ab, abc, center, depth + 1, out);
    subdivide(center, bcd, cd, d, depth + 1, out);
}
fn inside(p: Point, polygon: &[Point]) -> bool {
    let mut hit = false;
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            hit = !hit;
        }
    }
    hit
}
/// Topmost editable object; tolerance is converted from screen pixels to world.
pub fn hit_test(s: &MapSnapshot, r: &Registry, p: Point, scale: f64) -> Option<ObjectId> {
    for layer in s
        .document
        .layers
        .iter()
        .rev()
        .filter(|l| l.visible && !l.locked)
    {
        for o in s
            .document
            .objects
            .iter()
            .rev()
            .filter(|o| o.layer_id == layer.id && editable(s, o))
        {
            let paint = r.paint(&s.document.style, &o.paint);
            let t = transform(&o.geometry).unwrap_or_default();
            let local = inverse(t, p);
            let tol = 6. / scale / t.scale;
            let hit = if let Geometry::Custom {
                object_type,
                payload,
                bounds,
                ..
            } = &o.geometry
            {
                r.object_codec(object_type).map_or_else(
                    || bounds.contains(local),
                    |codec| codec.hit(payload, local, tol),
                )
            } else if let Some((nodes, closed)) = geometry_path(&o.geometry) {
                let points = flatten(&nodes, closed);
                points
                    .windows(2)
                    .any(|p| segment_distance(local, p[0], p[1]) <= tol + paint.width / 2.)
                    || (closed && paint.fill.is_some() && inside(local, &points))
                    || points.len() == 1 && local.distance(points[0]) <= tol + paint.width / 2.
            } else {
                object_bounds(o, s, r).is_some_and(|b| {
                    p.x >= b.min.x - 6. / scale
                        && p.x <= b.min.x + b.width + 6. / scale
                        && p.y >= b.min.y - 6. / scale
                        && p.y <= b.min.y + b.height + 6. / scale
                })
            };
            if hit {
                return Some(o.id.clone());
            }
        }
    }
    None
}
fn transform_attr(t: Transform) -> String {
    format!(
        "translate({} {}) rotate({}) scale({})",
        t.translation.x,
        t.translation.y,
        t.rotation.to_degrees(),
        t.scale
    )
}
/// All symbols are local, self-contained safe vector paths.
pub fn definitions(r: &Registry, prefix: &str) -> String {
    let mut out = String::new();
    let grain = resource_id(
        prefix,
        &ResourceRef {
            key: "scene:grain".into(),
            version: 1,
        },
    );
    let _=write!(out,"<pattern id=\"{grain}\" width=\"13\" height=\"17\" patternUnits=\"userSpaceOnUse\"><circle cx=\"2\" cy=\"3\" r=\"0.5\" fill=\"currentColor\" opacity=\"0.08\"/><circle cx=\"10\" cy=\"12\" r=\"0.6\" fill=\"currentColor\" opacity=\"0.05\"/></pattern>");
    let _ = write!(out, "<pattern id=\"{prefix}-water\" width=\"18\" height=\"12\" patternUnits=\"userSpaceOnUse\"><path d=\"M1 3 Q5 1 9 3 T17 3 M4 9h6\" fill=\"none\" stroke=\"#e4f0e9\" stroke-width=\"0.7\" opacity=\"0.65\"/></pattern><pattern id=\"{prefix}-road\" width=\"11\" height=\"13\" patternUnits=\"userSpaceOnUse\"><path d=\"M1 3l2 1m4 5l2-1\" stroke=\"#6d6248\" stroke-width=\"0.8\" opacity=\"0.45\"/><circle cx=\"6\" cy=\"3\" r=\"0.7\" fill=\"#f4e7c4\"/></pattern>");
    for stamp in r.stamps() {
        let _ = write!(out, "<g id=\"{}\">", resource_id(prefix, &stamp.resource));
        for path in &stamp.paths {
            let _ = write!(
                out,
                "<path d=\"{}\" fill=\"{}\"/>",
                path_data(&path.nodes, path.closed),
                if path.filled { "currentColor" } else { "none" }
            );
        }
        out.push_str("</g>");
    }
    out
}
/// Deterministic paper treatment in world coordinates, excluding editor chrome.
pub fn paper_svg(s: &MapSnapshot, r: &Registry, prefix: &str) -> String {
    let style = r.style(&s.document.style);
    if !style.artwork.paper_grain {
        return String::new();
    }
    let f = s.document.frame;
    let grain = resource_id(
        prefix,
        &ResourceRef {
            key: "scene:grain".into(),
            version: 1,
        },
    );
    format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" color=\"{}\" fill=\"url(#{grain})\" pointer-events=\"none\"/>",f.min.x,f.min.y,f.width,f.height,escape_xml(&style.roles["ink"].color))
}
/// Render one object. Unknown symbols show a recoverable placeholder.
pub fn object_svg(o: &MapObject, s: &MapSnapshot, r: &Registry, prefix: &str) -> String {
    let p = r.paint(&s.document.style, &o.paint);
    let color = escape_xml(&p.color);
    let label_font = if r.style(&s.document.style).artwork.serif_labels {
        "Georgia, Songti SC, serif"
    } else {
        "system-ui, sans-serif"
    };
    let fill = p.fill.as_deref().unwrap_or("none");
    let t = transform(&o.geometry).unwrap_or_default();
    let mut out=format!("<g transform=\"{}\" stroke=\"{}\" fill=\"{}\" color=\"{}\" stroke-width=\"{}\" opacity=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",transform_attr(t),color,escape_xml(fill),escape_xml(p.fill.as_deref().unwrap_or(&p.color)),p.width,p.opacity);
    if let Some((nodes, closed)) = geometry_path(&o.geometry) {
        if closed
            && o.extensions
                .get("map:ribbon")
                .and_then(|v| v.get("version"))
                .and_then(|v| v.as_u64())
                == Some(1)
            && matches!(o.paint.role.as_str(), "river" | "road" | "trail")
        {
            let data = path_data(&nodes, true);
            let texture = if o.paint.role == "river" {
                "water"
            } else {
                "road"
            };
            let _ = write!(out,"<path data-ribbon=\"{texture}\" d=\"{data}\" fill=\"{color}\"/><path d=\"{data}\" fill=\"url(#{prefix}-{texture})\" stroke=\"none\"/>");
            out.push_str("</g>");
            return out;
        }
        let artwork = &r.style(&s.document.style).artwork;
        if closed && o.paint.role == "land" && artwork.coast_width > 0. {
            for (factor, opacity) in [(1., 0.07), (0.65, 0.10), (0.3, 0.16)] {
                let _=write!(out,"<path data-coast=\"true\" d=\"{}\" fill=\"none\" stroke-width=\"{}\" opacity=\"{opacity}\"/>",path_data(&nodes,true),artwork.coast_width*factor);
            }
        }
        let _ = write!(
            out,
            "<path d=\"{}\" {}/>",
            path_data(&nodes, closed),
            if !closed {
                if o.paint.role == "trail" {
                    "fill=\"none\" stroke-dasharray=\"8 6\""
                } else {
                    "fill=\"none\""
                }
            } else {
                ""
            }
        );
    } else {
        match &o.geometry {
            Geometry::Custom {
                object_type,
                payload,
                bounds,
                ..
            } => {
                if let Some(drawing) = r
                    .object_codec(object_type)
                    .and_then(|codec| crate::registry::checked_drawing(codec, payload).ok())
                    .filter(|d| d.bounds == *bounds)
                {
                    for path in drawing.paths {
                        let _ = write!(
                            out,
                            "<path data-custom=\"true\" d=\"{}\" fill=\"{}\"/>",
                            path_data(&path.nodes, path.closed),
                            if path.filled { "currentColor" } else { "none" }
                        );
                    }
                } else {
                    let _=write!(out,"<rect data-missing-object=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke-dasharray=\"3 2\"/><path d=\"M {} {} l {} {} M {} {} l {} {}\"/>",escape_xml(&object_type.key),bounds.min.x,bounds.min.y,bounds.width,bounds.height,bounds.min.x,bounds.min.y,bounds.width,bounds.height,bounds.min.x,bounds.min.y+bounds.height,bounds.width,-bounds.height);
                }
            }
            Geometry::Text { text, .. } => {
                let font = if r.style(&s.document.style).artwork.serif_labels {
                    "Georgia, Songti SC, serif"
                } else {
                    "system-ui, sans-serif"
                };
                let _=write!(out,"<text transform=\"scale(1 -1)\" fill=\"{color}\" stroke=\"none\" font-size=\"{}\" font-family=\"{font}\">{}</text>",p.width,escape_xml(text));
            }
            Geometry::Stamp { stamp, .. } => {
                if r.stamp(stamp).is_some() {
                    let _ = write!(out, "<use href=\"#{}\"/>", resource_id(prefix, stamp));
                } else {
                    out.push_str("<rect x=\"-14\" y=\"-14\" width=\"28\" height=\"28\" fill=\"none\" stroke-dasharray=\"3 2\"/><path d=\"M -10 -10 L 10 10 M -10 10 L 10 -10\"/>");
                }
            }
            Geometry::BoundMarker {
                anchor_id,
                appearance,
            } => {
                if let Some(a) = s.anchors.get(anchor_id).filter(|a| {
                    crate::map::anchor_status(s, a) == crate::map::AnchorStatus::Located
                }) {
                    if let Some(pos) = a.position.as_ref().filter(|p| p.canvas == s.document.id) {
                        let _ = write!(
                            out,
                            "<g transform=\"translate({} {}) scale({})\">",
                            pos.point.x, pos.point.y, appearance.scale
                        );
                        if r.stamp(&appearance.icon).is_some() {
                            let _ = write!(
                                out,
                                "<use href=\"#{}\"/>",
                                resource_id(prefix, &appearance.icon)
                            );
                        } else {
                            out.push_str(
                                "<circle r=\"10\" fill=\"none\" stroke-dasharray=\"3 2\"/>",
                            );
                        }
                        let _=write!(out,"<text x=\"{}\" y=\"{}\" transform=\"scale(1 -1)\" stroke=\"none\" fill=\"{color}\" font-family=\"{label_font}\" font-size=\"14\">{}</text></g>",appearance.label_offset.x,-appearance.label_offset.y,escape_xml(&a.name));
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(label) = crate::map::node_label(o) {
        let _=write!(out,"<text x=\"{}\" y=\"{}\" transform=\"scale(1 -1)\" stroke=\"none\" fill=\"{color}\" font-family=\"{label_font}\" font-size=\"14\">{}</text>",label.offset.x,-label.offset.y,escape_xml(&label.name));
    }
    out.push_str("</g>");
    out
}
/// Cached committed object; rebuilding is necessary only after content changes.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneObject {
    /// Stable key for renderer reconciliation.
    pub id: ObjectId,
    /// Safe local SVG fragment.
    pub svg: String,
}
/// Build bottom-to-top visible object fragments.
pub fn build_scene(s: &MapSnapshot, r: &Registry, prefix: &str) -> Vec<SceneObject> {
    let projected = crate::map::projected_markers(s);
    let mut result = Vec::new();
    for layer in s.document.layers.iter().filter(|l| l.visible) {
        for o in s
            .document
            .objects
            .iter()
            .chain(projected.iter())
            .filter(|o| o.layer_id == layer.id)
        {
            result.push(SceneObject {
                id: o.id.clone(),
                svg: object_svg(o, s, r, prefix),
            });
        }
    }
    result
}
