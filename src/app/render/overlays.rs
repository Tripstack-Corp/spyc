//! Modal/overlay draw helpers: the centered harpoon menu
//! (`render_harpoon_menu`) and the top-right activity (`A`) monitor
//! (`render_activity_hud`). Split from `app/render.rs` verbatim; an `impl App`
//! child module reading App's private state via the descendant-module rule.

use ratatui::Frame;

use crate::app::{App, format_uptime};

impl App {
    /// P3-1 visual bell: paint the Charm gradient border pulse over the outermost
    /// ring of cells — a non-destructive bg tint, no reflow / layout change.
    /// Reads `view.visual_bell.frame` (advanced off the draw in
    /// `settle_visual_bell`, so this pure pass never reads the clock) to sweep the
    /// pink→violet→cyan gradient. A no-op when no flash is active — the channel is
    /// off by default (`[notify]`), so steady-state panes never touch this.
    pub(super) fn render_visual_bell(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let Some(bell) = self.view.visual_bell else {
            return;
        };
        if area.width < 2 || area.height < 2 {
            return;
        }
        let phase = bell.frame;
        let buf = frame.buffer_mut();
        let last_col = f32::from(area.width - 1);
        // Top + bottom rows: sweep the gradient horizontally across the frame.
        for x in area.x..area.right() {
            let frac = f32::from(x - area.x) / last_col;
            let color = self.charm_pulse_color(frac, phase);
            if let Some(cell) = buf.cell_mut((x, area.y)) {
                cell.set_bg(color);
            }
            if let Some(cell) = buf.cell_mut((x, area.bottom() - 1)) {
                cell.set_bg(color);
            }
        }
        // Left + right columns (corners already painted above): the gradient's
        // two ends.
        let left = self.charm_pulse_color(0.0, phase);
        let right = self.charm_pulse_color(1.0, phase);
        for y in (area.y + 1)..(area.bottom() - 1) {
            if let Some(cell) = buf.cell_mut((area.x, y)) {
                cell.set_bg(left);
            }
            if let Some(cell) = buf.cell_mut((area.right() - 1, y)) {
                cell.set_bg(right);
            }
        }
    }

