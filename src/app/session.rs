//! Session save / restore and the session UI: `save_session` (serialize
//! tabs + agent session ids on quit), `restore_session` (rebuild tabs +
//! cwd from a saved `Session`), `show_session_picker` (the `-r` picker
//! pager), `show_session_info` (the session-info overlay), `request_quit`
//! (the quit lifecycle), and the history-popup helpers
//! (`show_history_popup` / `show_jump_history_popup` /
//! `sync_history_editor_to_cursor` + `HIST_PREFIX_W`).
//!
//! Same child-module `impl App` pattern: reads App's private state via the
//! descendant-module rule. These are all `pub`, called from the run
//! loop / `commands` / `key_dispatch` / `pager_handler` / `actions`.
//! Worktree helpers moved to `git_state.rs` and pane-sizing helpers to
//! `pane_tabs.rs`.

use crate::pane::PaneTabs;
use crate::state::sessions::{AgentKind, Manifest, SavedProject, SavedTab, SavedVsplit};
use crate::ui::line_edit::LineEditor;
use crate::ui::pager::{self, PagerView};

use std::time::{Duration, Instant};

use super::state::{ProjectId, Side, VsplitMode};
use super::{App, Deadline, RunCtx};

/// P3-2 debounce window: persist this long after the last session-relevant
/// change. Bounds SIGKILL data loss to ~this window while coalescing bursts of
/// navigation / tab churn — short enough to be recovery-sufficient, long enough
/// not to write on every keystroke.
const AUTOSAVE_DEBOUNCE: Duration = Duration::from_secs(2);

/// What `settle_autosave` should do this loop iteration — the pure half of the
/// debounce, unit-tested below.
#[derive(Debug, PartialEq, Eq)]
enum AutosaveAction {
    /// Nothing to persist (no restorable state, or unchanged since the last
    /// save): clear any pending save and disarm.
    Idle,
    /// A change is pending and no debounce is running yet: arm at this instant.
    Arm(Instant),
    /// The debounce window elapsed: persist now.
    Fire,
    /// A change is pending but the window hasn't elapsed: keep waiting.
    Wait,
}

/// Pure autosave debounce decision. `dirty` = there is restorable state whose
/// fingerprint differs from the last-saved one; `due` = the armed fire instant
/// (if any). Kept free of `self` so it's testable in isolation (the
/// `route.rs` / `focus.rs` pure-decision template).
fn autosave_action(dirty: bool, due: Option<Instant>, now: Instant) -> AutosaveAction {
    if !dirty {
        return AutosaveAction::Idle;
    }
    match due {
        None => AutosaveAction::Arm(now + AUTOSAVE_DEBOUNCE),
        Some(d) if now >= d => AutosaveAction::Fire,
        Some(_) => AutosaveAction::Wait,
    }
}

/// The conversation a tab is running, and its name, as a save persists it.
///
/// Prefers the tab's PINNED session id — claude's `live_session_id` (set at
/// restore from the exact `/resume <sid>`) or codex's `codex_session_id` (the
/// rollout `codex_pin` claimed for it). That's the conversation this pane is
/// definitively running, so it bypasses the spawn-proximity resolver that
/// crosses panes restored together. Otherwise falls back to the profile
/// resolver, whose exit-banner read finds nothing on a *live* tab, since both
/// agents print their id only on the way out.
///
/// Never returns an id in `claimed`. Two tabs pinned to one conversation would
/// otherwise both save it and both restore into it, which is the collapse this
/// exists to prevent; `^a F` passes the other tabs' ids for the same reason.
pub(super) fn tab_conversation(
    tab: &crate::pane::tabs::TabEntry,
    claimed: &std::collections::HashSet<String>,
) -> (Option<String>, Option<String>) {
    let profile = tab.info.agent();
    match tab
        .info
        .pinned_session_id()
        .filter(|id| !claimed.contains(*id))
        .and_then(|id| profile.validate_live_session_id(&tab.info.cwd, id))
    {
        Some((id, name)) => (Some(id), name),
        None => profile.resolve_resume_target(
            &tab.pane,
            &tab.info.cwd,
            tab.info.agent_epoch_secs(),
            claimed,
        ),
    }
}

/// One `-r` picker row. With one project it reads as a session always has: the
/// project's path, its tab labels and its agents' conversations. With several,
/// the projects take the path's place. `running_column` makes room for the
/// `running` mark when any row carries it.
fn picker_row(index: usize, manifest: &Manifest, running: bool, running_column: bool) -> String {
    let age = crate::state::sessions::format_relative_time(manifest.epoch_secs);
    let name = if manifest.name.is_empty() {
        "(unnamed)"
    } else {
        manifest.name.as_str()
    };
    let status = if running_column {
        format!("{:<9}", if running { "running" } else { "" })
    } else {
        String::new()
    };
    let place = match manifest.projects.as_slice() {
        [project] => project_row(project),
        projects => crate::state::sessions::project_list_label(projects),
    };
    format!("  [{}]  {name:<22} {status}{age:<14} {place}", index + 1)
}

/// A one-project row's place: the project's path, its tab labels, and the
/// conversations its agents were in.
fn project_row(project: &SavedProject) -> String {
    use crate::state::sessions;
    let tab_count = project.tabs.len();
    let names: Vec<&str> = project.tabs.iter().map(|t| t.label.as_str()).collect();
    // Show agent session info (claude/codex) for tabs that have it.
    // Picker tooltips group by kind so a session with mixed
    // claude+codex panes is legible at a glance.
    let agent_info: Vec<String> = project
        .tabs
        .iter()
        .filter_map(|t| {
            let sid = t.agent_session_id.as_deref()?;
            let short_id = sessions::short_id(sid);
            let kind = t.effective_kind();
            if kind == AgentKind::Other {
                return None;
            }
            Some(
                crate::agent::profile_for(kind)
                    .picker_label(&short_id, t.agent_session_name.as_deref()),
            )
        })
        .collect();
    let tab_info = if tab_count == 0 {
        String::new()
    } else {
        format!("  [{}]", names.join(", "))
    };
    let agent_suffix = if agent_info.is_empty() {
        String::new()
    } else {
        format!("  {}", agent_info.join(", "))
    };
    format!("{}{tab_info}{agent_suffix}", project.anchor().display())
}

/// Conversations already saved for an earlier tab, by the codex home they live
/// under (see `App::saved_tabs`).
type ConversationClaims =
    std::collections::HashMap<Option<std::path::PathBuf>, std::collections::HashSet<String>>;

impl App {
    pub fn save_session(&mut self) {
        // SPYC-TRAP(session-prune-by-last-save): every file holds a picker slot,
        // so an empty quit writes nothing new. An existing file is still
        // overwritten, or tabs closed before quitting would come back on `-r`.
        if !self.has_restorable_state()
            && !self
                .state
                .session_id
                .is_some_and(crate::state::sessions::session_exists)
        {
            self.exit_summary = Some("no session saved — no tabs or split to restore".into());
            return;
        }
        let manifest = self.build_manifest();
        let save_result = crate::state::sessions::save_session(&manifest);
        // A successful save resets the autosave baseline so a follow-up
        // `settle_autosave` doesn't immediately re-write the identical session.
        if save_result.is_ok() {
            self.runtime.autosave_last_saved_fp = Some(self.session_fingerprint());
        }
        self.build_exit_summary(&manifest, &save_result);
    }

