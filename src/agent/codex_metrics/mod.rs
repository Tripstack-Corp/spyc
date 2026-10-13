//! Bounded, privacy-preserving Codex telemetry. Collection starts only for an
//! explicit consumer; snapshots do not change the semantic activity authority.

mod reducer;
pub mod stream;
pub mod watch;

use std::path::PathBuf;

use jiff::Timestamp;

pub use crate::agent::tail::Availability;
pub use reducer::Reducer;

/// A result belongs to one effective agent incarnation, not merely one cwd.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub pane_id: String,
    pub thread_id: String,
    pub codex_home: PathBuf,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    UsageRecord,
    LegacyTokenCount,
    TurnContext,
    Lifecycle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample<T> {
    pub at: Timestamp,
    pub source: Source,
    pub value: T,
}

impl<T> Sample<T> {
    /// Pure freshness decision; the consumer decides when an event warrants it.
    pub fn is_stale_at(&self, now: Timestamp, max_age_secs: i64) -> bool {
        now.as_second().saturating_sub(self.at.as_second()) > max_age_secs.max(0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: Option<u64>,
    pub cached_input: Option<u64>,
    pub cache_write_input: Option<u64>,
    pub output: Option<u64>,
    pub reasoning_output: Option<u64>,
    pub total: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mode {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub approval: Option<String>,
    pub sandbox: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolStatus {
    Started,
    Completed,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tool {
    pub name: Option<String>,
    pub status: ToolStatus,
    pub at: Timestamp,
    pub elapsed_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitState {
    NotReported,
    UnsupportedWindow,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub request: Option<Sample<Tokens>>,
    pub turn: Option<Sample<Tokens>>,
    pub thread: Option<Sample<Tokens>>,
    /// The legacy family has its own clock and is never added to the new one.
    pub legacy_last: Option<Sample<Tokens>>,
    pub legacy_thread: Option<Sample<Tokens>>,
    pub model_context_window: Option<Sample<u64>>,
    pub mode: Option<Sample<Mode>>,
    pub compaction: Option<Sample<u64>>,
    pub limit_bucket: Option<Sample<String>>,
    /// B1 verified neither current occupancy nor a populated rollout limit window.
    pub context_percent: Option<u8>,
    pub limits: Option<Sample<LimitState>>,
    pub tools: Vec<Tool>,
    /// Missing starts, tail gaps and unknown item kinds prevent an exhaustive count.
    pub tools_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    pub binding: Binding,
    pub availability: Availability,
    pub tail_only: bool,
    pub snapshot: Snapshot,
}

impl Update {
    /// Closed/rebound panes supply no matching binding and cannot accept a result.
    pub fn for_binding(&self, live: Option<&Binding>) -> Option<&Snapshot> {
        (live == Some(&self.binding)).then_some(&self.snapshot)
    }
}

#[cfg(test)]
mod tests;
