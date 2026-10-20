//! Reconcile the one visible agent strip consumer, for a Codex, Claude Code
//! or Antigravity tab, through effects. Metric wakes supplement the display
//! without changing the agent's semantic activity.

use jiff::Timestamp;
use ratatui::layout::Rect;

use super::state::agent_hud::{Binding, ConsumerChange, Feed, Target, Update};
use super::{App, Deadline, Effect, RunCtx};
use crate::config::AgentHudMode;
use crate::state::sessions::AgentKind;

pub(in crate::app) mod collector;
mod diagnostics;

impl App {
    pub(super) fn agent_hud_mode(&self) -> AgentHudMode {
        if self.view.image_view.is_some() {
            return AgentHudMode::Off;
        }
        let eligible = self
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .is_some_and(|tabs| {
                !tabs.active().is_closed() && feed(tabs.active_info().agent().kind()).is_some()
            });
        if eligible {
            self.agent_hud_preference()
        } else {
            AgentHudMode::Off
        }
    }

    /// The mode asked for, whether or not the active pane can show it.
    pub(super) fn agent_hud_preference(&self) -> AgentHudMode {
        self.state
            .active_project()
            .pane
            .agent_hud
            .mode(self.state.active_project().config.pane.agent_hud)
    }

    pub(super) fn agent_hud_target(&self) -> Option<Target> {
        let (cols, rows) = self.view.term_size;
        self.frame_layout(Rect::new(0, 0, cols, rows)).agent_hud?;
        let info = self.runtime.project().pane_tabs.as_ref()?.active_info();
        let feed = feed(info.agent().kind())?;
        let (session, home) = match feed {
            Feed::Codex => (&info.codex_session_id, &info.codex_home),
            Feed::Claude => (
                &info.live_session_id,
                &self.state.active_project().pane.agent_hud.claude_home,
            ),
            Feed::Agy => (
                &info.live_session_id,
                &self.state.active_project().pane.agent_hud.agy_home,
            ),
        };
        let session = session
            .as_deref()
            .filter(|id| crate::state::sessions::is_uuid(id))?;
        let home = home.as_ref().filter(|home| home.is_absolute())?;
        Some(Target {
            feed,
            pane_id: info.id.clone(),
            session_id: session.to_owned(),
            home: home.clone(),
        })
    }

    /// Whether terminal row `y` is one of the visible strip's rows.
    pub(super) fn on_agent_hud(&self, y: u16) -> bool {
        let (cols, rows) = self.view.term_size;
        self.frame_layout(Rect::new(0, 0, cols, rows))
            .agent_hud
            .is_some_and(|strip| (strip.y..strip.bottom()).contains(&y))
    }

    /// A statusline report from `pane_id`'s Claude Code. Only a live tab running
    /// Claude Code keeps one.
    pub(super) fn land_statusline(
        &mut self,
        pane_id: &str,
        statusline: crate::agent::claude_statusline::Statusline,
    ) {
        let Some(tabs) = self.runtime.project_mut().pane_tabs.as_mut() else {
            return;
        };
        if let Some(entry) = tabs.tabs_mut().iter_mut().find(|e| e.info.id == pane_id)
            && !entry.pane.is_closed()
            && entry.info.agent().kind() == AgentKind::Claude
        {
            entry.info.claude_statusline = Some((statusline, Timestamp::now()));
        }
    }

    pub(super) fn active_agent_hud_update(&self) -> Option<&Update> {
        self.state
            .active_project()
            .pane
            .agent_hud
            .update_for(&self.agent_hud_target()?)
    }

    pub(super) fn execute_agent_hud(&mut self, binding: Option<Binding>) {
        if binding.as_ref() != self.state.active_project().pane.agent_hud.binding.as_ref() {
            return;
        }
        self.runtime.agent_hud = binding
            .map(|binding| collector::Collector::start(binding, self.runtime.pane_wake_tx.clone()));
    }

    pub(super) fn apply_agent_hud_updates(&mut self) -> bool {
        let target = self.agent_hud_target();
        let Some(collector) = self.runtime.agent_hud.as_mut() else {
            return false;
        };
        let Some(update) = collector.take() else {
            return false;
        };
        if !target.as_ref().is_some_and(|target| {
            self.state
                .active_project()
                .pane
                .agent_hud
                .binding
                .as_ref()
                .is_some_and(|b| target.matches(b) && update.is_for(b))
        }) {
            return false;
        }
        let work = collector.work();
        self.land_agent_hud_update(update, work, Timestamp::now())
    }

    fn land_agent_hud_update(&mut self, update: Update, work: (u64, u64), now: Timestamp) -> bool {
        let (cols, rows) = self.view.term_size;
        let Some(rect) = self.frame_layout(Rect::new(0, 0, cols, rows)).agent_hud else {
            return false;
        };
        let before = self.agent_hud_lines(rect);
        self.state
            .active_project_mut()
            .pane
            .agent_hud
            .accept(update, work, now);
        before != self.agent_hud_lines(rect)
    }

