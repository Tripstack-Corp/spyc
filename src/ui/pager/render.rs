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
    COL_GAP, line_plain_text, pager_inner_area, partition_lines_static, wrap_line_capped,
};

pub fn render(frame: &mut Frame, area: Rect, view: &PagerView, theme: &Theme) {
    let inner_area = pager_inner_area(area, view);

    frame.render_widget(Clear, inner_area);

    // Borderless when:
    //   - `full_width` (current behavior — terminal text selection
    //     is clean without the box drawing).
    //   - `Mount::LowerPane` — the pager occupies the bottom-pane
    //     slot, which the pty renders into without a border. Drawing
    //     a border eats two rows of usable content and visually
    //     disrupts the layout the user just had on-screen.
    // The title bar and position indicator only exist on the bordered
    // block, so build them inside that branch — borderless mode discarded
    // them before (and fed `position_indicator` an `inner-2` viewport that
    // doesn't match a border-free body anyway).
    // Multi-column partition is viewport-independent, so compute it ONCE here
    // and share it with the position indicator and the body layout below.
    // Previously each of `position_indicator`, `render_multi_column` (and
    // `clamp_scroll` on input) re-ran `partition_lines_static` — up to 3
    // O(lines) scans per keystroke in multi-col mode. This collapses the two
    // render-pass scans into one.
    let ncols = view.columns.max(1) as usize;
    let multi_col_chunks = (ncols > 1).then(|| partition_lines_static(&view.lines, ncols));

    let borderless = view.full_width || matches!(view.mount, Mount::LowerPane);
    let block = if borderless {
        Block::default()
    } else {
        let pos = match &multi_col_chunks {
            Some(chunks) => {
                view.position_indicator_multi(chunks, inner_area.height.saturating_sub(2))
            }
            None => view.position_indicator(inner_area.height.saturating_sub(2)),
        };
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

    if let Some(chunks) = multi_col_chunks {
        render_multi_column(frame, content_area, view, theme, ncols, &chunks);
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
    let start = view.scroll;
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
        // Tabs always expand to `view.tab_width` columns so indentation lines
        // up (a raw `\t` would otherwise collapse or misalign). With `w` on the
        // expansion shows a `→` marker; with `w` off it's plain indentation.
        // `visual_rows` (scroll math) uses the same width, so wrap/scroll agree.
        let styled = expand_tabs(&styled, view.tab_width, view.show_whitespace, theme);
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
            // Only wrap as many rows as still fit the viewport — a single
            // 10k-char line shouldn't allocate 100s of off-screen pieces.
            let budget = viewport_h.saturating_sub(display_lines.len());
            wrap_line_capped(&styled, body_w, budget)
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
    chunks: &[(usize, usize)],
) {
    let viewport_h = content_area.height as usize;
    let scroll = view.scroll;
    let content_end = view.lines.len();
    // Divide available width evenly (minus gaps between columns).
    let total_gap = COL_GAP * (ncols as u16).saturating_sub(1);
    let col_w = content_area.width.saturating_sub(total_gap) / ncols as u16;

    let eof_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);

    // Static partition: content-to-column mapping is fixed (doesn't shift
    // as the user scrolls). Each column then applies the scroll offset
    // independently within its own chunk. The partition is computed once in
    // `render` and shared with the position indicator (passed in as `chunks`).
    for (col, &(chunk_start, chunk_end)) in chunks.iter().enumerate() {
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
    // Placement cursor: where the anchor will land when the user
    // commits with `^v` / `V`. Block placement (`^v`) shows a single
    // reverse-video cell at (row, col) since the column matters; Line
    // placement (the first `V` of the double-tap arm) highlights the
    // whole candidate row so it previews exactly which line the line
    // selection will start on.
    if let Some(p) = view.placement
        && p.row == abs_idx
    {
        match p.kind {
            VisualKind::Line => {
                styled = Line::from(
                    styled
                        .spans
                        .into_iter()
                        .map(|s| Span::styled(s.content, s.style.bg(theme.cursor_bg)))
                        .collect::<Vec<_>>(),
                );
            }
            VisualKind::Block => {
                // Re-style only the cursor cell so the rest of the row keeps
                // its syntax-highlight / search styling while the user
                // positions the placement anchor.
                let cursor_style = Style::default()
                    .bg(theme.cursor_bg)
                    .fg(theme.cursor_fg)
                    .add_modifier(Modifier::REVERSED | Modifier::BOLD);
                styled = restyle_cursor_cell(&styled, p.col, cursor_style);
            }
        }
    }

    if view.picker_cursor == Some(abs_idx) {
        if let Some((col, vi_mode)) = view.picker_edit_cursor {
            // History editor: the whole selected row reads as solid cursor
            // colors (like the non-edit picker row below); the edit cell on
            // top gets a reverse/underline cue for normal/insert mode.
            let row_style = Style::default().bg(theme.cursor_bg).fg(theme.cursor_fg);
            let solid = Line::from(
                styled
                    .spans
                    .iter()
                    .map(|s| Span::styled(s.content.clone(), row_style))
                    .collect::<Vec<_>>(),
            );
            let cursor_style = if vi_mode == crate::ui::line_edit::Mode::Normal {
                row_style.add_modifier(Modifier::REVERSED)
            } else {
                row_style.add_modifier(Modifier::UNDERLINED)
            };
            styled = restyle_cursor_cell(&solid, col, cursor_style);
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

/// Re-style a single character cell at char index `col` within a styled
/// line, leaving every other cell's styling untouched: the cursor cell gets
/// `cursor_style`, all other cells keep their original span style. If `col`
/// is at or past end-of-line, a synthetic space cell (styled `cursor_style`)
/// is appended — matching how a vi cursor sits past the last char.
///
/// Char-indexed (Unicode scalars), matching the placement / history-editor
/// cursor-column convention. Adjacent same-style chars coalesce into one
/// span so we don't explode the renderer with a span per char. The line's
/// own (line-level) style is preserved.
fn restyle_cursor_cell(line: &Line<'static>, col: usize, cursor_style: Style) -> Line<'static> {
    let mut new_spans: Vec<Span<'static>> = Vec::new();
    let mut current_text = String::new();
    let mut current_style: Option<Style> = None;
    let mut idx = 0usize;
    let mut placed = false;

    let mut push = |text: &mut String, style: &mut Option<Style>, next: Style| {
        if Some(next) != *style {
            if !text.is_empty() {
                new_spans.push(Span::styled(
                    std::mem::take(text),
                    style.unwrap_or_default(),
                ));
            }
            *style = Some(next);
        }
    };

    for span in &line.spans {
        for ch in span.content.chars() {
            let style = if idx == col {
                placed = true;
                cursor_style
            } else {
                span.style
            };
            push(&mut current_text, &mut current_style, style);
            current_text.push(ch);
            idx += 1;
        }
    }
    if !current_text.is_empty() {
        new_spans.push(Span::styled(
            current_text,
            current_style.unwrap_or_default(),
        ));
    }
    // Cursor parked at or past end-of-line: append a single styled space so
    // it's still visible.
    if !placed {
        new_spans.push(Span::styled(" ".to_string(), cursor_style));
    }

    let mut out = Line::from(new_spans);
    out.style = line.style;
    out
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

/// Expand tab characters to `tab_width` columns so indentation lines up in the
/// pager (a raw `\t` renders as a single cell / gets dropped otherwise). With
/// `show_marker` the first cell of each expanded tab is a `→` in the
/// whitespace-marker style and the rest is blank fill; otherwise the tab
/// becomes plain spaces carrying the original span's style. Fixed width per tab
/// — one indent level = one `tab_width`, matching how editors show a tab
/// indent. A line with no tabs is returned untouched (the common fast path).
fn expand_tabs(
    line: &Line<'static>,
    tab_width: usize,
    show_marker: bool,
    theme: &Theme,
) -> Line<'static> {
    if !line.spans.iter().any(|s| s.content.contains('\t')) {
        return line.clone();
    }
    let width = tab_width.max(1);
    let ws_style = Style::default().fg(theme.pick).add_modifier(Modifier::DIM);
    let mut out: Vec<Span<'static>> = Vec::new();
    for span in &line.spans {
        let mut segment = String::new();
        for ch in span.content.chars() {
            if ch == '\t' {
                if !segment.is_empty() {
                    out.push(Span::styled(std::mem::take(&mut segment), span.style));
                }
                if show_marker {
                    out.push(Span::styled("→", ws_style));
                    if width > 1 {
                        out.push(Span::styled(" ".repeat(width - 1), ws_style));
                    }
                } else {
                    out.push(Span::styled(" ".repeat(width), span.style));
                }
            } else {
                segment.push(ch);
            }
        }
        if !segment.is_empty() {
            out.push(Span::styled(segment, span.style));
        }
    }
    Line::from(out)
}

/// Vim-style whitespace substitution. Applied per span to keep existing
/// colors. Tabs are already expanded by [`expand_tabs`] before this runs, so
/// only the remaining cues are handled here. Visual cues:
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

#[cfg(test)]
mod tests {
    use super::restyle_cursor_cell;
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span};

    /// Flatten a styled line into `(text, fg)` runs for assertion.
    fn runs(line: &Line<'static>) -> Vec<(String, Option<Color>)> {
        line.spans
            .iter()
            .map(|s| (s.content.to_string(), s.style.fg))
            .collect()
    }

    fn two_color_line() -> Line<'static> {
        // "ab" red, "cd" blue.
        Line::from(vec![
            Span::styled("ab", Style::default().fg(Color::Red)),
            Span::styled("cd", Style::default().fg(Color::Blue)),
        ])
    }

    #[test]
    fn expand_tabs_off_marker_uses_plain_spaces_at_tab_width() {
        let theme = crate::ui::theme::Theme::default();
        let line = Line::from(Span::styled("\tx", Style::default().fg(Color::Red)));
        // tab_width 4, no marker: a leading tab becomes 4 plain spaces that
        // keep the surrounding span's color, so indentation aligns invisibly.
        let out = super::expand_tabs(&line, 4, false, &theme);
        let text: String = out.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "    x");
        assert!(!text.contains('→'), "no marker when whitespace is off");
    }

    #[test]
    fn expand_tabs_with_marker_shows_arrow_then_fill() {
        let theme = crate::ui::theme::Theme::default();
        let line = Line::from(Span::raw("\tx"));
        // tab_width 4, marker on: `→` then 3 fill columns, total width 4.
        let out = super::expand_tabs(&line, 4, true, &theme);
        let text: String = out.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "→   x");
    }

    #[test]
    fn expand_tabs_respects_configured_width() {
        let theme = crate::ui::theme::Theme::default();
        let line = Line::from(Span::raw("\tx"));
        let out = super::expand_tabs(&line, 2, false, &theme);
        let text: String = out.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "  x", "two-column tab");
    }

    #[test]
    fn expand_tabs_leaves_tab_free_lines_untouched() {
        let theme = crate::ui::theme::Theme::default();
        let line = two_color_line();
        let out = super::expand_tabs(&line, 4, true, &theme);
        assert_eq!(runs(&out), runs(&line), "no tabs → unchanged");
    }

    #[test]
    fn restyle_cursor_cell_preserves_other_cells_styling() {
        let line = two_color_line();
        let cursor = Style::default().fg(Color::Yellow);
        // Cursor on 'c' (col 2): 'a','b' stay red, 'c' becomes the cursor
        // style, 'd' stays blue — syntax highlight is NOT flattened.
        let out = restyle_cursor_cell(&line, 2, cursor);
        assert_eq!(
            runs(&out),
            vec![
                ("ab".into(), Some(Color::Red)),
                ("c".into(), Some(Color::Yellow)),
                ("d".into(), Some(Color::Blue)),
            ]
        );
    }

    #[test]
    fn restyle_cursor_cell_at_start_splits_first_span() {
        let line = two_color_line();
        let cursor = Style::default().fg(Color::Yellow);
        let out = restyle_cursor_cell(&line, 0, cursor);
        assert_eq!(
            runs(&out),
            vec![
                ("a".into(), Some(Color::Yellow)),
                ("b".into(), Some(Color::Red)),
                ("cd".into(), Some(Color::Blue)),
            ]
        );
    }

    #[test]
    fn restyle_cursor_cell_past_end_appends_styled_space() {
        let line = two_color_line();
        let cursor = Style::default().fg(Color::Yellow);
        // col == len: original runs intact, plus a trailing cursor space.
        let out = restyle_cursor_cell(&line, 4, cursor);
        assert_eq!(
            runs(&out),
            vec![
                ("ab".into(), Some(Color::Red)),
                ("cd".into(), Some(Color::Blue)),
                (" ".into(), Some(Color::Yellow)),
            ]
        );
    }
}
