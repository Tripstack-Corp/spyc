//! A pty-hosted subprocess rendered inside a ratatui frame.
//!
//! Bytes from the child are fed into a `vt100::Parser` which maintains a
//! 2D cell grid we render directly. Input keystrokes are encoded as ANSI
//! and written to the master side of the pty.
//!
//! This is the foundation for M8: eventually the subprocess will default
//! to `claude`, and cspy will be able to pipe its selection into the
//! pane's stdin. For the spike it is intentionally generic.

mod input;
mod widget;

pub use widget::PaneWidget;

use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

/// A hosted subprocess + its terminal emulator state.
pub struct Pane {
    /// Terminal emulator parser — keeps the cell grid we render.
    parser: vt100::Parser,
    /// Write half of the pty master (our stdin → child's stdin).
    writer: Box<dyn Write + Send>,
    /// The master; held so the pty stays open as long as the Pane lives.
    master: Box<dyn MasterPty + Send>,
    /// The child process handle. Kept for process lifetime; dropping it
    /// is how the subprocess gets cleaned up on close.
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Reader-thread events (bytes to process, or a "closed" signal).
    event_rx: mpsc::Receiver<PaneEvent>,
    /// Set when the reader thread observed EOF on the master — the
    /// subprocess has exited and the pane should be torn down.
    closed: bool,
}

/// Messages posted by the pty reader thread.
enum PaneEvent {
    Bytes(Vec<u8>),
    /// Child exited or the master was closed. Emitted exactly once before
    /// the reader thread terminates.
    Closed,
}

impl Pane {
    /// Spawn `command` in a fresh pty of `rows × cols`.
    pub fn spawn(command: &str, rows: u16, cols: u16) -> anyhow::Result<Self> {
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
        // Inherit cwd and env from the parent.
        cmd.cwd(std::env::current_dir()?);

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave); // We don't need our own handle on the slave.

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        // Background thread pumps reader → channel. We don't block the
        // render loop on child output.
        let (tx, event_rx) = mpsc::channel::<PaneEvent>();
        thread::spawn(move || reader_loop(reader, &tx));

        let parser = vt100::Parser::new(rows, cols, 0);

        Ok(Self {
            parser,
            writer,
            master: pair.master,
            _child: child,
            event_rx,
            closed: false,
        })
    }

    /// Drain any pending output from the child into the parser. Call
    /// each render tick before drawing. A `Closed` event marks the
    /// subprocess as finished so the caller can tear the pane down.
    pub fn drain_output(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                PaneEvent::Bytes(bytes) => self.parser.process(&bytes),
                PaneEvent::Closed => self.closed = true,
            }
        }
    }

    /// True once the subprocess has exited (reader thread saw EOF).
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// Tell the pty about a new size. We also resize the emulator so the
    /// cell grid matches — without this, the child keeps drawing at the
    /// old dimensions.
    pub fn resize(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
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
}

/// Pump bytes from the pty master until the child exits.
fn reader_loop(mut reader: Box<dyn Read + Send>, tx: &mpsc::Sender<PaneEvent>) {
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            // Ok(0) is EOF; Err is any I/O failure. Both mean the child is gone.
            Ok(0) | Err(_) => {
                let _ = tx.send(PaneEvent::Closed);
                return;
            }
            Ok(n) => {
                if tx.send(PaneEvent::Bytes(buf[..n].to_vec())).is_err() {
                    return; // Parent has dropped the Pane.
                }
            }
        }
    }
}
