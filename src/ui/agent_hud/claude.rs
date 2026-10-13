//! Claude Code's cells. Its context is a token count, never a percentage: the
//! transcript carries no window size, and the statusline input that does is
//! the user's.

use crate::agent::claude_metrics::{Snapshot, ToolStatus};

use super::{AgentRows, Field, HudView, Palette, Row, Tally, short_count, tool};

pub(super) fn rows(
    snapshot: Option<&Snapshot>,
    tail_only: bool,
    view: &HudView<'_>,
    p: Palette,
) -> AgentRows {
    let Some(s) = snapshot else {
        return AgentRows {
            usage: p.unavailable(view.availability, "transcript"),
            tools: Row::new(vec![
                Field(vec![p.label("tools")]),
                Field(vec![p.label("--")]),
            ]),
            mode: Row::new(vec![
                Field(vec![p.label("agents")]),
                Field(vec![p.label("--")]),
                p.pair("background", p.label("--")),
            ]),
        };
    };
    // A read that began part-way through can only have seen fewer.
    let at_least = if tail_only { "≥" } else { "" };
    AgentRows {
        usage: usage(s, at_least, p),
        tools: tools(s, tail_only, p),
        mode: Row::new(vec![
            Field(vec![p.label("agents")]),
            Field(vec![p.strong(&format!("{at_least}{}", s.subagent_calls))]),
            p.pair(
                "background",
                p.value(&format!("{at_least}{}", s.background_calls)),
            ),
        ]),
    }
}

fn usage(s: &Snapshot, at_least: &str, p: Palette) -> Row {
    // Occupancy holds until the next request, so an idle session's is current.
    let context = s
        .context_tokens
        .as_ref()
        .map_or_else(|| p.label("--"), |c| p.strong(&short_count(c.value)));
    let t = s.totals;
    let sent = t.context();
    let hit = t
        .cache_read
        .saturating_mul(100)
        .checked_div(sent)
        .map_or_else(|| p.label("--"), |pct| p.value(&format!("{pct}%")));
    Row::new(vec![
        Field(vec![p.label("context")]),
        Field(vec![context]),
        p.pair(
            "out",
            p.value(&format!("{at_least}{}", short_count(t.output))),
        ),
        p.pair("hit", hit),
        p.pair(
            "compacted",
            p.value(&format!("{at_least}{}", s.compactions)),
        ),
    ])
}

/// Every tool called since the read began, a running one first.
fn tools(s: &Snapshot, tail_only: bool, p: Palette) -> Row {
    let label = if tail_only { "recent" } else { "tools" };
    let mut cells = vec![Field(vec![p.label(label)])];
    if s.tool_counts.is_empty() {
        cells.push(Field(vec![p.label("none")]));
        return Row::new(cells);
    }
    let mut tallies: Vec<Tally> = s
        .tool_counts
        .iter()
        .map(|count| Tally {
            name: count
                .name
                .strip_prefix("mcp__spyc__")
                .map_or_else(|| count.name.clone(), |rest| format!("spyc:{rest}")),
            calls: count.calls,
            failed: count.failed,
            running: s.tools.iter().any(|t| {
                t.status == ToolStatus::Started && t.name.as_deref() == Some(count.name.as_str())
            }),
        })
        .collect();
    tallies.sort_by_key(|tally| !tally.running);
    cells.extend(tallies.iter().map(|tally| tool(tally, p)));
    Row::new(cells)
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use ratatui::style::Modifier;
    use ratatui::text::Span;

    use super::super::{Hooks, HudView, Metrics, lines};
    use crate::agent::claude_metrics::{
        Availability, Compaction, Mode, Sample, Snapshot, Tool, ToolCount, ToolStatus, Usage,
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
            attention: "working",
            note: None,
            repository: Some("spyc"),
            branch: Some("main"),
            elapsed_secs: Some(32 * 60),
            metrics: Metrics::Claude {
                snapshot,
                tail_only,
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
                "● working   df17b585        opus-5-5 xhigh   spyc ⎇ main    32m",
                "context     336.6k          out 370.2k       hit 98%        compacted 1",
                "tools       ◐ Bash 182 ✗3   Read 41          Edit 23 ✗1     spyc:report_s… 19   Grep 6",
                "agents      1               background 5     hooks ✓        mcp ✓",
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
            compacted[1].starts_with("context     --              out ≥370.2k"),
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
    fn the_tool_row_puts_a_running_call_first_and_failures_in_red() {
        let theme = Theme::default();
        let mut snapshot = session();
        snapshot.tool_counts = vec![count("Read", 41, 0), count("Bash", 18, 2)];
        let line = lines(
            &view(Some(&snapshot), false),
            AgentHudMode::Full,
            120,
            &theme,
        )
        .swap_remove(2);
        let text = line.to_string();
        assert!(
            text.find("◐ Bash 18 ✗2").unwrap() < text.find("Read 41").unwrap(),
            "{text}"
        );
        let style = |content: &str| {
            line.spans
                .iter()
                .find(|s| s.content == content)
                .unwrap()
                .style
        };
        assert_eq!(style("✗2").fg, Some(theme.diff_del_fg));
        assert_eq!(style("◐ ").fg, Some(theme.prompt_prefix));
        snapshot.tools.clear();
        let idle = rows(&view(Some(&snapshot), false), 120);
        assert!(
            idle[2].find("Read 41").unwrap() < idle[2].find("Bash 18 ✗2").unwrap(),
            "with nothing running, the most called leads: {idle:?}"
        );
        assert!(!idle[2].contains('◐'));
        snapshot.tool_counts.clear();
        assert!(rows(&view(Some(&snapshot), false), 120)[2].ends_with("none"));
    }
}
