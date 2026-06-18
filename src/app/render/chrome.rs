//! Frame chrome: the pane status-line / divider (`render_pane_status_line`),
//! the status-bar header halves (`header_parts`), and the list-rows cache
//! builder (`build_rows`). Split from `app/render.rs` verbatim; an `impl App`
//! child module reading App's private state via the descendant-module rule.

use std::path::PathBuf;

use ratatui::Frame;

use crate::ui::list_view::Row;

use crate::app::{App, TaskStatus, View, state};

impl App {
    /// Pane status line: tab indicators, active cwd, [SCROLL] tag.
    /// Replaces the old plain-rule divider.
    pub(super) fn render_pane_status_line(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
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
            .runtime
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
                .fg(self.view.theme.dir)
                .add_modifier(Modifier::BOLD)
        } else if self.state.pane_focused() {
            Style::default()
                .fg(self.view.theme.prompt_prefix)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.view.theme.status_suffix)
        };
        // Active tab gets REVERSED modifier (background fill) so it's
        // unambiguously distinct from a background tab with activity
        // — both used to render as plain amber-bold and were only
        // differentiated by the small `*`/`+` glyph, which Spencer
        // (and probably others) read past in peripheral vision. With
        // reverse, "you are here" registers before glyph parsing.
        let active_tab_style = if is_scrolling {
            Style::default()
                .fg(self.view.theme.dir)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default()
                .fg(self.view.theme.prompt_prefix)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        };
        let inactive_tab_style = Style::default().fg(self.view.theme.status_suffix);

        let mut spans: Vec<Span> = Vec::new();
        let mut used = 0usize;

        let activity_style = Style::default()
            .fg(self.view.theme.pick)
            .add_modifier(Modifier::BOLD);

        // Tab indicators: ─[1*] claude ─[2+] bash, then "── <live cwd>".
        // We render the indicators first (immutable iter) and capture the
        // active index, then read the active tab's live cwd below — a pure
        // `&self` `live_cwd()` cache read (the refresh kick moved to
        // `prepare_panes`, #347), so no `&mut` re-borrow is involved.
        let mut active_idx: Option<usize> = None;
        if let Some(tabs) = &self.runtime.pane_tabs {
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
                // Measure in display columns, not bytes — `sep` ("─") is 3
                // bytes but 1 column, and a label can carry multibyte chars;
                // `used`/`width` are column budgets.
                let tab_len = crate::ui::display_width(sep) + crate::ui::display_width(&tab_text);
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

        if let (Some(idx), Some(tabs)) = (active_idx, self.runtime.pane_tabs.as_ref()) {
            let entry = &tabs.tabs()[idx];
            let live = entry.live_cwd();
            let cwd_display = crate::paths::display_tilde(&live);
            // Mark when the live cwd has drifted from the spawn cwd
            // (e.g. user `cd`'d in a bash tab). Helps spot the case
            // the bug list called out.
            let drift = live != entry.info.cwd;
            let cwd_prefix = if drift { "── ↪ " } else { "── " };
            let avail = width.saturating_sub(used + 12); // room for [SCROLL] + trailing rule
            if avail > 4 {
                // Truncate by display columns on a char boundary, keeping the
                // tail (the leaf dirs). A byte slice here panics on a
                // non-ASCII cwd ("résumé", "日本語") when the cut lands
                // mid-codepoint.
                let truncated = if crate::ui::display_width(&cwd_display) > avail {
                    format!(
                        "…{}",
                        crate::ui::display_truncate_tail(&cwd_display, avail.saturating_sub(1))
                    )
                } else {
                    cwd_display
                };
                let cwd_fragment = format!("{cwd_prefix}{truncated} ");
                used += crate::ui::display_width(&cwd_fragment);
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
        let bg_running_color = self.view.theme.dir; // soft blue
        let bg_unread_color = self.view.theme.take; // teal -- pulls the eye
        let bg_ok_color = self.view.theme.exec; // soft green
        let bg_err_color = ratatui::style::Color::Rgb(0xf7, 0x76, 0x8e); // tokyo red
        let mut bg_pieces_rev: Vec<(String, ratatui::style::Color)> = Vec::new();
        let mut bg_width = 0usize;
        // Only `BottomPane` zoom keeps a divider (this line) to tag; `TopList`
        // zoom has no divider — its cue rides the status-bar suffix instead
        // (see `header_parts`).
        let zoom_tag = if self.state.pane.zoom == state::ZoomTarget::BottomPane {
            " [ZOOM]"
        } else {
            ""
        };
        let scroll_tag = if is_scrolling { " [SCROLL]" } else { "" };
        let tag_len = crate::ui::display_width(zoom_tag) + crate::ui::display_width(scroll_tag);
        // Reserve room for at least 4 dashes + the tag(s).
        let bg_budget = width.saturating_sub(used + tag_len + 4);
        for task in self.runtime.background_tasks.tasks.iter().rev() {
            let (glyph, color) = if task.paused && matches!(task.status, TaskStatus::Running) {
                // Pause glyph trumps the running/unread variants:
                // user explicitly paused, that's the headline state.
                ("\u{23f8}", bg_running_color) // ⏸
            } else {
                match (&task.status, task.has_unread_output) {
                    (TaskStatus::Running, true) => ("+", bg_unread_color),
                    (TaskStatus::Running, false) => ("\u{25cf}", bg_running_color),
                    (TaskStatus::Exited(0), _) => ("\u{2713}", bg_ok_color),
                    (TaskStatus::Exited(_) | TaskStatus::Crashed(_), _) => {
                        ("\u{2717}", bg_err_color)
                    }
                }
            };
            let text = format!(" [{}{glyph}]", task.id);
            // Column width, not bytes — the status glyphs (●✓✗⏸) are 3-byte,
            // 1-column chars.
            let text_w = crate::ui::display_width(&text);
            if bg_width + text_w > bg_budget {
                break;
            }
            bg_width += text_w;
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
            used += crate::ui::display_width(&text);
            spans.push(Span::styled(
                text,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }

        if self.state.pane.zoom == state::ZoomTarget::BottomPane {
            spans.push(Span::styled(
                zoom_tag,
                Style::default()
                    .fg(self.view.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ));
            used += crate::ui::display_width(zoom_tag);
        }
        if is_scrolling {
            spans.push(Span::styled(
                scroll_tag,
                Style::default()
                    .fg(self.view.theme.dir)
                    .add_modifier(Modifier::BOLD),
            ));
            used += crate::ui::display_width(scroll_tag);
        }
        // If anything's left (shouldn't be), pad.
        let _ = used;

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    /// Status-bar header: the left (path / view name) and right
    /// (status tags) halves of the top line, per current view.
    pub(super) fn header_parts(&self) -> (String, String) {
        match self.state.view {
            View::Dir => (crate::paths::display_tilde(&self.state.listing.dir), {
                let filter_tag = match &self.state.temp_filter {
                    Some(f) if f == "!" => " limit:picks".to_string(),
                    Some(f) => format!(" limit:{f}"),
                    None => String::new(),
                };
                {
                    let total = self.state.listing.entries.len();
                    let shown = self.state.rows.len();
                    let hidden = total.saturating_sub(shown);
                    let hidden_tag = format!(" hidden:{hidden}");
                    // Bg tasks normally render in the divider line above
                    // the pane (distinct color, right-aligned). When the
                    // pane is hidden there is no divider, so fall back
                    // to the status-bar suffix here.
                    let bg_tag = if self.runtime.pane_tabs.is_some() {
                        String::new()
                    } else {
                        let running = self.runtime.background_tasks.running_count();
                        let done = self.runtime.background_tasks.done_count();
                        if running == 0 && done == 0 {
                            String::new()
                        } else if done == 0 {
                            format!(" bg:{running}\u{25cf}")
                        } else {
                            format!(" bg:{running}\u{25cf}{done}\u{2713}")
                        }
                    };
                    let sort_tag = format!(
                        " sort:{}{}",
                        self.state.sort_order,
                        if self.state.sort_reversed {
                            "\u{2191}"
                        } else {
                            ""
                        },
                    );
                    let suffix = format!(
                        "[picks:{} inv:{} m1:{} m2:{}{}{}{}{}]",
                        self.state.picks.len(),
                        self.state.inventory.len(),
                        on_off(self.state.masks.mask1.enabled),
                        on_off(self.state.masks.mask2.enabled),
                        filter_tag,
                        hidden_tag,
                        sort_tag,
                        bg_tag,
                    );
                    // `TopList` zoom collapses the pane (no divider), so its
                    // zoom cue can't ride the pane divider like `BottomPane`'s
                    // does — surface it here, the same fallback the bg-task
                    // tag uses when there's no divider.
                    if self.state.pane.zoom == state::ZoomTarget::TopList {
                        format!("{suffix} [ZOOM]")
                    } else {
                        suffix
                    }
                }
            }),
            View::Inventory => (
                "<INVENTORY>".to_string(),
                format!(
                    "[{} items{}]  (t: tag, p: put, x: remove, ESC: return)",
                    self.state.inventory.len(),
                    if self.state.inventory.picks.is_empty() {
                        String::new()
                    } else {
                        format!(", {} tagged", self.state.inventory.picks.len())
                    }
                ),
            ),
            View::Graveyard => (
                "<GRAVEYARD>".to_string(),
                format!(
                    "[{} item(s)]  (p: put cwd, P: restore orig, dd/x: trash, Z: trash all, ESC: return)",
                    self.state.graveyard.len()
                ),
            ),
        }
    }

    pub(super) fn build_rows(&self) -> Vec<Row> {
        use crate::ui::list_view::GitFileStatus;
        let delete_preview: Option<&Vec<PathBuf>> = self.state.pending_delete_preview.as_ref();
        self.state
            .rows
            .iter()
            .map(|rd| {
                let git_status = self
                    .state
                    .git
                    .files
                    .get(&rd.display)
                    .copied()
                    .unwrap_or_else(GitFileStatus::clean);
                let pending_delete =
                    delete_preview.is_some_and(|v| v.iter().any(|p| p == &rd.path));
                Row {
                    display: rd.display.clone(),
                    kind: rd.kind,
                    picked: self.state.view == View::Dir && self.state.picks.contains(&rd.path),
                    taken: self.state.inventory.contains(&rd.path),
                    git_status,
                    pending_delete,
                }
            })
            .collect()
    }
}

const fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}
