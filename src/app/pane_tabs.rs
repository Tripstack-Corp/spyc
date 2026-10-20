//! Bottom-pane / tab lifecycle: create / hide / close / restart tabs,
//! directional focus, and the `:dump-scrollback` diagnostic. Resize + zoom and
//! the pane/overlay sizing helpers live in the sibling `pane_resize.rs`. The
//! methods here are called from sibling modules (`actions`, `key_dispatch`,
//! `commands`, `tasks`, `render`, `pager_handler`, `loop_steps`, `session`,
//! `session_restore`), so they're `pub` (or `pub(super)` for those only a
//! sibling needs).

use super::{App, Effect, Mode, Pane, PaneTabs, Prompt, PromptKind, TabEntry, TabInfo, state};

/// Where a freshly-spawned tab lands. `Replace` keeps the restarted tab's
/// number: appending after a close is what shifted every later tab left (#151).
#[derive(Clone, Copy)]
pub(super) enum TabSlot {
    Append,
    Replace(usize),
}

/// The bits `needs_live_child_confirm` decides on, snapshotted off the tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TabConfirmSnapshot {
    /// The pty reader saw EOF — the tab shows `[exited N]`.
    pub pane_closed: bool,
    /// An exit status was reaped, so the child is gone even if EOF was missed.
    pub exit_reaped: bool,
    /// `Mode::Normal`. A confirm can only be raised from the base mode; any
    /// other mode already owns the keyboard, so asking there would strand it.
    pub mode_normal: bool,
}

/// Whether a destructive tab-lifecycle action (`^a x` close, `^a R` restart)
/// must confirm first: only when a live child stands to be lost. An exited tab
/// prompting is pure friction.
pub(super) const fn needs_live_child_confirm(s: TabConfirmSnapshot) -> bool {
    !s.pane_closed && !s.exit_reaped && s.mode_normal
}

impl App {
    /// [`needs_live_child_confirm`] over a snapshot of the active tab. False
    /// when there is no pane at all.
    fn needs_active_tab_confirm(&self) -> bool {
        self.runtime
            .project()
            .pane_tabs
            .as_ref()
            .is_some_and(|tabs| {
                let p = tabs.active();
                needs_live_child_confirm(TabConfirmSnapshot {
                    pane_closed: p.is_closed(),
                    exit_reaped: p.exit_status().is_some(),
                    mode_normal: matches!(self.state.mode, Mode::Normal),
                })
            })
    }

    /// The active tab's info, or `None` with no pane open — the
    /// `self.runtime.project().pane_tabs.as_ref().map(PaneTabs::active_info)`
    /// shape, named once for the sibling modules that need just this.
    pub(super) fn active_tab_info(&self) -> Option<&TabInfo> {
        self.runtime
            .project()
            .pane_tabs
            .as_ref()
            .map(PaneTabs::active_info)
    }

    /// How many pane tabs are open in every project, for the activity HUD's
    /// `panes:` cell.
    pub(super) fn pane_tab_count(&self) -> usize {
        self.runtime
            .every_tab_set()
            .map(|(_, tabs)| tabs.tabs().len())
            .sum()
    }

    /// `(is_shell, suspended)` for the active tab, with no pane open treated
    /// as a shell (so `^z` is forwarded rather than toggling a suspend that
    /// has nothing to act on — see `pane_suspend_key_action`).
    pub(super) fn active_tab_shell_and_suspended(&self) -> (bool, bool) {
        self.runtime
            .project()
            .pane_tabs
            .as_ref()
            .map_or((true, false), |t| {
                let info = t.active_info();
                (
                    crate::shell::command_is_shell(&info.command),
                    info.suspended,
                )
            })
    }

    /// The active tab's label, so a confirm can name what it is about to kill.
    fn active_tab_label(&self) -> String {
        self.runtime
            .project()
            .pane_tabs
            .as_ref()
            .map_or_else(String::new, |t| t.active_info().label.clone())
    }

    /// Go to another tab, `pick` moving the active one, and give it the
    /// keyboard: switching tabs means "I want to interact with this tab".
    /// From a fullscreen list (`TopList` zoom) it fullscreens the chosen tab
    /// instead (`fullscreen_tab_if_list_zoomed`), as `^a c` does.
    pub(super) fn go_to_tab(&mut self, pick: impl FnOnce(&mut PaneTabs)) {
        self.stash_scrollback_pager_to_active_tab();
        if let Some(tabs) = self.runtime.project_mut().pane_tabs.as_mut() {
            pick(tabs);
            self.state.active_project_mut().focus = state::Focus::Pane;
        }
        self.fullscreen_tab_if_list_zoomed();
        self.restore_active_tab_scrollback_pager();
        self.view.needs_full_repaint = true;
    }

