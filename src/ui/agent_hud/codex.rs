//! Codex's cells: separately scoped token totals, with current context and
//! limits unknown until a source reports them, and the tools it was seen
//! calling, which are never a complete list.

use ratatui::text::Span;

use crate::agent::codex_metrics::{LimitState, Sample, Snapshot, Tokens, ToolStatus};

use super::{
    AgentRows, Field, HudView, Palette, Row, STALE, Tally, dim_if, short_count, tools_row,
};

pub(super) fn rows(snapshot: Option<&Snapshot>, view: &HudView<'_>, p: Palette) -> AgentRows {
    let mode = |approval: Option<&str>, sandbox: Option<&str>| {
        let field = |v: Option<&str>| Field(vec![v.map_or_else(|| p.label("--"), |v| p.value(v))]);
        Row::new(vec![
            Field(vec![p.label("mode")]),
            field(approval),
            field(sandbox),
        ])
    };
    let Some(s) = snapshot else {
        return AgentRows {
            usage: p.unavailable(view.availability, "rollout"),
            tools: Row::new(vec![
                Field(vec![p.label("recent")]),
                Field(vec![p.label("--")]),
            ]),
            mode: mode(None, None),
        };
    };
    let reported = s.mode.as_ref().map(|m| &m.value);
    AgentRows {
        usage: usage(s, view, p),
        tools: tools(s, p),
        mode: mode(
            reported.and_then(|m| m.approval.as_deref()),
            reported.and_then(|m| m.sandbox.as_deref()),
        ),
    }
}

fn usage(s: &Snapshot, view: &HudView<'_>, p: Palette) -> Row {
    let tokens = |name: &str, sample: Option<&Sample<Tokens>>| {
        let Some(sample) = sample else {
            return p.pair(name, p.label("--"));
        };
        let value = sample
            .value
            .total
            .map_or_else(|| p.label("--"), |n| p.value(&short_count(n)));
        let stale = view.now.is_some_and(|now| sample.is_stale_at(now, STALE));
        dim_if(p.pair(name, value), stale)
    };
    let mut thread = tokens("thread", s.thread.as_ref().or(s.legacy_thread.as_ref()));
    if s.thread.is_none() && s.legacy_thread.is_some() {
        thread.0.push(Span::raw(" "));
        thread.0.push(p.label("legacy"));
    }
    let limits = s.limits.as_ref().map_or_else(
        || p.pair("limits", p.label("--")),
        |sample| {
            let value = match sample.value {
                LimitState::NotReported => "--",
                LimitState::UnsupportedWindow => "unsupported",
            };
            let stale = view.now.is_some_and(|now| sample.is_stale_at(now, STALE));
            dim_if(p.pair("limits", p.label(value)), stale)
        },
    );
    Row::new(vec![
        Field(vec![p.label("context")]),
        Field(vec![p.label("--")]),
        tokens("req", s.request.as_ref().or(s.legacy_last.as_ref())),
        tokens("turn", s.turn.as_ref()),
        thread,
        limits,
    ])
}

/// Calls seen in the recent observations, most first. Codex's records don't
/// show every completion, so none is claimed to be running.
fn tools(s: &Snapshot, p: Palette) -> Row {
    tools_row("recent", &tallies(s), None, p)
}

/// The recent observations, by tool, most called first.
fn tallies(s: &Snapshot) -> Vec<Tally> {
    let mut tallies: Vec<Tally> = Vec::new();
    for observed in &s.tools {
        let name = observed.name.as_deref().unwrap_or("tool");
        let index = tallies
            .iter()
            .position(|t| t.name == name)
            .unwrap_or_else(|| {
                tallies.push(Tally {
                    name: name.to_owned(),
                    calls: 0,
                    failed: 0,
                });
                tallies.len() - 1
            });
        let tally = &mut tallies[index];
        tally.calls = tally.calls.saturating_add(1);
        if observed.status == ToolStatus::Failed {
            tally.failed = tally.failed.saturating_add(1);
        }
    }
    tallies.sort_by_key(|tally| std::cmp::Reverse(tally.calls));
    tallies
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;

    use super::super::{Hooks, HudView, Metrics, lines};
    use crate::agent::codex_metrics::{
        Availability, Mode, Sample, Snapshot, Source, Tokens, Tool, ToolStatus,
    };
    use crate::config::AgentHudMode;
    use crate::ui::theme::Theme;

    fn rows(snapshot: &Snapshot) -> Vec<String> {
        let view = HudView {
            session: Some("01a133b2-dc16-74c8-af3c-123456789abc"),
            dot: None,
            attention: "working",
            note: None,
            repository: Some("spyc"),
            branch: None,
            elapsed_secs: None,
            metrics: Metrics::Codex(Some(snapshot)),
            availability: Some(Availability::Available),
            now: None,
            hooks: Hooks::Reported,
            mcp_connections: 1,
        };
        lines(&view, AgentHudMode::Full, 160, &Theme::default())
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn codex_fills_the_same_rows_with_its_own_scopes() {
        let at: Timestamp = "2026-10-13T12:00:00Z".parse().unwrap();
        let tool = |name: &str, status| Tool {
            name: Some(name.into()),
            status,
            at,
            elapsed_ms: None,
        };
        let tokens = |total| Sample {
            at,
            source: Source::UsageRecord,
            value: Tokens {
                total: Some(total),
                ..Tokens::default()
            },
        };
        let snapshot = Snapshot {
            request: Some(tokens(1_200)),
            legacy_thread: Some(tokens(5_000_000)),
            mode: Some(Sample {
                at,
                source: Source::TurnContext,
                value: Mode {
                    approval: Some("on-request".into()),
                    sandbox: Some("workspace-write".into()),
                    ..Mode::default()
                },
            }),
            tools: vec![
                tool("exec", ToolStatus::Completed),
                tool("apply_patch", ToolStatus::Started),
                tool("exec", ToolStatus::Failed),
                tool("exec", ToolStatus::Started),
            ],
            ..Snapshot::default()
        };
        let rows = rows(&snapshot);
        assert!(rows[1].contains("req 1.2k") && rows[1].contains("turn --"));
        assert!(rows[1].contains("thread 5.0M legacy"), "{rows:?}");
        assert!(rows[2].starts_with("recent") && rows[2].contains("4 calls"));
        assert!(rows[2].contains("✗ 1 failed"), "{rows:?}");
        assert!(rows[2].ends_with("exec 3 ✗1 · ✓ apply_patch 1"), "{rows:?}");
        assert!(
            !rows[2].contains('◐'),
            "codex never claims a call is running"
        );
        assert!(rows[3].starts_with("mode") && rows[3].contains("on-request"));
        assert!(rows[3].contains("workspace-write") && rows[3].contains("hooks ✓"));
    }
}
