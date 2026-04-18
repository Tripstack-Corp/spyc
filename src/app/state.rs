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
use crate::keymap::{Resolver, UserKeymap};
use crate::state::{Cursor, History, IgnoreMasks, Inventory, Mark, Marks, Picks};
use crate::ui::list_view::Grid;

use super::{
    detect_kind, row_from_entry, FlashKind, FlashMessage, Matcher, Mode, RowData, View,
};

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

    pub fn take(&mut self) {
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

    pub fn drop_cursor(&mut self) {
        let Some(row) = self.rows.get(self.cursor.index) else {
            return;
        };
        let path = row.path.clone();
        self.inventory.remove(&path);
        self.rebuild_rows();
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

    #[test]
    fn take_cursor_item_to_inventory() {
        let mut s = state_with_rows(&["a.txt", "b.txt"]);
        s.take();
        assert_eq!(s.inventory.len(), 1);
        assert!(s.inventory.contains(Path::new("/tmp/test/a.txt")));
    }

    #[test]
    fn take_picks_to_inventory() {
        let mut s = state_with_rows(&["a.txt", "b.txt"]);
        s.picks.toggle(Path::new("/tmp/test/a.txt"));
        s.picks.toggle(Path::new("/tmp/test/b.txt"));
        s.take();
        assert_eq!(s.inventory.len(), 2);
    }

    #[test]
    fn drop_removes_from_inventory() {
        let mut s = state_with_rows(&["a.txt"]);
        s.inventory.extend(vec![PathBuf::from("/tmp/test/a.txt")]);
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
}
