//! The jar binary, driven as spyc would drive it: one real program on a PTY,
//! one or two connections, nothing of spyc's.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use spyc_jar::client::Client;
use spyc_jar::proto::{Msg, Role, SNAPSHOT_VT, Snapshot};

const WAIT: Duration = Duration::from_secs(10);
const TOKEN: &[u8] = b"test-token-0123456789";

struct Jar {
    _dir: tempfile::TempDir,
    socket: PathBuf,
    process: Child,
    program: Option<u32>,
}

impl Jar {
    /// Start a jar for `argv` with `extra` options. The socket lives under
    /// `/tmp`, because macOS caps a socket's path at about 104 bytes.
    fn start(extra: &[&str], argv: &[&str]) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("jar")
            .tempdir_in("/tmp")
            .expect("tempdir");
        let socket = dir.path().join("j.sock");
        let process = Command::new(env!("CARGO_BIN_EXE_spyc-jar"))
            .arg("--socket")
            .arg(&socket)
            .args(extra)
            .arg("--")
            .args(argv)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the jar");
        let mut process = process;
        // R26: the token goes over stdin, which then closes.
        process
            .stdin
            .take()
            .expect("the jar's stdin")
            .write_all(TOKEN)
            .expect("hand the jar its token");
        Self {
            _dir: dir,
            socket,
            process,
            program: None,
        }
    }

    fn connect(&mut self, role: Role) -> Client {
        let (client, welcome) = Client::connect(&self.socket, role, WAIT, TOKEN).expect("connect");
        if let Msg::Welcome { child_pid, .. } = welcome {
            self.program = Some(child_pid);
        }
        client
    }

    fn welcome(&mut self, role: Role) -> (Client, Msg) {
        let (client, welcome) = Client::connect(&self.socket, role, WAIT, TOKEN).expect("connect");
        if let Msg::Welcome { child_pid, .. } = welcome {
            self.program = Some(child_pid);
        }
        (client, welcome)
    }

    fn ended_within(&mut self, wait: Duration) -> bool {
        let until = Instant::now() + wait;
        while Instant::now() < until {
            if self.process.try_wait().ok().flatten().is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

impl Drop for Jar {
    fn drop(&mut self) {
        if let Some(pid) = self.program.and_then(|p| i32::try_from(p).ok())
            && let Some(pid) = rustix::process::Pid::from_raw(pid)
        {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// What a client saw: output bytes in order, checked contiguous, and the
/// other frames.
#[derive(Default)]
struct Seen {
    text: Vec<u8>,
    first_offset: Option<u64>,
    next_offset: Option<u64>,
    frames: Vec<Msg>,
}

impl Seen {
    fn take(&mut self, msg: Msg) {
        if let Msg::Output { offset, bytes } = &msg {
            if let Some(next) = self.next_offset {
                assert_eq!(*offset, next, "output arrives contiguous");
            }
            self.first_offset.get_or_insert(*offset);
            self.next_offset = Some(offset + bytes.len() as u64);
            self.text.extend(bytes);
        } else {
            self.frames.push(msg);
        }
    }

    fn contains(&self, needle: &str) -> bool {
        String::from_utf8_lossy(&self.text).contains(needle)
    }
}

/// Read until `done` holds, acknowledging output as it arrives when `ack`.
fn read_until(client: &mut Client, ack: bool, done: impl Fn(&Seen) -> bool) -> Seen {
    let mut seen = Seen::default();
    let until = Instant::now() + WAIT;
    while !done(&seen) {
        assert!(
            Instant::now() < until,
            "timed out; saw {} bytes, frames {:?}",
            seen.text.len(),
            seen.frames
        );
        if let Some(msg) = client.recv(Duration::from_millis(200)).expect("recv") {
            seen.take(msg);
            if ack && let Some(next) = seen.next_offset {
                client.send(&Msg::Ack { offset: next }).expect("ack");
            }
        }
    }
    seen
}

fn sh(script: &str) -> Vec<&str> {
    vec!["/bin/sh", "-c", script]
}

#[test]
fn input_reaches_the_program_and_its_output_comes_back() {
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    owner
        .send(&Msg::Input {
            bytes: b"hello jar\n".to_vec(),
        })
        .unwrap();
    let seen = read_until(&mut owner, true, |s| s.contains("hello jar\r\nhello jar"));
    assert_eq!(seen.first_offset, Some(0));
}

/// R11: a live owner that stops acknowledging holds the program a window
/// behind, and nothing is lost when it catches up.
#[test]
fn a_live_owner_paces_the_program_and_loses_nothing() {
    let window = 65_536u64;
    let mut jar = Jar::start(
        &["--window", &window.to_string()],
        &sh("head -c 2000000 /dev/zero | tr '\\000' a; echo; echo END"),
    );
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).unwrap();

    let mut stalled = Seen::default();
    let until = Instant::now() + Duration::from_millis(800);
    while Instant::now() < until {
        if let Some(m) = owner.recv(Duration::from_millis(50)).unwrap() {
            stalled.take(m);
        }
    }
    // What the program printed before the owner connected is sent whatever
    // the window; the window stops the program, so with no acks delivery
    // stops growing.
    let mut more = Seen::default();
    let until = Instant::now() + Duration::from_millis(800);
    while Instant::now() < until {
        if let Some(m) = owner.recv(Duration::from_millis(50)).unwrap() {
            more.take(m);
        }
    }
    assert!(
        more.text.is_empty(),
        "with no acks the program kept printing: {} more bytes",
        more.text.len()
    );
    let got = stalled.text.len() as u64;
    assert!(got < 2_000_000, "the program was paused before it finished");

    owner.send(&Msg::Ack { offset: got }).unwrap();
    let mut rest = read_until(&mut owner, true, |s| s.contains("END"));
    assert_eq!(
        rest.first_offset,
        Some(got),
        "picks up where the stall left off"
    );
    rest.text.splice(0..0, stalled.text);
    let a_count = rest.text.iter().filter(|b| **b == b'a').count();
    assert_eq!(a_count, 2_000_000, "every byte arrived once");
    assert!(
        rest.frames.iter().all(|f| !matches!(f, Msg::Gap { .. })),
        "{:?}",
        rest.frames
    );
}

/// The crash case: an owner vanishes mid-stream, the program runs on, and the
/// next owner replays from what the first one acknowledged.
#[test]
fn a_new_owner_replays_from_what_the_last_one_acknowledged() {
    let mut jar = Jar::start(
        &["--ring", "8388608"],
        &sh("i=0; while [ $i -lt 20000 ]; do echo line$i; i=$((i+1)); done; echo END"),
    );
    let mut first = jar.connect(Role::Owner);
    first.send(&Msg::Replay { from: 0 }).unwrap();
    let early = read_until(&mut first, true, |s| s.text.len() > 20_000);
    let acked = early.next_offset.unwrap();
    first.drop_connection();
    std::thread::sleep(Duration::from_millis(300));

    let mut second = jar.connect(Role::Owner);
    second.send(&Msg::Replay { from: acked }).unwrap();
    let rest = read_until(&mut second, true, |s| s.contains("END"));
    assert_eq!(rest.first_offset, Some(acked), "no byte lost or repeated");
    assert!(rest.contains("line19999"));
    assert!(
        rest.frames.iter().all(|f| !matches!(f, Msg::Gap { .. })),
        "{:?}",
        rest.frames
    );
}

/// R12/R12a: with no owner, a full log drops its oldest bytes, and a later
/// replay is told which.
#[test]
fn an_ownerless_jar_drops_the_oldest_bytes_and_says_which() {
    let mut jar = Jar::start(
        &["--ring", "65536"],
        &sh("head -c 1000000 /dev/zero | tr '\\000' b; echo; echo END; sleep 5"),
    );
    // Until the program has printed it all, as the jar describes its log:
    // the same twice running, past the first byte.
    let log = || {
        field(
            &spyc_jar::client::describe(&jar.socket).expect("describe"),
            "log",
        )
    };
    let until = Instant::now() + WAIT;
    let mut last = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(200));
        let now = log();
        if now == last && !now.ends_with("from 0") {
            break;
        }
        assert!(Instant::now() < until, "the log never settled: {now}");
        last = now;
    }
    let (mut owner, welcome) = jar.welcome(Role::Owner);
    let Msg::Welcome { base, head, .. } = welcome else {
        unreachable!()
    };
    assert!(
        base > 0,
        "the oldest bytes were dropped: base {base}, head {head}"
    );
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    let seen = read_until(&mut owner, true, |s| s.contains("END"));
    assert_eq!(seen.frames.first(), Some(&Msg::Gap { from: 0, to: base }));
    assert_eq!(seen.first_offset, Some(base));
}

#[test]
fn the_exit_is_reported_and_collecting_it_ends_the_jar() {
    let mut jar = Jar::start(&[], &sh("echo bye; exit 7"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    let seen = read_until(&mut owner, true, |s| {
        s.frames.iter().any(|f| matches!(f, Msg::Exited { .. }))
    });
    assert!(seen.contains("bye"), "output before the exit");
    assert!(
        seen.frames.contains(&Msg::Exited {
            code: 7,
            signaled: false
        }),
        "{:?}",
        seen.frames
    );
    owner.send(&Msg::AckExit).unwrap();
    assert!(
        jar.ended_within(Duration::from_secs(3)),
        "the jar ends once its exit is collected"
    );
    assert!(!jar.socket.exists(), "and removes its socket");
}

/// R9a: a resize is logged at the offset it took effect.
#[test]
fn a_resize_is_logged_where_it_happened() {
    let mut jar = Jar::start(&[], &sh("read go; stty size; sleep 1"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    owner
        .send(&Msg::Resize {
            rows: 41,
            cols: 101,
            px_w: 0,
            px_h: 0,
        })
        .unwrap();
    owner
        .send(&Msg::Input {
            bytes: b"\n".to_vec(),
        })
        .unwrap();
    let seen = read_until(&mut owner, true, |s| s.contains("41 101"));
    assert!(
        seen.frames.iter().any(|f| matches!(
            f,
            Msg::Resized {
                rows: 41,
                cols: 101,
                ..
            }
        )),
        "{:?}",
        seen.frames
    );
}

#[test]
fn a_stored_snapshot_is_handed_to_the_next_client() {
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    let owner = jar.connect(Role::Owner);
    let snap = Snapshot {
        format: SNAPSHOT_VT,
        offset: 0,
        rows: 24,
        cols: 80,
        blob: b"\x1b[2Jscreen".to_vec(),
    };
    owner.send(&Msg::StoreSnapshot(snap.clone())).unwrap();
    owner.drop_connection();
    std::thread::sleep(Duration::from_millis(200));
    let mut next = jar.connect(Role::Reader);
    next.send(&Msg::FetchSnapshot).unwrap();
    let reply = next.recv(WAIT).unwrap();
    assert_eq!(reply, Some(Msg::Snapshot(Some(snap))));
}

/// R21: an owner that goes silent past the deadline loses its lease, even
/// though its socket is still open, and another can take the jar.
#[test]
fn a_silent_owner_loses_the_jar() {
    let mut jar = Jar::start(&["--deadline-ms", "300"], &["/bin/cat"]);
    let mut silent = jar.connect(Role::Owner);
    std::thread::sleep(Duration::from_millis(800));
    let after = silent.recv(Duration::from_millis(500));
    assert!(
        !matches!(after, Ok(None)),
        "the jar closed the silent owner's connection: {after:?}"
    );
    let _next = jar.connect(Role::Owner);
}

#[test]
fn one_owner_at_a_time_and_a_reader_cannot_type() {
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    let _owner = jar.connect(Role::Owner);
    let second = Client::connect(&jar.socket, Role::Owner, WAIT, TOKEN);
    assert!(
        second.is_err(),
        "a second owner is refused while the first is live"
    );

    let mut reader = jar.connect(Role::Reader);
    reader
        .send(&Msg::Input {
            bytes: b"x".to_vec(),
        })
        .unwrap();
    let reply = reader.recv(WAIT).unwrap();
    assert!(matches!(reply, Some(Msg::Refused { .. })), "{reply:?}");
}

#[test]
fn ping_is_answered() {
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Ping { n: 41 }).unwrap();
    assert_eq!(owner.recv(WAIT).unwrap(), Some(Msg::Pong { n: 41 }));
}

/// R4: the jar reports the PTY's foreground process group, which only it can
/// read: the shell's at first, then a job's while the shell runs one.
#[test]
fn the_foreground_process_group_is_reported_as_it_changes() {
    let mut jar = Jar::start(&[], &["/bin/sh", "-i"]);
    let (mut owner, welcome) = jar.welcome(Role::Owner);
    let Msg::Welcome { child_pid, .. } = welcome else {
        unreachable!()
    };
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    let shell = read_until(&mut owner, true, |s| {
        s.frames.contains(&Msg::Foreground { pgid: child_pid })
    });
    assert!(!shell.frames.is_empty());
    owner
        .send(&Msg::Input {
            bytes: b"sleep 3\n".to_vec(),
        })
        .unwrap();
    let job = read_until(&mut owner, true, |s| {
        s.frames
            .iter()
            .any(|f| matches!(f, Msg::Foreground { pgid } if *pgid != child_pid))
    });
    assert!(!job.frames.is_empty());
}

// Stage 3 (R33): the jar replaces its own binary under a running program.

/// A program that counts forever and echoes back each line it's sent, so a
/// lost or repeated byte shows as a broken count and input as `got=`.
fn counter() -> Vec<&'static str> {
    vec![
        "/bin/sh",
        "-c",
        "(i=0; while :; do i=$((i+1)); echo n=$i; sleep 0.005; done) & while read l; do echo got=$l; done",
    ]
}

/// The counts seen, which must run 1, 2, 3… with nothing missing or doubled.
fn counts(seen: &Seen) -> Vec<u64> {
    String::from_utf8_lossy(&seen.text)
        .lines()
        .filter_map(|l| l.trim().strip_prefix("n=")?.parse().ok())
        .collect()
}

fn assert_unbroken(seen: &Seen) {
    let c = counts(seen);
    assert!(!c.is_empty());
    for w in c.windows(2) {
        assert_eq!(w[1], w[0] + 1, "the count runs on unbroken");
    }
}

/// A copy of the jar's binary under another name, so the upgrade can be seen
/// to have run it.
fn next_binary(dir: &std::path::Path) -> PathBuf {
    let next = dir.join("jar-next");
    std::fs::copy(env!("CARGO_BIN_EXE_spyc-jar"), &next).expect("copy the jar binary");
    next
}

#[test]
fn an_upgrade_keeps_the_program_its_pty_and_every_connection() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    let mut reader = jar.connect(Role::Reader);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    reader.send(&Msg::Replay { from: 0 }).expect("replay");
    let next = next_binary(jar._dir.path());
    let mut seen = read_until(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 30)
    });
    let before = jar.process.id();

    owner
        .send(&Msg::Upgrade {
            binary: next.display().to_string(),
        })
        .expect("upgrade");
    let until = Instant::now() + WAIT;
    while !seen
        .frames
        .iter()
        .any(|f| matches!(f, Msg::Upgraded { .. }))
    {
        assert!(
            Instant::now() < until,
            "no Upgraded; frames {:?}",
            seen.frames
        );
        if let Some(m) = owner.recv(Duration::from_millis(200)).expect("recv") {
            seen.take(m);
            if let Some(next) = seen.next_offset {
                owner.send(&Msg::Ack { offset: next }).expect("ack");
            }
        }
    }
    let build = seen.frames.iter().find_map(|f| match f {
        Msg::Upgraded { build } => Some(build.clone()),
        _ => None,
    });
    assert!(
        build.is_some_and(|b| b.ends_with("jar-next")),
        "the new binary runs"
    );
    assert!(
        jar.process.try_wait().expect("try_wait").is_none(),
        "the jar's process lives on"
    );
    assert_eq!(jar.process.id(), before);

    // The same program, counting on from where it was, still reading input.
    owner
        .send(&Msg::Input {
            bytes: b"after\n".to_vec(),
        })
        .expect("input");
    let at = counts(&seen).last().copied().unwrap_or(0);
    loop {
        assert!(
            Instant::now() < until + WAIT,
            "the program went quiet: last count {:?}, got: {}, tail {:?}, frames {:?}",
            counts(&seen).last(),
            seen.contains("got=after"),
            String::from_utf8_lossy(&seen.text[seen.text.len().saturating_sub(120)..]),
            seen.frames
        );
        if let Some(m) = owner.recv(Duration::from_millis(200)).expect("recv") {
            seen.take(m);
            if let Some(next) = seen.next_offset {
                owner.send(&Msg::Ack { offset: next }).expect("ack");
            }
        }
        if seen.contains("got=after") && counts(&seen).last().is_some_and(|n| *n >= at + 30) {
            break;
        }
    }
    assert_unbroken(&seen);

    // The reader's connection crossed too, and the listener takes new ones.
    let r = read_until(&mut reader, false, |s| {
        s.frames.iter().any(|f| matches!(f, Msg::Upgraded { .. }))
    });
    assert!(r.first_offset == Some(0));
    let (_late, welcome) = jar.welcome(Role::Reader);
    assert!(matches!(welcome, Msg::Welcome { .. }));

    // A resize after the upgrade reaches the PTY.
    owner
        .send(&Msg::Resize {
            rows: 33,
            cols: 99,
            px_w: 0,
            px_h: 0,
        })
        .expect("resize");
    let s = read_until(&mut owner, true, |s| {
        s.frames.iter().any(|f| {
            matches!(
                f,
                Msg::Resized {
                    rows: 33,
                    cols: 99,
                    ..
                }
            )
        })
    });
    assert!(!s.frames.is_empty());
}

#[test]
fn an_upgrade_the_new_binary_refuses_leaves_the_old_jar_running() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_until(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 10)
    });
    for binary in ["/usr/bin/false", "/nonexistent/spyc-jar"] {
        owner
            .send(&Msg::Upgrade {
                binary: binary.into(),
            })
            .expect("upgrade");
        let until = Instant::now() + WAIT;
        loop {
            assert!(Instant::now() < until, "no refusal for {binary}");
            if let Some(m) = owner.recv(Duration::from_millis(200)).expect("recv") {
                if let Msg::Refused { why } = &m {
                    assert!(why.contains("upgrade refused"), "{why}");
                    break;
                }
                seen.take(m);
                if let Some(next) = seen.next_offset {
                    owner.send(&Msg::Ack { offset: next }).expect("ack");
                }
            }
        }
    }
    let at = counts(&seen).last().copied().unwrap_or(0);
    let mut more = read_until(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= at + 20)
    });
    assert!(
        !seen
            .frames
            .iter()
            .chain(&more.frames)
            .any(|f| matches!(f, Msg::Upgraded { .. }))
    );
    more.text.splice(0..0, seen.text.clone());
    assert_unbroken(&more);
}

