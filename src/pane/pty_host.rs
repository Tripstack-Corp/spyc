//! Pty subprocess host shared by `Pane` and `BackgroundTask`.
//!
//! Both wrap the same kernel: spawn a pty, run a reader thread that
//! pumps bytes into a channel, expose a writer for the user side, hold
//! the master so we can resize and the child so we can reap. Pre-v1.5
//! that machinery was duplicated across `pane::Pane::spawn_with_env`
//! and `app::spawn_capture`; v1.5 Phase 6 hoists it here so both
//! consumers reduce to a thin wrapper plus their own state (vt100
//! parser for `Pane`, flat byte buffer + lifecycle metadata for
//! `BackgroundTask`).
//!
//! The shared kernel also unblocks Phase 6b/6c: with both consumers
//! holding a `PtyHost`, promoting a backgrounded task to a pane (or
//! demoting in reverse) becomes a state shift — same pty handles,
//! different shell around them. Pre-v1.5 that wasn't possible because
//! `spawn_capture` discarded the master after extracting reader/writer
//! (so a backgrounded task couldn't be resized → couldn't act as a
//! pane).
//!
//! Strict rule for Phase 6a: this module changes no observable
//! behavior. The reader-thread protocol, debug-byte-dump,
//! exit-status harvesting, and shutdown semantics all match the
//! pre-refactor `Pane`/`BackgroundTask` paths exactly.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::PaneWake;

/// MVU Phase 3b/3c: the wake half of a pty consumer — a lost-wakeup-safe
/// edge flag plus the closure fired on its 0→1 transition. Used by Pane's
/// parser worker (3b, via `adopt`) AND, in 3c, by the `PtyHost` reader
/// thread for main-loop-drained captures/tasks (installed at runtime via
/// [`PtyHost::set_wake`], so a `take_host`-reclaimed reader can be
/// re-targeted). `fire` is type-erased (`Arc<dyn Fn()>`), so no `Message`
/// leaks into the kernel — the `app → pane` layering stays one-directional.
#[derive(Clone)]
pub struct Wake {
    pub pending: Arc<AtomicBool>,
    pub fire: PaneWake,
}

/// Reader-thread → main-loop event. One message per chunk of pty
/// bytes, then exactly one `Closed` when the master sees EOF.
pub enum PtyEvent {
    Bytes(Vec<u8>),
    Closed,
}

/// Configuration for spawning a pty subprocess.
///
/// The fields cover everything that varied between
/// `Pane::spawn_with_env` (TERM=xterm-256color, resize-nudge,
/// SPYC_CONTEXT env) and `spawn_capture` (TERM=dumb, FORCE_COLOR,
/// PAGER=cat, no resize-nudge needed because captures aren't
/// interactive). Callers fill in only the fields that matter for
/// their use case.
pub struct PtySpec<'a> {
    pub command: &'a str,
    pub rows: u16,
    pub cols: u16,
    pub cwd: &'a Path,
    /// Env vars that override / extend the inherited env. Caller-
    /// supplied entries win over the defaults `PtyHost::spawn` sets
    /// (TERM, COLUMNS, LINES, plus any caller `extra_env`).
    pub env: &'a [(&'a str, &'a str)],
    /// Set as the child's `TERM`. `xterm-256color` for interactive
    /// panes, `dumb` for `!` captures.
    pub term: &'a str,
    /// Whether to send a SIGWINCH-equivalent resize after spawn so
    /// rc-file shells (p10k / oh-my-zsh) re-query the pty geometry.
    /// `Pane::spawn_with_env` did this; `spawn_capture` did not.
    pub nudge_winch: bool,
    /// True if `SPYC_PTY_DEBUG` was set in the spyc environment at
    /// startup. Cached at spawn so the per-tick drain doesn't pay a
    /// `std::env::var` lookup. `Pane` sets this; capture does not.
    pub debug_dump: bool,
}

