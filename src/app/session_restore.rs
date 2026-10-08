//! Rebuild safe tabs and workspace layout without losing refused saved records.

use super::state::restore::{DeferredTab, restored_active_index};
use super::state::{Focus, Side, VSplit, VsplitMode};
use super::{App, Effect, RESTORE_BANNER_SETTLE};

impl App {
    pub fn restore_session(&mut self, session: &crate::state::sessions::Session) -> Vec<Effect> {
        let plans: Vec<_> = session
            .tabs
            .iter()
            .map(|tab| {
                let cwd = if tab.cwd.is_dir() {
                    &tab.cwd
                } else {
                    &session.cwd
                };
                crate::agent::profile_for(tab.effective_kind()).reconstruct_restore(
                    &tab.command,
                    tab.agent_session_id.as_deref(),
                    cwd,
                )
            })
            .collect();
        // A wholly refused tab set leaves the current session intact. Mixed
        // sessions proceed, without ever spawning a refused command.
        if !plans.is_empty()
            && plans
                .iter()
                .all(|plan| matches!(plan.resume, crate::agent::ResumeAction::Refuse { .. }))
        {
            if let crate::agent::ResumeAction::Refuse { reason } = &plans[0].resume {
                self.state.flash_error(format!(
                    "session restore refused for all {} tabs: {reason}",
                    plans.len()
                ));
            }
            return Vec::new();
        }
        // Restore working directory and update start_dir so backtick (`)
        // jumps to the session's home, not where spyc was launched from.
        let mut effects = Vec::new();
        if session.cwd.is_dir() {
            if let Err(e) = self.state.chdir(&session.cwd) {
                self.state.flash_error(format!("session chdir: {e:#}"));
                return effects;
            }
            self.state.start_dir.clone_from(&session.cwd);
        } else if super::archive::archive_ancestor_of(&session.cwd).is_some() {
            // The user quit while browsing an archive. It isn't a directory and
            // never was — the effect screen mounts it and lands the column back
            // where they left off. The rest of the restore carries on meanwhile.
            self.state.start_dir.clone_from(&session.cwd);
            effects.push(Effect::ChangeDir {
                path: session.cwd.clone(),
                focus: None,
                on_ok: None,
                err_prefix: "session chdir failed",
            });
        } else {
            self.state
                .flash_error(format!("session dir gone: {}", session.cwd.display()));
            return effects;
        }
        // Keep the startup-generated name when an older session file
        // has no name field; otherwise take the saved one.
        if !session.name.is_empty() {
            self.state.session_name = Some(session.name.clone());
        }
        // Continue overwriting the restored session's own `<id>.json` on
        // subsequent saves/autosaves (P3-2), rather than forking a new file.
        self.state.session_id = Some(session.id);
        self.state.deferred_tabs.clear();
        self.state.project_home = session.project_home.clone().filter(|p| p.is_dir());
        // Restore pane layout.
        self.state.pane.pane_height_pct = session.pane_height_pct;
        if !session.tabs.is_empty() {
            self.runtime.pane_tabs = None;
            let mut restored_indices = Vec::new();
            for (index, (tab, plan)) in session.tabs.iter().zip(plans).enumerate() {
                if let crate::agent::ResumeAction::Refuse { reason } = &plan.resume {
                    self.state.deferred_tabs.push(DeferredTab {
                        saved_index: index,
                        tab: tab.clone(),
                        reason: reason.clone(),
                    });
                    continue;
                }
                let cwd = if tab.cwd.is_dir() {
                    &tab.cwd
                } else {
                    &session.cwd
                };
                // Codex restores by spawning `codex resume <UUID>`
                // directly — the CLI flag works, no `/resume` stdin
                // dance needed. Claude has a regression on the CLI
                // flag (crashes at mount with non-empty initialMessages),
                // so we always spawn fresh and type `/resume <sid>`
                // once it has settled.
                // Reconstruct the spawn command via the agent profile.
                // Codex/agy bake the resume into the command;
                // claude spawns fresh and arms the `/resume <sid>` stdin
                // send below (its `--resume` CLI flag crashes at mount
                // with non-empty initialMessages).
                // Only arm the `/resume` injection when the spawn actually
                // added a tab — `last_mut()` is "the tab we just pushed". If
                // the spawn failed, the last tab is a *different*, already-
                // restored pane, and we'd type `/resume <sid>` into the wrong
                // agent.
                let spawned = self.open_pane_tab_in(&plan.command, cwd);
                if spawned
                    && let Some(tabs) = self.runtime.pane_tabs.as_mut()
                    && let Some(entry) = tabs.tabs_mut().last_mut()
                {
                    restored_indices.push(index);
                    // Label the tab we just pushed from ITS saved entry.
                    // Setting labels inline (rather than zipping tabs ↔
                    // session.tabs after the loop) keeps them aligned even
                    // when an earlier tab's spawn failed and the two vectors
                    // diverge. Defensive `strip_exit_suffix` heals older
                    // session files saved before the save-side strip landed.
                    entry.info.label = crate::pane::tabs::strip_exit_suffix(&tab.label);
                    // P2: re-bind this respawned tab to its pre-restore scope-
                    // claim owner key (empty on an older save, or a tab that
                    // never had one) — leave the fresh one `TabInfo::new` just
                    // assigned rather than overwrite with an empty string.
                    if !tab.claim_owner.is_empty() {
                        entry.info.claim_owner.clone_from(&tab.claim_owner);
                    }
                    if let crate::agent::ResumeAction::ClaudeStdin { session_id } = plan.resume {
                        // Pin the exact session this pane is resuming so the next
                        // save persists it directly, never re-deriving it from the
                        // spawn-proximity heuristic that crosses panes restored
                        // together (they all spawn within the same second).
                        entry.info.live_session_id = Some(session_id.clone());
                        entry.info.pending_resume_send =
                            Some(crate::pane::tabs::PendingResumeSend::Text {
                                sid: session_id,
                                after: std::time::Instant::now() + RESTORE_BANNER_SETTLE,
                            });
                    }
                } else {
                    let reason = self
                        .state
                        .flash
                        .as_ref()
                        .map_or_else(|| "pane could not start".into(), |flash| flash.text.clone());
                    self.state.deferred_tabs.push(DeferredTab {
                        saved_index: index,
                        tab: tab.clone(),
                        reason,
                    });
                }
            }
            if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                tabs.switch_to(restored_active_index(&restored_indices, session.active_tab));
            }
            self.state.focus = if session.pane_focused && !restored_indices.is_empty() {
                Focus::Pane
            } else {
                Focus::FileList
            };
        }
        // Restore the vertical split (shape + previewed file). Independent of
        // the pane block above — a split can exist without a bottom pane.
        if let Some(sv) = &session.vsplit {
            self.restore_vsplit(sv, session.pane_focused);
        }
        // P2: restore the scope-coordination registry verbatim — independent
        // of tab-restore success, like the vsplit above. A claim whose owning
        // tab failed to respawn (or whose save predates `claim_owner`) just
        // shows up "orphaned" on the registry / orchestration screen, still
        // informative and releasable rather than silently lost.
        self.state.scope_registry.clone_from(&session.scope_claims);
        if self.state.deferred_tabs.is_empty() {
            self.state.flash_info("session restored");
        } else {
            let count = self.state.deferred_tabs.len();
            self.state.flash_error(format!(
                "session restored; {count} tab(s) unopened, kept saved — session info shows reasons"
            ));
        }
        effects
    }

    /// Restore a saved vertical split: reopen the second commander at its saved
    /// cwd (PR G) or re-load the Stage-1 preview file, then apply the saved
    /// shape. Split out of `restore_session` so it's unit-testable WITHOUT the
    /// session-cwd `chdir` (which `set_current_dir`s and would race the parallel
    /// test runner) — `open_second_commander_at` / `load_right_preview` don't
    /// touch the process cwd.
    pub(super) fn restore_vsplit(
        &mut self,
        sv: &crate::state::sessions::SavedVsplit,
        pane_focused: bool,
    ) {
        let mode = if sv.full_height {
            VsplitMode::FullHeight
        } else {
            VsplitMode::TopOnly
        };
        let focus = if sv.focus_right {
            Side::Right
        } else {
            Side::Left
        };
        let width_pct = sv.width_pct.clamp(20, 80); // clamp a hand-edited / older width
        // A column browsing a container has a cwd that is not a directory and
        // never was, so it fails the filter below, falls through to the preview
        // branch with no `preview_path`, and the blank-split guard drops the
        // whole split — silently. Land it on the directory the container is in,
        // cursor on the container itself: one `Enter` from where they were, and
        // a split they still have. (The left column remounts instead, but it can
        // afford to: `Effect::ChangeDir` acts on the focused column, so aiming
        // one at `b` would need the effect to be able to name a column.)
        let container = sv
            .right_cwd
            .as_ref()
            .filter(|p| !p.is_dir())
            .and_then(|p| super::archive::archive_ancestor_of(p))
            .map(|(archive, _)| archive);
        if let Some(archive) = container {
            let Some(parent) = archive.parent() else {
                return;
            };
            self.open_second_commander_at(parent);
            if let Some(v) = self.state.vsplit.as_mut() {
                v.width_pct = width_pct;
                v.mode = mode;
                v.focus = focus;
            }
            self.state.focus_on_path(&archive);
            self.state.focus = if pane_focused {
                Focus::Pane
            } else {
                Focus::FileList
            };
            self.state.flash_info(format!(
                "b was inside {} — reopened beside it",
                archive
                    .file_name()
                    .unwrap_or(archive.as_os_str())
                    .to_string_lossy()
            ));
            return;
        }
        if let Some(right_cwd) = sv.right_cwd.as_ref().filter(|p| p.is_dir()) {
            // PR G: reopen the second commander at its saved cwd (this sets
            // `state.right` + `vsplit` + git/harpoon + rows), then override the
            // split shape with the saved one (open_* uses defaults).
            self.open_second_commander_at(right_cwd);
            if let Some(v) = self.state.vsplit.as_mut() {
                v.width_pct = width_pct;
                v.mode = mode;
                v.focus = focus;
            }
            // `open_second_commander_at` forces `state.focus = FileList`;
            // re-apply the saved region focus (the pane block may have wanted
            // `Pane`).
            self.state.focus = if pane_focused {
                Focus::Pane
            } else {
                Focus::FileList
            };
        } else {
            self.state.vsplit = Some(VSplit {
                width_pct,
                mode,
                focus,
            });
            // Re-load the previewed file if it still exists (wraps to the
            // restored column width).
            if let Some(path) = sv.preview_path.as_ref().filter(|p| p.exists()) {
                self.load_right_preview(path);
            }
            // Don't restore a blank split: if the preview file is gone or
            // failed to load, there'd be a carved, empty right column with no
            // content. Match `cycle_vsplit`'s open-branch guard.
            if self.view.right_pager.is_none() {
                self.state.vsplit = None;
            }
        }
    }
}
