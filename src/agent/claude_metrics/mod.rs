//! Bounded, privacy-preserving Claude Code telemetry from one session's transcript.
//!
//! The fields and their sources are the ones `spikes/claude-hud` observed.
//! Snapshots never change the semantic activity authority: the tab's dot,
//! `waiting` included, comes from the status hooks.
//!
//! Claude's token counts are not Codex's: `input` excludes the cache reads and
//! cache writes, which are separate and additive. That is why its usage is
//! [`Usage`], from [`crate::agent::metrics`], and not `codex_metrics`'s `Tokens`.

pub mod records;
mod reducer;
pub mod stream;
pub mod watch;

use std::ffi::OsString;
use std::path::PathBuf;

use jiff::Timestamp;

pub use crate::agent::metrics::{
    MAX_TOOL_NAMES, MAX_TOOLS, OTHER_TOOLS, Sample, ToolCount, ToolStatus, Usage,
};
pub use crate::agent::tail::Availability;
pub use reducer::Reducer;

/// Claude Code's configuration directory: `$CLAUDE_CONFIG_DIR`, else
/// `~/.claude`. Only an absolute one names a place to read.
pub fn home(config_dir: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let dir = match config_dir.filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(home.filter(|home| !home.is_empty())?).join(".claude"),
    };
    dir.is_absolute().then_some(dir)
}

/// A result belongs to one agent incarnation: this pane's session, read from
/// this Claude home, not merely a cwd.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub pane_id: String,
    pub session_id: String,
    /// `CLAUDE_CONFIG_DIR`, else `~/.claude`: transcripts live under its
    /// `projects`, and nothing outside it is read.
    pub claude_home: PathBuf,
    pub generation: u64,
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

/// How the session runs; each field is its latest report, `None` until one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mode {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: Option<String>,
}

/// `TodoWrite`'s list, as counts. The `Task*` tools that also keep a plan are
/// not read: no transcript with them has been observed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub pending: u32,
    pub in_progress: u32,
    pub completed: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compaction {
    /// `auto` or `manual`.
    pub trigger: Option<String>,
    /// What the context held when it was compacted.
    pub pre_tokens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tool {
    pub name: Option<String>,
    pub status: ToolStatus,
    pub at: Timestamp,
    /// Started with `run_in_background`, so its result only says it began.
    pub background: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// The latest message's usage: what its request sent and produced.
    pub request: Option<Sample<Usage>>,
    /// What the context held at the latest request ([`Usage::context`]).
    /// Unknown after a compaction until the next request. Never a percentage:
    /// the transcript carries no window size.
    pub context_tokens: Option<Sample<u64>>,
    /// Summed over distinct messages since the read began, so a tail-only read
    /// misses the ones before it. Compaction doesn't reset it.
    pub totals: Usage,
    pub messages: u64,
    pub mode: Option<Sample<Mode>>,
    pub plan: Option<Sample<Plan>>,
    pub compaction: Option<Sample<Compaction>>,
    pub compactions: u32,
    /// Recent tool calls, oldest first, at most [`MAX_TOOLS`].
    pub tools: Vec<Tool>,
    /// False once a result named a call the read never saw start, or a call
    /// was evicted: counts over `tools` are then not exhaustive.
    pub tools_complete: bool,
    /// Every call since the read began, by tool, most called first.
    pub tool_counts: Vec<ToolCount>,
    /// `Agent` (or older `Task`) calls: subagents started, not parallel tools.
    pub subagent_calls: u32,
    pub background_calls: u32,
}

#[cfg(test)]
mod follow_tests;
#[cfg(test)]
mod tests;
