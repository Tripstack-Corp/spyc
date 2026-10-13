//! Explicit source diagnostics retain only the collector's bounded projection.

use jiff::Timestamp;

use crate::agent::claude_metrics;
use crate::agent::codex_metrics::{Sample, Snapshot, Tokens};
use crate::agent::tail::Availability;
use crate::config::HUD_STALE_AFTER_SECS;

use super::super::state::agent_hud::{Feed, Update};
use super::App;

pub(super) fn lines(app: &App) -> Vec<String> {
    diagnostic_lines(app)
        .iter()
        .map(|line| crate::ui::agent_hud::safe_label(line))
        .collect()
}

/// The mode in force and where it came from.
fn mode_line(app: &App) -> String {
    let source = if app.state.pane.agent_hud.choice.is_some() {
        "chosen with :agent hud; restored by -r"
    } else {
        "from the config file"
    };
    format!(
        "agent HUD: {} ({source})",
        app.agent_hud_preference().as_str()
    )
}

fn diagnostic_lines(app: &App) -> Vec<String> {
    let mut out = vec![mode_line(app)];
    let (cols, rows) = app.view.term_size;
    let layout = app.frame_layout(ratatui::layout::Rect::new(0, 0, cols, rows));
    out.push(match layout.agent_hud {
        Some(rect) => format!(
            "visible: {} rows; child: {} rows",
            rect.height,
            layout.pane.map_or(0, |p| p.height)
        ),
        None => {
            "not visible: off, hidden, short, closed or not an active Codex or Claude Code pane"
                .to_owned()
        }
    });
    out.push("Context occupancy: unknown (no verified native percentage).".to_owned());
    let Some(tabs) = app.runtime.pane_tabs.as_ref() else {
        out.push("No active pane.".to_owned());
        return out;
    };
    let info = tabs.active_info();
    let tilde = |home: Option<&std::path::PathBuf>| {
        home.map_or("--".to_owned(), |p| crate::paths::display_tilde(p))
    };
    out.push(format!("pane: {}", info.id));
    if super::feed(info.agent().kind()) == Some(Feed::Claude) {
        out.push(format!(
            "reported session: {}",
            info.live_session_id.as_deref().unwrap_or("--")
        ));
        out.push(format!(
            "Claude home (spyc's environment): {}",
            tilde(app.state.pane.agent_hud.claude_home.as_ref())
        ));
        out.extend(statusline_lines(tabs, info));
    } else {
        out.push(format!(
            "pinned thread: {}",
            info.codex_session_id.as_deref().unwrap_or("--")
        ));
        out.push(format!(
            "captured home: {}",
            tilde(info.codex_home.as_ref())
        ));
    }
    out.push(format!(
        "hooks present at launch: {}; restart needed: {}; reported events: {}",
        info.status_hooks_at_spawn,
        info.status_hooks_restart_needed,
        info.recent_hook_events.len()
    ));
    out.push(format!(
        "observed bound MCP connections: {}",
        app.view
            .activity
            .mcp_connections
            .values()
            .filter(|c| c.pane_id.as_deref() == Some(info.id.as_str()))
            .count()
    ));
    let Some(update) = app.active_agent_hud_update() else {
        out.push(
            "Collection has no matching outcome. Only a visible, enabled strip subscribes."
                .to_owned(),
        );
        return out;
    };
    out.push(format!(
        "source: {:?}; generation: {}; bounded tail: {}",
        update.availability(),
        update.generation(),
        update.tail_only()
    ));
    if update.availability() == Availability::Missing {
        let file = match update {
            Update::Codex(_) => "Pinned rollout",
            Update::Claude(_) => "Session transcript",
        };
        out.push(format!(
            "{file} unavailable in bounded lookup; toggle the strip to retry."
        ));
    }
    let work = app.runtime.agent_hud.as_ref().map_or(
        app.state.pane.agent_hud.work,
        super::collector::Collector::work,
    );
    out.push(format!(
        "collector bytes read: {}; refreshes: {}",
        work.0, work.1
    ));
    let now = Timestamp::now();
    out.extend(match update {
        Update::Codex(u) => metric_lines(&u.snapshot, now),
        Update::Claude(u) => claude_lines(&u.snapshot, u.tail_only, now),
    });
    out
}

