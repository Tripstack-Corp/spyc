//! Draws the status bar's powerline segments (pepper, `PROJECT_HOME`, session,
//! path, git, agent and the column's state), cutting the path in the middle
//! when the row is too short. While several projects are open, their chips
//! (`ui::project_chips`) take the place of `PROJECT_HOME` and the session.
//! The pass that draws the bar marks where each segment lands
//! (`ui::status_segments`), for a click to find.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::ui::project_chips::{self, ChipSpans, ProjectChips};
use crate::ui::status_segments::{SegmentSpan, StatusSegment};
use crate::ui::theme::Theme;

pub struct StatusBar<'a> {
    /// Basename of `PROJECT_HOME` if set (e.g. `spyc`). Hidden when `None`.
    pub project_home: Option<&'a str>,
    /// Session display name (e.g. `SAFFRON_CUMIN`). Hidden when `None`.
    pub session_name: Option<&'a str>,
    /// Every open project's chip while several are open, drawn in a third of
    /// the bar in place of `project_home` and `session_name`. `None` with one.
    pub projects: Option<&'a ProjectChips>,
    pub path: &'a str,
    /// Optional trailing state, e.g. `[picks:2 hidden:3]`.
    pub suffix: &'a str,
    /// A shorter `suffix` (`[p:2 h:3]`), drawn instead when the full one would
    /// cut the path. `None` keeps `suffix` whatever the width.
    pub suffix_short: Option<&'a str>,
    /// Git branch + dirty flag, e.g. `"main*"` or `None` if not in a repo.
    pub git_info: Option<&'a str>,
    /// Active pane's agent identity (e.g. `"claude:76422c62"` /
    /// `"agy:4c130f82"` / `"codex"`). `None` when no pane is open
    /// or its command isn't a known agent. Rendered as its own
    /// segment between `git` and `suffix` so it sits in roughly the
    /// same visual band as related state.
    pub agent_info: Option<&'a str>,
    pub theme: &'a Theme,
    /// Render the leading logo as a plain spice-red block instead of the 🌶️
    /// emoji. Set on terminals that mangle the 2-cell astral glyph (GNU screen,
    /// anything not truecolor) so the header reads as branded, not broken.
    pub plain_logo: bool,
    /// The `⚠N` of unread stderr lines ([`stderr_notice`]), drawn right after
    /// the logo. `None` draws nothing there.
    pub stderr_notice: Option<&'a str>,
    /// Draw [`MENU_KEY`] after the suffix: the key that opens an object's menu,
    /// until the first menu ever opens.
    pub teach_menu: bool,
}

/// The bar as drawn, and where each of its segments is.
pub struct StatusLine {
    pub line: Line<'static>,
    pub segments: Vec<SegmentSpan>,
    /// Where the suffix's own text was drawn; `None` when it wasn't.
    pub suffix_at: Option<SuffixAt>,
}

/// Where the suffix's text starts on the bar, and which form of it was drawn,
/// so a caller can find a field it placed in that form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuffixAt {
    /// The display column of its first character, from the bar's left edge.
    pub col: usize,
    /// The short form (`suffix_short`) was drawn, not `suffix`.
    pub short: bool,
}

/// The bar's spans as they are pushed, the columns they take, and the
/// segments marked on them.
#[derive(Default)]
struct Bar {
    spans: Vec<Span<'static>>,
    segments: Vec<SegmentSpan>,
    width: usize,
    suffix_at: Option<SuffixAt>,
}

impl Bar {
    fn push(&mut self, span: Span<'static>) {
        self.width += dw(&span.content);
        self.spans.push(span);
    }

    /// Mark what was pushed since column `from` as `segment`, one span with
    /// the `segment` marked last when it ends at `from`.
    fn mark(&mut self, segment: StatusSegment, from: usize) {
        if let Some(last) = self.segments.last_mut()
            && last.segment == segment
            && last.end == from
        {
            last.end = self.width;
        } else if self.width > from {
            self.segments.push(SegmentSpan {
                segment,
                start: from,
                end: self.width,
            });
        }
    }

    /// Mark each folder of `path`, drawn as `drawn` from column `from`.
    fn mark_folders(&mut self, path: &str, drawn: &str, from: usize) {
        self.segments
            .extend(crate::ui::status_segments::path_folder_spans(
                path, drawn, ELLIPSIS, from,
            ));
    }

    /// Push `chips`, each in `style` patched with its own, marking each chip
    /// and the count.
    fn push_chips(&mut self, chips: ChipSpans, style: Style) {
        for (span, part) in chips.spans.into_iter().zip(chips.parts) {
            let from = self.width;
            self.push(Span::styled(span.content, style.patch(span.style)));
            if let Some(part) = part {
                self.mark(part, from);
            }
        }
    }

    fn finish(self) -> StatusLine {
        StatusLine {
            line: Line::from(self.spans),
            segments: self.segments,
            suffix_at: self.suffix_at,
        }
    }
}

/// The status bar's lesson until a menu first opens: `.` opens one.
const MENU_KEY: &str = ". menu";

