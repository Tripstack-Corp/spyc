//! A pane hosted in a `spyc-jar` (stage 2 of the jar spike), behind
//! `SPYC_JARS=1`. The jar's socket stands in for the PTY master and the child:
//! [`JarMaster`] and [`JarChild`] implement `portable-pty`'s traits, so
//! `PtyHost` drives a jarred program exactly as it drives a local one.
//!
//! The owner's end: output arrives as frames and is acknowledged once it is
//! handed to `PtyHost`'s reader, whose bounded channel is what paces the
//! program (R11). An idle connection pings so the jar's liveness deadline
//! holds. The foreground process group (R4) arrives as `Foreground` frames
//! and is cached, so [`MasterPty::process_group_leader`] never waits.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

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
    /// Where the terminal's replies to the program's queries go (R15).
    pub replies: Box<dyn Write + Send>,
    /// A running program taken over, not a new one started.
    pub adopted: bool,
    /// On adoption, the snapshot the replay starts after.
    pub snapshot: Option<spyc_jar::proto::Snapshot>,
    /// Where the pane stores new snapshots.
    pub snapshots: SnapshotSink,
}

/// Stores the pane's engine state in its jar, at the jar offset it reflects.
#[derive(Clone)]
pub struct SnapshotSink {
    tx: Sender,
    /// The jar offset of the first byte this pane parsed.
    start: u64,
}

