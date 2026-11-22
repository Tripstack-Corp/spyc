//! A session's own jar (S4, decision D): the jar spyc itself runs in, so a
//! spyc outlives the terminal it was started from. It is a jar like a pane's,
//! kept apart by where it lives: `<runtime>/spyc/sessions/<SESSION_NAME>.sock`,
//! beside the pane jars' directory, named by the session it holds. Its name is
//! its address, so a spyc in its own jar takes its session's name from it.
//!
//! spyc is in its own jar when `SPYC_JAR_SOCKET` names a socket in that
//! directory and the jar takes spyc as its program, which the jar knows by
//! spyc's pid. Every pane's program sees `SPYC_JAR_SOCKET` too, naming its own
//! jar: a spyc that is a pane's program sees a pane jar's socket and runs as a
//! plain spyc, and the pid check refuses a spyc started from a shell that
//! inherited a session jar's.
//!
//! The jar tells its program when a terminal attaches and when it leaves.
//! [`OwnJar::watch`] hears that on a thread of its own and wakes the loop,
//! which sets the terminal up again on each attach (`app::proc`).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;
use spyc_jar::client::{Client, Sender};
use spyc_jar::proto::{Msg, Role};

use super::place::this_users_sessions;

/// How long spyc waits on its own jar at start: past the longest a jar is
/// silent, parked for an upgrade (asserted in `super`), so a busy jar isn't
/// taken for none.
pub(super) const LINK_WAIT: Duration = Duration::from_secs(20);

/// spyc's link to its own jar, as its program.
pub struct OwnJar {
    name: String,
    socket: PathBuf,
    /// Until [`Self::watch`] takes it.
    client: Option<Client>,
    /// The connection's sending half, once [`Self::watch`] has split it.
    tx: Option<Sender>,
    attached: Arc<AtomicBool>,
    attaches: Arc<AtomicU64>,
}

impl OwnJar {
    /// The session's name: its jar's socket's.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the terminal attached now reaches this machine over SSH, as its
    /// client told the jar (`spyc_jar::attach::TERMINAL_KEY`); `None` when it
    /// didn't say (an older client) or the jar can't be asked.
    pub fn terminal_over_ssh(&self) -> Option<bool> {
        let fields = spyc_jar::client::describe(&self.socket).ok()?;
        let (_, kind) = fields
            .iter()
            .find(|(k, _)| k == spyc_jar::attach::TERMINAL_KEY)?;
        Some(kind == spyc_jar::attach::terminal_kind(true))
    }

    /// Whether a terminal is attached.
    pub fn attached(&self) -> bool {
        self.attached.load(Ordering::SeqCst)
    }

    /// How many terminals have attached since spyc linked, the one attached
    /// then not counted. Read after [`Self::attached`]: an attach is counted
    /// before it is marked attached, so a terminal seen attached is counted.
    pub fn attaches(&self) -> u64 {
        self.attaches.load(Ordering::SeqCst)
    }

    /// Have the jar end its client, once what spyc wrote before asking (its
    /// terminal's reset) has reached it. spyc stops drawing at once rather
    /// than when the jar says the client left. `false` when the jar can't be
    /// asked.
    pub fn detach_client(&self) -> bool {
        self.attached.store(false, Ordering::SeqCst);
        self.tx
            .as_ref()
            .is_some_and(|tx| tx.send(&Msg::Detach).is_ok())
    }

    /// Hear the jar's attaches and detaches on a thread of its own, calling
    /// `wake` after each.
    pub fn watch(&mut self, wake: impl Fn() + Send + 'static) {
        let Some(client) = self.client.take() else {
            return;
        };
        let (attached, attaches) = (Arc::clone(&self.attached), Arc::clone(&self.attaches));
        // The jar holds no deadline over its program's connection, so the
        // thread only waits.
        let (mut rx, tx) = client.split();
        self.tx = Some(tx.clone());
        let spawned = std::thread::Builder::new()
            .name("own-jar".into())
            .spawn(move || {
                let _keep_open = tx;
                loop {
                    match rx.recv(WATCH_IDLE) {
                        Ok(Some(Msg::Attached)) => {
                            attaches.fetch_add(1, Ordering::SeqCst);
                            attached.store(true, Ordering::SeqCst);
                            wake();
                        }
                        Ok(Some(Msg::Detached)) => {
                            attached.store(false, Ordering::SeqCst);
                            wake();
                        }
                        Ok(_) => {}
                        Err(_) => return,
                    }
                }
            });
        if let Err(e) = spawned {
            crate::spyc_debug!("own jar: no watcher: {e:#}");
        }
    }
}

/// The socket of spyc's own jar: `socket`, its `SPYC_JAR_SOCKET`, when the
/// directory it is in is `sessions`. Both are compared as given, so the
/// caller resolves them alike.
pub fn own_jar_socket(socket: &Path, sessions: &Path) -> Option<PathBuf> {
    let in_sessions = socket.parent() == Some(sessions);
    let named = socket.extension() == Some(OsStr::new("sock"))
        && socket.file_stem().is_some_and(|s| !s.is_empty());
    (in_sessions && named).then(|| socket.to_path_buf())
}

