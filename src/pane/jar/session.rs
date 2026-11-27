//! A session's own jar (S4, decision D): the jar spyc itself runs in, so a
//! spyc outlives the terminal it was started from. It is a jar like a pane's,
//! kept apart by where it lives: `<runtime>/spyc/sessions/<SESSION_NAME>.sock`,
//! beside the pane jars' directory, named by the session it started as. That
//! name is its address and never changes; `:name` gives the session another
//! to go by, kept beside the socket in `<ADDRESS>.name` ([`shown_name`]), and
//! `spyc attach` finds the session by either.
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
use spyc_jar::state::{OWNER_LEFT, OwnerLeft};

use super::place::this_users_sessions;

/// How long spyc waits on its own jar at start: past the longest a jar is
/// silent, parked for an upgrade (asserted in `super`), so a busy jar isn't
/// taken for none.
pub(super) const LINK_WAIT: Duration = Duration::from_secs(20);

/// spyc's link to its own jar, as its program.
pub struct OwnJar {
    /// The name the session goes by.
    name: String,
    /// Its socket's name, which never changes.
    address: String,
    socket: PathBuf,
    /// Until [`Self::watch`] takes it.
    client: Option<Client>,
    /// The connection's sending half, once [`Self::watch`] has split it.
    tx: Option<Sender>,
    attached: Arc<AtomicBool>,
    attaches: Arc<AtomicU64>,
}

