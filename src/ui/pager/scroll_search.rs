//! `PagerView` scrolling, search, and view toggles: line/page math, scroll
//! clamping + jumps, position indicator, incremental `/` search, and the
//! whitespace/wrap/markdown toggles. Split from `pager` verbatim.

use super::{PagerView, Search, VisualKind};

use super::layout::{partition_lines_static, visual_rows};

/// The footer prompt sigil for a search's direction: `?` backward, `/` forward.
const fn search_sigil(backward: bool) -> char {
    if backward { '?' } else { '/' }
}

impl PagerView {
    pub const fn toggle_whitespace(&mut self) {
        self.show_whitespace = !self.show_whitespace;
    }

    pub const fn toggle_wrap(&mut self) {
        self.wrap = !self.wrap;
    }

    /// Toggle Markdown rendered ↔ source view. No-op (returns false)
    /// if this view doesn't have an alternate buffer (i.e. wasn't
    /// opened on a `.md`/`.markdown` file).
    ///
    /// **Scroll preservation:** the two views have different line
    /// counts (one rendered line ≠ one source line) so a literal
    /// scroll-index carryover would land arbitrarily. Instead we
    /// remember each side's last scroll position in `saved_alt_scroll`
    /// and restore it when the user comes back. The first time a
    /// view is visited there's no memory yet, so we fall back to a
    /// proportional projection of the departing scroll — close to
    /// the right neighborhood, never worse than the old "always
    /// reset to top" behavior.
    pub fn toggle_markdown(&mut self) -> bool {
        let Some(alt) = self.alt_lines.take() else {
            return false;
        };
        let old_scroll = self.scroll;
        let old_total = self.lines.len();
        let new_total = alt.len();
        let current = std::mem::replace(&mut self.lines, alt);
        self.alt_lines = Some(current);
        self.markdown_rendered = !self.markdown_rendered;

        let restored = self.saved_alt_scroll.take().unwrap_or_else(|| {
            // First visit: project proportionally so a user halfway
            // down the source lands halfway down the rendered view
            // (and vice versa). Bottom of one side maps to bottom of
            // the other. u128 for the multiply so the product of two
            // usize line indices can't overflow.
            if old_total <= 1 || new_total == 0 {
                0
            } else {
                let num = old_scroll as u128 * (new_total - 1) as u128;
                let denom = (old_total - 1) as u128;
                usize::try_from(num / denom).unwrap_or(usize::MAX)
            }
        });
        let max_index = new_total.saturating_sub(1);
        self.scroll = restored.min(max_index);
        self.saved_alt_scroll = Some(old_scroll);
        true
    }

    pub const fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Lines visible per "page" — viewport_height * columns.
    pub fn page_lines(&self, viewport_height: u16) -> usize {
        viewport_height as usize * self.columns.max(1) as usize
    }

    /// Maximum useful `scroll` value for the current layout. In multi-col
    /// the static partition means each column has its own chunk; the
    /// visible range is capped by the longest chunk minus viewport_h.
    /// In single-col, the obvious answer is `lines - viewport_h`, but
    /// that's wrong when `wrap` is on and lines exceed `body_w` —
    /// each wrapped line consumes multiple visual rows, and stopping
    /// at logical-line distance `viewport_h` from the end leaves the
    /// trailing lines invisible (the renderer fills the viewport with
    /// the wrapped portions of earlier lines and runs out of space
    /// before reaching them). When wrap is on and we have a cached
    /// `body_w` from the most recent render, we walk lines from the
    /// end summing visual rows; max_scroll = the highest logical line
    /// index whose inclusion still fits the viewport.
    pub fn scroll_max(&self, viewport_height: u16) -> usize {
        if self.columns.max(1) as usize > 1 {
            // Multi-col: bound by the longest column chunk. Wrap is irrelevant
            // here because multi-col is only used for pickers (find finder,
            // task viewer) where wrap is off. No EOF reservation (per-column
            // `[EOF]` is handled in the multi-col render).
            return Self::multi_col_max_scroll(
                &partition_lines_static(&self.lines, self.columns.max(1) as usize),
                viewport_height,
            );
        }
        let base = self.scroll_max_content(viewport_height);
        // Reserve one extra row at the true bottom so the `[EOF]` / `~` end
        // marker is reachable for files TALLER than the viewport — short files
        // (`base == 0`) already show it because there's spare room; this gives
        // long files the same end-of-file signal (the doc scrolls one row past
        // the last line, like vim/less). Skipped while streaming (no marker
        // yet) or when `[EOF]` is already a content line (`eof_in_content`).
        if base > 0 && !self.streaming && !self.eof_in_content {
            base.saturating_add(1)
        } else {
            base
        }
    }

