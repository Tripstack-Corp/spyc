//! Vertical (left/right) split handlers: the `^a |` cycle, a/b focus, and
//! width resize. The split *shape* is Model state (`AppState.vsplit`); the
//! right-region preview content lives in `ViewState.right_pager`. The pure
//! cycle transition is extracted + unit-tested (the `route.rs`/`focus.rs`
//! template). The cursor-file validation + preview load live with the other
//! pager-build code (`pager_handler::{previewable_cursor_path, load_right_preview}`).

use super::{App, state};

/// Default right-column width when a split opens (percent of the frame).
const DEFAULT_VSPLIT_PCT: u16 = 50;

/// Each split column floors at this width; below `2*MIN_COL + 1` (the two
/// columns + the divider) there's no room, so the split is refused.
const MIN_COL: u16 = 20;

/// The `(left_w, right_w)` column widths for a vertical split of a region `w`
/// columns wide at `width_pct` (the **right** column's share), reserving one
/// column for the divider. Clamps the percentage to `[20, 80]` and floors each
/// column at [`MIN_COL`]; returns `None` when `w` is too narrow for two usable
/// columns. The single source of truth for both `carve_vsplit`'s geometry and
/// the markdown wrap widths, so they can't drift (and a session-restored
/// out-of-range `width_pct` is clamped here too).
pub(super) fn vsplit_column_widths(w: u16, width_pct: u16) -> Option<(u16, u16)> {
    if w < MIN_COL * 2 + 1 {
        return None;
    }
    let pct = width_pct.clamp(20, 80);
    let right_w = (((u32::from(w) * u32::from(pct)) / 100) as u16).clamp(MIN_COL, w - MIN_COL - 1);
    Some((w - right_w - 1, right_w))
}

/// Pure `^a |` transition: off → top-only → full-height → off. Width and
/// focused side carry across the top-only → full-height flip.
pub(super) fn next_vsplit(current: Option<state::VSplit>) -> Option<state::VSplit> {
    use state::{Side, VSplit, VsplitMode};
    match current {
        None => Some(VSplit {
            width_pct: DEFAULT_VSPLIT_PCT,
            mode: VsplitMode::TopOnly,
            focus: Side::Left,
        }),
        Some(v) if v.mode == VsplitMode::TopOnly => Some(VSplit {
            mode: VsplitMode::FullHeight,
            ..v
        }),
        Some(_) => None,
    }
}

