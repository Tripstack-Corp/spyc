//! Colon-command dispatch: `App::dispatch_command`, the terminal-touching
//! half of the `:` command surface (shell-out, pager, overlay, task/pane
//! lifecycle). The pure-domain arms live in `AppState::dispatch_command`
//! (`src/app/state.rs`); this method tries that first, then handles the
//! arms that need terminal / pane / session access.
//!
//! Extracted from `app/mod.rs` (REFACTOR_PLAN Phase 2), same child-module
//! `impl App` pattern as `render` / `pager_handler`: reads App's private
//! state via the descendant rule, `pub` because `dispatch_prompt` in
//! `app` calls it.

use crate::keymap::Action;
use crate::pane::{Pane, PaneTabs};
use crate::ui::pager::PagerView;

use super::command_table::{self, CmdHandler};
use super::state::Focus;
use super::{App, Effect};

impl App {
    /// Run `cmd` as a foreground shell overlay that PERSISTS after exit (no
    /// auto-dismiss — the user reads the output), expanding `%`/selection
    /// placeholders first. Shared by the `:;cmd` command and the `;`-prompt
    /// (`ShellCmd`) arms — they had independent copies of this spawn block.
    /// Flashes on expand/spawn error.
    pub(super) fn run_foreground_shell_overlay(&mut self, cmd: &str) {
        let expanded = match crate::shell::expand_percent(cmd, &self.state.selection_paths()) {
            Ok(s) => s,
            Err(e) => {
                self.state.flash_error(e.to_string());
                return;
            }
        };
        let (rows, cols) =
            Self::top_overlay_size(self.effective_pane_pct(), self.runtime.pane_tabs.is_some());
        let cwd = self.state.cur().listing.dir.clone();
        let wake = self.make_pane_wake();
        match Pane::spawn(&expanded, rows, cols, &cwd, &self.view.context_path, wake) {
            Ok(p) => {
                // Initial focus on the new overlay so the user drives the
                // subprocess directly; `^a j` hands focus to the bottom pane.
                self.runtime.top_overlay = Some(p);
                self.state.focus = Focus::Overlay;
            }
            Err(e) => self.state.flash_error(format!("spawn: {e}")),
        }
    }

    /// Run `cmd` as a captured (background-able) shell command: record it as the
    /// last `!` command, expand `%`/selection placeholders, and hand off to
    /// `start_capture` with `display` as the task label. Shared by the `:!cmd`
    /// command and the `!`-prompt (`ShellCmdCaptured`) arms. Flashes on error.
    pub(super) fn run_captured_shell(&mut self, cmd: &str, display: &str) {
        self.state.last_captured_cmd = Some(cmd.to_string());
        match crate::shell::expand_percent(cmd, &self.state.selection_paths()) {
            Ok(expanded) => self.start_capture(&expanded, cmd, display),
            Err(e) => self.state.flash_error(e.to_string()),
        }
    }