    /// The greatest scroll that pins the last line to the bottom row (the
    /// viewport fills exactly, no room for an end marker). [`Self::scroll_max`]
    /// adds the `[EOF]` reservation on top of this.
    fn scroll_max_content(&self, viewport_height: u16) -> usize {
        let vh = viewport_height as usize;
        let logical_max = self.lines.len().saturating_sub(vh);
        let body_w = self.last_body_w.get() as usize;
        if !self.wrap || body_w == 0 || viewport_height == 0 {
            return logical_max;
        }
        // Walk from the end backwards, accumulating visual rows.
        // The first logical line index `i` whose visual-row sum
        // (including itself) reaches `viewport_height` is the
        // greatest scroll value that still keeps the last line
        // visible: starting from `i`, the renderer fills exactly
        // viewport_h rows ending at the document's last line.
        let mut acc = 0usize;
        for (i, line) in self.lines.iter().enumerate().rev() {
            acc = acc.saturating_add(visual_rows(line, body_w));
            if acc >= vh {
                return i;
            }
        }
        // Whole document fits in the viewport — no scrolling needed.
        0
    }

    fn clamp_scroll(&mut self, viewport_height: u16) {
        let max_scroll = self.scroll_max(viewport_height);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    pub fn scroll_by(&mut self, delta: i32, viewport_height: u16) {
        let current = self.scroll as i64;
        let new = (current + i64::from(delta)).max(0);
        self.scroll = usize::try_from(new).unwrap_or(usize::MAX);
        self.clamp_scroll(viewport_height);
    }

    pub const fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }

    pub fn scroll_to_bottom(&mut self, viewport_height: u16) {
        self.scroll = self.scroll_max(viewport_height);
    }

    /// Scroll-to-bottom using the viewport height the most recent
    /// render observed (cached in `last_viewport_h`). For
    /// streaming-capture auto-tail: the tick loop appends new
    /// output and wants to keep showing the latest, but it doesn't
    /// have direct access to terminal geometry. Falls back to a
    /// 40-row guess when nothing's been rendered yet (first frame).
    pub fn scroll_to_bottom_auto(&mut self) {
        let h = self.last_viewport_h.get();
        let h = if h == 0 { 40 } else { h };
        self.scroll_to_bottom(h);
    }

    /// Clamp `scroll` to the document end using the viewport height the most
    /// recent render observed (`last_viewport_h`), falling back to a 40-row
    /// guess before the first frame. Mirrors [`Self::scroll_to_bottom_auto`].
    ///
    /// Call after any wholesale `lines` replacement (e.g. the git-view `|`
    /// layout toggle) or absolute `scroll` jump (e.g. `:N`) that doesn't
    /// itself clamp — a stale `scroll` left past the new end renders nothing,
    /// blanking the viewport.
    pub fn clamp_scroll_auto(&mut self) {
        let h = self.last_viewport_h.get();
        let h = if h == 0 { 40 } else { h };
        self.clamp_scroll(h);
    }

    /// Greatest scroll offset for a multi-column layout: the longest column
    /// chunk (each column scrolls independently within its chunk) minus the
    /// viewport. Split out so the render pass can pass a partition it already
    /// computed instead of re-partitioning (see [`Self::position_indicator_multi`]).
    fn multi_col_max_scroll(chunks: &[(usize, usize)], viewport_height: u16) -> usize {
        let longest = chunks.iter().map(|(s, e)| e - s).max().unwrap_or(0);
        longest.saturating_sub(viewport_height as usize)
    }

    /// The "Top" / "Bot" / "All" / "NN%" label from a scroll position and its
    /// max. Shared by the single- and multi-column indicator paths.
    fn indicator_string(scroll: usize, max_scroll: usize) -> String {
        if max_scroll == 0 {
            return "All".to_string();
        }
        if scroll == 0 {
            return "Top".to_string();
        }
        if scroll >= max_scroll {
            return "Bot".to_string();
        }
        let pct = (scroll as u64 * 100) / max_scroll as u64;
        format!("{pct}%")
    }

    /// Position indicator: "Top", "Bot", "All", or "NN%".
    /// Percentage is based on scroll progress through the "effective"
    /// document length — in multi-col that's the longest chunk, not the
    /// total line count, since each column's chunk scrolls independently.
    pub fn position_indicator(&self, viewport_height: u16) -> String {
        Self::indicator_string(self.scroll, self.scroll_max(viewport_height))
    }

