//! Multi-tab management for the lower pane.
//!
//! `PaneTabs` wraps a `Vec<TabEntry>` and an active-tab index, keeping
//! all tab lifecycle logic out of `App`.

use std::path::PathBuf;

use super::Pane;

/// Two-phase scheduling for the `/resume <sid>` keystroke injection
/// that session restore uses to recover a Claude conversation. Each
/// variant carries the time the next write should fire so the App
/// event loop can drain pending sends each tick without per-tab
/// timers.
pub enum PendingResumeSend {
    /// Initial state right after spawn: wait for Claude's banner to
    /// finish rendering before typing anything. When the deadline
    /// passes we write `/resume <sid>` (no Enter) and transition to
    /// [`Self::Enter`].
    Text {
        sid: String,
        after: std::time::Instant,
    },
    /// Text has been written. After a small additional delay we
    /// write `\r` so the prompt actually submits. Splitting the
    /// write avoids the intermittent race where Claude's TUI was
    /// mid-render and dropped the trailing `\r` from a combined
    /// send.
    Enter { after: std::time::Instant },
}

/// Per-tab metadata displayed in the status line.
pub struct TabInfo {
    /// Full command string passed to `Pane::spawn`.
    pub command: String,
    /// Short display name — defaults to first word of command, user can rename.
    pub label: String,
    /// Working directory at spawn time.
    pub cwd: PathBuf,
    /// True when a background tab received output since last viewed.
    pub has_activity: bool,
    /// Set when the tab was spawned by session restore as a `claude
    /// --resume`. On a non-zero exit shortly after spawn we treat the
    /// resume as failed and replace the tab with a fresh spawn of this
    /// fallback command.
    pub restore_fallback: Option<String>,
    /// Set on session restore when we want claude to resume a specific
    /// conversation: spawn a *fresh* `claude` (the `--resume` CLI flag
    /// trips a known regression that crashes at mount), then once
    /// claude has had time to finish its banner, type `/resume <sid>`
    /// followed by Enter — the slash-command path doesn't hit the bug.
    /// Two-phase to avoid an intermittent race where Claude's TUI was
    /// mid-render when our bytes arrived: the chars were absorbed by
    /// the prompt but the trailing `\r` got dropped, leaving the
    /// command sitting unsubmitted. Sending text and Enter as
    /// separate writes a few hundred ms apart lets the prompt settle
    /// between them.
    pub pending_resume_send: Option<PendingResumeSend>,
    /// When the tab's subprocess was launched. Bounds the
    /// restore-fallback window so a real user-driven exit much later
    /// doesn't trigger an automatic respawn.
    pub spawn_at: std::time::Instant,
    /// Wall-clock spawn time, in epoch seconds. Used at session-save
    /// time to disambiguate which `~/.claude/sessions/*.json` record
    /// belongs to *this* pane when multiple Claude tabs share a cwd
    /// — the matching session record's `startedAt` is closest to
    /// this value. `Instant::now()` (above) is monotonic and can't
    /// be compared against wall-clock data, so we record both.
    pub spawn_epoch_secs: u64,
}

impl TabInfo {
    pub fn new(command: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        let command = command.into();
        let label = command
            .split_whitespace()
            .next()
            .unwrap_or("???")
            .to_string();
        let cwd = cwd.into();
        Self {
            command,
            label,
            cwd,
            has_activity: false,
            restore_fallback: None,
            pending_resume_send: None,
            spawn_at: std::time::Instant::now(),
            spawn_epoch_secs: crate::sysinfo::epoch_secs(),
        }
    }
}

/// A single tab: a `Pane` plus its metadata.
pub struct TabEntry {
    pub pane: Pane,
    pub info: TabInfo,
    /// Cached live cwd of the child process, refreshed at most once
    /// per `LIVE_CWD_TTL`. `None` until first refresh succeeds, or if
    /// the platform / process refuses the lookup.
    live_cwd_cache: Option<(std::time::Instant, PathBuf)>,
    /// Stashed `^a-v` scrollback pager. Holds the whole `PagerView`
    /// (scroll position, search state, visual selection, line buffer
    /// snapshot — everything) while the user is on another tab so a
    /// round-trip "scroll back, tab away, tab back" lands the pager
    /// exactly as the user left it. App-level `self.pager` carries
    /// at most one pager at a time; tab-switch swaps this slot in
    /// and out. `None` when the tab has no scrollback view stashed.
    pub stashed_scrollback_pager: Option<crate::ui::pager::PagerView>,
}

/// How long a cached live-cwd lookup is reused before re-polling.
/// Render-path cost on macOS is a fork-exec (~5ms), so cap polling
/// to ~1 Hz.
const LIVE_CWD_TTL: std::time::Duration = std::time::Duration::from_secs(1);

