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

use super::state::{Focus, Side, VSplit, VsplitMode};
use super::{App, RESTORE_BANNER_SETTLE};

impl App {
    pub fn save_session(&mut self) {
        use crate::state::sessions::{SavedTab, Session};
        let epoch_secs = crate::sysinfo::epoch_secs();
        // Session id is a millisecond timestamp -- unique within a
        // single spyc instance and human-glanceable in the picker.
        let id = (crate::sysinfo::epoch_nanos() / 1_000_000) as u64;

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
                        // Resolve the (session_id, session_name) to
                        // persist. The profile honors `claimed` internally
                        // so multi-pane saves don't collapse onto one
                        // conversation.
                        let (agent_session_id, agent_session_name) = profile.resolve_resume_target(
                            &t.pane,
                            &t.info.cwd,
                            t.info.spawn_epoch_secs,
                            &claimed,
                        );
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
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Anchor the session on `project_home` (explicit) → `start_dir`
        // (where spyc was launched) → `listing.dir` (last resort).
        // `load_sessions` dedups on cwd + tab commands, so saving from
        // a deep subdir produced a fresh entry that restored at the
        // subdir instead of the user's project root.
        //
        // We don't walk up for `.git` here: a Java monorepo cloned into
        // `~/src/foo/inner-repo` may have `.git` at `inner-repo`, but
        // the user thinks of the *whole workspace* (`~/src/foo`) as
        // their project. Honoring `start_dir` matches that — the user
        // launched spyc there, so that's the natural anchor. Anyone
        // who wants a different anchor can set `project_home`
        // explicitly with `:project` or `gP`.
        let session_cwd = self
            .state
            .project_home
            .clone()
            .unwrap_or_else(|| self.state.start_dir.clone());
        let session = Session {
            id,
            saved_at: crate::sysinfo::format_now(),
            epoch_secs,
            cwd: session_cwd.clone(),
            tabs,
            active_tab: self
                .runtime
                .pane_tabs
                .as_ref()
                .map_or(0, PaneTabs::active_index),
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
                    right_cwd: self.state.right.as_ref().map(|c| c.listing.dir.clone()),
                }),
        };
        let save_result = crate::state::sessions::save_session(&session);

        // Build exit summary for post-TUI output. Report the write result
        // truthfully — the old code ignored it and always said "session
        // saved", so a failed write (disk full, unwritable state dir) told
        // the user their session was safe when it wasn't, and `spyc -r`
        // would later find nothing.
        let cwd_display = crate::paths::display_tilde(&session_cwd);
        let tab_count = session.tabs.len();
        let saved_ok = save_result.is_ok();
        let mut parts = match &save_result {
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
        // (gemini) are omitted.
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
        let mut all_lines = vec!["  [n]  new session".to_string(), String::new()];
        all_lines.extend(lines);
        let mut view = pager::PagerView::new_plain(
            "sessions — j/k navigate, Enter restore, n new, q close",
            all_lines,
        );
        view.picker_cursor = Some(2); // Start on first session (after header).
        self.set_pager(view);
    }

    pub fn restore_session(&mut self, session: &crate::state::sessions::Session) {
        // Restore working directory and update start_dir so backtick (`)
        // jumps to the session's home, not where spyc was launched from.
        if session.cwd.is_dir() {
            if let Err(e) = self.state.chdir(&session.cwd) {
                self.state.flash_error(format!("session chdir: {e}"));
                return;
            }
            self.state.start_dir.clone_from(&session.cwd);
        } else {
            self.state
                .flash_error(format!("session dir gone: {}", session.cwd.display()));
            return;
        }
        // Keep the startup-generated name when an older session file
        // has no name field; otherwise take the saved one.
        if !session.name.is_empty() {
            self.state.session_name = Some(session.name.clone());
        }
        self.state.project_home = session.project_home.clone().filter(|p| p.is_dir());
        // Restore pane layout.
        self.state.pane.pane_height_pct = session.pane_height_pct;
        if !session.tabs.is_empty() {
            self.runtime.pane_tabs = None;
            for tab in &session.tabs {
                let cwd = if tab.cwd.is_dir() {
                    &tab.cwd
                } else {
                    &session.cwd
                };
                let kind = tab.effective_kind();
                // Codex restores by spawning `codex resume <UUID>`
                // directly — the CLI flag works, no `/resume` stdin
                // dance needed. Claude has a regression on the CLI
                // flag (crashes at mount with non-empty initialMessages),
                // so we always spawn fresh and type `/resume <sid>`
                // once it has settled.
                // Reconstruct the spawn command via the agent profile.
                // Codex/gemini/agy bake the resume into the command;
                // claude spawns fresh and arms the `/resume <sid>` stdin
                // send below (its `--resume` CLI flag crashes at mount
                // with non-empty initialMessages).
                let plan = crate::agent::profile_for(kind).reconstruct_restore(
                    &tab.command,
                    tab.agent_session_id.as_deref(),
                    cwd,
                );
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
                    // Label the tab we just pushed from ITS saved entry.
                    // Setting labels inline (rather than zipping tabs ↔
                    // session.tabs after the loop) keeps them aligned even
                    // when an earlier tab's spawn failed and the two vectors
                    // diverge. Defensive `strip_exit_suffix` heals older
                    // session files saved before the save-side strip landed.
                    entry.info.label = crate::pane::tabs::strip_exit_suffix(&tab.label);
                    if let crate::agent::ResumeAction::ClaudeStdin { session_id } = plan.resume {
                        entry.info.pending_resume_send =
                            Some(crate::pane::tabs::PendingResumeSend::Text {
                                sid: session_id,
                                after: std::time::Instant::now() + RESTORE_BANNER_SETTLE,
                            });
                    }
                }
            }
            // Restore the active tab. (On a partial-spawn-failure restore the
            // saved index may not line up 1:1; switch_to clamps.)
            if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                tabs.switch_to(session.active_tab);
            }
            self.state.focus = if session.pane_focused {
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
        self.state.flash_info("session restored");
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
        }
    }

    pub fn show_session_info(&mut self) {
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!(
            "\u{1f336}\u{fe0f} spyc {}",
            env!("CARGO_PKG_VERSION")
        ));
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
        lines.push(format!(
            "entries  : {}",
            self.state.left.listing.entries.len()
        ));
        lines.push(format!("visible  : {}", self.state.left.rows.len()));
        lines.push(format!("picks    : {}", self.state.left.picks.len()));
        lines.push(format!("inventory: {}", self.state.inventory.len()));
        lines.push(format!("marks    : {}", self.state.marks.entries.len()));
        lines.push(format!("rss      : {}", crate::sysinfo::format_rss()));
        lines.push(format!("time     : {}", crate::sysinfo::format_now()));
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
            if running > 0 {
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
