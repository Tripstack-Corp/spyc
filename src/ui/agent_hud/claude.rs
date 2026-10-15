//! Claude Code's cells. Its context is a token count, never a percentage: the
//! transcript carries no window size, and the statusline input that does is
//! the user's.

use crate::agent::claude_metrics::{Plan, Sample, Snapshot, ToolStatus};

use ratatui::text::Span;

use super::{AgentRows, Field, HudView, Palette, Reported, Row, Tally, short_count, tools_row};

pub(super) fn rows(
    snapshot: Option<&Snapshot>,
    tail_only: bool,
    reported: Reported<'_>,
    view: &HudView<'_>,
    p: Palette,
) -> AgentRows {
    let Some(s) = snapshot else {
        let mut usage = p.unavailable(view.availability, "transcript");
        // Claude Code's own share needs no transcript.
        if let Some(percent) = reported.context_percent {
            let share = Field(vec![p.label("--"), Span::raw(" "), p.meter(percent)]);
            usage.cells.insert(1, share);
        }
        usage.cells.extend(plan(reported, p));
        return AgentRows {
            usage,
            tools: Row::new(vec![
                Field(vec![p.heading("tools")]),
                Field(vec![p.label("--")]),
            ]),
            mode: Row::new(vec![
                Field(vec![p.heading("agents")]),
                Field(vec![p.label("--")]),
                p.pair("background", p.label("--")),
            ]),
        };
    };
    // A read that began part-way through can only have seen fewer.
    let at_least = if tail_only { "≥" } else { "" };
    AgentRows {
        usage: usage(s, at_least, reported, p),
        tools: tools(s, tail_only, p),
        mode: mode_row(s, at_least, p),
    }
}

fn mode_row(s: &Snapshot, at_least: &str, p: Palette) -> Row {
    let mut cells = vec![
        Field(vec![p.heading("agents")]),
        Field(vec![p.strong(&format!("{at_least}{}", s.subagent_calls))]),
        p.pair(
            "background",
            p.value(&format!("{at_least}{}", s.background_calls)),
        ),
    ];
    if let Some(cell) = todo_cell(s.plan.as_ref(), at_least, p) {
        cells.push(cell);
    }
    Row::new(cells)
}

/// `TodoWrite`'s list, or `TaskCreate`/`TaskUpdate`'s tasks — named `todo`, not
/// `plan`, since that already names the account's rate-limit windows here.
/// Omitted, not `--`, until one has been seen: most sessions use neither tool.
fn todo_cell(plan: Option<&Sample<Plan>>, at_least: &str, p: Palette) -> Option<Field> {
    let plan = &plan?.value;
    let total = plan
        .pending
        .saturating_add(plan.in_progress)
        .saturating_add(plan.completed);
    if total == 0 {
        return None;
    }
    let mut field = p.pair(
        "todo",
        p.strong(&format!("{at_least}{}/{total}", plan.completed)),
    );
    if plan.in_progress > 0 {
        field
            .0
            .push(p.label(&format!(" ({} active)", plan.in_progress)));
    }
    Some(field)
}

fn usage(s: &Snapshot, at_least: &str, reported: Reported<'_>, p: Palette) -> Row {
    // Occupancy holds until the next request, so an idle session's is current.
    let mut context = vec![
        s.context_tokens
            .as_ref()
            .map_or_else(|| p.label("--"), |c| p.strong(&short_count(c.value))),
    ];
    if let Some(percent) = reported.context_percent {
        context.push(Span::raw(" "));
        context.push(p.meter(percent));
    }
    let t = s.totals;
    let sent = t.context();
    let hit = t
        .cache_read
        .saturating_mul(100)
        .checked_div(sent)
        .map_or_else(|| p.label("--"), |pct| p.value(&format!("{pct}%")));
    let mut cells = vec![Field(vec![p.heading("context")]), Field(context)];
    cells.extend(plan(reported, p));
    cells.extend([
        p.pair(
            "out",
            p.value(&format!("{at_least}{}", short_count(t.output))),
        ),
        p.pair("hit", hit),
        p.pair(
            "compacted",
            p.value(&format!("{at_least}{}", s.compactions)),
        ),
    ]);
    Row::new(cells)
}

