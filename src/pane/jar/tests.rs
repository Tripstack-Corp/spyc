use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use spyc_jar::proto::{self, JarSignal, Msg, Role};

use super::*;
use crate::pane::pty_host::{PtyEvent, PtyHost};

const WAIT: Duration = Duration::from_secs(5);

/// A directory under `/tmp`, short enough for a socket's path on macOS.
fn short_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("jt")
        .tempdir_in("/tmp")
        .unwrap()
}

/// A place in `dir`, starting `binary`.
fn place_in(dir: &Path, binary: PathBuf) -> Place {
    Place {
        binary,
        dirs: vec![dir.join("spyc"), dir.join("spyc").join("jars")],
        tokens: dir.join("tokens"),
    }
}

/// A jar that speaks the protocol and does nothing else: it welcomes one
/// owner, then hands the test each frame it receives, and a way to send.
struct FakeJar {
    frames: mpsc::Receiver<Msg>,
    to_owner: UnixStream,
    /// Listening, so the jar counts as running.
    listener: Option<UnixListener>,
    socket: PathBuf,
    _dir: tempfile::TempDir,
}

impl FakeJar {
    /// The next frame that isn't a ping.
    fn next(&self) -> Msg {
        let until = Instant::now() + WAIT;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match self.frames.recv_timeout(left) {
                Ok(Msg::Ping { .. }) => {}
                Ok(m) => return m,
                Err(e) => panic!("the jar heard nothing more: {e}"),
            }
        }
    }

    fn send(&mut self, msg: &Msg) {
        proto::write_msg(&mut self.to_owner, msg).unwrap();
    }

    /// Send the program's exit once a signal arrives, as a jar does, and
    /// return that signal.
    fn exit_on_signal(&mut self) -> i32 {
        loop {
            if let Msg::Signal { signal } = self.next() {
                self.send(&Msg::Exited {
                    code: 15,
                    signaled: true,
                });
                return signal;
            }
        }
    }
}

/// A host whose program runs in a fake jar that names `pid` as its program.
fn host_in_fake_jar(pid: u32) -> (FakeJar, PtyHost) {
    let dir = short_dir();
    let socket = dir.path().join("fake.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let accepting = listener.try_clone().unwrap();
    let (tx, frames) = mpsc::channel();
    let (conn_tx, conn_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut conn, _) = accepting.accept().unwrap();
        drop(accepting);
        let Ok(Some(Msg::Hello { .. })) = proto::read_msg(&mut conn) else {
            panic!("no hello");
        };
        proto::write_msg(
            &mut conn,
            &Msg::Welcome {
                version: proto::VERSION,
                child_pid: pid,
                head: 0,
                base: 0,
                rows: 24,
                cols: 80,
            },
        )
        .unwrap();
        conn_tx.send(conn.try_clone().unwrap()).unwrap();
        while let Ok(Some(m)) = proto::read_msg(&mut conn) {
            if tx.send(m).is_err() {
                break;
            }
        }
    });
    let (client, welcome) = Client::connect(&socket, Role::Owner, WAIT, b"token").unwrap();
    let jarred = connect(client, &welcome, &socket, (24, 80)).unwrap();
    let host = PtyHost::around_jar(jarred, (24, 80)).unwrap();
    let to_owner = conn_rx.recv_timeout(WAIT).unwrap();
    let jar = FakeJar {
        frames,
        to_owner,
        listener: Some(listener),
        socket,
        _dir: dir,
    };
    assert_eq!(
        jar.next(),
        Msg::Replay { from: 0 },
        "premise: the owner asks for its log"
    );
    (jar, host)
}

/// The bytes the host hands its consumer, until `n` have arrived.
fn read_bytes(host: &PtyHost, n: usize) -> Vec<u8> {
    let rx = host.event_rx.as_ref().unwrap();
    let mut got = Vec::new();
    while got.len() < n {
        match rx.recv_timeout(WAIT) {
            Ok(PtyEvent::Bytes(b)) => got.extend(b),
            Ok(PtyEvent::Closed) => break,
            Err(e) => panic!("only {got:?} arrived: {e}"),
        }
    }
    got
}

