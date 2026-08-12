//! Top-level application state and event loop.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEventKind};

use crate::Tui;
use crate::config::{Config, StatusPosition};
use crate::fs::{Entry, EntryKind};
use crate::keymap::UserKeymap;
use crate::pane::{Pane, PaneTabs, TabEntry, TabInfo};
use crate::state::IgnoreMasks;
use crate::state::sessions::AgentKind;
// The loop's message vocabulary lives in its own module; re-exported here so
// every `super::Message` path in the app layer keeps resolving.
use crate::ui::{
    help,
    pager::{self, PagerView},
    theme::Theme,
};
use message::{Message, Wake};

/// How long to wait after spawning a restored Claude pane before
/// typing `/resume <sid>`. Banner / version-check / MCP-auth lines
/// can take well over a second to settle on cold starts; bumping
/// from the original 1500 ms reduces the race window where Claude
/// is still drawing when our keystrokes land.
const RESTORE_BANNER_SETTLE: Duration = Duration::from_secs(2);

/// Additional pause between typing `/resume <sid>` and pressing
/// Enter. A combined send (text + `\r` in one write) intermittently
/// landed in Claude's prompt mid-render — the chars stuck, the
/// trailing `\r` got dropped, and the user was left staring at an
/// unsubmitted command. Splitting the two writes a few hundred ms
/// apart gives the prompt time to settle in between.
const RESTORE_RESUME_ENTER_DELAY: Duration = Duration::from_millis(300);

/// Cadence of the post-Enter verify pass: each tick checks whether
/// `/resume <sid>` is still sitting unsubmitted in the pane tail and
/// re-sends `\r` if so. Claude eats a lone `\r` whenever its async
/// startup work (MCP connects, version check, org-message fetch)
/// remounts the input component — which can happen seconds after the
/// banner looks settled, so the fixed delays above can't close the
/// race on their own.
const RESTORE_RESUME_VERIFY_DELAY: Duration = Duration::from_secs(1);

/// How many retry `\r`s the verify pass may send before giving up
/// (≈5 s of cover past the first Enter). Retries are guarded by the
/// typed command still being visible, so a generous count is safe.
const RESTORE_RESUME_VERIFY_RETRIES: u8 = 5;

/// How many trailing pane lines the verify pass scans for the
/// unsubmitted `/resume <sid>`. The input box lives in the bottom few
/// rows; a margin covers the status line and any popup below it.
const RESTORE_RESUME_VERIFY_TAIL: usize = 15;

/// Precomputed rects for the current frame. Built by `App::compute_layout`.
/// `pub` so the `pub fn compute_layout` (now in the `render` child
/// module) doesn't expose a more-private return type; fields stay
/// private — only `app` and its descendants read them.
pub struct FrameLayout {
    status: ratatui::layout::Rect,
    list: ratatui::layout::Rect,
    divider: Option<ratatui::layout::Rect>,
    pane: Option<ratatui::layout::Rect>,
    prompt: ratatui::layout::Rect,
    /// The contiguous spyc-unit region a top overlay (`;cmd`/`$EDITOR`) or
    /// a `TopPane` pager paints over: everything above the divider when a
    /// pane is open, else the whole frame. NOT `status.y + Σheights` —
    /// with `status_position = "bottom"` the status row is the *last* row,
    /// so that construction anchors the overlay off-screen and panics.
    top_unit: ratatui::layout::Rect,
    /// The right column's content rect when a vertical split is open (the
    /// live-reloading preview), else `None`. Filled by `carve_vsplit`;
    /// `compute_layout`'s single-column branches leave it `None`.
    right: Option<ratatui::layout::Rect>,
    /// The 1-column vertical separator between the left and right columns
    /// when a vertical split is open, else `None`.
    vdivider: Option<ratatui::layout::Rect>,
}

