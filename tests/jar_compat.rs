//! A later spyc drives a released jar (the 3.0 review's A8 and F4). A jar
//! released with version 1 runs, and speaks version 1, for as long as its
//! program does, across every spyc installed after it; each must still start
//! one, talk to it (every request spyc sends a jar it owns), and upgrade it
//! to its own build.
//!
//! The jar under test is `SPYC_JAR_OLD_BIN` when it is set, a released
//! `spyc-jar` (`make test-jar-compat` fetches each stable release's from 3.0.0
//! on and runs this with it), and otherwise the one this crate builds. Every
//! wait is bounded by a limit the jar keeps (`spyc_jar::limits`).

use std::io::Read;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use spyc_jar::cli::Start;
use spyc_jar::client::Client;
use spyc_jar::limits::{OWNER_DEADLINE, UPGRADE_AT_MOST};
use spyc_jar::proto::{self, JarSignal, Msg, Role, SNAPSHOT_VT, Snapshot};

/// How long the test waits for any answer but an upgrade's: the jar's own
/// deadline for its owner. Out of an upgrade a jar answers well inside it,
/// and its start holds the program that long for a first owner.
const WAIT: Duration = OWNER_DEADLINE;
const TOKEN: &[u8] = b"jar-compat-token-0123456789";
/// The variable naming a released jar; `scripts/jar_fixtures.py` sets it,
/// and its test checks it names this one.
const OLD_BIN: &str = "SPYC_JAR_OLD_BIN";
/// The signal the program ends of: `SIGTERM`, 15 on macOS and Linux alike.
const SIGTERM: i32 = 15;
/// The kinds of a `hello` and a `welcome` (`JAR_PROTOCOL.md`, "Version 1
/// on the wire").
const HELLO: u8 = 1;
const WELCOME: u8 = 64;
/// Kinds no version-1 jar knows: one critical, one ancillary.
const UNKNOWN_CRITICAL: u8 = 59;
const UNKNOWN_ANCILLARY: u8 = 60;
/// The program: an echo of each line it reads, until its input ends.
const PROGRAM: &str = "stty -echo; while read l; do echo got=$l; done";

