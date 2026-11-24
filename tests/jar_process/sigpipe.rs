//! Review finding A1: a failed upgrade `exec` leaves the jar with `SIGPIPE`
//! at its default for the rest of its life (std's `exec` sets it so for the
//! image it starts), so from then on any send to a peer that has gone must
//! answer `EPIPE` rather than raise the signal, or the jar ends and its
//! program with it.
//!
//! Each test fails an `exec` first, then has the jar send to a peer that is
//! gone before the send by construction, never by chance.
//!
//! On macOS std sets `SO_NOSIGPIPE` on every socket it makes but a
//! socketpair, and an accepted connection inherits it from the listener; the
//! jar's socketpairs come from `jar::pair`, which sets it. On Linux std's
//! `write` carries `MSG_NOSIGNAL`, and the jar's `sendmsg` calls carry it as
//! `jar::NO_SIGNAL` (SPYC-TRAP `jar-no-sigpipe`).

use super::*;

/// Fail an upgrade's `exec` on `owner`, as finding 11's test does: a new
/// binary that loses its execute bit while it holds the sockets. The jar runs
/// on, its `SIGPIPE` at the default.
fn fail_an_exec(jar: &Jar, owner: &mut Client) {
    let flaky = script(
        jar,
        "jar-flaky",
        "hold-handover) chmod -x \"$0\"; exec \"$REAL\" \"$@\" ;;",
    );
    owner
        .send(&Msg::Upgrade {
            binary: flaky.display().to_string(),
        })
        .expect("upgrade");
    let m = upgrade_answer_pinging(owner, |m| {
        matches!(m, Msg::Refused { .. } | Msg::Upgraded { .. })
    });
    assert!(
        matches!(
            m,
            Msg::Refused {
                reason: spyc_jar::proto::reason::UPGRADE_FAILED,
                ..
            }
        ),
        "the exec fails: {m:?}"
    );
}

/// A new binary at `name` in the jar's directory: `case` does `arm` for its
/// first argument, and runs the real jar for anything else.
fn script(jar: &Jar, name: &str, arm: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = jar.dir.path().join(name);
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nREAL='{}'\ncase \"$1\" in\n{arm}\n*) exec \"$REAL\" \"$@\" ;;\nesac\n",
            env!("CARGO_BIN_EXE_spyc-jar")
        ),
    )
    .expect("write the binary");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