impl SnapshotSink {
    /// `parsed` is how many bytes the pane has parsed since `start`.
    pub fn store(&self, format: u16, parsed: u64, rows: u16, cols: u16, blob: Vec<u8>) {
        let _ = self.tx.send(&Msg::StoreSnapshot(spyc_jar::proto::Snapshot {
            format,
            offset: self.start + parsed,
            rows,
            cols,
            blob,
        }));
    }
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
    let pane_id = env
        .iter()
        .find(|(k, _)| k == "SPYC_PANE_ID")
        .map(|(_, v)| v.as_str());
    sweep_once();
    let mut socket = socket_path(pane_id);
    let state = socket_state(&socket);
    trace(&format!(
        "spawn: pane {pane_id:?} socket {} {state:?}",
        socket.display()
    ));
    match state {
        SocketState::Live => match attach(&socket, rows, cols) {
            Ok(mut adopted) => {
                adopted.adopted = true;
                return Ok(adopted);
            }
            // A live jar that won't take us (another spyc owns it) is left
            // alone; deleting its socket would make it unreachable. This pane
            // gets a jar of its own under a fresh name.
            Err(e) => {
                trace(&format!("spawn: attach refused: {e:#}"));
                socket = socket_path(None);
            }
        },
        SocketState::Stale => {
            let _ = std::fs::remove_file(&socket);
        }
        SocketState::Missing => {}
    }
    let mut cmd = Command::new(jar_binary()?);
    cmd.arg("--socket")
        .arg(&socket)
        .args(["--wire", "1"])
        .arg("--cwd")
        .arg(cwd)
        .arg("--rows")
        .arg(rows.to_string())
        .arg("--cols")
        .arg(cols.to_string())
        .args(
            std::env::var("SPYC_JAR_RING")
                .ok()
                .map(|n| vec!["--ring".to_string(), n])
                .unwrap_or_default(),
        )
        // R32a: a pane waits for its owner however long; this spike knob
        // gives every pane a finite linger instead.
        .args(
            std::env::var("SPYC_JAR_LINGER_MS")
                .ok()
                .map(|n| vec!["--linger-ownerless-ms".to_string(), n])
                .unwrap_or_default(),
        )
        .arg("--")
        .args(argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    // R26: a fresh token, kept where a spyc after a crash finds it, and
    // handed to the jar on its stdin.
    let token = new_token()?;
    save_token(&socket, &token)?;
    let mut jar = cmd.spawn().context("start spyc-jar")?;
    jar.stdin
        .take()
        .context("the jar's stdin")?
        .write_all(&token)
        .context("hand the jar its token")?;
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
    let token = load_token(socket).unwrap_or_default();
    let (client, welcome) = Client::connect(socket, Role::Owner, Duration::from_secs(5), &token)?;
    let Msg::Welcome {
        child_pid,
        base,
        head,
        rows: had_rows,
        cols: had_cols,
        ..
    } = welcome
    else {
        anyhow::bail!("expected a welcome");
    };
    let mut client = client;
    client.send(&Msg::FetchSnapshot)?;
    let mut fetched = Vec::new();
    loop {
        match client.recv(Duration::from_secs(5))? {
            Some(Msg::Snapshot(Some(s))) => fetched.push(s),
            Some(Msg::Snapshot(None)) => break,
            Some(_) => {}
            None => anyhow::bail!("the jar sent no snapshot list"),
        }
    }
    // Replay after a snapshot when there is one this build can use: the binary
    // one if it decodes here (same build), else the portable one; otherwise
    // from the oldest byte the jar holds. `SPYC_JAR_REFUSE_BINARY` plays a
    // different build, for the spike.
    let usable = |s: &spyc_jar::proto::Snapshot| s.offset >= base && s.offset <= head;
    let refuse_binary = std::env::var_os("SPYC_JAR_REFUSE_BINARY").is_some();
    let binary = fetched.iter().find(|s| {
        s.format == spyc_jar::proto::SNAPSHOT_BINARY
            && usable(s)
            && !refuse_binary
            && crate::pane::engine_ghostty::snapshot_decodes(&s.blob)
    });
    let portable = fetched
        .iter()
        .find(|s| s.format == spyc_jar::proto::SNAPSHOT_VT && usable(s));
    let mut snapshot = binary.or(portable).cloned();
    // Stage 4, Q7: across a gap no snapshot is current, since the bytes after
    // it are gone. Start from the newest one anyway and apply what's left
    // after the gap: the program's repaint redraws the screen but doesn't set
    // its modes again, so the snapshot is where alternate screen, mouse
    // reporting and bracketed paste come back from, and the history before
    // the gap too. `SPYC_JAR_GAP=blank` starts from an empty engine instead.
    let gap = snapshot.is_none() && base > 0;
    if gap && !std::env::var("SPYC_JAR_GAP").is_ok_and(|v| v == "blank") {
        let decodes = |s: &&spyc_jar::proto::Snapshot| {
            s.format != spyc_jar::proto::SNAPSHOT_BINARY
                || (!refuse_binary && crate::pane::engine_ghostty::snapshot_decodes(&s.blob))
        };
        snapshot = fetched
            .iter()
            .filter(decodes)
            .max_by_key(|s| (s.offset, s.format == spyc_jar::proto::SNAPSHOT_BINARY))
            .cloned();
    }
    trace(&format!(
        "attach: snapshots {:?}, using {:?}",
        fetched
            .iter()
            .map(|s| (s.format, s.offset))
            .collect::<Vec<_>>(),
        snapshot.as_ref().map(|s| (s.format, s.offset))
    ));
    let replay_from = snapshot.as_ref().map_or(base, |s| s.offset.max(base));
    let (mut rx, tx) = client.split();
    announce(&tx);
    owned().insert(socket.to_path_buf(), (tx.clone(), child_pid));
    tx.send(&Msg::Replay { from: replay_from })?;
    // Fit the jar to this pane. When the log has lost its oldest bytes the
    // replay can't rebuild the screen, so make the program repaint: a resize
    // to the size it already has repaints nothing (stage 0b), so change it
    // by a row and back.
    let resize = |r: u16| Msg::Resize {
        rows: r,
        cols,
        px_w: 0,
        px_h: 0,
    };
    if gap && rows > 1 {
        tx.send(&resize(rows - 1))?;
        tx.send(&resize(rows))?;
    } else if (had_rows, had_cols) != (rows, cols) {
        tx.send(&resize(rows))?;
    }

    // R15a: replaying a tail makes the engine see the program's old queries
    // again. Its answers are dropped until the tail has been handed over, and
    // a grace for the parser after that. A fresh jar has no tail: open at once.
    let gate = Arc::new(ReplayGate {
        live_from: head,
        fresh: head == base,
        caught_up: Mutex::new(None),
    });
    let (bytes_tx, bytes_rx) = mpsc::sync_channel::<Vec<u8>>(64);
    let fg = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let exit: Arc<ExitSlot> = Arc::new((Mutex::new(None), Condvar::new()));
    {
        let tx = tx.clone();
        let exit = Arc::clone(&exit);
        let gate = Arc::clone(&gate);
        let fg = Arc::clone(&fg);
        let socket = socket.to_path_buf();
        std::thread::spawn(move || {
            let mut pings = 0u64;
            let mut parsed = replay_from;
            loop {
                match rx.recv(Duration::from_secs(2)) {
                    Ok(Some(Msg::Output { offset, bytes })) => {
                        let end = offset + bytes.len() as u64;
                        if bytes_tx.send(bytes).is_err() {
                            break;
                        }
                        parsed = end;
                        gate.handed_over(end);
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
                    Ok(Some(Msg::Foreground { pgid })) => {
                        fg.store(pgid, std::sync::atomic::Ordering::Relaxed);
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        pings += 1;
                        let _ = tx.send(&Msg::Ping { n: pings });
                    }
                    // The connection ended with no exit reported. A jar this
                    // spyc handed on is left be.
                    Err(_) if is_released(child_pid) => break,
                    Err(e) => match reconnect(&socket, &tx, parsed, rows, cols) {
                        Ok(next) => {
                            trace(&format!(
                                "reader: reconnected to {} after {e:#}",
                                socket.display()
                            ));
                            rx = next;
                        }
                        Err(lost) => {
                            trace(&format!(
                                "reader: {} {lost:?} after {e:#}",
                                socket.display()
                            ));
                            // Its program now answers to another spyc, or to
                            // none: closing this tab mustn't signal it.
                            released().insert(child_pid);
                            set_exit(&exit, ExitStatus::with_signal(lost.label()));
                            break;
                        }
                    },
                }
            }
            if exit
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_none()
            {
                set_exit(&exit, ExitStatus::with_signal(Lost::Gone.label()));
            }
        });
    }
    Ok(Jarred {
        master: Box::new(JarMaster {
            tx: tx.clone(),
            size: Mutex::new(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            }),
            fg: Arc::clone(&fg),
            reader: Mutex::new(Some(JarReader {
                rx: bytes_rx,
                buf: Vec::new(),
                pos: 0,
            })),
        }),
        replies: Box::new(JarReplies {
            tx: tx.clone(),
            gate,
        }),
        adopted: false,
        snapshot,
        snapshots: SnapshotSink {
            tx: tx.clone(),
            start: replay_from,
        },
        child: Box::new(JarChild {
            pid: child_pid,
            exit,
            tx,
        }),
    })
}

/// R30a: tell the jar where this spyc's MCP socket is, so a pane's proxy and
/// hooks can find their owner after a move, an upgrade or a crash. The jar
/// forgets it when its owner goes.
fn announce(tx: &Sender) {
    if let Some(mcp) = crate::mcp::socket_path() {
        let _ = tx.send(&Msg::SetInfo {
            key: "owner_mcp".into(),
            value: mcp.display().to_string(),
        });
    }
}

/// Why a pane's jar can't be taken back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lost {
    /// Another spyc owns it (decision G): the pane is detached here.
    Held,
    /// The jar is gone, and its program with it.
    Gone,
}