impl OwnJar {
    /// The name the session goes by: its name file's, else its address.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The jar's socket's name: where `spyc attach` reaches it, whatever the
    /// session is called.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Have the session go by `name`, its socket kept.
    pub fn rename(&mut self, name: &str) -> anyhow::Result<()> {
        write_name(&self.socket, name)?;
        self.name = name.to_string();
        Ok(())
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

    /// Why the terminal attached last left, as the jar recorded it. `None`
    /// when the jar can't be asked or doesn't say: one built before it did,
    /// or one still ending a terminal spyc detached.
    pub fn why_left(&self) -> Option<OwnerLeft> {
        let fields = spyc_jar::client::describe(&self.socket).ok()?;
        let (_, word) = fields.iter().find(|(k, _)| k == OWNER_LEFT)?;
        OwnerLeft::from_word(word)
    }

    /// Forget this session's token: spyc ends it for good, and its jar with
    /// it ([`forget_token`]).
    pub fn forget_token(&self) {
        forget_token(&self.socket);
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
    let canonical = resolved(given.parent()?)?.join(given.file_name()?);
    own_jar_socket(&canonical, &resolved(&this_users_sessions())?)?;
    // SPYC-TRAP(own-jar-at-given-path): resolved only to compare. The jar is
    // reached where it was bound, at the path it was given: the resolved one
    // can be too long for a socket (`/tmp` is `/private/tmp` on macOS).
    match link(given) {
        Ok(jar) => Some(jar),
        Err(e) => {
            crate::spyc_debug!("own jar: {} won't take this spyc: {e:#}", given.display());
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
    let address = socket
        .file_stem()
        .map_or_default(|s| s.to_string_lossy().into_owned());
    Ok(OwnJar {
        name: shown_name(socket).unwrap_or_else(|| address.clone()),
        address,
        socket: socket.to_path_buf(),
        client: Some(client),
        tx: None,
        attached: Arc::new(AtomicBool::new(attached)),
        attaches: Arc::new(AtomicU64::new(0)),
    })
}

/// The socket of session `name`'s own jar in `sessions`, when one can be
/// bound there: `sun_path` holds at most 103 bytes of a path on macOS and 107
/// on Linux, and a session whose socket is longer can't start its jar.
pub fn socket_in(sessions: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let socket = sessions.join(format!("{name}.sock"));
    if std::os::unix::net::SocketAddr::from_pathname(&socket).is_err() {
        anyhow::bail!(
            "{name} is too long a session name here: its jar's socket, {}, would be {} bytes, \
             longer than a socket's path can be",
            socket.display(),
            socket.as_os_str().len()
        );
    }
    Ok(socket)
}

/// Forget the token of the session whose jar is at `socket`, once its jar
/// has ended or is ending (the 3.0 review, E7): it opens nothing any more,
/// and a token kept for a name is what a socket planted under that name
/// would collect.
pub fn forget_token(socket: &Path) {
    if let Ok(dir) = super::place::tokens_dir() {
        super::tokens::forget_in(&dir, socket);
    }
}

/// The file beside a session's socket holding the name it goes by, once
/// `:name` has given it one: `<ADDRESS>.name`.
pub fn name_file(socket: &Path) -> PathBuf {
    socket.with_extension("name")
}

/// The name the session at `socket` goes by: its name file's, else its
/// address. `None` for a path with no file name.
pub fn shown_name(socket: &Path) -> Option<String> {
    let address = socket.file_stem()?.to_str()?.to_string();
    let named = std::fs::read_to_string(name_file(socket))
        .ok()
        .and_then(|s| crate::state::session_names::normalize(s.trim()));
    Some(named.unwrap_or(address))
}

/// Record `name` as the one the session at `socket` goes by, renamed into
/// place so no reader sees half of it.
pub fn write_name(socket: &Path, name: &str) -> anyhow::Result<()> {
    let file = name_file(socket);
    let part = socket.with_extension("name.part");
    std::fs::write(&part, format!("{name}\n"))
        .with_context(|| format!("write {}", part.display()))?;
    std::fs::rename(&part, &file).with_context(|| format!("write {}", file.display()))
}

/// The own jar's watcher's wait between frames: it wakes on one, and the jar
/// holds no deadline over its program's connection.
const WATCH_IDLE: Duration = Duration::from_secs(3600);

/// How often a handover looks for the jar it started to be up.
const START_POLL: Duration = Duration::from_millis(20);

/// Start the own jar of the session `name`: at `sessions/<name>.sock`,
/// running `argv` in `cwd` at once, with no terminal until one attaches. Its
/// token is kept where `spyc attach` reads it. In `place`, else this
/// user's, as a tab's jar is.
pub fn start(
    place: Option<super::place::Place>,
    name: &str,
    argv: Vec<String>,
    cwd: &Path,
    size: (u16, u16),
) -> anyhow::Result<PathBuf> {
    let place = match place {
        Some(place) => place,
        None => super::place::Place::for_this_user()?,
    };
    start_in(&place, name, argv, cwd, size)
}

/// [`start`], with the jar and its directories `place` names.
pub(super) fn start_in(
    place: &super::place::Place,
    name: &str,
    argv: Vec<String>,
    cwd: &Path,
    (rows, cols): (u16, u16),
) -> anyhow::Result<PathBuf> {
    use super::place::{SocketState, socket_state};
    use std::os::unix::process::CommandExt;
    let socket = socket_in(&place.prepare_sessions()?, name)?;
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
    // A name the last session at this address went by isn't this one's.
    match std::fs::remove_file(name_file(&socket)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(e).with_context(|| format!("remove {}", name_file(&socket).display()));
        }
    }
    let token = super::tokens::new()?;
    super::tokens::save(place, &socket, &token)?;
    let start = spyc_jar::cli::Start {
        token,
        socket: socket.clone(),
        cwd: Some(cwd.to_path_buf()),
        rows,
        cols,
        hold: None,
        exit_linger: None,
        argv,
    };
    // SPYC-TRAP(jar-token-stdin): the token goes on the jar's standard
    // input, never in its arguments or environment; as a file, never a pipe
    // (`crate::child_io`).
    let input = crate::child_io::input(&start.encode()).context("hand the jar its start")?;
    let mut jar = std::process::Command::new(&place.binary)
        .arg0(format!("spyc-jar {name}"))
        .args(["--wire", &spyc_jar::proto::VERSION.to_string()])
        .stdin(std::process::Stdio::from(input))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("start {}", place.binary.display()))?;
    let until = Instant::now() + place.hold;
    while socket_state(&socket) != SocketState::Live {
        if Instant::now() >= until || jar.try_wait().is_ok_and(|exit| exit.is_some()) {
            let _ = jar.kill();
            let _ = jar.wait();
            super::tokens::forget(place, &socket);
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
    fn a_session_goes_by_its_name_file_else_its_address() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("CORIANDER_CLOVE.sock");
        assert_eq!(shown_name(&socket).as_deref(), Some("CORIANDER_CLOVE"));
        write_name(&socket, "GENERAL_WORK").unwrap();
        assert_eq!(shown_name(&socket).as_deref(), Some("GENERAL_WORK"));
        assert!(socket.with_extension("name").exists());
        assert!(
            !socket.with_extension("name.part").exists(),
            "renamed into place"
        );
        write_name(&socket, "GINGER").unwrap();
        assert_eq!(shown_name(&socket).as_deref(), Some("GINGER"));
        std::fs::write(name_file(&socket), " \n").unwrap();
        assert_eq!(
            shown_name(&socket).as_deref(),
            Some("CORIANDER_CLOVE"),
            "an empty name file names nothing"
        );
    }

    #[test]
    fn a_session_name_fits_only_when_its_socket_can_be_bound() {
        let sessions = Path::new("/run/u/spyc/sessions");
        assert_eq!(
            socket_in(sessions, "CORIANDER_CLOVE").unwrap(),
            sessions.join("CORIANDER_CLOVE.sock")
        );
        let long = "A".repeat(120);
        let err = socket_in(sessions, &long).unwrap_err().to_string();
        assert!(err.contains("too long"), "{err}");
        // The limit is the path's, not the name's: a name that fits one
        // directory doesn't fit a deeper one.
        let deep = PathBuf::from("/").join("d".repeat(90));
        assert!(socket_in(&deep, "CORIANDER_CLOVE").is_err());
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
