//! Multi-tab management for the lower pane.
//!
//! `PaneTabs` wraps a `Vec<TabEntry>` and an active-tab index, keeping
//! all tab lifecycle logic out of `App`.

use std::path::PathBuf;

use super::Pane;

/// Per-tab metadata displayed in the status line.
#[allow(dead_code)]
pub struct TabInfo {
    /// Full command string passed to `Pane::spawn`.
    pub command: String,
    /// Short display name (first word of command).
    pub label: String,
    /// Working directory at spawn time.
    pub cwd: PathBuf,
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
        }
    }
}

/// A single tab: a `Pane` plus its metadata.
pub struct TabEntry {
    pub pane: Pane,
    pub info: TabInfo,
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

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Switch to tab at `idx` (0-indexed). Clamped to valid range.
    pub fn switch_to(&mut self, idx: usize) {
        if !self.tabs.is_empty() {
            self.active = idx.min(self.tabs.len() - 1);
        }
    }

    pub fn next(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    pub fn prev(&mut self) {
        if !self.tabs.is_empty() {
            self.active = if self.active == 0 {
                self.tabs.len() - 1
            } else {
                self.active - 1
            };
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
        if self.tabs.is_empty() {
            return false;
        }
        self.tabs.remove(self.active);
        if self.tabs.is_empty() {
            return false;
        }
        // Keep active in bounds.
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        true
    }

    /// Drain output from *every* tab so background tabs don't lose data.
    pub fn drain_all(&mut self) {
        for entry in &mut self.tabs {
            entry.pane.drain_output();
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

    /// Slice of all tab entries (for rendering the tab bar).
    pub fn tabs(&self) -> &[TabEntry] {
        &self.tabs
    }
}