/// Review finding F7: after a failed `exec`, a validator that goes without
/// reading the blob leaves the jar's write of it answering `EPIPE`, and the
/// jar refuses the upgrade (reason 6) and runs on. The blob carries the
/// program's 2 MB of output, far more than a socket holds, so the write is
/// still waiting when the validator goes. On macOS it is the socketpair's
/// `SO_NOSIGPIPE` that keeps the jar alive (SPYC-TRAP `jar-no-sigpipe`).
#[test]
fn a_validator_gone_mid_blob_does_not_end_a_jar_whose_exec_failed() {
    let mut jar = Jar::start(&[], &sh("head -c 2000000 /dev/zero | tr '\\0' a; cat"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    read_until(&mut owner, true, |s| s.text.len() >= 2_000_000);
    fail_an_exec(&jar, &mut owner);
    let gone = script(&jar, "jar-gone", "validate-handover) exit 1 ;;");
    owner
        .send(&Msg::Upgrade {
            binary: gone.display().to_string(),
        })
        .expect("upgrade");
    // The jar ending as it writes to the validator that has gone ends the
    // owner's connection: `recv` fails.
    let answer = upgrade_answer_pinging(&mut owner, |m| {
        matches!(m, Msg::Refused { .. } | Msg::Upgraded { .. })
    });
    assert!(
        matches!(
            answer,
            Msg::Refused {
                reason: spyc_jar::proto::reason::UPGRADE_REJECTED,
                ..
            }
        ),
        "{answer:?}"
    );
    owner.send(&Msg::Ping { n: 4 }).expect("ping");
    recv_matching(&mut owner, |m| matches!(m, Msg::Pong { n: 4 }));
}

/// The holder of an upgrade after a failed `exec` has gone before the jar's
/// handshake with it: the handshake answers `EPIPE`, the upgrade fails
/// (reason 7) and the jar runs on. It was killed by `SIGPIPE`, since the
/// handshake's `sendmsg` went without `MSG_NOSIGNAL` (the addendum's P3-6).
///
/// The holder's exit comes first by construction: the test traces the jar,
/// and holds the thread that started the holder at that start until the
/// holder has gone.
#[cfg(target_os = "linux")]
#[test]
fn a_holder_gone_before_its_handshake_does_not_end_a_jar_whose_exec_failed() {
    let mut jar = Jar::start(&[], &sh("cat"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    fail_an_exec(&jar, &mut owner);
    let quick = script(&jar, "jar-quick", "hold-handover) exit 0 ;;");
    let pid = jar.process.id();
    let (ready, traced) = std::sync::mpsc::channel();
    let tracer = std::thread::spawn(move || trace::hold_back_the_holder(pid, &ready));
    traced
        .recv_timeout(WAIT)
        .expect("the jar traced")
        .expect("tracing the jar");
    owner
        .send(&Msg::Upgrade {
            binary: quick.display().to_string(),
        })
        .expect("upgrade");
    let held = tracer.join().expect("the tracer");
    assert!(held.is_ok(), "the holder held back: {held:?}");
    // The jar ending as it sends to the holder that has gone ends the
    // owner's connection: `recv` fails.
    let answer = upgrade_answer_pinging(&mut owner, |m| {
        matches!(m, Msg::Refused { .. } | Msg::Upgraded { .. })
    });
    assert!(
        matches!(
            answer,
            Msg::Refused {
                reason: spyc_jar::proto::reason::UPGRADE_FAILED,
                ..
            }
        ),
        "{answer:?}"
    );
    owner.send(&Msg::Ping { n: 9 }).expect("ping");
    recv_matching(&mut owner, |m| matches!(m, Msg::Pong { n: 9 }));
}

/// Tracing a jar to order its upgrade's steps against its holder's.
#[cfg(target_os = "linux")]
mod trace {
    use std::collections::BTreeSet;
    use std::time::Instant;

    use nix::sys::ptrace::{self, Event, Options};
    use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
    use nix::unistd::Pid;

    use super::WAIT;

    /// Trace every thread of jar `jar`, say so on `ready`, and let the jar
    /// run until it starts its third process from now: an upgrade's first
    /// validator, its parked one, then its holder. The thread that started
    /// the holder is held there until the holder has gone, then let go.
    /// Tracing ends with the thread this runs on, which must end next.
    pub fn hold_back_the_holder(
        jar: u32,
        ready: &std::sync::mpsc::Sender<Result<(), String>>,
    ) -> Result<(), String> {
        let traced = seize(jar);
        let failed = traced.as_ref().err().cloned();
        let _ = ready.send(traced.as_ref().map(|_| ()).map_err(Clone::clone));
        if let Some(e) = failed {
            return Err(e);
        }
        let mut threads = traced?;
        let mut children = BTreeSet::new();
        let mut started = 0;
        let until = Instant::now() + WAIT;
        while Instant::now() < until {
            let mut quiet = true;
            let mut all = threads.clone();
            all.extend(children.iter().copied());
            for t in all {
                let flags = WaitPidFlag::WNOHANG | WaitPidFlag::__WALL;
                let Ok(status) = waitpid(t, Some(flags)) else {
                    threads.remove(&t);
                    children.remove(&t);
                    continue;
                };
                quiet &= status == WaitStatus::StillAlive;
                match status {
                    WaitStatus::PtraceEvent(t, _, e) if e == Event::PTRACE_EVENT_CLONE as i32 => {
                        threads.insert(event_pid(t)?);
                        let _ = ptrace::cont(t, None);
                    }
                    WaitStatus::PtraceEvent(t, _, e)
                        if e == Event::PTRACE_EVENT_FORK as i32
                            || e == Event::PTRACE_EVENT_VFORK as i32 =>
                    {
                        let child = event_pid(t)?;
                        started += 1;
                        if started == 3 {
                            let_go_until_gone(child)?;
                            let _ = ptrace::cont(t, None);
                            return Ok(());
                        }
                        children.insert(child);
                        let _ = ptrace::cont(t, None);
                    }
                    // A child's first stop: it runs untraced from here.
                    WaitStatus::PtraceEvent(t, _, _) | WaitStatus::Stopped(t, _)
                        if children.contains(&t) =>
                    {
                        let _ = ptrace::detach(t, None);
                        children.remove(&t);
                    }
                    // A new thread's first stop, or a group stop.
                    WaitStatus::PtraceEvent(t, _, _) => {
                        let _ = ptrace::cont(t, None);
                    }
                    // A signal on its way to the jar goes on to it.
                    WaitStatus::Stopped(t, signal) => {
                        let _ = ptrace::cont(t, Some(signal));
                    }
                    WaitStatus::Exited(t, _) | WaitStatus::Signaled(t, _, _) => {
                        threads.remove(&t);
                    }
                    _ => {}
                }
            }
            if quiet {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        Err(format!("the jar started {started} processes, not a holder"))
    }

    /// Trace every thread of process `pid`, and each one it starts.
    fn seize(pid: u32) -> Result<BTreeSet<Pid>, String> {
        let options = Options::PTRACE_O_TRACECLONE
            | Options::PTRACE_O_TRACEFORK
            | Options::PTRACE_O_TRACEVFORK;
        let mut threads = BTreeSet::new();
        for entry in std::fs::read_dir(format!("/proc/{pid}/task")).map_err(|e| e.to_string())? {
            let tid = entry
                .map_err(|e| e.to_string())?
                .file_name()
                .to_string_lossy()
                .parse::<i32>()
                .map_err(|e| e.to_string())?;
            let tid = Pid::from_raw(tid);
            ptrace::seize(tid, options).map_err(|e| format!("trace thread {tid}: {e}"))?;
            threads.insert(tid);
        }
        Ok(threads)
    }

    /// The pid an event stop of `t` names.
    fn event_pid(t: Pid) -> Result<Pid, String> {
        let raw = ptrace::getevent(t).map_err(|e| e.to_string())?;
        Ok(Pid::from_raw(
            i32::try_from(raw).map_err(|e| e.to_string())?,
        ))
    }

    /// Let child `child`, traced from its start and stopped there, run on
    /// untraced, and wait until it has gone.
    fn let_go_until_gone(child: Pid) -> Result<(), String> {
        let until = Instant::now() + WAIT;
        loop {
            match waitpid(child, Some(WaitPidFlag::WNOHANG | WaitPidFlag::__WALL)) {
                Ok(WaitStatus::StillAlive) => {}
                Ok(_) => break,
                Err(e) => return Err(format!("the holder's first stop: {e}")),
            }
            if Instant::now() >= until {
                return Err("the holder never stopped at its start".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        ptrace::detach(child, None).map_err(|e| format!("let the holder go: {e}"))?;
        while Instant::now() < until {
            let state = std::fs::read_to_string(format!("/proc/{child}/stat")).unwrap_or_default();
            // A zombie or nothing: its descriptors are closed either way.
            let after_name = state.rsplit_once(')').map_or("", |(_, rest)| rest);
            if state.is_empty() || after_name.trim_start().starts_with('Z') {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Err("the holder never went".into())
    }
}
