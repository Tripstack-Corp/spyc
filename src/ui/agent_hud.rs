//! The native agent strip: one template that every agent with a metrics source
//! fills. Its rows are laid out as a grid, so each column starts at the same x
//! on every row. The template owns the activity row, the health cells and all
//! styling; an agent supplies only its own usage, tools and mode cells
//! (`claude`, `codex`). The caller supplies every source, timestamp and
//! attention state: nothing here reads a clock or collects.

use jiff::Timestamp;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::{display_truncate, display_truncate_tail, display_width, theme::Theme};
use crate::agent::tail::Availability;
use crate::agent::{claude_metrics, codex_metrics};
use crate::config::AgentHudMode;

mod claude;
mod codex;

/// Spaces between columns.
const GAP: usize = 3;
/// Value columns are at least this wide, so a count gaining a digit rarely
/// moves the next column.
const MIN_COLUMN: usize = 12;
/// A cell with a column after it is cut to this width, so one long branch
/// can't push every other column off the strip.
const MAX_CELL: usize = 32;
/// Tool names are cut to this width in the most-called list.
const MAX_TOOL_NAME: usize = 12;
/// How many of the most-called tools the collapsed row names.
const TOP_TOOLS: usize = 3;
/// Repository and branch are cut tighter than [`MAX_CELL`]: a long branch name
/// still widens its column, but not enough to push the row's later fields off
/// a typical terminal.
const MAX_PLACE: usize = 28;

pub struct HudLayout {
    pub child: Rect,
    pub strip: Option<Rect>,
}

pub fn partition(rect: Rect, requested: AgentHudMode) -> HudLayout {
    let rows = match requested {
        AgentHudMode::Off => 0,
        _ if rect.width == 0 || rect.height < 8 => 0,
        AgentHudMode::Full if rect.height >= 10 => 4,
        _ => 2,
    };
    let child = Rect {
        height: rect.height.saturating_sub(rows),
        ..rect
    };
    let strip = (rows != 0).then_some(Rect {
        y: child.y + child.height,
        height: rows,
        ..rect
    });
    HudLayout { child, strip }
}

/// The agent's own metrics, `None` until its source is available.
#[derive(Clone, Copy)]
pub enum Metrics<'a> {
    Codex(Option<&'a codex_metrics::Snapshot>),
    Claude {
        snapshot: Option<&'a claude_metrics::Snapshot>,
        /// The read began part-way through the transcript, so its totals
        /// miss what came before.
        tail_only: bool,
        reported: Reported<'a>,
    },
}

/// What Claude Code's statusline reported through `spyc --statusline`: its own
/// figures, which its transcript doesn't carry.
#[derive(Clone, Copy, Default)]
pub struct Reported<'a> {
    /// This session's share of its context window.
    pub context_percent: Option<u8>,
    /// The plan's rate-limit windows: the newest report any tab sent, since the
    /// plan is the account's, not one session's.
    pub plan: Option<&'a crate::agent::claude_statusline::Statusline>,
}

/// What the tab's status hooks have shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hooks {
    /// Events arrived.
    Reported,
    /// Defined at launch; no event yet.
    Unverified,
    /// A startup-only hook config changed after launch.
    RestartNeeded,
    /// Not present at launch.
    Absent,
}

pub struct HudView<'a> {
    pub session: Option<&'a str>,
    /// The tab's activity dot, in its own colour.
    pub dot: Option<Span<'static>>,
    pub attention: &'a str,
    /// What a `waiting` agent waits on.
    pub note: Option<&'a str>,
    pub repository: Option<&'a str>,
    pub branch: Option<&'a str>,
    pub elapsed_secs: Option<u64>,
    pub metrics: Metrics<'a>,
    pub availability: Option<Availability>,
    pub now: Option<Timestamp>,
    pub hooks: Hooks,
    pub mcp_connections: usize,
}