/// `suffix` with [`MENU_KEY`] after it.
fn with_menu_key(suffix: &str) -> String {
    if suffix.is_empty() {
        MENU_KEY.to_owned()
    } else {
        format!("{suffix} {MENU_KEY}")
    }
}

/// Powerline right-pointing triangle (requires a Nerd Font or powerline-patched font).
const PL_SEP: &str = "\u{e0b0}";

const ELLIPSIS: &str = "...";

/// The status bar's mark for `unread` stderr lines, `⚠3` or `⚠99+`; `None`
/// when there are none.
pub fn stderr_notice(unread: u64) -> Option<String> {
    match unread {
        0 => None,
        1..=99 => Some(format!("\u{26a0}{unread}")),
        _ => Some("\u{26a0}99+".to_string()),
    }
}

/// Whether display column `col` of a drawn bar `line` falls on its `notice`.
/// The mouse asks this of the line the renderer recorded, so a click and the
/// glyph it hits can't disagree about where the glyph is.
pub fn notice_at(line: &Line<'_>, col: usize, notice: &str) -> bool {
    let mut x = 0;
    for span in &line.spans {
        let w = dw(&span.content);
        if (x..x + w).contains(&col) {
            return span.content == notice;
        }
        x += w;
    }
    false
}

/// Segment `segment`: `text` in `fg` on `bg`, then the separator into
/// `next_bg`, which is part of it.
fn push_segment(
    bar: &mut Bar,
    segment: StatusSegment,
    text: &str,
    fg: Color,
    bg: Color,
    next_bg: Color,
) {
    let from = bar.width;
    bar.push(Span::styled(
        text.to_string(),
        Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
    ));
    bar.push(Span::styled(PL_SEP, Style::default().fg(bg).bg(next_bg)));
    bar.mark(segment, from);
}

/// The suffix's segment, `text` padded, noting where its own text starts and
/// whether it is the `short` form.
fn push_suffix(bar: &mut Bar, text: &str, short: bool, fg: Color, bg: Color, next_bg: Color) {
    // Past the space that pads it.
    bar.suffix_at = Some(SuffixAt {
        col: bar.width + 1,
        short,
    });
    push_segment(bar, StatusSegment::Suffix, text, fg, bg, next_bg);
}

/// [`push_segment`] for a segment of styled spans: each drawn on `bg`, in `fg`
/// unless it has a colour of its own; the projects segment on its own deeper
/// brown. The separator leaves the last span's background, the tab in view's
/// when it ends the segment.
fn push_spans(
    bar: &mut Bar,
    segment: StatusSegment,
    spans: ChipSpans,
    fg: Color,
    bg: Color,
    next_bg: Color,
) {
    let from = bar.width;
    let bg = if segment == StatusSegment::Projects {
        project_chips::SEGMENT_BG
    } else {
        bg
    };
    let end = spans.last().and_then(|s| s.style.bg).unwrap_or(bg);
    bar.push_chips(spans, Style::default().fg(fg).bg(bg));
    bar.push(Span::styled(PL_SEP, Style::default().fg(end).bg(next_bg)));
    bar.mark(segment, from);
}

