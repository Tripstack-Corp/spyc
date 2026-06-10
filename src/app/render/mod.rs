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

use super::{App, FrameLayout, View};

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
            let pane = Rect {
                x: area.x,
                y: divider.y + 1,
                width: w,
                height: pane_h_b,
            };
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
                pane: Some(pane),
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
        let pane = Rect {
            x: area.x,
            y: divider.y + 1,
            width: w,
            height: pane_h,
        };

        FrameLayout {
            status,
            list,
            divider: Some(divider),
            pane: Some(pane),
            prompt,
            // Top status + pane: status+list+prompt are contiguous above the
            // divider (== top_h rows starting at area.y).
            top_unit: Rect {
                x: area.x,
                y: area.y,
                width: w,
                height: top_h,
            },
        }
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
        if self.runtime.top_overlay.is_none() {
            self.settle_list_grid(&layout);
        }
        self.prepare_panes(&layout);
        layout
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
        if self.view.cached_rows_gen != self.state.list_generation {
            self.view.cached_rows = self.build_rows();
            self.view.cached_rows_gen = self.state.list_generation;
        }
        let rows = &self.view.cached_rows;
        let list_focused = !self.state.pane_focused();
        // Stabilize view_top ↔ grid.  Skip the expensive multi-round
        // loop when inputs haven't changed since the last frame.
        let grid_key = (
            self.state.list_generation,
            self.state.cursor.view_top,
            self.state.cursor.index,
            layout.list.width,
            layout.list.height,
        );
        if grid_key != self.view.cached_grid_key {
            self.view.cached_grid_key = grid_key;
            // The grid depends on view_top (different entries have different
            // name lengths → different column count → different items_per_page),
            // and view_top depends on the grid.
            //
            // This can produce a 2-cycle: vt=A gives grid that wants vt=B, and
            // vt=B gives grid that wants vt=A.  When we detect that, always pick
            // the lower of the two (shows more context, deterministic across
            // frames) and recompute the grid for that choice.
            {
                let mut prev_vt: Option<usize> = None; // for 2-cycle detection
                let mut settled = false;
                for round in 0..4 {
                    let probe = ListView {
                        rows,
                        cursor: self.state.cursor.index,
                        view_top: self.state.cursor.view_top,
                        empty_marker: self.state.view == View::Dir,
                        focused: list_focused,
                        theme: &self.view.theme,
                    };
                    self.state.grid_dims = probe.grid(layout.list).dims();
                    let old_vt = self.state.cursor.view_top;
                    let pp = self.state.grid_dims.items_per_page();
                    self.state.ensure_cursor_visible();
                    if self.state.cursor.view_top == old_vt {
                        spyc_debug!(
                            "grid settled round {}: vt={} cursor={} grid={}x{} pp={}",
                            round + 1,
                            old_vt,
                            self.state.cursor.index,
                            self.state.grid_dims.cols,
                            self.state.grid_dims.rows_per_col,
                            pp,
                        );
                        settled = true;
                        break;
                    }
                    spyc_debug!(
                        "grid unstable round {}: vt {} -> {} cursor={} grid={}x{} pp={}",
                        round + 1,
                        old_vt,
                        self.state.cursor.view_top,
                        self.state.cursor.index,
                        self.state.grid_dims.cols,
                        self.state.grid_dims.rows_per_col,
                        pp,
                    );
                    // 2-cycle: new vt equals the vt from two rounds ago.
                    if Some(self.state.cursor.view_top) == prev_vt {
                        // Always pick the lower vt — deterministic across frames.
                        let forced = old_vt.min(self.state.cursor.view_top);
                        self.state.cursor.view_top = forced;
                        // Recompute grid for the forced view_top.
                        let probe = ListView {
                            rows,
                            cursor: self.state.cursor.index,
                            view_top: self.state.cursor.view_top,
                            empty_marker: self.state.view == View::Dir,
                            focused: list_focused,
                            theme: &self.view.theme,
                        };
                        self.state.grid_dims = probe.grid(layout.list).dims();
                        spyc_debug!(
                            "grid 2-cycle broken: forcing vt={} (cursor={} grid={}x{} pp={})",
                            forced,
                            self.state.cursor.index,
                            self.state.grid_dims.cols,
                            self.state.grid_dims.rows_per_col,
                            self.state.grid_dims.items_per_page(),
                        );
                        settled = true;
                        break;
                    }
                    prev_vt = Some(old_vt);
                }
                if !settled {
                    spyc_debug!(
                        "grid did NOT settle after 4 rounds: vt={} cursor={}",
                        self.state.cursor.view_top,
                        self.state.cursor.index,
                    );
                }
            }
            // Update cache key in case the stabilization loop changed view_top.
            self.view.cached_grid_key = (
                self.state.list_generation,
                self.state.cursor.view_top,
                self.state.cursor.index,
                layout.list.width,
                layout.list.height,
            );
        } // end grid cache guard
    }
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
        app.state.listing.dir = PathBuf::from("/projects/demo");
        app.seed_rows(names);
        app.view.cached_rows_gen = app.state.list_generation.wrapping_sub(1);
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
        app.state.cursor.index = 180;
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
