# Changelog

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
