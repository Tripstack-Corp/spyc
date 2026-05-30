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

use super::{App, PostAction};

impl App {
    /// Parse and dispatch a `:` command.
    ///
    /// Pure-domain arms are handled by `AppState::dispatch_command`;
    /// terminal-touching arms (shell, pager, overlay) stay here.
    pub fn dispatch_command(&mut self, input: &str) -> PostAction {
        use super::state::CommandResult;

        // Try the pure-domain handler first.
        match self.state.dispatch_command(input) {
            CommandResult::Handled => return PostAction::None,
            CommandResult::OpenPager { title, lines } => {
                self.pager = Some(PagerView::new_plain(title, lines));
                return PostAction::None;
            }
            CommandResult::Quit => {
                // Same lifecycle as the Q keybinding: double-tap
                // confirm, running-process warning, save_session on
                // confirm. The typed variant means removing this
                // arm is a compile error — the wiring can't silently
                // regress to "unknown command".
                self.request_quit();
                return PostAction::None;
            }
            CommandResult::NotHandled => {}
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
            return PostAction::None;
        }

        // :!<cmd> — captured shell command
        if let Some(cmd) = input.strip_prefix('!') {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                self.state.flash_error("empty command");
                return PostAction::None;
            }
            self.state.last_captured_cmd = Some(cmd.to_string());
            let expanded = crate::shell::expand_percent(cmd, &self.state.selection_paths());
            self.start_capture(&expanded, cmd, cmd);
            return PostAction::None;
        }

