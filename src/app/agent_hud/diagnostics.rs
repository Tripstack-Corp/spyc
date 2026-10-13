//! Explicit source diagnostics retain only the collector's bounded projection.

use jiff::Timestamp;

use crate::agent::codex_metrics::{Sample, Snapshot, Tokens};
use crate::config::HUD_STALE_AFTER_SECS;

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
        None => "not visible: off, hidden, short, closed or not an active Codex pane".to_owned(),
    });
    out.push("Context occupancy: unknown (no verified native percentage).".to_owned());
    let Some(tabs) = app.runtime.pane_tabs.as_ref() else {
        out.push("No active pane.".to_owned());
        return out;
    };
    let info = tabs.active_info();
    out.push(format!("pane: {}", info.id));
    out.push(format!(
        "pinned thread: {}",
        info.codex_session_id.as_deref().unwrap_or("--")
    ));
    out.push(format!(
        "captured home: {}",
        info.codex_home
            .as_ref()
            .map_or("--".to_owned(), |p| crate::paths::display_tilde(p))
    ));
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
        update.availability, update.binding.generation, update.tail_only
    ));
    if update.availability == crate::agent::codex_metrics::Availability::Missing {
        out.push(
            "Pinned rollout unavailable in bounded lookup; toggle the strip to retry.".to_owned(),
        );
    }
    let work = app.runtime.agent_hud.as_ref().map_or(
        app.state.pane.agent_hud.work,
        super::collector::Collector::work,
    );
    out.push(format!(
        "collector bytes read: {}; refreshes: {}",
        work.0, work.1
    ));
    out.extend(metric_lines(&update.snapshot, Timestamp::now()));
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
