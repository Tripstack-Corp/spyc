//! `F` project-wide fuzzy filename picker. The walk runs in a worker
//! thread streaming batches of paths through `walk_rx`; the picker is
//! interactive immediately and the candidate list grows live as the
//! walker progresses. Re-rank runs on every keystroke and on every
//! fresh batch arrival (cheap: ~1us per candidate).
//!
//! Extracted verbatim from `app/mod.rs` (REFACTOR_PLAN Phase 1). Fields
//! are `pub` because the picker is built via a struct literal and its
//! state is read back in the picker key/render handlers in `app`.

use std::path::PathBuf;

pub struct FindPicker {
    /// Repo-relative paths accumulated from the walk so far.
    /// Append-only during the walk; never modified by the user.
    pub candidates: Vec<PathBuf>,
    /// Absolute root the walk started from. Used to construct the
    /// final absolute path on Enter.
    pub root: PathBuf,
    /// User's current input.
    pub query: String,
    /// Current ranked subset (paths only; scores discarded after
    /// sort). Re-built on keystroke or new-batch arrival.
    pub filtered: Vec<PathBuf>,
    /// Index into `filtered`. 0 when query just changed; arrows
    /// move it within `[0, filtered.len())`.
    pub selected: usize,
    /// Cap on rendered results so a 100K-file repo doesn't blow up
    /// the pager Line vec on first paint.
    pub limit: usize,
    /// Receiver for streaming candidate batches from the walker
    /// thread. Set to `None` once the walk completes (channel
    /// disconnects when the worker drops its sender).
    pub walk_rx: Option<std::sync::mpsc::Receiver<Vec<PathBuf>>>,
    /// True once the walker thread has finished. Drives the title
    /// suffix ("scanning..." vs final count).
    pub walk_complete: bool,
}

impl FindPicker {
    /// Re-rank `candidates` against the current `query`, store in
    /// `filtered`, reset `selected` to 0.
    pub fn refilter(&mut self) {
        self.filtered = crate::fs::finder::rank(&self.candidates, &self.query, self.limit)
            .into_iter()
            .map(|(p, _score)| p)
            .collect();
        self.selected = 0;
    }

    /// Drain any batches that have arrived since the last tick.
    /// Returns true when new candidates were appended OR when the
    /// walk completed (caller should re-render either way: title
    /// changes from "scanning..." to a final count).
    pub fn drain_walk(&mut self) -> bool {
        let Some(rx) = self.walk_rx.as_ref() else {
            return false;
        };
        let mut got_any = false;
        loop {
            match rx.try_recv() {
                Ok(batch) => {
                    self.candidates.extend(batch);
                    got_any = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.walk_rx = None;
                    self.walk_complete = true;
                    got_any = true;
                    break;
                }
            }
        }
        got_any
    }
}
