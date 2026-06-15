//! Activity-monitor counters for the `A` overlay (draws/sec, bytes/sec, redraw
//! reasons, watcher/MCP/git rates, peak frame/render/echo latencies, proc
//! stats). Grouped out of the loose `activity_*` `ViewState` fields: the live
//! accumulators and their last-second snapshot share one [`ActivityCounters`]
//! shape, so the 1 Hz rollover is a struct compare + copy instead of a dozen
//! hand-maintained parallel field assignments (the bug-prone "add a counter,
//! forget to wire it into all four lists" hazard).
//!
//! The visibility toggle (`show_activity`) stays on `ViewState` — it gates the
//! overlay and the rollover, but isn't part of the counter double-buffer.

use std::time::Instant;

/// Per-second activity counters. The live accumulators and their snapshot are
/// the same shape, so rolling the window is `snap = live; live = default()`.
/// `PartialEq` powers the "did anything change this second?" redraw predicate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActivityCounters {
    /// Frames drawn.
    pub draws: u32,
    /// Cells painted (width × height per frame).
    pub bytes: u64,
    /// Redraws attributed to pane output.
    pub reason_pane: u32,
    /// Redraws attributed to an input/event.
    pub reason_event: u32,
    /// Redraws with any other cause.
    pub reason_other: u32,
    /// `notify` watcher events (one per event, not per path).
    pub watcher_events: u32,
    /// Writable MCP commands executed.
    pub mcp_reqs: u32,
    /// Git-worker results ingested.
    pub git_results: u32,
}

/// Peak microsecond latencies within a second (frame total, render pass,
/// keystroke→echo). Kept apart from [`ActivityCounters`] because a changing
/// peak is a passive stat — it must NOT itself force an overlay redraw, so it's
/// excluded from the counters' change predicate.
#[derive(Clone, Copy, Default)]
// The shared `_us` suffix is the *unit* (microseconds), not redundant naming —
// it disambiguates these raw timings from the millisecond values the overlay
// derives, so keep it rather than take clippy's "remove the postfix" advice.
#[allow(clippy::struct_field_names)]
pub struct ActivityPeaks {
    pub frame_us: u64,
    pub render_us: u64,
    pub echo_us: u64,
}

/// Activity-monitor state: the live/snapshot counter pair, the peak pair, the
/// rollover clock, and the cached proc stats. One field on `ViewState`.
pub struct ActivityMonitor {
    /// Start of the current 1-second window.
    pub last_tick: Instant,
    /// Counters accumulating during the current window.
    pub live: ActivityCounters,
    /// Last completed window's counters — what the overlay renders.
    pub snap: ActivityCounters,
    /// Peaks accumulating during the current window.
    pub peaks_live: ActivityPeaks,
    /// Last completed window's peaks — what the overlay renders.
    pub peaks_snap: ActivityPeaks,
    /// Roundtrip (ms) of the most recent git-worker request.
    pub git_last_ms: u32,
    /// Cached RSS (KiB), refreshed once per 1 s tick.
    pub proc_rss_kb: u64,
    /// Cached thread count, refreshed once per 1 s tick.
    pub proc_threads: u32,
}

impl ActivityMonitor {
    /// Fresh monitor; `last_tick` seeds the first rollover window.
    pub fn new(now: Instant) -> Self {
        Self {
            last_tick: now,
            live: ActivityCounters::default(),
            snap: ActivityCounters::default(),
            peaks_live: ActivityPeaks::default(),
            peaks_snap: ActivityPeaks::default(),
            git_last_ms: 0,
            proc_rss_kb: 0,
            proc_threads: 0,
        }
    }

    /// Roll the 1-second window: snapshot the live counters/peaks and reset the
    /// accumulators, stamping `last_tick = now`. Returns whether any *counter*
    /// changed (peaks excluded), so the caller can decide if an overlay-only
    /// redraw is warranted. The caller owns the clock gate (`last_tick` +
    /// `show_activity`); this just does the swap.
    pub fn roll(&mut self, now: Instant) -> bool {
        let changed = self.live != self.snap;
        self.snap = self.live;
        self.live = ActivityCounters::default();
        self.peaks_snap = self.peaks_live;
        self.peaks_live = ActivityPeaks::default();
        self.last_tick = now;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roll_snapshots_live_and_resets_accumulators() {
        let t0 = Instant::now();
        let mut m = ActivityMonitor::new(t0);
        m.live.draws = 7;
        m.live.bytes = 4096;
        m.peaks_live.frame_us = 250;

        let t1 = t0 + std::time::Duration::from_secs(1);
        let changed = m.roll(t1);

        assert!(changed, "draws went 0 → 7");
        assert_eq!(m.snap.draws, 7, "snapshot captured the live count");
        assert_eq!(m.snap.bytes, 4096);
        assert_eq!(m.peaks_snap.frame_us, 250, "peak snapshotted");
        assert_eq!(m.live, ActivityCounters::default(), "live reset");
        assert_eq!(m.peaks_live.frame_us, 0, "peak reset");
        assert_eq!(m.last_tick, t1, "clock stamped");
    }

    #[test]
    fn roll_reports_no_change_when_counters_match_snapshot() {
        let t0 = Instant::now();
        let mut m = ActivityMonitor::new(t0);
        // First roll with zero activity: live == snap (both default) → no change.
        assert!(!m.roll(t0), "idle window signals no change");
        // A peak-only change must NOT count as a counter change.
        m.peaks_live.render_us = 999;
        assert!(!m.roll(t0), "peak-only change is not a counter change");
    }
}
