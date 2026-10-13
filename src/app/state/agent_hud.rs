//! One visible HUD consumer, keyed by its complete agent incarnation. No handles,
//! filesystem access or clock reads live in this Model.

use std::path::PathBuf;

use jiff::Timestamp;

use crate::agent::codex_metrics::{Binding, Update};
use crate::config::HUD_STALE_AFTER_SECS;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub pane_id: String,
    pub thread_id: String,
    pub codex_home: PathBuf,
}

impl Target {
    pub fn matches(&self, binding: &Binding) -> bool {
        self.pane_id == binding.pane_id
            && self.thread_id == binding.thread_id
            && self.codex_home == binding.codex_home
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConsumerChange {
    Start(Binding),
    Stop,
}

#[derive(Default)]
pub struct AgentHudState {
    pub binding: Option<Binding>,
    pub update: Option<Update>,
    pub as_of: Option<Timestamp>,
    pub work: (u64, u64),
    next_generation: u64,
}

impl AgentHudState {
    /// No change emits no effect; a transition starts or stops the consumer.
    pub fn set_target(&mut self, target: Option<Target>) -> Option<ConsumerChange> {
        let unchanged = match (&target, &self.binding) {
            (None, None) => true,
            (Some(target), Some(binding)) => target.matches(binding),
            _ => false,
        };
        if unchanged {
            return None;
        }
        Some(
            self.replace_target(target)
                .map_or(ConsumerChange::Stop, ConsumerChange::Start),
        )
    }

    /// A new consumer generation never reuses a cancelled worker's binding.
    fn replace_target(&mut self, target: Option<Target>) -> Option<Binding> {
        self.update = None;
        self.work = (0, 0);
        self.binding = target.and_then(|target| {
            // An exhausted counter cancels instead of reusing an old incarnation.
            self.next_generation = self.next_generation.checked_add(1)?;
            Some(Binding {
                pane_id: target.pane_id,
                thread_id: target.thread_id,
                codex_home: target.codex_home,
                generation: self.next_generation,
            })
        });
        self.binding.clone()
    }

    pub fn accept(&mut self, update: Update, work: (u64, u64), now: Timestamp) -> bool {
        if update.for_binding(self.binding.as_ref()).is_none() {
            return false;
        }
        let changed = self.update.as_ref() != Some(&update);
        self.update = Some(update);
        self.work = work;
        self.as_of = Some(now);
        changed
    }

    pub fn update_for(&self, target: &Target) -> Option<&Update> {
        let binding = self.binding.as_ref().filter(|b| target.matches(b))?;
        self.update.as_ref().filter(|u| &u.binding == binding)
    }

    /// The next finite freshness edge. Old samples arm nothing at idle.
    pub fn next_expiry(&self, now: Timestamp) -> Option<i64> {
        self.sample_seconds()
            .into_iter()
            .flatten()
            .filter_map(|at| at.checked_add(HUD_STALE_AFTER_SECS + 1))
            .filter(|at| *at > now.as_second())
            .min()
    }

    pub fn advance_time(&mut self, now: Timestamp) -> bool {
        let changed = self.as_of.is_some_and(|old| {
            self.sample_seconds().into_iter().flatten().any(|at| {
                old.as_second().saturating_sub(at) <= HUD_STALE_AFTER_SECS
                    && now.as_second().saturating_sub(at) > HUD_STALE_AFTER_SECS
            })
        });
        self.as_of = Some(now);
        changed
    }

    fn sample_seconds(&self) -> [Option<i64>; 7] {
        let Some(update) = self.update.as_ref() else {
            return [None; 7];
        };
        let s = &update.snapshot;
        [
            s.request.as_ref().map(|v| v.at.as_second()),
            s.turn.as_ref().map(|v| v.at.as_second()),
            s.thread.as_ref().map(|v| v.at.as_second()),
            s.legacy_last.as_ref().map(|v| v.at.as_second()),
            s.legacy_thread.as_ref().map(|v| v.at.as_second()),
            s.mode.as_ref().map(|v| v.at.as_second()),
            s.limits.as_ref().map(|v| v.at.as_second()),
        ]
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::agent::codex_metrics::{Availability, Sample, Snapshot, Source, Tokens};

    fn target(home: &str) -> Target {
        Target {
            pane_id: "pane-a".into(),
            thread_id: "thread-a".into(),
            codex_home: home.into(),
        }
    }

    fn start(model: &mut AgentHudState) -> Binding {
        match model.set_target(Some(target("/home/a"))) {
            Some(ConsumerChange::Start(binding)) => binding,
            other => panic!("expected a new consumer: {other:?}"),
        }
    }

    #[test]
    fn stopping_and_returning_to_the_same_pane_rejects_its_old_worker() {
        let mut model = AgentHudState::default();
        let first = start(&mut model);
        assert!(model.set_target(Some(target("/home/a"))).is_none());
        assert_eq!(model.set_target(None), Some(ConsumerChange::Stop));
        let second = start(&mut model);
        let now = "2026-10-13T12:00:00Z".parse().unwrap();
        let old = Update {
            binding: first,
            availability: Availability::Available,
            tail_only: false,
            snapshot: Snapshot::default(),
        };
        assert!(!model.accept(old, (1, 1), now));
        assert!(model.update.is_none());
        assert!(model.accept(
            Update {
                binding: second,
                availability: Availability::Available,
                tail_only: true,
                snapshot: Snapshot::default()
            },
            (2, 1),
            now
        ));
        assert!(model.update_for(&target("/home/b")).is_none());
        assert!(model.update_for(&target("/home/a")).unwrap().tail_only);
    }

    #[test]
    fn freshness_arms_only_a_finite_edge_and_never_an_idle_countdown() {
        let mut model = AgentHudState::default();
        let binding = start(&mut model);
        let at: Timestamp = "2026-10-13T12:00:00Z".parse().unwrap();
        let snapshot = Snapshot {
            request: Some(Sample {
                at,
                source: Source::UsageRecord,
                value: Tokens {
                    total: Some(0),
                    ..Tokens::default()
                },
            }),
            ..Snapshot::default()
        };
        model.accept(
            Update {
                binding,
                availability: Availability::Available,
                tail_only: false,
                snapshot,
            },
            (100, 1),
            at,
        );
        assert_eq!(model.next_expiry(at), Some(at.as_second() + 61));
        let later: Timestamp = "2026-10-13T12:01:01Z".parse().unwrap();
        assert!(model.advance_time(later));
        assert_eq!(model.next_expiry(later), None);
        assert!(!model.advance_time("2026-10-13T13:00:00Z".parse().unwrap()));
    }
}