/// What the host hands its consumer, until it holds `needle`.
fn read_until(host: &PtyHost, needle: &str) -> String {
    let rx = host.event_rx.as_ref().unwrap();
    let mut got = Vec::new();
    while !String::from_utf8_lossy(&got).contains(needle) {
        match rx.recv_timeout(WAIT) {
            Ok(PtyEvent::Bytes(b)) => got.extend(b),
            Ok(PtyEvent::Closed) | Err(_) => {
                panic!("{needle:?} never came: {:?}", String::from_utf8_lossy(&got))
            }
        }
    }
    String::from_utf8_lossy(&got).into_owned()
}

/// A process of the test's own in a process group of its own, standing for
/// whatever now holds the pid the jar reported.
fn bystander() -> std::process::Child {
    use std::os::unix::process::CommandExt;
    std::process::Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .unwrap()
}

/// Finding 10: shutting a jarred pane down asks its jar, and never signals
/// the pid the jar reported, which may be someone else's by then.
#[test]
fn every_signal_goes_through_the_jar() {
    let mut other = bystander();
    let (mut jar, mut host) = host_in_fake_jar(other.id());

    host.suspend(false).unwrap();
    assert_eq!(
        jar.next(),
        Msg::Signal {
            signal: JarSignal::Stop as i32
        }
    );
    host.suspend(true).unwrap();
    assert_eq!(
        jar.next(),
        Msg::Signal {
            signal: JarSignal::Continue as i32
        }
    );
    let jar = std::thread::spawn(move || jar.exit_on_signal());
    host.shutdown(Duration::from_millis(250));
    assert_eq!(jar.join().unwrap(), JarSignal::Terminate as i32);
    assert!(
        other.try_wait().unwrap().is_none(),
        "the bystander holding the reported pid was signalled"
    );
    let _ = other.kill();
    let _ = other.wait();
}

/// A dropped pane's program is killed by its jar, as a local one is by spyc.
#[test]
fn a_dropped_jarred_pane_asks_its_jar_to_kill_the_program() {
    let mut other = bystander();
    let (jar, host) = host_in_fake_jar(other.id());
    drop(host);
    assert_eq!(
        jar.next(),
        Msg::Signal {
            signal: JarSignal::Kill as i32
        }
    );
    assert!(other.try_wait().unwrap().is_none());
    let _ = other.kill();
    let _ = other.wait();
}

/// R11 and R15: output reaches the pane and is acknowledged; keystrokes go
/// as `Input` and the terminal's answers as `Reply`.
#[test]
fn output_input_and_replies_each_travel_as_their_own_kind() {
    let (mut jar, mut host) = host_in_fake_jar(std::process::id());
    jar.send(&Msg::Output {
        offset: 0,
        bytes: b"hi".to_vec(),
    });
    assert_eq!(read_bytes(&host, 2), b"hi");
    assert_eq!(jar.next(), Msg::Ack { offset: 2 });
    host.write_all(b"k").unwrap();
    assert_eq!(
        jar.next(),
        Msg::Input {
            bytes: b"k".to_vec()
        }
    );
    host.reply_writer().send(b"\x1b[?62c".to_vec()).unwrap();
    assert_eq!(
        jar.next(),
        Msg::Reply {
            bytes: b"\x1b[?62c".to_vec()
        }
    );
    host.resize(30, 100).unwrap();
    assert_eq!(
        jar.next(),
        Msg::Resize {
            rows: 30,
            cols: 100,
            px_w: 0,
            px_h: 0
        }
    );
}

