//! Domain state for the application — everything testable without a terminal.
//!
//! `AppState` holds navigation, selection, filtering, bookmarks, input mode,
//! config, history, and cached info. Event handlers that operate on pure
//! domain logic live here; the `App` shell in `mod.rs` owns terminal state
//! (pager widget, pane tabs, pty handles) and delegates to `AppState`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::config::Config;
use crate::fs::Listing;
use crate::fs;
use crate::keymap::{Action, Resolver, UserKeymap};
use crate::state::{Cursor, History, IgnoreMasks, Inventory, Mark, Marks, Picks};
use crate::ui::list_view::Grid;

use super::{
    row_from_entry, FlashKind, FlashMessage, Matcher, Mode, PostAction, Prompt,
    PromptKind, RowData, View,
};

/// Result of `AppState::dispatch_command` — tells the `App` caller what to do.
#[derive(Debug)]
pub enum CommandResult {
    /// Handled entirely by `AppState`. No terminal action needed.
    Handled,
    /// Open the marks view in the pager. Carries the lines.
    OpenPager { title: String, lines: Vec<String> },
    /// The input was a shell/pager/overlay command — caller should handle it.
    NotHandled,
}

/// Result of `AppState::dispatch_prompt` — tells the `App` caller what to do.
#[derive(Debug)]
pub enum PromptResult {
    /// Handled entirely by `AppState`.
    Handled,
    /// Needs terminal: caller handles this prompt kind.
    NotHandled,
}

/// Result of `AppState::apply` — tells the `App` caller what to do.
#[derive(Debug)]
pub enum ApplyResult {
    /// Handled entirely by `AppState`. Cursor already clamped.
    Handled,
    /// Open a pager with these contents.
    OpenPager(PagerRequest),
    /// Return this `PostAction` to the event loop (e.g. `Spawn` for `$SHELL`).
    Post(PostAction),
    /// Caller must handle this action (terminal-touching).
    NotHandled,
}

/// Description of a pager to open, without importing UI types.
#[derive(Debug)]
pub struct PagerRequest {
    pub title: String,
    pub lines: PagerLines,
    pub columns: u8,
}

/// Content for a pager — either plain strings or pre-styled lines.
#[derive(Debug)]
pub enum PagerLines {
    Plain(Vec<String>),
}

pub struct AppState {
    pub listing: Listing,
    pub picks: Picks,
    pub inventory: Inventory,
    pub marks: Marks,
    pub masks: IgnoreMasks,
    pub temp_filter: Option<String>,
    pub sort_order: crate::fs::listing::SortMode,
    pub view: View,
    pub cursor: Cursor,
    pub resolver: Resolver,
    pub user_keymap: UserKeymap,
    pub config: Config,
    pub mode: Mode,
    pub start_dir: PathBuf,
    pub prev_dir: Option<PathBuf>,
    pub last_search: Option<String>,
    pub last_captured_cmd: Option<String>,
    pub history: History,
    pub pane_history: History,
    pub flash: Option<FlashMessage>,
    pub should_quit: bool,
    pub quit_pending: Option<std::time::Instant>,
    pub git_info: Option<String>,
    pub git_files: std::collections::HashMap<String, crate::ui::list_view::GitFileStatus>,
    pub user_host: String,
    pub pending_new_tab_cmd: Option<String>,
    pub pending_worktrees: Option<Vec<PathBuf>>,
    pub pending_sessions: Option<Vec<crate::state::sessions::Session>>,
    pub pane_focused: bool,
    pub pane_height_pct: u16,
    pub rows: Vec<RowData>,
    pub last_grid: Grid,
}

impl AppState {
    // --- Cursor/navigation (Phase 1) ---

    pub const fn cursor_move_vertical(&mut self, delta: isize, len: usize) {
        if len == 0 {
            return;
        }
        let new_idx = (self.cursor.index as isize + delta).rem_euclid(len as isize);
        self.cursor.index = new_idx as usize;
    }

    /// `gg` — jump to the first entry of the current column.
    pub const fn goto_col_top(&mut self) {
        let rows_per_col = self.last_grid.rows as usize;
        if rows_per_col == 0 {
            self.cursor.index = 0;
            return;
        }
        let current_col = self.cursor.index / rows_per_col;
        self.cursor.index = current_col * rows_per_col;
    }

    /// `G` — jump to the last entry of the current column.
    pub fn goto_col_bottom(&mut self, len: usize) {
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

    /// Move the cursor by `delta` columns.
    pub fn cursor_move_columns(&mut self, delta: isize, rows_per_col: usize, len: usize) {
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
            return;
        }
        let target_idx = target_col * rows_per_col + current_row;
        if target_idx >= len {
            return;
        }
        self.cursor.index = target_idx;
    }

    pub fn ensure_cursor_visible(&mut self) {
        let per_page = self.last_grid.items_per_page();
        if per_page == 0 || self.rows.is_empty() {
            self.cursor.view_top = 0;
            return;
        }
        let page = self.cursor.index / per_page;
        self.cursor.view_top = page * per_page;
    }

