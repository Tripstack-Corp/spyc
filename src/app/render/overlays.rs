//! Modal/overlay draw helpers: the centered harpoon menu
//! (`render_harpoon_menu`) and the top-right activity (`A`) monitor
//! (`render_activity_hud`). Split from `app/render.rs` verbatim; an `impl App`
//! child module reading App's private state via the descendant-module rule.

use ratatui::Frame;

use crate::app::{App, format_uptime};

impl App {
    /// Render the harpoon menu overlay. Centered modal box listing
    /// the active project's slots, with the menu cursor on a
    /// highlighted row. Footer shows the bindings.
    pub(super) fn render_harpoon_menu(&self, frame: &mut Frame) {
        use ratatui::{
            layout::Rect,
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::{Block, Borders, Clear, Paragraph},
        };
        let Some(menu) = self.view.harpoon_menu.as_ref() else {
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
            .border_style(Style::default().fg(self.view.theme.prompt_prefix));
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
                Style::default().fg(self.view.theme.status_suffix),
            )));
        } else {
            let cursor_style = Style::default()
                .fg(Color::Black)
                .bg(self.view.theme.prompt_prefix)
                .add_modifier(Modifier::BOLD);
            let normal_style = Style::default().fg(self.view.theme.status_path);
            let key_style = Style::default()
                .fg(self.view.theme.pick)
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
            .fg(self.view.theme.status_suffix)
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

    /// Render the activity (`A`) monitor overlay (top-right corner). Called
    /// LAST from `render` so it sits over every render path — including the
    /// `$EDITOR` / `;cmd` overlay and top-pager paths that return early from
    /// `render_inner` (the "omnipresent" ask). Rows are padded to one common
    /// display width so the block is a clean flush-right rectangle with
    /// content right-justified, instead of the old ragged per-line staircase:
    /// throughput + frame timing (yellow), internals (teal), process stats
    /// (lavender), and a build + terminal-caps footer (blue). No-op unless the
    /// monitor is toggled on.
    pub(super) fn render_activity_hud(&self, frame: &mut Frame, frame_area: ratatui::layout::Rect) {
        if !self.view.show_activity {
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
            self.view.activity_dps,
            self.view.activity_snap_pane,
            self.view.activity_snap_event,
            self.view.activity_snap_other,
            self.view.activity_bps,
            self.view.activity_frame_peak_snap as f64 / 1000.0,
            self.view.activity_render_peak_snap as f64 / 1000.0,
            // Peak keystroke→echo round-trip (forward → agent echo → render).
            // `echo - r` ≈ the agent/pty round-trip (Claude re-rendering its
            // input box) we don't control; a small `echo` ⇒ spyc isn't the lag.
            self.view.activity_echo_snap as f64 / 1000.0,
        );

        // Line 2 — internals digest.
        let bg_running = self.runtime.background_tasks.running_count();
        let bg_done = self.runtime.background_tasks.done_count();
        let bg_paused = self
            .runtime
            .background_tasks
            .tasks
            .iter()
            .filter(|t| t.paused)
            .count();
        let pager_state = match self.view.pager.as_ref() {
            None => "none",
            Some(v) => match v.mount {
                crate::ui::pager::Mount::Overlay => "overlay",
                crate::ui::pager::Mount::TopPane => "top",
                crate::ui::pager::Mount::LowerPane => "lower",
            },
        };
        let git_last = if self.view.activity_git_last_ms == 0 {
            "—".to_string()
        } else {
            format!("{}ms", self.view.activity_git_last_ms)
        };
        let l2 = format!(
            " bg:{bg_running}\u{25cf}{bg_done}\u{2713}{}  git:{}/s last:{}  fs:{}/s  mcp:{}/s  list:{}  pager:{} ",
            if bg_paused > 0 {
                format!(" {bg_paused}\u{23f8}")
            } else {
                String::new()
            },
            self.view.activity_git_results_snap,
            git_last,
            self.view.activity_watcher_events_snap,
            self.view.activity_mcp_reqs_snap,
            self.state.listing.entries.len(),
            pager_state,
        );

        // Line 3 — process stats (PID for `sample`/lldb, RSS, threads).
        let pid = std::process::id();
        let uptime_str = format_uptime(self.view.started_at.elapsed().as_secs());
        let pane_count = self
            .runtime
            .pane_tabs
            .as_ref()
            .map_or(0, |t| t.tabs().len());
        let rss_mb = self.view.activity_proc_rss_kb / 1024;
        let l3 = format!(
            " pid:{pid}  up:{uptime_str}  rss:{rss_mb}m  thr:{}  panes:{pane_count} ",
            self.view.activity_proc_threads,
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
            (l2.as_str(), self.view.theme.take),
            (l3.as_str(), self.view.theme.status_user),
            (l4.as_str(), self.view.theme.dir),
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
