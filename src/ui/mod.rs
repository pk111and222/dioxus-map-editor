//! Composable Dioxus SVG editor components. The application chooses its renderer.
mod editor;
pub use editor::{
    CanvasInteractionOptions, CanvasTextDraft, EditorProperties, EditorToolbar, MapCanvas,
    MapCanvasHandle, MapEditor, MapTools, TextEditorLabels, EDITOR_CSS,
};