pub fn lines(
    view: &HudView<'_>,
    mode: AgentHudMode,
    width: u16,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if mode == AgentHudMode::Off || width == 0 {
        return Vec::new();
    }
    let p = Palette::of(theme);
    let agent = match view.metrics {
        Metrics::Codex(snapshot) => codex::rows(snapshot, view, p),
        Metrics::Claude {
            snapshot,
            tail_only,
            reported,
        } => claude::rows(snapshot, tail_only, reported, view, p),
    };
    let mut rows = vec![activity(view, p), agent.usage];
    if mode == AgentHudMode::Full {
        let mut mode_row = agent.mode;
        mode_row.cells.push(hooks(view.hooks, p));
        mode_row.cells.push(mcp(view.mcp_connections, p));
        rows.push(agent.tools);
        rows.push(mode_row);
    }
    let mut lines = grid(rows, usize::from(width));
    if theme.mono {
        // Shape and weight carry the meaning; no span keeps a colour.
        for span in lines.iter_mut().flat_map(|line| line.spans.iter_mut()) {
            span.style.fg = Some(Color::Reset);
        }
    }
    lines
}

/// An agent's rows, below the shared activity row.
struct AgentRows {
    usage: Row,
    tools: Row,
    /// The template appends the health cells.
    mode: Row,
}

#[derive(Default)]
struct Row {
    cells: Vec<Field>,
    /// Placed after the last cell and outside the grid, so it widens no column.
    trailer: Option<Field>,
}

impl Row {
    const fn new(cells: Vec<Field>) -> Self {
        Self {
            cells,
            trailer: None,
        }
    }
}

/// Styled spans the grid places but never splits.
#[derive(Clone, Default)]
struct Field(Vec<Span<'static>>);

impl Field {
    fn width(&self) -> usize {
        self.0.iter().map(|span| display_width(&span.content)).sum()
    }

    /// At most `max` columns wide, ending in `…` when cut.
    fn clip(self, max: usize) -> Self {
        if self.width() <= max {
            return self;
        }
        let mut room = max.saturating_sub(1);
        let mut spans = Vec::new();
        let mut last = Style::default();
        for span in self.0 {
            if room == 0 {
                break;
            }
            let kept = display_truncate(&span.content, room).to_owned();
            room -= display_width(&kept);
            last = span.style;
            spans.push(Span::styled(kept, span.style));
        }
        if max > 0 {
            spans.push(Span::styled("…", last));
        }
        Self(spans)
    }
}

/// Lays `rows` out so column `j` starts at the same x on every row. A cell
/// past the right edge is dropped, never wrapped, except in the first two
/// columns: the activity and the session have first claim on the width, so
/// they are cut instead.
fn grid(rows: Vec<Row>, width: usize) -> Vec<Line<'static>> {
    let columns = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
    // The last column moves nothing, so only the width of the strip cuts it.
    let rows: Vec<Row> = rows
        .into_iter()
        .map(|row| Row {
            cells: row
                .cells
                .into_iter()
                .enumerate()
                .map(|(j, c)| if j + 1 < columns { c.clip(MAX_CELL) } else { c })
                .collect(),
            trailer: row.trailer,
        })
        .collect();
    let mut starts = Vec::with_capacity(columns);
    let mut x = 0;
    for j in 0..columns {
        starts.push(x);
        let widest = rows
            .iter()
            .filter_map(|row| row.cells.get(j))
            .map(Field::width)
            .max()
            .unwrap_or(0);
        let floor = if j == 0 { 0 } else { MIN_COLUMN };
        x += widest.max(floor) + GAP;
    }
    rows.into_iter()
        .map(|row| place(row, &starts, width))
        .collect()
}

fn place(row: Row, starts: &[usize], width: usize) -> Line<'static> {
    let mut spans = Vec::new();
    let mut x = 0;
    for (j, cell) in row.cells.into_iter().enumerate() {
        let start = starts[j];
        if start >= width {
            break;
        }
        let room = width - start;
        let cell = if cell.width() <= room {
            cell
        } else if j < 2 {
            cell.clip(room)
        } else {
            break;
        };
        pad(&mut spans, start - x);
        x = start + cell.width();
        spans.extend(cell.0);
    }
    if let Some(trailer) = row.trailer {
        let start = if x == 0 { 0 } else { x + GAP };
        if start < width {
            let trailer = trailer.clip(width - start);
            pad(&mut spans, start - x);
            spans.extend(trailer.0);
        }
    }
    Line::from(spans)
}

fn pad(spans: &mut Vec<Span<'static>>, n: usize) {
    if n > 0 {
        spans.push(Span::raw(" ".repeat(n)));
    }
}

/// The strip's colours, from the theme. Monochrome keeps only the weights.
#[derive(Clone, Copy)]
struct Palette {
    mono: bool,
    label: Color,
    value: Color,
    session: Color,
    repository: Color,
    branch: Color,
    good: Color,
    bad: Color,
    warn: Color,
    active: Color,
}

