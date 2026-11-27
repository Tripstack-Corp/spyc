//! Where this user's jars live: their sockets in a private directory under the
//! runtime directory, their tokens under spyc's state (R26), and the
//! `spyc-jar` binary beside spyc.
//!
//! The socket directory is `$XDG_RUNTIME_DIR/spyc/jars`, else
//! `$TMPDIR/spyc/jars`, else `/tmp/spyc-<uid>/jars`, and each directory spyc
//! makes there must be this user's, `0700` and not a link before a socket in
//! it is trusted (finding 14): another user who made `/tmp/spyc-<uid>` first
//! could otherwise put a socket where spyc expects a jar. A pane's jar is found
//! by the path spyc started it at, never worked out again from a changed
//! environment.
//!
//! Every client reaches a jar's socket through this one check (the 3.0
//! review, E2): [`trusted_dir`] before a directory of sockets is read,
//! [`trusted`] before a socket is reached, and the wrappers [`connect`],
//! [`describe`] and [`attach`], the only places spyc connects to a jar.
//! Spawn, adopt, retake, a session's start, `spyc attach`, `spyc shutdown`,
//! `spyc --jars`, the orphan finder, the store's sweep and the MCP proxy that
//! follows `SPYC_JAR_SOCKET` all pass through them.

use std::io;
use std::path::{Path, PathBuf};

use anyhow::Context;

/// Where one spyc's jars are started and found.
#[derive(Debug, Clone)]
pub struct Place {
    /// The `spyc-jar` to start.
    pub binary: PathBuf,
    /// The directories spyc makes for the sockets, outermost first: each is
    /// checked, and the last holds the sockets.
    pub dirs: Vec<PathBuf>,
    /// Where each jar's token is kept, `0600`, by its socket's name.
    pub tokens: PathBuf,
    /// How long a jar started here holds its program for this spyc to
    /// connect (decision M), and so how long spyc waits for it to answer: the
    /// jar's clock starts after spyc's, so spyc never gives up on a jar that
    /// may still start the program.
    pub hold: std::time::Duration,
    /// How long a repaint after a gap holds the program's size a row off
    /// ([`super::repaint`]).
    pub repaint_hold: std::time::Duration,
}

impl Place {
    /// This user's: the jar beside the running spyc, sockets in the runtime
    /// directory, tokens in spyc's state directory.
    pub fn for_this_user() -> anyhow::Result<Self> {
        let dirs = socket_dirs(
            std::env::var_os("XDG_RUNTIME_DIR"),
            std::env::var_os("TMPDIR"),
            rustix::process::getuid().as_raw(),
        );
        start_store()?;
        let binary = match super::store::binary() {
            super::store::Binary::Entry(e) | super::store::Binary::Beside(e, _) => e,
            super::store::Binary::None(why) => return Err(super::NoUsableJar(why).into()),
        };
        let tokens = tokens_dir()?;
        Ok(Self {
            binary,
            dirs,
            tokens,
            hold: HOLD,
            repaint_hold: super::repaint::REPAINT_HOLD,
        })
    }

    /// The directory the sockets are in.
    pub fn sockets(&self) -> &Path {
        self.dirs.last().map_or(Path::new("/"), PathBuf::as_path)
    }

    /// The socket of the jar named `name`: the first 16 hex digits of the
    /// name's SHA-256, inside macOS's 104-byte limit on a socket's path. Not
    /// the name's own: a tab's id is a v7 UUID, whose leading digits are its
    /// millisecond and a counter, the same for tabs made together.
    pub fn socket(&self, name: &str) -> PathBuf {
        use sha2::{Digest, Sha256};
        let id = Sha256::digest(name.as_bytes())
            .iter()
            .take(8)
            .fold(String::new(), |mut s, b| {
                use std::fmt::Write as _;
                let _ = write!(s, "{b:02x}");
                s
            });
        self.sockets().join(format!("{id}.sock"))
    }

    /// Make the sessions' directory (`super::session`) this user's alone, and
    /// say where it is.
    pub fn prepare_sessions(&self) -> anyhow::Result<PathBuf> {
        let base = self.dirs.first().cloned().unwrap_or_default();
        let sessions = sessions_dir(&self.dirs);
        for dir in [&base, &sessions] {
            use std::os::unix::fs::DirBuilderExt;
            match std::fs::DirBuilder::new().mode(0o700).create(dir) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e).with_context(|| format!("create {}", dir.display())),
            }
            private_dir(dir)?;
        }
        Ok(sessions)
    }

    /// Make the socket directories this user's alone, or say why they can't
    /// be trusted.
    pub fn prepare(&self) -> anyhow::Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        for dir in &self.dirs {
            match std::fs::DirBuilder::new().mode(0o700).create(dir) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e).with_context(|| format!("create {}", dir.display())),
            }
            private_dir(dir)?;
        }
        Ok(())
    }
}