/// Link to spyc's own jar when it runs in one, as its program; `None` for a
/// plain spyc.
pub fn link_own() -> Option<OwnJar> {
    let env = std::env::var_os("SPYC_JAR_SOCKET")?;
    // SPYC-TRAP(own-jar-by-place): spyc is in its own jar only when the
    // socket is in the sessions directory; every pane's program has a
    // `SPYC_JAR_SOCKET` too, and a spyc run as one must not take that jar for
    // its own.
    let resolved = |p: &Path| std::fs::canonicalize(p).ok();
    let given = Path::new(&env);
    let socket = resolved(given.parent()?)?.join(given.file_name()?);
    let socket = own_jar_socket(&socket, &resolved(&this_users_sessions())?)?;
    match link(&socket) {
        Ok(jar) => Some(jar),
        Err(e) => {
            crate::spyc_debug!("own jar: {} won't take this spyc: {e:#}", socket.display());
            None
        }
    }
}

/// Connect to the jar at `socket` as its program, and ask whether a terminal
/// is attached: the jar says `attached` at once to a program that connects
/// while it has an owner, and its description, asked after, follows that.
fn link(socket: &Path) -> anyhow::Result<OwnJar> {
    let (mut client, _) = Client::connect(socket, Role::Program, LINK_WAIT, b"")?;
    client.send(&Msg::Describe)?;
    let until = Instant::now() + LINK_WAIT;
    let mut attached = false;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        anyhow::ensure!(!left.is_zero(), "the jar didn't describe itself");
        match client.recv(left)? {
            Some(Msg::Attached) => attached = true,
            Some(Msg::Detached) => attached = false,
            Some(Msg::Description { fields }) => {
                attached |= fields.iter().any(|(k, v)| k == "owner" && v == "connected");
                break;
            }
            _ => {}
        }
    }
    let name = socket
        .file_stem()
        .map_or_default(|s| s.to_string_lossy().into_owned());
    Ok(OwnJar {
        name,
        socket: socket.to_path_buf(),
        client: Some(client),
        tx: None,
        attached: Arc::new(AtomicBool::new(attached)),
        attaches: Arc::new(AtomicU64::new(0)),
    })
}

/// The own jar's watcher's wait between frames: it wakes on one, and the jar
/// holds no deadline over its program's connection.
const WATCH_IDLE: Duration = Duration::from_secs(3600);

/// How often a handover looks for the jar it started to be up.
const START_POLL: Duration = Duration::from_millis(20);

/// When a session's own jar starts its spyc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Begin {
    /// Once a terminal attaches, and never if none does within the jar's
    /// hold: `^d`'s handover, whose terminal attaches next.
    OnAttach,
    /// At once, with no terminal: a detached project's spyc, which takes its
    /// tabs back before anyone attaches (`app::project_move`).
    Now,
}

/// Start the own jar of the session `name`: at `sessions/<name>.sock`,
/// running `argv` in `cwd` when `begin` says. Its token is kept where
/// `spyc attach` reads it.
pub fn start(
    name: &str,
    argv: Vec<String>,
    cwd: &Path,
    (rows, cols): (u16, u16),
    begin: Begin,
) -> anyhow::Result<PathBuf> {
    use super::place::{Place, SocketState, socket_state};
    use std::io::Write;
    use std::os::unix::process::CommandExt;
    let place = Place::for_this_user()?;
    let socket = place.prepare_sessions()?.join(format!("{name}.sock"));
    match socket_state(&socket) {
        SocketState::Live => anyhow::bail!("{name} already runs in a jar of its own"),
        SocketState::Unknown => anyhow::bail!(
            "can't tell whether {name}'s last jar still runs at {}",
            socket.display()
        ),
        SocketState::Stale => {
            std::fs::remove_file(&socket)
                .with_context(|| format!("remove {}", socket.display()))?;
        }
        SocketState::Missing => {}
    }
    let token = super::tokens::new()?;
    super::tokens::save(&place, &socket, &token)?;
    let mut jar = std::process::Command::new(&place.binary)
        .arg0(format!("spyc-jar {name}"))
        .args(["--wire", &spyc_jar::proto::VERSION.to_string()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("start {}", place.binary.display()))?;
    let start = spyc_jar::cli::Start {
        token,
        socket: socket.clone(),
        cwd: Some(cwd.to_path_buf()),
        rows,
        cols,
        hold: (begin == Begin::OnAttach).then_some(place.hold),
        exit_linger: None,
        argv,
    };
    // SPYC-TRAP(jar-token-stdin): the token goes on the jar's standard
    // input, never in its arguments or environment.
    jar.stdin
        .take()
        .context("the jar's standard input")?
        .write_all(&start.encode())
        .context("hand the jar its start")?;
    let until = Instant::now() + place.hold;
    while socket_state(&socket) != SocketState::Live {
        if Instant::now() >= until || jar.try_wait().is_ok_and(|exit| exit.is_some()) {
            let _ = jar.kill();
            let _ = jar.wait();
            super::tokens::forget(&place, &socket);
            anyhow::bail!("{name}'s jar didn't start");
        }
        std::thread::sleep(START_POLL);
    }
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_socket_in_the_sessions_directory_is_spycs_own_jar() {
        let sessions = Path::new("/run/u/spyc/sessions");
        let own = sessions.join("CORIANDER_CLOVE.sock");
        assert_eq!(own_jar_socket(&own, sessions), Some(own.clone()));
    }

    #[test]
    fn a_panes_jar_is_never_spycs_own() {
        let sessions = Path::new("/run/u/spyc/sessions");
        for socket in [
            "/run/u/spyc/jars/0a1b2c3d4e5f6a7b.sock",
            "/run/u/spyc/sessions/deeper/X.sock",
            "/run/u/spyc/X.sock",
            "/run/u/spyc/sessions/X.lock",
            "/run/u/spyc/sessions/.sock",
            "/elsewhere/sessions/X.sock",
        ] {
            assert_eq!(
                own_jar_socket(Path::new(socket), sessions),
                None,
                "{socket}"
            );
        }
    }
}
