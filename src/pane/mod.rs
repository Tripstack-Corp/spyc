//! A pty-hosted subprocess rendered inside a ratatui frame.
//!
//! Bytes from the child are fed into a `vt100::Parser` which maintains a
//! 2D cell grid we render directly. Input keystrokes are encoded as ANSI
//! and written to the master side of the pty.
//!
//! This is the foundation for M8: eventually the subprocess will default
//! to `claude`, and spyc will be able to pipe its selection into the
//! pane's stdin. For the spike it is intentionally generic.

pub mod input;
pub mod pathref;
pub mod quick_select;
pub mod tabs;
mod widget;

pub use tabs::{PaneTabs, TabEntry, TabInfo};
pub use widget::{PaneWidget, cell_style};

// v1.5 Phase 6a: shared pty kernel that `Pane`, `PendingCapture`
// and `BackgroundTask` all wrap. Phase 6b/6c (promotion / demotion)
// becomes a state shift on the same handles.
pub mod pty_host;

use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::pane::pty_host::{PtyEvent, PtyHost, PtySpec};

/// A hosted subprocess + its terminal emulator state.
///
/// v1.5 Phase 6a: pty kernel (master/writer/child/reader thread/
/// event channel/last_size/closed/exit_status/has_pending/debug_dump)
/// lives in `host: PtyHost`. `Pane` is the vt100-parser shell on top.
/// Same struct shape `BackgroundTask` will adopt in 6a step 3, which
/// is what unlocks promotion / demotion in 6b/6c.
pub struct Pane {
    /// Shared pty kernel. Owns master/writer/child/reader-thread/
    /// event channel/closed/exit_status/last_size/debug_dump.
    pub host: PtyHost,
    /// Terminal emulator parser — keeps the cell grid we render.
    /// v1.50.84: shared with the parser worker thread via Mutex.
    /// The worker thread (spawned in `adopt`) consumes bytes from
    /// the reader-thread channel and parses them into this state;
    /// the main thread locks briefly during render to read the
    /// grid. Lock contention is small because each lock window is
    /// brief.
    parser: Arc<Mutex<vt100::Parser>>,
    /// Generation counter incremented by the parser worker each
    /// time it processes a byte chunk. `drain_output()` compares
    /// this to `last_seen_gen` to detect new output without locking
    /// the parser.
    parser_gen: Arc<AtomicU64>,
    /// Most recent `parser_gen` value the main thread has observed.
    /// Diffed against `parser_gen` in `drain_output()` to decide
    /// whether a redraw is needed.
    last_seen_gen: u64,
    /// Flag the parser worker checks periodically (between
    /// `recv_timeout` waits). Set by [`Self::take_host`] when the
    /// pane is being demoted to a `BackgroundTask` — the worker
    /// exits without seeing EOF and returns the receiver so the
    /// host can keep accepting raw bytes for the task.
    stop_parser: Arc<std::sync::atomic::AtomicBool>,
    /// Handle on the parser worker thread. Returns the receiver on
    /// thread exit so `take_host` can restore it to the host.
    parser_thread: Option<thread::JoinHandle<std::sync::mpsc::Receiver<PtyEvent>>>,
    /// Set by `drain_output()` when new bytes arrived; cleared after render.
    pub output_dirty: bool,
    /// When > 0, the visible viewport is shifted into the scrollback
    /// by this many lines. Independent of `scrolling` so entering
    /// scroll mode doesn't jump the view (we set the flag without
    /// shifting).
    scroll_offset: usize,
    /// True when the pane is in scroll mode: keys navigate the
    /// scrollback instead of being forwarded to the child.
    /// `drain_output()` still runs so no output is lost.
    scrolling: bool,
}