#[test]
fn a_later_spyc_drives_a_released_jar() {
    let mut jar = OldJar::start();

    // A lookup, as `spyc --jars` and the orphan look make: every key spyc
    // reads is there.
    let fields = jar.describe();
    for key in [
        "created",
        "build",
        "pid",
        "program",
        "program_pid",
        "owner",
        "linger",
        "log",
    ] {
        assert!(
            fields.iter().any(|(k, _)| k == key),
            "the jar's description has no {key:?}: {fields:?}"
        );
    }
    // The jar that runs is the one named, so a fixture run drives the
    // released jar and not this crate's.
    let image = fields
        .iter()
        .find(|(k, _)| k == "jar")
        .map_or_default(|(_, v)| v.clone());
    let named = std::fs::canonicalize(&jar.binary).expect("the jar's binary");
    assert!(
        image
            .split_once(' ')
            .is_some_and(|(_, path)| std::fs::canonicalize(path).is_ok_and(|p| p == named)),
        "the jar that runs is {image:?}, not {}",
        jar.binary.display()
    );

    // A later spyc's `hello`: its own version, and capabilities after the
    // token. A version-1 jar reads the fields it knows and welcomes it at a
    // version both speak.
    let later = proto::VERSION + 1;
    let welcome = jar.raw_hello(later, b"\x00\x04caps");
    assert_eq!(
        welcome.kind, WELCOME,
        "a longer hello is welcomed: {welcome:?}"
    );
    let spoken = u16::from_be_bytes([welcome.payload[0], welcome.payload[1]]);
    assert!(
        (1..=proto::VERSION).contains(&spoken),
        "welcomed at version {spoken}: a jar speaks a later client its own version, no later than this one's"
    );
    if !jar.released {
        assert_eq!(
            spoken,
            proto::VERSION,
            "this build's jar speaks a later client its own version"
        );
    }

    let mut owner = jar.owner();
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    owner.input(b"one\n");
    owner.until("its program's answer", |m, s| {
        matches!(m, Msg::Output { .. }) && s.text().contains("got=one")
    });

    // What else spyc sends a jar it owns: the tab's label, the answer to one
    // of the program's queries, an ack of what it parsed, and a snapshot
    // stored and fetched back.
    owner
        .send(&Msg::SetInfo {
            key: "label".into(),
            value: "jar-compat".into(),
        })
        .expect("set_info");
    owner
        .send(&Msg::Reply {
            bytes: b"reply\n".to_vec(),
        })
        .expect("reply");
    owner.until("the program's answer to a reply", |m, s| {
        matches!(m, Msg::Output { .. }) && s.text().contains("got=reply")
    });
    let parsed = owner.seen.end;
    owner.send(&Msg::Ack { offset: parsed }).expect("ack");
    let snap = Snapshot {
        format: SNAPSHOT_VT,
        offset: parsed,
        rows: 24,
        cols: 80,
        blob: b"\x1b[2Jcompat".to_vec(),
    };
    owner
        .send(&Msg::StoreSnapshot(snap.clone()))
        .expect("store_snapshot");
    owner.send(&Msg::FetchSnapshot).expect("fetch_snapshot");
    owner.until("the stored snapshot", |m, _| match m {
        Msg::Snapshot(Some(s)) => *s == snap,
        Msg::Refused { reason, why } => panic!("refused ({reason}): {why}"),
        Msg::Unsupported { kind } => panic!("kind {kind} unsupported"),
        _ => false,
    });
    assert!(
        jar.describe()
            .iter()
            .any(|(k, v)| k == "label" && v == "jar-compat"),
        "the owner's label is in the jar's description"
    );

    // A critical kind the jar doesn't know: refused as unsupported, and the
    // connection carries on.
    owner.raw(UNKNOWN_CRITICAL, proto::CRITICAL, b"a future request");
    owner.until("unsupported", |m, _| match m {
        Msg::Unsupported { kind } => *kind == UNKNOWN_CRITICAL,
        Msg::Refused { reason, why } => panic!("refused ({reason}): {why}"),
        _ => false,
    });

    // An ancillary one: skipped without a word.
    owner.raw(UNKNOWN_ANCILLARY, 0, b"a future notice");
    owner.send(&Msg::Ping { n: 7 }).expect("ping");
    owner.until("the pong", |m, _| match m {
        Msg::Unsupported { kind } => panic!("an ancillary kind {kind} was answered"),
        Msg::Pong { n } => *n == 7,
        _ => false,
    });

    owner
        .send(&Msg::Resize {
            rows: 30,
            cols: 100,
            px_w: 0,
            px_h: 0,
        })
        .expect("resize");
    owner.until("the resize", |m, _| {
        matches!(
            m,
            Msg::Resized {
                rows: 30,
                cols: 100,
                ..
            }
        )
    });

    // Upgraded to this crate's jar: the same process, its program and this
    // connection running on.
    let next = jar.next_binary();
    owner
        .send(&Msg::Upgrade {
            binary: next.display().to_string(),
        })
        .expect("upgrade");
    owner.until_within(UPGRADE_AT_MOST, "the upgrade's answer", |m, _| match m {
        Msg::Upgraded { .. } => true,
        Msg::Refused { reason, why } => panic!("the upgrade was refused ({reason}): {why}"),
        _ => false,
    });
    let pid = jar.process.id().to_string();
    assert!(
        jar.describe().iter().any(|(k, v)| k == "pid" && *v == pid),
        "the upgraded jar is the same process"
    );
    owner.input(b"two\n");
    owner.until("its program's answer after the upgrade", |m, s| {
        matches!(m, Msg::Output { .. }) && s.text().contains("got=two")
    });

    // A signal, as a closing tab sends one: the program ends of it, and its
    // end, reported and collected, ends the jar.
    owner
        .send(&Msg::Signal {
            signal: JarSignal::Terminate as i32,
        })
        .expect("signal");
    owner.until("the program's exit", |m, _| match m {
        Msg::Exited { code, signaled } => {
            assert!(
                *signaled && *code == SIGTERM,
                "ended by {code} (signalled: {signaled})"
            );
            true
        }
        Msg::Refused { reason, why } => panic!("the signal was refused ({reason}): {why}"),
        _ => false,
    });
    owner.send(&Msg::AckExit).expect("ack the exit");
    jar.ends();
}

