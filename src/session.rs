//! One authoritative document/history/revision and a single-flight save queue.
use crate::command::{anchor_changes, prepare, AnchorPatch, Command, Patch};
use crate::document::*;
use crate::error::{MapError, MapResult};
use crate::history::{Entry, History, HistoryBudget, HistoryInfo};
use crate::storage::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
#[allow(deprecated)] // fetch_update is available on the declared Rust 1.85 MSRV.
fn token() -> MapResult<u64> {
    NEXT_TOKEN
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| MapError::Exhausted)
}

/// Request identity used for async commands, saves and stale task rejection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionStamp {
    /// Opaque resource identity.
    pub map_id: MapId,
    /// Unique process-local session generation, changes on reload/switch.
    pub token: u64,
    /// Monotonic content revision; undo/redo also advance it.
    pub revision: u64,
}

/// Content access / lifecycle. Closing stops commands before flushing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionMode {
    /// Normal edits accepted.
    Editable,
    /// Host read-only mode; navigation/export outside core remain possible.
    ReadOnly,
    /// Unsupported content retained verbatim; normal saving forbidden.
    Protected,
    /// Accept no edits while the host drains saves.
    Closing,
    /// Finished. Old async results are ignored.
    Closed,
}

/// Observable persistence state, independent from revision and history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveState {
    /// No content changes since opening; absent empty resources are not created.
    Unmodified,
    /// Committed changes awaiting a save.
    Pending,
    /// One request is outstanding; further edits are coalesced after it completes.
    Saving,
    /// Current content equals the acknowledged durable snapshot.
    Saved,
    /// Adapter failed; current draft and history remain intact.
    Failed(MapError),
    /// External content differs; edits/undo are blocked until explicit resolution.
    Conflict(String),
}

/// Outcome of applying one user intent or undo/redo action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitOutcome {
    /// No content delta, no history, no revision or write.
    NoOp,
    /// Exactly one new revision.
    Committed(u64),
}

/// Non-cloneable permission for a future prepared edit.
#[derive(Debug)]
pub struct PreparedEdit {
    stamp: SessionStamp,
    after: MapSnapshot,
    patches: Vec<Patch>,
    anchors: Vec<AnchorPatch>,
    registry: crate::registry::Registry,
}
impl PreparedEdit {
    /// Host reviews/accepts only these field-limited changes.
    pub fn anchor_patches(&self) -> &[AnchorPatch] {
        &self.anchors
    }
    /// Identity to attach to an asynchronous host request.
    pub fn stamp(&self) -> &SessionStamp {
        &self.stamp
    }
}

/// No acceptance is needed when an edit does not touch host positions.
/// Mixed edits require a host transaction; rejecting it changes nothing.
pub trait AnchorTransaction {
    /// Validate all patches and atomically apply them in the host's in-memory
    /// draft. Persist the mixed resource later through MapStore::save.
    /// Returning Err must leave host data unchanged; do not reenter MapSession.
    fn accept(&mut self, patches: &[AnchorPatch]) -> MapResult<()>;
}

/// Default standalone policy rejects all host-coordinate mutations.
pub struct Standalone;
impl AnchorTransaction for Standalone {
    fn accept(&mut self, patches: &[AnchorPatch]) -> MapResult<()> {
        if patches.is_empty() {
            Ok(())
        } else {
            Err(MapError::HostRejected("no host anchor adapter".into()))
        }
    }
}

/// Ticket for begin/update/commit/cancel; preview commands never touch content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GestureTicket {
    stamp: SessionStamp,
    sequence: u64,
}
struct PendingGesture {
    ticket: GestureTicket,
    commands: Vec<Command>,
}

/// The sole owner of mutable content. Consumers receive immutable snapshots;
/// UI preferences/selection/view live elsewhere and never create revisions.
pub struct MapSession {
    snapshot: MapSnapshot,
    limits: Limits,
    history: History,
    token: u64,
    revision: u64,
    saved_revision: u64,
    mode: SessionMode,
    close_from: SessionMode,
    protected: Option<ProtectedDocument>,
    state: SaveState,
    baseline: Option<StoredMap>,
    clean: StoredMap,
    in_flight: Option<SaveRequest>,
    next_request: u64,
    next_gesture: u64,
    gesture: Option<PendingGesture>,
    needs_host_refresh: bool,
    registry: crate::registry::Registry,
}