#[test]
fn a_jar_upgrades_again_after_an_upgrade() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let next = next_binary(jar._dir.path());
    let mut seen = read_until(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    for _ in 0..3 {
        let upgrades = |s: &Seen| {
            s.frames
                .iter()
                .filter(|f| matches!(f, Msg::Upgraded { .. }))
                .count()
        };
        let had = upgrades(&seen);
        owner
            .send(&Msg::Upgrade {
                binary: next.display().to_string(),
            })
            .expect("upgrade");
        let until = Instant::now() + WAIT;
        while upgrades(&seen) == had {
            assert!(
                Instant::now() < until,
                "no Upgraded; frames {:?}",
                seen.frames
            );
            if let Some(m) = owner.recv(Duration::from_millis(200)).expect("recv") {
                seen.take(m);
                if let Some(next) = seen.next_offset {
                    owner.send(&Msg::Ack { offset: next }).expect("ack");
                }
            }
        }
    }
    let at = counts(&seen).last().copied().unwrap_or(0);
    let more = read_until(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= at + 10)
    });
    seen.text.extend(more.text);
    assert_unbroken(&seen);
}

#[test]
fn an_upgrade_survives_unix_sockets_closing_elsewhere() {
    // On macOS any unix socket closing runs the collector that flushes a
    // socket whose every reference is in flight, as a jar's connections are
    // across its `exec`. Without the holder, every upgrade here cut them.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let churn = {
        let stop = std::sync::Arc::clone(&stop);
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = std::os::unix::net::UnixStream::pair();
            }
        })
    };
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    let mut reader = jar.connect(Role::Reader);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let next = next_binary(jar._dir.path());
    let mut seen = read_until(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    for round in 1..=3 {
        owner
            .send(&Msg::Upgrade {
                binary: next.display().to_string(),
            })
            .expect("upgrade");
        let more = read_until(&mut owner, true, |s| {
            s.frames.iter().any(|f| matches!(f, Msg::Upgraded { .. }))
        });
        seen.text.extend(more.text);
        let at = counts(&seen).last().copied().unwrap_or(0);
        let more = read_until(&mut owner, true, |s| {
            counts(s).last().is_some_and(|n| *n >= at + 5)
        });
        seen.text.extend(more.text);
        reader.send(&Msg::Ping { n: round }).expect("ping");
        assert!(
            matches!(recv_matching(&mut reader, |m| matches!(m, Msg::Pong { .. })), Msg::Pong { n } if n == round)
        );
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = churn.join();
    assert_unbroken(&seen);
}

// Stage 4: how a jar ends (R32a), and how it describes itself (R31).

fn field(fields: &[(String, String)], key: &str) -> String {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

#[test]
fn a_finite_linger_ends_the_program_once_its_owner_has_gone() {
    for (program, why) in [
        ("sleep 1000", "a program that ends on a hangup"),
        (
            "trap '' HUP; while :; do sleep 1; done",
            "a program that ignores it",
        ),
    ] {
        let mut jar = Jar::start(&["--linger-ownerless-ms", "300"], &sh(program));
        let owner = jar.connect(Role::Owner);
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            jar.process.try_wait().expect("try_wait").is_none(),
            "{why}: alive while owned"
        );
        owner.drop_connection();
        assert!(
            jar.ended_within(Duration::from_secs(6)),
            "{why}: the jar ended after its linger"
        );
        let pid = jar
            .program
            .and_then(|p| i32::try_from(p).ok())
            .and_then(rustix::process::Pid::from_raw);
        assert!(
            pid.is_some_and(|p| rustix::process::test_kill_process(p).is_err()),
            "{why}: the program is gone too"
        );
    }
}

#[test]
fn without_a_linger_the_program_waits_for_an_owner() {
    let mut jar = Jar::start(&[], &sh("sleep 1000"));
    jar.connect(Role::Owner).drop_connection();
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        jar.process.try_wait().expect("try_wait").is_none(),
        "the jar waits"
    );
    let (_again, welcome) = jar.welcome(Role::Owner);
    assert!(
        matches!(welcome, Msg::Welcome { .. }),
        "and a new owner gets it"
    );
}