impl Palette {
    const fn of(theme: &Theme) -> Self {
        Self {
            mono: theme.mono,
            label: theme.status_suffix,
            value: theme.status_path,
            session: theme.status_user,
            repository: theme.dir,
            branch: theme.diff_add_fg,
            good: theme.diff_add_fg,
            bad: theme.diff_del_fg,
            warn: theme.prompt_prefix,
            active: theme.prompt_prefix,
        }
    }

    /// A field's name, or a value that is only a placeholder.
    fn label(self, text: &str) -> Span<'static> {
        let style = Style::default().fg(self.label);
        let style = if self.mono {
            style.add_modifier(Modifier::DIM)
        } else {
            style
        };
        Span::styled(safe_label(text), style)
    }

    fn value(self, text: &str) -> Span<'static> {
        Span::styled(safe_label(text), Style::default().fg(self.value))
    }

    fn strong(self, text: &str) -> Span<'static> {
        Span::styled(
            safe_label(text),
            Style::default().fg(self.value).add_modifier(Modifier::BOLD),
        )
    }

    /// A share of a limit, coloured by how close it is: green, then amber from
    /// half, then red from four fifths.
    fn meter(self, percent: u8) -> Span<'static> {
        let color = match percent {
            0..50 => self.good,
            50..80 => self.warn,
            _ => self.bad,
        };
        self.tint(&format!("{percent}%"), color)
    }

    fn tint(self, text: &str, color: Color) -> Span<'static> {
        Span::styled(safe_label(text), Style::default().fg(color))
    }

    /// `name value`, the name dimmed.
    fn pair(self, name: &str, value: Span<'static>) -> Field {
        Field(vec![self.label(name), Span::raw(" "), value])
    }

    /// Why an agent has no metrics, in place of its usage cells.
    fn unavailable(self, availability: Option<Availability>, file: &str) -> Row {
        let reason = match availability {
            None | Some(Availability::Available) => self.label("awaiting metrics"),
            Some(other) => self.tint(&source_label(other, file), self.warn),
        };
        Row::new(vec![
            Field(vec![self.label("context")]),
            Field(vec![reason]),
        ])
    }
}

/// A stale sample is dimmed, not labelled: an idle session's last report is
/// still its last report.
fn dim_if(mut cell: Field, stale: bool) -> Field {
    if stale {
        for span in &mut cell.0 {
            span.style = span.style.add_modifier(Modifier::DIM);
        }
    }
    cell
}

fn activity(view: &HudView<'_>, p: Palette) -> Row {
    let colour = view.dot.as_ref().and_then(|dot| dot.style.fg);
    let mut first = Vec::new();
    if let Some(dot) = &view.dot {
        first.push(dot.clone());
        first.push(Span::raw(" "));
    }
    first.push(Span::styled(
        safe_label(view.attention),
        Style::default()
            .fg(colour.unwrap_or(p.value))
            .add_modifier(Modifier::BOLD),
    ));
    let session = view.session.map_or_else(
        || p.label("--"),
        |id| p.tint(display_truncate_tail(id, 8), p.session),
    );
    let place = view.repository.map_or_else(
        || Field(vec![p.label("--")]),
        |repository| {
            let mut spans = vec![p.tint(repository, p.repository)];
            if let Some(branch) = view.branch {
                spans.push(p.label(" ⎇ "));
                spans.push(p.tint(branch, p.branch));
            }
            Field(spans).clip(MAX_PLACE)
        },
    );
    let elapsed = view
        .elapsed_secs
        .map_or_else(|| p.label("--"), |secs| p.label(&elapsed_label(secs)));
    Row {
        cells: vec![
            Field(first),
            Field(vec![session]),
            model(view, p),
            place,
            Field(vec![elapsed]),
        ],
        trailer: view.note.map(|note| {
            Field(vec![Span::styled(
                safe_label(note),
                Style::default().fg(colour.unwrap_or(p.value)),
            )])
        }),
    }
}