/// Follow-up side effect a key handler asks the main loop to perform.
///
/// Anything that needs to own the tty (editor, pager, shell-out) goes
/// through this so `run()` can tear the TUI down and restore it cleanly.
#[derive(Debug)]
pub enum PostAction {
    Spawn {
        program: String,
        args: Vec<String>,
        /// Whether to pause and wait for a keypress after the child exits,
        /// so the user can read any output before the TUI is restored.
        pause_after: bool,
    },
}

mod about;
mod actions;
mod activity;
mod agent_status;
mod archive;
mod archive_ops;
mod archive_route;
mod bootstrap;
mod capture;
mod clipboard;
mod codex_pin;
pub mod command_table;
mod commands;
mod config;
mod effect;
mod file_ops;
mod find_picker;
mod focus;
mod git_state;
mod git_view_session;
mod graveyard;
mod graveyard_ops;
mod grep_session;
#[cfg(test)]
mod harness_tests;
mod harpoon;
mod image_gallery;
pub mod image_ops;
mod inventory_ops;
mod key_dispatch;
mod loop_steps;
mod lua;
mod lua_events;
mod matcher;
mod mcp;
mod mermaid_ops;
mod message;
#[cfg(test)]
mod mod_tests;
mod modal;
mod mouse;
mod mouse_mode;
mod navigate;
mod pager_handler;
mod pager_history;
mod pager_stream;
mod pane_scroll;
mod pane_tabs;
mod pane_wake;
mod paste_capture;
mod preview_ops;
mod proc;
mod prompt;
mod quick_select;
mod render;
mod route;
mod run;
mod runtime;
mod scheduler;
mod session;
mod skill;
mod sources;
pub mod state;
mod status_hooks;
mod streaming;
mod tasks;
#[cfg(test)]
mod test_harness;
mod update;
mod util;
mod view_state;
mod vsplit;
mod watch;
mod worktree_clean;
mod worktree_ops;

/// The event loop's own scratch type. Defined in [`run`] with the loop it
/// serves; re-exported because the `pub(crate)` loop-step methods across this
/// module name it in their signatures.
pub use run::RunCtx;
// The two big `App` field types live with their field docs — over half of this
// file used to be that documentation. These imports are private, so each name
// resolves throughout the `app` subtree exactly as it did when the types were
// defined here.
use runtime::{PendingScopeWait, Runtime};
pub use view_state::{ChordHint, ImageGallery, ImageView, ViewState, VisualBell};
use view_state::{HarpoonMenu, PagerReturn, TabState};

use capture::PendingCapture;
#[cfg(unix)]
pub use effect::SigOk;
pub use effect::{ClipMsg, Effect, PaneInput, PaneTarget, PaneTextKind, PaneTextSink};
use find_picker::FindPicker;
pub use matcher::Matcher;
use pager_history::PagerHistory;
use proc::{ForegroundExec, spawn_input_reader};
pub use prompt::{Prompt, PromptKind};
use scheduler::{Deadline, Scheduler, arm_resume_deadlines};
use tasks::{BackgroundTasks, TASK_BUFFER_CAP, TaskStatus};
#[cfg(unix)]
use util::kill_pg;
use util::{
    buffer_to_lines, eof_marker_line, format_elapsed_hms, format_uptime, path_basename_display,
    strip_ansi_escapes, user_host_string,
};

/// Which collection the user is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dir,
    Inventory,
    /// Graveyard view: list of soft-deleted entries (most recent
    /// first). Bindings inside: `p` restore-to-cwd, `P`
    /// restore-to-original, `dd`/`x` purge entry to system trash,
    /// `Z` purge all (with confirm), `Esc`/`gy` close.
    Graveyard,
}

/// Input mode: normal key bindings or a one-line text prompt.
pub enum Mode {
    Normal,
    Prompting(Prompt),
}

#[derive(Debug, Clone, Copy)]
enum ActivateIntent {
    Display, // $PAGER on text files
    Edit,    // $EDITOR
}

#[derive(Debug, Clone)]
pub struct FlashMessage {
    pub text: String,
    pub kind: FlashKind,
}

