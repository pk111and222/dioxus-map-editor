//! Validated JSON loading and renderer-independent persistence contracts.
//!
//! Adapters compare the whole requested resource baseline and atomically replace
//! document plus coordinate patches. Names and unrelated host metadata must be
//! merged from fresh host data; a request never authorizes replacing them.
use crate::command::AnchorPatch;
use crate::document::*;
use crate::error::{MapError, MapResult};
use crate::session::SessionStamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Why a syntactically valid file is preserved read-only instead of edited.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtectionReason {
    /// A newer document schema is not understood.
    FutureSchema(u64),
    /// There is no migration for this older schema.
    MissingMigration(u64),
    /// Unknown top-level fields or geometry must not be silently dropped.
    UnknownStructure(String),
}

/// Original UTF-8 is retained verbatim, including fields unknown to this build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectedDocument {
    /// Original bytes represented as UTF-8.
    pub raw: String,
    /// Extracted identity if present and well formed.
    pub id: Option<MapId>,
    /// Why editing and normal saving are forbidden.
    pub reason: ProtectionReason,
}

/// Loader outcome; protected content cannot be passed to editable MapSession::open.
#[derive(Clone, Debug, PartialEq)]
pub enum LoadedDocument {
    /// Fully decoded and validated supported schema.
    Editable(MapDocument),
    /// Preserve-only representation with no lossy typed projection.
    Protected(ProtectedDocument),
}

/// Host-supplied one-step migration; the loader checks version advancement and
/// the final model. The first schema ships with no invented legacy migration.
pub trait Migration {
    /// Version accepted by this migration.
    fn source_version(&self) -> u32;
    /// Upgrade raw content by exactly one schema version, preserving provider data.
    fn migrate(&self, value: Value) -> MapResult<Value>;
}

/// Parse with no legacy migrations. Checks byte budget before parsing.
pub fn decode_document(raw: &str, limits: &Limits) -> MapResult<LoadedDocument> {
    decode_with_migrations(raw, limits, &[])
}

/// Parse, migrate known older schemas, or preserve unsupported content read-only.
pub fn decode_with_migrations(
    raw: &str,
    limits: &Limits,
    migrations: &[&dyn Migration],
) -> MapResult<LoadedDocument> {
    if raw.len() > limits.max_bytes {
        return Err(MapError::Budget("input bytes".into()));
    }
    let original: Value = serde_json::from_str(raw)?;
    let mut value = original.clone();
    let id = original
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| valid_id(s).is_ok())
        .map(MapId::from);
    let protect = |reason| {
        Ok(LoadedDocument::Protected(ProtectedDocument {
            raw: raw.to_owned(),
            id: id.clone(),
            reason,
        }))
    };
    let Some(mut version) = value.get("schema_version").and_then(Value::as_u64) else {
        return invalid("schema_version must be an unsigned integer");
    };
    if version > SCHEMA_VERSION as u64 {
        return protect(ProtectionReason::FutureSchema(version));
    }
    while version < SCHEMA_VERSION as u64 {
        let Some(migration) = migrations
            .iter()
            .find(|m| m.source_version() as u64 == version)
        else {
            return protect(ProtectionReason::MissingMigration(version));
        };
        value = migration.migrate(value)?;
        let next = value.get("schema_version").and_then(Value::as_u64);
        if next != Some(version + 1) {
            return invalid("migration must advance exactly one version");
        }
        version += 1;
    }
    let document: MapDocument = match serde_json::from_value(value) {
        Ok(document) => document,
        Err(error) => return protect(ProtectionReason::UnknownStructure(error.to_string())),
    };
    document.validate(limits)?;
    Ok(LoadedDocument::Editable(document))
}

/// Validated encoding. No preview, history, viewport or host names are included.
pub fn encode_document(document: &MapDocument, limits: &Limits) -> MapResult<String> {
    document.validate(limits)?;
    let encoded = serde_json::to_string(document)?;
    if encoded.len() > limits.max_bytes {
        return Err(MapError::Budget("encoded document".into()));
    }
    Ok(encoded)
}

/// Persistence-only anchor baseline, deliberately excluding name and metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredAnchor {
    /// Parent baseline; saving cannot reparent a place.
    pub parent_id: Option<String>,
    /// Optional coordinates including Z and map identity.
    pub position: Option<AnchorPosition>,
}

