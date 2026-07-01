/// The full vocabulary of things spyc can do in response to input.
///
/// Keep this enum stable and spy-parity-friendly: each variant should map to
/// one user-observable behavior, so `.spycrc` can bind any key to any action.
///
/// `strum::EnumIter` is derived *for the guard test only* (`action_names_round_trip`):
/// it lets that test iterate every variant so a new one that isn't wired into
/// [`Action::canonical_name`] / [`action_from_name`] fails the build — the
/// completeness guarantee for the Lua `spyc.action(name)` surface. The iterator
/// constructs each parametric variant with `Default::default()` fields (0 / `\0`
/// / `None`), which is fine: the test only reads `canonical_name`, which ignores
/// the payload.
#[derive(Debug, Clone, PartialEq, Eq, strum::EnumIter)]
pub enum Action {
    // Cursor motion. A count of 0 means "no explicit count" (default 1).
    Up(usize),
    Down(usize),
    Left(usize),
    Right(usize),
    PageUp,
    PageDown,
    GotoFirst,
    GotoLast,

    // Navigation.
    EnterOrDisplay, // <Enter> — dir: chdir; text file: pager
    EnterOrEdit,    // e / v   — dir: chdir; file: editor
    Climb,          // u / -
    Home,           // H / ~

    // Picks (per-directory multi-select).
    TogglePick,        // t
    PickPatternPrompt, // T
    PickToggleAll,     // ^T

    // Inventory (global, cross-directory).
    Take,                // yy — yank into inventory
    Untake,              // Y — remove from inventory (from dir view)
    Drop,                // p — put inventory to cwd
    ToggleInventoryView, // i
    EmptyInventory,      // z
    YankPrompt,          // yp — yank visible pane output to clipboard
    YankLastPrompt,      // yP — yank last typed pane prompt to clipboard
    YankScrollback,      // ya — yank full pane scrollback to clipboard
    YankPaths,           // yf — yank cursor file's absolute path (or all picks, newline-separated)

    // Ignore masks & filtering.
    ToggleMask(u8), // a -> 1, o -> 2
    LimitPrompt,    // = — temporary filter (glob pattern, `!` for picks, empty clears)
    CommandPrompt,  // : — vim-style command line (limit, !, !!, ;, etc.)

    // Shell-out.
    ShellCapturedPrompt, // ! — prompt command, capture output, show in pager with colors
    ShellForegroundPrompt, // ; — prompt command, run in foreground (for interactive tools)
    StartShell,          // $ — drops you into $SHELL in the current directory
    ChmodAdd(char),      // ^X -> 'x'

    // Search.
    SearchPrompt, // / — start incremental search
    SearchNext,   // n — repeat last search forward
    SearchPrev,   // N — repeat last search backward

    // Navigation.
    JumpPrompt, // J — prompt for a path (~, $VAR expanded) and chdir

    // File operations.
    CopyPrompt, // c — prompt for destination, cp -r selection
    MovePrompt, // M — prompt for destination, mv selection
    /// `R` (no count) or `dd` (no count) — confirm-then-remove
    /// the current selection (picks if any, else cursor entry).
    /// `Ndd` passes `Some(N)` to request N consecutive entries from
    /// the cursor down (clamped to end of list, no wrap). The
    /// explicit count ignores picks — the count is the user being
    /// explicit.
    RemovePrompt(Option<usize>),
    MakeDirPrompt, // + — prompt for a directory name
    NewFilePrompt, // N — prompt for a filename, open in $EDITOR
    LongList,      // L — ls -lh selection | $PAGER
    FileType,      // f — file(1) on selection, paged output

    // Sort. (Reverse-toggle lives on the `:sort reverse` command — no
    // default key since the keymap slim.)
    /// `S` — cycle through sort modes (Name → Size → Mtime → Ext → Name).
    SortCycle,

    // Marks (vi-style named bookmarks).
    SetMark(char),  // m{a-z}
    JumpMark(char), // '{a-z}
    JumpPrevDir,    // '' — jump back to directory before last chdir
    JumpStartDir,   // ` — jump to directory where spyc was launched

    // Project home (sticky project root).
    JumpProjectHome,  // Space p (leader) — jump to PROJECT_HOME (the overall project)
    JumpWorktreeRoot, // gw — jump the focused column to ITS repo/worktree root
    SetProjectHomeHere, // gP — set PROJECT_HOME to current directory

