//! Agent activity source and hook-readiness diagnostics.

use super::{App, Effect};

/// `:why-status` — explain the active tab's agent-activity classification
/// (debug aid, `docs/archive/AGENT_AWARENESS_PLAN.md`): the current state, its **source**
/// (a semantic `report_status` self-report vs the output-timing fallback), and
/// how long since its last pane output. App-layer (reads the live pane tabs +
/// clock).
pub(super) fn cmd_why_status(app: &mut App, _args: &str) -> Vec<Effect> {
    use crate::pane::AgentActivity;
    use crate::state::sessions::AgentKind;
    let Some(tabs) = app.runtime.pane_tabs.as_ref() else {
        app.state.flash_info("why-status: no pane open");
        return Vec::new();
    };
    let info = tabs.active_info();
    let is_agent = crate::agent::detect(&info.command).kind() != AgentKind::Other;
    let age = match info.last_output_at {
        Some(at) => format!("{:.1}s since last output", at.elapsed().as_secs_f32()),
        None => "no output yet".to_string(),
    };
    let msg = if is_agent {
        let state = match info.activity {
            AgentActivity::Working => "working",
            AgentActivity::Idle => "idle",
            AgentActivity::Blocked => "blocked",
            AgentActivity::Done => "done",
            AgentActivity::Unknown => "unknown",
        };
        // Match `effective_activity`, including Codex's temporary modal override.
        let source = if tabs.active().is_closed() {
            "process-exit".to_string()
        } else if info.reported.is_some()
            && !crate::agent::codex_approval::overrides_report(
                info.reported,
                info.scrape_status.map(|(status, _)| status),
                crate::agent::detect(&info.command).kind(),
            )
        {
            "self-reported".to_string()
        } else if let Some((_, hint)) = info.scrape_status {
            match hint {
                Some(h) => format!("scrape-fallback: {h}"),
                None => "scrape-fallback".to_string(),
            }
        } else {
            "output-timing".to_string()
        };
        // Name a missing hook install outright. Without it the fallback reads
        // as the answer ("idle (output-timing)") when the real story is that
        // nothing can report — the exact ambiguity that turned one removal into
        // a forensics session.
        let hooks = super::status_hooks::status_hooks_diagnostic(info)
            .map_or_else(String::new, |message| format!(" — hooks: {message}"));
        format!(
            "why-status [{}]: {state} ({source}) — {age}{hooks}",
            info.label
        )
    } else {
        format!("why-status: '{}' is not a known agent — no dot", info.label)
    };
    app.state.flash_info(msg);
    Vec::new()
}

