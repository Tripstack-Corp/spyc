//! `PagerView` visual / placement selection: Line and Block visual modes, the
//! placement cursor (vi-motion positioning), and visual-range yank. Verbatim.

use super::{PagerView, PlacementCursor, Search, VisualKind, VisualSelection};

use super::layout::{line_plain_text, next_word_start, prev_word_start};

impl PagerView {
    /// True while the user is selecting a line range with `V`.
    pub const fn is_visual(&self) -> bool {
        self.visual.is_some()
    }

    /// Enter visual line mode, anchoring the selection at the top
    /// visible line. `j`/`k`/`G`/etc. then move the cursor end (with
    /// auto-scroll) and `y` yanks the inclusive range. No-op on an
    /// empty buffer (nothing to select).
    pub fn enter_visual(&mut self) {
        self.enter_visual_with_kind(VisualKind::Line);
    }

    /// Enter (or upgrade to) `Block` visual mode. Anchors at the
    /// top visible line, column 0. If a `Line` selection is
    /// already active, preserve its anchor / cursor and just
    /// flip the kind — vim does the same when you press `^v`
    /// inside an active `V` selection.
    pub fn enter_visual_block(&mut self) {
        if let Some(sel) = self.visual.as_mut() {
            sel.kind = VisualKind::Block;
            // Keep anchor/cursor lines as-is. Columns default to
            // 0/0 if they were never set (Line mode ignored them).
        } else {
            self.enter_visual_with_kind(VisualKind::Block);
        }
    }

    /// Enter pre-visual-block "placement" state. A navigation
    /// cursor lands at (top visible line, col 0); the user can
    /// then move it with vi motions (`hjkl`, `w`/`b`, `0`/`$`,
    /// `gg`/`G`) before committing to a visual block selection
    /// via a second `^v` or to Line visual via `V`. `Esc` cancels.
    pub fn enter_placement(&mut self) {
        if self.lines.is_empty() {
            return;
        }
        // Clear any active selection so placement and visual are
        // mutually exclusive (they share the cursor highlight).
        self.visual = None;
        let row = (self.scroll as usize).min(self.lines.len() - 1);
        self.placement = Some(PlacementCursor { row, col: 0 });
    }

    pub const fn cancel_placement(&mut self) {
        self.placement = None;
    }

    pub const fn is_placement(&self) -> bool {
        self.placement.is_some()
    }

    /// Commit placement → visual block. Anchor lands at the
    /// placement cursor; initial selection is the single cell.
    pub const fn commit_placement_to_visual_block(&mut self) {
        let Some(p) = self.placement.take() else {
            return;
        };
        self.visual = Some(VisualSelection {
            anchor: p.row,
            cursor: p.row,
            anchor_col: p.col,
            cursor_col: p.col,
            kind: VisualKind::Block,
        });
    }

    /// Commit placement → visual line at the placement cursor row.
    /// `V` from placement: skip block setup, start a line-visual
    /// selection from the row the cursor is on.
    pub const fn commit_placement_to_visual_line(&mut self) {
        let Some(p) = self.placement.take() else {
            return;
        };
        self.visual = Some(VisualSelection {
            anchor: p.row,
            cursor: p.row,
            anchor_col: 0,
            cursor_col: 0,
            kind: VisualKind::Line,
        });
    }

    /// Number of characters in `lines[row]` for cursor-clamp math.
    /// Returns 0 if the row is out of range or empty.
    fn placement_row_len(&self, row: usize) -> usize {
        self.lines.get(row).map_or(0, |l| {
            l.spans.iter().map(|s| s.content.chars().count()).sum()
        })
    }

    /// Plain-text content of a line, joined across spans. Used for
    /// vi-style word motions where styling is irrelevant.
    fn placement_row_text(&self, row: usize) -> String {
        self.lines.get(row).map_or_else(String::new, |l| {
            l.spans.iter().map(|s| s.content.as_ref()).collect()
        })
    }

    pub fn placement_move(&mut self, delta_row: isize, delta_col: isize, viewport_height: u16) {
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let Some(p) = self.placement.as_mut() else {
            return;
        };
        let new_row = (p.row as isize + delta_row).clamp(0, n as isize - 1) as usize;
        p.row = new_row;
        let row_len = self.lines.get(new_row).map_or(0, |l| {
            l.spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>()
        });
        let max_col = row_len.saturating_sub(1);
        let new_col = (p.col as isize + delta_col).max(0).min(max_col as isize) as usize;
        p.col = new_col;
        self.scroll_to_keep_visible(new_row, viewport_height);
    }

