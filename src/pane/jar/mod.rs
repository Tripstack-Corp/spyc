//! A pane tab's program in a `spyc-jar` (#1099, `[pane] jars`): the jar holds
//! the program and its PTY, and outlives spyc. `PtyHost` drives it through
//! [`Jarred`]'s stand-ins for the PTY master and the child.
//!
//! - [`place`]: where the sockets, tokens and the binary are, and the sweep of
//!   sockets whose jars are gone (R32).
//! - `tokens`: each jar's token, on its standard input and in a `0600` file
//!   (R26).
//! - `link`: the connection, its reader thread and the `portable-pty` traits.
//! - `offsets`: the jar offset of what the pane has parsed (finding 8).
//! - [`snapshots`]: when a pane's engine is snapshotted, and its storing.
//! - [`store`]: the jar binaries every jar starts from (decision L).
//!
//! A jar that can't be started, or that ends before it answers (one that
//! doesn't speak this spyc's wire version exits with 75 first), leaves the
//! program to run without one: a tab never fails to open for want of a jar
//! (finding 1).
//!
//! A restored tab whose jar still runs takes it back, its program untouched:
//! the pane starts from the jar's snapshot and the jar replays what the
//! program printed since. A jar that won't take it (another spyc owns it, or
//! this one has no token for it) leaves the tab `[detached]`, and nothing is
//! started in its place (finding 7).

mod link;
mod offsets;
pub mod place;
pub mod snapshots;
pub mod store;
mod tokens;

#[cfg(test)]
mod tests;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use anyhow::Context;
use portable_pty::{Child, MasterPty, PtySize};
use spyc_jar::client::{Client, Refusal};
use spyc_jar::proto::{JarSignal, Msg, Role, Snapshot};
use std::path::PathBuf;

pub use link::Lost;
pub use place::Place;
pub use snapshots::{SnapshotSink, Snapshotter};

use link::{JarChild, JarMaster, JarReader, JarReplies, Link, ReplayGate};
use place::{SocketState, socket_state};

/// Whether this build reads `[pane] jars`: until jars are on by default, a
/// release runs every program as before, and only a `-CURRENT` or rc build
/// may opt in.
pub fn honoured() -> bool {
    honoured_by(env!("CARGO_PKG_VERSION"))
}

fn honoured_by(version: &str) -> bool {
    version.contains("-CURRENT") || version.contains("-rc.")
}

/// The jar for the tab `tab` when `[pane] jars` is `on`: `None` when it's
/// off, this build doesn't read it, or there's nowhere to put one (said on
/// spyc's standard error). `restoring` is the saved tab a restore opens, whose
/// jar, still running, is taken back.
pub fn wanted(
    on: bool,
    tab: &str,
    restoring: Option<&crate::state::sessions::SavedTab>,
) -> Option<Spec> {
    if !on || !honoured() {
        return None;
    }
    Place::for_this_user()
        .inspect_err(|e| eprintln!("spyc: no jar ({e:#}); the tab's program runs without one"))
        .ok()
        .map(|place| Spec {
            place,
            name: tab.to_string(),
            adopt: None,
        })
        .map(|spec| match restoring {
            Some(saved) => spec.restoring(saved.jar_socket.clone()),
            None => spec,
        })
}

/// A pane's jar, asked for: where, and the tab's id, which names its socket.
/// A tab's jar that still runs when the tab starts again is waited for, as a
/// restart's ends a moment after its program; one that goes on running (its
/// spyc crashed) keeps the tab from opening, so its program never runs twice.
pub struct Spec {
    pub place: Place,
    pub name: String,
    /// A restored tab's: the socket its jar was at, whose jar, still running,
    /// it takes back rather than start its program again.
    pub adopt: Option<PathBuf>,
}

impl Spec {
    /// This spec for a restored tab whose jar was at `saved`, or, on a save
    /// that didn't record it, at its own name's socket.
    #[must_use]
    pub fn restoring(mut self, saved: Option<PathBuf>) -> Self {
        self.adopt = Some(saved.unwrap_or_else(|| self.place.socket(&self.name)));
        self
    }
}

