//! Embeddable, renderer-independent vector map editor for Dioxus.
//!
//! Documents, atomic editing, history and persistence are usable without UI.
//! Compose the session-backed MapCanvas with host-owned controls, or use MapEditor.
//! Optional compiled plugins share the public command and registry protocols.

pub mod command;
pub mod document;
pub mod editor;
pub mod error;
pub mod export;
pub mod geometry;
pub mod history;
pub mod interaction;
pub mod map;
pub mod plugin;
pub mod plugins;
pub mod region;
pub mod registry;
pub mod scene;
pub mod session;
pub mod storage;

#[cfg(feature = "ui")]
pub mod ui;
