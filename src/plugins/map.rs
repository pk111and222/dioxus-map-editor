//! Optional map resource pack, derived from this crate's original vector artwork.
use crate::{error::MapResult, plugin::*, registry::Registry};
/// ID `official:map`, v1; no dependencies on brush/object/file plugins.
#[derive(Debug)]
pub struct MapPackPlugin;
impl Plugin for MapPackPlugin {
    fn info(&self) -> PluginInfo {
        super::info("official:map", "Paper atlas pack")
    }
    fn register(&self, r: &mut Registry) -> MapResult<()> {
        let builtin = Registry::default();
        let mut style = builtin.style(&resource("builtin:atlas")).clone();
        style.resource = resource("official:atlas");
        r.register_style(style)?;
        for (source, key) in [
            ("builtin:pine", "official:pine"),
            ("builtin:ridge", "official:ridge"),
            ("builtin:citadel", "official:citadel"),
        ] {
            if let Some(symbol) = builtin.stamp(&resource(source)) {
                let mut symbol = symbol.clone();
                symbol.resource = resource(key);
                r.register_stamp(symbol)?;
            }
        }
        r.contribute(Contribution {
            key: "official:map-palette".into(),
            label: "Paper atlas palette".into(),
            slot: Slot::Properties,
            shortcut: None,
        })
    }
}

/// Configure a semantic road/terrain preset through ordinary host-owned tool state.
/// This helper does not mutate document content or install UI components.
pub fn apply_preset(preset: crate::map::MapPreset, tools: &mut crate::editor::EditorTools) {
    tools.brush = None;
    preset.apply(tools);
}
/// Configure the optional pack's tree symbol without requiring the brush plugin.
pub fn apply_forest(tools: &mut crate::editor::EditorTools) {
    tools.tool = crate::editor::EditorTool::Scatter;
    tools.brush = None;
    tools.stamp = resource("official:pine");
    tools.paint = crate::document::Paint {
        role: "vegetation".into(),
        ..Default::default()
    };
}