/// What an owner acknowledges paces its jar: a jar keeps no more than its
/// window unacknowledged, so a program that prints far past the window
/// finishes only for an owner whose acks the jar honours, as spyc's are.
#[test]
fn a_released_jar_goes_on_as_far_as_its_owner_acknowledges() {
    const WINDOW: u64 = 4096;
    const PRINTED: u64 = 1 << 18;
    let program =
        format!("stty -echo; head -c {PRINTED} /dev/zero | tr '\\0' x; echo; echo all-printed");
    let mut jar = OldJar::start_with(&["--window", &WINDOW.to_string()], &program);
    let mut owner = jar.owner();
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut acked = 0;
    let end = Instant::now() + WAIT;
    while !owner.seen.output.ends_with(b"all-printed\r\n")
        && !owner.seen.output.ends_with(b"all-printed\n")
    {
        let left = end.saturating_duration_since(Instant::now());
        assert!(
            !left.is_zero(),
            "the program stopped at {} bytes of {PRINTED} with each one acknowledged",
            owner.seen.end
        );
        let got = owner.client.recv(left).expect("recv");
        if let Some(Msg::Output { offset, bytes }) = got {
            owner.seen.output.extend(&bytes);
            owner.seen.end = owner.seen.end.max(offset + bytes.len() as u64);
        }
        if owner.seen.end > acked {
            acked = owner.seen.end;
            owner.send(&Msg::Ack { offset: acked }).expect("ack");
        }
    }
    assert!(
        owner.seen.end > PRINTED,
        "all of it came through the window"
    );
    owner.until("the program's exit", |m, _| matches!(m, Msg::Exited { .. }));
    owner.send(&Msg::AckExit).expect("ack the exit");
    jar.ends();
}

/// A jar started as spyc starts one: `--wire 1`, its start on standard input.
struct OldJar {
    /// The binary started, and whether it is a released jar's.
    binary: PathBuf,
    released: bool,
    dir: tempfile::TempDir,
    socket: PathBuf,
    process: Child,
    program: Option<u32>,
    stderr: tempfile::NamedTempFile,
}

impl OldJar {
    fn start() -> Self {
        Self::start_with(&[], PROGRAM)
    }

    /// A jar started with `extra` after `--wire 1`, running `program`.
    fn start_with(extra: &[&str], program: &str) -> Self {
        let named = std::env::var_os(OLD_BIN);
        let released = named.is_some();
        let binary = named.map_or_else(
            || PathBuf::from(env!("CARGO_BIN_EXE_spyc-jar")),
            PathBuf::from,
        );
        // Under `/tmp`, because macOS caps a socket's path at about 104 bytes,
        // and `0700`, as a jar requires.
        let dir = tempfile::Builder::new()
            .prefix("jc")
            .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .tempdir_in("/tmp")
            .expect("tempdir");
        let socket = dir.path().join("j.sock");
        let start = Start {
            token: TOKEN.to_vec(),
            socket: socket.clone(),
            cwd: None,
            rows: 24,
            cols: 80,
            hold: Some(WAIT),
            exit_linger: None,
            argv: ["/bin/sh", "-c", program].map(String::from).to_vec(),
        };
        // A file, not a pipe: on macOS a pipe's ends become close-on-exec
        // only after they're made, so a process another test starts between
        // can hold the start open.
        let start_at = dir.path().join("start");
        std::fs::write(&start_at, start.encode()).expect("write the start");
        let stdin = std::fs::File::open(&start_at).expect("open the start");
        let stderr = tempfile::Builder::new()
            .prefix("jar-stderr")
            .tempfile()
            .expect("the jar's stderr file");
        let process = Command::new(&binary)
            .args(["--wire", "1"])
            .args(extra)
            .stdin(stdin)
            .stdout(Stdio::null())
            .stderr(stderr.reopen().expect("the jar's stderr file"))
            .spawn()
            .unwrap_or_else(|e| panic!("start {}: {e}", binary.display()));
        std::fs::remove_file(&start_at).expect("remove the start");
        Self {
            binary,
            released,
            dir,
            socket,
            process,
            program: None,
            stderr,
        }
    }

