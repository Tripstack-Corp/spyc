//! Graveyard view: the `View::Graveyard` key dispatcher and its restore /
//! purge actions. Extracted verbatim from `app/mod.rs` (the impl-extraction
//! sweep), same child-module `impl App` pattern as `key_dispatch` / `render`:
//! methods read App's private state via the descendant-module rule.
//! `handle_graveyard_view_key` is `pub` (called from `key_dispatch`); the
//! restore/purge helpers stay private to this module.

use crossterm::event::{KeyCode, KeyEvent};

use super::{App, Effect, Mode, Prompt, PromptKind};

impl App {
    /// Key dispatcher for `View::Graveyard`. Bindings:
    ///   `j`/`k`/arrows       — move cursor
    ///   `g`/`G`              — first / last
    ///   `p`                  — restore the cursor entry to cwd
    ///   `P`                  — restore to original path (refuses
    ///                          to clobber existing files)
    ///   `dd` (vim-style) /   — purge cursor entry to system trash
    ///   `x`
    ///   `Z`                  — purge ALL entries to system trash
    ///                          (single-key confirm: `y` to commit)
    ///   `Esc`                — close the view, return to dir
    ///
    /// `dd` arming uses a per-instance bool; first `d` arms, any
    /// other key (including a second non-`d`) clears it.
    pub fn handle_graveyard_view_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        // Confirm-purge-all is a transient inline confirm. We
        // signal it via a one-shot Mode::Prompting; routed there
        // directly rather than reusing RemoveConfirm because the
        // semantics are distinct (we're cascading to system trash,
        // not unlinking).
        match key.code {
            KeyCode::Char('?') | KeyCode::F(1) => {
                // Reported: graveyard view had no `?` help, so the
                // restore / purge bindings were undiscoverable
                // from within the view. The pager-mounted help
                // overlay coexists fine with the underlying
                // graveyard view — Esc on the help returns to the
                // same cursor position.
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                self.open_help();
            }
            KeyCode::Esc => {
                self.state.open_graveyard_view(); // toggle off
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let rpc = self.state.grid_dims.rows_per_col as usize;
                self.state
                    .cursor_move_vertical(1, rpc, self.state.rows.len());
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let rpc = self.state.grid_dims.rows_per_col as usize;
                self.state
                    .cursor_move_vertical(-1, rpc, self.state.rows.len());
            }
            KeyCode::Char('g') => {
                self.view.graveyard_pending_d = false;
                if self.view.graveyard_pending_g {
                    self.state.cursor.index = 0;
                    self.view.graveyard_pending_g = false;
                } else {
                    self.view.graveyard_pending_g = true;
                }
            }
            KeyCode::Char('G') => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                if !self.state.rows.is_empty() {
                    self.state.cursor.index = self.state.rows.len() - 1;
                }
            }
            KeyCode::Char('p') => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                self.graveyard_restore(false);
            }
            KeyCode::Char('P') => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                self.graveyard_restore(true);
            }
            KeyCode::Char('x') => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                self.graveyard_purge_cursor_entry();
            }
            KeyCode::Char('d') => {
                self.view.graveyard_pending_g = false;
                if self.view.graveyard_pending_d {
                    self.view.graveyard_pending_d = false;
                    self.graveyard_purge_cursor_entry();
                } else {
                    self.view.graveyard_pending_d = true;
                }
            }
            KeyCode::Char('Z') => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                self.state.mode = Mode::Prompting(Prompt::simple(
                    PromptKind::GraveyardPurgeAllConfirm,
                    "purge ALL graveyard entries to system trash? (y/N): ",
                ));
            }
            _ => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
            }
        }
        Vec::new()
    }

    /// Restore the cursor entry from the graveyard. `to_original`
    /// = true means the original path (use `Graveyard::restore`
    /// with the orig dir as dest); false = current cwd.
    fn graveyard_restore(&mut self, to_original: bool) {
        let Some(entry) = self.state.graveyard.get(self.state.cursor.index).cloned() else {
            self.state.flash_error("graveyard: no entry under cursor");
            return;
        };
        let dest = if to_original {
            entry.orig_path.parent().map_or_else(
                || std::path::PathBuf::from("/"),
                std::path::Path::to_path_buf,
            )
        } else {
            self.state.listing.dir.clone()
        };
        match crate::state::graveyard::Graveyard::restore(&entry, &dest) {
            Ok(()) => {
                // Restoration succeeded — drop the entry from the
                // graveyard so the user doesn't think it's still there.
                crate::state::graveyard::Graveyard::delete_entry(&entry);
                let where_ = if to_original { "original" } else { "cwd" };
                self.state
                    .flash_info(format!("restored {} ({where_})", entry.filename));
                self.state.graveyard = crate::state::graveyard::Graveyard::load().entries;
                self.state.cursor.clamp(self.state.graveyard.len());
                self.state.refresh_listing(); // dest may be cwd
                self.state.rebuild_rows();
            }
            Err(e) => {
                self.state
                    .flash_error(format!("restore failed: {e} (target may already exist)"));
            }
        }
    }

    /// Purge the cursor entry to system trash. Used by `dd` and `x`.
    fn graveyard_purge_cursor_entry(&mut self) {
        let Some(entry) = self.state.graveyard.get(self.state.cursor.index).cloned() else {
            self.state.flash_error("graveyard: no entry under cursor");
            return;
        };
        match crate::state::graveyard::Graveyard::cascade_entry_to_trash(&entry) {
            Ok(()) => {
                self.state
                    .flash_info(format!("→ system trash: {}", entry.filename));
                self.state.graveyard = crate::state::graveyard::Graveyard::load().entries;
                self.state.cursor.clamp(self.state.graveyard.len());
                self.state.rebuild_rows();
            }
            Err(e) => self.state.flash_error(format!("purge failed: {e}")),
        }
    }
}
