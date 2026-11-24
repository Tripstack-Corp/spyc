//! Draw the native strip from cached pane facts and the bound Model snapshot.

use ratatui::{Frame, layout::Rect};

use crate::agent::tail::Availability;
use crate::pane::AgentActivity;
use crate::state::sessions::AgentKind;
use crate::ui::agent_hud::{self, AgyReported, Attention, Hooks, HudView, Metrics, Reported};

use super::super::state::agent_hud::{Feed, Update};

use super::super::App;

impl App {
    pub(super) fn render_agent_hud_strip(&self, frame: &mut Frame, rect: Option<Rect>) {
        let Some(rect) = rect else { return };
        let mut lines = vec![agent_hud::rule(rect.width, &self.view.theme)];
        lines.extend(self.agent_hud_lines(rect));
        self.draw_chrome_rows(frame, rect, lines);
    }

    /// Shared visible projection for drawing and wake-driven dirty-frame decisions.
    pub(in crate::app) fn agent_hud_lines(&self, rect: Rect) -> Vec<ratatui::text::Line<'static>> {
        let Some(tabs) = self.runtime.project().pane_tabs.as_ref() else {
            return Vec::new();
        };
        let entry = &tabs.tabs()[tabs.active_index()];
        let info = &entry.info;
        let update = self.active_agent_hud_update();
        let metrics = metrics_of(update, tabs, info);
        let cwd = entry.live_cwd();
        let repository = super::super::path_basename_display(&cwd);
        // A different cached column cannot establish the pane's branch.
        let branch = self.state.active_sides().into_iter().find_map(|side| {
            let column = self.state.col(side);
            (column.listing.dir == cwd)
                .then(|| {
                    column
                        .git_cache
                        .head_branch_cache
                        .as_ref()
                        .filter(|(root, _, _)| {
                            column.git_cache.current_repo_root.as_deref() == Some(root.as_path())
                                && cwd.starts_with(root)
                        })
                        .map(|(_, _, name)| name.as_str())
                })
                .flatten()
        });
        let attention = if info.suspended {
            Attention::Suspended
        } else {
            match info.activity {
                AgentActivity::Working => Attention::Working,
                AgentActivity::Waiting => Attention::Waiting,
                AgentActivity::Blocked | AgentActivity::Error => Attention::NeedsYou,
                AgentActivity::Done => Attention::Done,
                AgentActivity::Idle => Attention::Idle,
                AgentActivity::Unknown => Attention::Unknown,
            }
        };
        let note = activity_note(info);
        let hooks = if !info.recent_hook_events.is_empty() {
            Hooks::Reported
        } else if info.status_hooks_restart_needed {
            Hooks::RestartNeeded
        } else if info.status_hooks_at_spawn {
            Hooks::Unverified
        } else {
            Hooks::Absent
        };
        // The tab's own dot, so the strip and the tab never disagree.
        let dot = (!info.suspended)
            .then(|| self.agent_activity_span(info.activity, info.anim_phase_offset))
            .flatten();
        let elapsed_secs = self
            .state
            .active_project()
            .pane
            .agent_hud
            .as_of
            .and_then(|now| {
                u64::try_from(now.as_second())
                    .ok()
                    .map(|now| now.saturating_sub(info.agent_epoch_secs()))
            });
        let view = HudView {
            session: info.pinned_session_id(),
            dot,
            attention,
            note: note.as_deref(),
            repository: Some(&repository),
            branch,
            elapsed_secs,
            metrics,
            availability: update.map(Update::availability),
            now: self.state.active_project().pane.agent_hud.as_of,
            local_tz: Some(&self.view.local_tz),
            resets: self.state.active_project().config.pane.agent_hud_resets,
            hooks,
            mcp_connections: self
                .view
                .activity
                .mcp_connections
                .values()
                .filter(|c| c.pane_id.as_deref() == Some(info.id.as_str()))
                .count(),
        };
        agent_hud::lines(
            &view,
            agent_hud::shown_mode(rect),
            rect.width,
            &self.view.theme,
        )
    }
}

/// A visible plan window whose reset or countdown pace estimate can change on
/// its own as time passes.
pub(in crate::app) struct ResetWindow {
    pub resets_at: i64,
    /// `(used_percent_x100, window_secs)` when the strip shows a pace run-out
    /// estimate in `countdown` mode.
    pub pace: Option<(u16, i64)>,
}

