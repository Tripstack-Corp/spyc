//! Review finding B1: a thread the OS refuses the jar is the failure of the
//! connection or the upgrade that wanted it, with the jar's state put back:
//! the jar goes on serving, a lease it can't serve is free for the next
//! owner, and an upgrade it can't run is busy.
//!
//! On Linux the refusal is the OS's own: the jar's soft `RLIMIT_NPROC` goes
//! to 0 while a test needs it, so every thread it asks for is refused with
//! `EAGAIN`. macOS's limit counts processes, not threads, and nothing outside
//! a process can make macOS refuse it a thread, so there, and for the threads
//! no limit reaches alone (an upgrade's validator's, which start after a
//! process does), a debug build's jar refuses the threads
//! `SPYC_JAR_THREAD_FAULT` names, once each.

use super::*;
#[cfg(target_os = "linux")]
use rustix::process::{Pid, Resource, Rlimit};

#[cfg(target_os = "linux")]
/// The jar refused every new thread while this lives (its soft
/// `RLIMIT_NPROC` at 0, below any user's count), and given them again when
/// it is dropped, a failed assertion included.
struct NoThreads {
    pid: Pid,
    was: Rlimit,
}

#[cfg(target_os = "linux")]
impl NoThreads {
    fn on(jar: &Jar) -> Self {
        let pid =
            Pid::from_raw(i32::try_from(jar.process.id()).expect("a pid")).expect("the jar's pid");
        let hard = rustix::process::getrlimit(Resource::Nproc).maximum;
        let was = rustix::process::prlimit(
            Some(pid),
            Resource::Nproc,
            Rlimit {
                current: Some(0),
                maximum: hard,
            },
        )
        .expect("lower the jar's RLIMIT_NPROC");
        Self { pid, was }
    }
}

#[cfg(target_os = "linux")]
impl Drop for NoThreads {
    fn drop(&mut self) {
        let _ = rustix::process::prlimit(Some(self.pid), Resource::Nproc, self.was);
    }
}

/// How many threads the jar runs.
#[cfg(target_os = "linux")]
fn tasks(jar: &Jar) -> usize {
    std::fs::read_dir(format!("/proc/{}/task", jar.process.id())).map_or(0, Iterator::count)
}

/// A connection that arrives while the jar can't have a thread for it is
/// closed unanswered, and the jar serves on: the acceptor used to panic,
/// ending the jar and its program.
#[cfg(target_os = "linux")]
#[test]
fn a_connection_the_jar_has_no_thread_for_is_closed_and_the_jar_serves_on() {
    let mut jar = Jar::start(&[], &sh("cat"));
    let mut owner = jar.connect(Role::Owner);
    {
        let _refused = NoThreads::on(&jar);
        let mut s = UnixStream::connect(&jar.socket).expect("connect");
        s.write_all(&LOOKUP_HELLO).expect("hello");
        s.set_read_timeout(Some(WAIT)).expect("timeout");
        let mut answer = Vec::new();
        let read = s.read_to_end(&mut answer);
        let closed = match &read {
            Ok(_) => true,
            Err(e) => e.kind() == std::io::ErrorKind::ConnectionReset,
        };
        assert!(
            closed && answer.is_empty(),
            "a connection the jar has no thread for is closed unanswered: {read:?}, {} bytes",
            answer.len()
        );
    }
    let described = describe_within(&jar.socket);
    assert!(
        described.is_ok(),
        "the jar serves on after a connection it had no thread for: {described:?}; the jar: {:?}",
        jar.process.try_wait()
    );
    owner.send(&Msg::Ping { n: 7 }).expect("ping");
    recv_matching(&mut owner, |m| matches!(m, Msg::Pong { n: 7 }));
}

/// An owner whose writer the jar can't start is closed, and its lease goes
/// with it: the lease used to stay with a connection nothing served, and every
/// later owner was refused for the jar's life.
#[cfg(target_os = "linux")]
#[test]
fn an_owner_the_jar_has_no_writer_for_leaves_the_lease_to_the_next() {
    let jar = Jar::start(&[], &sh("cat"));
    // A jar not held for its owner runs its acceptor and four threads: the
    // PTY's reader and writer, the program's waiter and the reaper.
    let idle = 5;
    eventually("the jar to listen, idle", || {
        jar.socket.exists() && tasks(&jar) == idle
    });
    let mut s = UnixStream::connect(&jar.socket).expect("connect");
    // The jar's thread for this connection waits for its hello; the writer
    // it starts once the hello makes the connection its owner is refused.
    eventually("the jar's thread for the connection", || tasks(&jar) > idle);
    {
        let _refused = NoThreads::on(&jar);
        let hello = spyc_jar::proto::encode(&Msg::Hello {
            version: 1,
            role: Role::Owner,
            token: TOKEN.to_vec(),
        });
        s.write_all(&hello).expect("hello");
        eventually("the connection's thread to be done with it", || {
            tasks(&jar) == idle
        });
    }
    drop(s);
    eventually("the lease to pass to the next owner", || {
        try_connect(&jar, Role::Owner, TOKEN).is_ok()
    });
}

