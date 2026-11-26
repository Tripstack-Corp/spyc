//! The jar's lock (R32b in `docs/JAR_PROTOCOL.md`): an `fcntl` lock on
//! a file beside the socket, held for as long as the jar lives.
//!
//! A client reads it to tell a jar that is only busy (parked, stopped, its
//! backlog full) from one that is gone, without connecting: macOS refuses a
//! connection to a full backlog as it does one to a socket nothing listens
//! on.
//!
//! An `fcntl` lock belongs to the process, not to a descriptor's copies: a
//! child never holds it, whatever descriptors it inherits, and the process
//! keeps it across `exec` while the descriptor stays open. So the upgrade
//! lets the descriptor cross (it is close-on-exec until then) and the new
//! image holds the lock without knowing which descriptor it is. A `flock`
//! lock would cross as well, but a validator or holder that inherited it
//! would keep it after the jar was gone.

use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use rustix::fs::FlockOperation;
use rustix::io::Errno;

/// How often a jar tries again for its lock while an old reader holds it.
const LOCK_RETRY: Duration = Duration::from_millis(10);

/// The lock file of the jar at `socket`: its path with `.lock` added.
pub fn path(socket: &Path) -> PathBuf {
    let mut p = socket.as_os_str().to_owned();
    p.push(".lock");
    p.into()
}

/// What a jar's lock says about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    /// A jar holds it: alive, however busy.
    Held,
    /// Nothing holds it: the jar is gone.
    Free,
    /// No lock file, or one that can't be read: nothing can be concluded.
    Unknown,
}

/// Read the lock of the jar at `socket`, taking it without waiting and
/// letting go at once; a jar starting meanwhile waits that moment out.
///
/// Never from a jar's own process: closing any descriptor of the file lets
/// go of the process's lock on it.
pub fn state(socket: &Path) -> LockState {
    let Ok(file) = File::open(path(socket)) else {
        return LockState::Unknown;
    };
    match rustix::fs::fcntl_lock(&file, FlockOperation::NonBlockingLockShared) {
        Ok(()) => LockState::Free,
        Err(Errno::AGAIN | Errno::ACCESS) => LockState::Held,
        Err(_) => LockState::Unknown,
    }
}

/// The lock this process took, when it took one. A jar that crossed an
/// upgrade's `exec` holds its lock through a descriptor it can't name, and
/// has `None` here until it makes the file again.
static HELD: Mutex<Option<File>> = Mutex::new(None);

/// How long a jar starting waits for a client reading its lock to let go.
const READER_GRACE: Duration = Duration::from_secs(1);

/// Take the lock of the jar about to bind `socket`, or fail when another
/// jar holds it.
pub(crate) fn take(socket: &Path) -> anyhow::Result<()> {
    let at = path(socket);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&at)
        .with_context(|| format!("open {}", at.display()))?;
    let until = Instant::now() + READER_GRACE;
    loop {
        match rustix::fs::fcntl_lock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => break,
            Err(Errno::AGAIN | Errno::ACCESS) if Instant::now() < until => {
                std::thread::sleep(LOCK_RETRY);
            }
            Err(Errno::AGAIN | Errno::ACCESS) => {
                bail!("a jar already runs at {}", socket.display());
            }
            Err(e) => return Err(e).with_context(|| format!("lock {}", at.display())),
        }
    }
    *HELD.lock().unwrap_or_else(PoisonError::into_inner) = Some(file);
    Ok(())
}

/// Make and take the lock again when its file has gone, as a cleaner of
/// `$TMPDIR` may delete it; the copy that was deleted is let go.
pub(crate) fn keep(socket: &Path) {
    if path(socket).symlink_metadata().is_err() {
        let _ = take(socket);
    }
}

/// Let the lock's descriptor cross the `exec` that follows, or stop it from
/// crossing one again when that `exec` failed. A descriptor that crossed an
/// earlier `exec` and isn't named here crosses every one.
pub(crate) fn cross_exec(cross: bool) {
    use rustix::io::FdFlags;
    if let Some(file) = HELD.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        let flags = if cross {
            FdFlags::empty()
        } else {
            FdFlags::CLOEXEC
        };
        let _ = rustix::io::fcntl_setfd(file, flags);
    }
}

/// Remove the lock file of a jar that is ending.
pub(crate) fn forget(socket: &Path) {
    let _ = std::fs::remove_file(path(socket));
}
