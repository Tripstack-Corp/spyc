//! Pure native agent strip geometry and text, for Codex and Claude Code tabs.
//! The caller supplies every source, timestamp and attention state; this
//! renderer owns no clock or collection.

use jiff::Timestamp;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::{display_truncate, display_truncate_tail, display_width, theme::Theme};
use crate::agent::claude_metrics;
use crate::agent::codex_metrics::{Availability, LimitState, Sample, Snapshot, Tokens, ToolStatus};
use crate::config::{AgentHudMode, HUD_STALE_AFTER_SECS};

mod claude;

pub struct HudLayout {
    pub child: Rect,
    pub strip: Option<Rect>,
}

pub fn partition(rect: Rect, requested: AgentHudMode) -> HudLayout {
    let rows = match requested {
        AgentHudMode::Off => 0,
        _ if rect.width == 0 || rect.height < 8 => 0,
        AgentHudMode::Full if rect.height >= 10 => 4,
        _ => 2,
    };
    let child = Rect {
        height: rect.height.saturating_sub(rows),
        ..rect
    };
    let strip = (rows != 0).then_some(Rect {
        y: child.y + child.height,
        height: rows,
        ..rect
    });
    HudLayout { child, strip }
}

/// The agent's own metrics, `None` until its source is available.
#[derive(Clone, Copy)]
pub enum Metrics<'a> {
    Codex(Option<&'a Snapshot>),
    Claude {
        snapshot: Option<&'a claude_metrics::Snapshot>,
        /// The read began part-way through the transcript, so its totals
        /// miss what came before.
        tail_only: bool,
    },
}

pub struct HudView<'a> {
    pub session: Option<&'a str>,
    pub attention: &'a str,
    pub needs_user: bool,
    pub repository: Option<&'a str>,
    pub branch: Option<&'a str>,
    pub elapsed_secs: Option<u64>,
    pub metrics: Metrics<'a>,
    pub availability: Option<Availability>,
    pub now: Option<Timestamp>,
    pub hooks: &'a str,
    pub mcp_connections: usize,
}

pub fn lines(
    view: &HudView<'_>,
    mode: AgentHudMode,
    width: u16,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if mode == AgentHudMode::Off || width == 0 {
        return Vec::new();
    }
    let (metrics, mode_row, tools) = match view.metrics {
        Metrics::Codex(snapshot) => (
            metrics(snapshot, view),
            operating_mode(snapshot, view),
            tools(snapshot),
        ),
        Metrics::Claude {
            snapshot,
            tail_only,
        } => (
            claude::metrics(snapshot, tail_only, view.availability),
            claude::operating_mode(snapshot, view),
            claude::tools(snapshot, tail_only),
        ),
    };
    let mut out = vec![identity(view, width, theme), plain(metrics, width, theme)];
    if mode == AgentHudMode::Full {
        out.push(plain(mode_row, width, theme));
        out.push(plain(tools, width, theme));
    }
    out
}

/// The model, effort and whether that report is stale, once one arrived.
fn model(view: &HudView<'_>) -> Option<String> {
    let fresh = |stale: bool| if stale { " (stale)" } else { "" };
    let (model, effort, stale) = match view.metrics {
        Metrics::Codex(snapshot) => {
            let mode = snapshot?.mode.as_ref()?;
            let value = &mode.value;
            (&value.model, &value.effort, stale(mode, view.now))
        }
        Metrics::Claude { snapshot, .. } => {
            let mode = snapshot?.mode.as_ref()?;
            let value = &mode.value;
            let stale = view
                .now
                .is_some_and(|now| mode.is_stale_at(now, HUD_STALE_AFTER_SECS));
            (&value.model, &value.effort, stale)
        }
    };
    let mut label = model.as_deref().unwrap_or("model --").to_owned();
    if let Some(effort) = effort.as_deref() {
        label.push_str(" · ");
        label.push_str(effort);
    }
    label.push_str(fresh(stale));
    Some(label)
}

