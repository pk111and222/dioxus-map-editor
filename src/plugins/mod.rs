//! Official optional providers, implemented entirely through public extension APIs.
//! No provider is activated automatically. Enable only the Cargo features needed.
#[cfg(feature = "plugin-brushes")]
pub mod brushes;
#[cfg(feature = "plugin-formats")]
pub mod formats;
#[cfg(feature = "plugin-map")]
pub mod map;
#[cfg(feature = "plugin-objects")]
pub mod objects;

/// Standard metadata used by official providers (version 1, extension API 1).
pub fn info(key: &str, name: &str) -> crate::plugin::PluginInfo {
    crate::plugin::PluginInfo {
        id: crate::plugin::PluginId(key.into()),
        name: name.into(),
        version: 1,
        api_version: crate::plugin::PLUGIN_API_VERSION,
        dependencies: Vec::new(),
    }
}
