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
//! behavior. The reader-thread protocol, debug-byte-dump, has-pending
//! flag, exit-status harvesting, and shutdown semantics all match the
//! pre-refactor `Pane`/`BackgroundTask` paths exactly.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

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
/// (last_size, closed, exit_status, has_pending, debug_dump).
///
/// Owned by `Pane` (which adds a vt100 parser on top) and by
/// `BackgroundTask` (which adds a flat byte buffer + lifecycle
/// metadata). Same fields, same semantics — only the shell
/// differs.
pub struct PtyHost {
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    pub event_rx: mpsc::Receiver<PtyEvent>,
    pub has_pending: Arc<AtomicBool>,
    pub closed: bool,
    pub exit_status: Option<portable_pty::ExitStatus>,
    pub last_size: (u16, u16),
    pub debug_dump: bool,
}

/// Result of a single drain pass: how much was processed and
/// whether this drain observed the EOF that closes the host.
pub struct DrainResult {
    pub had_bytes: bool,
    /// True when the reader thread's `Closed` event arrived during
    /// *this* drain. `closed` on the host is sticky (stays true
    /// across subsequent drains); `newly_closed` is only true the
    /// frame we transitioned.
    pub newly_closed: bool,
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
        let has_pending = Arc::new(AtomicBool::new(false));
        let pending_flag = Arc::clone(&has_pending);
        thread::spawn(move || reader_loop(reader, &tx, &pending_flag));

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
            master: pair.master,
            writer,
            child,
            event_rx,
            has_pending,
            closed: false,
            exit_status: None,
            last_size: (spec.rows, spec.cols),
            debug_dump: spec.debug_dump,
        })
    }

    /// Quick non-blocking check whether the reader thread has posted
    /// data since the last drain. Relaxed-atomic; no lock, no
    /// syscall — cheap to call every render tick.
    pub fn has_pending_output(&self) -> bool {
        self.has_pending.load(Ordering::Relaxed)
    }

    /// Drain all pending events, dispatching byte chunks to
    /// `on_bytes` and updating `closed` / `exit_status` when the
    /// reader thread observes EOF. Mirrors what `Pane::drain_output`
    /// did pre-refactor — extracted here so `BackgroundTask` can
    /// reuse the same lifecycle.
    pub fn drain<F: FnMut(&[u8])>(&mut self, mut on_bytes: F) -> DrainResult {
        let mut had_bytes = false;
        let mut newly_closed = false;
        while let Ok(event) = self.event_rx.try_recv() {
            if self.debug_dump {
                if let PtyEvent::Bytes(ref bytes) = event {
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/tmp/spyc_pty_debug.bin")
                    {
                        let _ = f.write_all(bytes);
                    }
                }
            }
            match event {
                PtyEvent::Bytes(bytes) => {
                    had_bytes = true;
                    on_bytes(&bytes);
                }
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
        self.has_pending.store(false, Ordering::Relaxed);
        DrainResult {
            had_bytes,
            newly_closed,
        }
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
            if self.exit_status.is_none() {
                if let Ok(Some(status)) = self.child.try_wait() {
                    self.exit_status = Some(status);
                }
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
    pending: &AtomicBool,
) {
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            // Ok(0) is EOF; Err is any I/O failure. Both mean the
            // child is gone.
            Ok(0) | Err(_) => {
                pending.store(true, Ordering::Relaxed);
                let _ = tx.send(PtyEvent::Closed);
                return;
            }
            Ok(n) => {
                pending.store(true, Ordering::Relaxed);
                if tx.send(PtyEvent::Bytes(buf[..n].to_vec())).is_err() {
                    return; // Parent dropped the host.
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
        let pending = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let pending_cl = std::sync::Arc::clone(&pending);

        let thread = std::thread::spawn(move || super::reader_loop(reader, &tx, &pending_cl));

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
        assert!(
            pending.load(std::sync::atomic::Ordering::Relaxed),
            "pending flag must be set when bytes arrive"
        );
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
}