impl Lost {
    /// What the pane's exit status carries, and its tab shows.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Held => "detached",
            Self::Gone => "jar lost",
        }
    }
}

/// Decision G: a spyc whose connection to a pane's jar ended (it was stopped
/// past the jar's deadline, say) takes the jar back when no one else has, and
/// resumes from the first byte its pane hasn't parsed. `tx`, which every
/// half of the pane shares, sends on the new connection from then on.
fn reconnect(
    socket: &Path,
    tx: &Sender,
    parsed: u64,
    rows: u16,
    cols: u16,
) -> Result<spyc_jar::client::Receiver, Lost> {
    if socket_state(socket) != SocketState::Live {
        return Err(Lost::Gone);
    }
    let token = load_token(socket).ok_or(Lost::Held)?;
    let (client, welcome) = Client::connect(socket, Role::Owner, Duration::from_secs(2), &token)
        .map_err(|e| {
            if socket_state(socket) == SocketState::Live {
                trace(&format!("reconnect: refused: {e:#}"));
                Lost::Held
            } else {
                Lost::Gone
            }
        })?;
    let Msg::Welcome { base, .. } = welcome else {
        return Err(Lost::Gone);
    };
    let rx = tx.rebind(client).map_err(|_| Lost::Gone)?;
    announce(tx);
    let _ = tx.send(&Msg::Replay {
        from: parsed.max(base),
    });
    // Bytes the jar dropped meanwhile can't be parsed: make the program
    // repaint, by a row and back.
    if base > parsed && rows > 1 {
        for r in [rows - 1, rows] {
            let _ = tx.send(&Msg::Resize {
                rows: r,
                cols,
                px_w: 0,
                px_h: 0,
            });
        }
    }
    Ok(rx)
}

