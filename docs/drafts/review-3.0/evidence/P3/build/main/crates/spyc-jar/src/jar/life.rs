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

use super::{PTY_READ, Shared, TICK, Who, readable};
use crate::proto::JarSignal;
use crate::state::{Event, State};

/// How often the waiter looks for the program's pid, then its exit.
const WAITER_POLL: Duration = Duration::from_millis(50);
/// How often the reaper wakes with nothing changed.
const REAPER_TICK: Duration = Duration::from_millis(200);
/// How long a program has to end on `SIGHUP` before it gets `SIGKILL`.
const END_GRACE: Duration = Duration::from_secs(2);
/// How often the end looks for the program's exit.
const END_POLL: Duration = Duration::from_millis(20);

/// How long a jar with a finite linger waits for its first owner, or for the
/// successor a release named: the spyc that spawned it connects within
/// milliseconds, one taking it over within seconds.
const FIRST_OWNER_GRACE: Duration = Duration::from_secs(10);

/// Decision M: end the jar, having started nothing, when no owner has come
/// within `wait`. Under the state's lock, so an owner being welcomed either
/// started the program first or finds no jar.
pub(super) fn await_first_owner(shared: &Shared, wait: Duration) {
    thread::sleep(wait);
    let st = shared.lock();
    if shared.child_pid() == 0 && st.owner.is_none() {
        let _ = std::fs::remove_file(&shared.cfg.socket);
        crate::lock::forget(&shared.cfg.socket);
        std::process::exit(0);
    }
}

/// Answer the program's detach (`super::Drain`) once the PTY is empty: here,
/// between reads, everything the pump has read is in the log.
fn answer_drain(shared: &Shared, st: &State, reader: &File) {
    let mut drain = shared.drain();
    if drain.answered < drain.asked && !readable(reader, Duration::ZERO) {
        drain.answered = drain.asked;
        drain.head = st.log.head;
        drop(drain);
        shared.cv.notify_all();
    }
}

/// Copy the program's output into the log, pausing while a live owner is a
/// window behind.
pub(super) fn pump_pty(shared: &Shared, mut reader: File) {
    // The window is checked before each read, so a live owner can be a
    // window plus one read behind: keep reads small.
    let mut buf = vec![0u8; PTY_READ];
    loop {
        if shared.upgrading() {
            shared.park(Who::Pty);
            continue;
        }
        let st = shared.lock();
        answer_drain(shared, &st, &reader);
        let unread = st.log.head - st.acked;
        let full = if st.owner.is_some() {
            unread >= shared.cfg.window
        } else {
            // R12b: the next read must fit in the ring beside all that's unread.
            shared.cfg.blocks_when_full() && unread + PTY_READ as u64 > shared.cfg.ring_cap as u64
        };
        if full {
            let _ = shared.cv.wait_timeout(st, TICK);
            continue;
        }
        drop(st);
        if !readable(&reader, TICK) {
            continue;
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(_) => break,
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
    // A program held for its first owner has no pid yet.
    while shared.child_pid() == 0 {
        thread::sleep(WAITER_POLL);
    }
    let pid = i32::try_from(shared.child_pid())
        .ok()
        .and_then(Pid::from_raw);
    loop {
        if shared.upgrading() {
            shared.park(Who::Waiter);
            continue;
        }
        let exit = match rustix::process::waitpid(pid, WaitOptions::NOHANG) {
            Ok(None) => {
                thread::sleep(WAITER_POLL);
                continue;
            }
            Ok(Some((_, status))) => match (status.exit_status(), status.terminating_signal()) {
                (Some(code), _) => (code, false),
                // As the frozen `exited` fixture has it: the code is the
                // signal's number.
                (None, Some(signal)) => (signal, true),
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
///
/// That time runs from the later of the exit and the owner's going (R2,
/// review finding 16): a program that ends long after spyc crashed keeps its
/// exit and its last output for the spyc that comes back.
pub(super) fn reap(shared: &Shared) {
    // When this thread first saw the exit: an upgrade starts the hold again.
    let mut exited_at: Option<Instant> = None;
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
        // A program held for its first owner has nothing to end: its wait is
        // the start's hold, and ending it here would spin, the pid being 0.
        if st.exit.is_none()
            && shared.child_pid() != 0
            && !shared.upgrading()
            && lingered(shared, &st)
        {
            drop(st);
            end_program(shared);
            st = shared.lock();
            continue;
        }
        if st.exit.is_some() && !shared.upgrading() {
            let exited = *exited_at.get_or_insert_with(Instant::now);
            let alone_too_long = st
                .ownerless_since
                .is_some_and(|t| t.max(exited).elapsed() >= shared.cfg.linger_after_exit);
            if st.exit_acked || alone_too_long {
                let _ = std::fs::remove_file(&shared.cfg.socket);
                crate::lock::forget(&shared.cfg.socket);
                std::process::exit(0);
            }
        }
        st = shared
            .cv
            .wait_timeout(st, REAPER_TICK)
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
    let Some(pid) = i32::try_from(shared.child_pid())
        .ok()
        .and_then(Pid::from_raw)
    else {
        return;
    };
    let _ = rustix::process::kill_process_group(pid, Signal::HUP);
    let until = Instant::now() + END_GRACE;
    while shared.lock().exit.is_none() && Instant::now() < until {
        thread::sleep(END_POLL);
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