    pub const fn placement_line_start(&mut self) {
        if let Some(p) = self.placement.as_mut() {
            p.col = 0;
        }
    }

    pub fn placement_line_end(&mut self) {
        let Some(row) = self.placement.as_ref().map(|p| p.row) else {
            return;
        };
        let row_len = self.placement_row_len(row);
        if let Some(p) = self.placement.as_mut() {
            p.col = row_len.saturating_sub(1);
        }
    }

    /// Vi `w`: jump to the next word start on the current row.
    /// Wraps to col 0 of the next non-empty row when no word
    /// remains. Word characters are alphanumeric + `_` (vi's
    /// default `iskeyword`); transitions in/out of that class
    /// count as a word boundary.
    pub fn placement_word_forward(&mut self) {
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let (row, col) = match self.placement {
            Some(p) => (p.row, p.col),
            None => return,
        };
        let chars: Vec<char> = self.placement_row_text(row).chars().collect();
        let target = next_word_start(&chars, col);
        // Read the next row's text up front if we'll need to wrap,
        // so the second `placement.as_mut()` borrow below doesn't
        // overlap with an immutable borrow of `self`.
        let next_row_text = if target.is_none() && row + 1 < n {
            Some(self.placement_row_text(row + 1))
        } else {
            None
        };
        let Some(p) = self.placement.as_mut() else {
            return;
        };
        if let Some(next_col) = target {
            p.col = next_col;
        } else if let Some(next_text) = next_row_text {
            p.row = row + 1;
            p.col = next_text
                .chars()
                .position(|c| !c.is_whitespace())
                .unwrap_or(0);
        }
    }

    /// Vi `b`: jump to the previous word start on the current row.
    /// Wraps to the last word of the previous row when no word
    /// precedes the cursor on this row.
    pub fn placement_word_backward(&mut self) {
        if self.lines.is_empty() {
            return;
        }
        let (row, col) = match self.placement {
            Some(p) => (p.row, p.col),
            None => return,
        };
        let chars: Vec<char> = self.placement_row_text(row).chars().collect();
        let target = prev_word_start(&chars, col);
        let prev_row_chars: Option<Vec<char>> = if target.is_none() && row > 0 {
            Some(self.placement_row_text(row - 1).chars().collect())
        } else {
            None
        };
        let Some(p) = self.placement.as_mut() else {
            return;
        };
        if let Some(prev_col) = target {
            p.col = prev_col;
        } else if let Some(prev_chars) = prev_row_chars {
            p.row = row - 1;
            // End-of-line: the previous word-start is `prev_word_start` from
            // one-past-the-last char (equivalent to a dedicated last-word scan).
            p.col = prev_word_start(&prev_chars, prev_chars.len()).unwrap_or(0);
        }
    }

    pub fn placement_jump_to(&mut self, row: usize, viewport_height: u16) {
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let target = row.min(n - 1);
        let row_len = self.placement_row_len(target);
        let Some(p) = self.placement.as_mut() else {
            return;
        };
        p.row = target;
        p.col = p.col.min(row_len.saturating_sub(1));
        self.scroll_to_keep_visible(target, viewport_height);
    }

    fn enter_visual_with_kind(&mut self, kind: VisualKind) {
        if self.lines.is_empty() {
            return;
        }
        let max = self.lines.len() - 1;
        let start = (self.scroll as usize).min(max);
        self.visual = Some(VisualSelection {
            anchor: start,
            cursor: start,
            anchor_col: 0,
            cursor_col: 0,
            kind,
        });
    }

    pub const fn cancel_visual(&mut self) {
        self.visual = None;
    }

    /// Move the visual-mode cursor by `delta` lines (clamped to the
    /// buffer), and auto-scroll the viewport so the cursor stays
    /// visible. No-op when not in visual mode.
    pub fn visual_move(&mut self, delta: isize, viewport_height: u16) {
        let Some(sel) = self.visual.as_mut() else {
            return;
        };
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let new = (sel.cursor as isize + delta).clamp(0, n as isize - 1) as usize;
        sel.cursor = new;
        self.scroll_to_keep_visible(new, viewport_height);
    }

    /// Jump the visual-mode cursor to a specific line, scrolling as
    /// needed. Used by `g`/`G`/`:N` while a selection is active.
    pub fn visual_jump_to(&mut self, line: usize, viewport_height: u16) {
        let Some(sel) = self.visual.as_mut() else {
            return;
        };
        let n = self.lines.len();
        if n == 0 {
            return;
        }
        let target = line.min(n - 1);
        sel.cursor = target;
        self.scroll_to_keep_visible(target, viewport_height);
    }

