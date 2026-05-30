//! State for an active `:grep` session. The worker thread runs the
//! content searcher and pushes batches of matches through `rx`; the
//! main tick loop (`App::drain_grep_session`) drains them and appends
//! to the pager view whose `grep_id` matches `id`. When the matching
//! pager is closed or replaced (`bprev`/`bnext`/Esc/etc.), the session
//! is dropped and the worker exits on its next send.
//!
//! Extracted verbatim from `app/mod.rs` (REFACTOR_PLAN Phase 1). Pure
//! data struct (the drain is App-coupled and stays in `app`); fields
//! are `pub` because it's built via a struct literal and read back in
//! the drain + title-render paths.

use std::path::PathBuf;

pub struct GrepSession {
    /// Unique session id; pasted onto the pager view's `grep_id` so
    /// stale workers can't bleed into a fresh search.
    pub id: u32,
    /// Receiver for streaming match batches from the worker.
    pub rx: std::sync::mpsc::Receiver<Vec<crate::fs::grep::GrepMatch>>,
    /// Total matches forwarded so far. Drives the title's progress
    /// suffix and the cap-hit warning.
    pub count: usize,
    /// True once the worker disconnected (walk complete or cap hit).
    /// The pager flips `streaming` off and the title shows the final
    /// count instead of "scanning…".
    pub complete: bool,
    /// Cap-hit flag — set when `count` reaches `MAX_MATCHES` so the
    /// final title can warn the user that results were truncated.
    pub capped: bool,
    /// Pattern echoed in the title.
    pub pattern: String,
    /// Display root (project home or listing dir) for context in the
    /// title.
    pub root: PathBuf,
}