/// The plan's five-hour and seven-day windows, those Claude Code reported.
fn plan(reported: Reported<'_>, p: Palette) -> Vec<Field> {
    let Some(plan) = reported.plan else {
        return Vec::new();
    };
    [("5h", plan.five_hour), ("7d", plan.seven_day)]
        .into_iter()
        .filter_map(|(name, window)| Some(p.pair(name, p.meter(window?.used_percent))))
        .collect()
}

/// Every tool called since the read began, most called first.
fn tools(s: &Snapshot, tail_only: bool, p: Palette) -> Row {
    let tallies: Vec<Tally> = s
        .tool_counts
        .iter()
        .map(|count| Tally {
            name: display_name(&count.name),
            calls: count.calls,
            failed: count.failed,
        })
        .collect();
    let mut running: Vec<String> = Vec::new();
    for tool in s.tools.iter().filter(|t| t.status == ToolStatus::Started) {
        let name = display_name(tool.name.as_deref().unwrap_or("tool"));
        if !running.contains(&name) {
            running.push(name);
        }
    }
    let label = if tail_only { "recent" } else { "tools" };
    tools_row(label, &tallies, Some(&running), p)
}

/// spyc's own MCP tools read `spyc:` rather than their full MCP name.
fn display_name(name: &str) -> String {
    name.strip_prefix("mcp__spyc__")
        .map_or_else(|| name.to_owned(), |rest| format!("spyc:{rest}"))
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use ratatui::style::Modifier;
    use ratatui::text::Span;

    use super::super::{Attention, Hooks, HudView, Metrics, lines};
    use crate::agent::claude_metrics::{
        Availability, Compaction, Mode, Plan, Sample, Snapshot, Tool, ToolCount, ToolStatus, Usage,
    };
    use crate::config::AgentHudMode;
    use crate::ui::theme::Theme;

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(1_791_892_800 + second).unwrap()
    }

    fn count(name: &str, calls: u32, failed: u32) -> ToolCount {
        ToolCount {
            name: name.into(),
            calls,
            failed,
        }
    }

    /// The screenshot's session: a compaction behind it, a call running.
    fn session() -> Snapshot {
        Snapshot {
            context_tokens: Some(Sample {
                at: at(1),
                value: 336_600,
            }),
            totals: Usage {
                input: 1_000,
                cache_write: 2_400_000,
                cache_read: 118_500_000,
                output: 370_200,
            },
            messages: 900,
            compactions: 1,
            mode: Some(Sample {
                at: at(1),
                value: Mode {
                    model: Some("claude-opus-5-5".into()),
                    effort: Some("xhigh".into()),
                    permission_mode: Some("auto".into()),
                },
            }),
            tool_counts: vec![
                count("Bash", 182, 3),
                count("Read", 41, 0),
                count("Edit", 23, 1),
                count("mcp__spyc__report_status", 19, 0),
                count("Grep", 6, 0),
            ],
            tools: vec![Tool {
                name: Some("Bash".into()),
                status: ToolStatus::Started,
                at: at(1),
                background: false,
            }],
            subagent_calls: 1,
            background_calls: 5,
            tools_complete: true,
            ..Snapshot::default()
        }
    }

    fn view(snapshot: Option<&Snapshot>, tail_only: bool) -> HudView<'_> {
        HudView {
            session: Some("537fb604-0a1b-478f-be17-3c76df17b585"),
            dot: Some(Span::raw("●")),
            attention: Attention::Working,
            note: None,
            repository: Some("spyc"),
            branch: Some("main"),
            elapsed_secs: Some(32 * 60),
            metrics: Metrics::Claude {
                snapshot,
                tail_only,
                reported: super::super::Reported::default(),
            },
            availability: snapshot.map(|_| Availability::Available),
            now: Some(at(10)),
            hooks: Hooks::Reported,
            mcp_connections: 1,
        }
    }

    fn rows(view: &HudView<'_>, width: u16) -> Vec<String> {
        lines(view, AgentHudMode::Full, width, &Theme::default())
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn the_full_strip_reads_as_aligned_columns() {
        let snapshot = session();
        assert_eq!(
            rows(&view(Some(&snapshot), false), 120),
            [
                "● working     df17b585       opus-5-5 xhigh   spyc ⎇ main    32m",
                "context       336.6k         out 370.2k       hit 98%        compacted 1",
                "tools         271 calls      ✗ 4 failed       ◐ Bash         Bash 182 ✗3 · ✓ Read 41 · Edit 23 ✗1",
                "agents        1              background 5     hooks ✓        mcp ✓",
            ]
        );
    }

    #[test]
    fn context_is_a_token_count_and_a_compaction_forgets_it() {
        let mut snapshot = session();
        let live = rows(&view(Some(&snapshot), false), 160);
        assert!(
            !live[1].contains("336.6k%"),
            "no window size, so no percentage"
        );
        snapshot.context_tokens = None;
        snapshot.compaction = Some(Sample {
            at: at(2),
            value: Compaction {
                trigger: Some("auto".into()),
                pre_tokens: Some(442_230),
            },
        });
        snapshot.compactions = 2;
        let compacted = rows(&view(Some(&snapshot), true), 160);
        assert!(
            compacted[1].starts_with("context       --             out ≥370.2k"),
            "lifetime totals survive; a tail-only read says they may be more: {compacted:?}"
        );
        assert!(compacted[1].ends_with("compacted ≥2"));
        assert!(compacted[2].starts_with("recent "), "{compacted:?}");
    }

    #[test]
    fn an_unavailable_transcript_says_why_and_shows_nothing_measured() {
        let mut missing = view(None, false);
        missing.availability = Some(Availability::Missing);
        let missing = rows(&missing, 120);
        assert!(missing[0].contains("model --"), "{missing:?}");
        assert!(missing[1].starts_with("context") && missing[1].ends_with("transcript missing"));
        assert!(missing[2].starts_with("tools") && missing[2].ends_with("--"));
        assert!(missing[3].contains("background --"), "{missing:?}");
        let awaiting = rows(&view(None, false), 120);
        assert!(awaiting[1].ends_with("awaiting metrics"));
    }

    #[test]
    fn a_stale_model_is_dimmed_not_labelled() {
        let snapshot = session();
        let mut late = view(Some(&snapshot), false);
        let model_style = |view: &HudView<'_>| {
            let line = lines(view, AgentHudMode::Compact, 120, &Theme::default()).swap_remove(0);
            assert!(!line.to_string().contains("stale"));
            line.spans
                .iter()
                .find(|s| s.content == "opus-5-5")
                .unwrap()
                .style
        };
        assert!(!model_style(&late).add_modifier.contains(Modifier::DIM));
        late.now = Some(at(62));
        assert!(model_style(&late).add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn the_todo_cell_shows_completed_over_total_and_in_progress_when_any() {
        let mut snapshot = session();
        snapshot.plan = Some(Sample {
            at: at(1),
            value: Plan {
                pending: 1,
                in_progress: 2,
                completed: 4,
            },
        });
        let full = rows(&view(Some(&snapshot), false), 200);
        assert!(full[3].contains("todo 4/7 (2 active)"), "{full:?}");
        snapshot.plan.as_mut().unwrap().value.in_progress = 0;
        let idle = rows(&view(Some(&snapshot), false), 200);
        assert!(
            idle[3].contains("todo 4/5") && !idle[3].contains("active"),
            "{idle:?}"
        );
        snapshot.plan.as_mut().unwrap().value = Plan::default();
        let empty = rows(&view(Some(&snapshot), false), 200);
        assert!(
            !empty[3].contains("todo"),
            "nothing reported is omitted, not shown as --: {empty:?}"
        );
        assert!(!rows(&view(None, false), 200)[3].contains("todo"));
    }

    #[test]
    fn the_tool_row_collapses_to_totals_the_running_call_and_the_most_called() {
        let theme = Theme::default();
        let mut snapshot = session();
        snapshot.tool_counts = vec![
            count("Read", 41, 0),
            count("Bash", 18, 2),
            count("Grep", 3, 0),
        ];
        snapshot.tools.push(Tool {
            name: Some("mcp__spyc__report_status".into()),
            status: ToolStatus::Started,
            at: at(2),
            background: false,
        });
        let line = lines(
            &view(Some(&snapshot), false),
            AgentHudMode::Full,
            160,
            &theme,
        )
        .swap_remove(2);
        let text = line.to_string();
        assert!(text.contains("62 calls"), "{text}");
        assert!(
            text.contains("◐ Bash +1"),
            "two running, the first named: {text}"
        );
        assert!(
            text.ends_with("✓ Read 41 · Bash 18 ✗2 · ✓ Grep 3"),
            "{text}"
        );
        let style = |content: &str| {
            line.spans
                .iter()
                .find(|s| s.content == content)
                .unwrap()
                .style
        };
        assert_eq!(style("✗ 2").fg, Some(theme.diff_del_fg));
        assert_eq!(style("◐ ").fg, Some(theme.prompt_prefix));
        assert_eq!(style("✓ ").fg, Some(theme.diff_add_fg));
        snapshot.tools.clear();
        let idle = rows(&view(Some(&snapshot), false), 160);
        assert!(
            idle[2].contains("◌ none") && !idle[2].contains('◐'),
            "{idle:?}"
        );
        snapshot.tool_counts.clear();
        let none = rows(&view(Some(&snapshot), false), 160);
        assert!(
            none[2].contains("0 calls") && none[2].contains("0 failed"),
            "{none:?}"
        );
    }

    /// Claude Code's own figures, when its statusline reported them: this
    /// session's context share beside the token count, then the plan's windows,
    /// each coloured by how close it is to its limit.
    #[test]
    fn reported_shares_follow_the_context_and_are_coloured_by_level() {
        use crate::agent::claude_statusline::{RateWindow, Statusline};
        let theme = Theme::default();
        let snapshot = session();
        let plan = Statusline {
            five_hour: Some(RateWindow {
                used_percent: 23,
                resets_at: None,
            }),
            seven_day: Some(RateWindow {
                used_percent: 85,
                resets_at: None,
            }),
            ..Statusline::default()
        };
        let mut v = view(Some(&snapshot), false);
        v.metrics = Metrics::Claude {
            snapshot: Some(&snapshot),
            tail_only: false,
            reported: super::super::Reported {
                context_percent: Some(55),
                plan: Some(&plan),
            },
        };
        let line = lines(&v, AgentHudMode::Compact, 200, &theme).swap_remove(1);
        let text = line.to_string();
        assert!(text.starts_with("context       336.6k 55%"), "{text}");
        assert!(
            text.find("5h 23%").unwrap() < text.find("7d 85%").unwrap()
                && text.find("7d 85%").unwrap() < text.find("out ").unwrap(),
            "{text}"
        );
        let fg = |content: &str| {
            line.spans
                .iter()
                .find(|s| s.content == content)
                .unwrap()
                .style
                .fg
        };
        assert_eq!(fg("23%"), Some(theme.diff_add_fg));
        assert_eq!(fg("55%"), Some(theme.prompt_prefix));
        assert_eq!(fg("85%"), Some(theme.diff_del_fg));
        let plain = rows(&view(Some(&snapshot), false), 200);
        assert!(
            !plain[1].contains('%') || plain[1].contains("hit "),
            "{plain:?}"
        );
        assert!(
            !plain[1].contains("5h") && !plain[1].contains("7d"),
            "nothing reported, nothing shown"
        );
    }
}