    // Start dir (target of backtick `).
    SetStartDirHere, // gS — set start_dir to current directory

    // (user@host lives on the `:whoami` command — no default key.)

    // Edit / display in top pane.
    EditInPane,    // V — open $EDITOR in top overlay (bottom pane stays visible)
    DisplayInPane, // D — open $PAGER in top overlay (bottom pane stays visible)

    // Info commands.
    Date,           // :date — show date/time
    Version,        // gV / :version — show spyc version
    ShowMemory,     // I — session info pager (version, pid, rss, counts)
    ColorToggle,    // C — toggle color theme on/off
    SetEnvPrompt,   // :setenv — open the NAME=VALUE prompt (no default key)
    ToggleActivity, // :activity — toggle draws/sec, bytes/sec overlay

    // Help.
    Help, // ? or F1 — key bindings overlay

    // Config reload.
    ReloadConfig, // ^R — re-read ~/.spycrc.toml + project config

    // Split pane (M8).
    TogglePane,         // Ctrl-\ / F10 / ^W \ / ^W c — open/close the pty pane
    ResumePane,         // F9 — open pane with `claude --resume`
    PaneFocusDown,      // ^W j — move focus down (to pane)
    PaneFocusUp,        // ^W k — move focus up (to list)
    PaneSendSelection,  // ^W s — send shell-quoted selection to pane stdin
    PaneSendPrefix,     // ^a ↓ — send literal Ctrl-A (0x01) to the active pane (send-prefix)
    PaneGrow,           // ^W + — bottom pane takes more height
    PaneShrink,         // ^W - — bottom pane takes less height
    TogglePaneZoom,     // ^W z — zoom (fullscreen) the pane / restore split
    PaneScrollEnter,    // ^W v — enter scroll mode (browse scrollback)
    PaneScrollSave,     // s (while in scroll mode) — save scrollback to file
    PaneNewTab,         // ^W n — open a new pane tab (prompt for command + cwd)
    PaneCloseTab,       // ^W x — close the active pane tab
    PaneTabByIndex(u8), // ^W 1..9 — switch to tab N
    PaneNextTab,        // ^W ] — next tab
    PanePrevTab,        // ^W [ — previous tab
    PaneLastTab,        // ^a ^a — jump to the previously-active tab (screen/tmux last-window)
    PaneRenameTab,      // ^W r — rename the active tab
    PaneRestartTab,     // ^W R — restart the active tab's command
    PanePipeContent,    // ^W p — send file contents of selection to pane
    PanePipeInventory,  // ^W i — send file contents of inventory to pane

    // Vertical (left/right) split — file panes labelled a/b.
    VsplitCycle,      // ^a | — cycle off → top-only → full-height → off (opens a preview)
    VsplitFocusLeft,  // ^a a / ^a h — focus the left region (a)
    VsplitFocusRight, // ^a b / ^a l — focus the right region (b)
    ToggleDim,        // ^a d — toggle the focus-dim of the inactive column / list
    // Second file-commander in the right column (^s chord family).
    OpenSecondCommander,  // ^s n — open a second commander (at PROJECT_HOME)
    CloseSecondCommander, // ^s x — close the second commander
    // `^d` — close the second commander if one is open, else quit (the
    // no-split quit keeps its own "press again to quit" confirm).
    QuitOrCloseCommander,

    // Graveyard. No default key since the keymap slim — reached via the
    // `:graveyard` command (which dispatches this action for its entry hint).
    OpenGraveyardView,

    // Quick Select — wezterm-style labeled overlay over pane output.
    QuickSelectOpen, // ^a u — scan visible pane, label matches, pick to yank/open

    // Harpoon — small per-project pinned list of file pointers.
    HarpoonJump(u8), // H 1..9 — chdir to slot N's parent + cursor on it
    HarpoonAppend,   // H a — append cursor file/dir to harpoon list
    HarpoonRemove,   // H x — remove cursor file from harpoon list
    HarpoonOpenMenu, // H h — open the harpoon menu (reorder, delete, jump)

    // Git worktree (M11).
    WorktreeList,   // W l — list worktrees, pick to chdir
    WorktreeNew,    // W n — prompt for branch, create worktree
    WorktreeDelete, // W d — confirm, remove current worktree