/// One atomic storage resource. A real host may merge this into a larger store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredMap {
    /// Persistent content.
    pub document: MapDocument,
    /// Coordinates captured with content; not redundant copies inside BoundMarker.
    pub anchors: BTreeMap<AnchorId, StoredAnchor>,
}
impl StoredMap {
    /// Extract persistence fields, excluding host names and other metadata.
    pub fn from_snapshot(snapshot: &MapSnapshot) -> Self {
        Self {
            document: snapshot.document.clone(),
            anchors: snapshot
                .anchors
                .iter()
                .map(|(id, a)| {
                    (
                        id.clone(),
                        StoredAnchor {
                            parent_id: a.parent_id.clone(),
                            position: a.position.clone(),
                        },
                    )
                })
                .collect(),
        }
    }
    /// Validate a stored resource without constructing artificial host names.
    pub fn validate(&self, limits: &Limits) -> MapResult<()> {
        self.document.validate(limits)?;
        if self.anchors.len() > limits.max_anchors {
            return Err(MapError::Budget("anchors".into()));
        }
        for (id, anchor) in &self.anchors {
            valid_id(&id.0)?;
            if let Some(parent) = &anchor.parent_id {
                valid_id(parent)?;
            }
            if let Some(position) = &anchor.position {
                validate_position(position)?;
            }
        }
        if serde_json::to_vec(self)?.len() > limits.max_bytes {
            return Err(MapError::Budget("storage envelope".into()));
        }
        Ok(())
    }
    /// Validate and encode the complete atomic resource.
    pub fn encode(&self, limits: &Limits) -> MapResult<String> {
        self.validate(limits)?;
        Ok(serde_json::to_string(self)?)
    }
    /// Validate a bounded storage envelope; unsupported documents are refused
    /// without granting permission to overwrite the underlying original bytes.
    pub fn decode(raw: &str, limits: &Limits) -> MapResult<Self> {
        if raw.len() > limits.max_bytes {
            return Err(MapError::Budget("storage input".into()));
        }
        let value: Self = serde_json::from_str(raw)?;
        value.validate(limits)?;
        Ok(value)
    }
    /// Reconstruct a standalone example snapshot. Hosts should instead supply
    /// their fresh authoritative names/metadata while applying these positions.
    pub fn into_snapshot(self) -> MapSnapshot {
        MapSnapshot {
            document: self.document,
            anchors: self
                .anchors
                .into_iter()
                .map(|(id, a)| {
                    (
                        id.clone(),
                        ExternalAnchor {
                            id,
                            name: String::new(),
                            parent_id: a.parent_id,
                            position: a.position,
                            metadata: Extensions::new(),
                        },
                    )
                })
                .collect(),
        }
    }
}

/// A single-flight save, created only from committed session content.
#[derive(Clone, Debug)]
pub struct SaveRequest {
    /// Map, session token and revision that produced this snapshot.
    pub stamp: SessionStamp,
    /// Unique sequence within that session; retries get a different number.
    pub request_id: u64,
    /// Exact expected stored resource; None means it must not exist.
    pub baseline: Option<StoredMap>,
    /// Complete proposed resource, for an atomic content + position write.
    pub snapshot: StoredMap,
    /// Field-limited coordinate intents, for host merge and validation.
    pub anchor_patches: Vec<AnchorPatch>,
}

/// Durable adapter acknowledgement. Failures before atomic replacement preserve
/// the old stored value. A flush failure after replacement may have committed;
/// adapters must report it as a failure so the host can reload and reconcile.
#[derive(Clone, Debug)]
pub struct SaveReceipt {
    /// Echo the request identity, never the UI's current revision.
    pub stamp: SessionStamp,
    /// Echo the exact request sequence.
    pub request_id: u64,
    /// Success means comparison, atomic replacement and required flush completed.
    pub result: MapResult<()>,
}
impl SaveRequest {
    /// Echo request identity into a completion without touching current UI state.
    pub fn receipt(&self, result: MapResult<()>) -> SaveReceipt {
        SaveReceipt {
            stamp: self.stamp.clone(),
            request_id: self.request_id,
            result,
        }
    }
}

/// Synchronous adapter contract. Offload blocking IO in the host; no pointermove
/// may call this. Serialize compare-and-replace with all writes to this resource.
/// This does not promise cross-process or multi-tab CAS without adapter support.
pub trait MapStore {
    /// Load the complete current resource, or None if truly absent.
    fn load(&mut self, id: &MapId, limits: &Limits) -> MapResult<Option<StoredMap>>;
    /// Compare the baseline under the adapter's write lock, validate host parent
    /// and coordinate fields, atomically replace/flush, and return success.
    fn save(&mut self, request: &SaveRequest, limits: &Limits) -> MapResult<()>;
}

/// Deterministic store for tests and embedding examples; no renderer or IO.
#[derive(Default)]
pub struct MemoryStore {
    records: BTreeMap<MapId, StoredMap>,
}
impl MemoryStore {
    /// Inject an external resource write for conflict tests.
    pub fn replace(&mut self, value: StoredMap, limits: &Limits) -> MapResult<()> {
        value.validate(limits)?;
        self.records.insert(value.document.id.clone(), value);
        Ok(())
    }
}
impl MapStore for MemoryStore {
    fn load(&mut self, id: &MapId, _: &Limits) -> MapResult<Option<StoredMap>> {
        Ok(self.records.get(id).cloned())
    }
    fn save(&mut self, request: &SaveRequest, limits: &Limits) -> MapResult<()> {
        validate_save_request(request, limits)?;
        if self.records.get(&request.stamp.map_id) != request.baseline.as_ref() {
            return Err(MapError::Conflict("storage baseline changed".into()));
        }
        self.records
            .insert(request.stamp.map_id.clone(), request.snapshot.clone());
        Ok(())
    }
}

/// Shared validation before an adapter acquires its compare-and-write boundary.
pub fn validate_save_request(request: &SaveRequest, limits: &Limits) -> MapResult<()> {
    request.snapshot.validate(limits)?;
    if request.snapshot.document.id != request.stamp.map_id {
        return invalid("save map identity");
    }
    if let Some(baseline) = &request.baseline {
        baseline.validate(limits)?;
        if baseline.document.id != request.stamp.map_id {
            return invalid("baseline map identity");
        }
    }
    Ok(())
}