impl Pane {
    /// Spawn `command` in a fresh pty of `rows × cols`, with `cwd` as
    /// the working directory. `context_path` points at *App's* live
    /// context file (the one the main loop writes to) so the child's
    /// `SPYC_CONTEXT` always resolves to a real file regardless of
    /// where the pane itself spawns — App writes one canonical
    /// `<start_dir>/.spyc-context-<pid>.json`, but a pane can spawn
    /// in any subdir, and recomputing from `cwd` would point at a
    /// path nobody writes.
    pub fn spawn(
        command: &str,
        rows: u16,
        cols: u16,
        cwd: &Path,
        context_path: &Path,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_env(command, rows, cols, cwd, context_path, &[])
    }

    pub fn spawn_with_env(
        command: &str,
        rows: u16,
        cols: u16,
        cwd: &Path,
        context_path: &Path,
        extra_env: &[(&str, &str)],
    ) -> anyhow::Result<Self> {
        // Pane env: tell child processes (e.g. Claude CLI's MCP server)
        // where this spyc instance's context file lives, and advertise
        // truecolor support so apps that negotiate it (bat, fzf, delta,
        // …) don't silently downgrade to 256-color. TERM=xterm-256color
        // alone doesn't signal 24-bit; COLORTERM=truecolor is the de
        // facto standard pair.
        let context_str = context_path.to_string_lossy().into_owned();
        let mut env: Vec<(&str, &str)> = Vec::with_capacity(extra_env.len() + 2);
        env.push((crate::context::CONTEXT_ENV_VAR, context_str.as_str()));
        env.push(("COLORTERM", "truecolor"));
        env.extend(extra_env.iter().copied());

        let host = PtyHost::spawn(PtySpec {
            command,
            rows,
            cols,
            cwd,
            env: &env,
            term: "xterm-256color",
            // Send SIGWINCH right after spawn so rc-file shells
            // (p10k / oh-my-zsh) re-query the pty size and render
            // their first prompt at the right geometry.
            nudge_winch: true,
            debug_dump: std::env::var("SPYC_PTY_DEBUG").is_ok(),
        })?;
        let parser = vt100::Parser::new(rows, cols, 10_000);
        Ok(Self::adopt(host, parser))
    }

    /// Build a `Pane` around an already-spawned `PtyHost` and a
    /// vt100 parser. v1.5 Phase 6 promotion path: a backgrounded
    /// task hands over its `PtyHost`, we wrap it in a fresh parser
    /// (or one pre-fed with the task's captured output), and the
    /// pty keeps running through the transition.
    ///
    /// v1.50.84: takes the byte-event receiver out of the host and
    /// spawns a parser worker thread. The thread owns the receiver
    /// and consumes bytes into the (mutex-wrapped) parser. Main
    /// thread reads grid state via `with_screen` / `with_screen_mut`.
    pub fn adopt(mut host: PtyHost, parser: vt100::Parser) -> Self {
        let parser = Arc::new(Mutex::new(parser));
        let parser_gen = Arc::new(AtomicU64::new(0));
        let stop_parser = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let event_rx = host
            .take_event_rx()
            .expect("PtyHost::take_event_rx returned None — already taken");
        let last_size = host.last_size;
        let debug_dump = host.debug_dump;
        let parser_clone = Arc::clone(&parser);
        let gen_clone = Arc::clone(&parser_gen);
        let stop_clone = Arc::clone(&stop_parser);
        let parser_thread = thread::spawn(move || {
            parser_worker(
                event_rx,
                stop_clone,
                parser_clone,
                gen_clone,
                last_size,
                debug_dump,
            )
        });
        Self {
            host,
            parser,
            parser_gen,
            last_seen_gen: 0,
            stop_parser,
            parser_thread: Some(parser_thread),
            output_dirty: false,
            scroll_offset: 0,
            scrolling: false,
        }
    }

