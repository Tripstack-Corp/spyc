//! Review finding B1: a thread the OS refuses the jar is the failure of the
//! connection or the upgrade that wanted it, with the jar's state put back:
//! the jar goes on serving, a lease it can't serve is free for the next
//! owner, and an upgrade it can't run is busy.
//!
//! The refusal here is the OS's own: the jar's soft `RLIMIT_NPROC` goes to 0
//! while a test needs it, so every thread it asks for is refused with
//! `EAGAIN`. That is Linux only, where the limit counts threads; macOS's
//! counts processes, and nothing outside a process can make macOS refuse it a
//! thread.

use super::*;
use rustix::process::{Pid, Resource, Rlimit};

/// The jar refused every new thread while this lives (its soft
/// `RLIMIT_NPROC` at 0, below any user's count), and given them again when
/// it is dropped, a failed assertion included.
struct NoThreads {
    pid: Pid,
    was: Rlimit,
}

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

impl Drop for NoThreads {
    fn drop(&mut self) {
        let _ = rustix::process::prlimit(Some(self.pid), Resource::Nproc, self.was);
    }
}

/// How many threads the jar runs.
fn tasks(jar: &Jar) -> usize {
    std::fs::read_dir(format!("/proc/{}/task", jar.process.id())).map_or(0, Iterator::count)
}

/// A connection that arrives while the jar can't have a thread for it is
/// closed unanswered, and the jar serves on: the acceptor used to panic,
/// ending the jar and its program.
#[test]
#[ignore = "red until B1 is fixed (fix/jar-thread-refusal)"]
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
#[test]
#[ignore = "red until B1 is fixed (fix/jar-thread-refusal)"]
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
#[test]
#[ignore = "red until B1 is fixed (fix/jar-thread-refusal)"]
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
