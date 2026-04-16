//! Top-level application state and event loop.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use glob::Pattern;
use ratatui::Frame;

use crate::config::Config;
use crate::fs::{self, Entry, EntryKind, Listing};
use crate::keymap::{Action, BoundAction, Resolver, ResolverOutcome, UserKeymap};
use crate::pane::{Pane, PaneTabs, PaneWidget, TabEntry, TabInfo};
use crate::shell;
use crate::state::{Cursor, History, IgnoreMasks, Inventory, Mark, Marks, Picks};
use crate::ui::line_edit::LineEditor;
use crate::ui::{
    help,
    list_view::{Grid, ListView, Row},
    pager::{self, PagerView},
    prompt::PromptLine,
    status::StatusBar,
    theme::Theme,
};
use crate::{resume_tui, suspend_tui, Tui};

/// Precomputed rects for the current frame. Built by `App::compute_layout`.
/// Background capture for a `!` command. The child runs with piped
/// stdout; a reader thread feeds bytes into the channel. Ctrl+C from
/// the user kills the child.
struct PendingCapture {
    child: std::process::Child,
    output_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    title: String,
    cmd_display: String,
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

/// Which collection the user is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Dir,
    Inventory,
}

/// Input mode: normal key bindings or a one-line text prompt.
enum Mode {
    Normal,
    Prompting(Prompt),
}

#[derive(Debug, Clone, Copy)]
enum ActivateIntent {
    Display, // $PAGER on text files
    Edit,    // $EDITOR
}

#[derive(Debug, Clone)]
struct FlashMessage {
    text: String,
    kind: FlashKind,
}

#[derive(Debug, Clone, Copy)]
enum FlashKind {
    Info,
    Error,
}

/// State for returning to the pager after `v` (edit) exits.
enum PagerReturn {
    /// Buffer content: reload from this temp file, then delete it.
    TempFile { path: PathBuf, title: String, scroll: u16 },
    /// On-disk file: reopen from the original path.
    SourceFile { path: PathBuf, scroll: u16 },
}

struct Prompt {
    kind: PromptKind,
    prefix: String,
    buffer: String,
    /// When set, this prompt uses the vi line editor with history.
    #[allow(dead_code)]
    editor: Option<crate::ui::line_edit::LineEditor>,
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

enum PromptKind {
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
}

#[allow(clippy::struct_excessive_bools)]
pub struct App {
    listing: Listing,
    picks: Picks,
    inventory: Inventory,
    marks: Marks,
    masks: IgnoreMasks,
    view: View,
    cursor: Cursor,
    resolver: Resolver,
    user_keymap: UserKeymap,
    config: Config,
    theme: Theme,
    mode: Mode,
    /// When true, the key-bindings overlay is drawn on top of everything
    /// and the next keypress dismisses it.
    /// When set, a scrollable text pager sits on top of the file list.
    /// `q` / `Esc` close it; `j` / `k` / `gg` / `G` scroll.
    pager: Option<PagerView>,
    /// Tabbed pty-hosted subprocesses shown as a horizontal split under the list.
    pane_tabs: Option<PaneTabs>,
    /// When set, an interactive `!` command has taken over the top pane.
    /// cspy's list/status/prompt are hidden; this pty renders instead.
    /// When the subprocess exits, cspy's file listing is restored and
    /// the bottom pane (claude) is completely unaffected.
    top_overlay: Option<Pane>,
    /// The overlay subprocess has exited but we're holding the screen
    /// so the user can read the final output. Enter dismisses.
    overlay_awaiting_dismiss: bool,
    pending_overlay_close: bool,
    /// A `!` command running in the background with piped output. The
    /// event loop stays alive so Ctrl+C can kill the child.
    pending_capture: Option<PendingCapture>,
    /// Whether the pane (vs the file list) is the current keyboard focus.
    /// Most keys are forwarded to the pane when this is true, but the
    /// Ctrl-W prefix and Ctrl-\\ / F10 toggle are always caught by cspy.
    pane_focused: bool,
    /// The bottom pane's share of the middle rect, in percent. Resized
    /// by `^W +` / `^W -`.
    pane_height_pct: u16,
    /// Stashed command for the two-step new-tab prompt flow.
    pending_new_tab_cmd: Option<String>,
    /// Worktree paths for the `W l` picker. Digit keys 1-9 select.
    pending_worktrees: Option<Vec<PathBuf>>,
    /// When `v` is pressed in the pager, stash info to restore it after
    /// the editor exits. `Some(path)` for temp-file buffers (reload on
    /// return); `None` for on-disk files (just reopen pager normally).
    pending_pager_return: Option<PagerReturn>,
    /// The directory cspy was launched in — `` ` `` jumps here (project root).
    start_dir: PathBuf,
    /// Directory before the last chdir — `''` jumps back here (like `cd -`).
    prev_dir: Option<PathBuf>,
    /// Most recent search term; `n` / `N` use this.
    last_search: Option<String>,
    /// Timestamp of the first quit press. Must press again within 2s to
    /// actually quit — prevents murdering a claude session by accident.
    quit_pending: Option<std::time::Instant>,
    /// Shared command history for `!` / `;` prompts — persisted.
    #[allow(dead_code)]
    history: History,
    pane_history: History,
    /// Transient message shown in the prompt row when no prompt is active.
    /// Cleared on the next keypress so it doesn't linger after you've read it.
    flash: Option<FlashMessage>,
    /// Set when a pane or overlay closes — triggers one `terminal.clear()`
    /// so ratatui repaints every cell instead of diffing against stale state.
    needs_full_repaint: bool,
    user_host: String,
    /// Cached git branch + dirty flag, refreshed on chdir.
    git_info: Option<String>,
    /// Pane cursor blink state: toggles every ~530ms for a visible blink.
    cursor_blink_on: bool,
    cursor_blink_instant: std::time::Instant,
    should_quit: bool,

    /// Rebuilt on chdir / mask toggle / inventory change.
    rows: Vec<RowData>,
    /// Geometry of the last rendered grid. Motion uses this.
    last_grid: Grid,
}

/// Internal per-item record used to build ListView rows each frame.
struct RowData {
    path: PathBuf,
    display: String,
    kind: EntryKind,
}

impl App {
    pub fn new() -> Result<Self> {
        let (cwd, start_error) = match std::env::current_dir() {
            Ok(d) => (d, None),
            Err(_) => {
                // cwd not accessible — fall back to $HOME.
                let home = std::env::var("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from("/tmp"));
                let _ = std::env::set_current_dir(&home);
                (home, Some("cwd not accessible, started in $HOME".to_string()))
            }
        };
        let (listing, start_error) = match Listing::read(&cwd) {
            Ok(l) => (l, start_error),
            Err(e) => (
                Listing::empty(cwd.clone()),
                Some(start_error.unwrap_or_default() + &format!("{e}")),
            ),
        };
        let git_info = crate::sysinfo::git_status(&cwd);
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
        let mut app = Self {
            listing,
            picks: Picks::new(),
            inventory: Inventory::new(),
            marks: Marks::load(),
            masks: {
                let mut m = IgnoreMasks::default();
                m.apply_config(&config.ignore_masks);
                m
            },
            view: View::Dir,
            cursor: Cursor::new(),
            resolver: Resolver::new(),
            user_keymap,
            config,
            theme,
            mode: Mode::Normal,
            pager: None,
            pane_tabs: None,
            top_overlay: None,
            overlay_awaiting_dismiss: false,
            pending_overlay_close: false,
            pending_capture: None,
            pane_focused: false,
            // cspy (top) = 30%, pane (bottom) = 70%. Resize with `^W +/-`.
            pane_height_pct: 70,
            pending_new_tab_cmd: None,
            pending_worktrees: None,
            pending_pager_return: None,
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
            needs_full_repaint: false,
            user_host: user_host_string(),
            git_info,
            cursor_blink_on: true,
            cursor_blink_instant: std::time::Instant::now(),
            should_quit: false,
            rows: Vec::new(),
            last_grid: Grid {
                cols: 1,
                rows: 1,
                col_widths: vec![20],
            },
        };
        app.rebuild_rows();
        if let Some(msg) = load_note {
            app.flash_info(msg);
        }
        Ok(app)
    }

    /// Reload `.cspyrc.toml` and rebuild the user keymap. Leaves the old
    /// config in place on failure and flashes the error.
    pub fn reload_config(&mut self) {
        match Config::load_default(&self.listing.dir) {
            Ok(new_config) => {
                self.user_keymap = UserKeymap::from_bindings(new_config.bindings.clone());
                self.theme = Theme::default().with_overrides(&new_config.colors);
                // Reset to built-in mask defaults first, then apply config
                // overrides — so removing `[[ignore_masks]]` entries from
                // the rc file reverts the group to defaults on reload.
                self.masks = IgnoreMasks::default();
                self.masks.apply_config(&new_config.ignore_masks);
                let count = new_config.sources.len();
                self.config = new_config;
                self.rebuild_rows();
                self.flash_info(format!("reloaded {count} config file(s)"));
            }
            Err(e) => self.flash_error(format!("config error: {e}")),
        }
    }

    /// Candidate config paths — used by the file watcher. We watch the
    /// directories holding these even when the files don't exist yet so
    /// that `touch ~/.cspyrc.toml` picks up immediately.
    fn candidate_config_paths(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(home).join(".cspyrc.toml"));
        }
        out.push(self.listing.dir.join(".cspyrc.toml"));
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
        sync_listing_watch(fs_watcher.as_mut(), &mut watched_listing, &self.listing.dir);