    pub fn find_match(&self, query: &str, from: usize, backward: bool) -> Option<usize> {
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

    // --- Selection/data helpers (Phase 2) ---

    pub fn flash_info<S: Into<String>>(&mut self, text: S) {
        self.flash = Some(FlashMessage {
            text: text.into(),
            kind: FlashKind::Info,
        });
    }

    pub fn flash_error<S: Into<String>>(&mut self, text: S) {
        self.flash = Some(FlashMessage {
            text: text.into(),
            kind: FlashKind::Error,
        });
    }

    pub fn selection_paths(&self) -> Vec<&Path> {
        if self.view == View::Dir && !self.picks.is_empty() {
            self.picks.iter().map(PathBuf::as_path).collect()
        } else if let Some(row) = self.rows.get(self.cursor.index) {
            vec![row.path.as_path()]
        } else {
            Vec::new()
        }
    }

    pub fn set_mark(&mut self, letter: char) {
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

    pub fn jump_to_mark(&mut self, letter: char) {
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

    pub fn toggle_pick_cursor(&mut self) {
        if self.view != View::Dir {
            return;
        }
        if let Some(row) = self.rows.get(self.cursor.index) {
            self.picks.toggle(&row.path);
        }
    }

    pub fn toggle_all_picks(&mut self) {
        if self.view != View::Dir {
            return;
        }
        let any_unpicked = self.rows.iter().any(|r| !self.picks.contains(&r.path));
        if any_unpicked {
            for r in &self.rows {
                self.picks.insert(&r.path);
            }
        } else {
            self.picks.clear();
        }
    }

    /// Yank files into the inventory cache. Takes picks if any, else
    /// cursor item. Only regular files are accepted.
    pub fn take(&mut self) -> Option<String> {
        if self.view != View::Dir {
            return None;
        }
        let to_take: Vec<PathBuf> = if !self.picks.is_empty() {
            self.picks.iter().cloned().collect()
        } else if let Some(row) = self.rows.get(self.cursor.index) {
            vec![row.path.clone()]
        } else {
            vec![]
        };
        let (count, err) = self.inventory.yank_many(&to_take);
        self.rebuild_rows();
        if count > 0 {
            return Some(format!("yanked {count} file(s) to inventory"));
        }
        err
    }

    /// Remove the cursor item from inventory (move to graveyard).
    pub fn drop_cursor(&mut self) {
        self.inventory.remove_at(self.cursor.index);
        self.rebuild_rows();
        self.cursor.clamp(self.rows.len());
    }

    pub fn toggle_inventory_view(&mut self) {
        self.view = match self.view {
            View::Dir => View::Inventory,
            View::Inventory => View::Dir,
        };
        self.cursor = Cursor::new();
        self.rebuild_rows();
    }

    pub fn focus_on_path(&mut self, path: &Path) {
        if let Some(i) = self.rows.iter().position(|r| r.path == path) {
            self.cursor.index = i;
        }
    }

    pub fn rebuild_rows(&mut self) {
        self.rows = match self.view {
            View::Dir => {
                let base: Vec<RowData> = self
                    .listing
                    .entries
                    .iter()
                    .filter(|e| !self.masks.hides(&e.name))
                    .map(row_from_entry)
                    .collect();
                self.apply_temp_filter(base)
            }
            View::Inventory => self
                .inventory
                .items()
                .map(|item| RowData {
                    path: item.orig_path.clone(),
                    display: format!("{}  ← {}", item.filename, item.orig_path.parent().unwrap_or(Path::new("/")).display()),
                    kind: crate::fs::EntryKind::File,
                })
                .collect(),
        };
        self.cursor.clamp(self.rows.len());
    }

    pub fn apply_temp_filter(&self, rows: Vec<RowData>) -> Vec<RowData> {
        let Some(ref pattern) = self.temp_filter else {
            return rows;
        };
        if pattern == "!" {
            rows.into_iter()
                .filter(|r| self.picks.contains(&r.path))
                .collect()
        } else {
            let matcher = Matcher::build(pattern);
            rows.into_iter()
                .filter(|r| matcher.matches(&r.display))
                .collect()
        }
    }

    pub fn refresh_listing(&mut self) {
        if let Ok(new) = Listing::read(&self.listing.dir) {
            self.listing = new;
            self.git_files = crate::sysinfo::git_file_statuses(&self.listing.dir);
            self.rebuild_rows();
        }
    }

    pub fn chdir(&mut self, path: &Path) -> Result<()> {
        let canonical = std::fs::canonicalize(path)?;
        let new_listing = Listing::read(&canonical)?;
        if self.listing.dir != canonical {
            self.prev_dir = Some(self.listing.dir.clone());
        }
        let _ = std::env::set_current_dir(&canonical);
        self.listing = new_listing;
        self.listing.sort(self.sort_order);
        self.git_info = crate::sysinfo::git_status(&canonical);
        self.git_files = crate::sysinfo::git_file_statuses(&canonical);
        self.picks.clear();
        self.temp_filter = None;
        self.cursor = Cursor::new();
        self.view = View::Dir;
        self.rebuild_rows();
        Ok(())
    }

    pub fn climb(&mut self) {
        if self.view == View::Inventory {
            self.view = View::Dir;
            self.rebuild_rows();
            return;
        }
        let prev_name = self
            .listing
            .dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        if let Some(parent) = self.listing.dir.parent().map(Path::to_path_buf) {
            if let Err(e) = self.chdir(&parent) {
                self.flash_error(format!("chdir: {e}"));
                return;
            }
            if let Some(name) = prev_name {
                if let Some(idx) = self
                    .rows
                    .iter()
                    .position(|r| r.display == name || r.display == format!("{name}/"))
                {
                    self.cursor.index = idx;
                }
            }
        }
    }

    // --- Action dispatch (pure-domain arms) ---

    /// Handle the pure-domain arms of `Action` dispatch.
    ///
    /// Returns `ApplyResult::Handled` when the action was fully processed
    /// (cursor is clamped before returning), `ApplyResult::OpenPager` when
    /// the caller should open a pager, `ApplyResult::Post` for a `PostAction`,
    /// or `ApplyResult::NotHandled` when the caller must handle the action
    /// (terminal-touching: pager, pane, theme, redraw, etc.).
    pub fn apply(&mut self, action: &Action) -> ApplyResult {
        let len = self.rows.len();
        let rows_per_col = self.last_grid.rows as usize;
        let per_page = self.last_grid.items_per_page();

        match action {
            // -- Cursor motion --
            Action::Up(n) => self.cursor_move_vertical(-(*n as isize), len),
            Action::Down(n) => self.cursor_move_vertical(*n as isize, len),
            Action::Left(n) => self.cursor_move_columns(-(*n as isize), rows_per_col, len),
            Action::Right(n) => self.cursor_move_columns(*n as isize, rows_per_col, len),
            Action::PageUp => self.cursor_move_vertical(-(per_page as isize), len),
            Action::PageDown => self.cursor_move_vertical(per_page as isize, len),
            Action::GotoFirst => self.goto_col_top(),
            Action::GotoLast => self.goto_col_bottom(len),

            // -- Navigation --
            Action::Climb => self.climb(),
            Action::Home => {
                if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                    if let Err(e) = self.chdir(&home) {
                        self.flash_error(format!("chdir: {e}"));
                    }
                }
            }

            // -- Picks --
            Action::TogglePick => self.toggle_pick_cursor(),
            Action::PickPatternPrompt => {
                if self.view == View::Dir {
                    self.mode =
                        Mode::Prompting(Prompt::simple(PromptKind::PatternPick, "pick pattern: "));
                }
            }
            Action::PickToggleAll => self.toggle_all_picks(),

            // -- Inventory --
            Action::Take => {
                match self.take() {
                    Some(msg) if msg.starts_with("yanked") => self.flash_info(msg),
                    Some(err) => self.flash_error(err),
                    None => {}
                }
            }
            Action::Untake => {
                if self.view != View::Dir {
                    return ApplyResult::Handled;
                }
                if let Some(row) = self.rows.get(self.cursor.index) {
                    let path = row.path.clone();
                    if self.inventory.contains(&path) {
                        // Find and remove by original path.
                        let id = self.inventory.items()
                            .find(|i| i.orig_path == path)
                            .map(|i| i.id.clone());
                        if let Some(id) = id {
                            self.inventory.remove_by_id(&id);
                            self.flash_info("removed from inventory");
                        }
                    } else {
                        self.flash_error("not in inventory");
                    }
                }
                self.rebuild_rows();
            }
            Action::Drop => {
                // In dir view, p = put (handled by App, not here).
                // This arm only fires from inventory view fallthrough.
                self.drop_cursor();
            }
            Action::ToggleInventoryView => self.toggle_inventory_view(),
            Action::EmptyInventory => {
                self.inventory.clear();
                self.rebuild_rows();
            }

            // -- Masks & filtering --
            Action::ToggleMask(n) => {
                if *n == 1 {
                    self.masks.toggle_mask1();
                } else if *n == 2 {
                    self.masks.toggle_mask2();
                }
                self.rebuild_rows();
            }
            Action::LimitPrompt => {
                let prefix = if self.temp_filter.is_some() {
                    "limit (active)="
                } else {
                    "limit="
                };
                self.mode = Mode::Prompting(Prompt::simple(PromptKind::Limit, prefix));
            }
            Action::CommandPrompt => {
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::Command, ":"));
            }

            // -- Shell prompts --
            Action::ShellCapturedPrompt => {
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::ShellCmdCaptured, "!"));
            }
            Action::ShellForegroundPrompt => {
                self.mode = Mode::Prompting(Prompt::shell(PromptKind::ShellCmd, ";"));
            }
            Action::StartShell => {
                let sh = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
                return ApplyResult::Post(PostAction::Spawn {
                    program: sh,
                    args: vec![],
                    pause_after: false,
                });
            }