/// Strip the ` [exited <N>]` suffix that [`PaneTabs::mark_exited`]
/// appends to dead-tab labels for display purposes. Returns the
/// label unchanged if no suffix is present.
///
/// Why this exists: the exit-status display is *runtime UI state*,
/// not persistent identity. Session save serializes
/// `TabEntry::info.label`, so without stripping, a tab that exited
/// at any point during the session ends up with a `[exited N]`
/// suffix glued onto its name in the JSON. On `spyc -r` the tab
/// respawns alive but the saved label is reapplied verbatim — the
/// user sees their freshly-running `htop` tagged "exited 0" until
/// they manually rename it (reported: "htop is actually still
/// running - the status is stale from resuming the session").
///
/// Callers apply this at both save and restore boundaries: save
/// strips so new sessions land clean; restore strips defensively
/// so older session files heal automatically the next time they
/// load.
pub fn strip_exit_suffix(label: &str) -> String {
    // mark_exited writes `format!("{} [exited {}]", label, code)`
    // where `code` is either a decimal integer or `"?"`. The
    // marker substring " [exited " is reserved (any user-set label
    // containing it is recovering display behavior they likely
    // didn't intend anyway), and the suffix is always at the *end*
    // of the label.
    if let Some((base, _)) = label.rsplit_once(" [exited ") {
        if label.ends_with(']') {
            return base.to_string();
        }
    }
    label.to_string()
}

impl TabEntry {
    pub const fn new(pane: Pane, info: TabInfo) -> Self {
        Self {
            pane,
            info,
            live_cwd_cache: None,
            stashed_scrollback_pager: None,
        }
    }

    /// Live cwd of the subprocess (TTL-cached). Returns the spawn-time
    /// cwd as a fallback when the lookup is unsupported or fails.
    pub fn live_cwd(&mut self) -> &std::path::Path {
        let now = std::time::Instant::now();
        let stale = self
            .live_cwd_cache
            .as_ref()
            .is_none_or(|(at, _)| now.duration_since(*at) >= LIVE_CWD_TTL);
        if stale {
            if let Some(pid) = self.pane.process_id() {
                if let Some(cwd) = crate::proc_cwd::cwd_for_pid(pid) {
                    self.live_cwd_cache = Some((now, cwd));
                }
            }
        }
        self.live_cwd_cache
            .as_ref()
            .map_or(self.info.cwd.as_path(), |(_, p)| p.as_path())
    }
}

/// Container for multiple pane tabs.
pub struct PaneTabs {
    tabs: Vec<TabEntry>,
    active: usize,
}

#[allow(dead_code)]
impl PaneTabs {
    /// Create a new tab container with one initial tab.
    pub fn new(entry: TabEntry) -> Self {
        Self {
            tabs: vec![entry],
            active: 0,
        }
    }

    pub fn active(&self) -> &Pane {
        &self.tabs[self.active].pane
    }

    pub fn active_mut(&mut self) -> &mut Pane {
        &mut self.tabs[self.active].pane
    }

    pub fn active_info(&self) -> &TabInfo {
        &self.tabs[self.active].info
    }

    pub fn active_info_mut(&mut self) -> &mut TabInfo {
        &mut self.tabs[self.active].info
    }

    /// Direct mutable access to the active `TabEntry` — for callers
    /// that need to touch per-tab state outside the `Pane` and
    /// `TabInfo` projections above (e.g. the scrollback-resume scroll
    /// memory used by `^a-v` ↔ tab-switch).
    pub fn active_entry_mut(&mut self) -> &mut TabEntry {
        &mut self.tabs[self.active]
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub const fn active_index(&self) -> usize {
        self.active
    }

    /// Switch to tab at `idx` (0-indexed). Clamped to valid range.
    /// Clears the activity flag on the newly active tab.
    pub fn switch_to(&mut self, idx: usize) {
        if !self.tabs.is_empty() {
            self.active = idx.min(self.tabs.len() - 1);
            self.tabs[self.active].info.has_activity = false;
        }
    }

    pub fn next(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
            self.tabs[self.active].info.has_activity = false;
        }
    }

    pub fn prev(&mut self) {
        if !self.tabs.is_empty() {
            self.active = if self.active == 0 {
                self.tabs.len() - 1
            } else {
                self.active - 1
            };
            self.tabs[self.active].info.has_activity = false;
        }
    }

    /// Add a new tab and switch to it.
    pub fn push(&mut self, entry: TabEntry) {
        self.tabs.push(entry);
        self.active = self.tabs.len() - 1;
    }

    /// Remove the active tab. Returns `true` if tabs remain, `false` if
    /// the last tab was removed (caller should tear down the pane area).
    pub fn close_active(&mut self) -> bool {
        self.remove_at(self.active)
    }

    /// Drain output from *every* tab so background tabs don't lose data.
    /// Sets `has_activity` on background tabs that received new output.
    pub fn drain_all(&mut self) {
        for (i, entry) in self.tabs.iter_mut().enumerate() {
            let had_bytes = entry.pane.drain_output();
            if had_bytes && i != self.active {
                entry.info.has_activity = true;
            }
        }
    }

