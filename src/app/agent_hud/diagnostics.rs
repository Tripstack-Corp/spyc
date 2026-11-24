//! Explicit source diagnostics retain only the collector's bounded projection.

use std::fmt::Write as _;

use jiff::Timestamp;

use crate::agent::agy_metrics;
use crate::agent::claude_metrics;
use crate::agent::codex_metrics::{Sample, Snapshot, Tokens};
use crate::agent::tail::Availability;
use crate::config::HUD_STALE_AFTER_SECS;
use crate::state::sessions::AgentKind;
use crate::ui::agent_hud::resets;

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
    let source = if app.state.active_project().pane.agent_hud.choice.is_some() {
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
            "not visible: off, hidden, short, closed or not an active Codex, Claude Code or Antigravity pane"
                .to_owned()
        }
    });
    out.push("Context occupancy: unknown (no verified native percentage).".to_owned());
    let Some(tabs) = app.runtime.project().pane_tabs.as_ref() else {
        out.push("No active pane.".to_owned());
        return out;
    };
    let info = tabs.active_info();
    out.push(format!("pane: {}", info.id));
    out.extend(agent_info_lines(app, tabs, info));
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
        let (file, retry) = match update {
            Update::Codex(_) => ("Pinned rollout", "toggle the strip to retry"),
            Update::Claude(_) | Update::Agy(_) => (
                "Session transcript",
                "looked up again on the tab's next status report, or toggle the strip",
            ),
        };
        out.push(format!("{file} unavailable in bounded lookup; {retry}."));
    }
    let work = app.runtime.agent_hud.as_ref().map_or(
        app.state.active_project().pane.agent_hud.work,
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
        Update::Agy(u) => agy_lines(&u.snapshot, u.tail_only, now),
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
                sample_freshness(r, now)
            )
        },
    )];
    out.push(s.context_tokens.as_ref().map_or_else(
        || "context (latest request's input and cache): -- (none since the read began or the last compaction)".to_owned(),
        |c| {
            format!(
                "context (latest request's input and cache): {} tokens; {}; no window size, so no percentage",
                c.value,
                sample_freshness(c, now)
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
                sample_freshness(m, now)
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
                sample_freshness(c, now)
            )
        },
    ));
    out.push(s.plan.as_ref().map_or_else(
        || "plan (TodoWrite's todos, or TaskCreate/TaskUpdate's tasks): --".to_owned(),
        |plan| {
            format!(
                "plan (TodoWrite's todos, or TaskCreate/TaskUpdate's tasks): {} pending; {} in progress; {} completed; {}",
                plan.value.pending,
                plan.value.in_progress,
                plan.value.completed,
                sample_freshness(plan, now)
            )
        },
    ));
    out.push(format!(
        "subagent calls (Agent tool): {}; background calls (run_in_background): {}",
        s.subagent_calls, s.background_calls
    ));
    out.push(script_line(s, now));
    out.push(format!(
        "organization (oauthAccount in Claude Code's .claude.json): {}",
        s.organization.as_deref().unwrap_or("-- (not logged in)")
    ));
    out.extend(claude_tool_lines(s, tail_only));
    out
}

