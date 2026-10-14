//! How a live `waiting` report meets the reports that follow it.
//!
//! `waiting` is "not your turn, not finished": the agent ended its turn while
//! background work it started is still running, and resumes when that work
//! wakes it. Every agent's `Stop` hook reports `done` at a turn end, so without
//! a rule here the hook would erase a `waiting` as soon as the turn that
//! reported it ended. Claude Code's `Stop` says whether work is pending, and
//! the reporter turns that `done` into `waiting` itself
//! (`mcp::effective_report_state`). Other agents' `Stop` can't say, so a
//! self-reported `waiting` absorbs the one `done` that ends its own turn.
//!
//! A `waiting` the agent reports while its turn is still open (starting a
//! background watch and carrying on) is held until that turn ends: until
//! then the agent is working. The hooks say when a turn opens and ends, so an
//! agent without them has no open turn and its `waiting` applies at once.

use std::time::Instant;

use crate::agent::status_hook::StatusHookEvent;
use crate::pane::{AgentActivity, ReportedStatus};

/// Where a report came from.
#[derive(Clone, Copy, Debug)]
pub enum Origin<'a> {
    /// The agent's own `report_status` call, or a Lua script's.
    Agent,
    /// spyc's status hook, with the event's metadata when the payload named it.
    Hook(Option<&'a StatusHookEvent>),
}

/// What a report does to a live `waiting` report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// It replaces the live report.
    Apply,
    /// It is the `done` ending the turn that reported `waiting`. The live
    /// report stays, marked so that the next turn's `done` applies.
    AbsorbTurnEnd,
    /// It says nothing new about the pending work, so the live report stays.
    Absorb,
}

impl Verdict {
    /// Why the report was recorded but not applied, for the diagnostics.
    pub const fn ignored_because(self) -> Option<&'static str> {
        match self {
            Self::Apply => None,
            Self::AbsorbTurnEnd => Some("ends the turn that reported waiting"),
            Self::Absorb => Some("background work is still pending"),
        }
    }
}

/// Whether the agent's turn is open after a hook report of `incoming`: a
/// prompt opens it, the turn's end closes it, and anything in between, a
/// question or Claude Code's idle nudge, leaves it as it was.
pub fn turn_open_after(
    open: bool,
    incoming: AgentActivity,
    event: Option<&StatusHookEvent>,
) -> bool {
    if event.is_some_and(|e| e.hook_event_name == "Notification") {
        return open;
    }
    match incoming {
        AgentActivity::Working => true,
        AgentActivity::Done | AgentActivity::Waiting => false,
        _ => open,
    }
}

/// Whether a report is the agent's own `waiting` sent while its turn is open,
/// which is held until the turn ends rather than shown at once.
pub fn held_for_turn_end(incoming: AgentActivity, origin: Origin<'_>, turn_open: bool) -> bool {
    turn_open && incoming == AgentActivity::Waiting && matches!(origin, Origin::Agent)
}

/// The tab's report, when it is a `waiting` that hasn't expired.
fn live_waiting(live: Option<&ReportedStatus>, now: Instant) -> Option<&ReportedStatus> {
    live.filter(|r| r.status == AgentActivity::Waiting && now < r.expiry)
}

/// How a report of `incoming` from `origin` meets the tab's `live` report.
///
/// Only a hook's `done` is held back, since only a hook reports a turn end the
/// agent didn't choose to. The agent's own reports always apply, and so does
/// any hook state other than `done`: a new prompt (`working`) means the agent
/// woke, and a question (`blocked`) needs the user more than the wait does.
pub fn meet(
    live: Option<&ReportedStatus>,
    incoming: AgentActivity,
    origin: Origin<'_>,
    now: Instant,
) -> Verdict {
    let Some(live) = live_waiting(live, now) else {
        return Verdict::Apply;
    };
    let Origin::Hook(event) = origin else {
        return Verdict::Apply;
    };
    if incoming != AgentActivity::Done {
        return Verdict::Apply;
    }
    // Claude's idle nudge, which the reporter turns from `blocked` into `done`.
    if event.is_some_and(|e| e.hook_event_name == "Notification") {
        return Verdict::Absorb;
    }
    match event.and_then(StatusHookEvent::wakes_pending) {
        // The agent itself says nothing will wake it.
        Some(false) => Verdict::Apply,
        Some(true) => Verdict::Absorb,
        None if live.turn_end_absorbed => Verdict::Apply,
        None => Verdict::AbsorbTurnEnd,
    }
}

