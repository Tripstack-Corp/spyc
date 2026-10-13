//! Draw the native strip from cached pane facts and the bound Model snapshot.

use ratatui::{Frame, layout::Rect};

use crate::agent::codex_metrics::Availability;
use crate::config::AgentHudMode;
use crate::pane::AgentActivity;
use crate::ui::agent_hud::{self, HudView};

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
        let snapshot =
            update.and_then(|u| (u.availability == Availability::Available).then_some(&u.snapshot));
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
        let hooks = if !info.recent_hook_events.is_empty() {
            "events reported"
        } else if info.status_hooks_restart_needed {
            "restart needed"
        } else if info.status_hooks_at_spawn {
            "defined; unverified"
        } else {
            "not present at launch"
        };
        let elapsed_secs = self.state.pane.agent_hud.as_of.and_then(|now| {
            u64::try_from(now.as_second())
                .ok()
                .map(|now| now.saturating_sub(info.agent_epoch_secs()))
        });
        let view = HudView {
            session: info.codex_session_id.as_deref(),
            attention,
            needs_user: info.activity == AgentActivity::Blocked,
            repository: Some(&repository),
            branch,
            elapsed_secs,
            snapshot,
            availability: update.map(|u| u.availability),
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