    fn describe(&self) -> Vec<(String, String)> {
        let (mut c, _) = Client::connect(&self.socket, Role::Lookup, WAIT, &[]).expect("a lookup");
        c.send(&Msg::Describe).expect("describe");
        let end = Instant::now() + WAIT;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "no description within {WAIT:?}");
            if let Some(Msg::Description { fields }) = c.recv(left).expect("recv") {
                return fields;
            }
        }
    }

    /// The jar's first frame to a reader's `hello` of `version` with
    /// `capabilities` after its token.
    fn raw_hello(&self, version: u16, capabilities: &[u8]) -> Frame {
        let mut hello = proto::encode(&Msg::Hello {
            version,
            role: Role::Reader,
            token: TOKEN.to_vec(),
        });
        hello.extend(capabilities);
        let len = u32::try_from(hello.len() - 4).expect("a small frame");
        hello[..4].copy_from_slice(&len.to_be_bytes());
        assert_eq!(hello[4], HELLO);
        let mut s = UnixStream::connect(&self.socket).expect("connect");
        std::io::Write::write_all(&mut s, &hello).expect("the hello");
        s.set_read_timeout(Some(WAIT)).expect("a read timeout");
        let mut head = [0u8; 6];
        s.read_exact(&mut head)
            .expect("an answer to a longer hello");
        let len = usize::try_from(u32::from_be_bytes([head[0], head[1], head[2], head[3]]))
            .expect("a frame's length fits");
        let mut payload = vec![0u8; len.saturating_sub(2)];
        s.read_exact(&mut payload).expect("the answer's payload");
        Frame {
            kind: head[4],
            payload,
        }
    }

    fn owner(&mut self) -> Owner {
        let (client, welcome) =
            Client::connect(&self.socket, Role::Owner, WAIT, TOKEN).expect("connect as owner");
        let Msg::Welcome { child_pid, .. } = welcome else {
            panic!("expected a welcome, got {welcome:?}");
        };
        self.program = Some(child_pid);
        Owner {
            client,
            seen: Seen::default(),
        }
    }

    /// A copy of this crate's jar, under another name, to upgrade to.
    fn next_binary(&self) -> PathBuf {
        let next = self.dir.path().join("jar-next");
        std::fs::copy(env!("CARGO_BIN_EXE_spyc-jar"), &next).expect("copy the jar binary");
        next
    }

    /// Wait for the jar to end, its program's exit collected.
    fn ends(&mut self) {
        let end = Instant::now() + WAIT;
        while self.process.try_wait().expect("try_wait").is_none() {
            assert!(
                Instant::now() < end,
                "the jar runs on after its exit was collected"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        self.program = None;
    }
}

impl Drop for OldJar {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "the jar (pid {}) wrote to its stderr:\n{}",
                self.process.id(),
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

#[derive(Debug)]
struct Frame {
    kind: u8,
    payload: Vec<u8>,
}

/// What an owner has heard, printed when a wait fails.
#[derive(Default)]
struct Seen {
    frames: Vec<String>,
    output: Vec<u8>,
    /// The end of the program's output heard so far, as the jar counts it.
    end: u64,
}

impl Seen {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }
}

struct Owner {
    client: Client,
    seen: Seen,
}

impl Owner {
    fn send(&self, msg: &Msg) -> std::io::Result<()> {
        self.client.send(msg)
    }

    fn input(&self, bytes: &[u8]) {
        self.send(&Msg::Input {
            bytes: bytes.to_vec(),
        })
        .expect("input");
    }

    /// A frame of `kind` with `flags`, as a newer spyc would send one.
    fn raw(&self, kind: u8, flags: u8, payload: &[u8]) {
        let len = u32::try_from(payload.len() + 2).expect("a small frame");
        let mut f = len.to_be_bytes().to_vec();
        f.extend([kind, flags]);
        f.extend(payload);
        self.client.send_raw(&f).expect("a raw frame");
    }

    fn until(&mut self, what: &str, want: impl FnMut(&Msg, &Seen) -> bool) {
        self.until_within(WAIT, what, want);
    }

    /// Read until `want` takes a frame, for at most `wait`.
    fn until_within(
        &mut self,
        wait: Duration,
        what: &str,
        mut want: impl FnMut(&Msg, &Seen) -> bool,
    ) {
        let end = Instant::now() + wait;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            assert!(
                !left.is_zero(),
                "no {what} within {wait:?}; heard {:?}, the program printed {:?}",
                self.seen.frames,
                self.seen.text()
            );
            let got = self.client.recv(left).unwrap_or_else(|e| {
                panic!(
                    "the connection ended waiting for {what}: {e:#}; heard {:?}, the program printed {:?}",
                    self.seen.frames,
                    self.seen.text()
                )
            });
            let Some(m) = got else {
                continue;
            };
            if let Msg::Output { offset, bytes } = &m {
                self.seen.output.extend(bytes);
                self.seen.end = self.seen.end.max(offset + bytes.len() as u64);
            }
            self.seen
                .frames
                .push(format!("{m:?}").chars().take(120).collect());
            if want(&m, &self.seen) {
                return;
            }
        }
    }
}
