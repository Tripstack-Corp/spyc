//! Pane/overlay sizing: `^W +`/`^W -` resize, `^a z` zoom (and its
//! layout-driven pty resize + fullscreen-tab follow-up), and the two pure
//! geometry functions `PaneTabs`/overlays spawn at. Split out of
//! `pane_tabs.rs` (a cohesive block per that file's own module doc — the
//! file-size ratchet asked for a split once #99 step 1's accessor rewrite
//! pushed it over 1,000 lines).

use super::{App, StatusPosition, state};

impl App {
    /// ^W + / ^W - — change the bottom pane's share of the middle rect
    /// in 5% steps, clamped to [10%, 90%].
    pub fn resize_pane(&mut self, delta_pct: i32) {
        if self.runtime.project().pane_tabs.is_none() {
            return;
        }
        if self.state.active_project().pane.zoom != state::ZoomTarget::None {
            self.state.flash_info("pane is zoomed (^a z to exit)");
            return;
        }
        let current = i32::from(self.state.active_project().pane.pane_height_pct);
        let new = (current + delta_pct).clamp(10, 90);
        self.state.active_project_mut().pane.pane_height_pct = new as u16;
        // Force a full clear+redraw: the divider moves, and dirty-frame
        // rendering otherwise leaves a stale row at the old boundary.
        self.view.needs_full_repaint = true;
    }

    /// The pane percentage to use for layout/sizing computations. Zoom drives
    /// it to a sentinel the stored `pane_height_pct` can never reach (it's
    /// clamped to `[10, 90]`): `BottomPane` → 100 (list collapses to 0 rows),
    /// `TopList` → 0 (the pane collapses to a single bottom tab-bar row and
    /// the list fills the rest — `compute_layout` drops the pane rect to
    /// `None` at 0). The stored split is left untouched, so un-zoom restores
    /// it exactly.
    pub fn effective_pane_pct(&self) -> u16 {
        match self.state.active_project().pane.zoom {
            state::ZoomTarget::BottomPane => 100,
            // Both collapse the pane and fill the body: `TopList` renders the
            // list there, `RightColumn` renders the preview (see `render_inner`).
            state::ZoomTarget::TopList | state::ZoomTarget::RightColumn => 0,
            state::ZoomTarget::None => self.state.active_project().pane.pane_height_pct,
        }
    }

    /// ^a z / ^w z — toggle "zoom" on the *active* region. Zoom-on
    /// enlarges whichever region currently has focus: the bottom pane
    /// (`BottomPane` — list collapses to 0 rows) or the file list
    /// (`TopList` — the pane collapses off-screen, the list fills the
    /// frame). Toggling again restores the prior split. Focus is left
    /// where it is — it already names the zoomed region, so no save/restore
    /// is needed (this is the fix for the old "zoom always grabbed the
    /// bottom pane" bug). The stored `pane_height_pct` is preserved across
    /// the round-trip. No-op (with a flash) when the pane is closed.
    pub fn toggle_pane_zoom(&mut self) {
        // Need *something* zoomable: a pane or a vertical split.
        if self.runtime.project().pane_tabs.is_none()
            && self.state.active_project().vsplit.is_none()
        {
            self.state.flash_info("nothing to zoom");
            return;
        }
        if self.state.active_project().pane.zoom != state::ZoomTarget::None {
            self.state.active_project_mut().pane.zoom = state::ZoomTarget::None;
            self.state.flash_info("zoom: off");
        } else if self.right_column_focused() {
            // Right split column focused → fullscreen the preview.
            self.state.active_project_mut().pane.zoom = state::ZoomTarget::RightColumn;
            self.state.flash_info("zoom: b (^a z to exit)");
        } else if self.state.pane_focused() {
            self.state.active_project_mut().pane.zoom = state::ZoomTarget::BottomPane;
            self.state.flash_info("zoom: pane (^a z to exit)");
        } else {
            // File list (or the left split column) focused.
            self.state.active_project_mut().pane.zoom = state::ZoomTarget::TopList;
            self.state.flash_info("zoom: list (^a z to exit)");
        }
        // Resize every pane pty to the new layout so child shells re-render at
        // the right dimensions; otherwise Claude's UI is the wrong size until
        // the next terminal resize.
        self.resize_panes_to_layout();
        self.view.needs_full_repaint = true;
    }

    /// Resize every pane pty to the current layout's pane rect, so child
    /// shells re-render at the right size after a zoom / fullscreen change.
    /// No-op when the layout has no pane rect — `TopList` zoom collapses the
    /// pane to a tab bar (`compute_layout` yields `pane: None` at pct 0), so
    /// the pty keeps its prior size, ready the instant the pane is revealed.
    pub(super) fn resize_panes_to_layout(&mut self) {
        let (cols, rows) = self.view.term_size;
        let area = ratatui::layout::Rect::new(0, 0, cols, rows);
        let pane_open = self.runtime.project().pane_tabs.is_some()
            && !self.state.active_project().pane.pane_hidden;
        let layout = Self::compute_layout(
            area,
            pane_open,
            self.effective_pane_pct(),
            self.state.active_project().config.layout.status_position,
        );
        if let (Some(pane_rect), Some(tabs)) =
            (layout.pane, self.runtime.project_mut().pane_tabs.as_mut())
        {
            for entry in tabs.tabs_mut() {
                let _ = entry.pane.resize(pane_rect.height, pane_rect.width);
            }
        }
    }

    /// After a tab switch from a fullscreen list (`TopList` zoom), fullscreen
    /// the newly-active tab (flip to `BottomPane` zoom) — so `^a n`/`^a p`/`^a #`
    /// navigate between fullscreen views instead of flipping a hidden pane.
    /// No-op when not list-zoomed.
    pub(super) fn fullscreen_tab_if_list_zoomed(&mut self) {
        if self.state.active_project().pane.zoom == state::ZoomTarget::TopList {
            self.state.active_project_mut().pane.zoom = state::ZoomTarget::BottomPane;
            self.resize_panes_to_layout();
        }
    }

    /// Compute the (rows, cols) the bottom pane will occupy.
    pub fn pane_spawn_size(height_pct: u16, status_position: StatusPosition) -> (u16, u16) {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let area = ratatui::layout::Rect::new(0, 0, cols, rows);
        let layout = Self::compute_layout(area, true, height_pct, status_position);
        match layout.pane {
            Some(r) => (r.height.max(1), r.width.max(1)),
            None => (rows.saturating_sub(3).max(1), cols.max(1)),
        }
    }

    /// Compute the (rows, cols) available for the top-overlay pty. This
    /// is the top area: everything above the divider (or the whole
    /// screen minus the prompt row if no bottom pane).
    pub fn top_overlay_size(pane_height_pct: u16, has_bottom_pane: bool) -> (u16, u16) {
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
}