impl App {
    /// `^a |` — the vertical-split key. Behavior depends on what's open and
    /// what's under the cursor:
    /// - **Closed:** open (top-only) previewing the cursor file. If the cursor
    ///   isn't a previewable file (e.g. a directory), warn and stay closed.
    /// - **Open, cursor on a *different* file:** swap the preview to that file,
    ///   keeping the current shape (mode/width) — "send this file to the split".
    /// - **Open, cursor on the same file (or not previewable):** cycle the
    ///   shape: top-only → full-height → off.
    pub(super) fn cycle_vsplit(&mut self) {
        // The right region hosts EITHER a preview (`^a |`) or a second
        // commander (`^s`) — never both. With a commander open, `^a |` is
        // disabled (no nesting); close it with `^s x` first.
        if self.state.right.is_some() {
            self.state
                .flash_info("right column has a commander (^s x to close)");
            return;
        }
        let cursor_file = self.previewable_cursor_path();
        if self.state.vsplit.is_none() {
            // Opening: require a previewable file under the cursor.
            let Some(path) = cursor_file else {
                self.state.flash_info("not a valid file for the pager");
                return;
            };
            // Default mode: full-height when no lower pane is open — top-only
            // would reserve a strip for a pane that isn't there. With a pane,
            // top-only keeps it full-width below both columns.
            let pane_open = self.runtime.pane_tabs.is_some() && !self.state.pane.pane_hidden;
            let mode = if pane_open {
                state::VsplitMode::TopOnly
            } else {
                state::VsplitMode::FullHeight
            };
            self.state.vsplit = Some(state::VSplit {
                width_pct: DEFAULT_VSPLIT_PCT,
                mode,
                focus: state::Side::Left,
            });
            self.load_right_preview(&path);
            // If the file couldn't be read/rendered, `load_right_preview` left
            // the slot empty (and flashed its own error) — don't leave a blank
            // split open.
            if self.view.right_pager.is_none() {
                self.state.vsplit = None;
                return;
            }
            let label = if pane_open { "top-only" } else { "full-height" };
            self.state
                .flash_info(format!("vsplit: {label} (^a | to cycle)"));
            self.view.needs_full_repaint = true;
            return;
        }

        // Already open. A *different* previewable file under the cursor means
        // "show this one instead" — swap the content, keep the shape.
        let preview_path = self
            .view
            .right_pager
            .as_ref()
            .and_then(|v| v.source_path.clone());
        if let Some(path) = cursor_file
            && Some(&path) != preview_path.as_ref()
        {
            self.load_right_preview(&path);
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            self.state.flash_info(format!("preview: {name}"));
            self.view.needs_full_repaint = true;
            return;
        }

        // Same file (or cursor not previewable): cycle the shape.
        self.state.vsplit = next_vsplit(self.state.vsplit);
        if let Some(v) = self.state.vsplit {
            let mode = match v.mode {
                state::VsplitMode::TopOnly => "top-only",
                state::VsplitMode::FullHeight => "full-height",
            };
            self.state
                .flash_info(format!("vsplit: {mode} (^a | to cycle)"));
        } else {
            // Closed: drop the preview. Keyboard focus stays where it is.
            self.view.right_pager = None;
            self.state.flash_info("vsplit: off");
        }
        self.view.needs_full_repaint = true;
    }

    /// `^s n` — open a second file-commander in the right column. `b` is a
    /// second view into the SAME project (it shares PROJECT_HOME and the rest of
    /// the global state), so it opens at **PROJECT_HOME** rather than prompting
    /// for a directory — navigate it from there. Falls back to the focused
    /// column's current dir when no PROJECT_HOME is set. Re-targets if `b` is
    /// already open.
    pub(super) fn open_second_commander(&mut self) {
        let dir = self
            .state
            .project_home
            .clone()
            .unwrap_or_else(|| self.state.cur().listing.dir.clone());
        self.open_second_commander_at(&dir);
    }

    /// Open (or re-target) the second file-commander rooted at `dir` and focus
    /// it. The right region hosts a real commander (`state.right`) — mutually
    /// exclusive with the `^a |` preview, so any open preview is dropped.
    /// Always **top-only**: full-height would clamp the bottom pane to the left
    /// column (pane under `a` only), which makes no sense for two peer browsers
    /// sharing one pane — the pane stays full-width below both columns and `b`
    /// occupies the top-right region. Reached from `^s n`.
    pub(super) fn open_second_commander_at(&mut self, dir: &std::path::Path) {
        // Canonicalize so a relative / `..`-laden path resolves cleanly (and so
        // `cur().listing.dir` matches what later path comparisons expect).
        let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let commander = match state::Commander::for_dir(&dir, &self.state.config) {
            Ok(c) => c,
            Err(e) => {
                self.state.flash_error(format!("open: {e}"));
                return;
            }
        };
        self.view.right_pager = None; // a commander and the preview are exclusive
        self.state.right = Some(commander);
        self.state.vsplit = Some(state::VSplit {
            width_pct: DEFAULT_VSPLIT_PCT,
            mode: state::VsplitMode::TopOnly,
            focus: state::Side::Right,
        });
        self.state.focus = state::Focus::FileList;
        // Build the right column's rows — `cur()` now resolves to it.
        self.state.rebuild_rows();
        self.state
            .flash_info("second commander (^s x / ^d to close)");
        self.view.needs_full_repaint = true;
    }

