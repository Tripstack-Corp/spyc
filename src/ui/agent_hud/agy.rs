//! Google Antigravity (`agy`)'s cells.
//!
//! Context is a token count (`input + cache_read`), joined by `agy`'s own
//! window share when `spyc --statusline` has reported one. Active-model quota
//! windows (`5h`, `7d`, and Google AI Pro's Argon weekly bucket when active or
//! half-spent) follow immediately so they survive narrow splits, ahead of
//! prompt-cache hit rate (with 3m/4m cold-TTL cues), output tokens, effective
//! throughput (`t/s`) and compactions.

use ratatui::text::Span;

use crate::agent::agy_metrics::{Snapshot, ToolStatus};
use crate::agent::agy_statusline::{AgyStatusline, QuotaBucket};
use crate::config::agent_hud::{HUD_CACHE_COLD_SECS, HUD_CACHE_WARN_SECS};

use super::{
    AgentRows, AgyReported, Field, HudView, MAX_CELL, Metrics, Palette, Row, STALE, Tally, Trailer,
    resets, safe_label, short_count, tools_row,
};

/// The most live subagents the `agents` row names, room allowing; the rest it counts.
const NAMED_SUBAGENTS: usize = 4;
/// Separate Argon weekly usage share at or above which it surfaces on non-Argon tabs.
const ARGON_SURFACE_USED_PCT: u8 = 50;

pub(super) fn rows(
    snapshot: Option<&Snapshot>,
    tail_only: bool,
    reported: AgyReported<'_>,
    view: &HudView<'_>,
    p: Palette,
) -> AgentRows {
    let Some(s) = snapshot else {
        let mut usage = p.unavailable(view.availability, "transcript");
        if let Some(percent) = reported_context_percent(reported) {
            let share = Field(vec![
                p.label("--"),
                Span::raw(" "),
                context_meter(percent, exceeds_200k(reported), p),
            ]);
            usage.cells.insert(1, share);
        }
        usage.cells.extend(plan_cells(None, reported, view, p));
        return AgentRows {
            usage,
            tools: Row::new(vec![
                Field(vec![p.heading("tools")]),
                Field(vec![p.label("--")]),
            ]),
            mode: mode_row(None, "", reported, p),
        };
    };

    let at_least = if tail_only { "≥" } else { "" };
    AgentRows {
        usage: usage(s, at_least, reported, view, p),
        tools: tools(s, tail_only, p),
        mode: mode_row(Some(s), at_least, reported, p),
    }
}

fn mode_row(s: Option<&Snapshot>, at_least: &str, reported: AgyReported<'_>, p: Palette) -> Row {
    let count = s.map_or_else(
        || p.label("--"),
        |s| p.strong(&format!("{at_least}{}", s.subagent_calls)),
    );
    let mut cells = vec![Field(vec![p.heading("agents")]), Field(vec![count])];
    if let Some(agy) = agy_report(reported) {
        if agy.task_count > 0 {
            cells.push(p.pair("background", p.value(&agy.task_count.to_string())));
        }
        if agy.pending_input_count > 0 {
            cells.push(p.pair("queued", p.strong(&agy.pending_input_count.to_string())));
        }
        if let Some(mode) = agy.approval_mode.as_deref() {
            cells.push(Field(vec![p.value(mode)]));
        }
        if let Some(sandbox) = agy.sandbox
            && sandbox.enabled
        {
            let label = if sandbox.allow_network == Some(false) {
                "sandbox (no net)"
            } else {
                "sandbox"
            };
            cells.push(Field(vec![p.value(label)]));
        }
        if let Some(agent) = agy.active_agent.as_deref() {
            cells.push(p.pair("agent", p.value(agent)));
        }
    }
    Row::new(cells)
}

