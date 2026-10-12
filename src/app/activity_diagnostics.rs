//! Agent activity source and hook-readiness diagnostics: `:why-status` (the
//! active tab) and `:activity dump` (every tab) print one shared per-tab report.

use std::time::Instant;

use super::{App, Effect};
use crate::pane::{AgentActivity, PaneTabs, ReportedStatus, TabEntry, TabInfo};
use crate::state::sessions::AgentKind;

const fn state_str(a: AgentActivity) -> &'static str {
    match a {
        AgentActivity::Working => "working",
        AgentActivity::Idle => "idle",
        AgentActivity::Blocked => "blocked",
        AgentActivity::Done => "done",
        AgentActivity::Unknown => "unknown",
    }
}

fn is_agent(info: &TabInfo) -> bool {
    info.agent().kind() != AgentKind::Other
}

/// The self-report the dot follows, as `effective_activity` decides it: none
/// once the process has exited, and none while Codex's visible approval modal
/// overrides it.
fn live_report(e: &TabEntry) -> Option<ReportedStatus> {
    let info = &e.info;
    info.reported.filter(|_| {
        !e.pane.is_closed()
            && !crate::agent::codex_approval::overrides_report(
                info.reported,
                info.scrape_status.map(|(status, _)| status),
                info.agent().kind(),
            )
    })
}

/// `:why-status` — the active tab's dot explained in the pager, as `:why-git`
/// does for git markers: the verdict first, then the tab's `:activity dump`
/// report, so it can be searched, copied and saved.
pub(super) fn cmd_why_status(app: &mut App, _args: &str) -> Vec<Effect> {
    let Some(tabs) = app.runtime.pane_tabs.as_ref() else {
        app.state.flash_info("why-status: no pane open");
        return Vec::new();
    };
    let mut view = crate::ui::pager::PagerView::new_plain("why-status", why_status_lines(tabs));
    view.saveable = true;
    app.set_pager(view);
    Vec::new()
}

fn why_status_lines(tabs: &PaneTabs) -> Vec<String> {
    let index = tabs.active_index();
    let entry = &tabs.tabs()[index];
    let mut out = vec![
        format!("why-status @ {}", crate::sysinfo::format_now()),
        String::new(),
    ];
    out.extend(verdict(entry));
    out.push(String::new());
    out.extend(tab_report(index, entry, true, Instant::now()));
    out
}

/// The answer, before the evidence: the dot's state, what it follows, and how
/// long the pane has been quiet. A tab that isn't an agent has no dot.
fn verdict(e: &TabEntry) -> Vec<String> {
    let info = &e.info;
    if !is_agent(info) {
        return vec![format!(
            "\"{}\" is not a known agent, so it has no dot",
            info.label
        )];
    }
    let source = if e.pane.is_closed() {
        "process-exit".to_string()
    } else if live_report(e).is_some() {
        "self-reported".to_string()
    } else if let Some((_, hint)) = info.scrape_status {
        hint.map_or_else(
            || "scrape-fallback".to_string(),
            |h| format!("scrape-fallback: {h}"),
        )
    } else {
        "output-timing".to_string()
    };
    let age = match info.last_output_at {
        Some(at) => format!("{:.1}s since last output", at.elapsed().as_secs_f32()),
        None => "no output yet".to_string(),
    };
    let mut out = vec![format!("{} ({source})", state_str(info.activity)), age];
    if info
        .running
        .is_some_and(|r| r.on_shared_daemon && !r.backgrounded)
    {
        out.push(
            "codex was started without --no-daemon: on codex's shared daemon its hooks can't \
             reach this tab, so the dot follows output. Start it with `codex --no-daemon` for \
             hook status; that session then isn't on the shared server, so `codex agents` and \
             `codex queue` can't reach it."
                .to_string(),
        );
    }
    out
}

/// Build the `:activity dump` report (see [`super::commands::cmd_activity`]). Reads the live
/// tabs + activity tallies, plus each agent dir's hook config (the one I/O — a
/// dot that can't report is the first thing to rule out) → plain lines.
pub(super) fn activity_dump_lines(app: &App) -> Vec<String> {
    let now = Instant::now();
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
        out.extend(tab_report(i, e, i == active, now));
        out.push(String::new());
    }
    out
}

/// One tab's report: what the dot shows, where that comes from, and the
/// evidence behind it. The hook facts get a line each, so "a reporter is
/// installed" and "a hook was seen to run" can't blur into one long line.
fn tab_report(i: usize, e: &TabEntry, is_active: bool, now: Instant) -> Vec<String> {
    let info = &e.info;
    let marker = if is_active { '*' } else { ' ' };
    let mut out = vec![
        format!(
            "{marker}[{}] \"{}\"  dot={}  agent={}  suspended={}",
            i + 1,
            info.label,
            state_str(info.activity),
            is_agent(info),
            info.suspended,
        ),
        format!("    command: {}", info.command),
        format!("    cwd: {}", info.cwd.display()),
    ];
    if let Some(running) = info.running {
        let age = crate::sysinfo::epoch_secs().saturating_sub(running.started_epoch_secs);
        out.push(format!(
            "    running: {} (pid {}, started {age}s ago{})",
            crate::agent::profile_for(running.kind).name(),
            running.pid,
            match (running.backgrounded, running.on_shared_daemon) {
                (true, _) => ", in the background; the tab reads as its shell",
                (false, true) => ", without --no-daemon: hooks can't name this tab",
                (false, false) => "",
            },
        ));
    }
    if let Some(facts) = super::wiring::status_hooks_facts(info) {
        out.push("    hooks:".to_string());
        out.extend(facts.into_iter().map(|fact| format!("      {fact}")));
    }
    out.extend(source_lines(e, now));
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
    out
}

/// Where the dot's state comes from, the last report received and whether it
/// still counts, and the hook events behind it.
fn source_lines(e: &TabEntry, now: Instant) -> Vec<String> {
    let info = &e.info;
    let closed = e.pane.is_closed();
    let reported = live_report(e);
    let mut out = vec![match (closed, reported, info.scrape_status) {
        (true, _, _) => "    source: process-exit (no live agent)".to_string(),
        (false, Some(r), _) => format!(
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
        ),
        (false, None, Some((s, hint))) => format!(
            "    source: SCRAPE-FALLBACK status={}{}",
            state_str(s),
            match hint {
                Some(h) => format!(" ({h})"),
                None => String::new(),
            },
        ),
        (false, None, None) => "    source: output-timing (no live report)".to_string(),
    }];
    out.push(match info.last_reported {
        Some(report) => format!(
            "    last_report: status={} received {:.1}s ago (hook or agent; {})",
            state_str(report.status),
            report.at.elapsed().as_secs_f32(),
            match info.last_report_ignored {
                Some(reason) => format!("not applied: {reason}"),
                None if reported.is_some_and(|live| live.at == report.at) => {
                    "authoritative".into()
                }
                None if !closed && info.reported.is_some_and(|live| live.at == report.at) => {
                    "retained behind visible approval".into()
                }
                None => "no longer authoritative".into(),
            },
        ),
        None => "    last_report: none received".to_string(),
    });
    if info.recent_hook_events.is_empty() {
        out.push("    hook_events: none received (older reporters may omit metadata)".to_string());
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
    out
}
