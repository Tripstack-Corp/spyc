//! Rendering: frame-layout computation and the top-level `render` pass —
//! status bar, file list, divider, pane status line, prompt, and the
//! harpoon menu overlay.
//!
//! Extracted from `app/mod.rs` (REFACTOR_PLAN Phase 2). These are
//! `impl App` methods living in a child module, so they read App's
//! private state directly via the descendant-module rule — no field is
//! made `pub`. Only the two entry points the run loop calls (`render`
//! and the `compute_layout` associated fn) are `pub`;
//! `render_pane_status_line` / `render_harpoon_menu` are render-internal
//! and stay private. `FrameLayout` stays in `app` because callers on
//! both sides of the split construct and read it.

use ratatui::Frame;

use crate::config::StatusPosition;
use crate::pane::PaneWidget;
use crate::spyc_debug;
use crate::ui::list_view::ListView;
use crate::ui::pager;
use crate::ui::prompt::PromptLine;
use crate::ui::status::StatusBar;

use super::{
    App, FlashKind, FrameLayout, Mode, TaskStatus, View, format_uptime, path_basename_display,
    place_pty_cursor_from_screen,
};

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
        }
    }

    /// Pane status line: tab indicators, active cwd, [SCROLL] tag.
    /// Replaces the old plain-rule divider.
    fn render_pane_status_line(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        use ratatui::{
            style::{Modifier, Style},
            text::{Line, Span},
            widgets::Paragraph,
        };
        let width = area.width as usize;
        // Tinting the rule + active tab in scroll mode is deliberate
        // redundancy with the [SCROLL] tag — three signals in different
        // parts of the divider make "you've left live view" hard to miss.
        let is_scrolling = self
            .pane_tabs
            .as_ref()
            .is_some_and(|t| t.active().is_scrolling());
        // Scroll mode flips the rule + active-tab color to blue
        // (theme.dir) so "you're in scrollback" is unambiguous from
        // peripheral vision. Amber stays the "focus" color for live
        // mode; blue is reserved for scrollback and unused elsewhere
        // as a UI signal. The `[SCROLL]` tag below uses the same
        // color, plus the active tab's label stays uppercased (shape
        // cue independent of color).
        let rule_style = if is_scrolling {
            Style::default()
                .fg(self.theme.dir)
                .add_modifier(Modifier::BOLD)
        } else if self.state.pane_focused() {
            Style::default()
                .fg(self.theme.prompt_prefix)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.theme.status_suffix)
        };
        // Active tab gets REVERSED modifier (background fill) so it's
        // unambiguously distinct from a background tab with activity
        // — both used to render as plain amber-bold and were only
        // differentiated by the small `*`/`+` glyph, which Spencer
        // (and probably others) read past in peripheral vision. With
        // reverse, "you are here" registers before glyph parsing.
        let active_tab_style = if is_scrolling {
            Style::default()
                .fg(self.theme.dir)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default()
                .fg(self.theme.prompt_prefix)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        };
        let inactive_tab_style = Style::default().fg(self.theme.status_suffix);

        let mut spans: Vec<Span> = Vec::new();
        let mut used = 0usize;

        let activity_style = Style::default()
            .fg(self.theme.pick)
            .add_modifier(Modifier::BOLD);

        // Tab indicators: ─[1*] claude ─[2+] bash, then "── <live cwd>".
        // We render the indicators first (immutable iter) and capture
        // the active index, then re-borrow mut to fetch the live cwd.
        let mut active_idx: Option<usize> = None;
        if let Some(tabs) = &self.pane_tabs {
            for (i, entry) in tabs.tabs().iter().enumerate() {
                let is_active = i == tabs.active_index();
                if is_active {
                    active_idx = Some(i);
                }
                let star = if is_active { "*" } else { "" };
                let activity = if entry.info.has_activity { "+" } else { "" };
                let sep = "─";
                // Uppercase the active tab label in scroll mode — the
                // shape change is a peripheral-vision cue even before
                // the color registers.
                let label = if is_active && is_scrolling {
                    entry.info.label.to_uppercase()
                } else {
                    entry.info.label.clone()
                };
                let tab_text = format!("[{}{star}{activity}] {label} ", i + 1);
                let tab_len = sep.len() + tab_text.len();
                if used + tab_len > width {
                    break;
                }
                spans.push(Span::styled(sep, rule_style));
                let style = if is_active {
                    active_tab_style
                } else if entry.info.has_activity {
                    activity_style
                } else {
                    inactive_tab_style
                };
                spans.push(Span::styled(tab_text, style));
                used += tab_len;
            }
        }

        if let (Some(idx), Some(tabs)) = (active_idx, self.pane_tabs.as_mut()) {
            let entry = &mut tabs.tabs_mut()[idx];
            let live = entry.live_cwd().to_path_buf();
            let cwd_display = crate::paths::display_tilde(&live);
            // Mark when the live cwd has drifted from the spawn cwd
            // (e.g. user `cd`'d in a bash tab). Helps spot the case
            // the bug list called out.
            let drift = live != entry.info.cwd;
            let cwd_prefix = if drift { "── ↪ " } else { "── " };
            let avail = width.saturating_sub(used + 12); // room for [SCROLL] + trailing rule
            if avail > 4 {
                let truncated = if cwd_display.len() > avail {
                    format!("…{}", &cwd_display[cwd_display.len() - avail + 1..])
                } else {
                    cwd_display
                };
                let cwd_fragment = format!("{cwd_prefix}{truncated} ");
                used += cwd_fragment.len();
                let style = if drift {
                    active_tab_style
                } else {
                    inactive_tab_style
                };
                spans.push(Span::styled(cwd_fragment, style));
            }
        }

        // Right-aligned background-task tags. Distinct color from pane
        // tabs so the numbering doesn't visually collide (pane tabs are
        // 1..N left-to-right; bg tasks are 1..N right-anchored). Keeps
        // the rendered group ordered ascending L→R, but if there isn't
        // room for all of them we drop the *oldest* first (keep newest
        // visible). Glyphs:
        //   `[N+]`  running, output arrived since last :fg
        //   `[N\u{25cf}]`  running, quiescent
        //   `[N\u{2713}]`  exited cleanly
        //   `[N\u{2717}]`  non-zero exit / killed / crashed
        let bg_running_color = self.theme.dir; // soft blue
        let bg_unread_color = self.theme.take; // teal -- pulls the eye
        let bg_ok_color = self.theme.exec; // soft green
        let bg_err_color = ratatui::style::Color::Rgb(0xf7, 0x76, 0x8e); // tokyo red
        let mut bg_pieces_rev: Vec<(String, ratatui::style::Color)> = Vec::new();
        let mut bg_width = 0usize;
        let zoom_tag = if self.state.pane_zoomed {
            " [ZOOM]"
        } else {
            ""
        };
        let scroll_tag = if is_scrolling { " [SCROLL]" } else { "" };
        let tag_len = zoom_tag.len() + scroll_tag.len();
        // Reserve room for at least 4 dashes + the tag(s).
        let bg_budget = width.saturating_sub(used + tag_len + 4);
        for task in self.background_tasks.tasks.iter().rev() {
            let (glyph, color) = if task.paused && matches!(task.status, TaskStatus::Running) {
                // Pause glyph trumps the running/unread variants:
                // user explicitly paused, that's the headline state.
                ("\u{23f8}", bg_running_color) // ⏸
            } else {
                match (&task.status, task.has_unread_output) {
                    (TaskStatus::Running, true) => ("+", bg_unread_color),
                    (TaskStatus::Running, false) => ("\u{25cf}", bg_running_color),
                    (TaskStatus::Exited(0), _) => ("\u{2713}", bg_ok_color),
                    (TaskStatus::Exited(_) | TaskStatus::Killed | TaskStatus::Crashed(_), _) => {
                        ("\u{2717}", bg_err_color)
                    }
                }
            };
            let text = format!(" [{}{glyph}]", task.id);
            if bg_width + text.len() > bg_budget {
                break;
            }
            bg_width += text.len();
            bg_pieces_rev.push((text, color));
        }

        // Dash fill between pane-tab area and bg group / mode tag(s).
        let fill = width.saturating_sub(used + tag_len + bg_width);
        if fill > 0 {
            spans.push(Span::styled("─".repeat(fill), rule_style));
            used += fill;
        }

        // Render bg tasks left-to-right (id-ascending) by reversing the
        // collection we built right-to-left.
        for (text, color) in bg_pieces_rev.into_iter().rev() {
            used += text.len();
            spans.push(Span::styled(
                text,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }

        if self.state.pane_zoomed {
            spans.push(Span::styled(
                zoom_tag,
                Style::default()
                    .fg(self.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ));
            used += zoom_tag.len();
        }
        if is_scrolling {
            spans.push(Span::styled(
                scroll_tag,
                Style::default()
                    .fg(self.theme.dir)
                    .add_modifier(Modifier::BOLD),
            ));
            used += scroll_tag.len();
        }
        // If anything's left (shouldn't be), pad.
        let _ = used;

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    /// Render the harpoon menu overlay. Centered modal box listing
    /// the active project's slots, with the menu cursor on a
    /// highlighted row. Footer shows the bindings.
    fn render_harpoon_menu(&self, frame: &mut Frame) {
        use ratatui::{
            layout::Rect,
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::{Block, Borders, Clear, Paragraph},
        };
        let Some(menu) = self.harpoon_menu.as_ref() else {
            return;
        };
        let Some(h) = self.state.harpoon.as_ref() else {
            return;
        };

        let area = frame.area();
        // Box dims: width clamped, height = 2 chrome + N slots + 2 footer.
        let width = area.width.clamp(40, 72);
        let body_h = (h.slots.len().max(1)) as u16;
        let height = (2 + body_h + 2).min(area.height); // borders + body + footer
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let rect = Rect {
            x,
            y,
            width,
            height,
        };
        frame.render_widget(Clear, rect);

        let title = format!(
            " harpoon — {} ",
            h.project.file_name().map_or_else(
                || h.project.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(self.theme.prompt_prefix));
        let inner = block.inner(rect);
        frame.render_widget(block, rect);

        let footer_h = 1u16;
        let body_rect = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(footer_h),
        };
        let footer_rect = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(footer_h),
            width: inner.width,
            height: footer_h,
        };

        // Body lines.
        let mut body_lines: Vec<Line> = Vec::with_capacity(h.slots.len().max(1));
        if h.slots.is_empty() {
            body_lines.push(Line::from(Span::styled(
                "  (empty — Ha to harpoon the cursor file/dir)",
                Style::default().fg(self.theme.status_suffix),
            )));
        } else {
            let cursor_style = Style::default()
                .fg(Color::Black)
                .bg(self.theme.prompt_prefix)
                .add_modifier(Modifier::BOLD);
            let normal_style = Style::default().fg(self.theme.status_path);
            let key_style = Style::default()
                .fg(self.theme.pick)
                .add_modifier(Modifier::BOLD);
            for (i, path) in h.slots.iter().enumerate() {
                let on_cursor = i == menu.cursor;
                let armed = on_cursor && menu.delete_armed;
                let prefix = if armed { " ⚠ " } else { "   " };
                // Display path relative to project_home when possible
                // (shorter, more readable); otherwise use the absolute.
                let shown = path
                    .strip_prefix(&h.project)
                    .map_or_else(|_| path.display().to_string(), |p| p.display().to_string());
                let line = Line::from(vec![
                    Span::styled(prefix, normal_style),
                    Span::styled(format!("{}  ", i + 1), key_style),
                    Span::styled(
                        shown,
                        if on_cursor {
                            cursor_style
                        } else {
                            normal_style
                        },
                    ),
                ]);
                body_lines.push(line);
            }
        }
        frame.render_widget(Paragraph::new(body_lines), body_rect);

        let footer_style = Style::default()
            .fg(self.theme.status_suffix)
            .add_modifier(Modifier::DIM);
        let footer_text = if menu.delete_armed {
            "   d again = delete · any other key cancels"
        } else {
            "   j/k move · 1-9/Enter jump · K/J reorder · dd delete · q/Esc close"
        };
        frame.render_widget(
            Paragraph::new(Span::styled(footer_text, footer_style)),
            footer_rect,
        );
    }

    /// Draw a full frame. Thin wrapper so the activity (`A`) monitor renders
    /// LAST and unconditionally — visible over the `$EDITOR` / `;cmd` overlay
    /// / top-pager paths too, which `return` early from `render_inner`
    /// (BUGS.md: "`A` monitoring should be omnipresent").
    pub fn render(&mut self, frame: &mut Frame) {
        self.render_inner(frame);
        let frame_area = frame.area();
        self.render_activity_hud(frame, frame_area);
    }

    fn render_inner(&mut self, frame: &mut Frame) {
        let frame_area = frame.area();

        // Layout:
        //   - No pane: status (top row), list (middle), prompt (bottom row).
        //   - With pane: status (top row of the top *pane*), list (rest of
        //     top pane), divider row, pane, prompt (bottom row).
        //   The status row is always at the top of the file-list region —
        //   so when the pane is open it sits *inside* the top pane rather
        //   than above the divider.
        let layout = Self::compute_layout(
            frame_area,
            // `pane_hidden` makes the toggle act like "no pane" for
            // layout purposes — file list reclaims the full middle
            // region. The pty stays alive in `pane_tabs`; just no
            // rect for it this frame.
            self.pane_tabs.is_some() && !self.state.pane_hidden,
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );

        // If a top-overlay pty is active (`;top`, `;vim`, etc.), it
        // replaces the entire spyc area. Status, list, and prompt are
        // hidden; only the overlay + divider + bottom pane render.
        if let Some(overlay) = self.top_overlay.as_mut() {
            // The overlay occupies status + list + prompt area.
            let overlay_area = ratatui::layout::Rect {
                x: layout.status.x,
                y: layout.status.y,
                width: layout.status.width,
                height: layout.status.height + layout.list.height + layout.prompt.height,
            };
            let _ = overlay.resize(overlay_area.height, overlay_area.width);
            overlay.drain_output();
            if overlay.is_closed() && !self.overlay_awaiting_dismiss {
                self.overlay_awaiting_dismiss = true;
            }
            // Visual focus tracks `state.pane_focused`: false ⇒
            // overlay focused (cursor block, full color); true ⇒
            // bottom pane focused (overlay dims to half-lightness via
            // PaneWidget's DIM modifier). User toggles with ^a-j/k.
            let overlay_focused = !self.state.pane_focused();
            let want_overlay_cursor = overlay_focused && !self.overlay_awaiting_dismiss;
            overlay.with_screen(|screen| {
                frame.render_widget(
                    PaneWidget {
                        screen,
                        focused: overlay_focused,
                    },
                    overlay_area,
                );
                if want_overlay_cursor {
                    place_pty_cursor_from_screen(frame, screen, overlay_area);
                }
            });
            // Show a dismiss prompt when the subprocess has exited.
            if self.overlay_awaiting_dismiss && overlay_area.height > 0 {
                use ratatui::{
                    style::{Modifier, Style},
                    text::{Line, Span},
                    widgets::Paragraph,
                };
                let prompt_y = overlay_area.y + overlay_area.height.saturating_sub(1);
                let prompt_rect = ratatui::layout::Rect {
                    x: overlay_area.x,
                    y: prompt_y,
                    width: overlay_area.width,
                    height: 1,
                };
                let style = Style::default()
                    .fg(self.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD);
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "[process exited — press any key to continue]",
                        style,
                    ))),
                    prompt_rect,
                );
            }

            // Divider + bottom pane still render normally.
            if let Some(divider_rect) = layout.divider {
                self.render_pane_status_line(frame, divider_rect);
            }
            let bottom_pane_rect: Option<ratatui::layout::Rect> =
                if let (Some(tabs), Some(rect)) = (self.pane_tabs.as_mut(), layout.pane) {
                    let _ = tabs.active_mut().resize(rect.height, rect.width);
                    tabs.drain_all();
                    let focused = self.state.pane_focused();
                    // Single lock window: render the pane AND place
                    // the OS cursor under the same screen snapshot,
                    // so a worker-thread parse landing between the
                    // two can't produce a cursor that's ahead of the
                    // rendered grid (off-by-one tearing in claude
                    // backspace was the symptom).
                    let want_cursor = focused && !self.overlay_awaiting_dismiss;
                    tabs.active().with_screen(|screen| {
                        frame.render_widget(PaneWidget { screen, focused }, rect);
                        if want_cursor {
                            place_pty_cursor_from_screen(frame, screen, rect);
                        }
                    });
                    Some(rect)
                } else {
                    None
                };
            // Cursor placement is now folded into the overlay and
            // bottom-pane with_screen blocks above, so the rendered
            // grid and the cursor share a single lock acquisition.
            // (Pre-v1.50.84 they were two separate calls; the worker
            // thread could parse a chunk between them, leaving the
            // cursor ahead of the rendered grid — visible as
            // off-by-one tearing during fast input.)
            let _ = bottom_pane_rect;
            return;
        }

        // v1.5 Phase 5: a `TopPane`-mounted pager (today: only `D`
        // opening a file in-pager) replaces the status + list +
        // prompt area, just like the `;cmd` top overlay does, so the
        // bottom pane (claude / zsh) keeps running visibly below.
        // The pager's own title bar provides the visual identity;
        // status / prompt rows aren't drawn behind it.
        // While the pager-help overlay is open, the underlying
        // pager (stashed for restore on dismiss) still owns its
        // slot — peek into the stash so the user doesn't see the
        // pane "jump" back to live-pty / file-list rendering for
        // the lifetime of the help overlay.
        let in_help = self
            .pager
            .as_ref()
            .is_some_and(|v| v.title == crate::ui::pager::PAGER_HELP_TITLE);
        let top_pager = if in_help {
            self.pager_help_stash.as_ref()
        } else {
            self.pager.as_ref()
        }
        .is_some_and(|v| matches!(v.mount, crate::ui::pager::Mount::TopPane));
        if top_pager {
            let top_area = ratatui::layout::Rect {
                x: layout.status.x,
                y: layout.status.y,
                width: layout.status.width,
                height: layout.status.height + layout.list.height + layout.prompt.height,
            };
            let underlying = if in_help {
                self.pager_help_stash.as_ref()
            } else {
                self.pager.as_ref()
            };
            if let Some(view) = underlying {
                crate::ui::pager::render(frame, top_area, view, &self.theme);
            }
            // Divider + bottom pane render normally below.
            if let Some(divider_rect) = layout.divider {
                self.render_pane_status_line(frame, divider_rect);
            }
            if let (Some(tabs), Some(rect)) = (self.pane_tabs.as_mut(), layout.pane) {
                let _ = tabs.active_mut().resize(rect.height, rect.width);
                tabs.drain_all();
                let focused = self.state.pane_focused();
                tabs.active().with_screen(|screen| {
                    frame.render_widget(PaneWidget { screen, focused }, rect);
                    if focused {
                        place_pty_cursor_from_screen(frame, screen, rect);
                    }
                });
                tabs.active_mut().output_dirty = false;
            }
            // The TopPane branch returns early — if the pager-help
            // overlay is up over a TopPane pager, render it here on
            // top of the just-drawn slot before returning. The
            // standard branch's centered-overlay tail (further down)
            // never runs in this path.
            if in_help && let Some(help) = self.pager.as_ref() {
                crate::ui::pager::render(frame, frame.area(), help, &self.theme);
            }
            return;
        }

        let (path, suffix) = self.header_parts();
        let project_label = self
            .state
            .project_home
            .as_deref()
            .map(path_basename_display);
        let agent_info = self.active_agent_status();
        StatusBar {
            project_home: project_label.as_deref(),
            session_name: self.state.session_name.as_deref(),
            path: &path,
            suffix: &suffix,
            git_info: self.state.git.info.as_deref(),
            agent_info: agent_info.as_deref(),
            theme: &self.theme,
        }
        .render(frame, layout.status);

        if self.cached_rows_gen != self.state.list_generation {
            self.cached_rows = self.build_rows();
            self.cached_rows_gen = self.state.list_generation;
        }
        let rows = &self.cached_rows;
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
        if grid_key != self.cached_grid_key {
            self.cached_grid_key = grid_key;
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
                        theme: &self.theme,
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
                            theme: &self.theme,
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
            self.cached_grid_key = (
                self.state.list_generation,
                self.state.cursor.view_top,
                self.state.cursor.index,
                layout.list.width,
                layout.list.height,
            );
        } // end grid cache guard

        frame.render_widget(
            ListView {
                rows,
                cursor: self.state.cursor.index,
                view_top: self.state.cursor.view_top,
                empty_marker: self.state.view == View::Dir,
                focused: list_focused,
                theme: &self.theme,
            },
            layout.list,
        );

        // v1.5 Phase 3: a `LowerPane`-mounted pager (today: only
        // pane scrollback view, opened via `^a-v`) replaces the
        // pty widget in the bottom slot — the pty keeps running
        // off-screen but the user is reading a frozen snapshot
        // through the pager. The standard centered-overlay pager
        // path further down is skipped for `LowerPane`-mounted
        // views (rect dispatch happens here instead).
        // Same `underlying` logic as the TopPane branch above:
        // while the help overlay is open, peek into the stash so
        // the LowerPane scrollback view stays drawn underneath
        // instead of flickering back to the live pty.
        let bottom_is_pager = if in_help {
            self.pager_help_stash.as_ref()
        } else {
            self.pager.as_ref()
        }
        .is_some_and(|v| matches!(v.mount, crate::ui::pager::Mount::LowerPane));
        let bottom_pane_rect: Option<ratatui::layout::Rect> =
            if let (Some(tabs), Some(rect)) = (self.pane_tabs.as_mut(), layout.pane) {
                let _ = tabs.active_mut().resize(rect.height, rect.width);
                tabs.drain_all();
                if bottom_is_pager {
                    // Skip the pty widget — the pager owns this rect now.
                    // First-frame snap: ^a-v opens the pager wanting to
                    // see *recent* output. We can't compute the right
                    // scroll value at construction time (no viewport
                    // height yet), so the opener sets
                    // `pending_scroll_to_bottom` and the renderer here
                    // — which now knows the actual rect — does the
                    // snap before drawing, so the user never sees a
                    // jump frame. Skipped while the help overlay is
                    // up: the stash's `pending_scroll_to_bottom` was
                    // already cleared on the original first frame.
                    if !in_help
                        && let Some(view) = self.pager.as_mut()
                        && view.pending_scroll_to_bottom.get()
                    {
                        view.scroll_to_bottom(rect.height);
                        view.pending_scroll_to_bottom.set(false);
                    }
                    let underlying = if in_help {
                        self.pager_help_stash.as_ref()
                    } else {
                        self.pager.as_ref()
                    };
                    if let Some(view) = underlying {
                        crate::ui::pager::render(frame, rect, view, &self.theme);
                    }
                } else {
                    let focused = self.state.pane_focused();
                    // Fold cursor placement into the same lock
                    // acquisition as the pane render — otherwise
                    // the worker thread can advance the screen
                    // between the two and we paint the grid from
                    // one frame and the cursor from the next
                    // (visible as off-by-one tearing during fast
                    // input).
                    tabs.active().with_screen(|screen| {
                        frame.render_widget(PaneWidget { screen, focused }, rect);
                        if focused {
                            place_pty_cursor_from_screen(frame, screen, rect);
                        }
                    });
                }
                tabs.active_mut().output_dirty = false;
                // Quick Select labels paint *over* the pane widget so
                // the user keeps the live output as context. Render
                // here, after the pane, before the divider.
                if self.quick_select.is_some() && !bottom_is_pager {
                    self.render_quick_select_overlay(frame, rect);
                }
                Some(rect)
            } else {
                None
            };
        // Cursor placement for the bottom-pane branch is folded into
        // the `with_screen` block above (single lock window for grid
        // + cursor). `bottom_pane_rect` is still computed so other
        // branches that need the geometry can read it.
        let _ = bottom_pane_rect;

        if let Some(divider_rect) = layout.divider {
            self.render_pane_status_line(frame, divider_rect);
        }

        if let Mode::Prompting(p) = &self.state.mode {
            PromptLine {
                prefix: &p.prefix,
                buffer: &p.buffer,
                theme: &self.theme,
                cursor_pos: p.editor.as_ref().map(|e| e.cursor),
                vi_mode: p.editor.as_ref().map(|e| e.mode),
            }
            .render(frame, layout.prompt);
        } else if let Some(flash) = &self.state.flash {
            use ratatui::{
                style::{Modifier, Style},
                text::{Line, Span},
                widgets::Paragraph,
            };
            let color = match flash.kind {
                FlashKind::Info => self.theme.take,
                FlashKind::Error => self.theme.cursor_bg,
            };
            let line = Line::from(Span::styled(
                flash.text.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            frame.render_widget(Paragraph::new(line), layout.prompt);
        } else if let Some(pending) = self.state.resolver.pending_display() {
            use ratatui::{
                style::{Modifier, Style},
                text::{Line, Span},
                widgets::Paragraph,
            };
            let line = Line::from(Span::styled(
                pending,
                Style::default()
                    .fg(self.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ));
            frame.render_widget(Paragraph::new(line), layout.prompt);
        }

        // Pager comes after list but before help (help always wins).
        // `LowerPane` and `TopPane` mounts already rendered into
        // their slots above; only `Overlay` mount hits this centered
        // render path.
        if let Some(view) = &self.pager
            && matches!(view.mount, crate::ui::pager::Mount::Overlay)
        {
            pager::render(frame, frame.area(), view, &self.theme);
        }

        // Harpoon menu overlay — modal, drawn on top of everything
        // except the activity monitor.
        if self.harpoon_menu.is_some() {
            self.render_harpoon_menu(frame);
        }
    }

    /// Render the activity (`A`) monitor overlay (top-right corner). Called
    /// LAST from `render` so it sits over every render path — including the
    /// `$EDITOR` / `;cmd` overlay and top-pager paths that return early from
    /// `render_inner` (the "omnipresent" ask). Rows are padded to one common
    /// display width so the block is a clean flush-right rectangle with
    /// content right-justified, instead of the old ragged per-line staircase:
    /// throughput + frame timing (yellow), internals (teal), process stats
    /// (lavender), and a build + terminal-caps footer (blue). No-op unless the
    /// monitor is toggled on.
    fn render_activity_hud(&self, frame: &mut Frame, frame_area: ratatui::layout::Rect) {
        if !self.show_activity {
            return;
        }
        use ratatui::style::{Color, Style};
        use ratatui::text::{Line as HudLine, Span};
        use ratatui::widgets::Paragraph as ActivityP;

        // Line 1 — throughput + frame timing. `pk` is the whole terminal.draw
        // (build + diff + tty emission); `r` is just the render closure (CPU).
        // pk-r ≈ diff+emission; pk near the inter-keystroke interval ⇒ render-bound.
        let l1 = format!(
            " {} dps [p:{} e:{} o:{}]  {} cells/s  pk {:.1}ms r{:.1}ms echo {:.1}ms ",
            self.activity_dps,
            self.activity_snap_pane,
            self.activity_snap_event,
            self.activity_snap_other,
            self.activity_bps,
            self.activity_frame_peak_snap as f64 / 1000.0,
            self.activity_render_peak_snap as f64 / 1000.0,
            // Peak keystroke→echo round-trip (forward → agent echo → render).
            // `echo - r` ≈ the agent/pty round-trip (Claude re-rendering its
            // input box) we don't control; a small `echo` ⇒ spyc isn't the lag.
            self.activity_echo_snap as f64 / 1000.0,
        );

        // Line 2 — internals digest.
        let bg_running = self.background_tasks.running_count();
        let bg_done = self.background_tasks.done_count();
        let bg_paused = self
            .background_tasks
            .tasks
            .iter()
            .filter(|t| t.paused)
            .count();
        let pager_state = match self.pager.as_ref() {
            None => "none",
            Some(v) => match v.mount {
                crate::ui::pager::Mount::Overlay => "overlay",
                crate::ui::pager::Mount::TopPane => "top",
                crate::ui::pager::Mount::LowerPane => "lower",
            },
        };
        let git_last = if self.activity_git_last_ms == 0 {
            "—".to_string()
        } else {
            format!("{}ms", self.activity_git_last_ms)
        };
        let l2 = format!(
            " bg:{bg_running}\u{25cf}{bg_done}\u{2713}{}  git:{}/s last:{}  fs:{}/s  mcp:{}/s  list:{}  pager:{} ",
            if bg_paused > 0 {
                format!(" {bg_paused}\u{23f8}")
            } else {
                String::new()
            },
            self.activity_git_results_snap,
            git_last,
            self.activity_watcher_events_snap,
            self.activity_mcp_reqs_snap,
            self.state.listing.entries.len(),
            pager_state,
        );

        // Line 3 — process stats (PID for `sample`/lldb, RSS, threads).
        let pid = std::process::id();
        let uptime_str = format_uptime(self.started_at.elapsed().as_secs());
        let pane_count = self.pane_tabs.as_ref().map_or(0, |t| t.tabs().len());
        let rss_mb = self.activity_proc_rss_kb / 1024;
        let l3 = format!(
            " pid:{pid}  up:{uptime_str}  rss:{rss_mb}m  thr:{}  panes:{pane_count} ",
            self.activity_proc_threads,
        );

        // Line 4 — build identity + terminal capabilities.
        let term = std::env::var("TERM").unwrap_or_else(|_| "?".to_string());
        let truecolor = std::env::var("COLORTERM")
            .is_ok_and(|c| c.contains("truecolor") || c.contains("24bit"));
        let l4 = format!(
            " spyc v{} ({})  {term}{}  {}\u{00d7}{} ",
            env!("CARGO_PKG_VERSION"),
            env!("SPYC_GIT_SHA"),
            if truecolor { " truecolor" } else { "" },
            frame_area.width,
            frame_area.height,
        );

        // Pad every row to one common display width → a clean flush-right
        // block (straight left edge), content right-justified.
        let rows: [(&str, Color); 4] = [
            (l1.as_str(), Color::Yellow),
            (l2.as_str(), self.theme.take),
            (l3.as_str(), self.theme.status_user),
            (l4.as_str(), self.theme.dir),
        ];
        let maxw = rows
            .iter()
            .map(|(s, _)| crate::ui::display_width(s))
            .max()
            .unwrap_or(0);
        let block_w = u16::try_from(maxw).unwrap_or(u16::MAX);
        // Need the block plus a 1-col right margin.
        if block_w == 0 || frame_area.width <= block_w + 1 {
            return;
        }
        let x = frame_area.width - block_w - 1;
        for (row, (text, bg)) in rows.iter().enumerate() {
            let Ok(y) = u16::try_from(row) else { break };
            if y >= frame_area.height {
                break;
            }
            let pad = maxw.saturating_sub(crate::ui::display_width(text));
            let padded = format!("{}{text}", " ".repeat(pad));
            let rect = ratatui::layout::Rect {
                x,
                y,
                width: block_w,
                height: 1,
            };
            let style = Style::default().fg(Color::Black).bg(*bg);
            frame.render_widget(
                ActivityP::new(HudLine::from(Span::styled(padded, style))),
                rect,
            );
        }
    }
}
