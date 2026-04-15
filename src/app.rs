//! Top-level application state and event loop.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use glob::Pattern;
use ratatui::Frame;

use crate::fs::{Entry, EntryKind, Listing};
use crate::keymap::{Action, Resolver, ResolverOutcome};
use crate::shell;
use crate::state::{Cursor, IgnoreMasks, Inventory, Picks};
use crate::ui::{
    help, layout,
    list_view::{Grid, ListView, Row},
    prompt::PromptLine,
    status::StatusBar,
};
use crate::{resume_tui, suspend_tui, Tui};

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

struct Prompt {
    kind: PromptKind,
    prefix: String,
    buffer: String,
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
}

pub struct App {
    listing: Listing,
    picks: Picks,
    inventory: Inventory,
    masks: IgnoreMasks,
    view: View,
    cursor: Cursor,
    resolver: Resolver,
    mode: Mode,
    /// When true, the key-bindings overlay is drawn on top of everything
    /// and the next keypress dismisses it.
    help_visible: bool,
    /// Most recent search term; `n` / `N` use this.
    last_search: Option<String>,
    user_host: String,
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
        let cwd = std::env::current_dir().context("getting current directory")?;
        let listing = Listing::read(&cwd)?;
        let mut app = Self {
            listing,
            picks: Picks::new(),
            inventory: Inventory::new(),
            masks: IgnoreMasks::default(),
            view: View::Dir,
            cursor: Cursor::new(),
            resolver: Resolver::new(),
            mode: Mode::Normal,
            help_visible: false,
            last_search: None,
            user_host: user_host_string(),
            should_quit: false,
            rows: Vec::new(),
            last_grid: Grid {
                cols: 1,
                rows: 1,
                col_widths: vec![20],
            },
        };
        app.rebuild_rows();
        Ok(app)
    }

    pub fn run(&mut self, terminal: &mut Tui) -> Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;
            if event::poll(Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()? {
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
                        }
                    }
                }
            }
        }
        Ok(())
    }

    // --- Rendering --------------------------------------------------------

    fn render(&mut self, frame: &mut Frame) {
        let panels = layout::split(frame.area());

        let (path, suffix) = self.header_parts();
        StatusBar {
            user_host: &self.user_host,
            path: &path,
            suffix: &suffix,
        }
        .render(frame, panels.status);

        let rows = self.build_rows();
        let probe = ListView {
            rows: &rows,
            cursor: self.cursor.index,
            view_top: self.cursor.view_top,
            empty_marker: self.view == View::Dir,
        };
        self.last_grid = probe.grid(panels.list);
        self.ensure_cursor_visible();

        frame.render_widget(
            ListView {
                rows: &rows,
                cursor: self.cursor.index,
                view_top: self.cursor.view_top,
                empty_marker: self.view == View::Dir,
            },
            panels.list,
        );

        if let Mode::Prompting(p) = &self.mode {
            PromptLine {
                prefix: &p.prefix,
                buffer: &p.buffer,
            }
            .render(frame, panels.prompt);
        }

        // Help overlay is painted last so it sits on top of everything.
        if self.help_visible {
            help::render(frame, frame.area());
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
        // While help is up, any keypress dismisses it and is then
        // swallowed — so the user doesn't accidentally trigger an action.
        if self.help_visible {
            let _ = key;
            self.help_visible = false;
            return Ok(PostAction::None);
        }
        if matches!(self.mode, Mode::Prompting(_)) {
            return Ok(self.handle_prompt_key(key));
        }
        if let ResolverOutcome::Action(action) = self.resolver.feed(key) {
            return self.apply(&action);
        }
        Ok(PostAction::None)
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) -> PostAction {
        // Esc cancels; Backspace on an empty buffer cancels too (it's a
        // natural reach — when you've typed nothing and hit Backspace again
        // you clearly meant to back out).
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
                let expanded = shell::expand_percent(&prompt.buffer, &self.selection_paths());
                sh_c(&expanded, true)
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
        }
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
            self.chdir(&canonical)?;
        } else if let Some(parent) = canonical.parent() {
            self.chdir(parent)?;
            self.focus_on_path(&canonical);
        }
        Ok(())
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
        let target_col =
            (current_col + delta).rem_euclid(num_cols as isize) as usize;
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
                    self.chdir(&home)?;
                }
            }

            Action::TogglePick => self.toggle_pick_cursor(),
            Action::PickPatternPrompt => {
                if self.view == View::Dir {
                    self.mode = Mode::Prompting(Prompt {
                        kind: PromptKind::PatternPick,
                        prefix: "pick pattern: ".to_string(),
                        buffer: String::new(),
                    });
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

            Action::ShellPrompt => {
                self.mode = Mode::Prompting(Prompt {
                    kind: PromptKind::ShellCmd,
                    prefix: "!".to_string(),
                    buffer: String::new(),
                });
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
                let template = format!("chmod +{mode_char} %");
                let cmd = shell::expand_percent(&template, &paths);
                return Ok(sh_c(&cmd, true));
            }

            Action::SearchPrompt => {
                self.mode = Mode::Prompting(Prompt {
                    kind: PromptKind::Search {
                        saved_cursor: self.cursor.index,
                    },
                    prefix: "/".to_string(),
                    buffer: String::new(),
                });
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
                self.mode = Mode::Prompting(Prompt {
                    kind: PromptKind::Jump,
                    prefix: "jump to: ".to_string(),
                    buffer: String::new(),
                });
            }
            Action::Help => {
                self.help_visible = true;
            }

            Action::Redraw | Action::Noop => {}
            Action::Quit => self.should_quit = true,
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
            self.chdir(&target_dir)?;
            self.view = View::Dir;
            self.focus_on_path(&path);
            self.rebuild_rows();
            if kind == EntryKind::Dir {
                return Ok(PostAction::None);
            }
        }

        if kind == EntryKind::Dir {
            self.chdir(&path)?;
            return Ok(PostAction::None);
        }

        // File: dispatch based on intent.
        match intent {
            ActivateIntent::Display => {
                if shell::looks_like_text(&path) {
                    let mut argv = shell::resolve_pager();
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
                } else {
                    // Binary file: nothing to page. Silent no-op for now.
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
        if let Some(parent) = self.listing.dir.parent().map(Path::to_path_buf) {
            self.chdir(&parent)?;
        }
        Ok(())
    }

    fn chdir(&mut self, path: &Path) -> Result<()> {
        // Canonicalize so listing.dir and the process cwd always agree and
        // the status bar shows a clean absolute path.
        let canonical = std::fs::canonicalize(path)?;
        let new_listing = Listing::read(&canonical)?;
        // Keep the process cwd in sync with navigation so subprocesses
        // (`!cmd`, `$SHELL`, `chmod`, editor, pager) run in the directory
        // the user is looking at.
        let _ = std::env::set_current_dir(&canonical);
        self.listing = new_listing;
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
