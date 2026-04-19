/// The full vocabulary of things spyc can do in response to input.
///
/// Keep this enum stable and spy-parity-friendly: each variant should map to
/// one user-observable behavior, so `.spycrc` can bind any key to any action.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    EnterOrDisplay, // d / <Enter> — dir: chdir; text file: pager
    EnterOrEdit,    // e / v       — dir: chdir; file: editor
    Climb,          // u / -
    Home,           // H / ~

    // Picks (per-directory multi-select).
    TogglePick,        // t
    PickPatternPrompt, // T
    PickToggleAll,     // ^T

    // Inventory (global, cross-directory).
    Take,                // y — yank into inventory
    Untake,              // Y — remove from inventory (from dir view)
    Drop,                // p — put inventory to cwd
    ToggleInventoryView, // i
    EmptyInventory,      // z

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
    CopyPrompt,    // c — prompt for destination, cp -r selection
    MovePrompt,    // M — prompt for destination, mv selection
    RemovePrompt,  // R — confirm, rm -rf selection
    MakeDirPrompt, // + — prompt for a directory name
    LongList,      // L — ls -lh selection | $PAGER
    FileType,      // f — file(1) on selection, paged output

    // Marks (vi-style named bookmarks).
    SetMark(char),  // m{a-z}
    JumpMark(char), // '{a-z}
    JumpPrevDir,    // '' — jump back to directory before last chdir
    JumpStartDir,   // ` — jump to directory where spyc was launched

    // Edit in pane.
    EditInPane, // V — open $EDITOR in pane tab (file list stays visible)

    // Info commands.
    Date,         // D — show date/time
    Version,      // gV / :version — show spyc version
    ShowMemory,   // I — session info pager (version, pid, rss, counts)
    ColorToggle,  // C — toggle color theme on/off
    SetEnvPrompt, // s — NAME=VALUE prompt

    // Help.
    Help, // ? or F1 — key bindings overlay

    // Config reload.
    ReloadConfig, // ^R — re-read ~/.spycrc.toml + project config

    // Split pane (M8).
    TogglePane,         // Ctrl-\ / F10 / ^W \ / ^W c — open/close the pty pane
    ResumePane,         // F11 — open pane with `claude --resume`
    PaneFocusDown,      // ^W j — move focus down (to pane)
    PaneFocusUp,        // ^W k — move focus up (to list)
    PaneSendSelection,  // ^W s — send shell-quoted selection to pane stdin
    PaneGrow,           // ^W + — bottom pane takes more height
    PaneShrink,         // ^W - — bottom pane takes less height
    PaneScrollEnter,    // ^W v — enter scroll mode (browse scrollback)
    PaneScrollSave,     // s (while in scroll mode) — save scrollback to file
    PaneNewTab,         // ^W n — open a new pane tab (prompt for command + cwd)
    PaneCloseTab,       // ^W x — close the active pane tab
    PaneTabByIndex(u8), // ^W 1..9 — switch to tab N
    PaneNextTab,        // ^W ] — next tab
    PanePrevTab,        // ^W [ — previous tab
    PaneRenameTab,      // ^W r — rename the active tab
    PanePipeContent,    // ^W p — send file contents of selection to pane
    PanePipeInventory,  // ^W i — send file contents of inventory to pane

    // Git worktree (M11).
    WorktreeList,   // W l — list worktrees, pick to chdir
    WorktreeNew,    // W n — prompt for branch, create worktree
    WorktreeDelete, // W d — confirm, remove current worktree

    // Git diff (M12).
    GitDiff,       // g d — unstaged diff for cursor file / selection
    GitDiffCached, // g D — staged (cached) diff

    // Path references (M13).
    GotoFile,     // g f — jump file list to path reference in pane output
    GotoFileLine, // g F — jump + open pager at line

    // Meta.
    Redraw, // ^L
    Quit,   // ^D / Q / q

    // Placeholder for keys we reserve but haven't implemented yet.
    #[allow(dead_code)]
    Noop,
}

impl Action {
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
            Self::EditInPane => "open editor in pane (file list stays visible)",
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
            Self::RemovePrompt => "remove (confirm)",
            Self::MakeDirPrompt => "make directory (prompt)",
            Self::LongList => "long listing",
            Self::FileType => "file type",
            Self::Help => "help",
            Self::ReloadConfig => "reload config",
            Self::TogglePane => "toggle split pane",
            Self::ResumePane => "open pane with claude --resume",
            Self::PaneFocusDown => "focus pane (down)",
            Self::PaneFocusUp => "focus list (up)",
            Self::PaneSendSelection => "send selection to pane",
            Self::PaneGrow => "grow pane",
            Self::PaneShrink => "shrink pane",
            Self::PaneScrollEnter => "scroll pane history",
            Self::PaneScrollSave => "save pane scrollback",
            Self::PaneNewTab => "new pane tab",
            Self::PaneCloseTab => "close pane tab",
            Self::PaneTabByIndex(_) => "switch pane tab",
            Self::PaneNextTab => "next pane tab",
            Self::PanePrevTab => "prev pane tab",
            Self::PaneRenameTab => "rename pane tab",
            Self::PanePipeContent => "pipe file contents to pane",
            Self::PanePipeInventory => "pipe inventory contents to pane",
            Self::WorktreeList => "list git worktrees",
            Self::WorktreeNew => "new git worktree",
            Self::WorktreeDelete => "delete git worktree",
            Self::GitDiff => "git diff (unstaged)",
            Self::GitDiffCached => "git diff --cached (staged)",
            Self::GotoFile => "jump to path in pane output",
            Self::GotoFileLine => "jump to path:line in pane output",
            Self::SetMark(_) => "set mark",
            Self::JumpMark(_) => "jump to mark",
            Self::JumpPrevDir => "jump to previous directory",
            Self::JumpStartDir => "jump to starting directory",
            Self::Date => "show date",
            Self::Version => "show version",
            Self::ShowMemory => "session info",
            Self::ColorToggle => "toggle colors",
            Self::SetEnvPrompt => "set env var",
            Self::Redraw => "redraw",
            Self::Quit => "quit",
            Self::Noop => "no-op",
        }
    }
}
