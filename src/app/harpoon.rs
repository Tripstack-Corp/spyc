//! Harpoon: project-local quick-mark bookmarks (`Ha`/`Hx`/`H<n>`) and the
//! harpoon menu overlay. Extracted verbatim from `app/mod.rs` (the
//! impl-extraction sweep), same child-module `impl App` pattern. The
//! append/remove/jump/open-menu entry points + the menu key handler are
//! `pub` (called from `actions` / `key_dispatch`); `harpoon_cursor_path` is
//! an internal helper. `sync_harpoon_filter_set` lives in `actions` (pub) and
//! resolves crate-wide.

use std::path::{Path, PathBuf};

use super::{App, Effect, HarpoonMenu};

impl App {
    /// Path under the cursor (file or directory) that the harpoon
    /// `Ha`/`Hx` actions operate on. Returns the absolute path of
    /// the focused row, or `None` if the listing is empty.
    fn harpoon_cursor_path(&self) -> Option<PathBuf> {
        self.state
            .left
            .rows
            .get(self.state.left.cursor.index)
            .map(|r| r.path.clone())
    }

    /// `Ha` — append the cursor file/dir to the project's harpoon
    /// list. Idempotent (already-harpooned paths flash and bail);
    /// hard-capped at `MAX_SLOTS`. Saves the list immediately so a
    /// crash before the next mutation doesn't lose the entry.
    pub fn harpoon_append(&mut self) {
        if self.state.harpoon.is_none() {
            self.state
                .flash_error("harpoon: set PROJECT_HOME first (gP)");
            return;
        }
        let Some(path) = self.harpoon_cursor_path() else {
            self.state.flash_error("harpoon: nothing under cursor");
            return;
        };
        let label = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let h = self
            .state
            .harpoon
            .as_mut()
            .expect("guarded by is_none check above");
        match h.append(path) {
            crate::state::harpoon::AppendResult::Added(slot) => {
                if let Err(e) = h.save() {
                    self.state.flash_error(format!("harpoon save failed: {e}"));
                    return;
                }
                self.sync_harpoon_filter_set();
                if matches!(self.state.left.temp_filter.as_deref(), Some("h")) {
                    self.state.rebuild_rows();
                }
                self.state.flash_info(format!("harpoon[{slot}] {label}"));
            }
            crate::state::harpoon::AppendResult::AlreadyPresent => {
                self.state
                    .flash_info(format!("harpoon: already in list — {label}"));
            }
            crate::state::harpoon::AppendResult::Full => {
                self.state.flash_error(format!(
                    "harpoon full ({} slots) — Hx to remove first",
                    crate::state::harpoon::MAX_SLOTS
                ));
            }
        }
    }

    /// `Hx` — remove the cursor file from the harpoon list (any
    /// slot). No-op + flash if it isn't harpooned.
    pub fn harpoon_remove(&mut self) {
        if self.state.harpoon.is_none() {
            self.state
                .flash_error("harpoon: set PROJECT_HOME first (gP)");
            return;
        }
        let Some(path) = self.harpoon_cursor_path() else {
            self.state.flash_error("harpoon: nothing under cursor");
            return;
        };
        let label = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let h = self
            .state
            .harpoon
            .as_mut()
            .expect("guarded by is_none check above");
        match h.remove(&path) {
            Some(slot) => {
                if let Err(e) = h.save() {
                    self.state.flash_error(format!("harpoon save failed: {e}"));
                    return;
                }
                self.sync_harpoon_filter_set();
                if matches!(self.state.left.temp_filter.as_deref(), Some("h")) {
                    self.state.rebuild_rows();
                }
                self.state
                    .flash_info(format!("harpoon: removed [{slot}] {label}"));
            }
            None => self
                .state
                .flash_info(format!("harpoon: not in list — {label}")),
        }
    }

