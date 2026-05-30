//! Back/forward stack of recently-closed pager views, for
//! `:bprev`/`:bnext` (and the `[b`/`]b` pager chord). Works like a
//! browser history stack.
//!
//! Extracted verbatim from `app/mod.rs` as the first step of the
//! `REFACTOR_PLAN.md` Phase 1 decomposition — no behavior change. The
//! one addition is `pop_back()`, replacing two inline
//! `pager_history.back.pop()` accesses so the field can stay private.

use crate::ui::pager::PagerView;

const MAX_PAGER_HISTORY: usize = 10;

pub struct PagerHistory {
    back: Vec<PagerView>,
    forward: Vec<PagerView>,
}

impl PagerHistory {
    pub const fn new() -> Self {
        Self {
            back: Vec::new(),
            forward: Vec::new(),
        }
    }

    /// Save a closed pager view. Skips views flagged `no_history`
    /// (e.g. the help overlay) so accidentally hitting `[b` doesn't
    /// surface stale chrome. Clears the forward stack.
    pub fn push(&mut self, view: PagerView) {
        if view.no_history {
            return;
        }
        self.back.push(view);
        self.forward.clear();
        if self.back.len() > MAX_PAGER_HISTORY {
            self.back.remove(0);
        }
    }

    /// Pop the most-recent back entry directly (no forward-stack dance)
    /// — for callers that already own the current view and just want
    /// the previous buffer. Was an inline `pager_history.back.pop()`
    /// before the extraction.
    pub fn pop_back(&mut self) -> Option<PagerView> {
        self.back.pop()
    }

    /// Go back. On success returns the prior view and tucks `current`
    /// onto the forward stack. On failure (back stack empty) hands
    /// `current` back unchanged so the caller can keep it on screen --
    /// hitting `[b` at the start of history shouldn't close the pager.
    /// PagerView is ~232B so clippy flags the Err variant size; the
    /// alternative (Box on Err only) buys nothing on an in-process,
    /// cold-path call.
    #[allow(clippy::result_large_err)]
    pub fn go_back(&mut self, current: PagerView) -> Result<PagerView, PagerView> {
        match self.back.pop() {
            Some(prev) => {
                self.forward.push(current);
                Ok(prev)
            }
            None => Err(current),
        }
    }

    /// Go forward. Same edge semantics as `go_back`.
    #[allow(clippy::result_large_err)]
    pub fn go_forward(&mut self, current: PagerView) -> Result<PagerView, PagerView> {
        match self.forward.pop() {
            Some(next) => {
                self.back.push(current);
                Ok(next)
            }
            None => Err(current),
        }
    }

    pub const fn back_len(&self) -> usize {
        self.back.len()
    }

    pub const fn forward_len(&self) -> usize {
        self.forward.len()
    }
}