    /// P3-2 debounced autosave: persist the current session silently (no exit
    /// summary), returning whether the write succeeded. Called by
    /// [`Self::settle_autosave`] when the debounce window elapses.
    fn autosave_session(&mut self) -> bool {
        let manifest = self.build_manifest();
        crate::state::sessions::save_session(&manifest).is_ok()
    }

    /// Build the [`Manifest`] from live state: every open project, in switcher
    /// order, each read inside it so the column and pane handles name its own.
    /// Shared by the quit-time [`Self::save_session`] and the debounced
    /// [`Self::autosave_session`].
    fn build_manifest(&mut self) -> Manifest {
        // Stable per-process session id (see `AppState::session_id`) so every
        // save overwrites one `<id>.json`; fresh-millis fallback only if it was
        // somehow never assigned.
        let id = self
            .state
            .session_id
            .unwrap_or_else(|| (crate::sysinfo::epoch_nanos() / 1_000_000) as u64);
        debug_assert!(
            !self.state.in_background,
            "a save starts in the user's project"
        );
        let users = self.state.active;
        let mut claims = ConversationClaims::new();
        let mut projects = Vec::new();
        for project in self.open_project_ids() {
            projects.push(self.in_project(project, |app| app.saved_project(&mut claims)));
        }
        Manifest {
            version: crate::state::sessions::MANIFEST_VERSION,
            id,
            epoch_secs: crate::sysinfo::epoch_secs(),
            saved_at: crate::sysinfo::format_now(),
            name: self.state.session_name.clone().unwrap_or_default(),
            pid: Some(std::process::id()),
            process_started: crate::proc_group::own_start_secs(),
            active_project: Some(users),
            projects,
            scope_claims: self.state.scope_registry.clone(),
        }
    }

    /// Every open project's id, in switcher order.
    fn open_project_ids(&self) -> Vec<ProjectId> {
        self.state.open_projects().map(|p| p.id).collect()
    }

    /// The active project as the manifest saves it, column `a`'s directory
    /// included.
    fn saved_project(&mut self, claims: &mut ConversationClaims) -> SavedProject {
        let tabs = self.saved_tabs(claims);
        // SPYC-TRAP(partial-restore-keeps-saved-tabs): autosave overwrites the
        // original session, so unopened entries must survive every snapshot.
        let live_active = self
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .map_or(0, PaneTabs::active_index);
        let (tabs, active_tab) = super::state::restore::merge_saved_tabs(
            tabs,
            live_active,
            &self.state.active_project().deferred_tabs,
        );
        let vsplit = self.saved_vsplit();
        let pane_focused = self.state.pane_focused();
        let left_cwd = self.state.col(Side::Left).listing.dir.clone();
        let project = self.state.active_project();
        SavedProject {
            id: project.id,
            home: project.home.clone(),
            label: project.label(),
            start_dir: project.start_dir.clone(),
            left_cwd: Some(left_cwd),
            vsplit,
            pane_height_pct: project.pane.pane_height_pct,
            pane_focused,
            tabs,
            active_tab,
            agent_hud: project.pane.agent_hud.choice,
        }
    }