/// Where this user's jars' tokens are kept, by their sockets' names.
pub(super) fn tokens_dir() -> anyhow::Result<PathBuf> {
    Ok(crate::state::state_root()
        .context("no state directory for jar tokens")?
        .join("jar-tokens"))
}

/// The directory sessions' own jars are in, for this user.
pub fn this_users_sessions() -> PathBuf {
    sessions_dir(&socket_dirs(
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("TMPDIR"),
        rustix::process::getuid().as_raw(),
    ))
}

/// How long a pane's jar holds its program for this spyc: far longer than
/// a connect takes, even to a jar whose first run is scanned under load, and
/// a jar whose spyc died meanwhile ends having started nothing.
pub const HOLD: std::time::Duration = std::time::Duration::from_secs(30);

/// Start this user's jar binary store (`super::store`), once a session:
/// at spyc's start when jars are on, so the first jar needn't wait.
pub fn start_store() -> anyhow::Result<()> {
    let source = std::env::current_exe()
        .context("where spyc runs from")?
        .with_file_name("spyc-jar");
    let root = crate::state::state_root()
        .context("no state directory for the jar store")?
        .join("jar-bin");
    let dirs = socket_dirs(
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("TMPDIR"),
        rustix::process::getuid().as_raw(),
    );
    let sockets = dirs.last().cloned().unwrap_or_default();
    super::store::start(root, source, sockets);
    Ok(())
}

/// The socket directories for these values of `XDG_RUNTIME_DIR` and
/// `TMPDIR`, outermost first.
pub(super) fn socket_dirs(
    runtime: Option<std::ffi::OsString>,
    tmpdir: Option<std::ffi::OsString>,
    uid: u32,
) -> Vec<PathBuf> {
    let nonempty = |v: Option<std::ffi::OsString>| v.filter(|v| !v.is_empty());
    let base = nonempty(runtime).or_else(|| nonempty(tmpdir)).map_or_else(
        || PathBuf::from(format!("/tmp/spyc-{uid}")),
        |root| PathBuf::from(root).join("spyc"),
    );
    let jars = base.join("jars");
    vec![base, jars]
}

/// The directory sessions' own jars are in (`super::session`), beside the
/// pane jars': `dirs` as [`socket_dirs`] gives them.
pub(super) fn sessions_dir(dirs: &[PathBuf]) -> PathBuf {
    dirs.first()
        .map_or_else(|| PathBuf::from("/"), |base| base.join("sessions"))
}

/// `dir` is a directory, not a link, owned by this user and closed to
/// everyone else.
pub(super) fn private_dir(dir: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let meta = dir
        .symlink_metadata()
        .with_context(|| format!("look at {}", dir.display()))?;
    anyhow::ensure!(
        meta.file_type().is_dir(),
        "{} isn't a directory (a link?); no jar is trusted there",
        dir.display()
    );
    let uid = rustix::process::getuid().as_raw();
    anyhow::ensure!(
        meta.uid() == uid,
        "{} belongs to uid {}, not this user's {uid}; no jar is trusted there",
        dir.display(),
        meta.uid()
    );
    let mode = meta.permissions().mode() & 0o777;
    let others = mode & 0o077;
    anyhow::ensure!(
        others == 0,
        "{} is open to others (mode {mode:o}); no jar is trusted there",
        dir.display()
    );
    Ok(())
}

/// The one check before a client trusts what a directory of jar sockets
/// holds (finding 14, the 3.0 review's E2): `dir` and the directory it is in,
/// the runtime directory's `spyc`, are this user's alone and not links. The
/// one above matters too: whoever can write there can put another `dir` in
/// its place.
pub fn trusted_dir(dir: &Path) -> anyhow::Result<()> {
    let base = dir
        .parent()
        .with_context(|| format!("{} is no directory of jars", dir.display()))?;
    private_dir(base)?;
    private_dir(dir)
}

/// [`trusted_dir`] for the directory `socket` is in, and `socket` itself a
/// socket of this user's, not a link.
pub fn trusted(socket: &Path) -> anyhow::Result<()> {
    trusted_dir(
        socket
            .parent()
            .with_context(|| format!("{} is in no directory", socket.display()))?,
    )?;
    let meta = socket
        .symlink_metadata()
        .with_context(|| format!("look at {}", socket.display()))?;
    own_socket(&meta, socket)
}

/// `meta`, the file at `path`, is a socket of this user's, not a link.
fn own_socket(meta: &std::fs::Metadata, path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    anyhow::ensure!(
        meta.file_type().is_socket(),
        "{} isn't a socket (a link?); no jar is trusted there",
        path.display()
    );
    let uid = rustix::process::getuid().as_raw();
    anyhow::ensure!(
        meta.uid() == uid,
        "{} belongs to uid {}, not this user's {uid}; no jar is trusted there",
        path.display(),
        meta.uid()
    );
    Ok(())
}

