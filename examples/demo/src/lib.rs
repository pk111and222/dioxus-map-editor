//! Shared bilingual map workspace for the website and native launcher.
mod atlas;
mod persistence;
mod plugin_panel;
pub mod store;
mod studio;
use dioxus::prelude::*;

/// Host-selected interface language; persistent map text is never translated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    /// English UI.
    #[default]
    English,
    /// Simplified Chinese UI.
    Chinese,
}
pub use studio::StudioTextEditor;
impl Language {
    /// Select host-owned UI copy.
    pub fn text(self, en: &'static str, zh: &'static str) -> &'static str {
        match self {
            Self::English => en,
            Self::Chinese => zh,
        }
    }
}
/// Shared public demo. Language changes preserve the mounted document session.
#[component]
pub fn Demo(
    invalidation: u64,
    #[props(default)] language: Language,
    /// Replace the complete text panel with host markup, including external controls.
    #[props(default)]
    text_overlay: Option<Callback<dioxus_map_editor::ui::MapCanvasHandle, Element>>,
    /// Replace text tool properties in the inspector independently from its input overlay.
    #[props(default)]
    text_properties: Option<Callback<dioxus_map_editor::ui::MapCanvasHandle, Element>>,
) -> Element {
    rsx! {
        style { {include_str!("../../../assets/studio.css")} }
        studio::Studio {invalidation,language,text_overlay,text_properties}
    }
}

pub(crate) fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
