//! Rendering: frame-layout computation and the top-level `render` pass.
//!
//! This is the `render` module root. It owns the frame lifecycle — layout
//! (`compute_layout`), the `render` entry point, and the pre-draw settle
//! (`prepare_frame` / `prepare_panes` / `settle_list_grid`) — and delegates
//! the painting itself to child modules:
//!   - `inner`    — the main draw pass (`render_inner`)
//!   - `chrome`   — pane status-line, status-bar header, list-rows cache
//!   - `overlays` — harpoon menu + activity (`A`) monitor
//!
//! Extracted from `app/mod.rs` (REFACTOR_PLAN Phase 2). These are `impl App`
//! methods living in child modules, so they read App's private state directly
//! via the descendant-module rule — no field is made `pub`. Only the two
//! entry points the run loop calls (`render` and the `compute_layout`
//! associated fn) are `pub`; the render-internal helpers are `pub(super)`
//! (visible across the `render` module group, nowhere else). `FrameLayout`
//! stays in `app` because callers on both sides of the split construct and
//! read it.

use ratatui::Frame;

use crate::config::StatusPosition;
use crate::spyc_debug;
use crate::ui::list_view::ListView;

use super::{App, FrameLayout, View, state};

mod chrome;
mod inner;
mod overlays;

impl App {
    /// Partition the frame into status/list/prompt rects — plus, when
    /// the pane is open, a divider row and the pane rect below it.
    ///
    /// The **entire spyc unit** (status, list, prompt) lives above the
    /// divider. That way the prompt row sits with the file list it's
    /// about rather than attached to the bottom of the screen where the
    /// pane's subprocess is typing.
    pub fn compute_layout(
        area: ratatui::layout::Rect,
        pane_open: bool,
        pane_pct: u16,
        status_position: StatusPosition,
    ) -> FrameLayout {
        use ratatui::layout::Rect;
        let w = area.width;
        let h = area.height;
        let bottom_status = matches!(status_position, StatusPosition::Bottom);

        if !pane_open {
            // Top:    [status][list…][prompt]
            // Bottom: [list…][prompt][status]   (vim-style)
            let (status_y, list_y, prompt_y) = if bottom_status {
                (
                    area.y + h.saturating_sub(1),
                    area.y,
                    area.y + h.saturating_sub(2),
                )
            } else {
                (area.y, area.y + 1.min(h), area.y + h.saturating_sub(1))
            };
            let status = Rect {
                x: area.x,
                y: status_y,
                width: w,
                height: 1.min(h),
            };
            let list = Rect {
                x: area.x,
                y: list_y,
                width: w,
                height: h.saturating_sub(2),
            };
            let prompt = Rect {
                x: area.x,
                y: prompt_y,
                width: w,
                height: u16::from(h != 0),
            };
            return FrameLayout {
                status,
                list,
                divider: None,
                pane: None,
                prompt,
                // No pane: the spyc unit is the whole frame, regardless of
                // where the status row sits within it.
                top_unit: Rect {
                    x: area.x,
                    y: area.y,
                    width: w,
                    height: h,
                },
                right: None,
                vdivider: None,
            };
        }

        // With pane: top unit holds list+prompt(+status if top).
        // Pane and divider sit below; if status is bottom, status is the
        // very last row, prompt one above, pane above that.
        let usable = h.saturating_sub(1); // minus divider
        let pane_h = (u32::from(usable) * u32::from(pane_pct) / 100) as u16;
        let top_h = usable.saturating_sub(pane_h);

        if bottom_status {
            // Layout (top → bottom): [list…][divider][pane…][prompt][status]
            // Reserve: 1 divider + 1 prompt + 1 status = 3 rows of chrome.
            // The remainder splits between list and pane by `pane_pct`.
            let chrome = 3u16;
            let usable_b = h.saturating_sub(chrome);
            let pane_h_b = (u32::from(usable_b) * u32::from(pane_pct) / 100) as u16;
            let list_h = usable_b.saturating_sub(pane_h_b);

            let list = Rect {
                x: area.x,
                y: area.y,
                width: w,
                height: list_h,
            };
            let divider = Rect {
                x: area.x,
                y: area.y + list_h,
                width: w,
                height: 1,
            };
            // No pane rect when it has no height — `TopList` zoom (pct 0)
            // leaves only the tab-bar divider; the pty runs off-screen.
            let pane = (pane_h_b > 0).then_some(Rect {
                x: area.x,
                y: divider.y + 1,
                width: w,
                height: pane_h_b,
            });
            let prompt = Rect {
                x: area.x,
                y: area.y + h.saturating_sub(2),
                width: w,
                height: u16::from(h >= 2),
            };
            let status = Rect {
                x: area.x,
                y: area.y + h.saturating_sub(1),
                width: w,
                height: 1.min(h),
            };
            return FrameLayout {
                status,
                list,
                divider: Some(divider),
                pane,
                prompt,
                // Bottom status + pane: only the list sits above the divider;
                // prompt and status are below the pane. A top overlay occupies
                // just the list region.
                top_unit: Rect {
                    x: area.x,
                    y: area.y,
                    width: w,
                    height: list_h,
                },
                right: None,
                vdivider: None,
            };
        }

        // BottomPane zoom (`pane_pct >= 100` — the only value `effective_pane_pct`
        // ever returns ≥ 100, and `pane_height_pct` is clamped to ≤ 90): the
        // pane fills everything below a single spyc status line. That one row
        // carries the status bar normally and flips to flash / chord-arming /
        // prompt when active (they share the row — `render_inner` draws one or
        // the other), so a zoomed session still surfaces arming + messages
        // while leaving the pane's own input line uncluttered at the bottom.
        // (The bottom-status path reserves its chrome even at pct 100, so only
        // this top-status path collapsed it — hence the targeted branch.)
        if pane_pct >= 100 {
            let status = Rect {
                x: area.x,
                y: area.y,
                width: w,
                height: 1.min(h),
            };
            let divider = Rect {
                x: area.x,
                y: area.y + status.height,
                width: w,
                height: 1.min(h.saturating_sub(status.height)),
            };
            let pane = Rect {
                x: area.x,
                y: divider.y + divider.height,
                width: w,
                height: h.saturating_sub(status.height + divider.height),
            };
            return FrameLayout {
                status,
                // No file list while the pane is zoomed.
                list: Rect {
                    x: area.x,
                    y: pane.y,
                    width: w,
                    height: 0,
                },
                divider: Some(divider),
                pane: Some(pane),
                // The flash / arming / prompt line shares the single top row.
                prompt: status,
                top_unit: status,
                right: None,
                vdivider: None,
            };
        }

        // Top status (default): [status][list…][prompt][divider][pane]
        let status = Rect {
            x: area.x,
            y: area.y,
            width: w,
            height: 1.min(top_h),
        };
        let list_h = top_h.saturating_sub(2);
        let list = Rect {
            x: area.x,
            y: area.y + status.height,
            width: w,
            height: list_h,
        };
        let prompt = Rect {
            x: area.x,
            y: area.y + top_h.saturating_sub(1),
            width: w,
            height: u16::from(top_h >= 2),
        };

        let divider = Rect {
            x: area.x,
            y: area.y + top_h,
            width: w,
            height: 1,
        };
        // No pane rect when it has no height — `TopList` zoom (pct 0) leaves
        // only the bottom tab-bar divider visible; the pty runs off-screen.
        let pane = (pane_h > 0).then_some(Rect {
            x: area.x,
            y: divider.y + 1,
            width: w,
            height: pane_h,
        });

        FrameLayout {
            status,
            list,
            divider: Some(divider),
            pane,
            prompt,
            // Top status + pane: status+list+prompt are contiguous above the
            // divider (== top_h rows starting at area.y).
            top_unit: Rect {
                x: area.x,
                y: area.y,
                width: w,
                height: top_h,
            },
            right: None,
            vdivider: None,
        }
    }

