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
//! - `repaint`: the program made to draw its screen again after a gap (R12c).
//! - [`snapshots`]: when a pane's engine is snapshotted, and its storing.
//! - [`store`]: the jar binaries every jar starts from (decision L).
//!
//! With jars on, a tab's program runs in a jar or not at all: a jar that
//! can't be started, ends before it answers (one that doesn't speak this
//! spyc's wire version exits with 75 first) or doesn't answer while it holds
//! the program fails the tab, and nothing runs (SPYC-TRAP `jar-or-nothing`).
//! One with no usable `spyc-jar` fails with [`NoUsableJar`], and spyc offers
//! to run tabs without jars (`app::jarless`).
//! Every wait here blocks, so the loop never makes one: a tab's jar is
//! started or taken back on a worker (`app::pane_starts`).
//!
//! A restored tab whose jar still runs takes it back, its program untouched:
//! the pane starts from the jar's snapshot and the jar replays what the
//! program printed since. A jar that won't take it (another spyc owns it, or
//! this one has no token for it) leaves the tab `[detached]`, and nothing is
//! started in its place (finding 7).

mod link;
mod offsets;
pub mod place;
mod repaint;
pub mod snapshots;
pub mod store;
mod tokens;

#[cfg(test)]
mod tests;

use std::io::Write;
use std::os::unix::process::CommandExt;
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

/// No `spyc-jar` this session can start: none beside spyc, one that can't
/// run, or another build's. Jars on, a tab fails with it, and spyc offers to
/// run tabs without jars (`app::jarless`).
#[derive(Debug)]
pub struct NoUsableJar(pub String);

impl std::fmt::Display for NoUsableJar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no usable spyc-jar: {}. Check spyc's installation: spyc-jar belongs beside spyc",
            self.0
        )
    }
}

impl std::error::Error for NoUsableJar {}

/// Why a tab's start failed for no usable jar, when it did.
pub fn no_usable_jar(e: &anyhow::Error) -> Option<&str> {
    e.downcast_ref::<NoUsableJar>().map(|n| n.0.as_str())
}

/// Whether tabs run in jars when no file of the user's says: yes, but not in
/// a unit test, whose tabs would start jars in the user's own runtime
/// directory. A test that wants jars turns them on, in a place of its own.
pub const BY_DEFAULT: bool = by_default(cfg!(test));

const fn by_default(unit_test: bool) -> bool {
    !unit_test
}

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
/// off or this build doesn't read it, and an error when there's no jar to
/// start ([`NoUsableJar`]) or nowhere to put one. `restoring` is the saved
/// tab a restore opens, whose jar, still running, is taken back.
pub fn wanted(
    on: bool,
    tab: &str,
    restoring: Option<&crate::state::sessions::SavedTab>,
) -> anyhow::Result<Option<Spec>> {
    if !on || !honoured() {
        return Ok(None);
    }
    Ok(Some(spec_in(Place::for_this_user()?, tab, restoring)))
}

/// [`wanted`] with jars on, for a worker: the store's check of the first jar
/// may still be running. In `place`, or this user's when `None`.
pub fn spec_for(
    place: Option<Place>,
    tab: &str,
    restoring: Option<&crate::state::sessions::SavedTab>,
) -> anyhow::Result<Option<Spec>> {
    match place {
        Some(place) => Ok(Some(spec_in(place, tab, restoring))),
        None => wanted(true, tab, restoring),
    }
}

fn spec_in(place: Place, tab: &str, restoring: Option<&crate::state::sessions::SavedTab>) -> Spec {
    let spec = Spec {
        place,
        name: tab.to_string(),
        adopt: None,
        retake: false,
    };
    match restoring {
        Some(saved) => spec.restoring(saved.jar_socket.clone()),
        None => spec,
    }
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
    /// A live tab's whose jar ended its lease: its jar is taken back, or the
    /// tab ends, its program never started afresh.
    pub retake: bool,
}

impl Spec {
    /// This spec for a restored tab whose jar was at `saved`, or, on a save
    /// that didn't record it, at its own name's socket.
    #[must_use]
    pub fn restoring(mut self, saved: Option<PathBuf>) -> Self {
        self.adopt = Some(saved.unwrap_or_else(|| self.place.socket(&self.name)));
        self
    }

