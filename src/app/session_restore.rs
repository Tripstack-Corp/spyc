//! Rebuild safe tabs and workspace layout without losing refused saved records,
//! and drive the `/resume` keystrokes that recover a Claude conversation.

use super::pane_tabs::TabSlot;
use super::state::restore::{DeferredTab, restored_active_index};
use super::state::{Focus, ProjectId, Side, VSplit, VsplitMode};
use super::{
    App, Effect, RESTORE_BANNER_SETTLE, RESTORE_RESUME_ENTER_DELAY, RESTORE_RESUME_VERIFY_DELAY,
    RESTORE_RESUME_VERIFY_RETRIES, RESTORE_RESUME_VERIFY_TAIL,
};
use crate::agent::ResumeAction;
use crate::state::sessions::{Held, Manifest, SavedProject, SavedTab};

impl App {
    /// Restore's spawn: [`Self::open_pane_tab_in_home`], as the saved tab's
    /// agent hint, taking back its saved `pane_id` (see `TabInfo::id`) unless
    /// it is empty or a live tab in any project already holds it.
    pub(super) fn open_restored_pane_tab(
        &mut self,
        cmd: &str,
        cwd: &std::path::Path,
        home: Option<&std::path::Path>,
        tab: &crate::state::sessions::SavedTab,
    ) -> bool {
        let pane_id = tab.pane_id.as_str();
        // Any project's: an MCP call finds its tab by id across them all.
        let taken = self.runtime.project_of_pane(pane_id).is_some();
        let reuse = (!pane_id.is_empty() && !taken).then_some(pane_id);
        self.spawn_pane_tab(cmd, cwd, TabSlot::Append, home, reuse, tab.agent_hint)
    }

    /// For tabs that have a `pending_resume_send` armed (set by
    /// `restore_session`), drive the two-phase keystroke injection
    /// that recovers a Claude conversation. We avoid the `--resume`
    /// CLI flag because it trips a known regression that crashes at
    /// mount; the slash-command path goes through `tM_` and works
    /// fine.
    ///
    /// Three phases:
    /// - `Text` (after banner-settle): write `/resume <sid>` with no
    ///   trailing Enter and transition to `Enter`.
    /// - `Enter` (after a small additional delay): write `\r` and
    ///   transition to `Verify`.
    /// - `Verify` (closed-loop): if `/resume <sid>` is still sitting
    ///   unsubmitted in the pane tail, re-send `\r` and re-arm, up to
    ///   `RESTORE_RESUME_VERIFY_RETRIES` times.
    ///
    /// Splitting the text/Enter writes narrows the race where
    /// Claude's TUI was still mid-render when the original combined
    /// `/resume <sid>\r` arrived, but no fixed delay closes it —
    /// async startup work (MCP connects, version check, org-message
    /// fetch) can remount the input component and eat a lone `\r`
    /// seconds after spawn. The `Verify` phase makes the submit
    /// observed rather than hoped-for: the sid vanishing from the
    /// screen tail is the success signal, and while it's visible a
    /// retry `\r` is harmless. (The v1.70 "bells" design subsumes
    /// this; delete the phase when that lands.)
    pub fn send_pending_resumes(&mut self, now: std::time::Instant) {
        use crate::pane::tabs::{PendingResumeSend, resume_still_unsubmitted};
        // Every project's tabs: a restored project the user isn't in still
        // resumes its conversations.
        let entries = self
            .runtime
            .every_tab_set_mut()
            .flat_map(|(_, tabs)| tabs.tabs_mut().iter_mut());
        for entry in entries {
            match entry.info.pending_resume_send.take() {
                Some(PendingResumeSend::Text { sid, after }) if now >= after => {
                    let _ = entry.pane.send_bytes(format!("/resume {sid}").as_bytes());
                    entry.info.pending_resume_send = Some(PendingResumeSend::Enter {
                        sid,
                        after: now + RESTORE_RESUME_ENTER_DELAY,
                    });
                }
                Some(PendingResumeSend::Enter { sid, after }) if now >= after => {
                    let _ = entry.pane.send_bytes(b"\r");
                    entry.info.pending_resume_send = Some(PendingResumeSend::Verify {
                        sid,
                        after: now + RESTORE_RESUME_VERIFY_DELAY,
                        retries_left: RESTORE_RESUME_VERIFY_RETRIES,
                    });
                }
                Some(PendingResumeSend::Verify {
                    sid,
                    after,
                    retries_left,
                }) if now >= after => {
                    let tail = entry.pane.recent_lines(RESTORE_RESUME_VERIFY_TAIL);
                    if resume_still_unsubmitted(&tail, &sid) && retries_left > 0 {
                        let _ = entry.pane.send_bytes(b"\r");
                        entry.info.pending_resume_send = Some(PendingResumeSend::Verify {
                            sid,
                            after: now + RESTORE_RESUME_VERIFY_DELAY,
                            retries_left: retries_left - 1,
                        });
                    }
                    // Submitted (sid gone from the tail) or retries
                    // exhausted: cleared by take(). On exhaustion the
                    // user recovers by pressing Enter themselves.
                }
                other => {
                    // Not yet due, or empty — put back what we took.
                    entry.info.pending_resume_send = other;
                }
            }
        }
    }