    /// `^s x` — close the second commander: drop `state.right` and the split,
    /// returning to a single (left) column with focus on it.
    pub(super) fn close_second_commander(&mut self) {
        if self.state.right.is_none() {
            self.state.flash_info("no second commander");
            return;
        }
        self.state.right = None;
        self.state.vsplit = None;
        // Drop any `V`/`D` overlay open in `b` along with its column — otherwise
        // the editor PTY / pager would linger with no column to render into.
        self.runtime.top_overlay_right = None;
        self.view.pager_right = None;
        self.state.focus = state::Focus::FileList;
        self.view.needs_full_repaint = true;
        self.state.flash_info("second commander closed");
    }

    /// Close the vertical split: drop the preview and clear the shape. Focus
    /// stays on the file list (now the sole column). This is the `q`/Esc close
    /// path when the right preview is focused.
    pub(super) fn close_vsplit(&mut self) {
        self.state.vsplit = None;
        self.view.right_pager = None;
        self.view.needs_full_repaint = true;
        self.state.flash_info("vsplit: off");
    }

    /// `^a a`/`^a h` (left, `a`) and `^a b`/`^a l` (right, `b`) — focus a
    /// file-pane column. No-op with no split (one column), and never wraps.
    ///
    /// From the **bottom pane** these are inert *except* `^a l` in full-height
    /// mode: there the right column sits beside the pane, so `^a l` jumps
    /// straight into it (leaving the pane). In top-only the pane is full-width
    /// below both columns, so h/l stay inert (use `^a k` to leave the pane).
    pub(super) fn vsplit_focus(&mut self, side: state::Side) {
        let Some(mode) = self.state.vsplit.map(|v| v.mode) else {
            return; // no split: nothing to switch
        };
        // A `V`/`D` overlay/pager stays pinned to its own column (`overlay_column`),
        // so switching focus to the other column is fine — it keeps running there.
        if self.state.pane_focused()
            && !(mode == state::VsplitMode::FullHeight && side == state::Side::Right)
        {
            return;
        }
        // Going right: remember the left side's vertical position (top list vs
        // bottom pane) so `^a h` returns there instead of always the top.
        if side == state::Side::Right {
            self.state.pane.vsplit_left_was_pane = self.state.pane_focused();
        }
        if let Some(v) = self.state.vsplit.as_mut() {
            v.focus = side;
        }
        // Right column owns input via the (file-pane) row; left restores the
        // remembered vertical position (the bottom pane if it left from there).
        self.state.focus = if side == state::Side::Left
            && self.state.pane.vsplit_left_was_pane
            && self.runtime.pane_tabs.is_some()
        {
            state::Focus::Pane
        } else {
            state::Focus::FileList
        };
        self.state.flash_info(match side {
            state::Side::Left => "focus: a (left)",
            state::Side::Right => "focus: b (right)",
        });
        self.view.needs_full_repaint = true;
    }

    /// Approximate width of the right preview column — the markdown wrap
    /// target. Mirrors `carve_vsplit`'s `right_w` (terminal width × width_pct),
    /// so the preview wraps to the column it lands in. (`build_pager_view_for_file`
    /// subtracts the gutter/padding from this.) Falls back to ~half on no split.
    pub(super) fn right_preview_body_width(&self) -> u16 {
        let (term_w, _) = crossterm::terminal::size().unwrap_or((80, 24));
        let pct = self
            .state
            .vsplit
            .map_or(DEFAULT_VSPLIT_PCT, |v| v.width_pct);
        // Derive from the same helper `carve_vsplit` uses, so the wrap width
        // matches the actual column. Too-narrow falls back to the full width.
        vsplit_column_widths(term_w, pct).map_or(term_w, |(_, right_w)| right_w)
    }

    /// True when the right (`b`) column of an open vertical split owns the
    /// keyboard — the file-pane row is focused and `b` is the active column.
    /// Routing sends its non-meta keys to `view.right_pager`.
    pub(super) fn right_column_focused(&self) -> bool {
        matches!(self.state.focus, state::Focus::FileList)
            && self
                .state
                .vsplit
                .is_some_and(|v| v.focus == state::Side::Right)
    }

