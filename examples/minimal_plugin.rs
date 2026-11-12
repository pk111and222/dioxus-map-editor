//! A headless, compiled third-party provider; run with no optional features.
use dioxus_map_editor::{
    document::{MapDocument, MapSnapshot},
    editor::EditorTools,
    error::MapResult,
    history::HistoryBudget,
    plugin::{Contribution, Plugin, PluginHost, PluginId, PluginInfo, Slot, PLUGIN_API_VERSION},
    registry::Registry,
    session::MapSession,
};

#[derive(Debug)]
struct Palette;
impl Plugin for Palette {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: PluginId("example:palette".into()),
            name: "My palette".into(),
            version: 1,
            api_version: PLUGIN_API_VERSION,
            dependencies: vec![],
        }
    }
    fn register(&self, registry: &mut Registry) -> MapResult<()> {
        // New resources appear in any host palette reading Registry::stamps().
        let mut symbol = Registry::default()
            .stamp(&dioxus_map_editor::plugin::resource("builtin:pine"))
            .cloned()
            .ok_or_else(|| dioxus_map_editor::error::MapError::Missing("builtin:pine".into()))?;
        symbol.resource = dioxus_map_editor::plugin::resource("example:woodland");
        symbol.name = "Woodland entity".into();
        registry.register_stamp(symbol)?;
        registry.contribute(Contribution {
            key: "example:palette-panel".into(),
            label: "My palette".into(),
            slot: Slot::Properties,
            shortcut: None,
        })
    }
}
fn main() -> MapResult<()> {
    let mut session = MapSession::open(
        MapSnapshot::standalone(MapDocument::new("minimal")),
        None,
        Default::default(),
        HistoryBudget::default(),
    )?;
    let mut host = PluginHost::default();
    host.register(Palette)?;
    host.set_enabled(
        &PluginId("example:palette".into()),
        true,
        &mut session,
        &mut EditorTools::default(),
    )?;
    for entry in host.registry().contributions() {
        println!("{}: {}", entry.key, entry.label);
    }
    Ok(())
}