    /// `^a-\` / F10 — toggle the bottom pane. Three states:
    ///   1. No tabs yet (`pane_tabs.is_none()`): spawn the default
    ///      command.
    ///   2. Tabs exist and visible: flip `pane_hidden = true`. The
    ///      child ptys keep running; render skips the pane area;
    ///      `pane_focused` parks at false so keystrokes go to the
    ///      file list. SIGWINCH is held off — the next show pass
    ///      restores the prior pane geometry.
    ///   3. Tabs exist and hidden: flip `pane_hidden = false`,
    ///      restore focus to the pane. Children pick up wherever
    ///      they left off.
    ///
    /// Hide, don't kill: the ptys keep running, so toggling for a few seconds
    /// doesn't lose an in-flight conversation. Killing is explicit — `^a-x`
    /// (`PaneCloseTab`) per tab, never a side effect of this toggle.
    pub fn toggle_pane(&mut self) {
        if self.runtime.project().pane_tabs.is_some() {
            let hidden = self.state.active_project().pane.pane_hidden;
            self.state.active_project_mut().pane.pane_hidden = !hidden;
            self.view.needs_full_repaint = true;
            if self.state.active_project().pane.pane_hidden {
                // Park focus on the list while hidden. Keystrokes
                // can't drive an off-screen pane sensibly. Zoom is
                // mutually exclusive with hidden — clear it so a
                // re-show doesn't try to render zoomed onto a
                // newly-resized area.
                self.state.active_project_mut().focus = state::Focus::FileList;
                self.state.active_project_mut().pane.zoom = state::ZoomTarget::None;
                self.state.flash_info("pane hidden — F10/^a-\\ to show");
            } else {
                // Re-show: focus the pane so the next keystroke
                // lands in the child. Matches the "I'm opening this
                // because I want to interact with it" intent.
                self.state.active_project_mut().focus = state::Focus::Pane;
                self.state.flash_info("pane shown");
            }
            return;
        }
        // No pane container exists — `^a-\` is a pure hide/show
        // toggle, not a create binding. Previously this silently
        // spawned the default command ($SPYC_PANE_CMD or `claude`),
        // which surprised users expecting a no-op (reported by
        // Justin: "I see ^a-c defaults to claude, but POLA"). Point
        // the user at the explicit creation binding instead.
        self.state.flash_info("no pane — ^a-c to create one");
    }

    /// Spawn a new pane tab. If no tabs exist, creates the container.
    /// Default cwd follows `[pane] new_tab_cwd` (`default_pane_cwd`):
    /// `PROJECT_HOME` by default (a new pane anchors to the project, not
    /// wherever the file list has wandered), or the focused column's listing
    /// dir under `browse_dir`. `^a-c` pre-fills the same value into its cwd
    /// prompt, so the prompted and bare-spawn paths stay in lockstep.
    ///
    /// Users who want a one-off anchor should use `^a-c` and edit the prompt,
    /// or invoke `:project` to move PROJECT_HOME.
    pub fn open_pane_tab(&mut self, cmd: &str) {
        let cwd = self.state.default_pane_cwd();
        self.open_pane_tab_in(cmd, &cwd);
    }

    /// Spawn a pane tab running `cmd` in `cwd`. Returns `true` iff the pty
    /// actually spawned and a tab was pushed — callers that arm follow-up
    /// state on "the tab we just added" (restart flash, restore `/resume`
    /// injection) MUST gate on this, or they act on the wrong tab when the
    /// spawn fails. On failure this flashes the error and returns `false`.
    pub fn open_pane_tab_in(&mut self, cmd: &str, cwd: &std::path::Path) -> bool {
        self.open_pane_tab_into(cmd, cwd, TabSlot::Append)
    }

    /// [`Self::open_pane_tab_in`], with control over which slot the new tab
    /// takes — `TabSlot::Replace` is the `^a R` restart-in-place path.
    pub(super) fn open_pane_tab_into(
        &mut self,
        cmd: &str,
        cwd: &std::path::Path,
        slot: TabSlot,
    ) -> bool {
        let (home, hint) = match slot {
            TabSlot::Append => (None, None),
            TabSlot::Replace(idx) => self
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .and_then(|tabs| tabs.tabs().get(idx))
                .map_or((None, None), |entry| {
                    (entry.info.codex_home.clone(), entry.info.agent_hint)
                }),
        };
        self.spawn_pane_tab(cmd, cwd, slot, home.as_deref(), None, hint)
    }

    /// A startup tab: [`Self::open_pane_tab_in`], as the agent `hint` names
    /// when its command is a wrapper spyc can't see the agent in.
    pub(super) fn open_pane_tab_as(
        &mut self,
        cmd: &str,
        cwd: &std::path::Path,
        hint: Option<crate::state::sessions::AgentKind>,
    ) -> bool {
        self.spawn_pane_tab(cmd, cwd, TabSlot::Append, None, None, hint)
    }