    /// Adjust `scroll` so `line` is in the viewport. Visual cursor
    /// helper, factored out so both `visual_move` and `visual_jump_to`
    /// share the same edge logic.
    const fn scroll_to_keep_visible(&mut self, line: usize, viewport_height: u16) {
        let top = self.scroll as usize;
        let vh = viewport_height as usize;
        if vh == 0 {
            return;
        }
        let bot = top + vh;
        if line < top {
            self.scroll = line as u16;
        } else if line >= bot {
            self.scroll = (line + 1).saturating_sub(vh) as u16;
        }
    }

    /// Move the block-mode column cursor by `delta` characters.
    /// Clamped at column 0 on the left; uncapped on the right
    /// (selection past the line end is allowed — vim does the same;
    /// short rows in the rectangle just contribute fewer chars to
    /// the yanked output). No-op outside block mode.
    pub fn visual_col_move(&mut self, delta: isize) {
        let Some(sel) = self.visual.as_mut() else {
            return;
        };
        if sel.kind != VisualKind::Block {
            return;
        }
        let new = (sel.cursor_col as isize + delta).max(0) as usize;
        sel.cursor_col = new;
    }

    /// Yank the visual-mode selection to the clipboard and exit.
    /// `Line` mode joins whole rows with newlines; `Block` mode
    /// joins the rectangular slice (rows × columns), where each
    /// row contributes `line[lo_col..=hi_col]` (character indices,
    /// not display columns) — rows shorter than the range
    /// contribute their available chars and stop. Returns the
    /// number of rows yanked. The header rule is the same as the
    /// full-buffer yank — when partial-range, the source context
    /// is *more* useful, not less.
    pub fn yank_visual_to_clipboard(&mut self, include_title: bool) -> std::io::Result<usize> {
        let Some(sel) = self.visual else {
            return Ok(0);
        };
        if self.lines.is_empty() {
            self.visual = None;
            return Ok(0);
        }
        // Clamp BOTH ends to the current buffer. The buffer can shrink under
        // an active selection (a streaming task viewer front-trims at
        // TASK_BUFFER_CAP), and `range()` may then return `lo`/`hi` past the
        // end. Clamping only `hi` leaves `lo > hi`, so `self.lines[lo..=hi]`
        // panics. Clamping both to the same ceiling preserves `lo <= hi`.
        let max = self.lines.len() - 1;
        let (lo, hi) = sel.range();
        let (lo, hi) = (lo.min(max), hi.min(max));
        let text = match sel.kind {
            VisualKind::Line => self.lines[lo..=hi]
                .iter()
                .map(line_plain_text)
                .collect::<Vec<_>>()
                .join("\n"),
            VisualKind::Block => {
                let (lo_col, hi_col) = sel.col_range();
                self.lines[lo..=hi]
                    .iter()
                    .map(|line| {
                        let plain = line_plain_text(line);
                        plain
                            .chars()
                            .skip(lo_col)
                            .take(hi_col + 1 - lo_col)
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };
        crate::clipboard::copy(&self.with_title_header(text, include_title))?;
        let count = hi - lo + 1;
        self.visual = None;
        Ok(count)
    }

    /// Clamp any state holding line indices into the buffer after `lines`
    /// has been replaced wholesale (e.g. a streaming task viewer front-trims
    /// at `TASK_BUFFER_CAP`, or `apply_exit_event` rebuilds the view). A
    /// visual selection / placement cursor whose rows now point past the end
    /// would otherwise yank the wrong content (or, before the yank clamp,
    /// panic), and `Search::Active` match indices would highlight the wrong
    /// lines. Call this at every site that reassigns `self.lines` under a
    /// potentially-active selection or search.
    pub fn clamp_state_to_lines(&mut self) {
        let len = self.lines.len();
        if len == 0 {
            self.visual = None;
            self.placement = None;
        } else {
            let max = len - 1;
            if let Some(sel) = self.visual.as_mut() {
                sel.anchor = sel.anchor.min(max);
                sel.cursor = sel.cursor.min(max);
            }
            if let Some(p) = self.placement.as_mut() {
                p.row = p.row.min(max);
            }
        }
        let cleared = if let Search::Active {
            matches, cursor, ..
        } = &mut self.search
        {
            matches.retain(|&m| m < len);
            if *cursor >= matches.len() {
                *cursor = matches.len().saturating_sub(1);
            }
            matches.is_empty()
        } else {
            false
        };
        if cleared {
            self.search = Search::Off;
        }
    }
}