    /// This spec for a live tab whose jar at `socket` ended its lease.
    #[must_use]
    pub fn retaking(mut self, socket: PathBuf) -> Self {
        self.adopt = Some(socket);
        self.retake = true;
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
    /// The jar's socket, which a saved session records: `None` for a tab
    /// whose jar hasn't been started.
    pub socket: Option<PathBuf>,
    /// Set when a running program was taken back rather than started.
    pub adoption: Option<Adoption>,
}

/// A running program taken back from its jar.
pub struct Adoption {
    /// The snapshot the pane's engine starts from; the replay begins at its
    /// offset. `None`: the replay begins at the oldest byte the jar holds.
    pub snapshot: Option<Snapshot>,
}

/// A tab's stand-ins while its jar starts or is taken back: `note` on its
/// screen, what's typed dropped, open until spyc replaces them. `socket` is a
/// restored tab's jar, which a session saved meanwhile still records.
pub fn starting(note: &str, socket: Option<&Path>, size: (u16, u16)) -> Jarred {
    link::stand_in(as_screen(note).as_bytes(), socket, None, size)
}

/// A tab with no program here: `why` on its screen, the tab ended as `lost`
/// labels it.
pub fn ended(why: &str, lost: Lost, size: (u16, u16)) -> Jarred {
    link::stand_in(as_screen(why).as_bytes(), None, Some(lost), size)
}

/// `text`'s lines as a terminal shows them.
fn as_screen(text: &str) -> String {
    text.replace('\n', "\r\n")
}

/// Signals to a jarred program's group, which only its jar sends.
#[derive(Clone)]
pub struct Signals(Arc<Link>);

impl Signals {
    pub fn send(&self, signal: JarSignal) -> std::io::Result<()> {
        self.0.signal(signal)
    }

