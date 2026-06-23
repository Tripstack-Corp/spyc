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

use crate::shell;
use crate::ui::line_edit::LineEditor;

use super::{App, Effect, Mode, PostAction, TabState, state};

pub struct Prompt {
    pub kind: PromptKind,
    pub prefix: String,
    pub buffer: String,
    /// When set, this prompt uses the vi line editor with history.
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
    /// Confirm removal. Only a single `y`/`Y` keypress proceeds; anything
    /// else (including Enter/Esc) cancels.
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

impl PromptKind {
    /// True when pressing Tab in this prompt completes a filesystem path.
    ///
    /// The single allowlist for path completion: both the simple-prompt
    /// (`handle_prompt_key`) and the vi-prompt (`handle_vi_prompt_key`) Tab
    /// handlers consult this one predicate, so the set can't drift between
    /// them — which it had (the simple list omitted the shell/command kinds).
    ///
    /// The shell/command kinds (`;`/`!`/`:`) take path arguments, so they
    /// complete too. They only ever appear as vi-editor prompts (built via
    /// `Prompt::shell`), so listing them here is harmless for the
    /// simple-prompt caller — those kinds never reach it.
    pub const fn wants_path_completion(&self) -> bool {
        matches!(
            self,
            Self::Jump
                | Self::CopyTo
                | Self::MoveTo
                | Self::MakeDir
                | Self::NewFile
                | Self::PaneNewTabCwd
                | Self::ShellCmd
                | Self::ShellCmdCaptured
                | Self::Command
        )
    }
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
                self.state.cur().listing.dir.clone()
            } else {
                input
            };
            (dir, String::new())
        } else {
            let dir = input.parent().map_or_else(
                || self.state.cur().listing.dir.clone(),
                |p| {
                    if p.as_os_str().is_empty() {
                        self.state.cur().listing.dir.clone()
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
                if dir == self.state.cur().listing.dir {
                    // Local dir — also filter the listing.
                    self.state.cur_mut().temp_filter = Some(format!("{file_prefix}*"));
                    self.state.rebuild_rows();
                }
                self.state.flash_info(cycle_hint(&matches));
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
    /// ([`crate::app::command_table::COMMAND_TABLE`]). Single match: fill the name
    /// plus a trailing space (so the user can keep typing args, or hit Enter
    /// for the no-arg form — `dispatch_command` trims). Common-prefix advance:
    /// fill the shared prefix and flash a count. Otherwise show all matches
    /// and stage cycle state for repeated Tab.
    fn tab_complete_spyc_command(&mut self, prefix: &str) {
        let matches: Vec<String> = crate::app::command_table::completion_command_names()
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
            self.state.flash_info(cycle_hint(&matches));
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
        self.state.flash_info(cycle_hint(&matches));
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

    /// Clear a Tab-completion preview: the cycle state (`tab_state`) **and**
    /// its paired preview filter (`temp_filter`), atomically.
    ///
    /// Only the Tab preview is paired with `tab_state`, so a user-set
    /// `=`/`:limit` filter (which has no `tab_state`) is left untouched —
    /// without this gate, cancelling any unrelated prompt wiped the active
    /// limit filter.
    ///
    /// Called wherever a Tab preview is abandoned: when a prompt is cancelled
    /// or dispatched, **and** on the first non-Tab key after a preview. Keeping
    /// the pair clear atomic is the invariant `cancel_prompt`/`dispatch_prompt`
    /// rely on: if `tab_state` were nulled alone, the preview filter would
    /// outlive the cycle and leak into the listing behind the prompt.
    pub(crate) fn clear_tab_preview(&mut self) {
        if self.view.tab_state.is_some() && self.state.cur().temp_filter.is_some() {
            self.state.cur_mut().temp_filter = None;
            self.state.rebuild_rows();
        }
        self.view.tab_state = None;
    }

    /// Close the prompt without dispatching. Restores search cursor,
    /// clears Tab-applied filters.
    pub fn cancel_prompt(&mut self) {
        let Mode::Prompting(p) = std::mem::replace(&mut self.state.mode, Mode::Normal) else {
            return;
        };
        if let PromptKind::Search { saved_cursor } = p.kind {
            // Restore the FOCUSED column's cursor (search ran on `cur()`).
            self.state.cur_mut().cursor.index = saved_cursor;
            let len = self.state.cur().rows.len();
            self.state.cur_mut().cursor.clamp(len);
        }
        self.clear_tab_preview();
        // Clear any stashed state from the two-step new-tab prompt.
        self.state.pending_new_tab_cmd = None;
    }

    /// Dispatch a submitted prompt.
    ///
    /// Pure-domain arms are handled by `AppState::dispatch_prompt`;
    /// terminal-touching arms (shell, pager, overlay, copy/move) stay here.
    #[allow(clippy::needless_pass_by_value)]
    pub fn dispatch_prompt(&mut self, prompt: Prompt) -> Vec<Effect> {
        use state::Update;

        // Clear a Tab-applied preview filter before dispatching (see
        // `clear_tab_preview` — a user-set `=`/`:limit` filter is preserved).
        self.clear_tab_preview();

        // Try the pure-domain handler first, normalized to the unified
        // `Update` (MVU Stage 3C).
        match Update::from(self.state.dispatch_prompt(&prompt.kind, &prompt.buffer)) {
            Update::Handled(_) => {
                // Some pure-domain prompts can shift PROJECT_HOME; `apply`'s
                // post-action reconcile only fires for `Action` dispatches, so
                // reconcile here too (a cheap no-op when project_home is
                // unchanged). `PromptResult` carries no effects, so the
                // `Handled` payload is always empty.
                self.reconcile_harpoon();
                return Vec::new();
            }
            // `PromptResult` only maps to `Handled`/`Defer`; `OpenPager`/`Quit`
            // are unreachable here — handle defensively (no panic on the input
            // path) should the producer's result type ever widen.
            Update::OpenPager(_) | Update::Quit => return Vec::new(),
            Update::Defer => {}
        }

        // --- Terminal-touching arms ---
        match prompt.kind {
            PromptKind::ShellCmd => {
                self.run_foreground_shell_overlay(&prompt.buffer);
                Vec::new()
            }
            PromptKind::ShellCmdCaptured => {
                // `!` alone repeats the last captured command.
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
                self.run_captured_shell(&cmd, &prompt.buffer);
                Vec::new()
            }
            PromptKind::CopyTo => self.run_selection_to(&prompt.buffer, false),
            PromptKind::MoveTo => self.run_selection_to(&prompt.buffer, true),
            PromptKind::PaneNewTabCwd => {
                let cwd = prompt.buffer.trim().to_string();
                if let Some(cmd) = self.state.pending_new_tab_cmd.take() {
                    let cwd_path = if cwd.is_empty() {
                        self.state
                            .project_home
                            .clone()
                            .unwrap_or_else(|| self.state.cur().listing.dir.clone())
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
                    self.state.cur().listing.dir.join(&target)
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
            // create_dir_all, in-process gix worktree ops — repo opens +
            // checkout writes), so the pure-domain
            // `AppState::dispatch_prompt` punts them here (MVU Stage 3 de-IO).
            // Each reconciles harpoon afterward — they used to return
            // `Handled`, whose App-side path called `reconcile_harpoon`; the
            // worktree arms re-anchor `project_home`, so the reconcile must
            // still run.
            PromptKind::Jump => {
                let trimmed = prompt.buffer.trim();
                if !trimmed.is_empty() {
                    // Surface a bad target (typo'd / nonexistent path) instead of
                    // swallowing it — `jump_to` errors only when the path can't
                    // be resolved (chdir failures already flash inside it).
                    if let Err(e) = self.state.jump_to(trimmed) {
                        self.state.flash_error(format!("jump: {e}"));
                    }
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
                        self.state.cur().listing.dir.join(&target)
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
                // POLA: base the new worktree off PROJECT_HOME's default branch
                // (the trunk), not the focused column's current HEAD.
                let base = self
                    .state
                    .project_home
                    .as_deref()
                    .and_then(crate::git::branch::default_base);
                match crate::git::worktree::add(
                    &self.state.cur().listing.dir,
                    branch,
                    base.as_deref(),
                ) {
                    Ok(path) => {
                        self.state
                            .flash_info(format!("created worktree: {}", path.display()));
                        // chdir the focused column INTO the new worktree so you
                        // can work there now. PROJECT_HOME stays the overall
                        // project anchor — `g w` jumps to this worktree's root,
                        // `g h` to PROJECT_HOME. (Per-column git tracks the
                        // worktree's own markers via PR E.)
                        if let Err(e) = self.state.chdir(&path) {
                            self.state.flash_error(format!("chdir: {e}"));
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
                let dir = self.state.cur().listing.dir.clone();
                match crate::git::worktree::remove(&dir) {
                    Ok(()) => {
                        self.state
                            .flash_info(format!("removed worktree: {}", dir.display()));
                        // chdir the focused column to the parent (the deleted
                        // dir is gone). PROJECT_HOME is left untouched — it's the
                        // overall project anchor, not tied to a worktree.
                        if let Some(parent) = dir.parent() {
                            let _ = self.state.chdir(parent);
                        }
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
/// Build the multi-match Tab-completion flash: candidates joined by two
/// spaces, truncated to the first 12 with a `(+N more)` tail, suffixed
/// with the cycle hint. Shared by the file / command / frecency completion
/// paths, which each reach the "no further shared prefix" branch identically.
fn cycle_hint(matches: &[String]) -> String {
    const MAX_SHOWN: usize = 12;
    let shown = if matches.len() > MAX_SHOWN {
        format!(
            "{}  (+{} more)",
            matches[..MAX_SHOWN]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("  "),
            matches.len() - MAX_SHOWN
        )
    } else {
        matches
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("  ")
    };
    format!("{shown}  — Tab to cycle")
}

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

#[cfg(test)]
mod tests {
    use super::cycle_hint;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn joins_with_two_spaces_and_cycle_suffix() {
        assert_eq!(cycle_hint(&v(&["a", "b"])), "a  b  — Tab to cycle");
    }

    #[test]
    fn shows_all_at_the_twelve_match_boundary() {
        let m = v(&[
            "m1", "m2", "m3", "m4", "m5", "m6", "m7", "m8", "m9", "m10", "m11", "m12",
        ]);
        let hint = cycle_hint(&m);
        assert!(
            !hint.contains("more"),
            "12 matches must not truncate: {hint}"
        );
        assert!(hint.ends_with("  — Tab to cycle"));
    }

    #[test]
    fn truncates_past_twelve_with_more_count() {
        let m: Vec<String> = (0..15).map(|i| format!("m{i}")).collect();
        let hint = cycle_hint(&m);
        assert!(hint.contains("(+3 more)"), "expected +3 more: {hint}");
        assert!(hint.starts_with("m0  m1  "));
        assert!(!hint.contains("m12"), "13th+ match must be hidden: {hint}");
    }

    /// The single path-completion allowlist (`wants_path_completion`) must
    /// cover exactly the path/shell/command kinds and nothing else — the
    /// drift guard for the two Tab handlers that both consult it.
    #[test]
    fn wants_path_completion_is_the_one_allowlist() {
        use super::PromptKind as K;
        for (name, k) in [
            ("Jump", K::Jump),
            ("CopyTo", K::CopyTo),
            ("MoveTo", K::MoveTo),
            ("MakeDir", K::MakeDir),
            ("NewFile", K::NewFile),
            ("PaneNewTabCwd", K::PaneNewTabCwd),
            ("ShellCmd", K::ShellCmd),
            ("ShellCmdCaptured", K::ShellCmdCaptured),
            ("Command", K::Command),
        ] {
            assert!(k.wants_path_completion(), "{name} should path-complete");
        }
        for (name, k) in [
            ("PatternPick", K::PatternPick),
            ("Search", K::Search { saved_cursor: 0 }),
            ("RemoveConfirm", K::RemoveConfirm),
            ("GraveyardPurgeAllConfirm", K::GraveyardPurgeAllConfirm),
            ("SetEnv", K::SetEnv),
            ("PaneNewTabCmd", K::PaneNewTabCmd),
            ("PaneRenameTab", K::PaneRenameTab),
            ("WorktreeNewBranch", K::WorktreeNewBranch),
            ("WorktreeDeleteConfirm", K::WorktreeDeleteConfirm),
            ("Limit", K::Limit),
            ("ClaudeCrashRecover", K::ClaudeCrashRecover { tab_idx: 0 }),
        ] {
            assert!(
                !k.wants_path_completion(),
                "{name} should not path-complete"
            );
        }
    }
}