    fn spawn_restored_tab(
        &mut self,
        command: &str,
        cwd: &std::path::Path,
        tab: &crate::state::sessions::SavedTab,
    ) -> bool {
        let home = (tab.effective_kind() == crate::state::sessions::AgentKind::Codex)
            .then_some(tab.codex_home.as_deref())
            .flatten();
        self.open_restored_pane_tab(command, cwd, home, tab)
    }

    /// Restore every project in `manifest`: the one the user was in into this
    /// spyc's active project, and each of the others beside it, out of view,
    /// in the order the file saved them.
    ///
    /// The restore adopts the file (its id and name, so later saves overwrite
    /// it) only when this spyc carries everything in it: every project opened,
    /// and no other spyc still writing it. Otherwise it is a copy and the file
    /// is left as it was, so nothing the restore didn't open is lost. A copy of
    /// a running session also leaves that spyc's agents alone: their
    /// conversations, tab ids and scope claims stay with it, and the copy's
    /// agents start fresh. So does any tab that would resume into a
    /// conversation another running spyc is in, whichever file saved it
    /// ([`restore_plans`]): a copy leaves its file's pid behind, and restoring
    /// that file again would resume the conversations the copy runs.
    pub fn restore_session(&mut self, manifest: &Manifest) -> Vec<Effect> {
        let Some(active) = manifest.active() else {
            self.state
                .flash_error("session restore: the session has no projects");
            return Vec::new();
        };
        // The project the user was in comes back in view, else the first that
        // can: one whose directories have all gone (a worktree removed after
        // its merge) stays shut, and the rest still open.
        let Some(project) = std::iter::once(active)
            .chain(&manifest.projects)
            .find(|p| has_a_place(p))
        else {
            self.state
                .flash_error(format!("session dir gone: {}", active.anchor().display()));
            return Vec::new();
        };
        let running = manifest.running();
        let held = Held::by_running(&crate::state::sessions::load_manifests());
        let TabPlans {
            tabs: saved_tabs,
            plans,
            held: held_here,
        } = restore_plans(project, running, &held);
        // A wholly refused tab set leaves the current session intact. Mixed
        // sessions proceed, without ever spawning a refused command.
        if !plans.is_empty()
            && plans
                .iter()
                .all(|plan| matches!(plan.resume, ResumeAction::Refuse { .. }))
        {
            if let ResumeAction::Refuse { reason } = &plans[0].resume {
                self.state.flash_error(format!(
                    "session restore refused for all {} tabs: {reason}",
                    plans.len()
                ));
            }
            return Vec::new();
        }
        let Some(effects) = self.restore_column_a(project, true) else {
            return Vec::new();
        };
        self.restore_project(project, &saved_tabs, plans);
        let mut others = self.restore_other_projects(manifest, project, running, &held);
        others.held.extend(held_here);
        let adopt = !running && others.unopened.is_empty();
        if adopt {
            self.adopt_session(manifest);
        }
        // P2: restore the scope-coordination registry, once every
        // project's tabs are back — independent of tab-restore success. A
        // claim whose owning
        // tab failed to respawn (or whose save predates `claim_owner`) just
        // shows up "orphaned" on the registry / orchestration screen, still
        // informative and releasable rather than silently lost. A running
        // session's claims belong to its live agents, so a copy leaves them,
        // and a restore leaves those of each tab it starts fresh because
        // another spyc's agent is in its conversation.
        if !running {
            self.state.scope_registry = manifest
                .scope_claims
                .iter()
                .filter(|claim| claim.owner.is_empty() || !others.held.contains(&claim.owner))
                .cloned()
                .collect();
        }
        self.record_open_roots();
        self.flash_restored(manifest, running, &others);
        effects
    }

