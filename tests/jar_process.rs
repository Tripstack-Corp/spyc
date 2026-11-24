//! The jar binary, driven as spyc would drive it: one real program on a PTY,
//! one or two connections, nothing of spyc's.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use spyc_jar::client::Client;
use spyc_jar::limits::UPGRADE_AT_MOST;
use spyc_jar::proto::{Msg, Role, SNAPSHOT_VT, Snapshot};

const WAIT: Duration = Duration::from_secs(10);
/// The name a test's jar is started under (its `argv[0]`), as spyc names a
/// pane's jar for its program.
const NAME: &str = "spyc-jar some-program";
const TOKEN: &[u8] = b"test-token-0123456789";

struct Jar {
    dir: tempfile::TempDir,
    socket: PathBuf,
    process: Child,
    program: Option<u32>,
    /// Where the jar's standard error goes, apart from its directory, which
    /// a test may delete: printed when the test fails.
    stderr: tempfile::NamedTempFile,
}

/// A start for a jar at `socket` running `argv` at 24×80, held for its
/// first owner for `hold` milliseconds.
fn start(socket: &std::path::Path, hold: Option<u64>, argv: &[&str]) -> spyc_jar::cli::Start {
    spyc_jar::cli::Start {
        token: TOKEN.to_vec(),
        socket: socket.to_path_buf(),
        cwd: None,
        rows: 24,
        cols: 80,
        hold: hold.map(Duration::from_millis),
        exit_linger: None,
        argv: argv.iter().map(|a| (*a).to_string()).collect(),
    }
}

/// Run the freshly built jar once, untimed, so macOS's first-run scan of it
/// is paid here rather than inside a test's wait.
fn warm(jar: &str) {
    static WARM: std::sync::Once = std::sync::Once::new();
    WARM.call_once(|| {
        let _ = Command::new(jar)
            .arg("build-info")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    });
}

impl Jar {
    /// Start a jar for `argv` with `extra` options. The socket lives under
    /// `/tmp`, because macOS caps a socket's path at about 104 bytes.
    fn start(extra: &[&str], argv: &[&str]) -> Self {
        Self::start_limited(None, extra, argv)
    }

    /// [`Jar::start`], with the jar's open descriptors limited to `nofile`
    /// when it is given.
    fn start_limited(nofile: Option<u32>, extra: &[&str], argv: &[&str]) -> Self {
        Self::spawn(nofile, &[], None, extra, argv)
    }

    /// [`Jar::start`], its exit kept until an owner collects it, as a `!`
    /// capture's is.
    fn keeping_its_exit(extra: &[&str], argv: &[&str]) -> Self {
        Self::spawn_with(
            None,
            &[],
            None,
            extra,
            argv,
            Some(spyc_jar::cli::UNTIL_READ),
        )
    }

    /// [`Jar::start`], with `env` set for the jar.
    fn start_env(env: &[(&str, &std::path::Path)], extra: &[&str], argv: &[&str]) -> Self {
        Self::spawn(None, env, None, extra, argv)
    }

    /// [`Jar::start`], its program held for a first owner for `ms`
    /// (decision M).
    fn holding(ms: u64, extra: &[&str], argv: &[&str]) -> Self {
        Self::spawn(None, &[], Some(ms), extra, argv)
    }

    fn spawn(
        nofile: Option<u32>,
        env: &[(&str, &std::path::Path)],
        hold: Option<u64>,
        extra: &[&str],
        argv: &[&str],
    ) -> Self {
        Self::spawn_with(nofile, env, hold, extra, argv, None)
    }

    fn spawn_with(
        nofile: Option<u32>,
        env: &[(&str, &std::path::Path)],
        hold: Option<u64>,
        extra: &[&str],
        argv: &[&str],
        exit_linger: Option<Duration>,
    ) -> Self {
        let dir = socket_dir();
        let socket = dir.path().join("j.sock");
        let stderr = tempfile::Builder::new()
            .prefix("jar-stderr")
            .tempfile()
            .expect("the jar's stderr file");
        let jar = env!("CARGO_BIN_EXE_spyc-jar");
        warm(jar);
        // R26: the token goes on stdin, here a file the jar reads to its end.
        // Not a pipe: macOS makes a pipe's ends close-on-exec only after it
        // has made them, so a jar another test starts in between inherits
        // this one's write end and holds its start open for its whole life,
        // and this jar never binds its socket.
        let start_at = dir.path().join("start");
        std::fs::write(
            &start_at,
            spyc_jar::cli::Start {
                exit_linger,
                ..start(&socket, hold, argv)
            }
            .encode(),
        )
        .expect("write the start");
        let start_file = std::fs::File::open(&start_at).expect("open the start");
        // A limit goes through a shell, whose `exec` can't give the jar its name.
        let mut command = if let Some(n) = nofile {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", &format!("ulimit -n {n} && exec \"$0\" \"$@\""), jar]);
            c
        } else {
            use std::os::unix::process::CommandExt;
            let mut c = Command::new(jar);
            c.arg0(NAME);
            c
        };
        let process = command
            .envs(env.iter().copied())
            .args(["--wire", "1"])
            .args(extra)
            .stdin(start_file)
            .stdout(Stdio::null())
            .stderr(stderr.reopen().expect("the jar's stderr file"))
            .spawn()
            .expect("spawn the jar");
        std::fs::remove_file(&start_at).expect("remove the start");
        Self {
            dir,
            socket,
            process,
            program: None,
            stderr,
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
        if std::thread::panicking() {
            let exit = self.process.try_wait().ok().flatten();
            eprintln!(
                "the jar (pid {}, {}) wrote to its stderr:\n{}",
                self.process.id(),
                exit.map_or_else(|| "running".to_string(), |s| format!("ended: {s}")),
                std::fs::read_to_string(self.stderr.path()).unwrap_or_else(|e| format!("({e})"))
            );
        }
        if let Some(pid) = self.program.and_then(|p| i32::try_from(p).ok())
            && let Some(pid) = rustix::process::Pid::from_raw(pid)
        {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// Poll until `done` holds, or fail naming what never happened. A test waits
/// for the state it needs, never for a fixed time.
/// A directory for a jar's socket: under `/tmp`, because macOS caps a
/// socket's path at about 104 bytes, and `0700`, as a jar requires.
fn socket_dir() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    tempfile::Builder::new()
        .prefix("jar")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .expect("tempdir")
}

fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// One field of the jar's description, asked as a lookup.
fn described(jar: &Jar, key: &str) -> String {
    field(
        &spyc_jar::client::describe(&jar.socket).expect("describe"),
        key,
    )
}

/// What a client saw: output bytes in order, checked contiguous, and the
/// other frames.
#[derive(Default)]
struct Seen {
    text: Vec<u8>,
    first_offset: Option<u64>,
    next_offset: Option<u64>,
    frames: Vec<Msg>,
    /// For each of `frames`, how many bytes of `text` had come before it.
    frames_at: Vec<usize>,
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
            self.frames_at.push(self.text.len());
        }
    }

    fn contains(&self, needle: &str) -> bool {
        String::from_utf8_lossy(&self.text).contains(needle)
    }
}

/// Read a program's finite output until `done` holds, acknowledging it as it
/// arrives when `ack`. The wait bounds each silence, never the whole: a
/// loaded machine slows a long transfer, and only one that stops fails.
fn read_while_it_comes(client: &mut Client, ack: bool, done: impl Fn(&Seen) -> bool) -> Seen {
    let mut seen = Seen::default();
    let mut heard = Instant::now();
    while !done(&seen) {
        assert!(
            Instant::now() < heard + WAIT,
            "nothing for {WAIT:?}; saw {} bytes, frames {:?}",
            seen.text.len(),
            seen.frames
        );
        if let Some(msg) = client.recv(Duration::from_millis(200)).expect("recv") {
            heard = Instant::now();
            seen.take(msg);
            if ack && let Some(next) = seen.next_offset {
                client.send(&Msg::Ack { offset: next }).expect("ack");
            }
        }
    }
    seen
}

/// Read until `done` holds, acknowledging output as it arrives when `ack`.
fn read_until(client: &mut Client, ack: bool, done: impl Fn(&Seen) -> bool) -> Seen {
    read_until_within(client, ack, WAIT, done)
}

/// [`read_until`], waiting up to `wait`: an upgrade's answer is waited for
/// as long as the jar promises to take, [`UPGRADE_AT_MOST`].
fn read_until_within(
    client: &mut Client,
    ack: bool,
    wait: Duration,
    done: impl Fn(&Seen) -> bool,
) -> Seen {
    let mut seen = Seen::default();
    let until = Instant::now() + wait;
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
        &sh("read go; head -c 2000000 /dev/zero | tr '\\000' a; echo; echo END"),
    );
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    // The program prints only once the owner is there to pace it: started
    // free, it may print everything before the owner connects.
    owner
        .send(&Msg::Input {
            bytes: b"go\n".to_vec(),
        })
        .unwrap();

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
    let mut rest = read_while_it_comes(&mut owner, true, |s| s.contains("END"));
    assert_eq!(
        rest.first_offset,
        Some(got),
        "picks up where the stall left off"
    );
    rest.text.splice(0..0, stalled.text);
    let a_count: usize = rest.text.iter().map(|b| usize::from(*b == b'a')).sum();
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
    let early = read_while_it_comes(&mut first, true, |s| s.text.len() > 20_000);
    let acked = early.next_offset.unwrap();
    first.drop_connection();
    eventually("the first owner's lease to end", || {
        described(&jar, "owner") != "connected"
    });

    let mut second = jar.connect(Role::Owner);
    second.send(&Msg::Replay { from: acked }).unwrap();
    let rest = read_while_it_comes(&mut second, true, |s| s.contains("END"));
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
        &["--ring", "65536", "--window", "32768"],
        &sh("head -c 1000000 /dev/zero | tr '\\000' b; echo; echo END; sleep 5"),
    );
    // Until the program has printed it all, as the jar describes its log:
    // the million `b`s, then `\r\n` and `END\r\n`.
    let mut log = String::new();
    while_it_grows(
        "the program to print everything",
        || {
            log = field(
                &spyc_jar::client::describe(&jar.socket).expect("describe"),
                "log",
            );
            printed_in(&log)
        },
        |n| n >= 1_000_007,
    );
    let (mut owner, welcome) = jar.welcome(Role::Owner);
    let Msg::Welcome { base, head, .. } = welcome else {
        unreachable!()
    };
    assert!(
        base > 0,
        "the oldest bytes were dropped: base {base}, head {head}"
    );
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    let seen = read_while_it_comes(&mut owner, true, |s| s.contains("END"));
    assert_eq!(seen.frames.first(), Some(&Msg::Gap { from: 0, to: base }));
    assert_eq!(seen.first_offset, Some(base));
    assert_eq!(described(&jar, "overflow"), "drop");
}

