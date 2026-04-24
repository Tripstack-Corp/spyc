//! A pty-hosted subprocess rendered inside a ratatui frame.
//!
//! Bytes from the child are fed into a `vt100::Parser` which maintains a
//! 2D cell grid we render directly. Input keystrokes are encoded as ANSI
//! and written to the master side of the pty.
//!
//! This is the foundation for M8: eventually the subprocess will default
//! to `claude`, and spyc will be able to pipe its selection into the
//! pane's stdin. For the spike it is intentionally generic.

mod input;
pub mod pathref;
pub mod tabs;
mod widget;

pub use tabs::{PaneTabs, TabEntry, TabInfo};
pub use widget::PaneWidget;

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

/// A hosted subprocess + its terminal emulator state.
pub struct Pane {
    /// Terminal emulator parser — keeps the cell grid we render.
    parser: vt100::Parser,
    /// Write half of the pty master (our stdin → child's stdin).
    writer: Box<dyn Write + Send>,
    /// The master; held so the pty stays open as long as the Pane lives.
    master: Box<dyn MasterPty + Send>,
    /// The child process handle. Used to retrieve exit status on close.
    pub(crate) child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Reader-thread events (bytes to process, or a "closed" signal).
    event_rx: mpsc::Receiver<PaneEvent>,
    /// Set by the reader thread when data is available; cleared by drain.
    has_pending: Arc<std::sync::atomic::AtomicBool>,
    /// Set when the reader thread observed EOF on the master — the
    /// subprocess has exited and the pane should be torn down.
    closed: bool,
    /// Exit status captured when the child process exits.
    pub exit_status: Option<portable_pty::ExitStatus>,
    /// Cached at construction so we don't call `std::env::var` per tick.
    debug_dump: bool,
    /// Last size passed to resize(), to skip redundant ioctl+SIGWINCH.
    last_size: (u16, u16),
    /// Set by `drain_output()` when new bytes arrived; cleared after render.
    pub output_dirty: bool,
    /// When > 0, the pane is in scroll mode: keys navigate the scrollback
    /// instead of being forwarded to the child. `drain_output()` still
    /// runs so no output is lost.
    scroll_offset: usize,
}

/// Messages posted by the pty reader thread.
enum PaneEvent {
    Bytes(Vec<u8>),
    /// Child exited or the master was closed. Emitted exactly once before
    /// the reader thread terminates.
    Closed,
}

impl Pane {
    /// Spawn `command` in a fresh pty of `rows × cols`, with `cwd` as
    /// the working directory.
    pub fn spawn(command: &str, rows: u16, cols: u16, cwd: &Path) -> anyhow::Result<Self> {
        Self::spawn_with_env(command, rows, cols, cwd, &[])
    }

