# Plugin integration

Plugins are trusted Rust code compiled into the application. There is no script loader, dynamic library loader or plugin store. Plugin API version is 1; provider, resource, payload and document schema versions are independent.

## Installation

Enable the features you need and explicitly enable providers at runtime. The default feature is `ui`; official providers are opt-in. All providers also work with `default-features = false`.

| Feature | Provider (version 1) | Resources |
| --- | --- | --- |
| `plugin-formats` | `official:formats` | `official:json`, `official:svg` |
| `plugin-brushes` | `official:brushes` | `official:foliage` |
| `plugin-objects` | `official:objects` | `official:compass` |
| `plugin-map` | `official:map` | `official:atlas`, `official:pine`, `official:ridge`, `official:citadel` |

`official-plugins` enables all four Cargo features. Registration alone does not activate a provider:

```rust
use dioxus_map_editor::{
    document::{MapDocument, MapSnapshot}, editor::EditorTools,
    plugin::{PluginHost, PluginId}, plugins::objects::ObjectsPlugin,
    session::MapSession,
};
# fn main() -> dioxus_map_editor::error::MapResult<()> {
let mut session = MapSession::open(
    MapSnapshot::standalone(MapDocument::new("my-map")),
    None, Default::default(), Default::default(),
)?;
let mut tools = EditorTools::default();
let mut host = PluginHost::default();
host.register(ObjectsPlugin)?;
host.set_enabled(&PluginId("official:objects".into()), true, &mut session, &mut tools)?;
// Publish host.registry().clone() to the MapCanvas registry signal.
# Ok(()) }
```

See [minimal_plugin.rs](../examples/minimal_plugin.rs) for an independent implementation and [plugin_panel.rs](../examples/demo/src/plugin_panel.rs) for a host-owned UI. The map pack uses its own `official:` namespace and preserves built-in resource compatibility. Artwork is original; see [THIRD_PARTY.md](../THIRD_PARTY.md).

## Lifecycle and asynchronous work

Use one `PluginHost` per canvas. `set_enabled` rebuilds the registry transactionally, checks duplicate resources, contributions, shortcuts and exact-version dependencies, and rejects cycles. Failure leaves the old registry, document and tools intact. Success cancels the session gesture and clears selection. Disabling a provider never removes saved objects. Also call `MapCanvasHandle::cancel()` to discard host-side text/path/pointer previews, then publish the new registry to the canvas.

`Plugin::attach` and `detach` scope subscriptions/tasks to a session token. Call `host.sync(&session)` after switching or reopening sessions, and `release` on unmount. Drop also releases. Providers must release their own subscriptions and handles in detach. Releasing invalidates old work even if the same session is later reattached.

Acquire a `ticket` before asynchronous work; submit through `commit`. Checks include provider enable generation, map/session token and revision. Any intervening edit or disable/re-enable rejects old results. Changes still pass through prepare/accept and the host anchor transaction. Plugins cannot obtain a mutable document through public APIs.

## Host UI and handles

`Registry::contributions()` supplies stable keys, fallback labels, slots and optional shortcuts. Slots include Toolbar, Menu, Properties and Overlay. **The host writes the divs, translations and layout.** Contributions do not instantiate components. Use `MapCanvasHandle` to access session/tools/registry/view and commit ordinary commands.

`MapCanvas::overlay` accepts `Callback<MapCanvasHandle, Element>` independently of `text_overlay`. Read-only overlay roots should use `pointer-events:none`. Put interactive property controls outside the SVG and prevent unintended event bubbling. The host routes optional shortcuts and ignores them during native text/IME composition. Escape/Enter/Delete/Backspace are reserved by the canvas. Duplicate extension shortcuts are rejected; normalize modifier ordering consistently, since platform aliases are not inferred.

`Registry::register_action` registers an `Action` that generates commands from a read-only snapshot and JSON parameters. Properties can use actions or helpers such as `edit_compass`, then submit through `handle.execute`. Unknown custom objects support preservation, removal and history operations, but cannot be transformed or rewritten without a matching codec.

## Brushes

Implement `Brush::resource/commands`, register it, then set `tools.tool = EditorTool::Brush` and `tools.brush = Some(resource)`. `BrushInput` supplies the snapshot, revision, bounded world samples, limits and host parameters. Store temporary controls in `EditorTools::brush_parameters`; they do not enter the document until committed. The official foliage brush uses the existing scatter settings.

Preview and commit call the same deterministic generator. Previews never save or edit the document. Pointer release commits one gesture and one undo entry. Cancellation, focus loss and disable discard previews. Prefer `PutObject` commands for shared SVG previews; specialized operations can provide a host overlay. Check budgets in the generator; the core also validates the result. Save generated objects rather than rerandomizing on load. Long strokes currently regenerate the preview on movement; this is not an incremental GPU or pressure-sensitive brush API.

## Entity resources

Trees and mountains are the same stamp/scatter protocol. Register new `StampDefinition` resources; the demo discovers them through `Registry::stamps()`. Use `map::apply_symbol` to pick an entity and `map::replace_symbols` for an undoable replacement of selected stamps. Resource keys/versions remain immutable and namespaced. See [customization](customization.md) for region edges and replaceable text panels.

## Custom objects

`Geometry::Custom` stores `object_type: ResourceRef`, a boxed JSON payload, local bounds and a normal transform. Implement `ObjectCodec::drawing` to validate an exact payload version and return bounded `ObjectDrawing` paths; optionally override local hit testing. Rendering and export share safe vector geometry, without arbitrary SVG, URLs or scripts. Selection, layers, transforms, history and JSON use core behavior.

Validation occurs at registry changes, prepare/accept, gesture commit, undo/redo, external refresh, session switch/reload and SVG export. Generated nodes count against limits; declared bounds must match the drawing. Invalid known payloads prevent provider activation without altering the document.

Missing providers or unsupported payload versions preserve all JSON and render a crossed placeholder using saved bounds. SVG export returns warnings and uses the same placeholder; show these warnings to users. Re-enabling the exact resource version restores rendering. Register a new version explicitly; automatic migration and silent downgrades are not provided. Older builds that cannot recognize the custom schema variant protect the whole document as read-only.

## Formats and IO

`Format` performs no IO. The host chooses files, bounds input bytes, downloads/writes output and displays messages. JSON format exports `MapDocument`; host anchor names/positions are not included, and bindings produce warnings. Business-level persistence continues through `MapStore`.

`ImportCandidate::parse` checks byte budgets, schema, structure and known codecs before presenting a preview. Retain the input string if the user needs to recover unsupported or damaged input. Dropping the candidate cancels. Applying checks the original session stamp and requires no dirty edits, pending save or host anchors. It commits one undoable ReplaceDocument command, preserves the destination map ID and increments revision. Save afterward to persist; importing does not change the storage baseline. The demo asks users to save before importing rather than overwriting drafts silently.

SVG is export-only. Other formats can use `register_format`; arbitrary SVG import, PNG/PDF and remote assets are not included.
