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
    /// Parse and dispatch a `:` command.
    ///
    /// Pure-domain arms are handled by `AppState::dispatch_command`;
    /// terminal-touching arms (shell, pager, overlay) stay here.
    pub fn dispatch_command(&mut self, input: &str) -> Vec<Effect> {
        use super::state::{PagerLines, Update};

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
                let PagerLines::Plain(lines) = req.lines;
                self.view.pager = Some(PagerView::new_plain(req.title, lines));
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
                    let expanded =
                        crate::shell::expand_percent(&cmd, &self.state.selection_paths());
                    self.start_capture(&expanded, &cmd, &cmd);
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
            self.state.last_captured_cmd = Some(cmd.to_string());
            let expanded = crate::shell::expand_percent(cmd, &self.state.selection_paths());
            self.start_capture(&expanded, cmd, cmd);
            return Vec::new();
        }

        // :;<cmd> — foreground shell command
        if let Some(cmd) = input.strip_prefix(';') {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                self.state.flash_error("empty command");
                return Vec::new();
            }
            let expanded = crate::shell::expand_percent(cmd, &self.state.selection_paths());
            let (rows, cols) =
                Self::top_overlay_size(self.effective_pane_pct(), self.runtime.pane_tabs.is_some());
            let cwd = self.state.listing.dir.clone();
            let wake = self.make_pane_wake();
            match Pane::spawn(&expanded, rows, cols, &cwd, &self.view.context_path, wake) {
                Ok(p) => {
                    self.runtime.top_overlay = Some(p);
                    // Initial focus is on the new overlay so the user
                    // can drive the subprocess directly. ^a-j hands
                    // focus to the bottom pane (when one is open).
                    self.state.focus = Focus::Overlay;
                }
                Err(e) => self.state.flash_error(format!("spawn: {e}")),
            }
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
            // Pure names were resolved above; anything unregistered is unknown.
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
    app.undo_last_remove();
    Vec::new()
}

/// `:date` — flash current date/time. Used to be bound to `D` but `D` now opens
/// the cursor file in $PAGER; the date utility lives on as a typed command.
pub(super) fn cmd_date(app: &mut App, _args: &str) -> Vec<Effect> {
    let _ = app.apply(&Action::Date);
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

/// `:graveyard` — open the graveyard viewer (typed alias for `gy`).
pub(super) fn cmd_graveyard(app: &mut App, _args: &str) -> Vec<Effect> {
    app.state.open_graveyard_view();
    Vec::new()
}

/// `:fg [N]` — bring a backgrounded task back to the foreground. No arg =
/// most-recently-backgrounded task; numeric arg = id.
pub(super) fn cmd_fg(app: &mut App, args: &str) -> Vec<Effect> {
    if args.is_empty() {
        app.foreground_task(None);
    } else {
        match args.parse::<u32>() {
            Ok(id) => app.foreground_task(Some(id)),
            Err(_) => app
                .state
                .flash_error(format!("fg: expected task id (got {args:?})")),
        }
    }
    Vec::new()
}

/// `:task-to-pane [N]` — promote a backgrounded `!` task to a new pane tab. The
/// pty keeps running through the transition; we attach a vt100 emulator and
/// register it in `pane_tabs`. No arg = most-recent task; numeric arg = id.
pub(super) fn cmd_task_to_pane(app: &mut App, args: &str) -> Vec<Effect> {
    if args.is_empty() {
        app.promote_task_to_pane(None);
    } else {
        match args.parse::<u32>() {
            Ok(id) => app.promote_task_to_pane(Some(id)),
            Err(_) => app
                .state
                .flash_error(format!("task-to-pane: expected task id (got {args:?})")),
        }
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
    if args.is_empty() {
        app.open_task_viewer(None);
    } else {
        match args.parse::<u32>() {
            Ok(id) => app.open_task_viewer(Some(id)),
            Err(_) => app
                .state
                .flash_error(format!("task: expected task id (got {args:?})")),
        }
    }
    Vec::new()
}

/// `:pause [N]` — pause a backgrounded task via SIGSTOP to its process group.
/// No arg = most-recent task; numeric = id.
pub(super) fn cmd_pause(app: &mut App, args: &str) -> Vec<Effect> {
    if args.is_empty() {
        app.pause_task(None)
    } else if let Ok(id) = args.parse::<u32>() {
        app.pause_task(Some(id))
    } else {
        app.state
            .flash_error(format!("pause: expected task id (got {args:?})"));
        Vec::new()
    }
}

/// `:resume [N]` — resume a paused backgrounded task via SIGCONT.
pub(super) fn cmd_resume(app: &mut App, args: &str) -> Vec<Effect> {
    if args.is_empty() {
        app.resume_task(None)
    } else if let Ok(id) = args.parse::<u32>() {
        app.resume_task(Some(id))
    } else {
        app.state
            .flash_error(format!("resume: expected task id (got {args:?})"));
        Vec::new()
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
