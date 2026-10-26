//! `AppState` cursor movement + jumps: vertical/column moves, goto-col,
//! viewport visibility, incremental find, and mark/dir/git-change/pick jumps.
//! Split from `state` verbatim; `impl AppState` reading fields directly.

use anyhow::{Context, Result};

use crate::app::{Effect, Matcher, RowData};
use crate::keymap::Action;

use super::{AppState, Side};

const NO_GIT_CHANGES: &str = "no git changes in this directory";
const NO_PICKS: &str = "no picks in this directory";

/// The first row `hit` accepts, walking from `cursor` `forward` or back and
/// wrapping around the list. The cursor's own row is tried last, so a walk
/// from one hit lands on the next.
fn wrapping_find(
    rows: &[RowData],
    cursor: usize,
    forward: bool,
    hit: impl Fn(&RowData) -> bool,
) -> Option<usize> {
    let len = rows.len();
    let cur = cursor.min(len.checked_sub(1)?);
    (1..=len)
        .map(|n| {
            if forward {
                (cur + n) % len
            } else {
                (cur + len - (n % len)) % len
            }
        })
        .find(|&idx| hit(&rows[idx]))
}

impl AppState {
    /// j/k — move within the current column only. Wraps at column
    /// boundaries. Returns false (flash) if the column has only one row.
    pub fn cursor_move_vertical(&mut self, delta: isize, rows_per_col: usize, len: usize) -> bool {
        if len == 0 {
            return false;
        }
        let rows_per_col = rows_per_col.max(1);
        let col_start = (self.cur().cursor.index / rows_per_col) * rows_per_col;
        let col_end = (col_start + rows_per_col).min(len);
        let col_len = col_end - col_start;
        if col_len <= 1 {
            return false; // single-item column — flash
        }
        let row_in_col = self.cur().cursor.index - col_start;
        let new_row = (row_in_col as isize + delta).rem_euclid(col_len as isize) as usize;
        self.cur_mut().cursor.index = col_start + new_row;
        true
    }

    /// Move a *specific* column's cursor by `delta`, **clamped** to the list — the
    /// mouse-wheel entry point.
    ///
    /// Two deliberate differences from [`Self::cursor_move_vertical`], which the
    /// keyboard uses:
    ///
    /// - **Targets `side`, not `cur()`.** The wheel scrolls the column under the
    ///   pointer, so in a vsplit it must move the pointed column's cursor without
    ///   moving keyboard focus there. `cur()` would move the *focused* column
    ///   instead, i.e. the one the user isn't pointing at.
    /// - **Clamps instead of wrapping.** `cursor_move_vertical` wraps with
    ///   `rem_euclid` (correct for `j`/`k`), but a wheel that teleports from the end
    ///   of the list back to the top reads as the list flying out of control — one
    ///   of the two causes of the "uncontrollably fast" report, alongside the
    ///   `scroll_lines` default.
    ///
    /// Scrolls the whole list, not the on-screen column: with a multi-column grid
    /// the wheel should walk the listing in order rather than stop dead at the
    /// bottom of the first column.
    pub fn cursor_scroll_side(&mut self, side: Side, delta: isize) {
        let len = self.col(side).rows.len();
        if len == 0 {
            return;
        }
        let cur = self.col(side).cursor.index as isize;
        let last = len as isize - 1;
        self.col_mut(side).cursor.index = (cur + delta).clamp(0, last) as usize;
    }

    /// PageDown / PageUp: the same column and row on the next or previous page,
    /// or that page's last row when it is shorter. Pages are the fixed ones the
    /// last frame placed (`Commander::page`). PageDown from the last page wraps
    /// to the first; PageUp from the first wraps to the last row.
    pub fn page_move(&mut self, forward: bool) {
        let len = self.cur().rows.len();
        if len == 0 {
            return;
        }
        let page = self.cur().page;
        let offset = self.cur().cursor.index.saturating_sub(page.start);
        let target = if forward { Some(page.next) } else { page.prev };
        let index = target.map_or(len - 1, |(start, page_len)| {
            start + offset.min(page_len.saturating_sub(1))
        });
        self.cur_mut().cursor.index = index.min(len - 1);
    }

    /// `gg` — jump to the first entry of the current column.
    pub fn goto_col_top(&mut self, rows_per_col: usize) {
        if rows_per_col == 0 {
            self.cur_mut().cursor.index = 0;
            return;
        }
        let current_col = self.cur().cursor.index / rows_per_col;
        self.cur_mut().cursor.index = current_col * rows_per_col;
    }

    /// `G` — jump to the last entry of the current column.
    pub fn goto_col_bottom(&mut self, rows_per_col: usize, len: usize) {
        if len == 0 {
            return;
        }
        if rows_per_col == 0 {
            self.cur_mut().cursor.index = len - 1;
            return;
        }
        let current_col = self.cur().cursor.index / rows_per_col;
        let col_end = ((current_col + 1) * rows_per_col).min(len);
        self.cur_mut().cursor.index = col_end - 1;
    }

    /// `]g` / `[g` / `]p` / `[p`: the cursor to the next or previous git
    /// change or pick, or a flash when the listing has none.
    pub fn jump_or_flash(&mut self, action: &Action) {
        let (landed, none) = match action {
            Action::JumpNextGitChange => (self.jump_to_git_change(true), NO_GIT_CHANGES),
            Action::JumpPrevGitChange => (self.jump_to_git_change(false), NO_GIT_CHANGES),
            Action::JumpNextPick => (self.jump_to_pick(true), NO_PICKS),
            Action::JumpPrevPick => (self.jump_to_pick(false), NO_PICKS),
            _ => return,
        };
        if !landed {
            self.flash_info(none);
        }
    }