#[test]
fn a_jar_describes_itself_and_keeps_its_identity_across_an_upgrade() {
    let mut jar = Jar::start(&["--linger-ownerless-ms", "60000"], &sh("sleep 1000"));
    let _owner = jar.connect(Role::Owner);
    let before = spyc_jar::client::describe(&jar.socket).expect("describe");
    assert_eq!(
        field(&before, "program_pid"),
        jar.program.expect("pid").to_string()
    );
    assert_eq!(field(&before, "linger"), "60000 ms");
    assert_eq!(field(&before, "overflow"), "drop");
    assert_eq!(field(&before, "owner"), "connected");
    assert_eq!(field(&before, "pid"), jar.process.id().to_string());

    let next = next_binary(jar._dir.path());
    let (mut asker, _) = Client::connect(&jar.socket, Role::Reader, WAIT, TOKEN).expect("connect");
    asker
        .send(&Msg::Upgrade {
            binary: next.display().to_string(),
        })
        .expect("upgrade");
    read_until(&mut asker, false, |s| {
        s.frames.iter().any(|f| matches!(f, Msg::Upgraded { .. }))
    });
    let after = spyc_jar::client::describe(&jar.socket).expect("describe");
    for key in ["instance", "created", "pid", "program_pid", "linger"] {
        assert_eq!(
            field(&before, key),
            field(&after, key),
            "{key} survives the upgrade"
        );
    }
    assert!(field(&after, "jar").ends_with("jar-next"));
}

