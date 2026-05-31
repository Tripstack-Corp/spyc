//! MVU Phase 2: the timer/deadline scheduler for `App::run`.
//!
//! Extracted from `app/mod.rs` to keep the event-loop file under the
//! anti-monolith ceiling (`app::guard_tests::mod_rs_stays_decomposed`).
//! The scheduler is **advisory** — it only computes the `recv_timeout`
//! wait from the set of armed deadlines; it never fires a timer (the
//! loop's existing predicates do that, against the threaded `now`).

use std::time::Instant;

use crate::pane::PaneTabs;

/// MVU Phase 2 timer/deadline kinds — one per loop-cadence timer in
/// `App::run`. An armed deadline is *advisory*: its only contract is
/// "the loop woke at or before the armed instant". The loop always
/// re-evaluates each timer's own fire predicate against the freshly
/// captured `now`, so an armed deadline never blindly fires work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deadline {
    /// 1 Hz git safety-net poll (1 s normal / 10 s huge-tree). Armed only
    /// while `git_info.is_some()`. PRE-recv timer.
    GitPoll,
    /// 1 s activity-monitor rollover. Armed only while `show_activity`.
    /// POST-recv timer.
    ActivityRollover,
    /// Watcher-driven listing-refresh trailing-debounce (`should_fire_refresh`).
    /// PRE-recv timer.
    RefreshQuiet,
    /// ~150 ms MCP context-write debounce. Armed while `context_dirty`.
    /// POST-recv timer.
    ContextWrite,
    /// Restore banner-settle (2 s) before sending `/resume <sid>`. The
    /// absolute instant lives per-tab in `pending_resume_send`. PRE-recv.
    RestoreSettle,
    /// Resume enter-delay (300 ms) before submitting `/resume`. Per-tab in
    /// `pending_resume_send`. PRE-recv.
    ResumeEnter,
}

/// Run()-local deadline scheduler (MVU Phase 2). **Advisory** — it only
/// computes the `recv_timeout` wait from the set of armed deadlines; it
/// never fires a timer (the loop's existing predicates do, against the
/// threaded `now`). No thread, no channel. `armed` stays tiny (≤ 6).
pub struct Scheduler {
    armed: Vec<(Instant, Deadline)>,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self { armed: Vec::new() }
    }

    /// Arm or re-arm `which` at `when` (replaces any existing entry — an
    /// idempotent re-arm).
    pub fn arm(&mut self, which: Deadline, when: Instant) {
        self.armed.retain(|(_, d)| *d != which);
        self.armed.push((when, which));
    }

    /// Disarm a deadline (its timer is no longer relevant).
    pub fn disarm(&mut self, which: Deadline) {
        self.armed.retain(|(_, d)| *d != which);
    }

    /// Earliest armed instant, or `None` when nothing is armed.
    pub fn next(&self) -> Option<Instant> {
        self.armed.iter().map(|(t, _)| *t).min()
    }
}

/// Arm `RestoreSettle`/`ResumeEnter` at the earliest pending resume
/// across all tabs, or disarm when none is pending. The absolute fire
/// instants already live per-tab in `pending_resume_send`; the scheduler
/// only needs the min so the wait can wake for it (the floor dominates
/// when a pane is present, so this is byte-identical on the real restore
/// path — it only tightens the no-pane edge). Re-scanned every iteration
/// since `Text → Enter` transitions change which is pending.
pub fn arm_resume_deadlines(scheduler: &mut Scheduler, tabs: Option<&PaneTabs>) {
    use crate::pane::tabs::PendingResumeSend;
    let mut settle: Option<Instant> = None;
    let mut enter: Option<Instant> = None;
    if let Some(tabs) = tabs {
        for entry in tabs.tabs() {
            match &entry.info.pending_resume_send {
                Some(PendingResumeSend::Text { after, .. }) => {
                    settle = Some(settle.map_or(*after, |m: Instant| m.min(*after)));
                }
                Some(PendingResumeSend::Enter { after }) => {
                    enter = Some(enter.map_or(*after, |m: Instant| m.min(*after)));
                }
                None => {}
            }
        }
    }
    match settle {
        Some(t) => scheduler.arm(Deadline::RestoreSettle, t),
        None => scheduler.disarm(Deadline::RestoreSettle),
    }
    match enter {
        Some(t) => scheduler.arm(Deadline::ResumeEnter, t),
        None => scheduler.disarm(Deadline::ResumeEnter),
    }
}
