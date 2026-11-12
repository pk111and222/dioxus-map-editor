# Region edges, entities and text UI

All three features use controlled host state. No layout or entity category is required by the core.

## Region edges

```rust
use dioxus_map_editor::{editor::EditorTools, region::EdgeAlignment};
let mut tools = EditorTools::default();
tools.closed = true;
tools.region.roughness = 6.0; // World-space displacement; 0 disables.
tools.region.detail = 12.0;
tools.region.seed = 42;
tools.region.alignment = EdgeAlignment::Shared;
tools.region.snap_distance = 12.0; // World units, independent of zoom.
tools.region.target = None; // Or Some(existing_region_id).
```

Free mode keeps the authored edge. Snap projects nearby vertices onto visible closed paths. Shared additionally copies the shorter contour arc between two consecutive vertices snapped to the same guide. Use click mode to choose the two endpoints precisely, then place the remaining vertices away from the guide. The closing edge can also share a border. The gold circle indicates a contour snap candidate. Select a specific guide when several overlap.

The shared arc retains exact cubic controls and source transforms, including partial segments. Only unshared edges receive natural displacement. Locked visible regions can be guides; hidden regions, route ribbons and the active click draft are excluded. Settings affect new geometry only. Existing objects, exports, undo and save/reopen use the stored paths. Shared geometry is copied at creation: there is no persistent topology link, polygon union, clipping or prevention of self-intersections. Later source edits do not propagate. Choose additional points to follow the longer side of a contour.

`region::region_nodes` is renderer-free; pass the session's `Limits`. `PathDraft::command_with_limits` and `brush_commands` use the same generator and reject expanded outlines exceeding budgets. Natural edges are seeded and never rerandomize on load.

## Trees, mountains and plugin entities

Both trees and mountains are `Geometry::Stamp` placed by `map::scatter`. A plugin registers a `StampDefinition` under its own key/version through `Registry::register_stamp`. Hosts populate a palette from `registry.stamps()`; the demo now does this for both scattering and single placements. See [minimal_plugin.rs](../examples/minimal_plugin.rs) for a small resource plugin.

Use `map::apply_symbol(&mut tools, &registry, resource, role)` to select any registered entity. Spread, density, spacing, scale and rotation remain host-owned. For specialized generation implement `Brush`; for entities with custom parameters implement `ObjectCodec`.

Use `map::replace_symbols(snapshot, &selected_ids, new_resource, &registry)` and submit the commands once through `handle.execute`. This replaces existing stamps atomically and preserves their transforms, labels and extensions. Locked/hidden, missing or mixed non-stamp selections reject the entire replacement. New artwork can have different local bounds. A namespaced resource is immutable at a given version; register a new key/version and explicitly replace selected entities instead of overwriting another plugin's resource. Disabling a plugin retains saved objects and their original resource references.

## Replace the whole text panel

`MapCanvas::text_overlay` and `MapEditor::text_overlay` accept `Callback<MapCanvasHandle, Element>`. Return your own `div`, modal or sidebar; return an empty element to hide the built-in text input and render external controls using the `onready` handle. `MapEditor::properties` replaces the entire default property panel, including text settings. With `MapCanvas`, all properties already belong to the host. The demo also exposes `Demo::text_overlay` for the input panel and `Demo::text_properties` for the inspector's text controls. `StudioTextEditor` is exported as an optional reusable example.

```rust,ignore
MapCanvas {
    session, tools, registry, view,
    onready: move |canvas| handle.set(Some(canvas)),
    text_overlay: move |canvas| rsx! { MyTextPanel { canvas } },
}
```

Read `canvas.text_draft()` reactively, then use `set_text`, `set_composing`, `commit_text`, and `cancel`. The draft's stable `id` keys a native input; use `initial_value`, not a controlled `value` that rewrites the input during IME composition. Forward composition start/end, stop keyboard bubbling, and ignore Enter/Escape while composing. The canvas also refuses `commit_text` during composition. Hosts own focus, translations, CSS, buttons and panel position. Access `canvas.tools()` to customize text size/paint or `canvas.execute` for selected-object changes. No raw HTML strings or core component edits are required.
