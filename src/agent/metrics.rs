//! The HUD metric types more than one agent's reader fills, and the reducer
//! helpers over them. Each agent's metrics module re-exports what its
//! `Snapshot` uses, so a consumer names one module per agent.
//!
//! Codex keeps its own `Sample` and `Tokens`: its samples carry their
//! `Source`, and its `input` includes the cached input, where [`Usage`]'s
//! counts are disjoint.

use jiff::Timestamp;

/// Recent tool calls a snapshot keeps.
pub const MAX_TOOLS: usize = 64;
/// Tools counted by name; later names share [`OTHER_TOOLS`].
pub const MAX_TOOL_NAMES: usize = 24;
pub const OTHER_TOOLS: &str = "other";

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

    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self {
            input: self.input.saturating_add(other.input),
            cache_write: self.cache_write.saturating_add(other.cache_write),
            cache_read: self.cache_read.saturating_add(other.cache_read),
            output: self.output.saturating_add(other.output),
        }
    }

    #[must_use]
    pub const fn saturating_sub(self, other: Self) -> Self {
        Self {
            input: self.input.saturating_sub(other.input),
            cache_write: self.cache_write.saturating_sub(other.cache_write),
            cache_read: self.cache_read.saturating_sub(other.cache_read),
            output: self.output.saturating_sub(other.output),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolStatus {
    Started,
    Completed,
    Failed,
}

/// Calls of one tool since the read began.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCount {
    /// [`OTHER_TOOLS`] once [`MAX_TOOL_NAMES`] are counted.
    pub name: String,
    pub calls: u32,
    pub failed: u32,
}

/// The tally for `name`. Past [`MAX_TOOL_NAMES`] distinct names, new ones
/// share [`OTHER_TOOLS`], so the list stays bounded.
pub fn count_tool<'a>(counts: &'a mut Vec<ToolCount>, name: Option<&str>) -> &'a mut ToolCount {
    let name = name.unwrap_or("tool");
    let index = counts.iter().position(|c| c.name == name).or_else(|| {
        (counts.len() >= MAX_TOOL_NAMES)
            .then(|| counts.iter().position(|c| c.name == OTHER_TOOLS))
            .flatten()
    });
    let index = index.unwrap_or_else(|| {
        let name = if counts.len() >= MAX_TOOL_NAMES {
            OTHER_TOOLS
        } else {
            name
        };
        counts.push(ToolCount {
            name: name.to_owned(),
            calls: 0,
            failed: 0,
        });
        counts.len() - 1
    });
    &mut counts[index]
}

/// Stores `value` unless `slot` already holds a later sample.
pub fn replace<T>(slot: &mut Option<Sample<T>>, at: Timestamp, value: T) {
    if slot.as_ref().is_none_or(|old| at >= old.at) {
        *slot = Some(Sample { at, value });
    }
}
