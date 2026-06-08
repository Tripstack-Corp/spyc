//! MVU Phase 3b: pane wake identity + closure construction.
//!
//! `SinkId` is a runtime-only, monotonically-allocated pane identifier
//! carried by [`super::Message::PaneOutput`]. In 3b it only *labels* the
//! wake — the loop re-scans every live pane on any `PaneOutput` rather than
//! targeting by id — but it is the shared identity that 3c's
//! `SinkOutput { sink }` (capture/task drains) and the later
//! sink-reassignment work build on, so it is introduced here.
//!
//! The wake closure is built here (in `crate::app`, where `Message` is in
//! scope) and handed to `Pane::adopt` as an opaque [`crate::pane::PaneWake`]
//! (`Arc<dyn Fn()>`). The pane module never names `SinkId` or `Message` —
//! the dependency runs `app → pane` only.

use std::num::NonZeroU64;
use std::sync::Arc;

use crate::pane::PaneWake;

use super::{App, Message};

/// Runtime-only pane identifier. Never serialized (`SavedTab` carries no
/// id; restored tabs respawn with fresh ones). `NonZero` reserves 0 and
/// makes `Option<SinkId>` free.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SinkId(NonZeroU64);

impl App {
    /// Mint a fresh `SinkId`. Monotonic, never reused — a wake for a closed
    /// or demoted pane carries a stale id that simply matches nothing, so
    /// it self-discards (the loop re-scans all live panes regardless).
    const fn alloc_sink_id(&mut self) -> SinkId {
        self.runtime.next_sink_id = self.runtime.next_sink_id.saturating_add(1);
        SinkId(
            NonZeroU64::new(self.runtime.next_sink_id)
                .expect("next_sink_id is >= 1 after increment"),
        )
    }

    /// Mint a `SinkId` and build the pane's wake closure: a cheap
    /// non-blocking send of `Message::PaneOutput { tab }` onto the unified
    /// channel. Before `run()` installs `pane_wake_tx` (the test harness and
    /// any pre-run spawn) this returns a no-op — harmless, since the poll
    /// floor (or, post-PR2, `MAX_IDLE_CAP`) still services such panes.
    pub(crate) fn make_pane_wake(&mut self) -> PaneWake {
        let tab = self.alloc_sink_id();
        match &self.runtime.pane_wake_tx {
            Some(tx) => {
                let tx = tx.clone();
                Arc::new(move || {
                    let _ = tx.send(Message::PaneOutput { tab });
                })
            }
            None => Arc::new(|| {}),
        }
    }

    /// MVU Phase 3c: mint a `SinkId` and build a `Wake` for a main-loop-
    /// drained capture/task. The fire closure sends `Message::SinkOutput`;
    /// the fresh `pending` flag is the edge the `PtyHost` reader CASes and
    /// the main loop clears (`clear_wake_pending`). Install it on the host
    /// via `host.set_wake(...)`. No-op fire before `run()` installs the
    /// sender (the floor / `MAX_IDLE_CAP` still services such hosts).
    pub(crate) fn make_sink_wake(&mut self) -> crate::pane::pty_host::Wake {
        let sink = self.alloc_sink_id();
        let fire: PaneWake = match &self.runtime.pane_wake_tx {
            Some(tx) => {
                let tx = tx.clone();
                Arc::new(move || {
                    let _ = tx.send(Message::SinkOutput { sink });
                })
            }
            None => Arc::new(|| {}),
        };
        crate::pane::pty_host::Wake {
            pending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            fire,
        }
    }

    /// Build a pager-stream worker's wake — a payloadless
    /// `Message::PagerStreamOutput` send (the payload rides the boxed stream's
    /// `rx`, re-drained by `drain_pager_stream`). Wrapped around the worker's
    /// data `Sender` by a `WakingSender`. The single wake for every pager
    /// stream (grep / git-view / transcript). No-op before `run()` installs the
    /// sender.
    pub(crate) fn make_pager_stream_wake(&self) -> PaneWake {
        match &self.runtime.pane_wake_tx {
            Some(tx) => {
                let tx = tx.clone();
                Arc::new(move || {
                    let _ = tx.send(Message::PagerStreamOutput);
                })
            }
            None => Arc::new(|| {}),
        }
    }

    /// MVU Phase 3d: build the F-finder walker's wake — a payloadless
    /// `Message::FindOutput` send (the candidates ride `FindPicker.walk_rx`,
    /// re-drained by `drain_walk`). No-op before `run()` installs the sender.
    pub(crate) fn make_find_wake(&self) -> PaneWake {
        match &self.runtime.pane_wake_tx {
            Some(tx) => {
                let tx = tx.clone();
                Arc::new(move || {
                    let _ = tx.send(Message::FindOutput);
                })
            }
            None => Arc::new(|| {}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sink_ids_are_monotonic_and_distinct() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            let a = app.alloc_sink_id();
            let b = app.alloc_sink_id();
            let c = app.alloc_sink_id();
            assert_ne!(a, b);
            assert_ne!(b, c);
            assert_ne!(a, c);
        });
    }

    #[test]
    fn make_pane_wake_sends_pane_output_when_armed() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            let (tx, rx) = std::sync::mpsc::channel::<Message>();
            app.runtime.pane_wake_tx = Some(tx);
            let wake = app.make_pane_wake();
            wake();
            match rx.try_recv() {
                Ok(Message::PaneOutput { .. }) => {}
                _ => panic!("expected a PaneOutput wake on the channel"),
            }
        });
    }

    #[test]
    fn make_sink_wake_sends_sink_output_when_armed() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            let (tx, rx) = std::sync::mpsc::channel::<Message>();
            app.runtime.pane_wake_tx = Some(tx);
            let wake = app.make_sink_wake();
            (wake.fire)();
            match rx.try_recv() {
                Ok(Message::SinkOutput { .. }) => {}
                _ => panic!("expected a SinkOutput wake on the channel"),
            }
        });
    }

    #[test]
    fn make_pane_wake_is_noop_before_run() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            // pane_wake_tx is None in the test harness — firing must not panic.
            let wake = app.make_pane_wake();
            wake();
        });
    }
}
