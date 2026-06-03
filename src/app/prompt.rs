//! The one-line input prompt: `Prompt` (kind + prefix + buffer +
//! optional vi line editor) and `PromptKind` (the full vocabulary of
//! prompt contexts). Extracted verbatim from `app/mod.rs`
//! (REFACTOR_PLAN Phase 1) and re-exported from `crate::app` so the
//! existing `super::{Prompt, PromptKind}` imports in `state`/`route`
//! keep resolving. The `simple`/`shell` ctors are `pub` because both
//! `app` and its sibling `state` module construct prompts.
//!
//! The `impl App` block (added by the impl-extraction sweep) holds the
//! prompt-completion + submit/cancel methods: Tab-completion of paths /
//! `:`-commands / frecency jumps, `cancel_prompt`, and `dispatch_prompt`
//! (the terminal-touching half of prompt submission; the pure-domain
//! half lives in `AppState::dispatch_prompt`). `tab_complete_path`,
//! `cancel_prompt`, and `dispatch_prompt` are `pub` (called from
//! `key_dispatch` / `actions`); the spyc-command / frecency completers
//! and the `common_prefix` helper are module-internal.

use crate::fs;
use crate::shell;
use crate::ui::line_edit::LineEditor;

use super::{App, Effect, Mode, Pane, PostAction, TabState, state};

pub struct Prompt {
    pub kind: PromptKind,
    pub prefix: String,
    pub buffer: String,
    /// When set, this prompt uses the vi line editor with history.
    #[allow(dead_code)]
    pub editor: Option<LineEditor>,
}

impl Prompt {
    /// Simple prompt (pattern pick, search, jump, etc.) — no vi editing.
    pub fn simple(kind: PromptKind, prefix: impl Into<String>) -> Self {
        Self {
            kind,
            prefix: prefix.into(),
            buffer: String::new(),
            editor: None,
        }
    }

    /// Shell prompt (`!` / `;`) — vi line editor with history support.
    pub fn shell(kind: PromptKind, prefix: impl Into<String>) -> Self {
        Self {
            kind,
            prefix: prefix.into(),
            buffer: String::new(),
            editor: Some(LineEditor::new()),
        }
    }
}

pub enum PromptKind {
    PatternPick,
    ShellCmd,
    /// Incremental search. `saved_cursor` is where the cursor was when `/`
    /// was pressed, so Esc can restore it.
    Search {
        saved_cursor: usize,
    },
    Jump,
    CopyTo,
    MoveTo,
    MakeDir,
    NewFile,
    /// Confirm removal. Only `y` / `yes` (case-insensitive) proceeds;
    /// anything else is treated as a cancel.
    RemoveConfirm,
    /// Confirm purge-all from the graveyard view (cascade
    /// everything to system trash). Same single-key shape as
    /// RemoveConfirm; routed separately because the verb and
    /// destination are different.
    GraveyardPurgeAllConfirm,
    SetEnv,
    /// `!` — capture command output with ANSI colors, show in in-app pager.
    ShellCmdCaptured,
    /// New pane tab step 1: command to run.
    PaneNewTabCmd,
    /// New pane tab step 2: working directory.
    PaneNewTabCwd,
    /// Rename the active pane tab.
    PaneRenameTab,
    /// W n — branch name for new worktree.
    WorktreeNewBranch,
    /// W d — confirm worktree removal (y/N).
    WorktreeDeleteConfirm,
    /// `=` — temporary file list filter (glob pattern, `!` for picks, empty clears).
    Limit,
    /// `:` — vim-style command line.
    Command,
    /// Auto-fired when a restored `claude --resume` tab looks broken;
    /// y/Enter respawns into the same slot. Cwd and fallback command
    /// live on the tab's `TabInfo` and are read at confirm time.
    ClaudeCrashRecover {
        tab_idx: usize,
    },
}

