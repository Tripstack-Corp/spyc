//! The pager's pure render pass: the `render` entry, single/multi-column body
//! painting, per-row styling, visual-block overlay, whitespace markers. Verbatim.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::ui::theme::Theme;

use super::{Mount, PagerView, VisualKind};

use super::layout::{
    COL_GAP, line_plain_text, pager_inner_area, partition_lines_static, wrap_line,
};

pub fn render(frame: &mut Frame, area: Rect, view: &PagerView, theme: &Theme) {
    let inner_area = pager_inner_area(area, view);

    frame.render_widget(Clear, inner_area);

    let pos = view.position_indicator(inner_area.height.saturating_sub(2));
    let title_style = Style::default()
        .fg(theme.prompt_prefix)
        .add_modifier(Modifier::BOLD);
    // Flash is teal + BOLD against the amber title so help notices
    // (e.g. "truncated at 5000 lines · press p for full file in
    // $PAGER") stand out clearly as a separate piece of info, not as
    // an extension of the filename.
    let flash_style = Style::default().fg(theme.take).add_modifier(Modifier::BOLD);
    let title_line: Line<'static> = if let Some(ref msg) = view.flash {
        Line::from(vec![
            Span::styled(format!("  {}  ", view.title), title_style),
            Span::styled(format!(" {msg} "), flash_style),
            Span::styled("  ", title_style),
        ])
    } else {
        Line::from(Span::styled(
            format!("  {}   ({} lines)  ", view.title, view.lines.len()),
            title_style,
        ))
    };
    let title_right = format!("  {pos}  ");
    // Borderless when:
    //   - `full_width` (current behavior — terminal text selection
    //     is clean without the box drawing).
    //   - `Mount::LowerPane` — the pager occupies the bottom-pane
    //     slot, which the pty renders into without a border. Drawing
    //     a border eats two rows of usable content and visually
    //     disrupts the layout the user just had on-screen.
    let borderless = view.full_width || matches!(view.mount, Mount::LowerPane);
    let block = if borderless {
        Block::default()
    } else {
        Block::default()
            .borders(Borders::ALL)
            .title(title_line)
            .title_bottom(
                Line::from(Span::styled(
                    title_right,
                    Style::default()
                        .fg(theme.status_suffix)
                        .add_modifier(Modifier::BOLD),
                ))
                .right_aligned(),
            )
    };
    let body_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    // Reserve the bottom row for the search/status bar. In multi-column
    // views (help) the row is always reserved so the viewport height stays
    // constant when search is activated — otherwise the column layout
    // would reflow. In single-column views it's only shown when active.
    let ncols = view.columns.max(1) as usize;
    let show_search_row = view.status_text().is_some() || ncols > 1;
    let (content_area, search_area) = if show_search_row {
        (
            Rect {
                x: body_area.x,
                y: body_area.y,
                width: body_area.width,
                height: body_area.height.saturating_sub(1),
            },
            Some(Rect {
                x: body_area.x,
                y: body_area.y + body_area.height.saturating_sub(1),
                width: body_area.width,
                height: 1,
            }),
        )
    } else {
        (body_area, None)
    };

    // Cache the viewport height the renderer is using *now* so the
    // tick-loop streaming-capture path can call scroll_to_bottom_auto
    // with a real number (instead of the v1.20-era hardcoded 40 that
    // caused the auto-tail to under-shoot on tall terminals --
    // showing only the top half of the pager filled with content
    // and the rest with `~` until the user manually scrolled).
    view.last_viewport_h.set(content_area.height);

    if ncols > 1 {
        render_multi_column(frame, content_area, view, theme, ncols);
    } else {
        render_single_column(frame, content_area, view, theme);
    }

    if let Some(rect) = search_area
        && let Some(text) = view.status_text()
    {
        let style = Style::default()
            .fg(theme.prompt_prefix)
            .add_modifier(Modifier::BOLD);
        frame.render_widget(Paragraph::new(Line::from(Span::styled(text, style))), rect);
    }
}