    pub fn spawn_with_env(
        command: &str,
        rows: u16,
        cols: u16,
        cwd: &Path,
        extra_env: &[(&str, &str)],
    ) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // Most shells look at $SHELL or argv[0] to decide if they're a
        // login shell. For the spike we just exec whatever command the
        // caller asked for, passed through sh -c so values like
        // "claude --print" work without us parsing shell syntax.
        let mut cmd = CommandBuilder::new("sh");
        cmd.args(["-c", command]);
        // Use the caller-specified working directory.
        cmd.cwd(cwd);
        // Ensure the child sees correct terminal type and dimensions.
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLUMNS", cols.to_string());
        cmd.env("LINES", rows.to_string());
        // Tell child processes (e.g. Claude CLI's MCP server) where to
        // find this spyc instance's context file.
        cmd.env(
            crate::context::CONTEXT_ENV_VAR,
            crate::context::context_path(cwd).to_string_lossy().as_ref(),
        );
        for (k, v) in extra_env {
            cmd.env(k, v);
        }

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave); // We don't need our own handle on the slave.

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        // Background thread pumps reader → channel. We don't block the
        // render loop on child output.
        let (tx, event_rx) = mpsc::channel::<PaneEvent>();
        let has_pending = Arc::new(AtomicBool::new(false));
        let pending_flag = Arc::clone(&has_pending);
        thread::spawn(move || reader_loop(reader, &tx, &pending_flag));

        let parser = vt100::Parser::new(rows, cols, 10_000);
        let debug_dump = std::env::var("SPYC_PTY_DEBUG").is_ok();

        // Nudge: send SIGWINCH so shells (especially p10k/oh-my-zsh)
        // re-query the pty size and render their first prompt correctly.
        let _ = pair.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });

        Ok(Self {
            parser,
            writer,
            master: pair.master,
            child,
            event_rx,
            has_pending,
            closed: false,
            exit_status: None,
            debug_dump,
            last_size: (rows, cols),
            output_dirty: false,
            scroll_offset: 0,
        })
    }

    /// Drain any pending output from the child into the parser. Call
    /// each render tick before drawing. A `Closed` event marks the
    /// Quick check whether the reader thread has posted data since the
    /// last drain. Uses a relaxed atomic — no locking, no syscall.
    pub fn has_pending_output(&self) -> bool {
        self.has_pending.load(Ordering::Relaxed)
    }

    /// subprocess as finished so the caller can tear the pane down.
    /// Returns `true` if any bytes were processed.
    pub fn drain_output(&mut self) -> bool {
        let mut had_bytes = false;
        while let Ok(event) = self.event_rx.try_recv() {
            // Debug: dump raw pty bytes when SPYC_PTY_DEBUG was set at spawn.
            if self.debug_dump {
                if let PaneEvent::Bytes(ref bytes) = event {
                    use std::io::Write;
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
                PaneEvent::Bytes(bytes) => {
                    had_bytes = true;
                    self.parser.process(&bytes);
                }
                PaneEvent::Closed => {
                    self.closed = true;
                    // Non-blocking harvest — if the child hasn't been
                    // reaped yet, mark_exited will retry on subsequent
                    // iterations. Never call wait() here — it blocks.
                    if let Ok(Some(status)) = self.child.try_wait() {
                        self.exit_status = Some(status);
                    }
                }
            }
        }
        self.has_pending.store(false, Ordering::Relaxed);
        if had_bytes {
            self.output_dirty = true;
        }
        had_bytes
    }

    /// True once the subprocess has exited (reader thread saw EOF).
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// Tell the pty about a new size. We also resize the emulator so the
    /// cell grid matches — without this, the child keeps drawing at the
    /// old dimensions.
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
        self.parser.set_size(rows, cols);
        Ok(())
    }

    /// Forward a crossterm key to the child as ANSI bytes.
    pub fn send_key(&mut self, key: crossterm::event::KeyEvent) -> anyhow::Result<()> {
        let bytes = input::encode_key(key);
        if !bytes.is_empty() {
            self.writer.write_all(&bytes)?;
        }
        Ok(())
    }

    /// Write arbitrary bytes to the child (e.g. paste, or send-selection).
    #[allow(dead_code)] // wired into the S-key handler in the next step
    pub fn send_bytes(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.writer.write_all(bytes)?;
        Ok(())
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// Return visible screen content as individual lines (plain text,
    /// no ANSI escapes).
    #[allow(dead_code)]
    pub fn visible_lines(&self) -> Vec<String> {
        let screen = self.parser.screen();
        screen.contents().lines().map(String::from).collect()
    }

    /// Return the most recent `max_lines` of output (scrollback + visible
    /// screen). Used by `gf`/`gF` so path references that scrolled past
    /// the viewport are still found.
    pub fn recent_lines(&mut self, max_lines: usize) -> Vec<String> {
        let prev = self.scroll_offset;
        let max_sb = self.max_scrollback();
        self.parser.set_scrollback(max_sb);
        let all: Vec<String> = self
            .parser
            .screen()
            .contents()
            .lines()
            .map(String::from)
            .collect();
        self.parser.set_scrollback(prev);
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
        self.scroll_offset > 0
    }

    /// Enter scroll mode — start one line above live so the user
    /// immediately sees "you left live view".
    pub fn enter_scroll_mode(&mut self) {
        self.scroll_offset = 1;
        self.apply_scroll();
    }

    /// Exit scroll mode and snap back to the live view.
    pub fn exit_scroll_mode(&mut self) {
        self.scroll_offset = 0;
        self.apply_scroll();
    }

    /// Scroll up (further into history) by `n` lines.
    pub fn scroll_up(&mut self, n: usize) {
        let max = self.max_scrollback();
        self.scroll_offset = self.scroll_offset.saturating_add(n).min(max);
        self.apply_scroll();
    }

    /// Scroll down (toward live) by `n` lines. Exits scroll mode
    /// automatically if we reach the bottom.
    pub fn scroll_down_or_exit(&mut self, n: usize) {
        if self.scroll_offset <= n {
            self.exit_scroll_mode();
        } else {
            self.scroll_offset -= n;
            self.apply_scroll();
        }
    }

    /// Jump to the oldest line in scrollback.
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = self.max_scrollback().max(1);
        self.apply_scroll();
    }

    /// Jump back to live view.
    pub fn scroll_to_bottom(&mut self) {
        self.exit_scroll_mode();
    }

    /// Save full scrollback + screen contents to a timestamped file.
    pub fn save_to_file(&mut self) -> std::io::Result<std::path::PathBuf> {
        // Temporarily set scrollback to max so contents() captures everything.
        let prev = self.scroll_offset;
        let max = self.max_scrollback();
        self.parser.set_scrollback(max);
        let text = self.parser.screen().contents();
        // Restore previous view.
        self.parser.set_scrollback(prev);

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

    fn apply_scroll(&mut self) {
        self.parser.set_scrollback(self.scroll_offset);
    }
}

/// Pump bytes from the pty master until the child exits.
fn reader_loop(
    mut reader: Box<dyn Read + Send>,
    tx: &mpsc::Sender<PaneEvent>,
    pending: &AtomicBool,
) {
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            // Ok(0) is EOF; Err is any I/O failure. Both mean the child is gone.
            Ok(0) | Err(_) => {
                pending.store(true, Ordering::Relaxed);
                let _ = tx.send(PaneEvent::Closed);
                return;
            }
            Ok(n) => {
                pending.store(true, Ordering::Relaxed);
                if tx.send(PaneEvent::Bytes(buf[..n].to_vec())).is_err() {
                    return; // Parent has dropped the Pane.
                }
            }
        }
    }
}
