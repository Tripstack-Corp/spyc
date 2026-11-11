//! A pane hosted in a `spyc-jar` (stage 2 of the jar spike), behind
//! `SPYC_JARS=1`. The jar's socket stands in for the PTY master and the child:
//! [`JarMaster`] and [`JarChild`] implement `portable-pty`'s traits, so
//! `PtyHost` drives a jarred program exactly as it drives a local one.
//!
//! The owner's end: output arrives as frames and is acknowledged once it is
//! handed to `PtyHost`'s reader, whose bounded channel is what paces the
//! program (R11). An idle connection pings so the jar's liveness deadline
//! holds. Not served yet: the foreground process group (R4), so
//! [`MasterPty::process_group_leader`] answers `None`.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use std::time::Duration;

use anyhow::Context;
use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
use spyc_jar::client::{Client, Sender};
use spyc_jar::proto::{JarSignal, Msg, Role};

/// Whether new panes run in jars: `SPYC_JARS=1`.
pub fn enabled() -> bool {
    std::env::var_os("SPYC_JARS").is_some_and(|v| v == "1")
}

pub struct Jarred {
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    pub socket: PathBuf,
}

/// Take over the pane's jar when one still answers on its socket (a restored
/// tab whose program outlived the last spyc); otherwise start a jar running
/// `argv` in `cwd` with `env` added to spyc's own, and take it as its owner.
/// The socket is named after the pane's `SPYC_PANE_ID`, which the session
/// manifest saves and a restored tab takes back.
pub fn spawn(
    argv: &[String],
    cwd: &Path,
    rows: u16,
    cols: u16,
    env: &[(String, String)],
) -> anyhow::Result<Jarred> {
    let pane_id = env.iter().find(|(k, _)| k == "SPYC_PANE_ID").map(|(_, v)| v.as_str());
    let socket = socket_path(pane_id);
    if socket.exists() {
        match attach(&socket, rows, cols) {
            Ok(adopted) => return Ok(adopted),
            Err(_) => {
                let _ = std::fs::remove_file(&socket);
            }
        }
    }
    let mut cmd = Command::new(jar_binary()?);
    cmd.arg("--socket")
        .arg(&socket)
        .arg("--cwd")
        .arg(cwd)
        .arg("--rows")
        .arg(rows.to_string())
        .arg("--cols")
        .arg(cols.to_string())
        .arg("--")
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut jar = cmd.spawn().context("start spyc-jar")?;
    // Reap the jar if it ends while spyc runs; if spyc ends first, it is
    // reparented and goes on.
    std::thread::spawn(move || {
        let _ = jar.wait();
    });
    attach(&socket, rows, cols)
}

