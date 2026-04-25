//! Top-level application state and event loop.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::spyc_debug;
use ansi_to_tui::IntoText;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use glob::Pattern;
use ratatui::Frame;

use crate::config::{Config, StatusPosition};
use crate::fs::{self, Entry, EntryKind, Listing};
use crate::keymap::{Action, BoundAction, Resolver, ResolverOutcome, UserKeymap};
use crate::pane::{Pane, PaneTabs, PaneWidget, TabEntry, TabInfo};
use crate::shell;
use crate::state::{Cursor, History, IgnoreMasks, Inventory, Marks, Picks};
use crate::ui::line_edit::LineEditor;
use crate::ui::{
    help,
    list_view::{Grid, ListView, Row},
    pager::{self, PagerView},
    prompt::PromptLine,
    status::StatusBar,
    theme::Theme,
};
use crate::{Tui, resume_tui, suspend_tui};

/// Precomputed rects for the current frame. Built by `App::compute_layout`.
/// Background capture for a `!` command. The child runs under a PTY
/// (so programs that open `/dev/tty` for prompts — sudo, ssh, gpg —
/// see the slave PTY instead of bleeding onto our real terminal).
/// A reader thread feeds bytes into the channel. While the capture is
/// live, typed keys are forwarded to the child via the master writer
/// so the user can answer prompts. Ctrl+C kills the child outright.
struct PendingCapture {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Master-side writer — typed keys go here as encoded ANSI bytes.
    writer: Box<dyn std::io::Write + Send>,
    /// Receives chunks of stdout as they arrive (not all at once).
    output_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    /// Accumulated raw bytes for the pager (ANSI included).
    buffer: Vec<u8>,
    title: String,
    cmd_display: String,
    /// When the capture started — for the elapsed timer.
    started: std::time::Instant,
    /// True once the reader thread has sent all output.
    finished: bool,
}

struct FrameLayout {
    status: ratatui::layout::Rect,
    list: ratatui::layout::Rect,
    divider: Option<ratatui::layout::Rect>,
    pane: Option<ratatui::layout::Rect>,
    prompt: ratatui::layout::Rect,
}

/// Follow-up side effect a key handler asks the main loop to perform.
///
/// Anything that needs to own the tty (editor, pager, shell-out) goes
/// through this so `run()` can tear the TUI down and restore it cleanly.
#[derive(Debug, Default)]
pub enum PostAction {
    #[default]
    None,
    Spawn {
        program: String,
        args: Vec<String>,
        /// Whether to pause and wait for a keypress after the child exits,
        /// so the user can read any output before the TUI is restored.
        pause_after: bool,
    },
}

pub mod state;

/// Which collection the user is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dir,
    Inventory,
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
}

/// State for returning to the pager after `v` (edit) exits.
enum PagerReturn {
    /// Buffer content: reload from this temp file, then delete it.
    TempFile {
        path: PathBuf,
        title: String,
        scroll: u16,
    },
    /// On-disk file: reopen from the original path.
    SourceFile { path: PathBuf, scroll: u16 },
}

pub struct Prompt {
    pub kind: PromptKind,
    pub prefix: String,
    pub buffer: String,
    /// When set, this prompt uses the vi line editor with history.
    #[allow(dead_code)]
    pub editor: Option<crate::ui::line_edit::LineEditor>,
}

impl Prompt {
    /// Simple prompt (pattern pick, search, jump, etc.) — no vi editing.
    fn simple(kind: PromptKind, prefix: impl Into<String>) -> Self {
        Self {
            kind,
            prefix: prefix.into(),
            buffer: String::new(),
            editor: None,
        }
    }

    /// Shell prompt (`!` / `;`) — vi line editor with history support.
    fn shell(kind: PromptKind, prefix: impl Into<String>) -> Self {
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
}

/// Stack of recently-closed pager views, for `:bprev`/`:bnext`.
/// Works like a browser back/forward stack.
struct PagerHistory {
    back: Vec<pager::PagerView>,
    forward: Vec<pager::PagerView>,
}

const MAX_PAGER_HISTORY: usize = 10;

impl PagerHistory {
    const fn new() -> Self {
        Self {
            back: Vec::new(),
            forward: Vec::new(),
        }
    }

    /// Save a closed pager view. Clears the forward stack.
    fn push(&mut self, view: pager::PagerView) {
        self.back.push(view);
        self.forward.clear();
        if self.back.len() > MAX_PAGER_HISTORY {
            self.back.remove(0);
        }
    }

    /// Go back: push current to forward, pop from back.
    fn go_back(&mut self, current: pager::PagerView) -> Option<pager::PagerView> {
        let prev = self.back.pop()?;
        self.forward.push(current);
        Some(prev)
    }

    /// Go forward: push current to back, pop from forward.
    fn go_forward(&mut self, current: pager::PagerView) -> Option<pager::PagerView> {
        let next = self.forward.pop()?;
        self.back.push(current);
        Some(next)
    }

    fn back_len(&self) -> usize {
        self.back.len()
    }

    fn forward_len(&self) -> usize {
        self.forward.len()
    }
}

#[allow(clippy::struct_excessive_bools)]
pub struct App {
    /// Domain state — navigation, selection, filtering, config, etc.
    pub state: state::AppState,

    // --- UI/terminal state (stays in App) ---
    pager: Option<PagerView>,
    pager_history: PagerHistory,
    pager_pending_bracket: Option<char>,
    pager_was_open: bool,
    pager_jump_buf: Option<String>,
    pane_tabs: Option<PaneTabs>,
    top_overlay: Option<Pane>,
    overlay_awaiting_dismiss: bool,
    pending_overlay_close: bool,
    pending_capture: Option<PendingCapture>,
    pending_history_pick: Option<LineEditor>,
    history_pending_g: bool,
    /// Pending `g` in pane scroll mode — `gg` scrolls to top, `gf`/`gF`
    /// jump to file reference.
    scroll_pending_g: bool,
    pending_pager_return: Option<PagerReturn>,
    needs_full_repaint: bool,
    theme: Theme,
    /// Path to the `.spyc-context.json` file (project root).
    /// Written each loop iteration so the MCP server can read it.
    context_path: PathBuf,
    /// Last serialized context JSON — skip disk write when unchanged.
    last_context_json: String,
    /// Whether the MCP socket server is running.
    mcp_running: bool,
    /// Summary printed to stdout after the TUI exits.
    pub exit_summary: Option<String>,
    /// Accumulates characters the user types while the pane is focused.
    pane_prompt_buf: String,
    /// The last complete prompt the user sent to the pane (Enter commits).
    last_pane_prompt: Option<String>,
    /// Activity monitor: draws/sec, bytes/sec overlay.
    show_activity: bool,
    activity_draws: u32,
    activity_bytes: u64,
    activity_last_tick: std::time::Instant,
    /// Snapshot from last 1-second window.
    activity_dps: u32,
    activity_bps: u64,
    /// Draw reason counters for the current window.
    activity_reason_pane: u32,
    activity_reason_event: u32,
    activity_reason_other: u32,
    /// Snapshot reasons.
    activity_snap_pane: u32,
    activity_snap_event: u32,
    activity_snap_other: u32,
    /// Cached `build_rows()` output; invalidated by `list_generation`.
    cached_rows: Vec<Row>,
    cached_rows_gen: u64,
    /// Grid stabilization cache key: (list_gen, view_top, cursor, width, height).
    cached_grid_key: (u64, usize, usize, u16, u16),
    /// Commands from the MCP server (writable actions from Claude).
    mcp_cmd_rx: std::sync::mpsc::Receiver<crate::mcp_cmd::McpRequest>,
    /// Tab completion / cycle state. Tracks matches from the last Tab
    /// press and supports cycling through them on repeated Tab.
    tab_state: Option<TabState>,
    /// Scroll throttle: timestamp + direction of last processed arrow key.
    /// DEC 1007 alternate-scroll turns trackpad into arrow keys at 60+ Hz;
    /// we rate-limit to ~25/sec (40ms gap) so inertia doesn't fly.
    scroll_last: Option<(std::time::Instant, KeyCode)>,
}

/// State for Tab-completion cycling. Tracks the original buffer, the
/// computed completions, and which one is currently filled in.
struct TabState {
    /// Buffer content when the first Tab was pressed.
    original_buf: String,
    /// Shell command prefix (e.g., "ls " for `!ls ~/Do<tab>`), empty for J prompt.
    buf_prefix: String,
    /// Path prefix up to the last `/` in the typed word (e.g., "~/").
    word_base: String,
    /// Matched file/dir names (e.g. `Documents/`, `Downloads/`).
    matches: Vec<String>,
    /// 0 = list was just shown (first Tab). 1+ = cycling through matches.
    cycle_index: usize,
}

/// Internal per-item record used to build ListView rows each frame.
pub struct RowData {
    pub path: PathBuf,
    pub display: String,
    pub kind: EntryKind,
}

impl App {
    pub fn new(resume: bool, mcp_takeover_allowed: bool) -> Self {
        let (cwd, start_error) = if let Ok(d) = std::env::current_dir() {
            (d, None)
        } else {
            // cwd not accessible — fall back to $HOME.
            let home = std::env::var("HOME").map_or_else(|_| PathBuf::from("/tmp"), PathBuf::from);
            let _ = std::env::set_current_dir(&home);
            (
                home,
                Some("cwd not accessible, started in $HOME".to_string()),
            )
        };
        let (listing, start_error) = match Listing::read(&cwd) {
            Ok(l) => (l, start_error),
            Err(e) => (
                Listing::empty(cwd.clone()),
                Some(start_error.unwrap_or_default() + &format!("{e}")),
            ),
        };
        let git_info = crate::sysinfo::git_status(&cwd);
        let git_files = crate::sysinfo::git_file_statuses(&cwd);
        let (config, load_note) = match Config::load_default(&cwd) {
            Ok(c) => {
                let note = if c.sources.is_empty() {
                    None
                } else {
                    Some(format!("loaded {} config file(s)", c.sources.len()))
                };
                (c, note)
            }
            Err(e) => (Config::default(), Some(format!("config error: {e}"))),
        };
        let user_keymap = UserKeymap::from_bindings(config.bindings.clone());
        let theme = Theme::default().with_overrides(&config.colors);

        // Literal .git check, no upward walk — keep the concept explicit.
        let project_home = cwd.join(".git").exists().then(|| cwd.clone());
        let session_name = Some(crate::state::session_names::generate());

        // Run health check before loading state — cleans up orphaned
        // files so Inventory::load() et al. see a consistent directory.
        let health_warnings = if let Some(sd) = crate::state::health::state_dir() {
            let report = crate::state::health::check(&sd);
            if report.cleaned > 0 {
                spyc_debug!("health check: cleaned {} orphaned file(s)", report.cleaned);
            }
            report.warnings
        } else {
            Vec::new()
        };

        let app_state = state::AppState {
            listing,
            picks: Picks::new(),
            inventory: Inventory::load(),
            marks: Marks::load(),
            masks: {
                let mut m = IgnoreMasks::default();
                m.apply_config(&config.ignore_masks);
                m
            },
            temp_filter: None,
            sort_order: crate::fs::listing::SortMode::Name,
            view: View::Dir,
            cursor: Cursor::new(),
            resolver: Resolver::new(),
            user_keymap,
            config,
            mode: Mode::Normal,
            project_home,
            session_name,
            frecency: crate::state::Frecency::load(),
            pane_focused: false,
            // spyc (top) = 30%, pane (bottom) = 70%. Resize with `^W +/-`.
            pane_height_pct: 70,
            pending_new_tab_cmd: None,
            last_captured_cmd: None,
            pending_worktrees: None,
            pending_sessions: None,
            start_dir: cwd,
            prev_dir: None,
            last_search: None,
            quit_pending: None,
            history: History::load(),
            pane_history: History::load_file("pane_history"),
            flash: start_error.map(|text| FlashMessage {
                text,
                kind: FlashKind::Error,
            }),
            user_host: user_host_string(),
            git_info,
            git_files,
            should_quit: false,
            rows: Vec::new(),
            last_grid: Grid {
                cols: 1,
                rows: 1,
                col_widths: vec![20],
            },
            list_generation: 0,
        };
        let context_path = crate::context::context_path(&app_state.start_dir);
        // Command channel for writable MCP actions (Claude → main loop).
        let (mcp_cmd_tx, mcp_cmd_rx) = std::sync::mpsc::channel();
        // Start the MCP Unix socket server so `spyc --mcp` (spawned by
        // Claude Code) can proxy to us for full read/write MCP access.
        let mcp_running = crate::mcp::start_socket_server(context_path.clone(), mcp_cmd_tx)
            .map_or_else(
                |e| {
                    spyc_debug!("MCP socket server failed to start: {e}");
                    false
                },
                |()| true,
            );
        let mut app = Self {
            state: app_state,
            pager: None,
            pager_history: PagerHistory::new(),
            pager_pending_bracket: None,
            pager_was_open: false,
            pager_jump_buf: None,
            pane_tabs: None,
            top_overlay: None,
            overlay_awaiting_dismiss: false,
            pending_overlay_close: false,
            pending_capture: None,
            pending_history_pick: None,
            history_pending_g: false,
            scroll_pending_g: false,
            pending_pager_return: None,
            needs_full_repaint: false,
            theme,
            context_path,
            last_context_json: String::new(),
            mcp_running,
            exit_summary: None,
            pane_prompt_buf: String::new(),
            last_pane_prompt: None,
            show_activity: false,
            activity_draws: 0,
            activity_bytes: 0,
            activity_last_tick: std::time::Instant::now(),
            activity_dps: 0,
            activity_bps: 0,
            activity_reason_pane: 0,
            activity_reason_event: 0,
            activity_reason_other: 0,
            activity_snap_pane: 0,
            activity_snap_event: 0,
            activity_snap_other: 0,
            cached_rows: Vec::new(),
            cached_rows_gen: u64::MAX, // force first build
            cached_grid_key: (u64::MAX, 0, 0, 0, 0),
            mcp_cmd_rx,
            tab_state: None,
            scroll_last: None,
        };
        app.state.rebuild_rows();
        if let Some(msg) = load_note {
            app.state.flash_info(msg);
        }
        // Surface any health check warnings so the user knows state
        // was repaired. Overrides the config load note if both exist.
        if !health_warnings.is_empty() {
            app.state.flash_error(health_warnings.join("; "));
        }
        if resume {
            app.show_session_picker();
        }
        // Write .mcp.json so Claude Code spawns `spyc --mcp` (stdio),
        // which proxies to our Unix socket.
        if app.mcp_running {
            app.ensure_mcp_config(mcp_takeover_allowed);
        }
        app
    }

    /// Write `.mcp.json` with stdio transport on startup.
    /// If enterprise policy blocks spyc, flash an error instead.
    fn ensure_mcp_config(&mut self, takeover_allowed: bool) {
        match crate::mcp::ensure_mcp_json(&self.state.listing.dir, takeover_allowed) {
            Ok(crate::mcp::McpConfigStatus::Configured) => {}
            Ok(crate::mcp::McpConfigStatus::TookOver { old_pid }) => {
                self.state
                    .flash_info(format!("MCP: took over from PID {old_pid}"));
            }
            Ok(crate::mcp::McpConfigStatus::SkippedTakeover { old_pid }) => {
                self.state.flash_info(format!(
                    "MCP: kept PID {old_pid} as owner (Claude here will talk to it)"
                ));
            }
            Ok(crate::mcp::McpConfigStatus::BlockedByEnterprise) => {
                self.state.flash_error(
                    "MCP: blocked by enterprise policy (deniedMcpServers or allowedMcpServers)",
                );
            }
            Err(e) => self.state.flash_error(format!(".mcp.json: {e}")),
        }
    }

    /// Build a context snapshot from the current state for MCP consumers.
    fn snapshot_context(&self) -> crate::context::SpycContext {
        let cursor_file = self
            .state
            .rows
            .get(self.state.cursor.index)
            .map(|r| r.display.clone());
        crate::context::SpycContext {
            cwd: self.state.listing.dir.clone(),
            cursor_file,
            picks: self.state.picks.iter().cloned().collect(),
            inventory: self.state.inventory.paths().cloned().collect(),
            filter: self.state.temp_filter.clone(),
            git_branch: self.state.git_info.clone(),
            project_home: self.state.project_home.clone(),
            session_name: self.state.session_name.clone().unwrap_or_default(),
        }
    }

    /// Write the context file (best-effort, errors are silently ignored).
    /// Skips the disk write when the serialized JSON is unchanged.
    fn write_context(&mut self) {
        let ctx = self.snapshot_context();
        let json = serde_json::to_string_pretty(&ctx).unwrap_or_default();
        if json == self.last_context_json {
            return;
        }
        let _ = crate::context::write_context_file(&self.context_path, &ctx);
        self.last_context_json = json;
    }

    /// Execute a writable MCP command from Claude. Runs on the main
    /// thread with full access to `AppState`. Returns a response that
    /// the MCP server thread forwards to Claude.
    fn execute_mcp_command(
        &mut self,
        cmd: crate::mcp_cmd::McpCommand,
    ) -> crate::mcp_cmd::McpResponse {
        use crate::mcp_cmd::{McpCommand, McpResponse};
        match cmd {
            McpCommand::NavigateTo { path } => {
                match self.state.jump_to(&path) {
                    Ok(()) => {
                        self.state.flash_info(format!(
                            "[mcp] navigated to {}",
                            self.state.listing.dir.display()
                        ));
                        // Force context write so get_spyc_context reflects
                        // the new state immediately.
                        self.write_context();
                        let ctx = self.snapshot_context();
                        let json = serde_json::to_string_pretty(&ctx).unwrap_or_default();
                        McpResponse::Ok { message: json }
                    }
                    Err(e) => McpResponse::Error {
                        message: format!("navigate failed: {e}"),
                    },
                }
            }
            McpCommand::SetFilter { pattern } => {
                match pattern {
                    Some(ref p) if p.is_empty() => self.state.temp_filter = None,
                    Some(p) => self.state.temp_filter = Some(p),
                    None => self.state.temp_filter = None,
                }
                self.state.rebuild_rows();
                let count = self.state.rows.len();
                let label = self.state.temp_filter.as_deref().unwrap_or("(cleared)");
                self.state.flash_info(format!("[mcp] filter: {label}"));
                self.write_context();
                McpResponse::Ok {
                    message: format!("filter applied, {count} items visible"),
                }
            }
            McpCommand::PickFiles { patterns } => {
                let mut total = 0usize;
                let mut errors = Vec::new();
                for pat_str in &patterns {
                    match glob::Pattern::new(pat_str) {
                        Ok(pat) => {
                            for e in &self.state.listing.entries {
                                if pat.matches(&e.name) {
                                    self.state.picks.insert(&e.path);
                                    total += 1;
                                }
                            }
                        }
                        Err(e) => errors.push(format!("{pat_str}: {e}")),
                    }
                }
                self.state.list_generation = self.state.list_generation.wrapping_add(1);
                if !errors.is_empty() {
                    return McpResponse::Error {
                        message: format!("invalid patterns: {}", errors.join(", ")),
                    };
                }
                self.state
                    .flash_info(format!("[mcp] picked {total} file(s)"));
                self.write_context();
                McpResponse::Ok {
                    message: format!("picked {total} file(s), {} total", self.state.picks.len()),
                }
            }
            McpCommand::ClearPicks => {
                let count = self.state.picks.len();
                self.state.picks.clear();
                self.state.list_generation = self.state.list_generation.wrapping_add(1);
                self.state.flash_info("[mcp] picks cleared");
                self.write_context();
                McpResponse::Ok {
                    message: format!("cleared {count} pick(s)"),
                }
            }
            McpCommand::Disconnected { new_pid } => {
                self.mcp_running = false;
                self.state.flash_error(format!(
                    "MCP taken over by spyc PID {new_pid} — Claude is connected to that instance"
                ));
                McpResponse::Ok {
                    message: "acknowledged".into(),
                }
            }
        }
    }