    /// Stop the parser worker and return the underlying `PtyHost`
    /// with the byte-event receiver restored. Used by the
    /// pane → background-task demotion path: after this call, a
    /// `BackgroundTask` can take the host and drain raw bytes from
    /// the receiver as before.
    pub fn take_host(mut self) -> PtyHost {
        self.stop_parser.store(true, Ordering::Release);
        if let Some(handle) = self.parser_thread.take() {
            if let Ok(rx) = handle.join() {
                self.host.event_rx = Some(rx);
            }
        }
        self.host
    }

    /// Drain any pending output from the child into the parser. Call
    /// Quick check whether the reader thread has posted data since
    /// the last drain. Uses a relaxed atomic — no locking, no
    /// syscall. Delegates to the shared `PtyHost`.
    pub fn has_pending_output(&self) -> bool {
        self.host.has_pending_output()
    }

    /// Check whether the parser worker has processed new bytes since
    /// the last call. Replaces the old "drain bytes into parser"
    /// path: bytes are now consumed continuously on the worker
    /// thread; this just samples the generation counter.
    ///
    /// Also syncs the closed-state mirror from the atomic the reader
    /// thread sets on EOF — without this the main thread can't see
    /// the close transition since `PtyHost::drain` (which used to
    /// observe `PtyEvent::Closed`) no longer runs for `Pane`.
    pub fn drain_output(&mut self) -> bool {
        // Sync close state. `host.closed` is the main-thread mirror
        // used by callers (and by `shutdown` to short-circuit). The
        // atomic is set by the reader thread on EOF.
        if !self.host.closed && self.host.closed_atomic.load(Ordering::Acquire) {
            self.host.closed = true;
            // Non-blocking harvest — same as `PtyHost::drain`.
            if let Ok(Some(status)) = self.host.child.try_wait() {
                self.host.exit_status = Some(status);
            }
        }
        let gen_now = self.parser_gen.load(Ordering::Acquire);
        if gen_now == self.last_seen_gen {
            false
        } else {
            self.last_seen_gen = gen_now;
            self.output_dirty = true;
            true
        }
    }

    /// True once the subprocess has exited (reader thread saw EOF).
    pub const fn is_closed(&self) -> bool {
        self.host.closed
    }

    /// Exit status captured from the reaped child, if available.
    /// Replaces the pre-v1.5 `pub exit_status` field.
    pub const fn exit_status(&self) -> Option<&portable_pty::ExitStatus> {
        self.host.exit_status.as_ref()
    }

    /// Retry harvesting the exit status if `drain_output` missed it
    /// (the reader thread closes before `try_wait` can reap).
    /// `tabs.rs::label_exited_panes` calls this once per tick.
    pub fn try_harvest_exit_status(&mut self) {
        if self.host.exit_status.is_none() {
            if let Ok(Some(status)) = self.host.child.try_wait() {
                self.host.exit_status = Some(status);
            }
        }
    }

    /// Tell the pty about a new size. We also resize the emulator so the
    /// cell grid matches — without this, the child keeps drawing at the
    /// old dimensions.
    pub fn resize(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        if (rows, cols) == self.host.last_size {
            return Ok(());
        }
        self.host.resize(rows, cols)?;
        if let Ok(mut p) = self.parser.lock() {
            p.screen_mut().set_size(rows, cols);
        }
        Ok(())
    }

    /// Forward a crossterm key to the child as ANSI bytes.
    pub fn send_key(&mut self, key: crossterm::event::KeyEvent) -> anyhow::Result<()> {
        let bytes = input::encode_key(key);
        if !bytes.is_empty() {
            if crate::key_trace::is_enabled() {
                crate::key_trace::log_tx(&format!(
                    "send_key code={:?} mods={:?} bytes={}",
                    key.code,
                    key.modifiers,
                    preview_bytes(&bytes),
                ));
            }
            self.host.write_all(&bytes)?;
        }
        Ok(())
    }

    /// PID of the spawned child, if available. `None` after the child
    /// has been reaped or if portable_pty couldn't surface it.
    pub fn process_id(&self) -> Option<u32> {
        self.host.process_id()
    }