/// Finding 8: after a gap the pane is handed one resynchronizing byte, then
/// what follows; and a snapshot of everything it was handed is stored at the
/// jar offset of the last byte, not at the count of bytes it parsed.
#[test]
fn a_snapshot_after_a_gap_is_stored_at_the_jars_offset() {
    let (mut jar, host) = host_in_fake_jar(std::process::id());
    jar.send(&Msg::Output {
        offset: 0,
        bytes: b"0123456789".to_vec(),
    });
    jar.send(&Msg::Gap { from: 10, to: 50 });
    jar.send(&Msg::Output {
        offset: 50,
        bytes: b"abcde".to_vec(),
    });
    let handed = read_bytes(&host, 16);
    assert_eq!(handed, b"0123456789\x18abcde");
    let sink = host.snapshot_sink().unwrap();
    sink.store(
        handed.len() as u64,
        (24, 80),
        b"engine",
        Some(b"vt".to_vec()),
    );
    let mut stored = Vec::new();
    while stored.len() < 2 {
        if let Msg::StoreSnapshot(s) = jar.next() {
            stored.push((s.format, s.offset));
        }
    }
    assert_eq!(
        stored,
        [(proto::SNAPSHOT_BINARY, 55), (proto::SNAPSHOT_VT, 55)]
    );
}

#[test]
fn offsets_follow_the_jar_across_gaps() {
    let mut o = offsets::Offsets::new(100);
    assert!(!o.hand(100, 10));
    assert_eq!(o.at(4), 104);
    assert!(o.hand(200, 5), "a jump is a gap");
    assert_eq!(o.at(10), 110, "before the resynchronizing byte");
    assert_eq!(o.at(11), 200, "after it, nothing past the gap yet");
    assert_eq!(o.at(16), 205);
    assert!(!o.hand(205, 1));
    assert_eq!(o.at(17), 206);
}

/// The byte handed over after a gap ends whatever sequence the bytes before
/// it left open, so what follows parses as the program sent it.
#[test]
fn the_resynchronizing_byte_ends_a_sequence_cut_by_a_gap() {
    let screen = |cut: &[u8]| {
        let mut e = <crate::pane::PaneEngine as crate::pane::engine::Engine>::new(4, 20, 100);
        crate::pane::engine::Engine::process(&mut e, cut);
        crate::pane::engine::Engine::process(&mut e, b"1;2Hafter");
        crate::pane::engine::TerminalScreen::contents(crate::pane::engine::Engine::screen(&e))
    };
    assert!(
        !screen(b"\x1b[").contains("1;2H"),
        "premise: a cut CSI swallows what follows"
    );
    assert!(screen(&[b'\x1b', b'[', link::RESYNC]).contains("1;2Hafter"));
    assert!(screen(&[b'\x1b', b']', b'0', b';', link::RESYNC]).contains("1;2Hafter"));
}

/// Finding 1: a jar that won't run (one that doesn't speak this wire version
/// exits with 75) or isn't there leaves the program to run without one.
#[test]
fn a_jar_that_wont_run_leaves_the_program_to_run_without_one() {
    use std::os::unix::fs::PermissionsExt;
    let dir = short_dir();
    let refusing = dir.path().join("old-spyc-jar");
    std::fs::write(&refusing, "#!/bin/sh\nexit 75\n").unwrap();
    std::fs::set_permissions(&refusing, std::fs::Permissions::from_mode(0o755)).unwrap();
    for binary in [refusing, dir.path().join("missing")] {
        let place = place_in(dir.path(), binary.clone());
        let at = Instant::now();
        let host = PtyHost::spawn(crate::pane::pty_host::PtySpec {
            command: "printf ran",
            rows: 24,
            cols: 80,
            cwd: dir.path(),
            env: &[],
            term: "dumb",
            nudge_winch: false,
            debug_dump: false,
            exec_replace: false,
            capture_launch_env: false,
            jar: Some(Spec {
                place: place.clone(),
                name: "0123456789abcdef".into(),
            }),
        })
        .unwrap_or_else(|e| panic!("{}: the tab failed to open: {e:#}", binary.display()));
        assert!(!host.jarred(), "{}", binary.display());
        assert_eq!(read_bytes(&host, 3), b"ran");
        assert!(
            at.elapsed() < Duration::from_secs(3),
            "waited {:?}",
            at.elapsed()
        );
        let left: Vec<_> = std::fs::read_dir(&place.tokens).unwrap().collect();
        assert!(left.is_empty(), "a token outlived its jar: {left:?}");
    }
}