    /// Parse and dispatch a `:` command.
    ///
    /// Pure-domain arms are handled by `AppState::dispatch_command`;
    /// terminal-touching arms (shell, pager, overlay) stay here.
    pub fn dispatch_command(&mut self, input: &str) -> Vec<Effect> {
        use super::state::Update;

        // A `spyc.command`-registered `:`-command (from init.lua) isn't in the
        // static COMMAND_TABLE, and the pure `AppState::dispatch_command` can't
        // see the runtime Lua registry — for an unregistered name it flashes
        // "unknown command" and returns `Handled`, short-circuiting this method
        // before any App-layer arm runs. So resolve a Lua command FIRST, but
        // only when the name isn't a built-in table command (built-ins keep
        // precedence — a Lua command can't shadow one).
        let (lua_name, _) = command_table::split_name_args(input.trim());
        if command_table::lookup(lua_name).is_none()
            && let Some(effects) = self.dispatch_lua_command(lua_name)
        {
            return effects;
        }

        // Try the pure-domain handler first, normalized to the unified
        // `Update` (MVU Stage 3C). `Handled`/`Post` collapse into
        // `Handled(effects)` (the run loop executes whatever's there).
        match Update::from(self.state.dispatch_command(input)) {
            Update::Handled(effects) => return effects,
            Update::OpenPager(req) => {
                // Command-path pagers (`:marks`) assigned `self.view.pager`
                // directly via `new_plain` — preserve that exactly (no
                // `set_pager` / `remember_pager_position`), rebuilt from the
                // normalized request (columns 1, no fit = `new_plain`
                // defaults, so byte-identical to the old call).
                self.view.pager = Some(PagerView::new_plain(req.title, req.lines));
                return Vec::new();
            }
            Update::Quit => {
                // Same lifecycle as the Q keybinding (double-tap confirm,
                // running-process warning, save_session on confirm). The
                // typed variant keeps the wiring a compile-time check.
                self.request_quit();
                return Vec::new();
            }
            Update::Defer => {}
        }

        // --- Terminal-touching arms (shell/pager/overlay) ---
        let input = input.trim();

        // :!! — repeat last captured command
        if input == "!!" || input == "!" {
            match self.state.last_captured_cmd.clone() {
                Some(cmd) => {
                    match crate::shell::expand_percent(&cmd, &self.state.selection_paths()) {
                        Ok(expanded) => self.start_capture(&expanded, &cmd, &cmd),
                        Err(e) => self.state.flash_error(e.to_string()),
                    }
                }
                None => self.state.flash_error("no previous ! command"),
            }
            return Vec::new();
        }

        // :!<cmd> — captured shell command
        if let Some(cmd) = input.strip_prefix('!') {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                self.state.flash_error("empty command");
                return Vec::new();
            }
            self.run_captured_shell(cmd, cmd);
            return Vec::new();
        }

        // :;<cmd> — foreground shell command
        if let Some(cmd) = input.strip_prefix(';') {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                self.state.flash_error("empty command");
                return Vec::new();
            }
            self.run_foreground_shell_overlay(cmd);
            return Vec::new();
        }

        // Named commands: compile-checked table dispatch. A `CmdHandler::App`
        // entry can't exist without naming its handler fn, so a missing arm is
        // a build error rather than a runtime "unknown command" flash. Pure-
        // domain names were resolved above; anything unregistered is unknown.
        let (name, args) = command_table::split_name_args(input);
        if let Some(CmdHandler::App(handler)) =
            command_table::lookup(name).map(|spec| &spec.handler)
        {
            handler(self, args)
        } else {
            // Pure names were resolved above; Lua `:`-commands were resolved at
            // the top of this method (before the pure short-circuit); anything
            // unregistered is genuinely unknown.
            self.state.flash_error(format!("unknown command: {input}"));
            Vec::new()
        }
    }
}

// ── App-layer `:command` handlers (registered in COMMAND_TABLE) ─────────────
// Each receives the trimmed argument string (everything after the command
// name); no-arg commands ignore it. Terminal-touching — they live next to
// `dispatch_command`, and the registry names them so a missing handler is a
// build error. Behavior mirrors the former `if input == "…"` / `strip_prefix`
// arms; the only change is that no-arg commands now tolerate trailing args
// instead of flashing "unknown" (a benign relaxation, no test depends on it).

/// `:undo` — restore the most-recent graveyard entry to its original path. The
/// "did I mean to do that?" escape hatch for `R`.
pub(super) fn cmd_undo(app: &mut App, _args: &str) -> Vec<Effect> {
    app.undo_last_remove()
}

/// `:date` — flash current date/time. Used to be bound to `D` but `D` now opens
/// the cursor file in $PAGER; the date utility lives on as a typed command.
pub(super) fn cmd_date(app: &mut App, _args: &str) -> Vec<Effect> {
    let _ = app.apply(&Action::Date);
    Vec::new()
}

