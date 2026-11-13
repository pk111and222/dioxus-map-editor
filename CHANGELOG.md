# Changelog

## 0.2.0 — 2026-11-13

Upgrading from 0.1: `MapCanvas { label: "…".into() }` becomes `label: Some("…".into())` (or set `labels: CanvasLabels { canvas, .. }`). Built-in UI text and resource display names are now English; pass `CanvasLabels` / `UiText` to translate. The document schema is unchanged (schema 1); the only new load-time rejection is coordinates beyond `MAX_COORDINATE` (1e12).

- All built-in UI strings default to English; `CanvasLabels` and `UiText` translate the canvas and the optional default panels. Built-in resource names are English.
- Theme tokens for every control and canvas chrome element, an opt-in `data-dme-theme="dark"` preset, `--dme-surface-height`, and `inject_css` to load `EDITOR_CSS` once.
- New `measure` module: undoable map scale, distance, bearing/compass, path lengths and anchor positions for host annotations.
- Fix: `onactivate` no longer runs while session/tools are borrowed; road/river textures stay visible while dragging a node; keyboard input inside host overlays no longer triggers canvas shortcuts; hover is tracked only for tools that preview it.
- Fix: a file watcher reporting the in-flight save no longer locks the session in a conflict; placing an unlocated host anchor respects the first layer's lock; `ReplaceDocument` validates with the session limits.
- Fix: object/scatter ID allocation is linear instead of quadratic (previews on large maps no longer stall); `hit_test` ignores non-positive scales; SVG ID prefixes are sanitized and XML-illegal characters are dropped from exported text; coordinates beyond `MAX_COORDINATE` (1e12) are rejected so smoothing and export stay finite.
- Breaking: `MapCanvas::label` is now `Option<String>` (defaults to `CanvasLabels::canvas`).

## 0.1.0 — 2026-11-12

- Seeded natural region edges, contour snapping and exact shared cubic border arcs in brush/click modes.
- Registry-driven entity palette, atomic symbol replacement, and independent demo text overlay/property slots.

- Reject drawing on locked/hidden layers before previewing; expose typed canvas failures through `onerror` for host notices.
- Clear bilingual layer lock states and explicit enable-drawing action; host-owned paper-style text editor with IME composition guards.
- Drag-to-apply area brushes and tapered textured river/road ribbons; click paths commit each valid node immediately.
- Canvas-scoped Ctrl/Cmd+Z and Shift+Z history, temporary Space pan and Ctrl+wheel zoom; native text shortcuts remain local.
- Linked scatter footprint/symbol resizing, independent symbol size controls, and public history handles.

- Versioned documents, atomic commands, bounded history, host anchor transactions and persistence contracts.
- Dioxus SVG canvas with generic and map tools, replaceable text/overlay UI and public ref handle.
- Trusted compiled plugin host with optional JSON/SVG, foliage brush, compass object and atlas resource providers.
- Lossless missing-provider payloads, safe SVG placeholders/warnings, undoable validated standalone import.

The 0.1 API is experimental. Custom geometry adds a new schema-1 variant; older builds preserve unsupported documents read-only. Renderer selection remains host-owned. Physical IME/pen/cross-platform/performance limits require device validation.