    /// Remove all tabs whose subprocess has exited. Returns `true` if
    /// any tabs remain, `false` if the container is now empty.
    pub fn remove_closed(&mut self) -> bool {
        self.tabs.retain(|entry| !entry.pane.is_closed());
        if self.tabs.is_empty() {
            return false;
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        true
    }

    /// Mark exited tabs with their exit code. Returns `true` if any
    /// tab was newly marked (caller should trigger a redraw).
    /// The suffix this appends is recognized by
    /// [`strip_exit_suffix`] so callers serializing tab labels (e.g.
    /// session save) can drop the runtime-only annotation before
    /// writing to disk.
    pub fn mark_exited(&mut self) -> bool {
        let mut changed = false;
        for entry in &mut self.tabs {
            if entry.pane.is_closed() && !entry.info.label.contains("[exited") {
                // Retry exit status harvest if drain_output missed it.
                entry.pane.try_harvest_exit_status();
                let code = entry
                    .pane
                    .exit_status()
                    .map_or_else(|| "?".to_string(), |s| s.exit_code().to_string());
                entry.info.label = format!("{} [exited {}]", entry.info.label, code);
                changed = true;
            }
        }
        changed
    }

    /// Replace the tab at `idx` in place. Active index and the order of
    /// remaining tabs are preserved. No-op if `idx` is out of range.
    pub fn replace_at(&mut self, idx: usize, entry: TabEntry) {
        if idx < self.tabs.len() {
            self.tabs[idx] = entry;
        }
    }

    /// Remove the tab at `idx`. Returns `true` if tabs remain, `false` if
    /// the container is now empty (caller should tear down the pane area).
    /// Active index follows the removed tab when the active tab itself is
    /// removed; otherwise it shifts to keep pointing at the same tab.
    ///
    /// Tears down the removed tab's child tree before dropping it —
    /// SIGTERM the process group, 250ms grace, then SIGKILL. Without
    /// this an `^a x` on a tab running `npm run dev` (or anything
    /// with subprocesses) would orphan the whole tree because
    /// `portable_pty::Child`'s default Drop is a no-op. The Pane's
    /// own `Drop` is a hard SIGKILL safety net, but going through
    /// `shutdown` here gives well-behaved children a chance to
    /// flush their own state first.
    pub fn remove_at(&mut self, idx: usize) -> bool {
        if idx >= self.tabs.len() {
            return !self.tabs.is_empty();
        }
        self.tabs[idx]
            .pane
            .shutdown(std::time::Duration::from_millis(250));
        self.tabs.remove(idx);
        if self.tabs.is_empty() {
            return false;
        }
        if idx < self.active {
            self.active -= 1;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        true
    }

    /// Take the active `TabEntry` out of the container *without*
    /// shutting down its pty. Used by v1.5 Phase 6c demotion
    /// (`:pane-to-task`): the pty keeps running, the entry just
    /// stops being a tab and becomes a `BackgroundTask`. Returns
    /// `None` when there are no tabs.
    ///
    /// The active index is fixed up the same way `remove_at` does
    /// for closed tabs (slide left when removed-idx < active;
    /// clamp to last otherwise).
    pub fn take_active(&mut self) -> Option<TabEntry> {
        if self.tabs.is_empty() {
            return None;
        }
        let idx = self.active;
        let entry = self.tabs.remove(idx);
        if !self.tabs.is_empty() {
            if idx < self.active {
                self.active -= 1;
            } else if self.active >= self.tabs.len() {
                self.active = self.tabs.len() - 1;
            }
        }
        Some(entry)
    }

    /// Slice of all tab entries (for rendering the tab bar).
    pub fn tabs_mut(&mut self) -> &mut [TabEntry] {
        &mut self.tabs
    }

    pub fn tabs(&self) -> &[TabEntry] {
        &self.tabs
    }
}

#[cfg(test)]
mod tests {
    use super::strip_exit_suffix;

    #[test]
    fn strips_numeric_exit_code() {
        assert_eq!(strip_exit_suffix("claude [exited 0]"), "claude");
        assert_eq!(strip_exit_suffix("htop [exited 130]"), "htop");
    }

    #[test]
    fn strips_question_mark_exit() {
        // mark_exited writes "?" when exit_status() is None.
        assert_eq!(strip_exit_suffix("zsh [exited ?]"), "zsh");
    }

    #[test]
    fn passes_through_label_without_suffix() {
        assert_eq!(strip_exit_suffix("claude"), "claude");
        assert_eq!(strip_exit_suffix(""), "");
        assert_eq!(strip_exit_suffix("npm run dev"), "npm run dev");
    }

    #[test]
    fn only_strips_the_trailing_suffix() {
        // A label that happens to contain "[exited" in the middle
        // (weird but plausible if user named it) is unaffected.
        assert_eq!(
            strip_exit_suffix("note about [exited stuff] here"),
            "note about [exited stuff] here"
        );
    }

    #[test]
    fn handles_nested_suffix_idempotently() {
        // Double-call should be a no-op after the first strip.
        let once = strip_exit_suffix("claude [exited 0]");
        let twice = strip_exit_suffix(&once);
        assert_eq!(once, "claude");
        assert_eq!(twice, "claude");
    }

    #[test]
    fn requires_terminating_bracket() {
        // No closing `]` means it wasn't our suffix; leave alone.
        assert_eq!(strip_exit_suffix("claude [exited 0"), "claude [exited 0");
    }
}