    /// Reload `.spycrc.toml` and rebuild the user keymap. Leaves the old
    /// config in place on failure and flashes the error.
    pub fn reload_config(&mut self) {
        match Config::load_default(&self.state.listing.dir) {
            Ok(new_config) => {
                self.state.user_keymap = UserKeymap::from_bindings(new_config.bindings.clone());
                self.theme = Theme::default().with_overrides(&new_config.colors);
                // Reset to built-in mask defaults first, then apply config
                // overrides — so removing `[[ignore_masks]]` entries from
                // the rc file reverts the group to defaults on reload.
                self.state.masks = IgnoreMasks::default();
                self.state.masks.apply_config(&new_config.ignore_masks);
                let count = new_config.sources.len();
                self.state.config = new_config;
                self.state.rebuild_rows();
                self.state
                    .flash_info(format!("reloaded {count} config file(s)"));
            }
            Err(e) => self.state.flash_error(format!("config error: {e}")),
        }
    }

    /// Candidate config paths — used by the file watcher. We watch the
    /// directories holding these even when the files don't exist yet so
    /// that `touch ~/.spycrc.toml` picks up immediately.
    fn candidate_config_paths(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(home).join(".spycrc.toml"));
        }
        out.push(self.state.listing.dir.join(".spycrc.toml"));
        out
    }

    pub fn run(&mut self, terminal: &mut Tui) -> Result<()> {
        use notify::{RecursiveMode, Watcher};
        use std::sync::mpsc;

        // File watcher: notify posts events into `rx`. Two kinds of watch:
        //
        // 1. Config files — we watch their *parent* directories, not the
        //    files, because editors that replace-on-save (vim, VS Code,
        //    nvim) remove the old inode before creating the new one.
        //
        // 2. The current listing directory (non-recursive) — so external
        //    changes (a build artifact dropping in, `git pull`, etc.) are
        //    reflected without a manual refresh.
        let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut fs_watcher: Option<notify::RecommendedWatcher> =
            notify::recommended_watcher(tx).ok();
        let mut already_watched: std::collections::HashSet<PathBuf> =
            std::collections::HashSet::new();
        if let Some(w) = fs_watcher.as_mut() {
            for path in self.candidate_config_paths() {
                if let Some(parent) = path.parent() {
                    if parent.is_dir() && already_watched.insert(parent.to_path_buf()) {
                        let _ = w.watch(parent, RecursiveMode::NonRecursive);
                    }
                }
            }
        }
        // Which listing dir is currently watched. On chdir we'll unwatch
        // this one and re-watch the new dir.
        let mut watched_listing: Option<PathBuf> = None;
        let mut watched_git: Option<PathBuf> = None;
        sync_listing_watch(
            fs_watcher.as_mut(),
            &mut watched_listing,
            &mut watched_git,
            &self.state.listing.dir,
        );

        let mut last_context_write = std::time::Instant::now();
        let mut last_refresh = std::time::Instant::now();
        let mut pending_refresh = false;

        let mut needs_draw = true; // draw at least once on startup
        // 0=none, 1=pane output, 2=input event, 3=other (refresh/config/repaint/activity)
        let mut draw_reason: u8 = 3;

        while !self.state.should_quit {
            // One-shot full repaint after a pane or overlay closes (or any
            // other event that leaves ratatui's diff buffer stale).
            // Also force repaint when the pager opens while a pane exists,
            // because the pane stops rendering and its stale cells need clearing.
            // When the pager opens over a pane, the pane's stale cells
            // need clearing. But don't use terminal.clear() for this — the
            // pager overlay will paint over everything anyway, and the
            // clear causes a visible flash. Just force a draw instead.
            if self.pager.is_some() && self.pane_tabs.is_some() && !self.pager_was_open {
                needs_draw = true;
                draw_reason = 3;
            }
            self.pager_was_open = self.pager.is_some();
            let mut pending_clear = false;
            if self.needs_full_repaint {
                self.needs_full_repaint = false;
                pending_clear = true;
                needs_draw = true;
                draw_reason = 3;
            }
            // NOTE: periodic ^L to Claude pane tabs was removed — it clears
            // any draft prompt the user has typed, even when focus is on the
            // file list (the text is still in Claude's input buffer).

            // pending_overlay_close is no longer used — the overlay stays
            // visible until Enter via overlay_awaiting_dismiss.
            let _ = self.pending_overlay_close;

            // Check if a background `!` capture has finished.
            // Stream captured command output into the pager in real-time.
            if let Some(capture) = &mut self.pending_capture {
                needs_draw = true;
                draw_reason = 3; // capture in progress
                let mut got_data = false;
                while let Ok(chunk) = capture.output_rx.try_recv() {
                    if chunk.is_empty() {
                        // EOF — child is done.
                        capture.finished = true;
                        break;
                    }
                    capture.buffer.extend_from_slice(&chunk);
                    got_data = true;
                }
                // Update elapsed timer in the title while running.
                if !capture.finished {
                    let elapsed = capture.started.elapsed().as_secs();
                    let human_time = if elapsed >= 3600 {
                        format!(
                            "{}h {}m {}s",
                            elapsed / 3600,
                            (elapsed % 3600) / 60,
                            elapsed % 60
                        )
                    } else if elapsed >= 60 {
                        format!("{}m {}s", elapsed / 60, elapsed % 60)
                    } else {
                        format!("{elapsed}s")
                    };
                    let timer_title =
                        format!("\u{23f3} {} — running... ({human_time})", capture.title);
                    if let Some(view) = self.pager.as_mut() {
                        view.title = timer_title;
                    }
                }
                if got_data || capture.finished {
                    // Rebuild pager content from the accumulated buffer.
                    use ansi_to_tui::IntoText;
                    let text = capture.buffer.as_slice().into_text().unwrap_or_default();
                    let at_bottom = self.pager.as_ref().is_some_and(|v| {
                        let total = v.line_count();
                        let page = v.page_lines(40); // approximate
                        v.scroll >= total.saturating_sub(page)
                    });
                    if let Some(view) = self.pager.as_mut() {
                        view.lines = text.lines;
                        if at_bottom {
                            view.scroll_to_bottom(40);
                        }
                    }
                }
                if capture.finished {
                    let status = capture.child.wait();
                    let exit_info = match status {
                        Ok(s) if s.success() => "exit 0".to_string(),
                        Ok(s) => format!("exit {}", s.exit_code()),
                        Err(e) => format!("error: {e}"),
                    };
                    let title = format!("{} — {exit_info}", capture.title);
                    // Final rebuild with stderr included.
                    let text = capture.buffer.as_slice().into_text().unwrap_or_default();
                    if let Some(view) = self.pager.as_mut() {
                        view.title = title;
                        view.lines = text.lines;
                        view.saveable = true;
                        view.streaming = false;
                        view.scroll_to_bottom(40);
                    }
                    self.pending_capture = None;
                }
            }

            // Pre-drain pane output so we know if anything arrived.
            // All tabs are drained (correctness), but only *active* tab
            // output triggers a redraw — background tab trickle bytes
            // (idle shell cursor blinks, prompt redraws) are silent.
            let mut pane_had_output = false;
            if let Some(tabs) = self.pane_tabs.as_mut() {
                let active_idx = tabs.active_index();
                for (i, entry) in tabs.tabs_mut().iter_mut().enumerate() {
                    // Fast path: skip try_recv() when the reader thread
                    // hasn't posted anything. The atomic check is ~1ns vs
                    // ~20ns for a channel operation.
                    if !entry.pane.has_pending_output() {
                        continue;
                    }
                    if entry.pane.drain_output() && i == active_idx {
                        pane_had_output = true;
                    }
                }
            }
            if let Some(overlay) = self.top_overlay.as_mut() {
                if overlay.has_pending_output() && overlay.drain_output() {
                    pane_had_output = true;
                }
            }
            if pane_had_output {
                needs_draw = true;
                draw_reason = 1;
            }

            // Mark exited tabs AFTER drain so the Closed event has been
            // processed and is_closed() returns true. Trigger a redraw
            // so the "[exited N]" label appears immediately.
            if let Some(tabs) = self.pane_tabs.as_mut() {
                if tabs.mark_exited() {
                    needs_draw = true;
                    draw_reason = 3;
                }
            }

            // Drain any pending watcher events. Refresh listing / reload
            // config at most once per poll iteration, and debounce
            // listing refreshes to avoid spawning git subprocesses on
            // every rapid-fire .git/index change.
            let mut needs_reload = false;
            // pending_refresh carries over from previous iterations when
            // the debounce timer hadn't elapsed yet.
            while let Ok(result) = rx.try_recv() {
                if let Ok(ev) = result {
                    for p in &ev.paths {
                        if self.is_config_path(p) {
                            needs_reload = true;
                        }
                        if self.is_listing_path(p) {
                            pending_refresh = true;
                        }
                    }
                }
            }
            if needs_reload {
                self.reload_config();
                needs_draw = true;
                draw_reason = 3;
            }
            if pending_refresh && last_refresh.elapsed() >= Duration::from_millis(500) {
                pending_refresh = false;
                self.state.refresh_listing();
                last_refresh = std::time::Instant::now();
                needs_draw = true;
                draw_reason = 3;
            }

            // Process writable MCP commands from Claude.
            while let Ok(req) = self.mcp_cmd_rx.try_recv() {
                let resp = self.execute_mcp_command(req.command);
                let _ = req.reply.send(resp);
                needs_draw = true;
                draw_reason = 3;
            }

            // Adaptive poll rate:
            // - 16ms when pane output just arrived (smooth streaming)
            // - 100ms when pane exists but is idle
            // - 250ms when no pane at all
            let poll_ms = if pane_had_output || self.pending_capture.is_some() {
                16 // smooth streaming
            } else if self.pane_tabs.is_some() || self.top_overlay.is_some() {
                100 // pane idle — responsive to new output
            } else {
                500 // no pane — only user input matters
            };
            if event::poll(Duration::from_millis(poll_ms))? {
                needs_draw = true;
                draw_reason = 2;
                match event::read()? {
                    Event::Key(key)
                        if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                    {
                        // Throttle rapid-fire arrow keys from trackpad scroll
                        // (DEC 1007 alternate-scroll). Allow ~25 events/sec.
                        if matches!(key.code, KeyCode::Up | KeyCode::Down)
                            && key.modifiers.is_empty()
                        {
                            let now = std::time::Instant::now();
                            if let Some((prev, dir)) = self.scroll_last {
                                if dir == key.code && now.duration_since(prev).as_millis() < 40 {
                                    continue;
                                }
                            }
                            self.scroll_last = Some((now, key.code));
                        } else {
                            self.scroll_last = None;
                        }
                        let post = self.handle_key(key)?;
                        if let PostAction::Spawn {
                            program,
                            args,
                            pause_after,
                        } = post
                        {
                            run_child_in_foreground(terminal, &program, &args, pause_after)?;
                            // The listing may have changed (mv, rm, chmod, etc).
                            self.state.refresh_listing();
                            // If we were editing a pager buffer, restore it.
                            if let Some(ret) = self.pending_pager_return.take() {
                                match ret {
                                    PagerReturn::TempFile {
                                        path,
                                        title,
                                        scroll,
                                    } => {
                                        if let Ok(content) = std::fs::read_to_string(&path) {
                                            let lines: Vec<String> =
                                                content.lines().map(String::from).collect();
                                            let mut view = PagerView::new_plain(title, lines);
                                            view.scroll = scroll;
                                            view.saveable = true;
                                            self.pager = Some(view);
                                        }
                                        let _ = std::fs::remove_file(&path);
                                    }
                                    PagerReturn::SourceFile { path, scroll } => {
                                        let name = path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .into_owned();
                                        if let Ok(content) = std::fs::read_to_string(&path) {
                                            let lines: Vec<String> =
                                                content.lines().map(String::from).collect();
                                            let mut view = PagerView::new_plain(name, lines);
                                            view.source_path = Some(path);
                                            view.scroll = scroll;
                                            self.pager = Some(view);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Event::Paste(text) => {
                        if let Mode::Prompting(ref mut p) = self.state.mode {
                            // Paste into the active prompt buffer.
                            // Strip newlines (prompts are single-line).
                            let clean = text.replace(['\n', '\r'], " ");
                            p.buffer.push_str(&clean);
                            if let Some(ed) = p.editor.as_mut() {
                                ed.set_content(&p.buffer);
                            }
                        } else if self.pane_tabs.is_some() {
                            // Switch focus to the pane — the user clearly
                            // intends to interact with it if they're pasting.
                            if !self.state.pane_focused {
                                self.set_pane_focus(true);
                            }
                            // Track pasted text for yP (yank last prompt).
                            self.pane_prompt_buf.push_str(&text);
                            // Wrap in bracketed paste so the child app (e.g. claude)
                            // receives the block as a single paste, not line-by-line.
                            let pane = self.pane_tabs.as_mut().unwrap().active_mut();
                            let mut buf = Vec::with_capacity(text.len() + 12);
                            buf.extend_from_slice(b"\x1b[200~");
                            buf.extend_from_slice(text.as_bytes());
                            buf.extend_from_slice(b"\x1b[201~");
                            pane.send_bytes(&buf)?;
                        }
                    }
                    Event::Resize(cols, rows) => {
                        // Terminal resized — immediately resize all pty tabs
                        // so the child shells re-render their prompts at the
                        // correct width.
                        let area = ratatui::layout::Rect::new(0, 0, cols, rows);
                        if let Some(tabs) = self.pane_tabs.as_mut() {
                            let layout = Self::compute_layout(
                                area,
                                true,
                                self.state.pane_height_pct,
                                self.state.config.layout.status_position,
                            );
                            if let Some(pane_rect) = layout.pane {
                                for entry in tabs.tabs_mut() {
                                    let _ = entry.pane.resize(pane_rect.height, pane_rect.width);
                                }
                            }
                        }
                        if let Some(overlay) = self.top_overlay.as_mut() {
                            let (r, c) = Self::top_overlay_size(
                                self.state.pane_height_pct,
                                self.pane_tabs.is_some(),
                            );
                            let _ = overlay.resize(r, c);
                        }
                        // Help content is baked at open time for the current
                        // width (wrap points, column count). Rebuild so the
                        // layout matches the new dimensions.
                        if self.help_is_open() {
                            self.open_help();
                        }
                    }
                    _ => {}
                }
            }

            // Activity monitor: roll over the 1-second window.
            let mut activity_only_draw = false;
            if self.show_activity && self.activity_last_tick.elapsed() >= Duration::from_secs(1) {
                let new_dps = self.activity_draws;
                let new_bps = self.activity_bytes;
                let new_sp = self.activity_reason_pane;
                let new_se = self.activity_reason_event;
                let new_so = self.activity_reason_other;
                // Only force a redraw if something changed.
                if (new_dps != self.activity_dps
                    || new_bps != self.activity_bps
                    || new_sp != self.activity_snap_pane
                    || new_se != self.activity_snap_event
                    || new_so != self.activity_snap_other)
                    && !needs_draw
                {
                    // This draw exists only to refresh the overlay —
                    // don't count it in the stats or it oscillates.
                    needs_draw = true;
                    activity_only_draw = true;
                }
                self.activity_dps = new_dps;
                self.activity_bps = new_bps;
                self.activity_snap_pane = new_sp;
                self.activity_snap_event = new_se;
                self.activity_snap_other = new_so;
                self.activity_draws = 0;
                self.activity_bytes = 0;
                self.activity_reason_pane = 0;
                self.activity_reason_event = 0;
                self.activity_reason_other = 0;
                self.activity_last_tick = std::time::Instant::now();
            }

            // Only redraw when something actually changed.
            // Wrap in DEC 2026 synchronized update so the terminal
            // emulator (iTerm2, etc.) buffers the entire frame and
            // paints it atomically — eliminates tearing and reduces
            // terminal-side CPU.
            if needs_draw {
                needs_draw = false;
                use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
                let _ = crossterm::execute!(terminal.backend_mut(), BeginSynchronizedUpdate);
                if pending_clear {
                    terminal.clear()?;
                }
                let frame_area = terminal.draw(|frame| self.render(frame))?.area;
                let _ = crossterm::execute!(terminal.backend_mut(), EndSynchronizedUpdate);
                if self.show_activity && !activity_only_draw {
                    self.activity_draws += 1;
                    self.activity_bytes +=
                        u64::from(frame_area.width) * u64::from(frame_area.height);
                    match draw_reason {
                        1 => self.activity_reason_pane += 1,
                        2 => self.activity_reason_event += 1,
                        _ => self.activity_reason_other += 1,
                    }
                }
                draw_reason = 0;
            }

            // Only re-sync the filesystem watcher when the cwd actually changed.
            if watched_listing.as_deref() != Some(self.state.listing.dir.as_path()) {
                sync_listing_watch(
                    fs_watcher.as_mut(),
                    &mut watched_listing,
                    &mut watched_git,
                    &self.state.listing.dir,
                );
            }

            // Update the MCP context file — throttled to at most once per
            // second to avoid hammering the disk on every 16ms frame.
            if last_context_write.elapsed() >= Duration::from_secs(1) {
                self.write_context();
                last_context_write = std::time::Instant::now();
            }
        }
        // Clean up the context file on exit.
        crate::context::remove_context_file(&self.context_path);
        Ok(())
    }

    fn is_config_path(&self, path: &Path) -> bool {
        self.candidate_config_paths().iter().any(|c| c == path)
            || self.state.config.sources.iter().any(|c| c == path)
    }

    /// True iff `path` is the listing directory or a direct child of it.
    /// `notify` events sometimes include just the directory and sometimes
    /// the affected child, so we accept both.
    fn is_listing_path(&self, path: &Path) -> bool {
        // Ignore our own context file writes — they land in the listing
        // directory and would otherwise trigger a self-perpetuating
        // refresh_listing → git subprocess → redraw cycle.
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with(".spyc-context-") {
                return false;
            }
        }
        let dir = self.state.listing.dir.as_path();
        path == dir
            || path.parent() == Some(dir)
            // .git/index changes (git add, commit, checkout, etc.)
            // trigger a refresh so git status markers stay current.
            || path == dir.join(".git/index")
    }

    // --- Rendering --------------------------------------------------------

    /// Partition the frame into status/list/prompt rects — plus, when
    /// the pane is open, a divider row and the pane rect below it.
    ///
    /// The **entire spyc unit** (status, list, prompt) lives above the
    /// divider. That way the prompt row sits with the file list it's
    /// about rather than attached to the bottom of the screen where the
    /// pane's subprocess is typing.
    fn compute_layout(
        area: ratatui::layout::Rect,
        pane_open: bool,
        pane_pct: u16,
        status_position: StatusPosition,
    ) -> FrameLayout {
        use ratatui::layout::Rect;
        let w = area.width;
        let h = area.height;
        let bottom_status = matches!(status_position, StatusPosition::Bottom);

        if !pane_open {
            // Top:    [status][list…][prompt]
            // Bottom: [list…][prompt][status]   (vim-style)
            let (status_y, list_y, prompt_y) = if bottom_status {
                (
                    area.y + h.saturating_sub(1),
                    area.y,
                    area.y + h.saturating_sub(2),
                )
            } else {
                (area.y, area.y + 1.min(h), area.y + h.saturating_sub(1))
            };
            let status = Rect {
                x: area.x,
                y: status_y,
                width: w,
                height: 1.min(h),
            };
            let list = Rect {
                x: area.x,
                y: list_y,
                width: w,
                height: h.saturating_sub(2),
            };
            let prompt = Rect {
                x: area.x,
                y: prompt_y,
                width: w,
                height: u16::from(h != 0),
            };
            return FrameLayout {
                status,
                list,
                divider: None,
                pane: None,
                prompt,
            };
        }

        // With pane: top unit holds list+prompt(+status if top).
        // Pane and divider sit below; if status is bottom, status is the
        // very last row, prompt one above, pane above that.
        let usable = h.saturating_sub(1); // minus divider
        let pane_h = (u32::from(usable) * u32::from(pane_pct) / 100) as u16;
        let top_h = usable.saturating_sub(pane_h);

        if bottom_status {
            // Layout (top → bottom): [list…][divider][pane…][prompt][status]
            // Reserve: 1 divider + 1 prompt + 1 status = 3 rows of chrome.
            // The remainder splits between list and pane by `pane_pct`.
            let chrome = 3u16;
            let usable_b = h.saturating_sub(chrome);
            let pane_h_b = (u32::from(usable_b) * u32::from(pane_pct) / 100) as u16;
            let list_h = usable_b.saturating_sub(pane_h_b);

            let list = Rect {
                x: area.x,
                y: area.y,
                width: w,
                height: list_h,
            };
            let divider = Rect {
                x: area.x,
                y: area.y + list_h,
                width: w,
                height: 1,
            };
            let pane = Rect {
                x: area.x,
                y: divider.y + 1,
                width: w,
                height: pane_h_b,
            };
            let prompt = Rect {
                x: area.x,
                y: area.y + h.saturating_sub(2),
                width: w,
                height: u16::from(h >= 2),
            };
            let status = Rect {
                x: area.x,
                y: area.y + h.saturating_sub(1),
                width: w,
                height: 1.min(h),
            };
            return FrameLayout {
                status,
                list,
                divider: Some(divider),
                pane: Some(pane),
                prompt,
            };
        }

        // Top status (default): [status][list…][prompt][divider][pane]
        let status = Rect {
            x: area.x,
            y: area.y,
            width: w,
            height: 1.min(top_h),
        };
        let list_h = top_h.saturating_sub(2);
        let list = Rect {
            x: area.x,
            y: area.y + status.height,
            width: w,
            height: list_h,
        };
        let prompt = Rect {
            x: area.x,
            y: area.y + top_h.saturating_sub(1),
            width: w,
            height: u16::from(top_h >= 2),
        };

        let divider = Rect {
            x: area.x,
            y: area.y + top_h,
            width: w,
            height: 1,
        };
        let pane = Rect {
            x: area.x,
            y: divider.y + 1,
            width: w,
            height: pane_h,
        };

        FrameLayout {
            status,
            list,
            divider: Some(divider),
            pane: Some(pane),
            prompt,
        }
    }

    /// Pane status line: tab indicators, active cwd, [SCROLL] tag.
    /// Replaces the old plain-rule divider.
    fn render_pane_status_line(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        use ratatui::{
            style::{Modifier, Style},
            text::{Line, Span},
            widgets::Paragraph,
        };
        let width = area.width as usize;
        let rule_style = if self.state.pane_focused {
            Style::default()
                .fg(self.theme.prompt_prefix)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.theme.status_suffix)
        };
        let active_tab_style = Style::default()
            .fg(self.theme.prompt_prefix)
            .add_modifier(Modifier::BOLD);
        let inactive_tab_style = Style::default().fg(self.theme.status_suffix);

        let mut spans: Vec<Span> = Vec::new();
        let mut used = 0usize;

        let activity_style = Style::default()
            .fg(self.theme.pick)
            .add_modifier(Modifier::BOLD);

        // Tab indicators: ─[1*] claude ─[2+] bash, then "── <live cwd>".
        // We render the indicators first (immutable iter) and capture
        // the active index, then re-borrow mut to fetch the live cwd.
        let mut active_idx: Option<usize> = None;
        if let Some(tabs) = &self.pane_tabs {
            for (i, entry) in tabs.tabs().iter().enumerate() {
                let is_active = i == tabs.active_index();
                if is_active {
                    active_idx = Some(i);
                }
                let star = if is_active { "*" } else { "" };
                let activity = if entry.info.has_activity { "+" } else { "" };
                let sep = "─";
                let tab_text = format!("[{}{star}{activity}] {} ", i + 1, entry.info.label);
                let tab_len = sep.len() + tab_text.len();
                if used + tab_len > width {
                    break;
                }
                spans.push(Span::styled(sep, rule_style));
                let style = if is_active {
                    active_tab_style
                } else if entry.info.has_activity {
                    activity_style
                } else {
                    inactive_tab_style
                };
                spans.push(Span::styled(tab_text, style));
                used += tab_len;
            }
        }

        if let (Some(idx), Some(tabs)) = (active_idx, self.pane_tabs.as_mut()) {
            let entry = &mut tabs.tabs_mut()[idx];
            let live = entry.live_cwd().to_path_buf();
            let cwd_display = crate::paths::display_tilde(&live);
            // Mark when the live cwd has drifted from the spawn cwd
            // (e.g. user `cd`'d in a bash tab). Helps spot the case
            // the bug list called out.
            let drift = live != entry.info.cwd;
            let cwd_prefix = if drift { "── ↪ " } else { "── " };
            let avail = width.saturating_sub(used + 12); // room for [SCROLL] + trailing rule
            if avail > 4 {
                let truncated = if cwd_display.len() > avail {
                    format!("…{}", &cwd_display[cwd_display.len() - avail + 1..])
                } else {
                    cwd_display
                };
                let cwd_fragment = format!("{cwd_prefix}{truncated} ");
                used += cwd_fragment.len();
                let style = if drift {
                    active_tab_style
                } else {
                    inactive_tab_style
                };
                spans.push(Span::styled(cwd_fragment, style));
            }
        }

        // Right-aligned [SCROLL] tag.
        let is_scrolling = self
            .pane_tabs
            .as_ref()
            .is_some_and(|t| t.active().is_scrolling());
        let tag = if is_scrolling { " [SCROLL]" } else { "" };
        let fill = width.saturating_sub(used + tag.len());
        if fill > 0 {
            spans.push(Span::styled("─".repeat(fill), rule_style));
            used += fill;
        }
        if is_scrolling {
            spans.push(Span::styled(
                tag,
                Style::default()
                    .fg(self.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ));
            used += tag.len();
        }
        // If anything's left (shouldn't be), pad.
        let _ = used;

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render(&mut self, frame: &mut Frame) {
        let frame_area = frame.area();

        // Layout:
        //   - No pane: status (top row), list (middle), prompt (bottom row).
        //   - With pane: status (top row of the top *pane*), list (rest of
        //     top pane), divider row, pane, prompt (bottom row).
        //   The status row is always at the top of the file-list region —
        //   so when the pane is open it sits *inside* the top pane rather
        //   than above the divider.
        let layout = Self::compute_layout(
            frame_area,
            self.pane_tabs.is_some(),
            self.state.pane_height_pct,
            self.state.config.layout.status_position,
        );

        // If a top-overlay pty is active (`;top`, `;vim`, etc.), it
        // replaces the entire spyc area. Status, list, and prompt are
        // hidden; only the overlay + divider + bottom pane render.
        if let Some(overlay) = self.top_overlay.as_mut() {
            // The overlay occupies status + list + prompt area.
            let overlay_area = ratatui::layout::Rect {
                x: layout.status.x,
                y: layout.status.y,
                width: layout.status.width,
                height: layout.status.height + layout.list.height + layout.prompt.height,
            };
            let _ = overlay.resize(overlay_area.height, overlay_area.width);
            overlay.drain_output();
            if overlay.is_closed() && !self.overlay_awaiting_dismiss {
                self.overlay_awaiting_dismiss = true;
            }
            frame.render_widget(
                PaneWidget {
                    screen: overlay.screen(),
                    focused: true,
                },
                overlay_area,
            );
            // Show a dismiss prompt when the subprocess has exited.
            if self.overlay_awaiting_dismiss && overlay_area.height > 0 {
                use ratatui::{
                    style::{Modifier, Style},
                    text::{Line, Span},
                    widgets::Paragraph,
                };
                let prompt_y = overlay_area.y + overlay_area.height.saturating_sub(1);
                let prompt_rect = ratatui::layout::Rect {
                    x: overlay_area.x,
                    y: prompt_y,
                    width: overlay_area.width,
                    height: 1,
                };
                let style = Style::default()
                    .fg(self.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD);
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "[process exited — press any key to continue]",
                        style,
                    ))),
                    prompt_rect,
                );
            }

            // Divider + bottom pane still render normally.
            if let Some(divider_rect) = layout.divider {
                self.render_pane_status_line(frame, divider_rect);
            }
            if let (Some(tabs), Some(rect)) = (self.pane_tabs.as_mut(), layout.pane) {
                let _ = tabs.active_mut().resize(rect.height, rect.width);
                tabs.drain_all();
                frame.render_widget(
                    PaneWidget {
                        screen: tabs.active().screen(),
                        focused: false,
                    },
                    rect,
                );
            }
            return;
        }

        let (path, suffix) = self.header_parts();
        let project_label = self
            .state
            .project_home
            .as_deref()
            .map(path_basename_display);
        StatusBar {
            project_home: project_label.as_deref(),
            session_name: self.state.session_name.as_deref(),
            path: &path,
            suffix: &suffix,
            git_info: self.state.git_info.as_deref(),
            theme: &self.theme,
        }
        .render(frame, layout.status);

        if self.cached_rows_gen != self.state.list_generation {
            self.cached_rows = self.build_rows();
            self.cached_rows_gen = self.state.list_generation;
        }
        let rows = &self.cached_rows;
        let list_focused = !self.state.pane_focused;
        // Stabilize view_top ↔ grid.  Skip the expensive multi-round
        // loop when inputs haven't changed since the last frame.
        let grid_key = (
            self.state.list_generation,
            self.state.cursor.view_top,
            self.state.cursor.index,
            layout.list.width,
            layout.list.height,
        );
        if grid_key != self.cached_grid_key {
            self.cached_grid_key = grid_key;
            // The grid depends on view_top (different entries have different
            // name lengths → different column count → different items_per_page),
            // and view_top depends on the grid.
            //
            // This can produce a 2-cycle: vt=A gives grid that wants vt=B, and
            // vt=B gives grid that wants vt=A.  When we detect that, always pick
            // the lower of the two (shows more context, deterministic across
            // frames) and recompute the grid for that choice.
            {
                let mut prev_vt: Option<usize> = None; // for 2-cycle detection
                let mut settled = false;
                for round in 0..4 {
                    let probe = ListView {
                        rows,
                        cursor: self.state.cursor.index,
                        view_top: self.state.cursor.view_top,
                        empty_marker: self.state.view == View::Dir,
                        focused: list_focused,
                        theme: &self.theme,
                    };
                    self.state.last_grid = probe.grid(layout.list);
                    let old_vt = self.state.cursor.view_top;
                    let pp = self.state.last_grid.items_per_page();
                    self.state.ensure_cursor_visible();
                    if self.state.cursor.view_top == old_vt {
                        spyc_debug!(
                            "grid settled round {}: vt={} cursor={} grid={}x{} pp={}",
                            round + 1,
                            old_vt,
                            self.state.cursor.index,
                            self.state.last_grid.cols,
                            self.state.last_grid.rows,
                            pp,
                        );
                        settled = true;
                        break;
                    }
                    spyc_debug!(
                        "grid unstable round {}: vt {} -> {} cursor={} grid={}x{} pp={}",
                        round + 1,
                        old_vt,
                        self.state.cursor.view_top,
                        self.state.cursor.index,
                        self.state.last_grid.cols,
                        self.state.last_grid.rows,
                        pp,
                    );
                    // 2-cycle: new vt equals the vt from two rounds ago.
                    if Some(self.state.cursor.view_top) == prev_vt {
                        // Always pick the lower vt — deterministic across frames.
                        let forced = old_vt.min(self.state.cursor.view_top);
                        self.state.cursor.view_top = forced;
                        // Recompute grid for the forced view_top.
                        let probe = ListView {
                            rows,
                            cursor: self.state.cursor.index,
                            view_top: self.state.cursor.view_top,
                            empty_marker: self.state.view == View::Dir,
                            focused: list_focused,
                            theme: &self.theme,
                        };
                        self.state.last_grid = probe.grid(layout.list);
                        spyc_debug!(
                            "grid 2-cycle broken: forcing vt={} (cursor={} grid={}x{} pp={})",
                            forced,
                            self.state.cursor.index,
                            self.state.last_grid.cols,
                            self.state.last_grid.rows,
                            self.state.last_grid.items_per_page(),
                        );
                        settled = true;
                        break;
                    }
                    prev_vt = Some(old_vt);
                }
                if !settled {
                    spyc_debug!(
                        "grid did NOT settle after 4 rounds: vt={} cursor={}",
                        self.state.cursor.view_top,
                        self.state.cursor.index,
                    );
                }
            }
            // Update cache key in case the stabilization loop changed view_top.
            self.cached_grid_key = (
                self.state.list_generation,
                self.state.cursor.view_top,
                self.state.cursor.index,
                layout.list.width,
                layout.list.height,
            );
        } // end grid cache guard

        frame.render_widget(
            ListView {
                rows,
                cursor: self.state.cursor.index,
                view_top: self.state.cursor.view_top,
                empty_marker: self.state.view == View::Dir,
                focused: list_focused,
                theme: &self.theme,
            },
            layout.list,
        );

        if let (Some(tabs), Some(rect)) = (self.pane_tabs.as_mut(), layout.pane) {
            let _ = tabs.active_mut().resize(rect.height, rect.width);
            tabs.drain_all();
            frame.render_widget(
                PaneWidget {
                    screen: tabs.active().screen(),
                    focused: self.state.pane_focused,
                },
                rect,
            );
            tabs.active_mut().output_dirty = false;
        }

        if let Some(divider_rect) = layout.divider {
            self.render_pane_status_line(frame, divider_rect);
        }

        if let Mode::Prompting(p) = &self.state.mode {
            PromptLine {
                prefix: &p.prefix,
                buffer: &p.buffer,
                theme: &self.theme,
                cursor_pos: p.editor.as_ref().map(|e| e.cursor),
                vi_mode: p.editor.as_ref().map(|e| e.mode),
            }
            .render(frame, layout.prompt);
        } else if let Some(flash) = &self.state.flash {
            use ratatui::{
                style::{Modifier, Style},
                text::{Line, Span},
                widgets::Paragraph,
            };
            let color = match flash.kind {
                FlashKind::Info => self.theme.take,
                FlashKind::Error => self.theme.cursor_bg,
            };
            let line = Line::from(Span::styled(
                flash.text.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            frame.render_widget(Paragraph::new(line), layout.prompt);
        } else if let Some(capture) = &self.pending_capture {
            // Persistent "running" indicator while a `!` capture is active.
            use ratatui::{
                style::{Modifier, Style},
                text::{Line, Span},
                widgets::Paragraph,
            };
            let line = Line::from(Span::styled(
                format!(
                    "⏳ running: {}  (keys → child, ^C interrupt, ^\\ kill)",
                    capture.cmd_display
                ),
                Style::default()
                    .fg(self.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ));
            frame.render_widget(Paragraph::new(line), layout.prompt);
        } else if let Some(pending) = self.state.resolver.pending_display() {
            use ratatui::{
                style::{Modifier, Style},
                text::{Line, Span},
                widgets::Paragraph,
            };
            let line = Line::from(Span::styled(
                pending,
                Style::default()
                    .fg(self.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ));
            frame.render_widget(Paragraph::new(line), layout.prompt);
        }

        // Pager comes after list but before help (help always wins).
        if let Some(view) = &self.pager {
            pager::render(frame, frame.area(), view, &self.theme);
        }

        // Activity monitor overlay (top-right corner).
        if self.show_activity {
            use ratatui::widgets::Paragraph as ActivityP;
            let text = format!(
                " {} dps [p:{} e:{} o:{}]  {} cells/s  poll {}ms ",
                self.activity_dps,
                self.activity_snap_pane,
                self.activity_snap_event,
                self.activity_snap_other,
                self.activity_bps,
                if self.pending_capture.is_some() {
                    16
                } else if self.pane_tabs.is_some() || self.top_overlay.is_some() {
                    50
                } else {
                    250
                },
            );
            let w = text.len() as u16;
            if frame_area.width > w + 2 {
                let rect = ratatui::layout::Rect {
                    x: frame_area.width - w - 1,
                    y: 0,
                    width: w,
                    height: 1,
                };
                let style = ratatui::style::Style::default()
                    .fg(ratatui::style::Color::Black)
                    .bg(ratatui::style::Color::Yellow);
                frame.render_widget(
                    ActivityP::new(ratatui::text::Line::from(ratatui::text::Span::styled(
                        text, style,
                    ))),
                    rect,
                );
            }
        }
    }

    fn header_parts(&self) -> (String, String) {
        match self.state.view {
            View::Dir => (crate::paths::display_tilde(&self.state.listing.dir), {
                let filter_tag = match &self.state.temp_filter {
                    Some(f) if f == "!" => " limit:picks".to_string(),
                    Some(f) => format!(" limit:{f}"),
                    None => String::new(),
                };
                {
                    let total = self.state.listing.entries.len();
                    let shown = self.state.rows.len();
                    let hidden = total.saturating_sub(shown);
                    let hidden_tag = format!(" hidden:{hidden}");
                    format!(
                        "[picks:{} inv:{} m1:{} m2:{}{}{}]",
                        self.state.picks.len(),
                        self.state.inventory.len(),
                        on_off(self.state.masks.mask1.enabled),
                        on_off(self.state.masks.mask2.enabled),
                        filter_tag,
                        hidden_tag,
                    )
                }
            }),
            View::Inventory => (
                "<INVENTORY>".to_string(),
                format!(
                    "[{} items{}]  (t: tag, p: put, x: remove, ESC: return)",
                    self.state.inventory.len(),
                    if self.state.inventory.picks.is_empty() {
                        String::new()
                    } else {
                        format!(", {} tagged", self.state.inventory.picks.len())
                    }
                ),
            ),
        }
    }

    fn build_rows(&self) -> Vec<Row> {
        use crate::ui::list_view::GitFileStatus;
        self.state
            .rows
            .iter()
            .map(|rd| {
                let git_status = self
                    .state
                    .git_files
                    .get(&rd.display)
                    .copied()
                    .unwrap_or(GitFileStatus::Clean);
                Row {
                    display: rd.display.clone(),
                    kind: rd.kind,
                    picked: self.state.view == View::Dir && self.state.picks.contains(&rd.path),
                    taken: self.state.inventory.contains(&rd.path),
                    git_status,
                }
            })
            .collect()
    }

    // --- Input handling ---------------------------------------------------

    fn handle_key(&mut self, key: KeyEvent) -> Result<PostAction> {
        // Any keypress clears a lingering flash message.
        self.state.flash = None;

        // While a `!` capture is running, forward typed keys to the
        // child via the master PTY writer so the user can answer
        // prompts (sudo password, ssh password, etc.). Ctrl+\ kills
        // the child outright; Ctrl+C is forwarded as 0x03 so the
        // child's tty driver can deliver SIGINT (matches a normal
        // terminal's behavior, and lets sudo cancel its prompt
        // cleanly).
        if let Some(capture) = &mut self.pending_capture {
            use std::io::Write as _;
            // Hard-kill escape: Ctrl+\ tears the child down even if
            // it has somehow detached from the controlling tty.
            if matches!(key.code, KeyCode::Char('\\'))
                && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                let _ = capture.child.kill();
                let _ = capture.child.wait();
                let title = format!("{} — interrupted", capture.title);
                if let Some(view) = self.pager.as_mut() {
                    view.title = title;
                    view.saveable = true;
                    view.streaming = false;
                }
                self.pending_capture = None;
                return Ok(PostAction::None);
            }
            let bytes = crate::pane::input::encode_key(key);
            if !bytes.is_empty() {
                let _ = capture.writer.write_all(&bytes);
                let _ = capture.writer.flush();
            }
            return Ok(PostAction::None);
        }

        // Top overlay: once the subprocess exits, hold the screen until
        // any key so short-lived commands (`;ls`) don't flash and vanish.
        if self.overlay_awaiting_dismiss {
            self.top_overlay = None;
            self.overlay_awaiting_dismiss = false;
            self.needs_full_repaint = true;
            self.state.flash_info("command finished");
            return Ok(PostAction::None);
        }

        // Top overlay (interactive `;` command) owns all keys — it's a
        // full takeover of the top area. The user exits by quitting the
        // subprocess itself (q in top, :q in vim, etc.).
        if let Some(overlay) = self.top_overlay.as_mut() {
            let _ = overlay.send_key(key);
            return Ok(PostAction::None);
        }

        // Pager eats all keys until dismissed.
        if self.pager.is_some() {
            let post = self.handle_pager_key(key);
            return Ok(post);
        }
        // When the pane is in scroll mode, navigation keys are handled
        // here instead of being forwarded to the child subprocess.
        // Let spyc meta keys (^W prefix, ^\\, F10) fall through so
        // pane commands still work from scroll mode.
        if let Some(tabs) = self.pane_tabs.as_mut() {
            let pane = tabs.active_mut();
            if pane.is_scrolling()
                && self.state.pane_focused
                && !is_spyc_meta_when_pane_focused(key, self.state.resolver.is_pending())
            {
                return Ok(self.handle_pane_scroll_key(key));
            }
        }
        // When the active tab's subprocess has exited, any key closes
        // the tab (so the user can read the error output, then dismiss).
        if let Some(tabs) = self.pane_tabs.as_mut() {
            if self.state.pane_focused && tabs.active().is_closed() {
                if !tabs.close_active() {
                    self.pane_tabs = None;
                    self.state.pane_focused = false;
                    self.needs_full_repaint = true;
                    self.state.flash_info("pane: last tab closed");
                }
                return Ok(PostAction::None);
            }
        }
        // When the pane is open *and focused*, forward keys to the
        // subprocess — except spyc meta keys, which are always caught
        // by spyc so the user can toggle / resize / focus-switch / send
        // selection from inside the pane.
        if self.pane_tabs.is_some()
            && self.state.pane_focused
            && !matches!(self.state.mode, Mode::Prompting(_))
            && !is_spyc_meta_when_pane_focused(key, self.state.resolver.is_pending())
        {
            // Track what the user types so yP can yank the last prompt.
            match key.code {
                KeyCode::Enter => {
                    let trimmed = strip_ansi_escapes(&self.pane_prompt_buf);
                    if !trimmed.is_empty() {
                        self.last_pane_prompt = Some(trimmed);
                    }
                    self.pane_prompt_buf.clear();
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.pane_prompt_buf.clear();
                }
                KeyCode::Backspace => {
                    self.pane_prompt_buf.pop();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.pane_prompt_buf.push(c);
                }
                _ => {}
            }
            if let Some(tabs) = self.pane_tabs.as_mut() {
                let _ = tabs.active_mut().send_key(key);
            }
            return Ok(PostAction::None);
        }
        if matches!(self.state.mode, Mode::Prompting(_)) {
            return Ok(self.handle_prompt_key(key));
        }
        // Inventory view: special key handling.
        if self.state.view == View::Inventory {
            match key.code {
                KeyCode::Esc => {
                    self.state.toggle_inventory_view();
                    return Ok(PostAction::None);
                }
                KeyCode::Char('x' | 'd') => {
                    self.state.drop_cursor();
                    return Ok(PostAction::None);
                }
                KeyCode::Char(' ' | 't') => {
                    self.state.inventory.toggle_pick(self.state.cursor.index);
                    self.state.list_generation = self.state.list_generation.wrapping_add(1);
                    self.state.cursor_move_vertical(1, self.state.rows.len());
                    return Ok(PostAction::None);
                }
                KeyCode::Char('p') => {
                    return Ok(self.put_inventory_to_cwd());
                }
                _ => {}
            }
        }
        match self.state.resolver.feed(key, &self.state.user_keymap) {
            ResolverOutcome::Action(action) => return self.apply(&action),
            ResolverOutcome::User(bound) => return self.apply_user(&bound),
            ResolverOutcome::Pending | ResolverOutcome::Ignored => {}
        }
        Ok(PostAction::None)
    }

    /// Dispatch a user-defined binding. Inline-data actions (unix command,
    /// preset pattern, preset path) run through the same machinery as the
    /// built-in prompts but skip the prompt UI.
    fn apply_user(&mut self, bound: &BoundAction) -> Result<PostAction> {
        match bound {
            BoundAction::Plain(action) => return self.apply(action),
            BoundAction::UnixCmd(template) => {
                let cmd = shell::expand_percent(template, &self.state.selection_paths());
                return Ok(sh_c(&cmd, true));
            }
            BoundAction::PatternPick(pattern) => {
                if let Ok(pat) = glob::Pattern::new(pattern) {
                    for e in &self.state.listing.entries {
                        if pat.matches(&e.name) {
                            self.state.picks.insert(&e.path);
                        }
                    }
                    self.state.list_generation = self.state.list_generation.wrapping_add(1);
                }
            }
            BoundAction::Jump(path) => {
                let _ = self.state.jump_to(path);
            }
            BoundAction::Copy(dest) => {
                self.run_selection_to(dest, fs::ops::copy_selection_to, "copied");
            }
            BoundAction::Move(dest) => {
                self.run_selection_to(dest, fs::ops::move_selection_to, "moved");
            }
            BoundAction::ToggleMaskFixed(n) => {
                if *n == 1 {
                    self.state.masks.toggle_mask1();
                } else if *n == 2 {
                    self.state.masks.toggle_mask2();
                }
                self.state.rebuild_rows();
            }
        }
        self.state.cursor.clamp(self.state.rows.len());
        Ok(PostAction::None)
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) -> PostAction {
        // Single-key confirm prompts: `y` / `Y` proceeds, anything else cancels.
        if matches!(
            &self.state.mode,
            Mode::Prompting(p) if matches!(p.kind, PromptKind::RemoveConfirm)
        ) {
            return self.handle_remove_confirm_key(key);
        }
        // Shell prompts (`!` / `;`) use the vi line editor + history.
        let has_editor = matches!(
            &self.state.mode,
            Mode::Prompting(p) if p.editor.is_some()
        );
        if has_editor {
            return self.handle_vi_prompt_key(key);
        }

        // --- Simple prompts (search, jump, pattern-pick, etc.) ---

        // Esc cancels; Backspace on an empty buffer cancels too.
        let backspace_on_empty = matches!(key.code, KeyCode::Backspace)
            && matches!(&self.state.mode, Mode::Prompting(p) if p.buffer.is_empty());
        if matches!(key.code, KeyCode::Esc) || backspace_on_empty {
            self.cancel_prompt();
            return PostAction::None;
        }
        if matches!(key.code, KeyCode::Enter) {
            let Mode::Prompting(p) = std::mem::replace(&mut self.state.mode, Mode::Normal) else {
                return PostAction::None;
            };
            return self.dispatch_prompt(p);
        }

        // Tab completion.
        if matches!(key.code, KeyCode::Tab | KeyCode::Char('\t')) {
            // Extract kind and buffer before taking &mut self.
            let (_kind, buffer) = if let Mode::Prompting(p) = &self.state.mode {
                (std::mem::discriminant(&p.kind), p.buffer.clone())
            } else {
                return PostAction::None;
            };
            let is_search = matches!(
                &self.state.mode,
                Mode::Prompting(p) if matches!(p.kind, PromptKind::Search { .. })
            );
            if is_search {
                if !buffer.is_empty() {
                    self.state.temp_filter = Some(format!("{buffer}*"));
                    self.state.rebuild_rows();
                }
            } else if matches!(
                &self.state.mode,
                Mode::Prompting(p) if matches!(
                    p.kind,
                    PromptKind::Jump
                        | PromptKind::CopyTo
                        | PromptKind::MoveTo
                        | PromptKind::MakeDir
                        | PromptKind::NewFile
                        | PromptKind::PaneNewTabCwd
                )
            ) {
                self.tab_complete_path();
            }
            return PostAction::None;
        }
        self.tab_state = None;

        // Edit the buffer. Scoped borrow so we can run search afterwards.
        {
            let Mode::Prompting(prompt) = &mut self.state.mode else {
                return PostAction::None;
            };
            match key.code {
                KeyCode::Backspace => {
                    prompt.buffer.pop();
                }
                KeyCode::Char(c) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match c {
                            'u' | 'U' => prompt.buffer.clear(),
                            'w' | 'W' => {
                                while matches!(prompt.buffer.chars().last(), Some(c) if c.is_whitespace())
                                {
                                    prompt.buffer.pop();
                                }
                                while matches!(prompt.buffer.chars().last(), Some(c) if !c.is_whitespace())
                                {
                                    prompt.buffer.pop();
                                }
                            }
                            _ => {}
                        }
                    } else {
                        prompt.buffer.push(c);
                    }
                }
                _ => {}
            }
        }

        // For an active search, re-run the match incrementally against the
        // original cursor position so typing narrows towards a result but
        // backspace widens again.
        let search_info = if let Mode::Prompting(Prompt {
            kind: PromptKind::Search { saved_cursor },
            buffer,
            ..
        }) = &self.state.mode
        {
            Some((*saved_cursor, buffer.clone()))
        } else {
            None
        };
        if let Some((saved, query)) = search_info {
            if query.is_empty() {
                self.state.cursor.index = saved;
            } else if let Some(i) = self.state.find_match(&query, saved, false) {
                self.state.cursor.index = i;
            }
            self.state.cursor.clamp(self.state.rows.len());
        }

        PostAction::None
    }

    /// Single-key confirmation for `R`. `y` / `Y` triggers the delete;
    /// anything else — including Enter, Esc, or any other letter — cancels.
    /// The prompt closes in every case.
    fn handle_remove_confirm_key(&mut self, key: KeyEvent) -> PostAction {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        self.state.mode = Mode::Normal;
        if !confirmed {
            return PostAction::None;
        }
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            return PostAction::None;
        }
        let count = paths.len();
        self.run_and_flash(
            fs::ops::remove_all(&paths),
            format!("removed {count} item(s)"),
        );
        self.state.picks.clear();
        self.state.refresh_listing();
        PostAction::None
    }

    /// Return the appropriate history for the current prompt kind.
    const fn history_for_prompt(&mut self) -> &mut History {
        let is_pane = matches!(
            self.state.mode,
            Mode::Prompting(Prompt {
                kind: PromptKind::PaneNewTabCmd | PromptKind::PaneNewTabCwd,
                ..
            })
        );
        if is_pane {
            &mut self.state.pane_history
        } else {
            &mut self.state.history
        }
    }

    /// Handle keys for shell prompts that use the vi line editor.
    fn handle_vi_prompt_key(&mut self, key: KeyEvent) -> PostAction {
        use crate::ui::line_edit::EditResult;

        // Tab completion — intercept before feeding to the editor so we
        // don't depend on the editor's Tab handling (which varies by
        // terminal key delivery).
        if matches!(key.code, KeyCode::Tab | KeyCode::Char('\t')) {
            let wants_path = matches!(
                &self.state.mode,
                Mode::Prompting(p) if matches!(
                    p.kind,
                    PromptKind::Jump
                        | PromptKind::CopyTo
                        | PromptKind::MoveTo
                        | PromptKind::MakeDir
                        | PromptKind::NewFile
                        | PromptKind::PaneNewTabCwd
                        | PromptKind::ShellCmd
                        | PromptKind::ShellCmdCaptured
                        | PromptKind::Command
                )
            );
            if wants_path {
                self.tab_complete_path();
            }
            return PostAction::None;
        }
        // Non-Tab clears double-Tab state.
        self.tab_state = None;

        // `!?` — when the buffer is empty and the user types '?',
        // immediately open the history editor (no Enter needed).
        if key.code == KeyCode::Char('?') {
            if let Mode::Prompting(Prompt {
                kind: PromptKind::ShellCmdCaptured,
                ref buffer,
                ..
            }) = self.state.mode
            {
                if buffer.is_empty() {
                    self.state.mode = Mode::Normal;
                    self.show_history_popup();
                    return PostAction::None;
                }
            }
        }

        // Feed key to the editor.
        let result = {
            let Mode::Prompting(prompt) = &mut self.state.mode else {
                return PostAction::None;
            };
            let editor = prompt.editor.as_mut().expect("checked above");
            let r = editor.feed(key);
            // Sync the buffer for display (prompt.buffer drives rendering).
            prompt.buffer = editor.text();
            r
        };

        match result {
            EditResult::Submit => {
                let is_pane_prompt = matches!(
                    self.state.mode,
                    Mode::Prompting(Prompt {
                        kind: PromptKind::PaneNewTabCmd | PromptKind::PaneNewTabCwd,
                        ..
                    })
                );
                let Mode::Prompting(p) = std::mem::replace(&mut self.state.mode, Mode::Normal)
                else {
                    return PostAction::None;
                };
                // Push to the appropriate history before dispatching.
                let hist = if is_pane_prompt {
                    &mut self.state.pane_history
                } else {
                    &mut self.state.history
                };
                if !p.buffer.trim().is_empty() {
                    hist.push(p.buffer.trim());
                }
                hist.reset_nav();
                return self.dispatch_prompt(p);
            }
            EditResult::Cancel => {
                self.history_for_prompt().reset_nav();
                self.cancel_prompt();
            }
            EditResult::HistoryPrev => {
                let current_text = {
                    let Mode::Prompting(p) = &self.state.mode else {
                        return PostAction::None;
                    };
                    p.buffer.clone()
                };
                let hist = self.history_for_prompt();
                if let Some(entry) = hist.prev(&current_text) {
                    let entry = entry.to_string();
                    let Mode::Prompting(p) = &mut self.state.mode else {
                        return PostAction::None;
                    };
                    if let Some(ed) = p.editor.as_mut() {
                        ed.set_content_keep_mode(&entry);
                    }
                    p.buffer = entry;
                }
            }
            EditResult::HistoryNext => {
                let hist = self.history_for_prompt();
                let replacement = match hist.next() {
                    Some(entry) => entry.to_string(),
                    None => hist.stashed().to_string(),
                };
                let Mode::Prompting(p) = &mut self.state.mode else {
                    return PostAction::None;
                };
                if let Some(ed) = p.editor.as_mut() {
                    ed.set_content_keep_mode(&replacement);
                }
                p.buffer = replacement;
            }
            // Tab is intercepted before editor.feed() — this arm is
            // only reachable if the editor somehow returns it.
            EditResult::TabComplete | EditResult::Continue => {}
        }
        PostAction::None
    }

    /// Tab-complete a filesystem path in the prompt buffer. For shell
    /// prompts, completes just the last whitespace-delimited word.
    fn tab_complete_path(&mut self) {
        // Extract data from prompt without holding the borrow.
        let (is_shell, is_jump, buffer) = {
            let Mode::Prompting(ref prompt) = self.state.mode else {
                return;
            };
            let is_shell = matches!(
                prompt.kind,
                PromptKind::ShellCmd | PromptKind::ShellCmdCaptured | PromptKind::Command
            );
            let is_jump = matches!(prompt.kind, PromptKind::Jump);
            (is_shell, is_jump, prompt.buffer.clone())
        };

        // Repeated Tab with active cycle state: cycle through matches
        // or re-flash the list for local dirs.
        if let Some(ref mut ts) = self.tab_state {
            if (ts.original_buf == buffer || ts.cycle_index > 0) && ts.matches.len() > 1 {
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
                self.tab_state = Some(TabState {
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
            self.tab_state = Some(TabState {
                original_buf: prompt.buffer.clone(),
                buf_prefix,
                word_base,
                matches,
                cycle_index: 0,
            });
        } else {
            self.tab_state = None;
        }
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
            self.tab_state = None;
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
            self.tab_state = Some(TabState {
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
    fn cancel_prompt(&mut self) {
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
        self.tab_state = None;
        // Clear any stashed state from the two-step new-tab prompt.
        self.state.pending_new_tab_cmd = None;
    }

    /// Parse and dispatch a `:` command.
    ///
    /// Pure-domain arms are handled by `AppState::dispatch_command`;
    /// terminal-touching arms (shell, pager, overlay) stay here.
    fn dispatch_command(&mut self, input: &str) -> PostAction {
        use state::CommandResult;

        // Try the pure-domain handler first.
        match self.state.dispatch_command(input) {
            CommandResult::Handled => return PostAction::None,
            CommandResult::OpenPager { title, lines } => {
                self.pager = Some(PagerView::new_plain(title, lines));
                return PostAction::None;
            }
            CommandResult::NotHandled => {}
        }

        // --- Terminal-touching arms (shell/pager/overlay) ---
        let input = input.trim();

        // :!! — repeat last captured command
        if input == "!!" || input == "!" {
            match self.state.last_captured_cmd.clone() {
                Some(cmd) => {
                    let expanded =
                        crate::shell::expand_percent(&cmd, &self.state.selection_paths());
                    let title = format!("! {cmd}");
                    match spawn_capture(&expanded) {
                        Ok((child, writer, rx)) => {
                            let mut view = PagerView::new_plain(
                                format!("\u{23f3} {title} — running... (0s)"),
                                Vec::new(),
                            );
                            view.streaming = true;
                            self.pager = Some(view);
                            self.pending_capture = Some(PendingCapture {
                                child,
                                writer,
                                output_rx: rx,
                                buffer: Vec::new(),
                                title,
                                cmd_display: cmd,
                                started: std::time::Instant::now(),
                                finished: false,
                            });
                        }
                        Err(e) => self.state.flash_error(format!("exec: {e}")),
                    }
                }
                None => self.state.flash_error("no previous ! command"),
            }
            return PostAction::None;
        }

        // :!<cmd> — captured shell command
        if let Some(cmd) = input.strip_prefix('!') {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                self.state.flash_error("empty command");
                return PostAction::None;
            }
            self.state.last_captured_cmd = Some(cmd.to_string());
            let expanded = crate::shell::expand_percent(cmd, &self.state.selection_paths());
            let title = format!("! {cmd}");
            match spawn_capture(&expanded) {
                Ok((child, writer, rx)) => {
                    let mut view = PagerView::new_plain(
                        format!("\u{23f3} {title} — running... (0s)"),
                        Vec::new(),
                    );
                    view.streaming = true;
                    self.pager = Some(view);
                    self.pending_capture = Some(PendingCapture {
                        child,
                        writer,
                        output_rx: rx,
                        buffer: Vec::new(),
                        title,
                        cmd_display: cmd.to_string(),
                        started: std::time::Instant::now(),
                        finished: false,
                    });
                }
                Err(e) => self.state.flash_error(format!("exec: {e}")),
            }
            return PostAction::None;
        }

        // :;<cmd> — foreground shell command
        if let Some(cmd) = input.strip_prefix(';') {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                self.state.flash_error("empty command");
                return PostAction::None;
            }
            let expanded = crate::shell::expand_percent(cmd, &self.state.selection_paths());
            let (rows, cols) =
                Self::top_overlay_size(self.state.pane_height_pct, self.pane_tabs.is_some());
            let cwd = self.state.listing.dir.clone();
            match Pane::spawn(&expanded, rows, cols, &cwd) {
                Ok(p) => {
                    self.top_overlay = Some(p);
                }
                Err(e) => self.state.flash_error(format!("spawn: {e}")),
            }
            return PostAction::None;
        }

        // :bprev / :bnext — pager buffer history
        if input == "bprev" {
            if let Some(current) = self.pager.take() {
                if let Some(prev) = self.pager_history.go_back(current) {
                    self.pager = Some(prev);
                    self.needs_full_repaint = true;
                    let back = self.pager_history.back_len();
                    let fwd = self.pager_history.forward_len();
                    self.state.flash_info(format!("buffer ←{back} →{fwd}"));
                } else {
                    // go_back returned None — restore current
                    self.pager = self.pager_history.forward.pop();
                    self.state.flash_info("no older buffers");
                }
            } else if let Some(prev) = self.pager_history.back.pop() {
                self.pager = Some(prev);
                self.needs_full_repaint = true;
                self.state
                    .flash_info(format!("buffer ←{}", self.pager_history.back_len()));
            } else {
                self.state.flash_info("no buffers in history");
            }
            return PostAction::None;
        }
        if input == "bnext" {
            if let Some(current) = self.pager.take() {
                if let Some(next) = self.pager_history.go_forward(current) {
                    self.pager = Some(next);
                    self.needs_full_repaint = true;
                    let back = self.pager_history.back_len();
                    let fwd = self.pager_history.forward_len();
                    self.state.flash_info(format!("buffer ←{back} →{fwd}"));
                } else {
                    // go_forward returned None — restore current
                    self.pager = self.pager_history.back.pop();
                    self.needs_full_repaint = true;
                    self.state.flash_info("no newer buffers");
                }
            } else {
                self.state.flash_info("no pager open");
            }
            return PostAction::None;
        }

        // If we get here, AppState said NotHandled but we don't recognize it
        // either — this shouldn't happen, but handle gracefully.
        self.state.flash_error(format!("unknown command: {input}"));
        PostAction::None
    }

    /// Dispatch a submitted prompt.
    ///
    /// Pure-domain arms are handled by `AppState::dispatch_prompt`;
    /// terminal-touching arms (shell, pager, overlay, copy/move) stay here.
    #[allow(clippy::needless_pass_by_value)]
    fn dispatch_prompt(&mut self, prompt: Prompt) -> PostAction {
        use state::PromptResult;

        // Clear any Tab-applied filter before dispatching.
        if self.state.temp_filter.is_some() {
            self.state.temp_filter = None;
            self.state.rebuild_rows();
        }
        self.tab_state = None;

        // Try the pure-domain handler first.
        match self.state.dispatch_prompt(&prompt.kind, &prompt.buffer) {
            PromptResult::Handled => return PostAction::None,
            PromptResult::NotHandled => {}
        }

        // --- Terminal-touching arms ---
        match prompt.kind {
            PromptKind::ShellCmd => {
                let expanded = shell::expand_percent(&prompt.buffer, &self.state.selection_paths());
                let (rows, cols) =
                    Self::top_overlay_size(self.state.pane_height_pct, self.pane_tabs.is_some());
                let cwd = self.state.listing.dir.clone();
                match Pane::spawn(&expanded, rows, cols, &cwd) {
                    Ok(p) => {
                        self.top_overlay = Some(p);
                    }
                    Err(e) => self.state.flash_error(format!("spawn: {e}")),
                }
                PostAction::None
            }
            PromptKind::ShellCmdCaptured => {
                let cmd = if prompt.buffer.trim() == "!" {
                    if let Some(c) = self.state.last_captured_cmd.clone() {
                        c
                    } else {
                        self.state.flash_error("no previous ! command");
                        return PostAction::None;
                    }
                } else {
                    prompt.buffer.clone()
                };
                self.state.last_captured_cmd = Some(cmd.clone());
                let expanded = shell::expand_percent(&cmd, &self.state.selection_paths());
                let title = format!("! {cmd}");
                match spawn_capture(&expanded) {
                    Ok((child, writer, rx)) => {
                        let cmd_display = prompt.buffer.clone();
                        let mut view = PagerView::new_plain(
                            format!("\u{23f3} {title} — running... (0s)"),
                            Vec::new(),
                        );
                        view.streaming = true;
                        self.pager = Some(view);
                        self.pending_capture = Some(PendingCapture {
                            child,
                            writer,
                            output_rx: rx,
                            buffer: Vec::new(),
                            title,
                            cmd_display,
                            started: std::time::Instant::now(),
                            finished: false,
                        });
                    }
                    Err(e) => self.state.flash_error(format!("exec: {e}")),
                }
                PostAction::None
            }
            PromptKind::CopyTo => {
                self.run_selection_to(&prompt.buffer, fs::ops::copy_selection_to, "copied");
                PostAction::None
            }
            PromptKind::MoveTo => {
                self.run_selection_to(&prompt.buffer, fs::ops::move_selection_to, "moved");
                PostAction::None
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
                PostAction::None
            }
            PromptKind::PaneRenameTab => {
                let name = prompt.buffer.trim().to_string();
                if !name.is_empty() {
                    if let Some(tabs) = self.pane_tabs.as_mut() {
                        tabs.active_info_mut().label = name;
                    }
                }
                PostAction::None
            }
            PromptKind::NewFile => {
                let name = prompt.buffer.trim().to_string();
                if name.is_empty() {
                    return PostAction::None;
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
                    return PostAction::None;
                }
                let program = argv.remove(0);
                argv.push(resolved.to_string_lossy().into_owned());
                PostAction::Spawn {
                    program,
                    args: argv,
                    pause_after: false,
                }
            }
            PromptKind::Command => self.dispatch_command(&prompt.buffer),
            // These should have been handled by AppState — unreachable in practice.
            _ => PostAction::None,
        }
    }

    /// Open the split pane if it's closed, close all tabs if it's open.
    fn toggle_pane(&mut self) {
        if self.pane_tabs.is_some() {
            self.pane_tabs = None;
            self.state.pane_focused = false;
            self.needs_full_repaint = true;
            self.state.flash_info("pane closed");
            return;
        }
        let cmd = std::env::var("SPYC_PANE_CMD").unwrap_or_else(|_| "claude".to_string());
        self.open_pane_tab(&cmd);
    }

    /// Spawn a new pane tab. If no tabs exist, creates the container.
    /// Default cwd is `PROJECT_HOME` when set, else the current listing dir.
    fn open_pane_tab(&mut self, cmd: &str) {
        let cwd = self
            .state
            .project_home
            .clone()
            .unwrap_or_else(|| self.state.listing.dir.clone());
        self.open_pane_tab_in(cmd, &cwd);
    }

    fn open_pane_tab_in(&mut self, cmd: &str, cwd: &std::path::Path) {
        let (rows, cols) = Self::pane_spawn_size(
            self.state.pane_height_pct,
            self.state.config.layout.status_position,
        );
        match Pane::spawn_with_env(cmd, rows, cols, cwd, &[]) {
            Ok(p) => {
                self.state.pane_focused = true;
                self.state
                    .flash_info(format!("pane: {cmd} (^W k for list)"));
                let entry = TabEntry::new(p, TabInfo::new(cmd, cwd));
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.push(entry);
                } else {
                    self.pane_tabs = Some(PaneTabs::new(entry));
                }
            }
            Err(e) => self.state.flash_error(format!("pane spawn failed: {e}")),
        }
    }

    /// V — open $EDITOR on the cursor file in the top overlay (replaces
    /// the file list) while the bottom pane stays visible and running.
    fn edit_in_pane(&mut self) {
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
            return;
        };
        let path = row.path.clone();
        if row.kind == EntryKind::Dir {
            self.state.flash_error("V: cannot edit a directory");
            return;
        }
        let argv = shell::resolve_editor();
        if argv.is_empty() {
            self.state.flash_error("no $VISUAL or $EDITOR set");
            return;
        }
        let cmd = format!(
            "{} {}",
            argv.join(" "),
            shell::shell_quote(&path.display().to_string()),
        );
        let (rows, cols) =
            Self::top_overlay_size(self.state.pane_height_pct, self.pane_tabs.is_some());
        let cwd = self.state.listing.dir.clone();
        match Pane::spawn(&cmd, rows, cols, &cwd) {
            Ok(p) => {
                self.top_overlay = Some(p);
            }
            Err(e) => self.state.flash_error(format!("spawn: {e}")),
        }
    }

    /// Does this command look like it's launching Claude CLI?
    fn is_claude_command(cmd: &str) -> bool {
        let first = cmd.split_whitespace().next().unwrap_or("");
        first == "claude" || first.ends_with("/claude")
    }

    /// Resolve the `claude --resume <token>` target to use on session save.
    ///
    /// Strategy, in order:
    /// 1. Read the exit-banner token from pane scrollback. If it's a UUID,
    ///    verify a JSONL exists for it under `~/.claude/projects/<slug>/`.
    ///    Claude sometimes prints the banner with a session ID it never
    ///    persisted (e.g. user `/clear`'d or `/resume`'d to a different
    ///    session before exit), so trusting the banner unconditionally
    ///    leads to "No conversation found with session ID …" on restore.
    /// 2. Fall back to the most-recently-modified JSONL in the project
    ///    slug — that's what the no-arg `claude --resume` picker picks.
    /// 3. Last-ditch: scan `~/.claude/sessions/` (PID-scoped, often stale).
    fn resolve_claude_resume_target(
        pane: &mut crate::pane::Pane,
        cwd: &std::path::Path,
    ) -> (Option<String>, Option<String>) {
        use crate::state::sessions as s;

        let banner_lines = pane.recent_lines(200);
        if let Some(tok) = s::extract_resume_token(&banner_lines) {
            if s::is_uuid(&tok) {
                if s::claude_jsonl_exists(cwd, &tok) {
                    let name = s::find_claude_session_name_public(&tok);
                    return (Some(tok), name);
                }
                // Banner UUID has no JSONL — fall through to most-recent.
            } else {
                // Named sessions: claude resolves names itself, trust it.
                return (Some(tok.clone()), Some(tok));
            }
        }

        if let Some(id) = s::most_recent_jsonl_for_cwd(cwd) {
            let name = s::find_claude_session_name_public(&id);
            return (Some(id), name);
        }

        match s::find_claude_session(cwd) {
            Some(info) => (Some(info.session_id), info.name),
            None => (None, None),
        }
    }

    /// yp — yank visible pane output to the system clipboard.
    fn yank_pane_to_clipboard(&mut self) -> PostAction {
        let Some(tabs) = self.pane_tabs.as_ref() else {
            self.state.flash_error("no pane open");
            return PostAction::None;
        };
        let lines = tabs.active().visible_lines();
        let text: String = lines
            .iter()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            self.state.flash_error("pane is empty");
            return PostAction::None;
        }
        match Self::copy_to_clipboard(&text) {
            Ok(()) => {
                let count = text.lines().count();
                self.state
                    .flash_info(format!("yanked {count} lines from pane"));
            }
            Err(e) => self.state.flash_error(format!("yank failed: {e}")),
        }
        PostAction::None
    }

    /// ya — yank the full scrollback + visible screen from the active pane.
    fn yank_scrollback_to_clipboard(&mut self) -> PostAction {
        let Some(tabs) = self.pane_tabs.as_mut() else {
            self.state.flash_error("no pane open");
            return PostAction::None;
        };
        let lines = tabs.active_mut().recent_lines(10_000);
        let text: String = lines
            .iter()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            self.state.flash_error("pane scrollback is empty");
            return PostAction::None;
        }
        match Self::copy_to_clipboard(&text) {
            Ok(()) => {
                let count = text.lines().count();
                self.state
                    .flash_info(format!("yanked {count} lines (full scrollback)"));
            }
            Err(e) => self.state.flash_error(format!("yank failed: {e}")),
        }
        PostAction::None
    }

    fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("pbcopy").stdin(Stdio::piped()).spawn()?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(text.as_bytes())?;
        }
        child.wait()?;
        Ok(())
    }

    /// yP — yank the last prompt the user typed into the pane.
    fn yank_last_prompt_to_clipboard(&mut self) -> PostAction {
        let Some(text) = self.last_pane_prompt.as_ref() else {
            self.state.flash_error("no prompt to yank");
            return PostAction::None;
        };
        match Self::copy_to_clipboard(text) {
            Ok(()) => {
                let preview: String = text.chars().take(60).collect();
                let ellipsis = if text.len() > 60 { "…" } else { "" };
                self.state
                    .flash_info(format!("yanked prompt: {preview}{ellipsis}"));
            }
            Err(e) => self.state.flash_error(format!("yank failed: {e}")),
        }
        PostAction::None
    }

    /// Put inventory items to the current working directory.
    /// Picked items only if any picks exist, else all.
    /// Items are removed from inventory after successful put.
    fn put_inventory_to_cwd(&mut self) -> PostAction {
        let dest = self.state.listing.dir.clone();
        let item_count = if self.state.inventory.picks.is_empty() {
            self.state.inventory.len()
        } else {
            self.state.inventory.picks.len()
        };
        if item_count == 0 {
            self.state.flash_error("inventory is empty");
            return PostAction::None;
        }
        // TODO: confirmation for large puts (>10 items)
        let (count, _, err) = self.state.inventory.put_to(&dest);
        self.state.rebuild_rows();
        if count > 0 {
            self.state.refresh_listing();
            self.state
                .flash_info(format!("put {count} file(s) to {}", dest.display()));
        }
        if let Some(e) = err {
            self.state.flash_error(e);
        }
        PostAction::None
    }

    fn start_new_tab_prompt(&mut self) {
        let default_cmd = std::env::var("SPYC_PANE_CMD").unwrap_or_else(|_| "claude".to_string());
        let mut p = Prompt::shell(PromptKind::PaneNewTabCmd, "pane command: ");
        p.buffer.clone_from(&default_cmd);
        if let Some(ed) = p.editor.as_mut() {
            ed.set_content(&default_cmd);
        }
        self.state.mode = Mode::Prompting(p);
    }

    /// ^W x — close the active pane tab.
    fn close_active_tab(&mut self) {
        if let Some(tabs) = self.pane_tabs.as_mut() {
            if !tabs.close_active() {
                // Last tab removed.
                self.pane_tabs = None;
                self.state.pane_focused = false;
                self.needs_full_repaint = true;
                self.state.flash_info("pane: last tab closed");
            }
        }
    }

    /// ^a R — restart the active tab's command. Closes the tab and spawns
    /// a fresh one with the same command and working directory.
    fn restart_active_tab(&mut self) {
        let Some(tabs) = self.pane_tabs.as_ref() else {
            return;
        };
        let cmd = tabs.active_info().command.clone();
        let cwd = tabs.active_info().cwd.clone();
        // Close the old tab first.
        if let Some(tabs) = self.pane_tabs.as_mut() {
            if !tabs.close_active() {
                self.pane_tabs = None;
                self.state.pane_focused = false;
            }
        }
        // Spawn a replacement with the same command and cwd.
        self.open_pane_tab_in(&cmd, &cwd);
        self.state.flash_info(format!("pane: restarted {cmd}"));
    }

    /// ^W j / ^W k — set keyboard focus directionally (no wrap).
    fn set_pane_focus(&mut self, want_pane: bool) {
        if self.pane_tabs.is_none() {
            return;
        }
        if self.state.pane_focused == want_pane {
            return; // already there — no-op
        }
        self.state.pane_focused = want_pane;
        if self.state.pane_focused {
            let label = self
                .pane_tabs
                .as_ref()
                .map_or("pane", |t| t.active_info().label.as_str());
            self.state.flash_info(format!("focus: {label}"));
        } else {
            self.state.flash_info("focus: spyc");
        }
    }

    /// Handle keys while the pane is in scroll mode. Vi-style navigation
    /// through the scrollback buffer; `Esc`/`q` exit back to live view.
    fn handle_pane_scroll_key(&mut self, key: crossterm::event::KeyEvent) -> PostAction {
        use crossterm::event::{KeyCode, KeyModifiers};
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Handle pending `g` prefix: gg = scroll top, gf/gF = goto file.
        if self.scroll_pending_g {
            self.scroll_pending_g = false;
            match key.code {
                KeyCode::Char('g') => {
                    self.pane_tabs
                        .as_mut()
                        .unwrap()
                        .active_mut()
                        .scroll_to_top();
                }
                KeyCode::Char('f') => {
                    self.goto_file_from_pane(false);
                }
                KeyCode::Char('F') => {
                    self.goto_file_from_pane(true);
                }
                _ => {} // Unknown g-sequence, ignore
            }
            return PostAction::None;
        }

        let pane = self.pane_tabs.as_mut().unwrap().active_mut();
        match key.code {
            KeyCode::Char('k') | KeyCode::Up => pane.scroll_up(1),
            KeyCode::Char('j') | KeyCode::Down => pane.scroll_down_or_exit(1),
            KeyCode::PageUp | KeyCode::Char('b') if ctrl => pane.scroll_up(20),
            KeyCode::Char('u') if ctrl => pane.scroll_up(10),
            KeyCode::PageDown | KeyCode::Char('f') if ctrl => pane.scroll_down_or_exit(20),
            KeyCode::Char('d') if ctrl => pane.scroll_down_or_exit(10),
            KeyCode::Char('g') => {
                self.scroll_pending_g = true;
            }
            KeyCode::Char('G') => pane.scroll_to_bottom(),
            KeyCode::Char('s') => match pane.save_to_file() {
                Ok(path) => {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    self.state.flash_info(format!("saved: {name}"));
                }
                Err(e) => self.state.flash_info(format!("save error: {e}")),
            },
            KeyCode::Esc | KeyCode::Char('q') => {
                pane.exit_scroll_mode();
                self.state.flash_info("scroll: off");
            }
            _ => {}
        }
        PostAction::None
    }

    /// ^W s — write the current selection as shell-quoted paths to the
    /// pane's stdin. A trailing space is appended so the user can keep
    /// typing without concatenating against the last path. No newline
    /// — let the user decide when to submit.
    fn send_selection_to_pane(&mut self) {
        if self.pane_tabs.is_none() {
            self.state.flash_error("no pane open (Ctrl-\\ to open one)");
            return;
        }
        // Build the payload before grabbing the pane mut-borrow, so we
        // can still call self.flash_* below without overlapping borrows.
        let (payload, count) = {
            let paths = self.state.selection_paths();
            if paths.is_empty() {
                self.state.flash_error("nothing selected");
                return;
            }
            let count = paths.len();
            let mut out = String::new();
            for (i, p) in paths.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&shell::shell_quote(&p.to_string_lossy()));
            }
            out.push(' ');
            (out, count)
        };
        let result = {
            let pane = self
                .pane_tabs
                .as_mut()
                .expect("pane existence already checked")
                .active_mut();
            pane.send_bytes(payload.as_bytes())
        };
        match result {
            Ok(()) => self
                .state
                .flash_info(format!("sent {count} path(s) to pane")),
            Err(e) => self.state.flash_error(format!("send failed: {e}")),
        }
    }

    /// ^W p / ^W i — read file contents of selection (or inventory) and
    /// send them to the active pane tab as bracketed paste. Each file is
    /// wrapped with a header so the recipient (e.g. Claude) knows what
    /// it's looking at.
    fn pipe_content_to_pane(&mut self, use_inventory: bool) {
        if self.pane_tabs.is_none() {
            self.state.flash_error("no pane open");
            return;
        }
        // Build payload: read from cache for inventory, from disk for selection.
        let mut payload = String::new();
        let mut count = 0usize;
        let mut skipped = 0usize;

        if use_inventory {
            let ids = self.state.inventory.selected_ids();
            if ids.is_empty() {
                self.state.flash_error("inventory is empty");
                return;
            }
            for id in &ids {
                if let Some(item) = self.state.inventory.items().find(|i| &i.id == id) {
                    if let Some(bytes) = self.state.inventory.read_content(id) {
                        if let Ok(text) = String::from_utf8(bytes) {
                            if !payload.is_empty() {
                                payload.push('\n');
                            }
                            let _ =
                                write!(payload, "[file: {}]\n{}", item.orig_path.display(), text);
                            count += 1;
                        } else {
                            skipped += 1;
                        }
                    } else {
                        skipped += 1;
                    }
                }
            }
        } else {
            let paths: Vec<PathBuf> = self
                .state
                .selection_paths()
                .into_iter()
                .map(Path::to_path_buf)
                .collect();
            if paths.is_empty() {
                self.state.flash_error("nothing selected");
                return;
            }
            for path in &paths {
                let Ok(contents) = std::fs::read_to_string(path) else {
                    skipped += 1;
                    continue;
                };
                if !payload.is_empty() {
                    payload.push('\n');
                }
                let _ = write!(payload, "[file: {}]\n{}", path.display(), contents);
                count += 1;
            }
        }

        if count == 0 {
            self.state
                .flash_error("no readable text files in selection");
            return;
        }
        // Send as bracketed paste so it arrives as a single block.
        let mut buf = Vec::with_capacity(payload.len() + 12);
        buf.extend_from_slice(b"\x1b[200~");
        buf.extend_from_slice(payload.as_bytes());
        buf.extend_from_slice(b"\x1b[201~");
        let result = {
            let pane = self.pane_tabs.as_mut().unwrap().active_mut();
            pane.send_bytes(&buf)
        };
        let msg = if skipped > 0 {
            format!("piped {count} file(s), skipped {skipped} binary/unreadable")
        } else {
            format!("piped {count} file(s) to pane")
        };
        match result {
            Ok(()) => self.state.flash_info(msg),
            Err(e) => self.state.flash_error(format!("pipe failed: {e}")),
        }
    }

    /// ^W + / ^W - — change the bottom pane's share of the middle rect
    /// in 5% steps, clamped to [10%, 90%].
    fn resize_pane(&mut self, delta_pct: i32) {
        if self.pane_tabs.is_none() {
            return;
        }
        let current = i32::from(self.state.pane_height_pct);
        let new = (current + delta_pct).clamp(10, 90);
        self.state.pane_height_pct = new as u16;
    }

    // ---- Git diff (M12) ----------------------------------------------------

    /// g d / g D — run `git diff` on selection and show in pager.
    ///
    /// `gd` (cached=false) also surfaces *untracked* files in the
    /// selection — without this, the cursor sitting on a `?`/`~`-flagged
    /// new file gives empty diff output and looks broken. We synthesize
    /// an "added" diff per untracked file via `git diff --no-index
    /// /dev/null <file>`, which exits 1 but still produces the diff bytes
    /// we want to render.
    fn open_git_diff(&mut self, cached: bool) {
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            return;
        }
        let cwd = &self.state.listing.dir;
        let path_strings: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();

        let mut args: Vec<&str> = vec!["diff", "--color=always"];
        if cached {
            args.push("--cached");
        }
        args.push("--");
        for s in &path_strings {
            args.push(s);
        }
        let modified_out = match std::process::Command::new("git")
            .args(&args)
            .current_dir(cwd)
            .output()
        {
            Ok(o) => o.stdout,
            Err(e) => {
                self.state.flash_error(format!("git diff: {e}"));
                return;
            }
        };

        let mut combined = modified_out;
        if !cached {
            combined.extend(untracked_diff_bytes(cwd, &path_strings));
        }

        if combined.is_empty() {
            let label = if cached { "staged" } else { "unstaged" };
            self.state.flash_info(format!("no {label} changes"));
            return;
        }
        let label = if cached {
            "git diff --cached"
        } else {
            "git diff (+ new)"
        };
        self.pager = Some(pager::PagerView::new_ansi(label, &combined));
    }

    /// g b — `git blame` on the cursor file. Selection is ignored
    /// (blame on multiple files / a directory is meaningless).
    fn open_git_blame(&mut self) {
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
            self.state.flash_error("git blame: no cursor file");
            return;
        };
        let path = row.path.clone();
        if path.is_dir() {
            self.state.flash_error("git blame: cursor is a directory");
            return;
        }
        let path_str = path.display().to_string();
        match std::process::Command::new("git")
            .args(["blame", "--color-lines", "--", &path_str])
            .current_dir(&self.state.listing.dir)
            .output()
        {
            Ok(out) if out.status.success() && !out.stdout.is_empty() => {
                let title = format!("git blame {}", row.display);
                self.pager = Some(pager::PagerView::new_ansi(title, &out.stdout));
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let msg = stderr.lines().next().unwrap_or("no output").trim();
                self.state.flash_error(format!("git blame: {msg}"));
            }
            Err(e) => self.state.flash_error(format!("git blame: {e}")),
        }
    }

    // ---- Path references (M13) ------------------------------------------------

    /// `gf` / `gF` — scan the active pane's visible output for a file path
    /// reference, navigate the file list there, and optionally open the
    /// pager at the referenced line.
    fn goto_file_from_pane(&mut self, open_at_line: bool) {
        let Some(tabs) = self.pane_tabs.as_mut() else {
            self.state.flash_error("no pane open");
            return;
        };
        // Scan the last 200 lines of output (not just the visible
        // viewport) so paths in large diffs are still found.
        let lines = tabs.active_mut().recent_lines(200);
        // Also try resolving against the spyc cwd (project root), not just
        // the pane tab's cwd — Claude often prints paths relative to the
        // project root regardless of the shell's cwd.
        let pane_cwd = tabs.active_info().cwd.clone();
        let spyc_cwd = self.state.listing.dir.clone();

        // Debug: dump visible lines to the debug log so we can see what
        // the vt100 screen actually contains.
        spyc_debug!(
            "gf: {} lines from pane, pane_cwd={}, spyc_cwd={}",
            lines.len(),
            pane_cwd.display(),
            spyc_cwd.display()
        );
        for (i, line) in lines.iter().enumerate() {
            if !line.trim().is_empty() {
                spyc_debug!("gf line[{i}]: {:?}", line);
            }
        }

        let pathref = crate::pane::pathref::extract_path_ref(&lines, &pane_cwd).or_else(|| {
            (pane_cwd != spyc_cwd)
                .then(|| crate::pane::pathref::extract_path_ref(&lines, &spyc_cwd))
                .flatten()
        });

        let Some(pathref) = pathref else {
            self.state
                .flash_error("no path reference found in pane output");
            return;
        };

        spyc_debug!(
            "gf: found path={}, line={:?}",
            pathref.path.display(),
            pathref.line
        );

        let path = pathref.path;
        let line = pathref.line;

        // Exit scroll mode and switch focus to the file list so the user
        // sees the navigation result.
        if let Some(tabs) = self.pane_tabs.as_mut() {
            if tabs.active().is_scrolling() {
                tabs.active_mut().exit_scroll_mode();
            }
        }
        self.state.pane_focused = false;
        self.needs_full_repaint = true;

        // Navigate: if it's a directory, chdir there; if a file, chdir to
        // its parent and focus on it.
        if path.is_dir() {
            if let Err(e) = self.state.chdir(&path) {
                self.state.flash_error(format!("gf: {e}"));
            }
            return;
        }

        if let Some(parent) = path.parent() {
            if parent != self.state.listing.dir {
                if let Err(e) = self.state.chdir(parent) {
                    self.state.flash_error(format!("gf: {e}"));
                    return;
                }
            }
            self.state.focus_on_path(&path);
        }

        // gF: also open the file in the pager at the referenced line.
        if open_at_line {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );

            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let lines_vec: Vec<String> = text.lines().map(String::from).collect();
                    let mut view = pager::PagerView::new_plain(&name, lines_vec);
                    view.source_path = Some(path);
                    // Jump to the referenced line (0-indexed scroll).
                    if let Some(ln) = line {
                        view.scroll = u16::try_from(ln.saturating_sub(1)).unwrap_or(u16::MAX);
                    }
                    self.pager = Some(view);
                }
                Err(e) => {
                    self.state
                        .flash_error(format!("gF: cannot read {name}: {e}"));
                }
            }
        } else if let Some(ln) = line {
            self.state.flash_info(format!(
                "{}:{}",
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned()
                ),
                ln
            ));
        }
    }

    // ---- Session management --------------------------------------------------

    fn save_session(&mut self) {
        use crate::state::sessions::{SavedTab, Session};
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let epoch_secs = now.as_secs();
        let id = now.as_millis() as u64;

        let tabs: Vec<SavedTab> = self
            .pane_tabs
            .as_mut()
            .map(|pt| {
                pt.tabs_mut()
                    .iter_mut()
                    .map(|t| {
                        let (claude_session_id, claude_session_name) =
                            if Self::is_claude_command(&t.info.command) {
                                Self::resolve_claude_resume_target(&mut t.pane, &t.info.cwd)
                            } else {
                                (None, None)
                            };
                        SavedTab {
                            command: t.info.command.clone(),
                            label: t.info.label.clone(),
                            cwd: t.info.cwd.clone(),
                            claude_session_id,
                            claude_session_name,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let session = Session {
            id,
            saved_at: crate::sysinfo::format_now(),
            epoch_secs,
            cwd: self.state.listing.dir.clone(),
            tabs,
            active_tab: self.pane_tabs.as_ref().map_or(0, PaneTabs::active_index),
            pane_height_pct: self.state.pane_height_pct,
            pane_focused: self.state.pane_focused,
            name: self.state.session_name.clone().unwrap_or_default(),
            project_home: self.state.project_home.clone(),
        };
        let _ = crate::state::sessions::save_session(&session);

        // Build exit summary for post-TUI output.
        let cwd_display = crate::paths::display_tilde(&self.state.listing.dir);
        let tab_count = session.tabs.len();
        let claude_names: Vec<String> = session
            .tabs
            .iter()
            .filter_map(|t| t.claude_session_name.clone())
            .collect();
        let mut parts = vec![format!("session saved — {cwd_display}")];
        if tab_count > 0 {
            parts.push(format!(
                "{tab_count} pane tab{}",
                if tab_count == 1 { "" } else { "s" }
            ));
        }
        if !claude_names.is_empty() {
            parts.push(format!("claude: {}", claude_names.join(", ")));
        }
        parts.push("restore with spyc -r".to_string());
        self.exit_summary = Some(parts.join(" · "));
    }

    fn show_session_picker(&mut self) {
        use crate::state::sessions;
        let sessions = sessions::load_sessions();
        if sessions.is_empty() {
            self.state.flash_info("no saved sessions");
            return;
        }
        let lines: Vec<String> = sessions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let age = sessions::format_relative_time(s.epoch_secs);
                let tab_count = s.tabs.len();
                let names: Vec<&str> = s.tabs.iter().map(|t| t.label.as_str()).collect();
                // Show Claude session info for tabs that have it.
                let claude_info: Vec<String> = s
                    .tabs
                    .iter()
                    .filter_map(|t| {
                        let sid = t.claude_session_id.as_deref()?;
                        let short_id = &sid[..sid.len().min(8)];
                        match &t.claude_session_name {
                            Some(name) => Some(format!("{name} ({short_id})")),
                            None => Some(short_id.to_string()),
                        }
                    })
                    .collect();
                let tab_info = if tab_count == 0 {
                    String::new()
                } else {
                    format!("  [{}]", names.join(", "))
                };
                let claude_suffix = if claude_info.is_empty() {
                    String::new()
                } else {
                    format!("  claude: {}", claude_info.join(", "))
                };
                let name_col = if s.name.is_empty() {
                    "(unnamed)"
                } else {
                    s.name.as_str()
                };
                format!(
                    "  [{}]  {:<22} {:<14} {}{}{}",
                    i + 1,
                    name_col,
                    age,
                    s.cwd.display(),
                    tab_info,
                    claude_suffix
                )
            })
            .collect();
        self.state.pending_sessions = Some(sessions);
        let mut all_lines = vec!["  [n]  new session".to_string(), String::new()];
        all_lines.extend(lines);
        let mut view = pager::PagerView::new_plain(
            "sessions — j/k navigate, Enter restore, n new, q close",
            all_lines,
        );
        view.picker_cursor = Some(2); // Start on first session (after header).
        self.pager = Some(view);
    }

    /// Prefix width for history editor lines: "  NNN  " = 7 chars.
    const HIST_PREFIX_W: usize = 7;

    /// Sync the history editor after moving the picker cursor to a new line.
    /// Updates the LineEditor content and the display line.
    fn sync_history_editor_to_cursor(&mut self) {
        Self::sync_hist_editor(
            &mut self.pager,
            &mut self.pending_history_pick,
            &self.state.history,
        );
    }

    fn sync_hist_editor(
        pager: &mut Option<pager::PagerView>,
        editor_opt: &mut Option<LineEditor>,
        history: &crate::state::history::History,
    ) {
        let Some(view) = pager else { return };
        let Some(editor) = editor_opt else { return };
        let new_cursor = view.picker_cursor.unwrap_or(0);
        let entries = history.entries();
        let hist_idx = entries.len().saturating_sub(1 + new_cursor);
        if let Some(cmd) = entries.get(hist_idx) {
            editor.set_content_keep_mode(cmd);
        }
        let text = format!("  {:>3}  {}", new_cursor + 1, editor.text());
        view.lines[new_cursor] = ratatui::text::Line::from(text);
        view.picker_edit_cursor = Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));
    }

    fn show_history_popup(&mut self) {
        let entries = self.state.history.entries();
        if entries.is_empty() {
            self.state.flash_info("history is empty");
            return;
        }
        // Show newest-first, numbered from 1.
        let lines: Vec<String> = entries
            .iter()
            .rev()
            .enumerate()
            .map(|(i, cmd)| format!("  {:>3}  {}", i + 1, cmd))
            .collect();
        // Create a line editor loaded with the newest entry, Normal mode.
        let newest = entries.last().unwrap();
        let mut editor = LineEditor::new();
        editor.set_content(newest);
        editor.mode = crate::ui::line_edit::Mode::Normal;
        if !editor.buf.is_empty() {
            editor.cursor = editor.buf.len() - 1;
        }
        let mut view = pager::PagerView::new_plain(
            "history — j/k move, i edit, Enter run, ^D delete, q close",
            lines,
        );
        view.picker_cursor = Some(0);
        view.picker_edit_cursor = Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));
        self.pending_history_pick = Some(editor);
        self.pager = Some(view);
    }

    fn restore_session(&mut self, session: &crate::state::sessions::Session) {
        // Restore working directory and update start_dir so backtick (`)
        // jumps to the session's home, not where spyc was launched from.
        if session.cwd.is_dir() {
            if let Err(e) = self.state.chdir(&session.cwd) {
                self.state.flash_error(format!("session chdir: {e}"));
                return;
            }
            self.state.start_dir.clone_from(&session.cwd);
        } else {
            self.state
                .flash_error(format!("session dir gone: {}", session.cwd.display()));
            return;
        }
        // Keep the startup-generated name when an older session file
        // has no name field; otherwise take the saved one.
        if !session.name.is_empty() {
            self.state.session_name = Some(session.name.clone());
        }
        self.state.project_home = session.project_home.clone().filter(|p| p.is_dir());
        // Restore pane layout.
        self.state.pane_height_pct = session.pane_height_pct;
        if !session.tabs.is_empty() {
            self.pane_tabs = None;
            for tab in &session.tabs {
                let cwd = if tab.cwd.is_dir() {
                    &tab.cwd
                } else {
                    &session.cwd
                };
                // If the tab has a Claude session ID, resume that
                // conversation instead of starting fresh.
                let cmd = if let Some(ref sid) = tab.claude_session_id {
                    format!("claude --resume {sid}")
                } else {
                    tab.command.clone()
                };
                self.open_pane_tab_in(&cmd, cwd);
            }
            // Restore active tab.
            if let Some(tabs) = self.pane_tabs.as_mut() {
                tabs.switch_to(session.active_tab);
                // Restore custom labels.
                for (entry, saved) in tabs.tabs_mut().iter_mut().zip(&session.tabs) {
                    entry.info.label.clone_from(&saved.label);
                }
            }
            self.state.pane_focused = session.pane_focused;
        }
        self.state.flash_info("session restored");
    }

    // ---- Git worktree (M11) -------------------------------------------------

    /// W l — list worktrees in a pager; digit keys 1-9 select.
    fn worktree_list(&mut self) {
        match crate::sysinfo::git_worktree_list(&self.state.listing.dir) {
            Some(worktrees) => {
                self.state.pending_worktrees =
                    Some(worktrees.iter().map(|w| w.path.clone()).collect());
                let lines: Vec<String> = worktrees
                    .iter()
                    .enumerate()
                    .map(|(i, wt)| {
                        let current = if wt.path == self.state.listing.dir {
                            " ← current"
                        } else {
                            ""
                        };
                        format!(
                            "  [{}]  {:<30} {:>8}  {}{}",
                            i + 1,
                            wt.branch,
                            wt.head,
                            wt.path.display(),
                            current,
                        )
                    })
                    .collect();
                let view = pager::PagerView::new_plain(
                    "git worktrees — press 1-9 to switch, q to close",
                    lines,
                );
                self.pager = Some(view);
            }
            None => self
                .state
                .flash_error("not in a git repository (or no worktrees)"),
        }
    }

    /// Compute the (rows, cols) the bottom pane will occupy.
    fn pane_spawn_size(height_pct: u16, status_position: StatusPosition) -> (u16, u16) {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let area = ratatui::layout::Rect::new(0, 0, cols, rows);
        let layout = Self::compute_layout(area, true, height_pct, status_position);
        match layout.pane {
            Some(r) => (r.height.max(1), r.width.max(1)),
            None => (rows.saturating_sub(3).max(1), cols.max(1)),
        }
    }

    /// Compute the (rows, cols) available for the top-overlay pty. This
    /// is the top area: everything above the divider (or the whole
    /// screen minus the prompt row if no bottom pane).
    fn top_overlay_size(pane_height_pct: u16, has_bottom_pane: bool) -> (u16, u16) {
        let (cols, total_rows) = crossterm::terminal::size().unwrap_or((80, 24));
        if !has_bottom_pane {
            // Full screen minus prompt row.
            return (total_rows.saturating_sub(1).max(1), cols.max(1));
        }
        // With bottom pane: top region = total - divider(1) - bottom pane.
        let usable = total_rows.saturating_sub(1); // minus divider
        let bottom = (u32::from(usable) * u32::from(pane_height_pct) / 100) as u16;
        let top = usable.saturating_sub(bottom);
        (top.max(1), cols.max(1))
    }

    fn show_session_info(&mut self) {
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!(
            "\u{1f336}\u{fe0f} spyc {}",
            env!("CARGO_PKG_VERSION")
        ));
        lines.push(format!("session  : {}", self.state.session_display()));
        lines.push(format!("project  : {}", self.state.project_home_display()));
        lines.push(format!("user@host: {}", self.state.user_host));
        lines.push(format!(
            "start dir: {}",
            crate::paths::display_tilde(&self.state.start_dir)
        ));
        lines.push(format!("pid      : {}", std::process::id()));
        lines.push(format!(
            "cwd      : {}",
            crate::paths::display_tilde(&self.state.listing.dir)
        ));
        lines.push(format!("entries  : {}", self.state.listing.entries.len()));
        lines.push(format!("visible  : {}", self.state.rows.len()));
        lines.push(format!("picks    : {}", self.state.picks.len()));
        lines.push(format!("inventory: {}", self.state.inventory.len()));
        lines.push(format!("marks    : {}", self.state.marks.entries.len()));
        lines.push(format!("rss      : {}", crate::sysinfo::format_rss()));
        lines.push(format!("time     : {}", crate::sysinfo::format_now()));
        if !self.state.config.sources.is_empty() {
            lines.push(String::new());
            lines.push("config sources:".into());
            for src in &self.state.config.sources {
                lines.push(format!("  {}", crate::paths::display_tilde(src)));
            }
        }
        self.pager = Some(PagerView::new_plain("session info", lines));
    }

    /// Title used for the help pager. Also used by the resize handler to
    /// detect when help is open and needs rebuilding for the new width.
    const HELP_TITLE: &'static str = "spyc — key bindings";

    /// Build and show the help pager. Called from `Action::Help` and on
    /// terminal resize (to re-wrap descriptions for the new width and
    /// pick the right column count).
    fn open_help(&mut self) {
        let (term_w, _) = crossterm::terminal::size().unwrap_or((80, 24));
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
        let lines = help::build_lines(&self.theme, &self.state.user_keymap, col_w);
        let mut view = pager::PagerView::new_styled(Self::HELP_TITLE, lines);
        view.columns = ncols as u8;
        self.pager = Some(view);
    }

    /// True when the help pager is the currently-open pager view.
    fn help_is_open(&self) -> bool {
        self.pager
            .as_ref()
            .is_some_and(|v| v.title == Self::HELP_TITLE)
    }

    /// Resolve `raw_dest` and run a copy-like or move-like operation across
    /// the current selection. Flash a success / error message afterwards
    /// and refresh the listing so results are visible immediately.
    fn run_selection_to(
        &mut self,
        raw_dest: &str,
        op: fn(&[&Path], &Path) -> std::io::Result<()>,
        verb: &str,
    ) {
        let dest_trim = raw_dest.trim();
        if dest_trim.is_empty() {
            return;
        }
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            self.state.flash_error("nothing selected");
            return;
        }
        let count = paths.len();
        let expanded = crate::paths::expand(dest_trim);
        let dest = if expanded.is_absolute() {
            expanded
        } else {
            self.state.listing.dir.join(&expanded)
        };
        self.run_and_flash(
            op(&paths, &dest),
            format!("{verb} {count} item(s) to {}", dest.display()),
        );
        // Picks point at paths that may no longer exist after a move.
        self.state.picks.clear();
        self.state.refresh_listing();
    }

    /// Set the flash message based on the result of a mutating operation.
    fn run_and_flash(&mut self, result: std::io::Result<()>, success_msg: String) {
        match result {
            Ok(()) => self.state.flash_info(success_msg),
            Err(e) => self.state.flash_error(format!("error: {e}")),
        }
    }

    /// Advance the cursor by `delta` entries in flat order, wrapping at
    /// both ends of the list. This is what `j` / `k` use so pressing `j`
    /// at the bottom jumps back to the top (and vice versa).
    /// Route a key to the pager overlay. Also uses vi-like motion so the
    /// pager feels native to the rest of the UI.
    fn handle_pager_key(&mut self, key: KeyEvent) -> PostAction {
        let Some(view) = &mut self.pager else {
            return PostAction::None;
        };
        // Clear any one-shot flash message from the previous keypress.
        view.flash = None;
        // Compute the pager's actual viewport from the terminal size.
        // The pager overlay occupies ~92% of the frame height, with a
        // 1-row border on each side, and possibly a search bar at the
        // bottom.
        let viewport = {
            let (_, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
            let pager_h = if view.full_width {
                term_h
            } else {
                (u32::from(term_h) * 92 / 100) as u16
            };
            pager_h.saturating_sub(2).max(2)
        };

        // While typing a search query, most keys feed the buffer.
        if view.is_typing_search() {
            match key.code {
                KeyCode::Esc => view.cancel_search(),
                KeyCode::Enter => {
                    let committed = view.commit_search(viewport);
                    if !committed {
                        self.state.flash_error("no matches");
                    } else if let Some(ref mut editor) = self.pending_history_pick {
                        // Sync picker cursor to the first match.
                        if let Some(line) = view.current_match_line() {
                            view.picker_cursor = Some(line);
                            let nc = line;
                            let entries = self.state.history.entries();
                            let hi = entries.len().saturating_sub(1 + nc);
                            if let Some(cmd) = entries.get(hi) {
                                editor.set_content_keep_mode(cmd);
                            }
                            let text = format!("  {:>3}  {}", nc + 1, editor.text());
                            view.lines[nc] = ratatui::text::Line::from(text);
                            view.picker_edit_cursor =
                                Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));
                        }
                    }
                }
                KeyCode::Backspace => view.search_backspace(),
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.search_push_char(c);
                }
                _ => {}
            }
            return PostAction::None;
        }

        // Inline `:N` jump — accumulate digits, Enter commits, Esc cancels.
        if let Some(ref mut buf) = self.pager_jump_buf {
            match key.code {
                KeyCode::Char(c @ '0'..='9') => {
                    buf.push(c);
                    view.jump_buf = Some(buf.clone());
                }
                KeyCode::Backspace => {
                    if buf.pop().is_none() {
                        self.pager_jump_buf = None;
                        view.jump_buf = None;
                    } else {
                        view.jump_buf = Some(buf.clone());
                    }
                }
                KeyCode::Enter => {
                    if let Ok(n) = buf.parse::<usize>() {
                        if n > 0 {
                            let target = n.saturating_sub(1);
                            if self.pending_history_pick.is_some() {
                                // History editor: jump to entry N.
                                let max = view.lines.len().saturating_sub(1);
                                let clamped = target.min(max);
                                view.picker_cursor = Some(clamped);
                                view.scroll =
                                    u16::try_from(clamped.saturating_sub(2)).unwrap_or(u16::MAX);
                            } else {
                                // Regular pager: jump to line N.
                                view.scroll = u16::try_from(target).unwrap_or(u16::MAX);
                            }
                        }
                    }
                    view.jump_buf = None;
                    self.pager_jump_buf = None;
                    if self.pending_history_pick.is_some() {
                        self.sync_history_editor_to_cursor();
                    }
                }
                _ => {
                    // Esc or non-digit cancels.
                    self.pager_jump_buf = None;
                    view.jump_buf = None;
                }
            }
            return PostAction::None;
        }

        // [b / ]b — pager buffer history navigation (two-key sequence).
        if let Some(bracket) = self.pager_pending_bracket.take() {
            if key.code == KeyCode::Char('b') {
                match bracket {
                    '[' => {
                        if let Some(current) = self.pager.take() {
                            if let Some(prev) = self.pager_history.go_back(current) {
                                self.pager = Some(prev);
                                self.needs_full_repaint = true;
                                let back = self.pager_history.back_len();
                                let fwd = self.pager_history.forward_len();
                                self.state.flash_info(format!("buffer ←{back} →{fwd}"));
                            } else {
                                // Restore — go_back returned None.
                                self.pager = self.pager_history.forward.pop();
                                self.state.flash_info("no older buffers");
                            }
                        }
                    }
                    ']' => {
                        if let Some(current) = self.pager.take() {
                            if let Some(next) = self.pager_history.go_forward(current) {
                                self.pager = Some(next);
                                self.needs_full_repaint = true;
                                let back = self.pager_history.back_len();
                                let fwd = self.pager_history.forward_len();
                                self.state.flash_info(format!("buffer ←{back} →{fwd}"));
                            } else {
                                self.pager = self.pager_history.back.pop();
                                self.state.flash_info("no newer buffers");
                            }
                        }
                    }
                    _ => {}
                }
            }
            return PostAction::None;
        }

        // Worktree picker: 1-9 selects a worktree and chdirs.
        if let Some(ref worktrees) = self.state.pending_worktrees {
            if let KeyCode::Char(c @ '1'..='9') = key.code {
                let idx = (c as u8 - b'1') as usize;
                if let Some(path) = worktrees.get(idx).cloned() {
                    self.pager = None;
                    self.state.pending_worktrees = None;
                    self.needs_full_repaint = true;
                    if let Err(e) = self.state.chdir(&path) {
                        self.state.flash_error(format!("chdir: {e}"));
                    }
                    return PostAction::None;
                }
            }
        }

        // History editor: vi-edit highlighted line, Enter runs, d/x deletes.
        if let Some(ref mut editor) = self.pending_history_pick {
            use crate::ui::line_edit::EditResult;
            let editor_is_normal = editor.mode == crate::ui::line_edit::Mode::Normal;

            // Ctrl+D deletes the highlighted entry from history (any mode).
            if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
                let cursor = view.picker_cursor.unwrap_or(0);
                let entries = self.state.history.entries();
                let hist_idx = entries.len().saturating_sub(1 + cursor);
                if hist_idx < entries.len() {
                    self.state.history.remove(hist_idx);
                    if self.state.history.entries().is_empty() {
                        self.pager = None;
                        self.pending_history_pick = None;
                        self.needs_full_repaint = true;
                        self.state.flash_info("history is empty");
                        return PostAction::None;
                    }
                    let old_cursor = cursor;
                    self.show_history_popup();
                    if let Some(ref mut v) = self.pager {
                        let max = (v.line_count() as usize).saturating_sub(1);
                        v.picker_cursor = Some(old_cursor.min(max));
                        let new_cur = v.picker_cursor.unwrap_or(0);
                        let entries = self.state.history.entries();
                        let hist_idx = entries.len().saturating_sub(1 + new_cur);
                        if let Some(ref mut ed) = self.pending_history_pick {
                            if let Some(cmd) = entries.get(hist_idx) {
                                ed.set_content_keep_mode(cmd);
                            }
                            v.picker_edit_cursor = Some((Self::HIST_PREFIX_W + ed.cursor, ed.mode));
                            let text = format!("  {:>3}  {}", new_cur + 1, ed.text());
                            v.lines[new_cur] = ratatui::text::Line::from(text);
                        }
                    }
                }
                return PostAction::None;
            }

            // Inline sync: update editor from the current picker line.
            // Uses `view` and `editor` already borrowed in this scope.
            macro_rules! sync_editor {
                ($v:expr, $ed:expr, $hist:expr) => {{
                    let nc = $v.picker_cursor.unwrap_or(0);
                    let entries = $hist.entries();
                    let hi = entries.len().saturating_sub(1 + nc);
                    if let Some(cmd) = entries.get(hi) {
                        $ed.set_content_keep_mode(cmd);
                    }
                    let text = format!("  {:>3}  {}", nc + 1, $ed.text());
                    $v.lines[nc] = ratatui::text::Line::from(text);
                    $v.picker_edit_cursor = Some((Self::HIST_PREFIX_W + $ed.cursor, $ed.mode));
                }};
            }

            // In Normal mode, j/k/G/gg/n/N navigate, / searches, : jumps.
            if editor_is_normal {
                let handled = match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        self.history_pending_g = false;
                        view.picker_move(1, viewport);
                        sync_editor!(view, editor, self.state.history);
                        true
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        self.history_pending_g = false;
                        view.picker_move(-1, viewport);
                        sync_editor!(view, editor, self.state.history);
                        true
                    }
                    KeyCode::Char('G') => {
                        self.history_pending_g = false;
                        let last = view.lines.len().saturating_sub(1);
                        let delta = last as isize - view.picker_cursor.unwrap_or(0) as isize;
                        view.picker_move(delta, viewport);
                        sync_editor!(view, editor, self.state.history);
                        true
                    }
                    KeyCode::Char('g') => {
                        if self.history_pending_g {
                            self.history_pending_g = false;
                            let delta = -(view.picker_cursor.unwrap_or(0) as isize);
                            view.picker_move(delta, viewport);
                            sync_editor!(view, editor, self.state.history);
                        } else {
                            self.history_pending_g = true;
                        }
                        true
                    }
                    KeyCode::Char('/') => {
                        self.history_pending_g = false;
                        view.begin_search();
                        true
                    }
                    KeyCode::Char('n') => {
                        self.history_pending_g = false;
                        view.search_next(viewport);
                        if let Some(line) = view.current_match_line() {
                            view.picker_cursor = Some(line);
                            sync_editor!(view, editor, self.state.history);
                        }
                        true
                    }
                    KeyCode::Char('N') => {
                        self.history_pending_g = false;
                        view.search_prev(viewport);
                        if let Some(line) = view.current_match_line() {
                            view.picker_cursor = Some(line);
                            sync_editor!(view, editor, self.state.history);
                        }
                        true
                    }
                    KeyCode::Char(':') => {
                        self.history_pending_g = false;
                        self.pager_jump_buf = Some(String::new());
                        view.jump_buf = Some(String::new());
                        true
                    }
                    // Disable pager keys that don't make sense here.
                    KeyCode::Char('l' | 'v') => true,
                    _ => {
                        self.history_pending_g = false;
                        false
                    }
                };
                if handled {
                    return PostAction::None;
                }
            }

            // Feed all other keys to the line editor.
            let result = editor.feed(key);
            // Sync the display line with the editor buffer.
            let pc = view.picker_cursor.unwrap_or(0);
            let text = format!("  {:>3}  {}", pc + 1, editor.text());
            view.lines[pc] = ratatui::text::Line::from(text);
            view.picker_edit_cursor = Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));

            match result {
                EditResult::Submit => {
                    let cmd = editor.text();
                    self.pager = None;
                    self.pending_history_pick = None;
                    self.needs_full_repaint = true;
                    if cmd.trim().is_empty() {
                        return PostAction::None;
                    }
                    // Execute the (possibly edited) command directly.
                    self.state.last_captured_cmd = Some(cmd.clone());
                    self.state.history.push(cmd.trim());
                    let expanded =
                        crate::shell::expand_percent(&cmd, &self.state.selection_paths());
                    let title = format!("! {cmd}");
                    match spawn_capture(&expanded) {
                        Ok((child, writer, rx)) => {
                            let mut cview = PagerView::new_plain(
                                format!("\u{23f3} {title} — running... (0s)"),
                                Vec::new(),
                            );
                            cview.streaming = true;
                            self.pager = Some(cview);
                            self.pending_capture = Some(PendingCapture {
                                child,
                                writer,
                                output_rx: rx,
                                buffer: Vec::new(),
                                title,
                                cmd_display: cmd,
                                started: std::time::Instant::now(),
                                finished: false,
                            });
                        }
                        Err(e) => self.state.flash_error(format!("exec: {e}")),
                    }
                }
                EditResult::Cancel => {
                    // Esc in Insert → Normal (handled by editor, returns Continue).
                    // Cancel only fires from Normal-mode Esc or Ctrl+C → close popup.
                    self.pager = None;
                    self.pending_history_pick = None;
                    self.needs_full_repaint = true;
                }
                EditResult::HistoryPrev | EditResult::HistoryNext => {
                    // Up/Down in Insert mode → move between lines.
                    // HistoryPrev = Up key → move toward top of list (newer).
                    let delta: isize = if result == EditResult::HistoryPrev {
                        -1
                    } else {
                        1
                    };
                    view.picker_move(delta, viewport);
                    let new_cursor = view.picker_cursor.unwrap_or(0);
                    let entries = self.state.history.entries();
                    let hist_idx = entries.len().saturating_sub(1 + new_cursor);
                    if let Some(cmd) = entries.get(hist_idx) {
                        editor.set_content(cmd);
                    }
                    let text = format!("  {:>3}  {}", new_cursor + 1, editor.text());
                    view.lines[new_cursor] = ratatui::text::Line::from(text);
                    view.picker_edit_cursor =
                        Some((Self::HIST_PREFIX_W + editor.cursor, editor.mode));
                }
                EditResult::TabComplete | EditResult::Continue => {}
            }
            return PostAction::None;
        }

        // Session picker: j/k navigate, Enter/1-9 select, n new.
        if self.state.pending_sessions.is_some() {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    view.picker_move(1, viewport);
                    return PostAction::None;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    view.picker_move(-1, viewport);
                    return PostAction::None;
                }
                KeyCode::Char(c @ '1'..='9') => {
                    // Direct selection — index into sessions (offset by 2 header lines).
                    let sessions = self.state.pending_sessions.take().unwrap();
                    let idx = (c as u8 - b'1') as usize;
                    if let Some(session) = sessions.get(idx) {
                        let session = session.clone();
                        self.pager = None;
                        self.needs_full_repaint = true;
                        self.restore_session(&session);
                        return PostAction::None;
                    }
                    self.state.pending_sessions = Some(sessions);
                }
                KeyCode::Enter => {
                    let cursor = view.picker_cursor.unwrap_or(0);
                    let sessions = self.state.pending_sessions.take().unwrap();
                    if cursor < 2 {
                        // "New session" header.
                        self.pager = None;
                        self.needs_full_repaint = true;
                        self.state.flash_info("new session");
                        return PostAction::None;
                    }
                    let idx = cursor - 2;
                    if let Some(session) = sessions.get(idx) {
                        let session = session.clone();
                        self.pager = None;
                        self.needs_full_repaint = true;
                        self.restore_session(&session);
                        return PostAction::None;
                    }
                    self.state.pending_sessions = Some(sessions);
                }
                KeyCode::Char('n' | 'N') => {
                    self.pager = None;
                    self.state.pending_sessions = None;
                    self.needs_full_repaint = true;
                    self.state.flash_info("new session");
                    return PostAction::None;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('q' | 'Q') | KeyCode::Esc => {
                // Save eligible pagers to history before closing.
                let is_picker = self.state.pending_worktrees.is_some()
                    || self.state.pending_sessions.is_some()
                    || self.pending_history_pick.is_some();
                if !is_picker {
                    if let Some(ref v) = self.pager {
                        if v.picker_cursor.is_none() && !v.streaming {
                            if let Some(v) = self.pager.take() {
                                self.pager_history.push(v);
                            }
                        }
                    }
                }
                self.pager = None;
                self.state.pending_worktrees = None;
                self.state.pending_sessions = None;
                self.pending_history_pick = None;
                self.pager_jump_buf = None;
                self.pager_pending_bracket = None;
                self.needs_full_repaint = true;
            }
            KeyCode::Char('/') => view.begin_search(),
            KeyCode::Char('n') => view.search_next(viewport),
            KeyCode::Char('N') => view.search_prev(viewport),
            KeyCode::Char(':') => {
                self.pager_jump_buf = Some(String::new());
                view.jump_buf = Some(String::new());
            }
            KeyCode::Char('[' | ']') => {
                if let KeyCode::Char(c) = key.code {
                    self.pager_pending_bracket = Some(c);
                }
            }
            KeyCode::Char('j') | KeyCode::Down => view.scroll_by(1, viewport),
            KeyCode::Char('k') | KeyCode::Up => view.scroll_by(-1, viewport),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                view.scroll_by(i32::from(viewport) / 2, viewport);
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                view.scroll_by(-i32::from(viewport) / 2, viewport);
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                view.scroll_by(i32::from(viewport), viewport);
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                view.scroll_by(-i32::from(viewport), viewport);
            }
            KeyCode::PageDown | KeyCode::Char(' ') => view.scroll_by(i32::from(viewport), viewport),
            KeyCode::PageUp | KeyCode::Char('b') => view.scroll_by(-i32::from(viewport), viewport),
            KeyCode::Char('g') | KeyCode::Home => view.scroll_to_top(),
            KeyCode::Char('G') | KeyCode::End => view.scroll_to_bottom(viewport),
            KeyCode::Char('l') => view.toggle_line_numbers(),
            KeyCode::Char('w') => view.toggle_whitespace(),
            KeyCode::Char('f') => view.toggle_full_width(),
            KeyCode::Char('y') => match view.yank_to_clipboard() {
                Ok(()) => view.flash = Some("yanked to clipboard".into()),
                Err(e) => view.flash = Some(format!("yank failed: {e}")),
            },
            KeyCode::Char('s') if view.saveable => match view.save_to_file() {
                Ok(path) => view.flash = Some(format!("saved: {}", path.display())),
                Err(e) => view.flash = Some(format!("save failed: {e}")),
            },
            KeyCode::Char('v') => {
                let argv = shell::resolve_editor();
                let editor_cmd = argv.join(" ");
                let scroll = view.scroll;
                // Determine the file to edit and the return state.
                let (edit_path, pager_return) = if let Some(ref src) = view.source_path {
                    (
                        src.clone(),
                        PagerReturn::SourceFile {
                            path: src.clone(),
                            scroll,
                        },
                    )
                } else {
                    let title = view.title.clone();
                    match view.write_to_temp() {
                        Ok(tmp) => (
                            tmp.clone(),
                            PagerReturn::TempFile {
                                path: tmp,
                                title,
                                scroll,
                            },
                        ),
                        Err(e) => {
                            self.state.flash_error(format!("write temp: {e}"));
                            return PostAction::None;
                        }
                    }
                };
                self.pending_pager_return = Some(pager_return);
                self.pager = None;
                self.needs_full_repaint = true;
                return sh_c(
                    &format!(
                        "{editor_cmd} {}",
                        shell::shell_quote(&edit_path.display().to_string())
                    ),
                    false,
                );
            }
            KeyCode::Char('?') | KeyCode::F(1) => {
                // Push current pager into history, open pager help on top.
                if let Some(current) = self.pager.take() {
                    self.pager_history.push(current);
                }
                self.pager = Some(crate::ui::pager::build_pager_help(&self.theme));
                self.needs_full_repaint = true;
            }
            _ => {}
        }
        PostAction::None
    }

    // --- Action handlers --------------------------------------------------

    fn apply(&mut self, action: &Action) -> Result<PostAction> {
        spyc_debug!(
            "apply {:?}: cursor={} vt={} grid={}x{} pp={} len={}",
            action,
            self.state.cursor.index,
            self.state.cursor.view_top,
            self.state.last_grid.cols,
            self.state.last_grid.rows,
            self.state.last_grid.items_per_page(),
            self.state.rows.len(),
        );

        // In dir view, `p` (Drop) means "put inventory to cwd".
        if *action == Action::Drop && self.state.view == View::Dir {
            return Ok(self.put_inventory_to_cwd());
        }

        // yp — yank visible pane output to system clipboard.
        if *action == Action::YankPrompt {
            return Ok(self.yank_pane_to_clipboard());
        }
        // yP — yank last typed pane prompt to system clipboard.
        if *action == Action::YankLastPrompt {
            return Ok(self.yank_last_prompt_to_clipboard());
        }
        // ya — yank full pane scrollback to system clipboard.
        if *action == Action::YankScrollback {
            return Ok(self.yank_scrollback_to_clipboard());
        }

        // Try pure-domain dispatch first.
        match self.state.apply(action) {
            state::ApplyResult::Handled => return Ok(PostAction::None),
            state::ApplyResult::Post(post) => return Ok(post),
            state::ApplyResult::OpenPager(req) => {
                let view = match req.lines {
                    state::PagerLines::Plain(lines) => {
                        let mut v = PagerView::new_plain(req.title, lines);
                        v.columns = req.columns;
                        v
                    }
                };
                self.pager = Some(view);
                return Ok(PostAction::None);
            }
            state::ApplyResult::NotHandled => {}
        }

        // Terminal-touching arms that must stay in App.
        match action {
            Action::EnterOrDisplay => {
                let post = self.activate(ActivateIntent::Display);
                self.state.cursor.clamp(self.state.rows.len());
                return Ok(post);
            }
            Action::EnterOrEdit => {
                let post = self.activate(ActivateIntent::Edit);
                self.state.cursor.clamp(self.state.rows.len());
                return Ok(post);
            }
            Action::EditInPane => {
                self.edit_in_pane();
                return Ok(PostAction::None);
            }

            Action::ChmodAdd(mode_char) => {
                let paths = self.state.selection_paths();
                if paths.is_empty() {
                    return Ok(PostAction::None);
                }
                let bits: u32 = match mode_char {
                    'w' => 0o200,
                    'x' => 0o111,
                    _ => return Ok(PostAction::None),
                };
                let count = paths.len();
                self.run_and_flash(
                    fs::ops::chmod_add_bits(&paths, bits),
                    format!("chmod +{mode_char} on {count} item(s)"),
                );
                self.state.refresh_listing();
            }

            Action::Help => self.open_help(),

            Action::ReloadConfig => self.reload_config(),

            Action::TogglePane
            | Action::ResumePane
            | Action::PaneFocusDown
            | Action::PaneFocusUp
            | Action::PaneSendSelection
            | Action::PaneGrow
            | Action::PaneShrink
            | Action::PaneScrollEnter
            | Action::PaneScrollSave
            | Action::PaneNewTab
            | Action::PaneCloseTab
            | Action::PaneTabByIndex(_)
            | Action::PaneNextTab
            | Action::PanePrevTab
            | Action::PaneRenameTab
            | Action::PaneRestartTab
            | Action::PanePipeContent
            | Action::PanePipeInventory
                if matches!(
                    self.state.mode,
                    Mode::Prompting(Prompt {
                        kind: PromptKind::PaneNewTabCmd
                            | PromptKind::PaneNewTabCwd
                            | PromptKind::PaneRenameTab,
                        ..
                    })
                ) =>
            {
                self.cancel_prompt();
                return self.apply(action);
            }

            Action::TogglePane => self.toggle_pane(),
            Action::ResumePane => self.open_pane_tab("claude --resume"),
            Action::PaneFocusDown => self.set_pane_focus(true),
            Action::PaneFocusUp => self.set_pane_focus(false),
            Action::PaneSendSelection => self.send_selection_to_pane(),
            Action::PaneGrow => self.resize_pane(5),
            Action::PaneShrink => self.resize_pane(-5),
            Action::PaneScrollEnter => {
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.active_mut().enter_scroll_mode();
                    self.state.pane_focused = true;
                    self.state
                        .flash_info("scroll: on (j/k nav, s save, Esc exit)");
                }
            }
            Action::PaneScrollSave => {
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    match tabs.active_mut().save_to_file() {
                        Ok(path) => {
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            self.state.flash_info(format!("saved: {name}"));
                        }
                        Err(e) => self.state.flash_info(format!("save error: {e}")),
                    }
                }
            }

            Action::PaneNewTab => self.start_new_tab_prompt(),
            Action::PaneCloseTab => self.close_active_tab(),
            Action::PaneTabByIndex(n) => {
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.switch_to((*n as usize).saturating_sub(1));
                }
            }
            Action::PaneNextTab => {
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.next();
                }
            }
            Action::PanePrevTab => {
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.prev();
                }
            }
            Action::PaneRenameTab => {
                if let Some(tabs) = self.pane_tabs.as_ref() {
                    let current = tabs.active_info().label.clone();
                    let mut p = Prompt::shell(PromptKind::PaneRenameTab, "tab name: ");
                    p.buffer.clone_from(&current);
                    if let Some(ed) = p.editor.as_mut() {
                        ed.set_content(&current);
                    }
                    self.state.mode = Mode::Prompting(p);
                }
            }

            Action::PaneRestartTab => self.restart_active_tab(),

            Action::PanePipeContent => self.pipe_content_to_pane(false),
            Action::PanePipeInventory => self.pipe_content_to_pane(true),

            Action::WorktreeList => self.worktree_list(),

            Action::GitDiff | Action::GitDiffCached => {
                let cached = matches!(action, Action::GitDiffCached);
                if self.state.git_info.is_none() {
                    self.state.flash_error("not in a git repository");
                } else {
                    self.open_git_diff(cached);
                }
            }
            Action::GitBlame => {
                if self.state.git_info.is_none() {
                    self.state.flash_error("not in a git repository");
                } else {
                    self.open_git_blame();
                }
            }

            Action::ShowMemory => self.show_session_info(),
            Action::ColorToggle => {
                self.theme = self.theme.toggled();
                self.state.flash_info(if self.theme.mono {
                    "colors off"
                } else {
                    "colors on"
                });
            }

            Action::ToggleActivity => {
                self.show_activity = !self.show_activity;
                self.state.flash_info(if self.show_activity {
                    "activity monitor on"
                } else {
                    "activity monitor off"
                });
            }

            Action::Redraw => {
                self.needs_full_repaint = true;
            }
            Action::Quit => {
                let now = std::time::Instant::now();
                if self
                    .state
                    .quit_pending
                    .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(2))
                {
                    self.save_session();
                    self.state.should_quit = true;
                } else {
                    self.state.quit_pending = Some(now);
                    let running = self.pane_tabs.as_ref().map_or(0, |tabs| {
                        tabs.tabs().iter().filter(|e| !e.pane.is_closed()).count()
                    });
                    if running > 0 {
                        self.state.flash_info(format!(
                            "{running} running process{} — press again to quit",
                            if running == 1 { "" } else { "es" }
                        ));
                    } else {
                        self.state.flash_info("press again to quit");
                    }
                }
            }

            Action::GotoFile | Action::GotoFileLine => {
                let open_at_line = matches!(action, Action::GotoFileLine);
                self.goto_file_from_pane(open_at_line);
            }

            // All other actions were already handled by `self.state.apply()`.
            _ => {}
        }
        self.state.cursor.clamp(self.state.rows.len());
        Ok(PostAction::None)
    }

    fn activate(&mut self, intent: ActivateIntent) -> PostAction {
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
            return PostAction::None;
        };
        let path = row.path.clone();
        let kind = row.kind;

        // Inventory view: enter drills down to the containing directory and
        // focuses on the item, then continues with the intent on that item.
        if self.state.view == View::Inventory {
            let target_dir = if kind == EntryKind::Dir {
                path.clone()
            } else {
                path.parent()
                    .map_or_else(|| path.clone(), Path::to_path_buf)
            };
            if let Err(e) = self.state.chdir(&target_dir) {
                self.state.flash_error(format!("chdir: {e}"));
                return PostAction::None;
            }
            self.state.view = View::Dir;
            self.state.focus_on_path(&path);
            self.state.rebuild_rows();
            if kind == EntryKind::Dir {
                return PostAction::None;
            }
        }

        if kind == EntryKind::Dir {
            if let Err(e) = self.state.chdir(&path) {
                self.state.flash_error(format!("chdir: {e}"));
            }
            return PostAction::None;
        }

        // File: dispatch based on intent.
        match intent {
            ActivateIntent::Display => {
                if shell::looks_like_text(&path) {
                    // Show text files in the in-app pager: no TUI teardown,
                    // consistent keybindings, keeps the pane running below.
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    match std::fs::read_to_string(&path) {
                        Ok(content) => {
                            let content = expand_tabs(&content);
                            let mut view = if let Some(styled) =
                                crate::ui::syntax::highlight_to_lines(&name, &content)
                            {
                                PagerView::new_styled(name, styled)
                            } else {
                                let lines: Vec<String> =
                                    content.lines().map(String::from).collect();
                                PagerView::new_plain(name, lines)
                            };
                            view.source_path = Some(path.clone());
                            self.pager = Some(view);
                        }
                        Err(e) => self.state.flash_error(format!("read: {e}")),
                    }
                    PostAction::None
                } else {
                    // Binary file: hex dump via pretty-hex.
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    match fs::ops::hex_dump_lines(&path, &self.theme) {
                        Ok(lines) => {
                            let mut view =
                                PagerView::new_plain(format!("{name} [hex]"), Vec::new());
                            // Replace the empty lines with our pre-styled hex lines.
                            view.lines = lines;
                            self.pager = Some(view);
                        }
                        Err(e) => self.state.flash_error(format!("hex: {e}")),
                    }
                    PostAction::None
                }
            }
            ActivateIntent::Edit => {
                let mut argv = shell::resolve_editor();
                if argv.is_empty() {
                    return PostAction::None;
                }
                let program = argv.remove(0);
                argv.push(path.to_string_lossy().into_owned());
                PostAction::Spawn {
                    program,
                    args: argv,
                    pause_after: false,
                }
            }
        }
    }
}