    /// Carve a single-column [`FrameLayout`] into a left/right vertical split.
    /// Pure geometry (no `self`, no IO) — unit-tested without a TUI, the
    /// `route.rs`/`focus.rs` template. Returns the layout **unchanged** when
    /// the frame is too narrow to host two usable columns (single-column
    /// fallback for that frame — never builds a 0/1-col rect). `TopOnly`
    /// splits only the list region (the PTY pane stays full-width below);
    /// `FullHeight` runs the divider the full frame height and clamps the
    /// left-column chrome — including the PTY pane — to the left width.
    pub fn carve_vsplit(
        layout: FrameLayout,
        vsplit: state::VSplit,
        area: ratatui::layout::Rect,
    ) -> FrameLayout {
        use ratatui::layout::Rect;
        // Column widths come from the shared, clamped helper (same source as
        // the markdown wrap widths, so they can't drift). `None` = too narrow
        // for two usable columns → stay single-column this frame.
        let w = area.width;
        let Some((left_w, right_w)) = super::vsplit::vsplit_column_widths(w, vsplit.width_pct)
        else {
            return layout;
        };
        let vdiv_x = area.x + left_w; // 1 column for the vertical divider
        let right_x = vdiv_x + 1;
        let mut out = layout;
        match vsplit.mode {
            state::VsplitMode::TopOnly => {
                // Split the list region into left | divider | right. The right
                // column also claims the prompt row — the preview doesn't need
                // the spyc arming/flash line (that's the left list's) — so it
                // runs one row taller, with the divider all the way down to the
                // horizontal pane divider. Only when the prompt sits in the top
                // region (top-status / no pane); in bottom-status the prompt is
                // below the pane, so the right column is just the list region.
                let list = out.list;
                out.list = Rect {
                    width: left_w,
                    ..list
                };
                // Scope the overlay/TopPane region (`top_unit`) to the left
                // column too, so a `V` editor / `;cmd` / `D` pager occupies the
                // left column and the right preview stays visible beside it.
                out.top_unit.width = out.top_unit.width.min(left_w);
                let pane_div_y = out.divider.map(|d| d.y);
                let prompt_in_top = pane_div_y.is_none_or(|dy| out.prompt.y < dy);
                let bottom = if prompt_in_top {
                    pane_div_y.unwrap_or(out.prompt.y + out.prompt.height)
                } else {
                    list.y + list.height
                };
                if prompt_in_top {
                    // Keep the arming/flash line under the left list only.
                    out.prompt.width = out.prompt.width.min(left_w);
                }
                let height = bottom.saturating_sub(list.y);
                out.vdivider = Some(Rect {
                    x: vdiv_x,
                    y: list.y,
                    width: 1,
                    height,
                });
                out.right = Some(Rect {
                    x: right_x,
                    y: list.y,
                    width: right_w,
                    height,
                });
            }
            state::VsplitMode::FullHeight => {
                // Divider runs the full frame height; clamp every left-column
                // rect (they all start at `area.x`) to `left_w`, including the
                // PTY pane — that's what confines the pane under the left
                // column. The right column spans the whole frame height.
                out.status.width = out.status.width.min(left_w);
                out.list.width = out.list.width.min(left_w);
                out.prompt.width = out.prompt.width.min(left_w);
                out.top_unit.width = out.top_unit.width.min(left_w);
                if let Some(p) = out.pane.as_mut() {
                    p.width = p.width.min(left_w);
                }
                if let Some(d) = out.divider.as_mut() {
                    d.width = d.width.min(left_w);
                }
                out.vdivider = Some(Rect {
                    x: vdiv_x,
                    y: area.y,
                    width: 1,
                    height: area.height,
                });
                out.right = Some(Rect {
                    x: right_x,
                    y: area.y,
                    width: right_w,
                    height: area.height,
                });
            }
        }
        out
    }