/// Finding 14: with no runtime directory or `TMPDIR`, jars live in a
/// directory of this user's own, never a `/tmp/spyc` anyone could make.
#[test]
fn the_socket_directory_is_per_user_without_a_runtime_dir() {
    let dirs = |r: Option<&str>, t: Option<&str>| {
        place::socket_dirs(r.map(Into::into), t.map(Into::into), 501)
    };
    assert_eq!(
        dirs(None, None),
        [
            PathBuf::from("/tmp/spyc-501"),
            PathBuf::from("/tmp/spyc-501/jars")
        ]
    );
    assert_eq!(dirs(Some(""), Some("")), dirs(None, None));
    assert_eq!(
        dirs(None, Some("/t")),
        [PathBuf::from("/t/spyc"), PathBuf::from("/t/spyc/jars")]
    );
    assert_eq!(
        dirs(Some("/run/user/501"), Some("/t")),
        [
            PathBuf::from("/run/user/501/spyc"),
            PathBuf::from("/run/user/501/spyc/jars")
        ]
    );
}

/// Finding 14: a socket directory is trusted only when it's this user's,
/// `0700` and not a link.
#[test]
fn a_socket_directory_others_could_reach_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let dir = short_dir();
    let place = place_in(dir.path(), PathBuf::from("/nonexistent"));
    place.prepare().unwrap();
    for d in &place.dirs {
        let mode = std::fs::metadata(d).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "{}", d.display());
    }
    std::fs::set_permissions(&place.dirs[0], std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = place.prepare().unwrap_err();
    assert!(format!("{e:#}").contains("open to others"), "{e:#}");
    std::fs::set_permissions(&place.dirs[0], std::fs::Permissions::from_mode(0o700)).unwrap();

    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).unwrap();
    let linked = dir.path().join("linked");
    std::os::unix::fs::symlink(&real, &linked).unwrap();
    let place = Place {
        dirs: vec![linked],
        ..place
    };
    let e = place.prepare().unwrap_err();
    assert!(format!("{e:#}").contains("isn't a directory"), "{e:#}");
}

/// R32: a sweep removes a socket nothing answers, and its token, but not one
/// too new to judge (a jar binds a moment before it listens), a live one, or
/// anything else.
#[test]
fn a_sweep_removes_only_old_sockets_nothing_answers() {
    let dir = short_dir();
    let place = place_in(dir.path(), PathBuf::from("/nonexistent"));
    place.prepare().unwrap();
    let live = place.socket("aaaa");
    let dead = place.socket("bbbb");
    let other = place.sockets().join("notes.txt");
    let _listening = UnixListener::bind(&live).unwrap();
    drop(UnixListener::bind(&dead).unwrap());
    std::fs::write(&other, "x").unwrap();
    tokens::save(&place, &dead, b"t").unwrap();
    tokens::save(&place, &live, b"t").unwrap();
    // A child another test forks meanwhile holds a copy of the listener until
    // it `exec`s.
    let until = Instant::now() + WAIT;
    while socket_state(&dead) != SocketState::Stale {
        assert!(Instant::now() < until, "premise: nothing answers");
        std::thread::sleep(Duration::from_millis(20));
    }

    assert_eq!(
        place::sweep(&place, Duration::from_secs(3600)),
        0,
        "too new to judge"
    );
    assert!(dead.exists());
    assert_eq!(place::sweep(&place, Duration::ZERO), 1);
    assert!(live.exists(), "a live jar's socket is kept");
    assert!(!dead.exists(), "a dead jar's socket is removed");
    assert!(other.exists(), "only sockets are swept");
    assert!(!place.tokens.join("bbbb").exists(), "with its token");
    assert!(place.tokens.join("aaaa").exists());
}

#[test]
fn only_a_current_or_rc_build_reads_the_setting() {
    assert!(honoured_by("2.5.0-CURRENT"));
    assert!(honoured_by("3.0.0-rc.1"));
    assert!(!honoured_by("2.5.0"));
    assert!(wanted(false, "0123").is_none(), "off is off");
}