fn identity(view: &HudView<'_>, width: u16, theme: &Theme) -> Line<'static> {
    let session = view
        .session
        .map_or("session --", |id| display_truncate_tail(id, 8));
    let primary = safe_label(&format!("{} · {session}", view.attention));
    let mut text = display_truncate(&primary, usize::from(width)).to_owned();
    if let Some(model) = model(view) {
        append(&mut text, &model, width);
    }
    if let Some(repository) = view.repository {
        let repo = view.branch.map_or_else(
            || repository.to_owned(),
            |branch| format!("{repository} ({branch})"),
        );
        append(&mut text, &repo, width);
    }
    if let Some(seconds) = view.elapsed_secs {
        append(&mut text, &format!("{}m", seconds / 60), width);
    }
    let color = if theme.mono {
        ratatui::style::Color::Reset
    } else if view.needs_user {
        theme.diff_del_fg
    } else {
        theme.status_path
    };
    Line::from(Span::styled(
        text,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
}

fn metrics(snapshot: Option<&Snapshot>, view: &HudView<'_>) -> String {
    let Some(s) = snapshot else {
        return format!(
            "Context -- | Usage -- | {}",
            source_label(view.availability, "rollout")
        );
    };
    let request = s.request.as_ref().or(s.legacy_last.as_ref());
    let thread = s.thread.as_ref().or(s.legacy_thread.as_ref());
    let legacy = if s.thread.is_none() && s.legacy_thread.is_some() {
        " legacy"
    } else {
        ""
    };
    let limits = s
        .limits
        .as_ref()
        .map_or("not reported".to_owned(), |sample| {
            let state = match sample.value {
                LimitState::NotReported => "not reported",
                LimitState::UnsupportedWindow => "unsupported",
            };
            format!(
                "{state}{}",
                if stale(sample, view.now) {
                    " (stale)"
                } else {
                    ""
                }
            )
        });
    format!(
        "Context -- | Req {} | Turn {} | Thread {}{legacy} | Limits {limits}",
        token_sample(request, view.now),
        token_sample(s.turn.as_ref(), view.now),
        token_sample(thread, view.now)
    )
}

fn operating_mode(snapshot: Option<&Snapshot>, view: &HudView<'_>) -> String {
    let mode = snapshot.and_then(|s| s.mode.as_ref()).map(|s| &s.value);
    let approval = mode.and_then(|m| m.approval.as_deref()).unwrap_or("--");
    let sandbox = mode.and_then(|m| m.sandbox.as_deref()).unwrap_or("--");
    format!(
        "Mode: {approval} / {sandbox} | hooks: {} | MCP: {} observed",
        view.hooks, view.mcp_connections
    )
}

fn tools(snapshot: Option<&Snapshot>) -> String {
    let Some(snapshot) = snapshot else {
        return "Observed tools: --".to_owned();
    };
    if snapshot.tools.is_empty() {
        return "Observed tools: -- (incomplete source)".to_owned();
    }
    let recent = snapshot
        .tools
        .iter()
        .rev()
        .take(2)
        .map(|tool| {
            let status = match tool.status {
                ToolStatus::Started => "start observed",
                ToolStatus::Completed => "completed",
                ToolStatus::Failed => "failed",
            };
            format!("{} {status}", tool.name.as_deref().unwrap_or("tool"))
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("Observed tools (partial): {recent}")
}

fn token_sample(sample: Option<&Sample<Tokens>>, now: Option<Timestamp>) -> String {
    let Some(sample) = sample else {
        return "--".to_owned();
    };
    let value = sample.value.total.map_or("--".to_owned(), short_count);
    format!("{value}{}", if stale(sample, now) { " stale" } else { "" })
}

fn stale<T>(sample: &Sample<T>, now: Option<Timestamp>) -> bool {
    now.is_some_and(|now| sample.is_stale_at(now, HUD_STALE_AFTER_SECS))
}

pub(super) fn short_count(n: u64) -> String {
    for (scale, suffix) in [(1_000_000_000, "B"), (1_000_000, "M"), (1_000, "k")] {
        if n >= scale {
            return format!("{}.{}{suffix}", n / scale, n % scale / (scale / 10));
        }
    }
    n.to_string()
}

/// Why the metrics are unknown. `file` names the agent's session file.
fn source_label(source: Option<Availability>, file: &str) -> String {
    match source {
        None => "awaiting pinned metrics".to_owned(),
        Some(Availability::Available) => "Limits not reported".to_owned(),
        Some(Availability::Missing) => format!("{file} missing"),
        Some(Availability::Unreadable) => format!("{file} unreadable"),
        Some(Availability::IdentityMismatch) => "identity mismatch".to_owned(),
        Some(Availability::InvalidHeader) => format!("invalid {file} header"),
        Some(Availability::WatchFailed) => "watch unavailable".to_owned(),
    }
}

fn append(text: &mut String, field: &str, width: u16) {
    let field = safe_label(field);
    let field = field.as_str();
    if display_width(text) + 3 + display_width(field) <= usize::from(width) {
        text.push_str(" | ");
        text.push_str(field);
    }
}

fn plain(text: String, width: u16, theme: &Theme) -> Line<'static> {
    let text = safe_label(&text);
    let color = if theme.mono {
        ratatui::style::Color::Reset
    } else {
        theme.status_path
    };
    Line::from(Span::styled(
        display_truncate(&text, usize::from(width)).to_owned(),
        Style::default().fg(color),
    ))
}

/// Paths and labels can contain control characters; never emit terminal commands.
pub fn safe_label(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::codex_metrics::Source;

    #[test]
    fn parallel_session_labels_remain_distinct_and_untrusted_labels_cannot_escape() {
        let mut view = HudView {
            session: Some("01a133b2-dc16-74c8-af3c-111111111111"),
            attention: "idle",
            needs_user: false,
            repository: Some("中文\x1b[31m\nrepo"),
            branch: Some("emoji❤️\tbranch"),
            elapsed_secs: None,
            metrics: Metrics::Codex(None),
            availability: None,
            now: None,
            hooks: "defined; unverified",
            mcp_connections: 0,
        };
        let first = lines(&view, AgentHudMode::Compact, 20, &Theme::default())[0].to_string();
        view.session = Some("01a133b2-dc16-74c8-af3c-222222222222");
        let second = lines(&view, AgentHudMode::Compact, 20, &Theme::default())[0].to_string();
        assert_ne!(
            first, second,
            "parallel sessions sharing a timestamp prefix need distinct labels"
        );
        let wide = lines(&view, AgentHudMode::Full, 180, &Theme::default());
        let header = wide[0].to_string();
        assert!(header.contains("中文") && header.contains("❤️"));
        assert!(
            !header.chars().any(char::is_control),
            "HUD labels must be terminal-safe: {header:?}"
        );
    }

    #[test]
    fn every_height_keeps_six_child_rows_or_hides_the_strip() {
        for height in 0..40 {
            for mode in [AgentHudMode::Off, AgentHudMode::Compact, AgentHudMode::Full] {
                let rect = Rect::new(7, 11, 32, height);
                let parts = partition(rect, mode);
                if let Some(strip) = parts.strip {
                    assert!(parts.child.height >= 6);
                    assert_eq!(strip.y, parts.child.y + parts.child.height);
                    assert_eq!(parts.child.height + strip.height, rect.height);
                    assert_eq!(strip.width, rect.width);
                } else {
                    assert_eq!(parts.child, rect);
                }
            }
        }
        assert_eq!(
            partition(Rect::new(0, 0, 80, 9), AgentHudMode::Full)
                .strip
                .unwrap()
                .height,
            2
        );
        assert!(
            partition(Rect::new(0, 0, 0, 20), AgentHudMode::Full)
                .strip
                .is_none()
        );
    }

    #[test]
    fn unknown_context_stays_unknown_even_when_lifetime_tokens_exceed_the_window() {
        let at = "2026-10-13T12:00:00Z".parse().unwrap();
        let snapshot = Snapshot {
            thread: Some(Sample {
                at,
                source: Source::UsageRecord,
                value: Tokens {
                    total: Some(900_000_000),
                    ..Tokens::default()
                },
            }),
            model_context_window: Some(Sample {
                at,
                source: Source::LegacyTokenCount,
                value: 100,
            }),
            ..Snapshot::default()
        };
        let view = HudView {
            session: Some("12345678"),
            attention: "needs you",
            needs_user: true,
            repository: Some("中文❤️"),
            branch: Some("👨‍👩‍👧"),
            elapsed_secs: None,
            metrics: Metrics::Codex(Some(&snapshot)),
            availability: Some(Availability::Available),
            now: Some(at),
            hooks: "defined, unverified",
            mcp_connections: 1,
        };
        for width in 1..130 {
            for mode in [AgentHudMode::Compact, AgentHudMode::Full] {
                let rendered = lines(&view, mode, width, &Theme::default());
                assert!(
                    rendered
                        .iter()
                        .all(|line| display_width(&line.to_string()) <= usize::from(width))
                );
                if width >= 24 {
                    assert!(rendered[0].to_string().contains("needs you · 12345678"));
                }
                assert!(!rendered.iter().any(|line| line.to_string().contains('%')));
            }
        }
        let rendered = lines(&view, AgentHudMode::Full, 160, &Theme::default());
        assert_eq!(rendered.len(), 4);
        assert!(rendered[1].to_string().contains("Context --"));
        assert!(rendered[1].to_string().contains("900.0M"));
        assert!(rendered[3].to_string().contains("incomplete"));
    }
}