/// How long after the replayed tail is handed over its queries may still be
/// in the parser.
const REPLY_GRACE: Duration = Duration::from_millis(500);

struct ReplayGate {
    /// The jar's head when this spyc took over: bytes before it are replay.
    live_from: u64,
    /// Nothing to replay: open from the start.
    fresh: bool,
    /// When the tail was handed over in full.
    caught_up: Mutex<Option<Instant>>,
}

impl ReplayGate {
    fn handed_over(&self, end: u64) {
        let mut at = self
            .caught_up
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if at.is_none() && end >= self.live_from {
            *at = Some(Instant::now());
        }
    }

    fn open(&self) -> bool {
        self.fresh
            || self
                .caught_up
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some_and(|t| t.elapsed() >= REPLY_GRACE)
    }
}

/// The terminal's replies, as `Reply` frames, dropped while a replayed tail
/// is still being parsed.
struct JarReplies {
    tx: Sender,
    gate: Arc<ReplayGate>,
}

impl Write for JarReplies {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.gate.open() {
            self.tx.send(&Msg::Reply {
                bytes: buf.to_vec(),
            })?;
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
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
        |id| {
            id.chars()
                .filter(char::is_ascii_hexdigit)
                .take(16)
                .collect()
        },
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
        self.tx.send(&Msg::Input {
            bytes: buf.to_vec(),
        })?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct JarMaster {
    tx: Sender,
    /// The foreground process group the jar last reported (R4); 0 unknown.
    fg: Arc<std::sync::atomic::AtomicU32>,
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
        let reader = self
            .reader
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        Ok(Box::new(reader.context("a jar's output has one reader")?))
    }
    fn take_writer(&self) -> anyhow::Result<Box<dyn Write + Send>> {
        Ok(Box::new(JarWriter {
            tx: self.tx.clone(),
        }))
    }
    fn process_group_leader(&self) -> Option<libc::pid_t> {
        match self.fg.load(std::sync::atomic::Ordering::Relaxed) {
            0 => None,
            pgid => libc::pid_t::try_from(pgid).ok(),
        }
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
        self.tx.send(&Msg::Signal {
            signal: JarSignal::Kill as i32,
        })
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(JarKiller(self.tx.clone()))
    }
}

impl Child for JarChild {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        Ok(self
            .exit
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }
    fn wait(&mut self) -> io::Result<ExitStatus> {
        let mut slot = self.exit.0.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(s) = slot.clone() {
                return Ok(s);
            }
            slot = self
                .exit
                .1
                .wait(slot)
                .unwrap_or_else(PoisonError::into_inner);
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
        self.0.send(&Msg::Signal {
            signal: JarSignal::Kill as i32,
        })
    }
    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(Self(self.0.clone()))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SocketState {
    /// Something answers: a jar, alive.
    Live,
    /// The file is there but nothing listens: its jar is gone (R32).
    Stale,
    Missing,
}

/// Whether a jar answers at `socket`. Only a refused connection or a vanished
/// file counts as stale; any other error is treated as live, so a jar is never
/// cut off by a transient failure.
fn socket_state(socket: &Path) -> SocketState {
    if !socket.exists() {
        return SocketState::Missing;
    }
    let refused = std::os::unix::net::UnixStream::connect(socket).is_err_and(|e| {
        matches!(
            e.kind(),
            io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
        )
    });
    if refused {
        SocketState::Stale
    } else {
        SocketState::Live
    }
}

/// Remove the sockets in `dir` whose jars are gone (R32); how many.
fn sweep_dir(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "sock"))
        .filter(|p| socket_state(p) == SocketState::Stale && std::fs::remove_file(p).is_ok())
        .inspect(|p| forget_token(p))
        .count()
}

