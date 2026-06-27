//! A pty-hosted subprocess rendered inside a ratatui frame.
//!
//! Bytes from the child are fed into a `vt100::Parser` which maintains a
//! 2D cell grid we render directly. Input keystrokes are encoded as ANSI
//! and written to the master side of the pty.
//!
//! The pane is deliberately a generic pty host; agent-specific defaults
//! (claude, send-selection) live in higher layers.

pub mod input;
pub mod pathref;
pub mod quick_select;
pub mod tabs;
mod widget;

pub use tabs::{AgentActivity, PaneTabs, TabEntry, TabInfo};
pub use widget::{PaneWidget, cell_style};

// Shared pty kernel that `Pane`, `PendingCapture` and `BackgroundTask`
// all wrap.
pub mod pty_host;

use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::pane::pty_host::{PtyEvent, PtyHost, PtySpec, Wake};

/// MVU Phase 3b: an opaque wake callback the parser worker fires when new
/// output arrives, so the unified event loop can sleep instead of polling
/// the pane at the old 16/100 ms floor. `crate::app` builds the closure
/// (capturing its `msg_tx` clone + the pane's `SinkId`) and hands it in at
/// `adopt`; the worker only ever sees `dyn Fn()` and never names
/// `app::Message` — the pane → app layering stays one-directional. Tests
/// pass `Arc::new(|| {})`.
pub type PaneWake = Arc<dyn Fn() + Send + Sync>;