/// The model and effort, from either agent's latest mode report.
fn model(view: &HudView<'_>, p: Palette) -> Field {
    let reported = match view.metrics {
        Metrics::Codex(snapshot) => snapshot.and_then(|s| s.mode.as_ref()).map(|m| {
            (
                &m.value.model,
                &m.value.effort,
                view.now.is_some_and(|now| m.is_stale_at(now, STALE)),
            )
        }),
        Metrics::Claude { snapshot, .. } => snapshot.and_then(|s| s.mode.as_ref()).map(|m| {
            (
                &m.value.model,
                &m.value.effort,
                view.now.is_some_and(|now| m.is_stale_at(now, STALE)),
            )
        }),
    };
    let Some((model, effort, stale)) = reported else {
        return Field(vec![p.label("model --")]);
    };
    let mut spans = vec![model.as_deref().map_or_else(
        || p.label("model --"),
        |model| p.strong(model.strip_prefix("claude-").unwrap_or(model)),
    )];
    if let Some(effort) = effort.as_deref() {
        spans.push(Span::raw(" "));
        spans.push(p.label(effort));
    }
    dim_if(Field(spans), stale)
}

const STALE: i64 = crate::config::HUD_STALE_AFTER_SECS;

fn hooks(hooks: Hooks, p: Palette) -> Field {
    p.pair(
        "hooks",
        match hooks {
            Hooks::Reported => p.tint("✓", p.good),
            Hooks::Unverified => p.tint("unverified", p.warn),
            Hooks::RestartNeeded => p.tint("restart", p.warn),
            Hooks::Absent => p.label("off"),
        },
    )
}

fn mcp(connections: usize, p: Palette) -> Field {
    p.pair(
        "mcp",
        match connections {
            0 => p.label("--"),
            1 => p.tint("✓", p.good),
            n => p.tint(&format!("✓{n}"), p.good),
        },
    )
}

/// One tool's calls and failures.
struct Tally {
    name: String,
    calls: u32,
    failed: u32,
}

/// The tools row, collapsed: every call, every failure, what is running and
/// the most called. The dump lists every tool. `running` is `None` for an
/// agent whose records can't show a call still running.
fn tools_row(label: &str, tallies: &[Tally], running: Option<&[String]>, p: Palette) -> Row {
    let calls: u32 = tallies.iter().map(|t| t.calls).sum();
    let failed: u32 = tallies.iter().map(|t| t.failed).sum();
    let mut cells = vec![
        Field(vec![p.label(label)]),
        Field(vec![p.strong(&calls.to_string()), p.label(" calls")]),
        if failed == 0 {
            Field(vec![p.label("0 failed")])
        } else {
            Field(vec![
                p.tint(&format!("✗ {failed}"), p.bad),
                p.label(" failed"),
            ])
        },
    ];
    if let Some(running) = running {
        cells.push(match running {
            [] => Field(vec![p.label("◌ none")]),
            [first, rest @ ..] => {
                let mut spans = vec![p.tint("◐ ", p.active), p.value(first)];
                if !rest.is_empty() {
                    spans.push(p.label(&format!(" +{}", rest.len())));
                }
                Field(spans)
            }
        });
    }
    let mut top = Vec::new();
    for (i, tally) in tallies.iter().take(TOP_TOOLS).enumerate() {
        if i > 0 {
            top.push(p.label(" · "));
        }
        if tally.failed == 0 {
            top.push(p.tint("✓ ", p.good));
        }
        top.extend(Field(vec![p.value(&tally.name)]).clip(MAX_TOOL_NAME).0);
        top.push(Span::raw(" "));
        top.push(p.strong(&tally.calls.to_string()));
        if tally.failed > 0 {
            top.push(Span::raw(" "));
            top.push(p.tint(&format!("✗{}", tally.failed), p.bad));
        }
    }
    if !top.is_empty() {
        cells.push(Field(top));
    }
    Row::new(cells)
}

fn elapsed_label(secs: u64) -> String {
    let minutes = secs / 60;
    if minutes < 60 {
        format!("{minutes}m")
    } else {
        format!("{}h{:02}m", minutes / 60, minutes % 60)
    }
}

fn short_count(n: u64) -> String {
    for (scale, suffix) in [(1_000_000_000, "B"), (1_000_000, "M"), (1_000, "k")] {
        if n >= scale {
            return format!("{}.{}{suffix}", n / scale, n % scale / (scale / 10));
        }
    }
    n.to_string()
}

/// Why the metrics are unknown. `file` names the agent's session file.
fn source_label(source: Availability, file: &str) -> String {
    match source {
        Availability::Available => "available".to_owned(),
        Availability::Missing => format!("{file} missing"),
        Availability::Unreadable => format!("{file} unreadable"),
        Availability::IdentityMismatch => "identity mismatch".to_owned(),
        Availability::InvalidHeader => format!("invalid {file} header"),
        Availability::WatchFailed => "watch unavailable".to_owned(),
    }
}