/// Shared pty host: master + writer + child + reader thread + drain
/// channel, plus the small bag of state every consumer needs
/// (last_size, closed, exit_status, debug_dump).
///
/// Owned by `Pane` (which adds a vt100 parser on top) and by
/// `BackgroundTask` (which adds a flat byte buffer + lifecycle
/// metadata). Same fields, same semantics — only the shell
/// differs.
pub struct PtyHost {
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Receiver for the reader thread's byte chunks. `None` after
    /// `take_event_rx()` — consumers (e.g. `Pane` with the v1.50.84
    /// parser worker) that want to drain on a separate thread move
    /// the receiver out at construction. Consumers that drain on
    /// the main thread (`PendingCapture`, `BackgroundTask`) leave
    /// the receiver in place and call [`Self::drain`].
    pub event_rx: Option<mpsc::Receiver<PtyEvent>>,
    /// Set by the reader thread when it sees EOF — exposes the
    /// closed state to whichever thread holds the parser. `closed`
    /// (below) is the main-thread-only mirror that also tracks
    /// whether we've harvested `exit_status` yet.
    pub closed_atomic: Arc<AtomicBool>,
    pub closed: bool,
    pub exit_status: Option<portable_pty::ExitStatus>,
    pub last_size: (u16, u16),
    pub debug_dump: bool,
    /// MVU Phase 3c: runtime-swappable wake slot the reader thread
    /// re-loads each iteration. `None` for a `Pane` (its parser worker
    /// wakes via its own `Wake`); `Some` for a main-loop-drained capture/
    /// task, installed via [`Self::set_wake`]. A slot (not a spawn-time
    /// closure) is required because `demote_pane_to_task` reclaims a host
    /// whose reader is already blocked in `read()` — only a slot can
    /// retro-target it. Cleared (`clear_wake_slot`) on promote / hard-kill
    /// so a teardown-racing close-wake fires through `None`.
    wake: Arc<Mutex<Option<Wake>>>,
}

/// Result of a single drain pass: whether this drain observed the
/// EOF that closes the host. Pre-v1.50.84 also carried a `had_bytes`
/// flag for the now-removed main-thread Pane-parsing path; the
/// remaining consumers (`PendingCapture`, `BackgroundTask`) only
/// care about the close transition.
pub struct DrainResult {
    /// True when the reader thread's `Closed` event arrived during
    /// *this* drain. `closed` on the host is sticky (stays true
    /// across subsequent drains); `newly_closed` is only true the
    /// frame we transitioned.
    pub newly_closed: bool,
}

/// MVU Phase 5 PR8: the digested outcome of a bounded exit reap
/// ([`PtyHost::reap_exit`]). Carries the few facts the capture/task
/// finalizers need to reproduce their exact status strings.
#[derive(Debug)]
pub enum ExitOutcome {
    /// The child was reaped with a status. `code` is `exit_code()`;
    /// `success` is `status.success()` (NOT `code == 0` — portable_pty
    /// maps signal deaths to `success() == false`). Captures hardcode
    /// `"exit 0"` on `success`.
    Exited { code: u32, success: bool },
    /// `wait()` returned `Err` — only the task path renders this
    /// (`"error: {msg}"` → `TaskStatus::Crashed`).
    Errored(String),
}

/// MVU Phase 5 PR8: pure digest of a reaped child status into an
/// [`ExitOutcome`] — split out of [`PtyHost::reap_exit`] so the code/
/// success mapping is unit-testable without spawning a subprocess (the
/// reap itself just supplies the `try_wait`-cached status or one
/// `wait()`).
fn digest_exit(reaped: std::io::Result<portable_pty::ExitStatus>) -> ExitOutcome {
    match reaped {
        Ok(status) => ExitOutcome::Exited {
            code: status.exit_code(),
            success: status.success(),
        },
        Err(e) => ExitOutcome::Errored(e.to_string()),
    }
}

