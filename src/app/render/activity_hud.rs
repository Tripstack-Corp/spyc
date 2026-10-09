//! The top-right activity (`A`) monitor overlay (`render_activity_hud`). Split
//! from `overlays.rs` verbatim; an `impl App` child module reading App's
//! private state via the descendant-module rule.

use ratatui::Frame;

use crate::app::activity::HudStyle;
use crate::app::{App, format_uptime};

impl App {
    /// Render the activity (`A`) monitor overlay (top-right corner). Called
    /// LAST from `render` so it sits over every render path — including the
    /// `$EDITOR` / `;cmd` overlay and top-pager paths that return early from
    /// `render_inner` (the "omnipresent" ask). Rows are right-justified to one
    /// common display width: throughput + frame timing (yellow), internals
    /// (teal), process stats (lavender), and a build + terminal-caps footer
    /// (blue). [`HudStyle`] picks how they paint — see the loop below. No-op
    /// unless the monitor is toggled on.
    pub(super) fn render_activity_hud(&self, frame: &mut Frame, frame_area: ratatui::layout::Rect) {
        if !self.view.show_activity {
            return;
        }
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::text::{Line as HudLine, Span};

        // Line 1 — throughput + frame timing. `pk` is the whole terminal.draw
        // (build + diff + tty emission); `r` is just the render closure (CPU).
        // pk-r ≈ diff+emission; pk near the inter-keystroke interval ⇒ render-bound.
        //
        // Each row renders in one style — no per-segment dimming. An earlier `Modifier::DIM` on the
        // `N dps` headline made that count a washed-out grey against the rest,
        // which read as an inconsistent font colour (same problem as the dropped
        // transcript-preview DIM). Fixed-width count/timing fields so the line —
        // and thus the whole block, since line 1 is the longest — keeps a
        // constant width instead of bouncing as throughput and latency move.
        let l1_head = format!(" {:>4} dps", self.view.activity.snap.draws);
        let l1_tail = format!(
            " [p:{:>3} e:{:>3} o:{:>3}]  {:>6} cells/s  pk {:>5.1}ms r{:>5.1}ms echo {:>5.1}ms ",
            self.view.activity.snap.reason_pane,
            self.view.activity.snap.reason_event,
            self.view.activity.snap.reason_other,
            self.view.activity.snap.bytes,
            self.view.activity.peaks_snap.frame_us as f64 / 1000.0,
            self.view.activity.peaks_snap.render_us as f64 / 1000.0,
            // Peak keystroke→echo round-trip (forward → agent echo → render).
            // `echo - r` ≈ the agent/pty round-trip (Claude re-rendering its
            // input box) we don't control; a small `echo` ⇒ spyc isn't the lag.
            self.view.activity.peaks_snap.echo_us as f64 / 1000.0,
        );
        let l1 = format!("{l1_head}{l1_tail}");

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
                crate::ui::pager::Mount::RightPane => "right",
            },
        };
        let git_last = if self.view.activity.git_last_ms == 0 {
            "—".to_string()
        } else {
            format!("{}ms", self.view.activity.git_last_ms)
        };
        let l2 = format!(
            " bg:{bg_running}\u{25cf}{bg_done}\u{2713}{}  git:{}/s last:{}  fs:{}/s  mcp:{}/s  list:{}  pager:{} ",
            if bg_paused > 0 {
                format!(" {bg_paused}\u{23f8}")
            } else {
                String::new()
            },
            self.view.activity.snap.git_results,
            git_last,
            self.view.activity.snap.watcher_events,
            self.view.activity.snap.mcp_reqs,
            self.state.cur().listing.entries.len(),
            pager_state,
        );

        // Line 3 — process stats (PID for `sample`/lldb, RSS, threads). The
        // pid is snapshotted in ViewState at startup — render reads no OS here.
        let pid = self.view.hud_pid;
        let uptime_str = format_uptime(self.view.started_at.elapsed().as_secs());
        let pane_count = self
            .runtime
            .pane_tabs
            .as_ref()
            .map_or(0, |t| t.tabs().len());
        let rss_mb = self.view.activity.proc_rss_kb / 1024;
        let l3 = format!(
            " pid:{pid}  up:{uptime_str}  rss:{rss_mb}m  thr:{}  panes:{pane_count} ",
            self.view.activity.proc_threads,
        );

        // Line 4 — build identity + terminal capabilities. `$TERM` + truecolor
        // are snapshotted in ViewState at startup — render reads no env here.
        let term = &self.view.hud_term;
        let truecolor = self.view.hud_truecolor;
        let l4 = format!(
            " spyc v{}  {term}{}  {}\u{00d7}{} ",
            crate::VERSION,
            if truecolor { " truecolor" } else { "" },
            frame_area.width,
            frame_area.height,
        );

        // The four base rows. Line 1 is fixed-width and the longest, so the
        // block width it sets is constant — the HUD no longer bounces.
        let mut rows: Vec<(String, Color)> = vec![
            (l1, Color::Yellow),
            (l2, self.view.theme.take),
            (l3, self.view.theme.status_user),
            (l4, self.view.theme.dir),
        ];
        let maxw = rows
            .iter()
            .map(|(s, _)| crate::ui::display_width(s))
            .max()
            .unwrap_or(0);

        // Extended section: cumulative per-tool MCP call counts (every agent
        // tools/call, read tools included). Greedy-wrapped to the base block
        // width so it never widens the HUD; stable name-sorted order.
        let calls = &self.view.activity.mcp_tool_calls;
        let entries: Vec<String> = calls
            .iter()
            .filter(|(_, c)| **c > 0)
            .map(|(name, c)| format!("{name}:{c}"))
            .collect();
        let mcp_color = self.view.theme.take;
        let conns = self.view.activity.mcp_connection_summary();
        if entries.is_empty() {
            rows.push((format!(" mcp {conns}  (no tool calls yet) "), mcp_color));
        } else {
            let total: u64 = calls.values().sum();
            let cont_prefix = "        "; // continuation lines indent under the tokens
            let avail = maxw.saturating_sub(2); // keep a trailing space inside the block
            let mut cur = format!(" mcp {conns} \u{2211}{total} ");
            let mut prefix_w = crate::ui::display_width(&cur); // this line's indent width
            let mut cur_w = prefix_w;
            for tok in &entries {
                let tok_w = tok.len() + 1; // a leading space + the "name:count" (ASCII)
                // Wrap when this line already holds a token and the next won't fit.
                if cur_w > prefix_w && cur_w + tok_w > avail {
                    rows.push((format!("{cur} "), mcp_color));
                    cur = cont_prefix.to_string();
                    prefix_w = crate::ui::display_width(cont_prefix);
                    cur_w = prefix_w;
                }
                cur.push(' ');
                cur.push_str(tok);
                cur_w += tok_w;
            }
            rows.push((format!("{cur} "), mcp_color));
        }

        let block_w = u16::try_from(maxw).unwrap_or(u16::MAX);
        // Need the block plus a 1-col right margin.
        if block_w == 0 || frame_area.width <= block_w + 1 {
            return;
        }
        let x = frame_area.width - block_w - 1;
        for (row, (text, band)) in rows.iter().enumerate() {
            let Ok(y) = u16::try_from(row) else { break };
            if y >= frame_area.height {
                break;
            }
            let text_w = u16::try_from(crate::ui::display_width(text))
                .unwrap_or(block_w)
                .min(block_w);
            let pad_w = block_w - text_w;
            // A cell keeps the modifiers of what was drawn there first; without
            // this the HUD turns bold over a directory name.
            let base = Style::default().remove_modifier(Modifier::all());
            let (rect, line) = match self.view.activity_style {
                // Only the text is painted and recorded for the hit-test, so
                // the padding shows (and clicks through to) what's beneath. Its
                // edge and gap spaces stay opaque, keeping the fields legible.
                HudStyle::Transparent => (
                    ratatui::layout::Rect {
                        x: x + pad_w,
                        y,
                        width: text_w,
                        height: 1,
                    },
                    HudLine::from(Span::styled(text.clone(), base.fg(*band).bg(Color::Reset))),
                ),
                HudStyle::Solid => (
                    ratatui::layout::Rect {
                        x,
                        y,
                        width: block_w,
                        height: 1,
                    },
                    HudLine::from(Span::styled(
                        format!("{}{text}", " ".repeat(usize::from(pad_w))),
                        base.fg(Color::Black).bg(*band),
                    )),
                ),
            };
            // Through the chrome funnel, not a bare `render_widget`: that
            // records the row so the pointer can hit-test it and a drag can
            // copy it. The HUD's whole purpose is reporting numbers a human
            // then quotes — pids, timings, `:why-status` counts — so it being
            // unselectable meant retyping them from a screenshot.
            self.draw_chrome_line(frame, rect, line);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)] // test fixtures; the module deny is for production
    use ratatui::buffer::{Buffer, Cell};
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::{Terminal, backend::TestBackend};

    use crate::app::App;
    use crate::app::activity::HudStyle;

    const W: u16 = 140;
    const H: u16 = 10;
    /// What's beneath the HUD: every cell `#`, bold, on blue.
    const BENEATH: Style = Style::new().bg(Color::Blue).add_modifier(Modifier::BOLD);

    /// Draw the HUD alone over a frame of [`BENEATH`], returning the buffer and
    /// the rects it recorded for the mouse hit-test.
    fn draw(style: HudStyle) -> (Buffer, Vec<Rect>) {
        let mut app = App::test_app(std::env::temp_dir());
        app.view.show_activity = true;
        app.view.activity_style = style;
        let mut terminal = Terminal::new(TestBackend::new(W, H)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                for cell in &mut f.buffer_mut().content {
                    cell.set_symbol("#").set_style(BENEATH);
                }
                app.render_activity_hud(f, area);
            })
            .unwrap();
        let rects = app
            .view
            .chrome_rows
            .borrow()
            .iter()
            .map(|r| Rect::new(r.x, r.y, r.width, 1))
            .collect();
        (terminal.backend().buffer().clone(), rects)
    }

    fn untouched(cell: &Cell) -> bool {
        cell.symbol() == "#" && cell.bg == Color::Blue && cell.modifier == Modifier::BOLD
    }

    /// The smallest rect holding every recorded row: the block solid mode fills.
    fn bounds(rects: &[Rect]) -> Rect {
        rects.iter().copied().reduce(Rect::union).unwrap()
    }

    #[test]
    fn transparent_hud_paints_only_the_text_it_records() {
        let (buf, rects) = draw(HudStyle::Transparent);
        assert!(rects.len() >= 5, "four base rows + the mcp row: {rects:?}");
        let mut shown_through = 0;
        for y in 0..H {
            for x in 0..W {
                let cell = &buf[(x, y)];
                if rects.iter().any(|r| r.contains((x, y).into())) {
                    assert_eq!(
                        cell.bg,
                        Color::Reset,
                        "({x},{y}) text sits on the terminal bg"
                    );
                    assert!(
                        cell.modifier.is_empty(),
                        "({x},{y}) inherited {:?}",
                        cell.modifier
                    );
                } else {
                    assert!(
                        untouched(cell),
                        "({x},{y}) painted outside the text: {cell:?}"
                    );
                    if bounds(&rects).contains((x, y).into()) {
                        shown_through += 1;
                    }
                }
            }
        }
        // Rows of different widths leave padding inside the block; with none
        // this test would pass without checking the see-through at all.
        assert!(shown_through > 0, "no padding inside the block: {rects:?}");
    }

    #[test]
    fn solid_hud_is_one_opaque_block() {
        let (buf, rects) = draw(HudStyle::Solid);
        let block = bounds(&rects);
        for r in &rects {
            assert_eq!(
                (r.x, r.width),
                (block.x, block.width),
                "every row spans the block"
            );
        }
        for y in block.top()..block.bottom() {
            for x in block.left()..block.right() {
                let cell = &buf[(x, y)];
                assert!(
                    cell.symbol() != "#" && !matches!(cell.bg, Color::Blue | Color::Reset),
                    "({x},{y}) not covered by a band: {cell:?}"
                );
                assert!(
                    cell.modifier.is_empty(),
                    "({x},{y}) inherited {:?}",
                    cell.modifier
                );
            }
        }
    }
}