/// A jarred program, as `PtyHost` drives it.
pub struct Jarred {
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    /// Where the terminal's replies to the program's queries go (R15).
    pub replies: Box<dyn Write + Send>,
    /// `None` for a detached tab, whose jar answers to someone else.
    pub signals: Option<Signals>,
    pub snapshots: Option<SnapshotSink>,
    /// The jar's socket, which a saved session records.
    pub socket: PathBuf,
    /// Set when a running program was taken back rather than started.
    pub adoption: Option<Adoption>,
}

/// A running program taken back from its jar.
pub struct Adoption {
    /// The snapshot the pane's engine starts from; the replay begins at its
    /// offset. `None`: the replay begins at the oldest byte the jar holds.
    pub snapshot: Option<Snapshot>,
}

/// Signals to a jarred program's group, which only its jar sends.
#[derive(Clone)]
pub struct Signals(Arc<Link>);

impl Signals {
    pub fn send(&self, signal: JarSignal) -> std::io::Result<()> {
        self.0.signal(signal)
    }

    /// End the connection as a crash would, signalling nothing.
    #[cfg(test)]
    pub fn sever(&self) {
        self.0.out.close();
    }
}

/// How long a tab's last jar has to end once its program has: the jar goes
/// once its owner acknowledges the exit, within its own 200 ms tick.
const ENDING_WAIT: Duration = Duration::from_secs(2);