/// A binary snapshot says which build made it; another build's, or one
/// with no stamp, is never decoded as this one's.
#[test]
fn a_binary_snapshot_carries_the_build_that_made_it() {
    let stamped = snapshots::stamp(b"engine");
    assert_eq!(snapshots::unstamp(&stamped), Some(&b"engine"[..]));
    assert_eq!(snapshots::unstamp(b"engine"), None);
    let mut foreign = stamped;
    foreign[snapshots::STAMP_MAGIC.len() + 2] ^= 0x20;
    assert_eq!(snapshots::unstamp(&foreign), None);
}

/// The `spyc-jar` this build made: in the profile's directory, which holds
/// the test binary at some depth.
fn built_jar() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.ancestors()
        .skip(1)
        .map(|d| d.join("spyc-jar"))
        .find(|j| j.is_file())
        .unwrap_or_else(|| {
            panic!(
                "no spyc-jar above {}: `cargo build --bin spyc-jar` (make test builds it)",
                exe.display()
            )
        })
}

/// A tab's program on a PTY of the real jar's, as `PtyHost` spawns it.
fn spawn_in(place: &Place, command: &str, cwd: &Path) -> anyhow::Result<PtyHost> {
    PtyHost::spawn(crate::pane::pty_host::PtySpec {
        command,
        rows: 24,
        cols: 80,
        cwd,
        env: &[],
        term: "dumb",
        nudge_winch: false,
        debug_dump: false,
        exec_replace: false,
        capture_launch_env: false,
        jar: Some(Spec {
            place: place.clone(),
            name: "0123456789abcdef".into(),
        }),
    })
}

