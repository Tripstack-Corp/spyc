//! MVU Phase 3d: an mpsc `Sender` wrapper that fires a wake closure after
//! every send, so a worker thread streaming onto a per-source channel also
//! wakes the unified `App::run` event loop (which then re-drains that
//! channel). Used for the finder walker and the pager-stream workers (grep /
//! git-view / transcript) once the poll floor is gone.
//!
//! Holds only an `Arc<dyn Fn() + Send + Sync>` — never `app::Message` — so
//! `crate::fs` gains no dependency on the app layer. The app builds the wake
//! closure (sending `Message::FindOutput` / `Message::PagerStreamOutput`) and
//! hands it in at spawn; in tests / before `run()` it's `Arc::new(|| {})`.

use std::sync::Arc;
use std::sync::mpsc::{SendError, Sender};

pub struct WakingSender<T> {
    inner: Sender<T>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl<T> WakingSender<T> {
    pub fn new(inner: Sender<T>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { inner, wake }
    }

    /// Send `value`, then fire the wake **after** the send so the woken
    /// consumer observes the data already in the channel. The wake fires
    /// even on a send error (receiver gone) — harmless: the loop wakes,
    /// re-drains, finds the source torn down, and no-ops.
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        let r = self.inner.send(value);
        (self.wake)();
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn fires_wake_after_each_send_and_delivers_in_order() {
        let (tx, rx) = std::sync::mpsc::channel::<u8>();
        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);
        let ws = WakingSender::new(
            tx,
            Arc::new(move || {
                c.fetch_add(1, Ordering::Release);
            }),
        );
        assert!(ws.send(1).is_ok());
        assert!(ws.send(2).is_ok());
        assert_eq!(count.load(Ordering::Acquire), 2, "one wake per send");
        assert_eq!(rx.recv().unwrap(), 1);
        assert_eq!(rx.recv().unwrap(), 2);
    }

    #[test]
    fn fires_wake_even_when_receiver_gone() {
        let (tx, rx) = std::sync::mpsc::channel::<u8>();
        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);
        let ws = WakingSender::new(
            tx,
            Arc::new(move || {
                c.fetch_add(1, Ordering::Release);
            }),
        );
        drop(rx);
        assert!(ws.send(1).is_err(), "send fails when receiver dropped");
        // The wake still fires — the loop wakes, re-drains, finds the source
        // torn down, and no-ops. (Harmless; keeps the impl branchless.)
        assert_eq!(count.load(Ordering::Acquire), 1);
    }
}