/// Code Mode's scripts, each figure with the records it comes from.
fn script_line(s: &claude_metrics::Snapshot, now: Timestamp) -> String {
    let sc = &s.scripts;
    let code_mode = match s.mode.as_ref().and_then(|m| m.value.code_mode) {
        Some(true) => "on",
        Some(false) => "off",
        None => "--",
    };
    let latest = sc.latest_ms.as_ref().map_or_else(
        || "--".to_owned(),
        |ms| format!("{} ms, {}", ms.value, sample_freshness(ms, now)),
    );
    let longest = sc
        .longest_ms
        .map_or_else(|| "--".to_owned(), |ms| format!("{ms} ms"));
    format!(
        "scripts (Code Mode: repl-mode records; REPL calls; <synthetic> tool calls; queued repl-eval envelopes): code mode {code_mode}; {} started; {} calls made; {} threw; {} running; latest {latest}; longest {longest}",
        sc.started, sc.calls, sc.failed, sc.running
    )
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
        let in_scripts = s
            .scripts
            .tools
            .iter()
            .find(|scripted| scripted.name == count.name)
            .map_or_else(String::new, |scripted| {
                format!(
                    "; in scripts {} calls, {} failed",
                    scripted.calls, scripted.failed
                )
            });
        out.push(format!(
            "  {}: {} calls; {} failed{in_scripts}",
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

/// What `spyc --statusline` forwarded for Claude Code: this tab's report, and
/// the plan windows from the newest report any Claude tab sent.
fn statusline_lines(
    tabs: &crate::pane::PaneTabs,
    info: &crate::pane::tabs::TabInfo,
) -> Vec<String> {
    let Some((report, at)) = info.statusline.as_ref() else {
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
        .filter(|entry| entry.info.agent().kind() == AgentKind::Claude)
        .filter_map(|entry| Some((&entry.info.id, entry.info.statusline.as_ref()?)))
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

/// What `spyc --statusline` forwarded for Antigravity: this tab's report, and
/// every quota bucket from the newest Antigravity report any tab sent.
fn agy_statusline_lines(
    tabs: &crate::pane::PaneTabs,
    info: &crate::pane::tabs::TabInfo,
    now: Timestamp,
) -> Vec<String> {
    let mut out = Vec::new();
    match info.statusline.as_ref() {
        None => {
            out.push(
                "statusline (spyc --statusline): no report from this tab. agy's statusLine runs it once spyc's wiring is in place (:hooks on agy).".to_owned(),
            );
        }
        Some((report, at)) => {
            let session = match report.session_id.as_deref() {
                None => "no session named",
                Some(id) if Some(id) == info.live_session_id.as_deref() => "this tab's session",
                Some(_) => "another session, so its context share isn't shown",
            };
            out.push(format!(
                "statusline (spyc --statusline): reported at {at}; {session}"
            ));
            out.push(report.context.map_or_else(
                || "  context window: not reported".to_owned(),
                |c| {
                    let over = report.agy.as_ref().is_some_and(|a| a.exceeds_200k);
                    let flag = if over { "; exceeds 200k" } else { "" };
                    format!(
                        "  context window: {} tokens; {} used{flag} (Antigravity's own share)",
                        c.size,
                        c.used_percent.map_or("--".to_owned(), |p| format!("{p}%"))
                    )
                },
            ));
            if let Some(agy) = report.agy.as_ref() {
                out.extend(agy_statusline_detail_lines(agy));
            }
        }
    }
    out.extend(agy_quota_lines(tabs, now));
    out
}

fn agy_statusline_detail_lines(agy: &crate::agent::agy_statusline::AgyStatusline) -> Vec<String> {
    let mut out = Vec::new();
    let sandbox = match agy.sandbox {
        None => "--",
        Some(s) if !s.enabled => "off",
        Some(s) if s.allow_network == Some(false) => "on (no network)",
        Some(_) => "on",
    };
    out.push(format!(
        "  model: {}; effort: {}; plan tier: {}",
        agy.model.as_deref().unwrap_or("--"),
        agy.effort.as_deref().unwrap_or("--"),
        agy.plan_tier.as_deref().unwrap_or("--"),
    ));
    out.push(format!(
        "  approval mode: {}; sandbox: {}; active agent: {}; tasks: {}; queued: {}",
        agy.approval_mode.as_deref().unwrap_or("--"),
        sandbox,
        agy.active_agent.as_deref().unwrap_or("default"),
        agy.task_count,
        agy.pending_input_count,
    ));
    if !agy.subagents.is_empty() {
        let running = agy.subagents.iter().filter(|s| s.running).count();
        let names: Vec<String> = agy
            .subagents
            .iter()
            .map(|s| {
                format!(
                    "{} ({})",
                    s.name,
                    if s.running { "running" } else { "done" }
                )
            })
            .collect();
        out.push(format!(
            "  subagents ({running} running, {} total): {}",
            agy.subagents.len(),
            names.join(", ")
        ));
    }
    out
}

fn agy_orchestrator_lines(
    app: &App,
    tabs: &crate::pane::PaneTabs,
    info: &crate::pane::tabs::TabInfo,
) -> Vec<String> {
    let snapshot = match app.active_agent_hud_update() {
        Some(Update::Agy(u)) if u.availability == Availability::Available => Some(&u.snapshot),
        _ => None,
    };
    let reported = crate::app::render::agent_hud::agy_reported(tabs, info);
    let outer = reported
        .statusline
        .and_then(|s| s.agy.as_ref())
        .and_then(|a| a.model.as_deref())
        .or_else(|| {
            snapshot
                .and_then(|s| s.mode.as_ref())
                .and_then(|m| m.value.model.as_deref())
        });
    let Some(outer) = outer.filter(|m| crate::agent::agy_orchestrator::is_orchestrator_wrapper(m))
    else {
        return Vec::new();
    };
    let line = match crate::ui::agent_hud::agy_active_model(snapshot, reported) {
        Some(active) => match active.role {
            Some(role) => format!(
                "orchestrator ({outer}): active {} [{}]; role {}",
                active.display,
                active.raw,
                role.label()
            ),
            None => format!(
                "orchestrator ({outer}): active {} [{}]",
                active.display, active.raw
            ),
        },
        None => format!("orchestrator ({outer}): pending (no routed step observed yet)"),
    };
    vec![line]
}

fn agy_quota_lines(tabs: &crate::pane::PaneTabs, now: Timestamp) -> Vec<String> {
    let newest = tabs
        .tabs()
        .iter()
        .filter(|entry| entry.info.agent().kind() == AgentKind::Agy)
        .filter_map(|entry| {
            let (report, at) = entry.info.statusline.as_ref()?;
            let agy = report.agy.as_ref()?;
            (!agy.quotas.is_empty()).then_some((&entry.info.id, agy, *at))
        })
        .max_by_key(|(_, _, at)| *at);
    let Some((pane, agy, at)) = newest else {
        return vec!["  quotas: none reported".to_owned()];
    };
    let mut out = vec![format!(
        "  quotas, the newest report from any agy tab (pane {pane}, at {at}):"
    )];
    for q in &agy.quotas {
        let used = resets::format_pct_2dp(q.used_percent_x100);
        let rem = resets::format_pct_2dp(q.remaining_percent_x100());
        let reset = q
            .resets_at
            .and_then(|secs| Timestamp::from_second(secs).ok())
            .map_or_else(|| "--".to_owned(), |t| t.to_string());
        let mut extra = String::new();
        if q.disabled {
            extra.push_str("; disabled");
        } else if let (Some(at), Some(win)) = (q.resets_at, q.window_secs()) {
            if let Some(delta) = resets::pace_delta_points(q.used_percent_x100, at, win, now) {
                let _ = write!(extra, "; pace {delta:+}%");
            }
            if let Some(runout) = resets::pace_runout(q.used_percent_x100, at, win, now) {
                let _ = write!(extra, " (runs out in {runout})");
            }
        }
        out.push(format!(
            "    {}: {used} used ({rem} left); resets at {reset}{extra}",
            q.key
        ));
    }
    out
}

fn sample_freshness<T>(sample: &claude_metrics::Sample<T>, now: Timestamp) -> String {
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

fn agent_info_lines(
    app: &App,
    tabs: &crate::pane::PaneTabs,
    info: &crate::pane::tabs::TabInfo,
) -> Vec<String> {
    let tilde = |home: Option<&std::path::PathBuf>| {
        home.map_or("--".to_owned(), |p| crate::paths::display_tilde(p))
    };
    let mut out = Vec::new();
    if super::feed(info.agent().kind()) == Some(Feed::Claude) {
        out.push(format!(
            "reported session: {}",
            info.live_session_id.as_deref().unwrap_or("--")
        ));
        out.push(format!(
            "Claude home (spyc's environment): {}",
            tilde(
                app.state
                    .active_project()
                    .pane
                    .agent_hud
                    .claude_home
                    .as_ref()
            )
        ));
        out.push(format!(
            "credential (status hook): {}",
            info.claude_credential.map_or(
                "-- (no hook report yet; the saved login is assumed)",
                crate::agent::claude_credential::Credential::label
            )
        ));
        out.extend(statusline_lines(tabs, info));
    } else if super::feed(info.agent().kind()) == Some(Feed::Agy) {
        out.push(format!(
            "reported session: {}",
            info.live_session_id.as_deref().unwrap_or("--")
        ));
        out.push(format!(
            "Agy home (spyc's environment): {}",
            tilde(app.state.active_project().pane.agent_hud.agy_home.as_ref())
        ));
        out.extend(agy_orchestrator_lines(app, tabs, info));
        out.extend(agy_statusline_lines(tabs, info, Timestamp::now()));
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
    out
}

fn agy_lines(s: &agy_metrics::Snapshot, tail_only: bool, now: Timestamp) -> Vec<String> {
    let mut out = vec![s.request.as_ref().map_or_else(
        || "request (planner response usage): --".to_owned(),
        |r| {
            format!(
                "request (planner response usage): input {}; cache read {}; output {}; {}",
                r.value.input,
                r.value.cache_read,
                r.value.output,
                sample_freshness(r, now)
            )
        },
    )];
    out.push(s.context_tokens.as_ref().map_or_else(
        || "context (latest request's input and cache read): -- (none since the read began or the last compaction)".to_owned(),
        |c| {
            format!(
                "context (latest request's input and cache read): {} tokens; {}; no window size, so no percentage",
                c.value,
                sample_freshness(c, now)
            )
        },
    ));
    let t = s.totals;
    out.push(format!(
        "totals over {} distinct messages since the read began: input {}; cache read {}; output {}",
        s.messages, t.input, t.cache_read, t.output
    ));
    out.push(s.mode.as_ref().map_or_else(
        || "mode: --".to_owned(),
        |m| {
            format!(
                "mode: model {}; {}",
                m.value.model.as_deref().unwrap_or("--"),
                sample_freshness(m, now)
            )
        },
    ));
    if let Some(step) = s.active_model.as_ref() {
        out.push(format!(
            "active step model (planner response): {}; {}",
            step.value,
            sample_freshness(step, now)
        ));
    }
    out.push(s.compaction.as_ref().map_or_else(
        || format!("compactions (checkpoint records): {}", s.compactions),
        |c| {
            format!(
                "compactions (checkpoint records): {}; latest {}",
                s.compactions,
                sample_freshness(c, now)
            )
        },
    ));
    out.push(format!(
        "subagent calls (invoke_subagent): {}",
        s.subagent_calls
    ));
    if let Some(req) = s.request.as_ref()
        && req.value.context() > 0
    {
        let hit = req
            .value
            .cache_read
            .saturating_mul(100)
            .checked_div(req.value.context())
            .unwrap_or(0);
        out.push(format!(
            "prompt cache (latest request): {hit}% hit; {}",
            sample_freshness(req, now)
        ));
    }
    if let Some(tps) = s.last_tps.as_ref() {
        out.push(format!(
            "effective throughput (step-to-step): {} tok/s; {}",
            tps.value,
            sample_freshness(tps, now)
        ));
    }
    out.extend(agy_tool_lines(s, tail_only));
    out
}

fn agy_tool_lines(s: &agy_metrics::Snapshot, tail_only: bool) -> Vec<String> {
    let calls: u32 = s.tool_counts.iter().map(|c| c.calls).sum();
    let failed: u32 = s.tool_counts.iter().map(|c| c.failed).sum();
    let mut out = vec![format!(
        "tool calls by tool, {}: {calls} calls; {failed} failed",
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
        agy_metrics::MAX_TOOLS,
        s.tools_complete
    ));
    for tool in &s.tools {
        out.push(format!(
            "  {}: {:?}; at {}",
            tool.name.as_deref().unwrap_or("tool"),
            tool.status,
            tool.at
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::claude_metrics::{Snapshot, Tool, ToolCount, ToolStatus};

    /// The plan line names its two possible sources and says when nothing has
    /// been reported, rather than claiming it was probed and found absent.
    #[test]
    fn the_dump_names_the_plans_source_and_freshness() {
        let at: Timestamp = "2026-10-13T12:00:00Z".parse().unwrap();
        let now = at.checked_add(std::time::Duration::from_secs(90)).unwrap();
        let unreported = claude_lines(&Snapshot::default(), false, now);
        assert!(
            unreported
                .iter()
                .any(|l| l == "plan (TodoWrite's todos, or TaskCreate/TaskUpdate's tasks): --")
        );
        let snapshot = Snapshot {
            plan: Some(claude_metrics::Sample {
                at,
                value: claude_metrics::Plan {
                    pending: 1,
                    in_progress: 2,
                    completed: 4,
                },
            }),
            ..Snapshot::default()
        };
        let reported = claude_lines(&snapshot, false, now);
        assert!(reported.iter().any(|l| {
            l.starts_with("plan (TodoWrite's todos, or TaskCreate/TaskUpdate's tasks): ")
                && l.contains("1 pending")
                && l.contains("2 in progress")
                && l.contains("4 completed")
                && l.contains("stale")
        }));
    }

    /// The dump gives Code Mode's figures, with how long scripts ran, which
    /// the strip leaves out.
    #[test]
    fn the_dump_gives_code_modes_scripts_and_how_long_they_ran() {
        let at: Timestamp = "2026-10-13T12:00:00Z".parse().unwrap();
        let line = |s: &Snapshot| {
            claude_lines(s, false, at)
                .into_iter()
                .find(|l| l.starts_with("scripts (Code Mode: "))
                .unwrap()
        };
        assert!(line(&Snapshot::default()).ends_with(
            "code mode --; 0 started; 0 calls made; 0 threw; 0 running; latest --; longest --"
        ));
        let snapshot = Snapshot {
            mode: Some(claude_metrics::Sample {
                at,
                value: claude_metrics::Mode {
                    code_mode: Some(true),
                    ..claude_metrics::Mode::default()
                },
            }),
            scripts: claude_metrics::Scripts {
                started: 3,
                calls: 7,
                failed: 2,
                running: 1,
                latest_ms: Some(claude_metrics::Sample { at, value: 49 }),
                longest_ms: Some(1_020),
                ..claude_metrics::Scripts::default()
            },
            ..Snapshot::default()
        };
        let reported = line(&snapshot);
        assert!(
            reported.contains(
                "code mode on; 3 started; 7 calls made; 2 threw; 1 running; latest 49 ms, "
            ) && reported.ends_with("; longest 1020 ms"),
            "{reported}"
        );
    }

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
                in_script: false,
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

        let scripted = Snapshot {
            scripts: claude_metrics::Scripts {
                calls: 7,
                tools: vec![count("Tool1", 7, 1)],
                ..claude_metrics::Scripts::default()
            },
            ..snapshot
        };
        let lines = claude_tool_lines(&scripted, false);
        assert!(
            lines
                .iter()
                .any(|l| l == "  Tool1: 29 calls; 1 failed; in scripts 7 calls, 1 failed"),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l == "  Tool0: 30 calls; 0 failed"),
            "{lines:?}"
        );
    }

    #[test]
    fn the_dump_formats_agy_lines_and_tools() {
        let at: Timestamp = "2026-10-14T08:00:00Z".parse().unwrap();
        let now = at.checked_add(std::time::Duration::from_secs(10)).unwrap();
        let snapshot = agy_metrics::Snapshot {
            request: Some(agy_metrics::Sample {
                at,
                value: agy_metrics::Usage {
                    input: 100,
                    cache_read: 40,
                    cache_write: 0,
                    output: 20,
                },
            }),
            context_tokens: Some(agy_metrics::Sample { at, value: 140 }),
            totals: agy_metrics::Usage {
                input: 200,
                cache_read: 80,
                cache_write: 0,
                output: 40,
            },
            messages: 2,
            mode: Some(agy_metrics::Sample {
                at,
                value: agy_metrics::Mode {
                    model: Some("gemini-2.5-pro".to_owned()),
                },
            }),
            tool_counts: vec![agy_metrics::ToolCount {
                name: "view_file".into(),
                calls: 5,
                failed: 1,
            }],
            tools: vec![agy_metrics::Tool {
                name: Some("view_file".into()),
                status: agy_metrics::ToolStatus::Completed,
                at,
            }],
            tools_complete: true,
            subagent_calls: 1,
            ..agy_metrics::Snapshot::default()
        };
        let lines = agy_lines(&snapshot, false, now);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("input 100; cache read 40; output 20"))
        );
        assert!(lines.iter().any(|l| l.contains("140 tokens")));
        assert!(lines.iter().any(|l| l.contains("model gemini-2.5-pro")));
        assert!(
            lines
                .iter()
                .any(|l| l.contains("subagent calls (invoke_subagent): 1"))
        );
        let tool_lines = agy_tool_lines(&snapshot, false);
        assert!(tool_lines[0].contains("every call in the session: 5 calls; 1 failed"));
        assert!(
            tool_lines
                .iter()
                .any(|l| l.contains("view_file: 5 calls; 1 failed"))
        );
    }

    #[test]
    fn diagnostic_dumps_omit_agy_reasoning_prose() {
        let at: Timestamp = "2026-10-14T08:00:00Z".parse().unwrap();
        let sentinel = "PRIVATE_REASONING_DUMP_SENTINEL";
        let agy_snap = agy_metrics::Snapshot {
            reasoning: Some(agy_metrics::Sample {
                at,
                value: sentinel.to_owned(),
            }),
            ..Default::default()
        };
        for line in agy_lines(&agy_snap, false, at) {
            assert!(
                !line.contains(sentinel),
                "reasoning leaked into dump: {line}"
            );
        }
    }
}