    /// Draw a full frame. Thin wrapper so the activity (`A`) monitor renders
    /// LAST and unconditionally — visible over the `$EDITOR` / `;cmd` overlay
    /// / top-pager paths too, which `return` early from `render_inner`
    /// (BUGS.md: "`A` monitoring should be omnipresent").
    pub fn render(&mut self, frame: &mut Frame) {
        // MVU Stage 2: settle the frame's derived state (layout + the list
        // rows/grid) BEFORE drawing, so the draw path itself performs no
        // domain/view transitions.
        let layout = self.prepare_frame(frame.area());
        self.render_inner(frame, layout);
        let frame_area = frame.area();
        self.render_activity_hud(frame, frame_area);
        // Full-screen mermaid image overlay (the `i` key) — drawn last so it
        // sits on top of everything, including the HUD.
        self.render_mermaid_overlay(frame, frame_area);
    }

    /// Pre-draw pass: compute the frame layout and settle the derived list
    /// state (rows cache + the `view_top`↔grid stabilization) before any
    /// drawing, so `render_inner` draws from already-settled state. Returns
    /// the layout the draw reuses (computed once). The list settle runs only
    /// on the file-list path — when a top-overlay owns the screen the list
    /// isn't drawn and its derived state isn't consulted, matching
    /// `render_inner`'s overlay early-return.
    fn prepare_frame(&mut self, area: ratatui::layout::Rect) -> FrameLayout {
        // Layout:
        //   - No pane: status (top row), list (middle), prompt (bottom row).
        //   - With pane: status (top row of the top *pane*), list (rest of
        //     top pane), divider row, pane, prompt (bottom row).
        // `pane_hidden` makes the toggle act like "no pane" for layout
        // purposes — the file list reclaims the full middle region; the pty
        // stays alive in `pane_tabs`, just no rect for it this frame.
        let layout = Self::compute_layout(
            area,
            self.runtime.pane_tabs.is_some() && !self.state.pane.pane_hidden,
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        // Carve the single-column layout into a left/right split when one is
        // open AND no zoom is active — a `^a z` zoom fills one region full-screen
        // and takes precedence over the split (otherwise the zoomed pane would
        // be clamped to the left column). The split state is kept, so un-zoom
        // restores it.
        let mut layout = match self.state.vsplit {
            Some(vsplit) if self.state.pane.zoom == state::ZoomTarget::None => {
                Self::carve_vsplit(layout, vsplit, area)
            }
            _ => layout,
        };
        // Grow the prompt upward to fit a long, wrapped command line (runs after
        // the carve so it uses the final, possibly column-scoped, prompt width).
        self.grow_prompt_for_wrap(&mut layout, area);
        if self.runtime.top_overlay.is_none() {
            self.settle_list_grid(&layout);
        }
        self.prepare_panes(&layout);
        layout
    }

    /// While a prompt is open, expand `layout.prompt` upward so a command line
    /// too wide for one row **wraps** across multiple rows instead of being
    /// truncated at the edge. The extra rows are drawn over the bottom of the
    /// list/pane — transient, only while typing — so no other rect is resized.
    /// Capped at half the frame and never grows over the top status/header row.
    /// The wrapped height comes from the same `PromptLine` the draw renders, so
    /// the reserved rows match the drawn rows exactly.
    fn grow_prompt_for_wrap(&self, layout: &mut FrameLayout, area: ratatui::layout::Rect) {
        let crate::app::Mode::Prompting(p) = &self.state.mode else {
            return;
        };
        if layout.prompt.width == 0 || layout.prompt.height == 0 {
            return;
        }
        let pl = crate::ui::prompt::PromptLine {
            prefix: &p.prefix,
            buffer: &p.buffer,
            theme: &self.view.theme,
            cursor_pos: p.editor.as_ref().map(|e| e.cursor),
            vi_mode: p.editor.as_ref().map(|e| e.mode),
        };
        let cap = (area.height / 2).max(1);
        let lines = pl.line_count(layout.prompt.width).min(cap);
        if lines <= layout.prompt.height {
            return;
        }
        let bottom = layout.prompt.y + layout.prompt.height;
        let min_y = area.y.saturating_add(1); // keep the top row (status/header)
        let new_y = bottom.saturating_sub(lines).max(min_y);
        layout.prompt.y = new_y;
        layout.prompt.height = bottom.saturating_sub(new_y);
    }

    /// Settle the Runtime-owned pane/overlay state for this frame BEFORE the
    /// draw: resize the panes/overlay to their laid-out rects, drain pending
    /// output, clear the per-pane `output_dirty`, flag overlay dismissal, and
    /// run the LowerPane pager's first-frame scroll snap. Relocated out of
    /// `render_inner` (MVU Stage 2) so the draw path performs no mutations.
    ///
    /// Behavior-equivalent to the old in-draw calls: `prepare_frame` runs
    /// immediately before `render_inner` with nothing between them, so the
    /// resize → drain → (clear/snap) → draw ordering each branch relied on is
    /// preserved. The conditions mirror `render_inner`'s branches — the bottom
    /// pane resize+drain is uniform across all of them (always to
    /// `layout.pane`); `output_dirty` is cleared only on the non-overlay draw
    /// paths (the overlay path leaves it set); the snap is the LowerPane,
    /// not-in-help, pending-only case. Uses `drain_all` only — never
    /// `clear_wake`, which stays owned by the pre-recv loop scan (the
    /// lost-wakeup CAS hazard); `drain_output`/`drain_all` are generation-gated
    /// so the second drain per frame is a no-op when nothing changed.
    fn prepare_panes(&mut self, layout: &FrameLayout) {
        let overlay_active = self.runtime.top_overlay.is_some();

        // Top overlay (`;cmd`/`V`/`D`): resize to the spyc area, drain, and
        // flag dismissal once the child exits.
        if let Some(overlay) = self.runtime.top_overlay.as_mut() {
            // Resize to the region the overlay actually paints (`top_unit`),
            // so the pty's row count matches what render_inner draws under
            // both status positions.
            let h = layout.top_unit.height;
            let w = layout.top_unit.width;
            let _ = overlay.resize(h, w);
            overlay.drain_output();
            if overlay.is_closed() && !self.view.overlay_awaiting_dismiss {
                self.view.overlay_awaiting_dismiss = true;
            }
        }

        // Bottom pane: resize to its rect + drain (uniform across the overlay,
        // TopPane-pager, and default draw paths). `output_dirty` is cleared
        // only on the non-overlay paths, matching `render_inner`.
        if let (Some(tabs), Some(rect)) = (self.runtime.pane_tabs.as_mut(), layout.pane) {
            let _ = tabs.active_mut().resize(rect.height, rect.width);
            tabs.drain_all();
            if !overlay_active {
                tabs.active_mut().output_dirty = false;
            }
            // Pick up any landed live-cwd + kick a stale refresh HERE (the
            // &mut settle point), so the active pane's status line can read
            // the cache purely in the draw — `cwd_for_pid` is an `lsof`
            // fork-exec that must never run on the render thread. Only the
            // active tab's cwd is shown (render_pane_status_line), so only it
            // is refreshed.
            tabs.active_tab_mut().refresh_live_cwd();
        }

        // Bottom-scrollback first-frame snap: the opener can't know the viewport
        // height, so it sets `pending_scroll_to_bottom` and the snap happens
        // here, before the draw — so the user never sees a jump frame.
        // `TranscriptStream::drain` re-arms the flag when its lines arrive
        // off-thread. Keyed only on the scrollback's presence + its rect, NOT on
        // which top region is up: `render_bottom_region` draws the scrollback
        // identically under the file list, a `D` TopPane pager, and a
        // `;cmd`/`$EDITOR` overlay, so the snap must fire in all three. (Gating
        // this on `!overlay_active` is what left the scrollback parked at the
        // top under an open editor — the draw and the snap have to agree on the
        // one fact "the bottom region is the scrollback", which is exactly
        // `scroll_pager.is_some()`.)
        if let Some(rect) = layout.pane
            && let Some(view) = self.view.scroll_pager.as_mut()
            && view.pending_scroll_to_bottom.get()
        {
            view.scroll_to_bottom(rect.height);
            view.pending_scroll_to_bottom.set(false);
        }
    }

    /// Rebuild the list-rows cache and run the `view_top`↔grid stabilization.
    /// Pure derived state: reads the listing/cursor + the list rect, writes
    /// `cached_rows`, `grid_dims`, and `cursor.view_top`. No drawing, no IO —
    /// extracted from the draw path (MVU Stage 2) so `render` stays
    /// mutation-free.
    fn settle_list_grid(&mut self, layout: &FrameLayout) {
        if self.view.cached_rows_gen != self.state.left.list_generation {
            self.view.cached_rows = self.build_rows();
            self.view.cached_rows_gen = self.state.left.list_generation;
        }
        let focused = !self.state.pane_focused();
        // Disjoint field borrows: cached_rows/theme (read) + cached_grid_key
        // (write) on `view`, and `state.left` (write) — all distinct fields, so
        // this passes the borrow checker without an accessor that locks all of
        // `self`. PR C settles the right column the same way against its own
        // commander + caches.
        stabilize_grid(
            &self.view.cached_rows,
            &mut self.state.left,
            layout.list,
            focused,
            &self.view.theme,
            &mut self.view.cached_grid_key,
        );

        // The right column's second commander (`state.right`) settles the same
        // way against its own row cache + grid key + rect. No-op until `^z`
        // opens one.
        if self.state.right.is_some() {
            let right_focused = self.right_column_focused();
            let right_gen = self
                .state
                .right
                .as_ref()
                .map_or(u64::MAX, |r| r.list_generation);
            if self.view.right_cached_rows_gen != right_gen
                && let Some(right) = self.state.right.as_ref()
            {
                self.view.right_cached_rows = self.build_rows_for(right);
                self.view.right_cached_rows_gen = right_gen;
            }
            if let (Some(rect), Some(right)) = (layout.right, self.state.right.as_mut()) {
                stabilize_grid(
                    &self.view.right_cached_rows,
                    right,
                    rect,
                    right_focused,
                    &self.view.theme,
                    &mut self.view.right_cached_grid_key,
                );
            }
        }
    }
}

/// The `view_top` ↔ grid stabilization for one commander column. The grid
/// depends on `view_top` (entry name lengths → column count → items-per-page)
/// and `view_top` depends on the grid, so this iterates to a fixpoint (≤4
/// rounds), breaking a 2-cycle by picking the lower `view_top` (more context,
/// deterministic across frames). Skips the loop when nothing changed since the
/// last frame (the `cached_grid_key` guard). Pure derived state — mutates only
/// the commander's `grid_dims`/`cursor.view_top` and the caller's cache key, no
/// drawing or IO. Free fn (not an `App` method) so the borrows on the caller's
/// `view`/`state` fields stay disjoint; called once per visible column.
fn stabilize_grid(
    rows: &[crate::ui::list_view::Row],
    commander: &mut state::Commander,
    rect: ratatui::layout::Rect,
    focused: bool,
    theme: &crate::ui::theme::Theme,
    cached_grid_key: &mut (u64, usize, usize, u16, u16),
) {
    let grid_key = (
        commander.list_generation,
        commander.cursor.view_top,
        commander.cursor.index,
        rect.width,
        rect.height,
    );
    if grid_key == *cached_grid_key {
        return;
    }
    *cached_grid_key = grid_key;
    let mut prev_vt: Option<usize> = None; // for 2-cycle detection
    let mut settled = false;
    for round in 0..4 {
        let probe = ListView {
            rows,
            cursor: commander.cursor.index,
            view_top: commander.cursor.view_top,
            empty_marker: commander.view == View::Dir,
            focused,
            theme,
        };
        commander.grid_dims = probe.grid(rect).dims();
        let old_vt = commander.cursor.view_top;
        let pp = commander.grid_dims.items_per_page();
        commander.ensure_cursor_visible();
        if commander.cursor.view_top == old_vt {
            spyc_debug!(
                "grid settled round {}: vt={} cursor={} grid={}x{} pp={}",
                round + 1,
                old_vt,
                commander.cursor.index,
                commander.grid_dims.cols,
                commander.grid_dims.rows_per_col,
                pp,
            );
            settled = true;
            break;
        }
        spyc_debug!(
            "grid unstable round {}: vt {} -> {} cursor={} grid={}x{} pp={}",
            round + 1,
            old_vt,
            commander.cursor.view_top,
            commander.cursor.index,
            commander.grid_dims.cols,
            commander.grid_dims.rows_per_col,
            pp,
        );
        // 2-cycle: new vt equals the vt from two rounds ago.
        if Some(commander.cursor.view_top) == prev_vt {
            // Always pick the lower vt — deterministic across frames.
            let forced = old_vt.min(commander.cursor.view_top);
            commander.cursor.view_top = forced;
            // Recompute grid for the forced view_top.
            let probe = ListView {
                rows,
                cursor: commander.cursor.index,
                view_top: commander.cursor.view_top,
                empty_marker: commander.view == View::Dir,
                focused,
                theme,
            };
            commander.grid_dims = probe.grid(rect).dims();
            spyc_debug!(
                "grid 2-cycle broken: forcing vt={} (cursor={} grid={}x{} pp={})",
                forced,
                commander.cursor.index,
                commander.grid_dims.cols,
                commander.grid_dims.rows_per_col,
                commander.grid_dims.items_per_page(),
            );
            settled = true;
            break;
        }
        prev_vt = Some(old_vt);
    }
    if !settled {
        spyc_debug!(
            "grid did NOT settle after 4 rounds: vt={} cursor={}",
            commander.cursor.view_top,
            commander.cursor.index,
        );
    }
    // Update cache key in case the stabilization loop changed view_top.
    *cached_grid_key = (
        commander.list_generation,
        commander.cursor.view_top,
        commander.cursor.index,
        rect.width,
        rect.height,
    );
}

#[cfg(test)]
mod render_tests {
    //! Full-frame render snapshots (ratatui `TestBackend` + `insta`).
    //!
    //! These pin the *composed* paneless frame — status bar + file list +
    //! prompt and their layout — at a fixed geometry, so the
    //! `prepare_frame` extraction (rows-cache + grid-stabilization settle
    //! moved out of the draw path) and the `&self` `render_inner` stay
    //! behavior-equivalent: a regression that shifts a glyph makes
    //! the `.snap` diff. Pane *content* is intentionally not snapshotted — it
    //! needs a live `PtyHost`; these cover the file-list surface, which is
    //! exactly what `prepare_frame` touches.
    use std::path::PathBuf;

