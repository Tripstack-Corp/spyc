//! Google Antigravity (agy)'s cells.
//!
//! Context is a token count (`input + cache_read`). Usage shows context, output tokens,
//! cache hit percentage, and compaction count.

use crate::agent::agy_metrics::{Snapshot, ToolStatus};

use super::{AgentRows, Field, HudView, Palette, Row, Tally, short_count, tools_row};

pub(super) fn rows(
    snapshot: Option<&Snapshot>,
    tail_only: bool,
    view: &HudView<'_>,
    p: Palette,
) -> AgentRows {
    let Some(s) = snapshot else {
        let usage = p.unavailable(view.availability, "transcript");
        return AgentRows {
            usage,
            tools: Row::new(vec![
                Field(vec![p.heading("tools")]),
                Field(vec![p.label("--")]),
            ]),
            mode: Row::new(vec![
                Field(vec![p.heading("agents")]),
                Field(vec![p.label("--")]),
            ]),
        };
    };

    let at_least = if tail_only { "≥" } else { "" };
    AgentRows {
        usage: usage(s, at_least, p),
        tools: tools(s, tail_only, p),
        mode: mode_row(s, at_least, p),
    }
}

fn mode_row(s: &Snapshot, at_least: &str, p: Palette) -> Row {
    Row::new(vec![
        Field(vec![p.heading("agents")]),
        Field(vec![p.strong(&format!("{at_least}{}", s.subagent_calls))]),
    ])
}

fn usage(s: &Snapshot, at_least: &str, p: Palette) -> Row {
    let context = vec![
        s.context_tokens
            .as_ref()
            .map_or_else(|| p.label("--"), |c| p.strong(&short_count(c.value))),
    ];
    let t = s.totals;
    let sent = t.context();
    let hit = t
        .cache_read
        .saturating_mul(100)
        .checked_div(sent)
        .map_or_else(|| p.label("--"), |pct| p.value(&format!("{pct}%")));
    let cells = vec![
        Field(vec![p.heading("context")]),
        Field(context),
        p.pair(
            "out",
            p.value(&format!("{at_least}{}", short_count(t.output))),
        ),
        p.pair("hit", hit),
        p.pair(
            "compacted",
            p.value(&format!("{at_least}{}", s.compactions)),
        ),
    ];
    Row::new(cells)
}

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

fn display_name(name: &str) -> String {
    name.strip_prefix("mcp_spyc_")
        .map_or_else(|| name.to_owned(), |rest| format!("spyc:{rest}"))
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use ratatui::text::Span;

    use super::super::{Attention, Hooks, HudView, Metrics, lines};
    use crate::agent::agy_metrics::{
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

    fn session() -> Snapshot {
        Snapshot {
            context_tokens: Some(Sample {
                at: at(1),
                value: 120_000,
            }),
            totals: Usage {
                input: 10_000,
                cache_write: 0,
                cache_read: 190_000,
                output: 25_000,
            },
            messages: 42,
            compactions: 1,
            mode: Some(Sample {
                at: at(1),
                value: Mode {
                    model: Some("gemini-2.5-pro".into()),
                },
            }),
            tool_counts: vec![
                count("run_command", 15, 1),
                count("view_file", 10, 0),
                count("mcp_spyc_get_context", 5, 0),
            ],
            tools: vec![Tool {
                name: Some("run_command".into()),
                status: ToolStatus::Started,
                at: at(2),
            }],
            subagent_calls: 3,
            tools_complete: true,
            ..Snapshot::default()
        }
    }

    fn view(snapshot: Option<&Snapshot>, tail_only: bool) -> HudView<'_> {
        HudView {
            session: Some("00000000-0000-0000-0000-000000000001"),
            dot: Some(Span::raw("●")),
            attention: Attention::Working,
            note: None,
            repository: Some("spyc"),
            branch: Some("main"),
            elapsed_secs: Some(15 * 60),
            metrics: Metrics::Agy {
                snapshot,
                tail_only,
            },
            availability: snapshot.map(|_| Availability::Available),
            now: Some(at(10)),
            local_tz: None,
            resets: crate::config::AgentHudResets::Clock,
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
        let rendered = rows(&view(Some(&snapshot), false), 120);
        assert_eq!(rendered.len(), 4);
        assert!(rendered[0].contains("gemini-2.5-pro"));
        assert!(rendered[1].contains("context"));
        assert!(rendered[1].contains("120.0k"));
        assert!(rendered[1].contains("out 25.0k"));
        assert!(rendered[1].contains("hit 95%"));
        assert!(rendered[1].contains("compacted 1"));
        assert!(rendered[2].contains("tools"));
        assert!(rendered[2].contains("30 calls"));
        assert!(rendered[3].contains("agents        3"));
        assert!(rendered[3].contains("hooks ✓"));
        assert!(rendered[3].contains("mcp ✓"));
    }

    #[test]
    fn context_is_a_token_count_and_a_compaction_forgets_it() {
        let mut snapshot = session();
        snapshot.context_tokens = None;
        snapshot.compaction = Some(Sample {
            at: at(2),
            value: Compaction,
        });
        snapshot.compactions = 2;
        let compacted = rows(&view(Some(&snapshot), true), 160);
        assert!(compacted[1].contains("context       --"));
        assert!(compacted[1].contains("compacted ≥2"));
    }

    #[test]
    fn an_unavailable_transcript_says_why_and_shows_nothing_measured() {
        let mut missing = view(None, false);
        missing.availability = Some(Availability::Missing);
        let rendered = rows(&missing, 120);
        assert!(rendered[1].contains("transcript missing"));
        assert!(rendered[2].contains("--"));
        assert!(rendered[3].contains("agents        --"));
    }
}
