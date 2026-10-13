//! Following one exact agent session file: bounded incremental reads of what it
//! appends, and a watch that wakes them. Agent-neutral: `codex_metrics`
//! supplies which file may be read, how to tell it is the expected session's,
//! and what one line means.
//!
//! No timer, directory scan or filesystem read runs at idle.

#[cfg(target_os = "macos")]
mod native;
pub mod reader;
pub mod watch;

pub use reader::{Follow, MAX_LINE_BYTES, MAX_TAIL_BYTES, Tail};

/// Whether the followed file could be read as the expected session's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Available,
    Missing,
    Unreadable,
    IdentityMismatch,
    InvalidHeader,
    WatchFailed,
}