/// The note an applied report keeps: its own, else, for a `waiting` that
/// follows a live `waiting`, that report's, else what the hook said is pending.
/// The agent's words outrank the hook's count.
pub fn note_for(
    live: Option<&ReportedStatus>,
    incoming: AgentActivity,
    note: Option<String>,
    event: Option<&StatusHookEvent>,
    now: Instant,
) -> Option<String> {
    if incoming != AgentActivity::Waiting || note.is_some() {
        return note;
    }
    live_waiting(live, now)
        .and_then(|r| r.note.clone())
        .or_else(|| event.and_then(StatusHookEvent::wakes_summary))
}

#[cfg(test)]
mod tests {
    use super::{AgentActivity, Origin, ReportedStatus, StatusHookEvent, Verdict, meet, note_for};
    use std::time::{Duration, Instant};

    use AgentActivity::{Blocked, Done, Idle, Waiting, Working};

    fn report(status: AgentActivity, now: Instant) -> ReportedStatus {
        ReportedStatus::new(status, now, now + Duration::from_secs(3600))
    }

    fn event(name: &str, tasks: Option<u32>, crons: Option<u32>) -> StatusHookEvent {
        StatusHookEvent {
            hook_event_name: name.into(),
            tool_name: None,
            turn_id: None,
            tool_use_id: None,
            notification_type: None,
            background_tasks: tasks,
            session_crons: crons,
        }
    }

    #[test]
    fn a_prompt_opens_the_turn_its_end_closes_it_and_nothing_between_moves_it() {
        use super::turn_open_after;
        let prompt = event("UserPromptSubmit", None, None);
        let stop = event("Stop", Some(1), Some(0));
        let nudge = event("Notification", None, None);
        let question = event("PreToolUse", None, None);
        assert!(turn_open_after(false, Working, Some(&prompt)));
        assert!(turn_open_after(false, Working, None), "agy names no event");
        for end in [Done, Waiting] {
            assert!(!turn_open_after(true, end, Some(&stop)), "{end:?}");
            assert!(!turn_open_after(true, end, None), "{end:?}: agy's Stop");
        }
        for open in [true, false] {
            assert_eq!(turn_open_after(open, Blocked, Some(&question)), open);
            assert_eq!(
                turn_open_after(open, Done, Some(&nudge)),
                open,
                "Claude Code's idle nudge, reported as done, ends no turn"
            );
            assert_eq!(turn_open_after(open, Blocked, Some(&nudge)), open);
        }
    }

    #[test]
    fn only_the_agents_own_waiting_inside_an_open_turn_is_held() {
        use super::held_for_turn_end;
        let stop = event("Stop", Some(1), Some(0));
        assert!(held_for_turn_end(Waiting, Origin::Agent, true));
        assert!(
            !held_for_turn_end(Waiting, Origin::Agent, false),
            "no turn open"
        );
        assert!(
            !held_for_turn_end(Waiting, Origin::Hook(Some(&stop)), true),
            "a hook's waiting is the turn's end"
        );
        for other in [Working, Blocked, Done, Idle] {
            assert!(!held_for_turn_end(other, Origin::Agent, true), "{other:?}");
        }
    }

