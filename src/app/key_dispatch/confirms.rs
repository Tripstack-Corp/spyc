//! Modal confirm-key handlers (remove, graveyard purge-all, Claude
//! crash-recover) + undo_last_remove. Split from key_dispatch.rs verbatim.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};

use crate::pane::{Pane, TabEntry, TabInfo};

use crate::app::graveyard_ops::GraveyardOp;
use crate::app::{App, Effect, Mode, Prompt, PromptKind};

impl App {
    /// Single-key confirmation for `R`. `y` / `Y` triggers the delete;
    /// anything else — including Enter, Esc, or any other letter — cancels.
    /// The prompt closes in every case.
    ///
    /// The archive + unlink is the heavy part (tar+zstd of the whole tree,
    /// seconds-to-minutes for a `target/` / `node_modules/`), so it runs
    /// OFF-thread via `Effect::Graveyard` — the loop stays live and the result
    /// flash + listing refresh land later via `apply_graveyard_outcomes`. Only
    /// the cheap prep (which paths) happens here.
    pub fn handle_remove_confirm_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        self.state.mode = Mode::Normal;
        // Pull the targeted paths out of the preview slot. This is the
        // authoritative list (computed at prompt time so `Ndd` ignores any
        // cursor wiggle); selection_paths() would re-derive from current state
        // and could disagree. Owned `PathBuf`s so they can move onto the worker.
        let preview = self.state.pending_delete_preview.take();
        if !confirmed {
            return Vec::new();
        }
        let paths: Vec<PathBuf> = preview.unwrap_or_else(|| {
            self.state
                .selection_paths()
                .iter()
                .map(|p| p.to_path_buf())
                .collect()
        });
        if paths.is_empty() {
            return Vec::new();
        }
        // The picks are the items being removed — clear them now (they're
        // gone from the user's intent); the listing still shows the files
        // until the worker unlinks them and the refresh lands.
        self.state.left.picks.clear();
        self.state
            .flash_info(format!("removing {} item(s)…", paths.len()));
        vec![Effect::Graveyard(GraveyardOp::Archive { paths })]
    }

    /// `:undo` — restore the most-recent graveyard entry to its original path.
    /// Best-effort recovery for the very common "I just deleted the wrong
    /// thing" case. The cheap part (load the inventory, pick the latest entry)
    /// runs here; the un-tar runs OFF-thread via `Effect::Graveyard` and
    /// reports via `apply_graveyard_outcomes` (which surfaces a tar
    /// `set_overwrite(false)` error if the original path is now occupied —
    /// the user can then `gy` + `p` to restore-to-cwd instead).
    pub fn undo_last_remove(&mut self) -> Vec<Effect> {
        let g = crate::state::graveyard::Graveyard::load();
        let Some(latest) = g.entries.into_iter().next() else {
            self.state.flash_info("undo: graveyard is empty");
            return Vec::new();
        };
        let dest = latest
            .orig_path
            .parent()
            .map_or_else(|| PathBuf::from("/"), std::path::Path::to_path_buf);
        self.state
            .flash_info(format!("undo: restoring {}…", latest.filename));
        vec![Effect::Graveyard(GraveyardOp::Restore {
            entry: Box::new(latest),
            dest,
        })]
    }

    /// Single-key confirmation for "purge ALL graveyard entries to system
    /// trash". Bound on `Z` from the graveyard view; routes to a separate
    /// prompt kind so the wording stays accurate. The cascade-to-trash (one
    /// extract per entry) runs OFF-thread via `Effect::Graveyard`; the count +
    /// graveyard-view refresh land via `apply_graveyard_outcomes`.
    pub(super) fn handle_graveyard_purge_all_confirm(&mut self, key: KeyEvent) -> Vec<Effect> {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        self.state.mode = Mode::Normal;
        if !confirmed {
            return Vec::new();
        }
        let entries = self.state.graveyard.clone();
        if entries.is_empty() {
            self.state.flash_info("graveyard: nothing to purge");
            return Vec::new();
        }
        self.state
            .flash_info(format!("purging {} item(s) to trash…", entries.len()));
        vec![Effect::Graveyard(GraveyardOp::PurgeAll { entries })]
    }

    /// Single-key confirmation for `^a x` on a tab whose child is still
    /// running. `y`/`Y` closes it; anything else keeps it. Always the active
    /// tab (the modal prompt blocks tab switching). An exited tab never opens
    /// this prompt — it closes silently in `close_active_tab`.
    pub(super) fn handle_close_pane_confirm_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        self.state.mode = Mode::Normal;
        if confirmed {
            self.close_active_tab_now();
        } else {
            self.view.needs_full_repaint = true;
        }
        Vec::new()
    }

    /// Single-key confirmation for the auto-fired claude crash recovery
    /// prompt. `y` / `Y` / Enter kills the broken tab and replaces it with
    /// a fresh `claude` (the user can then `/resume` manually); anything
    /// else kills it and removes the tab so the dump is off-screen.
    pub(super) fn handle_claude_crash_recover_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y') | KeyCode::Enter);
        let prev_mode = std::mem::replace(&mut self.state.mode, Mode::Normal);
        let Mode::Prompting(Prompt {
            kind: PromptKind::ClaudeCrashRecover { tab_idx },
            ..
        }) = prev_mode
        else {
            return Vec::new();
        };

        // Snapshot cwd + fallback from the tab and best-effort kill the
        // child (bunfs claude is often still alive post-crash; an
        // already-closed pane errors here, ignored).
        let Some((cwd, fallback)) = self.runtime.pane_tabs.as_mut().and_then(|tabs| {
            let entry = tabs.tabs_mut().get_mut(tab_idx)?;
            entry.pane.try_kill();
            let fallback = entry
                .info
                .restore_fallback
                .clone()
                .unwrap_or_else(|| "claude".to_string());
            Some((entry.info.cwd.clone(), fallback))
        }) else {
            return Vec::new();
        };

        if !confirmed {
            if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                let still_have_tabs = tabs.remove_at(tab_idx);
                if !still_have_tabs {
                    self.runtime.pane_tabs = None;
                }
            }
            // Reclaim the dismissed tab's parked scrollback stream (if any).
            self.prune_orphaned_pager_streams();
            self.state.flash_info("claude crash dismissed; tab closed");
            self.view.needs_full_repaint = true;
            return Vec::new();
        }

        let (rows, cols) = Self::pane_spawn_size(
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        let wake = self.make_pane_wake();
        match Pane::spawn_with_env(
            &fallback,
            rows,
            cols,
            &cwd,
            &self.view.context_path,
            &[],
            wake,
        ) {
            Ok(p) => {
                let entry = TabEntry::new(p, TabInfo::new(&fallback, &cwd));
                if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                    tabs.replace_at(tab_idx, entry);
                }
                // The replaced tab's old entry (and any stashed scrollback pager)
                // was dropped; reclaim its parked stream so it doesn't leak.
                self.prune_orphaned_pager_streams();
                self.state
                    .flash_info("started fresh claude — type /resume to recover");
            }
            Err(e) => self.state.flash_error(format!("claude spawn failed: {e}")),
        }
        Vec::new()
    }
}
