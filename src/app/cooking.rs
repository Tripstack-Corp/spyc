//! Whether spyc is cooking: work the user asked for, running off the UI
//! thread, that they may be waiting on. A worker holds a [`CookingGuard`]
//! while it runs; the status bar's 🌶️ simmers while any is held, once the
//! oldest has run [`COOKING_GRACE`], so quick work never flickers it.
//!
//! The count is shared with the workers, and only the loop reads the clock:
//! [`App::settle_cooking`] notes when cooking started and advances the frame
//! the draw pass reads, arming [`Deadline::Cooking`] only while cooking, so an
//! idle spyc still wakes 0 times a second. A worker's guard may drop just
//! after its wake; the next frame's tick notices.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::App;
use super::run::RunCtx;
use super::scheduler::Deadline;

/// How long work runs before the pepper simmers.
pub const COOKING_GRACE: Duration = Duration::from_millis(250);

/// One frame of the simmer, the agent dots' 100 ms.
pub const COOKING_FRAME: Duration = Duration::from_millis(100);

/// How many workers are cooking. Cloned into each worker as a guard.
#[derive(Clone, Default)]
pub struct Cooking(Arc<AtomicUsize>);

/// One worker's share of [`Cooking`]: counted from [`Cooking::start`] until
/// it drops, a panic's unwind included.
#[must_use = "the work counts only while the guard is held"]
pub struct CookingGuard(Arc<AtomicUsize>);

impl Cooking {
    /// Count a worker as cooking until the guard drops. Taken on the loop,
    /// before the spawn, so the loop sees the work at once.
    pub fn start(&self) -> CookingGuard {
        self.0.fetch_add(1, Ordering::Relaxed);
        CookingGuard(Arc::clone(&self.0))
    }

    /// Whether any worker is cooking.
    pub fn any(&self) -> bool {
        self.0.load(Ordering::Relaxed) > 0
    }
}

impl Drop for CookingGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// The simmer's frame at `now` for cooking that began at `since`: `None`
/// within the grace, then a frame count from where the grace ran out.
pub fn cooking_frame(since: Instant, now: Instant) -> Option<u64> {
    let simmering = now.checked_duration_since(since + COOKING_GRACE)?;
    Some(u64::try_from(simmering.as_millis() / COOKING_FRAME.as_millis()).unwrap_or(u64::MAX))
}

/// When the frame after `frame` begins, for cooking that began at `since`.
fn next_frame_at(since: Instant, frame: Option<u64>) -> Instant {
    let shown = since + COOKING_GRACE;
    frame.map_or(shown, |frame| {
        let next = u32::try_from(frame.saturating_add(1)).unwrap_or(u32::MAX);
        shown + COOKING_FRAME.saturating_mul(next)
    })
}

impl App {
    /// A guard counting one worker as cooking; move it into the worker.
    pub(super) fn start_cooking(&self) -> CookingGuard {
        self.runtime.cooking.start()
    }

    /// Advance the simmer while anything cooks and stop it when nothing does.
    /// Whether the frame changed.
    pub(super) fn settle_cooking(&mut self, now: Instant, ctx: &mut RunCtx) -> bool {
        if !self.runtime.cooking.any() {
            self.runtime.cooking_since = None;
            ctx.scheduler.disarm(Deadline::Cooking);
            return self.view.cooking_frame.take().is_some();
        }
        let since = *self.runtime.cooking_since.get_or_insert(now);
        let frame = cooking_frame(since, now);
        ctx.scheduler
            .arm(Deadline::Cooking, next_frame_at(since, frame));
        let changed = frame != self.view.cooking_frame;
        self.view.cooking_frame = frame;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guard_counts_until_it_drops() {
        let cooking = Cooking::default();
        assert!(!cooking.any());
        let first = cooking.start();
        let second = cooking.start();
        drop(first);
        assert!(cooking.any(), "one worker is still cooking");
        drop(second);
        assert!(!cooking.any());
    }

    /// A worker that panics still stops cooking: its guard drops in the unwind.
    #[test]
    fn a_panicking_worker_stops_cooking() {
        let cooking = Cooking::default();
        let guard = cooking.start();
        let _ = std::thread::spawn(move || {
            let _guard = guard;
            panic!("the worker failed");
        })
        .join();
        assert!(!cooking.any());
    }

    #[test]
    fn the_pepper_waits_out_the_grace_then_counts_frames() {
        let since = Instant::now();
        assert_eq!(cooking_frame(since, since), None);
        assert_eq!(cooking_frame(since, since + COOKING_GRACE / 2), None);
        assert_eq!(cooking_frame(since, since + COOKING_GRACE), Some(0));
        let later = since + COOKING_GRACE + COOKING_FRAME * 3 + COOKING_FRAME / 2;
        assert_eq!(cooking_frame(since, later), Some(3));
        assert_eq!(next_frame_at(since, None), since + COOKING_GRACE);
        assert_eq!(
            next_frame_at(since, Some(3)),
            since + COOKING_GRACE + COOKING_FRAME * 4
        );
    }

    /// The loop simmers the pepper only while a worker cooks past the grace,
    /// arms a wake only then, and stops both once the work is done.
    #[test]
    fn the_loop_simmers_only_while_a_worker_cooks() {
        let mut app = App::test_app(std::path::PathBuf::from("/tmp/proj"));
        let mut ctx = RunCtx::for_test();
        let start = Instant::now();
        assert!(!app.settle_cooking(start, &mut ctx));
        assert!(ctx.scheduler.next().is_none(), "idle arms nothing");

        let guard = app.start_cooking();
        assert!(!app.settle_cooking(start, &mut ctx), "still in the grace");
        assert_eq!(app.view.cooking_frame, None);
        assert_eq!(ctx.scheduler.next(), Some(start + COOKING_GRACE));

        let shown = start + COOKING_GRACE;
        assert!(app.settle_cooking(shown, &mut ctx));
        assert_eq!(app.view.cooking_frame, Some(0));
        assert_eq!(ctx.scheduler.next(), Some(shown + COOKING_FRAME));
        assert!(app.settle_cooking(shown + COOKING_FRAME, &mut ctx));
        assert_eq!(app.view.cooking_frame, Some(1));

        drop(guard);
        assert!(app.settle_cooking(shown + COOKING_FRAME * 2, &mut ctx));
        assert_eq!(app.view.cooking_frame, None);
        assert!(ctx.scheduler.next().is_none(), "done cooking disarms");
        assert_eq!(app.runtime.cooking_since, None);
    }
}