    /// Restore and fork reuse their source pane's home instead of current overrides.
    pub(super) fn open_pane_tab_in_home(
        &mut self,
        cmd: &str,
        cwd: &std::path::Path,
        home: Option<&std::path::Path>,
    ) -> bool {
        self.spawn_pane_tab(cmd, cwd, TabSlot::Append, home, None, None)
    }

    pub(super) fn spawn_pane_tab(
        &mut self,
        cmd: &str,
        cwd: &std::path::Path,
        slot: TabSlot,
        home: Option<&std::path::Path>,
        pane_id: Option<&str>,
        hint: Option<crate::state::sessions::AgentKind>,
    ) -> bool {
        // A new pane is meant to be used — if the file list is zoomed (the
        // pane is collapsed to its tab bar), reveal the split so the pane
        // isn't created behind a fullscreen list and silently lost. The
        // pane-zoom case already shows the pane, so leave it. (Cleared before
        // `pane_spawn_size` so the pty spawns at the real split size, not 0.)
        if self.state.active_project().pane.zoom == state::ZoomTarget::TopList {
            self.state.active_project_mut().pane.zoom = state::ZoomTarget::None;
        }
        let (rows, cols) = Self::pane_spawn_size(
            self.effective_pane_pct(),
            self.state.active_project().config.layout.status_position,
        );
        let wake = self.make_pane_wake();
        // The tab's agent: the hint's for a wrapper, else the command's. It
        // decides the env and the wiring; the command still runs as written.
        let kind = hint.unwrap_or_else(|| crate::agent::detect(cmd).kind());
        let shell = crate::shell::command_is_shell(cmd);
        // Build the tab metadata up front so its stable id is available to
        // inject as `SPYC_PANE_ID` (below). Option B: a `codex resume <uuid>`
        // pane knows its session id immediately — pin it now so `^a v` is exact
        // from the first keypress (the spawn-time scan handles fresh codex panes).
        let mut info = TabInfo::new(cmd, cwd);
        if let Some(id) = pane_id {
            info.id = id.to_string();
        }
        info.agent_hint = hint;
        info.status_hooks_at_spawn = super::wiring::hooks_present(kind);
        let is_agent = kind != crate::state::sessions::AgentKind::Other;
        match kind {
            crate::state::sessions::AgentKind::Codex => {
                info.codex_session_id =
                    crate::state::codex_transcript::resume_uuid_from_command(cmd);
            }
            // A hook report replaces it if the user switches conversation.
            crate::state::sessions::AgentKind::Agy => {
                info.live_session_id = crate::agent::resume::agy_conversation_in_command(cmd);
            }
            _ => {}
        }
        // Agent and shell panes get SPYC_MCP_SOCK + SPYC_PANE_ID in their env so
        // the `report_status` hook (a child of the agent, inheriting its env) can
        // reach this spyc instance's socket and target THIS tab by id; a shell's
        // agents inherit it from the shell. Owned
        // locals so the `&str` env slice outlives the spawn call; `info` is then
        // free to move into the `TabEntry`.
        // No state dir (neither XDG_STATE_HOME nor HOME) means no socket, so
        // there is nothing to point the hook at — spawn without it.
        let sock_str = crate::mcp::socket_path().map(|p| p.to_string_lossy().into_owned());
        let pane_id = info.id.clone();
        let mut extra_env: Vec<(&str, &str)> = match (is_agent || shell, sock_str.as_deref()) {
            (true, Some(sock)) => vec![("SPYC_MCP_SOCK", sock), ("SPYC_PANE_ID", pane_id.as_str())],
            (true, None) => vec![("SPYC_PANE_ID", pane_id.as_str())],
            (false, _) => Vec::new(),
        };
        if let Some(home) = home {
            let Some(home) = home.to_str() else {
                self.state
                    .flash_error("pane: Codex home cannot be represented in the child environment");
                return false;
            };
            extra_env.push(("CODEX_HOME", home));
        }
        match Pane::spawn_with_env(
            &self.spawn_command(cmd),
            rows,
            cols,
            cwd,
            &self.view.project().context_path,
            &extra_env,
            wake,
        ) {
            Ok(p) => {
                if kind == crate::state::sessions::AgentKind::Codex {
                    info.codex_home = home.map(std::path::Path::to_path_buf).or_else(|| {
                        p.host
                            .launch_environment()
                            .and_then(|env| crate::agent::codex_home::from_launch(&env, cwd))
                    });
                }
                self.state.active_project_mut().focus = state::Focus::Pane;
                self.state
                    .flash_info(format!("pane: {cmd} (^W k for list)"));
                let entry = TabEntry::new(p, info);
                match (self.runtime.project_mut().pane_tabs.as_mut(), slot) {
                    (Some(tabs), TabSlot::Replace(idx)) => tabs.replace_at(idx, entry),
                    (Some(tabs), TabSlot::Append) => tabs.push(entry),
                    (None, _) => self.runtime.project_mut().pane_tabs = Some(PaneTabs::new(entry)),
                }
                // The first launch of an agent asks whether to wire it. After
                // the tab exists, so the prompt doesn't cover a missing pane.
                self.offer_wiring(kind);
                true
            }
            Err(e) => {
                self.state.flash_error(format!("pane spawn failed: {e:#}"));
                false
            }
        }
    }