fn render_single_column(frame: &mut Frame, content_area: Rect, view: &PagerView, theme: &Theme) {
    let viewport_h = content_area.height as usize;
    let start = view.scroll as usize;
    let content_end = view.lines.len();

    let total_lines = view.lines.len();
    // Streaming views can grow during render; clamp to the caller's
    // expected upper bound so the gutter doesn't widen mid-scan.
    let gutter_basis = total_lines.max(view.line_count_hint.unwrap_or(0));
    let gutter_w = if view.show_line_numbers {
        gutter_basis.max(1).ilog10() as usize + 2
    } else {
        0
    };
    // Line-number gutter: muted but readable. Previously DIM-on-top
    // of status_suffix which left it almost invisible against dark
    // backgrounds; dropped the DIM modifier so the digits actually
    // register.
    let ln_style = Style::default().fg(theme.status_suffix);

    // Width available for content (after the line-number gutter).
    // Used by wrap to decide where to break visual rows. We render
    // body + gutter into the same Paragraph so the visual budget
    // matches the actual area ratatui will draw into.
    let body_w = (content_area.width as usize).saturating_sub(gutter_w);
    // Cache for `scroll_max` so the wrap-aware bound stays
    // accurate across keystrokes. 0 stays as "unknown" (e.g.
    // multi-col) so the wrap-aware path correctly falls back to
    // the logical-line bound there.
    view.last_body_w
        .set(u16::try_from(body_w).unwrap_or(u16::MAX));

    let mut display_lines: Vec<Line<'static>> = Vec::with_capacity(viewport_h);
    let mut src_idx = start;
    while src_idx < content_end && display_lines.len() < viewport_h {
        let line = &view.lines[src_idx];
        let abs_idx = src_idx;
        // Apply per-source-line styling: match highlight, picker
        // cursor highlight, optional whitespace markers. The `$`
        // end-of-line marker naturally ends up on the last wrapped
        // piece because we apply markers *before* wrap.
        let styled = apply_row_styling(line, view, abs_idx, theme);
        let styled = if view.show_whitespace {
            apply_whitespace_markers(&styled, theme)
        } else {
            styled
        };
        // Split into 1+ visual rows. wrap=false ⇒ exactly one piece;
        // wrap=true with body_w available width gives a Vec of
        // visually-bounded chunks, preserving styling per-span.
        // Block visual mode forces wrap off — the column-based
        // selection rectangle only makes sense against logical
        // rows; with wrap on, a "row" the user is selecting could
        // be split across multiple visual rows and the rectangle
        // would smear. Vim does the same.
        let block_mode = view.visual.is_some_and(|v| v.kind == VisualKind::Block);
        let pieces = if view.wrap && body_w > 0 && !block_mode {
            wrap_line(&styled, body_w)
        } else {
            vec![styled]
        };
        for (piece_idx, piece) in pieces.into_iter().enumerate() {
            if display_lines.len() >= viewport_h {
                break;
            }
            if gutter_w > 0 {
                let gutter_text = if piece_idx == 0 {
                    format!("{:>width$} ", abs_idx + 1, width = gutter_w - 1)
                } else {
                    // Continuation row: blank gutter so wrap pieces
                    // visually align with the source line's indent.
                    " ".repeat(gutter_w)
                };
                let mut spans = vec![Span::styled(gutter_text, ln_style)];
                spans.extend(piece.spans);
                display_lines.push(Line::from(spans));
            } else {
                display_lines.push(piece);
            }
        }
        src_idx += 1;
    }
    let reached_end = src_idx >= content_end;

    let eof_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);
    if reached_end && display_lines.len() < viewport_h && !view.streaming {
        if !view.eof_in_content {
            display_lines.push(Line::from(Span::styled("[EOF]", eof_style)));
        }
        while display_lines.len() < viewport_h {
            display_lines.push(Line::from(Span::styled("~", eof_style)));
        }
    }

    // Wrap is handled by `wrap_line` above; ratatui's Paragraph::wrap
    // is *not* used because it hard-breaks unbreakable "words" mid-
    // character and continuation rows wouldn't carry the gutter.
    // Yank / save / search operate on `view.lines` so they always
    // see the untruncated source regardless of wrap state.
    let paragraph = Paragraph::new(display_lines);
    frame.render_widget(paragraph, content_area);
}