fn metric_lines(snapshot: &Snapshot, now: Timestamp) -> Vec<String> {
    let mut out = Vec::new();
    for (name, sample) in [
        ("request", &snapshot.request),
        ("turn", &snapshot.turn),
        ("thread", &snapshot.thread),
        ("legacy last", &snapshot.legacy_last),
        ("legacy thread", &snapshot.legacy_thread),
    ] {
        out.push(usage(name, sample.as_ref(), now));
    }
    out.push(snapshot.mode.as_ref().map_or("mode: --".to_owned(), |s| {
        format!("mode: {:?}; {}", s.value, freshness(s, now))
    }));
    out.push(
        snapshot
            .limits
            .as_ref()
            .map_or("limits: not reported".to_owned(), |s| {
                format!("limits: {:?}; {}", s.value, freshness(s, now))
            }),
    );
    out.push(snapshot.model_context_window.as_ref().map_or(
        "reported model window: --".to_owned(),
        |s| {
            format!(
                "reported model window: {}; {}; not current occupancy",
                s.value,
                freshness(s, now)
            )
        },
    ));
    out.push(
        snapshot
            .limit_bucket
            .as_ref()
            .map_or("limit bucket: --".to_owned(), |s| {
                format!(
                    "limit bucket: {}; {}; shared account/bucket scope",
                    s.value,
                    freshness(s, now)
                )
            }),
    );
    out.push(
        snapshot
            .compaction
            .as_ref()
            .map_or("compaction: --".to_owned(), |s| {
                format!("compaction: {}; {}", s.value, freshness(s, now))
            }),
    );
    let mut tallies: Vec<(&str, u32, u32)> = Vec::new();
    for tool in &snapshot.tools {
        let name = tool.name.as_deref().unwrap_or("tool");
        let failed = u32::from(tool.status == crate::agent::codex_metrics::ToolStatus::Failed);
        match tallies.iter_mut().find(|t| t.0 == name) {
            Some(t) => {
                t.1 += 1;
                t.2 += failed;
            }
            None => tallies.push((name, 1, failed)),
        }
    }
    tallies.sort_by_key(|t| std::cmp::Reverse(t.1));
    out.push("tool calls by tool, from the recent observations (never every call):".to_owned());
    for (name, calls, failed) in tallies {
        out.push(format!("  {name}: {calls} calls; {failed} failed"));
    }
    out.push(format!(
        "recent tool observations: {}; exhaustive: {}",
        snapshot.tools.len(),
        snapshot.tools_complete
    ));
    for tool in &snapshot.tools {
        out.push(format!(
            "  {}: {:?}; at {}; elapsed ms: {}",
            tool.name.as_deref().unwrap_or("tool"),
            tool.status,
            tool.at,
            tool.elapsed_ms.map_or("--".to_owned(), |n| n.to_string())
        ));
    }
    out
}

fn usage(name: &str, sample: Option<&Sample<Tokens>>, now: Timestamp) -> String {
    sample.map_or(format!("{name}: --"), |s| {
        format!(
            "{name}: total {}; input {}; cached {}; cache write {}; output {}; reasoning {}; {}",
            number(s.value.total),
            number(s.value.input),
            number(s.value.cached_input),
            number(s.value.cache_write_input),
            number(s.value.output),
            number(s.value.reasoning_output),
            freshness(s, now)
        )
    })
}

fn number(value: Option<u64>) -> String {
    value.map_or("--".to_owned(), |n| n.to_string())
}

