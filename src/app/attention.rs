//! `Space !` / `^a !`: going to the agent that needs you, in any project (#99).
//!
//! [`pick`] orders every agent waiting on the user: blocked ones first, the
//! longest-waiting first, then dones not yet seen. It goes on from the agent
//! the user is at, so pressing again walks through them all, even as a done
//! just visited stops counting. [`App::go_to_attention`] goes there: the
//! project, the tab, and the keyboard in its pane, which is brought into view
//! first.

use std::time::Instant;

use super::App;
use super::state::{self, ProjectId};
use crate::pane::AgentActivity;

/// An agent waiting on the user: where it is, and since when.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Waiting {
    /// Its project's place in switcher order.
    pub project: usize,
    pub tab: usize,
    pub blocked: bool,
    /// When its wait began. `None` sorts after every known time: a blocked
    /// report lands before the walk that times it.
    pub since: Option<Instant>,
}

/// A waiting agent's place in urgency order: blocked before an unseen done,
/// the longest-waiting first, then switcher and tab order. Unique per tab, so
/// the order doesn't depend on the order the projects are walked in.
pub(super) type WaitKey = (bool, bool, Option<Instant>, usize, usize);

impl Waiting {
    pub(super) const fn key(&self) -> WaitKey {
        (
            !self.blocked,
            self.since.is_none(),
            self.since,
            self.project,
            self.tab,
        )
    }
}

/// The agent to go to: the first after `after` in urgency order, wrapping,
/// or the most urgent when the user isn't at one. `after` needn't be listed
/// any more: a done the user has just seen has dropped out.
pub(super) fn pick(waiting: &[Waiting], after: Option<WaitKey>) -> Option<Waiting> {
    let mut order: Vec<&Waiting> = waiting.iter().collect();
    order.sort_by_key(|w| w.key());
    after
        .and_then(|key| order.iter().find(|w| w.key() > key))
        .or_else(|| order.first())
        .map(|w| **w)
}

impl App {
    /// `Space !`: go to the agent that needs you ([`pick`]), switching project
    /// and tab and giving its pane the keyboard, or say that none does.
    pub(super) fn go_to_attention(&mut self) {
        let ids: Vec<ProjectId> = self.state.open_projects().map(|p| p.id).collect();
        let here = ids.iter().position(|id| *id == self.state.active);
        let waiting = self.waiting_agents(&ids);
        let Some(to) = pick(&waiting, self.attention_from(here, &waiting)) else {
            self.state.flash_info("nothing needs you");
            return;
        };
        if Some(to.project) != here {
            self.switch_project(ids[to.project]);
        }
        self.bring_pane_into_view();
        self.go_to_tab(|tabs| tabs.switch_to(to.tab));
        let pane = self.runtime.project().pane_tabs.as_ref();
        self.view.attention_last = pane.map(|tabs| (tabs.active_info().id.clone(), to.key()));
    }

    /// Where the walk goes on from: the agent in view if it is waiting, else
    /// the last one `Space !` went to while the user is still at it, though it
    /// may have stopped waiting; else nowhere, so the walk starts at the top.
    fn attention_from(&self, here: Option<usize>, waiting: &[Waiting]) -> Option<WaitKey> {
        let tab = self.in_view_tab()?;
        let project = here?;
        if let Some(w) = waiting
            .iter()
            .find(|w| (w.project, w.tab) == (project, tab))
        {
            return Some(w.key());
        }
        let pane = &self.runtime.project().pane_tabs.as_ref()?.active_info().id;
        self.view
            .attention_last
            .as_ref()
            .filter(|(last, _)| last == pane)
            .map(|(_, key)| *key)
    }

    /// Every agent waiting on the user, in every project: blocked, or showing
    /// a done the user hasn't seen. An agent that has gone back to work keeps
    /// its unseen done but isn't waiting, as its dot shows. `ids` is the
    /// switcher's order.
    fn waiting_agents(&self, ids: &[ProjectId]) -> Vec<Waiting> {
        let mut waiting = Vec::new();
        for (id, tabs) in self.runtime.every_tab_set() {
            let Some(project) = ids.iter().position(|p| *p == id) else {
                continue;
            };
            for (tab, entry) in tabs.tabs().iter().enumerate() {
                let info = &entry.info;
                let blocked = info.activity == AgentActivity::Blocked;
                let unseen =
                    info.unseen_done.is_some() && info.shown_activity(false) == AgentActivity::Done;
                if blocked || unseen {
                    waiting.push(Waiting {
                        project,
                        tab,
                        blocked,
                        since: if blocked {
                            info.blocked_since
                        } else {
                            info.unseen_done
                        },
                    });
                }
            }
        }
        waiting
    }

    /// The pane the user is sent to, drawn: shown, not zoomed onto the right
    /// column, and not under a modal pager, closed as `q` would close a
    /// picker. A list zoom becomes the agent's pane zoomed, in
    /// [`App::go_to_tab`].
    fn bring_pane_into_view(&mut self) {
        if self.modal_pager_open() {
            self.clear_pending_pickers();
            self.clear_pager();
        }
        let pane = &mut self.state.active_project_mut().pane;
        if pane.pane_hidden || pane.zoom == state::ZoomTarget::RightColumn {
            pane.pane_hidden = false;
            if pane.zoom == state::ZoomTarget::RightColumn {
                pane.zoom = state::ZoomTarget::None;
            }
            self.resize_panes_to_layout();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Waiting, pick};
    use std::time::{Duration, Instant};

    fn at(project: usize, tab: usize, blocked: bool, since: Option<Instant>) -> Waiting {
        Waiting {
            project,
            tab,
            blocked,
            since,
        }
    }

    /// Where each press lands, starting from nowhere, for `n` presses.
    fn walk(all: &[Waiting], n: usize) -> Vec<(usize, usize)> {
        let mut after = None;
        (0..n)
            .map(|_| {
                let w = pick(all, after).expect("an agent");
                after = Some(w.key());
                (w.project, w.tab)
            })
            .collect()
    }

    /// Blocked before an unseen done whatever their times, the longest wait
    /// first, an untimed wait last, and switcher then tab order on a tie;
    /// then it wraps.
    #[test]
    fn the_most_urgent_comes_first() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let all = [
            at(0, 0, false, Some(t0)),
            at(2, 0, true, Some(t1)),
            at(1, 3, true, Some(t0)),
            at(0, 1, true, None),
            at(1, 0, true, Some(t0)),
        ];
        assert_eq!(
            walk(&all, 6),
            [(1, 0), (1, 3), (2, 0), (0, 1), (0, 0), (1, 0)]
        );
    }

    /// Two equal waits go in switcher order, whatever order they're listed
    /// in: the walk over the projects has none of its own.
    #[test]
    fn a_tie_goes_in_switcher_order() {
        let t0 = Instant::now();
        let all = [at(1, 0, true, Some(t0)), at(0, 0, true, Some(t0))];
        assert_eq!(walk(&all, 2), [(0, 0), (1, 0)]);
        assert_eq!(pick(&[], None), None);
    }

    /// From a done the user has just seen, which no longer waits, the walk
    /// goes on to the next, rather than back to the top.
    #[test]
    fn it_goes_on_past_an_agent_no_longer_waiting() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let seen = at(1, 0, false, Some(t0)).key();
        let all = [at(0, 0, true, Some(t1)), at(1, 1, false, Some(t1))];
        assert_eq!(
            pick(&all, Some(seen)).map(|w| (w.project, w.tab)),
            Some((1, 1))
        );
    }
}
