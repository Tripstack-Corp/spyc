//! Claude Code's rows. Its context is a token count, never a percentage: the
//! transcript carries no window size, and the statusline input that does is
//! the user's.

use crate::agent::claude_metrics::{Availability, Snapshot, Tool, ToolStatus};

use super::{HudView, short_count, source_label};

pub(super) fn metrics(
    snapshot: Option<&Snapshot>,
    tail_only: bool,
    availability: Option<Availability>,
) -> String {
    let Some(s) = snapshot else {
        return format!(
            "Context -- | Usage -- | {}",
            source_label(availability, "transcript")
        );
    };
    // Occupancy holds until the next request, so an idle session's is current.
    let context = match (&s.context_tokens, &s.compaction) {
        (Some(sample), _) => short_count(sample.value),
        (None, Some(_)) => "-- (compacted)".to_owned(),
        (None, None) => "--".to_owned(),
    };
    let t = s.totals;
    let total = t.context().saturating_add(t.output);
    let partial = if tail_only { "; partial" } else { "" };
    format!(
        "Context {context} | Session {} (out {}{partial}) | Compactions {}",
        short_count(total),
        short_count(t.output),
        s.compactions
    )
}

pub(super) fn operating_mode(snapshot: Option<&Snapshot>, view: &HudView<'_>) -> String {
    let permission = snapshot
        .and_then(|s| s.mode.as_ref())
        .and_then(|s| s.value.permission_mode.as_deref())
        .unwrap_or("--");
    format!(
        "Mode: {permission} | hooks: {} | MCP: {} observed",
        view.hooks, view.mcp_connections
    )
}

pub(super) fn tools(snapshot: Option<&Snapshot>, tail_only: bool) -> String {
    let Some(s) = snapshot else {
        return "Tools: --".to_owned();
    };
    let partial = if tail_only || !s.tools_complete {
        " (partial)"
    } else {
        ""
    };
    let recent = if s.tools.is_empty() {
        "none".to_owned()
    } else {
        s.tools
            .iter()
            .rev()
            .take(2)
            .map(tool)
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "Tools{partial}: {recent} | Agents {} | Background {}",
        s.subagent_calls, s.background_calls
    )
}

