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
use crate::state::sessions::AgentKind;
use crate::ui::line_edit::LineEditor;
use crate::ui::pager::{self, PagerView};

use std::time::{Duration, Instant};

use super::state::{Side, VsplitMode};
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
    let profile = crate::agent::detect(&tab.info.command);
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
            tab.info.spawn_epoch_secs,
            claimed,
        ),
    }
}

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
        let session = self.build_session_snapshot();
        let save_result = crate::state::sessions::save_session(&session);
        // A successful save resets the autosave baseline so a follow-up
        // `settle_autosave` doesn't immediately re-write the identical session.
        if save_result.is_ok() {
            self.runtime.autosave_last_saved_fp = Some(self.session_fingerprint());
        }
        self.build_exit_summary(&session, &save_result);
    }

    /// P3-2 debounced autosave: persist the current session silently (no exit
    /// summary), returning whether the write succeeded. Called by
    /// [`Self::settle_autosave`] when the debounce window elapses.
    fn autosave_session(&mut self) -> bool {
        let session = self.build_session_snapshot();
        crate::state::sessions::save_session(&session).is_ok()
    }

    /// Build the `Session` snapshot from live state (tabs + per-agent resume
    /// ids, vsplit, project home, geometry). Shared by the quit-time
    /// [`Self::save_session`] and the debounced [`Self::autosave_session`].
    fn build_session_snapshot(&mut self) -> crate::state::sessions::Session {
        use crate::state::sessions::{SavedTab, Session};
        let epoch_secs = crate::sysinfo::epoch_secs();
        // Stable per-process session id (see `AppState::session_id`) so every
        // save overwrites one `<id>.json`; fresh-millis fallback only if it was
        // somehow never assigned.
        let id = self
            .state
            .session_id
            .unwrap_or_else(|| (crate::sysinfo::epoch_nanos() / 1_000_000) as u64);

        // Track session IDs already assigned to earlier tabs so each
        // Claude/Codex pane gets a distinct `agent_session_id` even
        // when several tabs share a cwd. Without this, the resolver's
        // "most-recent JSONL for cwd" fallback handed every Claude
        // pane the same ID and they all collapsed onto one
        // conversation at restore.
        let mut claimed: std::collections::HashSet<String> = std::collections::HashSet::new();
        let tabs: Vec<SavedTab> = self
            .runtime
            .pane_tabs
            .as_mut()
            .map(|pt| {
                pt.tabs_mut()
                    .iter_mut()
                    .map(|t| {
                        let profile = crate::agent::detect(&t.info.command);
                        let kind = profile.kind();
                        let (agent_session_id, agent_session_name) = tab_conversation(t, &claimed);
                        if let Some(ref id) = agent_session_id {
                            claimed.insert(id.clone());
                        }
                        // The sid lives in agent_session_id; baking
                        // --resume / `resume` into `command` would survive
                        // past a resolver miss and pollute the next restore.
                        let saved_command = profile.command_without_resume(&t.info.command);
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
                            claim_owner: t.info.claim_owner.clone(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        // SPYC-TRAP(partial-restore-keeps-saved-tabs): autosave overwrites the
        // original session, so unopened entries must survive every snapshot.
        let live_active = self
            .runtime
            .pane_tabs
            .as_ref()
            .map_or(0, PaneTabs::active_index);
        let (tabs, active_tab) =
            super::state::restore::merge_saved_tabs(tabs, live_active, &self.state.deferred_tabs);

        // Anchor the session on `project_home` (explicit) → `start_dir`
        // (where spyc was launched) → `listing.dir` (last resort).
        // `load_sessions` dedups on cwd + tab commands, so saving from
        // a deep subdir produced a fresh entry that restored at the
        // subdir instead of the user's project root.
        //
        // We don't walk up for `.git` here: a Java monorepo cloned into
        // `~/src/foo/inner-repo` may have `.git` at `inner-repo`, but
        // the user thinks of the *whole workspace* (`~/src/foo`) as
        // their project. Honouring `start_dir` matches that — the user
        // launched spyc there, so that's the natural anchor. Anyone
        // who wants a different anchor can set `project_home`
        // explicitly with `:project` or `gP`.
        let session_cwd = self
            .state
            .project_home
            .clone()
            .unwrap_or_else(|| self.state.start_dir.clone());
        Session {
            id,
            saved_at: crate::sysinfo::format_now(),
            epoch_secs,
            cwd: session_cwd,
            tabs,
            active_tab,
            pane_height_pct: self.state.pane.pane_height_pct,
            pane_focused: self.state.pane_focused(),
            name: self.state.session_name.clone().unwrap_or_default(),
            project_home: self.state.project_home.clone(),
            // Persist the open vertical split's shape + its content key: a
            // second *commander*'s cwd (`right_cwd`, reopened on restore — PR G)
            // or a Stage-1 *preview* file (`preview_path`). The two are mutually
            // exclusive (a commander clears `right_pager`).
            vsplit: self
                .state
                .vsplit
                .map(|v| crate::state::sessions::SavedVsplit {
                    width_pct: v.width_pct,
                    full_height: matches!(v.mode, VsplitMode::FullHeight),
                    focus_right: matches!(v.focus, Side::Right),
                    preview_path: self
                        .view
                        .right_pager
                        .as_ref()
                        .and_then(|p| p.source_path.clone()),
                    right_cwd: self
                        .state
                        .get_col(Side::Right)
                        .map(|c| c.listing.dir.clone()),
                }),
            scope_claims: self.state.scope_registry.clone(),
        }
    }

    /// Post-TUI exit summary line (`self.exit_summary`) from a just-saved
    /// session + its write result. Quit-path only — the debounced autosave
    /// persists silently.
    fn build_exit_summary(
        &mut self,
        session: &crate::state::sessions::Session,
        save_result: &std::io::Result<()>,
    ) {
        // Report the write result truthfully — a failed write (disk full,
        // unwritable state dir) must not tell the user their session was safe
        // when `spyc -r` would later find nothing.
        let cwd_display = crate::paths::display_tilde(&session.cwd);
        let tab_count = session.tabs.len();
        let saved_ok = save_result.is_ok();
        let mut parts = match save_result {
            Ok(()) => vec![format!("session saved — {cwd_display}")],
            Err(e) => vec![format!("session NOT saved ({e}) — {cwd_display}")],
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
                    let names: Vec<String> = session
                        .tabs
                        .iter()
                        .filter(|t| t.effective_kind() == kind)
                        .filter_map(|t| t.agent_session_name.clone())
                        .collect();
                    if !names.is_empty() {
                        parts.push(format!("{}: {}", profile.name(), names.join(", ")));
                    }
                }
                crate::agent::ExitSummaryMode::Count => {
                    let count = session
                        .tabs
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
    /// Read-only; must track what [`Self::build_session_snapshot`] persists.
    fn session_fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        if let Some(pt) = self.runtime.pane_tabs.as_ref() {
            pt.active_index().hash(&mut h);
            for t in pt.tabs() {
                t.info.command.hash(&mut h);
                t.info.cwd.hash(&mut h);
            }
        }
        super::state::restore::hash_deferred_tabs(&self.state.deferred_tabs, &mut h);
        self.state.project_home.hash(&mut h);
        self.state.pane.pane_height_pct.hash(&mut h);
        self.state.pane_focused().hash(&mut h);
        if let Some(v) = self.state.vsplit {
            v.width_pct.hash(&mut h);
            matches!(v.mode, VsplitMode::FullHeight).hash(&mut h);
            matches!(v.focus, Side::Right).hash(&mut h);
        }
        if let Some(c) = self.state.get_col(Side::Right) {
            c.listing.dir.hash(&mut h);
        }
        // P2: a scope-registry mutation (register/release) is session-relevant
        // — it's exactly what must survive a crash mid-merge-train.
        self.state.scope_registry.hash(&mut h);
        h.finish()
    }

    /// Whether a restore would bring anything back: tabs (open or kept
    /// unopened), a split, or scope claims. A cwd alone doesn't count.
    fn has_restorable_state(&self) -> bool {
        self.runtime
            .pane_tabs
            .as_ref()
            .is_some_and(|pt| !pt.tabs().is_empty())
            || !self.state.deferred_tabs.is_empty()
            || self.state.vsplit.is_some()
            || !self.state.scope_registry.is_empty()
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
        let sessions = sessions::load_sessions();
        if sessions.is_empty() {
            self.state.flash_info("no saved sessions");
            return;
        }
        let lines: Vec<String> = sessions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let age = sessions::format_relative_time(s.epoch_secs);
                let tab_count = s.tabs.len();
                let names: Vec<&str> = s.tabs.iter().map(|t| t.label.as_str()).collect();
                // Show agent session info (claude/codex) for tabs that have it.
                // Picker tooltips group by kind so a session with mixed
                // claude+codex panes is legible at a glance.
                let agent_info: Vec<String> = s
                    .tabs
                    .iter()
                    .filter_map(|t| {
                        let sid = t.agent_session_id.as_deref()?;
                        let short_id = &sid[..sid.len().min(8)];
                        let kind = t.effective_kind();
                        if kind == AgentKind::Other {
                            return None;
                        }
                        Some(
                            crate::agent::profile_for(kind)
                                .picker_label(short_id, t.agent_session_name.as_deref()),
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
                let name_col = if s.name.is_empty() {
                    "(unnamed)"
                } else {
                    s.name.as_str()
                };
                format!(
                    "  [{}]  {:<22} {:<14} {}{}{}",
                    i + 1,
                    name_col,
                    age,
                    s.cwd.display(),
                    tab_info,
                    agent_suffix
                )
            })
            .collect();
        self.state.pending_sessions = Some(sessions);
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
            crate::paths::display_tilde(&self.state.start_dir)
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
        if !self.state.deferred_tabs.is_empty() {
            lines.push(String::new());
            lines.push("unopened tabs (kept saved):".into());
            for deferred in &self.state.deferred_tabs {
                lines.push(format!(
                    "  saved tab {} ({}) — {}",
                    deferred.saved_index + 1,
                    deferred.tab.label,
                    deferred.reason
                ));
                lines.push(format!("    {}", deferred.tab.command));
            }
        }
        if !self.state.config.sources.is_empty() {
            lines.push(String::new());
            lines.push("config sources:".into());
            for src in &self.state.config.sources {
                lines.push(format!("  {}", crate::paths::display_tilde(src)));
            }
        }
        self.view.pager = Some(PagerView::new_plain("session info", lines));
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
            let running_panes = self.runtime.pane_tabs.as_ref().map_or(0, |tabs| {
                tabs.tabs().iter().filter(|e| !e.pane.is_closed()).count()
            });
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
        Self::sync_hist_editor(
            &mut self.view.pager,
            &mut self.view.pending_history_pick,
            &self.state.history,
        );
    }

    fn sync_hist_editor(
        pager: &mut Option<pager::PagerView>,
        editor_opt: &mut Option<LineEditor>,
        history: &crate::state::history::History,
    ) {
        let Some(view) = pager else { return };
        let Some(editor) = editor_opt else { return };
        let new_cursor = view.picker_cursor.unwrap_or(0);
        let entries = history.entries();
        let hist_idx = entries.len().saturating_sub(1 + new_cursor);
        if let Some(cmd) = entries.get(hist_idx) {
            editor.set_content_keep_mode(cmd);
        }
        let text = format!("  {:>3}  {}", new_cursor + 1, editor.text());
        view.lines[new_cursor] = ratatui::text::Line::from(text);
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
                    .pane_tabs
                    .as_mut()
                    .expect("a tab was opened")
                    .tabs_mut()[0]
                    .info;
                info.command = "codex resume OLD --model test --sandbox read-only".to_string();
                info.codex_session_id = Some(UUID.to_string());
            }

            let snapshot = app.build_session_snapshot();
            let saved = &snapshot.tabs[0];
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
                .pane_tabs
                .as_mut()
                .expect("two tabs were opened")
                .tabs_mut()
            {
                entry.info.command = "codex".to_string();
                entry.info.codex_session_id = Some(UUID.to_string());
            }

            let snapshot = app.build_session_snapshot();
            assert_eq!(snapshot.tabs.len(), 2);
            let claims = snapshot
                .tabs
                .iter()
                .filter(|t| t.agent_session_id.as_deref() == Some(UUID))
                .count();
            assert_eq!(claims, 1, "only one tab may claim a conversation");
        });
    }

    /// A restore point the picker can restore. Its `cwd` is the crate root,
    /// where the test process already runs, because restoring `chdir`s the
    /// whole process (#576).
    fn restorable(id: u64, epoch_secs: u64, name: &str) -> crate::state::sessions::Session {
        crate::state::sessions::Session {
            id,
            saved_at: String::new(),
            epoch_secs,
            cwd: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            tabs: Vec::new(),
            active_tab: 0,
            pane_height_pct: 30,
            pane_focused: false,
            name: name.to_string(),
            project_home: None,
            vsplit: None,
            scope_claims: Vec::new(),
        }
    }

    fn press(app: &mut App, code: crossterm::event::KeyCode) {
        app.handle_pager_key(crossterm::event::KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::empty(),
        ));
    }

    /// The text of the line under the open picker's cursor.
    fn cursored_line(app: &App) -> String {
        let view = app.view.pager.as_ref().expect("the picker is open");
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
                crate::state::sessions::load_sessions().is_empty(),
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
            saved.tabs.push(crate::state::sessions::SavedTab {
                command: "cat".into(),
                label: "cat".into(),
                cwd: tmp.path().to_path_buf(),
                agent_kind: crate::state::sessions::AgentKind::Other,
                agent_session_id: None,
                agent_session_name: None,
                claim_owner: String::new(),
            });
            crate::state::sessions::save_session(&saved).expect("seed the restore point");

            let mut app = App::test_app(tmp.path().to_path_buf());
            app.state.session_id = Some(ID);
            app.save_session();

            let loaded = crate::state::sessions::load_sessions();
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].id, ID);
            assert!(
                loaded[0].tabs.is_empty(),
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
            let reachable = app.view.pager.as_ref().expect("picker").lines.len();
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
            for _ in 0..app.view.pager.as_ref().expect("picker").lines.len() {
                press(&mut app, KeyCode::Up);
            }
            assert!(cursored_line(&app).contains("new session"));
            press(&mut app, KeyCode::Enter);
            assert_eq!(app.state.session_id, None, "the [n] row restores nothing");
            assert!(app.view.pager.is_none(), "and closes the picker");
        });
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