fn usage(
    s: &Snapshot,
    at_least: &str,
    reported: AgyReported<'_>,
    view: &HudView<'_>,
    p: Palette,
) -> Row {
    let mut context = vec![
        s.context_tokens
            .as_ref()
            .map_or_else(|| p.label("--"), |c| p.strong(&short_count(c.value))),
    ];
    if let Some(percent) = reported_context_percent(reported) {
        context.push(Span::raw(" "));
        context.push(context_meter(percent, exceeds_200k(reported), p));
    }
    let t = s.totals;
    let mut cells = vec![Field(vec![p.heading("context")]), Field(context)];
    cells.extend(plan_cells(Some(s), reported, view, p));
    cells.push(cache_hit_cell(s, view, p));
    cells.push(p.pair(
        "out",
        p.value(&format!("{at_least}{}", short_count(t.output))),
    ));
    if let Some(tps) = s.last_tps.as_ref()
        && !view.now.is_some_and(|now| tps.is_stale_at(now, STALE))
    {
        cells.push(Field(vec![p.value(&tps.value.to_string()), p.label("t/s")]));
    }
    cells.push(p.pair(
        "compacted",
        p.value(&format!("{at_least}{}", s.compactions)),
    ));
    Row::new(cells)
}

fn cache_hit_cell(s: &Snapshot, view: &HudView<'_>, p: Palette) -> Field {
    let t = s.totals;
    let sent = t.context();
    let hit = t
        .cache_read
        .saturating_mul(100)
        .checked_div(sent)
        .map_or_else(|| p.label("--"), |pct| p.value(&format!("{pct}%")));
    let mut field = p.pair("hit", hit);
    if let (Some(req), Some(now)) = (s.request.as_ref(), view.now)
        && req.value.context() > 0
    {
        let age = now.as_second().saturating_sub(req.at.as_second());
        if age >= HUD_CACHE_COLD_SECS {
            field.0.push(Span::raw(" "));
            field.0.push(p.tint("cold", p.bad));
        } else if age >= HUD_CACHE_WARN_SECS {
            field.0.push(Span::raw(" "));
            field.0.push(p.tint("3m", p.warn));
        }
    }
    field
}

fn plan_cells(
    snapshot: Option<&Snapshot>,
    reported: AgyReported<'_>,
    view: &HudView<'_>,
    p: Palette,
) -> Vec<Field> {
    let Some(plan) = reported.plan else {
        return Vec::new();
    };
    let model = agy_report(reported)
        .and_then(|a| a.model.as_deref())
        .or_else(|| {
            snapshot
                .and_then(|s| s.mode.as_ref())
                .and_then(|m| m.value.model.as_deref())
        });
    let active = plan.active_quotas(model);
    let mut windows: Vec<(&str, &QuotaBucket)> = Vec::new();
    if let Some(w) = active.five_hour {
        windows.push(("5h", w));
    }
    if let Some(w) = active.seven_day {
        windows.push(("7d", w));
    }
    if let Some(argon) = active.argon
        && argon.used_percent() >= ARGON_SURFACE_USED_PCT
    {
        windows.push(("argon", argon));
    }
    windows
        .into_iter()
        .map(|(name, bucket)| render_quota_window(name, bucket, view, p))
        .collect()
}

fn render_quota_window(name: &str, bucket: &QuotaBucket, view: &HudView<'_>, p: Palette) -> Field {
    let reset = bucket.resets_at.zip(view.now);
    if reset.is_some_and(|(at, now)| at <= now.as_second()) {
        return p.pair(name, p.label("--"));
    }
    let used = bucket.used_percent();
    let mut field = p.pair(name, p.meter(used));
    let label = reset
        .zip(view.local_tz)
        .and_then(|((at, now), tz)| resets::label(at, now, tz, view.resets));
    if let Some(label) = label {
        field.0.push(Span::raw(" "));
        field.0.push(p.label(&format!("\u{21bb}{label}")));
    }
    if let (Some((at, now)), Some(window_secs)) = (reset, bucket.window_secs())
        && let Some(runout) = resets::pace_runout(bucket.used_percent_x100, at, window_secs, now)
    {
        let colour = if used >= 80 { p.bad } else { p.warn };
        field.0.push(Span::raw(" "));
        field.0.push(p.tint(&runout, colour));
    }
    field
}

/// The Antigravity plan tier (e.g. `plan Google AI Pro`), after the health
/// cells on a full HUD's `agents` row. Omitted, not `--`, when unreported.
pub(super) fn plan_tier(view: &HudView<'_>, p: Palette) -> Option<Field> {
    let agy = reported_from(view)?;
    let tier = agy.plan_tier.as_deref()?;
    Some(p.pair("plan", p.value(&safe_label(tier))).clip(MAX_CELL))
}