/// `:why-status` — explain the active tab's agent-activity classification
/// (debug aid, `docs/AGENT_AWARENESS_PLAN.md`): the current state, its **source**
/// (a semantic `report_status` self-report vs the output-timing fallback), and
/// how long since its last pane output. App-layer (reads the live pane tabs +
/// clock).
pub(super) fn cmd_why_status(app: &mut App, _args: &str) -> Vec<Effect> {
    use crate::pane::AgentActivity;
    use crate::state::sessions::AgentKind;
    let Some(tabs) = app.runtime.pane_tabs.as_ref() else {
        app.state.flash_info("why-status: no pane open");
        return Vec::new();
    };
    let info = tabs.active_info();
    let is_agent = crate::agent::detect(&info.command).kind() != AgentKind::Other;
    let age = match info.last_output_at {
        Some(at) => format!("{:.1}s since last output", at.elapsed().as_secs_f32()),
        None => "no output yet".to_string(),
    };
    let msg = if is_agent {
        let state = match info.activity {
            AgentActivity::Working => "working",
            AgentActivity::Idle => "idle",
            AgentActivity::Blocked => "blocked",
            AgentActivity::Done => "done",
            AgentActivity::Unknown => "unknown",
        };
        // Priority: a live self-report wins, then the P1-2 scrape fallback,
        // then output timing (`effective_activity`'s exact order).
        let source = if info.reported.is_some() {
            "self-reported".to_string()
        } else if let Some((_, hint)) = info.scrape_status {
            match hint {
                Some(h) => format!("scrape-fallback: {h}"),
                None => "scrape-fallback".to_string(),
            }
        } else {
            "output-timing".to_string()
        };
        format!("why-status [{}]: {state} ({source}) — {age}", info.label)
    } else {
        format!("why-status: '{}' is not a known agent — no dot", info.label)
    };
    app.state.flash_info(msg);
    Vec::new()
}

/// `:notify test` — fire every notification channel on demand (bell, the
/// spice-heat visual border pulse, and BOTH desktop mechanisms: the OS notifier
/// plus an OSC-9 escape), bypassing `[notify]` gating so you can verify each
/// without waiting for a real agent transition (`docs/AGENT_AWARENESS_PLAN.md`).
/// Any other argument prints usage. App-layer (reads the clock to start the pulse).
pub(super) fn cmd_notify(app: &mut App, args: &str) -> Vec<Effect> {
    if args.trim() != "test" {
        app.state.flash_info("usage: :notify test");
        return Vec::new();
    }
    // Start the border pulse now; `settle_visual_bell` animates + clears it (it
    // re-arms its own deadline, so the command doesn't need `ctx`).
    app.view.visual_bell = Some(crate::app::VisualBell {
        start: std::time::Instant::now(),
        frame: 0,
    });
    app.state
        .flash_info("notify test: fired bell + visual + desktop (system + osc9)");
    vec![Effect::Notify {
        system: Some((
            "spyc".to_string(),
            "notification test — all channels".to_string(),
        )),
        osc9: Some("spyc — notification test (all channels)".to_string()),
        bell: true,
    }]
}

/// `:hooks [on|on!|off]` — manage agent status-hook consent for the current
/// project: the escape hatch from the first-launch popup (e.g. an accidental
/// `no`). `on` grants consent + installs the hooks for any running claude/codex
/// panes in this project (Claude live-reloads them → next message; codex reads
/// config at startup → next launch); `on!` additionally **restarts the active
/// claude pane and resumes** it, for when the live reload doesn't take (claude
/// only); `off` revokes + uninstalls; no arg reports the current state. Consent
/// is keyed by the project root and saved.
pub(super) fn cmd_hooks(app: &mut App, args: &str) -> Vec<Effect> {
    match args.trim() {
        "on!" | "enable!" => app.force_restart_status_hooks(),
        "on" | "enable" | "yes" | "y" => app.set_status_hooks(true),
        "off" | "disable" | "no" | "n" => app.set_status_hooks(false),
        "" | "status" => {
            let root = app.status_hooks_target_root();
            let state = match crate::state::hook_consent::consent_for(&root) {
                Some(true) => "on",
                Some(false) => "off",
                None => "unset (asks on next claude/codex launch)",
            };
            app.state.flash_info(format!(
                "status hooks: {state} for {} — `:hooks on|on!|off` to change",
                crate::paths::display_tilde(&root)
            ));
        }
        other => app
            .state
            .flash_error(format!("usage: :hooks on|on!|off  (got `{other}`)")),
    }
    Vec::new()
}

/// `:dump-scrollback` — diagnostic for the ^a-v snapshot path. Drains the active
/// pane and writes the snapshot (one line per row) to /tmp/spyc-scrollback.txt;
/// useful when content visible on the live pane seems to go missing in the pager
/// — `cat /tmp/spyc-scrollback.txt | tail` shows whether the bytes reach our
/// vt100 emulator at snapshot time.
pub(super) fn cmd_dump_scrollback(app: &mut App, _args: &str) -> Vec<Effect> {
    app.dump_scrollback_snapshot();
    Vec::new()
}