    /// Best-effort kill of just the immediate child (no signal-group
    /// escalation). Used by tab-restart, which then re-spawns.
    pub fn try_kill(&mut self) {
        let _ = self.host.child.kill();
    }

    /// Orderly shutdown of the child and its descendants. Sends
    /// `SIGTERM` to the child's process group (negative PID — reaches
    /// every grandchild, which is the actual user-reported scenario:
    /// `npm run dev` → node → esbuild → workers all need to die when
    /// the tab closes), waits up to `grace` for voluntary exit, then
    /// `SIGKILL`s the group if it's still alive. Reaps the child so
    /// no zombies are left behind.
    ///
    /// portable-pty calls `setsid` for spawned children on Unix, so
    /// the child's PID is also its process-group leader — sending to
    /// `-pid` reaches the whole tree. Already-exited children
    /// short-circuit (the reader thread set `closed`).
    pub fn shutdown(&mut self, grace: std::time::Duration) {
        // Delegated to the host — same SIGTERM-then-SIGKILL escalation
        // as the pre-v1.5 inline implementation, just owned by the
        // shared kernel now.
        self.host.shutdown(grace);
    }

    /// Write arbitrary bytes to the child (e.g. paste, or send-selection).
    #[allow(dead_code)] // wired into the S-key handler in the next step
    pub fn send_bytes(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if crate::key_trace::is_enabled() {
            crate::key_trace::log_tx(&format!(
                "send_bytes len={} preview={}",
                bytes.len(),
                preview_bytes(bytes),
            ));
        }
        self.host.write_all(bytes)
    }

    /// Run `f` with a shared reference to the vt100 screen. The
    /// parser is mutex-protected (the worker thread owns the write
    /// path); this acquires the lock for the duration of `f`, so
    /// keep the closure body short. Returns whatever `f` returns.
    pub fn with_screen<R, F: FnOnce(&vt100::Screen) -> R>(&self, f: F) -> R {
        let guard = self.parser.lock().expect("pane parser mutex poisoned");
        f(guard.screen())
    }

    /// Run `f` with a mutable reference to the vt100 screen.
    /// Used by the scrollback adapter (which walks scrollback by
    /// temporarily shifting the offset). Same lock semantics as
    /// `with_screen` — keep the closure short.
    pub fn with_screen_mut<R, F: FnOnce(&mut vt100::Screen) -> R>(&self, f: F) -> R {
        let mut guard = self.parser.lock().expect("pane parser mutex poisoned");
        f(guard.screen_mut())
    }

    /// True when the child has switched to the xterm alternate screen
    /// (`\e[?1049h` or `\e[?47h`). Full-screen TUIs (codex, claude
    /// post-startup, vim, htop, lazygit) live there. Content drawn in
    /// alt-screen never enters main-screen scrollback, so spyc's
    /// `^a v` scroll-back has nothing to show in that mode — callers
    /// can flash a hint pointing the user at the app's own history
    /// viewer instead.
    pub fn is_alternate_screen(&self) -> bool {
        self.with_screen(vt100::Screen::alternate_screen)
    }

    /// Return visible screen content as individual lines (plain text,
    /// no ANSI escapes). When the pane is in scroll mode, this is
    /// exactly the viewport the user is looking at — *not* the live
    /// tail. Use `pickable_text` for picker/scanner code that should
    /// follow the user's eye.
    pub fn visible_lines(&self) -> Vec<String> {
        self.with_screen(|s| s.contents().lines().map(String::from).collect())
    }

    /// Text that interactive pickers (`gf`/`gF`, `^a u`) should scan:
    /// what the user is currently looking at. While scrolling, that
    /// is the exact visible viewport at the user's scroll position;
    /// while live, we widen to the last `recent_n` lines so
    /// paths/URLs that just rolled past the bottom are still
    /// findable. Without this distinction, scanning from a fixed
    /// slice means a user who scrolled up to find a URL would have
    /// it ignored — the picker would read a different region than
    /// their eyes.
    pub fn pickable_text(&self, recent_n: usize) -> Vec<String> {
        if self.is_scrolling() {
            self.visible_lines()
        } else {
            self.recent_lines(recent_n)
        }
    }

