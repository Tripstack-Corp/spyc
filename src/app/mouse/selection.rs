//! Mouse drag-selection: the `begin` / `extend` / `finish` triples for the
//! four selectable surfaces — pager text, chrome (status bar + divider), pane
//! text, and file-list rows. Extracted verbatim from `mouse/mod.rs`.
//!
//! The four clusters share a shape: a press anchors and *claims* the drag
//! (`view.mouse_selection`), motion extends it, release copies and clears.
//! Claiming is what diverts subsequent drags away from the pane's child, so a
//! press that fails to anchor must not claim.

use crossterm::event::MouseEvent;
use ratatui::layout::Rect;

use super::super::Effect;
use super::super::pager_handler::PagerSlot;
use super::route::clamp_to_area;
use super::{ChromeSelection, ListSelection, MouseDragTarget};

impl super::super::App {
    /// Anchor a charwise selection where the pointer pressed.
    ///
    /// Claims the drag (`view.mouse_selection`) only if the press actually landed
    /// on a character. A press on the gutter or below the last line focuses the
    /// pager and nothing more, so it cannot strand a selection that never had an
    /// anchor — and, because claiming is what diverts drags away from the child,
    /// an unclaimed press leaves forwarding exactly as it was.
    pub(super) fn begin_pager_selection(&mut self, ev: MouseEvent, slot: PagerSlot) -> Vec<Effect> {
        let Some(view) = self.pager_in_slot_mut(slot) else {
            return Vec::new();
        };
        let Some((line, col)) = view.hit_test(ev.column, ev.row) else {
            return Vec::new();
        };
        view.begin_char_selection(line, col);
        self.view.mouse_selection = Some(MouseDragTarget::Pager(slot));
        Vec::new()
    }

