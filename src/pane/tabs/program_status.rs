//! A tab's program status records (OSC 7501, #927): what the program running in
//! it said about itself, kept by the rules libghostty-vt's header gives for
//! `GHOSTTY_TERMINAL_OPT_PROGRAM_STATUS`.
//!
//! - A report replaces its record whole; a `clear` removes the record and every
//!   record nested under it (`build` takes `build/test` with it), and a `clear`
//!   with no id removes them all.
//! - A new shell prompt means what ran has ended: `working`, `blocked` and
//!   `idle` records go. `done` and `error` stay until the user has seen them,
//!   which here is their next settling keystroke into the tab (`acknowledge`).
//! - At most [`MAX_RECORDS`]; past it, the one updated longest ago goes.
//!
//! Pure: the loop feeds it the engine's `ProgramEvent`s and reads back the most
//! urgent state, which `App::effective_activity` ranks below a live
//! `report_status` and above the screen scrape.

use super::AgentActivity;
use crate::pane::engine::{BlockedOn, ProgramEvent, ProgramState, ProgramStatus};

/// The most records a tab keeps: the header's ceiling.
pub const MAX_RECORDS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    id: String,
    state: ProgramState,
    blocked_on: Option<BlockedOn>,
}

/// One tab's records, the one updated longest ago first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProgramRecords {
    records: Vec<Record>,
}

impl ProgramRecords {
    /// Apply one event from the program, by the rules in the module doc.
    pub fn apply(&mut self, event: &ProgramEvent) {
        match event {
            ProgramEvent::PromptStart => self.records.retain(|r| {
                !matches!(
                    r.state,
                    ProgramState::Working | ProgramState::Blocked | ProgramState::Idle
                )
            }),
            ProgramEvent::Status(ProgramStatus { id, state, .. })
                if *state == ProgramState::Clear =>
            {
                if id.is_empty() {
                    self.records.clear();
                } else {
                    self.records.retain(|r| !is_at_or_under(&r.id, id));
                }
            }
            ProgramEvent::Status(status) => {
                self.records.retain(|r| r.id != status.id);
                if self.records.len() == MAX_RECORDS {
                    self.records.remove(0);
                }
                self.records.push(Record {
                    id: status.id.clone(),
                    state: status.state,
                    blocked_on: status.blocked_on,
                });
            }
        }
    }

    /// The user has seen the tab and answered it: what waited on them, and
    /// what finished or failed while they were away, has been seen.
    pub fn acknowledge(&mut self) {
        self.records.retain(|r| {
            !matches!(
                r.state,
                ProgramState::Blocked | ProgramState::Done | ProgramState::Error
            )
        });
    }

    /// Forget everything: the program has exited.
    pub fn clear(&mut self) {
        self.records.clear();
    }

    /// The tab's state from its records: the most urgent one's, blocked first,
    /// then error, working, done and idle. `None` with no records.
    pub fn activity(&self) -> Option<AgentActivity> {
        self.records
            .iter()
            .map(|r| activity_of(r.state))
            .max_by_key(|a| urgency(*a))
    }

    /// What the most urgent blocked record waits on, when the program said.
    pub fn blocked_on(&self) -> Option<BlockedOn> {
        self.records
            .iter()
            .rev()
            .find(|r| r.state == ProgramState::Blocked)
            .and_then(|r| r.blocked_on)
    }

    #[cfg(test)]
    fn ids(&self) -> Vec<&str> {
        self.records.iter().map(|r| r.id.as_str()).collect()
    }
}