/// `:graveyard` — open the graveyard viewer (the default key was dropped in the
/// keymap slim). Routes through the action so its entry-hint flash fires.
pub(super) fn cmd_graveyard(app: &mut App, _args: &str) -> Vec<Effect> {
    app.apply(&Action::OpenGraveyardView).unwrap_or_default()
}

// `:activity` / `:longlist` / `:filetype` / `:chmod` — typed entry points for
// features that are losing (or have lost) their default key, so they stay
// reachable and re-bindable (`map KEY command <name>`). Each just runs the
// same `Action` the key fired, returning its effects to the caller.

/// `:activity` — toggle the activity monitor overlay (was `A`). `:activity dump`
/// instead opens a saveable pager with a per-pane breakdown of how every agent
/// tab's dot is derived (the `:why-status` reasoning for ALL panes at once) —
/// the `source` line is the crux: a live `report_status` self-report vs the
/// output-timing fallback. Easy to yank/save and paste when debugging the dots.
pub(super) fn cmd_activity(app: &mut App, args: &str) -> Vec<Effect> {
    if args.trim() == "dump" {
        let mut view = PagerView::new_plain("activity dump", activity_dump_lines(app));
        view.saveable = true;
        app.set_pager(view);
        return Vec::new();
    }
    app.apply(&Action::ToggleActivity).unwrap_or_default()
}

/// Build the `:activity dump` report (see [`cmd_activity`]). Pure read of the
/// live tabs + activity tallies → plain lines; no I/O, no mutation.
fn activity_dump_lines(app: &App) -> Vec<String> {
    use crate::pane::AgentActivity;
    use crate::state::sessions::AgentKind;

    let state_str = |a: AgentActivity| match a {
        AgentActivity::Working => "working",
        AgentActivity::Idle => "idle",
        AgentActivity::Blocked => "blocked",
        AgentActivity::Done => "done",
        AgentActivity::Unknown => "unknown",
    };

    let now = std::time::Instant::now();
    let mut out = vec![format!(
        "spyc {} (pid {}) — activity dump @ {}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        crate::sysinfo::format_now(),
    )];
    // `report_status:N` here is the key signal: how many status reports
    // (hook-driven OR agent-driven) actually reached spyc this session.
    let calls = &app.view.activity.mcp_tool_calls;
    let tally: Vec<String> = calls
        .iter()
        .filter(|(_, c)| **c > 0)
        .map(|(n, c)| format!("{n}:{c}"))
        .collect();
    out.push(format!(
        "mcp tool calls: {}",
        if tally.is_empty() {
            "(none yet)".to_string()
        } else {
            tally.join("  ")
        }
    ));
    out.push(String::new());

    let Some(tabs) = app.runtime.pane_tabs.as_ref() else {
        out.push("(no panes open)".to_string());
        return out;
    };
    let active = tabs.active_index();
    for (i, e) in tabs.tabs().iter().enumerate() {
        let info = &e.info;
        let is_agent = crate::agent::detect(&info.command).kind() != AgentKind::Other;
        let marker = if i == active { '*' } else { ' ' };
        out.push(format!(
            "{marker}[{}] \"{}\"  dot={}  agent={is_agent}  suspended={}",
            i + 1,
            info.label,
            state_str(info.activity),
            info.suspended,
        ));
        out.push(format!("    command: {}", info.command));
        out.push(format!("    cwd: {}", info.cwd.display()));
        // The crux: live self-report > P1-2 scrape fallback > output timing.
        match (info.reported, info.scrape_status) {
            (Some(r), _) => out.push(format!(
                "    source: SELF-REPORT status={} set {:.1}s ago, expires in {:.0}s",
                state_str(r.status),
                r.at.elapsed().as_secs_f32(),
                r.expiry.saturating_duration_since(now).as_secs_f32(),
            )),
            (None, Some((s, hint))) => out.push(format!(
                "    source: SCRAPE-FALLBACK status={}{}",
                state_str(s),
                match hint {
                    Some(h) => format!(" ({h})"),
                    None => String::new(),
                },
            )),
            (None, None) => {
                out.push("    source: output-timing (no live report)".to_string());
            }
        }
        match info.last_output_at {
            Some(at) => out.push(format!(
                "    last_output: {:.1}s ago",
                at.elapsed().as_secs_f32()
            )),
            None => out.push("    last_output: none".to_string()),
        }
        out.push(format!(
            "    spawn: {:.0}s ago",
            info.spawn_at.elapsed().as_secs_f32()
        ));
        out.push(format!("    pane_id: {}", info.id));
        if let Some(s) = &info.claude_session_id {
            out.push(format!("    claude_session_id: {s}"));
        }
        if let Some(s) = &info.codex_session_id {
            out.push(format!("    codex_session_id: {s}"));
        }
        out.push(String::new());
    }
    out
}