    /// Bring the active project back as `project` saved it: its settings, its
    /// tabs, any it can't open kept saved, unopened, and its split.
    fn restore_project(
        &mut self,
        project: &SavedProject,
        saved_tabs: &[SavedTab],
        plans: Vec<crate::agent::RestorePlan>,
    ) {
        self.restore_project_settings(project);
        if !saved_tabs.is_empty() {
            self.runtime.project_mut().pane_tabs = None;
            let mut restored = Vec::new();
            for (index, (tab, plan)) in saved_tabs.iter().zip(plans).enumerate() {
                self.restore_tab(index, tab, plan, project.anchor(), &mut restored);
            }
            if let Some(tabs) = self.runtime.project_mut().pane_tabs.as_mut() {
                tabs.switch_to(restored_active_index(&restored, project.active_tab));
            }
            let want_pane = project.pane_focused && !restored.is_empty();
            self.state.set_focus_pane_or_list(want_pane);
        }
        // Restore the vertical split (shape + previewed file). Independent of
        // the pane block above — a split can exist without a bottom pane.
        if let Some(sv) = &project.vsplit {
            self.restore_vsplit(sv, project.pane_focused);
        }
    }

    /// Open each project of `manifest` but `active`, the one restored into the
    /// project in view, beside it and out of view, then put every project in
    /// the order the file saved them.
    fn restore_other_projects(
        &mut self,
        manifest: &Manifest,
        active: &SavedProject,
        running: bool,
        held: &Held,
    ) -> RestoreOutcome {
        let mut order = Vec::new();
        let mut others = RestoreOutcome::default();
        for saved in &manifest.projects {
            if std::ptr::eq(saved, active) {
                order.push(self.state.active);
            } else if let Some(opened) = self.restore_out_of_view(saved, running, held) {
                order.push(opened.id);
                others.held.extend(opened.held);
                others.problems.extend(
                    opened
                        .problem
                        .map(|p| format!("{}: {p}", saved_label(saved))),
                );
            } else {
                others.unopened.push(saved_label(saved));
            }
        }
        self.state.order_projects(&order);
        others
    }

    /// Open `saved` as a project beside the one in view, its config read where
    /// it [`opens_at`], with its column `a`, tabs and split as it saved them,
    /// and its context file written so its agents' tools answer for it.
    /// `None` when none of those directories is left, or when an open project
    /// already has its home or its start directory, which are one project's
    /// each (`App::project_at_saved`).
    pub(super) fn restore_out_of_view(
        &mut self,
        saved: &SavedProject,
        running: bool,
        held: &Held,
    ) -> Option<OutOfView> {
        let at = std::fs::canonicalize(opens_at(saved)?).ok()?;
        if self.project_at_saved(saved).is_some() {
            return None;
        }
        let (id, config_problem) = self.create_project(&at, saved.anchor());
        let tabs = restore_plans(saved, running, held);
        let column_problem = self.in_project(id, |app| {
            let landed = app.restore_column_a(saved, false).is_some();
            app.reconcile_harpoon();
            app.restore_project(saved, &tabs.tabs, tabs.plans);
            app.write_context();
            // `in_project` keeps the flash from the user, so its cause is read
            // here.
            (!landed).then(|| {
                app.state.flash.as_ref().map_or_else(
                    || "column a could not open".to_owned(),
                    |flash| flash.text.clone(),
                )
            })
        });
        Some(OutOfView {
            id,
            problem: config_problem.or(column_problem),
            held: tabs.held,
        })
    }