/// Sweep the jar directory once per spyc, on its first jar: a spyc that never
/// runs one never looks.
fn sweep_once() {
    static SWEPT: std::sync::Once = std::sync::Once::new();
    SWEPT.call_once(|| {
        if let Some(dir) = socket_path(None).parent() {
            let n = sweep_dir(dir);
            trace(&format!("sweep: {n} stale socket(s) removed"));
        }
    });
}

/// Stage 6: the jars this spyc owns, by socket, for handing them on.
fn owned() -> std::sync::MutexGuard<'static, std::collections::HashMap<PathBuf, (Sender, u32)>> {
    static OWNED: std::sync::OnceLock<Mutex<std::collections::HashMap<PathBuf, (Sender, u32)>>> =
        std::sync::OnceLock::new();
    OWNED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// R26: where the token of the jar at `socket` is kept: one `0600` file per
/// jar in spyc's state directory, which a spyc after a crash reads to take
/// its jars back. Not beside the socket: the runtime directory is the one an
/// agent's sandbox may be denied, and the tokens are what it guards.
fn token_path(socket: &Path) -> Option<PathBuf> {
    let name = socket.file_stem()?;
    Some(crate::state::state_root()?.join("jar-tokens").join(name))
}

/// `bytes` as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// 32 random bytes, as hex.
fn new_token() -> anyhow::Result<Vec<u8>> {
    let mut raw = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut raw))
        .context("read a jar token from /dev/urandom")?;
    Ok(hex(&raw).into_bytes())
}

fn save_token(socket: &Path, token: &[u8]) -> anyhow::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let path = token_path(socket).context("no state directory for jar tokens")?;
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("write {}", path.display()))?;
    f.write_all(token)?;
    Ok(())
}

fn load_token(socket: &Path) -> Option<Vec<u8>> {
    spyc_jar::client::read_token_file(&token_path(socket)?).ok()
}

fn forget_token(socket: &Path) {
    if let Some(p) = token_path(socket) {
        let _ = std::fs::remove_file(p);
    }
}

/// The socket of pane `pane_id`'s jar.
pub fn socket_for_pane(pane_id: &str) -> PathBuf {
    socket_path(Some(pane_id))
}

/// Stage 6 (R22): hand pane `pane_id`'s jar on, held for `token`. `false`
/// when this spyc doesn't own a jar for it.
pub fn release(pane_id: &str, token: &[u8]) -> bool {
    let socket = socket_for_pane(pane_id);
    let Some((tx, pid)) = owned().remove(&socket) else {
        return false;
    };
    released().insert(pid);
    // The jar's token becomes `token`; this spyc's opens nothing from now on.
    forget_token(&socket);
    tx.send(&Msg::Release {
        token: token.to_vec(),
    })
    .is_ok()
}