/// `:longlist` — long `ls -lh`-style listing of the selection (was `L`).
pub(super) fn cmd_longlist(app: &mut App, _args: &str) -> Vec<Effect> {
    app.apply(&Action::LongList).unwrap_or_default()
}

/// `:filetype` — run `file(1)` on the selection (was `f`).
pub(super) fn cmd_filetype(app: &mut App, _args: &str) -> Vec<Effect> {
    app.apply(&Action::FileType).unwrap_or_default()
}

/// `:chmod` — `chmod +x` the selection (was `^X`).
pub(super) fn cmd_chmod(app: &mut App, _args: &str) -> Vec<Effect> {
    app.apply(&Action::ChmodAdd('x')).unwrap_or_default()
}

/// `:setenv` — open the `NAME=VALUE` env-var prompt (was `s`). `:set` is for
/// app settings (`:set sort=…`), so the env-var setter gets its own command.
pub(super) fn cmd_setenv(app: &mut App, _args: &str) -> Vec<Effect> {
    app.apply(&Action::SetEnvPrompt).unwrap_or_default()
}

/// Outcome of parsing an optional numeric task-id `:command` argument.
enum TaskIdArg {
    /// No argument — use the command's "most-recent / default" path.
    Default,
    /// A valid numeric id.
    Id(u32),
    /// Non-numeric arg — `parse_task_id_arg` has already flashed the error;
    /// the caller should no-op.
    Invalid,
}

/// Parse the optional task-id argument shared by `:fg` / `:task` /
/// `:task-to-pane` / `:pause` / `:resume`. On a non-numeric arg, flashes
/// "`cmd`: expected task id (got …)" and returns [`TaskIdArg::Invalid`].
fn parse_task_id_arg(app: &mut App, args: &str, cmd: &str) -> TaskIdArg {
    if args.is_empty() {
        TaskIdArg::Default
    } else if let Ok(id) = args.parse::<u32>() {
        TaskIdArg::Id(id)
    } else {
        app.state
            .flash_error(format!("{cmd}: expected task id (got {args:?})"));
        TaskIdArg::Invalid
    }
}

/// `:fg [N]` — bring a backgrounded task back to the foreground. No arg =
/// most-recently-backgrounded task; numeric arg = id.
pub(super) fn cmd_fg(app: &mut App, args: &str) -> Vec<Effect> {
    match parse_task_id_arg(app, args, "fg") {
        TaskIdArg::Default => app.foreground_task(None),
        TaskIdArg::Id(id) => app.foreground_task(Some(id)),
        TaskIdArg::Invalid => {}
    }
    Vec::new()
}

/// `:task-to-pane [N]` — promote a backgrounded `!` task to a new pane tab. The
/// pty keeps running through the transition; we attach a vt100 emulator and
/// register it in `pane_tabs`. No arg = most-recent task; numeric arg = id.
pub(super) fn cmd_task_to_pane(app: &mut App, args: &str) -> Vec<Effect> {
    match parse_task_id_arg(app, args, "task-to-pane") {
        TaskIdArg::Default => app.promote_task_to_pane(None),
        TaskIdArg::Id(id) => app.promote_task_to_pane(Some(id)),
        TaskIdArg::Invalid => {}
    }
    Vec::new()
}