            // -- Search --
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

            // -- Navigation prompts --
            Action::JumpPrompt => {
                self.mode = Mode::Prompting(Prompt::simple(PromptKind::Jump, "jump to: "));
            }

            // -- File operation prompts --
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

            // -- Long listing (pager) --
            Action::LongList => {
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
                self.cursor.clamp(self.rows.len());
                return ApplyResult::OpenPager(PagerRequest {
                    title,
                    lines: PagerLines::Plain(lines),
                    columns: 1,
                });
            }

            // -- File type --
            Action::FileType => {
                let paths = self.selection_paths();
                if paths.is_empty() {
                    self.cursor.clamp(self.rows.len());
                    return ApplyResult::Post(PostAction::None);
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
                    self.cursor.clamp(self.rows.len());
                    return ApplyResult::OpenPager(PagerRequest {
                        title: "file types".to_string(),
                        lines: PagerLines::Plain(lines),
                        columns: 1,
                    });
                }
            }

            // -- Marks --
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

            // -- Info --
            Action::Date => self.flash_info(crate::sysinfo::format_now()),
            Action::Version => {
                self.flash_info(format!(
                    "\u{1f336}\u{fe0f} spyc {}",
                    env!("CARGO_PKG_VERSION")
                ));
            }
            Action::SetEnvPrompt => {
                self.mode =
                    Mode::Prompting(Prompt::simple(PromptKind::SetEnv, "setenv NAME=VALUE: "));
            }

            // -- Worktree prompts (pure state: just set mode) --
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

            // -- No-op --
            Action::Noop => {}

