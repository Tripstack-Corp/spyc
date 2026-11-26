//! How a jar ends: its socket goes, then its lock file, then the process,
//! holding the jar's address throughout, which its socket check also takes.
//! In a debug build, `SPYC_JAR_END_GAP` holds an end between its socket
//! going and its lock file going until the socket check that begins next has
//! run, or waits for the address; a release build reads no environment for
//! it and counts nothing.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};

/// The jar's address: its socket and its lock file. Its socket check, which
/// binds the socket again and makes the lock file again when a cleaner has
/// removed them, holds it, and so does the jar's end.
static ADDRESS: Mutex<()> = Mutex::new(());

/// The jar's address, for a socket check.
pub(super) fn address() -> MutexGuard<'static, ()> {
    match ADDRESS.try_lock() {
        Ok(held) => held,
        Err(TryLockError::Poisoned(held)) => held.into_inner(),
        Err(TryLockError::WouldBlock) => {
            held_at_address();
            ADDRESS.lock().unwrap_or_else(PoisonError::into_inner)
        }
    }
}

/// End the jar with `code`: the socket at `socket` goes, then its lock file,
/// then the process.
// SPYC-TRAP(jar-end-holds-its-address): the address is held from the
// socket's going to the process's end. A socket check between them would
// bind the socket again, and the jar would leave a socket with no lock file
// and nothing listening, which no spyc can tell from a running jar's.
pub(super) fn end(socket: &Path, code: i32) -> ! {
    let _address = ADDRESS.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = std::fs::remove_file(socket);
    gap();
    crate::lock::forget(socket);
    std::process::exit(code)
}

/// The jar's socket checks begun and done, and those that found the
/// address held.
#[cfg(debug_assertions)]
static BEGUN: AtomicU64 = AtomicU64::new(0);
#[cfg(debug_assertions)]
static DONE: AtomicU64 = AtomicU64::new(0);
#[cfg(debug_assertions)]
static HELD: AtomicU64 = AtomicU64::new(0);

/// How often a held end looks for the socket check to have run.
#[cfg(debug_assertions)]
const GAP_POLL: std::time::Duration = std::time::Duration::from_millis(10);

/// A socket check begins.
#[cfg(debug_assertions)]
pub(super) fn check_begins() {
    BEGUN.fetch_add(1, SeqCst);
}

/// A socket check is done.
#[cfg(debug_assertions)]
pub(super) fn check_done() {
    DONE.fetch_add(1, SeqCst);
}

/// A socket check found the address held, and waits for it.
#[cfg(debug_assertions)]
fn held_at_address() {
    HELD.fetch_add(1, SeqCst);
}

/// With `SPYC_JAR_END_GAP` set, wait until the socket check that begins
/// after this call has run, or waits for the address.
#[cfg(debug_assertions)]
fn gap() {
    if std::env::var_os("SPYC_JAR_END_GAP").is_none() {
        return;
    }
    let (next, held) = (BEGUN.load(SeqCst) + 1, HELD.load(SeqCst));
    while DONE.load(SeqCst) < next && HELD.load(SeqCst) == held {
        std::thread::sleep(GAP_POLL);
    }
}

#[cfg(not(debug_assertions))]
pub(super) const fn check_begins() {}

#[cfg(not(debug_assertions))]
pub(super) const fn check_done() {}

#[cfg(not(debug_assertions))]
const fn held_at_address() {}

#[cfg(not(debug_assertions))]
const fn gap() {}
