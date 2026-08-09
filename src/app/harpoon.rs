//! Harpoon: project-local quick-mark bookmarks (`Ha`/`Hx`/`H<n>`) and the
//! harpoon menu overlay. Extracted verbatim from `app/mod.rs` (the
//! impl-extraction sweep), same child-module `impl App` pattern. The
//! append/remove/jump/open-menu entry points + the menu key handler are
//! `pub` (called from `actions` / `key_dispatch`); `harpoon_cursor_path` is
//! an internal helper. `sync_harpoon_filter_set` lives in `actions` (pub) and
//! resolves crate-wide.
//!
//! The menu's key handling splits the `route.rs` / `focus.rs` way: the pure
//! [`decide_harpoon_menu_key`] maps a `Copy` snapshot + key to a
//! [`HarpoonMenuAction`], and `handle_harpoon_menu_key` only applies it. Every
//! bound (empty list, cursor at either edge, `dd` arming) is decided in the
//! pure half, so the whole key matrix is table-testable without a live menu.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

use super::{App, Effect, HarpoonMenu};

/// The menu state [`decide_harpoon_menu_key`] reads. `Copy` so tests construct
/// one inline.
#[derive(Debug, Clone, Copy)]
struct HarpoonMenuSnapshot {
    /// Cursor row inside the menu (0-based).
    cursor: usize,
    /// Occupied slot count.
    len: usize,
    /// A previous `d` armed the delete, so a `d` now is the confirming second.
    delete_armed: bool,
}

/// What a key does to the open harpoon menu. Each variant is a complete
/// instruction — every bound is checked in [`decide_harpoon_menu_key`], so the
/// applier never re-guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HarpoonMenuAction {
    /// Swallow it; the menu owns all input while open.
    Ignore,
    /// `Esc` / `q`.
    Close,
    /// `j` / `k` / `g` / `G` — park the cursor here, already clamped.
    MoveTo(usize),
    /// `1`..`9` / `Enter` — jump to this 1-based slot, closing the menu.
    Jump(u8),
    /// `K` / `J` — reorder; the cursor follows the moved slot to `to`.
    Swap { from: usize, to: usize },
    /// First `d` of a `dd`.
    ArmDelete,
    /// Second `d` — drop this slot.
    DeleteAt(usize),
}

/// Decide what a key does to the harpoon menu. **Pure** (the `route.rs` /
/// `focus.rs` template), so every branch is unit-testable without a live menu.
///
/// Takes a bare [`KeyCode`]: modifiers are not consulted, matching the handler
/// this was extracted from.
const fn decide_harpoon_menu_key(snap: HarpoonMenuSnapshot, code: KeyCode) -> HarpoonMenuAction {
    let HarpoonMenuSnapshot {
        cursor,
        len,
        delete_armed,
    } = snap;
    match code {
        KeyCode::Esc | KeyCode::Char('q') => HarpoonMenuAction::Close,
        KeyCode::Char('j') | KeyCode::Down if len > 0 => {
            let last = len - 1;
            HarpoonMenuAction::MoveTo(if cursor >= last { last } else { cursor + 1 })
        }
        KeyCode::Char('k') | KeyCode::Up if len > 0 => {
            HarpoonMenuAction::MoveTo(cursor.saturating_sub(1))
        }
        KeyCode::Char('g') if len > 0 => HarpoonMenuAction::MoveTo(0),
        KeyCode::Char('G') if len > 0 => HarpoonMenuAction::MoveTo(len - 1),
        // Deliberately unguarded on `len`: a digit is direct slot addressing,
        // the same verb as `H5` outside the menu, so an empty slot closes and
        // flashes rather than doing nothing.
        KeyCode::Char(c @ '1'..='9') => HarpoonMenuAction::Jump(c as u8 - b'0'),
        KeyCode::Enter if len > 0 => HarpoonMenuAction::Jump((cursor + 1) as u8),
        KeyCode::Char('K') if cursor > 0 && len > 1 => HarpoonMenuAction::Swap {
            from: cursor,
            to: cursor - 1,
        },
        KeyCode::Char('J') if cursor + 1 < len => HarpoonMenuAction::Swap {
            from: cursor,
            to: cursor + 1,
        },
        KeyCode::Char('d') if delete_armed && cursor < len => HarpoonMenuAction::DeleteAt(cursor),
        KeyCode::Char('d') => HarpoonMenuAction::ArmDelete,
        _ => HarpoonMenuAction::Ignore,
    }
}