    /// Like [`Self::position_indicator`] but for a multi-column layout whose
    /// partition the caller already holds — avoids re-partitioning. The render
    /// pass computes the partition once and shares it across the body layout
    /// and this indicator (previously each re-ran `partition_lines_static`).
    pub(super) fn position_indicator_multi(
        &self,
        chunks: &[(usize, usize)],
        viewport_height: u16,
    ) -> String {
        Self::indicator_string(
            self.scroll,
            Self::multi_col_max_scroll(chunks, viewport_height),
        )
    }

    // ---- Search ----------------------------------------------------------

    /// True when the pager is capturing text input for a `/` or `?` search.
    pub const fn is_typing_search(&self) -> bool {
        matches!(self.search, Search::Typing { .. })
    }

    /// Begin a forward (`/`) search: subsequent chars build the query and
    /// Enter commits it, landing on the first match at or *below* the current
    /// scroll (wrapping). `n` then repeats downward, `N` upward.
    pub fn begin_search(&mut self) {
        self.search = Search::Typing {
            query: String::new(),
            backward: false,
        };
    }

    /// Begin a backward (`?`) search: like [`Self::begin_search`] but the
    /// commit lands on the first match at or *above* the current scroll, and
    /// `n` repeats upward, `N` downward.
    pub fn begin_search_backward(&mut self) {
        self.search = Search::Typing {
            query: String::new(),
            backward: true,
        };
    }

    /// Append a char to the search buffer (only meaningful while typing).
    pub fn search_push_char(&mut self, c: char) {
        if let Search::Typing { query, .. } = &mut self.search {
            query.push(c);
        }
    }

    pub fn search_backspace(&mut self) {
        if let Search::Typing { query, .. } = &mut self.search {
            query.pop();
        }
    }

    /// Cancel an in-progress search and clear any active match state.
    pub fn cancel_search(&mut self) {
        self.search = Search::Off;
    }