fn render_multi_column(
    frame: &mut Frame,
    content_area: Rect,
    view: &PagerView,
    theme: &Theme,
    ncols: usize,
) {
    let viewport_h = content_area.height as usize;
    let scroll = view.scroll as usize;
    let content_end = view.lines.len();
    // Divide available width evenly (minus gaps between columns).
    let total_gap = COL_GAP * (ncols as u16).saturating_sub(1);
    let col_w = content_area.width.saturating_sub(total_gap) / ncols as u16;

    let eof_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);

    // Static partition: content-to-column mapping is fixed (doesn't shift
    // as the user scrolls). Each column then applies the scroll offset
    // independently within its own chunk.
    let chunks = partition_lines_static(&view.lines, ncols);

    for (col, (chunk_start, chunk_end)) in chunks.into_iter().enumerate() {
        let chunk_len = chunk_end - chunk_start;
        let local_scroll = scroll.min(chunk_len);
        let col_start = chunk_start + local_scroll;
        let col_end = (col_start + viewport_h).min(chunk_end);
        let x = content_area.x + (col as u16) * (col_w + COL_GAP);
        let col_rect = Rect {
            x,
            y: content_area.y,
            width: col_w,
            height: content_area.height,
        };

        let mut display_lines: Vec<Line<'static>> = if col_start < chunk_end {
            view.lines[col_start..col_end]
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let abs_idx = col_start + i;
                    styled_line_for_render(line, view, abs_idx, theme)
                })
                .collect()
        } else {
            Vec::new()
        };

        // Pad with tilde markers when this column has fewer lines than the
        // viewport. Only mark [EOF] on the last column — per-column EOFs
        // would wrongly imply the overall document ended early.
        if display_lines.len() < viewport_h && !view.streaming {
            let is_last_col = col + 1 == ncols;
            if is_last_col && col_start < content_end && !view.eof_in_content {
                display_lines.push(Line::from(Span::styled("[EOF]", eof_style)));
            }
            while display_lines.len() < viewport_h {
                display_lines.push(Line::from(Span::styled("~", eof_style)));
            }
        }

        let paragraph = Paragraph::new(display_lines);
        frame.render_widget(paragraph, col_rect);

        // Draw a thin separator between columns.
        if col + 1 < ncols {
            let sep_x = x + col_w;
            let sep_style = Style::default()
                .fg(theme.status_suffix)
                .add_modifier(Modifier::DIM);
            for row in 0..content_area.height {
                let buf = frame.buffer_mut();
                buf.set_string(sep_x, content_area.y + row, "│", sep_style);
            }
        }
    }
}

