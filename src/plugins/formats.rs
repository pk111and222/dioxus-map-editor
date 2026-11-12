//! Native JSON exchange and standalone SVG. Host owns file dialogs and persistence.
use crate::{
    document::*,
    error::{MapError, MapResult},
    plugin::*,
    registry::Registry,
    session::{MapSession, SessionStamp},
    storage::{decode_document, encode_document, LoadedDocument},
};
/// Official native JSON/SVG formats (ID `official:formats`, v1).
#[derive(Debug)]
pub struct FormatsPlugin;
impl Plugin for FormatsPlugin {
    fn info(&self) -> PluginInfo {
        super::info("official:formats", "Native JSON / SVG")
    }
    fn register(&self, r: &mut Registry) -> MapResult<()> {
        r.register_format(NativeJson)?;
        r.register_format(Svg)?;
        for (key, label) in [
            ("official:import-json", "Import JSON"),
            ("official:export-json", "Export JSON"),
            ("official:export-svg", "Export SVG"),
        ] {
            r.contribute(Contribution {
                key: key.into(),
                label: label.into(),
                slot: Slot::Menu,
                shortcut: None,
            })?;
        }
        Ok(())
    }
}
/// Validated MapDocument interchange; host anchors remain host-owned.
#[derive(Debug)]
pub struct NativeJson;
impl Format for NativeJson {
    fn resource(&self) -> ResourceRef {
        resource("official:json")
    }
    fn decode(&self, raw: &str, limits: &Limits) -> MapResult<MapDocument> {
        match decode_document(raw, limits)? {
            LoadedDocument::Editable(d) => Ok(d),
            LoadedDocument::Protected(p) => Err(MapError::Invalid(format!(
                "protected import: {:?}; original input retained by caller",
                p.reason
            ))),
        }
    }
    fn encode(&self, s: &MapSnapshot, _: &Registry, limits: &Limits) -> MapResult<ExportData> {
        let bound = s
            .document
            .objects
            .iter()
            .filter(|o| matches!(o.geometry, Geometry::BoundMarker { .. }))
            .count();
        Ok(ExportData {
            media_type: "application/json".into(),
            extension: "json".into(),
            text: encode_document(&s.document, limits)?,
            warnings: if bound > 0 {
                vec!["Host anchor coordinates/names are not embedded; bindings require the original host".into()]
            } else {
                Vec::new()
            },
        })
    }
}
/// Export-only safe SVG; importing arbitrary SVG is explicitly unsupported.
#[derive(Debug)]
pub struct Svg;
impl Format for Svg {
    fn resource(&self) -> ResourceRef {
        resource("official:svg")
    }
    fn encode(&self, s: &MapSnapshot, r: &Registry, limits: &Limits) -> MapResult<ExportData> {
        let result = crate::export::export_svg(s, r, limits)?;
        Ok(ExportData {
            media_type: "image/svg+xml".into(),
            extension: "svg".into(),
            text: result.svg,
            warnings: result.warnings,
        })
    }
}
/// Validated replacement candidate, tied to the exact source session revision.
#[derive(Clone, Debug)]
pub struct ImportCandidate {
    stamp: SessionStamp,
    document: MapDocument,
}
impl ImportCandidate {
    /// Decode and validate first, without altering content, draft or history.
    pub fn parse(raw: &str, session: &MapSession, registry: &Registry) -> MapResult<Self> {
        let document = NativeJson.decode(raw, &session.limits())?;
        registry.validate_snapshot_with_limits(
            &MapSnapshot::standalone(document.clone()),
            &session.limits(),
        )?;
        Ok(Self {
            stamp: session.stamp(),
            document,
        })
    }
    /// Review identity/content before confirmation. Dropping the candidate cancels.
    pub fn document(&self) -> &MapDocument {
        &self.document
    }
    /// Replace a clean standalone document as one undoable edit, retaining target ID.
    /// Dirty/in-flight sessions stay intact.
    /// Host-bound imports need a host-specific transaction and cannot overwrite anchors.
    pub fn apply(self, session: &mut MapSession) -> MapResult<()> {
        session.check_stamp(&self.stamp)?;
        if !session.snapshot().anchors.is_empty() {
            return Err(MapError::HostRejected(
                "standalone import cannot replace host anchors".into(),
            ));
        }
        if session.is_dirty() {
            return Err(MapError::Unsaved);
        }
        if session.is_saving() {
            return Err(MapError::SaveInFlight);
        }
        session
            .execute(vec![crate::command::Command::ReplaceDocument(Box::new(
                self.document,
            ))])
            .map(|_| ())
    }
}