    /// Land column `a` where the project left it: its own directory, else the
    /// project's anchor, which is all a version 1 file saved, else its start
    /// directory ([`landings`]). The first that is a directory, or a place
    /// inside an archive, wins, and the start directory (the `` ` `` target)
    /// comes back with it. `None`, flashed, when none is left.
    ///
    /// Inside an archive, the project in view gets back the effect that mounts
    /// it. `Effect::ChangeDir` acts on the project in view, so one restored out
    /// of view lands beside the container instead, cursor on it: one `Enter`
    /// from where it was.
    fn restore_column_a(&mut self, project: &SavedProject, in_view: bool) -> Option<Vec<Effect>> {
        let anchor = project.anchor();
        let start_dir = if project.start_dir.is_dir() {
            project.start_dir.clone()
        } else {
            anchor.to_path_buf()
        };
        let mut failure = None;
        for dir in landings(project) {
            if dir.is_dir() {
                match self.state.chdir_side(Side::Left, dir) {
                    Ok(()) => {
                        self.state.set_start_dir(&start_dir);
                        return Some(Vec::new());
                    }
                    Err(e) => failure = Some(format!("session chdir: {e:#}")),
                }
            } else if let Some((archive, _)) = super::archive::archive_ancestor_of(dir) {
                // The user quit while browsing an archive. It isn't a directory
                // and never was — the effect screen mounts it and lands the
                // column back where they left off. The rest of the restore
                // carries on meanwhile.
                self.state.set_start_dir(&start_dir);
                if in_view {
                    return Some(vec![Effect::ChangeDir {
                        path: dir.to_path_buf(),
                        focus: None,
                        on_ok: None,
                        err_prefix: "session chdir failed",
                    }]);
                }
                if let Some(parent) = archive.parent()
                    && self.state.chdir_side(Side::Left, parent).is_ok()
                {
                    self.state.focus_on_path(&archive);
                    return Some(Vec::new());
                }
            }
        }
        self.state.flash_error(
            failure.unwrap_or_else(|| format!("session dir gone: {}", anchor.display())),
        );
        None
    }