    /// Move the cursor to the next (or previous) row whose git status
    /// is non-clean — i.e., a row carrying a `~`/`+`/`?`/`-` marker
    /// in the listing. Wraps around the end of the list (so a user
    /// can keep pressing `]g` without worrying about direction).
    /// Returns `false` when no row in the listing has a git change,
    /// so the caller can flash an empty-search message.
    pub fn jump_to_git_change(&mut self, forward: bool) -> bool {
        let col = self.cur();
        let target = wrapping_find(&col.rows, col.cursor.index, forward, |r| {
            col.git
                .files
                .get(&r.display)
                .copied()
                .is_some_and(|s| !s.is_clean())
        });
        self.land_cursor(target)
    }

    /// [`Self::jump_to_git_change`] for picked rows. A pick the listing
    /// doesn't show, filtered or masked out, is no target.
    pub fn jump_to_pick(&mut self, forward: bool) -> bool {
        let col = self.cur();
        let target = wrapping_find(&col.rows, col.cursor.index, forward, |r| {
            col.picks.contains(&r.path)
        });
        self.land_cursor(target)
    }

    /// Put the cursor on `target`; `false` when there is none.
    fn land_cursor(&mut self, target: Option<usize>) -> bool {
        let Some(idx) = target else {
            return false;
        };
        self.cur_mut().cursor.index = idx;
        true
    }

    /// Move the cursor by `delta` columns.
    /// h/l — move across columns on the same row. Wraps at row
    /// boundaries. Returns false (flash) if only one column on this row.
    pub fn cursor_move_columns(&mut self, delta: isize, rows_per_col: usize, len: usize) -> bool {
        if rows_per_col == 0 || len == 0 {
            return false;
        }
        let num_cols = len.div_ceil(rows_per_col);
        let current_col = self.cur().cursor.index / rows_per_col;
        let current_row = self.cur().cursor.index % rows_per_col;
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
            self.cur_mut().cursor.index = target_idx;
        }
        true
    }

    pub fn find_match(&self, query: &str, from: usize, backward: bool) -> Option<usize> {
        if self.cur().rows.is_empty() {
            return None;
        }
        let matcher = Matcher::build(query);
        let n = self.cur().rows.len();
        for step in 0..n {
            let i = if backward {
                (from + n - step) % n
            } else {
                (from + step) % n
            };
            if matcher.matches(&self.cur().rows[i].display) {
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
            self.cur().listing.dir.join(&expanded)
        };
        // Name the path: a bare `?` here surfaced as "chdir: No such file or
        // directory" with no indication of WHICH path was missing.
        let canonical =
            std::fs::canonicalize(&abs).with_context(|| format!("resolving {}", abs.display()))?;
        let md = std::fs::metadata(&canonical)
            .with_context(|| format!("reading {}", canonical.display()))?;
        if md.is_dir() {
            if let Err(e) = self.chdir(&canonical) {
                self.flash_error(format!("chdir: {e:#}"));
                return Ok(());
            }
        } else if let Some(parent) = canonical.parent() {
            if let Err(e) = self.chdir(parent) {
                self.flash_error(format!("chdir: {e:#}"));
                return Ok(());
            }
            self.focus_on_path(&canonical);
        }
        Ok(())
    }
}

#[cfg(test)]
mod scroll_side_tests {
    use crate::app::state::{AppState, Side};

    fn state_with(rows: usize) -> AppState {
        let tmp = tempfile::tempdir().expect("tempdir").keep();
        let mut s = AppState::test_default(tmp);
        s.active_project_mut().left.rows = (0..rows)
            .map(|i| crate::app::RowData {
                path: std::path::PathBuf::from(format!("f{i}")),
                display: format!("f{i}"),
                kind: crate::fs::EntryKind::File,
                deleted: false,
            })
            .collect();
        s
    }

    /// The wheel must STOP at the ends. `cursor_move_vertical` wraps via
    /// `rem_euclid` — correct for `j`/`k`, but a wheel that teleports from the
    /// bottom of the list back to the top is what made scrolling feel out of
    /// control.
    #[test]
    fn cursor_scroll_side_clamps_instead_of_wrapping() {
        let mut s = state_with(5);
        s.active_project_mut().left.cursor.index = 4; // last row
        s.cursor_scroll_side(Side::Left, 3);
        assert_eq!(
            s.active_project().left.cursor.index,
            4,
            "must not wrap past the end"
        );

        s.active_project_mut().left.cursor.index = 0;
        s.cursor_scroll_side(Side::Left, -3);
        assert_eq!(
            s.active_project().left.cursor.index,
            0,
            "must not wrap past the start"
        );
    }

    #[test]
    fn cursor_scroll_side_moves_by_delta() {
        let mut s = state_with(10);
        s.cursor_scroll_side(Side::Left, 3);
        assert_eq!(s.active_project().left.cursor.index, 3);
        s.cursor_scroll_side(Side::Left, -1);
        assert_eq!(s.active_project().left.cursor.index, 2);
    }

    /// An empty listing is a no-op rather than a panic.
    #[test]
    fn cursor_scroll_side_on_an_empty_list_is_a_noop() {
        let mut s = state_with(0);
        s.cursor_scroll_side(Side::Left, 5);
        assert_eq!(s.active_project().left.cursor.index, 0);
    }
}