/// `:pane-to-task [N]` — demote a pane tab to a background task (inverse of
/// `:task-to-pane`). The pty keeps running; we stop displaying it. No arg =
/// active tab; numeric arg = 1-indexed tab number (matches the `[1]` `[2]`
/// divider labels).
pub(super) fn cmd_pane_to_task(app: &mut App, args: &str) -> Vec<Effect> {
    if args.is_empty() {
        app.demote_pane_to_task();
        return Vec::new();
    }
    match args.parse::<usize>() {
        Ok(n) if n >= 1 => {
            let idx = n - 1;
            let len = app.runtime.pane_tabs.as_ref().map_or(0, PaneTabs::len);
            if idx >= len {
                app.state
                    .flash_error(format!("pane-to-task: no tab #{n} (have {len})"));
            } else {
                if let Some(tabs) = app.runtime.pane_tabs.as_mut() {
                    tabs.switch_to(idx);
                }
                app.demote_pane_to_task();
            }
        }
        _ => app
            .state
            .flash_error(format!("pane-to-task: expected tab number (got {args:?})")),
    }
    Vec::new()
}

/// `:grep <pattern>` — project-wide content search. Walks PROJECT_HOME (or the
/// current listing dir if unset), gitignore-aware; results stream into a pager
/// as `path:line:col: text` so gf/gF jumps to the file.
pub(super) fn cmd_grep(app: &mut App, args: &str) -> Vec<Effect> {
    if args.is_empty() {
        app.state.flash_error("grep: pattern required");
    } else {
        app.open_grep_pager(args);
    }
    Vec::new()
}

/// `:task [N]` — open the task viewer (peek mode). No arg picks the most-recent
/// task; numeric arg targets a specific id.
pub(super) fn cmd_task(app: &mut App, args: &str) -> Vec<Effect> {
    match parse_task_id_arg(app, args, "task") {
        TaskIdArg::Default => app.open_task_viewer(None),
        TaskIdArg::Id(id) => app.open_task_viewer(Some(id)),
        TaskIdArg::Invalid => {}
    }
    Vec::new()
}

/// `:pause [N]` — pause a backgrounded task via SIGSTOP to its process group.
/// No arg = most-recent task; numeric = id.
pub(super) fn cmd_pause(app: &mut App, args: &str) -> Vec<Effect> {
    match parse_task_id_arg(app, args, "pause") {
        TaskIdArg::Default => app.pause_task(None),
        TaskIdArg::Id(id) => app.pause_task(Some(id)),
        TaskIdArg::Invalid => Vec::new(),
    }
}

/// `:resume [N]` — resume a paused backgrounded task via SIGCONT.
pub(super) fn cmd_resume(app: &mut App, args: &str) -> Vec<Effect> {
    match parse_task_id_arg(app, args, "resume") {
        TaskIdArg::Default => app.resume_task(None),
        TaskIdArg::Id(id) => app.resume_task(Some(id)),
        TaskIdArg::Invalid => Vec::new(),
    }
}

/// `:bprev` — step the pager buffer history backward (older).
pub(super) fn cmd_bprev(app: &mut App, _args: &str) -> Vec<Effect> {
    if let Some(current) = app.view.pager.take() {
        match app.view.pager_history.go_back(current) {
            Ok(prev) => {
                app.view.pager = Some(prev);
                app.view.needs_full_repaint = true;
                let back = app.view.pager_history.back_len();
                let fwd = app.view.pager_history.forward_len();
                app.state.flash_info(format!("buffer ←{back} →{fwd}"));
            }
            Err(current) => {
                // At the start of history -- keep the current pager
                // visible instead of closing it.
                app.view.pager = Some(current);
                app.state.flash_info("no older buffers");
            }
        }
    } else if let Some(prev) = app.view.pager_history.pop_back() {
        app.view.pager = Some(prev);
        app.view.needs_full_repaint = true;
        app.state
            .flash_info(format!("buffer ←{}", app.view.pager_history.back_len()));
    } else {
        app.state.flash_info("no buffers in history");
    }
    Vec::new()
}

/// `:bnext` — step the pager buffer history forward (newer).
pub(super) fn cmd_bnext(app: &mut App, _args: &str) -> Vec<Effect> {
    if let Some(current) = app.view.pager.take() {
        match app.view.pager_history.go_forward(current) {
            Ok(next) => {
                app.view.pager = Some(next);
                app.view.needs_full_repaint = true;
                let back = app.view.pager_history.back_len();
                let fwd = app.view.pager_history.forward_len();
                app.state.flash_info(format!("buffer ←{back} →{fwd}"));
            }
            Err(current) => {
                app.view.pager = Some(current);
                app.state.flash_info("no newer buffers");
            }
        }
    } else {
        app.state.flash_info("no pager open");
    }
    Vec::new()
}