    /// Spawn one saved tab by its plan, or keep it saved, unopened, when the
    /// plan refuses it or the spawn fails.
    fn restore_tab(
        &mut self,
        index: usize,
        tab: &SavedTab,
        plan: crate::agent::RestorePlan,
        anchor: &std::path::Path,
        restored: &mut Vec<usize>,
    ) {
        if let ResumeAction::Refuse { reason } = &plan.resume {
            self.state
                .active_project_mut()
                .deferred_tabs
                .push(DeferredTab {
                    saved_index: index,
                    tab: tab.clone(),
                    reason: reason.clone(),
                });
            return;
        }
        let cwd = if tab.cwd.is_dir() { &tab.cwd } else { anchor };
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
        let spawned = self.spawn_restored_tab(&plan.command, cwd, tab);
        if spawned
            && let Some(tabs) = self.runtime.project_mut().pane_tabs.as_mut()
            && let Some(entry) = tabs.tabs_mut().last_mut()
        {
            restored.push(index);
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
                entry.info.pending_resume_send = Some(crate::pane::tabs::PendingResumeSend::Text {
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
            self.state
                .active_project_mut()
                .deferred_tabs
                .push(DeferredTab {
                    saved_index: index,
                    tab: tab.clone(),
                    reason,
                });
        }
    }

    /// Take `manifest`'s identity, when the restore adopts it.
    fn adopt_session(&mut self, manifest: &Manifest) {
        // Keep the startup-generated name when an older session file
        // has no name field; otherwise take the saved one.
        if !manifest.name.is_empty() {
            self.state.session_name = Some(manifest.name.clone());
        }
        // Continue overwriting the restored session's own `<id>.json` on
        // subsequent saves/autosaves (P3-2), rather than forking a new file.
        self.state.session_id = Some(manifest.id);
        // The file's closed projects are this spyc's to reopen and save now.
        self.state.adopt_closed(&manifest.closed_projects);
    }

    /// The restored project's own settings apart from its tabs and split.
    fn restore_project_settings(&mut self, project: &SavedProject) {
        let restored = self.state.active_project_mut();
        restored.deferred_tabs.clear();
        restored.home = project.home.clone().filter(|p| p.is_dir());
        restored.pane.pane_height_pct = project.pane_height_pct;
        if let Some(mode) = project.agent_hud {
            restored.pane.agent_hud.choice = Some(mode);
        }
    }

    /// What the restore came to, in one flash: whether it was a copy and why,
    /// then any tabs kept unopened and anything that went wrong opening a
    /// project, so none of them hides another.
    fn flash_restored(&mut self, manifest: &Manifest, running: bool, others: &RestoreOutcome) {
        let name = if manifest.name.is_empty() {
            "(unnamed)"
        } else {
            manifest.name.as_str()
        };
        let total = manifest.projects.len();
        let headline = if running {
            format!(
                "restored a copy of {name}, running in pid {} — its agents start fresh here",
                manifest.pid.unwrap_or_default()
            )
        } else if !others.unopened.is_empty() {
            format!(
                "restored {} of {total} projects of {name} as a copy, without {}: the session keeps them all",
                total - others.unopened.len(),
                others.unopened.join(", ")
            )
        } else if total > 1 {
            format!("session restored: {total} projects")
        } else {
            "session restored".to_owned()
        };
        let unopened: usize = self
            .state
            .open_projects()
            .map(|p| p.deferred_tabs.len())
            .sum();
        let mut parts = vec![headline];
        if !others.held.is_empty() {
            parts.push(format!(
                "{} tab(s) start fresh: another spyc has their conversations",
                others.held.len()
            ));
        }
        if unopened > 0 {
            parts.push(format!(
                "{unopened} tab(s) unopened, kept saved — session info shows reasons"
            ));
        }
        parts.extend(others.problems.iter().cloned());
        let text = parts.join("; ");
        if parts.len() > 1 {
            self.state.flash_error(text);
        } else {
            self.state.flash_info(text);
        }
    }

    /// Restore a saved vertical split: reopen the second commander at its saved
    /// cwd (PR G) or re-load the Stage-1 preview file, then apply the saved
    /// shape. Split out of `restore_project` so it's unit-testable WITHOUT the
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
            if let Some(v) = self.state.active_project_mut().vsplit.as_mut() {
                v.width_pct = width_pct;
                v.mode = mode;
                v.focus = focus;
            }
            self.state.focus_on_path(&archive);
            self.state.active_project_mut().focus = if pane_focused {
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
            if let Some(v) = self.state.active_project_mut().vsplit.as_mut() {
                v.width_pct = width_pct;
                v.mode = mode;
                v.focus = focus;
            }
            // `open_second_commander_at` forces `state.focus = FileList`;
            // re-apply the saved region focus (the pane block may have wanted
            // `Pane`).
            self.state.active_project_mut().focus = if pane_focused {
                Focus::Pane
            } else {
                Focus::FileList
            };
        } else {
            self.state.active_project_mut().vsplit = Some(VSplit {
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
            if self.view.project().right_pager.is_none() {
                self.state.active_project_mut().vsplit = None;
            }
        }
    }
}

/// What restoring a manifest came to: the labels of the projects that didn't
/// open, each config problem of those that did, and, in every project, the
/// claim owners of the tabs that started fresh because another spyc's agent
/// is in their conversations ([`TabPlans::held`]), one entry per tab.
#[derive(Default)]
struct RestoreOutcome {
    unopened: Vec<String>,
    problems: Vec<String>,
    held: Vec<String>,
}

/// A project [`App::restore_out_of_view`] opened: its id, what went wrong
/// opening it, if anything, and its [`TabPlans::held`].
pub(super) struct OutOfView {
    pub(super) id: ProjectId,
    pub(super) problem: Option<String>,
    pub(super) held: Vec<String>,
}

/// A project's tabs as the restore opens them, each with how it comes back.
#[derive(Default)]
struct TabPlans {
    tabs: Vec<SavedTab>,
    plans: Vec<crate::agent::RestorePlan>,
    /// The saved claim owner of each tab that starts fresh because another
    /// spyc's agent is in its conversation; empty for one saved without.
    held: Vec<String>,
}

/// Where column `a` may land, in order: its own directory, the project's
/// anchor, its start directory.
fn landings(project: &SavedProject) -> impl Iterator<Item = &std::path::Path> {
    project
        .left_cwd
        .as_deref()
        .into_iter()
        .chain([project.anchor(), project.start_dir.as_path()])
}

/// Whether column `a` has somewhere to land: one of its [`landings`] is a
/// directory, or a place inside an archive.
fn has_a_place(project: &SavedProject) -> bool {
    landings(project).any(|dir| dir.is_dir() || super::archive::archive_ancestor_of(dir).is_some())
}

/// The directory a project opened out of view opens at: its start directory,
/// else its anchor, else column `a`'s, the first still a directory.
pub(super) fn opens_at(project: &SavedProject) -> Option<&std::path::Path> {
    [project.start_dir.as_path(), project.anchor()]
        .into_iter()
        .chain(project.left_cwd.as_deref())
        .find(|dir| dir.is_dir())
}

/// A saved project's name for the user: its label, else its anchor.
pub(super) fn saved_label(project: &SavedProject) -> String {
    if project.label.is_empty() {
        project.anchor().display().to_string()
    } else {
        project.label.clone()
    }
}

/// `project`'s tabs as the restore opens them, each by the plan its agent
/// rebuilds for it in the directory it spawns in. A tab the plan refuses stays
/// refused, its record whole. Any other tab starts fresh, with nothing resumed
/// ([`crate::agent::resume::fresh_command`]), when the session is still
/// running, or when its plan would resume into something `held`: a second
/// process in a conversation.
///
/// SPYC-TRAP(restore-skips-held-conversations): a resumed tab whose
/// conversation another spyc runs splits it silently, so the check reads the
/// plan, not the id alone: an id-less codex or zot resumes the newest.
fn restore_plans(project: &SavedProject, running: bool, held: &Held) -> TabPlans {
    let mut out = TabPlans::default();
    for tab in &project.tabs {
        let cwd = if tab.cwd.is_dir() {
            &tab.cwd
        } else {
            project.anchor()
        };
        let plan = crate::agent::resume::reconstruct_saved_tab(tab, cwd);
        let refused = matches!(plan.resume, ResumeAction::Refuse { .. });
        let is_held = !running && !refused && resumes(&plan, tab) && held.holds(tab, cwd);
        if is_held {
            out.held.push(tab.claim_owner.clone());
        }
        let fresh = (running || is_held) && !refused;
        out.tabs
            .push(if fresh { fresh_tab(tab) } else { tab.clone() });
        out.plans.push(if fresh {
            crate::agent::RestorePlan {
                command: crate::agent::resume::fresh_command(tab),
                resume: ResumeAction::None,
            }
        } else {
            plan
        });
    }
    out
}

/// Whether `plan` resumes a conversation: claude's by typing `/resume`, the
/// others' by a resume its rebuilt command has that a fresh start's doesn't.
fn resumes(plan: &crate::agent::RestorePlan, tab: &SavedTab) -> bool {
    match &plan.resume {
        ResumeAction::ClaudeStdin { .. } => true,
        ResumeAction::None => plan.command != crate::agent::resume::fresh_command(tab),
        ResumeAction::Refuse { .. } => false,
    }
}

/// `tab` as a restore that starts it fresh keeps it, a copy of a running
/// session's or one whose conversation another spyc's agent is in: without
/// the conversation, tab id and claim owner that agent still has.
fn fresh_tab(tab: &SavedTab) -> SavedTab {
    SavedTab {
        agent_session_id: None,
        agent_session_name: None,
        pane_id: String::new(),
        claim_owner: String::new(),
        ..tab.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID: &str = "019e8b21-9e7c-7553-a118-d1cdada725fd";

    fn tab(command: &str, hint: Option<&str>) -> SavedTab {
        serde_json::from_value(serde_json::json!({
            "command": command, "label": "t", "cwd": "/tmp", "agent_kind": "codex",
            "agent_session_id": UUID, "codex_home": "/tmp/codex-home", "agent_hint": hint
        }))
        .expect("fixture")
    }

    /// A plan resumes when it adds a resume to the fresh start, even one an
    /// older save kept in its command, and a wrapper's never does.
    #[test]
    fn a_plan_resumes_when_it_adds_a_resume_to_the_fresh_start() {
        let at = std::path::Path::new("/tmp");
        for command in ["codex", &format!("codex resume {UUID}")] {
            let saved = tab(command, None);
            let plan = crate::agent::resume::reconstruct_saved_tab(&saved, at);
            assert!(resumes(&plan, &saved), "{command}: {}", plan.command);
        }
        let wrapper = tab("my-codex", Some("codex"));
        let plan = crate::agent::resume::reconstruct_saved_tab(&wrapper, at);
        assert!(!resumes(&plan, &wrapper), "{}", plan.command);
    }
}
