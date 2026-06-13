//! Unit tests split out of `pane/mod.rs` (byte-debug preview, the parser
//! worker's stop/reclaim/Drop contract, and the lost-wakeup-safe wake
//! protocol). Relocated verbatim; `super::` references became `super::super::`
//! one level deeper (these submodules now sit under `pane::tests`, so `super`
//! is `tests` and `super::super` is `pane`).

#[cfg(test)]
mod preview_tests {
    use super::super::preview_bytes;

    #[test]
    fn preview_renders_printable_and_controls() {
        assert_eq!(preview_bytes(b"hi"), "\"hi\"");
        assert_eq!(preview_bytes(b"\r"), "\"^M\"");
        assert_eq!(preview_bytes(b"\x01"), "\"^A\""); // ^a as a byte
        assert_eq!(preview_bytes(b"\x1b[A"), "\"^[[A\""); // ESC seq
        assert_eq!(preview_bytes(b"a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    #[test]
    fn preview_truncates_long_buffers() {
        let buf = vec![b'x'; 40];
        let s = preview_bytes(&buf);
        assert!(s.contains("xxx"));
        assert!(s.ends_with("+8"), "expected `+8` truncation suffix: {s}");
    }
}

#[cfg(test)]
mod worker_tests {
    use super::super::ParserWorker;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// `stop_and_reclaim_rx` signals the worker, joins it, hands back
    /// the receiver, and is idempotent — a second call returns `None`.
    /// The subsequent `Drop` must then be a no-op (no double-join).
    #[test]
    fn reclaim_stops_joins_and_is_idempotent() {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_cl = Arc::clone(&stop);
        let (_tx, rx) = std::sync::mpsc::channel::<super::super::PtyEvent>();
        let (home_tx, home_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            while !stop_cl.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            // Mimic the real worker's RxReturn guard: ship the receiver
            // home on exit rather than returning it through `join`.
            let _ = home_tx.send(rx);
        });
        let mut worker = ParserWorker {
            stop: Arc::clone(&stop),
            handle: Some(handle),
            rx_home_rx: home_rx,
        };
        assert!(worker.stop_and_reclaim_rx().is_some());
        assert!(stop.load(Ordering::Acquire), "stop flag must be set");
        assert!(
            worker.stop_and_reclaim_rx().is_none(),
            "second reclaim is a no-op"
        );
        drop(worker); // handle already taken → Drop joins nothing
    }

    /// Dropping a live worker without reclaiming must still stop and
    /// join it (the tab-close / app-exit path).
    #[test]
    fn drop_stops_and_joins() {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_cl = Arc::clone(&stop);
        let (_home_tx, home_rx) =
            std::sync::mpsc::channel::<std::sync::mpsc::Receiver<super::super::PtyEvent>>();
        let handle = std::thread::spawn(move || {
            while !stop_cl.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        });
        let worker = ParserWorker {
            stop: Arc::clone(&stop),
            handle: Some(handle),
            rx_home_rx: home_rx,
        };
        drop(worker);
        assert!(stop.load(Ordering::Acquire), "Drop must set the stop flag");
    }
}

#[cfg(test)]
mod wake_tests {
    //! MVU Phase 3b: the parser worker's lost-wakeup-safe wake protocol.
    use super::super::{PtyEvent, RxReturn, Wake, parser_worker};
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn wait_until(label: &str, mut cond: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if cond() {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("timed out waiting for: {label}");
    }

    /// Spawn a worker fed by the returned `tx`; hand back the shared
    /// generation counter, the consumer's `wake_pending` flag, a wake-fire
    /// counter, and the join handle.
    #[allow(clippy::type_complexity)]
    fn spawn_worker(
        stop: &Arc<AtomicBool>,
    ) -> (
        std::sync::mpsc::Sender<PtyEvent>,
        Arc<AtomicU64>,
        Arc<AtomicBool>,
        Arc<AtomicUsize>,
        std::thread::JoinHandle<()>,
    ) {
        let (tx, rx) = std::sync::mpsc::channel::<PtyEvent>();
        let parser = Arc::new(Mutex::new(vt100::Parser::new(24, 80, 100)));
        let gen_ctr = Arc::new(AtomicU64::new(0));
        let pending = Arc::new(AtomicBool::new(false));
        let count = Arc::new(AtomicUsize::new(0));
        let count_cl = Arc::clone(&count);
        let wake = Wake {
            pending: Arc::clone(&pending),
            fire: Arc::new(move || {
                count_cl.fetch_add(1, Ordering::Release);
            }),
        };
        let gen_cl = Arc::clone(&gen_ctr);
        let stop_cl = Arc::clone(stop);
        // These wake tests only observe the wake counter + generation, not
        // the reclaimed receiver, so the rx-return end is discarded.
        let (rx_home_tx, _rx_home_rx) = std::sync::mpsc::channel();
        let guard = RxReturn {
            rx: Some(rx),
            home: rx_home_tx,
        };
        let handle = std::thread::spawn(move || {
            parser_worker(guard, stop_cl, parser, gen_cl, (24, 80), false, wake);
        });
        (tx, gen_ctr, pending, count, handle)
    }

    /// The worker wakes only on the `wake_pending` 0→1 edge: a chunk
    /// arriving while the flag is still set (consumer hasn't cleared)
    /// coalesces silently; clearing re-arms the edge for the next chunk.
    #[test]
    fn wakes_once_per_edge_and_recoalesces() {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, gen_ctr, pending, count, handle) = spawn_worker(&stop);

        tx.send(PtyEvent::Bytes(b"a".to_vec())).unwrap();
        wait_until("first edge wakes", || count.load(Ordering::Acquire) == 1);

        // Second chunk, consumer has NOT cleared → CAS fails, no new wake.
        tx.send(PtyEvent::Bytes(b"b".to_vec())).unwrap();
        wait_until("second chunk parsed", || {
            gen_ctr.load(Ordering::Acquire) >= 2
        });
        assert_eq!(
            count.load(Ordering::Acquire),
            1,
            "coalesced while wake_pending still set"
        );

        // Consumer clears (clear-before-read) → the next chunk re-arms.
        pending.store(false, Ordering::Release);
        tx.send(PtyEvent::Bytes(b"c".to_vec())).unwrap();
        wait_until("re-armed after clear", || {
            count.load(Ordering::Acquire) == 2
        });

        stop.store(true, Ordering::Release);
        drop(tx);
        let _ = handle.join();
    }

    /// A natural EOF (stop unset) fires exactly one final wake, so the loop
    /// runs `drain_output`, sees `closed`, and renders `[exited]` within one
    /// wakeup once the poll floor is gone.
    #[test]
    fn natural_eof_fires_one_final_wake() {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, _gen, _pending, count, handle) = spawn_worker(&stop);
        tx.send(PtyEvent::Closed).unwrap();
        let _ = handle.join(); // worker returns on Closed
        assert_eq!(
            count.load(Ordering::Acquire),
            1,
            "natural EOF must fire exactly one final wake"
        );
    }

    /// A deliberate stop (close / demote / restart sets `stop` before the
    /// Closed lands) suppresses the final wake — it must NOT target a pane
    /// the app just removed.
    #[test]
    fn deliberate_stop_suppresses_final_wake() {
        let stop = Arc::new(AtomicBool::new(true));
        let (tx, _gen, _pending, count, handle) = spawn_worker(&stop);
        tx.send(PtyEvent::Closed).unwrap();
        let _ = handle.join();
        assert_eq!(
            count.load(Ordering::Acquire),
            0,
            "deliberate stop must suppress the close wake"
        );
    }

    /// The wake closure is caller-supplied; if it ever panics the worker
    /// thread unwinds. The byte receiver must STILL come home via the
    /// `RxReturn` guard — otherwise `take_host` hands back a receiver-less
    /// host (silent dead task) and a later `adopt` panics on the missing rx.
    #[test]
    fn rx_returns_home_even_when_wake_closure_panics() {
        let (tx, rx) = std::sync::mpsc::channel::<PtyEvent>();
        let (home_tx, home_rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let parser = Arc::new(Mutex::new(vt100::Parser::new(24, 80, 100)));
        let gen_ctr = Arc::new(AtomicU64::new(0));
        let wake = Wake {
            pending: Arc::new(AtomicBool::new(false)),
            fire: Arc::new(|| panic!("wake closure blew up")),
        };
        let guard = RxReturn {
            rx: Some(rx),
            home: home_tx,
        };
        let handle = std::thread::spawn(move || {
            parser_worker(guard, stop, parser, gen_ctr, (24, 80), false, wake);
        });
        // First Bytes chunk parses, then fires the 0→1 wake edge → panic.
        tx.send(PtyEvent::Bytes(b"x".to_vec())).unwrap();
        assert!(
            handle.join().is_err(),
            "the panicking wake closure should unwind the worker"
        );
        // Despite the unwind, the guard shipped the receiver home.
        assert!(
            home_rx.recv_timeout(Duration::from_secs(2)).is_ok(),
            "byte receiver must survive a worker panic"
        );
    }
}