    /// The case the state exists for: a codex or agy `Stop`, which can't say
    /// whether work is pending, arrives right after the agent reported
    /// `waiting`. That one is held; the next turn's `Stop` is not.
    #[test]
    fn a_waiting_report_absorbs_the_turn_end_that_follows_it_once() {
        let now = Instant::now();
        let stop = event("Stop", None, None);
        let mut live = report(Waiting, now);
        assert_eq!(
            meet(Some(&live), Done, Origin::Hook(Some(&stop)), now),
            Verdict::AbsorbTurnEnd
        );
        live.turn_end_absorbed = true;
        assert_eq!(
            meet(Some(&live), Done, Origin::Hook(Some(&stop)), now),
            Verdict::Apply,
            "a later Stop means the agent resumed and finished"
        );
        live.turn_end_absorbed = false;
        assert_eq!(
            meet(Some(&live), Done, Origin::Hook(None), now),
            Verdict::AbsorbTurnEnd,
            "a hook payload with no event name is still a turn end"
        );
    }

    #[test]
    fn claude_code_stop_decides_for_itself_whether_work_is_pending() {
        let now = Instant::now();
        let live = report(Waiting, now);
        for (tasks, crons) in [(Some(0), Some(0)), (Some(0), None), (None, Some(0))] {
            assert_eq!(
                meet(
                    Some(&live),
                    Done,
                    Origin::Hook(Some(&event("Stop", tasks, crons))),
                    now
                ),
                Verdict::Apply,
                "empty lists ({tasks:?}, {crons:?}): nothing will wake it"
            );
        }
        assert_eq!(
            meet(
                Some(&live),
                Done,
                Origin::Hook(Some(&event("Stop", Some(1), Some(0)))),
                now
            ),
            Verdict::Absorb
        );
        assert_eq!(
            meet(
                Some(&live),
                Done,
                Origin::Hook(Some(&event("Notification", None, None))),
                now
            ),
            Verdict::Absorb,
            "Claude's idle nudge is not a turn end"
        );
    }

    #[test]
    fn only_a_hook_done_against_a_live_waiting_is_held_back() {
        let now = Instant::now();
        let stop = event("Stop", None, None);
        let live = report(Waiting, now);
        assert_eq!(meet(Some(&live), Done, Origin::Agent, now), Verdict::Apply);
        for incoming in [Working, Blocked, Idle, Waiting] {
            assert_eq!(
                meet(Some(&live), incoming, Origin::Hook(Some(&stop)), now),
                Verdict::Apply,
                "{incoming:?}"
            );
        }
        for other in [Working, Done, Idle, Blocked] {
            assert_eq!(
                meet(
                    Some(&report(other, now)),
                    Done,
                    Origin::Hook(Some(&stop)),
                    now
                ),
                Verdict::Apply,
                "a live {other:?} report"
            );
        }
        assert_eq!(
            meet(None, Done, Origin::Hook(Some(&stop)), now),
            Verdict::Apply
        );
        let expired = ReportedStatus::new(Waiting, now, now);
        assert_eq!(
            meet(Some(&expired), Done, Origin::Hook(Some(&stop)), now),
            Verdict::Apply,
            "an expired waiting holds nothing back"
        );
    }

    #[test]
    fn a_waiting_note_is_the_agents_words_before_the_hooks_count() {
        let now = Instant::now();
        let stop = event("Stop", Some(2), Some(1));
        let mut live = report(Waiting, now);
        live.note = Some("CI on #641".into());
        let note = |live: Option<&ReportedStatus>, incoming, own: Option<&str>| {
            note_for(live, incoming, own.map(String::from), Some(&stop), now)
        };

        assert_eq!(
            note(Some(&live), Waiting, Some("deploy")).as_deref(),
            Some("deploy")
        );
        assert_eq!(
            note(Some(&live), Waiting, None).as_deref(),
            Some("CI on #641")
        );
        assert_eq!(
            note(None, Waiting, None).as_deref(),
            Some("2 background tasks, 1 scheduled wakeup")
        );
        assert_eq!(note(Some(&live), Done, None), None, "only waiting inherits");
        assert_eq!(
            note_for(
                None,
                Waiting,
                None,
                Some(&event("Stop", Some(1), None)),
                now
            )
            .as_deref(),
            Some("1 background task")
        );
        assert_eq!(note_for(None, Waiting, None, None, now), None);
    }
}
