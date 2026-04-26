//! Multi-tab management for the lower pane.
//!
//! `PaneTabs` wraps a `Vec<TabEntry>` and an active-tab index, keeping
//! all tab lifecycle logic out of `App`.

use std::path::PathBuf;

use super::Pane;

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
    /// Holds `(session_id, spawn_time)`; cleared once sent.
    pub pending_resume_send: Option<(String, std::time::Instant)>,
    /// When the tab's subprocess was launched. Bounds the
    /// restore-fallback window so a real user-driven exit much later
    /// doesn't trigger an automatic respawn.
    pub spawn_at: std::time::Instant,
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
}

/// How long a cached live-cwd lookup is reused before re-polling.
/// Render-path cost on macOS is a fork-exec (~5ms), so cap polling
/// to ~1 Hz.
const LIVE_CWD_TTL: std::time::Duration = std::time::Duration::from_secs(1);

impl TabEntry {
    pub const fn new(pane: Pane, info: TabInfo) -> Self {
        Self {
            pane,
            info,
            live_cwd_cache: None,
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
    pub fn mark_exited(&mut self) -> bool {
        let mut changed = false;
        for entry in &mut self.tabs {
            if entry.pane.is_closed() && !entry.info.label.contains("[exited") {
                // Retry exit status harvest if drain_output missed it.
                if entry.pane.exit_status.is_none() {
                    if let Ok(Some(status)) = entry.pane.child.try_wait() {
                        entry.pane.exit_status = Some(status);
                    }
                }
                let code = entry
                    .pane
                    .exit_status
                    .as_ref()
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
    pub fn remove_at(&mut self, idx: usize) -> bool {
        if idx >= self.tabs.len() {
            return !self.tabs.is_empty();
        }
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

    /// Slice of all tab entries (for rendering the tab bar).
    pub fn tabs_mut(&mut self) -> &mut [TabEntry] {
        &mut self.tabs
    }

    pub fn tabs(&self) -> &[TabEntry] {
        &self.tabs
    }
}