    /// The active project's live tabs, each with the conversation it runs.
    ///
    /// `claims` holds the session IDs already assigned to earlier tabs, in any
    /// project, so each Claude/Codex pane gets a distinct `agent_session_id`
    /// even when several tabs share a cwd. Without this, the resolver's
    /// "most-recent JSONL for cwd" fallback handed every Claude
    /// pane the same ID and they all collapsed onto one
    /// conversation at restore.
    fn saved_tabs(&mut self, claims: &mut ConversationClaims) -> Vec<SavedTab> {
        self.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .map(|pt| {
                pt.tabs_mut()
                    .iter_mut()
                    .map(|t| {
                        // A shell tab running an agent is saved as its shell:
                        // restore relaunches the command, and the agent's
                        // conversation isn't the shell's to resume.
                        let profile = t.info.launched_agent();
                        let kind = profile.kind();
                        let claimed = claims.entry(t.info.codex_home.clone()).or_default();
                        let (agent_session_id, agent_session_name) =
                            if kind == t.info.agent().kind() {
                                tab_conversation(t, claimed)
                            } else {
                                (None, None)
                            };
                        if let Some(ref id) = agent_session_id {
                            claimed.insert(id.clone());
                        }
                        // The sid lives in agent_session_id; baking
                        // --resume / `resume` into `command` would survive
                        // past a resolver miss and pollute the next restore.
                        // A wrapper's command is never edited (#610).
                        let saved_command = if t.info.wraps_agent() {
                            t.info.command.clone()
                        } else {
                            profile.command_without_resume(&t.info.command)
                        };
                        SavedTab {
                            command: saved_command,
                            // Strip any `[exited N]` suffix — that's
                            // runtime display state for a tab whose
                            // child has died, not persistent identity.
                            // Without this, restoring a session that
                            // saw a tab exit at any point shows the
                            // freshly-respawned process tagged with
                            // a stale "exited" suffix.
                            label: crate::pane::tabs::strip_exit_suffix(&t.info.label),
                            cwd: t.info.cwd.clone(),
                            agent_kind: kind,
                            agent_session_id,
                            agent_session_name,
                            codex_home: t.info.codex_home.clone(),
                            codex_home_unresolved: kind == crate::state::sessions::AgentKind::Codex
                                && t.info.codex_home.is_none(),
                            claim_owner: t.info.claim_owner.clone(),
                            pane_id: t.info.id.clone(),
                            agent_hint: t.info.agent_hint,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Persist the open vertical split's shape + its content key: a
    /// second *commander*'s cwd (`right_cwd`, reopened on restore — PR G)
    /// or a Stage-1 *preview* file (`preview_path`). The two are mutually
    /// exclusive (a commander clears `right_pager`).
    fn saved_vsplit(&self) -> Option<SavedVsplit> {
        self.state.active_project().vsplit.map(|v| SavedVsplit {
            width_pct: v.width_pct,
            full_height: matches!(v.mode, VsplitMode::FullHeight),
            focus_right: matches!(v.focus, Side::Right),
            preview_path: self
                .view
                .project()
                .right_pager
                .as_ref()
                .and_then(|p| p.source_path.clone()),
            right_cwd: self
                .state
                .get_col(Side::Right)
                .map(|c| c.listing.dir.clone()),
        })
    }

    /// Post-TUI exit summary line (`self.exit_summary`) from a just-saved
    /// session + its write result. Quit-path only — the debounced autosave
    /// persists silently.
    fn build_exit_summary(&mut self, manifest: &Manifest, save_result: &std::io::Result<()>) {
        // Report the write result truthfully — a failed write (disk full,
        // unwritable state dir) must not tell the user their session was safe
        // when `spyc -r` would later find nothing.
        let place = match manifest.projects.as_slice() {
            [project] => crate::paths::display_tilde(project.anchor()),
            projects => crate::state::sessions::project_list_label(projects),
        };
        let tabs: Vec<&SavedTab> = manifest.projects.iter().flat_map(|p| &p.tabs).collect();
        let tab_count = tabs.len();
        let saved_ok = save_result.is_ok();
        let mut parts = match save_result {
            Ok(()) => vec![format!("session saved — {place}")],
            Err(e) => vec![format!("session NOT saved ({e}) — {place}")],
        };
        if tab_count > 0 {
            parts.push(format!(
                "{tab_count} pane tab{}",
                if tab_count == 1 { "" } else { "s" }
            ));
        }
        // Per-agent session summary, in registry order. `Names` lists
        // human-readable session names (claude); `Count` reports how
        // many panes captured a session id (codex/agy); `None` agents
        // are omitted.
        for profile in crate::agent::REGISTRY {
            let kind = profile.kind();
            match profile.exit_summary_mode() {
                crate::agent::ExitSummaryMode::Names => {
                    let names: Vec<String> = tabs
                        .iter()
                        .filter(|t| t.effective_kind() == kind)
                        .filter_map(|t| t.agent_session_name.clone())
                        .collect();
                    if !names.is_empty() {
                        parts.push(format!("{}: {}", profile.name(), names.join(", ")));
                    }
                }
                crate::agent::ExitSummaryMode::Count => {
                    let count = tabs
                        .iter()
                        .filter(|t| t.effective_kind() == kind && t.agent_session_id.is_some())
                        .count();
                    if count > 0 {
                        parts.push(format!(
                            "{}: {count} session{}",
                            profile.name(),
                            if count == 1 { "" } else { "s" }
                        ));
                    }
                }
                crate::agent::ExitSummaryMode::None => {}
            }
        }
        // Only advertise `spyc -r` when there's actually something to restore.
        if saved_ok {
            parts.push("restore with spyc -r".to_string());
        }
        self.exit_summary = Some(parts.join(" · "));
    }

    /// Cheap structural fingerprint of the session-relevant state, so
    /// [`Self::settle_autosave`] detects a genuine change without a full
    /// snapshot build (which resolves agent resume ids off the filesystem).
    /// Must track what [`Self::build_manifest`] persists, in every project.
    fn session_fingerprint(&mut self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for project in self.open_project_ids() {
            project.hash(&mut h);
            self.in_project(project, |app| app.hash_project(&mut h));
        }
        self.state.active.hash(&mut h);
        // P2: a scope-registry mutation (register/release) is session-relevant
        // — it's exactly what must survive a crash mid-merge-train.
        self.state.scope_registry.hash(&mut h);
        h.finish()
    }

    /// The active project's part of [`Self::session_fingerprint`].
    fn hash_project(&self, h: &mut std::collections::hash_map::DefaultHasher) {
        use std::hash::Hash;
        if let Some(pt) = self.runtime.project().pane_tabs.as_ref() {
            pt.active_index().hash(h);
            for t in pt.tabs() {
                t.info.command.hash(h);
                t.info.cwd.hash(h);
                // A pin usually lands after the tab's first save, which then
                // recorded no conversation, or a guessed one (#584).
                t.info.pinned_session_id().hash(h);
                t.info.codex_home.hash(h);
                // `^a R` respawns in place with the same command and a new id.
                t.info.id.hash(h);
            }
        }
        let project = self.state.active_project();
        super::state::restore::hash_deferred_tabs(&project.deferred_tabs, h);
        project.home.hash(h);
        project.start_dir.hash(h);
        project.pane.pane_height_pct.hash(h);
        self.state.pane_focused().hash(h);
        if let Some(v) = project.vsplit {
            v.width_pct.hash(h);
            matches!(v.mode, VsplitMode::FullHeight).hash(h);
            matches!(v.focus, Side::Right).hash(h);
        }
        self.state.col(Side::Left).listing.dir.hash(h);
        if let Some(c) = self.state.get_col(Side::Right) {
            c.listing.dir.hash(h);
        }
        project.pane.agent_hud.choice.hash(h);
    }

    /// Whether a restore would bring anything back: any project's tabs (open
    /// or kept unopened) or split, or scope claims. A cwd alone doesn't count.
    fn has_restorable_state(&mut self) -> bool {
        !self.state.scope_registry.is_empty()
            || self
                .open_project_ids()
                .into_iter()
                .any(|project| self.in_project(project, |app| app.project_has_restorable_state()))
    }

    fn project_has_restorable_state(&self) -> bool {
        self.runtime
            .project()
            .pane_tabs
            .as_ref()
            .is_some_and(|pt| !pt.tabs().is_empty())
            || !self.state.active_project().deferred_tabs.is_empty()
            || self.state.active_project().vsplit.is_some()
    }

    /// P3-2 crash-sufficient autosave (PRE-recv settle). Debounce: on a
    /// session-relevant change arm `Deadline::Autosave` `AUTOSAVE_DEBOUNCE` out
    /// (re-armed while changes keep landing); when the quiet window elapses,
    /// persist. Only armed while dirty ⇒ a clean/idle session wakes nothing
    /// (0 dps). Never needs a redraw.
    pub(crate) fn settle_autosave(&mut self, now: Instant, ctx: &mut RunCtx) {
        // Nothing worth restoring (bare launch: no tabs, no split, no scope
        // claims) ⇒ never write an empty session; stay disarmed.
        let dirty = self.has_restorable_state()
            && Some(self.session_fingerprint()) != self.runtime.autosave_last_saved_fp;
        match autosave_action(dirty, self.runtime.autosave_due, now) {
            AutosaveAction::Idle => {
                self.runtime.autosave_due = None;
                ctx.scheduler.disarm(Deadline::Autosave);
            }
            AutosaveAction::Arm(when) => {
                self.runtime.autosave_due = Some(when);
                ctx.scheduler.arm(Deadline::Autosave, when);
            }
            AutosaveAction::Wait => {}
            AutosaveAction::Fire => {
                self.autosave_session();
                self.runtime.autosave_last_saved_fp = Some(self.session_fingerprint());
                self.runtime.autosave_due = None;
                ctx.scheduler.disarm(Deadline::Autosave);
            }
        }
    }

    pub fn show_session_picker(&mut self) {
        use crate::state::sessions;
        let manifests = sessions::load_manifests();
        if manifests.is_empty() {
            self.state.flash_info("no saved sessions");
            return;
        }
        let running: Vec<bool> = manifests.iter().map(Manifest::running).collect();
        let running_column = running.contains(&true);
        let lines: Vec<String> = manifests
            .iter()
            .zip(&running)
            .enumerate()
            .map(|(i, (manifest, &running))| picker_row(i, manifest, running, running_column))
            .collect();
        self.state.pending_sessions = Some(manifests);
        let mut all_lines = vec!["  [n]  new session".to_string()];
        all_lines.extend(lines);
        let mut view = pager::PagerView::new_plain(
            "sessions — j/k navigate, Enter restore, n new, q close",
            all_lines,
        );
        // Opens on the newest session.
        view.picker_cursor = Some(Self::SESSION_PICKER_HEADER_ROWS);
        self.set_pager(view);
    }

    /// Rows above the first session in the `-r` picker: `[n] new session`.
    /// The cursor can stop on any row, so the picker has no spacer.
    pub(super) const SESSION_PICKER_HEADER_ROWS: usize = 1;

    pub fn show_session_info(&mut self) {
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("\u{1f336}\u{fe0f} spyc {}", crate::VERSION));
        lines.push(format!("session  : {}", self.state.session_display()));
        lines.push(format!("project  : {}", self.state.project_home_display()));
        lines.push(format!("user@host: {}", self.state.user_host));
        lines.push(format!(
            "start dir: {}",
            crate::paths::display_tilde(&self.state.active_project().start_dir)
        ));
        lines.push(format!("pid      : {}", std::process::id()));
        lines.push(format!(
            "cwd      : {}",
            crate::paths::display_tilde(&self.state.cur().listing.dir)
        ));
        let col = self.state.cur();
        lines.push(format!("entries  : {}", col.listing.entries.len()));
        lines.push(format!("visible  : {}", col.rows.len()));
        lines.push(format!("picks    : {}", col.picks.len()));
        lines.push(format!("inventory: {}", self.state.inventory.len()));
        lines.push(format!("marks    : {}", self.state.marks.entries.len()));
        lines.push(format!("rss      : {}", crate::sysinfo::format_rss()));
        lines.push(format!("time     : {}", crate::sysinfo::format_now()));
        let deferred_tabs = &self.state.active_project().deferred_tabs;
        if !deferred_tabs.is_empty() {
            lines.push(String::new());
            lines.push("unopened tabs (kept saved):".into());
            for deferred in deferred_tabs {
                lines.push(format!(
                    "  saved tab {} ({}) — {}",
                    deferred.saved_index + 1,
                    deferred.tab.label,
                    deferred.reason
                ));
                lines.push(format!("    {}", deferred.tab.command));
            }
        }
        if !self.state.active_project().config.sources.is_empty() {
            lines.push(String::new());
            lines.push("config sources:".into());
            for src in &self.state.active_project().config.sources {
                lines.push(format!("  {}", crate::paths::display_tilde(src)));
            }
        }
        self.view.project_mut().pager = Some(PagerView::new_plain("session info", lines));
    }

    /// Run the canonical quit lifecycle: first call arms a 2-second
    /// confirm window (and flashes any running-process count); a
    /// second call inside that window persists the session and sets
    /// `should_quit`. Shared by `Action::Quit` (the Q / ^D keybindings)
    /// and the `:q` / `:quit` command — both paths must save and warn
    /// identically.
    pub fn request_quit(&mut self) {
        let now = std::time::Instant::now();
        if self
            .state
            .quit_pending
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(2))
        {
            // If a file pager is open, capture its scroll before
            // shutdown so reopening the file in the next session
            // resumes where we left off. Bypassed close paths
            // (typically session save → quit, no Esc) would
            // otherwise drop the in-memory scroll on the floor.
            self.remember_pager_position();
            self.save_session();
            self.state.should_quit = true;
        } else {
            self.state.quit_pending = Some(now);
            // Every project's tabs: quitting ends them all.
            let running_panes: usize = self
                .runtime
                .every_tab_set()
                .map(|(_, tabs)| tabs.tabs().iter().filter(|e| !e.pane.is_closed()).count())
                .sum();
            let running_bg = self.runtime.background_tasks.running_count();
            let running = running_panes + running_bg;
            // Pending archive changes live only in memory, so quitting drops
            // them. Name that on the first tap, where the double-tap confirm
            // already gives the user somewhere to stop.
            self.scan_archive_edits();
            let dirty_archives = self.dirty_mounts();
            if !dirty_archives.is_empty() {
                self.state.flash_info(format!(
                    "{} archive(s) have unwritten changes (:archive write) — press again to quit",
                    dirty_archives.len()
                ));
            } else if running > 0 {
                self.state.flash_info(format!(
                    "{running} running process{} — press again to quit",
                    if running == 1 { "" } else { "es" }
                ));
            } else {
                self.state.flash_info("press again to quit");
            }
        }
    }

    /// Prefix width for history editor lines: "  NNN  " = 7 chars.
    pub const HIST_PREFIX_W: usize = 7;

    /// Sync the history editor after moving the picker cursor to a new line.
    /// Updates the LineEditor content and the display line.
    pub fn sync_history_editor_to_cursor(&mut self) {
        if let (Some(view), Some(editor)) = self.view.top_pager_and_history_pick_mut() {
            Self::show_history_selection(view, editor, &self.state.history, true);
        }
    }

    /// Load the selected command into the history editor and show it on its
    /// row. `keep_mode` leaves the editor in the mode it was in; otherwise it
    /// ends up in Insert mode.
    pub(super) fn show_history_selection(
        view: &mut pager::PagerView,
        editor: &mut LineEditor,
        history: &crate::state::history::History,
        keep_mode: bool,
    ) {
        let selected = view.picker_cursor.unwrap_or(0);
        let entries = history.entries();
        // Rows are newest first, history is oldest first.
        if let Some(cmd) = entries.get(entries.len().saturating_sub(1 + selected)) {
            if keep_mode {
                editor.set_content_keep_mode(cmd);
            } else {
                editor.set_content(cmd);
            }
        }
        Self::show_edited_history_row(view, editor);
    }

    /// Show the editor's text on the selected row, with the editor's cursor.
    pub(super) fn show_edited_history_row(view: &mut pager::PagerView, editor: &LineEditor) {
        let selected = view.picker_cursor.unwrap_or(0);
        let text = format!("  {:>3}  {}", selected + 1, editor.text());
        view.lines[selected] = ratatui::text::Line::from(text);
        view.picker_edit_cursor = Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));
    }

    /// Open a popup listing every entry in `jump_history`, newest at
    /// the top. j/k navigate, Enter chdirs to the cursored path,
    /// ^D deletes the entry from history, q/Esc closes. Two triggers:
    /// `?` on an empty `J` prompt (spy parity, the short reflex), or
    /// `<Space>` while the `J` line editor is in Normal mode
    /// (a vi-style alternative for users already exploring the
    /// prompt's editor).
    pub fn show_jump_history_popup(&mut self) {
        let entries = self.state.jump_history.entries();
        if entries.is_empty() {
            self.state.flash_info("jump history is empty");
            return;
        }
        // Snapshot newest-first paths into pending_jump_history so
        // index ↔ entry mapping stays stable even if the live history
        // is mutated (e.g. by another running spyc).
        let snapshot: Vec<String> = entries.iter().rev().cloned().collect();
        let lines: Vec<String> = snapshot
            .iter()
            .enumerate()
            .map(|(i, p)| format!("  {:>3}  {}", i + 1, p))
            .collect();
        let mut view = pager::PagerView::new_plain(
            "jump history — j/k move, Enter cd, x delete, q close",
            lines,
        );
        view.picker_cursor = Some(0);
        view.no_history = true;
        view.show_line_numbers = false;
        view.wrap = false;
        self.view.pending_jump_history = Some(snapshot);
        self.set_pager(view);
        self.view.needs_full_repaint = true;
    }

    pub fn show_history_popup(&mut self) {
        let entries = self.state.history.entries();
        if entries.is_empty() {
            self.state.flash_info("history is empty");
            return;
        }
        // Show newest-first, numbered from 1.
        let lines: Vec<String> = entries
            .iter()
            .rev()
            .enumerate()
            .map(|(i, cmd)| format!("  {:>3}  {}", i + 1, cmd))
            .collect();
        // Create a line editor loaded with the newest entry, Normal mode.
        let newest = entries.last().unwrap();
        let mut editor = LineEditor::new();
        editor.set_content(newest);
        editor.mode = crate::ui::line_edit::Mode::Normal;
        if !editor.buf.is_empty() {
            editor.cursor = editor.buf.len() - 1;
        }
        let mut view = pager::PagerView::new_plain(
            "history — j/k move, i edit, Enter run, ^D delete, q close",
            lines,
        );
        view.picker_cursor = Some(0);
        view.picker_edit_cursor = Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));
        self.view.pending_history_pick = Some(editor);
        self.set_pager(view);
    }
}