        // :;<cmd> — foreground shell command
        if let Some(cmd) = input.strip_prefix(';') {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                self.state.flash_error("empty command");
                return PostAction::None;
            }
            let expanded = crate::shell::expand_percent(cmd, &self.state.selection_paths());
            let (rows, cols) =
                Self::top_overlay_size(self.effective_pane_pct(), self.pane_tabs.is_some());
            let cwd = self.state.listing.dir.clone();
            match Pane::spawn(&expanded, rows, cols, &cwd, &self.context_path) {
                Ok(p) => {
                    self.top_overlay = Some(p);
                    // Initial focus is on the new overlay so the user
                    // can drive the subprocess directly. ^a-j hands
                    // focus to the bottom pane (when one is open).
                    self.state.pane_focused = false;
                }
                Err(e) => self.state.flash_error(format!("spawn: {e}")),
            }
            return PostAction::None;
        }

        // :undo — restore the most-recent graveyard entry to its
        // original path. The "did I mean to do that?" escape hatch
        // for `R`. No-arg only; if the user wants to pick a
        // specific entry they open the graveyard view (`gy`).
        if input == "undo" {
            self.undo_last_remove();
            return PostAction::None;
        }
        // :date — flash current date/time. Used to be bound to `D` but
        // `D` now opens the cursor file in $PAGER (the common
        // request); the date utility lives on as a typed command for
        // the rare hand-on-keyboard moment you actually want it.
        if input == "date" {
            let _ = self.apply(&Action::Date);
            return PostAction::None;
        }
        // :dump-scrollback — diagnostic for the v1.5 ^a-v
        // snapshot path. Drains the active pane and writes the
        // resulting snapshot (as plain text, one line per row) to
        // /tmp/spyc-scrollback.txt. Useful when content visible on
        // the live pane (e.g. a HUD plugin overlay) seems to go
        // missing in the pager view — `cat /tmp/spyc-scrollback.txt
        // | tail` shows whether the bytes are actually reaching
        // our vt100 emulator at snapshot time.
        if input == "dump-scrollback" {
            self.dump_scrollback_snapshot();
            return PostAction::None;
        }
        // :graveyard — open the graveyard viewer (typed alias for `gy`).
        if input == "graveyard" {
            self.state.open_graveyard_view();
            return PostAction::None;
        }

        // :fg [N] — bring a backgrounded task back to the foreground.
        // No arg = most-recently-backgrounded task; numeric arg = id.
        if input == "fg" {
            self.foreground_task(None);
            return PostAction::None;
        }
        if let Some(arg) = input.strip_prefix("fg ") {
            match arg.trim().parse::<u32>() {
                Ok(id) => self.foreground_task(Some(id)),
                Err(_) => self
                    .state
                    .flash_error(format!("fg: expected task id (got {arg:?})")),
            }
            return PostAction::None;
        }

        // :task-to-pane [N] — promote a backgrounded `!` task to a
        // new pane tab (v1.5 Phase 6b). The pty keeps running through
        // the transition; we just attach a vt100 emulator to it and
        // register it in `pane_tabs`. Useful when an `!` task you
        // started turns out to need persistent attention (e.g.
        // `npm run dev`) — promote it next to claude instead of
        // shuttling through `:fg` / `^z`. No arg = most-recent task;
        // numeric arg = specific id.
        if input == "task-to-pane" {
            self.promote_task_to_pane(None);
            return PostAction::None;
        }
        if let Some(arg) = input.strip_prefix("task-to-pane ") {
            match arg.trim().parse::<u32>() {
                Ok(id) => self.promote_task_to_pane(Some(id)),
                Err(_) => self
                    .state
                    .flash_error(format!("task-to-pane: expected task id (got {arg:?})")),
            }
            return PostAction::None;
        }

        // :pane-to-task [N] — demote a pane tab to a background
        // task (v1.5 Phase 6c). Inverse of `:task-to-pane`. The
        // pty keeps running; we just stop displaying it, and the
        // user can return to it via `:fg` or `:task-to-pane`. No
        // arg = active tab; numeric arg = 1-indexed tab number
        // (matches the divider's `[1]` `[2]` labels).
        if input == "pane-to-task" {
            self.demote_pane_to_task();
            return PostAction::None;
        }
        if let Some(arg) = input.strip_prefix("pane-to-task ") {
            match arg.trim().parse::<usize>() {
                Ok(n) if n >= 1 => {
                    let idx = n - 1;
                    let len = self.pane_tabs.as_ref().map_or(0, PaneTabs::len);
                    if idx >= len {
                        self.state
                            .flash_error(format!("pane-to-task: no tab #{n} (have {len})"));
                    } else {
                        if let Some(tabs) = self.pane_tabs.as_mut() {
                            tabs.switch_to(idx);
                        }
                        self.demote_pane_to_task();
                    }
                }
                _ => self
                    .state
                    .flash_error(format!("pane-to-task: expected tab number (got {arg:?})")),
            }
            return PostAction::None;
        }

        // :grep <pattern> — project-wide content search. Walks
        // PROJECT_HOME (or the current listing dir if unset),
        // gitignore-aware, results stream into a pager as
        // `path:line:col: text` so gf/gF jumps to the file.
        if input == "grep" {
            self.state.flash_error("grep: pattern required");
            return PostAction::None;
        }
        if let Some(pattern) = input.strip_prefix("grep ") {
            let pattern = pattern.trim();
            if pattern.is_empty() {
                self.state.flash_error("grep: pattern required");
            } else {
                self.open_grep_pager(pattern);
            }
            return PostAction::None;
        }

        // :task [N] — open the task viewer (peek mode). No arg picks
        // the most-recent task; numeric arg targets a specific id.
        if input == "task" {
            self.open_task_viewer(None);
            return PostAction::None;
        }
        if let Some(arg) = input.strip_prefix("task ") {
            match arg.trim().parse::<u32>() {
                Ok(id) => self.open_task_viewer(Some(id)),
                Err(_) => self
                    .state
                    .flash_error(format!("task: expected task id (got {arg:?})")),
            }
            return PostAction::None;
        }

        // :pause [N] — pause a backgrounded task via SIGSTOP to its
        // process group. No arg = most-recent task; numeric = id.
        if input == "pause" {
            self.pause_task(None);
            return PostAction::None;
        }
        if let Some(arg) = input.strip_prefix("pause ") {
            match arg.trim().parse::<u32>() {
                Ok(id) => self.pause_task(Some(id)),
                Err(_) => self
                    .state
                    .flash_error(format!("pause: expected task id (got {arg:?})")),
            }
            return PostAction::None;
        }

        // :resume [N] — resume a paused backgrounded task via SIGCONT.
        if input == "resume" {
            self.resume_task(None);
            return PostAction::None;
        }
        if let Some(arg) = input.strip_prefix("resume ") {
            match arg.trim().parse::<u32>() {
                Ok(id) => self.resume_task(Some(id)),
                Err(_) => self
                    .state
                    .flash_error(format!("resume: expected task id (got {arg:?})")),
            }
            return PostAction::None;
        }

        // :bprev / :bnext — pager buffer history
        if input == "bprev" {
            if let Some(current) = self.pager.take() {
                match self.pager_history.go_back(current) {
                    Ok(prev) => {
                        self.pager = Some(prev);
                        self.needs_full_repaint = true;
                        let back = self.pager_history.back_len();
                        let fwd = self.pager_history.forward_len();
                        self.state.flash_info(format!("buffer ←{back} →{fwd}"));
                    }
                    Err(current) => {
                        // At the start of history -- keep the current
                        // pager visible instead of closing it.
                        self.pager = Some(current);
                        self.state.flash_info("no older buffers");
                    }
                }
            } else if let Some(prev) = self.pager_history.pop_back() {
                self.pager = Some(prev);
                self.needs_full_repaint = true;
                self.state
                    .flash_info(format!("buffer ←{}", self.pager_history.back_len()));
            } else {
                self.state.flash_info("no buffers in history");
            }
            return PostAction::None;
        }
        if input == "bnext" {
            if let Some(current) = self.pager.take() {
                match self.pager_history.go_forward(current) {
                    Ok(next) => {
                        self.pager = Some(next);
                        self.needs_full_repaint = true;
                        let back = self.pager_history.back_len();
                        let fwd = self.pager_history.forward_len();
                        self.state.flash_info(format!("buffer ←{back} →{fwd}"));
                    }
                    Err(current) => {
                        self.pager = Some(current);
                        self.state.flash_info("no newer buffers");
                    }
                }
            } else {
                self.state.flash_info("no pager open");
            }
            return PostAction::None;
        }

        // If we get here, AppState said NotHandled but we don't recognize it
        // either — this shouldn't happen, but handle gracefully.
        self.state.flash_error(format!("unknown command: {input}"));
        PostAction::None
    }
}
