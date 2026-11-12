//! Self-contained SVG using exactly the canvas geometry and paint resolver.
use crate::{
    document::{Limits, MapSnapshot},
    error::MapResult,
    registry::Registry,
    scene,
};
/// Export result includes recoverable missing-resource notices.
#[derive(Clone, Debug)]
pub struct SvgExport {
    /// Standalone XML document, excluding previews, selection and handles.
    pub svg: String,
    /// Missing resources use visible fallback rendering, never silent omission.
    pub warnings: Vec<String>,
}
/// Export the finite frame. The host chooses how to save these bytes.
pub fn export_svg(
    snapshot: &MapSnapshot,
    registry: &Registry,
    limits: &Limits,
) -> MapResult<SvgExport> {
    snapshot.validate(limits)?;
    registry.validate_snapshot_with_limits(snapshot, limits)?;
    let f = snapshot.document.frame;
    let mut warnings = Vec::new();
    if !registry.has_style(&snapshot.document.style) {
        warnings.push(format!("Missing style {}", snapshot.document.style.key));
    }
    for o in &snapshot.document.objects {
        if let crate::document::Geometry::Custom { object_type, .. } = &o.geometry {
            if registry.object_codec(object_type).is_none() {
                warnings.push(format!(
                    "Missing object codec {} v{}; exported placeholder",
                    object_type.key, object_type.version
                ));
            }
        }
        let icon = match &o.geometry {
            crate::document::Geometry::Stamp { stamp, .. } => Some(stamp),
            crate::document::Geometry::BoundMarker { appearance, .. } => Some(&appearance.icon),
            _ => None,
        };
        if let Some(stamp) = icon {
            if registry.stamp(stamp).is_none() {
                warnings.push(format!("Missing stamp {}", stamp.key));
            }
        }
    }
    let scene = scene::build_scene(snapshot, registry, "export")
        .into_iter()
        .map(|o| o.svg)
        .collect::<String>();
    let scene = format!("{scene}{}", scene::paper_svg(snapshot, registry, "export"));
    let bg = &registry.style(&snapshot.document.style).background;
    let svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"{} {} {} {}\"><defs>{}</defs><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/><g transform=\"scale(1 -1)\">{}</g></svg>",f.width,f.height,f.min.x,-f.min.y-f.height,f.width,f.height,scene::definitions(registry,"export"),f.min.x,-f.min.y-f.height,f.width,f.height,scene::escape_xml(bg),scene);
    Ok(SvgExport { svg, warnings })
}