/// Search matcher: prefix match for plain text, glob for anything with
/// `*`, `?`, or `[`. Both modes are case-insensitive.
pub enum Matcher {
    Prefix(String),
    Glob(Pattern),
    /// An invalid glob produced by a malformed pattern. Matches nothing.
    Never,
}

impl Matcher {
    pub fn build(query: &str) -> Self {
        let is_glob = query.contains(['*', '?', '[']);
        let lower = query.to_lowercase();
        if is_glob {
            match Pattern::new(&lower) {
                Ok(p) => Self::Glob(p),
                Err(_) => Self::Never,
            }
        } else {
            Self::Prefix(lower)
        }
    }

    pub fn matches(&self, name: &str) -> bool {
        let lower = name.to_lowercase();
        match self {
            Self::Prefix(q) => lower.starts_with(q),
            Self::Glob(p) => p.matches(&lower),
            Self::Never => false,
        }
    }
}

/// Point the FS watcher at `new_dir`, unwatching the previously-watched
/// listing dir if any. No-op when the watcher failed to initialize or
/// when the same dir is already being watched.
/// Keys we intercept even when the pane is focused.
const fn is_spyc_meta_when_pane_focused(
    key: crossterm::event::KeyEvent,
    resolver_pending: bool,
) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};
    // Continuation of a multi-key spyc sequence must stay with spyc.
    if resolver_pending {
        return true;
    }
    // Raw FS byte or F10 — always the pane toggle.
    if matches!(key.code, KeyCode::F(10) | KeyCode::Char('\x1c')) {
        return true;
    }
    // Ctrl-\ (toggle), Ctrl-W (vim pane prefix), Ctrl-A (screen prefix).
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('\\' | 'w' | 'W' | 'a' | 'A'))
}