/// R12b: a jar that keeps its exit until read, a `!` capture's, keeps all its
/// output too. With no owner and its log full, the program waits to print
/// more, and the next owner is replayed every byte from the first.
#[test]
fn a_captures_jar_with_no_owner_holds_its_program_rather_than_drop_output() {
    let mut jar = Jar::keeping_its_exit(
        &["--ring", "65536", "--window", "32768"],
        &sh(&format!("{MILLION_BS}; echo; echo END")),
    );
    assert_every_byte_replays(&mut jar);
}

/// Prints a million `b`s.
const MILLION_BS: &str = "head -c 1000000 /dev/zero | tr '\\000' b";

/// The bytes the program has printed, from a jar's `log` description
/// (`HELD of CAP bytes held, PRINTED printed, from BASE`).
fn printed_in(log: &str) -> u64 {
    log.split(", ")
        .nth(1)
        .and_then(|s| s.split(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// Poll `measure` until `enough` holds of what it reads. The wait bounds
/// each stretch with no growth, never the whole: a loaded machine slows a
/// program, and only one that stops short fails.
fn while_it_grows(what: &str, mut measure: impl FnMut() -> u64, enough: impl Fn(u64) -> bool) {
    let mut last = measure();
    let mut grew = Instant::now();
    while !enough(last) {
        assert!(
            Instant::now() < grew + WAIT,
            "{what}: stopped at {last} for {WAIT:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
        let now = measure();
        if now != last {
            grew = Instant::now();
            last = now;
        }
    }
}

/// With no owner, `jar`'s program printed [`MILLION_BS`] past its 64 KiB
/// ring and then `END`: once the log stops growing, a new owner is replayed
/// every byte from the first, with no gap.
pub(crate) fn assert_every_byte_replays(jar: &mut Jar) {
    // Until the jar has read all it will hold: with nobody there, it reads
    // the PTY while one more read (`PTY_READ`, 8 KiB) fits in the 64 KiB
    // ring, so it stops past 56 KiB, or once the program has printed all.
    while_it_grows(
        "the jar to fill its ring",
        || {
            printed_in(&field(
                &spyc_jar::client::describe(&jar.socket).expect("describe"),
                "log",
            ))
        },
        |n| n > 65_536 - 8_192,
    );
    let (mut owner, welcome) = jar.welcome(Role::Owner);
    let Msg::Welcome { base, .. } = welcome else {
        unreachable!()
    };
    assert_eq!(base, 0, "the jar dropped the output's start");
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    let seen = read_while_it_comes(&mut owner, true, |s| s.contains("END"));
    assert!(
        !seen.frames.iter().any(|f| matches!(f, Msg::Gap { .. })),
        "{:?}",
        seen.frames
    );
    assert_eq!(seen.first_offset, Some(0));
    assert_eq!(
        String::from_utf8_lossy(&seen.text).matches('b').count(),
        1_000_000
    );
    assert_eq!(described(jar, "overflow"), "block");
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

/// S2c: a `!` capture's jar keeps its output and exit, once its program has
/// ended with nobody there, until an owner collects them, however long past
/// the jar's own exit linger; then it ends.
#[test]
fn a_captures_exit_is_kept_until_an_owner_collects_it() {
    let mut jar = Jar::keeping_its_exit(&["--linger-ms", "300"], &sh("echo RESULT=42; exit 7"));
    eventually("the program ended", || {
        spyc_jar::client::describe(&jar.socket).is_ok_and(|f| field(&f, "exit") == "7")
    });
    assert!(
        !jar.ended_within(Duration::from_secs(2)),
        "the jar ended with its output unread"
    );
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    let seen = read_until(&mut owner, true, |s| {
        s.frames.iter().any(|f| matches!(f, Msg::Exited { .. }))
    });
    assert!(seen.contains("RESULT=42"), "the output is all there");
    owner.send(&Msg::AckExit).unwrap();
    assert!(
        jar.ended_within(Duration::from_secs(3)),
        "the jar ends once its exit is collected"
    );
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
    eventually("the snapshot to be stored", || {
        !described(&jar, "snapshots").is_empty()
    });
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
    // It sends nothing, so the read ends only when the jar closes it.
    let after = silent.recv(WAIT);
    assert!(
        !matches!(after, Ok(None)),
        "the jar closed the silent owner's connection: {after:?}"
    );
    // The connection ends before the jar's reader for it lets the lease go.
    eventually("the jar to let the silent owner's lease go", || {
        described(&jar, "owner") != "connected"
    });
    assert_eq!(
        described(&jar, spyc_jar::state::OWNER_LEFT),
        spyc_jar::state::OwnerLeft::Silent.word()
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

/// A `hello` names the highest version its client speaks, and the welcome the
/// one the connection speaks: a newer client, capabilities after its token,
/// learns from the welcome that this jar is version 1, with no refusal and
/// no second try. Version 0 is no version.
#[test]
fn a_newer_client_is_welcomed_at_version_1() {
    use spyc_jar::proto;
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    drop(jar.connect(Role::Reader));
    let hello = |version: u16| {
        let mut p = version.to_be_bytes().to_vec();
        p.push(Role::Owner as u8);
        p.extend(u16::try_from(TOKEN.len()).unwrap().to_be_bytes());
        p.extend(TOKEN);
        p.extend(b"\x00\x02caps from a later version");
        let mut f = u32::try_from(p.len() + 2).unwrap().to_be_bytes().to_vec();
        f.extend([1, proto::CRITICAL]);
        f.extend(p);
        f
    };
    let mut s = std::os::unix::net::UnixStream::connect(&jar.socket).unwrap();
    s.set_read_timeout(Some(WAIT)).unwrap();
    s.write_all(&hello(2)).unwrap();
    let welcome = proto::read_msg(&mut s).unwrap();
    assert!(
        matches!(welcome, Some(Msg::Welcome { version: 1, .. })),
        "{welcome:?}"
    );
    // It owns the jar: the owner's frames come back to it.
    proto::write_msg(&mut s, &Msg::Ping { n: 9 }).unwrap();
    while proto::read_msg(&mut s).unwrap() != Some(Msg::Pong { n: 9 }) {}
    drop(s);

    let mut zero = std::os::unix::net::UnixStream::connect(&jar.socket).unwrap();
    zero.set_read_timeout(Some(WAIT)).unwrap();
    zero.write_all(&hello(0)).unwrap();
    assert!(matches!(
        proto::read_msg(&mut zero).unwrap(),
        Some(Msg::Refused { .. })
    ));
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
    assert!(
        !shell.frames.is_empty(),
        "the shell's own group is reported"
    );
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
    assert!(!job.frames.is_empty(), "a job's group is reported");
}

// Stage 3 (R33): the jar replaces its own binary under a running program.

/// A program that counts forever and echoes back each line it's sent, so a
/// lost or repeated byte shows as a broken count and input as `got=`. The
/// terminal doesn't echo what's typed: an echo lands inside a count's line
/// under load (refute-B, B13), which reads as a count missing.
fn counter() -> Vec<&'static str> {
    vec![
        "/bin/sh",
        "-c",
        "stty -echo; (i=0; while :; do i=$((i+1)); echo n=$i; sleep 0.005; done) & while read l; do echo got=$l; done",
    ]
}

/// The counts seen, which must run 1, 2, 3… with nothing missing or doubled.
fn counts(seen: &Seen) -> Vec<u64> {
    String::from_utf8_lossy(&seen.text)
        .lines()
        .filter_map(|l| l.trim().strip_prefix("n=")?.parse().ok())
        .collect()
}

/// The counts run 1, 2, 3… A break fails with the text around it and where
/// each other frame (an upgrade's `upgraded` among them) arrived.
fn assert_unbroken(seen: &Seen) {
    let mut last: Option<(u64, usize)> = None;
    let mut at = 0;
    for line in seen.text.split_inclusive(|b| *b == b'\n') {
        let n = std::str::from_utf8(line)
            .ok()
            .and_then(|l| l.trim().strip_prefix("n=")?.parse::<u64>().ok());
        if let Some(n) = n {
            if let Some((before, from)) = last
                && n != before + 1
            {
                let window = &seen.text
                    [from.saturating_sub(300)..(at + line.len() + 300).min(seen.text.len())];
                let frames: Vec<_> = seen.frames_at.iter().zip(&seen.frames).collect();
                panic!(
                    "the count runs on unbroken: {before} then {n}, at byte {at}; the text around it: {:?}; frames by the byte they came after: {frames:?}",
                    String::from_utf8_lossy(window)
                );
            }
            last = Some((n, at));
        }
        at += line.len();
    }
    assert!(last.is_some(), "the program printed counts");
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
    // The reader reads throughout, as a real one does: one that stops lets
    // its socket fill, its writer can't park, and the upgrade is busy. It
    // reads until it hears the upgrade or its connection ends; how long
    // that may take is bounded from the owner's `upgraded`, not from here.
    let (heard, reader_heard) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut seen = Seen::default();
        let got = loop {
            match reader.recv(Duration::from_millis(200)) {
                Ok(Some(m @ Msg::Upgraded { .. })) => {
                    seen.take(m);
                    break Ok(seen);
                }
                Ok(Some(m)) => seen.take(m),
                Ok(None) => {}
                Err(e) => {
                    break Err(format!(
                        "the reader's connection ended after {} bytes: {e:#}",
                        seen.text.len()
                    ));
                }
            }
        };
        let _ = heard.send(got);
    });
    let next = next_binary(jar.dir.path());
    let mut seen = read_while_it_comes(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 30)
    });
    let before = jar.process.id();
    owner
        .send(&Msg::Upgrade {
            binary: next.display().to_string(),
        })
        .expect("upgrade");
    let until = Instant::now() + UPGRADE_AT_MOST;
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
        build.is_some_and(|b| b.starts_with("jar-next")),
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
    let r = reader_heard
        .recv_timeout(WAIT)
        .expect("the reader heard the upgrade the owner heard")
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r.first_offset, Some(0), "the reader replays from the start");
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
    assert!(!s.frames.is_empty(), "the resize is in the log");
}

#[test]
fn an_upgrade_the_new_binary_refuses_leaves_the_old_jar_running() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_while_it_comes(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 10)
    });
    for binary in ["/usr/bin/false", "/nonexistent/spyc-jar"] {
        owner
            .send(&Msg::Upgrade {
                binary: binary.into(),
            })
            .expect("upgrade");
        let until = Instant::now() + UPGRADE_AT_MOST;
        loop {
            assert!(Instant::now() < until, "no refusal for {binary}");
            if let Some(m) = owner.recv(Duration::from_millis(200)).expect("recv") {
                if let Msg::Refused { why, .. } = &m {
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
    let mut more = read_while_it_comes(&mut owner, true, |s| {
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
    let next = next_binary(jar.dir.path());
    let mut seen = read_while_it_comes(&mut owner, true, |s| {
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
        let until = Instant::now() + UPGRADE_AT_MOST;
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
    let more = read_while_it_comes(&mut owner, true, |s| {
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
    let next = next_binary(jar.dir.path());
    let mut seen = read_while_it_comes(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    for round in 1..=3 {
        owner
            .send(&Msg::Upgrade {
                binary: next.display().to_string(),
            })
            .expect("upgrade");
        let more = read_until_within(&mut owner, true, UPGRADE_AT_MOST, |s| {
            s.frames.iter().any(|f| matches!(f, Msg::Upgraded { .. }))
        });
        seen.text.extend(more.text);
        let at = counts(&seen).last().copied().unwrap_or(0);
        let more = read_while_it_comes(&mut owner, true, |s| {
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
        .map_or_else(String::new, |(_, v)| v.clone())
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
    assert!(
        !field(&before, "commit").is_empty(),
        "the binary knows its commit"
    );

    let next = next_binary(jar.dir.path());
    let (mut asker, _) = Client::connect(&jar.socket, Role::Reader, WAIT, TOKEN).expect("connect");
    asker
        .send(&Msg::Upgrade {
            binary: next.display().to_string(),
        })
        .expect("upgrade");
    read_until_within(&mut asker, false, UPGRADE_AT_MOST, |s| {
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
    assert!(field(&after, "build").starts_with("jar-next"));
}

// Stage 6: a jar handed on (R22), where its owner's MCP socket is (R18,
// R30a), and the token every role but a lookup presents (R26).

#[test]
fn a_released_jar_opens_only_to_its_new_token() {
    let mut jar = Jar::start(&[], &sh("sleep 1000"));
    let mut first = jar.connect(Role::Owner);
    let mut reader = jar.connect(Role::Reader);
    first
        .send(&Msg::SetInfo {
            key: "owner_mcp".into(),
            value: "/tmp/spyc/a.sock".into(),
        })
        .expect("set info");
    eventually("the owner's MCP socket to be stored", || {
        described(&jar, "owner_mcp") == "/tmp/spyc/a.sock"
    });

    // The release, then the Kill a closing tab sends, on the same connection.
    first
        .send(&Msg::Release {
            token: b"successor-token-0001".to_vec(),
        })
        .expect("release");
    let _ = first.send(&Msg::Signal {
        signal: spyc_jar::proto::JarSignal::Kill as i32,
    });
    // The jar ends the connection at the release: past this, the Kill can
    // never be read.
    assert!(
        first.recv(WAIT).is_err(),
        "the release ends the owner's connection"
    );
    let pid = jar
        .program
        .and_then(|p| i32::try_from(p).ok())
        .and_then(rustix::process::Pid::from_raw)
        .expect("pid");
    assert!(
        rustix::process::test_kill_process(pid).is_ok(),
        "the Kill after a release never acts"
    );
    eventually("the old owner's socket to be forgotten", || {
        described(&jar, "owner_mcp").is_empty()
    });
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
    eventually("the successor's MCP socket to be stored", || {
        described(&jar, "owner_mcp") == "/tmp/spyc/b.sock"
    });
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
    recv_matching_within(client, WAIT, want)
}

/// [`recv_matching`], within `wait`.
fn recv_matching_within(client: &mut Client, wait: Duration, want: impl Fn(&Msg) -> bool) -> Msg {
    let until = Instant::now() + wait;
    while Instant::now() < until {
        if let Some(m) = client.recv(Duration::from_millis(200)).expect("recv")
            && want(&m)
        {
            return m;
        }
    }
    panic!("no matching message within {wait:?}");
}

/// The answer to an upgrade an owner asked for, within [`UPGRADE_AT_MOST`],
/// pinging meanwhile: an owner waiting out the answer keeps its lease as
/// spyc's does, rather than fall silent past the jar's deadline and lose it.
fn upgrade_answer_pinging(owner: &mut Client, want: impl Fn(&Msg) -> bool) -> Msg {
    let until = Instant::now() + UPGRADE_AT_MOST;
    let mut n = 1 << 40;
    let mut pinged = Instant::now();
    while Instant::now() < until {
        if pinged.elapsed() >= Duration::from_secs(1) {
            n += 1;
            let _ = owner.send(&Msg::Ping { n });
            pinged = Instant::now();
        }
        match owner.recv(Duration::from_millis(200)).expect("recv") {
            Some(m) if want(&m) => return m,
            Some(Msg::Output { offset, bytes }) => {
                let _ = owner.send(&Msg::Ack {
                    offset: offset + bytes.len() as u64,
                });
            }
            _ => {}
        }
    }
    panic!("no answer to the upgrade within {UPGRADE_AT_MOST:?}");
}

// The $TMPDIR check: macOS's cleaner may delete a socket left untouched for
// days. The jar puts its address back.

#[test]
fn a_jar_whose_socket_was_deleted_binds_it_again() {
    let mut jar = Jar::start(&[], &sh("sleep 1000"));
    let owner = jar.connect(Role::Owner);
    std::fs::remove_file(&jar.socket).expect("delete the socket");
    std::fs::remove_file(spyc_jar::lock::path(&jar.socket)).expect("and its lock file");
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
    eventually("the lock held again", || {
        spyc_jar::lock::state(&jar.socket) == spyc_jar::lock::LockState::Held
    });
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
    let next = next_binary(jar.dir.path());
    let mut reader = None;
    eventually("the new token to open the jar", || {
        reader = Client::connect(&jar.socket, Role::Reader, WAIT, b"successor-token-0002").ok();
        reader.is_some()
    });
    let (mut upgrader, _) = reader.expect("a reader on the new token");
    upgrader
        .send(&Msg::Upgrade {
            binary: next.display().to_string(),
        })
        .expect("upgrade");
    let done = recv_matching_within(&mut upgrader, UPGRADE_AT_MOST, |m| {
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

// The reopened version 1, after the review of 2026-11-12.

fn refusal(r: anyhow::Result<Client>) -> u16 {
    let e = r.err().expect("refused");
    e.downcast_ref::<spyc_jar::client::Refusal>()
        .unwrap_or_else(|| panic!("not a refusal: {e:#}"))
        .reason
}

fn try_connect(jar: &Jar, role: Role, token: &[u8]) -> anyhow::Result<Client> {
    Client::connect(&jar.socket, role, WAIT, token).map(|(c, _)| c)
}

/// Finding 2: a refusal says why, as a reason a client can act on.
#[test]
fn a_refusal_says_why() {
    use spyc_jar::proto::reason;
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    let mut owner = jar.connect(Role::Owner);
    assert_eq!(
        refusal(try_connect(&jar, Role::Reader, b"not-the-token-at-all")),
        reason::WRONG_TOKEN
    );
    assert_eq!(
        refusal(try_connect(&jar, Role::Owner, TOKEN)),
        reason::OWNER_PRESENT
    );
    assert_eq!(
        refusal(try_connect(&jar, Role::Program, b"")),
        reason::ROLE_NOT_PERMITTED,
        "the test isn't the jar's program"
    );
    let reader = jar.connect(Role::Reader);
    reader
        .send(&Msg::Input {
            bytes: b"x".to_vec(),
        })
        .unwrap();
    let mut reader = reader;
    let m = recv_matching(&mut reader, |m| matches!(m, Msg::Refused { .. }));
    assert!(
        matches!(
            m,
            Msg::Refused {
                reason: reason::ROLE_NOT_PERMITTED,
                ..
            }
        ),
        "{m:?}"
    );
    owner
        .send(&Msg::Release {
            token: b"short".to_vec(),
        })
        .unwrap();
    let m = recv_matching(&mut owner, |m| matches!(m, Msg::Refused { .. }));
    assert!(
        matches!(
            m,
            Msg::Refused {
                reason: reason::INVALID,
                ..
            }
        ),
        "{m:?}"
    );
}

/// Finding 1: a jar asked for a wire version it doesn't speak, or for none,
/// exits with 75 before it binds its socket or starts the program.
#[test]
fn a_jar_asked_for_a_wire_version_it_doesnt_speak_exits_75() {
    for wire in [Some("2"), Some("0"), None] {
        let dir = socket_dir();
        let socket = dir.path().join("j.sock");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_spyc-jar"));
        if let Some(w) = wire {
            cmd.args(["--wire", w]);
        }
        let mark = dir.path().join("ran");
        let script = format!("touch {}", mark.display());
        let mut jar = cmd
            .stdin(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // A jar that refuses may have gone before its start arrives.
        let _ = jar
            .stdin
            .take()
            .unwrap()
            .write_all(&start(&socket, None, &["/bin/sh", "-c", &script]).encode());
        let status = jar.wait().unwrap();
        assert_eq!(status.code(), Some(75), "--wire {wire:?}");
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            !socket.exists() && !mark.exists(),
            "--wire {wire:?} started something"
        );
    }
}

/// Finding 14: a jar listens only in a directory of its user's own, `0700`
/// and not a link; anywhere else it starts nothing.
#[test]
fn a_jar_refuses_a_socket_directory_others_could_reach() {
    use std::os::unix::fs::PermissionsExt;
    let dir = socket_dir();
    let open = dir.path().join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    let private = dir.path().join("private");
    std::fs::create_dir(&private).unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
    let linked = dir.path().join("linked");
    std::os::unix::fs::symlink(&private, &linked).unwrap();
    for (i, (sockets, ok)) in [(&private, true), (&open, false), (&linked, false)]
        .into_iter()
        .enumerate()
    {
        let socket = sockets.join(format!("j{i}.sock"));
        let mark = dir.path().join("ran");
        let _ = std::fs::remove_file(&mark);
        let mut jar = Command::new(env!("CARGO_BIN_EXE_spyc-jar"))
            .args(["--wire", "1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let touch = format!("touch {}", mark.display());
        let started = start(&socket, None, &["/bin/sh", "-c", &touch]);
        jar.stdin
            .take()
            .unwrap()
            .write_all(&started.encode())
            .unwrap();
        if ok {
            eventually("the program runs", || mark.exists());
            let _ = jar.kill();
            let _ = jar.wait();
            continue;
        }
        let status = jar.wait().unwrap();
        assert!(!status.success(), "{}: the jar ran", sockets.display());
        assert!(
            !socket.exists() && !mark.exists(),
            "{}: the jar started something",
            sockets.display()
        );
    }
}

/// A program in a jar, and whatever it starts, finds the jar by
/// `SPYC_JAR_SOCKET`, not by working the path out again (finding 14).
#[test]
fn a_program_knows_its_jars_socket() {
    let mut jar = Jar::start(&[], &sh("echo \"sock=$SPYC_JAR_SOCKET.\"; exec cat"));
    let want = format!("sock={}.", jar.socket.display());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    read_until(&mut owner, true, |s| s.contains(&want));
}

/// Finding 12: the program hears an owner attach and go, and can ask the jar
/// to detach its owner once what it printed has gone out.
#[test]
fn the_program_hears_attaches_and_detaches_its_owner() {
    let program = [env!("CARGO_BIN_EXE_spyc-jar"), "program-events"];
    let mut jar = Jar::start(&[], &program);
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    read_until(&mut owner, true, |s| s.contains("program: attached"));
    let fields = spyc_jar::client::describe(&jar.socket).unwrap();
    assert_eq!(field(&fields, "owner_pid"), std::process::id().to_string());
    owner
        .send(&Msg::Input {
            bytes: b"detach\r".to_vec(),
        })
        .unwrap();
    // Everything up to its last words, then the connection ends.
    let mut seen = Seen::default();
    // The jar says why the lease ended: the program's detach, not the
    // owner's 10 s deadline, which would end it anyway.
    let until = Instant::now() + WAIT;
    loop {
        assert!(Instant::now() < until, "the owner was never detached");
        match owner.recv(Duration::from_millis(200)) {
            Ok(Some(m)) => seen.take(m),
            Ok(None) => {}
            Err(_) => break,
        }
    }
    assert!(
        seen.contains("program: bye"),
        "its last words reached the owner"
    );
    // The connection ends before the jar's reader for it lets the lease go.
    eventually("the jar to let the detached owner's lease go", || {
        described(&jar, "owner").starts_with("none")
    });
    assert_eq!(
        described(&jar, spyc_jar::state::OWNER_LEFT),
        spyc_jar::state::OwnerLeft::Detached.word()
    );
    assert!(described(&jar, "exit").is_empty(), "the program runs on");
    let mut again = jar.connect(Role::Owner);
    again.send(&Msg::Replay { from: 0 }).unwrap();
    read_until(&mut again, true, |s| {
        String::from_utf8_lossy(&s.text)
            .matches("program: attached")
            .count()
            == 2
            && s.contains("program: detached")
    });
}

/// Finding 12: a program hears one `attached` for each owner, however its own
/// connection and the owner's race. An owner that connects as the program
/// does, here at once, used to be told to it twice: once by the owner's
/// arrival and once by the program's, both read after either had registered.
#[test]
fn a_program_hears_one_attached_per_owner_however_the_connections_race() {
    let program = [env!("CARGO_BIN_EXE_spyc-jar"), "program-events"];
    for round in 0..40 {
        let mut jar = Jar::start(&[], &program);
        let mut owner = jar.connect(Role::Owner);
        owner.send(&Msg::Replay { from: 0 }).unwrap();
        let mut seen = read_until(&mut owner, true, |s| s.contains("program: attached"));
        // A second one would follow at once: give it time to show.
        let until = Instant::now() + Duration::from_millis(300);
        while Instant::now() < until {
            if let Ok(Some(m)) = owner.recv(Duration::from_millis(50)) {
                seen.take(m);
                if let Some(next) = seen.next_offset {
                    let _ = owner.send(&Msg::Ack { offset: next });
                }
            }
        }
        let heard = String::from_utf8_lossy(&seen.text)
            .matches("program: attached")
            .count();
        assert_eq!(
            heard, 1,
            "round {round}: the program heard {heard} for one owner"
        );
    }
}

/// A program that asks to detach its owner has its last words reach the owner
/// first, however slowly the owner reads: here a window of 16 bytes holds the
/// rest of the program's output in its PTY until the owner acknowledges, which
/// it starts doing 2 s after the program was told to detach. spyc writes its
/// terminal's reset before it detaches, so a reset cut short would leave the
/// terminal in spyc's modes.
#[test]
fn a_programs_last_words_reach_a_slow_owner_before_it_is_detached() {
    let program = [env!("CARGO_BIN_EXE_spyc-jar"), "program-events"];
    let mut jar = Jar::start(&["--window", "16"], &program);
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    owner
        .send(&Msg::Input {
            bytes: b"detach\r".to_vec(),
        })
        .unwrap();
    std::thread::sleep(Duration::from_secs(2));
    let mut seen = Seen::default();
    let until = Instant::now() + WAIT;
    loop {
        assert!(Instant::now() < until, "the owner was never detached");
        match owner.recv(Duration::from_millis(200)) {
            Ok(Some(m)) => {
                seen.take(m);
                if let Some(next) = seen.next_offset {
                    let _ = owner.send(&Msg::Ack { offset: next });
                }
            }
            Ok(None) => {}
            Err(_) => break,
        }
    }
    assert!(
        seen.contains("program: bye"),
        "its last words reached the owner: {:?}",
        String::from_utf8_lossy(&seen.text)
    );
}

/// Finding 5, in part: a lookup is answered once and closed.
#[test]
fn a_lookup_is_answered_once_and_closed() {
    let jar = Jar::start(&[], &["/bin/cat"]);
    let mut lookup = try_connect(&jar, Role::Lookup, b"").unwrap();
    lookup.send(&Msg::Describe).unwrap();
    assert!(matches!(
        lookup.recv(WAIT).unwrap(),
        Some(Msg::Description { .. })
    ));
    // At once, not when its idle limit would close it: a second question,
    // asked before that limit, is never answered.
    let _ = lookup.send(&Msg::Describe);
    match lookup.recv(WAIT) {
        Err(_) => {}
        Ok(m) => panic!("the jar kept the lookup open after its answer: {m:?}"),
    }
    // One that asks nothing is closed too, when its idle limit is up (an
    // `Ok(None)` is the wait running out first).
    let mut idle = try_connect(&jar, Role::Lookup, b"").unwrap();
    let idled = idle.recv(Duration::from_secs(8));
    assert!(idled.is_err(), "the jar kept an idle lookup: {idled:?}");
}

/// Finding 15: a jar refuses what its own upgrade would reject, so it can
/// still be upgraded after.
#[test]
fn a_short_token_and_a_snapshot_past_the_head_are_refused_and_the_jar_still_upgrades() {
    use spyc_jar::proto::reason;
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    owner.send(&Msg::Release { token: Vec::new() }).unwrap();
    let m = recv_matching(&mut owner, |m| matches!(m, Msg::Refused { .. }));
    assert!(
        matches!(
            m,
            Msg::Refused {
                reason: reason::INVALID,
                ..
            }
        ),
        "{m:?}"
    );
    owner
        .send(&Msg::StoreSnapshot(Snapshot {
            format: SNAPSHOT_VT,
            offset: 1 << 40,
            rows: 24,
            cols: 80,
            blob: b"x".to_vec(),
        }))
        .unwrap();
    let m = recv_matching(&mut owner, |m| matches!(m, Msg::Refused { .. }));
    assert!(
        matches!(
            m,
            Msg::Refused {
                reason: reason::INVALID,
                ..
            }
        ),
        "{m:?}"
    );
    let next = next_binary(jar.dir.path());
    owner
        .send(&Msg::Upgrade {
            binary: next.display().to_string(),
        })
        .unwrap();
    let m = upgrade_answer_pinging(&mut owner, |m| {
        matches!(m, Msg::Upgraded { .. } | Msg::Refused { .. })
    });
    assert!(matches!(m, Msg::Upgraded { .. }), "{m:?}");
}

/// Finding 10: once the program's exit is recorded, a signal reaches no one.
#[test]
fn a_signal_after_the_exit_is_refused() {
    use spyc_jar::proto::reason;
    let mut jar = Jar::start(&[], &sh("exit 3"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    recv_matching(&mut owner, |m| matches!(m, Msg::Exited { .. }));
    owner.send(&Msg::Signal { signal: 3 }).unwrap();
    let m = recv_matching(&mut owner, |m| matches!(m, Msg::Refused { .. }));
    assert!(
        matches!(
            m,
            Msg::Refused {
                reason: reason::PROGRAM_EXITED,
                ..
            }
        ),
        "{m:?}"
    );
}

/// Finding 11: an `exec` that fails after the new binary accepted the
/// handover leaves a jar that a later refusing validator can't kill.
#[test]
fn a_failed_exec_leaves_a_jar_that_survives_the_next_refusal() {
    use std::os::unix::fs::PermissionsExt;
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    // A new binary that loses its execute bit while it holds the sockets, as
    // a package manager replacing it mid-upgrade might.
    let flaky = jar.dir.path().join("jar-flaky");
    std::fs::write(
        &flaky,
        format!(
            "#!/bin/sh\ncase \"$1\" in hold-handover) chmod -x \"$0\";; esac\nexec {} \"$@\"\n",
            env!("CARGO_BIN_EXE_spyc-jar")
        ),
    )
    .unwrap();
    std::fs::set_permissions(&flaky, std::fs::Permissions::from_mode(0o755)).unwrap();
    owner
        .send(&Msg::Upgrade {
            binary: flaky.display().to_string(),
        })
        .unwrap();
    let m = upgrade_answer_pinging(&mut owner, |m| {
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
        "{m:?}"
    );
    owner
        .send(&Msg::Upgrade {
            binary: "/usr/bin/false".into(),
        })
        .unwrap();
    let m = upgrade_answer_pinging(&mut owner, |m| matches!(m, Msg::Refused { .. }));
    assert!(
        matches!(
            m,
            Msg::Refused {
                reason: spyc_jar::proto::reason::UPGRADE_REJECTED,
                ..
            }
        ),
        "{m:?}"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(jar.process.try_wait().unwrap().is_none(), "the jar lives");
    owner.send(&Msg::Ping { n: 5 }).unwrap();
    recv_matching(&mut owner, |m| matches!(m, Msg::Pong { n: 5 }));
}

#[path = "jar_process/upgrade.rs"]
mod upgrade;

#[path = "jar_process/contain.rs"]
mod contain;

#[path = "jar_process/timers.rs"]
mod timers;

#[path = "jar_process/fixed_waits.rs"]
mod fixed_waits;

#[path = "jar_process/ownership.rs"]
mod ownership;

#[path = "jar_process/lock.rs"]
mod lock;

// The review's probes, as requirements. A jar can't be wedged by a client
// that stops reading or a program that stops reading (finding 4), nor held by
// connections without its token (finding 5), and it keeps what an owner yet
// to come will need (findings 16, 17 and 18).

/// The token a release hands to the next owner.
const NEXT_TOKEN: &[u8] = b"the-next-owners-token-0001";

/// [`try_connect`], giving up after `wait`.
fn connect_within(jar: &Jar, role: Role, token: &[u8], wait: Duration) -> anyhow::Result<Client> {
    Client::connect(&jar.socket, role, wait, token).map(|(c, _)| c)
}

/// Ask on `client` for an upgrade to a copy of the jar's binary, and return
/// the new build, or how the jar refused. Call it once a test: the copy is
/// running afterwards.
/// What `ps` shows as the command line of process `pid`.
fn command_line(pid: u32) -> String {
    let out = Command::new("ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn upgrade(jar: &Jar, client: &mut Client) -> Result<String, String> {
    let next = next_binary(jar.dir.path());
    client
        .send(&Msg::Upgrade {
            binary: next.display().to_string(),
        })
        .map_err(|e| format!("send: {e}"))?;
    // The jar serves its program while it first validates the new binary,
    // and keeps every deadline meanwhile: an owner waiting for the answer
    // still pings and acknowledges, as spyc does.
    let until = Instant::now() + UPGRADE_AT_MOST;
    let mut n = 0;
    while Instant::now() < until {
        n += 1;
        let _ = client.send(&Msg::Ping { n });
        match client.recv(Duration::from_millis(100)) {
            Ok(Some(Msg::Upgraded { build })) => return Ok(build),
            Ok(Some(Msg::Refused { reason, why })) => {
                return Err(format!("refused ({reason}): {why}"));
            }
            Ok(Some(Msg::Output { offset, bytes })) => {
                let _ = client.send(&Msg::Ack {
                    offset: offset + bytes.len() as u64,
                });
            }
            Ok(_) => {}
            Err(e) => return Err(format!("connection: {e:#}")),
        }
    }
    Err("no answer".into())
}

/// A lookup's `hello` as raw bytes, the frozen fixture: no token.
const LOOKUP_HELLO: [u8; 11] = [0, 0, 0, 7, 1, 0x80, 0, 1, 3, 0, 0];

/// A connection that has said `hello` as a lookup and been welcomed, and
/// that the test drives byte by byte from here.
fn raw_lookup(socket: &std::path::Path) -> UnixStream {
    let mut connected = None;
    eventually("the jar's socket", || {
        connected = UnixStream::connect(socket).ok();
        connected.is_some()
    });
    let mut s = connected.expect("connected");
    s.write_all(&LOOKUP_HELLO).expect("hello");
    s.set_read_timeout(Some(WAIT)).expect("timeout");
    let welcome = spyc_jar::proto::read_msg(&mut s);
    assert!(
        matches!(welcome, Ok(Some(Msg::Welcome { .. }))),
        "a lookup is welcomed: {welcome:?}"
    );
    s
}

/// A lookup's `describe`, or why there was none within `WAIT`, however the
/// jar fails to answer.
fn describe_within(socket: &std::path::Path) -> Result<Vec<(String, String)>, String> {
    let socket = socket.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(spyc_jar::client::describe(&socket).map_err(|e| format!("{e:#}")));
    });
    rx.recv_timeout(WAIT)
        .unwrap_or_else(|_| Err(format!("no answer within {WAIT:?}")))
}

/// Whether process `pid` exists.
fn running(pid: u32) -> bool {
    i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
}

/// The CPU time process `pid` has used, as `ps` reports it on macOS
/// (`M:SS.ss`) and Linux (`[DD-]HH:MM:SS`).
fn cpu_seconds(pid: u32) -> f64 {
    let out = Command::new("ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
        .expect("ps");
    let text = String::from_utf8_lossy(&out.stdout);
    let clock = text.trim().rsplit('-').next().unwrap_or_default();
    clock.split(':').fold(0.0, |total, part| {
        f64::mul_add(total, 60.0, part.parse::<f64>().unwrap_or(0.0))
    })
}

/// Finding 4 (R21): an owner gone silent while its program prints loses the
/// lease at its deadline, though the jar's writes to it are stuck.
#[test]
fn a_silent_owner_loses_the_lease_while_its_program_prints() {
    // The deadline binds the new owner too, which pings while it waits for
    // its upgrade: 2 s, so a loaded machine's pause between its pings isn't
    // taken for silence.
    let mut jar = Jar::start(
        &["--deadline-ms", "2000"],
        &["yes", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
    );
    let silent = jar.connect(Role::Owner);
    silent.send(&Msg::Replay { from: 0 }).expect("replay");
    // From here it never reads, acknowledges or pings: a stopped spyc.
    let mut next = None;
    eventually("a new owner's welcome", || {
        next = connect_within(&jar, Role::Owner, TOKEN, Duration::from_millis(500)).ok();
        next.is_some()
    });
    let mut next = next.expect("welcomed");
    let answer = upgrade(&jar, &mut next);
    assert!(
        answer.as_ref().is_ok_and(|b| b.starts_with("jar-next")),
        "the new owner can upgrade the jar: {answer:?}"
    );
    drop(silent);
}

/// Finding 4: a reader that has stopped reading can't freeze the jar when its
/// owner hands it on. The release, a lookup and the successor are answered.
#[test]
fn a_stalled_reader_does_not_freeze_a_release() {
    let mut jar = Jar::start(
        &[],
        &["yes", "line-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
    );
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let stalled = jar.connect(Role::Reader);
    stalled.send(&Msg::Replay { from: 0 }).expect("replay");
    // The owner reads on until far more has gone by than any socket holds, so
    // the jar's writes to the reader are stuck.
    read_while_it_comes(&mut owner, true, |s| s.text.len() > 4 << 20);
    owner
        .send(&Msg::Release {
            token: NEXT_TOKEN.to_vec(),
        })
        .expect("release");
    // The release is done when the jar ends the owner's connection: it is read
    // after every acknowledgement queued before it, so a successor that comes
    // sooner may still meet the old token.
    let until = Instant::now() + WAIT;
    while owner.recv(Duration::from_millis(200)).is_ok() {
        assert!(
            Instant::now() < until,
            "the release never ended the owner's connection"
        );
    }
    let described = describe_within(&jar.socket);
    assert!(described.is_ok(), "a lookup is answered: {described:?}");
    let next = connect_within(&jar, Role::Owner, NEXT_TOKEN, WAIT);
    assert!(
        next.is_ok(),
        "the successor is welcomed: {next:?}",
        next = next.err()
    );
    drop(stalled);
}

/// Finding 5: a connection without the token can't hold off an upgrade by
/// sending a frame a byte at a time.
#[test]
fn a_lookup_trickling_a_frame_does_not_block_an_upgrade() {
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let raw = raw_lookup(&jar.socket);
    // The header of a 4,000-byte ping, then a byte at a time.
    (&raw).write_all(&[0, 0, 0x0f, 0xa0, 9]).expect("header");
    let stop = Arc::new(AtomicBool::new(false));
    let trickle = {
        let stop = Arc::clone(&stop);
        let raw = raw.try_clone().expect("clone");
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) && (&raw).write_all(&[0]).is_ok() {
                std::thread::sleep(Duration::from_millis(200));
            }
        })
    };
    let answer = upgrade(&jar, &mut owner);
    stop.store(true, Ordering::SeqCst);
    let _ = trickle.join();
    assert!(
        answer.as_ref().is_ok_and(|b| b.starts_with("jar-next")),
        "the upgrade goes ahead: {answer:?}"
    );
}

/// Finding 5: a frame that has begun arrives whole by its deadline, however
/// its bytes trickle, or the jar closes the connection.
#[test]
fn a_frame_that_trickles_is_cut_off() {
    let jar = Jar::start(&[], &["/bin/cat"]);
    let raw = raw_lookup(&jar.socket);
    (&raw).write_all(&[0, 0, 0x0f, 0xa0, 9]).expect("header");
    let stop = Arc::new(AtomicBool::new(false));
    let trickle = {
        let stop = Arc::clone(&stop);
        let raw = raw.try_clone().expect("clone");
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) && (&raw).write_all(&[0]).is_ok() {
                std::thread::sleep(Duration::from_millis(200));
            }
        })
    };
    let mut rest = Vec::new();
    let read = (&raw).read_to_end(&mut rest);
    stop.store(true, Ordering::SeqCst);
    let _ = trickle.join();
    assert!(read.is_ok(), "the jar closed it: {read:?}");
}

/// Finding 5: idle connections without the token, more than a handover
/// carries, neither hold off an upgrade nor cross it.
#[test]
fn idle_lookups_neither_block_nor_cross_an_upgrade() {
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let lookups: Vec<UnixStream> = (0..201).map(|_| raw_lookup(&jar.socket)).collect();
    let answer = upgrade(&jar, &mut owner);
    assert!(
        answer.as_ref().is_ok_and(|b| b.starts_with("jar-next")),
        "the upgrade goes ahead: {answer:?}"
    );
    // A connection that crossed is told `upgraded`; a lookup reads to its end
    // and is told nothing.
    for (i, mut s) in lookups.into_iter().enumerate() {
        let mut rest = Vec::new();
        let read = s.read_to_end(&mut rest);
        assert!(read.is_ok(), "lookup {i} is closed: {read:?}");
        let mut frames = rest.as_slice();
        while let Ok(Some(m)) = spyc_jar::proto::read_msg(&mut frames) {
            assert!(!matches!(m, Msg::Upgraded { .. }), "lookup {i} crossed");
        }
    }
    let described = describe_within(&jar.socket);
    assert!(
        described.is_ok(),
        "the new image answers a lookup: {described:?}"
    );
}

/// Finding 5: connections without the token can't take every descriptor the
/// jar may open. A token holder is still welcomed, the upgrade goes ahead,
/// and the jar doesn't spin.
#[test]
fn tokenless_connections_cannot_exhaust_the_jars_descriptors() {
    let mut jar = Jar::start_limited(Some(64), &[], &["/bin/cat"]);
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut held = Vec::new();
    for _ in 0..60 {
        let Ok(mut s) = UnixStream::connect(&jar.socket) else {
            break;
        };
        let _ = s.write_all(&LOOKUP_HELLO);
        held.push(s);
    }
    let reader = connect_within(&jar, Role::Reader, TOKEN, Duration::from_secs(2));
    assert!(
        reader.is_ok(),
        "a token holder is welcomed: {:?}",
        reader.err()
    );
    let pid = jar.process.id();
    let before = cpu_seconds(pid);
    std::thread::sleep(Duration::from_secs(3));
    let spent = cpu_seconds(pid) - before;
    assert!(spent < 1.0, "the jar spent {spent:.2} s of CPU in 3 s");
    let answer = upgrade(&jar, &mut owner);
    assert!(
        answer.as_ref().is_ok_and(|b| b.starts_with("jar-next")),
        "the upgrade goes ahead: {answer:?}"
    );
    drop(held);
}

/// Finding 4: a large paste into a program that reads nothing doesn't stop
/// the jar hearing its owner. A ping is answered; once the owner has gone, its
/// paste goes with it, so the next owner can upgrade the jar and end the
/// program.
#[test]
fn a_paste_the_program_never_reads_does_not_block_its_owner() {
    let mut jar = Jar::start(&[], &sh("stty raw -echo; echo READY; exec sleep 1000"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    read_until(&mut owner, true, |s| s.contains("READY"));
    owner
        .send(&Msg::Input {
            bytes: vec![b'x'; 1 << 20],
        })
        .expect("paste");
    owner.send(&Msg::Ping { n: 42 }).expect("ping");
    recv_matching(&mut owner, |m| matches!(m, Msg::Pong { n: 42 }));
    drop(owner);
    let mut next = None;
    eventually("the owner's lease freed", || {
        next = connect_within(&jar, Role::Owner, TOKEN, Duration::from_millis(500)).ok();
        next.is_some()
    });
    let mut next = next.expect("welcomed");
    let answer = upgrade(&jar, &mut next);
    assert!(
        answer.as_ref().is_ok_and(|b| b.starts_with("jar-next")),
        "the upgrade goes ahead: {answer:?}"
    );
    next.send(&Msg::Replay { from: 0 }).expect("replay");
    next.send(&Msg::Signal {
        signal: spyc_jar::proto::JarSignal::Kill as i32,
    })
    .expect("kill");
    let exit = recv_matching(&mut next, |m| matches!(m, Msg::Exited { .. }));
    assert!(
        matches!(exit, Msg::Exited { signaled: true, .. }),
        "the program was killed: {exit:?}"
    );
}

/// Finding 18 (R11): a window the ring can't hold, plus one read, is refused
/// at the start; a live owner's bytes are never dropped from under it.
#[test]
fn a_window_must_fit_inside_the_ring() {
    let dir = socket_dir();
    let mut refused = Command::new(env!("CARGO_BIN_EXE_spyc-jar"))
        .args(["--wire", "1", "--ring", "65536"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the jar");
    // A jar that refuses its options may have gone before its token arrives.
    let _ = refused
        .stdin
        .take()
        .expect("stdin")
        .write_all(&start(&dir.path().join("j.sock"), None, &["/bin/cat"]).encode());
    let until = Instant::now() + WAIT;
    let mut status = None;
    while status.is_none() && Instant::now() < until {
        status = refused.try_wait().expect("try_wait");
        std::thread::sleep(Duration::from_millis(20));
    }
    // A jar that started after all is ended here, not left behind.
    let _ = refused.kill();
    let _ = refused.wait();
    assert!(
        status.is_some_and(|s| !s.success()),
        "a 1 MiB window in a 64 KiB ring is refused: {status:?}"
    );

    let mut jar = Jar::start(
        &["--ring", "65536", "--window", "32768"],
        &sh("read go; head -c 1000000 /dev/zero | tr '\\000' a; echo; echo END"),
    );
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    // The program prints once its owner is there: with none, the jar drops
    // the oldest bytes by design (R12).
    owner
        .send(&Msg::Input {
            bytes: b"go\n".to_vec(),
        })
        .expect("go");
    // A live owner, slow to read: every byte arrives, contiguous.
    let mut seen = Seen::default();
    let mut heard = Instant::now();
    while !seen.contains("END") {
        assert!(
            Instant::now() < heard + WAIT,
            "nothing for {WAIT:?}; saw {} bytes",
            seen.text.len()
        );
        if let Some(m) = owner.recv(Duration::from_millis(200)).expect("recv") {
            heard = Instant::now();
            seen.take(m);
            std::thread::sleep(Duration::from_millis(5));
            if let Some(next) = seen.next_offset {
                owner.send(&Msg::Ack { offset: next }).expect("ack");
            }
        }
    }
    assert_eq!(seen.first_offset, Some(0));
    assert!(
        seen.frames.iter().all(|f| !matches!(f, Msg::Gap { .. })),
        "{:?}",
        seen.frames
    );
}

/// Finding 17 (R22, R32a): a pane that doesn't linger survives being handed
/// on. A release grants the successor the time a first owner gets.
#[test]
fn a_pane_that_does_not_linger_survives_a_handover() {
    let mut jar = Jar::start(
        &["--linger-ownerless-ms", "0"],
        &sh("trap 'exit 9' HUP; echo READY; while :; do sleep 0.05; done"),
    );
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    read_until(&mut owner, true, |s| s.contains("READY"));
    let pid = jar.program.expect("welcomed");
    owner
        .send(&Msg::Release {
            token: NEXT_TOKEN.to_vec(),
        })
        .expect("release");
    // The successor is another spyc, which takes a moment to arrive.
    std::thread::sleep(Duration::from_millis(500));
    let next = connect_within(&jar, Role::Owner, NEXT_TOKEN, WAIT);
    assert!(next.is_ok(), "the successor is welcomed: {:?}", next.err());
    assert!(running(pid), "the program runs on");
}

/// Finding 16 (R2): a program that ends while nobody is there keeps its exit
/// for the hold, counted from the exit, not from when its owner left.
#[test]
fn an_exit_with_nobody_there_is_kept_from_the_exit() {
    let mut jar = Jar::start(
        &["--linger-ms", "2000"],
        &sh("sleep 3; echo RESULT=42; exit 7"),
    );
    let owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    // spyc crashes; the program ends three seconds later.
    drop(owner);
    let mut ended = false;
    eventually("the exit, or the jar's end", || {
        ended = jar.process.try_wait().ok().flatten().is_some();
        ended || spyc_jar::client::describe(&jar.socket).is_ok_and(|f| field(&f, "exit") == "7")
    });
    assert!(!ended, "the jar ended as its program did");
    let mut back = connect_within(&jar, Role::Owner, TOKEN, WAIT).expect("a new owner");
    back.send(&Msg::Replay { from: 0 }).expect("replay");
    let seen = read_until(&mut back, true, |s| {
        s.frames.iter().any(|f| matches!(f, Msg::Exited { .. }))
    });
    assert!(
        seen.contains("RESULT=42"),
        "the last of the output is there"
    );
    assert!(
        seen.frames
            .iter()
            .any(|f| matches!(f, Msg::Exited { code: 7, .. })),
        "{:?}",
        seen.frames
    );
}

// The review's smaller items: an exit says which signal ended it, a reader
// that falls behind is told what it missed, input is never written to a file,
// and a frame too big to send is refused before it is sent.

/// A program ended by a signal is reported with that signal as its code, as
/// the frozen `exited` fixture has it.
#[test]
fn a_signalled_exit_reports_its_signal() {
    let mut jar = Jar::start(&[], &sh("kill -TERM $$"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let exit = recv_matching(&mut owner, |m| matches!(m, Msg::Exited { .. }));
    assert_eq!(
        exit,
        Msg::Exited {
            code: 15,
            signaled: true
        }
    );
    assert_eq!(described(&jar, "exit"), "signal 15");
}

/// R12a: a reader that falls behind the ring is told which bytes it missed,
/// as a replay from before the ring would be, never handed a jump.
#[test]
fn a_reader_that_falls_behind_is_told_what_it_missed() {
    // The reader reads nothing while the owner takes all 3 MB, however long
    // a loaded machine makes that: a deadline past it, so the jar doesn't
    // close a reader that is only behind for one that has stopped.
    let mut jar = Jar::start(
        &[
            "--ring",
            "65536",
            "--window",
            "32768",
            "--deadline-ms",
            "60000",
        ],
        &sh(
            "read go; yes line-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | head -c 3000000; echo; echo END",
        ),
    );
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut late = jar.connect(Role::Reader);
    late.send(&Msg::Replay { from: 0 }).expect("replay");
    owner
        .send(&Msg::Input {
            bytes: b"go\n".to_vec(),
        })
        .expect("go");
    // The owner keeps up; the reader reads nothing until the program is done.
    read_while_it_comes(&mut owner, true, |s| s.contains("END"));
    let (mut next, mut gaps, mut tail) = (None::<u64>, 0, Vec::new());
    let mut heard = Instant::now();
    while !String::from_utf8_lossy(&tail).contains("END") {
        assert!(
            Instant::now() < heard + WAIT,
            "the reader stopped short of the end, at {next:?}"
        );
        let got = late.recv(Duration::from_millis(200)).expect("recv");
        if got.is_some() {
            heard = Instant::now();
        }
        match got {
            Some(Msg::Gap { from, to }) => {
                if let Some(n) = next {
                    assert_eq!(from, n, "a gap from where the reader was");
                }
                next = Some(to);
                gaps += 1;
            }
            Some(Msg::Output { offset, bytes }) => {
                if let Some(n) = next {
                    assert_eq!(offset, n, "output after {gaps} gap(s) jumped");
                }
                next = Some(offset + bytes.len() as u64);
                tail.extend_from_slice(&bytes);
                tail.drain(..tail.len().saturating_sub(64));
            }
            _ => {}
        }
    }
    assert!(
        gaps > 0,
        "a reader 3 MB behind a 64 KiB ring missed nothing?"
    );
}

/// What the owner types never reaches a file, whatever the jar's
/// environment says.
#[test]
fn input_is_never_written_to_a_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let trace = dir.path().join("trace");
    let mut jar = Jar::start_env(&[("SPYC_JAR_TRACE", &trace)], &[], &["/bin/cat"]);
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    owner
        .send(&Msg::Input {
            bytes: b"a secret\n".to_vec(),
        })
        .expect("input");
    read_until(&mut owner, true, |s| s.contains("a secret\r\na secret"));
    assert!(
        !trace.exists(),
        "the input was written to {}",
        trace.display()
    );
}

/// A frame over the limit would end the connection that sent it, so a
/// client refuses to send one, and keeps its connection.
#[test]
fn a_frame_over_the_limit_is_refused_before_it_is_sent() {
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let sent = owner.send(&Msg::StoreSnapshot(Snapshot {
        format: SNAPSHOT_VT,
        offset: 0,
        rows: 24,
        cols: 80,
        blob: vec![b's'; spyc_jar::proto::MAX_FRAME],
    }));
    assert!(
        sent.as_ref()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::InvalidInput),
        "{sent:?}"
    );
    owner.send(&Msg::Ping { n: 7 }).expect("ping");
    recv_matching(&mut owner, |m| matches!(m, Msg::Pong { n: 7 }));
}

/// A snapshot is handed back in a frame one byte bigger than the one that
/// stored it, so one that only just fits is refused when it is stored, not
/// stored and then impossible to fetch.
#[test]
fn a_snapshot_too_big_to_hand_back_is_refused() {
    use spyc_jar::proto::{MAX_FRAME, encode};
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    let mut owner = jar.connect(Role::Owner);
    let snapshot = |blob: Vec<u8>| {
        Msg::StoreSnapshot(Snapshot {
            format: SNAPSHOT_VT,
            offset: 0,
            rows: 24,
            cols: 80,
            blob,
        })
    };
    // A frame's length field counts everything after itself.
    let overhead = encode(&snapshot(Vec::new())).len() - 4;
    let biggest = snapshot(vec![b's'; MAX_FRAME - overhead]);
    assert_eq!(encode(&biggest).len() - 4, MAX_FRAME, "it fits a frame");
    owner.send(&biggest).expect("store");
    let answer = recv_matching(&mut owner, |m| matches!(m, Msg::Refused { .. }));
    assert!(
        matches!(
            answer,
            Msg::Refused {
                reason: spyc_jar::proto::reason::INVALID,
                ..
            }
        ),
        "{answer:?}"
    );
    owner.send(&Msg::FetchSnapshot).expect("fetch");
    let fetched = recv_matching(&mut owner, |m| matches!(m, Msg::Snapshot(_)));
    assert_eq!(fetched, Msg::Snapshot(None), "nothing was stored");
}

// The property tests JAR_PROTOCOL.md promises under "Fuzzing": sequences of
// messages, drawn at random, across the roles. Each runs against one jar,
// checking the property after every sequence.

mod sequences {
    use std::cell::RefCell;

    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};

    use super::*;

    /// What a reader types, if anything lets it.
    const MARK: &[u8] = b"R34DER-TYPED";

    fn runner() -> TestRunner {
        TestRunner::new(Config {
            cases: 32,
            // The jar's state carries from case to case, so a shrunk case may
            // pass on its own: a failure shows the sequence that failed.
            max_shrink_iters: 0,
            failure_persistence: None,
            ..Config::default()
        })
    }

    /// Any message a client could send, every kind a jar sends included,
    /// with whatever it would type carrying [`MARK`].
    fn any_msg() -> impl Strategy<Value = Msg> {
        let typed = [MARK, b"\n"].concat();
        prop_oneof![
            (0u64..4096).prop_map(|from| Msg::Replay { from }),
            any::<u64>().prop_map(|n| Msg::Ping { n }),
            any::<u64>().prop_map(|offset| Msg::Ack { offset }),
            Just(Msg::Input {
                bytes: typed.clone()
            }),
            Just(Msg::Reply { bytes: typed }),
            (1u16..200, 1u16..400).prop_map(|(rows, cols)| Msg::Resize {
                rows,
                cols,
                px_w: 0,
                px_h: 0
            }),
            (1i32..=6).prop_map(|signal| Msg::Signal { signal }),
            (0u64..4096).prop_map(|offset| Msg::StoreSnapshot(Snapshot {
                format: SNAPSHOT_VT,
                offset,
                rows: 24,
                cols: 80,
                blob: MARK.to_vec(),
            })),
            Just(Msg::FetchSnapshot),
            Just(Msg::Describe),
            Just(Msg::AckExit),
            Just(Msg::Detach),
            Just(Msg::Release {
                token: NEXT_TOKEN.to_vec()
            }),
            Just(Msg::SetInfo {
                key: "owner_mcp".into(),
                value: "elsewhere".into()
            }),
            Just(Msg::Upgrade {
                binary: "/nonexistent/spyc-jar".into()
            }),
            Just(Msg::Hello {
                version: 1,
                role: Role::Owner,
                token: TOKEN.to_vec()
            }),
            Just(Msg::Output {
                offset: 0,
                bytes: MARK.to_vec()
            }),
        ]
    }

    /// Send `seq` on `client`, then wait until the jar has acted on all of
    /// it: a ping's answer, or the connection's end.
    fn settle(client: &mut Client, seq: &[Msg]) {
        for m in seq {
            if client.send(m).is_err() {
                return;
            }
        }
        if client.send(&Msg::Ping { n: u64::MAX }).is_err() {
            return;
        }
        let until = Instant::now() + WAIT;
        while Instant::now() < until {
            match client.recv(Duration::from_millis(200)) {
                Ok(Some(Msg::Pong { n: u64::MAX })) | Err(_) => return,
                _ => {}
            }
        }
        panic!("the jar never answered the ping after {seq:?}");
    }

    /// The program's output by offset, as an owner hears it, whatever it was
    /// replayed from.
    #[derive(Default)]
    struct Screen(Vec<u8>);

    impl Screen {
        fn take(&mut self, msg: &Msg) {
            if let Msg::Output { offset, bytes } = msg {
                let at = usize::try_from(*offset).expect("offset");
                if self.0.len() < at + bytes.len() {
                    self.0.resize(at + bytes.len(), 0);
                }
                self.0[at..at + bytes.len()].copy_from_slice(bytes);
            }
        }

        fn contains(&self, needle: &[u8]) -> bool {
            self.0.windows(needle.len()).any(|w| w == needle)
        }
    }

    /// Have the owner type `line` and read until the program has echoed it.
    fn sentinel(owner: &mut Client, screen: &mut Screen, line: &str) {
        owner
            .send(&Msg::Input {
                bytes: format!("{line}\n").into_bytes(),
            })
            .expect("sentinel");
        let until = Instant::now() + WAIT;
        while !screen.contains(line.as_bytes()) {
            assert!(Instant::now() < until, "the program never echoed {line}");
            if let Some(m) = owner.recv(Duration::from_millis(200)).expect("recv") {
                screen.take(&m);
                if let Msg::Output { offset, bytes } = &m {
                    let _ = owner.send(&Msg::Ack {
                        offset: offset + bytes.len() as u64,
                    });
                }
            }
        }
    }

    /// No sequence lets a reader type: whatever a reader sends, the program
    /// receives only what the owner typed, and the token still opens the jar.
    #[test]
    fn no_sequence_lets_a_reader_type() {
        let mut jar = Jar::start(&[], &sh("stty raw -echo; exec cat"));
        let owner = jar.connect(Role::Owner);
        owner.send(&Msg::Replay { from: 0 }).expect("replay");
        let state = RefCell::new((owner, Screen::default(), 0u32));
        let result = runner().run(&prop::collection::vec(any_msg(), 1..12), |seq| {
            let mut reader = connect_within(&jar, Role::Reader, TOKEN, WAIT)
                .map_err(|e| TestCaseError::fail(format!("the token opens nothing: {e:#}")))?;
            settle(&mut reader, &seq);
            let (owner, screen, round) = &mut *state.borrow_mut();
            *round += 1;
            sentinel(owner, screen, &format!("SENTINEL-{round}"));
            prop_assert!(!screen.contains(MARK), "a reader typed, sending {seq:?}");
            Ok(())
        });
        assert!(result.is_ok(), "{result:?}");
    }

    /// A `hello`'s token, anything but the jar's own.
    fn wrong_token() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![
            Just(Vec::new()),
            prop::collection::vec(any::<u8>(), 1..40),
            (1..TOKEN.len()).prop_map(|n| TOKEN[..n].to_vec()),
            (0..TOKEN.len(), 1u8..=255).prop_map(|(i, x)| {
                let mut t = TOKEN.to_vec();
                t[i] ^= x;
                t
            }),
            Just([TOKEN, b"x"].concat()),
        ]
    }

    fn any_role() -> impl Strategy<Value = Role> {
        prop_oneof![
            Just(Role::Owner),
            Just(Role::Reader),
            Just(Role::Lookup),
            Just(Role::Program),
        ]
    }

    /// No sequence takes the lease without the token: every `hello` with
    /// another token is refused but a lookup's, nothing a lookup sends
    /// changes that, and the jar's own token still takes the lease.
    #[test]
    fn no_sequence_takes_the_lease_without_the_token() {
        let jar = Jar::start(&[], &["/bin/cat"]);
        let attempt = (
            any_role(),
            wrong_token(),
            prop::collection::vec(any_msg(), 0..6),
        );
        let result = runner().run(&prop::collection::vec(attempt, 1..6), |attempts| {
            for (role, token, seq) in &attempts {
                let got = Client::connect(&jar.socket, *role, WAIT, token);
                match (role, got) {
                    (Role::Lookup, Ok((mut c, _))) => settle(&mut c, seq),
                    (Role::Lookup, Err(e)) => {
                        return Err(TestCaseError::fail(format!("a lookup refused: {e:#}")));
                    }
                    (_, Ok(_)) => {
                        return Err(TestCaseError::fail(format!(
                            "{role:?} welcomed with token {token:?}"
                        )));
                    }
                    (_, Err(_)) => {}
                }
            }
            let owner = connect_within(&jar, Role::Owner, TOKEN, WAIT);
            prop_assert!(
                owner.is_ok(),
                "the token no longer takes the lease after {attempts:?}: {:?}",
                owner.err()
            );
            drop(owner);
            // Until the jar has seen this owner go, the next case's owner
            // would be refused as a second one.
            eventually("the owner's going", || {
                described(&jar, "owner").starts_with("none")
            });
            Ok(())
        });
        assert!(result.is_ok(), "{result:?}");
    }

    /// What clients ask of the jar's log without typing.
    #[derive(Debug, Clone)]
    enum Ask {
        OwnerReplay(u64),
        ReaderReplay(u64),
        NewOwner(u64),
        FetchSnapshot,
        StoreSnapshot(u64),
    }

    fn any_ask() -> impl Strategy<Value = Ask> {
        prop_oneof![
            (0u64..64).prop_map(Ask::OwnerReplay),
            (0u64..64).prop_map(Ask::ReaderReplay),
            (0u64..64).prop_map(Ask::NewOwner),
            Just(Ask::FetchSnapshot),
            (0u64..16).prop_map(Ask::StoreSnapshot),
        ]
    }

    /// No sequence answers a replayed query: the program's terminal queries
    /// are replayed, from anywhere, to owners old and new and to readers, and
    /// the program receives nothing but what an owner typed. Whether an
    /// owner answers what it replays is spyc's half (R15a).
    #[test]
    fn no_sequence_answers_a_replayed_query() {
        const QUERIES: &[u8] = b"\x1b[c\x1b[6n\x1b]11;?\x07";
        let mut jar = Jar::start(
            &[],
            &sh("stty raw -echo; printf '\\033[c\\033[6n\\033]11;?\\007'; exec cat"),
        );
        let mut owner = jar.connect(Role::Owner);
        owner.send(&Msg::Replay { from: 0 }).expect("replay");
        // Once the queries are out, the terminal no longer echoes: what
        // comes back after them is what the program read.
        let mut screen = Screen::default();
        let until = Instant::now() + WAIT;
        while !screen.contains(QUERIES) {
            assert!(Instant::now() < until, "the program never asked");
            if let Some(m) = owner.recv(Duration::from_millis(200)).expect("recv") {
                screen.take(&m);
            }
        }
        let state = RefCell::new((Some(owner), screen, 0u32, Vec::<String>::new()));
        let result = runner().run(&prop::collection::vec(any_ask(), 1..10), |asks| {
            let (held, screen, round, typed) = &mut *state.borrow_mut();
            for ask in &asks {
                if let Ask::NewOwner(_) = ask
                    && let Some(old) = held.take()
                {
                    old.drop_connection();
                    *held = Some(reconnect_owner(&jar).map_err(TestCaseError::fail)?);
                }
                let owner = held.as_mut().expect("an owner");
                match ask {
                    // A new owner was connected above.
                    Ask::OwnerReplay(from) | Ask::NewOwner(from) => {
                        settle(owner, &[Msg::Replay { from: *from }]);
                    }
                    Ask::ReaderReplay(from) => {
                        let mut r = connect_within(&jar, Role::Reader, TOKEN, WAIT)
                            .map_err(|e| TestCaseError::fail(format!("{e:#}")))?;
                        settle(&mut r, &[Msg::Replay { from: *from }]);
                    }
                    Ask::FetchSnapshot => settle(owner, &[Msg::FetchSnapshot]),
                    Ask::StoreSnapshot(at) => settle(
                        owner,
                        &[Msg::StoreSnapshot(Snapshot {
                            format: SNAPSHOT_VT,
                            offset: *at,
                            rows: 24,
                            cols: 80,
                            blob: QUERIES.to_vec(),
                        })],
                    ),
                }
            }
            *round += 1;
            let line = format!("SENTINEL-{round}");
            sentinel(held.as_mut().expect("an owner"), screen, &line);
            typed.push(line);
            // All the program ever printed: its queries, then each line an
            // owner typed, echoed once.
            let mut want = QUERIES.to_vec();
            for t in typed.iter() {
                want.extend_from_slice(t.as_bytes());
                want.push(b'\n');
            }
            prop_assert_eq!(
                String::from_utf8_lossy(&screen.0[..want.len().min(screen.0.len())]),
                String::from_utf8_lossy(&want),
                "after {:?}",
                asks
            );
            prop_assert_eq!(screen.0.len(), want.len());
            Ok(())
        });
        assert!(result.is_ok(), "{result:?}");
    }

    /// The owner, replaced: the old one goes, and a new one takes the lease
    /// once the jar has seen it go.
    fn reconnect_owner(jar: &Jar) -> Result<Client, String> {
        let until = Instant::now() + WAIT;
        loop {
            match connect_within(jar, Role::Owner, TOKEN, Duration::from_millis(500)) {
                Ok(c) => return Ok(c),
                Err(e) if Instant::now() >= until => return Err(format!("{e:#}")),
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }
}
