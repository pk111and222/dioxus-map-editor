//! Optional deterministic foliage brush on the public sampled-brush protocol.
use crate::{
    command::Command, document::ResourceRef, error::MapResult, plugin::*, registry::Registry,
};
/// ID `official:brushes`, v1; no provider dependencies.
#[derive(Debug)]
pub struct BrushesPlugin;
impl Plugin for BrushesPlugin {
    fn info(&self) -> PluginInfo {
        super::info("official:brushes", "Foliage brush")
    }
    fn register(&self, r: &mut Registry) -> MapResult<()> {
        r.register_brush(FoliageBrush)?;
        r.contribute(Contribution {
            key: "official:foliage".into(),
            label: "Foliage".into(),
            slot: Slot::Toolbar,
            shortcut: Some("shift+f".into()),
        })?;
        r.contribute(Contribution {
            key: "official:foliage-settings".into(),
            label: "Spacing / density / seed".into(),
            slot: Slot::Properties,
            shortcut: None,
        })
    }
}
/// Emits editable stamps with stable random placement; uses controlled scatter options.
#[derive(Debug)]
pub struct FoliageBrush;
impl Brush for FoliageBrush {
    fn resource(&self) -> ResourceRef {
        resource("official:foliage")
    }
    fn commands(&self, input: BrushInput<'_>) -> MapResult<Vec<Command>> {
        if input.points.is_empty() {
            return Ok(Vec::new());
        }
        crate::map::scatter(
            input.snapshot,
            input.revision,
            input.tools,
            input.points,
            &input.tools.scatter,
            input.limits,
        )
    }
}