    use super::*;
    use crate::app::Mode;
    use crate::app::prompt::{Prompt, PromptKind};
    use ratatui::{Terminal, backend::TestBackend};

    /// A paneless App with a fixed listing dir (keeps the status-bar path
    /// deterministic across machines — a real cwd would otherwise leak into
    /// the snapshot) and seeded rows. Forces the render-side rows cache to
    /// rebuild so the seeded rows actually paint (`seed_rows` sets `rows`
    /// without bumping `list_generation`, which the cache is keyed on).
    fn demo_app(names: &[&str]) -> App {
        let mut app = App::test_app(std::env::temp_dir());
        app.state.left.listing.dir = PathBuf::from("/projects/demo");
        app.seed_rows(names);
        app.view.cached_rows_gen = app.state.left.list_generation.wrapping_sub(1);
        app
    }

    /// Draw one frame into a `TestBackend` and dump the glyphs (no styling),
    /// trailing whitespace trimmed — same shape as the `ui::*` widget tests.
    fn render_to_string(app: &mut App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| app.render(f)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf.cell((x, y)).map_or(" ", |c| c.symbol()));
            }
            out.push('\n');
        }
        out.trim_end().to_string()
    }

    fn files() -> [&'static str; 6] {
        ["README.md", "Cargo.toml", "src", "tests", "docs", "BUGS.md"]
    }

    #[test]
    fn snapshot_frame_list_top_status() {
        let mut app = demo_app(&files());
        insta::assert_snapshot!(render_to_string(&mut app, 80, 24));
    }

    #[test]
    fn snapshot_frame_status_bottom() {
        let mut app = demo_app(&files());
        app.state.config.layout.status_position = StatusPosition::Bottom;
        insta::assert_snapshot!(render_to_string(&mut app, 80, 24));
    }

    #[test]
    fn snapshot_frame_list_scrolled() {
        // A list long enough to overflow the multi-column grid (spyc lays
        // files out in columns), with the cursor deep in it → forces the
        // `view_top` grid-stabilization that `prepare_frame` will own.
        let names: Vec<String> = (0..200).map(|i| format!("file-{i:03}.txt")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut app = demo_app(&refs);
        app.state.left.cursor.index = 180;
        insta::assert_snapshot!(render_to_string(&mut app, 80, 24));
    }

    #[test]
    fn snapshot_frame_prompting_command() {
        let mut app = demo_app(&files());
        app.state.mode = Mode::Prompting(Prompt::shell(PromptKind::Command, ":"));
        insta::assert_snapshot!(render_to_string(&mut app, 80, 24));
    }

    #[test]
    fn snapshot_frame_flash() {
        let mut app = demo_app(&files());
        app.state.flash_info("yanked 3 paths");
        insta::assert_snapshot!(render_to_string(&mut app, 80, 24));
    }
}