    /// What a pane actually runs for `cmd`: a codex pane gets `--no-daemon`
    /// (`agent::codex_without_daemon`) unless `[pane] codex_daemon` lets it
    /// join codex's shared server. The tab keeps `cmd` itself.
    fn spawn_command<'a>(&self, cmd: &'a str) -> std::borrow::Cow<'a, str> {
        let codex = crate::agent::detect(cmd).kind() == crate::state::sessions::AgentKind::Codex;
        if codex && !self.state.active_project().config.pane.codex_daemon {
            crate::agent::codex_without_daemon(cmd)
        } else {
            std::borrow::Cow::Borrowed(cmd)
        }
    }

    /// Respawn `cmd` (an agent) into the tab at `tab_idx`, replacing whatever
    /// is there, with the agent env (`SPYC_MCP_SOCK` + `SPYC_PANE_ID`) injected
    /// so the fresh process can report status — the same env `open_pane_tab_in`
    /// gives a launched agent. The CALLER kills the prior child first. When
    /// `resume_sid` is `Some`, arms the `/resume <sid>` stdin injection (shared
    /// with session restore) so a Claude conversation is recovered after the
    /// banner settles. Returns whether the spawn succeeded; prunes any orphaned
    /// scrollback stream left by the replaced entry.
    pub(super) fn spawn_agent_into_tab(
        &mut self,
        tab_idx: usize,
        cmd: &str,
        cwd: &std::path::Path,
        resume_sid: Option<String>,
    ) -> bool {
        if self
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .and_then(|tabs| tabs.tabs().get(tab_idx))
            .is_some_and(|entry| {
                entry.info.agent().kind() == crate::state::sessions::AgentKind::Codex
                    && entry.info.codex_home.is_none()
            })
        {
            self.state
                .flash_info("restart: Codex launch home is not identified yet");
            return false;
        }
        let (rows, cols) = Self::pane_spawn_size(
            self.effective_pane_pct(),
            self.state.active_project().config.layout.status_position,
        );
        let wake = self.make_pane_wake();
        let mut info = TabInfo::new(cmd, cwd);
        info.agent_hint = self
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .and_then(|tabs| tabs.tabs().get(tab_idx))
            .and_then(|entry| entry.info.agent_hint);
        if let Some(sid) = resume_sid {
            info.pending_resume_send = Some(crate::pane::tabs::PendingResumeSend::Text {
                sid,
                after: std::time::Instant::now() + super::RESTORE_BANNER_SETTLE,
            });
        }
        // Owned locals so the `&str` env slice outlives the spawn call. No
        // state dir means no socket to advertise (see the fresh-spawn path).
        let sock_str = crate::mcp::socket_path().map(|p| p.to_string_lossy().into_owned());
        let pane_id = info.id.clone();
        let home = self
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .and_then(|tabs| tabs.tabs().get(tab_idx))
            .and_then(|entry| entry.info.codex_home.clone());
        let mut extra_env = vec![("SPYC_PANE_ID", pane_id.as_str())];
        if let Some(sock) = sock_str.as_deref() {
            extra_env.push(("SPYC_MCP_SOCK", sock));
        }
        if let Some(home) = home.as_deref() {
            let Some(home) = home.to_str() else {
                self.state
                    .flash_error("pane: Codex home cannot be represented in the child environment");
                return false;
            };
            extra_env.push(("CODEX_HOME", home));
        }
        match Pane::spawn_with_env(
            &self.spawn_command(cmd),
            rows,
            cols,
            cwd,
            &self.view.project().context_path,
            &extra_env,
            wake,
        ) {
            Ok(p) => {
                if info.launched_agent().kind() == crate::state::sessions::AgentKind::Codex {
                    info.codex_home = home.clone().or_else(|| {
                        p.host
                            .launch_environment()
                            .and_then(|env| crate::agent::codex_home::from_launch(&env, cwd))
                    });
                    info.codex_session_id =
                        crate::state::codex_transcript::resume_uuid_from_command(cmd);
                }
                let entry = TabEntry::new(p, info);
                if let Some(tabs) = self.runtime.project_mut().pane_tabs.as_mut() {
                    tabs.replace_at(tab_idx, entry);
                }
                // The replaced entry (and any stashed scrollback pager) was
                // dropped; reclaim its parked stream so it doesn't leak.
                self.prune_orphaned_pager_streams();
                true
            }
            Err(e) => {
                self.state
                    .flash_error(format!("pane respawn failed: {e:#}"));
                false
            }
        }
    }

    /// `:hooks on!`, once claude is wired: **restart the claude tab `tab_id`**
    /// (fresh env + hooks present from launch) and auto-`/resume` its
    /// conversation. The forceful escape hatch for when a live reload doesn't
    /// take. Reuses the session-restore `/resume` injection; falls back to a
    /// fresh claude + "type /resume" when the live session id can't be
    /// resolved. A tab that closed while the install ran restarts nothing.
    pub(super) fn restart_claude_tab(&mut self, tab_id: &str) {
        let tabs = self.runtime.project().pane_tabs.as_ref();
        let Some(tab_idx) = tabs.and_then(|t| t.tabs().iter().position(|e| e.info.id == tab_id))
        else {
            return;
        };
        let Some((cmd, cwd, kind, in_shell, wraps)) = self
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .and_then(|t| t.tabs().get(tab_idx))
            .map(|entry| {
                let info = &entry.info;
                (
                    info.command.clone(),
                    info.cwd.clone(),
                    info.agent().kind(),
                    info.launched_agent().kind() != info.agent().kind(),
                    info.wraps_agent(),
                )
            })
        else {
            return;
        };
        if kind != crate::state::sessions::AgentKind::Claude {
            return;
        }
        // Restarting the tab would restart its shell, not claude, and type
        // `/resume` into that. Claude reloads its hooks live.
        if in_shell {
            self.state.flash_info(
                "claude runs in a shell here, so it isn't restarted; it picks the hooks up on its next turn",
            );
            return;
        }

        // Resolve the live session id (newest claude jsonl for this cwd) so we
        // can auto-`/resume`. Strip any `--resume` from the command so we
        // relaunch clean and drive the recovery via the stdin dance (claude's
        // `--resume` flag crashes at mount — same reason restore avoids it).
        let sid = crate::state::sessions::most_recent_jsonl_for_cwd(&cwd);
        let resumed = sid.is_some();
        let cmd_fresh = if wraps {
            cmd
        } else {
            crate::agent::resume::command_without_resume(&cmd)
        };

        if let Some(tabs) = self.runtime.project_mut().pane_tabs.as_mut()
            && let Some(entry) = tabs.tabs_mut().get_mut(tab_idx)
        {
            entry.pane.try_kill();
        }
        if self.spawn_agent_into_tab(tab_idx, &cmd_fresh, &cwd, sid) {
            self.state.active_project_mut().focus = state::Focus::Pane;
            if resumed {
                self.state.flash_info("restarting claude + resuming…");
            } else {
                self.state
                    .flash_info("fresh claude (no session found; type /resume)");
            }
        }
    }

    /// `^z` toggle for the active bottom pane: suspend it with `SIGSTOP` or
    /// resume it with `SIGCONT`. spyc manages the suspend itself (rather than
    /// forwarding `^z` for the child to self-suspend — a raw-mode TUI has
    /// `ISIG` off and discards the byte, and Claude, which does catch
    /// `^z`/SIGTSTP, has a handler that trips the macOS false-exit), so the tab
    /// tracks the state deterministically and the divider shows 💤. The
    /// `suspended` flip + flash are pure state here; only the signal is the
    /// `Effect` (`SignalPane`, which uses the uncatchable `SIGSTOP`). A shell
    /// tab's `^z` is forwarded for its own job control and never reaches this.
    /// No-op (flash) on a non-unix build.
    pub(super) fn toggle_pane_suspend(&mut self) -> Vec<Effect> {
        #[cfg(unix)]
        {
            let Some(tabs) = self.runtime.project_mut().pane_tabs.as_mut() else {
                return Vec::new();
            };
            // The pty's foreground process group — the command itself, since a
            // pane execs it (direct child / session leader, no wrapper shell).
            // This is exactly the group a real `^z` would signal.
            // Fall back to the child pid if `tcgetpgrp` is somehow unavailable.
            let active = tabs.active();
            let Some(pgrp) = active.foreground_pgrp().or_else(|| active.process_id()) else {
                self.state.flash_error("pane: no process group to suspend");
                return Vec::new();
            };
            let info = tabs.active_info_mut();
            // Currently suspended → this press RESUMES (SIGCONT); else suspends.
            let resume = info.suspended;
            info.suspended = !resume;
            let label = info.label.clone();
            if resume {
                self.state.flash_info(format!("resumed {label}"));
            } else {
                self.state
                    .flash_info(format!("suspended {label} — ^z to resume"));
            }
            // The frozen/woken child won't repaint the pane itself; force a
            // redraw so the 💤 marker (and, on resume, fresh output) lands.
            self.view.needs_full_repaint = true;
            vec![Effect::SignalPane { pgrp, resume }]
        }
        #[cfg(not(unix))]
        {
            self.state.flash_error("pane suspend is unix-only");
            Vec::new()
        }
    }

    pub fn start_new_tab_prompt(&mut self) {
        // Precedence: $SPYC_PANE_CMD > [pane] default_command in
        // .spycrc.toml > "claude" fallback. Env var wins so a user
        // can override on the fly per shell without editing config.
        let default_cmd = crate::envset::var("SPYC_PANE_CMD")
            .or_else(|| {
                self.state
                    .active_project()
                    .config
                    .pane
                    .default_command
                    .clone()
            })
            .unwrap_or_else(|| "claude".to_string());
        let mut p = Prompt::shell(PromptKind::PaneNewTabCmd, "pane command: ");
        p.buffer.clone_from(&default_cmd);
        if let Some(ed) = p.editor.as_mut() {
            ed.set_content(&default_cmd);
        }
        self.state.mode = Mode::Prompting(p);
    }

    /// ^W x — close the active pane tab. If its child is still running (a live
    /// claude, a shell mid-task), confirm first — closing it loses that
    /// session. An already-exited tab closes silently.
    pub fn close_active_tab(&mut self) {
        if self.needs_active_tab_confirm() {
            let label = self.active_tab_label();
            self.state.mode = Mode::Prompting(Prompt::simple(
                PromptKind::ClosePane,
                format!("close running tab '{label}'? [y/N] "),
            ));
            return;
        }
        self.close_active_tab_now();
    }

    /// Close the active pane tab unconditionally — the shared body reached
    /// directly for an exited tab, or after the running-tab confirm.
    pub(crate) fn close_active_tab_now(&mut self) {
        // Capture the closing tab's scope-claim owner key *before* it's removed,
        // so we can auto-release its claims — a closed tab never holds a stale
        // one (P2). `claim_owner` is unique per tab, so this drops exactly its
        // claims, not a sibling's.
        let closing_id = self
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .map(|tabs| tabs.active_info().id.clone());
        let closing_owner = self
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .map(|t| t.active_info().claim_owner.clone());
        if let Some(tabs) = self.runtime.project_mut().pane_tabs.as_mut()
            && !tabs.close_active()
        {
            // Last tab removed.
            self.runtime.project_mut().pane_tabs = None;
            self.state.active_project_mut().focus = state::Focus::FileList;
            self.view.needs_full_repaint = true;
            self.state.flash_info("pane: last tab closed");
        }
        if let Some(owner) = closing_owner {
            self.state.scope_registry.retain(|c| c.owner != owner);
        }
        if let Some(id) = closing_id {
            self.state
                .active_project_mut()
                .pane
                .pending_images
                .remove(&id);
            self.state
                .active_project_mut()
                .pane
                .image_capture_epochs
                .remove(&id);
        }
        // The closed tab may have had a stashed scrollback pager whose stream is
        // parked by id; reclaim it now that the tab is gone.
        self.prune_orphaned_pager_streams();
    }

    /// ^a R — restart the active tab's command. Confirms first when its child is
    /// still running: a restart kills that session exactly as `^a x` would, and
    /// `R` sits one shift away from `r` (rename). An exited tab restarts silently.
    pub fn restart_active_tab(&mut self) {
        if self.runtime.project().pane_tabs.is_none() {
            return;
        }
        if self.needs_active_tab_confirm() {
            let label = self.active_tab_label();
            self.state.mode = Mode::Prompting(Prompt::simple(
                PromptKind::RestartPane,
                format!("restart running tab '{label}'? [y/N] "),
            ));
            return;
        }
        self.restart_active_tab_now();
    }

    /// Restart the active tab unconditionally — the shared body reached directly
    /// for an exited tab, or after the running-tab confirm.
    ///
    /// Respawns **into the tab's own slot** rather than closing and appending:
    /// a close shifts every later tab's number down one and lands the
    /// replacement last, which is the numbering drift users see (#151).
    pub(crate) fn restart_active_tab_now(&mut self) {
        let Some(tabs) = self.runtime.project().pane_tabs.as_ref() else {
            return;
        };
        let idx = tabs.active_index();
        let info = tabs.active_info();
        let cmd = info.command.clone();
        let cwd = info.cwd.clone();
        let label = info.label.clone();
        if crate::agent::detect(&cmd).kind() == crate::state::sessions::AgentKind::Codex
            && info.codex_home.is_none()
        {
            self.state
                .flash_info("restart: Codex launch home is not identified yet");
            return;
        }
        let owner = info.claim_owner.clone();
        // Kill the outgoing child before its replacement spawns, so two agents
        // never briefly share the cwd; `replace_at` reaps the rest of the tree.
        if let Some(entry) = self
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .and_then(|tabs| tabs.tabs_mut().get_mut(idx))
        {
            entry.pane.try_kill();
        }
        // A failed spawn leaves the existing tab in place — keep
        // `open_pane_tab_into`'s error flash rather than claiming success.
        if !self.open_pane_tab_into(&cmd, &cwd, TabSlot::Replace(idx)) {
            return;
        }
        if let Some(entry) = self
            .runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .and_then(|tabs| tabs.tabs_mut().get_mut(idx))
        {
            // The fresh `TabInfo` re-derives the label from the command; restore
            // a `^a r` rename so a restart doesn't silently undo it. Strip the
            // `[exited N]` annotation first — it's runtime display state, and
            // `mark_exited` skips any label already carrying one, so restoring
            // it verbatim leaves a live tab tagged "exited 0" for good.
            entry.info.label = crate::pane::tabs::strip_exit_suffix(&label);
        }
        // The replacement carries a fresh `claim_owner`, so the old child's P2
        // scope claims can never be released by it — drop them now (mirrors
        // `close_active_tab_now`) instead of orphaning them in the registry.
        self.state.scope_registry.retain(|c| c.owner != owner);
        // Reclaim the replaced tab's parked scrollback stream, if it had one.
        self.prune_orphaned_pager_streams();
        self.state.flash_info(format!("pane: restarted {cmd}"));
    }

    /// ^W j / ^W k — set keyboard focus directionally (no wrap).
    pub fn set_pane_focus(&mut self, want_pane: bool) {
        // While zoomed, focus is pinned to the zoomed region: the other
        // region is collapsed/off-screen, so moving focus there is exactly the
        // confusing case. Refuse with a hint (mirrors `resize_pane`).
        if self.state.active_project().pane.zoom != state::ZoomTarget::None {
            self.state.flash_info("zoomed (^a z to exit)");
            return;
        }
        // From the right split column, `^a j` descends to the bottom pane (when
        // one's open) but *keeps* `vsplit.focus` on the right — so `^a k` from
        // the pane climbs back to the right column it came from, not the left
        // list (the generic path below restores it via `right_column_focused`).
        // `^a k` has nothing above the top-right column → no-op.
        if self.right_column_focused() {
            if want_pane && self.runtime.project().pane_tabs.is_some() {
                self.state.active_project_mut().focus = state::Focus::Pane;
                let label = self
                    .runtime
                    .project()
                    .pane_tabs
                    .as_ref()
                    .map_or("pane", |t| t.active_info().label.as_str());
                self.state.flash_info(format!("focus: {label}"));
                self.view.needs_full_repaint = true;
            }
            return;
        }
        if self.runtime.project().pane_tabs.is_none() {
            return;
        }
        if self.state.pane_focused() == want_pane {
            return; // already there — no-op
        }
        // The focus decision is a pure fn of these two bits (see
        // `super::focus::decide_focus`); the branch-order contract — every
        // non-Pane arm collapses to `pane_focused() == false` — is pinned by
        // tests there.
        self.state.active_project_mut().focus = super::focus::decide_focus(
            super::focus::FocusSnapshot {
                has_top_overlay: self.focused_col_has_overlay(),
                pager_mount: self.focused_top_pager_mount(),
            },
            want_pane,
        );
        if self.state.pane_focused() {
            let label = self
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .map_or("pane", |t| t.active_info().label.as_str());
            self.state.flash_info(format!("focus: {label}"));
        } else if self.column_focused(state::Side::Right) {
            // The right column owns the keyboard — its list, preview, or its own
            // `V`/`D` overlay (any surface in `b`).
            self.state.flash_info("focus: b (right)");
        } else {
            // When a `;cmd` overlay is showing the spyc-list slot, the
            // "non-pane" side is the overlay subprocess, not the file
            // list. Label accordingly so the user can read what just
            // got focus instead of guessing.
            let label = if self.runtime.project().top_overlay.is_some() {
                "overlay"
            } else {
                "spyc"
            };
            self.state.flash_info(format!("focus: {label}"));
        }
    }

    /// Re-derive `state.focus` from the live surfaces, preserving the current
    /// pane-vs-not intent.
    ///
    /// The pane-intent (is the bottom region focused?) is the stable bit set by
    /// `set_pane_focus` / pane opens / scrollback; the *non-pane* variant
    /// (`Overlay` / `Pager` / `FileList`) is simply whichever surface is
    /// front-most right now, derived by the same pure [`super::focus::decide_focus`]
    /// `set_pane_focus` uses. Called at the loop top (and on surface close) so
    /// `state.focus` always reflects reality with **no per-open-site
    /// bookkeeping** — most pager opens (`:grep`, git-view, help, …) never touch
    /// focus, and closes left a stale `Overlay`/`Pager` behind. The non-`Pane`
    /// discriminant is what routes a key to the editor or to a modal pager
    /// drawn over it.
    pub(super) fn recompute_focus(&mut self) {
        let want_pane = matches!(self.state.active_project().focus, state::Focus::Pane);
        self.state.active_project_mut().focus = super::focus::decide_focus(
            super::focus::FocusSnapshot {
                has_top_overlay: self.focused_col_has_overlay(),
                pager_mount: self.focused_top_pager_mount(),
            },
            want_pane,
        );
    }

    /// Does the FOCUSED vsplit column host a `V`/`D`/`;cmd` overlay PTY? Reads
    /// that column's own slot (`b` → `top_overlay_right`, else `top_overlay`), so
    /// an overlay open only in the OTHER column reads `false` — focus then
    /// belongs to the focused column's list, and `^a l`/`^a h` can move to the
    /// commander beside an open editor/pager.
    pub(super) fn focused_col_has_overlay(&self) -> bool {
        match self.focused_side() {
            state::Side::Right => self.runtime.project().top_overlay_right.is_some(),
            state::Side::Left => self.runtime.project().top_overlay.is_some(),
        }
    }

    /// The top-region pager mount that currently owns focus: a full-frame modal
    /// (grep / git-view / help / `;cmd` output) regardless of column, else the
    /// focused column's own `D` pager (`b` → `pager_right`, else `pager`). `None`
    /// when the focused column has no top pager. The right-column *preview* is
    /// deliberately excluded — it routes via `right_preview_focused`, keeping
    /// `Focus::FileList`.
    pub(super) fn focused_top_pager_mount(&self) -> Option<crate::ui::pager::Mount> {
        use crate::ui::pager::Mount;
        if matches!(
            self.view.project().pager.as_ref().map(|v| v.mount),
            Some(Mount::Overlay)
        ) {
            return Some(Mount::Overlay);
        }
        match self.focused_side() {
            state::Side::Right => self.view.project().pager_right.as_ref().map(|v| v.mount),
            state::Side::Left => self.view.project().pager.as_ref().map(|v| v.mount),
        }
    }

    /// `:dump-scrollback` diagnostic. Runs the same drain +
    /// snapshot path as `^a-v`, then writes the captured lines as
    /// plain text to `/tmp/spyc-scrollback.txt`. Tail the file to
    /// confirm whether content visible on the live pane (HUD
    /// overlays, etc.) is actually reaching our terminal engine at
    /// snapshot time.
    pub fn dump_scrollback_snapshot(&mut self) {
        let Some(tabs) = self.runtime.project_mut().pane_tabs.as_mut() else {
            self.state.flash_error("dump-scrollback: no pane open");
            return;
        };
        let active = tabs.active_mut();
        for _ in 0..3 {
            active.drain_output();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        active.drain_output();
        let lines = active.with_screen_mut(crate::ui::scrollback::lines_from_scrollback);
        let mut out = String::new();
        for line in &lines {
            for span in &line.spans {
                out.push_str(&span.content);
            }
            out.push('\n');
        }
        // Owner-only (0600) in the state dir, not the old world-readable,
        // symlink-followable `/tmp/spyc-scrollback.txt` (pane scrollback can
        // contain anything the agent printed). Truncate so each dump is a
        // fresh snapshot.
        let name = "spyc-scrollback.txt";
        let display = crate::state::state_file_path(name)
            .map_or_else(|| name.to_string(), |p| p.display().to_string());
        match crate::state::open_state_file_truncate(name) {
            Some(mut f) => match std::io::Write::write_all(&mut f, out.as_bytes()) {
                Ok(()) => self
                    .state
                    .flash_info(format!("wrote {} lines to {display}", lines.len())),
                Err(e) => self
                    .state
                    .flash_error(format!("dump-scrollback: write failed: {e:#}")),
            },
            None => self
                .state
                .flash_error("dump-scrollback: no state dir for the output file".to_string()),
        }
    }
}

