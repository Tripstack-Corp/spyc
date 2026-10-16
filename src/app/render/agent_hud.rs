//! Draw the native strip from cached pane facts and the bound Model snapshot.

use ratatui::{Frame, layout::Rect};

use crate::agent::claude_statusline::Statusline;
use crate::agent::tail::Availability;
use crate::config::AgentHudMode;
use crate::pane::AgentActivity;
use crate::ui::agent_hud::{self, Attention, Hooks, HudView, Metrics, Reported};

use super::super::state::agent_hud::{Feed, Update};

use super::super::App;

impl App {
    pub(super) fn render_agent_hud_strip(&self, frame: &mut Frame, rect: Option<Rect>) {
        let Some(rect) = rect else { return };
        self.draw_chrome_rows(frame, rect, self.agent_hud_lines(rect));
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
                AgentActivity::Blocked => Attention::NeedsYou,
                AgentActivity::Done => Attention::Done,
                AgentActivity::Idle => Attention::Idle,
                AgentActivity::Unknown => Attention::Unknown,
            }
        };
        // What a `waiting` agent waits on, as it or its `Stop` hook said.
        let note = (!info.suspended && info.activity == AgentActivity::Waiting)
            .then_some(info.reported.as_ref())
            .flatten()
            .filter(|r| r.status == AgentActivity::Waiting)
            .and_then(|r| r.note.as_deref());
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
            note,
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
        let mode = if rect.height == 2 {
            AgentHudMode::Compact
        } else {
            AgentHudMode::Full
        };
        agent_hud::lines(&view, mode, rect.width, &self.view.theme)
    }
}

impl App {
    /// The plan windows the strip shows, which only a Claude Code tab's has.
    pub(in crate::app) fn agent_hud_plan(&self) -> Option<&Statusline> {
        let tabs = self.runtime.project().pane_tabs.as_ref()?;
        let info = &tabs.tabs()[tabs.active_index()].info;
        match metrics_of(self.active_agent_hud_update(), tabs, info) {
            Metrics::Claude { reported, .. } => reported.plan,
            _ => None,
        }
    }
}

/// What the tabs' statuslines reported: this tab's context share, when its
/// report is about this tab's session, and the newest plan windows any tab saw.
fn reported<'a>(
    tabs: &'a crate::pane::PaneTabs,
    info: &'a crate::pane::tabs::TabInfo,
) -> Reported<'a> {
    let context_percent = info
        .claude_statusline
        .as_ref()
        .filter(|(report, _)| {
            report.session_id.is_none() || report.session_id == info.live_session_id
        })
        .and_then(|(report, _)| report.context?.used_percent);
    let plan = tabs
        .tabs()
        .iter()
        .filter_map(|entry| entry.info.claude_statusline.as_ref())
        .filter(|(report, _)| report.five_hour.is_some() || report.seven_day.is_some())
        .max_by_key(|(_, at)| *at)
        .map(|(report, _)| report);
    Reported {
        context_percent,
        plan,
    }
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
            },
            _ => Metrics::Codex(None),
        },
    }
}