fn sync_listing_watch(
    fs_watcher: Option<&mut notify::RecommendedWatcher>,
    active: &mut Option<PathBuf>,
    active_git: &mut Option<PathBuf>,
    new_dir: &Path,
) {
    use notify::{RecursiveMode, Watcher};
    let Some(w) = fs_watcher else {
        return;
    };
    if active.as_deref() != Some(new_dir) {
        if let Some(old) = active.as_ref() {
            let _ = w.unwatch(old);
        }
        if w.watch(new_dir, RecursiveMode::NonRecursive).is_ok() {
            *active = Some(new_dir.to_path_buf());
        } else {
            *active = None;
        }
    }
    // Watch .git/index — this single file is touched by virtually every
    // git operation that changes status (add, commit, checkout, reset,
    // merge, stash, rebase). Watching one file is safe even in huge repos.
    let git_index = new_dir.join(".git/index");
    let want_git = git_index.is_file();
    let have_git = active_git.as_deref() == Some(&git_index);
    if !have_git {
        if let Some(old) = active_git.take() {
            let _ = w.unwatch(&old);
        }
        if want_git && w.watch(&git_index, RecursiveMode::NonRecursive).is_ok() {
            *active_git = Some(git_index);
        }
    }
}

/// Spawn a shell command with stdout+stderr merged into one pipe.
/// The reader thread sends chunks as they arrive so the pager can
/// stream output in real-time. An empty Vec signals EOF.
/// Spawn `cmd` under a PTY for `!` captures. Returns the child handle,
/// a receiver streaming stdout/stderr/`/dev/tty` bytes, and the master
/// writer for forwarding the user's keystrokes to the child.
///
/// PTY (vs. a plain piped `Command`) is what stops sudo / ssh / gpg
/// from writing their password prompts directly to our real terminal:
/// inside the child, `/dev/tty` resolves to the slave PTY, so those
/// bytes flow back through the master and into the pager buffer.
type CaptureHandles = (
    Box<dyn portable_pty::Child + Send + Sync>,
    Box<dyn std::io::Write + Send>,
    std::sync::mpsc::Receiver<Vec<u8>>,
);

