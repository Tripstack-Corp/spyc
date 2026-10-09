//! Ordered, bounded input delivery; the child never owns the UI thread.

use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

const MAX_BATCHES: usize = 512;
pub const MAX_BYTES: usize = 8 * 1_048_576;

pub(super) struct InputWriter {
    tx: mpsc::SyncSender<Batch>,
    outstanding: Arc<AtomicUsize>,
}

/// The terminal's answers to the child's queries, queued behind the same limit
/// as input. A second producer: its batches interleave with input whole.
pub struct ReplyWriter {
    tx: mpsc::SyncSender<Batch>,
    outstanding: Arc<AtomicUsize>,
}

impl ReplyWriter {
    pub fn send(&self, bytes: Vec<u8>) -> io::Result<()> {
        send(&self.tx, &self.outstanding, bytes, false)
    }
}

struct Batch {
    bytes: Vec<u8>,
    outstanding: Arc<AtomicUsize>,
}

impl Drop for Batch {
    fn drop(&mut self) {
        self.outstanding
            .fetch_sub(self.bytes.len(), Ordering::AcqRel);
    }
}

impl InputWriter {
    pub(super) fn new(mut writer: Box<dyn Write + Send>) -> io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Batch>(MAX_BATCHES);
        // Only this worker owns the OS writer, including its destructor, which
        // portable-pty may implement by writing EOF. Dropping a host never joins
        // a worker waiting for a non-reading child. Descendants in other process
        // groups can retain the slave after host teardown; see the architecture.
        std::thread::Builder::new()
            .name("pty-input".into())
            .spawn(move || {
                while let Ok(batch) = rx.recv() {
                    if let Err(error) = writer.write_all(&batch.bytes).and_then(|()| writer.flush())
                    {
                        crate::spyc_debug!("pty input write failed: {error}");
                        return;
                    }
                }
            })?;
        Ok(Self {
            tx,
            outstanding: Arc::new(AtomicUsize::new(0)),
        })
    }

    // SPYC-TRAP(pty-input-never-waits): enqueue and drop must not wait for the child.
    // Exclusive producer access preserves input ordering.
    pub(super) fn enqueue(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "input exceeds the 8 MiB pane input limit; use file piping with confirmation",
            ));
        }
        self.enqueue_owned(bytes.to_vec(), false)
    }

    pub(super) fn reply_writer(&self) -> ReplyWriter {
        ReplyWriter {
            tx: self.tx.clone(),
            outstanding: Arc::clone(&self.outstanding),
        }
    }

    /// A confirmed file pipe may exceed the ordinary limit, only while empty.
    /// Move its existing allocation; never copy another large payload into the queue.
    pub(super) fn enqueue_confirmed_pipe(&mut self, bytes: Vec<u8>) -> io::Result<()> {
        self.enqueue_owned(bytes, true)
    }

    #[allow(clippy::needless_pass_by_ref_mut)]
    fn enqueue_owned(&mut self, bytes: Vec<u8>, confirmed: bool) -> io::Result<()> {
        send(&self.tx, &self.outstanding, bytes, confirmed)
    }
}

fn send(
    tx: &mpsc::SyncSender<Batch>,
    outstanding: &Arc<AtomicUsize>,
    bytes: Vec<u8>,
    confirmed: bool,
) -> io::Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    // Include the in-flight batch. A confirmed oversized pipe must be the
    // only outstanding batch; it cannot accumulate behind a stuck child.
    outstanding
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
            if confirmed && bytes.len() > MAX_BYTES {
                (used == 0).then_some(bytes.len())
            } else {
                used.checked_add(bytes.len())
                    .filter(|total| *total <= MAX_BYTES)
            }
        })
        .map_err(|_| queue_full())?;
    let batch = Batch {
        bytes,
        outstanding: Arc::clone(outstanding),
    };
    match tx.try_send(batch) {
        Ok(()) => Ok(()),
        Err(mpsc::TrySendError::Full(_)) => Err(queue_full()),
        Err(mpsc::TrySendError::Disconnected(_)) => Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "child input writer closed",
        )),
    }
}

#[cfg(test)]
impl ReplyWriter {
    /// A reply writer whose writes arrive on the returned channel.
    pub fn capture() -> (Self, mpsc::Receiver<Vec<u8>>) {
        struct Capture(mpsc::Sender<Vec<u8>>);
        impl Write for Capture {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                let _ = self.0.send(bytes.to_vec());
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let (tx, rx) = mpsc::channel();
        let input = InputWriter::new(Box::new(Capture(tx))).expect("spawn the pty-input thread");
        (input.reply_writer(), rx)
    }
}

fn queue_full() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "child input queue full; input was not sent",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    struct HeldWriter {
        entered: Option<mpsc::Sender<()>>,
        release: mpsc::Receiver<()>,
        written: mpsc::Sender<Vec<u8>>,
    }

