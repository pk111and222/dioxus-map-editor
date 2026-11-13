//! Composable Dioxus SVG editor components. The application chooses its renderer.
mod editor;
pub use editor::{
    CanvasInteractionOptions, CanvasLabels, CanvasTextDraft, EditorProperties, EditorToolbar,
    MapCanvas, MapCanvasHandle, MapEditor, MapTools, TextEditorLabels, UiText, EDITOR_CSS,
};
