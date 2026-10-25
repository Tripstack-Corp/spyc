//! Bounded, privacy-preserving Google Antigravity (agy) telemetry from one session's transcript.
//!
//! Snapshots never change the semantic activity authority: the tab's dot,
//! `waiting` included, comes from the status hooks.
//!
//! agy's token counts consist of `input`, `cache_read`, and `output`.
//! The context held during a turn is `input + cache_read`.

pub mod records;
mod reducer;
pub mod stream;
pub mod watch;

use std::ffi::OsString;
use std::path::PathBuf;

use jiff::Timestamp;

pub use crate::agent::metrics::{MAX_TOOLS, Sample, ToolCount, ToolStatus, Usage};
pub use crate::agent::tail::Availability;
pub use reducer::Reducer;

/// agy's user home directory (`$HOME`): state lives under
/// `.gemini/antigravity-cli/brain/<session_id>/`.
pub fn home(home: Option<OsString>) -> Option<PathBuf> {
    let dir = PathBuf::from(home.filter(|h| !h.is_empty())?);
    dir.is_absolute().then_some(dir)
}

/// A result belongs to one agent incarnation: this pane's session, read from
/// this agy home, not merely a cwd.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub pane_id: String,
    pub session_id: String,
    /// agy's user home directory containing `.gemini/antigravity-cli/brain/<session_id>`.
    pub agy_home: PathBuf,
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

/// How the session runs; latest report of active model name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mode {
    pub model: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Compaction;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tool {
    pub name: Option<String>,
    pub status: ToolStatus,
    pub at: Timestamp,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// The latest message's usage: what its request sent and produced.
    pub request: Option<Sample<Usage>>,
    /// What the context held at the latest request ([`Usage::context`]).
    /// Unknown after a compaction until the next request.
    pub context_tokens: Option<Sample<u64>>,
    /// Summed over distinct planner responses since the read began.
    pub totals: Usage,
    pub messages: u64,
    pub mode: Option<Sample<Mode>>,
    /// The current turn's latest reasoning line, from a planner response's
    /// `thinking`. A new prompt or a compaction clears it.
    pub reasoning: Option<Sample<String>>,
    pub compaction: Option<Sample<Compaction>>,
    pub compactions: u32,
    /// Recent tool calls, oldest first, at most [`MAX_TOOLS`].
    pub tools: Vec<Tool>,
    /// False once an evicted call or an unobserved start prevents exhaustive history.
    pub tools_complete: bool,
    /// Every call since the read began, by tool, most called first.
    pub tool_counts: Vec<ToolCount>,
    /// `invoke_subagent` calls: subagents started.
    pub subagent_calls: u32,
}

#[cfg(test)]
mod tests;
