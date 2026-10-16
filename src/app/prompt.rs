//! The one-line input prompt: `Prompt` (kind + prefix + buffer +
//! optional vi line editor) and `PromptKind` (the full vocabulary of
//! prompt contexts). Extracted verbatim from `app/mod.rs`
//! (REFACTOR_PLAN Phase 1) and re-exported from `crate::app` so the
//! existing `super::{Prompt, PromptKind}` imports in `state`/`route`
//! keep resolving. The `simple`/`shell` ctors are `pub` because both
//! `app` and its sibling `state` module construct prompts.
//!
//! The `impl App` block holds the submit/cancel methods: `cancel_prompt` and
//! `dispatch_prompt` (the terminal-touching half of prompt submission; the
//! pure-domain half lives in `AppState::dispatch_prompt`). Tab completion is
//! `tab_completion`'s.

use crate::shell;
use crate::ui::line_edit::LineEditor;

use super::{App, Effect, Mode, state};

pub struct Prompt {
    pub kind: PromptKind,
    pub prefix: String,
    pub buffer: String,
    /// When set, this prompt uses the vi line editor with history.
    pub editor: Option<LineEditor>,
    /// What an empty buffer submits, drawn dimmed in its place (`J` offers
    /// the last path the pane printed). Typing hides it; `→`/`End` on the
    /// empty buffer loads it for editing. Honoured by the vi-editor prompts.
    pub suggestion: Option<String>,
}

impl Prompt {
    /// Simple prompt (pattern pick, search, jump, etc.) — no vi editing.
    pub fn simple(kind: PromptKind, prefix: impl Into<String>) -> Self {
        Self {
            kind,
            prefix: prefix.into(),
            buffer: String::new(),
            editor: None,
            suggestion: None,
        }
    }

    /// Shell prompt (`!` / `;`) — vi line editor with history support.
    pub fn shell(kind: PromptKind, prefix: impl Into<String>) -> Self {
        Self {
            kind,
            prefix: prefix.into(),
            buffer: String::new(),
            editor: Some(LineEditor::new()),
            suggestion: None,
        }
    }

