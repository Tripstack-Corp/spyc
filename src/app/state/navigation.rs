//! `AppState` cursor movement + jumps: vertical/column moves, goto-col,
//! viewport visibility, incremental find, and mark/dir/git-change jumps.
//! Split from `state` verbatim; `impl AppState` reading fields directly.

use anyhow::Result;

use crate::app::{Effect, Matcher};

use super::AppState;

impl AppState {
    /// j/k — move within the current column only. Wraps at column
    /// boundaries. Returns false (flash) if the column has only one row.
    pub fn cursor_move_vertical(&mut self, delta: isize, rows_per_col: usize, len: usize) -> bool {
        if len == 0 {
            return false;
        }
        let rows_per_col = rows_per_col.max(1);
        let col_start = (self.cursor.index / rows_per_col) * rows_per_col;
        let col_end = (col_start + rows_per_col).min(len);
        let col_len = col_end - col_start;
        if col_len <= 1 {
            return false; // single-item column — flash
        }
        let row_in_col = self.cursor.index - col_start;
        let new_row = (row_in_col as isize + delta).rem_euclid(col_len as isize) as usize;
        self.cursor.index = col_start + new_row;
        true
    }

    /// Move across the entire list (PageUp/PageDown). Wraps globally.
    pub const fn cursor_move_global(&mut self, delta: isize, len: usize) {
        if len == 0 {
            return;
        }
        let new_idx = (self.cursor.index as isize + delta).rem_euclid(len as isize);
        self.cursor.index = new_idx as usize;
    }

    /// `gg` — jump to the first entry of the current column.
    pub const fn goto_col_top(&mut self, rows_per_col: usize) {
        if rows_per_col == 0 {
            self.cursor.index = 0;
            return;
        }
        let current_col = self.cursor.index / rows_per_col;
        self.cursor.index = current_col * rows_per_col;
    }

    /// `G` — jump to the last entry of the current column.
    pub fn goto_col_bottom(&mut self, rows_per_col: usize, len: usize) {
        if len == 0 {
            return;
        }
        if rows_per_col == 0 {
            self.cursor.index = len - 1;
            return;
        }
        let current_col = self.cursor.index / rows_per_col;
        let col_end = ((current_col + 1) * rows_per_col).min(len);
        self.cursor.index = col_end - 1;
    }

    /// Move the cursor to the next (or previous) row whose git status
    /// is non-clean — i.e., a row carrying a `~`/`+`/`?`/`-` marker
    /// in the listing. Wraps around the end of the list (so a user
    /// can keep pressing `]g` without worrying about direction).
    /// Returns `false` when no row in the listing has a git change,
    /// so the caller can flash an empty-search message.
    pub fn jump_to_git_change(&mut self, forward: bool) -> bool {
        let len = self.rows.len();
        if len == 0 || self.git.files.is_empty() {
            return false;
        }
        let cur = self.cursor.index.min(len.saturating_sub(1));
        let is_changed = |idx: usize| -> bool {
            self.rows.get(idx).is_some_and(|r| {
                self.git
                    .files
                    .get(&r.display)
                    .copied()
                    .is_some_and(|s| !s.is_clean())
            })
        };
        // Walk every other index, in the requested direction, with wrap.
        // `n` = 1..len means we never re-test the cursor's own row, so a
        // press from a dirty row advances to the *next* dirty one rather
        // than staying put.
        for n in 1..=len {
            let idx = if forward {
                (cur + n) % len
            } else {
                (cur + len - (n % len)) % len
            };
            if is_changed(idx) {
                self.cursor.index = idx;
                return true;
            }
        }
        false
    }

    /// Move the cursor by `delta` columns.
    /// h/l — move across columns on the same row. Wraps at row
    /// boundaries. Returns false (flash) if only one column on this row.
    pub fn cursor_move_columns(&mut self, delta: isize, rows_per_col: usize, len: usize) -> bool {
        if rows_per_col == 0 || len == 0 {
            return false;
        }
        let num_cols = len.div_ceil(rows_per_col);
        let current_col = self.cursor.index / rows_per_col;
        let current_row = self.cursor.index % rows_per_col;
        // Count how many columns actually have an item on this row.
        let cols_on_row = {
            let mut n = 0usize;
            for c in 0..num_cols {
                if c * rows_per_col + current_row < len {
                    n += 1;
                }
            }
            n
        };
        if cols_on_row <= 1 {
            return false; // single-column row — flash
        }
        // Wrap within the columns that exist on this row.
        let col_in_row = {
            // Map current_col to its ordinal among valid columns on this row.
            let mut ord = 0usize;
            for c in 0..current_col {
                if c * rows_per_col + current_row < len {
                    ord += 1;
                }
            }
            ord
        };
        let new_ord = (col_in_row as isize + delta).rem_euclid(cols_on_row as isize) as usize;
        // Map ordinal back to column index.
        let mut target_col = 0usize;
        let mut count = 0usize;
        for c in 0..num_cols {
            if c * rows_per_col + current_row < len {
                if count == new_ord {
                    target_col = c;
                    break;
                }
                count += 1;
            }
        }
        let target_idx = target_col * rows_per_col + current_row;
        if target_idx < len {
            self.cursor.index = target_idx;
        }
        true
    }

    pub const fn ensure_cursor_visible(&mut self) {
        let per_page = self.grid_dims.items_per_page();
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

    /// `'<letter>` — jump to a saved mark. MVU Phase 5: the chdir is a
    /// deferred [`Effect::ChangeDir`] (returned for the `apply()` arm to
    /// emit), carrying the mark's saved `focus` and the success flash so the
    /// executor reproduces this site byte-for-byte. The mark-not-set case
    /// flashes and returns no effect.
    pub fn jump_to_mark(&mut self, letter: char) -> Vec<Effect> {
        let Some(mark) = self.marks.get(letter).cloned() else {
            self.flash_error(format!("mark '{letter}' not set"));
            return Vec::new();
        };
        vec![Effect::ChangeDir {
            path: mark.dir,
            focus: mark.focus,
            on_ok: Some(format!("jumped to mark '{letter}'")),
            err_prefix: "jump failed",
        }]
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
