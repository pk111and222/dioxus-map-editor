//! Count- and byte-bounded reversible history, owned only by MapSession.
use crate::command::Patch;
use crate::error::MapResult;
use std::collections::VecDeque;

/// Both budgets count undo and redo entries together.
#[derive(Clone, Copy, Debug)]
pub struct HistoryBudget {
    /// Maximum retained user actions.
    pub entries: usize,
    /// Maximum serialized patch bytes (an estimate, not allocator heap accounting).
    pub bytes: usize,
}
impl Default for HistoryBudget {
    fn default() -> Self {
        Self {
            entries: 128,
            bytes: 8 * 1024 * 1024,
        }
    }
}

/// Read-only history diagnostics for UI and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryInfo {
    /// Available undo actions.
    pub undo: usize,
    /// Available redo actions.
    pub redo: usize,
    /// Estimated retained patch bytes.
    pub bytes: usize,
    /// Cumulative evictions, so the host can show a shortened-range notice.
    pub evicted: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub patches: Vec<Patch>,
    pub bytes: usize,
}
impl Entry {
    pub fn new(patches: Vec<Patch>) -> MapResult<Self> {
        let bytes = serde_json::to_vec(&patches)?.len();
        Ok(Self { patches, bytes })
    }
}

pub(crate) struct History {
    pub undo: VecDeque<Entry>,
    pub redo: Vec<Entry>,
    budget: HistoryBudget,
    evicted: u64,
}
impl History {
    pub fn new(budget: HistoryBudget) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            budget,
            evicted: 0,
        }
    }
    pub fn budget(&self) -> HistoryBudget {
        self.budget
    }
    pub fn info(&self) -> HistoryInfo {
        HistoryInfo {
            undo: self.undo.len(),
            redo: self.redo.len(),
            bytes: self
                .undo
                .iter()
                .chain(self.redo.iter())
                .map(|e| e.bytes)
                .sum(),
            evicted: self.evicted,
        }
    }
    pub fn push(&mut self, entry: Entry) {
        self.redo.clear();
        self.undo.push_back(entry);
        while self.info().undo + self.info().redo > self.budget.entries
            || self.info().bytes > self.budget.bytes
        {
            if self.undo.pop_front().is_none() {
                break;
            }
            self.evicted = self.evicted.saturating_add(1);
        }
    }
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}