    // Git diff (M12).
    GitDiff,       // g d — diff-vs-HEAD for cursor file / selection (staged + unstaged + new)
    GitDiffCached, // g D — staged (cached) diff
    GitDiffUnstaged, // g u — unstaged diff (index vs worktree — plain `git diff`)
    GitBlame,      // g b — git blame on cursor file
    GitRestore,    // g r — restore a deleted (struck-through) file from index/HEAD

    // Cursor jumps to next / previous git-changed entry in the current
    // listing. Vim-style "next/prev hunk" muscle memory.
    JumpNextGitChange, // ] g — cursor to next file/dir with non-clean git status (wraps)
    JumpPrevGitChange, // [ g — cursor to prev file/dir with non-clean git status (wraps)

    // Path references (M13).
    GotoFile,     // g f — jump file list to path reference in pane output
    GotoFileLine, // g F — jump + open pager at line

    // Meta.
    Redraw, // ^L
    Quit,   // ^D / Q / :q

    // Reserved for a future vim-style macro recording feature
    // (qa ... q ... @a). For now, flashes a hint so an accidental `q`
    // press doesn't quit the app.
    MacroRecordReserved,

    /// `gB` from the file list -- open the task viewer for the
    /// most-recently-backgrounded task.
    OpenTaskViewer,

    /// `gp` from the file list -- reopen the most-recently-closed
    /// pager view from the buffer-history stack. Same as `:bprev`
    /// when no pager is currently open.
    ReopenLastBuffer,

    /// `F` from the file list -- open the project-wide filename
    /// finder. Walks PROJECT_HOME (or the listing dir) honoring
    /// gitignore, fuzzy-matches against typed input, Enter chdirs
    /// to the matched file's parent and places the cursor on it.
    FindFile,

    // Placeholder for keys we reserve but haven't implemented yet.
    #[allow(dead_code)]
    Noop,
}

/// Which namespace tier an action belongs to — the documented binding taxonomy
/// (see DESIGN.md "Binding taxonomy"). Tagged on [`Action::tier`] and consumed
/// by the `leader_and_pane_namespaces_respect_tiers` guard: the leader (`Space`
/// / `^a Space`) carries only `Global`/`Meta`, the `^a` pane prefix only
/// `Pane`/`Meta`. `Frame` ops act on the file view and live on the
/// letter / `g` / `H` / `[`/`]` chords. The tier makes the namespace split a
/// build-checked contract (the guard) and is also read at runtime to pause
/// `Pane`-tier commands while a top-overlay editor / foreground command is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Workspace-level, meaningful from any focus (worktree, project, session).
    Global,
    /// Acts on the file-commander view (nav, picks, git, sort, marks, files).
    Frame,
    /// Acts on the pty pane / split layout (tabs, focus, zoom, scroll, send).
    Pane,
    /// Cross-cutting / informational (help, version, redraw) — allowed anywhere.
    Meta,
}

impl Action {
    /// The binding-taxonomy tier this action belongs to. Explicitly enumerates
    /// the `Global` / `Pane` / `Meta` actions; everything else is `Frame` (the
    /// default), so a new global/pane action must be tagged here or the
    /// namespace guard rejects placing it on the leader / pane prefix.
    /// Also consulted at runtime to pause `Pane`-tier commands while a
    /// top-overlay editor / foreground command owns the screen.
    pub const fn tier(&self) -> Tier {
        match self {
            // Global — workspace ops (the leader namespace).
            Self::WorktreeList
            | Self::WorktreeNew
            | Self::WorktreeDelete
            | Self::JumpProjectHome
            | Self::SetProjectHomeHere
            | Self::ShowMemory => Tier::Global,
            // Pane — the pty pane + vertical split (the `^a` prefix).
            Self::TogglePane
            | Self::ResumePane
            | Self::PaneFocusDown
            | Self::PaneFocusUp
            | Self::PaneSendSelection
            | Self::PaneSendPrefix
            | Self::PaneGrow
            | Self::PaneShrink
            | Self::TogglePaneZoom
            | Self::PaneScrollEnter
            | Self::PaneScrollSave
            | Self::PaneNewTab
            | Self::PaneCloseTab
            | Self::PaneTabByIndex(_)
            | Self::PaneNextTab
            | Self::PanePrevTab
            | Self::PaneLastTab
            | Self::PaneRenameTab
            | Self::PaneRestartTab
            | Self::PanePipeContent
            | Self::PanePipeInventory
            | Self::VsplitCycle
            | Self::VsplitFocusLeft
            | Self::VsplitFocusRight
            | Self::ToggleDim
            | Self::OpenSecondCommander
            | Self::CloseSecondCommander
            | Self::QuitOrCloseCommander
            | Self::QuickSelectOpen => Tier::Pane,
            // Meta — cross-cutting / informational.
            Self::Help
            | Self::Version
            | Self::Redraw
            | Self::ReloadConfig
            | Self::Date
            | Self::ColorToggle
            | Self::ToggleActivity
            | Self::Quit
            | Self::MacroRecordReserved
            | Self::Noop => Tier::Meta,
            // Frame — everything else (nav, picks, files, git, sort, marks,
            // harpoon, filter, search, inventory, …).
            _ => Tier::Frame,
        }
    }

