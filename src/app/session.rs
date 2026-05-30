//! Session save / restore and the session UI: `save_session` (serialize
//! tabs + agent session ids on quit), `restore_session` (rebuild tabs +
//! cwd from a saved `Session`), `show_session_picker` (the `-r` picker
//! pager), and `show_session_info` (the session-info overlay).
//!
//! Extracted from `app/mod.rs` (REFACTOR_PLAN Phase 2 tidy-up). Same
//! child-module `impl App` pattern: reads App's private state via the
//! descendant-module rule. All four are `pub` — called from the run
//! loop / `commands` / `key_dispatch` / `pager_handler` / `actions`. The
//! history-popup, worktree, and pane-sizing helpers that were interleaved
//! between them in mod.rs stay there.

use crate::pane::PaneTabs;
use crate::state::sessions::AgentKind;
use crate::ui::pager::{self, PagerView};

use super::state::Focus;
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
            active_tab: self.pane_tabs.as_ref().map_or(0, PaneTabs::active_index),
            pane_height_pct: self.state.pane_height_pct,
            pane_focused: self.state.pane_focused(),
            name: self.state.session_name.clone().unwrap_or_default(),
            project_home: self.state.project_home.clone(),
        };
        let _ = crate::state::sessions::save_session(&session);

        // Build exit summary for post-TUI output.
        let cwd_display = crate::paths::display_tilde(&session_cwd);
        let tab_count = session.tabs.len();
        let mut parts = vec![format!("session saved — {cwd_display}")];
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
        parts.push("restore with spyc -r".to_string());
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
        self.state.pane_height_pct = session.pane_height_pct;
        if !session.tabs.is_empty() {
            self.pane_tabs = None;
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
                self.open_pane_tab_in(&plan.command, cwd);
                if let crate::agent::ResumeAction::ClaudeStdin { session_id } = plan.resume
                    && let Some(tabs) = self.pane_tabs.as_mut()
                    && let Some(entry) = tabs.tabs_mut().last_mut()
                {
                    entry.info.pending_resume_send =
                        Some(crate::pane::tabs::PendingResumeSend::Text {
                            sid: session_id,
                            after: std::time::Instant::now() + RESTORE_BANNER_SETTLE,
                        });
                }
            }
            // Restore active tab.
            if let Some(tabs) = self.pane_tabs.as_mut() {
                tabs.switch_to(session.active_tab);
                // Restore custom labels. Defensive strip of any
                // `[exited N]` suffix so older session files
                // (saved before the save-side strip landed) heal
                // automatically the first time they load.
                for (entry, saved) in tabs.tabs_mut().iter_mut().zip(&session.tabs) {
                    entry.info.label = crate::pane::tabs::strip_exit_suffix(&saved.label);
                }
            }
            self.state.focus = if session.pane_focused {
                Focus::Pane
            } else {
                Focus::FileList
            };
        }
        self.state.flash_info("session restored");
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
            crate::paths::display_tilde(&self.state.listing.dir)
        ));
        lines.push(format!("entries  : {}", self.state.listing.entries.len()));
        lines.push(format!("visible  : {}", self.state.rows.len()));
        lines.push(format!("picks    : {}", self.state.picks.len()));
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
        self.pager = Some(PagerView::new_plain("session info", lines));
    }
}