/// Live running subagents reported by `agy`'s statusline, trailing the full
/// HUD's `agents` row (`research · running │ ...`).
pub(super) fn subagents(view: &HudView<'_>, p: Palette) -> Option<Trailer> {
    let agy = reported_from(view)?;
    let running: Vec<&str> = agy
        .subagents
        .iter()
        .filter(|s| s.running)
        .map(|s| s.name.as_str())
        .collect();
    if running.is_empty() {
        return None;
    }
    let items: Vec<Field> = running
        .into_iter()
        .map(|name| Field(vec![p.strong(name), p.label(" · "), p.value("running")]))
        .collect();
    Some(Trailer::List {
        items,
        most: NAMED_SUBAGENTS,
        separator: Field(vec![p.label(" │ ")]),
        more: p.label("").style,
    })
}

/// The compact HUD's count of live running subagents.
pub(super) fn running_subagents(view: &HudView<'_>, p: Palette) -> Option<Field> {
    let running = reported_from(view)?
        .subagents
        .iter()
        .filter(|s| s.running)
        .count();
    (running > 0).then(|| p.pair("subagents", p.strong(&running.to_string())))
}

fn reported_from<'a>(view: &HudView<'a>) -> Option<&'a AgyStatusline> {
    match view.metrics {
        Metrics::Agy { reported, .. } => agy_report(reported),
        _ => None,
    }
}

fn agy_report(reported: AgyReported<'_>) -> Option<&AgyStatusline> {
    reported.statusline?.agy.as_ref()
}

fn reported_context_percent(reported: AgyReported<'_>) -> Option<u8> {
    reported.statusline?.context?.used_percent
}

fn exceeds_200k(reported: AgyReported<'_>) -> bool {
    agy_report(reported).is_some_and(|a| a.exceeds_200k)
}