    /// Short, present-tense description for the help overlay.
    pub const fn describe(&self) -> &'static str {
        match self {
            Self::Up(_) => "move up",
            Self::Down(_) => "move down",
            Self::Left(_) => "move left",
            Self::Right(_) => "move right",
            Self::PageUp => "page up",
            Self::PageDown => "page down",
            Self::GotoFirst => "top of column",
            Self::GotoLast => "bottom of column",
            Self::EnterOrDisplay => "enter dir / pager on text file",
            Self::EnterOrEdit => "enter dir / editor on file (suspends TUI)",
            Self::EditInPane => "open editor in top pane (bottom pane stays visible)",
            Self::DisplayInPane => "open $PAGER in top pane (bottom pane stays visible)",
            Self::Climb => "climb to parent",
            Self::Home => "home directory",
            Self::TogglePick => "toggle pick",
            Self::PickPatternPrompt => "pick by pattern (prompt)",
            Self::PickToggleAll => "pick all / clear",
            Self::Take => "take into inventory",
            Self::Untake => "remove from inventory",
            Self::Drop => "drop from inventory",
            Self::ToggleInventoryView => "toggle inventory view",
            Self::EmptyInventory => "empty inventory",
            Self::YankPrompt => "yank visible pane output to clipboard",
            Self::YankLastPrompt => "yank last typed prompt to clipboard",
            Self::YankScrollback => "yank full pane scrollback to clipboard",
            Self::YankPaths => "yank cursor file path (or picks) to clipboard",
            Self::JumpNextGitChange => "jump to next git-changed entry",
            Self::JumpPrevGitChange => "jump to prev git-changed entry",
            Self::ToggleMask(_) => "toggle ignore mask",
            Self::LimitPrompt => "filter file list (glob, ! for picks, empty clears)",
            Self::CommandPrompt => "command line (:limit, :!, :!!, :;, etc.)",
            Self::ShellCapturedPrompt => "shell command (captured, pager)",
            Self::ShellForegroundPrompt => "shell command (foreground)",
            Self::StartShell => "start shell",
            Self::ChmodAdd(_) => "chmod add bits",
            Self::SearchPrompt => "search",
            Self::SearchNext => "search next",
            Self::SearchPrev => "search previous",
            Self::JumpPrompt => "jump to path (prompt)",
            Self::CopyPrompt => "copy (prompt)",
            Self::MovePrompt => "move (prompt)",
            Self::RemovePrompt(_) => "remove (confirm)",
            Self::MakeDirPrompt => "make directory (prompt)",
            Self::NewFilePrompt => "new file in editor (prompt)",
            Self::LongList => "long listing",
            Self::FileType => "file type",
            Self::SortCycle => "cycle sort (name/size/mtime/ext)",
            Self::Help => "help",
            Self::ReloadConfig => "reload config",
            Self::TogglePane => "toggle split pane",
            Self::ResumePane => "open pane with claude --resume",
            Self::PaneFocusDown => "focus pane (down)",
            Self::PaneFocusUp => "focus list (up)",
            Self::PaneSendSelection => "send selection to pane",
            Self::PaneSendPrefix => "send literal ^a to pane",
            Self::PaneGrow => "grow pane",
            Self::PaneShrink => "shrink pane",
            Self::TogglePaneZoom => "zoom pane (toggle fullscreen)",
            Self::PaneScrollEnter => "scroll pane history",
            Self::PaneScrollSave => "save pane scrollback",
            Self::PaneNewTab => "new pane tab",
            Self::PaneCloseTab => "close pane tab",
            Self::PaneTabByIndex(_) => "switch pane tab",
            Self::PaneNextTab => "next pane tab",
            Self::PanePrevTab => "prev pane tab",
            Self::PaneLastTab => "last pane tab",
            Self::PaneRenameTab => "rename pane tab",
            Self::PaneRestartTab => "restart pane tab command",
            Self::PanePipeContent => "pipe file contents to pane",
            Self::PanePipeInventory => "pipe inventory contents to pane",
            Self::VsplitCycle => "vertical split: off / top-only / full-height",
            Self::VsplitFocusLeft => "focus left pane (a)",
            Self::VsplitFocusRight => "focus right pane (b)",
            Self::ToggleDim => "toggle dimming of the inactive pane",
            Self::OpenSecondCommander => "open a second file-commander (right column)",
            Self::CloseSecondCommander => "close the second file-commander",
            Self::OpenGraveyardView => "open graveyard viewer (recover deleted)",
            Self::QuickSelectOpen => "quick select — pick URL/path/SHA/IP from pane",
            Self::HarpoonJump(_) => "jump to harpoon slot N",
            Self::HarpoonAppend => "harpoon — append cursor file",
            Self::HarpoonRemove => "harpoon — remove cursor file",
            Self::HarpoonOpenMenu => "harpoon — open menu",
            Self::WorktreeList => "list git worktrees",
            Self::WorktreeNew => "new git worktree",
            Self::WorktreeDelete => "delete git worktree",
            Self::GitDiff => "git diff HEAD (staged + unstaged + new)",
            Self::GitDiffCached => "git diff --cached (staged)",
            Self::GitDiffUnstaged => "git diff (unstaged — since you staged)",
            Self::GitBlame => "git blame (cursor file)",
            Self::GitRestore => "restore deleted file (struck-through row) from git",
            Self::GotoFile => "jump to path in pane output",
            Self::GotoFileLine => "jump to path:line in pane output",
            Self::SetMark(_) => "set mark",
            Self::JumpMark(_) => "jump to mark",
            Self::JumpPrevDir => "jump to previous directory",
            Self::JumpStartDir => "jump to starting directory",
            Self::JumpProjectHome => "jump to PROJECT_HOME",
            Self::JumpWorktreeRoot => "jump the focused column to its worktree/repo root",
            Self::SetProjectHomeHere => "set PROJECT_HOME to current dir",
            Self::SetStartDirHere => "set start dir to current dir (target of `)",
            Self::Date => "show date",
            Self::Version => "show version",
            Self::ShowMemory => "session info",
            Self::ColorToggle => "toggle colors",
            Self::SetEnvPrompt => "set env var",
            Self::ToggleActivity => "toggle activity monitor",
            Self::Redraw => "redraw",
            Self::Quit => "quit",
            Self::QuitOrCloseCommander => "close the second commander, else quit",
            Self::MacroRecordReserved => "(reserved: macro recording)",
            Self::OpenTaskViewer => "open task viewer (most-recent bg task)",
            Self::ReopenLastBuffer => "reopen the most-recent closed pager buffer",
            Self::FindFile => "find file (project-wide fuzzy)",
            Self::Noop => "no-op",
        }
    }

    /// A stable, machine-readable snake_case name for this action — the vocabulary
    /// the Lua `spyc.action(name)` bridge (and any future name-addressed dispatch)
    /// resolves against. Distinct from [`Action::describe`] (human prose) and the
    /// curated `.spycrc` DSL verbs in [`crate::config::dsl::parse_action`] (a
    /// smaller, alias-rich set): this covers **every** variant.
    ///
    /// The `match self` is exhaustive on purpose — that's the completeness guard.
    /// A new variant won't compile until it's named here, and the
    /// `action_names_round_trip` test then forces it to be wired into
    /// [`action_from_name`] too (or explicitly excluded there with a reason).
    /// Where a DSL verb already spells an action (`down`, `pick`, `search`, …),
    /// the canonical name matches that spelling so the two stay consistent.
    // Consumed by the `action_names_round_trip` guard (the other half of the
    // completeness pair with `action_from_name`); it earns its place as the
    // canonical inverse even before a production caller name-addresses actions.
    #[allow(dead_code)]
    pub const fn canonical_name(&self) -> &'static str {
        match self {
            // Cursor motion.
            Self::Up(_) => "up",
            Self::Down(_) => "down",
            Self::Left(_) => "left",
            Self::Right(_) => "right",
            Self::PageUp => "page_up",
            Self::PageDown => "page_down",
            Self::GotoFirst => "goto_first",
            Self::GotoLast => "goto_last",
            Self::JumpNextGitChange => "jump_next_git_change",
            Self::JumpPrevGitChange => "jump_prev_git_change",
            // Navigation.
            Self::EnterOrDisplay => "enter_or_display",
            Self::EnterOrEdit => "enter_or_edit",
            Self::Climb => "climb",
            Self::Home => "home",
            // Picks.
            Self::TogglePick => "toggle_pick",
            Self::PickPatternPrompt => "pick_pattern_prompt",
            Self::PickToggleAll => "pick_toggle_all",
            // Inventory.
            Self::Take => "take",
            Self::Untake => "untake",
            Self::Drop => "drop",
            Self::ToggleInventoryView => "toggle_inventory_view",
            Self::EmptyInventory => "empty_inventory",
            Self::YankPrompt => "yank_prompt",
            Self::YankLastPrompt => "yank_last_prompt",
            Self::YankScrollback => "yank_scrollback",
            Self::YankPaths => "yank_paths",
            // Masks & filtering.
            Self::ToggleMask(_) => "toggle_mask",
            Self::LimitPrompt => "limit_prompt",
            Self::CommandPrompt => "command_prompt",
            // Shell-out.
            Self::ShellCapturedPrompt => "shell_captured_prompt",
            Self::ShellForegroundPrompt => "shell_foreground_prompt",
            Self::StartShell => "start_shell",
            Self::ChmodAdd(_) => "chmod_add",
            // Search.
            Self::SearchPrompt => "search_prompt",
            Self::SearchNext => "search_next",
            Self::SearchPrev => "search_prev",
            // Navigation prompts.
            Self::JumpPrompt => "jump_prompt",
            // File operations.
            Self::CopyPrompt => "copy_prompt",
            Self::MovePrompt => "move_prompt",
            Self::RemovePrompt(_) => "remove_prompt",
            Self::MakeDirPrompt => "make_dir_prompt",
            Self::NewFilePrompt => "new_file_prompt",
            Self::LongList => "long_list",
            Self::FileType => "file_type",
            // Sort.
            Self::SortCycle => "sort_cycle",
            // Marks.
            Self::SetMark(_) => "set_mark",
            Self::JumpMark(_) => "jump_mark",
            Self::JumpPrevDir => "jump_prev_dir",
            Self::JumpStartDir => "jump_start_dir",
            // Project home / worktree root / start dir.
            Self::JumpProjectHome => "jump_project_home",
            Self::JumpWorktreeRoot => "jump_worktree_root",
            Self::SetProjectHomeHere => "set_project_home_here",
            Self::SetStartDirHere => "set_start_dir_here",
            // Top-pane edit / display.
            Self::EditInPane => "edit_in_pane",
            Self::DisplayInPane => "display_in_pane",
            // Info.
            Self::Date => "date",
            Self::Version => "version",
            Self::ShowMemory => "show_memory",
            Self::ColorToggle => "color_toggle",
            Self::SetEnvPrompt => "set_env_prompt",
            Self::ToggleActivity => "toggle_activity",
            Self::Help => "help",
            Self::ReloadConfig => "reload_config",
            // Split pane.
            Self::TogglePane => "toggle_pane",
            Self::ResumePane => "resume_pane",
            Self::PaneFocusDown => "pane_focus_down",
            Self::PaneFocusUp => "pane_focus_up",
            Self::PaneSendSelection => "pane_send_selection",
            Self::PaneSendPrefix => "pane_send_prefix",
            Self::PaneGrow => "pane_grow",
            Self::PaneShrink => "pane_shrink",
            Self::TogglePaneZoom => "toggle_pane_zoom",
            Self::PaneScrollEnter => "pane_scroll_enter",
            Self::PaneScrollSave => "pane_scroll_save",
            Self::PaneNewTab => "pane_new_tab",
            Self::PaneCloseTab => "pane_close_tab",
            Self::PaneTabByIndex(_) => "pane_tab_by_index",
            Self::PaneNextTab => "pane_next_tab",
            Self::PanePrevTab => "pane_prev_tab",
            Self::PaneLastTab => "pane_last_tab",
            Self::PaneRenameTab => "pane_rename_tab",
            Self::PaneRestartTab => "pane_restart_tab",
            Self::PanePipeContent => "pane_pipe_content",
            Self::PanePipeInventory => "pane_pipe_inventory",
            // Vertical split.
            Self::VsplitCycle => "vsplit_cycle",
            Self::VsplitFocusLeft => "vsplit_focus_left",
            Self::VsplitFocusRight => "vsplit_focus_right",
            Self::ToggleDim => "toggle_dim",
            Self::OpenSecondCommander => "open_second_commander",
            Self::CloseSecondCommander => "close_second_commander",
            Self::QuitOrCloseCommander => "quit_or_close_commander",
            // Graveyard.
            Self::OpenGraveyardView => "open_graveyard_view",
            // Quick Select.
            Self::QuickSelectOpen => "quick_select_open",
            // Harpoon.
            Self::HarpoonJump(_) => "harpoon_jump",
            Self::HarpoonAppend => "harpoon_append",
            Self::HarpoonRemove => "harpoon_remove",
            Self::HarpoonOpenMenu => "harpoon_open_menu",
            // Git worktree.
            Self::WorktreeList => "worktree_list",
            Self::WorktreeNew => "worktree_new",
            Self::WorktreeDelete => "worktree_delete",
            // Git diff / blame / restore.
            Self::GitDiff => "git_diff",
            Self::GitDiffCached => "git_diff_cached",
            Self::GitDiffUnstaged => "git_diff_unstaged",
            Self::GitBlame => "git_blame",
            Self::GitRestore => "git_restore",
            // Path references.
            Self::GotoFile => "goto_file",
            Self::GotoFileLine => "goto_file_line",
            // Meta.
            Self::Redraw => "redraw",
            Self::Quit => "quit",
            Self::MacroRecordReserved => "macro_record_reserved",
            Self::OpenTaskViewer => "open_task_viewer",
            Self::ReopenLastBuffer => "reopen_last_buffer",
            Self::FindFile => "find_file",
            Self::Noop => "noop",
        }
    }
}