impl PtyHost {
    /// Spawn `spec.command` in a fresh pty of `spec.rows × spec.cols`.
    /// Same machinery that `Pane::spawn_with_env` and `spawn_capture`
    /// used pre-v1.5; consumers now share it.
    pub fn spawn(spec: PtySpec) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: spec.rows,
            cols: spec.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let (shell, shell_args) = crate::shell::user_shell_invocation(spec.command);
        let mut cmd = CommandBuilder::new(&shell);
        cmd.args(shell_args.iter().map(String::as_str));
        cmd.cwd(spec.cwd);
        // Runtime `:s` overrides (formerly applied by mutating the
        // process env, which children inherited). Applied before the
        // caller's env so spec keys (TERM/COLUMNS/LINES/…) still win on
        // collision, matching the old inherit-then-override order.
        for (k, v) in crate::envset::overrides() {
            cmd.env(k, v);
        }
        cmd.env("TERM", spec.term);
        cmd.env("COLUMNS", spec.cols.to_string());
        cmd.env("LINES", spec.rows.to_string());
        for (k, v) in spec.env {
            cmd.env(k, v);
        }

        let child = pair.slave.spawn_command(cmd)?;
        // We don't need our own handle on the slave — once the child
        // exits, the master read side will see EOF.
        drop(pair.slave);

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        // Background thread pumps reader → channel. The render loop
        // drains the channel without blocking on child output.
        let (tx, event_rx) = mpsc::channel::<PtyEvent>();
        let closed_atomic = Arc::new(AtomicBool::new(false));
        let closed_flag = Arc::clone(&closed_atomic);
        // MVU Phase 3c: the wake slot the reader re-loads each iteration.
        // Starts empty; a capture/task installs its wake via `set_wake`.
        let wake: Arc<Mutex<Option<Wake>>> = Arc::new(Mutex::new(None));
        let wake_reader = Arc::clone(&wake);
        thread::spawn(move || reader_loop(reader, &tx, &closed_flag, &wake_reader));