impl StatusBar<'_> {
    /// The suffix to draw in `room` columns, given what a suffix costs there
    /// and what the uncut path needs: the full one, unless it would cut the path
    /// and a short form exists. Whether it is the short one, too.
    fn fitting_suffix(
        &self,
        room: usize,
        cost: impl Fn(&str) -> usize,
        path_need: usize,
    ) -> (&str, bool) {
        match self.suffix_short {
            Some(short) if room.saturating_sub(cost(self.suffix)) < path_need => (short, true),
            _ => (self.suffix, false),
        }
    }

    /// [`Self::build`]'s line alone.
    #[cfg(test)]
    pub fn build_line(&self, area: Rect) -> Line<'static> {
        self.build(area).line
    }

    /// The bar as a styled [`Line`], exactly as it will be drawn — including the
    /// width-driven truncation — and where each segment of it is. Split out so
    /// the app layer can record what was actually rendered: a mouse selection
    /// maps screen COLUMNS back to characters, and a click a column to a
    /// segment, which only works against the drawn line.
    pub fn build(&self, area: Rect) -> StatusLine {
        if self.teach_menu {
            let full = with_menu_key(self.suffix);
            let short = self.suffix_short.map(with_menu_key);
            let bar = StatusBar {
                suffix: &full,
                suffix_short: short.as_deref(),
                teach_menu: false,
                ..*self
            };
            return bar.build(area);
        }
        if self.theme.mono {
            return self.plain_line(area);
        }
        self.powerline(area)
    }

    /// The logo, and the stderr notice after it. Truecolor terminals get the 🌶️
    /// emoji; degraded ones (GNU screen, non-truecolor) mangle the 2-cell astral
    /// glyph into junk, so paint a spice-red block instead — same 4-col
    /// footprint, reads as intentional.
    fn push_logo(&self, bar: &mut Bar, term_bg: Color) {
        self.push_pepper(bar, term_bg);
        if let Some(notice) = self.stderr_notice {
            bar.push(Span::styled(
                notice.to_string(),
                Style::default()
                    .fg(self.theme.prompt_prefix)
                    .bg(term_bg)
                    .add_modifier(Modifier::BOLD),
            ));
            bar.push(Span::styled(" ", Style::default().bg(term_bg)));
        }
    }

    /// Columns [`Self::push_logo`] takes: the pepper's four, and the notice
    /// with its trailing space.
    fn logo_width(&self) -> usize {
        4 + self.stderr_notice.map_or(0, |notice| dw(notice) + 1)
    }

    fn push_pepper(&self, bar: &mut Bar, term_bg: Color) {
        // Pepper emoji gets its own span with terminal-default bg so the
        // emoji colour rendering isn't clipped by the segment background.
        let emoji_text = " \u{1f336}\u{fe0f} ";
        if self.plain_logo {
            let pepper_red = Color::Rgb(0xE0, 0x32, 0x22); // SPICE_HEAT[0]
            bar.push(Span::styled(" ", Style::default().bg(term_bg)));
            bar.push(Span::styled("  ", Style::default().bg(pepper_red)));
            bar.push(Span::styled(" ", Style::default().bg(term_bg)));
        } else {
            bar.push(Span::styled(emoji_text, Style::default().bg(term_bg)));
        }
    }

    #[allow(clippy::similar_names)]
    fn powerline(&self, area: Rect) -> StatusLine {
        let avail = area.width as usize;

        // Segment background/foreground colours.
        let proj_bg = Color::Rgb(0x7a, 0x4d, 0x3a); // warm brown (pepper-ish)
        let proj_fg = Color::Rgb(0xf2, 0xe8, 0xcf); // cream

        let sess_bg = self.theme.status_user; // lavender (reused)
        let sess_fg = Color::Rgb(0x1a, 0x1b, 0x26); // tokyo night bg

        let path_bg = Color::Rgb(0x3b, 0x40, 0x61); // muted indigo
        let path_fg = self.theme.status_path;

        let git_bg = Color::Rgb(0x2a, 0x2e, 0x45); // slightly lighter than suffix
        let git_fg = self.theme.exec; // soft green

        // Agent segment sits between git and suffix. Uses the pick
        // accent so it visually distinguishes from the
        // git/file-state band.
        let agent_bg = Color::Rgb(0x32, 0x36, 0x52);
        let agent_fg = self.theme.pick;

        let suffix_bg = Color::Rgb(0x24, 0x28, 0x3b); // darker
        let suffix_fg = self.theme.status_suffix;

        let term_bg = Color::Reset;

        let emoji_w = self.logo_width(); // space + 🌶️ (2 cols) + space, then the notice

        let project_spans = self.project_spans(avail);
        let session_text = self
            .session_name
            .filter(|_| self.projects.is_none())
            .map(|n| format!(" {n} "));
        let git_text = self.git_info.map(|g| format!(" \u{e0a0} {g} "));
        let agent_text = self.agent_info.map(|a| format!(" \u{f120} {a} ")); // 󰰠 terminal-shell-like glyph

        let width_of = |t: &Option<String>| t.as_deref().map_or(0, |s| dw(s) + 1);
        let project_w = project_spans
            .as_ref()
            .map_or(0, |(_, s)| project_chips::width(s) + 1);
        let session_w = width_of(&session_text);
        let git_w = width_of(&git_text);
        let agent_w = width_of(&agent_text);
        // A suffix segment is ` text ` plus its separator; the path's is ` path `.
        let (suffix, short) = self.fitting_suffix(
            avail.saturating_sub(emoji_w + project_w + session_w + git_w + agent_w + 1),
            |s| if s.is_empty() { 0 } else { dw(s) + 3 },
            dw(self.path) + 2,
        );
        let suffix_text = (!suffix.is_empty()).then(|| format!(" {suffix} "));
        let suffix_w = width_of(&suffix_text);

        // path_budget = avail − (emoji + optional segments + path-sep).
        let fixed = emoji_w + project_w + session_w + git_w + agent_w + suffix_w + 1;
        let path_budget = avail.saturating_sub(fixed);
        let path_cut = truncate_middle(self.path, path_budget.saturating_sub(2));
        let path_text = format!(" {path_cut} ");

        let path_next_bg = if git_text.is_some() {
            git_bg
        } else if agent_text.is_some() {
            agent_bg
        } else if suffix_text.is_some() {
            suffix_bg
        } else {
            term_bg
        };

        let mut bar = Bar::default();
        self.push_logo(&mut bar, term_bg);

        if let Some((segment, project)) = project_spans {
            let next_bg = session_text.as_ref().map_or(path_bg, |_| sess_bg);
            push_spans(&mut bar, segment, project, proj_fg, proj_bg, next_bg);
        }
        if let Some(ref text) = session_text {
            let session = StatusSegment::Session;
            push_segment(&mut bar, session, text, sess_fg, sess_bg, path_bg);
        }

        // Path (always present).
        let from = bar.width;
        bar.push(Span::styled(
            path_text.clone(),
            Style::default().fg(path_fg).bg(path_bg),
        ));
        bar.push(Span::styled(
            PL_SEP,
            Style::default().fg(path_bg).bg(path_next_bg),
        ));
        bar.mark(StatusSegment::Path, from);
        bar.mark_folders(self.path, &path_cut, from + 1);

        if let Some(ref text) = git_text {
            let next_bg = if agent_text.is_some() {
                agent_bg
            } else if suffix_text.is_some() {
                suffix_bg
            } else {
                term_bg
            };
            push_segment(&mut bar, StatusSegment::Git, text, git_fg, git_bg, next_bg);
        }
        if let Some(ref text) = agent_text {
            let next_bg = suffix_text.as_ref().map_or(term_bg, |_| suffix_bg);
            let agent = StatusSegment::Agent;
            push_segment(&mut bar, agent, text, agent_fg, agent_bg, next_bg);
        }
        if let Some(ref text) = suffix_text {
            push_suffix(&mut bar, text, short, suffix_fg, suffix_bg, term_bg);
        }

        // Fill remaining width.
        let used: usize =
            emoji_w + project_w + session_w + dw(&path_text) + 1 + git_w + agent_w + suffix_w;
        if used < avail {
            bar.push(Span::styled(
                " ".repeat(avail - used),
                Style::default().bg(term_bg),
            ));
        }

        bar.finish()
    }

    /// The project segment, padded, before the bar's colours: `PROJECT_HOME`'s
    /// basename with one project open, else the chips, in a third of the bar
    /// with the padding and the separator.
    fn project_spans(&self, avail: usize) -> Option<(StatusSegment, ChipSpans)> {
        let mut spans = ChipSpans::default();
        let Some(chips) = self.projects else {
            let home = self.project_home?;
            let bold = Style::default().add_modifier(Modifier::BOLD);
            spans.push(Span::styled(format!(" {home} "), bold), None);
            return Some((StatusSegment::Project, spans));
        };
        // The logo, the padding and the separator.
        let most = avail.saturating_sub(4 + 3);
        let drawn = chips.spans(chip_room(avail), most, self.theme);
        let [before, after] = drawn.edges;
        let [first, last] = drawn.edge_parts();
        spans.push(Span::styled(" ", before), first);
        spans.append(drawn);
        spans.push(Span::styled(" ", after), last);
        Some((StatusSegment::Projects, spans))
    }

    /// Fallback plain rendering for mono mode.
    fn plain_line(&self, area: Rect) -> StatusLine {
        let avail = area.width as usize;

        let bold = Style::default().add_modifier(Modifier::BOLD);
        let mut bar = Bar::default();
        if let Some(notice) = self.stderr_notice {
            bar.push(Span::styled(notice.to_string(), bold));
            bar.push(Span::raw(" "));
        }
        if let Some(chips) = self.projects {
            let from = bar.width;
            let most = avail.saturating_sub(2);
            bar.push_chips(
                chips.spans(chip_room(avail), most, self.theme),
                Style::default(),
            );
            bar.push(Span::raw(": "));
            bar.mark(StatusSegment::Projects, from);
        } else {
            self.push_plain_prefix(&mut bar, bold);
        }
        let pre_w = bar.width;
        let cost = |s: &str| if s.is_empty() { 0 } else { 2 + dw(s) };
        let (suffix, short) = self.fitting_suffix(avail.saturating_sub(pre_w), cost, dw(self.path));
        let suffix_w = cost(suffix);

        let path_budget = avail.saturating_sub(pre_w + suffix_w);
        let (path_disp, suffix_disp) = if path_budget >= 8 {
            (truncate_middle(self.path, path_budget), suffix.to_string())
        } else {
            let budget = avail.saturating_sub(pre_w);
            (truncate_middle(self.path, budget), String::new())
        };

        let from = bar.width;
        bar.mark_folders(self.path, &path_disp, from);
        bar.push(Span::raw(path_disp));
        bar.mark(StatusSegment::Path, from);
        if !suffix_disp.is_empty() {
            bar.push(Span::raw("  "));
            let from = bar.width;
            bar.suffix_at = Some(SuffixAt { col: from, short });
            bar.push(Span::styled(
                suffix_disp,
                Style::default().add_modifier(Modifier::DIM),
            ));
            bar.mark(StatusSegment::Suffix, from);
        }
        bar.finish()
    }

    /// Mono's `spyc SAFFRON_CUMIN: `: the project and the session, each a
    /// segment of its own, and nothing when neither is set.
    fn push_plain_prefix(&self, bar: &mut Bar, bold: Style) {
        let parts = [
            (self.project_home, StatusSegment::Project),
            (self.session_name, StatusSegment::Session),
        ];
        let mut any = false;
        for (text, segment) in parts {
            let Some(text) = text else { continue };
            if any {
                bar.push(Span::styled(" ", bold));
            }
            let from = bar.width;
            bar.push(Span::styled(text.to_string(), bold));
            bar.mark(segment, from);
            any = true;
        }
        if any {
            bar.push(Span::styled(": ", bold));
        }
    }
}