fn spawn_capture(cmd: &str) -> Result<CaptureHandles> {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::io::Read as _;
    use std::sync::mpsc;

    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut builder = CommandBuilder::new("sh");
    builder.args(["-c", cmd]);
    builder.env("TERM", "xterm-256color");
    builder.env("CLICOLOR_FORCE", "1");
    builder.env("FORCE_COLOR", "1");
    builder.env("COLORTERM", "truecolor");
    builder.env("COLUMNS", cols.to_string());
    builder.env("LINES", rows.to_string());

    let child = pair.slave.spawn_command(builder)?;
    // Drop the slave handle — once the child exits, the master read
    // side will see EOF, which is how we detect "done".
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
        // Signal EOF with an empty vec.
        let _ = tx.send(Vec::new());
    });

    Ok((child, writer, rx))
}

/// Build a PostAction that runs `cmd` through `sh -c` so shell features
/// (pipes, redirection, `$VAR`) work.
fn sh_c(cmd: &str, pause_after: bool) -> PostAction {
    PostAction::Spawn {
        program: "sh".to_string(),
        args: vec!["-c".to_string(), cmd.to_string()],
        pause_after,
    }
}

/// Hand the tty to a child process, optionally pausing for a keypress
/// afterwards so the user can read the command's output before we repaint
/// over it.
fn run_child_in_foreground(
    terminal: &mut Tui,
    program: &str,
    args: &[String],
    pause_after: bool,
) -> Result<()> {
    use std::io::Write;
    suspend_tui(terminal)?;

    // Ignoring status on purpose: non-zero exits (e.g. less with `q`, or a
    // grep that found nothing) are normal and should not crash spyc.
    let _ = std::process::Command::new(program).args(args).status();

    if pause_after {
        let mut stdout = std::io::stdout();
        write!(stdout, "\n[spyc] press any key to continue…")?;
        stdout.flush()?;
        // We're not in raw mode right now, so read a single byte directly
        // from stdin. Any key (including Enter) unblocks.
        let mut byte = [0u8; 1];
        let _ = std::io::Read::read(&mut std::io::stdin(), &mut byte);
    }

    resume_tui(terminal)?;
    Ok(())
}

