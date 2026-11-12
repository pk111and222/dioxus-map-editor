//! Trusted, compiled Rust extensions. Plugins produce data and commands, never DOM or IO.
use crate::{
    command::Command,
    document::*,
    editor::EditorTools,
    error::{MapError, MapResult},
    geometry::{Point, WorldRect},
    registry::{Registry, SymbolPath},
    session::{AnchorTransaction, CommitOutcome, MapSession, SessionStamp},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    rc::Rc,
};

/// Extension ABI version, independent of document and crate versions.
pub const PLUGIN_API_VERSION: u32 = 1;
/// Stable namespaced provider identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PluginId(pub String);
/// Required provider and exact provider version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    /// Provider key.
    pub id: PluginId,
    /// Exact supported version.
    pub version: u32,
}
/// Plugin identity and compatibility contract; plugins start disabled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginInfo {
    /// Stable identity, never translated.
    pub id: PluginId,
    /// Host-visible fallback name; hosts may localize by ID.
    pub name: String,
    /// Provider version.
    pub version: u32,
    /// Required extension ABI.
    pub api_version: u32,
    /// Explicit dependencies, checked before activation.
    pub dependencies: Vec<Dependency>,
}
/// UI contribution location. The host owns the actual Dioxus elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// Tool or brush selector.
    Toolbar,
    /// Menu/action button.
    Menu,
    /// Selection/tool properties.
    Properties,
    /// Passive overlay: hosts must use pointer-events:none on its root.
    Overlay,
}
/// Stable UI declaration; no hard-coded component or global shortcut listener.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contribution {
    /// Namespaced action/panel identity.
    pub key: String,
    /// Fallback label, replaceable by host translations.
    pub label: String,
    /// Host placement.
    pub slot: Slot,
    /// Optional normalized shortcut; duplicates reject activation.
    pub shortcut: Option<String>,
}
/// Safe object rendering, reused by the canvas and SVG exporter.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectDrawing {
    /// Exact local bounds used for fallback selection.
    pub bounds: WorldRect,
    /// Bounded code-native paths, no arbitrary markup.
    pub paths: Vec<SymbolPath>,
}
/// Full custom-object payload protocol. Transform and history are supplied by core.
pub trait ObjectCodec: Debug {
    /// Exact supported type and payload version.
    fn resource(&self) -> ResourceRef;
    /// Validate payload and return bounded vector geometry for display/export.
    fn drawing(&self, payload: &serde_json::Value) -> MapResult<ObjectDrawing>;
    /// Hit test in local coordinates; override for nonrectangular shapes.
    fn hit(&self, payload: &serde_json::Value, point: Point, tolerance: f64) -> bool {
        self.drawing(payload).is_ok_and(|d| {
            point.x >= d.bounds.min.x - tolerance
                && point.x <= d.bounds.min.x + d.bounds.width + tolerance
                && point.y >= d.bounds.min.y - tolerance
                && point.y <= d.bounds.min.y + d.bounds.height + tolerance
        })
    }
}
/// Read-only sampled brush input. Preview and commit call exactly the same generator.
pub struct BrushInput<'a> {
    /// Current authoritative content.
    pub snapshot: &'a MapSnapshot,
    /// Current revision for deterministic IDs.
    pub revision: u64,
    /// Host-controlled brush parameters, paint and target layer.
    pub tools: &'a EditorTools,
    /// World samples; never persisted until commit.
    pub points: &'a [Point],
    /// Core budgets.
    pub limits: &'a Limits,
}
/// Command-producing freehand extension, using the existing pointer sampler.
pub trait Brush: Debug {
    /// Stable versioned brush key.
    fn resource(&self) -> ResourceRef;
    /// Generate preview or final commands without changing content or performing IO.
    fn commands(&self, input: BrushInput<'_>) -> MapResult<Vec<Command>>;
}
/// Data returned to the host for its own file picker/download implementation.
#[derive(Clone, Debug)]
pub struct ExportData {
    /// MIME type supplied by the handler.
    pub media_type: String,
    /// Suggested filename suffix.
    pub extension: String,
    /// Encoded UTF-8 content.
    pub text: String,
    /// Recoverable resource notices.
    pub warnings: Vec<String>,
}
/// Optional read/write format handler; unsupported directions return explicit errors.
pub trait Format: Debug {
    /// Stable versioned format identity.
    fn resource(&self) -> ResourceRef;
    /// Decode without changing a session. Hosts review before committing replacement.
    fn decode(&self, _raw: &str, _limits: &Limits) -> MapResult<MapDocument> {
        Err(MapError::Invalid("format does not support import".into()))
    }
    /// Encode one snapshot, without writing files or declaring save success.
    fn encode(&self, _s: &MapSnapshot, _r: &Registry, _limits: &Limits) -> MapResult<ExportData> {
        Err(MapError::Invalid("format does not support export".into()))
    }
}
/// Menu/property action, returning an atomic command batch.
pub trait Action: Debug {
    /// Namespaced action key.
    fn key(&self) -> &str;
    /// Interpret host-supplied parameters against a read-only snapshot.
    fn commands(
        &self,
        snapshot: &MapSnapshot,
        parameters: &serde_json::Value,
    ) -> MapResult<Vec<Command>>;
}
/// Compiled provider; registration must only build the supplied candidate registry.
pub trait Plugin: Debug {
    /// Stable metadata.
    fn info(&self) -> PluginInfo;
    /// Register capabilities atomically. No global state or IO here.
    fn register(&self, registry: &mut Registry) -> MapResult<()>;
    /// Attach subscriptions/tasks for this session token. No mutable document access.
    fn attach(&self, _session: &SessionStamp) {}
    /// Release subscriptions/tasks for the old session token, including on drop.
    fn detach(&self, _session: &SessionStamp) {}
}
/// Ticket for plugin async work. Disable/re-enable and session switching invalidate it.
#[derive(Clone, Debug)]
pub struct PluginTicket {
    provider: PluginId,
    generation: u64,
    stamp: SessionStamp,
}
/// One host per canvas/session; separate instances never share activation state.
#[derive(Debug)]
pub struct PluginHost {
    base: Registry,
    registry: Registry,
    providers: BTreeMap<PluginId, Rc<dyn Plugin>>,
    enabled: BTreeSet<PluginId>,
    generation: u64,
    attached: Option<SessionStamp>,
}
impl Default for PluginHost {
    fn default() -> Self {
        Self::new(Registry::default())
    }
}
impl PluginHost {
    /// Start with a host-owned base resource collection, no enabled plugins.
    pub fn new(base: Registry) -> Self {
        Self {
            registry: base.clone(),
            base,
            providers: BTreeMap::new(),
            enabled: BTreeSet::new(),
            generation: 0,
            attached: None,
        }
    }
    /// Register a disabled provider; reject duplicate IDs or incompatible API versions.
    pub fn register(&mut self, plugin: impl Plugin + 'static) -> MapResult<()> {
        let info = plugin.info();
        validate_key(&info.id.0)?;
        if info.version == 0 || info.api_version != PLUGIN_API_VERSION {
            return Err(MapError::Invalid("incompatible plugin version".into()));
        }
        if self.providers.contains_key(&info.id) {
            return Err(MapError::Duplicate(info.id.0));
        }
        self.providers.insert(info.id, Rc::new(plugin));
        Ok(())
    }
    /// Registered provider metadata paired with activation state.
    pub fn plugins(&self) -> Vec<(PluginInfo, bool)> {
        self.providers
            .iter()
            .map(|(id, p)| (p.info(), self.enabled.contains(id)))
            .collect()
    }
    /// Current immutable catalogue. Clone it into a MapCanvas registry signal.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
    /// Activate/deactivate atomically; failed dependencies/conflicts leave state intact.
    /// Cancels active content preview and clears stale UI tool/selection on success.
    pub fn set_enabled(
        &mut self,
        id: &PluginId,
        enabled: bool,
        session: &mut MapSession,
        tools: &mut EditorTools,
    ) -> MapResult<()> {
        if !self.providers.contains_key(id) {
            return Err(MapError::Missing(id.0.clone()));
        }
        if self.enabled.contains(id) == enabled {
            return Ok(());
        }
        let mut next = self.enabled.clone();
        if enabled {
            next.insert(id.clone());
        } else {
            next.remove(id);
        }
        for key in &next {
            for dependency in &self.providers[key].info().dependencies {
                if !next.contains(&dependency.id)
                    || self
                        .providers
                        .get(&dependency.id)
                        .is_none_or(|p| p.info().version != dependency.version)
                {
                    return Err(MapError::Missing(format!(
                        "plugin dependency {} v{}",
                        dependency.id.0, dependency.version
                    )));
                }
            }
        }
        let order = self.registration_order(&next)?;
        let mut registry = self.base.clone();
        for key in order {
            self.providers[&key].register(&mut registry)?;
        }
        registry.validate_snapshot_with_limits(session.snapshot(), &session.limits())?;
        let generation = self.generation.checked_add(1).ok_or(MapError::Exhausted)?;
        self.sync(session);
        let stamp = session.stamp();
        if !enabled {
            self.providers[id].detach(&stamp);
        }
        session.cancel_active_gesture();
        tools.tool = crate::editor::EditorTool::Select;
        tools.brush = None;
        tools.selection.clear();
        session.set_registry(registry.clone())?;
        self.registry = registry;
        self.enabled = next;
        self.generation = generation;
        if enabled {
            self.providers[id].attach(&stamp);
        }
        Ok(())
    }
    fn registration_order(&self, enabled: &BTreeSet<PluginId>) -> MapResult<Vec<PluginId>> {
        let mut pending = enabled.clone();
        let mut order = Vec::new();
        while !pending.is_empty() {
            let ready = pending
                .iter()
                .find(|id| {
                    self.providers[*id]
                        .info()
                        .dependencies
                        .iter()
                        .all(|dependency| !pending.contains(&dependency.id))
                })
                .cloned();
            let Some(id) = ready else {
                return Err(MapError::Invalid("cyclic plugin dependencies".into()));
            };
            pending.remove(&id);
            order.push(id);
        }
        Ok(order)
    }
    /// Synchronize lifecycle after a successful session switch/reload. Old tasks expire.
    pub fn sync(&mut self, session: &MapSession) {
        if session.mode() == crate::session::SessionMode::Closed {
            self.release();
            return;
        }
        let stamp = session.stamp();
        if self
            .attached
            .as_ref()
            .is_some_and(|s| s.token == stamp.token)
        {
            return;
        }
        if let Some(old) = self.attached.take() {
            for key in &self.enabled {
                self.providers[key].detach(&old);
            }
        }
        for key in &self.enabled {
            self.providers[key].attach(&stamp);
        }
        self.attached = Some(stamp);
    }
    /// Release this host's session-scoped resources without deleting map content.
    /// Call on host unmount/close; Drop is the final safety net.
    pub fn release(&mut self) {
        self.generation = self.generation.saturating_add(1);
        if let Some(stamp) = self.attached.take() {
            for key in &self.enabled {
                self.providers[key].detach(&stamp);
            }
        }
    }
    /// Capture origin before launching async host work.
    pub fn ticket(&self, id: &PluginId, session: &MapSession) -> MapResult<PluginTicket> {
        if self.generation == u64::MAX {
            return Err(MapError::Exhausted);
        }
        if !self
            .attached
            .as_ref()
            .is_some_and(|stamp| stamp.token == session.stamp().token)
        {
            return Err(MapError::Stale);
        }
        if !self.enabled.contains(id) {
            return Err(MapError::Missing(id.0.clone()));
        }
        Ok(PluginTicket {
            provider: id.clone(),
            generation: self.generation,
            stamp: session.stamp(),
        })
    }
    /// Commit an async plugin result through the same host transaction and history.
    pub fn commit(
        &self,
        ticket: &PluginTicket,
        session: &mut MapSession,
        commands: Vec<Command>,
        host: &mut impl AnchorTransaction,
    ) -> MapResult<CommitOutcome> {
        if !self
            .attached
            .as_ref()
            .is_some_and(|stamp| stamp.token == session.stamp().token)
            || ticket.generation != self.generation
            || !self.enabled.contains(&ticket.provider)
        {
            return Err(MapError::Stale);
        }
        let edit = session.prepare(&ticket.stamp, commands)?;
        session.accept(edit, host)
    }
}
impl Drop for PluginHost {
    fn drop(&mut self) {
        if let Some(stamp) = &self.attached {
            for key in &self.enabled {
                self.providers[key].detach(stamp);
            }
        }
    }
}
pub(crate) fn validate_key(key: &str) -> MapResult<()> {
    crate::document::valid_id(key)?;
    if !key
        .split_once(':')
        .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty())
    {
        return Err(MapError::Invalid("namespaced plugin key required".into()));
    }
    Ok(())
}
/// Exact resource identity helper for compiled definitions.
pub fn resource(key: &str) -> ResourceRef {
    ResourceRef {
        key: key.into(),
        version: 1,
    }
}