/// The chips' room in an `avail`-wide bar: a third of it, less the powerline
/// segment's padding and separator. Mono takes the same, so both narrow at the
/// same widths.
const fn chip_room(avail: usize) -> usize {
    (avail / 3).saturating_sub(3)
}

/// Elide the middle of `s` to fit `max` display columns, keeping a third of the
/// budget for the head and two thirds for the tail — a path's leaf identifies it
/// better than its root does. Both halves cut on grapheme-cluster boundaries.
/// (Deliberately not `unicode_truncate_centered`, which would split the budget
/// evenly and lose that bias.)
pub fn truncate_middle(s: &str, max: usize) -> String {
    if dw(s) <= max {
        return s.to_string();
    }
    let ellipsis_w = dw(ELLIPSIS);
    if max <= ellipsis_w {
        // Degenerate: just return the tail that fits.
        return display_take_tail(s, max);
    }
    let budget = max - ellipsis_w;
    let head_budget = budget / 3;
    let tail_budget = budget - head_budget;
    let head = super::display_truncate(s, head_budget);
    let tail = display_take_tail(s, tail_budget);
    format!("{head}{ELLIPSIS}{tail}")
}

/// Take the last `max` display columns from a string. Thin owned wrapper over
/// the shared [`super::display_truncate_tail`], which cuts on grapheme-cluster
/// boundaries — this used to walk chars and could both mismeasure a cluster's
/// width and slice one in half.
fn display_take_tail(s: &str, max: usize) -> String {
    super::display_truncate_tail(s, max).to_string()
}

