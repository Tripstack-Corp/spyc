//! Draw the native strip from cached pane facts and the bound Model snapshot.

use ratatui::{Frame, layout::Rect};

use crate::agent::tail::Availability;
use crate::config::AgentHudMode;
use crate::pane::AgentActivity;
use crate::ui::agent_hud::{self, Hooks, HudView, Metrics};

use super::super::state::agent_hud::{Feed, Update};

use super::super::App;

impl App {
    pub(super) fn render_agent_hud_strip(&self, frame: &mut Frame, rect: Option<Rect>) {
        let Some(rect) = rect else { return };
        self.draw_chrome_rows(frame, rect, self.agent_hud_lines(rect));
    }

    /// Shared visible projection for drawing and wake-driven dirty-frame decisions.
    pub(in crate::app) fn agent_hud_lines(&self, rect: Rect) -> Vec<ratatui::text::Line<'static>> {
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return Vec::new();
        };
        let entry = &tabs.tabs()[tabs.active_index()];
        let info = &entry.info;
        let update = self.active_agent_hud_update();
        let metrics = match update {
            Some(Update::Codex(u)) => {
                Metrics::Codex((u.availability == Availability::Available).then_some(&u.snapshot))
            }
            Some(Update::Claude(u)) => Metrics::Claude {
                snapshot: (u.availability == Availability::Available).then_some(&u.snapshot),
                tail_only: u.tail_only,
            },
            None => match super::super::agent_hud::feed(info.agent().kind()) {
                Some(Feed::Claude) => Metrics::Claude {
                    snapshot: None,
                    tail_only: false,
                },
                _ => Metrics::Codex(None),
            },
        };
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
            "suspended"
        } else {
            match info.activity {
                AgentActivity::Working => "working",
                AgentActivity::Waiting => "waiting",
                AgentActivity::Blocked => "needs you",
                AgentActivity::Done => "done",
                AgentActivity::Idle => "idle",
                AgentActivity::Unknown => "status --",
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
        let elapsed_secs = self.state.pane.agent_hud.as_of.and_then(|now| {
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
            now: self.state.pane.agent_hud.as_of,
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