    /// The vsplit column input is currently directed at, ignoring the surface
    /// type (list / overlay / pager). `Left` with no split (the single column)
    /// or when the left column is focused; `Right` when `b` is. Independent of
    /// the pane-vs-not axis — use [`Self::column_focused`] when that matters.
    /// Drives per-column slot selection (which `top_overlay*` / pager a key or a
    /// spawn targets).
    pub(super) fn focused_side(&self) -> state::Side {
        self.state.vsplit.map_or(state::Side::Left, |v| v.focus)
    }

    /// Does `side`'s column currently own the keyboard? True only when the
    /// bottom pane is NOT focused and `side` is the active column (treating the
    /// no-split single column as `Left`). Drives the bright/dim render of each
    /// column's overlay/list.
    pub(super) fn column_focused(&self, side: state::Side) -> bool {
        !self.state.pane_focused() && self.focused_side() == side
    }

    /// True when a column-scoped `V`/`D` opened from the focused commander
    /// should target the RIGHT column's slots: `b` exists and is the focused
    /// column. (The left/single/no-split case targets the existing slots.)
    pub(super) fn overlay_targets_right(&self) -> bool {
        self.state.right.is_some() && self.focused_side() == state::Side::Right
    }

    /// Context-sensitive `^a +`/`^a -`: resize the vertical split's width when
    /// a column is focused, else the bottom pane's height (`resize_pane`).
    pub(super) fn resize_focused_split(&mut self, delta_pct: i32) {
        if self.state.vsplit.is_some() && !self.state.pane_focused() {
            if let Some(v) = self.state.vsplit.as_mut() {
                v.width_pct = (i32::from(v.width_pct) + delta_pct).clamp(20, 80) as u16;
                self.view.needs_full_repaint = true;
            }
        } else {
            self.resize_pane(delta_pct);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use state::{Side, VsplitMode};

    #[test]
    fn cycle_off_to_top_to_full_to_off() {
        let top = next_vsplit(None);
        assert_eq!(top.map(|v| v.mode), Some(VsplitMode::TopOnly));
        let full = next_vsplit(top);
        assert_eq!(full.map(|v| v.mode), Some(VsplitMode::FullHeight));
        assert_eq!(next_vsplit(full), None, "full-height cycles back to off");
    }

    #[test]
    fn open_defaults_to_left_focus_and_clamped_width() {
        let v = next_vsplit(None).unwrap();
        assert_eq!(v.focus, Side::Left, "opening keeps focus on the list (a)");
        assert!((20..=80).contains(&v.width_pct));
    }

    #[test]
    fn column_widths_clamp_and_refuse_narrow() {
        // Even-ish split (1 column for the divider) on a wide region.
        let (l, r) = vsplit_column_widths(200, 50).unwrap();
        assert_eq!(l + r + 1, 200, "left + divider + right == width");
        assert!((i32::from(l) - i32::from(r)).abs() <= 1, "≈ even split");
        // Out-of-range pct clamps to [20, 80] (same result as the boundary).
        assert_eq!(
            vsplit_column_widths(200, 5),
            vsplit_column_widths(200, 20),
            "pct 5 clamps to 20"
        );
        assert_eq!(
            vsplit_column_widths(200, 95),
            vsplit_column_widths(200, 80),
            "pct 95 clamps to 80"
        );
        // Too narrow for two usable columns → no split.
        assert!(vsplit_column_widths(30, 50).is_none());
        // Each column floors at MIN_COL.
        let (l_min, r_min) = vsplit_column_widths(MIN_COL * 2 + 1, 50).unwrap();
        assert!(l_min >= MIN_COL && r_min >= MIN_COL);
    }

    #[test]
    fn flip_carries_width_and_focus() {
        let mut top = next_vsplit(None).unwrap();
        top.width_pct = 60;
        top.focus = Side::Right;
        let full = next_vsplit(Some(top)).unwrap();
        assert_eq!(full.mode, VsplitMode::FullHeight);
        assert_eq!(full.width_pct, 60, "width carries across the mode flip");
        assert_eq!(
            full.focus,
            Side::Right,
            "focus carries across the mode flip"
        );
    }
}