fn context_meter(percent: u8, exceeds_200k: bool, p: Palette) -> Span<'static> {
    if exceeds_200k {
        p.tint(&format!("{percent}%"), p.bad)
    } else {
        p.meter(percent)
    }
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

    use super::super::{AgyReported, Attention, Hooks, HudView, Metrics, lines};
    use crate::agent::agy_metrics::{
        Availability, Compaction, Mode, Sample, Snapshot, Tool, ToolCount, ToolStatus, Usage,
    };
    use crate::agent::agy_statusline::{
        ARGON_BUCKET, AgyStatusline, AgySubagent, QuotaBucket, SandboxState,
    };
    use crate::agent::claude_statusline::{ContextWindow, Statusline};
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
            request: Some(Sample {
                at: at(1),
                value: Usage {
                    input: 10_000,
                    cache_write: 0,
                    cache_read: 110_000,
                    output: 500,
                },
            }),
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
                reported: AgyReported::default(),
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
        assert!(rendered[3].contains("│ agents      3"));
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
        assert!(compacted[1].contains("│ context     --"));
        assert!(compacted[1].contains("compacted ≥2"));
    }

    #[test]
    fn an_unavailable_transcript_says_why_and_shows_nothing_measured() {
        let mut missing = view(None, false);
        missing.availability = Some(Availability::Missing);
        let rendered = rows(&missing, 120);
        assert!(rendered[1].contains("transcript missing"));
        assert!(rendered[2].contains("--"));
        assert!(rendered[3].contains("│ agents      --"));
    }

    #[test]
    fn statusline_supplies_context_share_quotas_pace_mode_plan_and_live_subagents() {
        let theme = Theme::default();
        let mut snapshot = session();
        snapshot.last_tps = Some(Sample {
            at: at(9),
            value: 42,
        });
        let agy_sl = AgyStatusline {
            model: Some("Gemini 4 Neon (High)".into()),
            effort: Some("high".into()),
            exceeds_200k: true,
            plan_tier: Some("Google AI Pro".into()),
            approval_mode: Some("accept-edits".into()),
            sandbox: Some(SandboxState {
                enabled: true,
                allow_network: Some(false),
            }),
            quotas: vec![
                QuotaBucket {
                    key: "gemini-5h".into(),
                    // 75% used with 3h left (40% elapsed) -> burning fast!
                    used_percent_x100: 7500,
                    resets_at: Some(at(10 + 3 * 3600).as_second()),
                    disabled: false,
                },
                QuotaBucket {
                    key: "gemini-weekly".into(),
                    used_percent_x100: 3000,
                    resets_at: Some(at(10 + 2 * 86_400).as_second()),
                    disabled: false,
                },
                QuotaBucket {
                    key: ARGON_BUCKET.into(),
                    used_percent_x100: 8500,
                    resets_at: Some(at(10 + 2 * 86_400).as_second()),
                    disabled: false,
                },
            ],
            subagents: vec![AgySubagent {
                name: "research".into(),
                running: true,
            }],
            task_count: 2,
            pending_input_count: 1,
            ..AgyStatusline::default()
        };
        let sl = Statusline {
            context: Some(ContextWindow {
                size: 1_048_576,
                used_percent: Some(21),
            }),
            agy: Some(agy_sl.clone()),
            ..Statusline::default()
        };
        let mut v = view(Some(&snapshot), false);
        v.resets = crate::config::AgentHudResets::Countdown;
        let utc = jiff::tz::TimeZone::UTC;
        v.local_tz = Some(&utc);
        v.metrics = Metrics::Agy {
            snapshot: Some(&snapshot),
            tail_only: false,
            reported: AgyReported {
                statusline: Some(&sl),
                plan: Some(&agy_sl),
            },
        };
        let rendered = lines(&v, AgentHudMode::Full, 240, &theme);
        let text: Vec<String> = rendered.iter().map(ToString::to_string).collect();
        // Effort is already inside "(High)", so it isn't duplicated.
        assert!(text[0].contains("Gemini 4 Neon (High)"), "{text:?}");
        assert!(!text[0].contains("(High) high"), "{text:?}");
        // Context share is tinted red past 200k even at 21%.
        assert!(text[1].contains("120.0k 21%"), "{text:?}");
        let pct_span = rendered[1]
            .spans
            .iter()
            .find(|s| s.content == "21%")
            .unwrap();
        assert_eq!(pct_span.style.fg, Some(theme.diff_del_fg));
        // Quotas, countdown, pace run-out, Argon, hit, out, t/s, compacted.
        assert!(text[1].contains("5h 75% ↻3h00m ~40m"), "{text:?}");
        assert!(text[1].contains("7d 30% ↻2d0h"), "{text:?}");
        assert!(text[1].contains("argon 85% ↻2d0h"), "{text:?}");
        assert!(text[1].contains("42t/s"), "{text:?}");
        // Mode row has background, queued, approval, sandbox, plan tier, live subagent.
        assert!(text[3].contains("background 2"), "{text:?}");
        assert!(text[3].contains("queued 1"), "{text:?}");
        assert!(text[3].contains("accept-edits"), "{text:?}");
        assert!(text[3].contains("sandbox (no net)"), "{text:?}");
        assert!(text[3].contains("plan Google AI Pro"), "{text:?}");
        assert!(text[3].contains("research · running"), "{text:?}");
        // Compact mode surfaces running subagents count.
        let compact = lines(&v, AgentHudMode::Compact, 240, &theme)[1].to_string();
        assert!(compact.contains("subagents 1"), "{compact:?}");
    }

    #[test]
    fn prompt_cache_ttl_warns_at_three_minutes_and_reads_cold_at_four() {
        let theme = Theme::default();
        let snapshot = session();
        let mut v = view(Some(&snapshot), false);

        v.now = Some(at(1 + 179));
        let warm = rows(&v, 160)[1].clone();
        assert!(warm.contains("hit 95%"), "{warm}");
        assert!(!warm.contains("3m") && !warm.contains("cold"), "{warm}");

        v.now = Some(at(1 + 180));
        let warn_line = lines(&v, AgentHudMode::Full, 160, &theme).swap_remove(1);
        assert!(warn_line.to_string().contains("hit 95% 3m"), "{warn_line}");
        assert_eq!(
            warn_line
                .spans
                .iter()
                .find(|s| s.content == "3m")
                .unwrap()
                .style
                .fg,
            Some(theme.prompt_prefix)
        );

        v.now = Some(at(1 + 240));
        let cold_line = lines(&v, AgentHudMode::Full, 160, &theme).swap_remove(1);
        assert!(
            cold_line.to_string().contains("hit 95% cold"),
            "{cold_line}"
        );
        assert_eq!(
            cold_line
                .spans
                .iter()
                .find(|s| s.content == "cold")
                .unwrap()
                .style
                .fg,
            Some(theme.diff_del_fg)
        );
    }
}