/// A hosted subprocess + its terminal emulator state.
///
/// v1.5 Phase 6a: pty kernel (master/writer/child/reader thread/
/// event channel/last_size/closed/exit_status/debug_dump)
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
    /// MVU Phase 3b: lost-wakeup-safe edge flag. The worker CASes it
    /// `false → true` after bumping `parser_gen` and fires the wake only on
    /// the 0→1 transition (so a byte storm collapses to one channel
    /// message). The main loop CLEARS it (`clear_wake`) at the top of its
    /// pre-recv scan, BEFORE `drain_output`'s gen load — so a chunk racing
    /// the clear re-arms and re-sends (at most one redundant wakeup, never a
    /// lost final echo). Cleared by the consumer, set by the producer.
    wake_pending: Arc<AtomicBool>,
    /// Owns the parser worker thread and its stop-flag. Its `Drop`
    /// guarantees the thread is stopped and joined on every teardown
    /// path (tab close, restart, app exit) — `Pane` can't implement
    /// `Drop` itself because `take_host` moves `host` out, which
    /// `Drop` forbids (E0509).
    worker: ParserWorker,
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
        wake: PaneWake,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_env(command, rows, cols, cwd, context_path, &[], wake)
    }

    pub fn spawn_with_env(
        command: &str,
        rows: u16,
        cols: u16,
        cwd: &Path,
        context_path: &Path,
        extra_env: &[(&str, &str)],
        wake: PaneWake,
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
        Ok(Self::adopt(host, parser, wake))
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
    pub fn adopt(mut host: PtyHost, parser: vt100::Parser, wake: PaneWake) -> Self {
        let parser = Arc::new(Mutex::new(parser));
        let parser_gen = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        // MVU Phase 3b: edge flag shared with the worker. Fresh per
        // `adopt`, so a re-promoted (demote→promote) pane gets a new flag
        // and the old worker's wake can never re-arm this one.
        let wake_pending = Arc::new(AtomicBool::new(false));
        let event_rx = host
            .take_event_rx()
            .expect("PtyHost::take_event_rx returned None — already taken");
        let last_size = host.last_size;
        let debug_dump = host.debug_dump;
        let parser_clone = Arc::clone(&parser);
        let gen_clone = Arc::clone(&parser_gen);
        let stop_clone = Arc::clone(&stop);
        let wake = Wake {
            pending: Arc::clone(&wake_pending),
            fire: wake,
        };
        // Channel the worker ships its byte receiver back through when it
        // exits — normal return OR panic-unwind. The receiver no longer
        // rides the `JoinHandle` return value: a panicked thread can't
        // hand a value back through `join()`, which would strand the
        // receiver (see `RxReturn` / `parser_worker`). The receiver + its
        // return path travel together as the `RxReturn` guard.
        let (rx_home_tx, rx_home_rx) = std::sync::mpsc::channel();
        let rx_guard = RxReturn {
            rx: Some(event_rx),
            home: rx_home_tx,
        };
        let handle = thread::spawn(move || {
            parser_worker(
                rx_guard,
                stop_clone,
                parser_clone,
                gen_clone,
                last_size,
                debug_dump,
                wake,
            );
        });
        Self {
            host,
            parser,
            parser_gen,
            last_seen_gen: 0,
            wake_pending,
            worker: ParserWorker {
                stop,
                handle: Some(handle),
                rx_home_rx,
            },
            output_dirty: false,
            scroll_offset: 0,
            scrolling: false,
        }
    }

    /// MVU Phase 3b: clear the wake edge flag. Called by the main loop at
    /// the top of its pre-recv pane scan, BEFORE `drain_output`'s gen load
    /// (clear-before-read). MUST NOT be folded into `drain_output` — that
    /// runs from render's `drain_all` every frame and would re-clear the
    /// flag mid-stream, defeating the worker-side CAS coalescer.
    pub fn clear_wake(&self) {
        self.wake_pending.store(false, Ordering::Release);
    }

    /// Stop the parser worker and return the underlying `PtyHost`
    /// with the byte-event receiver restored. Used by the
    /// pane → background-task demotion path: after this call, a
    /// `BackgroundTask` can take the host and drain raw bytes from
    /// the receiver as before.
    pub fn take_host(mut self) -> PtyHost {
        if let Some(rx) = self.worker.stop_and_reclaim_rx() {
            self.host.event_rx = Some(rx);
        }
        self.host
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
        if self.host.exit_status.is_none()
            && let Ok(Some(status)) = self.host.child.try_wait()
        {
            self.host.exit_status = Some(status);
        }
    }

    /// Tell the pty about a new size. We also resize the emulator so the
    /// cell grid matches — without this, the child keeps drawing at the
    /// old dimensions.
    pub fn resize(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        // vt100's `set_size` does `rows - 1` unconditionally, so a
        // zero dimension underflow-panics (debug) / wraps to 65535
        // (release). A tiny terminal can produce a 0-row pane rect, so
        // never hand the emulator a zero dimension.
        let rows = rows.max(1);
        let cols = cols.max(1);
        if (rows, cols) == self.host.last_size {
            return Ok(());
        }
        self.host.resize(rows, cols)?;
        self.lock_parser().screen_mut().set_size(rows, cols);
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
    /// Synchronous child shutdown (SIGTERM → grace → SIGKILL) that names a
    /// slow child: `on_linger` fires once if the process is still alive a beat
    /// after SIGTERM. Used by `run_teardown` so a hang at exit tells the user
    /// which pane is holding things up. The interactive close uses
    /// [`Self::shutdown_detached`] instead (off-thread, no narration).
    pub fn shutdown_reporting(&mut self, grace: std::time::Duration, on_linger: impl FnOnce()) {
        self.host.shutdown_reporting(grace, on_linger);
    }

    /// Off-thread child shutdown for the **interactive** tab close.
    /// The SIGTERM→grace→SIGKILL→reap can block up to `grace` (~20-250 ms) on
    /// a child that doesn't exit promptly on SIGTERM (a `npm run dev` tree,
    /// say); running it on the input thread froze the UI for that long on
    /// every close. The whole `Pane` moves to a detached thread, so the
    /// parser-worker stop/join leaves the input thread too. `PtyHost::Drop`
    /// (hard SIGKILL) is the backstop.
    ///
    /// NOT for app exit: `run_teardown` keeps a *synchronous* shutdown,
    /// because spawned children live in their own process groups (`setsid`)
    /// and a detached reaper would die with the process before killing them,
    /// orphaning the tree. Interactive close has no such race — the run loop
    /// keeps going — so off-threading is safe there.
    pub fn shutdown_detached(self, grace: std::time::Duration) {
        std::thread::spawn(move || {
            let mut host = self.take_host();
            host.shutdown(grace);
        });
    }

    /// Write arbitrary bytes to the child (e.g. paste, or send-selection).
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
        let guard = self.lock_parser();
        f(guard.screen())
    }

    /// Lock the parser, tolerating poison. The worker recovers a
    /// panicked parser in place (see [`parser_worker`]); a poisoned
    /// guard here would otherwise crash the render thread, turning a
    /// recoverable one-frame glitch into a whole-session crash.
    fn lock_parser(&self) -> std::sync::MutexGuard<'_, vt100::Parser> {
        self.parser
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Run `f` with a mutable reference to the vt100 screen.
    /// Used by the scrollback adapter (which walks scrollback by
    /// temporarily shifting the offset). Same lock semantics as
    /// `with_screen` — keep the closure short.
    pub fn with_screen_mut<R, F: FnOnce(&mut vt100::Screen) -> R>(&self, f: F) -> R {
        let mut guard = self.lock_parser();
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
        // `vt100::Screen::contents()` is viewport-only — it returns at most
        // terminal_height rows at the current scrollback offset. Walking the
        // full scrollback requires the page-walk in `lines_from_scrollback`.
        let all: Vec<String> = self.with_screen_mut(|s| {
            crate::ui::scrollback::lines_from_scrollback(s)
                .into_iter()
                .map(|l| {
                    l.spans
                        .iter()
                        .map(|sp| sp.content.as_ref())
                        .collect::<String>()
                })
                .collect()
        });
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
        // `vt100::Screen::contents()` is viewport-only. Walk the full
        // scrollback + live screen via the page-walk in `lines_from_scrollback`.
        let text = self.with_screen_mut(|s| {
            crate::ui::scrollback::lines_from_scrollback(s)
                .into_iter()
                .map(|l| {
                    l.spans
                        .iter()
                        .map(|sp| sp.content.as_ref())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        });

        let now = crate::sysinfo::format_now().replace([' ', ':'], "_");
        let stamp = now.trim_end_matches("_UTC");
        let filename = format!("spyc_pane_{stamp}.txt");
        let path = std::env::current_dir()?.join(&filename);
        std::fs::write(&path, text.trim_end().to_string() + "\n")?;
        Ok(path)
    }

    /// The real maximum scrollback offset: vt100's actual buffered history
    /// length, not a guess. `scrollback_len` probes it via
    /// `set_scrollback(usize::MAX)` + readback (restoring the live offset),
    /// so clamping against it matches what vt100 will actually accept — no
    /// phantom offset above the real top.
    fn max_scrollback(&self) -> usize {
        self.with_screen_mut(crate::ui::scrollback::scrollback_len)
    }

    /// Push `scroll_offset` into vt100, then sync it back to the value vt100
    /// actually clamped to. Without the read-back, `scroll_offset` could sit
    /// *above* vt100's real top (e.g. after `g` set it to a stale 10_000
    /// guess) — then every `scroll_down` just decrements a phantom counter
    /// with no visible movement until it falls back below the real length
    /// (the "scroll-down is dead" / "dead zone after g" dead zone). Keeping
    /// the field equal to vt100's clamp keeps Top/Bot and the key math honest.
    fn apply_scroll(&mut self) {
        let off = self.scroll_offset;
        let clamped = self.with_screen_mut(|s| {
            s.set_scrollback(off);
            s.scrollback()
        });
        self.scroll_offset = clamped;
    }
}

/// Append raw pty bytes to the `SPYC_PTY_DEBUG` dump. Owner-only in the
/// state dir — never the old world-readable, symlink-followable
/// `/tmp/spyc_pty_debug.bin` (raw pty bytes can carry secrets the child
/// printed). One helper so the two dump sites (this worker and
/// `PtyHost::drain`) can't drift: the /tmp→state-dir hardening once
/// landed in `drain` but missed the copy in `parser_worker`.
fn append_pty_debug(bytes: &[u8]) {
    if let Some(mut f) = crate::state::open_state_file_append("spyc_pty_debug.bin") {
        let _ = f.write_all(bytes);
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
    guard: RxReturn,
    stop: Arc<AtomicBool>,
    parser: Arc<Mutex<vt100::Parser>>,
    parser_gen: Arc<AtomicU64>,
    initial_size: (u16, u16),
    debug_dump: bool,
    wake: Wake,
) {
    // `guard` owns the byte receiver and ships it back to the pane on EVERY
    // exit from this function — normal return AND panic-unwind. A worker
    // that unwinds (e.g. the caller-supplied `wake.fire` closure panics)
    // can't return its receiver through `join()`; without the guard that
    // strands the channel, and `take_host` would then hand back a
    // receiver-less `PtyHost` — the demoted task drains nothing forever,
    // and a later `Pane::adopt` panics on the missing rx. The guard's
    // `Drop` runs during the unwind, so the receiver always makes it home.
    // MVU Phase 3b: fire the wake once for the close transition too, so the
    // loop runs `drain_output` (and sees `closed`) within one wakeup after
    // the floor is gone — but ONLY on a natural EOF, never a deliberate
    // stop (close/demote/restart sets `stop` then enqueues/observes Closed;
    // waking then would target a just-removed pane).
    let wake_on_close = || {
        if !stop.load(Ordering::Acquire) {
            (wake.fire)();
        }
    };
    loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        match guard
            .rx
            .as_ref()
            .expect("rx present until worker exit")
            .recv_timeout(std::time::Duration::from_millis(50))
        {
            Ok(PtyEvent::Bytes(bytes)) => {
                if debug_dump {
                    append_pty_debug(&bytes);
                }
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut p = parser
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    p.process(&bytes);
                }));
                if result.is_err() {
                    crate::spyc_debug!(
                        "vt100 parser panicked on {} bytes; replacing parser to recover",
                        bytes.len()
                    );
                    // The panic unwound through a held MutexGuard, so the
                    // mutex is now poisoned. Recover the guard via
                    // into_inner, install a fresh parser, then clear the
                    // poison so the main thread's render lock is healthy
                    // again (the child repaints into the blank grid).
                    {
                        let mut p = parser
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        *p = vt100::Parser::new(initial_size.0, initial_size.1, 10_000);
                    }
                    parser.clear_poison();
                }
                // Publish the grid (UNCHANGED Release edge; pairs with
                // `drain_output`'s Acquire gen load). MUST stay BEFORE the
                // wake so a woken loop that Acquire-loads the gen always
                // sees these bytes.
                parser_gen.fetch_add(1, Ordering::Release);
                // MVU Phase 3b: wake the loop only on the 0→1 edge, so a
                // byte storm collapses to one channel message. The loop
                // clears the flag (clear-before-read) before its gen load,
                // so a chunk racing the clear re-arms and re-sends.
                if wake
                    .pending
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                    .is_ok()
                {
                    (wake.fire)();
                }
            }
            Ok(PtyEvent::Closed) => {
                // EOF — done. Reader thread already set the `closed_atomic`
                // on PtyHost. One final wake (natural EOF only) so the loop
                // runs `drain_output`, observes `closed`, and renders
                // `[exited]` within one wakeup once the poll floor is gone.
                wake_on_close();
                return;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                wake_on_close();
                return;
            }
        }
    }
}

