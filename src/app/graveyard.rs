//! Graveyard view: the `View::Graveyard` key dispatcher and its restore /
//! purge actions. Extracted verbatim from `app/mod.rs` (the impl-extraction
//! sweep), same child-module `impl App` pattern as `key_dispatch` / `render`:
//! methods read App's private state via the descendant-module rule.
//! `handle_graveyard_view_key` is `pub` (called from `key_dispatch`); the
//! restore/purge helpers stay private to this module.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
        //
        // The destructive verbs (restore p/P, purge x/dd/Z) require a *bare*
        // key — matching `key.code` alone let a Ctrl-chord (`^p`, `^d`, `^x`)
        // trigger an irreversible purge/restore. A modified key falls through
        // to the `_` arm (no-op + clears the `dd` arming).
        let bare = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
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
                // Navigation clears the pending chord arming — the documented
                // contract is "any other key clears it". Without this, `d` then
                // `j` then `d` would purge the *freshly-navigated-to* entry (a
                // surprise purge), and `g` then `j` then `g` would jump to top.
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                let rpc = self.state.left.grid_dims.rows_per_col as usize;
                self.state
                    .cursor_move_vertical(1, rpc, self.state.left.rows.len());
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                let rpc = self.state.left.grid_dims.rows_per_col as usize;
                self.state
                    .cursor_move_vertical(-1, rpc, self.state.left.rows.len());
            }
            KeyCode::Char('g') => {
                self.view.graveyard_pending_d = false;
                if self.view.graveyard_pending_g {
                    self.state.left.cursor.index = 0;
                    self.view.graveyard_pending_g = false;
                } else {
                    self.view.graveyard_pending_g = true;
                }
            }
            KeyCode::Char('G') => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                if !self.state.left.rows.is_empty() {
                    self.state.left.cursor.index = self.state.left.rows.len() - 1;
                }
            }
            KeyCode::Char('p') if bare => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                self.graveyard_restore(false);
            }
            KeyCode::Char('P') if bare => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                self.graveyard_restore(true);
            }
            KeyCode::Char('x') if bare => {
                self.view.graveyard_pending_d = false;
                self.view.graveyard_pending_g = false;
                self.graveyard_purge_cursor_entry();
            }
            KeyCode::Char('d') if bare => {
                self.view.graveyard_pending_g = false;
                if self.view.graveyard_pending_d {
                    self.view.graveyard_pending_d = false;
                    self.graveyard_purge_cursor_entry();
                } else {
                    self.view.graveyard_pending_d = true;
                }
            }
            KeyCode::Char('Z') if bare => {
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
        let Some(entry) = self
            .state
            .graveyard
            .get(self.state.left.cursor.index)
            .cloned()
        else {
            self.state.flash_error("graveyard: no entry under cursor");
            return;
        };
        let dest = if to_original {
            entry.orig_path.parent().map_or_else(
                || std::path::PathBuf::from("/"),
                std::path::Path::to_path_buf,
            )
        } else {
            self.state.left.listing.dir.clone()
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
                self.state.left.cursor.clamp(self.state.graveyard.len());
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
        let Some(entry) = self
            .state
            .graveyard
            .get(self.state.left.cursor.index)
            .cloned()
        else {
            self.state.flash_error("graveyard: no entry under cursor");
            return;
        };
        match crate::state::graveyard::Graveyard::cascade_entry_to_trash(&entry) {
            Ok(()) => {
                self.state
                    .flash_info(format!("→ system trash: {}", entry.filename));
                self.state.graveyard = crate::state::graveyard::Graveyard::load().entries;
                self.state.left.cursor.clamp(self.state.graveyard.len());
                self.state.rebuild_rows();
            }
            Err(e) => self.state.flash_error(format!("purge failed: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// Regression: `d` arms a single-key purge; navigating with `j`/`k` must
    /// clear that arming so a subsequent stray `d` can't purge whatever entry
    /// the cursor landed on. (Documented contract: "any other key clears it".)
    #[test]
    fn navigation_clears_dd_arming() {
        let mut app = App::test_app(std::env::temp_dir());
        app.seed_rows(&["a", "b", "c"]);

        app.handle_graveyard_view_key(bare('d'));
        assert!(
            app.view.graveyard_pending_d,
            "first `d` should arm the purge"
        );
        app.handle_graveyard_view_key(bare('j'));
        assert!(
            !app.view.graveyard_pending_d,
            "`j` must clear the `dd` arming (else `d j d` is a surprise purge)"
        );

        app.handle_graveyard_view_key(bare('d'));
        app.handle_graveyard_view_key(bare('k'));
        assert!(
            !app.view.graveyard_pending_d,
            "`k` must clear the `dd` arming"
        );
    }

    /// Same contract for the `gg` chord: navigating between the two `g`s must
    /// cancel the pending jump-to-top.
    #[test]
    fn navigation_clears_gg_arming() {
        let mut app = App::test_app(std::env::temp_dir());
        app.seed_rows(&["a", "b", "c"]);

        app.handle_graveyard_view_key(bare('g'));
        assert!(app.view.graveyard_pending_g, "first `g` should arm `gg`");
        app.handle_graveyard_view_key(bare('j'));
        assert!(
            !app.view.graveyard_pending_g,
            "`j` must clear the `gg` arming"
        );
    }
}
