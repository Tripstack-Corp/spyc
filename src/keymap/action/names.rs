//! [`action_from_name`], an action's snake_case name read back into the
//! [`Action`] it names: what a `.spycrc.toml` `map` and Lua's `spyc.action`
//! resolve through.

use super::{Action, ProjectAction};

/// Resolve a snake_case [`Action::canonical_name`] back to an [`Action`],
/// constructing parametric variants with sensible defaults (`up` → `Up(1)`,
/// `remove_prompt` → `RemovePrompt(None)`, `harpoon_jump` → `HarpoonJump(1)`,
/// …). This is the resolver behind Lua's `spyc.action(name)` and the `.spycrc`
/// DSL's fallback past its curated verbs; the DSL takes a parametric action's
/// parameter as `=value` instead of these defaults.
///
/// Stays in lockstep with [`Action::canonical_name`] via the
/// `action_names_round_trip` guard test: every variant's canonical name must
/// resolve back to that variant (parametric variants to their default form).
///
/// Two variants are deliberately **excluded** (return `None`): `set_mark` and
/// `jump_mark` carry a mark *letter* with no meaningful default — "set which
/// mark?" is unanswerable context-free, and `spyc.action("set_mark")` would
/// silently pick an arbitrary register. A script that wants a specific mark uses
/// `spyc.cmd(":…")` / a keymap binding instead. Every other variant is
/// invocable: nullary actions, count-motions (default count 1), and
/// prompt-openers (the prompt just opens) are all safe to trigger from a script.
#[must_use]
pub fn action_from_name(name: &str) -> Option<Action> {
    Some(match name {
        // Cursor motion (parametric count defaults to 1).
        "up" => Action::Up(1),
        "down" => Action::Down(1),
        "left" => Action::Left(1),
        "right" => Action::Right(1),
        "page_up" => Action::PageUp,
        "page_down" => Action::PageDown,
        "goto_first" => Action::GotoFirst,
        "goto_last" => Action::GotoLast,
        "jump_next_git_change" => Action::JumpNextGitChange,
        "jump_prev_git_change" => Action::JumpPrevGitChange,
        "jump_next_pick" => Action::JumpNextPick,
        "jump_prev_pick" => Action::JumpPrevPick,
        // Navigation.
        "enter_or_display" => Action::EnterOrDisplay,
        "enter_or_edit" => Action::EnterOrEdit,
        "execute_entry" => Action::ExecuteEntry,
        "climb" => Action::Climb,
        "home" => Action::Home,
        // Picks.
        "toggle_pick" => Action::TogglePick,
        "pick_pattern_prompt" => Action::PickPatternPrompt,
        "pick_toggle_all" => Action::PickToggleAll,
        // Inventory.
        "take" => Action::Take,
        "untake" => Action::Untake,
        "drop" => Action::Drop,
        "toggle_inventory_view" => Action::ToggleInventoryView,
        "empty_inventory" => Action::EmptyInventory,
        "yank_prompt" => Action::YankPrompt,
        "yank_last_prompt" => Action::YankLastPrompt,
        "yank_scrollback" => Action::YankScrollback,
        "yank_paths" | "yank_dir" => path_yank_from_name(name),
        // Masks & filtering (default to mask group 1 — the `a` key).
        "toggle_mask" => Action::ToggleMask(1),
        "limit_prompt" => Action::LimitPrompt,
        "command_prompt" => Action::CommandPrompt,
        // Shell-out (default chmod bits to +x — the built-in `^X` key).
        "shell_captured_prompt" => Action::ShellCapturedPrompt,
        "shell_foreground_prompt" => Action::ShellForegroundPrompt,
        "start_shell" => Action::StartShell,
        "chmod_add" => Action::ChmodAdd('x'),
        // Search.
        "search_prompt" => Action::SearchPrompt,
        "search_next" => Action::SearchNext,
        "search_prev" => Action::SearchPrev,
        // Navigation prompts.
        "jump_prompt" => Action::JumpPrompt,
        // File operations.
        "copy_prompt" => Action::CopyPrompt,
        "move_prompt" => Action::MovePrompt,
        "remove_prompt" => Action::RemovePrompt(None),
        "make_dir_prompt" => Action::MakeDirPrompt,
        "new_file_prompt" => Action::NewFilePrompt,
        "long_list" => Action::LongList,
        "file_type" => Action::FileType,
        // Sort.
        "sort_cycle" => Action::SortCycle,
        // Marks: `set_mark` / `jump_mark` excluded (need a mark letter) — see doc.
        "jump_prev_dir" => Action::JumpPrevDir,
        "jump_start_dir" => Action::JumpStartDir,
        // Project home / worktree root / start dir.
        "jump_worktree_root" => Action::JumpWorktreeRoot,
        "set_start_dir_here" => Action::SetStartDirHere,
        // Top-pane edit / display.
        "edit_in_pane" => Action::EditInPane,
        "display_in_pane" => Action::DisplayInPane,
        // Info.
        "date" => Action::Date,
        "version" => Action::Version,
        "show_memory" => Action::ShowMemory,
        "color_toggle" => Action::ColorToggle,
        "set_env_prompt" => Action::SetEnvPrompt,
        "toggle_activity" => Action::ToggleActivity,
        "help" => Action::Help,
        "about" => Action::About,
        "reload_config" => Action::ReloadConfig,
        // Split pane.
        "toggle_pane" => Action::TogglePane,
        "resume_pane" => Action::ResumePane,
        "toggle_pane_zoom" => Action::TogglePaneZoom,
        // Default to tab 1 (the `^W 1` key).
        // Vertical split. `vsplit_cycle` was the pre-toggle name for the same
        // key; kept so an existing `map KEY vsplit_cycle` still binds.
        "vsplit_toggle" | "vsplit_cycle" => Action::VsplitToggle,
        "vsplit_toggle_height" => Action::VsplitToggleHeight,
        "vsplit_focus_left" => Action::VsplitFocusLeft,
        "vsplit_focus_right" => Action::VsplitFocusRight,
        "toggle_dim" => Action::ToggleDim,
        "open_second_commander" => Action::OpenSecondCommander,
        "close_second_commander" => Action::CloseSecondCommander,
        // Graveyard.
        "open_graveyard_view" => Action::OpenGraveyardView,
        // Quick Select.
        "quick_select_open" => Action::QuickSelectOpen,
        "open_image_gallery" => Action::OpenImageGallery,
        "open_thinking_history" => Action::OpenThinkingHistory,
        // Harpoon (default to slot 1).
        "harpoon_jump" => Action::HarpoonJump(1),
        "harpoon_append" => Action::HarpoonAppend,
        "harpoon_remove" => Action::HarpoonRemove,
        "harpoon_open_menu" => Action::HarpoonOpenMenu,
        // Git worktree.
        "worktree_list" => Action::WorktreeList,
        "worktree_new" => Action::WorktreeNew,
        "worktree_delete" => Action::WorktreeDelete,
        // Git diff / blame / restore.
        "git_diff" => Action::GitDiff,
        "git_diff_cached" => Action::GitDiffCached,
        "git_diff_unstaged" => Action::GitDiffUnstaged,
        "git_blame" => Action::GitBlame,
        "git_restore" => Action::GitRestore,
        // Path references.
        "goto_file" | "goto_file_line" => goto_from_name(name),
        // Meta.
        "redraw" => Action::Redraw,
        "quit" => Action::Quit,
        "macro_record_reserved" => Action::MacroRecordReserved,
        "open_task_viewer" => Action::OpenTaskViewer,
        "reopen_last_buffer" => Action::ReopenLastBuffer,
        "find_file" => Action::FindFile,
        "noop" => Action::Noop,
        _ => {
            return pane_action_from_name(name)
                .or_else(|| ProjectAction::from_name(name).map(Action::Project));
        }
    })
}