        if spec.nudge_winch {
            // Send SIGWINCH so rc-file shells re-query the pty size
            // and render their first prompt at the right geometry.
            let _ = pair.master.resize(PtySize {
                rows: spec.rows,
                cols: spec.cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }

        Ok(Self {
            wake,
            master: pair.master,
            writer,
            child,
            event_rx: Some(event_rx),
            closed_atomic,
            closed: false,
            exit_status: None,
            last_size: (spec.rows, spec.cols),
            debug_dump: spec.debug_dump,
        })
    }

    /// Take the byte-event receiver out of the host so a worker
    /// thread can drain it directly (Pane's parser thread, v1.50.84).
    /// After this call, [`Self::drain`] becomes a no-op — callers
    /// that move the receiver are responsible for processing bytes
    /// elsewhere.
    pub const fn take_event_rx(&mut self) -> Option<mpsc::Receiver<PtyEvent>> {
        self.event_rx.take()
    }

    /// MVU Phase 3c: install (or replace) the reader-thread wake. Called
    /// when a host enters a main-loop-drained container (capture/task) and
    /// on each `:fg`/`^Z`/demote transition — a single atomic store
    /// re-targets the already-running reader. The reader re-loads the slot
    /// per iteration, so this takes effect on the next chunk/EOF.
    pub fn set_wake(&self, w: Wake) {
        *self.wake.lock().unwrap() = Some(w);
    }

    /// MVU Phase 3c: fire the installed wake once, now. Called right after
    /// `set_wake` so any bytes the reader delivered during the install window
    /// (slot was `None`, so the reader fired nothing) get drained within one
    /// wakeup rather than waiting out the `MAX_IDLE_CAP` ceiling. No-op when
    /// no wake is set.
    pub fn fire_wake_now(&self) {
        // Clone the fire closure out and drop the guard before calling it
        // (don't hold the per-host lock across the wake's send).
        let fire = self.wake.lock().unwrap().as_ref().map(|w| w.fire.clone());
        if let Some(fire) = fire {
            fire();
        }
    }

    /// MVU Phase 3c: clear the wake slot (back to `None`). Used on promote
    /// (host → Pane, which installs its own parser-worker wake) and on
    /// hard-kill, so a teardown-racing close-wake fires through `None` and
    /// never targets a container the host just left.
    pub fn clear_wake_slot(&self) {
        *self.wake.lock().unwrap() = None;
    }

    /// MVU Phase 3c: clear the wake edge flag (clear-before-read). The main
    /// loop calls this as the FIRST statement before `drain()` at each
    /// capture/task drain site — never inside `drain()` — so a chunk racing
    /// the clear re-arms the producer CAS and re-sends (at most one
    /// redundant wake, never a lost final chunk). No-op when no wake is set.
    pub fn clear_wake_pending(&self) {
        // Clone the flag Arc out and drop the guard before the store
        // (clippy nursery significant-drop discipline; cheap refcount bump).
        let pending = self
            .wake
            .lock()
            .unwrap()
            .as_ref()
            .map(|w| Arc::clone(&w.pending));
        if let Some(p) = pending {
            p.store(false, Ordering::Release);
        }
    }

    /// Drain all pending events, dispatching byte chunks to
    /// `on_bytes` and updating `closed` / `exit_status` when the
    /// reader thread observes EOF. Mirrors what `Pane::drain_output`
    /// did pre-refactor — extracted here so `BackgroundTask` can
    /// reuse the same lifecycle.
    pub fn drain<F: FnMut(&[u8])>(&mut self, mut on_bytes: F) -> DrainResult {
        let mut newly_closed = false;
        let Some(rx) = self.event_rx.as_ref() else {
            return DrainResult { newly_closed };
        };
        while let Ok(event) = rx.try_recv() {
            if self.debug_dump
                && let PtyEvent::Bytes(ref bytes) = event
                && let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/tmp/spyc_pty_debug.bin")
            {
                let _ = f.write_all(bytes);
            }
            match event {
                PtyEvent::Bytes(bytes) => on_bytes(&bytes),
                PtyEvent::Closed => {
                    if !self.closed {
                        newly_closed = true;
                    }
                    self.closed = true;
                    // Non-blocking harvest — never call wait() here,
                    // it blocks the render loop. mark_exited (a
                    // helper on the consumer) can retry on
                    // subsequent ticks.
                    if let Ok(Some(status)) = self.child.try_wait() {
                        self.exit_status = Some(status);
                    }
                }
            }
        }
        DrainResult { newly_closed }
    }

    /// MVU Phase 5 PR8: the loop's single, bounded exit reap. Call ONLY
    /// after a drain observed `newly_closed` — the reader saw EOF on the
    /// pty master, which means every fd to the slave is closed, so the
    /// direct child has already exited and this returns immediately. It is
    /// never a speculative `wait()`. Prefers an already-harvested
    /// `exit_status` (the non-blocking `try_wait` in [`Self::drain`]),
    /// falling back to one `wait()` for the brief race where `try_wait`
    /// returned `None` just as the child exited. This is the documented
    /// Phase-5 "bounded synchronous reap" exception to the
    /// no-blocking-IO-in-`update` rule (Design B: the child stays on the
    /// main thread; no per-child waiter thread).
    pub fn reap_exit(&mut self) -> ExitOutcome {
        digest_exit(
            self.exit_status
                .take()
                .map_or_else(|| self.child.wait(), Ok),
        )
    }

    /// Tell the pty about a new size. Coalesces redundant calls so
    /// repeated resizes (window-drag) don't flood the kernel with
    /// SIGWINCH. Caller is responsible for resizing any emulator
    /// (vt100 grid) sitting on top.
    pub fn resize(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        if (rows, cols) == self.last_size {
            return Ok(());
        }
        self.last_size = (rows, cols);
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    /// Forward arbitrary bytes to the child. Used for paste, send-
    /// selection, and the per-keystroke `send_key` path on `Pane`.
    pub fn write_all(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.writer.write_all(bytes)?;
        Ok(())
    }

    /// PID of the spawned child, if available. `None` after reap or
    /// if portable_pty couldn't surface one.
    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// Orderly shutdown: SIGTERM the child's process group, wait up
    /// to `grace` for voluntary exit, then SIGKILL the group and
    /// reap. Mirrors `Pane::shutdown` from before the refactor —
    /// the negative-pid signal reaches grandchildren too
    /// (portable_pty calls `setsid`, so child PID == process-group
    /// leader on Unix).
    pub fn shutdown(&mut self, grace: std::time::Duration) {
        if self.closed {
            // Reader thread already saw EOF; harvest exit_status if
            // we haven't yet. Non-blocking.
            if self.exit_status.is_none()
                && let Ok(Some(status)) = self.child.try_wait()
            {
                self.exit_status = Some(status);
            }
            return;
        }
        let Some(pid) = self.child.process_id() else {
            // No PID. Best-effort: ask portable-pty to kill the
            // immediate child and move on.
            let _ = self.child.kill();
            self.closed = true;
            return;
        };

        #[cfg(unix)]
        kill_group(pid, rustix::process::Signal::TERM);
        let deadline = std::time::Instant::now() + grace;
        while std::time::Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.exit_status = Some(status);
                    self.closed = true;
                    return;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(_) => break,
            }
        }
        #[cfg(unix)]
        kill_group(pid, rustix::process::Signal::KILL);
        if let Ok(status) = self.child.wait() {
            self.exit_status = Some(status);
        }
        self.closed = true;
    }
}