// Stage 6: a jar handed on (R22), where its owner's MCP socket is (R18,
// R30a), and the token every role but a lookup presents (R26).

#[test]
fn a_released_jar_opens_only_to_its_new_token() {
    let mut jar = Jar::start(&[], &sh("sleep 1000"));
    let first = jar.connect(Role::Owner);
    let mut reader = jar.connect(Role::Reader);
    first
        .send(&Msg::SetInfo {
            key: "owner_mcp".into(),
            value: "/tmp/spyc/a.sock".into(),
        })
        .expect("set info");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        field(
            &spyc_jar::client::describe(&jar.socket).expect("describe"),
            "owner_mcp"
        ),
        "/tmp/spyc/a.sock"
    );

    // The release, then the Kill a closing tab sends, on the same connection.
    first
        .send(&Msg::Release {
            token: b"successor-token-0001".to_vec(),
        })
        .expect("release");
    let _ = first.send(&Msg::Signal {
        signal: spyc_jar::proto::JarSignal::Kill as i32,
    });
    std::thread::sleep(Duration::from_millis(300));
    let pid = jar
        .program
        .and_then(|p| i32::try_from(p).ok())
        .and_then(rustix::process::Pid::from_raw)
        .expect("pid");
    assert!(
        rustix::process::test_kill_process(pid).is_ok(),
        "the Kill after a release never acts"
    );
    let held = spyc_jar::client::describe(&jar.socket).expect("describe");
    assert_eq!(
        field(&held, "owner_mcp"),
        "",
        "the old owner's socket is forgotten"
    );
    let closed = (0..20).any(|_| reader.recv(Duration::from_millis(100)).is_err());
    assert!(closed, "a reader that came in on the old token is closed");

    let old = Client::connect(&jar.socket, Role::Owner, WAIT, TOKEN);
    assert!(
        old.is_err_and(|e| e.to_string().contains("wrong token")),
        "the old token opens nothing"
    );
    let (next, welcome) = Client::connect(&jar.socket, Role::Owner, WAIT, b"successor-token-0001")
        .expect("successor");
    assert!(matches!(welcome, Msg::Welcome { .. }));
    next.send(&Msg::SetInfo {
        key: "owner_mcp".into(),
        value: "/tmp/spyc/b.sock".into(),
    })
    .expect("set info");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        field(
            &spyc_jar::client::describe(&jar.socket).expect("describe"),
            "owner_mcp"
        ),
        "/tmp/spyc/b.sock"
    );
}

