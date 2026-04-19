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

use crate::config::Config;
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
/// Background capture for a `!` command. The child runs with piped
/// stdout; a reader thread feeds bytes into the channel. Ctrl+C from
/// the user kills the child.
struct PendingCapture {
    child: std::process::Child,
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
    cursor_blink_on: bool,
    cursor_blink_instant: std::time::Instant,
    /// Path to the `.spyc-context.json` file (project root).
    /// Written each loop iteration so the MCP server can read it.
    context_path: PathBuf,
    /// MCP HTTP server port (0 = not running).
    mcp_port: u16,
}

/// Internal per-item record used to build ListView rows each frame.
pub struct RowData {
    pub path: PathBuf,
    pub display: String,
    pub kind: EntryKind,
}

impl App {
    pub fn new(resume: bool) -> Self {
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
        };
        let context_path = crate::context::context_path(&app_state.start_dir);
        // Start the MCP HTTP server so Claude CLI can connect via
        // --mcp-config. Port 0 = OS assigns a free port.
        let mcp_port = crate::mcp::start_http_server(context_path.clone())
            .unwrap_or_else(|e| {
                spyc_debug!("MCP HTTP server failed to start: {e}");
                0
            });
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
            cursor_blink_on: true,
            cursor_blink_instant: std::time::Instant::now(),
            context_path,
            mcp_port,
        };
        app.state.rebuild_rows();
        if let Some(msg) = load_note {
            app.state.flash_info(msg);
        }
        if resume {
            app.show_session_picker();
        }
        app
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
        }
    }

    /// Write the context file (best-effort, errors are silently ignored).
    fn write_context(&self) {
        let ctx = self.snapshot_context();
        let _ = crate::context::write_context_file(&self.context_path, &ctx);
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
                self.state.flash_info(format!("reloaded {count} config file(s)"));
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

        while !self.state.should_quit {
            // One-shot full repaint after a pane or overlay closes (or any
            // other event that leaves ratatui's diff buffer stale).
            // Also force repaint when the pager opens while a pane exists,
            // because the pane stops rendering and its stale cells need clearing.
            if self.pager.is_some() && self.pane_tabs.is_some() && !self.pager_was_open {
                self.needs_full_repaint = true;
            }
            self.pager_was_open = self.pager.is_some();
            if self.needs_full_repaint {
                self.needs_full_repaint = false;
                terminal.clear()?;
            }
            // Tick the pane cursor blink (~530ms cycle).
            if self.cursor_blink_instant.elapsed() >= Duration::from_millis(530) {
                self.cursor_blink_on = !self.cursor_blink_on;
                self.cursor_blink_instant = std::time::Instant::now();
            }
            terminal.draw(|frame| self.render(frame))?;

            // Mark exited tabs so the user can read their output.
            // Tabs stay open until the user explicitly closes (^W x).
            if let Some(tabs) = self.pane_tabs.as_mut() {
                tabs.mark_exited();
            }
            // pending_overlay_close is no longer used — the overlay stays
            // visible until Enter via overlay_awaiting_dismiss.
            let _ = self.pending_overlay_close;

            // Check if a background `!` capture has finished.
            // Stream captured command output into the pager in real-time.
            if let Some(capture) = &mut self.pending_capture {
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
                    let timer_title =
                        format!("\u{23f3} {} — running... ({elapsed}s)", capture.title);
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
                        Ok(s) => format!(
                            "exit {}",
                            s.code().map_or_else(|| "?".to_string(), |c| c.to_string())
                        ),
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

            // Drain any pending watcher events. Refresh listing / reload
            // config at most once per poll iteration — natural debounce.
            let mut needs_reload = false;
            let mut needs_refresh = false;
            while let Ok(result) = rx.try_recv() {
                if let Ok(ev) = result {
                    for p in &ev.paths {
                        if self.is_config_path(p) {
                            needs_reload = true;
                        }
                        if self.is_listing_path(p) {
                            needs_refresh = true;
                        }
                    }
                }
            }
            if needs_reload {
                self.reload_config();
            }
            if needs_refresh {
                self.state.refresh_listing();
            }

            // Short poll while the pane or a capture is active so output
            // streams without lag. Long poll otherwise to keep CPU near
            // zero when the user is just browsing files.
            let poll_ms = if self.pane_tabs.is_some() || self.pending_capture.is_some() {
                16
            } else {
                250
            };
            if event::poll(Duration::from_millis(poll_ms))? {
                match event::read()? {
                    Event::Key(key)
                        if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                    {
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
                        } else if let Some(pane) = self.pane_tabs.as_mut().map(PaneTabs::active_mut)
                        {
                            // Wrap in bracketed paste so the child app (e.g. claude)
                            // receives the block as a single paste, not line-by-line.
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
                            let layout = Self::compute_layout(area, true, self.state.pane_height_pct);
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
                    }
                    _ => {}
                }
            }

            // chdir may have happened in the tick just finished — keep the
            // watcher pointed at the current listing dir.
            sync_listing_watch(
                fs_watcher.as_mut(),
                &mut watched_listing,
                &mut watched_git,
                &self.state.listing.dir,
            );

            // Update the MCP context file so external consumers (e.g.
            // `spyc --mcp`) see the latest state.
            self.write_context();
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
    fn compute_layout(area: ratatui::layout::Rect, pane_open: bool, pane_pct: u16) -> FrameLayout {
        use ratatui::layout::Rect;
        let w = area.width;
        let h = area.height;

        if !pane_open {
            let status = Rect {
                x: area.x,
                y: area.y,
                width: w,
                height: 1.min(h),
            };
            let list = Rect {
                x: area.x,
                y: area.y + status.height,
                width: w,
                height: h.saturating_sub(2),
            };
            let prompt = Rect {
                x: area.x,
                y: area.y + h.saturating_sub(1),
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

        // With pane: [status + list + prompt] (top unit) + divider + pane.
        // Divider eats one row from whatever height is available.
        let usable = h.saturating_sub(1); // minus divider
        let pane_h = (u32::from(usable) * u32::from(pane_pct) / 100) as u16;
        let top_h = usable.saturating_sub(pane_h);

        // Inside the top region: status(1) + list(top_h - 2) + prompt(1).
        // If top_h is too small (< 2), the list simply collapses to 0.
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
    fn render_pane_status_line(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
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

        // Tab indicators: ─[1*] claude ─[2+] bash
        if let Some(tabs) = &self.pane_tabs {
            for (i, entry) in tabs.tabs().iter().enumerate() {
                let is_active = i == tabs.active_index();
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

            // CWD of active tab.
            let cwd_display = {
                let cwd = &tabs.active_info().cwd;
                let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
                match home {
                    Some(h) if cwd.starts_with(&h) => {
                        format!("~/{}", cwd.strip_prefix(&h).unwrap().display())
                    }
                    _ => cwd.display().to_string(),
                }
            };
            // "── ~/path"
            let cwd_prefix = "── ";
            let avail = width.saturating_sub(used + 12); // leave room for [SCROLL] + trailing rule
            if avail > 4 {
                let truncated = if cwd_display.len() > avail {
                    format!("…{}", &cwd_display[cwd_display.len() - avail + 1..])
                } else {
                    cwd_display
                };
                let cwd_fragment = format!("{cwd_prefix}{truncated} ");
                used += cwd_fragment.len();
                spans.push(Span::styled(cwd_fragment, inactive_tab_style));
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
        let layout =
            Self::compute_layout(frame_area, self.pane_tabs.is_some(), self.state.pane_height_pct);

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
                    blink_on: self.cursor_blink_on,
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
                        blink_on: false,
                    },
                    rect,
                );
            }
            return;
        }

        let (path, suffix) = self.header_parts();
        StatusBar {
            user_host: &self.state.user_host,
            path: &path,
            suffix: &suffix,
            git_info: self.state.git_info.as_deref(),
            theme: &self.theme,
        }
        .render(frame, layout.status);

        let rows = self.build_rows();
        let list_focused = !self.state.pane_focused;
        // Stabilize view_top ↔ grid.  The grid depends on view_top (different
        // entries have different name lengths → different column count →
        // different items_per_page), and view_top depends on the grid.
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
                    rows: &rows,
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
                        rows: &rows,
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

        frame.render_widget(
            ListView {
                rows: &rows,
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
                    blink_on: self.cursor_blink_on,
                },
                rect,
            );
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
                format!("⏳ running: {}  (^C to cancel)", capture.cmd_display),
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
    }

    fn header_parts(&self) -> (String, String) {
        match self.state.view {
            View::Dir => (self.state.listing.dir.display().to_string(), {
                let filter_tag = match &self.state.temp_filter {
                    Some(f) if f == "!" => " limit:picks".to_string(),
                    Some(f) => format!(" limit:{f}"),
                    None => String::new(),
                };
                format!(
                    "[picks:{} inv:{} m1:{} m2:{}{}]",
                    self.state.picks.len(),
                    self.state.inventory.len(),
                    on_off(self.state.masks.mask1.enabled),
                    on_off(self.state.masks.mask2.enabled),
                    filter_tag,
                )
            }),
            View::Inventory => (
                "<INVENTORY>".to_string(),
                format!("[{} items]  (x: remove, ESC/i: return, z: clear)", self.state.inventory.len()),
            ),
        }
    }

    fn build_rows(&self) -> Vec<Row> {
        use crate::ui::list_view::GitFileStatus;
        self.state.rows
            .iter()
            .map(|rd| {
                let git_status = self.state
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

        // While a `!` capture is running, Ctrl+C kills it.
        // The pager is already open with streamed output.
        if let Some(capture) = &mut self.pending_capture {
            if matches!(key.code, KeyCode::Char('c' | 'C'))
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
            if let Some(tabs) = self.pane_tabs.as_mut() {
                let _ = tabs.active_mut().send_key(key);
            }
            return Ok(PostAction::None);
        }
        if matches!(self.state.mode, Mode::Prompting(_)) {
            return Ok(self.handle_prompt_key(key));
        }
        // Inventory view: ESC returns to dir view, x/d removes cursor item.
        if self.state.view == View::Inventory {
            match key.code {
                KeyCode::Esc => {
                    self.state.toggle_inventory_view();
                    return Ok(PostAction::None);
                }
                KeyCode::Char('x') | KeyCode::Char('d') => {
                    self.state.drop_cursor();
                    return Ok(PostAction::None);
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
        // Destructive-confirm prompts are single-key: `y` / `Y` proceeds
        // immediately, anything else cancels. No Enter needed.
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
                let Mode::Prompting(p) = std::mem::replace(&mut self.state.mode, Mode::Normal) else {
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
            EditResult::Continue => {}
        }
        PostAction::None
    }

    /// Close the prompt without dispatching. For a Search prompt, also
    /// restore the cursor position that was saved when `/` was pressed.
    fn cancel_prompt(&mut self) {
        let Mode::Prompting(p) = std::mem::replace(&mut self.state.mode, Mode::Normal) else {
            return;
        };
        if let PromptKind::Search { saved_cursor } = p.kind {
            self.state.cursor.index = saved_cursor;
            self.state.cursor.clamp(self.state.rows.len());
        }
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
                    let expanded = crate::shell::expand_percent(&cmd, &self.state.selection_paths());
                    let title = format!("! {cmd}");
                    match spawn_capture(&expanded) {
                        Ok((child, rx)) => {
                            let mut view = PagerView::new_plain(
                                format!("\u{23f3} {title} — running... (0s)"),
                                Vec::new(),
                            );
                            view.streaming = true;
                            self.pager = Some(view);
                            self.pending_capture = Some(PendingCapture {
                                child,
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
                Ok((child, rx)) => {
                    let mut view = PagerView::new_plain(
                        format!("\u{23f3} {title} — running... (0s)"),
                        Vec::new(),
                    );
                    view.streaming = true;
                    self.pager = Some(view);
                    self.pending_capture = Some(PendingCapture {
                        child,
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
                self.state.flash_info(format!("buffer ←{}", self.pager_history.back_len()));
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
                    Ok((child, rx)) => {
                        let cmd_display = prompt.buffer.clone();
                        let mut view = PagerView::new_plain(
                            format!("\u{23f3} {title} — running... (0s)"),
                            Vec::new(),
                        );
                        view.streaming = true;
                        self.pager = Some(view);
                        self.pending_capture = Some(PendingCapture {
                            child,
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
                        self.state.listing.dir.clone()
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
    fn open_pane_tab(&mut self, cmd: &str) {
        self.open_pane_tab_in(cmd, &self.state.listing.dir.clone());
    }

    fn open_pane_tab_in(&mut self, cmd: &str, cwd: &std::path::Path) {
        // Inject --mcp-config when spawning Claude so it connects to
        // our HTTP MCP server for context awareness.
        let cmd = if self.mcp_port > 0 && Self::is_claude_command(cmd) {
            let config = crate::mcp::mcp_config_json(self.mcp_port);
            let full = format!("{cmd} --mcp-config '{config}'");
            spyc_debug!("pane cmd: {full}");
            full
        } else {
            cmd.to_string()
        };
        let (rows, cols) = Self::pane_spawn_size(self.state.pane_height_pct);
        match Pane::spawn(&cmd, rows, cols, cwd) {
            Ok(p) => {
                self.state.pane_focused = true;
                self.state.flash_info(format!("pane: {cmd} (^W k for list)"));
                let entry = TabEntry {
                    pane: p,
                    info: TabInfo::new(cmd, cwd),
                };
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.push(entry);
                } else {
                    self.pane_tabs = Some(PaneTabs::new(entry));
                }
            }
            Err(e) => self.state.flash_error(format!("pane spawn failed: {e}")),
        }
    }

    /// Does this command look like it's launching Claude CLI?
    fn is_claude_command(cmd: &str) -> bool {
        let first = cmd.split_whitespace().next().unwrap_or("");
        first == "claude" || first.ends_with("/claude")
    }

    /// ^W n — start the two-step prompt for a new pane tab.
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

    /// ^W j / ^W k — set keyboard focus directionally (no wrap).
    fn set_pane_focus(&mut self, want_pane: bool) {
        if self.pane_tabs.is_none() {
            return;
        }
        if self.state.pane_focused == want_pane {
            return; // already there — no-op
        }
        self.state.pane_focused = want_pane;
        self.state.flash_info(if self.state.pane_focused {
            "focus: pane"
        } else {
            "focus: list"
        });
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
                    self.pane_tabs.as_mut().unwrap().active_mut().scroll_to_top();
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
            Ok(()) => self.state.flash_info(format!("sent {count} path(s) to pane")),
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
        let paths: Vec<PathBuf> = if use_inventory {
            self.state.inventory.paths().cloned().collect()
        } else {
            self.state.selection_paths()
                .into_iter()
                .map(Path::to_path_buf)
                .collect()
        };
        if paths.is_empty() {
            self.state.flash_error(if use_inventory {
                "inventory is empty"
            } else {
                "nothing selected"
            });
            return;
        }
        // Read file contents and build payload.
        let mut payload = String::new();
        let mut count = 0usize;
        let mut skipped = 0usize;
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
        if count == 0 {
            self.state.flash_error("no readable text files in selection");
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
    fn open_git_diff(&mut self, cached: bool) {
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            return;
        }
        let mut args = vec!["diff", "--color=always"];
        if cached {
            args.push("--cached");
        }
        args.push("--");
        let path_strings: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        for s in &path_strings {
            args.push(s);
        }
        match std::process::Command::new("git")
            .args(&args)
            .current_dir(&self.state.listing.dir)
            .output()
        {
            Ok(out) => {
                if out.stdout.is_empty() {
                    let label = if cached { "staged" } else { "unstaged" };
                    self.state.flash_info(format!("no {label} changes"));
                } else {
                    let label = if cached {
                        "git diff --cached"
                    } else {
                        "git diff"
                    };
                    self.pager = Some(pager::PagerView::new_ansi(label, &out.stdout));
                }
            }
            Err(e) => self.state.flash_error(format!("git diff: {e}")),
        }
    }

    // ---- Path references (M13) ------------------------------------------------

    /// `gf` / `gF` — scan the active pane's visible output for a file path
    /// reference, navigate the file list there, and optionally open the
    /// pager at the referenced line.
    fn goto_file_from_pane(&mut self, open_at_line: bool) {
        let Some(tabs) = self.pane_tabs.as_ref() else {
            self.state.flash_error("no pane open");
            return;
        };
        let lines = tabs.active().visible_lines();
        // Also try resolving against the spyc cwd (project root), not just
        // the pane tab's cwd — Claude often prints paths relative to the
        // project root regardless of the shell's cwd.
        let pane_cwd = tabs.active_info().cwd.clone();
        let spyc_cwd = self.state.listing.dir.clone();

        // Debug: dump visible lines to the debug log so we can see what
        // the vt100 screen actually contains.
        spyc_debug!("gf: {} lines from pane, pane_cwd={}, spyc_cwd={}", lines.len(), pane_cwd.display(), spyc_cwd.display());
        for (i, line) in lines.iter().enumerate() {
            if !line.trim().is_empty() {
                spyc_debug!("gf line[{i}]: {:?}", line);
            }
        }

        let pathref = crate::pane::pathref::extract_path_ref(&lines, &pane_cwd)
            .or_else(|| {
                (pane_cwd != spyc_cwd)
                    .then(|| crate::pane::pathref::extract_path_ref(&lines, &spyc_cwd))
                    .flatten()
            });

        let Some(pathref) = pathref else {
            self.state.flash_error("no path reference found in pane output");
            return;
        };

        spyc_debug!("gf: found path={}, line={:?}", pathref.path.display(), pathref.line);

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
            let name = path
                .file_name()
                .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());

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
                path.file_name()
                    .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned()),
                ln
            ));
        }
    }

    // ---- Session management --------------------------------------------------

    fn save_session(&self) {
        use crate::state::sessions::{SavedTab, Session};
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let epoch_secs = now.as_secs();
        let id = now.as_millis() as u64;

        let tabs: Vec<SavedTab> = self
            .pane_tabs
            .as_ref()
            .map(|pt| {
                pt.tabs()
                    .iter()
                    .map(|t| {
                        let (claude_session_id, claude_session_name) =
                            if Self::is_claude_command(&t.info.command) {
                                match crate::state::sessions::find_claude_session(&t.info.cwd) {
                                    Some(info) => (Some(info.session_id), info.name),
                                    None => (None, None),
                                }
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
        };
        let _ = crate::state::sessions::save_session(&session);
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
                            Some(name) => Some(format!("{} ({})", name, short_id)),
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
                format!(
                    "  [{}]  {:<20} {}{}{}",
                    i + 1,
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
        // Restore working directory.
        if session.cwd.is_dir() {
            if let Err(e) = self.state.chdir(&session.cwd) {
                self.state.flash_error(format!("session chdir: {e}"));
                return;
            }
        } else {
            self.state.flash_error(format!("session dir gone: {}", session.cwd.display()));
            return;
        }
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
                self.state.pending_worktrees = Some(worktrees.iter().map(|w| w.path.clone()).collect());
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
            None => self.state.flash_error("not in a git repository (or no worktrees)"),
        }
    }

    /// Compute the (rows, cols) the bottom pane will occupy.
    fn pane_spawn_size(height_pct: u16) -> (u16, u16) {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let area = ratatui::layout::Rect::new(0, 0, cols, rows);
        let layout = Self::compute_layout(area, true, height_pct);
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
        lines.push(format!("pid      : {}", std::process::id()));
        lines.push(format!("cwd      : {}", self.state.listing.dir.display()));
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
                lines.push(format!("  {}", src.display()));
            }
        }
        self.pager = Some(PagerView::new_plain("session info", lines));
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
                    let expanded = crate::shell::expand_percent(&cmd, &self.state.selection_paths());
                    let title = format!("! {cmd}");
                    match spawn_capture(&expanded) {
                        Ok((child, rx)) => {
                            let mut cview = PagerView::new_plain(
                                format!("\u{23f3} {title} — running... (0s)"),
                                Vec::new(),
                            );
                            cview.streaming = true;
                            self.pager = Some(cview);
                            self.pending_capture = Some(PendingCapture {
                                child,
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
                EditResult::Continue => {}
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
            KeyCode::Char('l') => view.toggle_whitespace(),
            KeyCode::Char('f') => view.toggle_full_width(),
            KeyCode::Char('y') => match view.yank_to_clipboard() {
                Ok(()) => self.state.flash_info("yanked to clipboard"),
                Err(e) => self.state.flash_error(format!("yank failed: {e}")),
            },
            KeyCode::Char('s') if view.saveable => match view.save_to_file() {
                Ok(path) => self.state.flash_info(format!("saved: {}", path.display())),
                Err(e) => self.state.flash_error(format!("save failed: {e}")),
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

            Action::Help => {
                let lines = help::build_lines(&self.theme, &self.state.user_keymap);
                let mut view = pager::PagerView::new_styled("spyc — key bindings", lines);
                view.columns = 2;
                self.pager = Some(view);
            }

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
                    self.state.flash_info("scroll: on (j/k nav, s save, Esc exit)");
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

            Action::ShowMemory => self.show_session_info(),
            Action::ColorToggle => {
                self.theme = self.theme.toggled();
                self.state.flash_info(if self.theme.mono {
                    "colors off"
                } else {
                    "colors on"
                });
            }

            Action::Redraw => {
                self.needs_full_repaint = true;
            }
            Action::Quit => {
                let now = std::time::Instant::now();
                if self.state
                    .quit_pending
                    .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(2))
                {
                    self.save_session();
                    self.state.should_quit = true;
                } else {
                    self.state.quit_pending = Some(now);
                    self.state.flash_info("press again to quit");
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
    // Ctrl-\ (toggle) and Ctrl-W (pane-command prefix).
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('\\' | 'w' | 'W'))
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
fn spawn_capture(cmd: &str) -> Result<(std::process::Child, std::sync::mpsc::Receiver<Vec<u8>>)> {
    use std::io::Read as _;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;

    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    // Redirect stderr into stdout (`2>&1`) so all output streams together.
    let merged_cmd = format!("({cmd}) 2>&1");
    let mut child = Command::new("sh")
        .args(["-c", &merged_cmd])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("CLICOLOR_FORCE", "1")
        .env("FORCE_COLOR", "1")
        .env("COLORTERM", "truecolor")
        .env("COLUMNS", cols.to_string())
        .env("LINES", rows.to_string())
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stdout"))?;

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = stdout;
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

    Ok((child, rx))
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

pub fn detect_kind(p: &Path) -> EntryKind {
    match std::fs::symlink_metadata(p) {
        Ok(md) if md.is_dir() => EntryKind::Dir,
        Ok(md) if md.file_type().is_symlink() => EntryKind::Symlink,
        Ok(md) if md.is_file() => EntryKind::File,
        _ => EntryKind::Other,
    }
}

const fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
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