/// Shorthand for `super::display_width(s)`.
fn dw(s: &str) -> usize {
    super::display_width(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::widgets::Paragraph;

    #[test]
    fn short_path_unchanged() {
        assert_eq!(truncate_middle("/a/b", 20), "/a/b");
    }

    #[test]
    fn middle_truncation_favours_tail() {
        let s = "/Users/x/src/spyc/a/b/c/very_long_directory_name";
        let out = truncate_middle(s, 25);
        assert_eq!(dw(&out), 25);
        assert!(out.starts_with("/User"));
        assert!(out.contains("..."));
        assert!(out.ends_with("directory_name") || out.ends_with("ctory_name"));
    }

    #[test]
    fn degenerate_budget_keeps_tail() {
        let out = truncate_middle("/very/long/path", 3);
        assert_eq!(out, "ath");
    }

    // ── snapshot tests (TestBackend) ──────────────────────────────

    use ratatui::{Terminal, backend::TestBackend};

    #[allow(clippy::too_many_arguments)]
    fn render_status_to_string(
        project_home: Option<&str>,
        session_name: Option<&str>,
        path: &str,
        suffix: &str,
        git_info: Option<&str>,
        agent_info: Option<&str>,
        mono: bool,
        width: u16,
    ) -> String {
        render_with_logo(
            project_home,
            session_name,
            path,
            suffix,
            git_info,
            agent_info,
            mono,
            false,
            width,
        )
    }

    #[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
    fn render_with_logo(
        project_home: Option<&str>,
        session_name: Option<&str>,
        path: &str,
        suffix: &str,
        git_info: Option<&str>,
        agent_info: Option<&str>,
        mono: bool,
        plain_logo: bool,
        width: u16,
    ) -> String {
        let backend = TestBackend::new(width, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = if mono {
            Theme {
                mono: true,
                ..Theme::default()
            }
        } else {
            Theme::default()
        };
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, width, 1);
                let bar = StatusBar {
                    project_home,
                    session_name,
                    projects: None,
                    path,
                    suffix,
                    suffix_short: None,
                    git_info,
                    agent_info,
                    theme: &theme,
                    plain_logo,
                    stderr_notice: None,
                    teach_menu: false,
                };
                f.render_widget(Paragraph::new(bar.build_line(area)), area);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for x in 0..buf.area.width {
            out.push_str(buf.cell((x, 0)).map_or(" ", |c| c.symbol()));
        }
        out.trim_end().to_string()
    }

    /// The bar's text at `width`, with a short suffix to fall back on.
    fn bar_text(path: &str, mono: bool, width: u16) -> String {
        let theme = Theme {
            mono,
            ..Theme::default()
        };
        let bar = StatusBar {
            project_home: Some("spyc"),
            session_name: None,
            projects: None,
            path,
            suffix: "[picks:3 hidden:12 sort:mtime]",
            suffix_short: Some("[p:3 h:12 s:t]"),
            git_info: None,
            agent_info: None,
            theme: &theme,
            plain_logo: true,
            stderr_notice: None,
            teach_menu: false,
        };
        bar.build_line(Rect::new(0, 0, width, 1))
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    const PATH: &str = "~/src/spyc/src/app/render";

    #[test]
    fn the_full_suffix_is_drawn_while_the_path_fits() {
        for mono in [false, true] {
            let text = bar_text(PATH, mono, 120);
            assert!(text.contains("[picks:3 hidden:12 sort:mtime]"), "{text}");
            assert!(text.contains(PATH), "{text}");
        }
    }

    /// The width where the full words would cut the path but the short forms
    /// don't: the path is what the user navigates by, so it wins the room.
    #[test]
    fn the_short_suffix_is_drawn_when_the_full_one_would_cut_the_path() {
        for mono in [false, true] {
            let text = bar_text(PATH, mono, 58);
            assert!(text.contains("[p:3 h:12 s:t]"), "mono={mono}: {text}");
            assert!(text.contains(PATH), "mono={mono}: {text}");
        }
    }

    /// The lesson rides the suffix in both styles: after the column's state,
    /// in the short form too, and alone when the column has none.
    #[test]
    fn teach_menu_follows_the_suffix_or_stands_alone() {
        let theme = Theme::default();
        let mono = Theme {
            mono: true,
            ..Theme::default()
        };
        for theme in [&theme, &mono] {
            let text = |suffix: &str, suffix_short: Option<&str>, width: u16| -> String {
                let bar = StatusBar {
                    project_home: Some("spyc"),
                    session_name: None,
                    projects: None,
                    path: PATH,
                    suffix,
                    suffix_short,
                    git_info: None,
                    agent_info: None,
                    theme,
                    plain_logo: true,
                    stderr_notice: None,
                    teach_menu: true,
                };
                bar.build_line(Rect::new(0, 0, width, 1))
                    .spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect()
            };
            let mono = theme.mono;
            let full = text(
                "[picks:3 hidden:12 sort:mtime]",
                Some("[p:3 h:12 s:t]"),
                120,
            );
            assert!(
                full.contains("[picks:3 hidden:12 sort:mtime] . menu"),
                "mono={mono}: {full}"
            );
            let short = text("[picks:3 hidden:12 sort:mtime]", Some("[p:3 h:12 s:t]"), 65);
            assert!(
                short.contains("[p:3 h:12 s:t] . menu"),
                "mono={mono}: {short}"
            );
            assert!(short.contains(PATH), "mono={mono}: {short}");
            let alone = text("", None, 120);
            assert!(alone.contains(". menu"), "mono={mono}: {alone}");
        }
    }

    #[test]
    fn with_no_short_form_the_path_truncates_as_before() {
        let theme = Theme::default();
        let bar = StatusBar {
            project_home: Some("spyc"),
            session_name: None,
            projects: None,
            path: PATH,
            suffix: "[picks:3 hidden:12 sort:mtime]",
            suffix_short: None,
            git_info: None,
            agent_info: None,
            theme: &theme,
            plain_logo: true,
            stderr_notice: None,
            teach_menu: false,
        };
        let text: String = bar
            .build_line(Rect::new(0, 0, 58, 1))
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(text.contains("[picks:3 hidden:12 sort:mtime]"), "{text}");
        assert!(text.contains(ELLIPSIS), "{text}");
    }

    #[test]
    fn snapshot_status_mono_basic() {
        let out = render_status_to_string(
            None,
            Some("SAFFRON_CUMIN"),
            "/tmp",
            "",
            None,
            None,
            true,
            60,
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_status_mono_with_suffix() {
        let out = render_status_to_string(
            Some("spyc"),
            Some("SAFFRON_CUMIN"),
            "/home/user/src",
            "[picks:2 m1:on]",
            None,
            None,
            true,
            60,
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_status_powerline_basic() {
        let out = render_status_to_string(
            None,
            Some("SAFFRON_CUMIN"),
            "/tmp",
            "",
            None,
            None,
            false,
            80,
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_status_powerline_with_git() {
        let out = render_status_to_string(
            Some("spyc"),
            Some("SAFFRON_CUMIN"),
            "/home/src",
            "[picks:1]",
            Some("main*"),
            None,
            false,
            80,
        );
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_status_powerline_with_agent() {
        // New segment: agent identity (claude/codex/agy with short
        // session-id when known) renders between `git` and `suffix`.
        let out = render_status_to_string(
            Some("spyc"),
            Some("SAFFRON_CUMIN"),
            "/home/src",
            "[picks:1]",
            Some("main*"),
            Some("claude:76422c62"),
            false,
            96,
        );
        insta::assert_snapshot!(out);
    }

    /// The chips take the project and session segments' place, on the
    /// project segment's background, each glyph keeping its own colour.
    #[test]
    fn the_chips_take_the_project_and_session_place() {
        use crate::ui::project_chips::{BLOCKED_RED, ProjectAttention, ProjectChip};
        let theme = Theme::default();
        let chips = ProjectChips {
            chips: vec![
                ProjectChip {
                    label: "web".into(),
                    attention: Some(ProjectAttention::Blocked),
                    active: false,
                },
                ProjectChip {
                    label: "spyc".into(),
                    attention: None,
                    active: true,
                },
            ],
            pulse: Color::Red,
        };
        let bar = StatusBar {
            project_home: Some("spyc"),
            session_name: Some("SAFFRON_CUMIN"),
            projects: Some(&chips),
            path: PATH,
            suffix: "",
            suffix_short: None,
            git_info: None,
            agent_info: None,
            theme: &theme,
            plain_logo: true,
            stderr_notice: None,
            teach_menu: false,
        };
        let line = bar.build_line(Rect::new(0, 0, 120, 1));
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains(" web ■ \u{e0b0} spyc "), "{text}");
        assert!(!text.contains("SAFFRON_CUMIN"), "{text}");
        let glyph = line
            .spans
            .iter()
            .find(|s| s.content == "■")
            .expect("a glyph");
        let label = |name: &str| {
            line.spans
                .iter()
                .find(|s| s.content == name)
                .expect("a label")
                .style
        };
        assert_eq!(glyph.style.fg, Some(BLOCKED_RED));
        assert_eq!(glyph.style.bg, label("web").bg);
        assert!(glyph.style.bg.is_some());
        assert_eq!(label("web").bg, Some(project_chips::SEGMENT_BG));
        let tab = Some(theme.cursor_bg);
        assert_eq!(label("spyc").bg, tab);
        let at = line
            .spans
            .iter()
            .position(|s| s.content == "spyc")
            .expect("the tab");
        let after = &line.spans[at + 3];
        assert_eq!(
            (after.content.as_ref(), after.style.bg),
            (" ", tab),
            "the tab reaches the bar's padding"
        );
        let sep = &line.spans[at + 4];
        assert_eq!(
            (sep.content.as_ref(), sep.style.fg),
            (PL_SEP, tab),
            "and the segment ends from the tab's colour"
        );
    }

    /// However narrow the bar, the narrowest step's `■` and the segment's end
    /// (the separator, or mono's `: `) are on it: the label is cut first.
    #[test]
    fn the_narrowest_chips_fit_the_bar() {
        use crate::ui::project_chips::{ProjectAttention, ProjectChip};
        let chips = ProjectChips {
            chips: vec![
                ProjectChip {
                    label: "a-long-project-name".into(),
                    attention: None,
                    active: true,
                },
                ProjectChip {
                    label: "x".into(),
                    attention: Some(ProjectAttention::Blocked),
                    active: false,
                },
            ],
            pulse: Color::Red,
        };
        for mono in [false, true] {
            let theme = Theme {
                mono,
                ..Theme::default()
            };
            let end = if mono { ": " } else { PL_SEP };
            for w in 12..40u16 {
                let bar = StatusBar {
                    project_home: None,
                    session_name: None,
                    projects: Some(&chips),
                    path: PATH,
                    suffix: "",
                    suffix_short: None,
                    git_info: None,
                    agent_info: None,
                    theme: &theme,
                    plain_logo: true,
                    stderr_notice: None,
                    teach_menu: false,
                };
                let line = bar.build_line(Rect::new(0, 0, w, 1));
                let mut col = 0;
                let mut block = false;
                let ends = line.spans.iter().find_map(|s| {
                    block |= s.content == "■";
                    col += dw(&s.content);
                    (block && s.content == end).then_some(col)
                });
                let ends = ends.expect("the block and the segment's end");
                assert!(ends <= usize::from(w), "mono={mono} w={w}: {line:?}");
            }
        }
    }

    /// What follows the chips on the bar stays where it is as the projects'
    /// agents change, in colour and in mono, at any width: the path starts
    /// at one column whatever they are doing.
    #[test]
    fn the_bar_holds_its_place_as_agents_change() {
        use crate::ui::project_chips::{ProjectAttention, ProjectChip};
        for mono in [false, true] {
            let theme = Theme {
                mono,
                ..Theme::default()
            };
            for w in [120u16, 40, 30] {
                let mut starts = std::collections::BTreeSet::new();
                for attention in [
                    None,
                    Some(ProjectAttention::Working),
                    Some(ProjectAttention::Blocked),
                ] {
                    let chips = ProjectChips {
                        chips: vec![
                            ProjectChip {
                                label: "web".into(),
                                attention,
                                active: false,
                            },
                            ProjectChip {
                                label: "spyc".into(),
                                attention: None,
                                active: true,
                            },
                        ],
                        pulse: Color::Red,
                    };
                    let bar = StatusBar {
                        project_home: None,
                        session_name: None,
                        projects: Some(&chips),
                        path: PATH,
                        suffix: "",
                        suffix_short: None,
                        git_info: None,
                        agent_info: None,
                        theme: &theme,
                        plain_logo: true,
                        stderr_notice: None,
                        teach_menu: false,
                    };
                    let line = bar.build_line(Rect::new(0, 0, w, 1));
                    let path = line
                        .spans
                        .iter()
                        .position(|s| s.content.trim_start().starts_with("~/"))
                        .expect("the path");
                    let before = line.spans[..path]
                        .iter()
                        .map(|s| dw(&s.content))
                        .sum::<usize>();
                    starts.insert(before);
                }
                assert_eq!(starts.len(), 1, "mono={mono} w={w}: {starts:?}");
            }
        }
    }

    /// The parting between the chips is drawn neither bold nor dim, so it
    /// stands out from the dim labels on either side of it.
    #[test]
    fn the_parting_stands_out_from_the_dim_labels() {
        use crate::ui::project_chips::ProjectChip;
        let theme = Theme::default();
        let chips = ProjectChips {
            chips: vec![
                ProjectChip {
                    label: "web".into(),
                    attention: None,
                    active: false,
                },
                ProjectChip {
                    label: "api".into(),
                    attention: None,
                    active: false,
                },
                ProjectChip {
                    label: "spyc".into(),
                    attention: None,
                    active: true,
                },
            ],
            pulse: Color::Red,
        };
        let bar = StatusBar {
            project_home: None,
            session_name: None,
            projects: Some(&chips),
            path: PATH,
            suffix: "",
            suffix_short: None,
            git_info: None,
            agent_info: None,
            theme: &theme,
            plain_logo: true,
            stderr_notice: None,
            teach_menu: false,
        };
        let line = bar.build_line(Rect::new(0, 0, 120, 1));
        let parting = line
            .spans
            .iter()
            .find(|s| s.content.contains('\u{e0b1}'))
            .expect("a parting");
        let web = line
            .spans
            .iter()
            .find(|s| s.content == "web")
            .expect("web's label");
        let loud = Modifier::DIM | Modifier::BOLD;
        assert!(!parting.style.add_modifier.intersects(loud), "{parting:?}");
        assert!(web.style.add_modifier.contains(Modifier::DIM), "{web:?}");
    }

    #[test]
    fn plain_logo_replaces_pepper_emoji_with_a_block() {
        // Truecolor: the 🌶️ emoji is present in the rendered header.
        let emoji = render_with_logo(None, Some("S"), "/tmp", "", None, None, false, false, 60);
        assert!(
            emoji.contains('\u{1f336}'),
            "expected pepper emoji, got {emoji:?}"
        );
        // Degraded (GNU screen / non-truecolor): the mangled glyph is gone,
        // replaced by a plain block — the segment text still renders after it.
        let plain = render_with_logo(None, Some("S"), "/tmp", "", None, None, false, true, 60);
        assert!(
            !plain.contains('\u{1f336}'),
            "emoji must be dropped, got {plain:?}"
        );
        assert!(
            plain.contains("/tmp"),
            "rest of the bar still renders: {plain:?}"
        );
    }

    #[test]
    fn the_notice_counts_unread_lines_and_caps_at_ninety_nine() {
        assert_eq!(stderr_notice(0), None);
        assert_eq!(stderr_notice(3).as_deref(), Some("\u{26a0}3"));
        assert_eq!(stderr_notice(99).as_deref(), Some("\u{26a0}99"));
        assert_eq!(stderr_notice(150).as_deref(), Some("\u{26a0}99+"));
    }

    /// The notice is drawn right after the pepper, and the column the terminal
    /// actually paints it in is the one `notice_at` answers for — the two
    /// halves a click on it depends on.
    #[test]
    fn the_notice_sits_by_the_pepper_where_a_click_finds_it() {
        for (mono, plain_logo) in [(false, false), (false, true), (true, false)] {
            let theme = Theme {
                mono,
                ..Theme::default()
            };
            let area = Rect::new(0, 0, 80, 1);
            let bar = StatusBar {
                project_home: Some("spyc"),
                session_name: None,
                projects: None,
                path: PATH,
                suffix: "[hidden:1]",
                suffix_short: None,
                git_info: Some("main"),
                agent_info: None,
                theme: &theme,
                plain_logo,
                stderr_notice: Some("\u{26a0}3"),
                teach_menu: false,
            };
            let line = bar.build_line(area);
            let mut terminal = Terminal::new(TestBackend::new(area.width, 1)).unwrap();
            terminal
                .draw(|f| f.render_widget(Paragraph::new(line.clone()), area))
                .unwrap();
            let buf = terminal.backend().buffer().clone();
            let cells: Vec<&str> = (0..area.width)
                .map(|x| buf.cell((x, 0)).map_or(" ", |c| c.symbol()))
                .collect();
            let at = cells
                .iter()
                .position(|c| *c == "\u{26a0}")
                .unwrap_or_else(|| panic!("no notice in {cells:?}"));
            let expected = if mono { 0 } else { 4 };
            assert_eq!(
                at, expected,
                "mono={mono} plain_logo={plain_logo}: {cells:?}"
            );
            assert_eq!(cells[at + 1], "3");
            assert!(notice_at(&line, at, "\u{26a0}3"), "mono={mono}");
            assert!(notice_at(&line, at + 1, "\u{26a0}3"), "mono={mono}");
            assert!(!notice_at(&line, at + 2, "\u{26a0}3"), "mono={mono}");
            if at > 0 {
                assert!(!notice_at(&line, at - 1, "\u{26a0}3"), "mono={mono}");
            }
            let text: String = cells.concat();
            assert!(text.contains(PATH), "the path still fits: {text}");
            if !mono {
                assert_eq!(
                    line.width(),
                    usize::from(area.width),
                    "the bar fills its row"
                );
            }
        }
    }
}
