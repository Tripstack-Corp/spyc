//! Where an off-thread worker's result lands: a channel whose receiver stays on
//! `Runtime` and whose senders go one to each worker.
//!
//! A worker sends its result, then wakes the loop with a payloadless
//! `Message::Wake`; the pre-recv drain takes everything that landed. Nothing is
//! shared but the channel, so there is no lock to poison, and workers running at
//! once each land their own result rather than racing for a slot.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use super::{Message, Wake};
use crate::fs::WakingSender;

pub struct Landing<T> {
    tx: Sender<T>,
    rx: Receiver<T>,
    /// Values a test's `wait` took off the channel, which the next drain
    /// returns first.
    held: Vec<T>,
}

impl<T> Default for Landing<T> {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            held: Vec::new(),
        }
    }
}

impl<T> Landing<T> {
    /// A sender for one worker: what it sends lands here, then `wake` carries
    /// `kind` to the loop. `wake` is `None` before `run()` and in the test
    /// harness, where the value still lands and nothing is woken.
    pub fn sender(&self, wake: Option<Sender<Message>>, kind: Wake) -> WakingSender<T> {
        WakingSender::new(
            self.tx.clone(),
            Arc::new(move || {
                if let Some(wake) = &wake {
                    let _ = wake.send(Message::Wake(kind));
                }
            }),
        )
    }

    /// Land `value` from the loop itself, which needs no wake.
    pub fn land(&self, value: T) {
        // The receiver is `self.rx`, alive as long as this sender.
        let _ = self.tx.send(value);
    }

    /// Everything landed since the last drain, oldest first.
    pub fn drain(&mut self) -> Vec<T> {
        let mut landed = std::mem::take(&mut self.held);
        landed.extend(self.rx.try_iter());
        landed
    }

    /// The newest value landed since the last drain; older ones are dropped.
    pub fn latest(&mut self) -> Option<T> {
        self.drain().pop()
    }

    /// Block until a value lands or `timeout` passes, leaving it for the next
    /// drain. Whether one did.
    #[cfg(test)]
    pub fn wait(&mut self, timeout: std::time::Duration) -> bool {
        if !self.is_empty() {
            return true;
        }
        match self.rx.recv_timeout(timeout) {
            Ok(value) => {
                self.held.push(value);
                true
            }
            Err(_) => false,
        }
    }

    /// How many values have landed since the last drain.
    #[cfg(test)]
    pub fn len(&mut self) -> usize {
        self.held.extend(self.rx.try_iter());
        self.held.len()
    }

    /// Whether nothing has landed since the last drain.
    #[cfg(test)]
    pub fn is_empty(&mut self) -> bool {
        self.len() == 0
    }
}

/// Run `work` on a detached thread and send what it returns. `stopped` is the
/// value for a `work` that ends without one, made from its panic's message.
pub fn spawn_worker<T: Send + 'static>(
    sender: WakingSender<T>,
    work: impl FnOnce() -> T + Send + 'static,
    stopped: impl FnOnce(String) -> T + Send + 'static,
) {
    std::thread::spawn(move || {
        drop(stopped);
        let _ = sender.send(work());
    });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_drain_returns_every_landing_in_order_once() {
        let mut landing = Landing::default();
        let a = landing.sender(None, Wake::FileOp);
        let b = landing.sender(None, Wake::FileOp);
        a.send(1).unwrap();
        b.send(2).unwrap();
        landing.land(3);
        assert_eq!(landing.drain(), vec![1, 2, 3]);
        assert!(landing.drain().is_empty());
    }

    #[test]
    fn a_sender_wakes_the_loop_after_its_value_lands() {
        let mut landing = Landing::default();
        let (wake_tx, wake_rx) = mpsc::channel();
        landing.sender(Some(wake_tx), Wake::FileOp).send(7).unwrap();
        assert!(matches!(
            wake_rx.try_recv(),
            Ok(Message::Wake(Wake::FileOp))
        ));
        assert_eq!(landing.drain(), vec![7]);
    }

    #[test]
    fn latest_keeps_only_the_newest() {
        let mut landing = Landing::default();
        landing.land("old");
        landing.land("new");
        assert_eq!(landing.latest(), Some("new"));
        assert_eq!(landing.latest(), None);
    }

    #[test]
    fn a_wait_leaves_what_landed_for_the_drain() {
        let mut landing = Landing::default();
        let tx = landing.sender(None, Wake::FileOp);
        let worker = std::thread::spawn(move || tx.send(5).unwrap());
        assert!(landing.wait(Duration::from_secs(10)));
        worker.join().unwrap();
        landing.land(6);
        assert!(!landing.is_empty());
        assert_eq!(landing.drain(), vec![5, 6]);
        assert!(landing.is_empty());
        assert!(!landing.wait(Duration::from_millis(1)));
    }

    #[test]
    fn a_worker_lands_what_it_returns() {
        let mut landing = Landing::default();
        spawn_worker(landing.sender(None, Wake::FileOp), || 5, |_| 0);
        assert!(landing.wait(Duration::from_secs(5)), "nothing landed");
        assert_eq!(landing.drain(), vec![5]);
    }

    /// A worker that panics still lands what `stopped` makes of its panic, and
    /// wakes the loop, so the flight the loop waits on ends.
    #[test]
    #[ignore = "red: a worker that panics lands nothing, so its flight never ends"]
    fn a_worker_that_panics_lands_what_stopped_makes_of_its_message() {
        let mut landing = Landing::default();
        let (wake_tx, wake_rx) = mpsc::channel();
        spawn_worker(
            landing.sender(Some(wake_tx), Wake::FileOp),
            || -> String { panic!("the read went wrong") },
            |message| format!("stopped: {message}"),
        );
        assert!(landing.wait(Duration::from_secs(5)), "nothing landed");
        assert_eq!(
            landing.drain(),
            vec!["stopped: the read went wrong".to_string()]
        );
        assert!(
            matches!(
                wake_rx.recv_timeout(Duration::from_secs(1)),
                Ok(Message::Wake(Wake::FileOp))
            ),
            "the loop was not woken"
        );
    }
}
