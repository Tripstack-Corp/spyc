//! Modal confirm-key handlers (remove, graveyard purge-all, Claude
//! crash-recover) + undo_last_remove. Split from key_dispatch.rs verbatim.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};

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
        // Optimistically ghost the removed rows so they don't momentarily
        // vanish before the off-thread `git status` re-adds a tracked file as a
        // struck-through ghost (the post-`R` list "bounce"). Only with a git
        // worker active (the sync no-worker path already has fresh status, no
        // gap) and inside a repo; skip files git already reports untracked —
        // those should drop cleanly with no ghost. The ghosts are dir-scoped
        // basenames and `apply_git_worker_result` clears them once the
        // authoritative status lands. (Files in subdirs of a removed dir aren't
        // ghosted — only the dir's own row is, and a dir never ghosts.)
        let (worker, in_repo, dir) = {
            let col = self.state.cur();
            (
                col.git_cache.git_worker_available,
                col.git_cache.current_repo_root.is_some(),
                col.listing.dir.clone(),
            )
        };
        if worker && in_repo {
            let names: Vec<String> = paths
                .iter()
                .filter(|p| p.parent() == Some(dir.as_path()))
                .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .filter(|name| {
                    self.state
                        .cur()
                        .git
                        .files
                        .get(name)
                        .is_none_or(|st| !st.untracked)
                })
                .collect();
            self.state.cur_mut().pending_ghosts.extend(names);
        }
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

        // Respawn fresh claude into the tab with the agent env injected (so
        // the recovered pane can report status via its hooks) — shared with
        // `:hooks on!`. No `/resume` arm here: the user types it manually after
        // a crash so they can decide whether to recover or start clean.
        if self.spawn_agent_into_tab(tab_idx, &fallback, &cwd, None) {
            self.state
                .flash_info("started fresh claude — type /resume to recover");
        }
        Vec::new()
    }

    /// First-launch consent before spyc writes Claude status hooks. Only an
    /// explicit `y`/`n` records a decision — `y` installs the hooks for the
    /// launching cwd, `n` remembers the denial. Any other key (including Esc)
    /// keeps the prompt open; the dialogue cannot be bypassed without choosing.
    pub(super) fn handle_hook_consent_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let prev_mode = std::mem::replace(&mut self.state.mode, Mode::Normal);
        let Mode::Prompting(Prompt {
            kind: PromptKind::HookConsent { root, cwd, agent },
            prefix,
            buffer,
            editor,
        }) = prev_mode
        else {
            return Vec::new();
        };
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                crate::state::hook_consent::set_consent(&root, true);
                self.install_status_hooks(&cwd, agent);
                // Claude reloads its config live (effect on the next message);
                // codex reads config at startup, so the hooks it just got apply
                // on its next launch.
                let profile = crate::agent::profile_for(agent);
                let live = profile.status_hooks().is_some_and(|s| s.live_reload);
                let name = profile.name();
                self.state.flash_info(if live {
                    format!("status hooks on — {name} reports its live activity (saved; `:hooks off` to undo)")
                } else {
                    format!("status hooks on — active on {name}'s next launch (saved; `:hooks off` to undo)")
                });
            }
            KeyCode::Char('n' | 'N') => {
                crate::state::hook_consent::set_consent(&root, false);
                self.state
                    .flash_info("status hooks declined for this project (`:hooks on` to enable)");
            }
            _ => {
                // Restore the prompt — y/n is required; Esc does not defer.
                self.state.mode = Mode::Prompting(Prompt {
                    kind: PromptKind::HookConsent { root, cwd, agent },
                    prefix,
                    buffer,
                    editor,
                });
                self.state.flash_info("press y or n");
                return Vec::new();
            }
        }
        self.view.needs_full_repaint = true;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn seed_hook_consent(app: &mut App) {
        app.state.mode = Mode::Prompting(Prompt::simple(
            PromptKind::HookConsent {
                root: PathBuf::from("/repo"),
                cwd: PathBuf::from("/repo"),
                agent: crate::state::sessions::AgentKind::Claude,
            },
            "consent? [y]es / [n]o ",
        ));
    }

    fn is_hook_consent_prompt(app: &App) -> bool {
        matches!(
            &app.state.mode,
            Mode::Prompting(Prompt {
                kind: PromptKind::HookConsent { .. },
                ..
            })
        )
    }

    /// The consent dialogue cannot be dismissed without an explicit decision:
    /// Esc (and Enter, ^C, a stray letter) leave it open with a y/n nudge.
    #[test]
    fn non_yn_keys_keep_hook_consent_prompt_open() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            for code in [
                KeyCode::Esc,
                KeyCode::Enter,
                KeyCode::Char('q'),
                KeyCode::Char(' '),
            ] {
                let mut app = App::test_app(tmp.path().to_path_buf());
                seed_hook_consent(&mut app);
                let _ = app.handle_key(key(code)).unwrap();
                assert!(
                    is_hook_consent_prompt(&app),
                    "{code:?} must not dismiss the consent dialogue"
                );
                assert_eq!(app.flash_text(), Some("press y or n"));
            }
        });
    }

    /// An explicit `n` records the (recoverable) denial and closes the prompt.
    #[test]
    fn n_closes_hook_consent_prompt() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            seed_hook_consent(&mut app);
            let _ = app.handle_key(key(KeyCode::Char('n'))).unwrap();
            assert!(
                matches!(app.state.mode, Mode::Normal),
                "n must close the consent dialogue"
            );
        });
    }
}