    /// Render the harpoon menu overlay. Centered modal box listing
    /// the active project's slots, with the menu cursor on a
    /// highlighted row. Footer shows the bindings. `h_divider_row` /
    /// `v_divider_col` let it nudge its border off a structural divider
    /// (see [`Self::render_chord_hint`]).
    pub(super) fn render_harpoon_menu(
        &self,
        frame: &mut Frame,
        h_divider_row: Option<u16>,
        v_divider_col: Option<u16>,
    ) {
        use ratatui::{
            layout::Rect,
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::{Block, Borders, Clear, Paragraph},
        };
        let Some(menu) = self.view.harpoon_menu.as_ref() else {
            return;
        };
        let Some(h) = self.state.cur().harpoon.as_ref() else {
            return;
        };

        let area = frame.area();
        // Box dims: width clamped, height = 2 chrome + N slots + 2 footer.
        let width = area.width.clamp(40, 72);
        let body_h = (h.slots.len().max(1)) as u16;
        let height = (2 + body_h + 2).min(area.height); // borders + body + footer
        let cx = area.x + (area.width.saturating_sub(width)) / 2;
        let cy = area.y + (area.height.saturating_sub(height)) / 2;
        let x = place_clear_of_line(cx, width, v_divider_col, area.x, area.right());
        let y = place_clear_of_line(cy, height, h_divider_row, area.y, area.bottom());
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
            .border_style(Style::default().fg(self.view.theme.popup_border));
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
        //
        // The whole HUD renders in one foreground colour (solid black on each
        // band) — no per-segment dimming. An earlier `Modifier::DIM` on the
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
            self.state.left.listing.entries.len(),
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
            " spyc v{} ({})  {term}{}  {}\u{00d7}{} ",
            env!("CARGO_PKG_VERSION"),
            env!("SPYC_GIT_SHA"),
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
        if entries.is_empty() {
            rows.push((" mcp  (no tool calls yet) ".to_string(), mcp_color));
        } else {
            let total: u64 = calls.values().sum();
            let cont_prefix = "        "; // continuation lines indent under the tokens
            let avail = maxw.saturating_sub(2); // keep a trailing space inside the block
            let mut cur = format!(" mcp \u{2211}{total} ");
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
        for (row, (text, bg)) in rows.iter().enumerate() {
            let Ok(y) = u16::try_from(row) else { break };
            if y >= frame_area.height {
                break;
            }
            let pad = " ".repeat(maxw.saturating_sub(crate::ui::display_width(text)));
            let rect = ratatui::layout::Rect {
                x,
                y,
                width: block_w,
                height: 1,
            };
            // Every row renders in one uniform style (solid black on its band)
            // so the HUD font colour stays consistent — including row 0, whose
            // `N dps` headline used to be dimmed.
            let normal = Style::default().fg(Color::Black).bg(*bg);
            let line = HudLine::from(Span::styled(format!("{pad}{text}"), normal));
            frame.render_widget(ActivityP::new(line), rect);
        }
    }

    /// Render the which-key chord-hint popup: a centered box listing the armed
    /// chord's continuations (`keys → label`). Long labels **wrap** onto indented
    /// continuation lines (never truncated); entries flow into as many columns as
    /// the terminal height needs. Drawn from `view.chord_hint` (built in
    /// `settle_chord_hint`); a pure `&self` read. No-op when no popup is active.
    ///
    /// `h_divider_row` / `v_divider_col` are the horizontal pane divider's row
    /// and the vertical split separator's column (when present), so the popup
    /// can nudge its border off them — a box edge sitting exactly on a divider
    /// merges the two single-cell rules into one line that reads as broken.
    pub(super) fn render_chord_hint(
        &self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
        h_divider_row: Option<u16>,
        v_divider_col: Option<u16>,
    ) {
        use ratatui::{
            layout::Rect,
            style::{Modifier, Style},
            text::{Line, Span},
            widgets::{Block, Borders, Clear, Paragraph},
        };
        let Some(hint) = self.view.chord_hint.as_ref() else {
            return;
        };
        if hint.rows.is_empty() || area.width < 24 || area.height < 8 {
            return;
        }

        // Column geometry. The label column is capped so a verbose entry wraps
        // instead of widening the box past a comfortable size; the key column is
        // the widest key. One rendered cell line is exactly `col_w` wide:
        // " <key padded>  <label padded> ".
        const LABEL_MAX: usize = 38;
        let key_w = hint
            .rows
            .iter()
            .map(|(k, _)| crate::ui::display_width(k))
            .max()
            .unwrap_or(0);
        let label_w = hint
            .rows
            .iter()
            .map(|(_, l)| crate::ui::display_width(l))
            .max()
            .unwrap_or(0)
            .clamp(1, LABEL_MAX);
        let col_w = key_w + label_w + 4;

        let key_style = Style::default()
            .fg(self.view.theme.pick)
            .add_modifier(Modifier::BOLD);
        let label_style = Style::default().fg(self.view.theme.status_path);

        // Build one cell per entry: its key on the first line, the label
        // word-wrapped to `label_w` across as many lines as it needs (the
        // continuation lines leave the key column blank).
        let cells: Vec<Vec<Line>> = hint
            .rows
            .iter()
            .map(|(keys, label)| {
                let segs = wrap_label(label, label_w);
                segs.iter()
                    .enumerate()
                    .map(|(i, seg)| {
                        let key_cell = if i == 0 {
                            format!(" {keys:>key_w$}  ")
                        } else {
                            " ".repeat(key_w + 3)
                        };
                        Line::from(vec![
                            Span::styled(key_cell, key_style),
                            Span::styled(format!("{seg:<label_w$} "), label_style),
                        ])
                    })
                    .collect()
            })
            .collect();

        // Flow cells into columns, balancing by rendered-line count so a tall
        // menu (or one with wrapped labels) splits into columns rather than
        // overflowing the screen height. An entry never splits across columns.
        let body_h = (area.height as usize).saturating_sub(2).max(1);
        let total_lines: usize = cells.iter().map(Vec::len).sum();
        let max_cols = ((area.width as usize).saturating_sub(2) / col_w.max(1)).max(1);
        let n_cols = total_lines.div_ceil(body_h).clamp(1, max_cols);
        let target = total_lines.div_ceil(n_cols).max(1);

        let mut columns: Vec<Vec<Line>> = Vec::new();
        let mut cur: Vec<Line> = Vec::new();
        for cell in cells {
            if !cur.is_empty() && cur.len() + cell.len() > target {
                columns.push(std::mem::take(&mut cur));
            }
            cur.extend(cell);
        }
        if !cur.is_empty() {
            columns.push(cur);
        }

        let n = columns.len().max(1);
        let rows_tall = columns.iter().map(Vec::len).max().unwrap_or(0);
        let width = ((n * col_w) as u16 + 2).min(area.width);
        let height = (rows_tall as u16 + 2).min(area.height);
        // Center, then nudge a border off a structural divider it would land on
        // (a box edge collinear with the pane / split rule merges into it).
        let cx = area.x + area.width.saturating_sub(width) / 2;
        let cy = area.y + area.height.saturating_sub(height) / 2;
        let x = place_clear_of_line(cx, width, v_divider_col, area.x, area.right());
        let y = place_clear_of_line(cy, height, h_divider_row, area.y, area.bottom());
        let rect = Rect {
            x,
            y,
            width,
            height,
        };
        frame.render_widget(Clear, rect);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", hint.title))
            .border_style(Style::default().fg(self.view.theme.popup_border));
        let inner = block.inner(rect);
        frame.render_widget(block, rect);

        // Stitch the columns side by side row-by-row; pad missing cells with a
        // blank `col_w`-wide span so columns stay aligned.
        let blank = Span::styled(" ".repeat(col_w), label_style);
        let mut lines: Vec<Line> = Vec::with_capacity(rows_tall);
        for r in 0..rows_tall {
            let mut spans: Vec<Span> = Vec::with_capacity(n * 2);
            for col in &columns {
                match col.get(r) {
                    Some(line) => spans.extend(line.spans.iter().cloned()),
                    None => spans.push(blank.clone()),
                }
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

/// Place a popup span of `size` cells so neither of its edges (`start` and
/// `start + size - 1`) sits on `line`, staying within `[min, max_end)`. Starts
/// from the centered `start` and, only if it collides, tries the nearest
/// offsets (±1, ±2); if none fit (the popup nearly fills the axis, leaving no
/// room to dodge), returns the centered value unchanged. `line: None` (no such
/// divider on screen) is a no-op.
fn place_clear_of_line(start: u16, size: u16, line: Option<u16>, min: u16, max_end: u16) -> u16 {
    let Some(line) = line else {
        return start;
    };
    let collides = |s: u16| s == line || s + size.saturating_sub(1) == line;
    if !collides(start) {
        return start;
    }
    for delta in [1i32, -1, 2, -2] {
        let s = i32::from(start) + delta;
        if s >= i32::from(min) && s + i32::from(size) <= i32::from(max_end) && !collides(s as u16) {
            return s as u16;
        }
    }
    start
}

/// Greedy word-wrap a popup label to `width` display columns. Returns one
/// segment per line. A single word longer than `width` lands on its own line
/// (popup labels never contain such words, so this stays simple).
fn wrap_label(label: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    if crate::ui::display_width(label) <= width {
        return vec![label.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;
    for word in label.split(' ') {
        let ww = crate::ui::display_width(word);
        if cur_w == 0 {
            cur.push_str(word);
            cur_w = ww;
        } else if cur_w + 1 + ww <= width {
            cur.push(' ');
            cur.push_str(word);
            cur_w += 1 + ww;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur.push_str(word);
            cur_w = ww;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::place_clear_of_line;

    #[test]
    fn no_line_or_no_collision_keeps_centered() {
        // No divider on screen → unchanged.
        assert_eq!(place_clear_of_line(10, 5, None, 0, 80), 10);
        // Divider clear of both edges (10..=14) → unchanged.
        assert_eq!(place_clear_of_line(10, 5, Some(20), 0, 80), 10);
    }

    #[test]
    fn top_edge_on_line_shifts_down() {
        // start == line: nudged to 11 (edges 11 and 15, both clear of 10).
        assert_eq!(place_clear_of_line(10, 5, Some(10), 0, 80), 11);
    }

    #[test]
    fn bottom_edge_on_line_shifts() {
        // bottom edge (start+size-1 = 14) == line → +1 makes edges 11 and 15,
        // neither is 14 → resolved.
        assert_eq!(place_clear_of_line(10, 5, Some(14), 0, 80), 11);
    }

    #[test]
    fn falls_back_to_minus_one_when_down_would_overflow() {
        // Height 5 at the very bottom: start=5, max_end=10 (edges 5..=9),
        // line=5 (top edge). +1 would push the bottom edge past max_end, so it
        // shifts up to 4 instead (edges 4 and 8).
        assert_eq!(place_clear_of_line(5, 5, Some(5), 0, 10), 4);
    }

    #[test]
    fn no_room_to_dodge_returns_centered() {
        // Popup fills the whole axis (size == span): can't move, stays put.
        assert_eq!(place_clear_of_line(0, 10, Some(0), 0, 10), 0);
    }
}