pub fn row_from_entry(e: &Entry) -> RowData {
    RowData {
        path: e.path.clone(),
        display: e.display_name(),
        kind: e.kind,
    }
}

const fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

/// Last segment of a path as a displayable String, falling back to the full
/// display if the path has no terminating file-name component (root, `..`).
fn path_basename_display(p: &std::path::Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn user_host_string() -> String {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let host = hostname_best_effort();
    format!("{user}@{host}")
}

fn hostname_best_effort() -> String {
    if let Ok(h) = std::env::var("HOSTNAME") {
        if !h.is_empty() {
            return h;
        }
    }
    if let Ok(out) = std::process::Command::new("hostname").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    "localhost".to_string()
}

/// Expand tab characters to spaces (8-column tab stops).
fn expand_tabs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for ch in s.chars() {
        if ch == '\t' {
            let spaces = 8 - (col % 8);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else if ch == '\n' {
            out.push(ch);
            col = 0;
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
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

/// Strip ANSI escape sequences (CSI, OSC, bracketed paste markers, etc.)
/// from a string, returning only printable content.
fn strip_ansi_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip ESC + whatever follows (CSI sequence, OSC, etc.).
            if let Some(&next) = chars.peek() {
                if next == '[' {
                    chars.next(); // consume '['
                    // Consume until a letter or ~ terminates the sequence.
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c.is_ascii_alphabetic() || c == '~' {
                            break;
                        }
                    }
                } else if next == ']' {
                    // OSC: skip until ST (ESC \ or BEL).
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                } else {
                    chars.next(); // consume the char after ESC
                }
            }
        } else if ch >= ' ' || ch == '\n' || ch == '\t' {
            out.push(ch);
        }
    }
    out.trim().to_string()
}

