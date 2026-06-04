//! Process I/O machinery for the event loop: the parkable crossterm
//! input-reader thread (`ReaderHandle` / `spawn_input_reader`) and the
//! TUI-teardown foreground-exec runner (`ForegroundExec` /
//! `run_child_in_foreground`). Moved verbatim from `app/mod.rs` (800-LoC
//! campaign).

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};

use crate::spyc_debug;
use crate::{Tui, resume_tui, suspend_tui};

use super::Message;

/// Owns the parkable crossterm input-reader thread (MVU Phase 1). The
/// reader becomes the SOLE caller of `event::poll`/`event::read`. Modeled
/// on `ParserWorker` (src/pane/mod.rs): a stop flag set on `Drop`, then
/// `unpark` + `join`. See `docs/MVU_PLAN.md` Phase 1 for the
/// park/ack/drain handshake the executor below relies on.
pub(super) struct ReaderHandle {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) park: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) acked: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) reader_done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) read_err: std::sync::Arc<std::sync::Mutex<Option<std::io::Error>>>,
    pub(super) handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ReaderHandle {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(h) = self.handle.take() {
            h.thread().unpark(); // in case it's parked
            let _ = h.join();
        }
    }
}

/// Spawn the parkable input reader. It uses a FINITE `event::poll(10ms)`
/// loop — never a bare `event::read()`, which would pin crossterm's
/// process-global reader mutex indefinitely — so a parked reader holds
/// no lock and issues no tty read, leaving a foreground child's stdin
/// uncontended. On park it drains crossterm's buffered events to empty
/// (dropping them) BEFORE acking, so nothing is stranded across the
/// child's tty ownership.
pub(super) fn spawn_input_reader(tx: std::sync::mpsc::Sender<Message>) -> ReaderHandle {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering::{Acquire, Release};

    let stop = Arc::new(AtomicBool::new(false));
    let park = Arc::new(AtomicBool::new(false));
    let acked = Arc::new(AtomicBool::new(false));
    let reader_done = Arc::new(AtomicBool::new(false));
    let read_err = Arc::new(std::sync::Mutex::new(None));

    let handle = {
        let stop = stop.clone();
        let park = park.clone();
        let acked = acked.clone();
        let reader_done = reader_done.clone();
        let read_err = read_err.clone();
        std::thread::Builder::new()
            .name("spyc-input-reader".to_string())
            .spawn(move || {
                // Park-latency bound — NOT the loop cadence. 10ms <= the
                // 16ms typing tier, so input still surfaces within one
                // main-loop tick.
                const READER_POLL: Duration = Duration::from_millis(10);
                loop {
                    if stop.load(Acquire) {
                        reader_done.store(true, Release);
                        return;
                    }
                    if park.load(Acquire) {
                        // Reached only at loop top, i.e. AFTER any poll/read
                        // returned: we hold no crossterm lock and issue no
                        // tty read. Drain buffered events to empty (dropped)
                        // so none is stranded across the child's tty
                        // ownership, then ack and park. Spurious unparks are
                        // safe — the loop top re-checks `park`.
                        while matches!(event::poll(Duration::ZERO), Ok(true)) {
                            if event::read().is_err() {
                                break;
                            }
                        }
                        acked.store(true, Release);
                        std::thread::park();
                        continue;
                    }
                    match event::poll(READER_POLL) {
                        Ok(true) => match event::read() {
                            Ok(ev) => {
                                // Press-filter for Key (verbatim from the
                                // old inline guard); everything else
                                // (Paste/Resize/Focus/Mouse) forwarded.
                                let forward = match &ev {
                                    Event::Key(k) => {
                                        k.kind == KeyEventKind::Press
                                            || k.kind == KeyEventKind::Repeat
                                    }
                                    _ => true,
                                };
                                if forward && tx.send(Message::Input(ev)).is_err() {
                                    reader_done.store(true, Release); // main loop gone
                                    return;
                                }
                            }
                            Err(e) => {
                                *read_err.lock().unwrap() = Some(e);
                                reader_done.store(true, Release);
                                // MVU Phase 3d death-wake: store THEN send, so
                                // the loop-top Acquire-load sees the error. With
                                // no poll floor, this kicks the blocking recv.
                                let _ = tx.send(Message::ReaderExited);
                                return;
                            }
                        },
                        Ok(false) => {} // poll timeout — re-check stop/park
                        Err(e) => {
                            *read_err.lock().unwrap() = Some(e);
                            reader_done.store(true, Release);
                            let _ = tx.send(Message::ReaderExited); // death-wake (see above)
                            return;
                        }
                    }
                }
            })
            .expect("spawn spyc-input-reader thread")
    };

    ReaderHandle {
        stop,
        park,
        acked,
        reader_done,
        read_err,
        handle: Some(handle),
    }
}