/// Whether the program `pid` runs in a jar this spyc handed on: closing its
/// tab here mustn't signal it, as closing a tab otherwise does.
pub fn is_released(pid: u32) -> bool {
    released().contains(&pid)
}

fn released() -> std::sync::MutexGuard<'static, std::collections::HashSet<u32>> {
    static RELEASED: std::sync::OnceLock<Mutex<std::collections::HashSet<u32>>> =
        std::sync::OnceLock::new();
    RELEASED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Stage 6: the token pane `pane_id`'s jar was handed to this spyc with,
/// kept as this spyc's own for the jar, for its tab's adoption to present.
pub fn expect_claim(pane_id: &str, token: &[u8]) {
    if let Err(e) = save_token(&socket_for_pane(pane_id), token) {
        trace(&format!("expect_claim: {e:#}"));
    }
}

/// R18: what pane `pane_id`'s jar is, as this spyc knows it, kept in the jar
/// so a spyc that finds it with no session describing it can say. The jar
/// keeps these when its owner goes.
pub fn describe_pane(pane_id: &str, fields: &[(&str, String)]) {
    if let Some((tx, _)) = owned().get(&socket_for_pane(pane_id)) {
        for (key, value) in fields {
            let _ = tx.send(&Msg::SetInfo {
                key: (*key).to_string(),
                value: value.clone(),
            });
        }
    }
}

/// R30a: where pane `pane_id`'s owner's MCP socket is, as its jar says;
/// `None` while it has no owner, or no jar answers.
pub fn owner_mcp(pane_id: &str) -> Option<PathBuf> {
    let socket = socket_for_pane(pane_id);
    if socket_state(&socket) != SocketState::Live {
        return None;
    }
    spyc_jar::client::describe(&socket)
        .ok()?
        .into_iter()
        .find(|(k, v)| k == "owner_mcp" && !v.is_empty())
        .map(|(_, v)| PathBuf::from(v))
}

/// The live jars with no owner that this spyc didn't start or take, each by
/// its label (R18), else its program.
pub fn orphans() -> Vec<String> {
    let Some(dir) = socket_path(None).parent().map(Path::to_path_buf) else {
        return Vec::new();
    };
    let mine: std::collections::HashSet<PathBuf> = owned().keys().cloned().collect();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "sock") && !mine.contains(p))
        .filter(|p| socket_state(p) == SocketState::Live)
        .filter_map(|p| spyc_jar::client::describe(&p).ok())
        .filter(|f| f.iter().any(|(k, v)| k == "owner" && v.starts_with("none")))
        .map(|f| {
            let get = |k: &str| f.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
            match (get("label"), get("project")) {
                (Some(l), Some(p)) => format!("{l} in {p}"),
                (Some(l), None) => l,
                _ => get("program").unwrap_or_default(),
            }
        })
        .collect();
    found.sort();
    found
}

/// `spyc --jars`: every jar in this user's jar directory, as each describes
/// itself (R31), and the sockets whose jars are gone.
pub fn list_jars() {
    let dir = socket_path(None)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let mut socks: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "sock"))
                .collect()
        })
        .unwrap_or_default();
    socks.sort();
    println!("jars in {}:", dir.display());
    if socks.is_empty() {
        println!("  none");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    for sock in socks {
        let name = sock
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if socket_state(&sock) == SocketState::Stale {
            println!("  {name}  stale: its jar is gone");
            continue;
        }
        match spyc_jar::client::describe(&sock) {
            Ok(f) => {
                let get = |k: &str| {
                    f.iter()
                        .find(|(key, _)| key == k)
                        .map_or("", |(_, v)| v.as_str())
                };
                let age = get("created").parse::<u64>().map_or(String::new(), |c| {
                    let s = now.saturating_sub(c);
                    format!("{}h{:02}m", s / 3600, s / 60 % 60)
                });
                let what = match get("label") {
                    "" => get("program").to_string(),
                    label => format!("{label} ({} in {})", get("agent"), get("project")),
                };
                println!(
                    "  {name}  {what}  pid {}  up {age}  owner {}  linger {}  log {}",
                    get("program_pid"),
                    get("owner"),
                    get("linger"),
                    get("log")
                );
            }
            Err(e) => println!("  {name}  no answer ({e:#})"),
        }
    }
}

