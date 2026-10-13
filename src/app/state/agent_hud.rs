//! One visible HUD consumer, keyed by its complete agent incarnation. No handles,
//! filesystem access or clock reads live in this Model.

use std::path::PathBuf;

use jiff::Timestamp;

use crate::agent::tail::Availability;
use crate::agent::{claude_metrics, codex_metrics};
use crate::config::HUD_STALE_AFTER_SECS;

/// Whose session files feed the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feed {
    Codex,
    Claude,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub feed: Feed,
    pub pane_id: String,
    /// Codex's thread, or Claude Code's session.
    pub session_id: String,
    /// `CODEX_HOME`, or Claude Code's configuration directory.
    pub home: PathBuf,
}

impl Target {
    pub fn matches(&self, binding: &Binding) -> bool {
        match binding {
            Binding::Codex(b) => {
                self.feed == Feed::Codex
                    && self.pane_id == b.pane_id
                    && self.session_id == b.thread_id
                    && self.home == b.codex_home
            }
            Binding::Claude(b) => {
                self.feed == Feed::Claude
                    && self.pane_id == b.pane_id
                    && self.session_id == b.session_id
                    && self.home == b.claude_home
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Binding {
    Codex(codex_metrics::Binding),
    Claude(claude_metrics::Binding),
}

impl Binding {
    fn new(target: Target, generation: u64) -> Self {
        match target.feed {
            Feed::Codex => Self::Codex(codex_metrics::Binding {
                pane_id: target.pane_id,
                thread_id: target.session_id,
                codex_home: target.home,
                generation,
            }),
            Feed::Claude => Self::Claude(claude_metrics::Binding {
                pane_id: target.pane_id,
                session_id: target.session_id,
                claude_home: target.home,
                generation,
            }),
        }
    }

    pub const fn generation(&self) -> u64 {
        match self {
            Self::Codex(b) => b.generation,
            Self::Claude(b) => b.generation,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    Codex(Box<codex_metrics::Update>),
    Claude(Box<claude_metrics::Update>),
}

impl Update {
    /// An empty outcome for a source that never opened.
    pub fn unavailable(binding: Binding, availability: Availability) -> Self {
        match binding {
            Binding::Codex(binding) => Self::Codex(Box::new(codex_metrics::Update {
                binding,
                availability,
                tail_only: false,
                snapshot: codex_metrics::Snapshot::default(),
            })),
            Binding::Claude(binding) => Self::Claude(Box::new(claude_metrics::Update {
                binding,
                availability,
                tail_only: false,
                snapshot: claude_metrics::Snapshot::default(),
            })),
        }
    }

    /// Closed or rebound panes supply no matching binding.
    pub fn is_for(&self, binding: &Binding) -> bool {
        match (self, binding) {
            (Self::Codex(u), Binding::Codex(b)) => &u.binding == b,
            (Self::Claude(u), Binding::Claude(b)) => &u.binding == b,
            _ => false,
        }
    }

    pub const fn availability(&self) -> Availability {
        match self {
            Self::Codex(u) => u.availability,
            Self::Claude(u) => u.availability,
        }
    }

    pub const fn tail_only(&self) -> bool {
        match self {
            Self::Codex(u) => u.tail_only,
            Self::Claude(u) => u.tail_only,
        }
    }

    pub const fn generation(&self) -> u64 {
        match self {
            Self::Codex(u) => u.binding.generation,
            Self::Claude(u) => u.binding.generation,
        }
    }

    /// Every sample whose freshness the strip or diagnostics show.
    fn sample_seconds(&self) -> Vec<i64> {
        let times = match self {
            Self::Codex(u) => {
                let s = &u.snapshot;
                vec![
                    s.request.as_ref().map(|v| v.at),
                    s.turn.as_ref().map(|v| v.at),
                    s.thread.as_ref().map(|v| v.at),
                    s.legacy_last.as_ref().map(|v| v.at),
                    s.legacy_thread.as_ref().map(|v| v.at),
                    s.mode.as_ref().map(|v| v.at),
                    s.limits.as_ref().map(|v| v.at),
                ]
            }
            Self::Claude(u) => {
                let s = &u.snapshot;
                vec![
                    s.request.as_ref().map(|v| v.at),
                    s.context_tokens.as_ref().map(|v| v.at),
                    s.mode.as_ref().map(|v| v.at),
                    s.compaction.as_ref().map(|v| v.at),
                ]
            }
        };
        times
            .into_iter()
            .flatten()
            .map(Timestamp::as_second)
            .collect()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConsumerChange {
    Start(Binding),
    Stop,
}

#[derive(Default)]
pub struct AgentHudState {
    /// Claude Code's configuration directory, from spyc's environment at
    /// startup: the one its panes inherit, and where spyc wired their hooks.
    pub claude_home: Option<PathBuf>,
    pub binding: Option<Binding>,
    pub update: Option<Update>,
    pub as_of: Option<Timestamp>,
    pub work: (u64, u64),
    next_generation: u64,
}

impl AgentHudState {
    pub fn new(claude_home: Option<PathBuf>) -> Self {
        Self {
            claude_home,
            ..Self::default()
        }
    }

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
            Some(Binding::new(target, self.next_generation))
        });
        self.binding.clone()
    }

    pub fn accept(&mut self, update: Update, work: (u64, u64), now: Timestamp) -> bool {
        if !self.binding.as_ref().is_some_and(|b| update.is_for(b)) {
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
        self.update.as_ref().filter(|u| u.is_for(binding))
    }

    /// The next finite freshness edge. Old samples arm nothing at idle.
    pub fn next_expiry(&self, now: Timestamp) -> Option<i64> {
        self.sample_seconds()
            .into_iter()
            .filter_map(|at| at.checked_add(HUD_STALE_AFTER_SECS + 1))
            .filter(|at| *at > now.as_second())
            .min()
    }

    pub fn advance_time(&mut self, now: Timestamp) -> bool {
        let changed = self.as_of.is_some_and(|old| {
            self.sample_seconds().into_iter().any(|at| {
                old.as_second().saturating_sub(at) <= HUD_STALE_AFTER_SECS
                    && now.as_second().saturating_sub(at) > HUD_STALE_AFTER_SECS
            })
        });
        self.as_of = Some(now);
        changed
    }

    fn sample_seconds(&self) -> Vec<i64> {
        self.update
            .as_ref()
            .map_or_else(Vec::new, Update::sample_seconds)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::agent::codex_metrics::{Sample, Snapshot, Source, Tokens};

    fn target(home: &str) -> Target {
        Target {
            feed: Feed::Codex,
            pane_id: "pane-a".into(),
            session_id: "thread-a".into(),
            home: home.into(),
        }
    }

    fn start(model: &mut AgentHudState) -> codex_metrics::Binding {
        match model.set_target(Some(target("/home/a"))) {
            Some(ConsumerChange::Start(Binding::Codex(binding))) => binding,
            other => panic!("expected a new consumer: {other:?}"),
        }
    }

    fn codex(binding: codex_metrics::Binding, tail_only: bool, snapshot: Snapshot) -> Update {
        Update::Codex(Box::new(codex_metrics::Update {
            binding,
            availability: Availability::Available,
            tail_only,
            snapshot,
        }))
    }

    #[test]
    fn stopping_and_returning_to_the_same_pane_rejects_its_old_worker() {
        let mut model = AgentHudState::default();
        let first = start(&mut model);
        assert!(model.set_target(Some(target("/home/a"))).is_none());
        assert_eq!(model.set_target(None), Some(ConsumerChange::Stop));
        let second = start(&mut model);
        let now = "2026-10-13T12:00:00Z".parse().unwrap();
        let old = codex(first, false, Snapshot::default());
        assert!(!model.accept(old, (1, 1), now));
        assert!(model.update.is_none());
        assert!(model.accept(codex(second, true, Snapshot::default()), (2, 1), now));
        assert!(model.update_for(&target("/home/b")).is_none());
        assert!(model.update_for(&target("/home/a")).unwrap().tail_only());
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
        model.accept(codex(binding, false, snapshot), (100, 1), at);
        assert_eq!(model.next_expiry(at), Some(at.as_second() + 61));
        let later: Timestamp = "2026-10-13T12:01:01Z".parse().unwrap();
        assert!(model.advance_time(later));
        assert_eq!(model.next_expiry(later), None);
        assert!(!model.advance_time("2026-10-13T13:00:00Z".parse().unwrap()));
    }

    /// A tab that switches agents keeps its pane id, and the same session id and
    /// home can name either agent's files: neither may read the other's.
    #[test]
    fn a_claude_target_never_accepts_or_matches_a_codex_incarnation() {
        let mut model = AgentHudState::default();
        let codex_binding = start(&mut model);
        let claude = Target {
            feed: Feed::Claude,
            ..target("/home/a")
        };
        assert!(!claude.matches(&Binding::Codex(codex_binding.clone())));
        let Some(ConsumerChange::Start(Binding::Claude(binding))) =
            model.set_target(Some(claude.clone()))
        else {
            panic!("a feed change must start a new consumer");
        };
        assert_eq!(binding.session_id, "thread-a");
        assert_eq!(binding.claude_home, PathBuf::from("/home/a"));
        let now = "2026-10-13T12:00:00Z".parse().unwrap();
        let mut stale_codex = codex(codex_binding, false, Snapshot::default());
        if let Update::Codex(update) = &mut stale_codex {
            update.binding.generation = binding.generation;
        }
        assert!(
            !model.accept(stale_codex, (1, 1), now),
            "a Codex result cannot land on a Claude consumer of the same generation"
        );
        let update = Update::Claude(Box::new(claude_metrics::Update {
            binding,
            availability: Availability::Available,
            tail_only: false,
            snapshot: claude_metrics::Snapshot::default(),
        }));
        assert!(model.accept(update, (1, 1), now));
        assert!(model.update_for(&claude).is_some());
        assert!(model.update_for(&target("/home/a")).is_none());
    }
}