#[derive(Debug, Clone, Copy)]
pub enum FlashKind {
    Info,
    Error,
    /// An operation is in flight. Reads like `Info`, but it names something that
    /// hasn't finished — so whatever the operation lands as replaces it, and a
    /// completion with nothing to say clears it rather than leaving a message
    /// that claims work is still going on.
    Progress,
}

/// True when the active prompt is a file/directory-path entry
/// (copy-to, move-to, mkdir). These prompts get vi editing via
/// `Prompt::shell` but skip history nav — they share the
/// shell-command history slot, which has nothing useful for a
/// path prompt and was surfacing `make sync-all` on Up arrow.
const fn is_path_prompt_kind(mode: &Mode) -> bool {
    matches!(
        mode,
        Mode::Prompting(Prompt {
            kind: PromptKind::CopyTo | PromptKind::MoveTo | PromptKind::MakeDir,
            ..
        })
    )
}

/// Which persistent history bucket a prompt kind browses and records
/// into. Kept as a single pure mapping so the *browse* path
/// (`history_for_prompt`) and the *record-on-submit* path can't drift.
/// They did drift once: the `^a c` "pane cwd:" prompt recorded into the
/// same bucket as "pane command:", so directory paths leaked into the
/// command history's Up/Down browse. One mapping, two callers, no skew.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HistoryBucket {
    /// Shell prompts (`!`, `;`, path prompts) — the default bucket.
    Shell,
    /// "pane command:" — commands tabs were launched with.
    PaneCmd,
    /// "pane cwd:" — working directories tabs were launched in.
    PaneCwd,
    /// `J` jump-to-path destinations.
    Jump,
    /// `:` vim-style command line.
    Command,
}

const fn history_bucket_for(kind: Option<&PromptKind>) -> HistoryBucket {
    match kind {
        Some(PromptKind::PaneNewTabCmd) => HistoryBucket::PaneCmd,
        Some(PromptKind::PaneNewTabCwd) => HistoryBucket::PaneCwd,
        Some(PromptKind::Jump) => HistoryBucket::Jump,
        Some(PromptKind::Command) => HistoryBucket::Command,
        _ => HistoryBucket::Shell,
    }
}

/// TTL cache for the active pane's status-line session short-id.
/// Keyed by the active pane's `(kind, cwd, spawn_epoch_secs)` — the
/// *spawn-time* cwd from `TabInfo`, which is immutable, so switching
/// tabs re-keys but a chdir *inside* a pane does not. Anything the key
/// doesn't capture (e.g. a custom session title) is bounded by the
/// `AGENT_STATUS_TTL` re-resolve.
struct AgentStatusCache {
    computed_at: std::time::Instant,
    kind: AgentKind,
    cwd: std::path::PathBuf,
    spawn_epoch_secs: u64,
    /// Cached status string (e.g. `"claude:9a7c4dc6"`) or `None`
    /// when no session resolved. Cached either way to avoid
    /// re-running the JSON walk for "no session" repeatedly.
    status: Option<String>,
}

const AGENT_STATUS_TTL: std::time::Duration = std::time::Duration::from_secs(30);

pub struct App {
    /// Domain state — navigation, selection, filtering, config, etc.
    pub state: state::AppState,
    /// Render ephemerals + caches (see [`ViewState`]).
    view: ViewState,
    /// IO-handle cluster (channels, PtyHosts, worker endpoints, off-thread
    /// slots) — see [`Runtime`].
    runtime: Runtime,
    /// Summary printed to stdout after the TUI exits (read by `main`).
    pub exit_summary: Option<String>,
}

/// Internal per-item record used to build ListView rows each frame.
pub struct RowData {
    pub path: PathBuf,
    pub display: String,
    pub kind: EntryKind,
    /// A git-deleted file that no longer exists on disk, synthesized into the
    /// `Dir` listing so the deletion is visible (rendered struck-through).
    /// `path` is the would-be location; opening it is guarded, and a future
    /// restore (`gr`) brings it back. `false` for every real on-disk row.
    pub deleted: bool,
}