/// The jars' sockets in `dir`, sorted, once [`trusted_dir`] passes: each
/// `*.sock` that is a socket of this user's. None when `dir` doesn't exist.
pub fn sockets_in(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if dir
        .symlink_metadata()
        .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
    {
        return Ok(Vec::new());
    }
    trusted_dir(dir)?;
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("read {}", dir.display()))?
        .filter_map(Result::ok)
        .filter(|e| {
            e.path().extension().is_some_and(|x| x == "sock")
                && e.metadata()
                    .is_ok_and(|m| own_socket(&m, &e.path()).is_ok())
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    Ok(found)
}

/// [`trusted`] for a client that may ask before its jar binds: the directory
/// always, the socket once its file is there. A socket not made yet is left
/// to `Client::connect`, which waits for it.
pub fn trusted_now(socket: &Path) -> anyhow::Result<()> {
    if socket.symlink_metadata().is_ok() {
        return trusted(socket);
    }
    trusted_dir(
        socket
            .parent()
            .with_context(|| format!("{} is in no directory", socket.display()))?,
    )
}

/// Connect to the jar at `socket` as `role`, once [`trusted_now`].
pub fn connect(
    socket: &Path,
    role: spyc_jar::proto::Role,
    wait: std::time::Duration,
    token: &[u8],
) -> anyhow::Result<(spyc_jar::client::Client, spyc_jar::proto::Msg)> {
    trusted_now(socket)?;
    spyc_jar::client::Client::connect(socket, role, wait, token)
}

/// What the jar at `socket` says of itself, asked as a lookup, once
/// [`trusted`].
pub fn describe(socket: &Path) -> anyhow::Result<Vec<(String, String)>> {
    trusted(socket)?;
    spyc_jar::client::describe(socket)
}

/// Attach this terminal to the session's jar at `socket` with `token`, once
/// [`trusted`] (`spyc attach`).
pub fn attach(socket: &Path, token: &[u8]) -> anyhow::Result<()> {
    trusted(socket)?;
    spyc_jar::attach::run(socket, token)
}

/// Whether a jar runs at a socket's path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketState {
    /// A jar holds the socket's lock, however busy, or one from before the
    /// lock answers there.
    Live,
    /// The lock is free: its jar is gone (R32).
    Stale,
    /// Nothing can be told: no lock file, and nothing answered at once.
    /// Never removed.
    Unknown,
    Missing,
}

/// SPYC-TRAP(jar-gone-on-evidence): a socket counts as gone only when its
/// jar's lock is free (R32b). A refused connection proves nothing (macOS
/// refuses a full backlog), and deleting a live jar's socket and token
/// can't be undone.
pub fn socket_state(socket: &Path) -> SocketState {
    use spyc_jar::lock::LockState;
    let lock = spyc_jar::lock::state(socket);
    // The lock first: a cleaner can remove a live jar's socket, which the jar
    // binds again a moment later, its lock held throughout.
    if socket.symlink_metadata().is_err() {
        return if lock == LockState::Held {
            SocketState::Live
        } else {
            SocketState::Missing
        };
    }
    match lock {
        LockState::Held => SocketState::Live,
        LockState::Free => SocketState::Stale,
        LockState::Unknown if spyc_jar::client::answers_now(socket) => SocketState::Live,
        LockState::Unknown => SocketState::Unknown,
    }
}

/// How old a socket must be before a sweep may judge it: a jar binds its
/// socket a moment before it listens, and is refused meanwhile.
pub(super) const SWEEP_AFTER: std::time::Duration = std::time::Duration::from_secs(10);

/// Remove the sockets in `place` at least `age` old whose jars are gone, and
/// their tokens and lock files (R32): the pane jars' and the sessions' (the
/// 3.0 review, E7). How many.
pub(super) fn sweep(place: &Place, age: std::time::Duration) -> usize {
    sweep_dir(place, place.sockets(), age) + sweep_dir(place, &sessions_dir(&place.dirs), age)
}

/// [`sweep`], in `dir`.
fn sweep_dir(place: &Place, dir: &Path, age: std::time::Duration) -> usize {
    // Nothing is removed from a directory this user can't trust.
    let Ok(sockets) = sockets_in(dir) else {
        return 0;
    };
    let old_enough = |p: &Path| {
        p.symlink_metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|old| old >= age))
    };
    sockets
        .into_iter()
        .filter(|p| old_enough(p))
        .filter(|p| socket_state(p) == SocketState::Stale && std::fs::remove_file(p).is_ok())
        .inspect(|p| {
            super::tokens::forget(place, p);
            let _ = std::fs::remove_file(spyc_jar::lock::path(p));
        })
        .count()
}

#[cfg(test)]
mod tests;