fn freshness<T>(sample: &Sample<T>, now: Timestamp) -> String {
    format!(
        "source {:?}; at {}; {}",
        sample.source,
        sample.at,
        if sample.is_stale_at(now, HUD_STALE_AFTER_SECS) {
            "stale"
        } else {
            "fresh"
        }
    )
}

/// Each field with the transcript record it comes from.
fn claude_lines(s: &claude_metrics::Snapshot, tail_only: bool, now: Timestamp) -> Vec<String> {
    let mut out = vec![s.request.as_ref().map_or_else(
        || "request (assistant message.usage): --".to_owned(),
        |r| {
            format!(
                "request (assistant message.usage): input {}; cache write {}; cache read {}; output {}; {}",
                r.value.input,
                r.value.cache_write,
                r.value.cache_read,
                r.value.output,
                claude_freshness(r, now)
            )
        },
    )];
    out.push(s.context_tokens.as_ref().map_or_else(
        || "context (latest request's input and cache): -- (none since the read began or the last compaction)".to_owned(),
        |c| {
            format!(
                "context (latest request's input and cache): {} tokens; {}; no window size, so no percentage",
                c.value,
                claude_freshness(c, now)
            )
        },
    ));
    let t = s.totals;
    out.push(format!(
        "totals over {} distinct messages since the read began: input {}; cache write {}; cache read {}; output {}",
        s.messages, t.input, t.cache_write, t.cache_read, t.output
    ));
    out.push(s.mode.as_ref().map_or_else(
        || "mode: --".to_owned(),
        |m| {
            format!(
                "mode: model {} and effort {} (assistant records); permission {} (permission-mode records); {}",
                m.value.model.as_deref().unwrap_or("--"),
                m.value.effort.as_deref().unwrap_or("--"),
                m.value.permission_mode.as_deref().unwrap_or("--"),
                claude_freshness(m, now)
            )
        },
    ));
    out.push(s.compaction.as_ref().map_or_else(
        || format!("compactions (compact_boundary records): {}", s.compactions),
        |c| {
            format!(
                "compactions (compact_boundary records): {}; latest {} at {} tokens; {}",
                s.compactions,
                c.value.trigger.as_deref().unwrap_or("--"),
                c.value
                    .pre_tokens
                    .map_or("--".to_owned(), |n| n.to_string()),
                claude_freshness(c, now)
            )
        },
    ));
    out.push(
        "plan: not shown; no transcript with TodoWrite or the Task tools has been probed"
            .to_owned(),
    );
    out.push(format!(
        "subagent calls (Agent tool): {}; background calls (run_in_background): {}",
        s.subagent_calls, s.background_calls
    ));
    out.extend(claude_tool_lines(s, tail_only));
    out
}

/// Every tool's tally, which is complete unless the read began part-way, then
/// the recent calls the HUD keeps.
fn claude_tool_lines(s: &claude_metrics::Snapshot, tail_only: bool) -> Vec<String> {
    let calls: u32 = s.tool_counts.iter().map(|c| c.calls).sum();
    let failed: u32 = s.tool_counts.iter().map(|c| c.failed).sum();
    let mut out = vec![format!(
        "tool calls by tool (tool_use and tool_result blocks), {}: {calls} calls; {failed} failed",
        if tail_only {
            "since the read began part-way through"
        } else {
            "every call in the session"
        }
    )];
    for count in &s.tool_counts {
        out.push(format!(
            "  {}: {} calls; {} failed",
            count.name, count.calls, count.failed
        ));
    }
    out.push(format!(
        "recent calls, oldest first: {} (at most {}); every call since they began: {}",
        s.tools.len(),
        claude_metrics::MAX_TOOLS,
        s.tools_complete
    ));
    for tool in &s.tools {
        out.push(format!(
            "  {}: {:?}; at {}; background: {}",
            tool.name.as_deref().unwrap_or("tool"),
            tool.status,
            tool.at,
            tool.background
        ));
    }
    out
}