#[cfg(test)]
mod tests {
    use super::{App, AutosaveAction, autosave_action};
    use std::time::{Duration, Instant};

    /// A live codex tab must save the rollout uuid spyc pinned to it.
    ///
    /// `resolve_resume_target` reads codex's **exit banner**, which a tab that's
    /// still running at quit has never printed — so save persisted `None` and
    /// restore spawned `codex resume --last`, which attaches to whichever rollout
    /// in the cwd was written most recently, including another spyc instance's.
    /// The uuid was sitting in `info.codex_session_id` the whole time.
    #[test]
    fn a_live_codex_tab_saves_the_rollout_uuid_it_is_pinned_to() {
        const UUID: &str = "0198c2f4-1a2b-7c3d-8e4f-5a6b7c8d9e0f";
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            // `cat` gives a harmless long-lived pty; `agent::detect` keys on the
            // command string, so relabelling it is what makes this a codex tab.
            app.open_pane_tab("cat");
            {
                let info = &mut app
                    .runtime
                    .project_mut()
                    .pane_tabs
                    .as_mut()
                    .expect("a tab was opened")
                    .tabs_mut()[0]
                    .info;
                info.command = "codex resume OLD --model test --sandbox read-only".to_string();
                info.codex_session_id = Some(UUID.to_string());
                info.codex_home = Some(tmp.path().join("codex-home"));
            }

            let snapshot = app.build_manifest();
            let saved = &snapshot.projects[0].tabs[0];
            assert_eq!(saved.agent_session_id.as_deref(), Some(UUID));
            assert_eq!(saved.command, "codex --model test --sandbox read-only");
            // And the id has to survive into the spawn, or saving it changed
            // nothing the user can see.
            assert_eq!(
                crate::agent::detect(&saved.command)
                    .reconstruct_restore(
                        &saved.command,
                        saved.agent_session_id.as_deref(),
                        tmp.path()
                    )
                    .command,
                format!("codex --model test --sandbox read-only resume {UUID}"),
                "a restored tab must resume that exact rollout, not --last"
            );
        });
    }

    /// Two tabs pinned to the same conversation must not both save it: restoring
    /// them would put two panes in one conversation, which is the collapse #230
    /// was filed for. The pinned path honours `claimed` for the same reason the
    /// spawn-proximity resolver always has.
    #[test]
    fn two_tabs_pinned_to_one_conversation_do_not_both_save_it() {
        const UUID: &str = "0198c2f4-1a2b-7c3d-8e4f-5a6b7c8d9e0f";
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.open_pane_tab("cat");
            app.open_pane_tab("cat");
            for entry in app
                .runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .expect("two tabs were opened")
                .tabs_mut()
            {
                entry.info.command = "codex".to_string();
                entry.info.codex_session_id = Some(UUID.to_string());
            }

            let snapshot = app.build_manifest();
            let tabs = &snapshot.projects[0].tabs;
            assert_eq!(tabs.len(), 2);
            let claims = tabs
                .iter()
                .filter(|t| t.agent_session_id.as_deref() == Some(UUID))
                .count();
            assert_eq!(claims, 1, "only one tab may claim a conversation");
        });
    }

    /// The same holds across projects: a tab in another project pinned to the
    /// same conversation doesn't save it a second time.
    #[test]
    fn tabs_in_two_projects_pinned_to_one_conversation_do_not_both_save_it() {
        const UUID: &str = "0198c2f4-1a2b-7c3d-8e4f-5a6b7c8d9e0f";
        two_projects(|app, b_id, b| {
            app.open_tab_in_project(b_id, b);
            let pin = |app: &mut App| {
                let tabs = app.runtime.project_mut().pane_tabs.as_mut().expect("a tab");
                let info = &mut tabs.tabs_mut()[0].info;
                info.command = "codex".to_string();
                info.codex_session_id = Some(UUID.to_string());
            };
            pin(app);
            app.as_project(b_id, pin);
            let saved = app.build_manifest();
            let claims = saved
                .projects
                .iter()
                .flat_map(|p| &p.tabs)
                .filter(|t| t.agent_session_id.as_deref() == Some(UUID))
                .count();
            assert_eq!(claims, 1, "only one tab may claim a conversation");
        });
    }

    #[test]
    fn codex_homes_and_duplicate_ids_survive_a_session_roundtrip() {
        const UUID: &str = "0198c2f4-1a2b-7c3d-8e4f-5a6b7c8d9e0f";
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.open_pane_tab("cat");
            app.open_pane_tab("cat");
            for (index, entry) in app
                .runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .unwrap()
                .tabs_mut()
                .iter_mut()
                .enumerate()
            {
                entry.info.command = "codex".into();
                entry.info.codex_session_id = Some(UUID.into());
                entry.info.codex_home = Some(tmp.path().join(format!("home-{index}")));
            }
            let saved = app.build_manifest();
            let json = serde_json::to_string(&saved).unwrap();
            let restored = parse(&json);
            for (index, tab) in restored.projects[0].tabs.iter().enumerate() {
                assert_eq!(tab.agent_session_id.as_deref(), Some(UUID));
                assert_eq!(
                    tab.codex_home,
                    Some(tmp.path().join(format!("home-{index}")))
                );
            }
        });
    }

    #[test]
    fn codex_save_before_home_capture_stays_unopened_on_restore() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.open_pane_tab("cat");
            let entry = &mut app
                .runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .unwrap()
                .tabs_mut()[0];
            entry.info.command = "codex".into();
            entry.info.codex_session_id = Some("019e8b21-9e7c-7553-a118-d1cdada725fd".into());
            entry.info.codex_home = None;
            let saved = app.build_manifest().anchored_at(crate_root());
            let id = entry_id(&app).to_owned();
            let restored = parse(&serde_json::to_string(&saved).unwrap());
            assert!(restored.projects[0].tabs[0].codex_home_unresolved);
            app.restore_session(&restored);
            assert_eq!(entry_id(&app), id, "a refused restore keeps the live pane");
            assert!(
                app.state
                    .flash
                    .as_ref()
                    .unwrap()
                    .text
                    .contains("home was not captured")
            );
        });
    }

    /// `-r` brings back what `:agent hud` chose. With no choice nothing is
    /// saved, so an rc edit between runs still takes effect.
    #[test]
    fn the_agent_hud_choice_survives_a_session_roundtrip() {
        use crate::config::AgentHudMode;
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.open_pane_tab("cat");
            assert_eq!(app.build_manifest().projects[0].agent_hud, None);
            let before = app.session_fingerprint();
            app.cmd_agent_hud("full");
            assert_ne!(
                app.session_fingerprint(),
                before,
                "the autosave must see the choice"
            );
            let saved = app.build_manifest().anchored_at(crate_root());
            let json = serde_json::to_string(&saved).unwrap();
            let restored = parse(&json);
            let mut fresh = App::test_app(tmp.path().to_path_buf());
            assert_eq!(fresh.agent_hud_preference(), AgentHudMode::Off);
            fresh.restore_session(&restored);
            assert_eq!(fresh.agent_hud_preference(), AgentHudMode::Full);
            let mut older: serde_json::Value = serde_json::from_str(&json).unwrap();
            older["projects"][0]
                .as_object_mut()
                .unwrap()
                .remove("agent_hud");
            let older = crate::state::sessions::parse_manifest(older).unwrap();
            assert_eq!(
                older.projects[0].agent_hud, None,
                "a session saved before the field reads"
            );
        });
    }

    fn entry_id(app: &App) -> &str {
        &app.runtime.project().pane_tabs.as_ref().unwrap().tabs()[0]
            .info
            .id
    }

    fn crate_root() -> &'static std::path::Path {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    /// A saved manifest's text, read back as `spyc -r` reads it.
    fn parse(json: &str) -> crate::state::sessions::Manifest {
        crate::state::sessions::parse_manifest(serde_json::from_str(json).unwrap())
            .expect("a manifest")
    }

    /// A restore point the picker can restore. Its column `a` is the crate
    /// root, where the test process already runs, because restoring `chdir`s
    /// the whole process (#576).
    fn restorable(id: u64, epoch_secs: u64, name: &str) -> crate::state::sessions::Manifest {
        crate::state::sessions::Manifest::one_project(
            id,
            epoch_secs,
            name,
            crate_root(),
            Vec::new(),
        )
    }

    fn press(app: &mut App, code: crossterm::event::KeyCode) {
        app.handle_pager_key(crossterm::event::KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::empty(),
        ));
    }

    /// The text of the line under the open picker's cursor.
    fn cursored_line(app: &App) -> String {
        let view = app
            .view
            .project()
            .pager
            .as_ref()
            .expect("the picker is open");
        let cursor = view.picker_cursor.expect("a picker has a cursor");
        view.lines[cursor].to_string()
    }

    /// Quitting with nothing to restore — no tabs, no split, no scope claims —
    /// must not write a restore point. Each one took a `MAX_SESSIONS` slot, so
    /// opening `spyc -r`, finding the session missing and quitting pushed out
    /// another real session every time.
    #[test]
    fn quitting_with_nothing_to_restore_writes_no_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.save_session();
            assert!(
                crate::state::sessions::load_manifests().is_empty(),
                "an empty quit must not take a picker slot"
            );
            let summary = app.exit_summary.as_deref().unwrap_or_default();
            assert!(
                !summary.contains("spyc -r"),
                "the exit summary must not offer a restore that isn't there: {summary}"
            );
        });
    }

    /// The other side of the rule above: a restored session whose tabs were all
    /// closed is still overwritten on quit. Skipping that save would leave the
    /// old file in place, and `-r` would reopen the tabs the user closed.
    #[test]
    fn quitting_an_emptied_restored_session_still_overwrites_it() {
        const ID: u64 = 4242;
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::state::with_state_root(tmp.path(), || {
            let mut saved = restorable(ID, 1_700_000_000, "OLD");
            saved.projects[0]
                .tabs
                .push(crate::state::sessions::SavedTab {
                    command: "cat".into(),
                    label: "cat".into(),
                    cwd: tmp.path().to_path_buf(),
                    agent_kind: crate::state::sessions::AgentKind::Other,
                    agent_session_id: None,
                    agent_session_name: None,
                    codex_home: None,
                    codex_home_unresolved: false,
                    claim_owner: String::new(),
                    pane_id: String::new(),
                    agent_hint: None,
                });
            crate::state::sessions::save_session(&saved).expect("seed the restore point");

            let mut app = App::test_app(tmp.path().to_path_buf());
            app.state.session_id = Some(ID);
            app.save_session();

            let loaded = crate::state::sessions::load_manifests();
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].id, ID);
            assert!(
                loaded[0].projects[0].tabs.is_empty(),
                "the closed tab must not come back"
            );
        });
    }

    /// Every line the session picker's cursor can stop on is a choice. A blank
    /// spacer under `[n] new session` was selectable, and `Enter` on it quietly
    /// started a new session, which looks like a corrupt entry.
    #[test]
    fn every_line_the_session_picker_cursor_reaches_is_a_choice() {
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::state::with_state_root(tmp.path(), || {
            for (id, name) in [(1, "FIRST"), (2, "SECOND")] {
                crate::state::sessions::save_session(&restorable(id, 1_700_000_000 + id, name))
                    .expect("seed a restore point");
            }
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.show_session_picker();
            let reachable = app
                .view
                .project()
                .pager
                .as_ref()
                .expect("picker")
                .lines
                .len();
            for code in [
                crossterm::event::KeyCode::Up,
                crossterm::event::KeyCode::Down,
            ] {
                for _ in 0..reachable {
                    let line = cursored_line(&app);
                    assert!(
                        !line.trim().is_empty(),
                        "the cursor stopped on a line that is not a choice"
                    );
                    press(&mut app, code);
                }
            }
        });
    }

    /// The picker opens on the newest session, `Enter` restores the row under
    /// the cursor, and the row above the list starts a new session.
    #[test]
    fn the_session_picker_restores_the_row_under_the_cursor() {
        use crossterm::event::KeyCode;
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::state::with_state_root(tmp.path(), || {
            for (id, name) in [(1, "OLDER"), (2, "NEWER")] {
                crate::state::sessions::save_session(&restorable(id, 1_700_000_000 + id, name))
                    .expect("seed a restore point");
            }

            let mut app = App::test_app(tmp.path().to_path_buf());
            app.show_session_picker();
            assert!(cursored_line(&app).contains("NEWER"));
            press(&mut app, KeyCode::Down);
            press(&mut app, KeyCode::Enter);
            assert_eq!(app.state.session_id, Some(1));
            assert_eq!(app.state.session_name.as_deref(), Some("OLDER"));

            let mut app = App::test_app(tmp.path().to_path_buf());
            app.show_session_picker();
            press(&mut app, KeyCode::Enter);
            assert_eq!(app.state.session_id, Some(2));

            let mut app = App::test_app(tmp.path().to_path_buf());
            app.show_session_picker();
            for _ in 0..app
                .view
                .project()
                .pager
                .as_ref()
                .expect("picker")
                .lines
                .len()
            {
                press(&mut app, KeyCode::Up);
            }
            assert!(cursored_line(&app).contains("new session"));
            press(&mut app, KeyCode::Enter);
            assert_eq!(app.state.session_id, None, "the [n] row restores nothing");
            assert!(app.view.project().pager.is_none(), "and closes the picker");
        });
    }

    // ── which conversation a claude tab saves (#584) ─────────────────
    //
    // Claude writes a conversation's transcript only once it has a message,
    // but its per-process record and the hook's pin exist from startup. These
    // fixtures lay out `~/.claude` in a temp dir to catch the save between the
    // two, where it used to record another tab's conversation.

    const MINE: &str = "11111111-1111-4111-8111-111111111111";
    const OTHER: &str = "22222222-2222-4222-8222-222222222222";
    const SPAWN: u64 = 1_800_000_000;

    /// One claude tab in `cwd`, spawned at [`SPAWN`] and pinned to `pin`.
    fn claude_tab(app: &mut App, cwd: &std::path::Path, pin: Option<&str>) {
        app.open_pane_tab("cat");
        let info = &mut app
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .expect("a tab was opened")
            .tabs_mut()[0]
            .info;
        info.command = "claude".into();
        info.cwd = cwd.to_path_buf();
        info.spawn_epoch_secs = SPAWN;
        info.live_session_id = pin.map(String::from);
    }

    /// Claude's record for its running process: `sessions/<pid>.json`.
    fn claude_record(
        claude: &std::path::Path,
        pid: u32,
        sid: &str,
        cwd: &std::path::Path,
        started: u64,
    ) {
        let dir = claude.join("sessions");
        std::fs::create_dir_all(&dir).expect("sessions dir");
        let record = serde_json::json!({
            "pid": pid, "sessionId": sid, "cwd": cwd, "startedAt": started * 1000
        });
        std::fs::write(dir.join(format!("{pid}.json")), record.to_string()).expect("record");
    }

    /// A transcript for `sid` in `cwd`'s project dir, last written at `written`.
    fn claude_transcript(claude: &std::path::Path, cwd: &std::path::Path, sid: &str, written: u64) {
        let dir = claude
            .join("projects")
            .join(crate::state::sessions::project_slug(cwd));
        std::fs::create_dir_all(&dir).expect("project dir");
        let file = dir.join(format!("{sid}.jsonl"));
        std::fs::write(&file, "{}\n").expect("transcript");
        std::fs::File::options()
            .write(true)
            .open(&file)
            .expect("open transcript")
            .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(written))
            .expect("set mtime");
    }

    /// Runs `body` with the state root and `~/.claude` pinned under a temp dir,
    /// handing it the dir claude would run in and the fake `~/.claude`.
    fn with_claude_home(body: impl FnOnce(&std::path::Path, &std::path::Path)) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = std::fs::canonicalize(tmp.path()).expect("canonical tempdir");
        let (project, claude) = (root.join("project"), root.join("claude"));
        std::fs::create_dir_all(&project).expect("project");
        crate::state::with_state_root(&root.join("state"), || {
            crate::state::sessions::with_claude_dir(&claude, || body(&project, &claude));
        });
    }

    fn saved_conversation(app: &mut App) -> Option<String> {
        app.build_manifest().projects[0].tabs[0]
            .agent_session_id
            .clone()
    }

    /// The observed case: the hook has pinned the tab's own conversation, which
    /// claude hasn't written yet, and the newest transcript in the folder
    /// belongs to a conversation from before this tab existed.
    #[test]
    fn a_pinned_claude_tab_saves_its_own_conversation_before_claude_writes_it() {
        with_claude_home(|project, claude| {
            claude_transcript(claude, project, OTHER, SPAWN - 60);
            claude_record(claude, 4242, MINE, project, SPAWN + 1);
            let mut app = App::test_app(project.to_path_buf());
            claude_tab(&mut app, project, Some(MINE));
            assert_eq!(saved_conversation(&mut app).as_deref(), Some(MINE));
        });
    }

    /// Without a pin (no status hooks), the tab's own process record still
    /// identifies it. Its conversation having no transcript yet means there
    /// is nothing to resume, not that the tab is someone else's.
    #[test]
    fn an_unpinned_claude_tab_never_borrows_another_conversation_before_its_own_exists() {
        with_claude_home(|project, claude| {
            claude_transcript(claude, project, OTHER, SPAWN - 60);
            claude_record(claude, 4242, MINE, project, SPAWN + 1);
            let mut app = App::test_app(project.to_path_buf());
            claude_tab(&mut app, project, None);
            assert_eq!(saved_conversation(&mut app), None);
        });
    }

    /// Another claude has been running in the same cwd for an hour and is
    /// writing its conversation now. While this tab's own conversation is
    /// unwritten, the other one is still not this tab's.
    #[test]
    fn an_unpinned_claude_tab_never_takes_another_running_claudes_conversation() {
        with_claude_home(|project, claude| {
            claude_record(claude, 1111, OTHER, project, SPAWN - 3_600);
            claude_transcript(claude, project, OTHER, SPAWN + 50);
            claude_record(claude, 4242, MINE, project, SPAWN + 1);
            let mut app = App::test_app(project.to_path_buf());
            claude_tab(&mut app, project, None);
            assert_eq!(saved_conversation(&mut app), None);
        });
    }

    /// A claude started just after this tab's, as by another tab opened beside
    /// it, has already written its conversation. This tab's own record is the
    /// nearer one, and its unwritten conversation still isn't the other's.
    #[test]
    fn an_unpinned_claude_tab_matches_its_own_record_before_a_written_neighbour() {
        with_claude_home(|project, claude| {
            claude_record(claude, 4242, MINE, project, SPAWN + 1);
            claude_record(claude, 4343, OTHER, project, SPAWN + 3);
            claude_transcript(claude, project, OTHER, SPAWN + 20);
            let mut app = App::test_app(project.to_path_buf());
            claude_tab(&mut app, project, None);
            assert_eq!(saved_conversation(&mut app), None);
        });
    }

    /// The same with no record for this tab's own claude: the last-resort guess
    /// can't take a conversation another running claude owns.
    #[test]
    fn the_last_resort_guess_skips_a_running_claudes_conversation() {
        with_claude_home(|project, claude| {
            claude_record(claude, 1111, OTHER, project, SPAWN - 3_600);
            claude_transcript(claude, project, OTHER, SPAWN + 50);
            let mut app = App::test_app(project.to_path_buf());
            claude_tab(&mut app, project, None);
            assert_eq!(saved_conversation(&mut app), None);
        });
    }

    /// With no process record at all, a transcript last written before the tab
    /// started can't be the tab's conversation.
    #[test]
    fn a_transcript_older_than_the_tab_is_not_its_conversation() {
        with_claude_home(|project, claude| {
            claude_transcript(claude, project, OTHER, SPAWN - 60);
            let mut app = App::test_app(project.to_path_buf());
            claude_tab(&mut app, project, None);
            assert_eq!(saved_conversation(&mut app), None);
        });
    }

    /// The last-resort guess still finds a conversation written since the tab
    /// started, for a claude that leaves no process record.
    #[test]
    fn with_no_record_a_conversation_written_since_the_tab_started_is_saved() {
        with_claude_home(|project, claude| {
            claude_transcript(claude, project, MINE, SPAWN + 30);
            let mut app = App::test_app(project.to_path_buf());
            claude_tab(&mut app, project, None);
            assert_eq!(saved_conversation(&mut app).as_deref(), Some(MINE));
        });
    }

    /// Spawn-time matching still picks the tab's own conversation over an
    /// older one in the same cwd once both are written.
    #[test]
    fn an_unpinned_claude_tab_saves_its_own_written_conversation() {
        with_claude_home(|project, claude| {
            claude_record(claude, 1111, OTHER, project, SPAWN - 3_600);
            claude_transcript(claude, project, OTHER, SPAWN + 50);
            claude_record(claude, 4242, MINE, project, SPAWN + 1);
            claude_transcript(claude, project, MINE, SPAWN + 40);
            let mut app = App::test_app(project.to_path_buf());
            claude_tab(&mut app, project, None);
            assert_eq!(saved_conversation(&mut app).as_deref(), Some(MINE));
        });
    }

    /// The autosave writes only when its fingerprint moves, so the moment a
    /// tab's conversation id becomes known has to move it. Otherwise the file
    /// keeps whatever the previous save resolved until something unrelated
    /// changes.
    #[test]
    fn a_pin_arriving_moves_the_autosave_fingerprint() {
        with_claude_home(|project, _| {
            let mut app = App::test_app(project.to_path_buf());
            claude_tab(&mut app, project, None);
            let before = app.session_fingerprint();
            app.runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .expect("the tab")
                .tabs_mut()[0]
                .info
                .live_session_id = Some(MINE.into());
            assert_ne!(app.session_fingerprint(), before);
        });
    }

    /// One agy tab in `cwd`, spawned at [`SPAWN`] and pinned to `pin`.
    fn agy_tab(app: &mut App, cwd: &std::path::Path, pin: Option<&str>) {
        claude_tab(app, cwd, pin);
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .expect("the tab")
            .tabs_mut()[0]
            .info
            .command = "agy".into();
    }

    /// A line of agy's prompt history: a message sent in `conversation`, in
    /// `workspace`, at `secs`.
    fn agy_history(
        home: &std::path::Path,
        workspace: &std::path::Path,
        conversation: &str,
        secs: u64,
    ) {
        use std::io::Write as _;
        let path = home.join(".gemini/antigravity-cli/history.jsonl");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("agy dir");
        let line = serde_json::json!({
            "workspace": workspace, "conversationId": conversation, "timestamp": secs * 1000,
        });
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("history");
        writeln!(f, "{line}").expect("history line");
    }

    fn with_agy_home(body: impl FnOnce(&std::path::Path, &std::path::Path)) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = std::fs::canonicalize(tmp.path()).expect("canonical tempdir");
        let (project, home) = (root.join("project"), root.join("home"));
        std::fs::create_dir_all(&project).expect("project");
        crate::state::with_state_root(&root.join("state"), || {
            crate::state::sessions::with_agy_home(&home, || body(&project, &home));
        });
    }

    /// #589: an agy tab with no pin and no exit banner saves nothing. History
    /// lists an older conversation in its workspace, but never a new one, so
    /// whatever it offers is not this tab's.
    #[test]
    fn an_unpinned_agy_tab_never_borrows_an_older_conversation() {
        with_agy_home(|project, home| {
            agy_history(home, project, OTHER, SPAWN - 60);
            let mut app = App::test_app(project.to_path_buf());
            agy_tab(&mut app, project, None);
            assert_eq!(saved_conversation(&mut app), None);
        });
    }

    /// The pin is saved before agy has written the conversation; restore is
    /// what checks for the transcript.
    #[test]
    fn a_pinned_agy_tab_saves_its_pin_before_agy_writes_it() {
        with_agy_home(|project, home| {
            agy_history(home, project, OTHER, SPAWN - 60);
            let mut app = App::test_app(project.to_path_buf());
            agy_tab(&mut app, project, Some(MINE));
            assert_eq!(saved_conversation(&mut app).as_deref(), Some(MINE));
        });
    }

    /// `^a R` respawns a tab in place with the same command and cwd and a new
    /// pane id. The saved id has to follow, or a crash restores the old one
    /// and the respawned agent's hooks name a tab that isn't there.
    #[test]
    fn a_new_pane_id_moves_the_autosave_fingerprint() {
        with_claude_home(|project, _| {
            let mut app = App::test_app(project.to_path_buf());
            claude_tab(&mut app, project, None);
            let before = app.session_fingerprint();
            app.runtime
                .project_mut()
                .pane_tabs
                .as_mut()
                .expect("the tab")
                .tabs_mut()[0]
                .info
                .id = "respawned".into();
            assert_ne!(app.session_fingerprint(), before);
        });
    }

    /// Two projects, each homed in its own directory under a temp dir, `a`
    /// active with a tab.
    fn two_projects(body: impl FnOnce(&mut App, crate::app::state::ProjectId, &std::path::Path)) {
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::state::with_state_root(tmp.path(), || {
            let base = std::fs::canonicalize(tmp.path()).expect("canonical");
            let (a, b) = (base.join("a"), base.join("b"));
            std::fs::create_dir_all(&a).expect("a");
            std::fs::create_dir_all(&b).expect("b");
            let mut app = App::test_app(a.clone());
            app.open_pane_tab_in("cat", &a);
            let b_id = app.add_test_project(&b);
            body(&mut app, b_id, &b);
        });
    }

    /// A tab opening in a project the user isn't in is saved, so the autosave
    /// has to see it.
    #[test]
    fn a_tab_in_a_background_project_moves_the_autosave_fingerprint() {
        two_projects(|app, b_id, b| {
            let before = app.session_fingerprint();
            app.open_tab_in_project(b_id, b);
            assert_ne!(app.session_fingerprint(), before);
        });
    }

    /// Column `a`'s directory is saved, so moving it is a change to save.
    #[test]
    fn navigating_column_a_moves_the_autosave_fingerprint() {
        two_projects(|app, _, b| {
            let before = app.session_fingerprint();
            app.state.active_project_mut().left.listing =
                crate::fs::listing::Listing::empty(b.to_path_buf());
            assert_ne!(app.session_fingerprint(), before);
        });
    }

    /// A project's start directory is saved and restored, so changing it is a
    /// change to save.
    #[test]
    fn a_new_start_dir_moves_the_autosave_fingerprint() {
        two_projects(|app, _, b| {
            let before = app.session_fingerprint();
            app.state.active_project_mut().start_dir = b.to_path_buf();
            assert_ne!(app.session_fingerprint(), before);
        });
    }

    /// A one-project row reads as a session's row always has; several projects
    /// take the path's place; the running column appears only when asked for,
    /// and keeps the rows aligned when it does.
    #[test]
    fn a_picker_row_names_its_place_and_whether_it_runs() {
        use crate::state::sessions::{AgentKind, Manifest, SavedTab};
        let tab = |label: &str| SavedTab {
            command: label.into(),
            label: label.into(),
            cwd: "/w".into(),
            agent_kind: AgentKind::Other,
            agent_session_id: None,
            agent_session_name: None,
            codex_home: None,
            codex_home_unresolved: false,
            claim_owner: String::new(),
            pane_id: String::new(),
            agent_hint: None,
        };
        let one = Manifest::one_project(1, 0, "ONE", "/w/spyc", vec![tab("vim"), tab("zsh")]);
        let row = super::picker_row(0, &one, false, false);
        assert!(row.starts_with("  [1]  ONE "), "{row}");
        assert!(row.ends_with(" /w/spyc  [vim, zsh]"), "{row}");

        let mut two = one.clone();
        two.projects.push(
            Manifest::one_project(2, 0, "", "/w/web", Vec::new())
                .projects
                .remove(0),
        );
        let row = super::picker_row(1, &two, false, false);
        assert!(row.starts_with("  [2]  ONE "), "{row}");
        assert!(row.ends_with(" spyc (2 tabs) · web"), "{row}");

        let running = super::picker_row(0, &one, true, true);
        let idle = super::picker_row(0, &one, false, true);
        assert!(running.contains(" running "), "{running}");
        assert!(!idle.contains("running"), "{idle}");
        assert_eq!(running.len(), idle.len(), "the column keeps rows aligned");
        assert_eq!(
            super::picker_row(0, &one, false, false).len() + 9,
            idle.len()
        );
    }

    #[test]
    fn autosave_idle_when_not_dirty() {
        let now = Instant::now();
        // Not dirty ⇒ always Idle, even with a due already armed (a save just
        // landed / the change was reverted).
        assert_eq!(autosave_action(false, None, now), AutosaveAction::Idle);
        assert_eq!(autosave_action(false, Some(now), now), AutosaveAction::Idle);
    }

    #[test]
    fn autosave_arms_debounce_on_first_dirty() {
        let now = Instant::now();
        match autosave_action(true, None, now) {
            AutosaveAction::Arm(when) => {
                assert!(when > now, "arm instant is debounced into the future");
            }
            other => panic!("expected Arm, got {other:?}"),
        }
    }

    #[test]
    fn autosave_waits_inside_window_then_fires_at_or_after_due() {
        let now = Instant::now();
        let due = now + Duration::from_secs(2);
        // Dirty + armed, window not elapsed ⇒ Wait.
        assert_eq!(autosave_action(true, Some(due), now), AutosaveAction::Wait);
        // Exactly at due ⇒ Fire (the predicate is `>=`).
        assert_eq!(autosave_action(true, Some(due), due), AutosaveAction::Fire);
        // Past due ⇒ Fire.
        assert_eq!(
            autosave_action(true, Some(due), due + Duration::from_millis(1)),
            AutosaveAction::Fire
        );
    }
}