fn tool(tool: &Tool) -> String {
    let name = tool.name.as_deref().unwrap_or("tool");
    let name = name
        .strip_prefix("mcp__spyc__")
        .map_or_else(|| name.to_owned(), |rest| format!("spyc:{rest}"));
    let status = match tool.status {
        ToolStatus::Started => "running",
        // A background call's result only says it began.
        ToolStatus::Completed if tool.background => "started in background",
        ToolStatus::Completed => "completed",
        ToolStatus::Failed => "failed",
    };
    format!("{name} {status}")
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;

    use super::super::{HudView, Metrics, lines};
    use crate::agent::claude_metrics::{
        Availability, Compaction, Mode, Sample, Snapshot, Tool, ToolStatus, Usage,
    };
    use crate::config::AgentHudMode;
    use crate::ui::{display_width, theme::Theme};

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(1_791_892_800 + second).unwrap()
    }

    fn view(snapshot: Option<&Snapshot>, tail_only: bool) -> HudView<'_> {
        HudView {
            session: Some("c1a0de00-0000-4000-8000-000000000001"),
            attention: "working",
            needs_user: false,
            repository: Some("spyc"),
            branch: Some("main"),
            elapsed_secs: Some(600),
            metrics: Metrics::Claude {
                snapshot,
                tail_only,
            },
            availability: snapshot.map(|_| Availability::Available),
            now: Some(at(10)),
            hooks: "events reported",
            mcp_connections: 1,
        }
    }

    fn rows(view: &HudView<'_>) -> Vec<String> {
        lines(view, AgentHudMode::Full, 200, &Theme::default())
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// The spike's session: 441,725 tokens in context, then an auto-compaction.
    fn session() -> Snapshot {
        let request = Usage {
            input: 6,
            cache_write: 1_200,
            cache_read: 440_519,
            output: 505,
        };
        Snapshot {
            request: Some(Sample {
                at: at(1),
                value: request,
            }),
            context_tokens: Some(Sample {
                at: at(1),
                value: request.context(),
            }),
            totals: Usage {
                input: 1_000,
                cache_write: 2_000_000,
                cache_read: 39_000_000,
                output: 120_300,
            },
            messages: 246,
            mode: Some(Sample {
                at: at(1),
                value: Mode {
                    model: Some("claude-opus-5-5".into()),
                    effort: Some("xhigh".into()),
                    permission_mode: Some("auto".into()),
                },
            }),
            tools_complete: true,
            ..Snapshot::default()
        }
    }

    #[test]
    fn context_is_a_token_count_and_a_compaction_forgets_it() {
        let mut snapshot = session();
        let live = rows(&view(Some(&snapshot), false));
        assert!(live[0].contains("claude-opus-5-5 · xhigh"), "{live:?}");
        assert_eq!(
            live[1],
            "Context 441.7k | Session 41.1M (out 120.3k) | Compactions 0"
        );
        assert!(live[2].starts_with("Mode: auto | hooks: events reported | MCP: 1"));

        snapshot.context_tokens = None;
        snapshot.compaction = Some(Sample {
            at: at(2),
            value: Compaction {
                trigger: Some("auto".into()),
                pre_tokens: Some(442_230),
            },
        });
        snapshot.compactions = 1;
        let compacted = rows(&view(Some(&snapshot), true));
        assert_eq!(
            compacted[1],
            "Context -- (compacted) | Session 41.1M (out 120.3k; partial) | Compactions 1",
            "lifetime totals survive; a tail-only read says they are partial"
        );
        for width in 1..130 {
            for mode in [AgentHudMode::Compact, AgentHudMode::Full] {
                let view = view(Some(&snapshot), true);
                for line in lines(&view, mode, width, &Theme::default()) {
                    let text = line.to_string();
                    assert!(display_width(&text) <= usize::from(width), "{text:?}");
                    assert!(!text.contains('%'), "no window size, so no percentage");
                }
            }
        }
    }

    #[test]
    fn an_unavailable_transcript_says_why_and_shows_nothing_measured() {
        let mut missing = view(None, false);
        missing.availability = Some(Availability::Missing);
        let missing = rows(&missing);
        assert_eq!(missing[1], "Context -- | Usage -- | transcript missing");
        assert_eq!(
            missing[2],
            "Mode: -- | hooks: events reported | MCP: 1 observed"
        );
        assert_eq!(missing[3], "Tools: --");
        assert!(!missing[0].contains("model"), "no model before a record");
        let awaiting = rows(&view(None, false));
        assert!(awaiting[1].ends_with("awaiting pinned metrics"));
    }

    #[test]
    fn a_stale_model_is_marked_and_monochrome_has_no_colour() {
        let snapshot = session();
        let mut late = view(Some(&snapshot), false);
        assert!(!rows(&late)[0].contains("(stale)"));
        late.now = Some(at(62));
        assert!(rows(&late)[0].contains("claude-opus-5-5 · xhigh (stale)"));
        let mono = Theme {
            mono: true,
            ..Theme::default()
        };
        for line in lines(&late, AgentHudMode::Full, 120, &mono) {
            for span in &line.spans {
                assert_eq!(span.style.fg, Some(ratatui::style::Color::Reset));
            }
        }
    }

    #[test]
    fn the_tool_row_names_the_latest_calls_and_says_when_it_is_partial() {
        let tool = |second, name: &str, status, background| Tool {
            name: Some(name.into()),
            status,
            at: at(second),
            background,
        };
        let mut snapshot = session();
        assert_eq!(
            rows(&view(Some(&snapshot), false))[3],
            "Tools: none | Agents 0 | Background 0"
        );
        snapshot.tools = vec![
            tool(1, "Read", ToolStatus::Failed, false),
            tool(2, "Bash", ToolStatus::Completed, true),
            tool(3, "mcp__spyc__report_status", ToolStatus::Started, false),
        ];
        snapshot.subagent_calls = 2;
        snapshot.background_calls = 1;
        assert_eq!(
            rows(&view(Some(&snapshot), false))[3],
            "Tools: spyc:report_status running, Bash started in background | Agents 2 | Background 1"
        );
        snapshot.tools_complete = false;
        assert!(rows(&view(Some(&snapshot), false))[3].starts_with("Tools (partial): "));
    }
}