/// Build the `:activity dump` report (see [`super::commands::cmd_activity`]). Reads the live
/// tabs + activity tallies, plus each agent dir's hook config (the one I/O — a
/// dot that can't report is the first thing to rule out) → plain lines.
pub(super) fn activity_dump_lines(app: &App) -> Vec<String> {
    use crate::pane::AgentActivity;
    use crate::state::sessions::AgentKind;

    let state_str = |a: AgentActivity| match a {
        AgentActivity::Working => "working",
        AgentActivity::Idle => "idle",
        AgentActivity::Blocked => "blocked",
        AgentActivity::Done => "done",
        AgentActivity::Unknown => "unknown",
    };

    let now = std::time::Instant::now();
    let mut out = vec![format!(
        "spyc {} (pid {}) — activity dump @ {}",
        crate::VERSION,
        std::process::id(),
        crate::sysinfo::format_now(),
    )];
    // `report_status:N` here is the key signal: how many status reports
    // (hook-driven OR agent-driven) actually reached spyc this session.
    let calls = &app.view.activity.mcp_tool_calls;
    let tally: Vec<String> = calls
        .iter()
        .filter(|(_, c)| **c > 0)
        .map(|(n, c)| format!("{n}:{c}"))
        .collect();
    out.push(format!(
        "mcp tool calls: {}",
        if tally.is_empty() {
            "(none yet)".to_string()
        } else {
            tally.join("  ")
        }
    ));
    out.push(format!(
        "mcp connections: {}",
        app.view.activity.mcp_connection_summary()
    ));
    for (conn, c) in &app.view.activity.mcp_connections {
        let tab = c.pane_id.as_deref().and_then(|id| {
            app.runtime
                .pane_tabs
                .as_ref()?
                .tabs()
                .iter()
                .enumerate()
                .find(|(_, t)| t.info.id == id)
        });
        let who = match (c.pane_id.as_deref(), tab) {
            (_, Some((i, t))) => format!("tab [{}] \"{}\"", i + 1, t.info.label),
            (Some(id), None) => format!("pane {id} (tab closed)"),
            (None, None) => "unattributed (an older proxy, or no live tab had its id)".into(),
        };
        out.push(format!(
            "  #{conn}  {who}  since {}  calls:{}",
            c.since, c.calls
        ));
    }
    out.push(String::new());

    let Some(tabs) = app.runtime.pane_tabs.as_ref() else {
        out.push("(no panes open)".to_string());
        return out;
    };
    let active = tabs.active_index();
    for (i, e) in tabs.tabs().iter().enumerate() {
        let info = &e.info;
        let is_agent = crate::agent::detect(&info.command).kind() != AgentKind::Other;
        let marker = if i == active { '*' } else { ' ' };
        out.push(format!(
            "{marker}[{}] \"{}\"  dot={}  agent={is_agent}  suspended={}",
            i + 1,
            info.label,
            state_str(info.activity),
            info.suspended,
        ));
        out.push(format!("    command: {}", info.command));
        out.push(format!("    cwd: {}", info.cwd.display()));
        if let Some(message) = super::status_hooks::status_hooks_diagnostic(info) {
            out.push(format!("    hooks: {message}"));
        }
        // Match the effective source, including Codex's visible approval modal.
        let closed = e.pane.is_closed();
        let reported = info.reported.filter(|_| {
            !closed
                && !crate::agent::codex_approval::overrides_report(
                    info.reported,
                    info.scrape_status.map(|(status, _)| status),
                    crate::agent::detect(&info.command).kind(),
                )
        });
        match (closed, reported, info.scrape_status) {
            (true, _, _) => out.push("    source: process-exit (no live agent)".to_string()),
            (false, Some(r), _) => out.push(format!(
                "    source: SELF-REPORT status={} set {:.1}s ago, {}",
                state_str(r.status),
                r.at.elapsed().as_secs_f32(),
                if r.status == AgentActivity::Blocked {
                    "latched until answered/dismissed or a newer report".to_string()
                } else {
                    format!(
                        "expires in {:.0}s",
                        r.expiry.saturating_duration_since(now).as_secs_f32()
                    )
                },
            )),
            (false, None, Some((s, hint))) => out.push(format!(
                "    source: SCRAPE-FALLBACK status={}{}",
                state_str(s),
                match hint {
                    Some(h) => format!(" ({h})"),
                    None => String::new(),
                },
            )),
            (false, None, None) => {
                out.push("    source: output-timing (no live report)".to_string());
            }
        }
        match info.last_reported {
            Some(report) => out.push(format!(
                "    last_report: status={} received {:.1}s ago (hook or agent; {})",
                state_str(report.status),
                report.at.elapsed().as_secs_f32(),
                match info.last_report_ignored {
                    Some(reason) => format!("not applied: {reason}"),
                    None if reported.is_some_and(|live| live.at == report.at) =>
                        "authoritative".into(),
                    None if !closed && info.reported.is_some_and(|live| live.at == report.at) =>
                        "retained behind visible approval".into(),
                    None => "no longer authoritative".into(),
                },
            )),
            None => out.push("    last_report: none received".to_string()),
        }
        if info.recent_hook_events.is_empty() {
            out.push(
                "    hook_events: none received (older reporters may omit metadata)".to_string(),
            );
        } else {
            out.push("    hook_events: reported metadata (unverified; oldest first)".to_string());
            for (event, status, at) in &info.recent_hook_events {
                out.push(format!(
                    "      event={} status={} tool={} turn={} call={} received {:.1}s ago",
                    event.hook_event_name,
                    state_str(*status),
                    event.tool_name.as_deref().unwrap_or("not-reported"),
                    event.turn_id.as_deref().unwrap_or("not-reported"),
                    event.tool_use_id.as_deref().unwrap_or("not-reported"),
                    at.elapsed().as_secs_f32(),
                ));
            }
        }
        match info.last_output_at {
            Some(at) => out.push(format!(
                "    last_output: {:.1}s ago",
                at.elapsed().as_secs_f32()
            )),
            None => out.push("    last_output: none".to_string()),
        }
        out.push(format!(
            "    spawn: {:.0}s ago",
            info.spawn_at.elapsed().as_secs_f32()
        ));
        out.push(format!("    pane_id: {}", info.id));
        if let Some(s) = &info.live_session_id {
            out.push(format!("    live_session_id: {s}"));
        }
        if let Some(s) = &info.codex_session_id {
            out.push(format!("    codex_session_id: {s}"));
        }
        out.push(String::new());
    }
    out
}