/// Apply match-highlight + picker-cursor styling to a source line.
/// Extracted from `render_single_column` so wrap can re-use it
/// (styling decisions happen before the visual split).
fn apply_row_styling(
    line: &Line<'static>,
    view: &PagerView,
    abs_idx: usize,
    theme: &Theme,
) -> Line<'static> {
    let mut styled = styled_line_for_render(line, view, abs_idx, theme);
    // Visual selection: paint a muted background across the
    // selected region so the user can see what `y` will yank.
    // - Line mode (`V`): whole rows in `[lo..=hi]`. Cursor row
    //   gets the brighter cursor_bg, others cursor_bg_dim.
    // - Block mode (`^v`): only the rectangular slice
    //   `[lo_col..=hi_col]` of each row in `[lo..=hi]` is
    //   highlighted, painted character-by-character. The cursor
    //   *cell* (cursor_line, cursor_col) gets the brighter bg.
    // Applied before the picker-cursor branch so visual mode wins
    // when both would coincide.
    if let Some(sel) = view.visual {
        let (lo, hi) = sel.range();
        if (lo..=hi).contains(&abs_idx) {
            match sel.kind {
                VisualKind::Line => {
                    let bg = if abs_idx == sel.cursor {
                        theme.cursor_bg
                    } else {
                        theme.cursor_bg_dim
                    };
                    styled = Line::from(
                        styled
                            .spans
                            .into_iter()
                            .map(|s| Span::styled(s.content, s.style.bg(bg)))
                            .collect::<Vec<_>>(),
                    );
                }
                VisualKind::Block => {
                    let (lo_col, hi_col) = sel.col_range();
                    let cursor_col = if abs_idx == sel.cursor {
                        Some(sel.cursor_col)
                    } else {
                        None
                    };
                    styled = paint_block_selection(
                        &styled,
                        lo_col,
                        hi_col,
                        cursor_col,
                        theme.cursor_bg_dim,
                        theme.cursor_bg,
                    );
                }
            }
        }
    }
    // Placement cursor: single reverse-video cell at the current
    // (row, col). The visual cue the user asked for so they can see
    // where the anchor will land when they commit with `^v` / `V`.
    if let Some(p) = view.placement
        && p.row == abs_idx
    {
        let plain: String = styled.spans.iter().map(|s| s.content.as_ref()).collect();
        let row_style = styled.style;
        let before: String = plain.chars().take(p.col).collect();
        let cursor_ch: String = plain
            .chars()
            .nth(p.col)
            .map_or_else(|| " ".into(), |c| c.to_string());
        let after: String = plain.chars().skip(p.col + 1).collect();
        let cursor_style = Style::default()
            .bg(theme.cursor_bg)
            .fg(theme.cursor_fg)
            .add_modifier(Modifier::REVERSED | Modifier::BOLD);
        styled = Line::from(vec![
            Span::styled(before, row_style),
            Span::styled(cursor_ch, cursor_style),
            Span::styled(after, row_style),
        ]);
    }

    if view.picker_cursor == Some(abs_idx) {
        if let Some((col, vi_mode)) = view.picker_edit_cursor {
            // History editor: show editing cursor on this line.
            let plain: String = styled.spans.iter().map(|s| s.content.as_ref()).collect();
            let row_style = Style::default().bg(theme.cursor_bg).fg(theme.cursor_fg);
            let before: String = plain.chars().take(col).collect();
            let cursor_ch: String = plain
                .chars()
                .nth(col)
                .map_or_else(|| " ".into(), |c| c.to_string());
            let after: String = plain.chars().skip(col + 1).collect();
            let cursor_style = if vi_mode == crate::ui::line_edit::Mode::Normal {
                row_style.add_modifier(Modifier::REVERSED)
            } else {
                row_style.add_modifier(Modifier::UNDERLINED)
            };
            styled = Line::from(vec![
                Span::styled(before, row_style),
                Span::styled(cursor_ch, cursor_style),
                Span::styled(after, row_style),
            ]);
        } else {
            styled = Line::from(
                styled
                    .spans
                    .into_iter()
                    .map(|s| {
                        Span::styled(
                            s.content,
                            s.style
                                .bg(theme.cursor_bg)
                                .fg(theme.cursor_fg)
                                .add_modifier(Modifier::BOLD),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
        }
    }
    styled
}

/// Paint a block-selection rectangle's row contribution onto a
/// styled line: chars in `[lo_col..=hi_col]` get
/// `selection_bg` overlaid on their existing style; the
/// `cursor_col` cell (when set) gets `cursor_bg` instead so it
/// reads like vi's "I'm here" cell. Char index is character-
/// based (Unicode scalars), not display-width, so wide-glyph
/// (CJK / emoji) counts as 1 — same convention vim uses for
/// block mode.
///
/// Adjacent characters with identical styles merge into a
/// single span so we don't explode the renderer with one span
/// per char.
fn paint_block_selection(
    line: &Line<'static>,
    lo_col: usize,
    hi_col: usize,
    cursor_col: Option<usize>,
    selection_bg: ratatui::style::Color,
    cursor_bg: ratatui::style::Color,
) -> Line<'static> {
    let mut new_spans: Vec<Span<'static>> = Vec::new();
    let mut current_text = String::new();
    let mut current_style: Option<Style> = None;
    let mut col = 0usize;

    for span in &line.spans {
        for ch in span.content.chars() {
            let in_sel = col >= lo_col && col <= hi_col;
            let style = if in_sel {
                if cursor_col == Some(col) {
                    span.style.bg(cursor_bg)
                } else {
                    span.style.bg(selection_bg)
                }
            } else {
                span.style
            };
            if Some(style) != current_style {
                if !current_text.is_empty() {
                    new_spans.push(Span::styled(
                        std::mem::take(&mut current_text),
                        current_style.unwrap_or_default(),
                    ));
                }
                current_style = Some(style);
            }
            current_text.push(ch);
            col += 1;
        }
    }
    if !current_text.is_empty() {
        new_spans.push(Span::styled(
            current_text,
            current_style.unwrap_or_default(),
        ));
    }
    Line::from(new_spans)
}

/// Apply match highlighting to a line when a search is active. The
/// current match gets the cursor-bg color for max pop; other matches get
/// a softer bg tint.
fn styled_line_for_render(
    line: &Line<'static>,
    view: &PagerView,
    idx: usize,
    theme: &Theme,
) -> Line<'static> {
    let (is_match, is_current) = view.match_state(idx);
    if !is_match {
        return line.clone();
    }
    let bg = if is_current {
        theme.cursor_bg
    } else {
        theme.other
    };
    // Apply the background across every span in the line so the whole
    // row reads as "a hit" without clobbering existing fg colors.
    let spans = line
        .spans
        .iter()
        .map(|s| {
            let mut style = s.style;
            style = style.bg(bg);
            if is_current {
                style = style.add_modifier(Modifier::BOLD);
            }
            Span::styled(s.content.clone(), style)
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

/// Vim-style whitespace substitution. Applied per span to keep existing
/// colors. Visual cues:
///   `→`  tab
///   `·`  trailing space
///   `^M` carriage return
///   `$`  end-of-line (non-empty lines only — blank lines are obviously blank)
fn apply_whitespace_markers(line: &Line<'static>, theme: &Theme) -> Line<'static> {
    // Warm amber-ish so markers are visible against dark backgrounds
    // without fighting the content. Uses the pick color (amber) dimmed.
    let ws_style = Style::default().fg(theme.pick).add_modifier(Modifier::DIM);

    // Check if the whole line is empty / whitespace-only.
    let plain = line_plain_text(line);
    if plain.trim().is_empty() {
        // Don't clutter blank lines with `$`.
        return line.clone();
    }

    let mut out: Vec<Span<'static>> = Vec::new();
    for span in &line.spans {
        let text: &str = &span.content;
        let mut segment = String::new();
        for ch in text.chars() {
            match ch {
                '\t' => {
                    if !segment.is_empty() {
                        out.push(Span::styled(std::mem::take(&mut segment), span.style));
                    }
                    out.push(Span::styled("→", ws_style));
                }
                '\r' => {
                    if !segment.is_empty() {
                        out.push(Span::styled(std::mem::take(&mut segment), span.style));
                    }
                    out.push(Span::styled("^M", ws_style));
                }
                _ => segment.push(ch),
            }
        }
        if !segment.is_empty() {
            out.push(Span::styled(segment, span.style));
        }
    }

    // Replace trailing spaces with `·` for visibility.
    if let Some(last) = out.last_mut() {
        let content: &str = &last.content;
        if content.ends_with(' ') {
            let trimmed = content.trim_end();
            // Trailing spaces are always ASCII, so byte len == display width.
            let trailing_count = content.len() - trimmed.len();
            let style = last.style;
            *last = Span::styled(trimmed.to_string(), style);
            let dots: String = "·".repeat(trailing_count);
            out.push(Span::styled(dots, ws_style));
        }
    }

    out.push(Span::styled("$", ws_style));
    Line::from(out)
}
