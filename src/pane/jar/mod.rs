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
//!
//! A jar that can't be started, or that ends before it answers (one that
//! doesn't speak this spyc's wire version exits with 75 first), leaves the
//! program to run without one: a tab never fails to open for want of a jar
//! (finding 1).

mod link;
mod offsets;
pub mod place;
pub mod snapshots;
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
use spyc_jar::proto::{JarSignal, Msg, Role};

pub use link::Lost;
pub use place::Place;
pub use snapshots::{SnapshotSink, Snapshotter};

use link::{JarChild, JarMaster, JarReader, JarReplies, Link};
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
/// spyc's standard error).
pub fn wanted(on: bool, tab: &str) -> Option<Spec> {
    if !on || !honoured() {
        return None;
    }
    Place::for_this_user()
        .inspect_err(|e| eprintln!("spyc: no jar ({e:#}); the tab's program runs without one"))
        .ok()
        .map(|place| Spec {
            place,
            name: tab.to_string(),
        })
}

/// A pane's jar, asked for: where, and the tab's id, which its socket's name
/// begins with. The rest of the name is new each time, so a restarted tab's
/// jar never meets the last one's, still ending.
pub struct Spec {
    pub place: Place,
    pub name: String,
}

/// A jarred program, as `PtyHost` drives it.
pub struct Jarred {
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    /// Where the terminal's replies to the program's queries go (R15).
    pub replies: Box<dyn Write + Send>,
    pub signals: Signals,
    pub snapshots: SnapshotSink,
}

/// Signals to a jarred program's group, which only its jar sends.
#[derive(Clone)]
pub struct Signals(Arc<Link>);

impl Signals {
    pub fn send(&self, signal: JarSignal) -> std::io::Result<()> {
        self.0.signal(signal)
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
    let tab: String = spec
        .name
        .chars()
        .filter(char::is_ascii_hexdigit)
        .take(8)
        .collect();
    let socket = place.socket(&format!("{tab}{}", tokens::random_hex(4)?));
    match socket_state(&socket) {
        SocketState::Live => anyhow::bail!(
            "a jar already runs at {}; its program wasn't started twice",
            socket.display()
        ),
        SocketState::Stale => {
            std::fs::remove_file(&socket)
                .with_context(|| format!("remove {}", socket.display()))?;
            tokens::forget(place, &socket);
        }
        SocketState::Missing => {}
    }
    let token = tokens::new()?;
    tokens::save(place, &socket, &token)?;
    let mut cmd = Command::new(&place.binary);
    cmd.arg("--socket")
        .arg(&socket)
        .args(["--wire", &spyc_jar::proto::VERSION.to_string()])
        .arg("--cwd")
        .arg(cwd)
        .args(["--rows", &rows.to_string(), "--cols", &cols.to_string()])
        .arg("--")
        .args(argv)
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
    connect(client, &welcome, &socket, (rows, cols)).map(Some)
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

/// Take a jar that welcomed this spyc as its owner, from its oldest byte.
fn connect(
    client: Client,
    welcome: &Msg,
    socket: &Path,
    (rows, cols): (u16, u16),
) -> anyhow::Result<Jarred> {
    let Msg::Welcome {
        child_pid, base, ..
    } = *welcome
    else {
        anyhow::bail!("expected a welcome, got {welcome:?}");
    };
    let (rx, out) = client.split_queued(PING_EVERY, WRITE_STALL)?;
    out.send(&Msg::Replay { from: base })?;
    let link = Arc::new(Link {
        out,
        pid: child_pid,
        fg: std::sync::atomic::AtomicU32::new(0),
        exit: Mutex::new(None),
        exited: Condvar::new(),
        offsets: Mutex::new(offsets::Offsets::new(base)),
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
        signals: Signals(Arc::clone(&link)),
        snapshots: SnapshotSink(link),
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