/// Whether `id` is `parent` or nested under it (`parent/...`).
fn is_at_or_under(id: &str, parent: &str) -> bool {
    id.strip_prefix(parent)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

const fn activity_of(state: ProgramState) -> AgentActivity {
    match state {
        ProgramState::Working => AgentActivity::Working,
        ProgramState::Done => AgentActivity::Done,
        ProgramState::Blocked => AgentActivity::Blocked,
        ProgramState::Error => AgentActivity::Error,
        ProgramState::Idle | ProgramState::Clear => AgentActivity::Idle,
    }
}

const fn urgency(a: AgentActivity) -> u8 {
    match a {
        AgentActivity::Blocked => 5,
        AgentActivity::Error => 4,
        AgentActivity::Working => 3,
        AgentActivity::Done => 2,
        AgentActivity::Waiting => 1,
        AgentActivity::Idle | AgentActivity::Unknown => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(id: &str, state: ProgramState) -> ProgramEvent {
        ProgramEvent::Status(ProgramStatus {
            id: id.into(),
            state,
            blocked_on: None,
            app: String::new(),
        })
    }

    fn with(events: &[ProgramEvent]) -> ProgramRecords {
        let mut r = ProgramRecords::default();
        for e in events {
            r.apply(e);
        }
        r
    }

    #[test]
    fn a_report_replaces_its_record() {
        let r = with(&[
            status("", ProgramState::Working),
            status("", ProgramState::Done),
        ]);
        assert_eq!(r.ids(), [""]);
        assert_eq!(r.activity(), Some(AgentActivity::Done));
    }

    #[test]
    fn the_most_urgent_record_sets_the_state() {
        let r = with(&[
            status("a", ProgramState::Done),
            status("b", ProgramState::Working),
            status("c", ProgramState::Error),
        ]);
        assert_eq!(r.activity(), Some(AgentActivity::Error));
        let r = with(&[
            status("a", ProgramState::Error),
            status("b", ProgramState::Blocked),
        ]);
        assert_eq!(r.activity(), Some(AgentActivity::Blocked));
        assert_eq!(ProgramRecords::default().activity(), None);
    }

    #[test]
    fn a_clear_takes_its_record_and_those_nested_under_it() {
        let mut r = with(&[
            status("build", ProgramState::Working),
            status("build/test", ProgramState::Error),
            status("builder", ProgramState::Working),
            status("lint", ProgramState::Done),
        ]);
        r.apply(&status("build", ProgramState::Clear));
        assert_eq!(r.ids(), ["builder", "lint"]);
        r.apply(&status("", ProgramState::Clear));
        assert_eq!(r.activity(), None);
    }

    #[test]
    fn a_new_prompt_keeps_only_what_the_user_has_yet_to_see() {
        let mut r = with(&[
            status("w", ProgramState::Working),
            status("b", ProgramState::Blocked),
            status("i", ProgramState::Idle),
            status("d", ProgramState::Done),
            status("e", ProgramState::Error),
        ]);
        r.apply(&ProgramEvent::PromptStart);
        assert_eq!(r.ids(), ["d", "e"]);
    }

    #[test]
    fn acknowledging_drops_what_waited_on_the_user_and_what_they_have_now_seen() {
        let mut r = with(&[
            status("w", ProgramState::Working),
            status("b", ProgramState::Blocked),
            status("d", ProgramState::Done),
            status("e", ProgramState::Error),
        ]);
        r.acknowledge();
        assert_eq!(r.ids(), ["w"]);
    }

    #[test]
    fn past_the_limit_the_record_updated_longest_ago_goes() {
        let mut r = ProgramRecords::default();
        for i in 0..=MAX_RECORDS {
            r.apply(&status(&i.to_string(), ProgramState::Working));
        }
        assert_eq!(r.records.len(), MAX_RECORDS);
        assert_eq!(r.ids()[0], "1");
        // An update moves a record to the newest end.
        r.apply(&status("1", ProgramState::Done));
        assert_eq!(*r.ids().last().unwrap(), "1");
    }

    #[test]
    fn a_blocked_record_says_what_it_waits_on() {
        let r = with(&[ProgramEvent::Status(ProgramStatus {
            id: String::new(),
            state: ProgramState::Blocked,
            blocked_on: Some(BlockedOn::Permission),
            app: "deploy".into(),
        })]);
        assert_eq!(r.blocked_on(), Some(BlockedOn::Permission));
    }
}