impl App {
    /// Path under the cursor (file or directory) that the harpoon
    /// `Ha`/`Hx` actions operate on. Returns the absolute path of
    /// the focused row, or `None` if the listing is empty.
    fn harpoon_cursor_path(&self) -> Option<PathBuf> {
        self.state
            .cur()
            .rows
            .get(self.state.cur().cursor.index)
            .map(|r| r.path.clone())
    }

    /// `Ha` — append the cursor file/dir to the project's harpoon
    /// list. Idempotent (already-harpooned paths flash and bail);
    /// hard-capped at `MAX_SLOTS`. Saves the list immediately so a
    /// crash before the next mutation doesn't lose the entry.
    pub fn harpoon_append(&mut self) {
        if self.state.cur().harpoon.is_none() {
            self.state
                .flash_error("harpoon: open a repo or set PROJECT_HOME (gP)");
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
            .cur_mut()
            .harpoon
            .as_mut()
            .expect("guarded by is_none check above");
        match h.append(path) {
            crate::state::harpoon::AppendResult::Added(slot) => {
                if let Err(e) = h.save() {
                    self.state
                        .flash_error(format!("harpoon save failed: {e:#}"));
                    return;
                }
                self.sync_harpoon_filter_set();
                if matches!(self.state.cur().temp_filter.as_deref(), Some("h")) {
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
        if self.state.cur().harpoon.is_none() {
            self.state
                .flash_error("harpoon: open a repo or set PROJECT_HOME (gP)");
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
            .cur_mut()
            .harpoon
            .as_mut()
            .expect("guarded by is_none check above");
        match h.remove(&path) {
            Some(slot) => {
                if let Err(e) = h.save() {
                    self.state
                        .flash_error(format!("harpoon save failed: {e:#}"));
                    return;
                }
                self.sync_harpoon_filter_set();
                if matches!(self.state.cur().temp_filter.as_deref(), Some("h")) {
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
        let Some(h) = self.state.cur().harpoon.as_ref() else {
            self.state
                .flash_error("harpoon: open a repo or set PROJECT_HOME (gP)");
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
            self.state.flash_error(format!("harpoon chdir: {e:#}"));
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
        if self.state.cur().harpoon.is_none() {
            self.state
                .flash_error("harpoon: open a repo or set PROJECT_HOME (gP)");
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
    pub fn handle_harpoon_menu_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let Some(menu) = self.view.harpoon_menu.as_ref() else {
            return Vec::new();
        };
        let Some(h) = self.state.cur().harpoon.as_ref() else {
            self.close_harpoon_menu();
            return Vec::new();
        };
        let snap = HarpoonMenuSnapshot {
            cursor: menu.cursor,
            len: h.slots.len(),
            delete_armed: menu.delete_armed,
        };
        // Any key disarms a pending `dd`; `ArmDelete` re-arms below.
        if let Some(m) = self.view.harpoon_menu.as_mut() {
            m.delete_armed = false;
        }

        match decide_harpoon_menu_key(snap, key.code) {
            HarpoonMenuAction::Ignore => {}
            HarpoonMenuAction::Close => self.close_harpoon_menu(),
            HarpoonMenuAction::MoveTo(idx) => {
                if let Some(m) = self.view.harpoon_menu.as_mut() {
                    m.cursor = idx;
                }
            }
            HarpoonMenuAction::Jump(slot) => {
                self.close_harpoon_menu();
                self.harpoon_jump(slot);
            }
            HarpoonMenuAction::Swap { from, to } => {
                if let Some(h) = self.state.cur_mut().harpoon.as_mut() {
                    h.swap(from, to);
                }
                if let Some(m) = self.view.harpoon_menu.as_mut() {
                    m.cursor = to;
                }
                self.save_harpoon_after_menu_edit();
            }
            HarpoonMenuAction::ArmDelete => {
                if let Some(m) = self.view.harpoon_menu.as_mut() {
                    m.delete_armed = true;
                }
            }
            HarpoonMenuAction::DeleteAt(idx) => self.harpoon_menu_delete(idx),
        }
        Vec::new()
    }

    /// Drop the menu overlay and force the repaint that erases it.
    const fn close_harpoon_menu(&mut self) {
        self.view.harpoon_menu = None;
        self.view.needs_full_repaint = true;
    }

    /// Persist a menu-driven mutation and refresh the `=h` filter set. A write
    /// failure flashes rather than unwinds: the in-memory list already changed,
    /// so the menu must keep rendering what it now holds.
    fn save_harpoon_after_menu_edit(&mut self) {
        let err = self
            .state
            .cur()
            .harpoon
            .as_ref()
            .and_then(|h| h.save().err());
        if let Some(e) = err {
            self.state
                .flash_error(format!("harpoon save failed: {e:#}"));
        }
        self.sync_harpoon_filter_set();
    }

    /// `dd` — remove the slot at `idx`, then re-clamp the menu cursor against
    /// the shortened list so it can't dangle past the end.
    fn harpoon_menu_delete(&mut self, idx: usize) {
        if let Some(h) = self.state.cur_mut().harpoon.as_mut() {
            h.remove_at(idx);
        }
        self.save_harpoon_after_menu_edit();
        if matches!(self.state.cur().temp_filter.as_deref(), Some("h")) {
            self.state.rebuild_rows();
        }
        let new_len = self
            .state
            .cur()
            .harpoon
            .as_ref()
            .map_or(0, |h| h.slots.len());
        if let Some(m) = self.view.harpoon_menu.as_mut() {
            m.cursor = if new_len == 0 {
                0
            } else {
                idx.min(new_len - 1)
            };
        }
    }
}

#[cfg(test)]
mod menu_key_tests {
    use super::{HarpoonMenuAction, HarpoonMenuSnapshot, decide_harpoon_menu_key};
    use crossterm::event::KeyCode;

    /// A menu over `len` slots with the cursor parked at `cursor`, nothing armed.
    const fn snap(cursor: usize, len: usize) -> HarpoonMenuSnapshot {
        HarpoonMenuSnapshot {
            cursor,
            len,
            delete_armed: false,
        }
    }

    const fn armed(cursor: usize, len: usize) -> HarpoonMenuSnapshot {
        HarpoonMenuSnapshot {
            cursor,
            len,
            delete_armed: true,
        }
    }

    fn decide(s: HarpoonMenuSnapshot, c: KeyCode) -> HarpoonMenuAction {
        decide_harpoon_menu_key(s, c)
    }

    #[test]
    fn esc_and_q_close() {
        assert_eq!(decide(snap(0, 3), KeyCode::Esc), HarpoonMenuAction::Close);
        assert_eq!(
            decide(snap(0, 3), KeyCode::Char('q')),
            HarpoonMenuAction::Close
        );
    }

    #[test]
    fn j_and_k_move_and_clamp_at_both_ends() {
        assert_eq!(
            decide(snap(0, 3), KeyCode::Char('j')),
            HarpoonMenuAction::MoveTo(1)
        );
        // At the last slot `j` stays put — the menu does not wrap.
        assert_eq!(
            decide(snap(2, 3), KeyCode::Char('j')),
            HarpoonMenuAction::MoveTo(2)
        );
        assert_eq!(
            decide(snap(2, 3), KeyCode::Char('k')),
            HarpoonMenuAction::MoveTo(1)
        );
        // At the top `k` stays put.
        assert_eq!(
            decide(snap(0, 3), KeyCode::Char('k')),
            HarpoonMenuAction::MoveTo(0)
        );
    }

    #[test]
    fn arrows_mirror_j_and_k() {
        assert_eq!(
            decide(snap(0, 3), KeyCode::Down),
            decide(snap(0, 3), KeyCode::Char('j'))
        );
        assert_eq!(
            decide(snap(2, 3), KeyCode::Up),
            decide(snap(2, 3), KeyCode::Char('k'))
        );
    }

    #[test]
    fn g_and_shift_g_jump_to_the_ends() {
        assert_eq!(
            decide(snap(2, 5), KeyCode::Char('g')),
            HarpoonMenuAction::MoveTo(0)
        );
        assert_eq!(
            decide(snap(0, 5), KeyCode::Char('G')),
            HarpoonMenuAction::MoveTo(4)
        );
    }

    #[test]
    fn motions_are_inert_on_an_empty_list() {
        // Every `len`-guarded motion must fall through rather than compute a
        // cursor against `len - 1` and underflow.
        for code in [
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Char('g'),
            KeyCode::Char('G'),
            KeyCode::Down,
            KeyCode::Up,
            KeyCode::Enter,
        ] {
            assert_eq!(
                decide(snap(0, 0), code),
                HarpoonMenuAction::Ignore,
                "{code:?} should be inert with no slots"
            );
        }
    }

    #[test]
    fn enter_jumps_to_the_one_based_slot_under_the_cursor() {
        assert_eq!(
            decide(snap(0, 3), KeyCode::Enter),
            HarpoonMenuAction::Jump(1)
        );
        assert_eq!(
            decide(snap(2, 3), KeyCode::Enter),
            HarpoonMenuAction::Jump(3)
        );
    }

    #[test]
    fn digits_address_slots_directly_even_past_the_end() {
        // Deliberate asymmetry with the motion keys: a digit is the same verb
        // as `H5` outside the menu, so an out-of-range digit still closes the
        // menu and lets `harpoon_jump` flash "slot N empty".
        assert_eq!(
            decide(snap(0, 2), KeyCode::Char('5')),
            HarpoonMenuAction::Jump(5)
        );
        assert_eq!(
            decide(snap(0, 0), KeyCode::Char('1')),
            HarpoonMenuAction::Jump(1)
        );
        // `0` is not a slot.
        assert_eq!(
            decide(snap(0, 3), KeyCode::Char('0')),
            HarpoonMenuAction::Ignore
        );
    }

    #[test]
    fn shift_k_and_shift_j_reorder_and_carry_the_cursor() {
        assert_eq!(
            decide(snap(1, 3), KeyCode::Char('K')),
            HarpoonMenuAction::Swap { from: 1, to: 0 }
        );
        assert_eq!(
            decide(snap(1, 3), KeyCode::Char('J')),
            HarpoonMenuAction::Swap { from: 1, to: 2 }
        );
    }

    #[test]
    fn reorder_is_inert_at_the_edges() {
        // `K` at the top and `J` at the bottom have nothing to swap with.
        assert_eq!(
            decide(snap(0, 3), KeyCode::Char('K')),
            HarpoonMenuAction::Ignore
        );
        assert_eq!(
            decide(snap(2, 3), KeyCode::Char('J')),
            HarpoonMenuAction::Ignore
        );
        // A single slot can't be reordered in either direction.
        assert_eq!(
            decide(snap(0, 1), KeyCode::Char('K')),
            HarpoonMenuAction::Ignore
        );
        assert_eq!(
            decide(snap(0, 1), KeyCode::Char('J')),
            HarpoonMenuAction::Ignore
        );
    }

    #[test]
    fn first_d_arms_and_second_d_deletes() {
        assert_eq!(
            decide(snap(1, 3), KeyCode::Char('d')),
            HarpoonMenuAction::ArmDelete
        );
        assert_eq!(
            decide(armed(1, 3), KeyCode::Char('d')),
            HarpoonMenuAction::DeleteAt(1)
        );
    }

    #[test]
    fn an_armed_d_over_no_slots_re_arms_instead_of_deleting() {
        assert_eq!(
            decide(armed(0, 0), KeyCode::Char('d')),
            HarpoonMenuAction::ArmDelete
        );
    }

    #[test]
    fn unbound_keys_are_swallowed_not_leaked() {
        // The menu owns all input while open — nothing may fall through to the
        // resolver behind it.
        for code in [
            KeyCode::Char('x'),
            KeyCode::Char('z'),
            KeyCode::Tab,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Home,
            KeyCode::F(1),
        ] {
            assert_eq!(
                decide(snap(1, 3), code),
                HarpoonMenuAction::Ignore,
                "{code:?} must be swallowed"
            );
        }
    }
}
