/// The full vocabulary of things cspy can do in response to input.
///
/// Keep this enum stable and spy-parity-friendly: each variant should map to
/// one user-observable behavior, so `.cspyrc` can bind any key to any action.
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
    Take,                // y / Y
    Drop,                // p
    ToggleInventoryView, // i
    EmptyInventory,      // z

    // Ignore masks.
    ToggleMask(u8), // a -> 1, o -> 2

    // Shell-out.
    ShellPrompt,    // ! or ; — prompts for a command; `%` expands to selection
    StartShell,     // $ — drops you into $SHELL in the current directory
    ChmodAdd(char), // ^W -> 'w', ^X -> 'x'

    // Search.
    SearchPrompt, // / — start incremental search
    SearchNext,   // n — repeat last search forward
    SearchPrev,   // N — repeat last search backward

    // Navigation.
    JumpPrompt, // J — prompt for a path (~, $VAR expanded) and chdir

    // File operations.
    CopyPrompt,    // c — prompt for destination, cp -r selection
    MovePrompt,    // m — prompt for destination, mv selection
    RemovePrompt,  // R — confirm, rm -rf selection
    MakeDirPrompt, // N — prompt for a directory name
    LongList,      // L — ls -lh selection | $PAGER
    FileType,      // f — file(1) on selection, paged output

    // Help.
    Help, // ? or F1 — key bindings overlay

    // Meta.
    Redraw, // ^L
    Quit,   // ^D / Q / q

    // Placeholder for keys we reserve but haven't implemented yet.
    #[allow(dead_code)]
    Noop,
}
