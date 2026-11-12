# Contributing

This project is fully AI-generated and has undergone human regression testing. AI-generated MR/PR contributions are accepted without AI disclosure labels. If you object to AI-generated software, please do not use it.

Use Rust 1.85+, Dioxus CLI 0.7.10 and Python 3.9+. Run:

```sh
cargo fmt --all --check
cargo test --locked -p dioxus-map-editor --all-features
cargo test --locked -p dioxus-map-editor --no-default-features
cargo test --locked -p map-editor-demo
cargo clippy --locked -p dioxus-map-editor -p map-editor-demo --all-features --all-targets -- -D warnings
cargo clippy --locked -p map-editor-website --target wasm32-unknown-unknown -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked -p dioxus-map-editor --all-features --no-deps
python3 scripts/test-release.py
scripts/build-pages /dioxus-map-editor/
```

CI also checks each optional plugin, MSRV, Wasm and Desktop builds. Build Web and Desktop separately to avoid merging renderer features. Preview the generated site over HTTP at its configured base path. Check both languages, responsive layout, API tabs, drawing/undo, plugin toggles, export and save/reload. For input changes, manually verify native IME composition, clipping, pointer re-entry and focus recovery. Close only your own test browsers and servers afterward. Do not report unrun platform checks as passed.

Product runtime code is Rust. Test drivers may use JavaScript, but must not enter the application bundle. Keep IO, business data and translations in the host. Every content edit goes through command/session history. Preserve unknown extensions and missing-provider payloads. Add original or license-compatible assets and update THIRD_PARTY.md.

Main pushes deploy Pages after checks. Deliberate root package version changes additionally publish crates.io using CARGO_REGISTRY_TOKEN; update Cargo.lock and CHANGELOG.md together. Unchanged versions skip publication. Manual dispatch redeploys Pages only. Before merging a release, inspect `cargo package --list --allow-dirty` and run `cargo publish --dry-run --allow-dirty --locked --registry crates-io`. Do not run a real local publication as a routine check.