impl App {
    /// Visible plan windows whose resets (or countdown pace estimates) need
    /// wake edges on the active Claude Code or Antigravity tab.
    pub(in crate::app) fn agent_hud_reset_windows(&self) -> Vec<ResetWindow> {
        let Some(tabs) = self.runtime.project().pane_tabs.as_ref() else {
            return Vec::new();
        };
        let info = &tabs.tabs()[tabs.active_index()].info;
        match metrics_of(self.active_agent_hud_update(), tabs, info) {
            Metrics::Claude { reported, .. } => reported
                .plan
                .into_iter()
                .flat_map(|plan| [plan.five_hour, plan.seven_day])
                .flatten()
                .filter_map(|w| {
                    Some(ResetWindow {
                        resets_at: w.resets_at?,
                        pace: None,
                    })
                })
                .collect(),
            Metrics::Agy {
                snapshot, reported, ..
            } => {
                let model = reported
                    .statusline
                    .and_then(|s| s.agy.as_ref())
                    .and_then(|a| a.model.as_deref())
                    .or_else(|| {
                        snapshot
                            .and_then(|s| s.mode.as_ref())
                            .and_then(|m| m.value.model.as_deref())
                    });
                reported.plan.map_or_default(|plan| {
                    let active = plan.active_quotas(model);
                    let argon = active
                        .argon
                        .filter(|b| b.used_percent() >= agent_hud::ARGON_SURFACE_USED_PCT);
                    [active.five_hour, active.seven_day, argon]
                        .into_iter()
                        .flatten()
                        .filter_map(|b| {
                            Some(ResetWindow {
                                resets_at: b.resets_at?,
                                pace: b.window_secs().map(|w| (b.used_percent_x100, w)),
                            })
                        })
                        .collect()
                })
            }
            Metrics::Codex(_) => Vec::new(),
        }
    }
}

/// What ends the activity row: what a `waiting` agent waits on, as it or its
/// `Stop` hook said, or what a blocked one does, as its hook or screen said.
fn activity_note(info: &crate::pane::tabs::TabInfo) -> Option<std::borrow::Cow<'_, str>> {
    match info.activity {
        _ if info.suspended => None,
        AgentActivity::Waiting => info
            .reported
            .as_ref()
            .filter(|r| r.status == AgentActivity::Waiting)
            .and_then(|r| r.note.as_deref())
            .map(std::borrow::Cow::Borrowed),
        AgentActivity::Blocked => info.waiting_for(),
        _ => None,
    }
}

/// What the tabs' statuslines reported for Claude Code: this tab's context
/// share, when its report is about this tab's session, and the newest plan
/// windows any Claude tab saw.
fn reported<'a>(
    tabs: &'a crate::pane::PaneTabs,
    info: &'a crate::pane::tabs::TabInfo,
) -> Reported<'a> {
    let context_percent = info
        .statusline
        .as_ref()
        .filter(|(report, _)| {
            report.session_id.is_none() || report.session_id == info.live_session_id
        })
        .and_then(|(report, _)| report.context?.used_percent);
    let plan = tabs
        .tabs()
        .iter()
        .filter(|entry| entry.info.agent().kind() == AgentKind::Claude)
        .filter_map(|entry| entry.info.statusline.as_ref())
        .filter(|(report, _)| report.five_hour.is_some() || report.seven_day.is_some())
        .max_by_key(|(_, at)| *at)
        .map(|(report, _)| report);
    Reported {
        context_percent,
        plan,
        credential: info.claude_credential,
    }
}

/// What the tabs' statuslines reported for Antigravity: this tab's own report
/// (model, context share, mode, sandbox, live subagents) and the newest quota
/// snapshot any Antigravity tab saw.
pub(in crate::app) fn agy_reported<'a>(
    tabs: &'a crate::pane::PaneTabs,
    info: &'a crate::pane::tabs::TabInfo,
) -> AgyReported<'a> {
    let statusline = info
        .statusline
        .as_ref()
        .filter(|(report, _)| {
            report.session_id.is_none() || report.session_id == info.live_session_id
        })
        .map(|(report, _)| report);
    let plan = tabs
        .tabs()
        .iter()
        .filter(|entry| entry.info.agent().kind() == AgentKind::Agy)
        .filter_map(|entry| entry.info.statusline.as_ref())
        .filter_map(|(report, at)| {
            let agy = report.agy.as_ref()?;
            (!agy.quotas.is_empty()).then_some((agy, *at))
        })
        .max_by_key(|(_, at)| *at)
        .map(|(agy, _)| agy);
    AgyReported { statusline, plan }
}

/// The agent's own metrics for the strip, from its update and the tabs'
/// statusline reports.
fn metrics_of<'a>(
    update: Option<&'a Update>,
    tabs: &'a crate::pane::PaneTabs,
    info: &'a crate::pane::tabs::TabInfo,
) -> Metrics<'a> {
    match update {
        Some(Update::Codex(u)) => {
            Metrics::Codex((u.availability == Availability::Available).then_some(&u.snapshot))
        }
        Some(Update::Claude(u)) => Metrics::Claude {
            snapshot: (u.availability == Availability::Available).then_some(&u.snapshot),
            tail_only: u.tail_only,
            reported: reported(tabs, info),
        },
        Some(Update::Agy(u)) => Metrics::Agy {
            snapshot: (u.availability == Availability::Available).then_some(&u.snapshot),
            tail_only: u.tail_only,
            reported: agy_reported(tabs, info),
        },
        None => match super::super::agent_hud::feed(info.agent().kind()) {
            Some(Feed::Claude) => Metrics::Claude {
                snapshot: None,
                tail_only: false,
                reported: reported(tabs, info),
            },
            Some(Feed::Agy) => Metrics::Agy {
                snapshot: None,
                tail_only: false,
                reported: agy_reported(tabs, info),
            },
            _ => Metrics::Codex(None),
        },
    }
}