/// The real jar: a tab's program runs in it, reads what's typed, finds its
/// jar by `SPYC_JAR_SOCKET`, and ends with it, the jar's socket gone after.
#[test]
fn a_program_runs_in_a_real_jar_and_ends_with_it() {
    let dir = short_dir();
    let place = place_in(dir.path(), built_jar());
    let mut host = spawn_in(
        &place,
        "printf \"[$SPYC_JAR_SOCKET]\"; read x; printf \"got:$x\"; exit 3",
        dir.path(),
    )
    .unwrap();
    assert!(host.jarred());
    let socket = place.socket("0123456789abcdef");
    read_until(&host, &format!("[{}]", socket.display()));
    host.write_all(b"hi\n").unwrap();
    read_until(&host, "got:hi");
    assert_eq!(host.child.wait().unwrap().exit_code(), 3);
    let until = Instant::now() + WAIT;
    while socket.exists() {
        assert!(
            Instant::now() < until,
            "the jar outlived its acknowledged exit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Finding 7, for a start: a tab whose jar still runs (its spyc crashed)
/// doesn't open, so its program never runs twice; a restart, whose last jar
/// ends a moment after its program, gets a new one.
#[test]
fn a_tab_whose_jar_still_runs_never_starts_its_program_twice() {
    let dir = short_dir();
    let place = place_in(dir.path(), built_jar());
    let mark = dir.path().join("ran");
    let mut first = spawn_in(&place, "exec cat", dir.path()).unwrap();
    assert!(first.jarred());
    let e = spawn_in(&place, &format!("touch {}", mark.display()), dir.path())
        .err()
        .expect("a second start while the tab's jar runs");
    assert!(format!("{e:#}").contains("wasn't started twice"), "{e:#}");
    assert!(!mark.exists(), "the program ran twice");
    first.write_all(b"still\n").unwrap();
    read_until(&first, "still");

    // As a tab's restart does (`Pane::try_kill`): the kill, then at once the
    // new start, while the last jar is still ending.
    first.child.kill().unwrap();
    let restarted = spawn_in(&place, "exec cat", dir.path()).unwrap();
    assert!(restarted.jarred(), "a restart gets a jar");
}

/// R17e: a snapshot follows a second of quiet, never a frame the program
/// holds open, and isn't taken again until more is parsed.
#[test]
fn a_snapshot_follows_a_second_of_quiet_and_never_a_held_frame() {
    let (jar, host) = host_in_fake_jar(std::process::id());
    let engine = std::sync::Mutex::new(
        <crate::pane::PaneEngine as crate::pane::engine::Engine>::new(24, 80, 100),
    );
    crate::pane::engine::Engine::process(&mut *engine.lock().unwrap(), b"hello");
    let mut s = Snapshotter::new(host.snapshot_sink().unwrap());
    s.parsed(5);
    s.tick(&engine, true);
    std::thread::sleep(Duration::from_millis(1100));
    s.tick(&engine, false);
    assert!(
        std::iter::from_fn(|| jar.frames.try_recv().ok()).all(|m| matches!(m, Msg::Ping { .. })),
        "a snapshot before a second's quiet, or mid-frame"
    );
    s.tick(&engine, true);
    let mut stored = Vec::new();
    while stored.len() < 2 {
        if let Msg::StoreSnapshot(snap) = jar.next() {
            stored.push((snap.format, snap.offset));
        }
    }
    assert_eq!(
        stored,
        [(proto::SNAPSHOT_BINARY, 5), (proto::SNAPSHOT_VT, 5)]
    );
    std::thread::sleep(Duration::from_millis(1100));
    s.tick(&engine, true);
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        std::iter::from_fn(|| jar.frames.try_recv().ok()).all(|m| matches!(m, Msg::Ping { .. })),
        "a second snapshot of nothing new"
    );
}

/// A jar that never reports the exit can't hold spyc's shutdown.
#[test]
fn a_jar_that_never_reports_an_exit_cannot_hold_a_shutdown() {
    let (jar, mut host) = host_in_fake_jar(std::process::id());
    let at = Instant::now();
    host.shutdown(Duration::from_millis(100));
    assert!(
        at.elapsed() < Duration::from_secs(4),
        "the shutdown waited {:?}",
        at.elapsed()
    );
    drop(jar);
}

/// Decision G's labels: a connection the jar ended while it still runs
/// leaves the tab `[detached]`; one whose jar is gone, `[jar lost]`.
#[test]
fn a_lost_connection_says_whether_the_jar_still_runs() {
    let (jar, mut host) = host_in_fake_jar(std::process::id());
    jar.to_owner.shutdown(std::net::Shutdown::Both).unwrap();
    let status = host.child.wait().unwrap();
    assert_eq!(status.signal(), Some(Lost::Held.label()));

    let (mut jar, mut host) = host_in_fake_jar(std::process::id());
    let socket = jar.socket.clone();
    drop(jar.listener.take());
    std::fs::remove_file(&socket).unwrap();
    jar.to_owner.shutdown(std::net::Shutdown::Both).unwrap();
    let status = host.child.wait().unwrap();
    assert_eq!(status.signal(), Some(Lost::Gone.label()));
}

/// The real jar holds both forms of a quiet pane's snapshot, at the offset
/// of the last byte the pane parsed: everything the jar has logged.
#[test]
fn a_quiet_pane_leaves_both_snapshots_in_its_jar() {
    let dir = short_dir();
    let place = place_in(dir.path(), built_jar());
    let pane = crate::pane::Pane::launch(
        crate::pane::Launch {
            command: "sh -c 'printf hello; exec cat'",
            rows: 24,
            cols: 80,
            cwd: dir.path(),
            context_path: &dir.path().join("context"),
            extra_env: &[],
            jar: Some(Spec {
                place: place.clone(),
                name: "0123456789abcdef".into(),
            }),
        },
        std::sync::Arc::new(|| {}),
    )
    .unwrap();
    assert!(pane.jarred());
    let socket = std::fs::read_dir(place.sockets())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let field = |fields: &[(String, String)], key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map_or_else(String::new, |(_, v)| v.clone())
    };
    let until = Instant::now() + WAIT;
    loop {
        let fields = spyc_jar::client::describe(&socket).unwrap();
        let printed = field(&fields, "log")
            .split(" printed")
            .next()
            .and_then(|s| s.rsplit(' ').next())
            .map(str::to_string)
            .unwrap();
        let want = format!("1@{printed} 2@{printed}");
        if field(&fields, "snapshots") == want {
            break;
        }
        assert!(
            Instant::now() < until,
            "snapshots {:?}, log {:?}",
            field(&fields, "snapshots"),
            field(&fields, "log")
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(pane);
}