#[test]
fn without_the_token_a_client_can_only_ask_what_the_jar_is() {
    let mut jar = Jar::start(&[], &sh("echo secret-on-screen; sleep 1000"));
    let _owner = jar.connect(Role::Owner);
    for role in [Role::Owner, Role::Reader] {
        for token in [&b""[..], b"test-token-0123456780", b"test-token-012345678"] {
            let got = Client::connect(&jar.socket, role, WAIT, token);
            assert!(
                got.is_err_and(|e| e.to_string().contains("wrong token")),
                "{role:?} with {token:?}"
            );
        }
    }
    let (mut lookup, _) = Client::connect(&jar.socket, Role::Lookup, WAIT, b"").expect("a lookup");
    for msg in [
        Msg::Replay { from: 0 },
        Msg::FetchSnapshot,
        Msg::Input {
            bytes: b"x".to_vec(),
        },
        Msg::Upgrade {
            binary: "/bin/sh".into(),
        },
        Msg::Release {
            token: b"mine-mine-mine-mine".to_vec(),
        },
    ] {
        lookup.send(&msg).expect("send");
        let answer = recv_matching(&mut lookup, |m| {
            matches!(m, Msg::Refused { .. } | Msg::Output { .. })
        });
        assert!(
            matches!(answer, Msg::Refused { .. }),
            "{msg:?} from a lookup: {answer:?}"
        );
    }
    lookup.send(&Msg::Describe).expect("describe");
    let answer = recv_matching(&mut lookup, |m| matches!(m, Msg::Description { .. }));
    let Msg::Description { fields } = answer else {
        unreachable!()
    };
    assert!(
        fields.iter().all(|(_, v)| !v.contains("test-token")),
        "the token is never described"
    );
}