    /// Return the most recent `max_lines` of output (scrollback + visible
    /// screen). Used by `gf`/`gF` so path references that scrolled past
    /// the viewport are still found.
    pub fn recent_lines(&self, max_lines: usize) -> Vec<String> {
        let prev = self.scroll_offset;
        let max_sb = self.max_scrollback();
        let all: Vec<String> = self.with_screen_mut(|s| {
            s.set_scrollback(max_sb);
            let lines: Vec<String> = s.contents().lines().map(String::from).collect();
            s.set_scrollback(prev);
            lines
        });
        // Return only the last `max_lines` lines.
        if all.len() > max_lines {
            all[all.len() - max_lines..].to_vec()
        } else {
            all
        }
    }

    // ---- Scroll mode ------------------------------------------------

    /// True when the user is browsing scrollback history.
    pub const fn is_scrolling(&self) -> bool {
        self.scrolling
    }

    /// Enter scroll mode without shifting the view. The mode flag
    /// drives the divider re-color and tab uppercase that signal
    /// "you've left live view" — shifting the viewport would just
    /// add a jarring jump.
    pub const fn enter_scroll_mode(&mut self) {
        self.scrolling = true;
    }

    /// Exit scroll mode and snap back to live.
    pub fn exit_scroll_mode(&mut self) {
        self.scrolling = false;
        self.scroll_offset = 0;
        self.apply_scroll();
    }

    /// Scroll up (further into history) by `n` lines.
    pub fn scroll_up(&mut self, n: usize) {
        let max = self.max_scrollback();
        self.scroll_offset = self.scroll_offset.saturating_add(n).min(max);
        self.apply_scroll();
    }

    /// Scroll down (toward live) by `n` lines. If we're at the live
    /// position, this is a no-op while still in scroll mode (use
    /// `exit_scroll_mode` to leave).
    pub fn scroll_down_or_exit(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
        self.apply_scroll();
    }

    /// Jump to the oldest line in scrollback.
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = self.max_scrollback().max(1);
        self.apply_scroll();
    }

    /// Jump back to live view (stays in scroll mode; user must
    /// explicitly `exit_scroll_mode` to leave).
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.apply_scroll();
    }

    /// Save full scrollback + screen contents to a timestamped file.
    pub fn save_to_file(&self) -> std::io::Result<std::path::PathBuf> {
        // Temporarily set scrollback to max so contents() captures everything.
        let prev = self.scroll_offset;
        let max = self.max_scrollback();
        let text = self.with_screen_mut(|s| {
            s.set_scrollback(max);
            let text = s.contents();
            s.set_scrollback(prev);
            text
        });

        let now = crate::sysinfo::format_now().replace([' ', ':'], "_");
        let stamp = now.trim_end_matches("_UTC");
        let filename = format!("spyc_pane_{stamp}.txt");
        let path = std::env::current_dir()?.join(&filename);
        std::fs::write(&path, text.trim_end().to_string() + "\n")?;
        Ok(path)
    }

    #[allow(clippy::unused_self)]
    const fn max_scrollback(&self) -> usize {
        // vt100 stores scrollback rows in an internal VecDeque.
        // screen().scrollback() returns the *current* offset, not
        // the maximum. We can probe by temporarily setting a huge
        // offset — set_scrollback clamps to the actual buffer length.
        // But that mutates, so we use a simpler heuristic: the
        // contents() method with full scrollback gives us everything.
        // For navigation we need the max offset. The internal buffer
        // length isn't directly exposed, so we binary-search or just
        // set a large value and read back what it clamped to.
        //
        // Actually, set_scrollback(usize::MAX) clamps internally,
        // but we'd need &mut. Since we already have &self in some
        // callers, let's store it. For now, use a reasonable upper
        // bound and accept the clamp.
        10_000 // matches our scrollback_len
    }

    fn apply_scroll(&self) {
        let off = self.scroll_offset;
        self.with_screen_mut(|s| s.set_scrollback(off));
    }
}