            // -- Everything else stays in App --
            _ => return ApplyResult::NotHandled,
        }

        self.cursor.clamp(self.rows.len());
        ApplyResult::Handled
    }

    // --- Command / prompt dispatch (pure-domain arms) ---

    /// Handle the pure-domain arms of `:` commands.
    ///
    /// Returns `CommandResult::Handled` when the command was fully processed,
    /// `CommandResult::OpenPager` when the caller should open the pager with
    /// the supplied lines, or `CommandResult::NotHandled` when the caller
    /// (which owns the terminal) must process it.
    pub fn dispatch_command(&mut self, input: &str) -> CommandResult {
        let input = input.trim();
        if input.is_empty() {
            return CommandResult::Handled;
        }

        // :q / :quit
        if input == "q" || input == "quit" {
            self.should_quit = true;
            return CommandResult::Handled;
        }

        // :limit [pattern]
        if input == "limit" {
            self.temp_filter = None;
            self.flash_info("limit cleared");
            self.rebuild_rows();
            return CommandResult::Handled;
        }
        if let Some(pat) = input.strip_prefix("limit ") {
            let pat = pat.trim();
            if pat.is_empty() {
                self.temp_filter = None;
                self.flash_info("limit cleared");
            } else if pat == "!" {
                self.temp_filter = Some("!".to_string());
                self.flash_info("limit: picks only");
            } else {
                self.temp_filter = Some(pat.to_string());
                self.flash_info(format!("limit: {pat}"));
            }
            self.rebuild_rows();
            return CommandResult::Handled;
        }

        // :cd <path>
        if input == "cd" {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
            match self.chdir(&PathBuf::from(home)) {
                Ok(()) => {}
                Err(e) => self.flash_error(format!("cd: {e}")),
            }
            return CommandResult::Handled;
        }
        if let Some(raw) = input.strip_prefix("cd ") {
            let raw = raw.trim();
            if raw.is_empty() {
                self.flash_error("cd: missing path");
                return CommandResult::Handled;
            }
            let path = crate::paths::expand(raw);
            match self.chdir(&path) {
                Ok(()) => {}
                Err(e) => self.flash_error(format!("cd: {e}")),
            }
            return CommandResult::Handled;
        }

        // :sort [mode]
        if input == "sort" {
            self.flash_info(format!("sort: {}", self.sort_order));
            return CommandResult::Handled;
        }
        if let Some(mode_str) = input.strip_prefix("sort ") {
            let mode_str = mode_str.trim();
            match crate::fs::listing::SortMode::parse(mode_str) {
                Some(mode) => {
                    self.sort_order = mode;
                    self.listing.sort(mode);
                    self.rebuild_rows();
                    self.flash_info(format!("sort: {mode}"));
                }
                None => self.flash_error(format!(
                    "unknown sort mode: {mode_str} (name|size|mtime|ext)"
                )),
            }
            return CommandResult::Handled;
        }

        // :marks
        if input == "marks" {
            if self.marks.entries.is_empty() {
                self.flash_info("no marks set");
                return CommandResult::Handled;
            }
            let lines: Vec<String> = self
                .marks
                .entries
                .iter()
                .map(|(key, mark)| {
                    let focus = match &mark.focus {
                        Some(p) => format!("  → {}", p.display()),
                        None => String::new(),
                    };
                    format!("  {key}  {}{focus}", mark.dir.display())
                })
                .collect();
            return CommandResult::OpenPager {
                title: "marks".to_string(),
                lines,
            };
        }

        // :set key=value
        if let Some(assignment) = input.strip_prefix("set ") {
            let assignment = assignment.trim();
            if let Some((key, value)) = assignment.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "sort" => match crate::fs::listing::SortMode::parse(value) {
                        Some(mode) => {
                            self.sort_order = mode;
                            self.listing.sort(mode);
                            self.rebuild_rows();
                            self.flash_info(format!("sort={mode}"));
                        }
                        None => self.flash_error(format!("invalid sort mode: {value}")),
                    },
                    _ => self.flash_error(format!("unknown setting: {key}")),
                }
            } else {
                self.flash_error("usage: :set key=value");
            }
            return CommandResult::Handled;
        }

        // Commands that need terminal/pager/overlay: :!cmd, :!!, :;cmd, :bprev, :bnext
        if input.starts_with('!')
            || input.starts_with(';')
            || input == "bprev"
            || input == "bnext"
        {
            return CommandResult::NotHandled;
        }

        // Unknown command
        self.flash_error(format!("unknown command: {input}"));
        CommandResult::Handled
    }

    /// Handle the pure-domain arms of prompt submission.
    ///
    /// Returns `PromptResult::Handled` when fully processed, or
    /// `PromptResult::NotHandled` when the caller must handle it (terminal I/O).
    pub fn dispatch_prompt(&mut self, kind: &PromptKind, buffer: &str) -> PromptResult {
        match kind {
            PromptKind::PatternPick => {
                if let Ok(pat) = glob::Pattern::new(buffer) {
                    for e in &self.listing.entries {
                        if pat.matches(&e.name) {
                            self.picks.insert(&e.path);
                        }
                    }
                }
                PromptResult::Handled
            }
            PromptKind::Search { .. } => {
                if !buffer.is_empty() {
                    self.last_search = Some(buffer.to_string());
                }
                PromptResult::Handled
            }
            PromptKind::Jump => {
                let trimmed = buffer.trim();
                if !trimmed.is_empty() {
                    let _ = self.jump_to(trimmed);
                }
                PromptResult::Handled
            }
            PromptKind::MakeDir => {
                let name = buffer.trim();
                if !name.is_empty() {
                    let target = crate::paths::expand(name);
                    let resolved = if target.is_absolute() {
                        target
                    } else {
                        self.listing.dir.join(&target)
                    };
                    match std::fs::create_dir_all(&resolved) {
                        Ok(()) => self.flash_info(format!("created {}", resolved.display())),
                        Err(e) => self.flash_error(format!("error: {e}")),
                    }
                    self.refresh_listing();
                }
                PromptResult::Handled
            }
            PromptKind::SetEnv => {
                let line = buffer.trim();
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
                PromptResult::Handled
            }
            PromptKind::Limit => {
                let pattern = buffer.trim();
                if pattern.is_empty() {
                    self.temp_filter = None;
                    self.flash_info("limit cleared");
                } else if pattern == "!" {
                    self.temp_filter = Some("!".to_string());
                    self.flash_info("limit: picks only");
                } else {
                    self.temp_filter = Some(pattern.to_string());
                    self.flash_info(format!("limit: {pattern}"));
                }
                self.rebuild_rows();
                PromptResult::Handled
            }
            PromptKind::WorktreeNewBranch => {
                let branch = buffer.trim();
                if branch.is_empty() {
                    return PromptResult::Handled;
                }
                match crate::sysinfo::git_worktree_add(&self.listing.dir, branch) {
                    Ok(path) => {
                        self.flash_info(format!("created worktree: {}", path.display()));
                        if let Err(e) = self.chdir(&path) {
                            self.flash_error(format!("chdir: {e}"));
                        }
                    }
                    Err(e) => self.flash_error(format!("worktree add: {e}")),
                }
                PromptResult::Handled
            }
            PromptKind::WorktreeDeleteConfirm => {
                let confirmed = buffer.trim().eq_ignore_ascii_case("y");
                if !confirmed {
                    return PromptResult::Handled;
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
                PromptResult::Handled
            }
            PromptKind::PaneNewTabCmd => {
                let cmd = buffer.trim().to_string();
                if cmd.is_empty() {
                    return PromptResult::Handled;
                }
                self.pending_new_tab_cmd = Some(cmd);
                let cwd_default = self.listing.dir.display().to_string();
                let mut p = Prompt::shell(PromptKind::PaneNewTabCwd, "pane cwd: ");
                p.buffer.clone_from(&cwd_default);
                if let Some(ed) = p.editor.as_mut() {
                    ed.set_content(&cwd_default);
                }
                self.mode = Mode::Prompting(p);
                PromptResult::Handled
            }
            PromptKind::RemoveConfirm => PromptResult::Handled,
            // These need terminal/overlay/pager — caller handles them.
            PromptKind::ShellCmd
            | PromptKind::ShellCmdCaptured
            | PromptKind::CopyTo
            | PromptKind::MoveTo
            | PromptKind::PaneNewTabCwd
            | PromptKind::PaneRenameTab
            | PromptKind::Command => PromptResult::NotHandled,
        }
    }

    pub fn jump_to(&mut self, target: &str) -> Result<()> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::entry::EntryKind;
    use crate::fs::listing::SortMode;

    /// Build a minimal `AppState` for testing. Uses an empty listing
    /// and sensible defaults — no disk I/O, no terminal.
    fn test_state() -> AppState {
        AppState {
            listing: Listing::empty(PathBuf::from("/tmp/test")),
            picks: Picks::new(),
            inventory: Inventory::new(),
            marks: Marks::default(),
            masks: IgnoreMasks::default(),
            temp_filter: None,
            sort_order: SortMode::Name,
            view: View::Dir,
            cursor: Cursor::new(),
            resolver: Resolver::new(),
            user_keymap: UserKeymap::default(),
            config: Config::default(),
            mode: Mode::Normal,
            start_dir: PathBuf::from("/tmp/test"),
            prev_dir: None,
            last_search: None,
            last_captured_cmd: None,
            history: History::load_file("test_state_h"),
            pane_history: History::load_file("test_state_ph"),
            flash: None,
            should_quit: false,
            quit_pending: None,
            git_info: None,
            git_files: std::collections::HashMap::new(),
            user_host: "test@host".to_string(),
            pending_new_tab_cmd: None,
            pending_worktrees: None,
            pending_sessions: None,
            pane_focused: false,
            pane_height_pct: 30,
            rows: Vec::new(),
            last_grid: Grid { cols: 1, rows: 20, col_widths: vec![20] },
        }
    }

    /// Build a test state with named rows (simulating a directory listing).
    fn state_with_rows(names: &[&str]) -> AppState {
        let mut s = test_state();
        s.rows = names
            .iter()
            .map(|n| RowData {
                path: PathBuf::from(format!("/tmp/test/{n}")),
                display: n.to_string(),
                kind: EntryKind::File,
            })
            .collect();
        s
    }

    // ── cursor_move_vertical ──────────────────────────────────────

    #[test]
    fn vertical_move_wraps_forward() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.cursor.index = 2;
        s.cursor_move_vertical(1, 3);
        assert_eq!(s.cursor.index, 0);
    }

    #[test]
    fn vertical_move_wraps_backward() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.cursor.index = 0;
        s.cursor_move_vertical(-1, 3);
        assert_eq!(s.cursor.index, 2);
    }

    #[test]
    fn vertical_move_no_op_on_empty() {
        let mut s = test_state();
        s.cursor_move_vertical(1, 0);
        assert_eq!(s.cursor.index, 0);
    }

    #[test]
    fn vertical_move_multi_step() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
        s.cursor.index = 1;
        s.cursor_move_vertical(3, 5);
        assert_eq!(s.cursor.index, 4);
    }

    // ── goto_col_top / goto_col_bottom ────────────────────────────

    #[test]
    fn goto_col_top_first_column() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
        s.last_grid = Grid { cols: 2, rows: 3, col_widths: vec![10, 10] };
        s.cursor.index = 2; // last in first column
        s.goto_col_top();
        assert_eq!(s.cursor.index, 0);
    }

    #[test]
    fn goto_col_top_second_column() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
        s.last_grid = Grid { cols: 2, rows: 3, col_widths: vec![10, 10] };
        s.cursor.index = 4; // second column, row 1
        s.goto_col_top();
        assert_eq!(s.cursor.index, 3); // top of second column
    }

    #[test]
    fn goto_col_bottom_first_column() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
        s.last_grid = Grid { cols: 2, rows: 3, col_widths: vec![10, 10] };
        s.cursor.index = 0;
        s.goto_col_bottom(5);
        assert_eq!(s.cursor.index, 2); // last in first column (3 rows)
    }

    #[test]
    fn goto_col_bottom_partial_column() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
        s.last_grid = Grid { cols: 2, rows: 3, col_widths: vec![10, 10] };
        s.cursor.index = 3; // second column
        s.goto_col_bottom(5);
        assert_eq!(s.cursor.index, 4); // last entry in partial column
    }

    // ── cursor_move_columns ───────────────────────────────────────

    #[test]
    fn column_move_right() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
        s.last_grid = Grid { cols: 2, rows: 3, col_widths: vec![10, 10] };
        s.cursor.index = 1; // col 0, row 1
        s.cursor_move_columns(1, 3, 6);
        assert_eq!(s.cursor.index, 4); // col 1, row 1
    }

    #[test]
    fn column_move_left() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
        s.last_grid = Grid { cols: 2, rows: 3, col_widths: vec![10, 10] };
        s.cursor.index = 4; // col 1, row 1
        s.cursor_move_columns(-1, 3, 6);
        assert_eq!(s.cursor.index, 1); // col 0, row 1
    }

    #[test]
    fn column_move_clamps_at_edge() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
        s.last_grid = Grid { cols: 2, rows: 3, col_widths: vec![10, 10] };
        s.cursor.index = 4; // col 1, row 1
        s.cursor_move_columns(1, 3, 6); // try to go further right
        assert_eq!(s.cursor.index, 4); // stays put
    }

    #[test]
    fn column_move_single_column_noop() {
        let mut s = state_with_rows(&["a", "b"]);
        s.last_grid = Grid { cols: 1, rows: 10, col_widths: vec![20] };
        s.cursor.index = 0;
        s.cursor_move_columns(1, 10, 2);
        assert_eq!(s.cursor.index, 0); // no-op
    }

    // ── ensure_cursor_visible ─────────────────────────────────────

    #[test]
    fn ensure_visible_snaps_view_top() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        s.last_grid = Grid { cols: 1, rows: 3, col_widths: vec![20] }; // 3 items per page
        s.cursor.index = 5; // page 1 (items 3-5)
        s.ensure_cursor_visible();
        assert_eq!(s.cursor.view_top, 3);
    }

    #[test]
    fn ensure_visible_first_page() {
        let mut s = state_with_rows(&["a", "b", "c", "d"]);
        s.last_grid = Grid { cols: 1, rows: 3, col_widths: vec![20] };
        s.cursor.index = 1;
        s.ensure_cursor_visible();
        assert_eq!(s.cursor.view_top, 0);
    }

    // ── find_match ────────────────────────────────────────────────

    #[test]
    fn find_prefix_match() {
        let s = state_with_rows(&["alpha", "beta", "gamma"]);
        assert_eq!(s.find_match("b", 0, false), Some(1));
    }

    #[test]
    fn find_wraps_around() {
        let s = state_with_rows(&["alpha", "beta", "gamma"]);
        assert_eq!(s.find_match("a", 2, false), Some(0)); // wraps from gamma to alpha
    }

    #[test]
    fn find_backward() {
        let s = state_with_rows(&["alpha", "beta", "gamma"]);
        assert_eq!(s.find_match("b", 2, true), Some(1));
    }

    #[test]
    fn find_no_match() {
        let s = state_with_rows(&["alpha", "beta"]);
        assert_eq!(s.find_match("xyz", 0, false), None);
    }

    #[test]
    fn find_glob_pattern() {
        let s = state_with_rows(&["foo.rs", "bar.txt", "baz.rs"]);
        assert_eq!(s.find_match("*.rs", 0, false), Some(0));
        assert_eq!(s.find_match("*.rs", 1, false), Some(2));
    }

    #[test]
    fn find_empty_rows() {
        let s = test_state();
        assert_eq!(s.find_match("a", 0, false), None);
    }

    // ── flash ─────────────────────────────────────────────────────

    #[test]
    fn flash_info_sets_message() {
        let mut s = test_state();
        s.flash_info("hello");
        let flash = s.flash.as_ref().unwrap();
        assert_eq!(flash.text, "hello");
        assert!(matches!(flash.kind, FlashKind::Info));
    }

    #[test]
    fn flash_error_sets_message() {
        let mut s = test_state();
        s.flash_error("oops");
        let flash = s.flash.as_ref().unwrap();
        assert_eq!(flash.text, "oops");
        assert!(matches!(flash.kind, FlashKind::Error));
    }

    // ── selection_paths ───────────────────────────────────────────

    #[test]
    fn selection_returns_cursor_item_when_no_picks() {
        let s = state_with_rows(&["a.txt", "b.txt"]);
        let paths = s.selection_paths();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("a.txt"));
    }

    #[test]
    fn selection_returns_picks_when_present() {
        let mut s = state_with_rows(&["a.txt", "b.txt", "c.txt"]);
        s.picks.toggle(Path::new("/tmp/test/b.txt"));
        s.picks.toggle(Path::new("/tmp/test/c.txt"));
        let paths = s.selection_paths();
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn selection_empty_when_no_rows() {
        let s = test_state();
        assert!(s.selection_paths().is_empty());
    }

    // ── toggle_pick_cursor ────────────────────────────────────────

    #[test]
    fn toggle_pick_adds_and_removes() {
        let mut s = state_with_rows(&["a.txt", "b.txt"]);
        s.toggle_pick_cursor();
        assert!(s.picks.contains(Path::new("/tmp/test/a.txt")));
        s.toggle_pick_cursor();
        assert!(!s.picks.contains(Path::new("/tmp/test/a.txt")));
    }

    #[test]
    fn toggle_pick_noop_in_inventory_view() {
        let mut s = state_with_rows(&["a.txt"]);
        s.view = View::Inventory;
        s.toggle_pick_cursor();
        assert!(s.picks.is_empty());
    }

    // ── toggle_all_picks ──────────────────────────────────────────

    #[test]
    fn toggle_all_picks_selects_then_clears() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.toggle_all_picks();
        assert_eq!(s.picks.len(), 3);
        s.toggle_all_picks();
        assert!(s.picks.is_empty());
    }

    // ── take / drop / inventory ───────────────────────────────────

    fn state_with_real_files(tmp: &std::path::Path, names: &[&str]) -> AppState {
        let mut s = test_state();
        for name in names {
            std::fs::write(tmp.join(name), format!("content of {name}")).unwrap();
        }
        s.rows = names
            .iter()
            .map(|n| RowData {
                path: tmp.join(n),
                display: n.to_string(),
                kind: EntryKind::File,
            })
            .collect();
        s
    }

    #[test]
    fn take_cursor_item_to_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_STATE_HOME", tmp.path()); }
        let mut s = state_with_real_files(tmp.path(), &["a.txt", "b.txt"]);
        s.take();
        assert_eq!(s.inventory.len(), 1);
        assert!(s.inventory.contains(&tmp.path().join("a.txt")));
    }

    #[test]
    fn take_picks_to_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_STATE_HOME", tmp.path()); }
        let mut s = state_with_real_files(tmp.path(), &["a.txt", "b.txt"]);
        s.picks.toggle(&tmp.path().join("a.txt"));
        s.picks.toggle(&tmp.path().join("b.txt"));
        s.take();
        assert_eq!(s.inventory.len(), 2);
    }

    #[test]
    fn drop_removes_from_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_STATE_HOME", tmp.path()); }
        let mut s = state_with_real_files(tmp.path(), &["a.txt"]);
        s.take(); // yank it first
        assert_eq!(s.inventory.len(), 1);
        // Switch to inventory view to drop
        s.toggle_inventory_view();
        s.drop_cursor();
        assert!(s.inventory.is_empty());
    }

    // ── toggle_inventory_view ─────────────────────────────────────

    #[test]
    fn toggle_inventory_switches_view() {
        let mut s = test_state();
        assert_eq!(s.view, View::Dir);
        s.toggle_inventory_view();
        assert_eq!(s.view, View::Inventory);
        s.toggle_inventory_view();
        assert_eq!(s.view, View::Dir);
    }

    // ── focus_on_path ─────────────────────────────────────────────

    #[test]
    fn focus_on_path_sets_cursor() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.focus_on_path(Path::new("/tmp/test/c"));
        assert_eq!(s.cursor.index, 2);
    }

    #[test]
    fn focus_on_missing_path_is_noop() {
        let mut s = state_with_rows(&["a", "b"]);
        s.cursor.index = 1;
        s.focus_on_path(Path::new("/tmp/test/nope"));
        assert_eq!(s.cursor.index, 1); // unchanged
    }

    // ── dispatch_command ──────────────────────────────────────────

    #[test]
    fn cmd_empty_is_handled() {
        let mut s = test_state();
        assert!(matches!(s.dispatch_command(""), CommandResult::Handled));
        assert!(matches!(s.dispatch_command("   "), CommandResult::Handled));
    }

    #[test]
    fn cmd_quit() {
        let mut s = test_state();
        assert!(matches!(s.dispatch_command("q"), CommandResult::Handled));
        assert!(s.should_quit);
    }

    #[test]
    fn cmd_quit_long() {
        let mut s = test_state();
        assert!(matches!(s.dispatch_command("quit"), CommandResult::Handled));
        assert!(s.should_quit);
    }

    #[test]
    fn cmd_limit_set_and_clear() {
        let mut s = state_with_rows(&["foo.rs", "bar.txt", "baz.rs"]);
        s.dispatch_command("limit *.rs");
        assert_eq!(s.temp_filter.as_deref(), Some("*.rs"));
        assert!(s.flash.as_ref().unwrap().text.contains("limit:"));

        s.dispatch_command("limit");
        assert!(s.temp_filter.is_none());
        assert!(s.flash.as_ref().unwrap().text.contains("cleared"));
    }

    #[test]
    fn cmd_limit_picks_only() {
        let mut s = test_state();
        s.dispatch_command("limit !");
        assert_eq!(s.temp_filter.as_deref(), Some("!"));
    }

    #[test]
    fn cmd_sort_query() {
        let mut s = test_state();
        s.dispatch_command("sort");
        assert!(s.flash.as_ref().unwrap().text.contains("name"));
    }

    #[test]
    fn cmd_sort_set() {
        let mut s = test_state();
        s.dispatch_command("sort size");
        assert_eq!(s.sort_order, SortMode::Size);
        assert!(s.flash.as_ref().unwrap().text.contains("size"));
    }

    #[test]
    fn cmd_sort_invalid() {
        let mut s = test_state();
        s.dispatch_command("sort bogus");
        assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
    }

    #[test]
    fn cmd_marks_empty() {
        let mut s = test_state();
        let result = s.dispatch_command("marks");
        assert!(matches!(result, CommandResult::Handled));
        assert!(s.flash.as_ref().unwrap().text.contains("no marks"));
    }

    #[test]
    fn cmd_marks_with_entries() {
        let mut s = test_state();
        s.marks.set(
            'a',
            Mark {
                dir: PathBuf::from("/tmp"),
                focus: None,
            },
        );
        let result = s.dispatch_command("marks");
        match result {
            CommandResult::OpenPager { title, lines } => {
                assert_eq!(title, "marks");
                assert_eq!(lines.len(), 1);
                assert!(lines[0].contains("/tmp"));
            }
            _ => panic!("expected OpenPager"),
        }
    }

    #[test]
    fn cmd_set_sort() {
        let mut s = test_state();
        s.dispatch_command("set sort=mtime");
        assert_eq!(s.sort_order, SortMode::Mtime);
    }

    #[test]
    fn cmd_set_unknown_key() {
        let mut s = test_state();
        s.dispatch_command("set foo=bar");
        assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
    }

    #[test]
    fn cmd_shell_not_handled() {
        let mut s = test_state();
        assert!(matches!(
            s.dispatch_command("!ls"),
            CommandResult::NotHandled
        ));
        assert!(matches!(
            s.dispatch_command(";htop"),
            CommandResult::NotHandled
        ));
        assert!(matches!(
            s.dispatch_command("bprev"),
            CommandResult::NotHandled
        ));
        assert!(matches!(
            s.dispatch_command("bnext"),
            CommandResult::NotHandled
        ));
    }

    #[test]
    fn cmd_unknown() {
        let mut s = test_state();
        s.dispatch_command("foobar");
        let flash = s.flash.as_ref().unwrap();
        assert!(matches!(flash.kind, FlashKind::Error));
        assert!(flash.text.contains("foobar"));
    }

    // ── dispatch_prompt ───────────────────────────────────────────

    #[test]
    fn prompt_search_saves_last_search() {
        let mut s = test_state();
        let result = s.dispatch_prompt(
            &PromptKind::Search { saved_cursor: 0 },
            "foo",
        );
        assert!(matches!(result, PromptResult::Handled));
        assert_eq!(s.last_search.as_deref(), Some("foo"));
    }

    #[test]
    fn prompt_search_empty_does_not_save() {
        let mut s = test_state();
        s.last_search = Some("old".to_string());
        s.dispatch_prompt(&PromptKind::Search { saved_cursor: 0 }, "");
        assert_eq!(s.last_search.as_deref(), Some("old"));
    }

    #[test]
    fn prompt_limit_sets_filter() {
        let mut s = test_state();
        s.dispatch_prompt(&PromptKind::Limit, "*.rs");
        assert_eq!(s.temp_filter.as_deref(), Some("*.rs"));
    }

    #[test]
    fn prompt_limit_empty_clears() {
        let mut s = test_state();
        s.temp_filter = Some("old".to_string());
        s.dispatch_prompt(&PromptKind::Limit, "");
        assert!(s.temp_filter.is_none());
    }

    #[test]
    fn prompt_set_env() {
        let mut s = test_state();
        s.dispatch_prompt(&PromptKind::SetEnv, "TEST_SPYC_VAR=hello");
        assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Info));
        // Verify env was set
        assert_eq!(std::env::var("TEST_SPYC_VAR").unwrap(), "hello");
    }

    #[test]
    fn prompt_set_env_bad_format() {
        let mut s = test_state();
        s.dispatch_prompt(&PromptKind::SetEnv, "no_equals_sign");
        assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
    }

    #[test]
    fn prompt_pattern_pick() {
        let mut s = test_state();
        // Add some listing entries for the pattern to match against
        s.listing = Listing::empty(PathBuf::from("/tmp/test"));
        use crate::fs::entry::{Entry, EntryKind};
        s.listing.entries = vec![
            Entry {
                path: PathBuf::from("/tmp/test/foo.rs"),
                name: "foo.rs".to_string(),
                kind: EntryKind::File,
                size: 0,
                mtime: std::time::SystemTime::UNIX_EPOCH,
            },
            Entry {
                path: PathBuf::from("/tmp/test/bar.txt"),
                name: "bar.txt".to_string(),
                kind: EntryKind::File,
                size: 0,
                mtime: std::time::SystemTime::UNIX_EPOCH,
            },
        ];
        s.dispatch_prompt(&PromptKind::PatternPick, "*.rs");
        assert!(s.picks.contains(Path::new("/tmp/test/foo.rs")));
        assert!(!s.picks.contains(Path::new("/tmp/test/bar.txt")));
    }

    #[test]
    fn prompt_pane_new_tab_cmd_stashes() {
        let mut s = test_state();
        s.dispatch_prompt(&PromptKind::PaneNewTabCmd, "bash");
        assert_eq!(s.pending_new_tab_cmd.as_deref(), Some("bash"));
        assert!(matches!(s.mode, Mode::Prompting(_)));
    }

    #[test]
    fn prompt_pane_new_tab_cmd_empty_is_noop() {
        let mut s = test_state();
        s.dispatch_prompt(&PromptKind::PaneNewTabCmd, "");
        assert!(s.pending_new_tab_cmd.is_none());
    }

    #[test]
    fn prompt_shell_cmd_not_handled() {
        let mut s = test_state();
        assert!(matches!(
            s.dispatch_prompt(&PromptKind::ShellCmd, "ls"),
            PromptResult::NotHandled
        ));
        assert!(matches!(
            s.dispatch_prompt(&PromptKind::ShellCmdCaptured, "ls"),
            PromptResult::NotHandled
        ));
        assert!(matches!(
            s.dispatch_prompt(&PromptKind::CopyTo, "/tmp"),
            PromptResult::NotHandled
        ));
    }

    #[test]
    fn prompt_remove_confirm_handled() {
        let mut s = test_state();
        assert!(matches!(
            s.dispatch_prompt(&PromptKind::RemoveConfirm, "n"),
            PromptResult::Handled
        ));
    }

    // ── apply() action dispatch ───────────────────────────────────

    #[test]
    fn apply_down_moves_cursor() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        assert!(matches!(s.apply(&Action::Down(1)), ApplyResult::Handled));
        assert_eq!(s.cursor.index, 1);
    }

    #[test]
    fn apply_up_wraps() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        assert!(matches!(s.apply(&Action::Up(1)), ApplyResult::Handled));
        assert_eq!(s.cursor.index, 2);
    }

    #[test]
    fn apply_down_with_count() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e"]);
        s.apply(&Action::Down(3));
        assert_eq!(s.cursor.index, 3);
    }

    #[test]
    fn apply_page_down() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
        s.last_grid = Grid { cols: 1, rows: 3, col_widths: vec![20] };
        s.apply(&Action::PageDown);
        assert_eq!(s.cursor.index, 3);
    }

    #[test]
    fn apply_goto_first() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.cursor.index = 2;
        s.apply(&Action::GotoFirst);
        assert_eq!(s.cursor.index, 0);
    }

    #[test]
    fn apply_goto_last() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.apply(&Action::GotoLast);
        assert_eq!(s.cursor.index, 2);
    }

    #[test]
    fn apply_left_right_columns() {
        let mut s = state_with_rows(&["a", "b", "c", "d", "e", "f"]);
        s.last_grid = Grid { cols: 2, rows: 3, col_widths: vec![10, 10] };
        s.apply(&Action::Right(1));
        assert_eq!(s.cursor.index, 3);
        s.apply(&Action::Left(1));
        assert_eq!(s.cursor.index, 0);
    }

    #[test]
    fn apply_toggle_pick() {
        let mut s = state_with_rows(&["a.txt", "b.txt"]);
        s.apply(&Action::TogglePick);
        assert!(s.picks.contains(Path::new("/tmp/test/a.txt")));
    }

    #[test]
    fn apply_pick_toggle_all() {
        let mut s = state_with_rows(&["a", "b", "c"]);
        s.apply(&Action::PickToggleAll);
        assert_eq!(s.picks.len(), 3);
        s.apply(&Action::PickToggleAll);
        assert!(s.picks.is_empty());
    }

    #[test]
    fn apply_take_adds_to_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_STATE_HOME", tmp.path()); }
        let mut s = state_with_real_files(tmp.path(), &["a.txt"]);
        s.apply(&Action::Take);
        assert_eq!(s.inventory.len(), 1);
        assert!(s.inventory.contains(&tmp.path().join("a.txt")));
    }

    #[test]
    fn apply_drop_removes_from_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_STATE_HOME", tmp.path()); }
        let mut s = state_with_real_files(tmp.path(), &["a.txt"]);
        s.take(); // yank first
        s.toggle_inventory_view();
        s.apply(&Action::Drop);
        assert!(s.inventory.is_empty());
    }

    #[test]
    fn apply_toggle_inventory_view() {
        let mut s = test_state();
        s.apply(&Action::ToggleInventoryView);
        assert_eq!(s.view, View::Inventory);
        s.apply(&Action::ToggleInventoryView);
        assert_eq!(s.view, View::Dir);
    }

    #[test]
    fn apply_empty_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_STATE_HOME", tmp.path()); }
        let mut s = state_with_real_files(tmp.path(), &["a.txt"]);
        s.take(); // yank first
        assert_eq!(s.inventory.len(), 1);
        s.apply(&Action::EmptyInventory);
        assert!(s.inventory.is_empty());
    }

    #[test]
    fn apply_toggle_mask() {
        let mut s = test_state();
        let was_enabled = s.masks.mask1.enabled;
        s.apply(&Action::ToggleMask(1));
        assert_ne!(s.masks.mask1.enabled, was_enabled);
    }

    #[test]
    fn apply_search_next_finds_match() {
        let mut s = state_with_rows(&["alpha", "beta", "gamma"]);
        s.last_search = Some("g".to_string());
        s.apply(&Action::SearchNext);
        assert_eq!(s.cursor.index, 2);
    }

    #[test]
    fn apply_search_prev_finds_match() {
        let mut s = state_with_rows(&["alpha", "beta", "gamma"]);
        s.cursor.index = 2;
        s.last_search = Some("a".to_string());
        s.apply(&Action::SearchPrev);
        assert_eq!(s.cursor.index, 0);
    }

    #[test]
    fn apply_start_shell_returns_spawn() {
        let mut s = test_state();
        let result = s.apply(&Action::StartShell);
        assert!(matches!(result, ApplyResult::Post(PostAction::Spawn { .. })));
    }

    #[test]
    fn apply_prompt_actions_set_mode() {
        let mut s = test_state();
        s.apply(&Action::SearchPrompt);
        assert!(matches!(s.mode, Mode::Prompting(_)));

        s.mode = Mode::Normal;
        s.apply(&Action::ShellCapturedPrompt);
        assert!(matches!(s.mode, Mode::Prompting(_)));

        s.mode = Mode::Normal;
        s.apply(&Action::CommandPrompt);
        assert!(matches!(s.mode, Mode::Prompting(_)));

        s.mode = Mode::Normal;
        s.apply(&Action::JumpPrompt);
        assert!(matches!(s.mode, Mode::Prompting(_)));

        s.mode = Mode::Normal;
        s.apply(&Action::LimitPrompt);
        assert!(matches!(s.mode, Mode::Prompting(_)));

        s.mode = Mode::Normal;
        s.apply(&Action::SetEnvPrompt);
        assert!(matches!(s.mode, Mode::Prompting(_)));
    }

    #[test]
    fn apply_set_mark() {
        let mut s = state_with_rows(&["file.txt"]);
        s.apply(&Action::SetMark('a'));
        assert!(s.marks.get('a').is_some());
    }

    #[test]
    fn apply_date_flashes() {
        let mut s = test_state();
        s.apply(&Action::Date);
        assert!(s.flash.is_some());
        assert!(s.flash.as_ref().unwrap().text.contains("UTC"));
    }

    #[test]
    fn apply_version_flashes() {
        let mut s = test_state();
        s.apply(&Action::Version);
        let flash = s.flash.as_ref().unwrap();
        assert!(flash.text.contains("spyc"));
    }

    #[test]
    fn apply_noop_does_nothing() {
        let mut s = test_state();
        let result = s.apply(&Action::Noop);
        assert!(matches!(result, ApplyResult::Handled));
    }

    #[test]
    fn apply_long_list_returns_pager() {
        let mut s = state_with_rows(&["a.txt"]);
        let result = s.apply(&Action::LongList);
        assert!(matches!(result, ApplyResult::OpenPager(_)));
    }

    #[test]
    fn apply_file_type_single_flashes() {
        let mut s = state_with_rows(&["a.txt"]);
        let result = s.apply(&Action::FileType);
        // Single file: flashes info, returns Handled
        assert!(matches!(result, ApplyResult::Handled));
        assert!(s.flash.is_some());
    }

    #[test]
    fn apply_pane_actions_not_handled() {
        let mut s = test_state();
        assert!(matches!(
            s.apply(&Action::TogglePane),
            ApplyResult::NotHandled
        ));
        assert!(matches!(
            s.apply(&Action::PaneFocusDown),
            ApplyResult::NotHandled
        ));
        assert!(matches!(
            s.apply(&Action::Help),
            ApplyResult::NotHandled
        ));
        assert!(matches!(
            s.apply(&Action::Redraw),
            ApplyResult::NotHandled
        ));
        assert!(matches!(
            s.apply(&Action::ColorToggle),
            ApplyResult::NotHandled
        ));
    }

    #[test]
    fn apply_worktree_new_sets_prompt_or_errors() {
        let mut s = test_state();
        // No git info → error
        s.apply(&Action::WorktreeNew);
        assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));

        // With git info → prompt
        s.flash = None;
        s.git_info = Some("main".to_string());
        s.apply(&Action::WorktreeNew);
        assert!(matches!(s.mode, Mode::Prompting(_)));
    }

    #[test]
    fn apply_jump_prev_dir() {
        let mut s = test_state();
        // No prev dir → error
        s.apply(&Action::JumpPrevDir);
        assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
    }

    #[test]
    fn apply_clamps_cursor_after_action() {
        let mut s = state_with_rows(&["a", "b"]);
        s.cursor.index = 10; // out of bounds
        s.apply(&Action::Noop); // any handled action should clamp
        assert_eq!(s.cursor.index, 1); // clamped to last valid
    }
}
