//! The program's life in the jar: its output into the log, its exit, the
//! linger that ends it when no owner comes back (R32a), and the signals an
//! owner sends it.

use std::fs::File;
use std::io::Read;
use std::sync::PoisonError;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use rustix::process::{Pid, WaitOptions};

use super::{Shared, TICK, Who, readable};
use crate::proto::JarSignal;
use crate::state::{Event, State};

/// How long a jar with a finite linger waits for its first owner: the spyc
/// that spawned it connects within milliseconds.
const FIRST_OWNER_GRACE: Duration = Duration::from_secs(10);

/// Copy the program's output into the log, pausing while a live owner is a
/// window behind.
pub(super) fn pump_pty(shared: &Shared, mut reader: File) {
    // The window is checked before each read, so a live owner can be a
    // window plus one read behind: keep reads small.
    let mut buf = vec![0u8; 8192];
    loop {
        if shared.upgrading() {
            shared.park(Who::Pty);
            continue;
        }
        let st = shared.lock();
        if st.owner.is_some() && st.log.head - st.acked >= shared.cfg.window {
            let _ = shared.cv.wait_timeout(st, TICK);
            continue;
        }
        drop(st);
        if !readable(&reader, TICK) {
            continue;
        }
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let mut st = shared.lock();
        let at = st.log.head;
        st.log.push(Event::Out(at, buf[..n].to_vec()));
        if let Some(keep) = st.snapshots.iter().map(|s| s.offset).min() {
            st.log.trim(keep);
        }
        drop(st);
        shared.cv.notify_all();
    }
    shared.lock().pty_eof = true;
    shared.pty_done.store(true, Ordering::SeqCst);
    shared.cv.notify_all();
}

/// Record the program's exit, polled so the waiter can park for an upgrade.
pub(super) fn wait_child(shared: &Shared) {
    let pid = i32::try_from(shared.child_pid).ok().and_then(Pid::from_raw);
    loop {
        if shared.upgrading() {
            shared.park(Who::Waiter);
            continue;
        }
        let exit = match rustix::process::waitpid(pid, WaitOptions::NOHANG) {
            Ok(None) => {
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            Ok(Some((_, status))) => match (status.exit_status(), status.terminating_signal()) {
                (Some(code), _) => (code, false),
                (None, Some(_)) => (-1, true),
                _ => (-1, false),
            },
            Err(_) => (-1, false),
        };
        shared.lock().exit = Some(exit);
        shared.waiter_done.store(true, Ordering::SeqCst);
        shared.cv.notify_all();
        return;
    }
}

/// End the jar once its program has ended and the exit was collected, or
/// nobody came back for it within the linger time.
pub(super) fn reap(shared: &Shared) {
    let mut st = shared.lock();
    loop {
        // R4: the foreground process group, read here because only the
        // master's holder can. A job starting needn't print anything.
        let fg = rustix::termios::tcgetpgrp(&shared.master)
            .ok()
            .and_then(|p| u32::try_from(p.as_raw_nonzero().get()).ok())
            .unwrap_or(0);
        if fg != st.fg {
            st.fg = fg;
            shared.cv.notify_all();
        }
        if st.exit.is_none() && !shared.upgrading() && lingered(shared, &st) {
            drop(st);
            end_program(shared);
            st = shared.lock();
            continue;
        }
        if st.exit.is_some() && !shared.upgrading() {
            let alone_too_long = st
                .ownerless_since
                .is_some_and(|t| t.elapsed() >= shared.cfg.linger_after_exit);
            if st.exit_acked || alone_too_long {
                let _ = std::fs::remove_file(&shared.cfg.socket);
                std::process::exit(0);
            }
        }
        st = shared
            .cv
            .wait_timeout(st, Duration::from_millis(200))
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

/// Whether the program has been without an owner for longer than its linger
/// policy allows (R32a).
fn lingered(shared: &Shared, st: &State) -> bool {
    let (Some(linger), Some(since)) = (shared.cfg.linger, st.ownerless_since) else {
        return false;
    };
    let allowed = if st.had_owner {
        linger
    } else {
        linger.max(FIRST_OWNER_GRACE)
    };
    since.elapsed() >= allowed
}

/// End the program as closing its terminal would: `SIGHUP` to its process
/// group, then `SIGKILL` if it hasn't gone within two seconds. The reaper ends
/// the jar once the exit is recorded, no owner being there to collect it.
fn end_program(shared: &Shared) {
    use rustix::process::Signal;
    let Some(pid) = i32::try_from(shared.child_pid).ok().and_then(Pid::from_raw) else {
        return;
    };
    let _ = rustix::process::kill_process_group(pid, Signal::HUP);
    let until = Instant::now() + Duration::from_secs(2);
    while shared.lock().exit.is_none() && Instant::now() < until {
        thread::sleep(Duration::from_millis(20));
    }
    if shared.lock().exit.is_none() {
        let _ = rustix::process::kill_process_group(pid, Signal::KILL);
    }
    // No owner will come for the exit: don't linger for one.
    shared.lock().exit_acked = true;
}

pub(super) fn deliver(pid: u32, signal: i32) {
    use rustix::process::Signal;
    let Some(sig) = JarSignal::from_wire(signal) else {
        return;
    };
    let sig = match sig {
        JarSignal::Interrupt => Signal::INT,
        JarSignal::Terminate => Signal::TERM,
        JarSignal::Kill => Signal::KILL,
        JarSignal::Stop => Signal::STOP,
        JarSignal::Continue => Signal::CONT,
        JarSignal::Hangup => Signal::HUP,
    };
    if let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) {
        let _ = rustix::process::kill_process_group(pid, sig);
    }
}