/// Owns the parser worker thread and its stop-flag, with a `Drop`
/// that guarantees the thread is signalled and joined. This lives as a
/// `Pane` field rather than a `Pane: Drop` impl because `take_host`
/// moves `host` out of `self`, and a type that implements `Drop`
/// can't be partially moved (E0509). As a field, its own `Drop` still
/// fires on every `Pane` teardown — including the leftover after
/// `take_host` reclaims the receiver.
struct ParserWorker {
    stop: Arc<std::sync::atomic::AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    /// Receiving end of the worker's byte-receiver-return channel. The
    /// worker ships its `Receiver<PtyEvent>` back through here (via the
    /// [`RxReturn`] guard) when it exits — including on a panic-unwind,
    /// which a `JoinHandle` value can't survive. Drained by
    /// [`Self::stop_and_reclaim_rx`].
    rx_home_rx: std::sync::mpsc::Receiver<std::sync::mpsc::Receiver<PtyEvent>>,
}

impl ParserWorker {
    /// Signal the worker to stop, join it, and hand back the byte-event
    /// receiver (so the pane→background demotion can resume draining on
    /// the main thread). The receiver comes back via `rx_home_rx`, not the
    /// join value, so it survives even a panicked worker. Idempotent — a
    /// second call returns `None`.
    fn stop_and_reclaim_rx(&mut self) -> Option<std::sync::mpsc::Receiver<PtyEvent>> {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            // Wait for the worker to fully exit; its `RxReturn` guard sends
            // the receiver into `rx_home_rx` as it unwinds/returns, so by
            // the time `join` resolves the receiver is already home.
            let _ = h.join();
        }
        self.rx_home_rx.try_recv().ok()
    }
}

/// Carries the parser worker's byte receiver back to the pane when the
/// worker exits. Its `Drop` fires on a normal return **and** during a
/// panic-unwind — the latter is the point: a panicked worker thread
/// can't return its receiver through `JoinHandle::join`, so without this
/// the channel would be stranded and the host left receiver-less. See
/// [`parser_worker`].
struct RxReturn {
    rx: Option<std::sync::mpsc::Receiver<PtyEvent>>,
    home: std::sync::mpsc::Sender<std::sync::mpsc::Receiver<PtyEvent>>,
}

impl Drop for RxReturn {
    fn drop(&mut self) {
        if let Some(rx) = self.rx.take() {
            let _ = self.home.send(rx);
        }
    }
}

impl Drop for ParserWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
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
mod tests;