impl MapSession {
    /// Install the enabled codec catalogue, validating known payloads before replacing it.
    pub fn set_registry(&mut self, registry: crate::registry::Registry) -> MapResult<()> {
        registry.validate_snapshot_with_limits(&self.snapshot, &self.limits)?;
        self.registry = registry;
        Ok(())
    }
    /// Cancel any content preview, including when an extension is disabled.
    pub fn cancel_active_gesture(&mut self) {
        self.gesture = None;
    }
    /// Open validated content. `persisted=None` means the resource did not exist;
    /// the supplied initial document is clean and is not written merely on open.
    /// A baseline must equal the supplied persistent fields, never stale input.
    pub fn open(
        snapshot: MapSnapshot,
        persisted: Option<StoredMap>,
        limits: Limits,
        budget: HistoryBudget,
    ) -> MapResult<Self> {
        snapshot.validate(&limits)?;
        let clean = StoredMap::from_snapshot(&snapshot);
        clean.validate(&limits)?;
        if persisted.as_ref().is_some_and(|p| p != &clean) {
            return Err(MapError::Conflict(
                "opening snapshot differs from baseline".into(),
            ));
        }
        Ok(Self {
            snapshot,
            limits,
            history: History::new(budget),
            token: token()?,
            revision: 0,
            saved_revision: 0,
            mode: SessionMode::Editable,
            close_from: SessionMode::Editable,
            protected: None,
            state: SaveState::Unmodified,
            baseline: persisted,
            clean,
            in_flight: None,
            next_request: 0,
            next_gesture: 0,
            gesture: None,
            needs_host_refresh: false,
            registry: crate::registry::Registry::default(),
        })
    }
    /// Open an unsupported document safely. The placeholder is never saved or
    /// exported as the original; use protected_document() to recover original text.
    pub fn open_protected(
        document: ProtectedDocument,
        limits: Limits,
        budget: HistoryBudget,
    ) -> MapResult<Self> {
        let id = document
            .id
            .clone()
            .unwrap_or_else(|| MapId::from("protected"));
        let mut session = Self::open(
            MapSnapshot::standalone(MapDocument::new(id)),
            None,
            limits,
            budget,
        )?;
        session.mode = SessionMode::Protected;
        session.protected = Some(document);
        Ok(session)
    }
    /// Borrow committed content, never unfinished preview commands.
    pub fn snapshot(&self) -> &MapSnapshot {
        &self.snapshot
    }
    /// Borrow the original unsupported document.
    pub fn protected_document(&self) -> Option<&ProtectedDocument> {
        self.protected.as_ref()
    }
    /// Current request identity.
    pub fn stamp(&self) -> SessionStamp {
        SessionStamp {
            map_id: self.snapshot.document.id.clone(),
            token: self.token,
            revision: self.revision,
        }
    }
    /// Highest revision satisfied by acknowledged durable-equivalent content.
    pub fn saved_revision(&self) -> u64 {
        self.saved_revision
    }
    /// Current access/lifecycle mode.
    pub fn mode(&self) -> SessionMode {
        self.mode
    }
    /// Observable save state.
    pub fn save_state(&self) -> &SaveState {
        &self.state
    }
    /// History counters, estimates and truncation diagnostics.
    pub fn history(&self) -> HistoryInfo {
        self.history.info()
    }
    /// Validation limits used by commands and adapters.
    pub fn limits(&self) -> Limits {
        self.limits
    }
    /// Exact current committed resource differs from the acknowledged snapshot.
    pub fn is_dirty(&self) -> bool {
        StoredMap::from_snapshot(&self.snapshot) != self.clean
    }
    /// True while an adapter completion is outstanding.
    pub fn is_saving(&self) -> bool {
        self.in_flight.is_some()
    }
    /// Check map, token AND revision for async edit results.
    pub fn check_stamp(&self, stamp: &SessionStamp) -> MapResult<()> {
        if &self.stamp() == stamp {
            Ok(())
        } else {
            Err(MapError::Stale)
        }
    }

