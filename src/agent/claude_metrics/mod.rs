//! Bounded, privacy-preserving Claude Code telemetry from one session's transcript.
//!
//! The fields and their sources are the ones `spikes/claude-hud` observed.
//! Snapshots never change the semantic activity authority: the tab's dot,
//! `waiting` included, comes from the status hooks.
//!
//! Claude's token counts are not Codex's: `input` excludes the cache reads and
//! cache writes, which are separate and additive. That is why these types are
//! not `codex_metrics`'s.

pub mod records;
mod reducer;

use jiff::Timestamp;

pub use reducer::Reducer;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample<T> {
    pub at: Timestamp,
    pub value: T,
}

impl<T> Sample<T> {
    /// Pure freshness decision; the consumer decides when an event warrants it.
    pub fn is_stale_at(&self, now: Timestamp, max_age_secs: i64) -> bool {
        now.as_second().saturating_sub(self.at.as_second()) > max_age_secs.max(0)
    }
}

/// One message's `usage`. The four counts are disjoint, so a sum is a total.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Input not read from or written to the prompt cache.
    pub input: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub output: u64,
}

impl Usage {
    /// What the context held when this message was requested.
    pub const fn context(&self) -> u64 {
        self.input
            .saturating_add(self.cache_write)
            .saturating_add(self.cache_read)
    }

    const fn saturating_add(self, other: Self) -> Self {
        Self {
            input: self.input.saturating_add(other.input),
            cache_write: self.cache_write.saturating_add(other.cache_write),
            cache_read: self.cache_read.saturating_add(other.cache_read),
            output: self.output.saturating_add(other.output),
        }
    }

    const fn saturating_sub(self, other: Self) -> Self {
        Self {
            input: self.input.saturating_sub(other.input),
            cache_write: self.cache_write.saturating_sub(other.cache_write),
            cache_read: self.cache_read.saturating_sub(other.cache_read),
            output: self.output.saturating_sub(other.output),
        }
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
    /// Recent tool calls, oldest first, at most `reducer::MAX_TOOLS`.
    pub tools: Vec<Tool>,
    /// False once a result named a call the read never saw start, or a call
    /// was evicted: counts over `tools` are then not exhaustive.
    pub tools_complete: bool,
    /// `Agent` (or older `Task`) calls: subagents started, not parallel tools.
    pub subagent_calls: u32,
    pub background_calls: u32,
}

#[cfg(test)]
mod tests;
