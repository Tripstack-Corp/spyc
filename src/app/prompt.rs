//! The one-line input prompt: `Prompt` (kind + prefix + buffer +
//! optional vi line editor) and `PromptKind` (the full vocabulary of
//! prompt contexts). Extracted verbatim from `app/mod.rs`
//! (REFACTOR_PLAN Phase 1) and re-exported from `crate::app` so the
//! existing `super::{Prompt, PromptKind}` imports in `state`/`route`
//! keep resolving. The `simple`/`shell` ctors are `pub` because both
//! `app` and its sibling `state` module construct prompts.

use crate::ui::line_edit::LineEditor;

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
