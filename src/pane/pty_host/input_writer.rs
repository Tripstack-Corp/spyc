//! Ordered, bounded input delivery; the child never owns the UI thread.

use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

const MAX_BATCHES: usize = 512;
const MAX_BYTES: usize = 8 * 1_048_576;

pub(super) struct InputWriter {
    tx: mpsc::SyncSender<Batch>,
    outstanding: Arc<AtomicUsize>,
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
    pub(super) fn new(mut writer: Box<dyn Write + Send>) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Batch>(MAX_BATCHES);
        // Only this worker owns the OS writer, including its destructor, which
        // portable-pty may implement by writing EOF. Dropping a host never joins
        // a worker waiting for a non-reading child; host teardown kills the child.
        std::thread::spawn(move || {
            while let Ok(batch) = rx.recv() {
                if let Err(error) = writer.write_all(&batch.bytes).and_then(|()| writer.flush()) {
                    crate::spyc_debug!("pty input write failed: {error}");
                    return;
                }
            }
        });
        Self {
            tx,
            outstanding: Arc::new(AtomicUsize::new(0)),
        }
    }

    // Exclusive producer access preserves input ordering even though the
    // channel and byte accounting use interior mutability.
    #[allow(clippy::needless_pass_by_ref_mut)]
    pub(super) fn enqueue(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        // Count the worker's in-flight batch too: a blocked write must not let
        // repeated large pastes grow memory after they leave the channel.
        self.outstanding
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes.len())
                    .filter(|total| *total <= MAX_BYTES)
            })
            .map_err(|_| queue_full())?;
        let batch = Batch {
            bytes: bytes.to_vec(),
            outstanding: Arc::clone(&self.outstanding),
        };
        match self.tx.try_send(batch) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Err(queue_full()),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "child input writer closed",
            )),
        }
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
                self.release.recv().unwrap();
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
            })),
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
            output.recv_timeout(Duration::from_secs(2)).is_err(),
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
        assert!(output.recv_timeout(Duration::from_secs(2)).is_err());
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
        assert!(output.recv_timeout(Duration::from_secs(2)).is_err());
    }
}