impl RowData {
    /// The key under which this row's git status lives in `git.files`. That
    /// map keys files by bare basename and directories by `basename/` (see
    /// `git::status::map_to_listing`) — which equals `display` for every kind
    /// EXCEPT executables: `Entry::display_name` decorates those with a
    /// trailing `*` (ls -F style) that the git map never carries, so looking up
    /// by raw `display` silently fails to find any executable's status. Strip
    /// that one suffix so executable files surface their markers like any other
    /// file. (A file genuinely named `foo*` decorates to `foo**`, so stripping
    /// one `*` still yields its real basename key.)
    pub fn git_key(&self) -> &str {
        match self.kind {
            EntryKind::Executable => self.display.strip_suffix('*').unwrap_or(&self.display),
            _ => &self.display,
        }
    }
}

impl App {
    // MVU Phase 5: `yank_pane_to_clipboard` / `yank_scrollback_to_clipboard`
    // are gone — their live-pane read + guards + clipboard IO moved into
    // `run_effects`'s `Effect::ReadPaneText` executor. The `yp`/`ya` action
    // arms in `actions.rs` now emit `ReadPaneText { kind, then: Clipboard }`
    // directly, so the handler stays pure-Model (no Runtime read).

    /// Title used for the help pager. Also used by the resize handler to
    /// detect when help is open and needs rebuilding for the new width.
    const HELP_TITLE: &'static str = "spyc — key bindings";

    /// Build and show the help pager. Called from `Action::Help` and on
    /// terminal resize (to re-wrap descriptions for the new width and
    /// pick the right column count).
    fn open_help(&mut self) {
        let (term_w, _) = self.view.term_size;
        // Require at least ~40 chars of description space per column
        // before committing to 2-col (prefix is ~30 chars, so col_w ≥ 70,
        // body ≥ 140). Below that, 2-col cramps descriptions more than a
        // single wider column would.
        let ncols: u16 = if pager::centered_body_width(term_w) < 140 {
            1
        } else {
            2
        };
        let col_w = pager::centered_col_width(term_w, ncols) as usize;
        let lines = help::build_lines(&self.view.theme, &self.state.user_keymap, col_w);
        let mut view = pager::PagerView::new_styled(Self::HELP_TITLE, lines);
        view.columns = ncols as u8;
        view.no_history = true;
        self.set_pager(view);
    }

    /// True when the help pager is the currently-open pager view.
    fn help_is_open(&self) -> bool {
        self.view
            .pager
            .as_ref()
            .is_some_and(|v| v.title == Self::HELP_TITLE)
    }
}

/// Place the OS terminal cursor at the focused pty pane's vt100
/// cursor position so alt-screen TUIs (nvim, less, htop, lazygit)
/// render a visible cursor. Without this they show no cursor at
/// all: spyc hides the host cursor at startup
/// (`main.rs::setup_terminal`), and the v1.41.18-era pane-widget
/// guard correctly stops us from painting a reverse-block over the
/// child's cursor shape in alt-screen — but the host cursor stays
/// hidden unless something asks ratatui to position it.
///
/// No-ops when the pane is missing or the child has hidden the
/// cursor via DEC ?25l (vt100 surfaces this as `hide_cursor()`).
/// Skips the call when the cursor would land outside the pane's
/// drawable rect, which can happen briefly during a resize.
/// Build a `ForegroundExec` effect that runs `cmd` through `sh -c` so shell features
/// (pipes, redirection, `$VAR`) work.
fn sh_c(cmd: &str, pause_after: bool) -> Vec<Effect> {
    PostAction::Spawn {
        program: "sh".to_string(),
        args: vec!["-c".to_string(), cmd.to_string()],
        pause_after,
    }
    .into()
}

pub fn row_from_entry(e: &Entry) -> RowData {
    RowData {
        path: e.path.clone(),
        display: e.display_name(),
        kind: e.kind,
        deleted: false,
    }
}
