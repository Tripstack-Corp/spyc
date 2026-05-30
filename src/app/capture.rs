//! Foreground `!` shell capture (`PendingCapture`). The child runs
//! under a PTY so programs that open `/dev/tty` for prompts — sudo,
//! ssh, gpg — see the slave PTY instead of bleeding onto our real
//! terminal. A reader thread feeds bytes into the channel; while the
//! capture is live, typed keys are forwarded to the child via the
//! master writer so the user can answer prompts, and Ctrl+C kills the
//! child outright.
//!
//! Extracted verbatim from `app/mod.rs` (REFACTOR_PLAN Phase 1). The
//! capture lifecycle (spawn, drain, `:fg`/`^Z` round-trip, pager
//! attach) stays in `app` and reads these fields directly, so the
//! struct and its fields are `pub`.

use crate::pane::pty_host::PtyHost;

/// Background capture for a `!` command. The child runs under a PTY
/// (so programs that open `/dev/tty` for prompts — sudo, ssh, gpg —
/// see the slave PTY instead of bleeding onto our real terminal).
/// A reader thread feeds bytes into the channel. While the capture is
/// live, typed keys are forwarded to the child via the master writer
/// so the user can answer prompts. Ctrl+C kills the child outright.
pub struct PendingCapture {
    /// Shared pty kernel (master / writer / child / reader-thread /
    /// event channel / closed / exit_status / last_size). v1.5
    /// Phase 6a unified the pty plumbing across PendingCapture,
    /// BackgroundTask, and Pane.
    pub host: PtyHost,
    /// Accumulated raw bytes for the pager (ANSI included).
    pub buffer: Vec<u8>,
    pub title: String,
    pub cmd_display: String,
    /// When the capture started — for the elapsed timer.
    pub started: std::time::Instant,
    /// True once the reader thread has sent all output.
    pub finished: bool,
    /// Set when this capture was promoted from a previously-backgrounded
    /// task via `:fg`. ^Z will reuse the same id when re-backgrounding so
    /// the user sees `task #3` consistently across the round-trip.
    pub original_id: Option<u32>,
}