/// Jar spike diagnostics: append `line` to `$SPYC_JAR_TRACE`.
pub fn trace(line: &str) {
    if let Some(path) = std::env::var_os("SPYC_JAR_TRACE")
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(f, "spyc {} {line}", std::process::id());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// R32: a socket with a live listener is kept, one whose listener is gone
    /// is removed, and anything else in the directory is left alone.
    #[test]
    fn a_sweep_removes_only_the_sockets_nothing_answers() {
        let dir = tempfile::Builder::new()
            .prefix("jsw")
            .tempdir_in("/tmp")
            .unwrap();
        let live = dir.path().join("live.sock");
        let dead = dir.path().join("dead.sock");
        let other = dir.path().join("notes.txt");
        let _listening = UnixListener::bind(&live).unwrap();
        drop(UnixListener::bind(&dead).unwrap());
        std::fs::write(&other, "x").unwrap();
        // A child another test forks meanwhile holds a copy of the listener
        // until it `exec`s.
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while socket_state(&dead) != SocketState::Stale {
            assert!(
                std::time::Instant::now() < until,
                "premise: nothing answers"
            );
            std::thread::sleep(Duration::from_millis(20));
        }

        assert_eq!(sweep_dir(dir.path()), 1);
        assert!(live.exists(), "a live jar's socket is kept");
        assert!(!dead.exists(), "a dead jar's socket is removed");
        assert!(other.exists(), "only sockets are swept");
        assert_eq!(socket_state(&dead), SocketState::Missing);
    }
}

/// Jar spike: spyc's link to the jar it runs in as the program (shape 2), when
/// it does. Taken by the run loop once it can wake.
static OUTER: Mutex<Option<Client>> = Mutex::new(None);

/// Whether spyc is the program of a jar: `SPYC_JAR_SOCKET` names one, and it
/// takes spyc as its program, which it knows by spyc's pid. A spyc run in a
/// shell inside a pane's jar is refused, and isn't one.
pub fn link_outer_jar() -> bool {
    let Some(socket) = std::env::var_os("SPYC_JAR_SOCKET") else {
        return false;
    };
    match Client::connect(Path::new(&socket), Role::Program, Duration::from_millis(500), b"") {
        Ok((client, _)) => {
            *OUTER.lock().unwrap_or_else(PoisonError::into_inner) = Some(client);
            true
        }
        Err(e) => {
            trace(&format!("outer jar: not its program: {e:#}"));
            false
        }
    }
}

/// The outer jar's sender, while spyc is its program.
static OUTER_TX: Mutex<Option<Sender>> = Mutex::new(None);

/// Hear the outer jar's attaches on a thread of their own: each one calls
/// `attached`.
pub fn watch_outer_jar(attached: impl Fn() + Send + 'static) {
    let Some(client) = OUTER.lock().unwrap_or_else(PoisonError::into_inner).take() else {
        return;
    };
    let (mut rx, tx) = client.split();
    *OUTER_TX.lock().unwrap_or_else(PoisonError::into_inner) = Some(tx);
    std::thread::spawn(move || {
        loop {
            match rx.recv(Duration::from_secs(60)) {
                Ok(Some(Msg::Attached)) => attached(),
                Ok(_) => {}
                Err(_) => return,
            }
        }
    });
}

/// Ask the outer jar to detach its client, once what spyc wrote has reached
/// it. `false` when spyc isn't in one.
pub fn detach_outer_jar() -> bool {
    OUTER_TX
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|tx| tx.send(&Msg::Detach).is_ok())
}

/// Whether spyc is its outer jar's program, linked and listening.
pub fn in_outer_jar() -> bool {
    OUTER_TX
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some()
}