        while !self.should_quit {
            // One-shot full repaint after a pane or overlay closes (or any
            // other event that leaves ratatui's diff buffer stale).
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

            // Auto-remove tabs whose subprocess has exited. If the last
            // tab closes, tear down the entire pane area.
            if let Some(tabs) = self.pane_tabs.as_mut() {
                if !tabs.remove_closed() {
                    self.pane_tabs = None;
                    self.pane_focused = false;
                    self.needs_full_repaint = true;
                    self.flash_info("pane: last tab exited");
                }
            }
            // pending_overlay_close is no longer used — the overlay stays
            // visible until Enter via overlay_awaiting_dismiss.
            let _ = self.pending_overlay_close;

            // Check if a background `!` capture has finished.
            if let Some(capture) = &mut self.pending_capture {
                if let Ok(stdout) = capture.output_rx.try_recv() {
                    // Collect stderr too.
                    let mut bytes = stdout;
                    if let Some(mut stderr) = capture.child.stderr.take() {
                        use std::io::Read as _;
                        let mut err = Vec::new();
                        let _ = stderr.read_to_end(&mut err);
                        if !err.is_empty() {
                            if !bytes.is_empty() {
                                bytes.push(b'\n');
                            }
                            bytes.extend_from_slice(&err);
                        }
                    }
                    let status = capture.child.wait();
                    let exit_info = match status {
                        Ok(s) => {
                            if s.success() {
                                "exit 0".to_string()
                            } else {
                                format!(
                                    "exit {}",
                                    s.code().map_or_else(|| "?".to_string(), |c| c.to_string())
                                )
                            }
                        }
                        Err(e) => format!("error: {e}"),
                    };
                    let title = format!("{} — {exit_info}", capture.title);
                    self.pending_capture = None;
                    self.pager = Some(PagerView::new_ansi(title, &bytes));
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
                self.refresh_listing();
            }

            // Short poll while the pane is active so child output
            // (and cursor motion in e.g. claude's visual mode) feels
            // snappy. Long poll otherwise to keep CPU near zero when
            // the user is just browsing files.
            let poll_ms = if self.pane_tabs.is_some() { 16 } else { 250 };
            if event::poll(Duration::from_millis(poll_ms))? {
                match event::read()? {
                    Event::Key(key) => {
                        if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat {
                            let post = self.handle_key(key)?;
                            if let PostAction::Spawn {
                                program,
                                args,
                                pause_after,
                            } = post
                            {
                                run_child_in_foreground(terminal, &program, &args, pause_after)?;
                                // The listing may have changed (mv, rm, chmod, etc).
                                self.refresh_listing();
                                // If we were editing a pager buffer, restore it.
                                if let Some(ret) = self.pending_pager_return.take() {
                                    match ret {
                                        PagerReturn::TempFile { path, title, scroll } => {
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
                    }
                    Event::Paste(text) => {
                        if let Mode::Prompting(ref mut p) = self.mode {
                            // Paste into the active prompt buffer.
                            // Strip newlines (prompts are single-line).
                            let clean = text.replace(['\n', '\r'], " ");
                            p.buffer.push_str(&clean);
                            if let Some(ed) = p.editor.as_mut() {
                                ed.set_content(&p.buffer);
                            }
                        } else if let Some(pane) = self.pane_tabs.as_mut().map(|t| t.active_mut()) {
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
                            let layout = Self::compute_layout(
                                area,
                                true,
                                self.pane_height_pct,
                            );
                            if let Some(pane_rect) = layout.pane {
                                for entry in tabs.tabs_mut() {
                                    let _ = entry.pane.resize(pane_rect.height, pane_rect.width);
                                }
                            }
                        }
                        if let Some(overlay) = self.top_overlay.as_mut() {
                            let (r, c) = Self::top_overlay_size(
                                self.pane_height_pct,
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
            sync_listing_watch(fs_watcher.as_mut(), &mut watched_listing, &self.listing.dir);
        }
        Ok(())
    }

    fn is_config_path(&self, path: &Path) -> bool {
        self.candidate_config_paths().iter().any(|c| c == path)
            || self.config.sources.iter().any(|c| c == path)
    }

    /// True iff `path` is the listing directory or a direct child of it.
    /// `notify` events sometimes include just the directory and sometimes
    /// the affected child, so we accept both.
    fn is_listing_path(&self, path: &Path) -> bool {
        path == self.listing.dir || path.parent() == Some(self.listing.dir.as_path())
    }

    // --- Rendering --------------------------------------------------------

    /// Partition the frame into status/list/prompt rects — plus, when
    /// the pane is open, a divider row and the pane rect below it.
    ///
    /// The **entire cspy unit** (status, list, prompt) lives above the
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
        let rule_style = if self.pane_focused {
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
        let layout = Self::compute_layout(frame_area, self.pane_tabs.is_some(), self.pane_height_pct);

        // If a top-overlay pty is active (`;top`, `;vim`, etc.), it
        // replaces the entire cspy area. Status, list, and prompt are
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
            user_host: &self.user_host,
            path: &path,
            suffix: &suffix,
            git_info: self.git_info.as_deref(),
            theme: &self.theme,
        }
        .render(frame, layout.status);

        let rows = self.build_rows();
        let list_focused = !self.pane_focused;
        let probe = ListView {
            rows: &rows,
            cursor: self.cursor.index,
            view_top: self.cursor.view_top,
            empty_marker: self.view == View::Dir,
            focused: list_focused,
            theme: &self.theme,
        };
        self.last_grid = probe.grid(layout.list);
        self.ensure_cursor_visible();

        frame.render_widget(
            ListView {
                rows: &rows,
                cursor: self.cursor.index,
                view_top: self.cursor.view_top,
                empty_marker: self.view == View::Dir,
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
                    focused: self.pane_focused,
                    blink_on: self.cursor_blink_on,
                },
                rect,
            );
        }

        if let Some(divider_rect) = layout.divider {
            self.render_pane_status_line(frame, divider_rect);
        }

        if let Mode::Prompting(p) = &self.mode {
            PromptLine {
                prefix: &p.prefix,
                buffer: &p.buffer,
                theme: &self.theme,
                cursor_pos: p.editor.as_ref().map(|e| e.cursor),
                vi_mode: p.editor.as_ref().map(|e| e.mode),
            }
            .render(frame, layout.prompt);
        } else if let Some(flash) = &self.flash {
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
        } else if let Some(pending) = self.resolver.pending_display() {
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
        match self.view {
            View::Dir => (
                self.listing.dir.display().to_string(),
                format!(
                    "[picks:{} inv:{} m1:{} m2:{}]",
                    self.picks.len(),
                    self.inventory.len(),
                    on_off(self.masks.mask1.enabled),
                    on_off(self.masks.mask2.enabled),
                ),
            ),
            View::Inventory => (
                "<INVENTORY>".to_string(),
                format!("[{} items]  (i: return, u: back)", self.inventory.len()),
            ),
        }
    }

    fn build_rows(&self) -> Vec<Row> {
        self.rows
            .iter()
            .map(|rd| Row {
                display: rd.display.clone(),
                kind: rd.kind,
                picked: self.view == View::Dir && self.picks.contains(&rd.path),
                taken: self.inventory.contains(&rd.path),
            })
            .collect()
    }

    fn ensure_cursor_visible(&mut self) {
        let per_page = self.last_grid.items_per_page();
        if per_page == 0 || self.rows.is_empty() {
            self.cursor.view_top = 0;
            return;
        }
        // Pages always start on an items_per_page boundary. Any time the
        // cursor strays outside the current page, snap view_top to the
        // page that contains it.
        let page = self.cursor.index / per_page;
        self.cursor.view_top = page * per_page;
    }

    // --- Input handling ---------------------------------------------------

    fn handle_key(&mut self, key: KeyEvent) -> Result<PostAction> {
        // Any keypress clears a lingering flash message.
        self.flash = None;

        // While a `!` capture is running, Ctrl+C kills it and opens the
        // pager with whatever partial output was collected.
        if let Some(capture) = &mut self.pending_capture {
            if matches!(key.code, KeyCode::Char('c' | 'C'))
                && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                let _ = capture.child.kill();
                // Kill closes the pipe → reader thread sees EOF → sends
                // whatever it collected. Give it a moment to deliver.
                let stdout = capture
                    .output_rx
                    .recv_timeout(std::time::Duration::from_secs(1))
                    .unwrap_or_default();
                let mut bytes = stdout;
                if let Some(mut stderr) = capture.child.stderr.take() {
                    use std::io::Read as _;
                    let mut err = Vec::new();
                    let _ = stderr.read_to_end(&mut err);
                    if !err.is_empty() {
                        if !bytes.is_empty() {
                            bytes.push(b'\n');
                        }
                        bytes.extend_from_slice(&err);
                    }
                }
                let _ = capture.child.wait();
                let title = format!("{} — interrupted", capture.title);
                self.pending_capture = None;
                if bytes.is_empty() {
                    self.flash_info("command interrupted (no output)");
                } else {
                    self.pager = Some(PagerView::new_ansi(title, &bytes));
                }
            }
            return Ok(PostAction::None);
        }

        // Top overlay: once the subprocess exits, hold the screen until
        // any key so short-lived commands (`;ls`) don't flash and vanish.
        if self.overlay_awaiting_dismiss {
            self.top_overlay = None;
            self.overlay_awaiting_dismiss = false;
            self.needs_full_repaint = true;
            self.flash_info("command finished");
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
        // Let cspy meta keys (^W prefix, ^\\, F10) fall through so
        // pane commands still work from scroll mode.
        if let Some(tabs) = self.pane_tabs.as_mut() {
            let pane = tabs.active_mut();
            if pane.is_scrolling()
                && self.pane_focused
                && !is_cspy_meta_when_pane_focused(key, self.resolver.is_pending())
            {
                return self.handle_pane_scroll_key(key);
            }
        }
        // When the pane is open *and focused*, forward keys to the
        // subprocess — except cspy meta keys, which are always caught
        // by cspy so the user can toggle / resize / focus-switch / send
        // selection from inside the pane.
        if self.pane_tabs.is_some()
            && self.pane_focused
            && !matches!(self.mode, Mode::Prompting(_))
            && !is_cspy_meta_when_pane_focused(key, self.resolver.is_pending())
        {
            if let Some(tabs) = self.pane_tabs.as_mut() {
                let _ = tabs.active_mut().send_key(key);
            }
            return Ok(PostAction::None);
        }
        if matches!(self.mode, Mode::Prompting(_)) {
            return Ok(self.handle_prompt_key(key));
        }
        match self.resolver.feed(key, &self.user_keymap) {
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
                let cmd = shell::expand_percent(template, &self.selection_paths());
                return Ok(sh_c(&cmd, true));
            }
            BoundAction::PatternPick(pattern) => {
                if let Ok(pat) = glob::Pattern::new(pattern) {
                    for e in &self.listing.entries {
                        if pat.matches(&e.name) {
                            self.picks.insert(&e.path);
                        }
                    }
                }
            }
            BoundAction::Jump(path) => {
                let _ = self.jump_to(path);
            }
            BoundAction::Copy(dest) => {
                self.run_selection_to(dest, fs::ops::copy_selection_to, "copied");
            }
            BoundAction::Move(dest) => {
                self.run_selection_to(dest, fs::ops::move_selection_to, "moved");
            }
            BoundAction::ToggleMaskFixed(n) => {
                if *n == 1 {
                    self.masks.toggle_mask1();
                } else if *n == 2 {
                    self.masks.toggle_mask2();
                }
                self.rebuild_rows();
            }
        }
        self.cursor.clamp(self.rows.len());
        Ok(PostAction::None)
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) -> PostAction {
        // Destructive-confirm prompts are single-key: `y` / `Y` proceeds
        // immediately, anything else cancels. No Enter needed.
        if matches!(
            &self.mode,
            Mode::Prompting(p) if matches!(p.kind, PromptKind::RemoveConfirm)
        ) {
            return self.handle_remove_confirm_key(key);
        }

        // Shell prompts (`!` / `;`) use the vi line editor + history.
        let has_editor = matches!(
            &self.mode,
            Mode::Prompting(p) if p.editor.is_some()
        );
        if has_editor {
            return self.handle_vi_prompt_key(key);
        }

        // --- Simple prompts (search, jump, pattern-pick, etc.) ---

        // Esc cancels; Backspace on an empty buffer cancels too.
        let backspace_on_empty = matches!(key.code, KeyCode::Backspace)
            && matches!(&self.mode, Mode::Prompting(p) if p.buffer.is_empty());
        if matches!(key.code, KeyCode::Esc) || backspace_on_empty {
            self.cancel_prompt();
            return PostAction::None;
        }
        if matches!(key.code, KeyCode::Enter) {
            let Mode::Prompting(p) = std::mem::replace(&mut self.mode, Mode::Normal) else {
                return PostAction::None;
            };
            return self.dispatch_prompt(p);
        }

        // Edit the buffer. Scoped borrow so we can run search afterwards.
        {
            let Mode::Prompting(prompt) = &mut self.mode else {
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
        }) = &self.mode
        {
            Some((*saved_cursor, buffer.clone()))
        } else {
            None
        };
        if let Some((saved, query)) = search_info {
            if query.is_empty() {
                self.cursor.index = saved;
            } else if let Some(i) = self.find_match(&query, saved, false) {
                self.cursor.index = i;
            }
            self.cursor.clamp(self.rows.len());
        }

        PostAction::None
    }

    /// Single-key confirmation for `R`. `y` / `Y` triggers the delete;
    /// anything else — including Enter, Esc, or any other letter — cancels.
    /// The prompt closes in every case.
    fn handle_remove_confirm_key(&mut self, key: KeyEvent) -> PostAction {
        let confirmed = matches!(key.code, KeyCode::Char('y' | 'Y'));
        self.mode = Mode::Normal;
        if !confirmed {
            return PostAction::None;
        }
        let paths = self.selection_paths();
        if paths.is_empty() {
            return PostAction::None;
        }
        let count = paths.len();
        self.run_and_flash(
            fs::ops::remove_all(&paths),
            format!("removed {count} item(s)"),
        );
        self.picks.clear();
        self.refresh_listing();
        PostAction::None
    }

    /// Return the appropriate history for the current prompt kind.
    fn history_for_prompt(&mut self) -> &mut History {
        let is_pane = matches!(
            self.mode,
            Mode::Prompting(Prompt {
                kind: PromptKind::PaneNewTabCmd | PromptKind::PaneNewTabCwd,
                ..
            })
        );
        if is_pane {
            &mut self.pane_history
        } else {
            &mut self.history
        }
    }

    /// Handle keys for shell prompts that use the vi line editor.
    fn handle_vi_prompt_key(&mut self, key: KeyEvent) -> PostAction {
        use crate::ui::line_edit::EditResult;

        // Feed key to the editor.
        let result = {
            let Mode::Prompting(prompt) = &mut self.mode else {
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
                    self.mode,
                    Mode::Prompting(Prompt {
                        kind: PromptKind::PaneNewTabCmd | PromptKind::PaneNewTabCwd,
                        ..
                    })
                );
                let Mode::Prompting(p) = std::mem::replace(&mut self.mode, Mode::Normal) else {
                    return PostAction::None;
                };
                // Push to the appropriate history before dispatching.
                let hist = if is_pane_prompt {
                    &mut self.pane_history
                } else {
                    &mut self.history
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
                    let Mode::Prompting(p) = &self.mode else {
                        return PostAction::None;
                    };
                    p.buffer.clone()
                };
                let hist = self.history_for_prompt();
                if let Some(entry) = hist.prev(&current_text) {
                    let entry = entry.to_string();
                    let Mode::Prompting(p) = &mut self.mode else {
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
                let Mode::Prompting(p) = &mut self.mode else {
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
        let Mode::Prompting(p) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return;
        };
        if let PromptKind::Search { saved_cursor } = p.kind {
            self.cursor.index = saved_cursor;
            self.cursor.clamp(self.rows.len());
        }
        // Clear any stashed state from the two-step new-tab prompt.
        self.pending_new_tab_cmd = None;
    }

    fn dispatch_prompt(&mut self, prompt: Prompt) -> PostAction {
        match prompt.kind {
            PromptKind::PatternPick => {
                if let Ok(pat) = Pattern::new(&prompt.buffer) {
                    for e in &self.listing.entries {
                        if pat.matches(&e.name) {
                            self.picks.insert(&e.path);
                        }
                    }
                }
                PostAction::None
            }
            PromptKind::ShellCmd => {
                // `;cmd` — run interactively in a top-overlay pty that
                // replaces the cspy listing. Bottom pane (claude) stays
                // untouched. When the command exits, cspy comes back.
                let expanded = shell::expand_percent(&prompt.buffer, &self.selection_paths());
                let (rows, cols) =
                    Self::top_overlay_size(self.pane_height_pct, self.pane_tabs.is_some());
                let cwd = self.listing.dir.clone();
                match Pane::spawn(&expanded, rows, cols, &cwd) {
                    Ok(p) => {
                        self.top_overlay = Some(p);
                    }
                    Err(e) => self.flash_error(format!("spawn: {e}")),
                }
                PostAction::None
            }
            PromptKind::ShellCmdCaptured => {
                let expanded = shell::expand_percent(&prompt.buffer, &self.selection_paths());
                let title = format!("! {}", prompt.buffer);
                // Spawn non-blocking so the event loop stays alive and
                // Ctrl+C can kill the child.
                match spawn_capture(&expanded) {
                    Ok((child, rx)) => {
                        let cmd_display = prompt.buffer.clone();
                        self.pending_capture = Some(PendingCapture {
                            child,
                            output_rx: rx,
                            title,
                            cmd_display,
                        });
                    }
                    Err(e) => self.flash_error(format!("exec: {e}")),
                }
                PostAction::None
            }
            PromptKind::Search { .. } => {
                // Cursor is already where the incremental search left it.
                if !prompt.buffer.is_empty() {
                    self.last_search = Some(prompt.buffer);
                }
                PostAction::None
            }
            PromptKind::Jump => {
                let trimmed = prompt.buffer.trim();
                if !trimmed.is_empty() {
                    let _ = self.jump_to(trimmed);
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
            PromptKind::MakeDir => {
                let name = prompt.buffer.trim();
                if !name.is_empty() {
                    let target = crate::paths::expand(name);
                    let resolved = if target.is_absolute() {
                        target
                    } else {
                        self.listing.dir.join(&target)
                    };
                    self.run_and_flash(
                        std::fs::create_dir_all(&resolved),
                        format!("created {}", resolved.display()),
                    );
                    self.refresh_listing();
                }
                PostAction::None
            }
            // RemoveConfirm never reaches dispatch — it's handled as a
            // single-key confirm in `handle_remove_confirm_key`.
            PromptKind::RemoveConfirm => PostAction::None,
            PromptKind::SetEnv => {
                let line = prompt.buffer.trim();
                if let Some((name, value)) = line.split_once('=') {
                    let name = name.trim();
                    if name.is_empty() {
                        self.flash_error("setenv: missing variable name");
                    } else {
                        // SAFETY: single-threaded TUI; no other thread is
                        // reading env concurrently.
                        unsafe {
                            std::env::set_var(name, value);
                        }
                        self.flash_info(format!("setenv {name}={value}"));
                    }
                } else if !line.is_empty() {
                    self.flash_error("setenv: expected NAME=VALUE");
                }
                PostAction::None
            }
            PromptKind::PaneNewTabCmd => {
                let cmd = prompt.buffer.trim().to_string();
                if cmd.is_empty() {
                    return PostAction::None;
                }
                self.pending_new_tab_cmd = Some(cmd);
                let cwd_default = self.listing.dir.display().to_string();
                let mut p = Prompt::shell(PromptKind::PaneNewTabCwd, "pane cwd: ");
                p.buffer.clone_from(&cwd_default);
                if let Some(ed) = p.editor.as_mut() {
                    ed.set_content(&cwd_default);
                }
                self.mode = Mode::Prompting(p);
                return PostAction::None;
            }
            PromptKind::PaneNewTabCwd => {
                let cwd = prompt.buffer.trim().to_string();
                if let Some(cmd) = self.pending_new_tab_cmd.take() {
                    let cwd_path = if cwd.is_empty() {
                        self.listing.dir.clone()
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
            PromptKind::WorktreeNewBranch => {
                let branch = prompt.buffer.trim().to_string();
                if branch.is_empty() {
                    return PostAction::None;
                }
                match crate::sysinfo::git_worktree_add(&self.listing.dir, &branch) {
                    Ok(path) => {
                        self.flash_info(format!("created worktree: {}", path.display()));
                        if let Err(e) = self.chdir(&path) {
                            self.flash_error(format!("chdir: {e}"));
                        }
                    }
                    Err(e) => self.flash_error(format!("worktree add: {e}")),
                }
                PostAction::None
            }
            PromptKind::WorktreeDeleteConfirm => {
                let confirmed = prompt.buffer.trim().eq_ignore_ascii_case("y");
                if !confirmed {
                    return PostAction::None;
                }
                let dir = self.listing.dir.clone();
                match crate::sysinfo::git_worktree_remove(&dir) {
                    Ok(()) => {
                        self.flash_info(format!("removed worktree: {}", dir.display()));
                        if let Some(parent) = dir.parent() {
                            let _ = self.chdir(parent);
                        }
                    }
                    Err(e) => self.flash_error(format!("worktree remove: {e}")),
                }
                PostAction::None
            }
        }
    }

    /// Open the split pane if it's closed, close all tabs if it's open.
    fn toggle_pane(&mut self) {
        if self.pane_tabs.is_some() {
            self.pane_tabs = None;
            self.pane_focused = false;
            self.needs_full_repaint = true;
            self.flash_info("pane closed");
            return;
        }
        let cmd = std::env::var("CSPY_PANE_CMD").unwrap_or_else(|_| "claude".to_string());
        self.open_pane_tab(&cmd);
    }

    /// Spawn a new pane tab. If no tabs exist, creates the container.
    fn open_pane_tab(&mut self, cmd: &str) {
        self.open_pane_tab_in(cmd, &self.listing.dir.clone());
    }

    fn open_pane_tab_in(&mut self, cmd: &str, cwd: &std::path::Path) {
        let (rows, cols) = Self::pane_spawn_size(self.pane_height_pct);
        match Pane::spawn(cmd, rows, cols, cwd) {
            Ok(p) => {
                let entry = TabEntry {
                    pane: p,
                    info: TabInfo::new(cmd, cwd),
                };
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.push(entry);
                } else {
                    self.pane_tabs = Some(PaneTabs::new(entry));
                }
                self.pane_focused = true;
                self.flash_info(format!("pane: {cmd} (^W k for list)"));
            }
            Err(e) => self.flash_error(format!("pane spawn failed: {e}")),
        }
    }

    /// ^W n — start the two-step prompt for a new pane tab.
    fn start_new_tab_prompt(&mut self) {
        let default_cmd =
            std::env::var("CSPY_PANE_CMD").unwrap_or_else(|_| "claude".to_string());
        let mut p = Prompt::shell(PromptKind::PaneNewTabCmd, "pane command: ");
        p.buffer.clone_from(&default_cmd);
        if let Some(ed) = p.editor.as_mut() {
            ed.set_content(&default_cmd);
        }
        self.mode = Mode::Prompting(p);
    }

    /// ^W x — close the active pane tab.
    fn close_active_tab(&mut self) {
        if let Some(tabs) = self.pane_tabs.as_mut() {
            if !tabs.close_active() {
                // Last tab removed.
                self.pane_tabs = None;
                self.pane_focused = false;
                self.needs_full_repaint = true;
                self.flash_info("pane: last tab closed");
            }
        }
    }

    /// ^W j / ^W k — flip keyboard focus between the list and the pane.
    fn toggle_pane_focus(&mut self) {
        if self.pane_tabs.is_none() {
            return;
        }
        self.pane_focused = !self.pane_focused;
        self.flash_info(if self.pane_focused {
            "focus: pane"
        } else {
            "focus: list"
        });
    }

    /// Handle keys while the pane is in scroll mode. Vi-style navigation
    /// through the scrollback buffer; `Esc`/`q` exit back to live view.
    fn handle_pane_scroll_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Result<PostAction> {
        use crossterm::event::{KeyCode, KeyModifiers};
        let pane = self.pane_tabs.as_mut().unwrap().active_mut();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('k') | KeyCode::Up => pane.scroll_up(1),
            KeyCode::Char('j') | KeyCode::Down => pane.scroll_down_or_exit(1),
            KeyCode::PageUp | KeyCode::Char('b') if ctrl => pane.scroll_up(20),
            KeyCode::Char('u') if ctrl => pane.scroll_up(10),
            KeyCode::PageDown | KeyCode::Char('f') if ctrl => pane.scroll_down_or_exit(20),
            KeyCode::Char('d') if ctrl => pane.scroll_down_or_exit(10),
            KeyCode::Char('g') => {
                pane.scroll_to_top();
            }
            KeyCode::Char('G') => pane.scroll_to_bottom(),
            KeyCode::Char('s') => {
                match pane.save_to_file() {
                    Ok(path) => {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        self.flash_info(&format!("saved: {name}"));
                    }
                    Err(e) => self.flash_info(&format!("save error: {e}")),
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                pane.exit_scroll_mode();
                self.flash_info("scroll: off");
            }
            _ => {}
        }
        Ok(PostAction::None)
    }

    /// ^W s — write the current selection as shell-quoted paths to the
    /// pane's stdin. A trailing space is appended so the user can keep
    /// typing without concatenating against the last path. No newline
    /// — let the user decide when to submit.
    fn send_selection_to_pane(&mut self) {
        if self.pane_tabs.is_none() {
            self.flash_error("no pane open (Ctrl-\\ to open one)");
            return;
        }
        // Build the payload before grabbing the pane mut-borrow, so we
        // can still call self.flash_* below without overlapping borrows.
        let (payload, count) = {
            let paths = self.selection_paths();
            if paths.is_empty() {
                self.flash_error("nothing selected");
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
            let pane = self.pane_tabs.as_mut().expect("pane existence already checked").active_mut();
            pane.send_bytes(payload.as_bytes())
        };
        match result {
            Ok(()) => self.flash_info(format!("sent {count} path(s) to pane")),
            Err(e) => self.flash_error(format!("send failed: {e}")),
        }
    }

    /// ^W p / ^W i — read file contents of selection (or inventory) and
    /// send them to the active pane tab as bracketed paste. Each file is
    /// wrapped with a header so the recipient (e.g. Claude) knows what
    /// it's looking at.
    fn pipe_content_to_pane(&mut self, use_inventory: bool) {
        if self.pane_tabs.is_none() {
            self.flash_error("no pane open");
            return;
        }
        let paths: Vec<PathBuf> = if use_inventory {
            self.inventory.paths().cloned().collect()
        } else {
            self.selection_paths().into_iter().map(Path::to_path_buf).collect()
        };
        if paths.is_empty() {
            self.flash_error(if use_inventory {
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
            payload.push_str(&format!(
                "[file: {}]\n{}",
                path.display(),
                contents
            ));
            count += 1;
        }
        if count == 0 {
            self.flash_error("no readable text files in selection");
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
            Ok(()) => self.flash_info(msg),
            Err(e) => self.flash_error(format!("pipe failed: {e}")),
        }
    }

    /// ^W + / ^W - — change the bottom pane's share of the middle rect
    /// in 5% steps, clamped to [10%, 90%].
    fn resize_pane(&mut self, delta_pct: i32) {
        if self.pane_tabs.is_none() {
            return;
        }
        let current = i32::from(self.pane_height_pct);
        let new = (current + delta_pct).clamp(10, 90);
        self.pane_height_pct = new as u16;
    }

    // ---- Git diff (M12) ----------------------------------------------------

    /// g d / g D — run `git diff` on selection and show in pager.
    fn open_git_diff(&mut self, cached: bool) {
        let paths = self.selection_paths();
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
            .current_dir(&self.listing.dir)
            .output()
        {
            Ok(out) => {
                if out.stdout.is_empty() {
                    let label = if cached { "staged" } else { "unstaged" };
                    self.flash_info(format!("no {label} changes"));
                } else {
                    let label = if cached { "git diff --cached" } else { "git diff" };
                    self.pager = Some(pager::PagerView::new_ansi(label, &out.stdout));
                }
            }
            Err(e) => self.flash_error(format!("git diff: {e}")),
        }
    }

    // ---- Git worktree (M11) -------------------------------------------------

    /// W l — list worktrees in a pager; digit keys 1-9 select.
    fn worktree_list(&mut self) {
        match crate::sysinfo::git_worktree_list(&self.listing.dir) {
            Some(worktrees) => {
                self.pending_worktrees =
                    Some(worktrees.iter().map(|w| w.path.clone()).collect());
                let lines: Vec<String> = worktrees
                    .iter()
                    .enumerate()
                    .map(|(i, wt)| {
                        let current = if wt.path == self.listing.dir {
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
            None => self.flash_error("not in a git repository (or no worktrees)"),
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
        lines.push(format!("cspy {}", env!("CARGO_PKG_VERSION")));
        lines.push(format!("pid      : {}", std::process::id()));
        lines.push(format!("cwd      : {}", self.listing.dir.display()));
        lines.push(format!("entries  : {}", self.listing.entries.len()));
        lines.push(format!("visible  : {}", self.rows.len()));
        lines.push(format!("picks    : {}", self.picks.len()));
        lines.push(format!("inventory: {}", self.inventory.len()));
        lines.push(format!("marks    : {}", self.marks.entries.len()));
        lines.push(format!("rss      : {}", crate::sysinfo::format_rss()));
        lines.push(format!("time     : {}", crate::sysinfo::format_now()));
        if !self.config.sources.is_empty() {
            lines.push(String::new());
            lines.push("config sources:".into());
            for src in &self.config.sources {
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
        let paths = self.selection_paths();
        if paths.is_empty() {
            self.flash_error("nothing selected");
            return;
        }
        let count = paths.len();
        let expanded = crate::paths::expand(dest_trim);
        let dest = if expanded.is_absolute() {
            expanded
        } else {
            self.listing.dir.join(&expanded)
        };
        self.run_and_flash(
            op(&paths, &dest),
            format!("{verb} {count} item(s) to {}", dest.display()),
        );
        // Picks point at paths that may no longer exist after a move.
        self.picks.clear();
        self.refresh_listing();
    }

    /// Set the flash message based on the result of a mutating operation.
    fn run_and_flash(&mut self, result: std::io::Result<()>, success_msg: String) {
        match result {
            Ok(()) => self.flash_info(success_msg),
            Err(e) => self.flash_error(format!("error: {e}")),
        }
    }

    fn flash_info<S: Into<String>>(&mut self, text: S) {
        self.flash = Some(FlashMessage {
            text: text.into(),
            kind: FlashKind::Info,
        });
    }

    fn flash_error<S: Into<String>>(&mut self, text: S) {
        self.flash = Some(FlashMessage {
            text: text.into(),
            kind: FlashKind::Error,
        });
    }

    /// Navigate to an arbitrary path. `target` is expanded for `~` and
    /// `$VAR`; relative paths are resolved against the current directory.
    /// If the expanded path points to a file, we chdir to its parent and
    /// focus the cursor on it.
    fn jump_to(&mut self, target: &str) -> Result<()> {
        let expanded = crate::paths::expand(target);
        let abs = if expanded.is_absolute() {
            expanded
        } else {
            self.listing.dir.join(&expanded)
        };
        let canonical = std::fs::canonicalize(&abs)?;
        let md = std::fs::metadata(&canonical)?;
        if md.is_dir() {
            if let Err(e) = self.chdir(&canonical) {
                self.flash_error(format!("chdir: {e}"));
                return Ok(());
            }
        } else if let Some(parent) = canonical.parent() {
            if let Err(e) = self.chdir(parent) {
                self.flash_error(format!("chdir: {e}"));
                return Ok(());
            }
            self.focus_on_path(&canonical);
        }
        Ok(())
    }

    /// `ma` — remember the current directory and cursor entry under
    /// letter `letter`. Saved to disk best-effort so marks survive
    /// restarts; a disk failure flashes but doesn't block the in-memory
    /// set.
    fn set_mark(&mut self, letter: char) {
        let focus = self.rows.get(self.cursor.index).map(|r| r.path.clone());
        self.marks.set(
            letter,
            Mark {
                dir: self.listing.dir.clone(),
                focus,
            },
        );
        match self.marks.save() {
            Ok(()) => self.flash_info(format!("mark '{letter}' set")),
            Err(e) => self.flash_error(format!("mark saved in-memory only: {e}")),
        }
    }

    /// `'a` — chdir to the remembered directory and focus on the entry
    /// if it still exists.
    fn jump_to_mark(&mut self, letter: char) {
        let Some(mark) = self.marks.get(letter).cloned() else {
            self.flash_error(format!("mark '{letter}' not set"));
            return;
        };
        if let Err(e) = self.chdir(&mark.dir) {
            self.flash_error(format!("jump failed: {e}"));
            return;
        }
        if let Some(focus) = mark.focus {
            self.focus_on_path(&focus);
        }
        self.flash_info(format!("jumped to mark '{letter}'"));
    }

    /// Search over visible rows. Case-insensitive.
    ///
    /// - If `query` contains no glob metacharacters (`*`, `?`, `[`), it is
    ///   treated as a **prefix**: `/R` lands on `README.md`, not the first
    ///   file that happens to contain an `r`.
    /// - If `query` does contain glob metacharacters, it's treated as a
    ///   glob pattern against the full name: `/*.rs` matches all Rust
    ///   files, `/*README*` matches anything containing `README`.
    fn find_match(&self, query: &str, from: usize, backward: bool) -> Option<usize> {
        if self.rows.is_empty() {
            return None;
        }
        let matcher = Matcher::build(query);
        let n = self.rows.len();
        for step in 0..n {
            let i = if backward {
                (from + n - step) % n
            } else {
                (from + step) % n
            };
            if matcher.matches(&self.rows[i].display) {
                return Some(i);
            }
        }
        None
    }

    fn selection_paths(&self) -> Vec<&Path> {
        // `%` expands to picks if any, else the cursor item.
        if self.view == View::Dir && !self.picks.is_empty() {
            self.picks.iter().map(PathBuf::as_path).collect()
        } else if let Some(row) = self.rows.get(self.cursor.index) {
            vec![row.path.as_path()]
        } else {
            Vec::new()
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
                        self.flash_error("no matches");
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

        // Worktree picker: 1-9 selects a worktree and chdirs.
        if let Some(ref worktrees) = self.pending_worktrees {
            if let KeyCode::Char(c @ '1'..='9') = key.code {
                let idx = (c as u8 - b'1') as usize;
                if let Some(path) = worktrees.get(idx).cloned() {
                    self.pager = None;
                    self.pending_worktrees = None;
                    self.needs_full_repaint = true;
                    if let Err(e) = self.chdir(&path) {
                        self.flash_error(format!("chdir: {e}"));
                    }
                    return PostAction::None;
                }
            }
        }

        match key.code {
            KeyCode::Char('q' | 'Q') | KeyCode::Esc => {
                self.pager = None;
                self.pending_worktrees = None;
                self.needs_full_repaint = true;
            }
            KeyCode::Char('/') => view.begin_search(),
            KeyCode::Char('n') => view.search_next(viewport),
            KeyCode::Char('N') => view.search_prev(viewport),
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
                Ok(()) => self.flash_info("yanked to clipboard"),
                Err(e) => self.flash_error(format!("yank failed: {e}")),
            },
            KeyCode::Char('s') if view.saveable => match view.save_to_file() {
                Ok(path) => self.flash_info(format!("saved: {}", path.display())),
                Err(e) => self.flash_error(format!("save failed: {e}")),
            },
            KeyCode::Char('v') => {
                let argv = shell::resolve_editor();
                let editor_cmd = argv.join(" ");
                let scroll = view.scroll;
                // Determine the file to edit and the return state.
                let (edit_path, pager_return) = if let Some(ref src) = view.source_path {
                    (src.clone(), PagerReturn::SourceFile { path: src.clone(), scroll })
                } else {
                    let title = view.title.clone();
                    match view.write_to_temp() {
                        Ok(tmp) => (tmp.clone(), PagerReturn::TempFile { path: tmp, title, scroll }),
                        Err(e) => {
                            self.flash_error(format!("write temp: {e}"));
                            return PostAction::None;
                        }
                    }
                };
                self.pending_pager_return = Some(pager_return);
                self.pager = None;
                self.needs_full_repaint = true;
                return sh_c(
                    &format!("{editor_cmd} {}", shell::shell_quote(&edit_path.display().to_string())),
                    false,
                );
            }
            _ => {}
        }
        PostAction::None
    }

    fn cursor_move_vertical(&mut self, delta: isize, len: usize) {
        if len == 0 {
            return;
        }
        let new_idx = (self.cursor.index as isize + delta).rem_euclid(len as isize);
        self.cursor.index = new_idx as usize;
    }

    /// `gg` — jump to the first entry of the current column.
    fn goto_col_top(&mut self) {
        let rows_per_col = self.last_grid.rows as usize;
        if rows_per_col == 0 {
            self.cursor.index = 0;
            return;
        }
        let current_col = self.cursor.index / rows_per_col;
        self.cursor.index = current_col * rows_per_col;
    }

    /// `G` — jump to the last entry of the current column. For partial
    /// columns (the rightmost column when it doesn't fill all rows) this
    /// is the last entry in that column, not in the list.
    fn goto_col_bottom(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        let rows_per_col = self.last_grid.rows as usize;
        if rows_per_col == 0 {
            self.cursor.index = len - 1;
            return;
        }
        let current_col = self.cursor.index / rows_per_col;
        let col_end = ((current_col + 1) * rows_per_col).min(len);
        self.cursor.index = col_end - 1;
    }

    /// Move the cursor by `delta` columns, preserving the row within each
    /// column and wrapping around the grid. When only one column exists,
    /// horizontal motion is a no-op — there is nowhere to go. When the
    /// target column is partial (fewer rows than the current row), land
    /// on its last valid entry so we never leave the target column.
    fn cursor_move_columns(&mut self, delta: isize, rows_per_col: usize, len: usize) {
        if rows_per_col == 0 || len == 0 {
            return;
        }
        let num_cols = len.div_ceil(rows_per_col);
        if num_cols <= 1 {
            return;
        }
        let current_col = (self.cursor.index / rows_per_col) as isize;
        let current_row = self.cursor.index % rows_per_col;
        let target_col = (current_col + delta).clamp(0, num_cols as isize - 1) as usize;
        if target_col == current_col as usize {
            return; // already at the edge
        }
        let target_idx = target_col * rows_per_col + current_row;
        self.cursor.index = if target_idx < len {
            target_idx
        } else {
            // Partial column — clamp to its last entry.
            let col_end = ((target_col + 1) * rows_per_col).min(len);
            col_end - 1
        };
    }

    // --- Action handlers --------------------------------------------------

    fn apply(&mut self, action: &Action) -> Result<PostAction> {
        let len = self.rows.len();
        // In a column-major grid, moving one column horizontally advances
        // the flat index by `rows_per_col`. Moving vertically is ±1.
        let rows_per_col = self.last_grid.rows as usize;
        let per_page = self.last_grid.items_per_page();
        match action {
            Action::Up(n) => self.cursor_move_vertical(-(*n as isize), len),
            Action::Down(n) => self.cursor_move_vertical(*n as isize, len),
            Action::Left(n) => self.cursor_move_columns(-(*n as isize), rows_per_col, len),
            Action::Right(n) => self.cursor_move_columns(*n as isize, rows_per_col, len),
            Action::PageUp => self.cursor_move_vertical(-(per_page as isize), len),
            Action::PageDown => self.cursor_move_vertical(per_page as isize, len),
            Action::GotoFirst => self.goto_col_top(),
            Action::GotoLast => self.goto_col_bottom(len),

            Action::EnterOrDisplay => {
                let post = self.activate(ActivateIntent::Display)?;
                self.cursor.clamp(self.rows.len());
                return Ok(post);
            }
            Action::EnterOrEdit => {
                let post = self.activate(ActivateIntent::Edit)?;
                self.cursor.clamp(self.rows.len());
                return Ok(post);
            }
            Action::Climb => self.climb()?,
            Action::Home => {
                if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                    if let Err(e) = self.chdir(&home) {
                        self.flash_error(format!("chdir: {e}"));
                    }
                }
            }

            Action::TogglePick => self.toggle_pick_cursor(),
            Action::PickPatternPrompt => {
                if self.view == View::Dir {
                    self.mode =
                        Mode::Prompting(Prompt::simple(PromptKind::PatternPick, "pick pattern: "));
                }
            }
            Action::PickToggleAll => self.toggle_all_picks(),

            Action::Take => self.take(),
            Action::Drop => self.drop_cursor(),
            Action::ToggleInventoryView => self.toggle_inventory_view(),
            Action::EmptyInventory => {
                self.inventory.clear();
                self.rebuild_rows();
            }

            Action::ToggleMask(n) => {
                if *n == 1 {
                    self.masks.toggle_mask1();
                } else if *n == 2 {
                    self.masks.toggle_mask2();
                }
                self.rebuild_rows();
            }

            Action::ShellCapturedPrompt => {
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::ShellCmdCaptured, "!"));
            }
            Action::ShellForegroundPrompt => {
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::ShellCmd, ";"));
            }
            Action::StartShell => {
                let sh = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
                return Ok(PostAction::Spawn {
                    program: sh,
                    args: vec![],
                    pause_after: false,
                });
            }
            Action::ChmodAdd(mode_char) => {
                let paths = self.selection_paths();
                if paths.is_empty() {
                    return Ok(PostAction::None);
                }
                // +w adds user-write (0o200); +x adds exec-for-all (0o111).
                // Mirroring the common shell conventions without consulting
                // umask — if the user wants finer control they can !chmod %.
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
                self.refresh_listing();
            }

            Action::SearchPrompt => {
                self.mode = Mode::Prompting(Prompt::simple(
                    PromptKind::Search {
                        saved_cursor: self.cursor.index,
                    },
                    "/",
                ));
            }
            Action::SearchNext => {
                if let Some(term) = self.last_search.clone() {
                    let n = self.rows.len();
                    if n > 0 {
                        let start = (self.cursor.index + 1) % n;
                        if let Some(i) = self.find_match(&term, start, false) {
                            self.cursor.index = i;
                        }
                    }
                }
            }
            Action::SearchPrev => {
                if let Some(term) = self.last_search.clone() {
                    let n = self.rows.len();
                    if n > 0 {
                        let start = if self.cursor.index == 0 {
                            n - 1
                        } else {
                            self.cursor.index - 1
                        };
                        if let Some(i) = self.find_match(&term, start, true) {
                            self.cursor.index = i;
                        }
                    }
                }
            }

            Action::JumpPrompt => {
                self.mode = Mode::Prompting(Prompt::simple(PromptKind::Jump, "jump to: "));
            }

            Action::CopyPrompt => {
                if !self.selection_paths().is_empty() {
                    self.mode = Mode::Prompting(Prompt::simple(PromptKind::CopyTo, "copy to: "));
                }
            }
            Action::MovePrompt => {
                if !self.selection_paths().is_empty() {
                    self.mode = Mode::Prompting(Prompt::simple(PromptKind::MoveTo, "move to: "));
                }
            }
            Action::MakeDirPrompt => {
                self.mode = Mode::Prompting(Prompt::simple(PromptKind::MakeDir, "mkdir: "));
            }
            Action::RemovePrompt => {
                let count = self.selection_paths().len();
                if count > 0 {
                    self.mode = Mode::Prompting(Prompt::simple(
                        PromptKind::RemoveConfirm,
                        format!("remove {count} file(s)? (y/N): "),
                    ));
                }
            }
            Action::LongList => {
                // No selection → list the whole current directory.
                let owned: Vec<PathBuf>;
                let paths: Vec<&Path> = if self.selection_paths().is_empty() {
                    owned = self
                        .listing
                        .entries
                        .iter()
                        .map(|e| e.path.clone())
                        .collect();
                    owned.iter().map(PathBuf::as_path).collect()
                } else {
                    self.selection_paths()
                };
                let lines = fs::ops::format_long_listing(&paths);
                let title = format!("long listing — {}", self.listing.dir.display());
                self.pager = Some(PagerView::new_plain(title, lines));
            }
            Action::FileType => {
                let paths = self.selection_paths();
                if paths.is_empty() {
                    return Ok(PostAction::None);
                }
                if paths.len() == 1 {
                    let label = fs::ops::file_type_label(paths[0]);
                    let name = paths[0].file_name().map_or_else(
                        || paths[0].display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    self.flash_info(format!("{name}: {label}"));
                } else {
                    let lines: Vec<String> = paths
                        .iter()
                        .map(|p| {
                            let name = p.file_name().map_or_else(
                                || p.display().to_string(),
                                |n| n.to_string_lossy().into_owned(),
                            );
                            format!("{name}: {}", fs::ops::file_type_label(p))
                        })
                        .collect();
                    self.pager = Some(PagerView::new_plain("file types", lines));
                }
            }

            Action::Help => {
                let lines = help::build_lines(&self.theme, &self.user_keymap);
                let mut view = pager::PagerView::new_styled("cspy — key bindings", lines);
                view.columns = 2;
                self.pager = Some(view);
            }

            Action::ReloadConfig => self.reload_config(),

            Action::TogglePane
            | Action::ResumePane
            | Action::PaneFocusToggle
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
                    self.mode,
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
            Action::PaneFocusToggle => self.toggle_pane_focus(),
            Action::PaneSendSelection => self.send_selection_to_pane(),
            Action::PaneGrow => self.resize_pane(5),
            Action::PaneShrink => self.resize_pane(-5),
            Action::PaneScrollEnter => {
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    tabs.active_mut().enter_scroll_mode();
                    self.pane_focused = true;
                    self.flash_info("scroll: on (j/k nav, s save, Esc exit)");
                }
            }
            Action::PaneScrollSave => {
                if let Some(tabs) = self.pane_tabs.as_mut() {
                    match tabs.active_mut().save_to_file() {
                        Ok(path) => {
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            self.flash_info(&format!("saved: {name}"));
                        }
                        Err(e) => self.flash_info(&format!("save error: {e}")),
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
                    self.mode = Mode::Prompting(p);
                }
            }

            Action::PanePipeContent => self.pipe_content_to_pane(false),
            Action::PanePipeInventory => self.pipe_content_to_pane(true),

            Action::WorktreeList => self.worktree_list(),
            Action::WorktreeNew => {
                if self.git_info.is_none() {
                    self.flash_error("not in a git repository");
                } else {
                    let p = Prompt::shell(PromptKind::WorktreeNewBranch, "worktree branch: ");
                    self.mode = Mode::Prompting(p);
                }
            }
            Action::WorktreeDelete => {
                if self.git_info.is_none() {
                    self.flash_error("not in a git repository");
                } else {
                    let dir = self.listing.dir.display().to_string();
                    self.mode = Mode::Prompting(Prompt::simple(
                        PromptKind::WorktreeDeleteConfirm,
                        format!("remove worktree {dir}? (y/N): "),
                    ));
                }
            }

            Action::GitDiff | Action::GitDiffCached => {
                let cached = matches!(action, Action::GitDiffCached);
                if self.git_info.is_none() {
                    self.flash_error("not in a git repository");
                } else {
                    self.open_git_diff(cached);
                }
            }

            Action::SetMark(letter) => self.set_mark(*letter),
            Action::JumpMark(letter) => self.jump_to_mark(*letter),
            Action::JumpStartDir => {
                let dir = self.start_dir.clone();
                if let Err(e) = self.chdir(&dir) {
                    self.flash_error(format!("jump to start failed: {e}"));
                }
            }
            Action::JumpPrevDir => {
                if let Some(prev) = self.prev_dir.clone() {
                    if let Err(e) = self.chdir(&prev) {
                        self.flash_error(format!("jump back failed: {e}"));
                    }
                } else {
                    self.flash_error("no previous directory");
                }
            }

            Action::Date => self.flash_info(crate::sysinfo::format_now()),
            Action::Version => {
                self.flash_info(format!("cspy {}", env!("CARGO_PKG_VERSION")));
            }
            Action::ShowMemory => self.show_session_info(),
            Action::ColorToggle => {
                self.theme = self.theme.toggled();
                self.flash_info(if self.theme.mono {
                    "colors off"
                } else {
                    "colors on"
                });
            }
            Action::SetEnvPrompt => {
                self.mode =
                    Mode::Prompting(Prompt::simple(PromptKind::SetEnv, "setenv NAME=VALUE: "));
            }

            Action::Redraw => {
                self.needs_full_repaint = true;
            }
            Action::Noop => {}
            Action::Quit => {
                let now = std::time::Instant::now();
                if self
                    .quit_pending
                    .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(2))
                {
                    self.should_quit = true;
                } else {
                    self.quit_pending = Some(now);
                    self.flash_info("press again to quit");
                }
            }
        }
        self.cursor.clamp(self.rows.len());
        Ok(PostAction::None)
    }

    fn activate(&mut self, intent: ActivateIntent) -> Result<PostAction> {
        let Some(row) = self.rows.get(self.cursor.index) else {
            return Ok(PostAction::None);
        };
        let path = row.path.clone();
        let kind = row.kind;

        // Inventory view: enter drills down to the containing directory and
        // focuses on the item, then continues with the intent on that item.
        if self.view == View::Inventory {
            let target_dir = if kind == EntryKind::Dir {
                path.clone()
            } else {
                path.parent()
                    .map_or_else(|| path.clone(), Path::to_path_buf)
            };
            if let Err(e) = self.chdir(&target_dir) {
                self.flash_error(format!("chdir: {e}"));
                return Ok(PostAction::None);
            }
            self.view = View::Dir;
            self.focus_on_path(&path);
            self.rebuild_rows();
            if kind == EntryKind::Dir {
                return Ok(PostAction::None);
            }
        }

        if kind == EntryKind::Dir {
            if let Err(e) = self.chdir(&path) {
                self.flash_error(format!("chdir: {e}"));
            }
            return Ok(PostAction::None);
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
                            let lines: Vec<String> = content.lines().map(String::from).collect();
                            let mut view = PagerView::new_plain(name, lines);
                            view.source_path = Some(path.clone());
                            self.pager = Some(view);
                        }
                        Err(e) => self.flash_error(format!("read: {e}")),
                    }
                    Ok(PostAction::None)
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
                        Err(e) => self.flash_error(format!("hex: {e}")),
                    }
                    Ok(PostAction::None)
                }
            }
            ActivateIntent::Edit => {
                let mut argv = shell::resolve_editor();
                if argv.is_empty() {
                    return Ok(PostAction::None);
                }
                let program = argv.remove(0);
                argv.push(path.to_string_lossy().into_owned());
                Ok(PostAction::Spawn {
                    program,
                    args: argv,
                    pause_after: false,
                })
            }
        }
    }

    fn refresh_listing(&mut self) {
        if let Ok(new) = Listing::read(&self.listing.dir) {
            self.listing = new;
            self.rebuild_rows();
        }
    }

    fn climb(&mut self) -> Result<()> {
        if self.view == View::Inventory {
            self.view = View::Dir;
            self.rebuild_rows();
            return Ok(());
        }
        // Remember the directory we're leaving so we can focus it after chdir.
        let prev_name = self
            .listing
            .dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        if let Some(parent) = self.listing.dir.parent().map(Path::to_path_buf) {
            if let Err(e) = self.chdir(&parent) {
                self.flash_error(format!("chdir: {e}"));
                return Ok(());
            }
            // Place cursor on the directory we just came from.
            if let Some(name) = prev_name {
                if let Some(idx) = self.rows.iter().position(|r| r.display == name || r.display == format!("{name}/")) {
                    self.cursor.index = idx;
                }
            }
        }
        Ok(())
    }

    fn chdir(&mut self, path: &Path) -> Result<()> {
        // Canonicalize so listing.dir and the process cwd always agree and
        // the status bar shows a clean absolute path.
        let canonical = std::fs::canonicalize(path)?;
        let new_listing = Listing::read(&canonical)?;
        // Save previous directory for `''` (jump-back).
        if self.listing.dir != canonical {
            self.prev_dir = Some(self.listing.dir.clone());
        }
        // Keep the process cwd in sync with navigation so subprocesses
        // (`!cmd`, `$SHELL`, `chmod`, editor, pager) run in the directory
        // the user is looking at.
        let _ = std::env::set_current_dir(&canonical);
        self.listing = new_listing;
        self.git_info = crate::sysinfo::git_status(&canonical);
        self.picks.clear();
        self.cursor = Cursor::new();
        self.view = View::Dir;
        self.rebuild_rows();
        Ok(())
    }

    fn toggle_pick_cursor(&mut self) {
        if self.view != View::Dir {
            return;
        }
        if let Some(row) = self.rows.get(self.cursor.index) {
            self.picks.toggle(&row.path);
        }
    }

    fn toggle_all_picks(&mut self) {
        if self.view != View::Dir {
            return;
        }
        // If anything in the current visible set is unpicked, pick all; else clear.
        let any_unpicked = self.rows.iter().any(|r| !self.picks.contains(&r.path));
        if any_unpicked {
            for r in &self.rows {
                self.picks.insert(&r.path);
            }
        } else {
            self.picks.clear();
        }
    }

    fn take(&mut self) {
        if self.view != View::Dir {
            return;
        }
        let to_take: Vec<PathBuf> = if !self.picks.is_empty() {
            self.picks.iter().cloned().collect()
        } else if let Some(row) = self.rows.get(self.cursor.index) {
            vec![row.path.clone()]
        } else {
            vec![]
        };
        self.inventory.extend(to_take);
        self.rebuild_rows();
    }

    fn drop_cursor(&mut self) {
        // Drop removes from inventory. In Dir view: the cursor item if taken.
        // In Inventory view: the cursor inventory item, and then re-focus.
        let Some(row) = self.rows.get(self.cursor.index) else {
            return;
        };
        let path = row.path.clone();
        self.inventory.remove(&path);
        self.rebuild_rows();
    }

    fn toggle_inventory_view(&mut self) {
        self.view = match self.view {
            View::Dir => View::Inventory,
            View::Inventory => View::Dir,
        };
        self.cursor = Cursor::new();
        self.rebuild_rows();
    }

    fn focus_on_path(&mut self, path: &Path) {
        if let Some(i) = self.rows.iter().position(|r| r.path == path) {
            self.cursor.index = i;
        }
    }

    // --- Row construction -------------------------------------------------

    fn rebuild_rows(&mut self) {
        self.rows = match self.view {
            View::Dir => self
                .listing
                .entries
                .iter()
                .filter(|e| !self.masks.hides(&e.name))
                .map(row_from_entry)
                .collect(),
            View::Inventory => self
                .inventory
                .paths()
                .map(|p| RowData {
                    path: p.clone(),
                    display: p.display().to_string(),
                    kind: detect_kind(p),
                })
                .collect(),
        };
        self.cursor.clamp(self.rows.len());
    }
}

/// Search matcher: prefix match for plain text, glob for anything with
/// `*`, `?`, or `[`. Both modes are case-insensitive.
enum Matcher {
    Prefix(String),
    Glob(Pattern),
    /// An invalid glob produced by a malformed pattern. Matches nothing.
    Never,
}

impl Matcher {
    fn build(query: &str) -> Self {
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

    fn matches(&self, name: &str) -> bool {
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
const fn is_cspy_meta_when_pane_focused(
    key: crossterm::event::KeyEvent,
    resolver_pending: bool,
) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};
    // Continuation of a multi-key cspy sequence must stay with cspy.
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
    new_dir: &Path,
) {
    use notify::{RecursiveMode, Watcher};
    let Some(w) = fs_watcher else {
        return;
    };
    if active.as_deref() == Some(new_dir) {
        return;
    }
    if let Some(old) = active.as_ref() {
        let _ = w.unwatch(old);
    }
    if w.watch(new_dir, RecursiveMode::NonRecursive).is_ok() {
        *active = Some(new_dir.to_path_buf());
    } else {
        *active = None;
    }
}

/// Spawn a shell command with piped stdout/stderr. Returns the child handle
/// and a channel that will deliver the collected stdout bytes when EOF.
fn spawn_capture(cmd: &str) -> Result<(std::process::Child, std::sync::mpsc::Receiver<Vec<u8>>)> {
    use std::io::Read as _;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;

    // stdout is piped so isatty(1) returns false → programs switch to
    // batch/non-interactive mode automatically (top, vim, less, etc.).
    //
    // We set COLUMNS/LINES so tools that format batch output (ls, ps,
    // top -l) know our viewport width. We also set the color-force env
    // vars so tools that check them emit ANSI even on a pipe.
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let mut child = Command::new("sh")
        .args(["-c", cmd])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        let _ = tx.send(buf);
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
    // grep that found nothing) are normal and should not crash cspy.
    let _ = std::process::Command::new(program).args(args).status();

    if pause_after {
        let mut stdout = std::io::stdout();
        write!(stdout, "\n[cspy] press any key to continue…")?;
        stdout.flush()?;
        // We're not in raw mode right now, so read a single byte directly
        // from stdin. Any key (including Enter) unblocks.
        let mut byte = [0u8; 1];
        let _ = std::io::Read::read(&mut std::io::stdin(), &mut byte);
    }

    resume_tui(terminal)?;
    Ok(())
}

fn row_from_entry(e: &Entry) -> RowData {
    RowData {
        path: e.path.clone(),
        display: e.display_name(),
        kind: e.kind,
    }
}

fn detect_kind(p: &Path) -> EntryKind {
    match std::fs::symlink_metadata(p) {
        Ok(md) if md.is_dir() => EntryKind::Dir,
        Ok(md) if md.file_type().is_symlink() => EntryKind::Symlink,
        Ok(md) if md.is_file() => EntryKind::File,
        _ => EntryKind::Other,
    }
}

const fn on_off(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
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
