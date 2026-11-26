//! A debug build's hold in a jar's end, for the test that an ending jar
//! leaves nothing at its address (`SPYC_JAR_END_GAP`): its end waits,
//! between its socket going and its lock file going, until the socket check
//! that begins next has run. A release build reads no environment for it and
//! counts nothing.

#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};

/// The jar's socket checks begun, and done.
#[cfg(debug_assertions)]
static BEGUN: AtomicU64 = AtomicU64::new(0);
#[cfg(debug_assertions)]
static DONE: AtomicU64 = AtomicU64::new(0);

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

/// With `SPYC_JAR_END_GAP` set, wait until the socket check that begins
/// after this call has run.
#[cfg(debug_assertions)]
pub(super) fn gap() {
    if std::env::var_os("SPYC_JAR_END_GAP").is_none() {
        return;
    }
    let next = BEGUN.load(SeqCst) + 1;
    while DONE.load(SeqCst) < next {
        std::thread::sleep(GAP_POLL);
    }
}

#[cfg(not(debug_assertions))]
pub(super) const fn check_begins() {}

#[cfg(not(debug_assertions))]
pub(super) const fn check_done() {}

#[cfg(not(debug_assertions))]
pub(super) const fn gap() {}
