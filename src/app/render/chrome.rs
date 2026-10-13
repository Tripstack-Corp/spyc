//! Frame chrome: the pane status-line / divider (`render_pane_status_line`),
//! the status-bar header halves (`header_parts`), and the list-rows cache
//! builder (`build_rows`). Split from `app/render.rs` verbatim; an `impl App`
//! child module reading App's private state via the descendant-module rule.

use std::path::PathBuf;

use ratatui::Frame;

use crate::ui::list_view::Row;
use crate::ui::status_flags;

use crate::app::{App, TaskStatus, View, state};

/// A listing row's pending-archive-change marker, mapped onto the gutter's
/// vocabulary. `None` when the row isn't a member of a mount, or nothing about it
/// is pending — which is every row outside an archive.
fn archive_change(
    mounts: &crate::archive::Mounts,
    path: &std::path::Path,
) -> Option<crate::ui::list_view::GitChange> {
    use crate::archive::MemberChange;
    use crate::ui::list_view::GitChange;
    // The archive *file*, seen from the directory it lives in: an unwritten change
    // inside it is invisible out here otherwise — the suffix badge only shows while
    // you're standing in the mount, and the members aren't listed.
    if let Some(mount) = mounts.get(path) {
        return mount.journal.is_dirty().then_some(GitChange::Modified);
    }
    let (mount, _) = mounts.member_of(path)?;
    let entry = mount.entry_at(path)?;
    Some(match mount.journal.state_of(&entry.inner)? {
        MemberChange::Added => GitChange::Added,
        MemberChange::Replaced => GitChange::Modified,
        MemberChange::Renamed => GitChange::Renamed,
    })
}

/// The divider's styles: its rule, the active tab, the other tabs, and a
/// shell tab with output the user hasn't seen.
struct DividerStyles {
    rule: ratatui::style::Style,
    active: ratatui::style::Style,
    inactive: ratatui::style::Style,
    unseen: ratatui::style::Style,
}

/// The active tab's live cwd, `↪`-marked when it has drifted from where the
/// tab started (e.g. a `cd` in a shell tab), in the `room` columns left after
/// the tabs. The mode tags (`tags_width` columns) come after it, so the cwd
/// gives way to them: its tail, the leaf directories, is kept when it's too
/// long, and it's left off when there's no room.
fn cwd_span(
    tabs: &crate::pane::tabs::PaneTabs,
    styles: &DividerStyles,
    room: usize,
    tags_width: usize,
) -> Option<ratatui::text::Span<'static>> {
    let entry = &tabs.tabs()[tabs.active_index()];
    let live = entry.live_cwd();
    let cwd_display = crate::paths::display_tilde(&live);
    let drift = live != entry.info.cwd;
    let cwd_prefix = if drift { "── ↪ " } else { "── " };
    // Its own prefix and trailing space, then a dash of rule before the tags,
    // or the 12 columns a divider without tags keeps clear, whichever is more.
    let reserve = (crate::ui::display_width(cwd_prefix) + 1 + 1 + tags_width).max(12);
    let avail = room.saturating_sub(reserve);
    if avail <= 4 {
        return None;
    }
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
    let style = if drift {
        styles.active
    } else {
        styles.inactive
    };
    Some(ratatui::text::Span::styled(
        format!("{cwd_prefix}{truncated} "),
        style,
    ))
}

/// How a background task's divider tag is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskTone {
    Running,
    Unread,
    Ok,
    Failed,
}

/// A background task's divider glyph: `⏸` paused, `+` running with output
/// arrived since the last `:fg`, `●` running quietly, `✓` exited cleanly, `✗`
/// a non-zero exit, a kill or a crash.
const fn task_glyph(status: &TaskStatus, paused: bool, unread: bool) -> (&'static str, TaskTone) {
    match (status, unread) {
        // Pausing is the user's explicit act, so it outranks unread output.
        (TaskStatus::Running, _) if paused => ("\u{23f8}", TaskTone::Running),
        (TaskStatus::Running, true) => ("+", TaskTone::Unread),
        (TaskStatus::Running, false) => ("\u{25cf}", TaskTone::Running),
        (TaskStatus::Exited(0), _) => ("\u{2713}", TaskTone::Ok),
        (TaskStatus::Exited(_) | TaskStatus::Crashed(_), _) => ("\u{2717}", TaskTone::Failed),
    }
}

