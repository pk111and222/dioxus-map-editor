//! Typed core failures. No filesystem paths or host-global events are emitted.
use std::fmt;

/// Validation, editing, lifecycle or adapter error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapError {
    /// Malformed JSON or a structural decoding error.
    Decode(String),
    /// Invalid model, geometry, reference or namespace.
    Invalid(String),
    /// A configured input/document/history budget was exceeded.
    Budget(String),
    /// An ID was already present.
    Duplicate(String),
    /// A referenced entity does not exist.
    Missing(String),
    /// Request belongs to an old session or revision.
    Stale,
    /// Content cannot be edited in this mode.
    ReadOnly,
    /// Session has been closed or is closing.
    Closed,
    /// Current content has not been flushed; draft is retained.
    Unsaved,
    /// An earlier save is still outstanding.
    SaveInFlight,
    /// Content changed outside this session; draft is retained.
    Conflict(String),
    /// Host rejected a mixed anchor transaction; no edit was applied.
    HostRejected(String),
    /// A gesture is already active or its ticket is no longer valid.
    Gesture,
    /// Revision, token or request counter exhausted.
    Exhausted,
    /// Storage adapter could not complete its operation.
    Storage(String),
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(s) => write!(f, "decode: {s}"),
            Self::Invalid(s) => write!(f, "invalid: {s}"),
            Self::Budget(s) => write!(f, "budget: {s}"),
            Self::Duplicate(s) => write!(f, "duplicate: {s}"),
            Self::Missing(s) => write!(f, "missing: {s}"),
            Self::Conflict(s) => write!(f, "conflict: {s}"),
            Self::HostRejected(s) => write!(f, "host rejected: {s}"),
            Self::Storage(s) => write!(f, "storage: {s}"),
            Self::Stale => f.write_str("stale session or revision"),
            Self::ReadOnly => f.write_str("session is read-only"),
            Self::Closed => f.write_str("session is closing or closed"),
            Self::Unsaved => f.write_str("flush required; draft retained"),
            Self::SaveInFlight => f.write_str("save already in flight"),
            Self::Gesture => f.write_str("invalid or overlapping gesture"),
            Self::Exhausted => f.write_str("monotonic counter exhausted"),
        }
    }
}
impl std::error::Error for MapError {}

impl From<serde_json::Error> for MapError {
    fn from(error: serde_json::Error) -> Self {
        Self::Decode(error.to_string())
    }
}

/// Core result alias.
pub type MapResult<T> = Result<T, MapError>;