/// Parking-aware wrapper around `run_child_in_foreground` (MVU Phase 1).
/// Synchronously parks + acks + drains the input reader BEFORE the child
/// takes the tty, and re-arms it after — so only the child reads stdin
/// during the takeover (no keystroke leakage either direction).
pub(super) struct ForegroundExec {
    pub(super) park: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) acked: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) reader_done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) reader: std::thread::Thread,
}

impl ForegroundExec {
    /// Quiesce the reader: clear any stale ack, request park, then wait
    /// (bounded, ~200 ms) for the ack — at which point the reader has
    /// returned from poll/read AND drained crossterm's buffers, so the
    /// tty is clean for the child. Returns early if the reader has
    /// already exited (`reader_done`). Bounded so a descheduled reader
    /// can't freeze the UI on an editor/pager launch.
    fn park_and_wait(&self) {
        use std::sync::atomic::Ordering::{Acquire, Release};
        self.acked.store(false, Release);
        self.park.store(true, Release);
        let deadline = std::time::Instant::now() + Duration::from_millis(200);
        while !self.acked.load(Acquire) {
            if self.reader_done.load(Acquire) {
                break; // reader exited — provably not reading the tty
            }
            if std::time::Instant::now() >= deadline {
                spyc_debug!("FG park ack timed out; proceeding");
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Re-arm the reader after the child returns (clear park BEFORE
    /// unpark — ordering matters so a spurious wake can't strand it).
    fn unpark_reader(&self) {
        self.park.store(false, std::sync::atomic::Ordering::Release);
        self.reader.unpark();
    }

    pub(super) fn run(
        &self,
        terminal: &mut Tui,
        program: &str,
        args: &[String],
        pause_after: bool,
    ) -> Result<()> {
        self.park_and_wait();
        // The existing takeover, byte-for-byte unchanged.
        let r = run_child_in_foreground(terminal, program, args, pause_after);
        self.unpark_reader();
        r
    }
}

/// Hand the tty to a child process, optionally pausing for a keypress
/// afterwards so the user can read the command's output before we repaint
/// over it.
///
/// Job-control aware: the child is placed in its own process group and
/// becomes the foreground process group of the controlling tty for the
/// duration of the run. This is what a normal shell does when launching
/// a foreground command, and it's what makes Ctrl+C / Ctrl+\ delivery
/// land *only* on the child instead of being broadcast to spyc + child.
/// Without this, less running line-counts would react to ^C *and* spyc
/// would see it (caught by our no-op handler, but the FG-group ambiguity
/// caused other anomalies -- less appearing to miss the signal, etc.).
fn run_child_in_foreground(
    terminal: &mut Tui,
    program: &str,
    args: &[String],
    pause_after: bool,
) -> Result<()> {
    use std::io::Write;
    suspend_tui(terminal)?;

    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    // process_group(0) ⇒ child becomes leader of a new process group
    // (PGID == child PID). Equivalent to setpgid(0, 0) right before
    // exec. The child no longer shares spyc's group, so a tty signal
    // delivered to spyc's FG group can't accidentally hit it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut child = cmd.spawn()?;

    // Make the child's process group the foreground group of the
    // controlling tty. Now ^C / ^\ from the kernel's tty driver go
    // to the child only. SIGTTOU is ignored globally (see
    // `install_signal_handlers`) so the restore call below doesn't
    // suspend us.
    #[cfg(unix)]
    let saved_pgid = {
        use std::os::fd::AsFd;
        let our_pgid = rustix::process::getpgrp();
        if let Some(child_pid) = rustix::process::Pid::from_raw(child.id() as i32) {
            let stdin = std::io::stdin();
            let _ = rustix::termios::tcsetpgrp(stdin.as_fd(), child_pid);
        }
        our_pgid
    };

    // Ignoring status on purpose: non-zero exits (e.g. less with `q`, or a
    // grep that found nothing) are normal and should not crash spyc.
    let _ = child.wait();

    // Restore tty foreground to spyc's group. Without this, the next
    // tty input would still be delivered to the child's (now-dead)
    // group and the kernel would EIO subsequent reads.
    #[cfg(unix)]
    {
        use std::os::fd::AsFd;
        let stdin = std::io::stdin();
        let _ = rustix::termios::tcsetpgrp(stdin.as_fd(), saved_pgid);
    }

    if pause_after {
        let mut stdout = std::io::stdout();
        write!(stdout, "\n[spyc] press any key to continue…")?;
        stdout.flush()?;
        // We're not in raw mode right now, so read a single byte directly
        // from stdin. Any key (including Enter) unblocks.
        let mut byte = [0u8; 1];
        let _ = std::io::Read::read(&mut std::io::stdin(), &mut byte);
    }

    resume_tui(terminal)?;
    Ok(())
}

#[cfg(test)]
mod foreground_exec_tests {
    use super::ForegroundExec;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering::{Acquire, Release};
    use std::time::{Duration, Instant};

    /// The ForegroundExec park handshake acks (proving the reader
    /// quiesced before a child would take the tty) and a second cycle
    /// re-acks (proving unpark resumed the parked reader). Driven by a
    /// stub reader mirroring the real reader's park branch — CI-safe, no
    /// tty / no App::run.
    #[test]
    fn park_handshake_acks_and_resumes() {
        let stop = Arc::new(AtomicBool::new(false));
        let park = Arc::new(AtomicBool::new(false));
        let acked = Arc::new(AtomicBool::new(false));
        let reader_done = Arc::new(AtomicBool::new(false));
        let handle = {
            let (stop, park, acked) = (stop.clone(), park.clone(), acked.clone());
            std::thread::spawn(move || {
                loop {
                    if stop.load(Acquire) {
                        return;
                    }
                    if park.load(Acquire) {
                        acked.store(true, Release);
                        std::thread::park();
                        continue;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        };
        let fe = ForegroundExec {
            park,
            acked: acked.clone(),
            reader_done,
            reader: handle.thread().clone(),
        };

        fe.park_and_wait();
        assert!(acked.load(Acquire), "reader should ack the park");

        fe.unpark_reader();
        fe.park_and_wait();
        assert!(acked.load(Acquire), "reader should re-ack after unpark");

        stop.store(true, Release);
        handle.thread().unpark();
        let _ = handle.join();
    }

    /// `park_and_wait` short-circuits (does not burn the full ~200 ms
    /// deadline) when the reader has already exited, and records no ack.
    #[test]
    fn wait_short_circuits_on_reader_done() {
        let park = Arc::new(AtomicBool::new(false));
        let acked = Arc::new(AtomicBool::new(false));
        let reader_done = Arc::new(AtomicBool::new(true)); // already exited
        let dummy = std::thread::spawn(std::thread::park);
        let fe = ForegroundExec {
            park,
            acked: acked.clone(),
            reader_done,
            reader: dummy.thread().clone(),
        };

        let start = Instant::now();
        fe.park_and_wait();
        assert!(
            start.elapsed() < Duration::from_millis(150),
            "must short-circuit on reader_done, not wait the full deadline"
        );
        assert!(
            !acked.load(Acquire),
            "no ack when the reader is already done"
        );

        dummy.thread().unpark();
        let _ = dummy.join();
    }
}