#[cfg(test)]
mod purity_guard {
    //! Mechanical guard for the CLAUDE.md contract "Render is pure (`&self`);
    //! the draw pass reads … and mutates nothing." The June-2026 deep review
    //! found three OS-in-draw violations (`agent_status.rs:80`, `tabs.rs:239`,
    //! `overlays.rs:215`) where the `&self` draw spawned threads / forked
    //! `lsof` / read env per frame — each individually plausible, collectively
    //! a documented invariant that had silently eroded. The fixes (#346–#348)
    //! moved every side effect to the `&mut` `prepare_*` settle steps or an
    //! `Effect`. This test stops the regression at write time, the same way
    //! `mod_rs_stays_decomposed` / the `COMMAND_TABLE` build-error guard turn
    //! prose rules into failures.
    //!
    //! Scope: the PURE DRAW modules only — `inner` / `chrome` / `overlays`.
    //! `render/mod.rs` is deliberately NOT covered: it holds the `&mut`
    //! settle (`prepare_frame` / `prepare_panes`), which is exactly where the
    //! OS kicks legitimately live. As further off-thread fixes land (e.g. the
    //! remaining Tier-5 `apply.rs` / `clipboard.rs` items), add each newly-pure
    //! module to `PURE_DRAW` to lock the fix in.
    //!
    //! Why a grep test and not clippy `disallowed-methods`: that config is
    //! crate-global, so it would also fire on the executor / worker bodies that
    //! SHOULD do this IO. Per-module scoping needs a source scan.

    /// `(label, source)` for each module that must stay free of OS access.
    const PURE_DRAW: &[(&str, &str)] = &[
        ("render/inner.rs", include_str!("inner.rs")),
        ("render/chrome.rs", include_str!("chrome.rs")),
        ("render/overlays.rs", include_str!("overlays.rs")),
    ];

    /// High-signal tokens for blocking IO / OS access / thread spawning that
    /// must never appear in a pure draw pass. Not exhaustive — a backstop for
    /// the realistic full-path style spyc uses, not a formal proof.
    const FORBIDDEN: &[&str] = &[
        "thread::spawn",
        "std::fs::",
        "crate::fs::",
        "read_to_string",
        "env::var",
        "Command::new",
        "process::id",
    ];

    #[test]
    fn pure_draw_modules_touch_no_os() {
        for (label, src) in PURE_DRAW {
            for pat in FORBIDDEN {
                assert!(
                    !src.contains(pat),
                    "`{label}` contains `{pat}` — the draw pass must stay pure (&self). \
                     Move the side effect to a `prepare_*` settle step (&mut) or an `Effect`; \
                     see CLAUDE.md \"Render is pure\" and the #346–#348 fixes."
                );
            }
        }
    }
}