    /// End the connection as a crash would, signalling nothing.
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

/// How long a running jar has to welcome this spyc and list its snapshots:
/// past the longest it answers nothing, parked for an upgrade.
const ADOPT_WAIT: Duration = Duration::from_secs(20);
/// An owner with nothing to send pings this often, inside the jar's deadline.
const PING_EVERY: Duration = Duration::from_secs(2);
/// A jar whose socket takes no bytes for this long has stopped, and its
/// connection is ended: past the longest it reads nothing, parked for an
/// upgrade.
const WRITE_STALL: Duration = Duration::from_secs(20);

/// Whether a wait on a jar outlasts the longest it can be silent, so a busy
/// jar is never taken for a gone one (finding 7).
const fn outwaits_a_parked_jar(wait: Duration) -> bool {
    wait.as_millis() > spyc_jar::limits::PARKED_AT_MOST.as_millis()
}

// SPYC-TRAP(jar-outwait): a wait whose end takes a jar for gone outlasts the
// jar's longest silence; an owner is heard from inside the jar's deadline.
const _: () = {
    assert!(outwaits_a_parked_jar(ADOPT_WAIT));
    assert!(outwaits_a_parked_jar(WRITE_STALL));
    assert!(PING_EVERY.as_millis() < spyc_jar::limits::OWNER_DEADLINE.as_millis());
};

/// Start a jar running `argv` in `cwd` at `rows` x `cols`, with `env` added
/// to spyc's own, and take it as its owner.
///
/// SPYC-TRAP(jar-or-nothing): with jars on, a tab's program runs in a jar or
/// not at all. A jar that can't be started is the tab's failure, said in the
/// error, never a program run another way that would die with spyc.
pub fn spawn(
    spec: &Spec,
    command: &str,
    argv: &[String],
    cwd: &Path,
    (rows, cols): (u16, u16),
    env: &[(String, String)],
) -> anyhow::Result<Jarred> {
    let place = &spec.place;
    place.prepare().context("no place for the tab's jar")?;
    sweep_once(place);
    if let Some(at) = &spec.adopt
        && socket_state(at) == SocketState::Live
    {
        return adopt(place, at, (rows, cols));
    }
    if spec.retake {
        anyhow::bail!("this tab's jar has gone, and its program with it");
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
    // What `ps` shows: the program's name and the wire version. Everything
    // else goes on its standard input.
    cmd.arg0(jar_name(command, argv.first().map_or("", String::as_str)))
        .args(["--wire", &spyc_jar::proto::VERSION.to_string()])
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut jar = match cmd.spawn() {
        Ok(jar) => jar,
        Err(e) => {
            tokens::forget(place, &socket);
            let why = format!("the tab's jar, {}, didn't start", place.binary.display());
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
            ) {
                return Err(NoUsableJar(format!("{why}: {e}")).into());
            }
            return Err(e).context(why);
        }
    };
    // SPYC-TRAP(jar-token-stdin): the token goes on the jar's standard input,
    // closed once written, never in its arguments or environment. A long
    // program is written on a thread: a jar that never reads it can't hold
    // this start past `answer`'s wait.
    if let Some(mut stdin) = jar.stdin.take() {
        let input = spyc_jar::cli::Start {
            token: token.clone(),
            socket: socket.clone(),
            cwd: Some(cwd.to_path_buf()),
            rows,
            cols,
            hold: Some(place.hold),
            argv: argv.to_vec(),
        }
        .encode();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let (client, welcome) = match answer(&mut jar, &socket, &token, place.hold) {
        Ok(answered) => answered,
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
    connect(client, from, &socket, (rows, cols), place.repaint_hold)
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
    let (mut client, welcome) = match Client::connect(socket, Role::Owner, ADOPT_WAIT, &token) {
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
        match client.recv(ADOPT_WAIT)? {
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
    let jarred = connect(client, resume, socket, (rows, cols), place.repaint_hold)?;
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

/// The jar's name in `ps`: `spyc-jar` and the program the tab runs, `command`'s
/// first word or, for a tab with none, `shell`, without its directory.
fn jar_name(command: &str, shell: &str) -> String {
    let words = shlex::split(command)
        .unwrap_or_else(|| command.split_whitespace().map(String::from).collect());
    let assignment = |w: &str| {
        w.split_once('=').is_some_and(|(k, _)| {
            !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    };
    let word = words
        .iter()
        .map(String::as_str)
        .find(|w| !matches!(*w, "exec" | "env" | "command") && !assignment(w))
        .unwrap_or(shell);
    let program: String = word
        .rsplit('/')
        .next()
        .unwrap_or(word)
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .take(32)
        .collect();
    if program.is_empty() {
        "spyc-jar".into()
    } else {
        format!("spyc-jar {program}")
    }
}

/// The new jar's welcome, or why there is none: it ended first, or didn't
/// answer within `wait`, the time it holds its program.
fn answer(
    jar: &mut std::process::Child,
    socket: &Path,
    token: &[u8],
    wait: Duration,
) -> anyhow::Result<(Client, Msg)> {
    let until = Instant::now() + wait;
    loop {
        match Client::connect(socket, Role::Owner, Duration::from_millis(50), token) {
            Ok(answered) => return Ok(answered),
            Err(e) => {
                if let Some(status) = jar.try_wait().context("wait for spyc-jar")? {
                    if status.code() == Some(spyc_jar::cli::WIRE_NOT_SERVED) {
                        return Err(NoUsableJar(format!(
                            "the tab's jar doesn't speak wire version {}",
                            spyc_jar::proto::VERSION
                        ))
                        .into());
                    }
                    anyhow::bail!("the tab's jar ended before it answered: {status}");
                }
                if e.downcast_ref::<Refusal>().is_some() {
                    return Err(e.context(format!("start a jar at {}", socket.display())));
                }
                if Instant::now() >= until {
                    return Err(e.context(format!(
                        "the tab's jar didn't answer within {} s",
                        wait.as_secs()
                    )));
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
    repaint_hold: Duration,
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
        size: Mutex::new(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }),
    });
    let (bytes_tx, bytes_rx) = mpsc::sync_channel::<Vec<u8>>(64);
    {
        let link = Arc::clone(&link);
        let socket = socket.to_path_buf();
        std::thread::Builder::new()
            .name("jar-reader".into())
            .spawn(move || link::read_frames(rx, &link, &bytes_tx, &socket, repaint_hold))?;
    }
    Ok(Jarred {
        master: Box::new(JarMaster {
            link: Arc::clone(&link),
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
        socket: Some(socket.to_path_buf()),
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