    /// Commit the typed query: find matching lines, then jump to the match
    /// nearest the current scroll in the search's direction (forward → first
    /// at/below, backward → first at/above; each wraps). No matches → revert
    /// to Off and return false so the caller can flash.
    pub fn commit_search(&mut self, viewport_height: u16) -> bool {
        let (query, backward) = match std::mem::replace(&mut self.search, Search::Off) {
            Search::Typing { query, backward } => (query, backward),
            other => {
                self.search = other;
                return true;
            }
        };
        if query.is_empty() {
            return true;
        }
        let needle = query.to_lowercase();
        // Reuse one plain-text scratch buffer across the whole buffer rather
        // than allocating a fresh `line_plain_text` String per line. (The
        // lowercase still allocates once per line — `to_lowercase` is kept
        // for its exact, full-string casing semantics — so this is 1 alloc
        // per line instead of 2, which matters on a large file pager where a
        // single search commit scans every line.)
        let mut plain = String::new();
        let matches: Vec<usize> = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                plain.clear();
                for span in &line.spans {
                    plain.push_str(&span.content);
                }
                plain.to_lowercase().contains(&needle)
            })
            .map(|(i, _)| i)
            .collect();
        if matches.is_empty() {
            return false;
        }
        // Land relative to the current scroll, honoring the search direction
        // (rather than always snapping to the top of the file).
        let cursor = Self::select_match(&matches, self.scroll, backward);
        self.scroll_to_match(matches[cursor], viewport_height);
        self.search = Search::Active {
            query,
            matches,
            cursor,
            backward,
        };
        true
    }

    /// Pick the match-list cursor for a freshly committed search, relative to
    /// the scroll `anchor` and direction. Forward → the first match at or
    /// after `anchor`; backward → the last match at or before it. Each wraps
    /// to the far end when nothing lies on the requested side.
    ///
    /// `matches` must be sorted ascending and non-empty (`commit_search`
    /// guarantees both — it collects in line order and returns early when
    /// empty).
    pub(super) fn select_match(matches: &[usize], anchor: usize, backward: bool) -> usize {
        if backward {
            matches
                .iter()
                .rposition(|&m| m <= anchor)
                .unwrap_or(matches.len() - 1)
        } else {
            matches.iter().position(|&m| m >= anchor).unwrap_or(0)
        }
    }

    /// Step the active-search cursor by one match in match-index order
    /// (wraps). `forward` true → next index, false → previous. No-op unless a
    /// search is active.
    fn step_match(&mut self, forward: bool, viewport_height: u16) {
        let Search::Active {
            matches, cursor, ..
        } = &mut self.search
        else {
            return;
        };
        if matches.is_empty() {
            return;
        }
        *cursor = if forward {
            (*cursor + 1) % matches.len()
        } else if *cursor == 0 {
            matches.len() - 1
        } else {
            *cursor - 1
        };
        let line_idx = matches[*cursor];
        self.scroll_to_match(line_idx, viewport_height);
    }

    /// True when the active search was initiated backward (`?`).
    const fn search_is_backward(&self) -> bool {
        matches!(self.search, Search::Active { backward: true, .. })
    }

    /// Advance to the next match in match-index order (wraps). The picker
    /// pagers bind `n`/`N` straight to this / [`Self::search_prev`]; their
    /// search is always forward, so direction-awareness isn't needed.
    pub fn search_next(&mut self, viewport_height: u16) {
        self.step_match(true, viewport_height);
    }

    /// Step to the previous match in match-index order (wraps).
    pub fn search_prev(&mut self, viewport_height: u16) {
        self.step_match(false, viewport_height);
    }

    /// Repeat the active search in its initiating direction (`n`): downward
    /// for a `/` search, upward for a `?` search.
    pub fn search_repeat(&mut self, viewport_height: u16) {
        let forward = !self.search_is_backward();
        self.step_match(forward, viewport_height);
    }

    /// Repeat the active search against its initiating direction (`N`).
    pub fn search_repeat_opposite(&mut self, viewport_height: u16) {
        let forward = self.search_is_backward();
        self.step_match(forward, viewport_height);
    }

    /// Returns the line index of the current search match, if any.
    pub fn current_match_line(&self) -> Option<usize> {
        if let Search::Active {
            matches, cursor, ..
        } = &self.search
        {
            matches.get(*cursor).copied()
        } else {
            None
        }
    }

    /// Scroll the viewport so `line_idx` is roughly a third of the way
    /// down — gives context above and more content below.
    ///
    /// In multi-column mode `scroll` is interpreted per-column (each
    /// column applies the same offset within its own chunk), so a
    /// match in column 2+ has to be translated to a chunk-local
    /// offset before being assigned to `self.scroll` — otherwise the
    /// global line index gets clamped to `scroll_max` (= longest
    /// chunk minus viewport_h) and every column pins to the bottom
    /// of its chunk, hiding the match. Symptom: `/show` then `n n n`
    /// in the help pager left the view stuck at the bottom.
    fn scroll_to_match(&mut self, line_idx: usize, viewport_height: u16) {
        let third = i64::from(viewport_height) / 3;
        let ncols = self.columns.max(1) as usize;
        let local_idx = if ncols > 1 {
            partition_lines_static(&self.lines, ncols)
                .into_iter()
                .find(|(s, e)| (*s..*e).contains(&line_idx))
                .map_or(line_idx, |(s, _)| line_idx - s)
        } else {
            line_idx
        };
        let target = local_idx as i64 - third;
        let scroll = target.max(0);
        self.scroll = usize::try_from(scroll).unwrap_or(usize::MAX);
        self.clamp_scroll(viewport_height);
    }

    /// For the render layer: is the given line index one of the search
    /// matches? (Returns (is_match, is_current_match).)
    pub(super) fn match_state(&self, line_idx: usize) -> (bool, bool) {
        match &self.search {
            Search::Active {
                matches, cursor, ..
            } => (
                matches.binary_search(&line_idx).is_ok(),
                matches.get(*cursor) == Some(&line_idx),
            ),
            _ => (false, false),
        }
    }

    /// Current search status for the footer line (e.g. `/foo 3/17`).
    pub(super) fn status_text(&self) -> Option<String> {
        if let Some(sel) = self.visual {
            let (lo, hi) = sel.range();
            let count = hi - lo + 1;
            return Some(match sel.kind {
                VisualKind::Line => format!(
                    "-- VISUAL --  L{}-L{}  ({count} line{})",
                    lo + 1,
                    hi + 1,
                    if count == 1 { "" } else { "s" },
                ),
                VisualKind::Block => {
                    let (lo_col, hi_col) = sel.col_range();
                    let cols = hi_col - lo_col + 1;
                    format!(
                        "-- VISUAL BLOCK --  L{}-L{} C{}-C{}  ({count}×{cols})",
                        lo + 1,
                        hi + 1,
                        lo_col + 1,
                        hi_col + 1,
                    )
                }
            });
        }
        if let Some(ref buf) = self.jump_buf {
            return Some(format!(":{buf}_"));
        }
        match &self.search {
            Search::Off => None,
            Search::Typing { query, backward } => {
                Some(format!("{}{query}_", search_sigil(*backward)))
            }
            Search::Active {
                query,
                matches,
                cursor,
                backward,
            } => Some(format!(
                "{}{query}  {}/{}",
                search_sigil(*backward),
                cursor + 1,
                matches.len()
            )),
        }
    }
}
