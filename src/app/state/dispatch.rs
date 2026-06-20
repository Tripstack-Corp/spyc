//! `AppState` command/prompt dispatch: `dispatch_command` (the `:`-command
//! pure transition) and `dispatch_prompt`. Split from `state` verbatim.

use std::path::PathBuf;

use crate::app::command_table::{CmdLayer, command_layer};
use crate::app::{Effect, Mode, Prompt, PromptKind};

use super::{AppState, CommandResult, PromptResult};

impl AppState {
    /// Handle the pure-domain arms of `:` commands.
    ///
    /// (See [`COMMAND_TABLE`] for the canonical registry of base names — the
    /// `NotHandled` routing at the bottom of this fn derives from it, so the
    /// two can't drift.)
    ///
    /// Returns `CommandResult::Handled` when the command was fully processed,
    /// `CommandResult::OpenPager` when the caller should open the pager with
    /// the supplied lines, or `CommandResult::NotHandled` when the caller
    /// (which owns the terminal) must process it.
    pub fn dispatch_command(&mut self, input: &str) -> CommandResult {
        let input = input.trim();
        if input.is_empty() {
            return CommandResult::Handled;
        }

        // :q / :quit — defer to App so the path matches Action::Quit
        // exactly (double-tap confirm, running-process warning, and
        // save_session on confirm). Setting should_quit from
        // pure-domain would skip pane teardown + session persistence.
        // The typed `Quit` variant forces the App-side match to
        // handle it; dropping that arm is a compile error.
        if input == "q" || input == "quit" {
            return CommandResult::Quit;
        }

        // :limit [pattern]
        if input == "limit" {
            self.cur_mut().temp_filter = None;
            self.flash_info("limit cleared");
            self.rebuild_rows();
            return CommandResult::Handled;
        }
        if let Some(pat) = input.strip_prefix("limit ") {
            let pat = pat.trim();
            if pat.is_empty() {
                self.cur_mut().temp_filter = None;
                self.flash_info("limit cleared");
            } else if pat == "!" {
                self.cur_mut().temp_filter = Some("!".to_string());
                self.flash_info("limit: picks only");
            } else {
                self.cur_mut().temp_filter = Some(pat.to_string());
                self.flash_info(format!("limit: {pat}"));
            }
            self.rebuild_rows();
            return CommandResult::Handled;
        }

        // :cd [<path>] — chdir via the synchronous `Effect::ChangeDir` (PR9:
        // the `CommandResult::Post` carrier keeps this pure-domain arm free of
        // the blocking listing read; `run_effects` runs it via `change_dir`,
        // flashing `"cd: {e}"` on failure — same as the former inline match).
        if input == "cd" {
            // Honor a `:setenv HOME=…` override (consistent with `gh`/Home and
            // the shell spawn); envset::var falls back to the real environment.
            let home = crate::envset::var("HOME").unwrap_or_else(|| "/".into());
            return CommandResult::Post(vec![Effect::ChangeDir {
                path: PathBuf::from(home),
                focus: None,
                on_ok: None,
                err_prefix: "cd",
            }]);
        }
        if let Some(raw) = input.strip_prefix("cd ") {
            let raw = raw.trim();
            if raw.is_empty() {
                self.flash_error("cd: missing path");
                return CommandResult::Handled;
            }
            return CommandResult::Post(vec![Effect::ChangeDir {
                path: crate::paths::expand(raw),
                focus: None,
                on_ok: None,
                err_prefix: "cd",
            }]);
        }

        // :sort [mode]
        if input == "sort" {
            self.flash_info(format!("sort: {}", self.cur().sort_order));
            return CommandResult::Handled;
        }
        if let Some(rest) = input.strip_prefix("sort ") {
            let rest = rest.trim();
            // `:sort reverse` / `:sort -` toggles direction.
            if rest == "reverse" || rest == "-" {
                self.cur_mut().sort_reversed = !self.cur().sort_reversed;
                self.apply_sort();
                self.flash_info(format!(
                    "sort: {}{}",
                    self.cur().sort_order,
                    if self.cur().sort_reversed {
                        " (reversed)"
                    } else {
                        ""
                    },
                ));
                return CommandResult::Handled;
            }
            match crate::fs::listing::SortMode::parse(rest) {
                Some(mode) => {
                    self.cur_mut().sort_order = mode;
                    self.apply_sort();
                    self.flash_info(format!(
                        "sort: {mode}{}",
                        if self.cur().sort_reversed {
                            " (reversed)"
                        } else {
                            ""
                        },
                    ));
                }
                None => self.flash_error(format!(
                    "unknown sort mode: {rest} (name|size|mtime|ext|reverse)"
                )),
            }
            return CommandResult::Handled;
        }

        // :version
        if input == "version" {
            self.flash_info(format!(
                "\u{1f336}\u{fe0f} spyc {}",
                env!("CARGO_PKG_VERSION")
            ));
            return CommandResult::Handled;
        }

        // :whoami — flash user@host.
        if input == "whoami" {
            self.flash_info(self.user_host.clone());
            return CommandResult::Handled;
        }

        // :startdir [.|<path>] — manage the `` ` `` jump target.
        if input == "startdir" {
            self.flash_info(format!("start dir: {}", self.start_dir.display()));
            return CommandResult::Handled;
        }
        if let Some(arg) = input.strip_prefix("startdir ") {
            match self.resolve_dir_arg(arg.trim()) {
                Ok(canon) => {
                    self.flash_info(format!("start dir: {}", canon.display()));
                    self.start_dir = canon;
                }
                Err(e) => self.flash_error(format!("startdir: {e}")),
            }
            return CommandResult::Handled;
        }

        // :project [.|<path>|clear] — manage PROJECT_HOME.
        if input == "project" {
            self.flash_info(format!("PROJECT_HOME: {}", self.project_home_display()));
            return CommandResult::Handled;
        }
        if let Some(arg) = input.strip_prefix("project ") {
            let arg = arg.trim();
            if arg == "clear" {
                self.project_home = None;
                self.flash_info("PROJECT_HOME cleared");
                return CommandResult::Handled;
            }
            match self.resolve_dir_arg(arg) {
                Ok(canon) => {
                    self.flash_info(format!("PROJECT_HOME: {}", canon.display()));
                    self.project_home = Some(canon);
                }
                Err(e) => self.flash_error(format!("project: {e}")),
            }
            return CommandResult::Handled;
        }

        // :name [NEW] — rename session, or print current name when bare.
        if input == "name" {
            self.flash_info(format!("session name: {}", self.session_display()));
            return CommandResult::Handled;
        }
        if let Some(arg) = input.strip_prefix("name ") {
            match crate::state::session_names::normalize(arg) {
                Some(norm) => {
                    self.flash_info(format!("session name: {norm}"));
                    self.session_name = Some(norm);
                }
                None => self.flash_error("name: empty after normalization"),
            }
            return CommandResult::Handled;
        }

        // :marks
        if input == "marks" {
            if self.marks.entries.is_empty() {
                self.flash_info("no marks set");
                return CommandResult::Handled;
            }
            let lines: Vec<String> = self
                .marks
                .entries
                .iter()
                .map(|(key, mark)| {
                    let focus = match &mark.focus {
                        Some(p) => format!("  → {}", p.display()),
                        None => String::new(),
                    };
                    format!("  {key}  {}{focus}", mark.dir.display())
                })
                .collect();
            return CommandResult::OpenPager {
                title: "marks".to_string(),
                lines,
            };
        }

        // :set key=value
        if input == "set" {
            self.flash_error("usage: :set key=value");
            return CommandResult::Handled;
        }
        if let Some(assignment) = input.strip_prefix("set ") {
            let assignment = assignment.trim();
            if let Some((key, value)) = assignment.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "sort" => match crate::fs::listing::SortMode::parse(value) {
                        Some(mode) => {
                            self.cur_mut().sort_order = mode;
                            self.apply_sort();
                            self.flash_info(format!("sort={mode}"));
                        }
                        None => self.flash_error(format!("invalid sort mode: {value}")),
                    },
                    _ => self.flash_error(format!("unknown setting: {key}")),
                }
            } else {
                self.flash_error("usage: :set key=value");
            }
            return CommandResult::Handled;
        }

        // Symbol-prefixed shell commands (free-form args) are App-handled;
        // they don't name-complete and aren't in the registry.
        if input.starts_with('!') || input.starts_with(';') {
            return CommandResult::NotHandled;
        }

        // Named commands: the registry decides (MVU Phase 6 — replaces the
        // former hand-maintained allowlist). An `App`-layer name passes
        // through for `App::dispatch_command`; a `Pure`-layer name matched
        // its arm above and already returned, so it can't reach here.
        // Anything not in the table is genuinely unknown.
        let name = input.split_whitespace().next().unwrap_or("");
        if matches!(command_layer(name), Some(CmdLayer::App)) {
            return CommandResult::NotHandled;
        }

        self.flash_error(format!("unknown command: {input}"));
        CommandResult::Handled
    }

    /// Handle the pure-domain arms of prompt submission.
    ///
    /// Returns `PromptResult::Handled` when fully processed, or
    /// `PromptResult::NotHandled` when the caller must handle it (terminal I/O).
    pub fn dispatch_prompt(&mut self, kind: &PromptKind, buffer: &str) -> PromptResult {
        match kind {
            PromptKind::PatternPick => {
                match glob::Pattern::new(buffer) {
                    Ok(pat) => {
                        // Collect first: the entries read borrows `cur()`, which
                        // would clash with the `cur_mut()` insert in the loop body.
                        let matched: Vec<std::path::PathBuf> = self
                            .cur()
                            .listing
                            .entries
                            .iter()
                            .filter(|e| pat.matches(&e.name))
                            .map(|e| e.path.clone())
                            .collect();
                        for path in &matched {
                            self.cur_mut().picks.insert(path);
                        }
                        self.cur_mut().list_generation = self.cur().list_generation.wrapping_add(1);
                    }
                    // Don't swallow an invalid glob — tell the user why nothing
                    // got picked instead of silently no-op'ing.
                    Err(e) => self.flash_error(format!("bad pattern: {e}")),
                }
                PromptResult::Handled
            }
            PromptKind::Search { .. } => {
                if !buffer.is_empty() {
                    self.last_search = Some(buffer.to_string());
                }
                PromptResult::Handled
            }
            PromptKind::SetEnv => {
                let line = buffer.trim();
                if let Some((name, value)) = line.split_once('=') {
                    let name = name.trim();
                    if name.is_empty() {
                        self.flash_error("setenv: missing variable name");
                    } else {
                        // Record a runtime override instead of mutating the
                        // process env: `std::env::set_var` is unsound now
                        // that worker threads may read env concurrently.
                        // `envset` layers this over the real environment and
                        // is merged into every child spawn. See `crate::envset`.
                        crate::envset::set(name, value);
                        self.flash_info(format!("setenv {name}={value}"));
                    }
                } else if !line.is_empty() {
                    self.flash_error("setenv: expected NAME=VALUE");
                }
                PromptResult::Handled
            }
            PromptKind::Limit => {
                let pattern = buffer.trim();
                if pattern.is_empty() {
                    self.cur_mut().temp_filter = None;
                    self.flash_info("limit cleared");
                } else if pattern == "!" {
                    self.cur_mut().temp_filter = Some("!".to_string());
                    self.flash_info("limit: picks only");
                } else if pattern == "h" || pattern == "harpoon" {
                    if self.harpoon_filter_set.is_empty() {
                        self.flash_error(
                            "harpoon empty (or PROJECT_HOME unset) — nothing to filter",
                        );
                        return PromptResult::Handled;
                    }
                    self.cur_mut().temp_filter = Some("h".to_string());
                    self.flash_info("limit: harpoon");
                } else if pattern == "git" || pattern == "g" {
                    if self.git.files.is_empty() {
                        self.flash_error("not in a git repo (or no changes)");
                        return PromptResult::Handled;
                    }
                    self.cur_mut().temp_filter = Some("git".to_string());
                    self.flash_info("limit: git changes");
                } else {
                    self.cur_mut().temp_filter = Some(pattern.to_string());
                    self.flash_info(format!("limit: {pattern}"));
                }
                self.rebuild_rows();
                PromptResult::Handled
            }
            PromptKind::PaneNewTabCmd => {
                let cmd = buffer.trim().to_string();
                if cmd.is_empty() {
                    return PromptResult::Handled;
                }
                self.pending_new_tab_cmd = Some(cmd);
                let cwd_default = self.cur().listing.dir.display().to_string();
                let mut p = Prompt::shell(PromptKind::PaneNewTabCwd, "pane cwd: ");
                p.buffer.clone_from(&cwd_default);
                if let Some(ed) = p.editor.as_mut() {
                    ed.set_content(&cwd_default);
                }
                self.mode = Mode::Prompting(p);
                PromptResult::Handled
            }
            PromptKind::RemoveConfirm
            | PromptKind::ClaudeCrashRecover { .. }
            | PromptKind::GraveyardPurgeAllConfirm => PromptResult::Handled,
            // These need terminal/overlay/pager — caller handles them.
            // Terminal-touching — handled by `App::dispatch_prompt`.
            // Jump / MakeDir / Worktree* do unbounded blocking IO (chdir,
            // create_dir_all, git shell-outs); keeping them out of this
            // pure-domain producer is the Stage-3 de-IO. The rest spawn
            // panes / pagers / editors.
            PromptKind::NewFile
            | PromptKind::ShellCmd
            | PromptKind::ShellCmdCaptured
            | PromptKind::CopyTo
            | PromptKind::MoveTo
            | PromptKind::PaneNewTabCwd
            | PromptKind::PaneRenameTab
            | PromptKind::Command
            | PromptKind::Jump
            | PromptKind::SecondCommanderCwd
            | PromptKind::MakeDir
            | PromptKind::WorktreeNewBranch
            | PromptKind::WorktreeDeleteConfirm => PromptResult::NotHandled,
        }
    }
}
