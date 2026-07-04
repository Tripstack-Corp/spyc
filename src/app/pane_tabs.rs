//! Bottom-pane / tab lifecycle: create / hide / close / restart tabs,
//! directional focus, resize + zoom, the pane/overlay sizing helpers,
//! the session-restore resume driver + crashed-restore detection, and
//! the `:dump-scrollback` diagnostic. The methods here are called from
//! sibling modules (`actions`, `key_dispatch`, `commands`, `tasks`, `render`,
//! `pager_handler`, `loop_steps`, `session`), so they're `pub` (or
//! `pub(super)` for the one that only a sibling needs).

use std::time::Duration;

use super::{
    App, Effect, Mode, Pane, PaneTabs, Prompt, PromptKind, RESTORE_RESUME_ENTER_DELAY,
    RESTORE_RESUME_VERIFY_DELAY, RESTORE_RESUME_VERIFY_RETRIES, RESTORE_RESUME_VERIFY_TAIL,
    StatusPosition, TabEntry, TabInfo, state,
};

impl App {
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
        if self.runtime.pane_tabs.is_some() {
            self.state.pane.pane_hidden = !self.state.pane.pane_hidden;
            self.view.needs_full_repaint = true;
            if self.state.pane.pane_hidden {
                // Park focus on the list while hidden. Keystrokes
                // can't drive an off-screen pane sensibly. Zoom is
                // mutually exclusive with hidden — clear it so a
                // re-show doesn't try to render zoomed onto a
                // newly-resized area.
                self.state.focus = state::Focus::FileList;
                self.state.pane.zoom = state::ZoomTarget::None;
                self.state.flash_info("pane hidden — F10/^a-\\ to show");
            } else {
                // Re-show: focus the pane so the next keystroke
                // lands in the child. Matches the "I'm opening this
                // because I want to interact with it" intent.
                self.state.focus = state::Focus::Pane;
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
        // A new pane is meant to be used — if the file list is zoomed (the
        // pane is collapsed to its tab bar), reveal the split so the pane
        // isn't created behind a fullscreen list and silently lost. The
        // pane-zoom case already shows the pane, so leave it. (Cleared before
        // `pane_spawn_size` so the pty spawns at the real split size, not 0.)
        if self.state.pane.zoom == state::ZoomTarget::TopList {
            self.state.pane.zoom = state::ZoomTarget::None;
        }
        let (rows, cols) = Self::pane_spawn_size(
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        let wake = self.make_pane_wake();
        // Write the agent's MCP client config (`.mcp.json` / `.codex/config.toml`)
        // into the launch dir *before* the pty spawns, so claude/codex discover
        // spyc's socket on startup. Gated on the agent here, so directories where
        // no agent is ever launched don't get a stray config dir. No-op otherwise.
        self.ensure_agent_mcp_config(cmd, cwd);
        // Codex reads `.codex/config.toml` once at startup, so an already-consented
        // repo's status hooks must be written now, before the pty spawns — else
        // they'd be missed until codex's next launch. Live-reload agents (claude)
        // are handled post-spawn. No-op for non-hook agents / un-consented repos.
        self.maybe_preinstall_startup_hooks(cmd, cwd);
        // Build the tab metadata up front so its stable id is available to
        // inject as `SPYC_PANE_ID` (below). Option B: a `codex resume <uuid>`
        // pane knows its session id immediately — pin it now so `^a v` is exact
        // from the first keypress (the spawn-time scan handles fresh codex panes).
        let mut info = TabInfo::new(cmd, cwd);
        let is_agent = crate::agent::detect(cmd).kind() != crate::state::sessions::AgentKind::Other;
        if crate::agent::detect(cmd).kind() == crate::state::sessions::AgentKind::Codex {
            info.codex_session_id = crate::state::codex_transcript::resume_uuid_from_command(cmd);
        }
        // Agent panes get SPYC_MCP_SOCK + SPYC_PANE_ID in their env so the
        // `report_status` hook (a child of the agent, inheriting its env) can
        // reach this spyc instance's socket and target THIS tab by id. Owned
        // locals so the `&str` env slice outlives the spawn call; `info` is then
        // free to move into the `TabEntry`.
        let sock_str = crate::mcp::socket_path().to_string_lossy().into_owned();
        let pane_id = info.id.clone();
        let extra_env: Vec<(&str, &str)> = if is_agent {
            vec![
                ("SPYC_MCP_SOCK", sock_str.as_str()),
                ("SPYC_PANE_ID", pane_id.as_str()),
            ]
        } else {
            Vec::new()
        };
        match Pane::spawn_with_env(
            cmd,
            rows,
            cols,
            cwd,
            &self.view.context_path,
            &extra_env,
            wake,
        ) {
            Ok(p) => {
                self.state.focus = state::Focus::Pane;
                self.state
                    .flash_info(format!("pane: {cmd} (^W k for list)"));
                let entry = TabEntry::new(p, info);
                if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                    tabs.push(entry);
                } else {
                    self.runtime.pane_tabs = Some(PaneTabs::new(entry));
                }
                // Claude status hooks: install (consented), skip (denied), or
                // raise the first-launch per-project consent popup. After the
                // tab exists so a Yes can target it; the env (SPYC_PANE_ID/SOCK)
                // is already in place from the spawn above.
                self.maybe_offer_status_hooks(cmd, cwd);
                true
            }
            Err(e) => {
                self.state.flash_error(format!("pane spawn failed: {e}"));
                false
            }
        }
    }

    /// After launching an agent pane that supports status hooks (claude/codex):
    /// install them if this project has already consented, do nothing if it
    /// declined, else raise the first-launch consent popup (`HookConsent`).
    /// Consent is keyed by the **project root** (`find_repo_root`, else the cwd)
    /// and saved, so a given repo is asked exactly once. Gated on MCP running
    /// (the hooks report over the socket) and the agent having a status-hook
    /// installer. For codex the already-consented case is also written
    /// pre-spawn ([`maybe_preinstall_startup_hooks`](Self::maybe_preinstall_startup_hooks))
    /// so the hooks are live this session; here it's a harmless idempotent re-run.
    fn maybe_offer_status_hooks(&mut self, cmd: &str, cwd: &std::path::Path) {
        if !self.view.mcp_running {
            return;
        }
        let profile = crate::agent::detect(cmd);
        let Some(support) = profile.status_hooks() else {
            return;
        };
        let kind = profile.kind();
        let root = state::find_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
        match crate::state::hook_consent::consent_for(&root) {
            Some(true) => self.install_status_hooks(cwd, kind),
            Some(false) => {}
            None => {
                self.state.mode = Mode::Prompting(Prompt::simple(
                    PromptKind::HookConsent {
                        root,
                        cwd: cwd.to_path_buf(),
                        agent: kind,
                    },
                    format!(
                        "Let spyc show this agent's live status? Writes hooks to {} (removed on exit). [y]es / [n]o ",
                        support.config_label
                    ),
                ));
            }
        }
    }

    /// Pre-spawn install for agents that read their hook config only at startup
    /// (codex): when this project has already consented, write the status hooks
    /// BEFORE the pty spawns so they're live for THIS session — a post-spawn
    /// install would be missed until the agent's next launch. Live-reload agents
    /// (claude) skip this; their post-spawn `maybe_offer_status_hooks` install
    /// is picked up on the next turn. First-launch consent is still handled
    /// post-spawn (it can't pre-empt the interactive popup).
    pub(super) fn maybe_preinstall_startup_hooks(&mut self, cmd: &str, cwd: &std::path::Path) {
        if !self.view.mcp_running {
            return;
        }
        let profile = crate::agent::detect(cmd);
        let Some(support) = profile.status_hooks() else {
            return;
        };
        if support.live_reload {
            return;
        }
        let root = state::find_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
        if crate::state::hook_consent::consent_for(&root) == Some(true) {
            self.install_status_hooks(cwd, profile.kind());
        }
    }

    /// Write `kind`'s status hooks into `cwd` and record the dir so teardown
    /// cleans them (shares `mcp_config_dirs` with the `.mcp.json` cleanup).
    /// Assumes consent is already granted. A no-op for an agent without a
    /// status-hook installer; a no-op-returning write (git-tracked config)
    /// simply isn't tracked.
    pub(super) fn install_status_hooks(
        &mut self,
        cwd: &std::path::Path,
        kind: crate::state::sessions::AgentKind,
    ) {
        let Some(support) = crate::agent::profile_for(kind).status_hooks() else {
            return;
        };
        if (support.ensure)(cwd) && !self.runtime.mcp_config_dirs.iter().any(|d| d == cwd) {
            self.runtime.mcp_config_dirs.push(cwd.to_path_buf());
        }
    }

    /// The project root `:hooks` acts on: the active pane's cwd's repo root
    /// (the agent the user means), else the focused commander's. Falls back to
    /// the dir itself when it isn't in a repo. Consent is keyed by this root.
    pub(super) fn status_hooks_target_root(&self) -> std::path::PathBuf {
        let dir = self.runtime.pane_tabs.as_ref().map_or_else(
            || self.state.cur().listing.dir.clone(),
            |t| t.active_info().cwd.clone(),
        );
        state::find_repo_root(&dir).unwrap_or(dir)
    }

    /// `:hooks on|off` — grant/revoke per-project consent for agent status hooks
    /// and (un)install them for every open hook-supporting pane (claude/codex)
    /// in that project, so a *running* agent gains/loses the hooks, not just
    /// future launches. Claude reloads `.claude/settings.json` live (effect on
    /// its next message); codex reads its config at startup, so an `on` for a
    /// running codex pane writes the hooks but they only apply on codex's next
    /// launch. This is the escape hatch from an accidental `no` at launch.
    pub(super) fn set_status_hooks(&mut self, enable: bool) {
        let root = self.status_hooks_target_root();
        crate::state::hook_consent::set_consent(&root, enable);
        // Hook-supporting panes whose project root matches — collect (cwd, kind)
        // first (immutable borrow) so the install/cleanup mutations don't alias.
        let panes: Vec<(std::path::PathBuf, crate::state::sessions::AgentKind)> = self
            .runtime
            .pane_tabs
            .as_ref()
            .map(|tabs| {
                tabs.tabs()
                    .iter()
                    .filter_map(|t| {
                        let profile = crate::agent::detect(&t.info.command);
                        profile
                            .status_hooks()
                            .is_some()
                            .then(|| (t.info.cwd.clone(), profile.kind()))
                    })
                    .filter(|(cwd, _)| state::find_repo_root(cwd).as_deref().unwrap_or(cwd) == root)
                    .collect()
            })
            .unwrap_or_default();
        for (cwd, kind) in &panes {
            if enable {
                self.install_status_hooks(cwd, *kind);
            } else if let Some(support) = crate::agent::profile_for(*kind).status_hooks() {
                (support.cleanup)(cwd);
                self.runtime.mcp_config_dirs.retain(|d| d != cwd);
            }
        }
        let proj = crate::paths::display_tilde(&root);
        if enable {
            let note = Self::ephemeral_hook_binary_note();
            self.state.flash_info(format!(
                "status hooks ON for {proj} ({} pane(s)) — claude live next message, codex on next launch (`:hooks on!` force-restarts claude){note}",
                panes.len()
            ));
        } else {
            self.state
                .flash_info(format!("status hooks OFF for {proj}"));
        }
    }

    /// A trailing flash note (or empty) warning that the running binary lives
    /// in a `target/{debug,release}` build dir — its absolute path is baked
    /// into the hook command (`current_exe()`), so it goes stale if that tree
    /// (e.g. a throwaway worktree) is cleaned. `make install` + running from a
    /// stable path avoids it. Cheap; computed only on user-initiated enable.
    fn ephemeral_hook_binary_note() -> String {
        let in_build_dir = std::env::current_exe().is_ok_and(|exe| {
            let has = |name: &str| {
                exe.components()
                    .any(|c| c.as_os_str().to_str() == Some(name))
            };
            has("target") && (has("debug") || has("release"))
        });
        if in_build_dir {
            " — note: build-dir binary, `make install` for a stable hook path".to_string()
        } else {
            String::new()
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
        let (rows, cols) = Self::pane_spawn_size(
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        let wake = self.make_pane_wake();
        let mut info = TabInfo::new(cmd, cwd);
        if let Some(sid) = resume_sid {
            info.pending_resume_send = Some(crate::pane::tabs::PendingResumeSend::Text {
                sid,
                after: std::time::Instant::now() + super::RESTORE_BANNER_SETTLE,
            });
        }
        // Owned locals so the `&str` env slice outlives the spawn call.
        let sock_str = crate::mcp::socket_path().to_string_lossy().into_owned();
        let pane_id = info.id.clone();
        let extra_env = [
            ("SPYC_MCP_SOCK", sock_str.as_str()),
            ("SPYC_PANE_ID", pane_id.as_str()),
        ];
        match Pane::spawn_with_env(
            cmd,
            rows,
            cols,
            cwd,
            &self.view.context_path,
            &extra_env,
            wake,
        ) {
            Ok(p) => {
                let entry = TabEntry::new(p, info);
                if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                    tabs.replace_at(tab_idx, entry);
                }
                // The replaced entry (and any stashed scrollback pager) was
                // dropped; reclaim its parked stream so it doesn't leak.
                self.prune_orphaned_pager_streams();
                true
            }
            Err(e) => {
                self.state.flash_error(format!("pane respawn failed: {e}"));
                false
            }
        }
    }

    /// `:hooks on!` — grant consent + install the hooks, then **restart the
    /// active claude pane** (fresh env + hooks present from launch) and
    /// auto-`/resume` its conversation. The forceful escape hatch for when a
    /// live-reload `:hooks on` doesn't take (a stale baked binary path, or
    /// Claude not picking up the settings change). Reuses the session-restore
    /// `/resume` injection; falls back to a fresh claude + "type /resume" when
    /// the live session id can't be resolved.
    pub(super) fn force_restart_status_hooks(&mut self) {
        let root = self.status_hooks_target_root();
        crate::state::hook_consent::set_consent(&root, true);

        let Some((tab_idx, cmd, cwd)) = self.runtime.pane_tabs.as_ref().map(|t| {
            let info = t.active_info();
            (t.active_index(), info.command.clone(), info.cwd.clone())
        }) else {
            self.state
                .flash_error(":hooks on! — no active pane to restart");
            return;
        };
        if crate::agent::detect(&cmd).kind() != crate::state::sessions::AgentKind::Claude {
            self.state
                .flash_error(":hooks on! — active pane is not claude");
            return;
        }
        self.install_status_hooks(&cwd, crate::state::sessions::AgentKind::Claude);

        // Resolve the live session id (newest claude jsonl for this cwd) so we
        // can auto-`/resume`. Strip any `--resume` from the command so we
        // relaunch clean and drive the recovery via the stdin dance (claude's
        // `--resume` flag crashes at mount — same reason restore avoids it).
        let sid = crate::state::sessions::most_recent_jsonl_for_cwd(&cwd);
        let resumed = sid.is_some();
        let cmd_fresh = crate::agent::resume::command_without_resume(&cmd);

        if let Some(tabs) = self.runtime.pane_tabs.as_mut()
            && let Some(entry) = tabs.tabs_mut().get_mut(tab_idx)
        {
            entry.pane.try_kill();
        }
        if self.spawn_agent_into_tab(tab_idx, &cmd_fresh, &cwd, sid) {
            self.state.focus = state::Focus::Pane;
            if resumed {
                self.state
                    .flash_info("status hooks on — restarting claude + resuming…");
            } else {
                self.state
                    .flash_info("status hooks on — fresh claude (no session found; type /resume)");
            }
        }
    }

    /// `^z` toggle for the active (agent) bottom pane: suspend it with
    /// `SIGSTOP` or resume it with `SIGCONT`. spyc manages the suspend itself
    /// (rather than forwarding `^z` for the agent to self-suspend — Claude
    /// catches `^z`/SIGTSTP and its handler trips the macOS false-exit), so the
    /// tab tracks the state deterministically and the divider shows 💤. The
    /// `suspended` flip + flash are pure state here; only the signal is the
    /// `Effect` (`SignalPane`, which uses the uncatchable `SIGSTOP`). A shell
    /// tab's `^z` is forwarded for its own job control and never reaches this.
    /// No-op (flash) on a non-unix build.
    pub(super) fn toggle_pane_suspend(&mut self) -> Vec<Effect> {
        #[cfg(unix)]
        {
            let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
                return Vec::new();
            };
            // The pty's foreground process group — the agent itself, since a
            // pane execs the agent (direct child / session leader, no wrapper
            // shell). This is exactly the group a real `^z` would signal.
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
        let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
            return;
        };
        for entry in tabs.tabs_mut() {
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

    /// Locate a `claude --resume` tab from session restore that looks
    /// broken (non-zero exit, or alive-but-printed-a-crash-dump within
    /// the 30s window). Disarms the marker on tabs whose window has
    /// passed without trouble, so a real user-driven exit later isn't
    /// mistaken for a restore failure. Returns the index of the first
    /// crashed tab found, if any.
    pub fn find_crashed_restore_tab(&mut self, now: std::time::Instant) -> Option<usize> {
        let tabs = self.runtime.pane_tabs.as_mut()?;
        let window = Duration::from_secs(30);
        let dump_grace = Duration::from_secs(3);
        for (i, entry) in tabs.tabs_mut().iter_mut().enumerate() {
            if entry.info.restore_fallback.is_none() {
                continue;
            }
            let age = now.duration_since(entry.info.spawn_at);
            if age > window {
                entry.info.restore_fallback = None;
                continue;
            }
            let bad_exit = entry.pane.is_closed()
                && entry.pane.exit_status().is_some_and(|s| s.exit_code() != 0);
            // Always re-scan once dump_grace has elapsed: claude often
            // prints the entire crash dump in <1s then sits quiescent,
            // and `output_dirty` gets cleared on every render — gating
            // on it would silently swallow the prompt.
            let dump_signature = !entry.pane.is_closed()
                && age >= dump_grace
                && pane_has_crash_marker(&entry.pane.recent_lines(200));
            if bad_exit || dump_signature {
                return Some(i);
            }
        }
        None
    }

    pub fn start_new_tab_prompt(&mut self) {
        // Precedence: $SPYC_PANE_CMD > [pane] default_command in
        // .spycrc.toml > "claude" fallback. Env var wins so a user
        // can override on the fly per shell without editing config.
        let default_cmd = crate::envset::var("SPYC_PANE_CMD")
            .or_else(|| self.state.config.pane.default_command.clone())
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
        let running_label = self.runtime.pane_tabs.as_ref().and_then(|tabs| {
            let p = tabs.active();
            (!p.is_closed() && p.exit_status().is_none()).then(|| tabs.active_info().label.clone())
        });
        if let Some(label) = running_label
            && matches!(self.state.mode, Mode::Normal)
        {
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
        let closing_owner = self
            .runtime
            .pane_tabs
            .as_ref()
            .map(|t| t.active_info().claim_owner.clone());
        if let Some(tabs) = self.runtime.pane_tabs.as_mut()
            && !tabs.close_active()
        {
            // Last tab removed.
            self.runtime.pane_tabs = None;
            self.state.focus = state::Focus::FileList;
            self.view.needs_full_repaint = true;
            self.state.flash_info("pane: last tab closed");
        }
        if let Some(owner) = closing_owner {
            self.state.scope_registry.retain(|c| c.owner != owner);
        }
        // The closed tab may have had a stashed scrollback pager whose stream is
        // parked by id; reclaim it now that the tab is gone.
        self.prune_orphaned_pager_streams();
    }

    /// ^a R — restart the active tab's command. Closes the tab and spawns
    /// a fresh one with the same command and working directory.
    pub fn restart_active_tab(&mut self) {
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return;
        };
        let cmd = tabs.active_info().command.clone();
        let cwd = tabs.active_info().cwd.clone();
        // Close the old tab first.
        if let Some(tabs) = self.runtime.pane_tabs.as_mut()
            && !tabs.close_active()
        {
            self.runtime.pane_tabs = None;
            self.state.focus = state::Focus::FileList;
        }
        // The old tab is gone; reclaim its parked scrollback stream (if any)
        // before spawning the replacement (which starts with no stash).
        self.prune_orphaned_pager_streams();
        // Spawn a replacement with the same command and cwd. Only claim
        // "restarted" if it actually spawned — otherwise leave
        // open_pane_tab_in's "pane spawn failed" flash in place rather than
        // clobbering it with a false success (the old tab is already gone).
        if self.open_pane_tab_in(&cmd, &cwd) {
            self.state.flash_info(format!("pane: restarted {cmd}"));
        }
    }

    /// ^W j / ^W k — set keyboard focus directionally (no wrap).
    pub fn set_pane_focus(&mut self, want_pane: bool) {
        // While zoomed, focus is pinned to the zoomed region: the other
        // region is collapsed/off-screen, so moving focus there is exactly the
        // confusing case. Refuse with a hint (mirrors `resize_pane`).
        if self.state.pane.zoom != state::ZoomTarget::None {
            self.state.flash_info("zoomed (^a z to exit)");
            return;
        }
        // From the right split column, `^a j` descends to the bottom pane (when
        // one's open) but *keeps* `vsplit.focus` on the right — so `^a k` from
        // the pane climbs back to the right column it came from, not the left
        // list (the generic path below restores it via `right_column_focused`).
        // `^a k` has nothing above the top-right column → no-op.
        if self.right_column_focused() {
            if want_pane && self.runtime.pane_tabs.is_some() {
                self.state.focus = state::Focus::Pane;
                let label = self
                    .runtime
                    .pane_tabs
                    .as_ref()
                    .map_or("pane", |t| t.active_info().label.as_str());
                self.state.flash_info(format!("focus: {label}"));
                self.view.needs_full_repaint = true;
            }
            return;
        }
        if self.runtime.pane_tabs.is_none() {
            return;
        }
        if self.state.pane_focused() == want_pane {
            return; // already there — no-op
        }
        // The focus decision is a pure fn of these two bits (see
        // `super::focus::decide_focus`); the branch-order contract — every
        // non-Pane arm collapses to `pane_focused() == false` — is pinned by
        // tests there.
        self.state.focus = super::focus::decide_focus(
            super::focus::FocusSnapshot {
                has_top_overlay: self.focused_col_has_overlay(),
                pager_mount: self.focused_top_pager_mount(),
            },
            want_pane,
        );
        if self.state.pane_focused() {
            let label = self
                .runtime
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
            let label = if self.runtime.top_overlay.is_some() {
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
    /// focus, and closes left a stale `Overlay`/`Pager` behind. Behavior-
    /// preserving while routing/render still read only `pane_focused()`: this
    /// only refines the non-`Pane` discriminant, which `pane_focused()` ignores.
    /// Does the FOCUSED vsplit column host a `V`/`D`/`;cmd` overlay PTY? Reads
    /// that column's own slot (`b` → `top_overlay_right`, else `top_overlay`), so
    /// an overlay open only in the OTHER column reads `false` — focus then
    /// belongs to the focused column's list, and `^a l`/`^a h` can move to the
    /// commander beside an open editor/pager.
    pub(super) fn focused_col_has_overlay(&self) -> bool {
        match self.focused_side() {
            state::Side::Right => self.runtime.top_overlay_right.is_some(),
            state::Side::Left => self.runtime.top_overlay.is_some(),
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
            self.view.pager.as_ref().map(|v| v.mount),
            Some(Mount::Overlay)
        ) {
            return Some(Mount::Overlay);
        }
        match self.focused_side() {
            state::Side::Right => self.view.pager_right.as_ref().map(|v| v.mount),
            state::Side::Left => self.view.pager.as_ref().map(|v| v.mount),
        }
    }

    pub(super) fn recompute_focus(&mut self) {
        let want_pane = matches!(self.state.focus, state::Focus::Pane);
        self.state.focus = super::focus::decide_focus(
            super::focus::FocusSnapshot {
                has_top_overlay: self.focused_col_has_overlay(),
                pager_mount: self.focused_top_pager_mount(),
            },
            want_pane,
        );
    }

    /// `:dump-scrollback` diagnostic. Runs the same drain +
    /// snapshot path as `^a-v`, then writes the captured lines as
    /// plain text to `/tmp/spyc-scrollback.txt`. Tail the file to
    /// confirm whether content visible on the live pane (HUD
    /// overlays, etc.) is actually reaching our vt100 emulator at
    /// snapshot time.
    pub fn dump_scrollback_snapshot(&mut self) {
        let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
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
                    .flash_error(format!("dump-scrollback: write failed: {e}")),
            },
            None => self
                .state
                .flash_error("dump-scrollback: no state dir for the output file".to_string()),
        }
    }

    /// ^W + / ^W - — change the bottom pane's share of the middle rect
    /// in 5% steps, clamped to [10%, 90%].
    pub fn resize_pane(&mut self, delta_pct: i32) {
        if self.runtime.pane_tabs.is_none() {
            return;
        }
        if self.state.pane.zoom != state::ZoomTarget::None {
            self.state.flash_info("pane is zoomed (^a z to exit)");
            return;
        }
        let current = i32::from(self.state.pane.pane_height_pct);
        let new = (current + delta_pct).clamp(10, 90);
        self.state.pane.pane_height_pct = new as u16;
        // Force a full clear+redraw: the divider moves, and dirty-frame
        // rendering otherwise leaves a stale row at the old boundary.
        self.view.needs_full_repaint = true;
    }

    /// The pane percentage to use for layout/sizing computations. Zoom drives
    /// it to a sentinel the stored `pane_height_pct` can never reach (it's
    /// clamped to `[10, 90]`): `BottomPane` → 100 (list collapses to 0 rows),
    /// `TopList` → 0 (the pane collapses to a single bottom tab-bar row and
    /// the list fills the rest — `compute_layout` drops the pane rect to
    /// `None` at 0). The stored split is left untouched, so un-zoom restores
    /// it exactly.
    pub const fn effective_pane_pct(&self) -> u16 {
        match self.state.pane.zoom {
            state::ZoomTarget::BottomPane => 100,
            // Both collapse the pane and fill the body: `TopList` renders the
            // list there, `RightColumn` renders the preview (see `render_inner`).
            state::ZoomTarget::TopList | state::ZoomTarget::RightColumn => 0,
            state::ZoomTarget::None => self.state.pane.pane_height_pct,
        }
    }

    /// ^a z / ^w z — toggle "zoom" on the *active* region. Zoom-on
    /// enlarges whichever region currently has focus: the bottom pane
    /// (`BottomPane` — list collapses to 0 rows) or the file list
    /// (`TopList` — the pane collapses off-screen, the list fills the
    /// frame). Toggling again restores the prior split. Focus is left
    /// where it is — it already names the zoomed region, so no save/restore
    /// is needed (this is the fix for the old "zoom always grabbed the
    /// bottom pane" bug). The stored `pane_height_pct` is preserved across
    /// the round-trip. No-op (with a flash) when the pane is closed.
    pub fn toggle_pane_zoom(&mut self) {
        // Need *something* zoomable: a pane or a vertical split.
        if self.runtime.pane_tabs.is_none() && self.state.vsplit.is_none() {
            self.state.flash_info("nothing to zoom");
            return;
        }
        if self.state.pane.zoom != state::ZoomTarget::None {
            self.state.pane.zoom = state::ZoomTarget::None;
            self.state.flash_info("zoom: off");
        } else if self.right_column_focused() {
            // Right split column focused → fullscreen the preview.
            self.state.pane.zoom = state::ZoomTarget::RightColumn;
            self.state.flash_info("zoom: b (^a z to exit)");
        } else if self.state.pane_focused() {
            self.state.pane.zoom = state::ZoomTarget::BottomPane;
            self.state.flash_info("zoom: pane (^a z to exit)");
        } else {
            // File list (or the left split column) focused.
            self.state.pane.zoom = state::ZoomTarget::TopList;
            self.state.flash_info("zoom: list (^a z to exit)");
        }
        // Resize every pane pty to the new layout so child shells re-render at
        // the right dimensions; otherwise Claude's UI is the wrong size until
        // the next terminal resize.
        self.resize_panes_to_layout();
        self.view.needs_full_repaint = true;
    }

    /// Resize every pane pty to the current layout's pane rect, so child
    /// shells re-render at the right size after a zoom / fullscreen change.
    /// No-op when the layout has no pane rect — `TopList` zoom collapses the
    /// pane to a tab bar (`compute_layout` yields `pane: None` at pct 0), so
    /// the pty keeps its prior size, ready the instant the pane is revealed.
    pub(super) fn resize_panes_to_layout(&mut self) {
        let (cols, rows) = self.view.term_size;
        let area = ratatui::layout::Rect::new(0, 0, cols, rows);
        let pane_open = self.runtime.pane_tabs.is_some() && !self.state.pane.pane_hidden;
        let layout = Self::compute_layout(
            area,
            pane_open,
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        if let (Some(pane_rect), Some(tabs)) = (layout.pane, self.runtime.pane_tabs.as_mut()) {
            for entry in tabs.tabs_mut() {
                let _ = entry.pane.resize(pane_rect.height, pane_rect.width);
            }
        }
    }

    /// After a tab switch from a fullscreen list (`TopList` zoom), fullscreen
    /// the newly-active tab (flip to `BottomPane` zoom) — so `^a n`/`^a p`/`^a #`
    /// navigate between fullscreen views instead of flipping a hidden pane.
    /// No-op when not list-zoomed.
    pub(super) fn fullscreen_tab_if_list_zoomed(&mut self) {
        if self.state.pane.zoom == state::ZoomTarget::TopList {
            self.state.pane.zoom = state::ZoomTarget::BottomPane;
            self.resize_panes_to_layout();
        }
    }

    /// Compute the (rows, cols) the bottom pane will occupy.
    pub fn pane_spawn_size(height_pct: u16, status_position: StatusPosition) -> (u16, u16) {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let area = ratatui::layout::Rect::new(0, 0, cols, rows);
        let layout = Self::compute_layout(area, true, height_pct, status_position);
        match layout.pane {
            Some(r) => (r.height.max(1), r.width.max(1)),
            None => (rows.saturating_sub(3).max(1), cols.max(1)),
        }
    }

    /// Compute the (rows, cols) available for the top-overlay pty. This
    /// is the top area: everything above the divider (or the whole
    /// screen minus the prompt row if no bottom pane).
    pub fn top_overlay_size(pane_height_pct: u16, has_bottom_pane: bool) -> (u16, u16) {
        let (cols, total_rows) = crossterm::terminal::size().unwrap_or((80, 24));
        if !has_bottom_pane {
            // Full screen minus prompt row.
            return (total_rows.saturating_sub(1).max(1), cols.max(1));
        }
        // With bottom pane: top region = total - divider(1) - bottom pane.
        let usable = total_rows.saturating_sub(1); // minus divider
        let bottom = (u32::from(usable) * u32::from(pane_height_pct) / 100) as u16;
        let top = usable.saturating_sub(bottom);
        (top.max(1), cols.max(1))
    }
}

/// True when scrollback contains a known Claude/bun crash signature.
/// These markers don't appear in healthy Claude startup output.
fn pane_has_crash_marker(lines: &[String]) -> bool {
    const MARKERS: &[&str] = &[
        // bun's single-file runtime path; appears in unhandled-exception dumps.
        "/$bunfs/root/",
        // e.g. `g9H is not a function` on the resume path regression.
        "is not a function",
        // sandbox helper failed and `failIfUnavailable` is set.
        "Error: sandbox required but unavailable",
    ];
    lines
        .iter()
        .any(|line| MARKERS.iter().any(|m| line.contains(m)))
}