/// Render an "added" diff for every untracked file under `paths`.
/// Two-step: list with `git ls-files --others --exclude-standard`,
/// then `git diff --no-index /dev/null <file>` per result. Returns the
/// concatenated colored diff bytes (empty if no untracked files match).
fn untracked_diff_bytes(cwd: &std::path::Path, paths: &[String]) -> Vec<u8> {
    let mut args: Vec<&str> = vec!["ls-files", "--others", "--exclude-standard", "--"];
    for s in paths {
        args.push(s);
    }
    let listing = match std::process::Command::new("git")
        .args(&args)
        .current_dir(cwd)
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in listing.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(file) = std::str::from_utf8(line) else {
            continue;
        };
        // --no-index exits 1 when files differ — that's the success
        // case for us. Just take whatever it printed.
        if let Ok(o) = std::process::Command::new("git")
            .args([
                "diff",
                "--no-index",
                "--color=always",
                "--",
                "/dev/null",
                file,
            ])
            .current_dir(cwd)
            .output()
        {
            out.extend(o.stdout);
        }
    }
    out
}

#[cfg(test)]
mod layout_tests {
    use super::{App, StatusPosition};
    use ratatui::layout::Rect;

    fn area(w: u16, h: u16) -> Rect {
        Rect::new(0, 0, w, h)
    }