/// The `pane_*` names and the menus' of [`action_from_name`], which is at its
/// length pin.
fn pane_action_from_name(name: &str) -> Option<Action> {
    Some(match name {
        "context_menu" => Action::ContextMenu,
        "make_menu" => Action::MakeMenu,
        "pane_context_menu" => Action::PaneContextMenu,
        "pane_focus_down" => Action::PaneFocusDown,
        "pane_focus_up" => Action::PaneFocusUp,
        "pane_send_selection" => Action::PaneSendSelection,
        "pane_send_prefix" => Action::PaneSendPrefix,
        "pane_send_esc" => Action::PaneSendEsc,
        "pane_send_ctrl_w" => Action::PaneSendCtrlW,
        "pane_grow" => Action::PaneGrow,
        "pane_shrink" => Action::PaneShrink,
        "pane_scroll_enter" => Action::PaneScrollEnter,
        "pane_scroll_save" => Action::PaneScrollSave,
        "pane_new_tab" => Action::PaneNewTab,
        "pane_close_tab" => Action::PaneCloseTab,
        "pane_tab_by_index" => Action::PaneTabByIndex(1),
        "pane_next_tab" => Action::PaneNextTab,
        "pane_prev_tab" => Action::PanePrevTab,
        "pane_last_tab" => Action::PaneLastTab,
        "pane_rename_tab" => Action::PaneRenameTab,
        "pane_restart_tab" => Action::PaneRestartTab,
        "pane_fork_tab" => Action::PaneForkTab,
        "pane_toggle_suspend" => Action::PaneToggleSuspend,
        "pane_to_task" => Action::PaneToTask,
        "pane_pipe_content" => Action::PanePipeContent,
        "pane_pipe_inventory" => Action::PanePipeInventory,
        _ => return None,
    })
}

/// The path yanks' arm of [`action_from_name`], which is at its length pin.
fn path_yank_from_name(name: &str) -> Action {
    if name == "yank_dir" {
        Action::YankDir
    } else {
        Action::YankPaths
    }
}

fn goto_from_name(name: &str) -> Action {
    if name == "goto_file_line" {
        Action::GotoFileLine
    } else {
        Action::GotoFile
    }
}
