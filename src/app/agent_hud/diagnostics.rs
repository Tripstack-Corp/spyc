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

fn diagnostic_lines(app: &App) -> Vec<String> {
    let mut out = vec![format!(
        "agent HUD: {}",
        app.state.config.pane.agent_hud.as_str()
    )];
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
        Update::Claude(u) => claude_lines(&u.snapshot, now),
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
fn claude_lines(s: &claude_metrics::Snapshot, now: Timestamp) -> Vec<String> {
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
    out.push(format!(
        "recent tool calls (tool_use and tool_result blocks): {}; exhaustive: {}",
        s.tools.len(),
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
