//! Modal confirm-key handlers (remove, graveyard purge-all, Claude
//! crash-recover) + undo_last_remove. Split from key_dispatch.rs verbatim.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

use crate::fs;
use crate::pane::{Pane, TabEntry, TabInfo};
use crate::spyc_debug;

use crate::app::{App, Effect, Mode, Prompt, PromptKind, View};

impl App {
    /// Single-key confirmation for `R`. `y` / `Y` triggers the delete;
    /// anything else — including Enter, Esc, or any other letter — cancels.
    /// The prompt closes in every case.
    pub fn handle_remove_confirm_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        self.state.mode = Mode::Normal;
        // Pull the targeted paths out of the preview slot. This is
        // the authoritative list (it was computed at prompt time so
        // `Ndd` ignores any cursor wiggle that might have happened);
        // selection_paths() would re-derive from current state and
        // could disagree.
        let preview = self.state.pending_delete_preview.take();
        if !confirmed {
            return Vec::new();
        }
        let paths: Vec<&Path> = preview.as_ref().map_or_else(
            || self.state.selection_paths(),
            |v| v.iter().map(PathBuf::as_path).collect(),
        );
        if paths.is_empty() {
            return Vec::new();
        }
        // Route through the graveyard: archive each path into
        // `<uuid>.tar.zst` first, then unlink the source. If the
        // archive step fails for any path we skip the unlink for
        // *that* path and surface a clear error — the user keeps
        // the file. Per-path failures don't stop the rest of the
        // batch; we report the count at the end.
        let mut archived = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for p in &paths {
            match crate::state::graveyard::Graveyard::write_entry(p) {
                Ok(_entry) => match fs::ops::remove_tree(p) {
                    Ok(()) => archived += 1,
                    Err(e) => {
                        failures.push(format!("{}: archived but unlink failed: {e}", p.display()));
                    }
                },
                Err(e) => {
                    // Archive failed — fall back to a hard delete
                    // would surprise the user (they expect undo);
                    // instead, leave the file alone and report.
                    failures.push(format!(
                        "{}: graveyard archive failed: {e} — file NOT removed",
                        p.display()
                    ));
                }
            }
        }
        if failures.is_empty() {
            self.state
                .flash_info(format!("removed {archived} item(s) (recoverable: gy)"));
        } else {
            // First failure goes in the flash; remainder in debug log.
            self.state.flash_error(failures[0].clone());
            for msg in &failures[1..] {
                spyc_debug!("R: {msg}");
            }
        }
        self.state.picks.clear();
        self.state.refresh_listing();
        Vec::new()
    }

    /// `:undo` — restore the most-recent graveyard entry to its
    /// original path. Best-effort recovery for the very common
    /// "I just deleted the wrong thing" case. If the original
    /// path is occupied (rare; user recreated it), tar's
    /// `set_overwrite(false)` errors and we surface that — the
    /// user can open `gy` and pick `p` to restore-to-cwd instead.
    pub fn undo_last_remove(&mut self) {
        let g = crate::state::graveyard::Graveyard::load();
        let Some(latest) = g.entries.into_iter().next() else {
            self.state.flash_info("undo: graveyard is empty");
            return;
        };
        let dest = latest.orig_path.parent().map_or_else(
            || std::path::PathBuf::from("/"),
            std::path::Path::to_path_buf,
        );
        match crate::state::graveyard::Graveyard::restore(&latest, &dest) {
            Ok(()) => {
                crate::state::graveyard::Graveyard::delete_entry(&latest);
                self.state.flash_info(format!(
                    "undo: restored {} → {}",
                    latest.filename,
                    dest.display()
                ));
                if matches!(self.state.view, View::Graveyard) {
                    self.state.graveyard = crate::state::graveyard::Graveyard::load().entries;
                    self.state.cursor.clamp(self.state.graveyard.len());
                    self.state.rebuild_rows();
                }
                self.state.refresh_listing();
            }
            Err(e) => self
                .state
                .flash_error(format!("undo: {e} — try `gy` then `p` to restore to cwd")),
        }
    }

    /// Single-key confirmation for "purge ALL graveyard entries to
    /// system trash". Bound on `Z` from the graveyard view; routes
    /// to a separate prompt kind so the wording stays accurate.
    pub(super) fn handle_graveyard_purge_all_confirm(&mut self, key: KeyEvent) -> Vec<Effect> {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        self.state.mode = Mode::Normal;
        if !confirmed {
            return Vec::new();
        }
        let mut trashed = 0usize;
        let mut errors = 0usize;
        for entry in self.state.graveyard.clone() {
            match crate::state::graveyard::Graveyard::cascade_entry_to_trash(&entry) {
                Ok(()) => trashed += 1,
                Err(_) => errors += 1,
            }
        }
        if errors > 0 {
            self.state
                .flash_error(format!("graveyard: trashed {trashed}, {errors} failed"));
        } else {
            self.state
                .flash_info(format!("graveyard: trashed {trashed} item(s)"));
        }
        self.state.graveyard = crate::state::graveyard::Graveyard::load().entries;
        self.state.cursor.clamp(self.state.graveyard.len());
        self.state.rebuild_rows();
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
                self.state
                    .flash_info("started fresh claude — type /resume to recover");
            }
            Err(e) => self.state.flash_error(format!("claude spawn failed: {e}")),
        }
        Vec::new()
    }
}
