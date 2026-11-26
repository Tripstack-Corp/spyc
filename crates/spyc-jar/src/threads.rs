//! Every thread the crate starts, started so that the OS's refusal comes back
//! as an error: `std::thread::spawn` panics instead, which on the jar's main
//! thread ends the jar and its program, and on a connection's thread leaves a
//! lease or an upgrade claimed for good (review finding B1). A thread the OS
//! refuses (`RLIMIT_NPROC`, no memory for a stack) is the failure of what
//! wanted it: that connection is closed and its lease freed, that upgrade
//! is busy, that start fails. Bare `std::thread::spawn` is kept out of the
//! crate by a guard (`every_thread_the_jar_starts_can_be_refused`).
//!
//! A debug build refuses, once each, the threads named in
//! `SPYC_JAR_THREAD_FAULT` (comma-separated), so a test can refuse one where
//! no limit reaches it; a release build has no such hook.

use std::io;
use std::thread::JoinHandle;

/// Start `f` on a thread named `name`, or say why the OS refused it.
pub fn start<F, T>(name: &'static str, f: F) -> io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    fault(name)?;
    std::thread::Builder::new().name(name.into()).spawn(f)
}

#[cfg(debug_assertions)]
fn fault(name: &'static str) -> io::Result<()> {
    use std::sync::{Mutex, PoisonError};
    static REFUSED: Mutex<Vec<&str>> = Mutex::new(Vec::new());
    let asked = std::env::var("SPYC_JAR_THREAD_FAULT").unwrap_or_default();
    if !asked.split(',').any(|n| n.trim() == name) {
        return Ok(());
    }
    let mut refused = REFUSED.lock().unwrap_or_else(PoisonError::into_inner);
    if refused.contains(&name) {
        return Ok(());
    }
    refused.push(name);
    drop(refused);
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        format!("the {name} thread refused, as SPYC_JAR_THREAD_FAULT asks"),
    ))
}

#[cfg(not(debug_assertions))]
#[allow(clippy::unnecessary_wraps)]
const fn fault(_: &'static str) -> io::Result<()> {
    Ok(())
}