    /// `H<digit>` — jump to slot N. Cursor-land semantics: chdir to
    /// the file's parent and place the cursor on it (or chdir into
    /// the directory if the slot is a directory). The user picks
    /// the verb (Enter, V, ^a s) afterwards. Missing-on-disk → flash
    /// and bail; we don't auto-prune (the user might be mid-rebase).
    pub fn harpoon_jump(&mut self, slot: u8) {
        let Some(h) = self.state.harpoon.as_ref() else {
            self.state
                .flash_error("harpoon: set PROJECT_HOME first (gP)");
            return;
        };
        let Some(target) = h.get(slot).map(Path::to_path_buf) else {
            self.state.flash_info(format!("harpoon: slot {slot} empty"));
            return;
        };
        if !target.exists() {
            self.state.flash_error(format!(
                "harpoon: gone — {}",
                target.file_name().map_or_else(
                    || target.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
            ));
            return;
        }
        let (chdir_to, focus) = if target.is_dir() {
            (target, None)
        } else if let Some(parent) = target.parent() {
            (parent.to_path_buf(), Some(target.clone()))
        } else {
            self.state.flash_error("harpoon: slot has no parent dir");
            return;
        };
        if let Err(e) = self.state.chdir(&chdir_to) {
            self.state.flash_error(format!("harpoon chdir: {e}"));
            return;
        }
        if let Some(p) = focus {
            self.state.focus_on_path(&p);
        }
        self.state.rebuild_rows();
        self.state.flash_info(format!("harpoon[{slot}]"));
    }

    /// `Hh` / `gh` — open the harpoon menu overlay. The menu
    /// intercepts subsequent keys until closed (Esc/q). No-op when
    /// the list is unset (no PROJECT_HOME).
    pub fn harpoon_open_menu(&mut self) {
        if self.state.harpoon.is_none() {
            self.state
                .flash_error("harpoon: set PROJECT_HOME first (gP)");
            return;
        }
        self.view.harpoon_menu = Some(HarpoonMenu {
            cursor: 0,
            delete_armed: false,
        });
        self.view.needs_full_repaint = true;
    }

    /// Key handler for the harpoon menu overlay. Owns all input
    /// while the menu is open. Bindings:
    ///   `j`/`k` (and arrows) — move cursor in the menu
    ///   `g`/`G` — jump to first/last slot
    ///   `1`..`9` — jump directly to slot N (and close)
    ///   `Enter` — jump to slot under cursor (and close)
    ///   `K`/`J` — swap slot up / down (reorder)
    ///   `dd` — delete slot under cursor (vim convention; first `d`
    ///          arms, second `d` confirms; any other key disarms)
    ///   `Esc`/`q` — close menu
    pub fn handle_harpoon_menu_key(&mut self, key: crossterm::event::KeyEvent) -> Vec<Effect> {
        use crossterm::event::KeyCode;
        let Some(menu) = self.view.harpoon_menu.as_mut() else {
            return Vec::new();
        };
        let Some(h) = self.state.harpoon.as_mut() else {
            self.view.harpoon_menu = None;
            self.view.needs_full_repaint = true;
            return Vec::new();
        };
        let len = h.slots.len();

        // `dd` arming. The pending-d flag lives on App so it survives
        // across this call (which can't borrow `menu` mutably across
        // re-entry). Using a local approach: piggyback on `cursor`'s
        // high bit would be hacky — keep it simple and use a separate
        // bool field on `HarpoonMenu`.
        let pending_delete = menu.delete_armed;
        if pending_delete {
            menu.delete_armed = false;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.view.harpoon_menu = None;
                self.view.needs_full_repaint = true;
            }
            KeyCode::Char('j') | KeyCode::Down if len > 0 => {
                menu.cursor = (menu.cursor + 1).min(len - 1);
            }
            KeyCode::Char('k') | KeyCode::Up if len > 0 => {
                menu.cursor = menu.cursor.saturating_sub(1);
            }
            KeyCode::Char('g') if len > 0 => {
                menu.cursor = 0;
            }
            KeyCode::Char('G') if len > 0 => {
                menu.cursor = len - 1;
            }
            KeyCode::Char(c @ '1'..='9') => {
                let slot = c as u8 - b'0';
                self.view.harpoon_menu = None;
                self.view.needs_full_repaint = true;
                self.harpoon_jump(slot);
            }
            KeyCode::Enter if len > 0 => {
                let slot = (menu.cursor + 1) as u8;
                self.view.harpoon_menu = None;
                self.view.needs_full_repaint = true;
                self.harpoon_jump(slot);
            }
            KeyCode::Char('K') if menu.cursor > 0 && len > 1 => {
                h.swap(menu.cursor, menu.cursor - 1);
                menu.cursor -= 1;
                if let Err(e) = h.save() {
                    self.state.flash_error(format!("harpoon save failed: {e}"));
                }
                self.sync_harpoon_filter_set();
            }
            KeyCode::Char('J') if menu.cursor + 1 < len => {
                h.swap(menu.cursor, menu.cursor + 1);
                menu.cursor += 1;
                if let Err(e) = h.save() {
                    self.state.flash_error(format!("harpoon save failed: {e}"));
                }
                self.sync_harpoon_filter_set();
            }
            KeyCode::Char('d') => {
                if pending_delete && menu.cursor < len {
                    let removed_idx = menu.cursor;
                    h.remove_at(removed_idx);
                    if let Err(e) = h.save() {
                        self.state.flash_error(format!("harpoon save failed: {e}"));
                    }
                    self.sync_harpoon_filter_set();
                    if matches!(self.state.left.temp_filter.as_deref(), Some("h")) {
                        self.state.rebuild_rows();
                    }
                    // Re-fetch menu since filter sync invalidates `menu` borrow
                    if let Some(m) = self.view.harpoon_menu.as_mut() {
                        let new_len = self.state.harpoon.as_ref().map_or(0, |hh| hh.slots.len());
                        if new_len == 0 {
                            m.cursor = 0;
                        } else {
                            m.cursor = removed_idx.min(new_len - 1);
                        }
                    }
                } else if let Some(m) = self.view.harpoon_menu.as_mut() {
                    m.delete_armed = true;
                }
            }
            _ => {}
        }
        Vec::new()
    }
}