/// Parser worker thread: consumes bytes from the PTY reader-thread
/// channel and processes them into the shared vt100 parser. Runs
/// concurrently with the main thread; main thread reads the grid
/// via `with_screen` while the worker writes — both serialized by
/// the mutex.
///
/// vt100 0.15 has a known panic-on-`unwrap` path on some valid
/// escape sequences (nvim's exit-from-alt-screen byte stream is a
/// known trigger). `catch_unwind` recovers by rebuilding a fresh
/// parser at the current dimensions; the screen looks blank
/// briefly, then the child repaints. Same safety net as the
/// pre-v1.50.84 main-thread `process_bytes_safe`.
fn parser_worker(
    rx: std::sync::mpsc::Receiver<PtyEvent>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    parser: Arc<Mutex<vt100::Parser>>,
    parser_gen: Arc<AtomicU64>,
    initial_size: (u16, u16),
    debug_dump: bool,
) -> std::sync::mpsc::Receiver<PtyEvent> {
    loop {
        if stop.load(Ordering::Acquire) {
            return rx;
        }
        match rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(PtyEvent::Bytes(bytes)) => {
                if debug_dump {
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/tmp/spyc_pty_debug.bin")
                    {
                        let _ = f.write_all(&bytes);
                    }
                }
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut p = parser.lock().expect("pane parser mutex poisoned");
                    p.process(&bytes);
                }));
                if result.is_err() {
                    crate::spyc_debug!(
                        "vt100 parser panicked on {} bytes; replacing parser to recover",
                        bytes.len()
                    );
                    if let Ok(mut p) = parser.lock() {
                        *p = vt100::Parser::new(initial_size.0, initial_size.1, 10_000);
                    }
                }
                parser_gen.fetch_add(1, Ordering::Release);
            }
            Ok(PtyEvent::Closed) => {
                // EOF — done. Reader thread already set the
                // `closed_atomic` on PtyHost. Main thread will pick
                // it up on its next `drain_output`.
                return rx;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return rx,
        }
    }
}

// Drop and reader_loop moved into `pty_host.rs` in Phase 6a — Pane
// no longer owns the pty kernel directly, so the safety-net SIGKILL
// and the byte-pump thread live with the kernel. PtyHost::Drop
// fires when a Pane is dropped (PtyHost is the only field that
// owns the child).

/// Human-readable preview of bytes for `key_trace::log_tx`. ASCII
/// printables stay as-is; controls render with `^X` notation
/// (CR → `^M`, ESC → `^[`, etc.). Truncated past 32 bytes so a
/// large paste doesn't dump the whole buffer into the log.
fn preview_bytes(bytes: &[u8]) -> String {
    const MAX: usize = 32;
    let slice = &bytes[..bytes.len().min(MAX)];
    let mut out = String::with_capacity(slice.len() * 2 + 2);
    out.push('"');
    for &b in slice {
        match b {
            0x20..=0x7e if b != b'"' && b != b'\\' => out.push(b as char),
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\r' => out.push_str("^M"),
            b'\n' => out.push_str("^J"),
            b'\t' => out.push_str("^I"),
            0x1b => out.push_str("^["),
            0x00..=0x1f => {
                use std::fmt::Write;
                let _ = write!(out, "^{}", (b + b'@') as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    out.push('"');
    if bytes.len() > MAX {
        use std::fmt::Write;
        let _ = write!(out, "+{}", bytes.len() - MAX);
    }
    out
}

#[cfg(test)]
mod preview_tests {
    use super::preview_bytes;

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