/// The state of the jar at `socket`, once it has ended or `wait` has passed.
fn ended_within(socket: &Path, wait: Duration) -> SocketState {
    let until = Instant::now() + wait;
    loop {
        let state = socket_state(socket);
        if state != SocketState::Live || Instant::now() >= until {
            return state;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// How long a new jar has to answer.
const ANSWER_WAIT: Duration = Duration::from_secs(5);
/// An owner with nothing to send pings this often, inside the jar's deadline.
const PING_EVERY: Duration = Duration::from_secs(2);
/// A jar whose socket takes no bytes for this long has stopped: the jar's own
/// deadline for its owner.
const WRITE_STALL: Duration = Duration::from_secs(10);

/// Start a jar running `argv` in `cwd` at `rows` x `cols`, with `env` added
/// to spyc's own, and take it as its owner. `Ok(None)`: no jar could be
/// started, and the caller runs the program itself.
pub fn spawn(
    spec: &Spec,
    argv: &[String],
    cwd: &Path,
    (rows, cols): (u16, u16),
    env: &[(String, String)],
) -> anyhow::Result<Option<Jarred>> {
    let place = &spec.place;
    if let Err(e) = place.prepare() {
        eprintln!("spyc: no jar ({e:#}); the tab's program runs without one");
        return Ok(None);
    }
    sweep_once(place);
    if let Some(at) = &spec.adopt
        && socket_state(at) == SocketState::Live
    {
        return adopt(place, at, (rows, cols)).map(Some);
    }
    let socket = place.socket(&spec.name);
    match ended_within(&socket, ENDING_WAIT) {
        SocketState::Live => anyhow::bail!(
            "this tab's jar still runs at {}; its program wasn't started twice",
            socket.display()
        ),
        SocketState::Unknown => anyhow::bail!(
            "can't tell whether this tab's last jar still runs at {}; its program wasn't started twice",
            socket.display()
        ),
        SocketState::Stale => {
            std::fs::remove_file(&socket)
                .with_context(|| format!("remove {}", socket.display()))?;
            tokens::forget(place, &socket);
        }
        SocketState::Missing => {}
    }
    if let Some(entry) = place.binary.parent()
        && spyc_jar::store::store_of(&place.binary).is_some()
    {
        store::touch(entry);
    }
    let token = tokens::new()?;
    tokens::save(place, &socket, &token)?;
    let mut cmd = Command::new(&place.binary);
    cmd.args(jar_args(&socket, cwd, (rows, cols), argv))
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut jar = match cmd.spawn() {
        Ok(jar) => jar,
        Err(e) => {
            tokens::forget(place, &socket);
            eprintln!(
                "spyc: no jar ({} didn't start: {e}); the tab's program runs without one",
                place.binary.display()
            );
            return Ok(None);
        }
    };
    // SPYC-TRAP(jar-token-stdin): the token goes on the jar's standard input,
    // closed once written, never in its arguments or environment.
    if let Some(mut stdin) = jar.stdin.take() {
        let _ = stdin.write_all(&token);
    }
    let (client, welcome) = match answer(&mut jar, &socket, &token) {
        Ok(Some(answered)) => answered,
        Ok(None) => {
            tokens::forget(place, &socket);
            return Ok(None);
        }
        Err(e) => {
            let _ = jar.kill();
            let _ = jar.wait();
            tokens::forget(place, &socket);
            return Err(e);
        }
    };
    // Reaped here while spyc runs; once spyc is gone, by whoever inherits it.
    std::thread::spawn(move || {
        let _ = jar.wait();
    });
    let Msg::Welcome {
        child_pid, base, ..
    } = welcome
    else {
        anyhow::bail!("expected a welcome, got {welcome:?}");
    };
    let from = Resume {
        pid: child_pid,
        replay: base,
        offsets: offsets::Offsets::new(base),
        gate: ReplayGate::open(),
    };
    connect(client, from, &socket, (rows, cols)).map(Some)
}

/// Take back the running jar at `socket`, as a restored tab's. A jar that
/// won't take this spyc leaves the tab detached.
fn adopt(place: &Place, socket: &Path, (rows, cols): (u16, u16)) -> anyhow::Result<Jarred> {
    let detached = |why: &str| {
        eprintln!("spyc: a restored tab's jar runs on without it ({why}); the tab is detached");
        Ok(link::detached(socket, (rows, cols)))
    };
    let Some(token) = tokens::load(place, socket) else {
        return detached("this spyc has no token for it");
    };
    let (mut client, welcome) = match Client::connect(socket, Role::Owner, ANSWER_WAIT, &token) {
        Ok(answered) => answered,
        Err(e) => return detached(&format!("{e:#}")),
    };
    let Msg::Welcome {
        child_pid,
        base,
        head,
        rows: had_rows,
        cols: had_cols,
        ..
    } = welcome
    else {
        return detached(&format!("expected a welcome, got {welcome:?}"));
    };
    client.send(&Msg::FetchSnapshot)?;
    let mut fetched = Vec::new();
    loop {
        match client.recv(ANSWER_WAIT)? {
            Some(Msg::Snapshot(Some(s))) => fetched.push(s),
            Some(Msg::Snapshot(None)) => break,
            Some(_) => {}
            None => anyhow::bail!("{} listed no snapshots", socket.display()),
        }
    }
    let snapshot = choose(&fetched, head, |blob| {
        snapshots::unstamp(blob).is_some_and(crate::pane::engine_ghostty::snapshot_decodes)
    })
    .cloned();
    // What the pane holds reaches the snapshot's offset; the jar holds bytes
    // from `base`. Past a gap the first byte handed over follows a cut.
    let from = snapshot.as_ref().map_or(base, |s| s.offset);
    let offsets = if snapshot.is_none() && base > 0 {
        offsets::Offsets::after_cut(base)
    } else {
        offsets::Offsets::new(from)
    };
    let replay = from.max(base);
    let resume = Resume {
        pid: child_pid,
        replay,
        offsets,
        gate: ReplayGate::until(head),
    };
    let jarred = connect(client, resume, socket, (rows, cols))?;
    if (had_rows, had_cols) != (rows, cols) {
        jarred.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
    }
    Ok(Jarred {
        adoption: Some(Adoption { snapshot }),
        ..jarred
    })
}

/// R17b and R12c: the snapshot an adoption starts from. The newest this
/// build can use, binary before portable at one offset; one older than the
/// jar's oldest byte too, since past a gap it's still where the program's
/// modes and history come back from.
fn choose(fetched: &[Snapshot], head: u64, decodes: impl Fn(&[u8]) -> bool) -> Option<&Snapshot> {
    use spyc_jar::proto::{SNAPSHOT_BINARY, SNAPSHOT_VT};
    fetched
        .iter()
        .filter(|s| s.offset <= head)
        .filter(|s| match s.format {
            SNAPSHOT_BINARY => decodes(&s.blob),
            SNAPSHOT_VT => true,
            _ => false,
        })
        .max_by_key(|s| (s.offset, s.format == SNAPSHOT_BINARY))
}

/// Where a connection's stream begins, and what the pane already holds.
struct Resume {
    pid: u32,
    replay: u64,
    offsets: offsets::Offsets,
    gate: ReplayGate,
}

/// How long a pane's jar holds its program for this spyc to connect
/// (decision M): far longer than the connect takes, and a jar whose spyc
/// died meanwhile ends having started nothing.
const START_WAIT: Duration = Duration::from_secs(30);

/// A pane's jar's arguments: its socket, wire version and size, its program
/// held for its first owner (decision M), then the program.
fn jar_args(
    socket: &Path,
    cwd: &Path,
    (rows, cols): (u16, u16),
    argv: &[String],
) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = vec!["--socket".into(), socket.into()];
    for (flag, value) in [
        ("--wire", spyc_jar::proto::VERSION.to_string()),
        ("--rows", rows.to_string()),
        ("--cols", cols.to_string()),
        ("--start-on-owner", START_WAIT.as_millis().to_string()),
    ] {
        args.push(flag.into());
        args.push(value.into());
    }
    args.push("--cwd".into());
    args.push(cwd.into());
    args.push("--".into());
    args.extend(argv.iter().map(Into::into));
    args
}

/// The new jar's welcome. `Ok(None)` when it ended first, which it says on
/// spyc's standard error.
fn answer(
    jar: &mut std::process::Child,
    socket: &Path,
    token: &[u8],
) -> anyhow::Result<Option<(Client, Msg)>> {
    let until = Instant::now() + ANSWER_WAIT;
    loop {
        match Client::connect(socket, Role::Owner, Duration::from_millis(50), token) {
            Ok(answered) => return Ok(Some(answered)),
            Err(e) => {
                if let Some(status) = jar.try_wait().context("wait for spyc-jar")? {
                    let why = if status.code() == Some(spyc_jar::cli::WIRE_NOT_SERVED) {
                        format!("it doesn't speak wire version {}", spyc_jar::proto::VERSION)
                    } else {
                        format!("it ended: {status}")
                    };
                    eprintln!("spyc: no jar ({why}); the tab's program runs without one");
                    return Ok(None);
                }
                if e.downcast_ref::<Refusal>().is_some() || Instant::now() >= until {
                    return Err(e.context(format!("start a jar at {}", socket.display())));
                }
            }
        }
    }
}

/// Take a jar that welcomed this spyc as its owner, streaming from `from`.
fn connect(
    client: Client,
    from: Resume,
    socket: &Path,
    (rows, cols): (u16, u16),
) -> anyhow::Result<Jarred> {
    let (rx, out) = client.split_queued(PING_EVERY, WRITE_STALL)?;
    out.send(&Msg::Replay { from: from.replay })?;
    let link = Arc::new(Link {
        out,
        pid: from.pid,
        fg: std::sync::atomic::AtomicU32::new(0),
        exit: Mutex::new(None),
        exited: Condvar::new(),
        offsets: Mutex::new(from.offsets),
        gate: from.gate,
    });
    let (bytes_tx, bytes_rx) = mpsc::sync_channel::<Vec<u8>>(64);
    {
        let link = Arc::clone(&link);
        let socket = socket.to_path_buf();
        std::thread::Builder::new()
            .name("jar-reader".into())
            .spawn(move || link::read_frames(rx, &link, &bytes_tx, &socket))?;
    }
    Ok(Jarred {
        master: Box::new(JarMaster {
            link: Arc::clone(&link),
            size: Mutex::new(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            }),
            reader: Mutex::new(Some(JarReader {
                rx: bytes_rx,
                buf: Vec::new(),
                pos: 0,
            })),
        }),
        child: Box::new(JarChild(Arc::clone(&link))),
        replies: Box::new(JarReplies(Arc::clone(&link))),
        signals: Some(Signals(Arc::clone(&link))),
        snapshots: Some(SnapshotSink(link)),
        socket: socket.to_path_buf(),
        adoption: None,
    })
}

/// Sweep the socket directory once per spyc, on its first jar: a spyc that
/// never runs one never looks.
fn sweep_once(place: &Place) {
    static SWEPT: std::sync::Once = std::sync::Once::new();
    SWEPT.call_once(|| {
        let n = place::sweep(place, place::SWEEP_AFTER);
        crate::spyc_debug!("jar sweep: {n} stale socket(s) removed");
    });
}