/// Take the jar at `socket` as its owner and stream its log from the oldest
/// byte it still holds.
pub fn attach(socket: &Path, rows: u16, cols: u16) -> anyhow::Result<Jarred> {
    let (client, welcome) = Client::connect(socket, Role::Owner, Duration::from_secs(5))?;
    let Msg::Welcome { child_pid, base, rows: had_rows, cols: had_cols, .. } = welcome else {
        anyhow::bail!("expected a welcome");
    };
    let (mut rx, tx) = client.split();
    tx.send(&Msg::Replay { from: base })?;
    // Fit the jar to this pane. When the log has lost its oldest bytes the
    // replay can't rebuild the screen, so make the program repaint: a resize
    // to the size it already has repaints nothing (stage 0b), so change it
    // by a row and back.
    let resize = |r: u16| Msg::Resize { rows: r, cols, px_w: 0, px_h: 0 };
    if base > 0 && rows > 1 {
        tx.send(&resize(rows - 1))?;
        tx.send(&resize(rows))?;
    } else if (had_rows, had_cols) != (rows, cols) {
        tx.send(&resize(rows))?;
    }

    let (bytes_tx, bytes_rx) = mpsc::sync_channel::<Vec<u8>>(64);
    let exit: Arc<ExitSlot> = Arc::new((Mutex::new(None), Condvar::new()));
    {
        let tx = tx.clone();
        let exit = Arc::clone(&exit);
        std::thread::spawn(move || {
            let mut pings = 0u64;
            loop {
                match rx.recv(Duration::from_secs(2)) {
                    Ok(Some(Msg::Output { offset, bytes })) => {
                        let end = offset + bytes.len() as u64;
                        if bytes_tx.send(bytes).is_err() {
                            break;
                        }
                        let _ = tx.send(&Msg::Ack { offset: end });
                    }
                    Ok(Some(Msg::Exited { code, signaled })) => {
                        let status = if signaled {
                            ExitStatus::with_signal("signal")
                        } else {
                            ExitStatus::with_exit_code(u32::try_from(code).unwrap_or(1))
                        };
                        set_exit(&exit, status);
                        let _ = tx.send(&Msg::AckExit);
                        break;
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        pings += 1;
                        if tx.send(&Msg::Ping { n: pings }).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            if exit.0.lock().unwrap_or_else(PoisonError::into_inner).is_none() {
                set_exit(&exit, ExitStatus::with_signal("jar connection lost"));
            }
        });
    }
    Ok(Jarred {
        master: Box::new(JarMaster {
            tx: tx.clone(),
            size: Mutex::new(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }),
            reader: Mutex::new(Some(JarReader { rx: bytes_rx, buf: Vec::new(), pos: 0 })),
        }),
        child: Box::new(JarChild { pid: child_pid, exit, tx }),
        socket: socket.to_path_buf(),
    })
}

type ExitSlot = (Mutex<Option<ExitStatus>>, Condvar);

fn set_exit(slot: &ExitSlot, status: ExitStatus) {
    *slot.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(status);
    slot.1.notify_all();
}

/// `spyc-jar` beside the running spyc, or `SPYC_JAR_BIN`.
fn jar_binary() -> anyhow::Result<PathBuf> {
    if let Some(p) = std::env::var_os("SPYC_JAR_BIN") {
        return Ok(p.into());
    }
    Ok(std::env::current_exe()?.with_file_name("spyc-jar"))
}

/// `spyc/jars/<id>.sock` under `$XDG_RUNTIME_DIR`, else `$TMPDIR`, else
/// `/tmp`. The id is the pane's (its first 16 hex digits, to stay inside
/// macOS's socket-path limit), or a fresh one for a task or capture.
fn socket_path(pane_id: Option<&str>) -> PathBuf {
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .or_else(|| std::env::var_os("TMPDIR"))
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    let id = pane_id.map_or_else(
        || {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            format!("{:x}{:x}", std::process::id(), nanos & 0xffff_ffff)
        },
        |id| id.chars().filter(char::is_ascii_hexdigit).take(16).collect(),
    );
    root.join("spyc").join("jars").join(format!("{id}.sock"))
}

struct JarReader {
    rx: mpsc::Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for JarReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.buf.len() {
            match self.rx.recv() {
                Ok(b) => {
                    self.buf = b;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

struct JarWriter {
    tx: Sender,
}

impl Write for JarWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.tx.send(&Msg::Input { bytes: buf.to_vec() })?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct JarMaster {
    tx: Sender,
    size: Mutex<PtySize>,
    reader: Mutex<Option<JarReader>>,
}

impl MasterPty for JarMaster {
    fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        *self.size.lock().unwrap_or_else(PoisonError::into_inner) = size;
        self.tx.send(&Msg::Resize {
            rows: size.rows,
            cols: size.cols,
            px_w: size.pixel_width,
            px_h: size.pixel_height,
        })?;
        Ok(())
    }
    fn get_size(&self) -> anyhow::Result<PtySize> {
        Ok(*self.size.lock().unwrap_or_else(PoisonError::into_inner))
    }
    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        let reader = self.reader.lock().unwrap_or_else(PoisonError::into_inner).take();
        Ok(Box::new(reader.context("a jar's output has one reader")?))
    }
    fn take_writer(&self) -> anyhow::Result<Box<dyn Write + Send>> {
        Ok(Box::new(JarWriter { tx: self.tx.clone() }))
    }
    fn process_group_leader(&self) -> Option<libc::pid_t> {
        None
    }
    fn as_raw_fd(&self) -> Option<std::os::fd::RawFd> {
        None
    }
    fn tty_name(&self) -> Option<PathBuf> {
        None
    }
}

struct JarChild {
    pid: u32,
    exit: Arc<ExitSlot>,
    tx: Sender,
}

impl std::fmt::Debug for JarChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JarChild({})", self.pid)
    }
}

impl ChildKiller for JarChild {
    fn kill(&mut self) -> io::Result<()> {
        self.tx.send(&Msg::Signal { signal: JarSignal::Kill as i32 })
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(JarKiller(self.tx.clone()))
    }
}

impl Child for JarChild {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        Ok(self.exit.0.lock().unwrap_or_else(PoisonError::into_inner).clone())
    }
    fn wait(&mut self) -> io::Result<ExitStatus> {
        let mut slot = self.exit.0.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(s) = slot.clone() {
                return Ok(s);
            }
            slot = self.exit.1.wait(slot).unwrap_or_else(PoisonError::into_inner);
        }
    }
    fn process_id(&self) -> Option<u32> {
        Some(self.pid)
    }
}

#[derive(Debug)]
struct JarKiller(Sender);

impl ChildKiller for JarKiller {
    fn kill(&mut self) -> io::Result<()> {
        self.0.send(&Msg::Signal { signal: JarSignal::Kill as i32 })
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(Self(self.0.clone()))
    }
}