impl Drop for PtyHost {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        if let Some(pid) = self.child.process_id() {
            #[cfg(unix)]
            kill_group(pid, rustix::process::Signal::KILL);
        } else {
            let _ = self.child.kill();
        }
        // Non-blocking reap; kernel handles it if already gone.
        let _ = self.child.try_wait();
    }
}

/// `kill(-pid, sig)` — send a signal to the process group leadered by
/// `pid`. portable-pty calls `setsid` on spawn, so child PID == group
/// leader; sending to `-pid` reaches grandchildren too. Errors are
/// swallowed (matches the old `libc::kill` call which dropped the
/// return value); failure modes are "already-gone" and we don't care.
#[cfg(unix)]
fn kill_group(pid: u32, sig: rustix::process::Signal) {
    if let Some(rpid) = rustix::process::Pid::from_raw(pid as i32) {
        let _ = rustix::process::kill_process_group(rpid, sig);
    }
}

/// Reader thread: pump bytes from the pty master into the channel
/// until EOF or the channel is dropped. Same shape as the pre-v1.5
/// `pane::reader_loop`.
fn reader_loop(
    mut reader: Box<dyn Read + Send>,
    tx: &mpsc::Sender<PtyEvent>,
    closed: &AtomicBool,
    wake: &Arc<Mutex<Option<Wake>>>,
) {
    // Clone the installed Wake out of the slot (dropping the guard) so the
    // CAS + fire() never run while holding the per-host lock — clippy
    // nursery discipline + never hold a lock across the wake's msg_tx.send.
    let load_wake = || wake.lock().unwrap().clone();
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            // Ok(0) is EOF; Err is any I/O failure. Both mean the
            // child is gone.
            Ok(0) | Err(_) => {
                // Release (MVU Phase 3b, was Relaxed): publishes the close
                // so `Pane::drain_output`'s Acquire load of `closed_atomic`
                // observes it. The old 100 ms poll floor masked the missing
                // edge by re-polling; once PR2 deletes the floor the close
                // is observed ONLY when the worker's final wake drives a
                // `drain_output`, so the happens-before must be real.
                closed.store(true, Ordering::Release);
                let _ = tx.send(PtyEvent::Closed);
                // MVU Phase 3c: one final close-wake (AFTER the Release store
                // + the Closed send) so the woken main-loop drain observes
                // `newly_closed` within one wakeup. Fires through `None`
                // (no-op) on a deliberate teardown that cleared the slot.
                if let Some(w) = load_wake() {
                    (w.fire)();
                }
                return;
            }
            Ok(n) => {
                if tx.send(PtyEvent::Bytes(buf[..n].to_vec())).is_err() {
                    return; // Parent dropped the host.
                }
                // MVU Phase 3c: wake on the 0→1 edge only (collapse a byte
                // storm to one channel message); the main loop clears
                // `pending` before its drain (clear-before-read).
                if let Some(w) = load_wake()
                    && w.pending
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                        .is_ok()
                {
                    (w.fire)();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Spin a small spec for `echo` so tests can spawn a quick
    /// finite child and observe drain → bytes → close.
    fn echo_spec() -> PtySpec<'static> {
        PtySpec {
            command: "echo hello",
            rows: 24,
            cols: 80,
            cwd: Path::new("/tmp"),
            env: &[],
            term: "dumb",
            nudge_winch: false,
            debug_dump: false,
        }
    }

    /// Drain test that doesn't fork a subprocess. We feed the
    /// reader-thread function a `std::io::Cursor` over a known
    /// byte buffer, then verify the pumped chunks come back out
    /// of the channel in order, followed by a `Closed` on EOF —
    /// exactly the contract every consumer (Pane,
    /// PendingCapture, BackgroundTask) depends on. No
    /// `user_shell_invocation`, no rc files, no parallel-load
    /// timing variance.
    ///
    /// This replaced an earlier `spawn_and_drain_echo` test that
    /// spawned a real `echo` and waited for it to exit. That
    /// test was flaky because PtyHost spawns through
    /// `$SHELL -i -c <cmd>` (interactive — pulls in the user's
    /// full rc-file load). Even on machines that pass it
    /// reliably, the deadline depends on the developer's shell
    /// config, which is the wrong shape for a unit test.
    #[test]
    fn reader_loop_pumps_bytes_then_signals_close() {
        let payload: Vec<u8> = b"alpha\nbeta\n".to_vec();
        let reader =
            Box::new(std::io::Cursor::new(payload.clone())) as Box<dyn std::io::Read + Send>;
        let (tx, rx) = std::sync::mpsc::channel::<PtyEvent>();
        let closed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let closed_cl = std::sync::Arc::clone(&closed);

        let wake = std::sync::Arc::new(std::sync::Mutex::new(None));
        let thread = std::thread::spawn(move || {
            super::reader_loop(reader, &tx, &closed_cl, &wake);
        });

        // Collect events until we observe Closed. The reader
        // pumps in 8 KB chunks, then sees EOF (Cursor read
        // returns Ok(0)) and emits Closed.
        let mut got_bytes: Vec<u8> = Vec::new();
        let mut got_close = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !got_close && std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(PtyEvent::Bytes(b)) => got_bytes.extend_from_slice(&b),
                Ok(PtyEvent::Closed) => got_close = true,
                Err(_) => {}
            }
        }
        thread.join().unwrap();
        assert!(got_close, "reader_loop must emit Closed on EOF");
        assert_eq!(got_bytes, payload, "byte stream must round-trip in order");
    }

    /// MVU Phase 3c: with a `Some` wake slot, the reader fires on the byte
    /// 0→1 edge AND once on EOF — exactly two fires for one chunk (the
    /// output edge coalesces; the close-wake is unconditional). This is the
    /// wake that lets a main-loop-drained capture/task observe output + exit
    /// without the poll floor.
    #[test]
    fn reader_loop_some_slot_fires_on_output_and_close() {
        use std::sync::atomic::AtomicUsize;
        let payload = b"hello\n".to_vec();
        let reader = Box::new(std::io::Cursor::new(payload)) as Box<dyn std::io::Read + Send>;
        let (tx, rx) = std::sync::mpsc::channel::<PtyEvent>();
        let closed = Arc::new(AtomicBool::new(false));
        let closed_cl = Arc::clone(&closed);
        let count = Arc::new(AtomicUsize::new(0));
        let count_cl = Arc::clone(&count);
        let wake = Arc::new(Mutex::new(Some(Wake {
            pending: Arc::new(AtomicBool::new(false)),
            fire: Arc::new(move || {
                count_cl.fetch_add(1, Ordering::Release);
            }),
        })));
        let wake_cl = Arc::clone(&wake);
        // rx stays alive in this scope so `tx.send` succeeds and the byte
        // fire actually runs (a dropped rx would early-return the Bytes arm).
        let thread =
            std::thread::spawn(move || super::reader_loop(reader, &tx, &closed_cl, &wake_cl));
        thread.join().unwrap();
        assert_eq!(
            count.load(Ordering::Acquire),
            2,
            "one output edge (coalesced) + one unconditional close wake"
        );
        assert!(closed.load(Ordering::Acquire));
        drop(rx);
    }

    /// A `None` slot (the `Pane` case — it wakes via its parser worker) must
    /// run the reader to completion with no fire and no panic.
    #[test]
    fn reader_loop_none_slot_is_silent() {
        let reader = Box::new(std::io::Cursor::new(b"x".to_vec())) as Box<dyn std::io::Read + Send>;
        let (tx, rx) = std::sync::mpsc::channel::<PtyEvent>();
        let closed = Arc::new(AtomicBool::new(false));
        let closed_cl = Arc::clone(&closed);
        let wake = Arc::new(Mutex::new(None));
        let thread = std::thread::spawn(move || super::reader_loop(reader, &tx, &closed_cl, &wake));
        thread.join().unwrap();
        assert!(closed.load(Ordering::Acquire));
        drop(rx);
    }

    #[test]
    fn resize_updates_last_size_and_coalesces() {
        let mut host = PtyHost::spawn(echo_spec()).expect("spawn echo");
        assert_eq!(host.last_size, (24, 80));
        host.resize(30, 100).unwrap();
        assert_eq!(host.last_size, (30, 100));
        // Same dims again — should be coalesced (no error, but more
        // importantly no syscall side-effect; we can only verify by
        // confirming last_size is unchanged).
        host.resize(30, 100).unwrap();
        assert_eq!(host.last_size, (30, 100));
    }

    #[test]
    fn process_id_is_some_after_spawn() {
        let host = PtyHost::spawn(echo_spec()).expect("spawn echo");
        assert!(host.process_id().is_some());
    }

    /// MVU Phase 5 PR8: `digest_exit` (the logic `reap_exit` delegates to)
    /// maps a clean status to `Exited { code: 0, success: true }`. Hermetic
    /// — constructs `ExitStatus` directly, so it neither spawns through
    /// `$SHELL -i` nor blocks on a real `wait()` (the spawn path here would
    /// reintroduce the rc-file coupling the drain tests above deliberately
    /// dropped).
    #[test]
    fn digest_exit_clean() {
        match digest_exit(Ok(portable_pty::ExitStatus::with_exit_code(0))) {
            ExitOutcome::Exited { code, success } => {
                assert_eq!(code, 0);
                assert!(success);
            }
            ExitOutcome::Errored(e) => panic!("expected clean exit, got error: {e}"),
        }
    }

    /// A non-zero code digests to `Exited { code, success: false }` — the
    /// capture path renders `"exit {code}"`, the task path
    /// `TaskStatus::Exited(code)`.
    #[test]
    fn digest_exit_nonzero() {
        match digest_exit(Ok(portable_pty::ExitStatus::with_exit_code(3))) {
            ExitOutcome::Exited { code, success } => {
                assert_eq!(code, 3);
                assert!(!success);
            }
            ExitOutcome::Errored(e) => panic!("expected exit 3, got error: {e}"),
        }
    }

    /// A signal death is `success() == false` — so it takes the same
    /// `"exit {code}"` / `TaskStatus::Exited` branch as a non-zero exit,
    /// matching the pre-PR8 `s.success()` test.
    #[test]
    fn digest_exit_signal_is_failure() {
        match digest_exit(Ok(portable_pty::ExitStatus::with_signal("SIGKILL"))) {
            ExitOutcome::Exited { success, .. } => assert!(!success),
            ExitOutcome::Errored(e) => panic!("expected exited (signal), got error: {e}"),
        }
    }

    /// A `wait()` error digests to `Errored` (the task path's
    /// `TaskStatus::Crashed` / the capture path's `"error: {msg}"`).
    #[test]
    fn digest_exit_error() {
        match digest_exit(Err(std::io::Error::other("boom"))) {
            ExitOutcome::Errored(msg) => assert_eq!(msg, "boom"),
            ExitOutcome::Exited { .. } => panic!("expected Errored"),
        }
    }
}
