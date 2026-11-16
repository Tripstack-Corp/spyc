//! The repaint after a gap (R12c, finding 9). Bytes a jar dropped can't be
//! replayed, so the program is made to draw its screen again: its PTY is
//! given a size a row off for [`REPAINT_HOLD`], then its own size again. A
//! `SIGWINCH` at an unchanged size repaints nothing, and two sizes sent back
//! to back reach a program as one: a Node program on macOS saw the size a
//! row off in none of 40 trials so, idle or busy, and in all 40 held.
//!
//! The hold isn't cut short when output arrives: bytes the program wrote
//! before it handled the signal can reach the jar after the resize, and
//! ending the hold on them sends the second size before the program read
//! the first.

use std::time::{Duration, Instant};

/// How long a repaint holds the size a row off.
pub(super) const REPAINT_HOLD: Duration = Duration::from_millis(100);

/// A pane's repaint: how long it holds, and since when, if it is holding.
#[derive(Debug)]
pub(super) struct Repaint {
    hold: Duration,
    since: Option<Instant>,
}

impl Repaint {
    /// A repaint that holds a size a row off for `hold`.
    pub(super) const fn new(hold: Duration) -> Self {
        Self { hold, since: None }
    }

    /// A gap, with the PTY at `size`: the size to hold the PTY at, unless one
    /// is held already or the PTY has no row or column to spare.
    pub(super) const fn start(
        &mut self,
        (rows, cols): (u16, u16),
        now: Instant,
    ) -> Option<(u16, u16)> {
        if self.since.is_some() {
            return None;
        }
        let off = if rows > 1 {
            (rows - 1, cols)
        } else if cols > 1 {
            (rows, cols - 1)
        } else {
            return None;
        };
        self.since = Some(now);
        Some(off)
    }

    /// How long the hold has left at `now`, while one is held.
    pub(super) fn left(&self, now: Instant) -> Option<Duration> {
        self.since.map(|since| {
            self.hold
                .saturating_sub(now.saturating_duration_since(since))
        })
    }

    /// Whether the hold has run out at `now`; it ends if so.
    pub(super) fn ran_out(&mut self, now: Instant) -> bool {
        let out = self.left(now).is_some_and(|left| left.is_zero());
        if out {
            self.since = None;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A size a row off, held for its time and then given up, once; one hold
    /// at a time, and a later gap holds again.
    #[test]
    fn a_hold_runs_its_time_once() {
        let now = Instant::now();
        let mut r = Repaint::new(REPAINT_HOLD);
        assert_eq!(r.start((24, 80), now), Some((23, 80)));
        assert_eq!(r.start((24, 80), now), None, "one hold at a time");
        assert!(!r.ran_out(now));
        assert!(r.ran_out(now + REPAINT_HOLD));
        assert!(!r.ran_out(now + REPAINT_HOLD), "once");
        assert_eq!(r.left(now), None);
        assert_eq!(
            r.start((24, 80), now),
            Some((23, 80)),
            "a later gap holds again"
        );
    }

    /// One row: a column off. One cell: nothing to spare, no hold.
    #[test]
    fn a_pane_with_no_row_to_spare_holds_a_column_off_or_nothing() {
        let now = Instant::now();
        assert_eq!(
            Repaint::new(REPAINT_HOLD).start((1, 80), now),
            Some((1, 79))
        );
        assert_eq!(Repaint::new(REPAINT_HOLD).start((1, 1), now), None);
    }
}