    pub(super) fn settle_agent_hud(
        &mut self,
        now: std::time::Instant,
        ctx: &mut RunCtx,
    ) -> Vec<Effect> {
        let target = self.agent_hud_target();
        let change = self
            .state
            .active_project_mut()
            .pane
            .agent_hud
            .set_target(target)
            .or_else(|| self.collector_lag());
        if change.is_some() {
            ctx.draw.mark(3);
        }
        let wall = Timestamp::now();
        let (cols, rows) = self.view.term_size;
        let rect = self.frame_layout(Rect::new(0, 0, cols, rows)).agent_hud;
        let before = rect.map(|rect| self.agent_hud_lines(rect));
        // A plan window reset, or a countdown ticked over, since the last settle.
        let reset_passed = rect
            .and(self.state.active_project().pane.agent_hud.as_of)
            .and_then(|since| self.agent_hud_reset_edge(since))
            .is_some_and(|edge| edge <= wall.as_second());
        let tz_moved = rect.is_some() && self.refresh_local_tz();
        let aged = self
            .state
            .active_project_mut()
            .pane
            .agent_hud
            .advance_time(wall);
        if (aged || reset_passed || tz_moved)
            && before != rect.map(|rect| self.agent_hud_lines(rect))
        {
            ctx.draw.mark(3);
        }
        let next = [
            self.state.active_project().pane.agent_hud.next_expiry(wall),
            rect.and_then(|_| self.agent_hud_reset_edge(wall)),
        ]
        .into_iter()
        .flatten()
        .min();
        match next {
            Some(epoch) => {
                let delay = epoch.saturating_sub(wall.as_second()) as u64;
                if let Some(when) = now.checked_add(std::time::Duration::from_secs(delay)) {
                    ctx.scheduler.arm(Deadline::AgentHudExpiry, when);
                } else {
                    ctx.scheduler.disarm(Deadline::AgentHudExpiry);
                }
            }
            None => ctx.scheduler.disarm(Deadline::AgentHudExpiry),
        }
        change.map_or_else(Vec::new, |change| {
            vec![Effect::AgentHud(match change {
                ConsumerChange::Start(binding) => Some(binding),
                ConsumerChange::Stop => None,
            })]
        })
    }

    /// The process runs one collector and each project keeps its own
    /// binding, so after a switch the project in view can hold a binding the
    /// collector isn't following, its target unchanged since it was last in
    /// view: start one on it, or stop the one following a project out of view.
    ///
    /// The collector started may reuse a binding whose earlier collector was
    /// cancelled. That is safe because results reach the Model only through
    /// the collector in `runtime.agent_hud`, each with its own channel, so a
    /// dropped one's never land. Each return re-runs the bounded startup
    /// lookup off the loop thread, as a tab switch does.
    fn collector_lag(&self) -> Option<ConsumerChange> {
        let bound = self.state.active_project().pane.agent_hud.binding.as_ref();
        let followed = self.runtime.agent_hud.as_ref().map(|c| &c.binding);
        (bound != followed).then(|| {
            bound
                .cloned()
                .map_or(ConsumerChange::Stop, ConsumerChange::Start)
        })
    }

    /// When the strip's plan cells next change on their own: a window
    /// resetting, or a countdown ticking over.
    fn agent_hud_reset_edge(&self, now: Timestamp) -> Option<i64> {
        let plan = self.agent_hud_plan()?;
        let style = self.state.active_project().config.pane.agent_hud_resets;
        [plan.five_hour, plan.seven_day]
            .into_iter()
            .flatten()
            .filter_map(|w| crate::ui::agent_hud::resets::next_change(w.resets_at?, now, style))
            .min()
    }

    /// Follows the system's zone, so a laptop that changes zone reads its new
    /// local time. jiff caches the lookup, so this is cheap to repeat.
    fn refresh_local_tz(&mut self) -> bool {
        let tz = jiff::tz::TimeZone::system();
        let moved = tz != self.view.local_tz;
        if moved {
            self.view.local_tz = tz;
        }
        moved
    }

    pub(super) fn cmd_agent_hud(&mut self, args: &str) -> Vec<Effect> {
        let mut words = args.split_whitespace();
        let command = words.next();
        if words.next().is_some() {
            return self.agent_hud_usage();
        }
        if matches!(command, Some("info" | "dump")) {
            let mut pager = crate::ui::pager::PagerView::new_plain(
                "agent HUD sources",
                diagnostics::lines(self),
            );
            pager.saveable = true;
            self.set_pager(pager);
            return Vec::new();
        }
        let Some(mode) = command.and_then(AgentHudMode::parse) else {
            return self.agent_hud_usage();
        };
        self.state.active_project_mut().pane.agent_hud.choice = Some(mode);
        self.clear_pane_selection();
        self.state
            .flash_info(format!("agent HUD: {}", mode.as_str()));
        Vec::new()
    }

    fn agent_hud_usage(&mut self) -> Vec<Effect> {
        self.state
            .flash_info("usage: :agent hud off|compact|full|info|dump");
        Vec::new()
    }
}

/// The agents whose session files can feed the strip.
pub(super) const fn feed(kind: AgentKind) -> Option<Feed> {
    match kind {
        AgentKind::Codex => Some(Feed::Codex),
        AgentKind::Claude => Some(Feed::Claude),
        AgentKind::Agy => Some(Feed::Agy),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