/// Paths and labels can contain control characters; never emit terminal commands.
pub fn safe_label(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::claude_metrics::{self as claude, ToolCount};
    use crate::agent::codex_metrics::{Sample, Snapshot, Source, Tokens};

    fn codex_view(snapshot: Option<&Snapshot>) -> HudView<'_> {
        HudView {
            session: Some("01a133b2-dc16-74c8-af3c-111111111111"),
            dot: None,
            attention: "idle",
            note: None,
            repository: Some("spyc"),
            branch: Some("main"),
            elapsed_secs: None,
            metrics: Metrics::Codex(snapshot),
            availability: snapshot.map(|_| Availability::Available),
            now: None,
            hooks: Hooks::Unverified,
            mcp_connections: 0,
        }
    }

    fn text(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
    }

    /// Where `needle` starts on `line`, in display columns.
    fn x_of(line: &str, needle: &str) -> Option<usize> {
        line.find(needle).map(|i| display_width(&line[..i]))
    }

    fn claude_session(branch: &'static str) -> (claude::Snapshot, &'static str) {
        let at = Timestamp::from_second(1_791_892_800).unwrap();
        let count = |name: &str, calls, failed| ToolCount {
            name: name.into(),
            calls,
            failed,
        };
        let snapshot = claude::Snapshot {
            context_tokens: Some(claude::Sample { at, value: 336_600 }),
            totals: claude::Usage {
                input: 1_000,
                cache_write: 2_400_000,
                cache_read: 118_500_000,
                output: 370_200,
            },
            mode: Some(claude::Sample {
                at,
                value: claude::Mode {
                    model: Some("claude-opus-5-5".into()),
                    effort: Some("xhigh".into()),
                    permission_mode: None,
                },
            }),
            tool_counts: vec![
                count("Read", 141, 0),
                count("mcp__spyc__get_spyc_context", 19, 0),
                count("Edit", 9, 1),
                count("Grep", 6, 0),
            ],
            tools: vec![claude::Tool {
                name: Some("Edit".into()),
                status: claude::ToolStatus::Started,
                at,
                background: false,
            }],
            subagent_calls: 7,
            background_calls: 5,
            tools_complete: true,
            ..claude::Snapshot::default()
        };
        (snapshot, branch)
    }

    fn claude_view<'a>(snapshot: &'a claude::Snapshot, branch: &'a str) -> HudView<'a> {
        HudView {
            session: Some("537fb604-0a1b-478f-be17-3c76df17b585"),
            dot: Some(Span::raw("●")),
            attention: "working",
            note: None,
            repository: Some("spyc"),
            branch: Some(branch),
            elapsed_secs: Some(32 * 60),
            metrics: Metrics::Claude {
                snapshot: Some(snapshot),
                tail_only: false,
                reported: Reported::default(),
            },
            availability: Some(Availability::Available),
            now: None,
            hooks: Hooks::Reported,
            mcp_connections: 1,
        }
    }

    /// The point of the layout: a column starts at the same x on every row
    /// that reaches it, whatever the cells hold and however wide the strip is.
    #[test]
    fn every_column_starts_at_the_same_x_on_every_row() {
        let columns: [&[&str]; 4] = [
            &["●", "df17b585", "opus-5-5", "spyc", "32m"],
            &["context", "336.6k", "out ", "hit ", "compacted "],
            &["tools", "175", "✗", "◐", "✓ Read 141"],
            &["agents", "7", "background", "hooks", "mcp"],
        ];
        for branch in ["main", "feat/a-branch-name-much-longer-than-any-column"] {
            let (snapshot, branch) = claude_session(branch);
            let view = claude_view(&snapshot, branch);
            for width in 20..=200 {
                let rows = text(&lines(&view, AgentHudMode::Full, width, &Theme::default()));
                for row in &rows {
                    assert!(display_width(row) <= usize::from(width), "{row:?}");
                }
                for j in 0..5 {
                    let starts: Vec<usize> = rows
                        .iter()
                        .zip(columns)
                        .filter_map(|(row, cells)| x_of(row, cells.get(j)?))
                        .collect();
                    assert!(
                        starts.windows(2).all(|pair| pair[0] == pair[1]),
                        "column {j} at width {width}: {starts:?}\n{}",
                        rows.join("\n")
                    );
                }
            }
        }
    }

    /// The repository and branch field is capped tighter than most cells, so an
    /// unusually long branch name can only push the columns after it so far.
    #[test]
    fn a_long_branch_name_only_widens_its_column_up_to_max_place() {
        let (snapshot, _) = claude_session("main");
        // The field's own rendered width: from where "spyc" starts to where the
        // next column's "32m" starts, less the gap between them.
        let place_width = |branch: &str| {
            let view = claude_view(&snapshot, branch);
            let rows = text(&lines(&view, AgentHudMode::Full, 200, &Theme::default()));
            let start = x_of(&rows[0], "spyc").unwrap();
            let next = x_of(&rows[0], "32m").unwrap();
            next - start - GAP
        };
        assert_eq!(
            place_width("main"),
            display_width("spyc ⎇ main").max(MIN_COLUMN)
        );
        for branch in [
            "a-branch-name-well-past-the-cap-on-this-field",
            &"x".repeat(200),
        ] {
            assert_eq!(
                place_width(branch),
                MAX_PLACE,
                "branch {branch:?} should be cut to exactly the {MAX_PLACE}-column cap"
            );
        }
    }

    #[test]
    fn a_cell_past_the_edge_is_dropped_whole_and_the_first_two_are_cut() {
        let (snapshot, branch) = claude_session("main");
        let view = claude_view(&snapshot, branch);
        let wide = text(&lines(&view, AgentHudMode::Full, 200, &Theme::default()));
        let edge = x_of(&wide[1], "hit ").unwrap() + 3;
        let narrow = text(&lines(
            &view,
            AgentHudMode::Full,
            edge as u16,
            &Theme::default(),
        ));
        assert!(
            narrow[1].trim_end().ends_with("out 370.2k"),
            "a partial cell is never drawn: {narrow:?}"
        );
        let last = text(&lines(&view, AgentHudMode::Full, 200, &Theme::default()));
        assert!(
            last[2].ends_with("✓ Read 141 · ✓ spyc:get_sp… 19 · Edit 9 ✗1"),
            "the last column is cut only by the strip: {last:?}"
        );
        let tight = text(&lines(&view, AgentHudMode::Compact, 14, &Theme::default()));
        assert!(tight[0].starts_with("● working"));
        assert!(
            tight[0].ends_with('…'),
            "the session is cut, not dropped: {tight:?}"
        );
    }

    #[test]
    fn the_waiting_note_trails_the_activity_row_without_moving_a_column() {
        let (snapshot, branch) = claude_session("main");
        let mut view = claude_view(&snapshot, branch);
        let before = text(&lines(&view, AgentHudMode::Full, 200, &Theme::default()));
        view.attention = "waiting";
        view.note = Some("CI on #641, the long way round");
        let after = text(&lines(&view, AgentHudMode::Full, 200, &Theme::default()));
        assert!(
            after[0].ends_with("32m   CI on #641, the long way round"),
            "the note follows the row's last cell, not a column: {after:?}"
        );
        assert_eq!(before[1..], after[1..]);
    }

    #[test]
    fn monochrome_keeps_the_weights_and_drops_every_colour() {
        let (snapshot, branch) = claude_session("main");
        let view = claude_view(&snapshot, branch);
        let mono = Theme {
            mono: true,
            ..Theme::default()
        };
        let lines = lines(&view, AgentHudMode::Full, 160, &mono);
        let spans: Vec<&Span<'_>> = lines.iter().flat_map(|l| &l.spans).collect();
        assert!(spans.iter().all(|s| s.style.fg == Some(Color::Reset)));
        let style = |text: &str| spans.iter().find(|s| s.content == text).unwrap().style;
        assert!(style("working").add_modifier.contains(Modifier::BOLD));
        assert!(style("context").add_modifier.contains(Modifier::DIM));
        assert!(style("336.6k").add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn health_is_a_tick_when_it_works_and_a_word_when_it_does_not() {
        let (snapshot, branch) = claude_session("main");
        let mut view = claude_view(&snapshot, branch);
        let theme = Theme::default();
        let row = |view: &HudView<'_>| lines(view, AgentHudMode::Full, 200, &theme).pop().unwrap();
        let ok = row(&view);
        assert!(ok.to_string().contains("hooks ✓") && ok.to_string().contains("mcp ✓"));
        let tick = ok.spans.iter().find(|s| s.content == "✓").unwrap();
        assert_eq!(tick.style.fg, Some(theme.diff_add_fg));
        for (hooks, word) in [
            (Hooks::Unverified, "hooks unverified"),
            (Hooks::RestartNeeded, "hooks restart"),
            (Hooks::Absent, "hooks off"),
        ] {
            view.hooks = hooks;
            assert!(row(&view).to_string().contains(word));
        }
        view.mcp_connections = 0;
        assert!(row(&view).to_string().contains("mcp --"));
        view.mcp_connections = 2;
        assert!(row(&view).to_string().contains("mcp ✓2"));
    }

    #[test]
    fn parallel_session_labels_remain_distinct_and_untrusted_labels_cannot_escape() {
        let mut view = codex_view(None);
        view.repository = Some("中文❤️\x1b[31m");
        view.branch = Some("a\tb");
        let first = text(&lines(&view, AgentHudMode::Compact, 20, &Theme::default()))[0].clone();
        view.session = Some("01a133b2-dc16-74c8-af3c-222222222222");
        let second = text(&lines(&view, AgentHudMode::Compact, 20, &Theme::default()))[0].clone();
        assert_ne!(
            first, second,
            "parallel sessions sharing a timestamp prefix need distinct labels"
        );
        view.note = Some("note\x07with a bell");
        let wide = text(&lines(&view, AgentHudMode::Full, 180, &Theme::default()));
        assert!(
            wide[0].contains("中文") && wide[0].contains("❤️"),
            "{wide:?}"
        );
        for row in &wide {
            assert!(
                !row.chars().any(char::is_control),
                "HUD labels must be terminal-safe: {row:?}"
            );
        }
    }

    #[test]
    fn every_height_keeps_six_child_rows_or_hides_the_strip() {
        for height in 0..40 {
            for mode in [AgentHudMode::Off, AgentHudMode::Compact, AgentHudMode::Full] {
                let rect = Rect::new(7, 11, 32, height);
                let parts = partition(rect, mode);
                if let Some(strip) = parts.strip {
                    assert!(parts.child.height >= 6);
                    assert_eq!(strip.y, parts.child.y + parts.child.height);
                    assert_eq!(parts.child.height + strip.height, rect.height);
                    assert_eq!(strip.width, rect.width);
                } else {
                    assert_eq!(parts.child, rect);
                }
            }
        }
        assert_eq!(
            partition(Rect::new(0, 0, 80, 9), AgentHudMode::Full)
                .strip
                .unwrap()
                .height,
            2
        );
        assert!(
            partition(Rect::new(0, 0, 0, 20), AgentHudMode::Full)
                .strip
                .is_none()
        );
    }

    #[test]
    fn unknown_context_stays_unknown_even_when_lifetime_tokens_exceed_the_window() {
        let at = "2026-10-13T12:00:00Z".parse().unwrap();
        let snapshot = Snapshot {
            thread: Some(Sample {
                at,
                source: Source::UsageRecord,
                value: Tokens {
                    total: Some(900_000_000),
                    ..Tokens::default()
                },
            }),
            model_context_window: Some(Sample {
                at,
                source: Source::LegacyTokenCount,
                value: 100,
            }),
            ..Snapshot::default()
        };
        let mut view = codex_view(Some(&snapshot));
        view.attention = "needs you";
        view.session = Some("12345678");
        view.now = Some(at);
        for width in 1..130 {
            for mode in [AgentHudMode::Compact, AgentHudMode::Full] {
                let rendered = text(&lines(&view, mode, width, &Theme::default()));
                assert!(
                    rendered
                        .iter()
                        .all(|line| display_width(line) <= usize::from(width))
                );
                if width >= 24 {
                    assert!(rendered[0].starts_with("needs you"), "{rendered:?}");
                    assert!(rendered[0].contains("12345678"), "{rendered:?}");
                }
                assert!(!rendered.iter().any(|line| line.contains('%')));
            }
        }
        let rendered = text(&lines(&view, AgentHudMode::Full, 160, &Theme::default()));
        assert_eq!(rendered.len(), 4);
        assert!(rendered[1].starts_with("context") && rendered[1].contains(" -- "));
        assert!(rendered[1].contains("thread 900.0M"));
        assert!(rendered[2].starts_with("recent") && rendered[2].contains("0 calls"));
    }
}