    fn editable(&self) -> MapResult<()> {
        match self.mode {
            SessionMode::Editable => {}
            SessionMode::ReadOnly | SessionMode::Protected => return Err(MapError::ReadOnly),
            _ => return Err(MapError::Closed),
        }
        if let SaveState::Conflict(reason) = &self.state {
            return Err(MapError::Conflict(reason.clone()));
        }
        Ok(())
    }
    fn check_custom_edits(&self, commands: &[Command]) -> MapResult<()> {
        for command in commands {
            if let Command::ReplaceDocument(document) = command {
                self.registry
                    .validate_snapshot(&MapSnapshot::standalone(*document.clone()))?;
            }
            if let Command::PutObject { object, .. } = command {
                let existing = self
                    .snapshot
                    .document
                    .objects
                    .iter()
                    .find(|o| o.id == object.id);
                let missing = |o: &crate::document::MapObject| matches!(&o.geometry,crate::document::Geometry::Custom{object_type,..} if self.registry.object_codec(object_type).is_none());
                if existing.is_some_and(|o| missing(o) && o != object)
                    || (missing(object) && existing != Some(object))
                {
                    return Err(MapError::Missing(
                        "custom object codec; payload retained".into(),
                    ));
                }
            }
        }
        Ok(())
    }
    /// Prepare a batch on a private copy. No document/history/revision changes.
    pub fn prepare(&self, stamp: &SessionStamp, commands: Vec<Command>) -> MapResult<PreparedEdit> {
        self.editable()?;
        self.check_stamp(stamp)?;
        if self.gesture.is_some() {
            return Err(MapError::Gesture);
        }
        self.check_custom_edits(&commands)?;
        let (after, patches) = prepare(&self.snapshot, commands, &self.limits)?;
        self.registry
            .validate_snapshot_with_limits(&after, &self.limits)?;
        let anchors = anchor_changes(&self.snapshot, &after);
        Ok(PreparedEdit {
            stamp: stamp.clone(),
            registry: self.registry.clone(),
            after,
            patches,
            anchors,
        })
    }
    /// Commit after host acceptance; all fallible core work happens first.
    pub fn accept(
        &mut self,
        mut edit: PreparedEdit,
        host: &mut impl AnchorTransaction,
    ) -> MapResult<CommitOutcome> {
        self.editable()?;
        self.check_stamp(&edit.stamp)?;
        if self.gesture.is_some() {
            return Err(MapError::Gesture);
        }
        if edit.registry != self.registry {
            return Err(MapError::Stale);
        }
        self.registry
            .validate_snapshot_with_limits(&edit.after, &self.limits)?;
        // Metadata refresh does not advance content revision; an async prepared
        // edit must not restore names/metadata captured before that refresh.
        for (id, anchor) in &mut edit.after.anchors {
            if let Some(current) = self.snapshot.anchors.get(id) {
                anchor.name = current.name.clone();
                anchor.metadata = current.metadata.clone();
            }
        }
        if edit.after == self.snapshot {
            return Ok(CommitOutcome::NoOp);
        }
        let revision = self.revision.checked_add(1).ok_or(MapError::Exhausted)?;
        let entry = Entry::new(edit.patches)?;
        if !edit.anchors.is_empty() {
            host.accept(&edit.anchors)?;
        }
        self.snapshot = edit.after;
        self.history.push(entry);
        self.revision = revision;
        self.refresh_state();
        Ok(CommitOutcome::Committed(revision))
    }
    /// Synchronous standalone convenience; anchor edits are rejected by default.
    pub fn execute(&mut self, commands: Vec<Command>) -> MapResult<CommitOutcome> {
        let edit = self.prepare(&self.stamp(), commands)?;
        self.accept(edit, &mut Standalone)
    }
    /// Host-controlled edit shares exactly the same session/history implementation.
    pub fn execute_with_host(
        &mut self,
        commands: Vec<Command>,
        host: &mut impl AnchorTransaction,
    ) -> MapResult<CommitOutcome> {
        let edit = self.prepare(&self.stamp(), commands)?;
        self.accept(edit, host)
    }
    /// Undo one atomic action. Mixed entries require the same host acceptance.
    pub fn undo(&mut self, host: &mut impl AnchorTransaction) -> MapResult<CommitOutcome> {
        self.replay(false, host)
    }
    /// Redo one atomic action; advances revision rather than restoring old numbers.
    pub fn redo(&mut self, host: &mut impl AnchorTransaction) -> MapResult<CommitOutcome> {
        self.replay(true, host)
    }
    fn replay(
        &mut self,
        redo: bool,
        host: &mut impl AnchorTransaction,
    ) -> MapResult<CommitOutcome> {
        self.editable()?;
        if self.gesture.is_some() {
            return Err(MapError::Gesture);
        }
        let entry = if redo {
            self.history.redo.last()
        } else {
            self.history.undo.back()
        };
        let Some(entry) = entry.cloned() else {
            return Ok(CommitOutcome::NoOp);
        };
        let patches = if redo {
            entry.patches.clone()
        } else {
            entry.patches.iter().rev().map(Patch::reversed).collect()
        };
        let mut next = self.snapshot.clone();
        for patch in patches {
            patch.apply(&mut next)?;
        }
        next.validate(&self.limits)?;
        self.registry
            .validate_snapshot_with_limits(&next, &self.limits)?;
        StoredMap::from_snapshot(&next).validate(&self.limits)?;
        let revision = self.revision.checked_add(1).ok_or(MapError::Exhausted)?;
        let anchors = anchor_changes(&self.snapshot, &next);
        if !anchors.is_empty() {
            host.accept(&anchors)?;
        }
        if redo {
            self.history.redo.pop();
            self.history.undo.push_back(entry);
        } else {
            self.history.undo.pop_back();
            self.history.redo.push(entry);
        }
        self.snapshot = next;
        self.revision = revision;
        self.refresh_state();
        Ok(CommitOutcome::Committed(revision))
    }
    /// Start a content gesture; the UI may keep raw high-frequency samples outside
    /// core and publish a command preview only when necessary.
    pub fn begin_gesture(&mut self) -> MapResult<GestureTicket> {
        self.editable()?;
        if self.gesture.is_some() {
            return Err(MapError::Gesture);
        }
        self.next_gesture = self
            .next_gesture
            .checked_add(1)
            .ok_or(MapError::Exhausted)?;
        let ticket = GestureTicket {
            stamp: self.stamp(),
            sequence: self.next_gesture,
        };
        self.gesture = Some(PendingGesture {
            ticket: ticket.clone(),
            commands: Vec::new(),
        });
        Ok(ticket)
    }
    /// Whether the ticket still owns the current editable content gesture.
    pub fn is_gesture_current(&self, ticket: &GestureTicket) -> bool {
        self.mode == SessionMode::Editable
            && self.gesture.as_ref().is_some_and(|g| &g.ticket == ticket)
            && self.check_stamp(&ticket.stamp).is_ok()
            && !matches!(self.state, SaveState::Conflict(_))
    }
    /// Replace preview intent without document cloning, validation, revision or IO.
    pub fn update_gesture(
        &mut self,
        ticket: &GestureTicket,
        commands: Vec<Command>,
    ) -> MapResult<()> {
        self.editable()?;
        self.check_stamp(&ticket.stamp)?;
        if commands.len() > self.limits.max_commands {
            return Err(MapError::Budget("preview commands".into()));
        }
        let gesture = self
            .gesture
            .as_mut()
            .filter(|g| &g.ticket == ticket)
            .ok_or(MapError::Gesture)?;
        gesture.commands = commands;
        Ok(())
    }
    /// Borrow preview commands separately from committed content and save requests.
    pub fn preview(&self) -> &[Command] {
        self.gesture.as_ref().map_or(&[], |g| g.commands.as_slice())
    }
    /// Commit all preview commands as one validated atomic history entry. On
    /// validation/host failure the preview remains available for retry/cancel.
    pub fn commit_gesture(
        &mut self,
        ticket: &GestureTicket,
        host: &mut impl AnchorTransaction,
    ) -> MapResult<CommitOutcome> {
        self.editable()?;
        self.check_stamp(&ticket.stamp)?;
        let commands = self
            .gesture
            .as_ref()
            .filter(|g| &g.ticket == ticket)
            .ok_or(MapError::Gesture)?
            .commands
            .clone();
        self.check_custom_edits(&commands)?;
        let (after, patches) = prepare(&self.snapshot, commands, &self.limits)?;
        self.registry
            .validate_snapshot_with_limits(&after, &self.limits)?;
        let edit = PreparedEdit {
            stamp: self.stamp(),
            registry: self.registry.clone(),
            anchors: anchor_changes(&self.snapshot, &after),
            after,
            patches,
        };
        let pending = self.gesture.take();
        let result = self.accept(edit, host);
        if result.is_err() {
            self.gesture = pending;
        }
        result
    }
    /// Cancel with no history/revision/content change.
    pub fn cancel_gesture(&mut self, ticket: &GestureTicket) -> MapResult<()> {
        if self.gesture.as_ref().is_some_and(|g| &g.ticket == ticket) {
            self.gesture = None;
            Ok(())
        } else {
            Err(MapError::Gesture)
        }
    }
    fn refresh_state(&mut self) {
        if matches!(self.state, SaveState::Conflict(_)) {
            return;
        }
        self.state = if self.in_flight.is_some() {
            SaveState::Saving
        } else if self.is_dirty() {
            SaveState::Pending
        } else {
            self.saved_revision = self.revision;
            SaveState::Saved
        };
    }
    /// Produce at most one committed snapshot. r+1 edits coalesce while r is
    /// outstanding; after completion call this again to drain the latest state.
    pub fn begin_save(&mut self) -> MapResult<Option<SaveRequest>> {
        if matches!(self.mode, SessionMode::Closed) {
            return Err(MapError::Closed);
        }
        if self.protected.is_some() || self.mode == SessionMode::ReadOnly {
            return Err(MapError::ReadOnly);
        }
        if let SaveState::Conflict(reason) = &self.state {
            return Err(MapError::Conflict(reason.clone()));
        }
        if self.in_flight.is_some() {
            return Err(MapError::SaveInFlight);
        }
        if !self.is_dirty() {
            return Ok(None);
        }
        if self.needs_host_refresh {
            return Err(MapError::Conflict(
                "refresh host snapshot after outstanding external change".into(),
            ));
        }
        let snapshot = StoredMap::from_snapshot(&self.snapshot);
        snapshot.validate(&self.limits)?;
        self.next_request = self
            .next_request
            .checked_add(1)
            .ok_or(MapError::Exhausted)?;
        let anchor_patches = snapshot
            .anchors
            .iter()
            .filter_map(|(id, after)| {
                let before = self.clean.anchors.get(id)?;
                (before.position != after.position).then(|| AnchorPatch {
                    id: id.clone(),
                    parent_id: before.parent_id.clone(),
                    before: before.position.clone(),
                    after: after.position.clone(),
                })
            })
            .collect();
        let request = SaveRequest {
            stamp: self.stamp(),
            request_id: self.next_request,
            baseline: self.baseline.clone(),
            snapshot,
            anchor_patches,
        };
        self.in_flight = Some(request.clone());
        self.state = SaveState::Saving;
        Ok(Some(request))
    }
    /// Apply only the matching outstanding completion. A revision mismatch with
    /// *current* content is expected when r+1 was edited while saving r.
    pub fn complete_save(&mut self, receipt: SaveReceipt) -> bool {
        if self.mode == SessionMode::Closed {
            return false;
        }
        let Some(request) = self.in_flight.as_ref() else {
            return false;
        };
        if request.stamp != receipt.stamp || request.request_id != receipt.request_id {
            return false;
        }
        let Some(request) = self.in_flight.take() else {
            return false;
        };
        match receipt.result {
            Ok(()) => {
                self.baseline = Some(request.snapshot.clone());
                self.clean = request.snapshot;
                self.saved_revision = request.stamp.revision;
                if self.needs_host_refresh {
                    self.state =
                        SaveState::Conflict("external change needs fresh host snapshot".into());
                } else if !matches!(self.state, SaveState::Conflict(_)) {
                    self.refresh_state();
                }
            }
            Err(MapError::Conflict(reason)) => {
                self.state = SaveState::Conflict(reason);
                self.gesture = None;
            }
            Err(error) => {
                if !matches!(self.state, SaveState::Conflict(_)) {
                    self.state = SaveState::Failed(error);
                }
            }
        }
        true
    }
    /// Synchronous convenience, suitable for tests; host UIs should offload
    /// blocking adapters and use begin_save/complete_save with captured requests.
    pub fn flush(&mut self, store: &mut impl MapStore) -> MapResult<()> {
        if let Some(request) = self.begin_save()? {
            let result = store.save(&request, &self.limits);
            self.complete_save(request.receipt(result.clone()));
            result?;
        }
        if self.is_dirty() {
            return Err(MapError::Unsaved);
        }
        Ok(())
    }
    /// Stop edits and cancel preview before a close flush. On save failure the
    /// closing session and all committed content/history are retained.
    pub fn begin_close(&mut self) -> MapResult<()> {
        if self.mode == SessionMode::Closed {
            return Err(MapError::Closed);
        }
        if self.mode != SessionMode::Closing {
            self.close_from = self.mode;
            self.mode = SessionMode::Closing;
        }
        self.gesture = None;
        Ok(())
    }
    /// Reopen the same retained draft after a failed/aborted close attempt.
    pub fn abort_close(&mut self) -> MapResult<()> {
        if self.mode != SessionMode::Closing {
            return Err(MapError::Closed);
        }
        self.mode = self.close_from;
        Ok(())
    }
    /// Finish only once all committed edits are durable. No failure discards data.
    pub fn finish_close(&mut self) -> MapResult<()> {
        if self.mode != SessionMode::Closing {
            return Err(MapError::Closed);
        }
        if self.in_flight.is_some() || self.is_dirty() {
            return Err(MapError::Unsaved);
        }
        if let SaveState::Conflict(reason) = &self.state {
            return Err(MapError::Conflict(reason.clone()));
        }
        self.mode = SessionMode::Closed;
        self.history.clear();
        Ok(())
    }
    /// Explicit host-authorized discard. Never called by ordinary close/switch.
    pub fn discard_and_close(&mut self) {
        self.gesture = None;
        self.mode = SessionMode::Closed;
        self.history.clear();
        self.in_flight = None;
    }
    /// Toggle host read-only without changing content. Protected documents cannot
    /// become editable. A dirty session must be flushed before entering read-only.
    pub fn set_read_only(&mut self, value: bool) -> MapResult<()> {
        if !matches!(self.mode, SessionMode::Editable | SessionMode::ReadOnly) {
            return Err(MapError::ReadOnly);
        }
        if value && (self.is_dirty() || self.is_saving()) {
            return Err(MapError::Unsaved);
        }
        self.gesture = None;
        self.mode = if value {
            SessionMode::ReadOnly
        } else {
            SessionMode::Editable
        };
        Ok(())
    }
    /// Switch only after flush. Validate the target first, so invalid target,
    /// dirty source or failed save leaves this draft untouched.
    pub fn switch(&mut self, snapshot: MapSnapshot, persisted: Option<StoredMap>) -> MapResult<()> {
        let mut next = Self::open(snapshot, persisted, self.limits, self.history_budget())?;
        next.set_registry(self.registry.clone())?;
        if self.mode == SessionMode::Closed {
            return Err(MapError::Closed);
        }
        if self.is_dirty() || self.is_saving() {
            return Err(MapError::Unsaved);
        }
        if matches!(self.state, SaveState::Conflict(_)) {
            return Err(MapError::Conflict("resolve before switching".into()));
        }
        *self = next;
        Ok(())
    }
    fn history_budget(&self) -> HistoryBudget {
        self.history.budget()
    }
    /// Refresh externally observed content. Rename/metadata-only changes merge
    /// into snapshots without touching revisions or invalidating undo. Same-map
    /// content/coordinates conflicts freeze edits and preserve the local draft.
    pub fn observe_external(&mut self, stamp: &SessionStamp, fresh: MapSnapshot) -> MapResult<()> {
        self.check_stamp(stamp)?;
        if matches!(self.mode, SessionMode::Closed | SessionMode::Protected) {
            return Err(MapError::ReadOnly);
        }
        fresh.validate(&self.limits)?;
        self.registry
            .validate_snapshot_with_limits(&fresh, &self.limits)?;
        if fresh.document.id != self.snapshot.document.id {
            return Err(MapError::Stale);
        }
        let stored = StoredMap::from_snapshot(&fresh);
        stored.validate(&self.limits)?;
        if stored != self.clean {
            self.gesture = None;
            if self.is_dirty() || self.is_saving() {
                self.needs_host_refresh = self.is_saving();
                self.state = SaveState::Conflict(
                    "document or host coordinates changed externally; draft retained".into(),
                );
                return Err(MapError::Conflict("external resource changed".into()));
            }
            let revision = self.revision.checked_add(1).ok_or(MapError::Exhausted)?;
            self.snapshot = fresh;
            self.clean = stored.clone();
            self.baseline = Some(stored);
            self.history.clear();
            self.revision = revision;
            self.saved_revision = revision;
            self.state = SaveState::Saved;
        } else {
            for (id, anchor) in fresh.anchors {
                if let Some(current) = self.snapshot.anchors.get_mut(&id) {
                    current.name = anchor.name;
                    current.metadata = anchor.metadata;
                }
            }
        }
        Ok(())
    }
    /// Explicitly reload after a conflict, replacing the local draft only when
    /// the caller chose reload. Outstanding saves must settle first. A new token
    /// invalidates every old task and history is reset.
    pub fn reload(&mut self, fresh: MapSnapshot, persisted: Option<StoredMap>) -> MapResult<()> {
        if self.mode == SessionMode::Closed {
            return Err(MapError::Closed);
        }
        if self.is_saving() {
            return Err(MapError::SaveInFlight);
        }
        let mut next = Self::open(fresh, persisted, self.limits, self.history_budget())?;
        next.set_registry(self.registry.clone())?;
        *self = next;
        Ok(())
    }
}
