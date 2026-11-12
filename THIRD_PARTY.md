# Resources and third-party notices

All vector symbols in `src/registry.rs`, the compass rose in `src/plugins/objects.rs`, icons in the demo, and the sample atlas in `examples/demo/src/atlas.rs` are original code-native artwork distributed under this repository's MIT license. No external map textures, fonts, icons, or commercial cartography assets are bundled.

Fantasy Map Editor, Nortantis and Hex Cartographer were product references. Their source code and artwork were not copied. References: [Fantasy Map Generator](https://github.com/Azgaar/Fantasy-Map-Generator), [Nortantis](https://github.com/jeheydorn/nortantis). Their licenses do not apply to this project because no upstream code or artwork is bundled.

Runtime dependencies retain their own licenses: Rust serde/serde_json and web-time (MIT OR Apache-2.0), Dioxus (MIT OR Apache-2.0) and its transitive dependencies. Cargo.lock records the development resolution; consult the corresponding upstream package metadata for full notices. Browser/system fonts are referenced by family names and are not redistributed. Playwright and resvg/usvg used for development validation are not bundled into the library.