/// Resolve a snake_case [`Action::canonical_name`] back to an [`Action`],
/// constructing parametric variants with sensible defaults (`up` → `Up(1)`,
/// `remove_prompt` → `RemovePrompt(None)`, `harpoon_jump` → `HarpoonJump(1)`,
/// …). This is the resolver behind Lua's `spyc.action(name)` — it accepts the
/// **full** action vocabulary, unlike the curated `.spycrc` DSL verbs.
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
        // Navigation.
        "enter_or_display" => Action::EnterOrDisplay,
        "enter_or_edit" => Action::EnterOrEdit,
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
        "yank_paths" => Action::YankPaths,
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
        "jump_project_home" => Action::JumpProjectHome,
        "jump_worktree_root" => Action::JumpWorktreeRoot,
        "set_project_home_here" => Action::SetProjectHomeHere,
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
        "reload_config" => Action::ReloadConfig,
        // Split pane.
        "toggle_pane" => Action::TogglePane,
        "resume_pane" => Action::ResumePane,
        "pane_focus_down" => Action::PaneFocusDown,
        "pane_focus_up" => Action::PaneFocusUp,
        "pane_send_selection" => Action::PaneSendSelection,
        "pane_send_prefix" => Action::PaneSendPrefix,
        "pane_grow" => Action::PaneGrow,
        "pane_shrink" => Action::PaneShrink,
        "toggle_pane_zoom" => Action::TogglePaneZoom,
        "pane_scroll_enter" => Action::PaneScrollEnter,
        "pane_scroll_save" => Action::PaneScrollSave,
        "pane_new_tab" => Action::PaneNewTab,
        "pane_close_tab" => Action::PaneCloseTab,
        // Default to tab 1 (the `^W 1` key).
        "pane_tab_by_index" => Action::PaneTabByIndex(1),
        "pane_next_tab" => Action::PaneNextTab,
        "pane_prev_tab" => Action::PanePrevTab,
        "pane_last_tab" => Action::PaneLastTab,
        "pane_rename_tab" => Action::PaneRenameTab,
        "pane_restart_tab" => Action::PaneRestartTab,
        "pane_pipe_content" => Action::PanePipeContent,
        "pane_pipe_inventory" => Action::PanePipeInventory,
        // Vertical split.
        "vsplit_cycle" => Action::VsplitCycle,
        "vsplit_focus_left" => Action::VsplitFocusLeft,
        "vsplit_focus_right" => Action::VsplitFocusRight,
        "toggle_dim" => Action::ToggleDim,
        "open_second_commander" => Action::OpenSecondCommander,
        "close_second_commander" => Action::CloseSecondCommander,
        "quit_or_close_commander" => Action::QuitOrCloseCommander,
        // Graveyard.
        "open_graveyard_view" => Action::OpenGraveyardView,
        // Quick Select.
        "quick_select_open" => Action::QuickSelectOpen,
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
        "goto_file" => Action::GotoFile,
        "goto_file_line" => Action::GotoFileLine,
        // Meta.
        "redraw" => Action::Redraw,
        "quit" => Action::Quit,
        "macro_record_reserved" => Action::MacroRecordReserved,
        "open_task_viewer" => Action::OpenTaskViewer,
        "reopen_last_buffer" => Action::ReopenLastBuffer,
        "find_file" => Action::FindFile,
        "noop" => Action::Noop,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{Action, action_from_name};
    use std::collections::HashSet;
    use strum::IntoEnumIterator;

    /// The mark actions carry a register letter with no meaningful context-free
    /// default, so [`action_from_name`] deliberately returns `None` for them
    /// (see its doc). Kept in one place so the round-trip guard can special-case
    /// exactly these and nothing else.
    fn is_intentionally_excluded(a: &Action) -> bool {
        matches!(a, Action::SetMark(_) | Action::JumpMark(_))
    }

    /// Anti-drift completeness guard: iterate **every** `Action` variant (via
    /// `strum::EnumIter`) and assert its `canonical_name` round-trips through
    /// `action_from_name` back to the same variant — parametric variants to
    /// their default form (`Up(1)`, `RemovePrompt(None)`, `HarpoonJump(1)`, …).
    ///
    /// Because the iterator is generated from the enum itself, a NEW variant is
    /// automatically covered: it must be given a `canonical_name` (already forced
    /// by that `match`'s exhaustiveness) *and* wired into `action_from_name`, or
    /// this test fails — the same compile-time-completeness philosophy as the
    /// COMMAND_TABLE registry. To exclude a variant, add it to
    /// `is_intentionally_excluded` (which documents the "why").
    #[test]
    fn action_names_round_trip() {
        for variant in Action::iter() {
            let name = variant.canonical_name();
            if is_intentionally_excluded(&variant) {
                assert!(
                    action_from_name(name).is_none(),
                    "{name:?} is excluded from action_from_name but resolved anyway \
                     — either wire it in or drop it from is_intentionally_excluded"
                );
                continue;
            }
            let resolved = action_from_name(name).unwrap_or_else(|| {
                panic!(
                    "canonical name {name:?} (for {variant:?}) does not resolve via \
                     action_from_name — a new Action variant must be wired into both \
                     canonical_name and action_from_name"
                )
            });
            // Parametric variants round-trip to their default form, so compare
            // the resolved action's canonical name rather than the payload.
            assert_eq!(
                resolved.canonical_name(),
                name,
                "{name:?} resolved to a different action ({resolved:?})"
            );
        }
    }

    /// No two variants may share a canonical name — the names are the public
    /// `spyc.action(...)` keys, so a collision would make one action
    /// unreachable (the earlier arm in `action_from_name` would always win).
    #[test]
    fn canonical_names_are_unique() {
        let mut seen: HashSet<&'static str> = HashSet::new();
        for variant in Action::iter() {
            let name = variant.canonical_name();
            assert!(
                seen.insert(name),
                "duplicate canonical name {name:?} (collides on {variant:?})"
            );
        }
    }

    /// The headline promise of PR-A: full-vocabulary actions the curated DSL
    /// never exposed are now resolvable by snake_case name.
    #[test]
    fn full_vocabulary_actions_resolve() {
        assert_eq!(action_from_name("git_blame"), Some(Action::GitBlame));
        assert_eq!(action_from_name("git_diff"), Some(Action::GitDiff));
        assert_eq!(
            action_from_name("worktree_list"),
            Some(Action::WorktreeList)
        );
        assert_eq!(action_from_name("goto_first"), Some(Action::GotoFirst));
        assert_eq!(action_from_name("toggle_pick"), Some(Action::TogglePick));
        // Count-motion defaults to 1.
        assert_eq!(action_from_name("down"), Some(Action::Down(1)));
        // Unknown names don't resolve.
        assert_eq!(action_from_name("banana"), None);
        // Excluded mark actions don't resolve.
        assert_eq!(action_from_name("set_mark"), None);
        assert_eq!(action_from_name("jump_mark"), None);
    }
}
