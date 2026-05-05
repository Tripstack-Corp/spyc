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
    CopyPrompt,    // c — prompt for destination, cp -r selection
    MovePrompt,    // M — prompt for destination, mv selection
    RemovePrompt,  // R — confirm, rm -rf selection
    MakeDirPrompt, // + — prompt for a directory name
    NewFilePrompt, // N — prompt for a filename, open in $EDITOR
    LongList,      // L — ls -lh selection | $PAGER
    FileType,      // f — file(1) on selection, paged output

    // Marks (vi-style named bookmarks).
    SetMark(char),  // m{a-z}
    JumpMark(char), // '{a-z}
    JumpPrevDir,    // '' — jump back to directory before last chdir
    JumpStartDir,   // ` — jump to directory where spyc was launched

    // Project home (sticky project root).
    JumpProjectHome,    // gh — jump to PROJECT_HOME
    SetProjectHomeHere, // gP — set PROJECT_HOME to current directory

    // Start dir (target of backtick `).
    SetStartDirHere, // gS — set start_dir to current directory

    // Identity.
    ShowUserHost, // gU — flash user@host in the status line

    // Edit in pane.
    EditInPane, // V — open $EDITOR in pane tab (file list stays visible)

    // Info commands.
    Date,           // D — show date/time
    Version,        // gV / :version — show spyc version
    ShowMemory,     // I — session info pager (version, pid, rss, counts)
    ColorToggle,    // C — toggle color theme on/off
    SetEnvPrompt,   // s — NAME=VALUE prompt
    ToggleActivity, // A — toggle draws/sec, bytes/sec overlay

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
    TogglePaneZoom,     // ^W z — zoom (fullscreen) the pane / restore split
    PaneScrollEnter,    // ^W v — enter scroll mode (browse scrollback)
    PaneScrollSave,     // s (while in scroll mode) — save scrollback to file
    PaneNewTab,         // ^W n — open a new pane tab (prompt for command + cwd)
    PaneCloseTab,       // ^W x — close the active pane tab
    PaneTabByIndex(u8), // ^W 1..9 — switch to tab N
    PaneNextTab,        // ^W ] — next tab
    PanePrevTab,        // ^W [ — previous tab
    PaneRenameTab,      // ^W r — rename the active tab
    PaneRestartTab,     // ^W R — restart the active tab's command
    PanePipeContent,    // ^W p — send file contents of selection to pane
    PanePipeInventory,  // ^W i — send file contents of inventory to pane

    // Graveyard.
    OpenGraveyardView, // g y — open the graveyard viewer

    // Quick Select — wezterm-style labeled overlay over pane output.
    QuickSelectOpen, // ^a u — scan visible pane, label matches, pick to yank/open

    // Harpoon — small per-project pinned list of file pointers.
    HarpoonJump(u8), // H 1..9 — chdir to slot N's parent + cursor on it
    HarpoonAppend,   // H a — append cursor file/dir to harpoon list
    HarpoonRemove,   // H x — remove cursor file from harpoon list
    HarpoonOpenMenu, // H h / g h — open the harpoon menu (reorder, delete, jump)

    // Git worktree (M11).
    WorktreeList,   // W l — list worktrees, pick to chdir
    WorktreeNew,    // W n — prompt for branch, create worktree
    WorktreeDelete, // W d — confirm, remove current worktree

    // Git diff (M12).
    GitDiff,       // g d — diff-vs-HEAD for cursor file / selection (staged + unstaged + new)
    GitDiffCached, // g D — staged (cached) diff
    GitBlame,      // g b — git blame on cursor file

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
            Self::EditInPane => "open editor in top pane (Claude pane stays visible)",
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
            Self::RemovePrompt => "remove (confirm)",
            Self::MakeDirPrompt => "make directory (prompt)",
            Self::NewFilePrompt => "new file in editor (prompt)",
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
            Self::TogglePaneZoom => "zoom pane (toggle fullscreen)",
            Self::PaneScrollEnter => "scroll pane history",
            Self::PaneScrollSave => "save pane scrollback",
            Self::PaneNewTab => "new pane tab",
            Self::PaneCloseTab => "close pane tab",
            Self::PaneTabByIndex(_) => "switch pane tab",
            Self::PaneNextTab => "next pane tab",
            Self::PanePrevTab => "prev pane tab",
            Self::PaneRenameTab => "rename pane tab",
            Self::PaneRestartTab => "restart pane tab command",
            Self::PanePipeContent => "pipe file contents to pane",
            Self::PanePipeInventory => "pipe inventory contents to pane",
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
            Self::GitBlame => "git blame (cursor file)",
            Self::GotoFile => "jump to path in pane output",
            Self::GotoFileLine => "jump to path:line in pane output",
            Self::SetMark(_) => "set mark",
            Self::JumpMark(_) => "jump to mark",
            Self::JumpPrevDir => "jump to previous directory",
            Self::JumpStartDir => "jump to starting directory",
            Self::JumpProjectHome => "jump to PROJECT_HOME",
            Self::SetProjectHomeHere => "set PROJECT_HOME to current dir",
            Self::SetStartDirHere => "set start dir to current dir (target of `)",
            Self::ShowUserHost => "flash user@host",
            Self::Date => "show date",
            Self::Version => "show version",
            Self::ShowMemory => "session info",
            Self::ColorToggle => "toggle colors",
            Self::SetEnvPrompt => "set env var",
            Self::ToggleActivity => "toggle activity monitor",
            Self::Redraw => "redraw",
            Self::Quit => "quit",
            Self::MacroRecordReserved => "(reserved: macro recording)",
            Self::OpenTaskViewer => "open task viewer (most-recent bg task)",
            Self::ReopenLastBuffer => "reopen the most-recent closed pager buffer",
            Self::FindFile => "find file (project-wide fuzzy)",
            Self::Noop => "no-op",
        }
    }
}