/// An upgrade the jar can't start a thread for is busy, and the next one
/// runs: the owner's reader used to panic with the upgrade claimed, leaving a
/// jar that refused every later upgrade and an owner nothing heard.
#[cfg(target_os = "linux")]
#[test]
fn an_upgrade_the_jar_has_no_thread_for_is_busy_and_the_next_one_runs() {
    let mut jar = Jar::start(&[], &sh("cat"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let next = next_binary(jar.dir.path());
    let answer = {
        let _refused = NoThreads::on(&jar);
        owner
            .send(&Msg::Upgrade {
                binary: next.display().to_string(),
            })
            .expect("upgrade");
        upgrade_answer_pinging(&mut owner, |m| {
            matches!(m, Msg::Refused { .. } | Msg::Upgraded { .. })
        })
    };
    assert!(
        matches!(
            answer,
            Msg::Refused {
                reason: spyc_jar::proto::reason::UPGRADE_BUSY,
                ..
            }
        ),
        "{answer:?}"
    );
    let again = upgrade(&jar, &mut owner);
    assert!(again.is_ok(), "the next upgrade runs: {again:?}");
}

/// [`Jar::start`], the threads named in `names` refused once each.
fn refusing(names: &str, argv: &[&str]) -> Jar {
    Jar::start_env(
        &[("SPYC_JAR_THREAD_FAULT", std::path::Path::new(names))],
        &[],
        argv,
    )
}

/// A connection whose thread is refused is closed unanswered; the next is
/// served.
#[test]
fn a_connection_whose_thread_is_refused_is_closed_and_the_next_served() {
    let jar = refusing("jar-conn", &sh("cat"));
    let first = try_connect(&jar, Role::Owner, TOKEN);
    assert!(first.is_err(), "the first connection is closed");
    let next = try_connect(&jar, Role::Owner, TOKEN);
    assert!(next.is_ok(), "the next is served: {:?}", next.err());
}

/// An owner whose writer is refused leaves the lease to the next.
#[test]
fn an_owner_whose_writer_is_refused_leaves_the_lease_to_the_next() {
    let jar = refusing("jar-writer", &sh("cat"));
    let first = try_connect(&jar, Role::Owner, TOKEN);
    assert!(first.is_err(), "the first owner is closed");
    let next = try_connect(&jar, Role::Owner, TOKEN);
    assert!(next.is_ok(), "the next owner is welcomed: {:?}", next.err());
}

/// An upgrade whose thread, or one of its validator's, is refused is busy,
/// and the next one runs.
#[test]
fn an_upgrade_whose_threads_are_refused_is_busy_and_the_next_one_runs() {
    for name in ["jar-upgrade", "jar-blob-in", "jar-proof", "jar-why"] {
        let mut jar = refusing(name, &sh("cat"));
        let mut owner = jar.connect(Role::Owner);
        owner.send(&Msg::Replay { from: 0 }).expect("replay");
        let first = upgrade(&jar, &mut owner);
        assert!(
            first.as_ref().is_err_and(|e| e.starts_with("refused (5)")),
            "{name} refused: the upgrade is busy: {first:?}"
        );
        let again = upgrade(&jar, &mut owner);
        assert!(
            again.is_ok(),
            "{name} refused: the next upgrade runs: {again:?}"
        );
    }
}

/// A holder whose thread for the blob is refused says so before it goes,
/// and the upgrade is busy (5), as any start the machine can't give now is
/// (review finding A2, the owner's choice of 2026-11-25): it used to be read
/// as the binary's failure (7), which stops upgrades for the session. The old
/// jar runs on.
#[test]
#[ignore = "red until A2 is fixed for a holder refused a thread (fix/jar-upgrade-answers)"]
fn a_holder_whose_thread_is_refused_is_busy_and_the_jar_runs_on() {
    let mut jar = refusing("jar-blob-out", &sh("cat"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let answer = upgrade(&jar, &mut owner);
    assert!(
        answer.as_ref().is_err_and(|e| e.starts_with("refused (5)")),
        "{answer:?}"
    );
    owner.send(&Msg::Ping { n: 3 }).expect("ping");
    recv_matching(&mut owner, |m| matches!(m, Msg::Pong { n: 3 }));
}

/// A new image whose threads are refused as it starts holds what crossed
/// rather than lose the program: the connection hears reason 7, and the
/// program lives.
#[test]
fn a_new_image_whose_threads_are_refused_holds_the_program() {
    // Only a new image starts a thread for each connection that crossed.
    let mut jar = refusing("jar-resumed", &sh("cat"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let program = jar.program.expect("the program's pid");
    let answer = upgrade(&jar, &mut owner);
    assert!(
        answer.as_ref().is_err_and(|e| e.starts_with("refused (7)")),
        "{answer:?}"
    );
    assert!(running(program), "the program lives");
    let held = described(&jar, "held");
    assert!(!held.is_empty(), "the jar says it holds");
}