#[cfg(test)]
mod confirm_tests {
    use super::{TabConfirmSnapshot, needs_live_child_confirm};

    const LIVE: TabConfirmSnapshot = TabConfirmSnapshot {
        pane_closed: false,
        exit_reaped: false,
        mode_normal: true,
    };

    /// The live child is the whole reason the confirm exists — `^a x` / `^a R`
    /// on it destroys a session.
    #[test]
    fn a_live_child_needs_confirming() {
        assert!(needs_live_child_confirm(LIVE));
    }

    /// An `[exited N]` tab has nothing left to lose, so it must NOT prompt.
    /// Either liveness signal alone settles it: pty EOF, or a reaped exit
    /// status when EOF was missed.
    #[test]
    fn an_exited_child_skips_the_confirm() {
        for s in [
            TabConfirmSnapshot {
                pane_closed: true,
                ..LIVE
            },
            TabConfirmSnapshot {
                exit_reaped: true,
                ..LIVE
            },
            TabConfirmSnapshot {
                pane_closed: true,
                exit_reaped: true,
                ..LIVE
            },
        ] {
            assert!(!needs_live_child_confirm(s), "{s:?} must restart silently");
        }
    }

    /// Outside `Mode::Normal` another modal owns the keyboard, so the action
    /// proceeds rather than stranding a prompt nothing can route a key to.
    #[test]
    fn a_non_normal_mode_skips_the_confirm() {
        assert!(!needs_live_child_confirm(TabConfirmSnapshot {
            mode_normal: false,
            ..LIVE
        }));
    }
}