    /// Extend the in-progress selection to the pointer.
    ///
    /// A pointer past the content rect's top/bottom edge scrolls the pager toward
    /// it and extends to the new edge line — the pager keeps hold of the gesture
    /// (**"full priority" while it's open**) instead of freezing once the pointer
    /// leaves its rect, which read as the pager handing the drag off to whatever
    /// is on screen below it. `clamp_to_area` folds a horizontal miss (left/right
    /// of the box, or before the first render) into the same treatment: extend to
    /// the nearest edge rather than doing nothing.
    pub(super) fn extend_pager_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        let Some(MouseDragTarget::Pager(slot)) = self.view.mouse_selection else {
            return Vec::new();
        };
        let Some(view) = self.pager_in_slot_mut(slot) else {
            return Vec::new();
        };
        let Some((col, row)) = clamp_to_area(view, ev.column, ev.row) else {
            return Vec::new(); // never rendered — nothing to scroll or clamp to
        };
        // Scroll toward the pointer BEFORE hit-testing the clamped edge row, so the
        // same on-screen row resolves to a new (further) line each time this fires
        // — which, since a physically-held drag keeps generating `Drag` events as
        // long as the pointer moves at all, gives a continuous scroll-and-extend
        // for as long as the user holds past the edge and wiggles the pointer.
        let area = view.last_content_area.get();
        if ev.row < area.y {
            view.scroll_by_within_content(-1, view.last_viewport_h.get());
        } else if ev.row >= area.y.saturating_add(area.height) {
            view.scroll_by_within_content(1, view.last_viewport_h.get());
        }
        if let Some((line, c)) = view.hit_test(col, row) {
            view.extend_char_selection(line, c);
        }
        Vec::new()
    }

    /// Finish the gesture: copy when it selected something, and treat a press that
    /// never moved as the plain click it was.
    ///
    /// The selection is left highlighted rather than cleared. Unlike the keyboard
    /// `y` key — vim convention: yank exits visual mode, unchanged here — a mouse
    /// release is the terminal-native "select, then it just sits there" contract,
    /// and it's what lets a follow-up `y` / `Y` / a fresh drag still find it.
    pub(super) fn finish_pager_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        let Some(MouseDragTarget::Pager(slot)) = self.view.mouse_selection.take() else {
            return Vec::new();
        };
        let Some(view) = self.pager_in_slot_mut(slot) else {
            return Vec::new();
        };
        if let Some((col, row)) = clamp_to_area(view, ev.column, ev.row)
            && let Some((line, c)) = view.hit_test(col, row)
        {
            view.extend_char_selection(line, c);
        }
        if !view.char_selection_is_nonempty() {
            // A click, not a drag. Drop the zero-width selection so it doesn't
            // linger as a stray highlight, and leave the clipboard alone.
            view.visual = None;
            return Vec::new();
        }
        // Captured before `visual_yank_text` consumes it (exits visual mode — the
        // contract the `y` key relies on), so it can be reinstated below.
        let sel = view.visual;
        let Some((text, lines, _)) = view.visual_yank_text(false) else {
            return Vec::new();
        };
        view.visual = sel;
        if text.is_empty() {
            return Vec::new();
        }
        // `CopyToPagerClipboard`, not `CopyToClipboard`: it lands the confirmation
        // in the pager title where the user is already looking, which is the same
        // place the `y` key reports to. A status-bar flash would be behind the
        // full-frame mount.
        let ok_msg = format!("copied {lines} line{}", if lines == 1 { "" } else { "s" });
        vec![Effect::CopyToPagerClipboard { text, ok_msg }]
    }

    /// The chrome row under the pointer, and the pointer's column within it.
    ///
    /// Matched on the recorded row's `y`, which is its identity — each chrome
    /// surface is exactly one screen row. Reads what the renderer actually drew, so
    /// the column the user pressed maps to the character they saw.
    fn chrome_col_at(&self, ev: MouseEvent) -> Option<(u16, u16)> {
        let rows = self.view.chrome_rows.borrow();
        let row = rows.iter().find(|r| r.y == ev.row && ev.column >= r.x)?;
        Some((row.y, ev.column - row.x))
    }

    /// Anchor a chrome-line selection where the pointer pressed.
    pub(super) fn begin_chrome_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        let Some((y, col)) = self.chrome_col_at(ev) else {
            return Vec::new();
        };
        self.view.chrome_selection = Some(ChromeSelection {
            y,
            anchor_col: col,
            focus_col: col,
        });
        self.view.mouse_selection = Some(MouseDragTarget::Chrome);
        Vec::new()
    }

    /// Extend it. The pointer is clamped to the row it started on: these surfaces
    /// are one row each, so drifting vertically mid-drag should widen the selection
    /// rather than abandon it.
    pub(super) fn extend_chrome_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        let Some(sel) = self.view.chrome_selection else {
            return Vec::new();
        };
        let col = {
            let rows = self.view.chrome_rows.borrow();
            let Some(row) = rows.iter().find(|r| r.y == sel.y) else {
                return Vec::new();
            };
            ev.column.saturating_sub(row.x)
        };
        if let Some(s) = self.view.chrome_selection.as_mut() {
            s.focus_col = col;
        }
        Vec::new()
    }

    /// Copy the selected columns, keeping the highlight up.
    ///
    /// A press that never moved is a click, not a selection — the highlight is
    /// dropped and the clipboard left alone, matching every other surface. That is
    /// also why this replaced click-copies-everything: a click now does nothing
    /// surprising.
    pub(super) fn finish_chrome_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        self.view.mouse_selection = None;
        self.extend_chrome_selection(ev);
        let Some(sel) = self.view.chrome_selection else {
            return Vec::new();
        };
        if sel.anchor_col == sel.focus_col {
            self.view.chrome_selection = None;
            return Vec::new();
        }
        let (lo, hi) = sel.range();
        let text = {
            let rows = self.view.chrome_rows.borrow();
            let Some(row) = rows.iter().find(|r| r.y == sel.y) else {
                return Vec::new();
            };
            crate::ui::line_select::text_between_columns(
                &row.line,
                usize::from(lo),
                usize::from(hi),
            )
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            return Vec::new();
        }
        vec![Effect::CopyToClipboard {
            text,
            ok: super::super::effect::ClipMsg::StatusLine,
        }]
    }

    /// Translate the pointer into the pane's own grid coordinates, clamped into it.
    ///
    /// Clamped rather than rejected outside the rect: dragging a little past the
    /// pane's edge is normal, and it should extend to the edge cell instead of
    /// stalling. `None` only when there is no pane rect at all.
    fn pane_cell_at(&self, ev: MouseEvent) -> Option<(u16, u16)> {
        let (cols, rows) = self.view.term_size;
        let pane = self.frame_layout(Rect::new(0, 0, cols, rows)).pane?;
        if pane.width == 0 || pane.height == 0 {
            return None;
        }
        let col = ev.column.clamp(pane.x, pane.x + pane.width - 1) - pane.x;
        let row = ev.row.clamp(pane.y, pane.y + pane.height - 1) - pane.y;
        Some((row, col))
    }

    /// Anchor a pane text selection where the pointer pressed.
    pub(super) fn begin_pane_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        let Some(cell) = self.pane_cell_at(ev) else {
            return Vec::new();
        };
        self.view.pane_selection = Some((cell, cell));
        self.view.mouse_selection = Some(MouseDragTarget::Pane);
        Vec::new()
    }

    /// Extend it to the pointer.
    pub(super) fn extend_pane_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        if let Some(cell) = self.pane_cell_at(ev)
            && let Some((_, focus)) = self.view.pane_selection.as_mut()
        {
            *focus = cell;
        }
        Vec::new()
    }

    /// Copy the selected grid text, keeping the highlight up.
    ///
    /// A press that never moved is a click, not a selection: the highlight is
    /// dropped and the clipboard left alone, matching the pager. Ordering happens
    /// here (`(row, col)` pairs, not per-axis) so a backwards drag copies the same
    /// text as the forward one.
    pub(super) fn finish_pane_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        self.view.mouse_selection = None;
        if let Some(cell) = self.pane_cell_at(ev)
            && let Some((_, focus)) = self.view.pane_selection.as_mut()
        {
            *focus = cell;
        }
        let Some((a, b)) = self.view.pane_selection else {
            return Vec::new();
        };
        if a == b {
            self.view.pane_selection = None;
            return Vec::new();
        }
        let (start, end) = if a.0 < b.0 || (a.0 == b.0 && a.1 <= b.1) {
            (a, b)
        } else {
            (b, a)
        };
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return Vec::new();
        };
        let text = tabs.active().selection_text(start, end);
        if text.trim().is_empty() {
            return Vec::new();
        }
        vec![Effect::CopyToClipboard {
            text,
            ok: super::super::effect::ClipMsg::PaneLines,
        }]
    }

    /// The rect a column's list is drawn into, and a `ListView` over its cached
    /// rows — the same pair the renderer builds, so a hit-test can't disagree with
    /// what's on screen.
    fn list_row_at(&self, side: crate::app::state::Side, ev: MouseEvent) -> Option<usize> {
        use crate::app::state::Side;
        let (cols, rows) = self.view.term_size;
        let layout = self.frame_layout(Rect::new(0, 0, cols, rows));
        let (area, cached, commander) = match side {
            Side::Left => (layout.list, &self.view.cached_rows, &self.state.left),
            Side::Right => (
                layout.right?,
                &self.view.right_cached_rows,
                self.state.right.as_ref()?,
            ),
        };
        crate::ui::list_view::ListView {
            rows: cached,
            cursor: commander.cursor.index,
            view_top: commander.cursor.view_top,
            empty_marker: false,
            focused: true,
            theme: &self.view.theme,
            selection: None,
        }
        .row_at(area, ev.column, ev.row)
    }

    /// Anchor a row selection where the pointer pressed, and move the cursor there.
    ///
    /// Moving the cursor is deliberate: clicking a file should put the cursor on it,
    /// which is what makes the click useful on its own rather than only as the start
    /// of a drag. Claims the drag only if the press landed on a row, so a press in
    /// the gutter between columns leaves forwarding and focus untouched.
    ///
    /// The modifier is read HERE rather than carried in the sink because it selects
    /// the copy's *content*, not its routing. Ctrl, not Shift — Shift is the
    /// terminal's own selection-bypass and is frequently consumed before spyc sees
    /// it.
    pub(super) fn begin_row_selection(
        &mut self,
        ev: MouseEvent,
        side: crate::app::state::Side,
    ) -> Vec<Effect> {
        let Some(idx) = self.list_row_at(side, ev) else {
            return Vec::new();
        };
        let full_path = ev
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL);
        self.state.col_mut(side).cursor.index = idx;
        self.view.list_selection = Some(ListSelection {
            side,
            anchor: idx,
            focus: idx,
            full_path,
        });
        self.view.mouse_selection = Some(MouseDragTarget::List(side));
        Vec::new()
    }

    /// Extend the row selection to the pointer, dragging the cursor with it.
    ///
    /// A pointer off the rows holds the selection where it was, matching the pager:
    /// drifting a few pixels past the last row mid-drag is normal, and collapsing
    /// there would read as a bug.
    pub(super) fn extend_row_selection(
        &mut self,
        ev: MouseEvent,
        side: crate::app::state::Side,
    ) -> Vec<Effect> {
        if let Some(idx) = self.list_row_at(side, ev) {
            if let Some(sel) = self.view.list_selection.as_mut() {
                sel.focus = idx;
            }
            self.state.col_mut(side).cursor.index = idx;
        }

        Vec::new()
    }

    /// Copy the selected rows' names (or absolute paths) and keep the highlight.
    ///
    /// Paths are resolved from the LIVE listing here rather than captured at press
    /// time, so a selection can't paste stale paths after a refresh. A single row is
    /// still a copy — unlike the pager, where a click that never moved is just a
    /// click, clicking one filename to copy it is the obvious gesture.
    pub(super) fn finish_row_selection(&mut self, ev: MouseEvent) -> Vec<Effect> {
        self.view.mouse_selection = None;
        let Some(sel) = self.view.list_selection else {
            return Vec::new();
        };
        if let Some(idx) = self.list_row_at(sel.side, ev)
            && let Some(s) = self.view.list_selection.as_mut()
        {
            s.focus = idx;
        }
        let sel = self.view.list_selection.expect("checked above");
        let (lo, hi) = sel.range();
        let commander = self.state.col(sel.side);
        let picked: Vec<String> = commander
            .rows
            .iter()
            .skip(lo)
            .take(hi.saturating_sub(lo) + 1)
            .map(|r| {
                if sel.full_path {
                    r.path.display().to_string()
                } else {
                    r.display.clone()
                }
            })
            .collect();
        if picked.is_empty() {
            return Vec::new();
        }
        let count = picked.len();
        vec![Effect::CopyToClipboard {
            text: picked.join("\n"),
            ok: super::super::effect::ClipMsg::ListNames {
                count,
                paths: sel.full_path,
            },
        }]
    }
}
