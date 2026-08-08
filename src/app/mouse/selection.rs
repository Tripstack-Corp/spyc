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
    pub(super) fn chrome_col_at(&self, ev: MouseEvent) -> Option<(u16, u16)> {
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

#[cfg(test)]
mod tests {

    use crate::app::App;
    use crate::app::pager_handler::PagerSlot;
    use crate::app::state::Side;

    use crate::ui::pager::{PagerView, VisualKind};
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    // ── pager selection: begin / extend / finish (App methods, not the pure
    // `route_mouse` decision) ──────────────────────────────────────────────

    use crate::app::Effect;

    /// A pager mounted at a known, small content rect — as if it had just
    /// rendered — with enough lines to scroll. Lines are single chars at a
    /// known width so column math in the tests is easy to check by hand.
    fn mounted_pager() -> PagerView {
        let mut view = PagerView::new_plain("t", (0..10).map(|i| format!("line{i}")).collect());
        // Small on purpose: a 2-row viewport out of 10 lines guarantees both
        // "past the bottom" and "past the top" are reachable within a few
        // Drag events, without wrap complicating the row math.
        view.wrap = false;
        view.show_line_numbers = false;
        view.last_content_area.set(Rect::new(0, 0, 10, 2));
        view.last_viewport_h.set(2);
        view
    }

    fn app_with_pager(view: PagerView) -> App {
        let tmp = tempfile::tempdir().expect("tempdir");
        // `test_app` reads/writes under the state root; leak the tempdir for
        // the test's lifetime rather than threading a closure through every
        // call site below.
        let path = tmp.keep();
        crate::state::with_state_root(&path, || {
            let mut app = App::test_app(path.clone());
            app.view.pager = Some(view);
            app
        })
    }

    fn left_down(col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn left_drag(col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn left_up(col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    /// The headline fix: a drag held past the BOTTOM edge scrolls the pager
    /// toward the pointer and keeps extending, rather than freezing once the
    /// pointer leaves the content rect — which is what made the pager seem to
    /// hand the gesture off to whatever is on screen below it.
    #[test]
    fn drag_past_the_bottom_edge_scrolls_and_keeps_extending() {
        let mut app = app_with_pager(mounted_pager());
        app.begin_pager_selection(left_down(0, 0), PagerSlot::Modal);
        let before = app.view.pager.as_ref().unwrap().scroll;

        // Row 5 is three rows past the 2-row viewport — squarely "in the pane".
        app.extend_pager_selection(left_drag(0, 5));

        let view = app.view.pager.as_ref().expect("still mounted");
        assert!(
            view.scroll > before,
            "a drag past the bottom edge must scroll the pager, not freeze it"
        );
        let sel = view
            .visual
            .expect("selection survives a drag past the edge");
        assert!(
            sel.cursor > sel.anchor,
            "the selection must keep growing, not stick at the edge"
        );
    }

    /// The symmetric case: past the TOP edge scrolls up. The rect is offset
    /// from row 0 specifically so there is screen space above it to drag into.
    #[test]
    fn drag_past_the_top_edge_scrolls_up() {
        let mut view = mounted_pager();
        view.scroll = 5; // start mid-document so there's room to scroll up
        view.last_content_area.set(Rect::new(0, 3, 10, 2)); // rows 3-4; row 0 is "above"
        let mut app = app_with_pager(view);
        app.begin_pager_selection(left_down(0, 3), PagerSlot::Modal);

        app.extend_pager_selection(left_drag(0, 0));

        let view = app.view.pager.as_ref().expect("still mounted");
        assert!(view.scroll < 5, "a drag past the top edge must scroll up");
    }

    /// A pointer beside (not above/below) the rect clamps horizontally and
    /// keeps extending — it must not require the drag to stay inside the box
    /// pixel-perfectly to keep selecting.
    #[test]
    fn drag_beyond_the_right_edge_clamps_without_scrolling() {
        let mut app = app_with_pager(mounted_pager());
        app.begin_pager_selection(left_down(0, 0), PagerSlot::Modal);
        let scroll_before = app.view.pager.as_ref().unwrap().scroll;

        app.extend_pager_selection(left_drag(50, 0)); // same row, way past the right edge

        let view = app.view.pager.as_ref().expect("still mounted");
        assert_eq!(
            view.scroll, scroll_before,
            "a horizontal miss must not scroll"
        );
        let sel = view.visual.expect("still selecting");
        // The rect's right edge (col 9) clamps first, and THEN `hit_test` clamps
        // again to the line's own length ("line0" is 5 chars, max index 4) — the
        // same end-of-line behavior `hit_test_clamps_past_end_of_line` pins.
        assert_eq!(
            sel.cursor_col, 4,
            "clamped into the rect, then to end-of-line"
        );
    }

    /// The other half of the report: releasing after a real drag must leave
    /// the selection highlighted (the terminal-native "it just sits there"
    /// contract), not clear it — while still copying and reporting the count.
    #[test]
    fn release_after_a_drag_copies_and_keeps_the_highlight() {
        let mut app = app_with_pager(mounted_pager());
        app.begin_pager_selection(left_down(0, 0), PagerSlot::Modal);
        app.extend_pager_selection(left_drag(3, 1));

        let effects = app.finish_pager_selection(left_up(3, 1));

        assert!(
            matches!(effects.as_slice(), [Effect::CopyToPagerClipboard { .. }]),
            "a real drag must copy on release: {effects:?}"
        );
        let sel = app
            .view
            .pager
            .as_ref()
            .and_then(|v| v.visual)
            .expect("selection must survive the copy, unlike the keyboard `y` contract");
        assert_eq!(sel.kind, VisualKind::Char);
    }

    /// A press that never moves is a plain click: no copy, and no stray
    /// zero-width highlight left behind.
    #[test]
    fn release_without_motion_is_a_click_not_a_selection() {
        let mut app = app_with_pager(mounted_pager());
        app.begin_pager_selection(left_down(2, 0), PagerSlot::Modal);

        let effects = app.finish_pager_selection(left_up(2, 0));

        assert!(effects.is_empty(), "a plain click must not copy anything");
        assert!(
            app.view.pager.as_ref().unwrap().visual.is_none(),
            "a plain click must not leave a stray highlight"
        );
    }
    // ── file list + status bar selection ─────────────────────────────────

    /// The `Row`s the renderer would have cached for `rows`. Built here rather than
    /// via `build_rows` (scoped to `app::render`) so the test doesn't widen
    /// production visibility; `row_at` only reads `len` and `display`.
    fn cached_for(rows: &[crate::app::RowData]) -> Vec<crate::ui::list_view::Row> {
        rows.iter()
            .map(|rd| crate::ui::list_view::Row {
                display: rd.display.clone(),
                kind: rd.kind,
                picked: false,
                taken: false,
                deleted: false,
                git_status: crate::ui::list_view::GitFileStatus::clean(),
                pending_delete: false,
            })
            .collect()
    }

    /// A list drag copies the dragged rows' NAMES, and with Ctrl held their absolute
    /// PATHS — the modifier is read at press time because it selects the copy's
    /// content, not its routing.
    ///
    /// Ctrl and not Shift: Shift is the terminal's own selection-bypass modifier and
    /// is frequently consumed before spyc ever sees the event.
    #[test]
    fn a_list_drag_copies_names_or_full_paths_with_ctrl() {
        use crossterm::event::KeyModifiers;

        for (mods, want_paths) in [(KeyModifiers::NONE, false), (KeyModifiers::CONTROL, true)] {
            let tmp = tempfile::tempdir().expect("tempdir").keep();
            let effects = crate::state::with_state_root(&tmp, || {
                let mut app = App::test_app(tmp.clone());
                app.state.left.rows = ["alpha.rs", "beta.rs", "gamma.rs"]
                    .iter()
                    .map(|n| crate::app::RowData {
                        path: tmp.join(n),
                        display: (*n).to_string(),
                        kind: crate::fs::EntryKind::File,
                        deleted: false,
                    })
                    .collect();
                // Geometry the renderer would have settled.
                app.view.term_size = (80, 24);
                app.view.cached_rows = cached_for(&app.state.left.rows);

                let at = |kind, row, m| MouseEvent {
                    kind,
                    column: 0,
                    row,
                    modifiers: m,
                };
                app.begin_row_selection(
                    at(MouseEventKind::Down(MouseButton::Left), 1, mods),
                    Side::Left,
                );
                app.extend_row_selection(
                    at(MouseEventKind::Drag(MouseButton::Left), 3, mods),
                    Side::Left,
                );
                app.finish_row_selection(at(MouseEventKind::Up(MouseButton::Left), 3, mods))
            });

            let [Effect::CopyToClipboard { text, .. }] = effects.as_slice() else {
                panic!("expected one clipboard copy, got {effects:?}");
            };
            let lines: Vec<&str> = text.lines().collect();
            if want_paths {
                assert!(
                    lines.iter().all(|l| l.starts_with('/')),
                    "Ctrl held must copy absolute paths, got {lines:?}"
                );
                assert!(lines.iter().any(|l| l.ends_with("alpha.rs")));
            } else {
                assert!(
                    lines.iter().all(|l| !l.contains('/')),
                    "without Ctrl must copy bare names, got {lines:?}"
                );
            }
            assert!(lines.len() > 1, "a drag across rows selects a range");
        }
    }

    /// The highlight survives the copy, so it's visible and a follow-up yank can
    /// still find it — the same contract the pager's selection has.
    #[test]
    fn a_list_selection_persists_after_the_copy() {
        use crossterm::event::KeyModifiers;
        let tmp = tempfile::tempdir().expect("tempdir").keep();
        crate::state::with_state_root(&tmp, || {
            let mut app = App::test_app(tmp.clone());
            app.state.left.rows = (0..4)
                .map(|i| crate::app::RowData {
                    path: tmp.join(format!("f{i}")),
                    display: format!("f{i}"),
                    kind: crate::fs::EntryKind::File,
                    deleted: false,
                })
                .collect();
            app.view.term_size = (80, 24);
            app.view.cached_rows = cached_for(&app.state.left.rows);
            let at = |kind, row| MouseEvent {
                kind,
                column: 0,
                row,
                modifiers: KeyModifiers::NONE,
            };
            // Screen row 1 is the list's first row — row 0 is the status bar, which
            // `row_at` correctly refuses.
            app.begin_row_selection(at(MouseEventKind::Down(MouseButton::Left), 1), Side::Left);
            app.extend_row_selection(at(MouseEventKind::Drag(MouseButton::Left), 3), Side::Left);
            let _ = app.finish_row_selection(at(MouseEventKind::Up(MouseButton::Left), 3));

            let sel = app
                .view
                .list_selection
                .expect("highlight must survive the copy");
            assert_eq!(sel.range(), (0, 2));
            assert!(
                app.view.mouse_selection.is_none(),
                "the drag itself must end, even though the selection stays"
            );
        });
    }
    // ── pane text selection (a child that ignores mouse) ──────────────────

    /// A mouse selection holds POSITIONS, not content, so the next keyboard action
    /// must retire it — otherwise navigating away leaves a highlight band sitting on
    /// whatever file now occupies those row indices, which is how this was reported.
    ///
    /// The pager's `visual` selection is exempt: it's modal by request, so `j`/`k`
    /// extend it and `Esc` cancels.
    #[test]
    fn a_key_retires_a_mouse_selection_but_not_the_pagers_visual_mode() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let tmp = tempfile::tempdir().expect("tempdir").keep();
        crate::state::with_state_root(&tmp, || {
            let mut app = App::test_app(tmp.clone());
            app.view.list_selection = Some(super::ListSelection {
                side: Side::Left,
                anchor: 1,
                focus: 3,
                full_path: false,
            });
            app.view.pane_selection = Some(((0, 0), (2, 4)));
            app.view.chrome_selection = Some(super::ChromeSelection {
                y: 0,
                anchor_col: 2,
                focus_col: 8,
            });

            let mut pager = PagerView::new_plain("p", vec!["a".into(), "b".into()]);
            pager.visual = Some(crate::ui::pager::VisualSelection {
                anchor: 0,
                cursor: 1,
                anchor_col: 0,
                cursor_col: 0,
                kind: VisualKind::Char,
            });
            app.view.pager = Some(pager);

            let _ = app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

            assert!(
                app.view.list_selection.is_none(),
                "a keypress must retire the list selection"
            );
            assert!(
                app.view.pane_selection.is_none(),
                "a keypress must retire the pane selection"
            );
            assert!(
                app.view.chrome_selection.is_none(),
                "a keypress must retire the chrome selection"
            );
            assert!(
                app.view.pager.as_ref().and_then(|v| v.visual).is_some(),
                "the pager's visual mode is modal and must survive"
            );
        });
    }
    /// End-to-end: a drag across part of a chrome line copies exactly those columns
    /// — the point of the feature being substring selection rather than
    /// copy-the-whole-line (so you can take just a session id or a branch name).
    #[test]
    fn a_chrome_drag_copies_only_the_dragged_columns() {
        use crossterm::event::KeyModifiers;
        let tmp = tempfile::tempdir().expect("tempdir").keep();
        crate::state::with_state_root(&tmp, || {
            let mut app = App::test_app(tmp.clone());
            // Stand in for what the renderer records: one chrome row at y=0, x=0.
            app.view
                .chrome_rows
                .borrow_mut()
                .push(crate::app::mouse::ChromeRow {
                    y: 0,
                    x: 0,
                    line: ratatui::text::Line::from(vec![
                        ratatui::text::Span::raw("spyc"),
                        ratatui::text::Span::raw("|"),
                        ratatui::text::Span::raw("main*"),
                    ]),
                });
            let at = |kind, col| MouseEvent {
                kind,
                column: col,
                row: 0,
                modifiers: KeyModifiers::NONE,
            };
            // Drag columns 5..9 — "main*", crossing no segment seam of its own but
            // starting past two.
            app.begin_chrome_selection(at(MouseEventKind::Down(MouseButton::Left), 5));
            app.extend_chrome_selection(at(MouseEventKind::Drag(MouseButton::Left), 9));
            let effects = app.finish_chrome_selection(at(MouseEventKind::Up(MouseButton::Left), 9));

            let [Effect::CopyToClipboard { text, .. }] = effects.as_slice() else {
                panic!("expected one clipboard copy, got {effects:?}");
            };
            assert_eq!(text, "main*");
            assert!(
                app.view.chrome_selection.is_some(),
                "the highlight stays up after the copy, like the other surfaces"
            );
        });
    }

    /// A click that never moved is a click: no copy, no lingering highlight. This is
    /// what makes replacing click-copies-the-whole-line safe — a stray click on the
    /// status bar no longer silently overwrites the clipboard.
    #[test]
    fn a_chrome_click_without_motion_copies_nothing() {
        use crossterm::event::KeyModifiers;
        let tmp = tempfile::tempdir().expect("tempdir").keep();
        crate::state::with_state_root(&tmp, || {
            let mut app = App::test_app(tmp.clone());
            app.view
                .chrome_rows
                .borrow_mut()
                .push(crate::app::mouse::ChromeRow {
                    y: 0,
                    x: 0,
                    line: ratatui::text::Line::from(vec![ratatui::text::Span::raw("spyc|main*")]),
                });
            let at = |kind| MouseEvent {
                kind,
                column: 3,
                row: 0,
                modifiers: KeyModifiers::NONE,
            };
            app.begin_chrome_selection(at(MouseEventKind::Down(MouseButton::Left)));
            let effects = app.finish_chrome_selection(at(MouseEventKind::Up(MouseButton::Left)));
            assert!(effects.is_empty(), "a click must not copy: {effects:?}");
            assert!(
                app.view.chrome_selection.is_none(),
                "and leaves no highlight"
            );
        });
    }
}