/// What `spyc --statusline` forwarded: this tab's report, and the plan windows
/// from the newest report any tab sent.
fn statusline_lines(
    tabs: &crate::pane::PaneTabs,
    info: &crate::pane::tabs::TabInfo,
) -> Vec<String> {
    let Some((report, at)) = info.claude_statusline.as_ref() else {
        return vec![
            "statusline (spyc --statusline): no report from this tab. claude's statusLine runs it once spyc's wiring is in place (:hooks on claude).".to_owned(),
        ];
    };
    let session = match report.session_id.as_deref() {
        None => "no session named",
        Some(id) if Some(id) == info.live_session_id.as_deref() => "this tab's session",
        Some(_) => "another session, so its context share isn't shown",
    };
    let mut out = vec![format!(
        "statusline (spyc --statusline): reported at {at}; {session}"
    )];
    out.push(report.context.map_or_else(
        || "  context window: not reported".to_owned(),
        |c| {
            format!(
                "  context window: {} tokens; {} used (Claude Code's own share)",
                c.size,
                c.used_percent.map_or("--".to_owned(), |p| format!("{p}%"))
            )
        },
    ));
    let newest = tabs
        .tabs()
        .iter()
        .filter_map(|entry| Some((&entry.info.id, entry.info.claude_statusline.as_ref()?)))
        .filter(|(_, (r, _))| r.five_hour.is_some() || r.seven_day.is_some())
        .max_by_key(|(_, (_, at))| *at);
    let Some((pane, (plan, at))) = newest else {
        out.push("  plan: no rate-limit windows reported (API-key sessions have none)".to_owned());
        return out;
    };
    out.push(format!(
        "  plan, the newest report from any tab (pane {pane}, at {at}):"
    ));
    for (name, window) in [("five-hour", plan.five_hour), ("seven-day", plan.seven_day)] {
        out.push(window.map_or_else(
            || format!("    {name}: not reported"),
            |w| {
                let resets = w
                    .resets_at
                    .and_then(|secs| Timestamp::from_second(secs).ok())
                    .map_or("--".to_owned(), |t| t.to_string());
                format!("    {name}: {}% used; resets at {resets}", w.used_percent)
            },
        ));
    }
    out
}

fn claude_freshness<T>(sample: &claude_metrics::Sample<T>, now: Timestamp) -> String {
    format!(
        "at {}; {}",
        sample.at,
        if sample.is_stale_at(now, HUD_STALE_AFTER_SECS) {
            "stale"
        } else {
            "fresh"
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::claude_metrics::{Snapshot, Tool, ToolCount, ToolStatus};

    /// The dump names every tool counted, not only the recent calls the HUD
    /// keeps, and says whether the tally is the whole session.
    #[test]
    fn the_dump_lists_every_tools_tally_then_the_recent_calls() {
        let at: Timestamp = "2026-10-13T12:00:00Z".parse().unwrap();
        let count = |name: &str, calls, failed| ToolCount {
            name: name.into(),
            calls,
            failed,
        };
        let snapshot = Snapshot {
            tool_counts: (0..30)
                .map(|i| count(&format!("Tool{i}"), 30 - i, i % 2))
                .collect(),
            tools: vec![Tool {
                name: Some("Tool0".into()),
                status: ToolStatus::Completed,
                at,
                background: false,
            }],
            tools_complete: true,
            ..Snapshot::default()
        };
        let lines = claude_tool_lines(&snapshot, false);
        assert!(lines[0].contains("every call in the session: 465 calls; 15 failed"));
        for i in 0..30 {
            assert!(
                lines.iter().any(|l| l.starts_with(&format!("  Tool{i}: "))),
                "Tool{i} missing"
            );
        }
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("recent calls, oldest first: 1"))
        );
        let partial = claude_tool_lines(&snapshot, true);
        assert!(partial[0].contains("since the read began part-way through"));
    }
}