    /// The renderer's view of this prompt. One constructor for the draw and
    /// for the layout's height reservation, so the two can't wrap differently.
    pub fn line<'a>(
        &'a self,
        theme: &'a crate::ui::theme::Theme,
    ) -> crate::ui::prompt::PromptLine<'a> {
        crate::ui::prompt::PromptLine {
            prefix: &self.prefix,
            buffer: &self.buffer,
            theme,
            cursor_pos: self.editor.as_ref().map(|e| e.cursor),
            vi_mode: self.editor.as_ref().map(|e| e.mode),
            suggestion: self.suggestion.as_deref(),
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
    /// `!` — capture command output with ANSI colours, show in in-app pager.
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
    /// The first time `agent` starts in spyc: wire it into its own user
    /// configuration (status hooks + MCP entry), once, for every project?
    /// `y`/`Y` → remember-allow + install; `n`/`N` → remember-deny. Any other
    /// key (including Esc) defers the prompt without recording anything.
    HookConsent {
        agent: crate::state::sessions::AgentKind,
    },
    /// A project-local `.spycrc.toml` declares startup tabs this user hasn't
    /// answered for in this exact form (`state::tab_consent`). A centred
    /// pop-up listing every command. `y` approves this list and opens it, `n`
    /// declines it (remembered for this list), `Esc` skips it this launch;
    /// both of those open the user's own tabs instead. Anything else re-raises
    /// it, because a stray key must not answer a question about running
    /// commands.
    ProjectTabsConsent {
        source: std::path::PathBuf,
        tabs: Vec<crate::config::PaneTabConfig>,
    },
    /// Startup offer to install/update the Claude skill in `~/.claude/skills/`,
    /// raised only when the embedded copy differs from what's installed. Same
    /// single-key `[Y/n]` shape as `HookConsent`; `n` is remembered against
    /// `fingerprint` (the skill's content identity) so it asks again only when
    /// the skill itself changes. `overwrites_edits` is set when the installed
    /// copy carries local edits an update would discard, so the prompt can say so.
    SkillUpdate {
        fingerprint: String,
        overwrites_edits: bool,
    },
    /// `^a x` on a tab whose child is still running — confirm before killing it
    /// (closing a live claude loses the session). Single-key: `y`/`Y` closes,
    /// anything else keeps it. Always targets the active tab (the modal prompt
    /// blocks tab switching), so it needs no index. An exited tab skips this.
    ClosePane,
    /// Large file-pipe confirmation: only an unmodified `y`/`Y` queues it.
    PipeConfirm {
        payload: Vec<u8>,
        tab_id: String,
        on_ok: String,
    },
    /// `^a R` on a tab whose child is still running — a restart kills it, so it
    /// loses the session exactly as `^a x` does. Same single-key shape as
    /// [`Self::ClosePane`]: `y`/`Y` restarts, anything else keeps the tab. An
    /// exited tab has nothing to lose and restarts without asking.
    RestartPane,
    /// A mount big enough to ask about first — a `.tar.gz` that has to be
    /// extracted, or an archive whose expansion ratio looks like a bomb. `y`/`Y`
    /// / Enter re-issues the mount as confirmed; anything else abandons it.
    /// `path` is the archive, carried so the answer doesn't depend on where the
    /// cursor has wandered since.
    ArchiveMountConfirm {
        path: std::path::PathBuf,
    },
    /// Unmounting an archive that has unwritten changes. `y`/`Y`/Enter writes it
    /// back (and the write's own completion drops the mount); anything else drops
    /// the mount and the changes with it. Defaults to yes — the changes are the
    /// user's work, so the safe answer is to keep them.
    ArchiveWriteConfirm {
        archive: std::path::PathBuf,
    },
    /// A Lua script has run past the soft threshold — "keep waiting? [y/N]".
    /// `y`/`Y` re-arms the watchdog and keeps waiting; `n`/`N`/Esc (or any
    /// other key) requests the abort. Raised from the runaway watchdog
    /// (`settle_lua_runaway`); the worker's hard ceiling is the backstop.
    LuaRunaway,
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
                    // Empty = the user cleared the pre-fill; fall back to the
                    // configured default cwd (so clearing it agrees with what
                    // the prompt was pre-filled with under either mode).
                    let cwd_path = if cwd.is_empty() {
                        self.state.default_pane_cwd()
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
                    && let Some(tabs) = self.runtime.project_mut().pane_tabs.as_mut()
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
                super::spawn_foreground(program, argv, false)
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
                        self.state.flash_error(format!("jump: {e:#}"));
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
                        Err(e) => self.state.flash_error(format!("error: {e:#}")),
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
                    .active_project()
                    .home
                    .as_deref()
                    .and_then(crate::git::branch::default_base);
                // The `gix` checkout in `worktree::add` is a full-tree write that
                // froze the input thread when it ran inline (code-review HIGH) —
                // hand it to the worktree worker. `apply_worktree_outcomes`
                // chdirs the focused column into the new tree + flashes +
                // reconciles harpoon when it lands. `g w` then jumps to this
                // worktree's root, `g h` to PROJECT_HOME (the overall anchor).
                vec![Effect::WorktreeCreateInteractive {
                    dir: self.state.cur().listing.dir.clone(),
                    branch: branch.to_string(),
                    base,
                }]
            }
            PromptKind::WorktreeDeleteConfirm => {
                let confirmed = prompt.buffer.trim().eq_ignore_ascii_case("y");
                if !confirmed {
                    return Vec::new();
                }
                let dir = self.state.cur().listing.dir.clone();
                match crate::git::worktree::remove(&dir) {
                    Ok(removal) => {
                        if removal.leftovers.is_empty() {
                            self.state
                                .flash_info(format!("removed worktree: {}", dir.display()));
                        } else {
                            self.state.flash_info(format!(
                                "removed worktree: {} (a process was still writing, so {} is left to delete)",
                                dir.display(),
                                crate::git::worktree::show_paths(&removal.leftovers)
                            ));
                        }
                        // chdir the focused column to the parent (the deleted
                        // dir is gone). PROJECT_HOME is left untouched — it's the
                        // overall project anchor, not tied to a worktree.
                        if let Some(parent) = dir.parent() {
                            let _ = self.state.chdir(parent);
                        }
                    }
                    Err(e) => self.state.flash_error(format!("worktree remove: {e:#}")),
                }
                self.reconcile_harpoon();
                Vec::new()
            }
            // These should have been handled by AppState — unreachable in practice.
            _ => Vec::new(),
        }
    }
}