/// The first message `want` accepts, within `WAIT`.
fn recv_matching(client: &mut Client, want: impl Fn(&Msg) -> bool) -> Msg {
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if let Some(m) = client.recv(Duration::from_millis(200)).expect("recv")
            && want(&m)
        {
            return m;
        }
    }
    panic!("no matching message within {WAIT:?}");
}

// The $TMPDIR check: macOS's cleaner may delete a socket left untouched for
// days. The jar puts its address back.

#[test]
fn a_jar_whose_socket_was_deleted_binds_it_again() {
    let mut jar = Jar::start(&[], &sh("sleep 1000"));
    let owner = jar.connect(Role::Owner);
    std::fs::remove_file(&jar.socket).expect("delete the socket");
    std::fs::remove_dir(jar.socket.parent().expect("its directory")).expect("and its directory");
    let back = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        jar.socket.exists()
    });
    assert!(back, "the socket is back within 5 s");
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(jar.socket.parent().expect("dir"))
            .expect("stat")
            .permissions(),
    );
    assert_eq!(mode & 0o777, 0o700, "its directory is private again");
    let (mut reader, _) =
        Client::connect(&jar.socket, Role::Reader, WAIT, TOKEN).expect("a new connection");
    reader.send(&Msg::Ping { n: 7 }).expect("ping");
    assert!(matches!(
        recv_matching(&mut reader, |m| matches!(m, Msg::Pong { .. })),
        Msg::Pong { n: 7 }
    ));
    owner
        .send(&Msg::Ping { n: 1 })
        .expect("the owner's connection is untouched");
}

#[test]
fn a_released_jars_new_token_crosses_an_upgrade() {
    let mut jar = Jar::start(&[], &sh("sleep 1000"));
    let first = jar.connect(Role::Owner);
    first
        .send(&Msg::Release {
            token: b"successor-token-0002".to_vec(),
        })
        .expect("release");
    std::thread::sleep(Duration::from_millis(300));
    let next = next_binary(jar._dir.path());
    let (mut upgrader, _) =
        Client::connect(&jar.socket, Role::Reader, WAIT, b"successor-token-0002").expect("reader");
    upgrader
        .send(&Msg::Upgrade {
            binary: next.display().to_string(),
        })
        .expect("upgrade");
    let done = recv_matching(&mut upgrader, |m| {
        matches!(m, Msg::Upgraded { .. } | Msg::Refused { .. })
    });
    assert!(matches!(done, Msg::Upgraded { .. }), "{done:?}");
    let old = Client::connect(&jar.socket, Role::Owner, WAIT, TOKEN);
    assert!(
        old.is_err_and(|e| e.to_string().contains("wrong token")),
        "the old token still opens nothing"
    );
    Client::connect(&jar.socket, Role::Owner, WAIT, b"successor-token-0002")
        .expect("the new one does");
}