impl App {
    /// Tab-complete a filesystem path in the prompt buffer. For shell
    /// prompts, completes just the last whitespace-delimited word.
    pub fn tab_complete_path(&mut self) {
        // Extract data from prompt without holding the borrow.
        let (is_shell, is_jump, is_command, buffer) = {
            let Mode::Prompting(ref prompt) = self.state.mode else {
                return;
            };
            let is_shell = matches!(
                prompt.kind,
                PromptKind::ShellCmd | PromptKind::ShellCmdCaptured | PromptKind::Command
            );
            let is_jump = matches!(prompt.kind, PromptKind::Jump);
            let is_command = matches!(prompt.kind, PromptKind::Command);
            (is_shell, is_jump, is_command, prompt.buffer.clone())
        };

        // Repeated Tab with active cycle state: cycle through matches
        // or re-flash the list for local dirs.
        if let Some(ref mut ts) = self.view.tab_state
            && (ts.original_buf == buffer || ts.cycle_index > 0)
            && ts.matches.len() > 1
        {
            // Cycle to next match, fill it in.
            let idx = ts.cycle_index % ts.matches.len();
            let completed = format!("{}{}{}", ts.buf_prefix, ts.word_base, ts.matches[idx]);
            ts.cycle_index = idx + 1;
            let flash = format!("{} — {}/{}", ts.matches[idx], idx + 1, ts.matches.len());
            self.state.flash_info(flash);
            let Mode::Prompting(ref mut prompt) = self.state.mode else {
                return;
            };
            prompt.buffer = completed;
            if let Some(ed) = prompt.editor.as_mut() {
                ed.set_content(&prompt.buffer);
            }
            return;
        }

        // `:` prompt with no whitespace yet — complete the spyc command
        // name from the command registry (`COMMAND_TABLE`) rather than
        // falling through to filesystem completion (which would try to match
        // a file starting with "pa" in cwd, almost never useful here).
        if is_command && !buffer.contains(char::is_whitespace) {
            self.tab_complete_spyc_command(&buffer);
            return;
        }

        // For shell prompts, extract just the last word for completion.
        let (buf_prefix, word) = if is_shell {
            let last_space = buffer.rfind(' ').map_or(0, |i| i + 1);
            (
                buffer[..last_space].to_string(),
                buffer[last_space..].to_string(),
            )
        } else {
            (String::new(), buffer)
        };

        let input = crate::paths::expand(&word);
        let input_str = input.to_string_lossy().to_string();
        let (dir, file_prefix) = if input_str.ends_with('/') || input_str.is_empty() {
            let dir = if input_str.is_empty() {
                self.state.listing.dir.clone()
            } else {
                input
            };
            (dir, String::new())
        } else {
            let dir = input.parent().map_or_else(
                || self.state.listing.dir.clone(),
                |p| {
                    if p.as_os_str().is_empty() {
                        self.state.listing.dir.clone()
                    } else {
                        p.to_path_buf()
                    }
                },
            );
            let name = input
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            (dir, name)
        };

        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        let mut matches: Vec<String> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(&file_prefix) {
                    let is_dir = e.file_type().is_ok_and(|ft| ft.is_dir());
                    let suffix = if is_dir { "/" } else { "" };
                    Some(format!("{name}{suffix}"))
                } else {
                    None
                }
            })
            .collect();
        matches.sort();

        if matches.is_empty() {
            // No filesystem matches — try frecency for Jump prompts.
            if is_jump {
                self.frecency_complete(&word, &buf_prefix);
            }
            return;
        }

        let word_base = if word.ends_with('/') || word.is_empty() {
            word.clone()
        } else {
            let last_sep = word.rfind('/').map_or(0, |i| i + 1);
            word[..last_sep].to_string()
        };

        let (completed_word, flash) = if matches.len() == 1 {
            (format!("{word_base}{}", matches[0]), None)
        } else {
            let common = common_prefix(&matches);
            if common.len() > file_prefix.len() {
                let msg = format!("{} matches", matches.len());
                (format!("{word_base}{common}"), Some(msg))
            } else {
                // No text progress — show matches and set up cycle state.
                let display: Vec<&str> = matches.iter().map(std::string::String::as_str).collect();
                let shown = if display.len() > 12 {
                    format!(
                        "{}  (+{} more)",
                        display[..12].join("  "),
                        display.len() - 12
                    )
                } else {
                    display.join("  ")
                };
                if dir == self.state.listing.dir {
                    // Local dir — also filter the listing.
                    self.state.temp_filter = Some(format!("{file_prefix}*"));
                    self.state.rebuild_rows();
                    self.state.flash_info(format!("{shown}  — Tab to cycle"));
                } else {
                    self.state.flash_info(format!("{shown}  — Tab to cycle"));
                }
                let Mode::Prompting(ref prompt) = self.state.mode else {
                    return;
                };
                self.view.tab_state = Some(TabState {
                    original_buf: prompt.buffer.clone(),
                    buf_prefix: buf_prefix.clone(),
                    word_base,
                    matches,
                    cycle_index: 0,
                });
                return;
            }
        };

        if let Some(msg) = flash {
            self.state.flash_info(msg);
        }

        let Mode::Prompting(ref mut prompt) = self.state.mode else {
            return;
        };
        prompt.buffer = format!("{buf_prefix}{completed_word}");
        if let Some(ed) = prompt.editor.as_mut() {
            ed.set_content(&prompt.buffer);
        }
        // Store cycle state for multi-match (common prefix advanced but
        // further Tabs should still be able to cycle).
        if matches.len() > 1 {
            self.view.tab_state = Some(TabState {
                original_buf: prompt.buffer.clone(),
                buf_prefix,
                word_base,
                matches,
                cycle_index: 0,
            });
        } else {
            self.view.tab_state = None;
        }
    }

    /// Tab-complete a `:` command base name from the command registry
    /// ([`crate::app::state::COMMAND_TABLE`]). Single match: fill the name
    /// plus a trailing space (so the user can keep typing args, or hit Enter
    /// for the no-arg form — `dispatch_command` trims). Common-prefix advance:
    /// fill the shared prefix and flash a count. Otherwise show all matches
    /// and stage cycle state for repeated Tab.
    fn tab_complete_spyc_command(&mut self, prefix: &str) {
        let matches: Vec<String> = crate::app::state::completion_command_names()
            .filter(|c| c.starts_with(prefix))
            .map(str::to_string)
            .collect();
        if matches.is_empty() {
            return;
        }

        if matches.len() == 1 {
            let buffer = format!("{} ", matches[0]);
            let Mode::Prompting(ref mut prompt) = self.state.mode else {
                return;
            };
            if let Some(ed) = prompt.editor.as_mut() {
                ed.set_content(&buffer);
            }
            prompt.buffer = buffer;
            self.view.tab_state = None;
            return;
        }

        let common = common_prefix(&matches);
        if common.len() > prefix.len() {
            // Filled some chars but more matches remain — stage cycle
            // state so a follow-up Tab on the same buffer can rotate.
            let display: Vec<&str> = matches.iter().map(String::as_str).collect();
            let shown = if display.len() > 12 {
                format!(
                    "{}  (+{} more)",
                    display[..12].join("  "),
                    display.len() - 12
                )
            } else {
                display.join("  ")
            };
            self.state.flash_info(format!("{shown}  — Tab to cycle"));
            let Mode::Prompting(ref mut prompt) = self.state.mode else {
                return;
            };
            if let Some(ed) = prompt.editor.as_mut() {
                ed.set_content(&common);
            }
            prompt.buffer = common;
            self.view.tab_state = Some(TabState {
                original_buf: prompt.buffer.clone(),
                buf_prefix: String::new(),
                word_base: String::new(),
                matches,
                cycle_index: 0,
            });
            return;
        }

        // No textual progress — leave the buffer alone, show all
        // matches, and stage cycle state. The cycle path on the next
        // Tab will compare `original_buf == buffer` (true since we
        // didn't change the buffer) and rotate.
        let display: Vec<&str> = matches.iter().map(String::as_str).collect();
        let shown = if display.len() > 12 {
            format!(
                "{}  (+{} more)",
                display[..12].join("  "),
                display.len() - 12
            )
        } else {
            display.join("  ")
        };
        self.state.flash_info(format!("{shown}  — Tab to cycle"));
        self.view.tab_state = Some(TabState {
            original_buf: prefix.to_string(),
            buf_prefix: String::new(),
            word_base: String::new(),
            matches,
            cycle_index: 0,
        });
    }

    /// Frecency fallback for the J prompt: when filesystem completion finds
    /// no matches, search the frecency database for directories matching
    /// the typed fragment.
    fn frecency_complete(&mut self, word: &str, buf_prefix: &str) {
        let hits = self.state.frecency.search(word);
        if hits.is_empty() {
            return;
        }

        // Convert to display strings with trailing slash.
        let names: Vec<String> = hits
            .iter()
            .map(|p| format!("{}/", p.to_string_lossy()))
            .collect();

        if names.len() == 1 {
            // Single match — fill it in directly.
            let completed = format!("{buf_prefix}{}", names[0]);
            let Mode::Prompting(ref mut prompt) = self.state.mode else {
                return;
            };
            prompt.buffer = completed;
            if let Some(ed) = prompt.editor.as_mut() {
                ed.set_content(&prompt.buffer);
            }
            self.view.tab_state = None;
        } else {
            // Multiple frecency matches — fill best, set up cycling.
            let completed = format!("{buf_prefix}{}", names[0]);
            self.state
                .flash_info(format!("{} — 1/{} frecency", names[0], names.len()));
            let Mode::Prompting(ref mut prompt) = self.state.mode else {
                return;
            };
            let original = prompt.buffer.clone();
            prompt.buffer = completed;
            if let Some(ed) = prompt.editor.as_mut() {
                ed.set_content(&prompt.buffer);
            }
            self.view.tab_state = Some(TabState {
                original_buf: original,
                buf_prefix: buf_prefix.to_string(),
                word_base: String::new(),
                matches: names,
                cycle_index: 1, // already showing first match
            });
        }
    }

    /// Close the prompt without dispatching. Restores search cursor,
    /// clears Tab-applied filters.
    pub fn cancel_prompt(&mut self) {
        let Mode::Prompting(p) = std::mem::replace(&mut self.state.mode, Mode::Normal) else {
            return;
        };
        if let PromptKind::Search { saved_cursor } = p.kind {
            self.state.cursor.index = saved_cursor;
            self.state.cursor.clamp(self.state.rows.len());
        }
        // Clear any Tab-applied filter (search or shell prompt).
        if self.state.temp_filter.is_some() {
            self.state.temp_filter = None;
            self.state.rebuild_rows();
        }
        self.view.tab_state = None;
        // Clear any stashed state from the two-step new-tab prompt.
        self.state.pending_new_tab_cmd = None;
    }

    /// Dispatch a submitted prompt.
    ///
    /// Pure-domain arms are handled by `AppState::dispatch_prompt`;
    /// terminal-touching arms (shell, pager, overlay, copy/move) stay here.
    #[allow(clippy::needless_pass_by_value)]
    pub fn dispatch_prompt(&mut self, prompt: Prompt) -> Vec<Effect> {
        use state::PromptResult;

        // Clear any Tab-applied filter before dispatching.
        if self.state.temp_filter.is_some() {
            self.state.temp_filter = None;
            self.state.rebuild_rows();
        }
        self.view.tab_state = None;

        // Try the pure-domain handler first.
        match self.state.dispatch_prompt(&prompt.kind, &prompt.buffer) {
            PromptResult::Handled => {
                // Some pure-domain prompts shift PROJECT_HOME (e.g.
                // `WorktreeNewBranch` chdirs into the new worktree
                // and re-anchors). `apply`'s post-action
                // reconciliation only fires for `Action` dispatches,
                // not prompt submissions — call directly so harpoon
                // reloads on prompts that move us between project
                // roots. The call is cheap when project_home is
                // unchanged (compares paths and returns early).
                self.reconcile_harpoon();
                return Vec::new();
            }
            PromptResult::NotHandled => {}
        }

        // --- Terminal-touching arms ---
        match prompt.kind {
            PromptKind::ShellCmd => {
                let expanded = shell::expand_percent(&prompt.buffer, &self.state.selection_paths());
                let (rows, cols) = Self::top_overlay_size(
                    self.effective_pane_pct(),
                    self.runtime.pane_tabs.is_some(),
                );
                let cwd = self.state.listing.dir.clone();
                let wake = self.make_pane_wake();
                match Pane::spawn(&expanded, rows, cols, &cwd, &self.view.context_path, wake) {
                    Ok(p) => {
                        self.runtime.top_overlay = Some(p);
                        self.state.focus = state::Focus::Overlay;
                    }
                    Err(e) => self.state.flash_error(format!("spawn: {e}")),
                }
                Vec::new()
            }
            PromptKind::ShellCmdCaptured => {
                let cmd = if prompt.buffer.trim() == "!" {
                    if let Some(c) = self.state.last_captured_cmd.clone() {
                        c
                    } else {
                        self.state.flash_error("no previous ! command");
                        return Vec::new();
                    }
                } else {
                    prompt.buffer.clone()
                };
                self.state.last_captured_cmd = Some(cmd.clone());
                let expanded = shell::expand_percent(&cmd, &self.state.selection_paths());
                self.start_capture(&expanded, &cmd, &prompt.buffer);
                Vec::new()
            }
            PromptKind::CopyTo => {
                self.run_selection_to(&prompt.buffer, fs::ops::copy_selection_to, "copied");
                Vec::new()
            }
            PromptKind::MoveTo => {
                self.run_selection_to(&prompt.buffer, fs::ops::move_selection_to, "moved");
                Vec::new()
            }
            PromptKind::PaneNewTabCwd => {
                let cwd = prompt.buffer.trim().to_string();
                if let Some(cmd) = self.state.pending_new_tab_cmd.take() {
                    let cwd_path = if cwd.is_empty() {
                        self.state
                            .project_home
                            .clone()
                            .unwrap_or_else(|| self.state.listing.dir.clone())
                    } else if cwd.starts_with('~') {
                        let home = std::env::var("HOME").unwrap_or_default();
                        std::path::PathBuf::from(cwd.replacen('~', &home, 1))
                    } else {
                        std::path::PathBuf::from(&cwd)
                    };
                    self.open_pane_tab_in(&cmd, &cwd_path);
                }
                Vec::new()
            }
            PromptKind::PaneRenameTab => {
                let name = prompt.buffer.trim().to_string();
                if !name.is_empty()
                    && let Some(tabs) = self.runtime.pane_tabs.as_mut()
                {
                    tabs.active_info_mut().label = name;
                }
                Vec::new()
            }
            PromptKind::NewFile => {
                let name = prompt.buffer.trim().to_string();
                if name.is_empty() {
                    return Vec::new();
                }
                let target = crate::paths::expand(&name);
                let resolved = if target.is_absolute() {
                    target
                } else {
                    self.state.listing.dir.join(&target)
                };
                // Create parent dirs if needed, then touch the file.
                if let Some(parent) = resolved.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if !resolved.exists() {
                    let _ = std::fs::write(&resolved, "");
                }
                let mut argv = shell::resolve_editor();
                if argv.is_empty() {
                    self.state.flash_error("$EDITOR not set");
                    return Vec::new();
                }
                let program = argv.remove(0);
                argv.push(resolved.to_string_lossy().into_owned());
                PostAction::Spawn {
                    program,
                    args: argv,
                    pause_after: false,
                }
                .into()
            }
            PromptKind::Command => self.dispatch_command(&prompt.buffer),
            // Jump / MakeDir / Worktree do unbounded blocking IO (chdir,
            // create_dir_all, git shell-outs), so the pure-domain
            // `AppState::dispatch_prompt` punts them here (MVU Stage 3 de-IO).
            // Each reconciles harpoon afterward — they used to return
            // `Handled`, whose App-side path called `reconcile_harpoon`; the
            // worktree arms re-anchor `project_home`, so the reconcile must
            // still run.
            PromptKind::Jump => {
                let trimmed = prompt.buffer.trim();
                if !trimmed.is_empty() {
                    let _ = self.state.jump_to(trimmed);
                }
                self.reconcile_harpoon();
                Vec::new()
            }
            PromptKind::MakeDir => {
                let name = prompt.buffer.trim();
                if !name.is_empty() {
                    let target = crate::paths::expand(name);
                    let resolved = if target.is_absolute() {
                        target
                    } else {
                        self.state.listing.dir.join(&target)
                    };
                    match std::fs::create_dir_all(&resolved) {
                        Ok(()) => self
                            .state
                            .flash_info(format!("created {}", resolved.display())),
                        Err(e) => self.state.flash_error(format!("error: {e}")),
                    }
                    self.state.refresh_listing();
                }
                self.reconcile_harpoon();
                Vec::new()
            }
            PromptKind::WorktreeNewBranch => {
                let branch = prompt.buffer.trim();
                if branch.is_empty() {
                    return Vec::new();
                }
                match crate::sysinfo::git_worktree_add(&self.state.listing.dir, branch) {
                    Ok(path) => {
                        self.state
                            .flash_info(format!("created worktree: {}", path.display()));
                        if let Err(e) = self.state.chdir(&path) {
                            self.state.flash_error(format!("chdir: {e}"));
                        } else {
                            // Re-anchor PROJECT_HOME on the new worktree
                            // (harpoon / grep / MCP context want the worktree
                            // root, not the parent repo); reconcile_harpoon
                            // below reloads the per-project harpoon list.
                            self.state.project_home = Some(self.state.listing.dir.clone());
                        }
                    }
                    Err(e) => self.state.flash_error(format!("worktree add: {e}")),
                }
                self.reconcile_harpoon();
                Vec::new()
            }
            PromptKind::WorktreeDeleteConfirm => {
                let confirmed = prompt.buffer.trim().eq_ignore_ascii_case("y");
                if !confirmed {
                    return Vec::new();
                }
                let dir = self.state.listing.dir.clone();
                // Capture the main repo path *before* removing — once the
                // worktree's directory is gone we can't `git worktree list`
                // from inside it, and the chdir-to-parent below lands in a
                // non-git dir, so PROJECT_HOME would have nothing to reanchor
                // on. The main worktree is the first `git worktree list
                // --porcelain` entry.
                let main_repo = crate::sysinfo::git_worktree_list(&dir)
                    .and_then(|wts| wts.into_iter().next().map(|wt| wt.path));
                match crate::sysinfo::git_worktree_remove(&dir) {
                    Ok(()) => {
                        self.state
                            .flash_info(format!("removed worktree: {}", dir.display()));
                        if let Some(parent) = dir.parent() {
                            let _ = self.state.chdir(parent);
                        }
                        // Re-anchor PROJECT_HOME on the main repo so harpoon /
                        // MCP context / `gh` don't keep pointing at the
                        // just-deleted directory. The chdir target stays the
                        // parent (the user may be browsing sibling worktrees);
                        // listing.dir and project_home can differ, that's normal.
                        self.state.project_home = main_repo;
                    }
                    Err(e) => self.state.flash_error(format!("worktree remove: {e}")),
                }
                self.reconcile_harpoon();
                Vec::new()
            }
            // These should have been handled by AppState — unreachable in practice.
            _ => Vec::new(),
        }
    }
}

/// Longest common prefix of a slice of strings (byte-safe for UTF-8).
fn common_prefix(strings: &[String]) -> String {
    let Some(first) = strings.first() else {
        return String::new();
    };
    let mut byte_len = first.len();
    for s in &strings[1..] {
        byte_len = byte_len.min(s.len());
        for ((i, a), b) in first.char_indices().zip(s.chars()) {
            if a != b {
                byte_len = byte_len.min(i);
                break;
            }
        }
    }
    first[..byte_len].to_string()
}