    #[test]
    fn no_pane_top_status_at_row_0() {
        let l = App::compute_layout(area(80, 24), false, 50, StatusPosition::Top);
        assert_eq!(l.status.y, 0);
        assert_eq!(l.list.y, 1);
        assert_eq!(l.prompt.y, 23);
    }

    #[test]
    fn no_pane_bottom_status_at_last_row() {
        let l = App::compute_layout(area(80, 24), false, 50, StatusPosition::Bottom);
        assert_eq!(l.list.y, 0);
        assert_eq!(l.prompt.y, 22);
        assert_eq!(l.status.y, 23);
    }

    #[test]
    fn pane_open_top_status_above_list() {
        let l = App::compute_layout(area(80, 24), true, 50, StatusPosition::Top);
        assert_eq!(l.status.y, 0);
        assert!(l.list.y > l.status.y);
        let pane = l.pane.unwrap();
        let div = l.divider.unwrap();
        assert_eq!(div.y + 1, pane.y);
        // prompt sits in the top region, above the divider.
        assert!(l.prompt.y < div.y);
    }

    #[test]
    fn pane_open_bottom_status_below_pane() {
        let l = App::compute_layout(area(80, 24), true, 50, StatusPosition::Bottom);
        let pane = l.pane.unwrap();
        let div = l.divider.unwrap();
        assert_eq!(l.list.y, 0);
        assert_eq!(l.list.y + l.list.height, div.y);
        assert_eq!(div.y + 1, pane.y);
        // prompt one above status, both at the very bottom.
        assert_eq!(l.prompt.y, 22);
        assert_eq!(l.status.y, 23);
        // pane ends at the row above prompt.
        assert!(pane.y + pane.height <= l.prompt.y);
    }
}