    impl Write for HeldWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if let Some(entered) = self.entered.take() {
                entered.send(()).unwrap();
                self.release
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::TimedOut, "test writer never released")
                    })?;
            }
            self.written.send(bytes.to_vec()).unwrap();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn held_writer() -> (
        InputWriter,
        mpsc::Receiver<()>,
        mpsc::Sender<()>,
        mpsc::Receiver<Vec<u8>>,
    ) {
        let (entered, started) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let (written, output) = mpsc::channel();
        (
            InputWriter::new(Box::new(HeldWriter {
                entered: Some(entered),
                release: resume,
                written,
            }))
            .unwrap(),
            started,
            release,
            output,
        )
    }

    #[test]
    fn blocked_child_preserves_order_without_blocking_input_or_teardown() {
        let (mut input, started, release, output) = held_writer();
        input.enqueue(b"first").unwrap();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        let start = Instant::now();
        input.enqueue(b"second").unwrap();
        input.enqueue(b"third").unwrap();
        drop(input);
        let elapsed = start.elapsed();
        release.send(()).unwrap();
        assert!(
            elapsed < Duration::from_millis(200),
            "input/teardown waited for the child: {elapsed:?}"
        );
        for expected in [b"first".as_slice(), b"second", b"third"] {
            assert_eq!(
                output.recv_timeout(Duration::from_secs(2)).unwrap(),
                expected
            );
        }
        assert!(
            matches!(
                output.recv_timeout(Duration::from_secs(2)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ),
            "writer outlived its input queue"
        );
    }

    #[test]
    fn in_flight_bytes_count_toward_the_limit_and_rejection_is_atomic() {
        let (mut input, started, release, output) = held_writer();
        let payload = vec![b'x'; MAX_BYTES];
        input.enqueue(&payload).unwrap();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        let error = input.enqueue(b"rejected").unwrap_err();
        release.send(()).unwrap();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert_eq!(
            output.recv_timeout(Duration::from_secs(2)).unwrap(),
            payload
        );
        drop(input);
        assert!(matches!(
            output.recv_timeout(Duration::from_secs(2)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn many_small_inputs_cannot_grow_the_queue_without_limit() {
        let (mut input, started, release, output) = held_writer();
        input.enqueue(b"first").unwrap();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 0..MAX_BATCHES {
            input.enqueue(b"x").unwrap();
        }
        let error = input.enqueue(b"overflow").unwrap_err();
        release.send(()).unwrap();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        drop(input);
        assert_eq!(
            output.recv_timeout(Duration::from_secs(2)).unwrap(),
            b"first"
        );
        for _ in 0..MAX_BATCHES {
            assert_eq!(output.recv_timeout(Duration::from_secs(2)).unwrap(), b"x");
        }
        assert!(matches!(
            output.recv_timeout(Duration::from_secs(2)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }
    #[test]
    fn oversized_input_is_not_reported_as_a_retryable_full_queue() {
        let (mut input, _started, _release, output) = held_writer();
        let error = input.enqueue(&vec![b'x'; MAX_BYTES + 1]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(
            error
                .to_string()
                .contains("input exceeds the 8 MiB pane input limit")
        );
        drop(input);
        assert!(matches!(
            output.recv_timeout(Duration::from_secs(2)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn a_confirmed_large_pipe_is_exclusive_and_does_not_bypass_backpressure() {
        let (mut input, started, release, output) = held_writer();
        let payload = vec![b'x'; MAX_BYTES + 1];
        input.enqueue_confirmed_pipe(payload.clone()).unwrap();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            input.enqueue(b"later key").unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            input
                .enqueue_confirmed_pipe(payload.clone())
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(input);
        release.send(()).unwrap();
        assert_eq!(
            output.recv_timeout(Duration::from_secs(2)).unwrap(),
            payload
        );
        assert!(matches!(
            output.recv_timeout(Duration::from_secs(2)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn a_large_pipe_cannot_follow_already_queued_input() {
        let (mut input, started, release, output) = held_writer();
        input.enqueue(b"first").unwrap();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            input
                .enqueue_confirmed_pipe(vec![b'x'; MAX_BYTES + 1])
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(input);
        release.send(()).unwrap();
        assert_eq!(
            output.recv_timeout(Duration::from_secs(2)).unwrap(),
            b"first"
        );
        assert!(matches!(
            output.recv_timeout(Duration::from_secs(2)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }
}