/// The task tags that fit in `budget` columns, given oldest first. They're
/// right-anchored, so when they don't all fit the oldest are dropped; the ones
/// kept come back oldest first.
fn fit_task_tags(
    tags: impl DoubleEndedIterator<Item = (String, TaskTone)>,
    budget: usize,
) -> Vec<(String, TaskTone)> {
    let mut kept = Vec::new();
    let mut used = 0;
    for (text, tone) in tags.rev() {
        // Column width, not bytes — the status glyphs (●✓✗⏸) are 3-byte,
        // 1-column chars.
        let width = crate::ui::display_width(&text);
        if used + width > budget {
            break;
        }
        used += width;
        kept.push((text, tone));
    }
    kept.reverse();
    kept
}

impl App {
    /// Pane status line: tab indicators, active cwd, [SCROLL] tag.
    /// Replaces the old plain-rule divider.
    pub(super) fn render_pane_status_line(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        use ratatui::{
            style::{Modifier, Style},
            text::{Line, Span},
        };
        let width = area.width as usize;
        let is_scrolling = self
            .runtime
            .pane_tabs
            .as_ref()
            .is_some_and(|t| t.active().is_scrolling());
        let styles = self.divider_styles(is_scrolling);
        // Only `BottomPane` zoom keeps a divider (this line) to tag; `TopList`
        // zoom has no divider — its cue rides the status-bar suffix instead
        // (see `header_parts`).
        let zoomed = self.state.pane.zoom == state::ZoomTarget::BottomPane;
        let zoom_tag = if zoomed { " [ZOOM]" } else { "" };
        let scroll_tag = if is_scrolling { " [SCROLL]" } else { "" };
        let tag_len = crate::ui::display_width(zoom_tag) + crate::ui::display_width(scroll_tag);
        let mut spans: Vec<Span> = Vec::new();
        let mut used = 0usize;
        if let Some(tabs) = &self.runtime.pane_tabs {
            let active_shown;
            (used, active_shown) =
                self.push_tab_spans(&mut spans, tabs, &styles, is_scrolling, area.width);
            // The active tab's cwd follows its tab, so it shows only when that
            // tab fitted.
            if active_shown
                && let Some(cwd) = cwd_span(tabs, &styles, width.saturating_sub(used), tag_len)
            {
                used += crate::ui::display_width(cwd.content.as_ref());
                spans.push(cwd);
            }
        }

        // Background-task tags, right-aligned. Distinct colour from pane tabs
        // so the numbering doesn't visually collide (pane tabs are 1..N
        // left-to-right; bg tasks are 1..N right-anchored). Reserve room for at
        // least 4 dashes + the mode tag(s).
        let tasks = self.runtime.background_tasks.tasks.iter().map(|task| {
            let (glyph, tone) = task_glyph(&task.status, task.paused, task.has_unread_output);
            (format!(" [{}{glyph}]", task.id), tone)
        });
        let task_tags = fit_task_tags(tasks, width.saturating_sub(used + tag_len + 4));
        let tags_width: usize = task_tags
            .iter()
            .map(|(text, _)| crate::ui::display_width(text))
            .sum();

        // Dash fill between pane-tab area and bg group / mode tag(s).
        let fill = width.saturating_sub(used + tag_len + tags_width);
        if fill > 0 {
            spans.push(Span::styled("─".repeat(fill), styles.rule));
        }
        for (text, tone) in task_tags {
            let color = match tone {
                TaskTone::Running => self.view.theme.dir, // soft blue
                TaskTone::Unread => self.view.theme.take, // teal -- pulls the eye
                TaskTone::Ok => self.view.theme.exec,     // soft green
                TaskTone::Failed => ratatui::style::Color::Rgb(0xf7, 0x76, 0x8e), // tokyo red
            };
            spans.push(Span::styled(
                text,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }
        if zoomed {
            spans.push(Span::styled(
                zoom_tag,
                Style::default()
                    .fg(self.view.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        if is_scrolling {
            spans.push(Span::styled(
                scroll_tag,
                Style::default()
                    .fg(self.view.theme.dir)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        self.draw_chrome_line(frame, area, Line::from(spans));
    }

    /// The divider's styles. Tinting the rule + active tab in scroll mode is
    /// deliberate redundancy with the [SCROLL] tag — three signals in different
    /// parts of the divider make "you've left live view" hard to miss.
    fn divider_styles(&self, is_scrolling: bool) -> DividerStyles {
        use ratatui::style::{Modifier, Style};
        // Scroll mode flips the rule + active-tab colour to blue
        // (theme.dir) so "you're in scrollback" is unambiguous from
        // peripheral vision. Amber stays the "focus" colour for live
        // mode; blue is reserved for scrollback and unused elsewhere
        // as a UI signal. The `[SCROLL]` tag uses the same
        // colour, plus the active tab's label stays uppercased (shape
        // cue independent of colour).
        let rule = if is_scrolling {
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
        // differentiated by the small `*`/`+` glyph, which users
        // read past in peripheral vision. With
        // reverse, "you are here" registers before glyph parsing.
        let active_colour = if is_scrolling {
            self.view.theme.dir
        } else {
            self.view.theme.prompt_prefix
        };
        DividerStyles {
            rule,
            active: Style::default()
                .fg(active_colour)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            inactive: Style::default().fg(self.view.theme.status_suffix),
            unseen: Style::default()
                .fg(self.view.theme.pick)
                .add_modifier(Modifier::BOLD),
        }
    }

    /// Push each tab's indicator — `─[N]`, one status cell, its label — for as
    /// many tabs as fit. Returns the columns used and whether the active tab
    /// was among them.
    fn push_tab_spans(
        &self,
        spans: &mut Vec<ratatui::text::Span<'static>>,
        tabs: &crate::pane::tabs::PaneTabs,
        styles: &DividerStyles,
        is_scrolling: bool,
        bar_width: u16,
    ) -> (usize, bool) {
        use ratatui::style::Style;
        use ratatui::text::Span;
        let tab_layout = super::super::mouse::tab_hit::tab_layout(tabs, is_scrolling, bar_width);
        let mut used = 0usize;
        let mut active_shown = false;
        for (i, entry) in tabs.tabs().iter().enumerate() {
            let is_active = i == tabs.active_index();
            let sep = "─";
            // FIXED-WIDTH indicator so the divider never reflows as statuses
            // change: every tab is `[N]` + exactly one status cell + label.
            // The active tab is marked by reverse-video (`styles.active`),
            // not a `*` glyph — that glyph was redundant with the highlight
            // AND made the active tab one column wider (a focus-switch jump).
            // The single cell is: the coloured activity dot for an agent tab
            // (one with a resolved activity), else `+` for a shell tab with
            // unseen output, else a blank space (still reserved, so a shell's
            // `+` flicking on/off — e.g. htop — no longer changes the width).
            let is_agent = entry.info.activity != crate::pane::AgentActivity::Unknown;
            let suspended = entry.info.suspended;
            let bracket = format!("[{}]", i + 1);
            // Suspended (`^z`) → 💤, overriding the activity dot; else an
            // agent's own coloured dot span; else a shell's `+`/blank cell.
            // 💤 is 2 cols wide, so a suspended tab is 1 col wider than its
            // running self — acceptable for a deliberate, sticky toggle
            // (unlike the per-frame flicker the fixed-width cell prevents).
            let dot = if suspended {
                Some(Span::styled(
                    "\u{1F4A4}", // 💤
                    Style::default().fg(self.view.theme.status_suffix),
                ))
            } else if is_agent {
                self.agent_activity_span(entry.info.activity, entry.info.anim_phase_offset)
            } else {
                None
            };
            let shell_cell = if is_agent || suspended {
                "" // the dot span fills the cell instead
            } else if entry.info.has_activity {
                "+"
            } else {
                " " // reserved blank — keeps the width fixed
            };
            // Label text (uppercased in scroll mode, padding cropped and
            // letters shaved to fit the bar) AND width both come from
            // `tab_hit::tab_layout`, NOT recomputed here: the mouse
            // hit-test lays the bar out from that same list, and any drift
            // between the two puts clicks on the wrong tab. See the module
            // doc on `mouse::tab_hit`.
            let label_text = tab_layout
                .get(i)
                .map(|c| c.label_text.clone())
                .unwrap_or_default();
            let tab_len = tab_layout.get(i).map_or(0, |c| c.width) as usize;
            if used + tab_len > bar_width as usize {
                break;
            }
            // The painted spans must add up to the budgeted width, or the
            // bar drifts from what the hit-test believes.
            debug_assert_eq!(
                tab_len,
                crate::ui::display_width(sep)
                    + crate::ui::display_width(&bracket)
                    + dot
                        .as_ref()
                        .map_or(0, |s| crate::ui::display_width(s.content.as_ref()))
                    + crate::ui::display_width(shell_cell)
                    + crate::ui::display_width(&label_text),
                "tab {i} paints a different width than tab_layout budgeted"
            );
            spans.push(Span::styled(sep, styles.rule));
            // An agent tab's eye-pull comes from the coloured dot, so its
            // label stays calm (inactive style); a shell tab keeps the teal
            // `unseen` highlight for unseen output.
            let style = if is_active {
                styles.active
            } else if !is_agent && entry.info.has_activity {
                styles.unseen
            } else {
                styles.inactive
            };
            spans.push(Span::styled(bracket, style));
            if let Some(dot) = dot {
                spans.push(dot);
            } else {
                spans.push(Span::styled(shell_cell, style));
            }
            spans.push(Span::styled(label_text, style));
            used += tab_len;
            active_shown |= is_active;
        }
        (used, active_shown)
    }

    /// The per-tab agent-activity dot span: a spicy heat-pulse `●` while
    /// Working, then a *square* `■` (same visual weight as the dot) for the two
    /// settled "your turn" states — steady hot-red when Blocked ("needs me"),
    /// calm teal when Done ("finished, waiting for the next prompt") — a steady
    /// hollow `○` when Waiting on background work that will wake it, a quiet
    /// `·` when Idle, and `None` for a non-agent tab. Circle = live, square =
    /// stopped, hollow = not working this moment, so shape (not just colour)
    /// tells them apart.
    /// The glyph is TIGHT (no leading
    /// space) — the caller renders it right against the `[N*]` bracket so it's
    /// unambiguously that tab's, replacing the redundant `+`. PURE — reads the
    /// theme and the already-settled `agent_anim_frame` (advanced off the draw
    /// path in `settle_agent_activity`); the draw never touches the clock.
    pub(super) fn agent_activity_span(
        &self,
        activity: crate::pane::AgentActivity,
        phase_offset: u64,
    ) -> Option<ratatui::text::Span<'static>> {
        use crate::pane::AgentActivity;
        use ratatui::style::Color;
        use ratatui::style::{Modifier, Style};
        use ratatui::text::Span;
        match activity {
            AgentActivity::Unknown => None,
            AgentActivity::Idle => Some(Span::styled(
                "·",
                Style::default().fg(self.view.theme.status_suffix),
            )),
            AgentActivity::Working => Some(Span::styled(
                "\u{25cf}", // ●
                Style::default()
                    .fg(self
                        .spicy_pulse_color(self.view.agent_anim_frame.wrapping_add(phase_offset)))
                    .add_modifier(Modifier::BOLD),
            )),
            // Blocked = "needs me": a STEADY hot-red SQUARE `■` (not the pulse,
            // not a circle) so it reads as a stopped, attention-grabbing state.
            AgentActivity::Blocked => Some(Span::styled(
                "\u{25a0}", // ■
                Style::default()
                    .fg(Color::Rgb(0xF7, 0x76, 0x8E)) // hot red (matches bg-task err)
                    .add_modifier(Modifier::BOLD),
            )),
            // Done = finished a turn, waiting for the next prompt: a calm teal
            // SQUARE `■` — settled (square, not the live circle).
            AgentActivity::Done => Some(Span::styled(
                "\u{25a0}", // ■
                Style::default()
                    .fg(self.view.theme.take)
                    .add_modifier(Modifier::BOLD),
            )),
            // Waiting = not the user's turn: a steady HOLLOW circle `○`, in the
            // calm directory blue. Mono drops the colour; the shape carries it.
            AgentActivity::Waiting => Some(Span::styled(
                "\u{25cb}", // ○
                if self.view.theme.mono {
                    Style::default()
                } else {
                    Style::default().fg(self.view.theme.dir)
                },
            )),
        }
    }

    /// Map a "spicy pulse" frame to a warm pepper-heat colour: a 6-phase
    /// ping-pong (deep-red → ember → orange → spark and back) so the Working dot
    /// *breathes* like a heating pepper rather than blinking. Truecolor walks an
    /// RGB gradient; a 16/256-colour terminal alternates two warm ANSI hues; a
    /// mono theme drops to the single accent (no pulse). PURE.
    const fn spicy_pulse_color(&self, frame: u64) -> ratatui::style::Color {
        use ratatui::style::Color;
        if self.view.theme.mono {
            return self.view.theme.prompt_prefix;
        }
        if self.view.hud_truecolor {
            const PHASE: [usize; 6] = [0, 1, 2, 3, 2, 1];
            let (r, g, b) = SPICE_HEAT[PHASE[(frame % PHASE.len() as u64) as usize]];
            Color::Rgb(r, g, b)
        } else if frame.is_multiple_of(2) {
            Color::Red
        } else {
            Color::LightYellow
        }
    }

    /// P3-1 visual-bell colour: a point on spyc's spice-heat gradient
    /// (pepper→ember→orange→spark) at `frac` (0..1 around the border), swept by
    /// the animation `frame`. Truecolor walks the smooth gradient
    /// (`spice_gradient`); a 256-colour terminal still animates via a 3-colour warm
    /// cycle; mono falls back to the popup-border accent.
    pub(super) fn spice_pulse_color(&self, frac: f32, frame: u64) -> ratatui::style::Color {
        use ratatui::style::Color;
        if self.view.theme.mono {
            return self.view.theme.popup_border;
        }
        if !self.view.hud_truecolor {
            const CYCLE: [Color; 3] = [Color::Red, Color::LightRed, Color::Yellow];
            return CYCLE[(frame % 3) as usize];
        }
        let (r, g, b) = spice_gradient(frac, frame);
        Color::Rgb(r, g, b)
    }

    /// Status-bar header: the left (path / view name) and right
    /// (status tags) halves of the top line, per current view.
    pub(super) fn header_parts(&self) -> (String, status_flags::Suffix) {
        match self.state.cur().view {
            View::Dir => (
                crate::paths::display_tilde(&self.state.cur().listing.dir),
                {
                    let col = self.state.cur();
                    let hidden = col.listing.entries.len().saturating_sub(col.rows.len());
                    // Bg tasks normally render in the divider line above the
                    // pane (distinct colour, right-aligned). When the pane is
                    // hidden there is no divider, so fall back to the status-bar
                    // suffix here.
                    let bg_tag = if self.runtime.pane_tabs.is_some() {
                        None
                    } else {
                        let running = self.runtime.background_tasks.running_count();
                        let done = self.runtime.background_tasks.done_count();
                        if running == 0 && done == 0 {
                            None
                        } else if done == 0 {
                            Some(format!("bg:{running}\u{25cf}"))
                        } else {
                            Some(format!("bg:{running}\u{25cf}{done}\u{2713}"))
                        }
                    };
                    // Inside a mounted archive, say so — the path alone reads
                    // like an ordinary directory under a file, which is exactly
                    // what it is, but not obviously deliberate.
                    let archive_tag =
                        self.state
                            .mounts
                            .resolve(&col.listing.dir)
                            .map(|(mount, _)| {
                                let ro = if mount.capability.is_writable() {
                                    ""
                                } else {
                                    " ro"
                                };
                                let pending = if mount.is_dirty() {
                                    format!(" {}", mount.journal.counts().badge())
                                } else {
                                    String::new()
                                };
                                format!("{}{ro}{pending}", mount.format().label())
                            });
                    let ignore = &self.state.config.ignore_masks;
                    let mask = |on, group| status_flags::MaskFlag {
                        on,
                        default_on: crate::state::ignore::default_enabled(ignore, group),
                    };
                    let mut suffix = status_flags::suffix(
                        &status_flags::FlagState {
                            picks: col.picks.len(),
                            inventory: self.state.inventory.len(),
                            masks: [
                                mask(col.masks.mask1.enabled, 1),
                                mask(col.masks.mask2.enabled, 2),
                            ],
                            limit: col.temp_filter.as_deref(),
                            hidden,
                            sort: col.sort_order,
                            sort_reversed: col.sort_reversed,
                            archive: archive_tag.as_deref(),
                            bg: bg_tag.as_deref(),
                        },
                        self.state.status_flags(),
                    );
                    // `TopList` zoom collapses the pane (no divider), so its
                    // zoom cue can't ride the pane divider like `BottomPane`'s
                    // does — surface it here, the same fallback the bg-task tag
                    // uses when there's no divider.
                    if matches!(
                        self.state.pane.zoom,
                        state::ZoomTarget::TopList | state::ZoomTarget::RightColumn
                    ) {
                        let zoom = |s: &mut String| {
                            s.push_str(if s.is_empty() { "[ZOOM]" } else { " [ZOOM]" });
                        };
                        zoom(&mut suffix.full);
                        if let Some(short) = suffix.short.as_mut() {
                            zoom(short);
                        }
                    }
                    suffix
                },
            ),
            View::Inventory => (
                "<INVENTORY>".to_string(),
                status_flags::Suffix::fixed(format!(
                    "[{} items{}]  (t: tag, p: put, x: remove, ESC: return)",
                    self.state.inventory.len(),
                    if self.state.inventory.picks.is_empty() {
                        String::new()
                    } else {
                        format!(", {} tagged", self.state.inventory.picks.len())
                    }
                )),
            ),
            View::Graveyard => (
                "<GRAVEYARD>".to_string(),
                status_flags::Suffix::fixed(format!(
                    "[{} item(s)]  (p: put cwd, P: restore orig, dd/x: trash, Z: trash all, ESC: return)",
                    self.state.graveyard.len()
                )),
            ),
        }
    }

    /// Build the styled display rows for one commander column `c` (its
    /// `rows`/`view`/`picks`), folding in this column's own git markers plus the
    /// app-wide overlays (inventory `taken`, pending-delete highlight).
    /// Per-column so the right split list can be built the same way; `build_rows`
    /// is the left default. (Git status is per-column — each commander reads its
    /// own `git.files`, so `b` in a different repo shows its own markers, not
    /// `a`'s.)
    pub(super) fn build_rows_for(&self, c: &state::Commander) -> Vec<Row> {
        use crate::ui::list_view::GitFileStatus;
        use std::collections::HashSet;
        use std::path::Path;
        // Hoist the pending-delete paths into a set once. The naive
        // `delete_preview.iter().any(|p| p == &rd.path)` per row was O(rows ×
        // preview paths) — quadratic when you "delete picks" in a big
        // directory, where every picked path is also in the preview. An O(1)
        // set lookup per row keeps `build_rows` linear.
        let delete_set: Option<HashSet<&Path>> = self
            .state
            .pending_delete_preview
            .as_ref()
            .map(|v| v.iter().map(PathBuf::as_path).collect());
        c.rows
            .iter()
            .map(|rd| {
                // Per-column git: read THIS commander's markers, so `b` shows
                // its own repo's status, not `a`'s.
                //
                // Inside an archive the gutter carries the *archive's* pending
                // changes instead. It's the same question the git markers answer —
                // "what about this row isn't written yet" — and `:archive write` is
                // the commit, so a pending change reads as unstaged.
                let git_status = archive_change(&self.state.mounts, &rd.path).map_or_else(
                    || {
                        c.git
                            .files
                            .get(rd.git_key())
                            .copied()
                            .unwrap_or_else(GitFileStatus::clean)
                    },
                    |change| GitFileStatus {
                        staged: None,
                        unstaged: Some(change),
                        untracked: false,
                    },
                );
                let pending_delete = delete_set
                    .as_ref()
                    .is_some_and(|s| s.contains(rd.path.as_path()));
                Row {
                    display: rd.display.clone(),
                    kind: rd.kind,
                    picked: c.view == View::Dir && c.picks.contains(&rd.path),
                    taken: self.state.inventory.contains(&rd.path),
                    deleted: rd.deleted,
                    git_status,
                    pending_delete,
                }
            })
            .collect()
    }
}

/// spyc's "spice heat" palette — the warm pepper→ember→orange→spark ramp shared
/// by the Working activity dot's breathing pulse (`spicy_pulse_color`) and the
/// visual-bell border sweep (`spice_gradient`), so both read as the same heat
/// rather than two unrelated colour stories.
const SPICE_HEAT: [(u8, u8, u8); 4] = [
    (0xE0, 0x32, 0x22), // pepper red
    (0xF4, 0x62, 0x12), // ember
    (0xFF, 0x95, 0x12), // orange
    (0xFF, 0xC4, 0x36), // spark
];

/// A point on spyc's spice-heat gradient ([`SPICE_HEAT`]:
/// pepper→ember→orange→spark) at `frac` (0..1 around the border ring), swept by
/// the animation `frame` so the band of heat travels. Pure + truecolor; the
/// visual-bell pulse (`spice_pulse_color`) maps it to a cell bg. Looped, so the
/// gradient wraps seamlessly around the ring (spark folds back into pepper red,
/// an all-warm loop — no cool seam).
fn spice_gradient(frac: f32, frame: u64) -> (u8, u8, u8) {
    let count = SPICE_HEAT.len();
    // Sweep the sampled position by the frame (~0.08 stop-widths/tick).
    let pos = (frame as f32).mul_add(0.08, frac).rem_euclid(1.0);
    let scaled = pos * count as f32;
    let lo = (scaled as usize) % count;
    let hi = (lo + 1) % count;
    let blend = scaled - scaled.floor();
    let mix = |from: u8, to: u8| {
        (f32::from(to) - f32::from(from))
            .mul_add(blend, f32::from(from))
            .round() as u8
    };
    (
        mix(SPICE_HEAT[lo].0, SPICE_HEAT[hi].0),
        mix(SPICE_HEAT[lo].1, SPICE_HEAT[hi].1),
        mix(SPICE_HEAT[lo].2, SPICE_HEAT[hi].2),
    )
}

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::pane::AgentActivity;
    use std::path::PathBuf;

    /// The pane-tab indicator is fixed-width: `[N]` + exactly one status cell.
    /// Guard the agent half of that invariant — every *agent* activity renders a
    /// single-column dot (so the divider can't reflow as a tab's status changes,
    /// the bug this design fixes). `Unknown` (a non-agent tab) yields no dot; its
    /// cell is the `+`/blank shell glyph (1 col by construction in the loop).
    #[test]
    fn agent_status_dot_is_always_one_column() {
        let app = App::test_app(PathBuf::from("/tmp/proj"));
        for activity in [
            AgentActivity::Working,
            AgentActivity::Idle,
            AgentActivity::Blocked,
            AgentActivity::Done,
            AgentActivity::Waiting,
        ] {
            let span = app
                .agent_activity_span(activity, 0)
                .unwrap_or_else(|| panic!("agent activity {activity:?} should render a dot"));
            assert_eq!(
                crate::ui::display_width(span.content.as_ref()),
                1,
                "{activity:?} dot must be exactly one column (fixed-width indicator)"
            );
        }
        assert!(
            app.agent_activity_span(AgentActivity::Unknown, 0).is_none(),
            "a non-agent tab carries no dot"
        );
    }

    /// `waiting` is told apart by shape, which survives a theme with no
    /// colour: hollow, unlike working's `●`, and round, unlike the squares of
    /// the two states that are the user's turn. It never pulses.
    #[test]
    fn waiting_dot_is_a_steady_hollow_circle_in_both_themes() {
        let mut app = App::test_app(PathBuf::from("/tmp/proj"));
        for mono in [false, true] {
            app.view.theme.mono = mono;
            let dots: Vec<_> = (0..6)
                .map(|frame| {
                    app.view.agent_anim_frame = frame;
                    app.agent_activity_span(AgentActivity::Waiting, 0)
                        .expect("a waiting agent has a dot")
                })
                .collect();
            assert!(dots.iter().all(|dot| dot == &dots[0]), "mono={mono}");
            assert_eq!(dots[0].content, "\u{25cb}", "mono={mono}");
            let fg = dots[0].style.fg;
            assert_eq!(fg, (!mono).then_some(app.view.theme.dir), "mono={mono}");
        }
        for other in [
            AgentActivity::Working,
            AgentActivity::Blocked,
            AgentActivity::Done,
            AgentActivity::Idle,
        ] {
            let glyph = app
                .agent_activity_span(other, 0)
                .expect("an agent state has a dot")
                .content;
            assert_ne!(glyph, "\u{25cb}", "{other:?}");
        }
    }

    /// The requirement behind `anim_phase_offset`: two Working tabs at the SAME
    /// shared frame but different phase seeds land on different heat colours, so
    /// the dots don't breathe in lockstep. Also pins the overflow contract — a
    /// full-range seed must `wrapping_add` onto the frame, not panic.
    #[test]
    fn working_pulse_phase_offset_desyncs_tabs() {
        let mut app = App::test_app(PathBuf::from("/tmp/proj"));
        // Force the truecolor gradient path (env-independent) so the seed maps
        // through the 6-phase HEAT walk rather than the 2-hue fallback.
        app.view.hud_truecolor = true;
        app.view.theme.mono = false;
        app.view.agent_anim_frame = 1;

        // Offsets 0 and 3 sit at opposite ends of the ping-pong (pepper red vs
        // spark) — distinct seeds, distinct colours at one instant.
        let dot = |offset| {
            app.agent_activity_span(AgentActivity::Working, offset)
                .expect("a working agent has a dot")
                .style
                .fg
        };
        assert_ne!(
            dot(0),
            dot(3),
            "distinct phase seeds must yield distinct pulse colors"
        );

        // A near-max seed added to a non-zero frame must not overflow — the
        // call site wraps. (A raw `+` would panic here in a checked build.)
        let _ = dot(u64::MAX);
    }

    /// The visual bell's sweep: it starts at pepper red, and the ring has no
    /// seam where its two ends meet.
    #[test]
    fn spice_gradient_starts_at_pepper_red_and_wraps_seamlessly() {
        // frame 0, frac 0 → the first stop (pepper red) exactly.
        assert_eq!(super::spice_gradient(0.0, 0), (0xE0, 0x32, 0x22));
        // frac 1.0 == frac 0.0 at any frame (`rem_euclid`).
        assert_eq!(super::spice_gradient(1.0, 3), super::spice_gradient(0.0, 3));
    }

    /// Task tags are right-anchored: when they don't all fit, the oldest are
    /// dropped and the newest kept. Width is counted in columns, so a 3-byte
    /// glyph costs one.
    #[test]
    fn task_tags_drop_the_oldest_when_they_dont_fit() {
        use super::{TaskTone, fit_task_tags};
        let tags = || {
            ["one", "two", "three"]
                .map(|t| (format!(" [{t}\u{25cf}]"), TaskTone::Running))
                .into_iter()
        };
        let texts = |kept: Vec<(String, TaskTone)>| -> Vec<String> {
            kept.into_iter().map(|(text, _)| text).collect()
        };
        // " [one●]" is 7 columns, " [two●]" 7, " [three●]" 9.
        assert_eq!(texts(fit_task_tags(tags(), 23)).len(), 3);
        assert_eq!(
            texts(fit_task_tags(tags(), 22)),
            [" [two\u{25cf}]", " [three\u{25cf}]"],
            "the oldest goes first"
        );
        assert_eq!(texts(fit_task_tags(tags(), 9)), [" [three\u{25cf}]"]);
        assert!(fit_task_tags(tags(), 8).is_empty());
    }
}
