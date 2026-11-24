use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use spyc_jar::proto::{self, JarSignal, Msg, Role};

use super::*;
use crate::pane::pty_host::{PtyEvent, PtyHost};

const WAIT: Duration = Duration::from_secs(5);
/// A test's program is ended in order before the test does: a dropped
/// pane's kill may not reach its jar before the test process exits, and a
/// pane's jar waits for an owner for ever.
const END: Duration = Duration::from_millis(500);

/// A directory under `/tmp` whose jars end with it.
fn short_dir() -> test_dir::TestDir {
    test_dir::TestDir::new("jt")
}

/// Where `place` keeps the token of the jar at `socket`: by the socket's name.
fn token_of(place: &Place, socket: &Path) -> PathBuf {
    place.tokens.join(socket.file_stem().unwrap())
}

/// A place in `dir`, starting `binary`.
fn place_in(dir: &Path, binary: PathBuf) -> Place {
    Place {
        binary,
        dirs: vec![dir.join("spyc"), dir.join("spyc").join("jars")],
        tokens: dir.join("tokens"),
        hold: place::HOLD,
        repaint_hold: Duration::from_secs(2),
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
    _dir: test_dir::TestDir,
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
    let Msg::Welcome { child_pid, .. } = welcome else {
        panic!("no welcome: {welcome:?}");
    };
    let fresh = Resume {
        pid: child_pid,
        replay: 0,
        offsets: offsets::Offsets::new(0),
        gate: ReplayGate::open(),
    };
    let jarred = connect(client, fresh, &socket, (24, 80), Duration::from_secs(2)).unwrap();
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

/// The next resize the fake jar hears.
fn next_resize(jar: &FakeJar) -> (u16, u16) {
    loop {
        if let Msg::Resize { rows, cols, .. } = jar.next() {
            return (rows, cols);
        }
    }
}

/// R12c, finding 9: past a gap the program is given its size a row off,
/// held, then the pane's size: the pane's as it is when the hold ends, a
/// resize meanwhile included. Output without a gap moves nothing.
#[test]
fn a_gap_holds_the_size_a_row_off_then_gives_the_panes_back() {
    let (mut jar, mut host) = host_in_fake_jar(std::process::id());
    let output = |offset, bytes: &[u8]| Msg::Output {
        offset,
        bytes: bytes.to_vec(),
    };
    jar.send(&output(0, b"ab"));
    jar.send(&output(50, b"cd"));
    assert_eq!(next_resize(&jar), (23, 80), "a row off, at the gap");
    host.resize(30, 100).unwrap();
    assert_eq!(next_resize(&jar), (30, 100), "the pane's resize");
    assert_eq!(
        next_resize(&jar),
        (30, 100),
        "the hold ends at the pane's size"
    );
    jar.send(&output(52, b"ef"));
    jar.send(&output(90, b"gh"));
    assert_eq!(next_resize(&jar), (29, 100), "a later gap holds again");
    assert_eq!(next_resize(&jar), (30, 100));
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

/// A tab with jars on, its program a mark that it ran.
fn spawn_marking(place: &Place, mark: &Path) -> anyhow::Result<PtyHost> {
    spawn_in(
        place,
        &format!("touch {}", mark.display()),
        mark.parent().unwrap(),
    )
}

/// With jars on, a tab's program runs in a jar or not at all: a jar that
/// won't run (one that doesn't speak this wire version exits with 75) or
/// isn't there fails the tab, says why, at once, and the program never runs.
/// Either is no usable jar, which offers to run tabs without one.
#[test]
fn a_jar_that_wont_run_fails_the_tab_and_runs_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let dir = short_dir();
    let refusing = dir.path().join("old-spyc-jar");
    std::fs::write(&refusing, "#!/bin/sh\nexit 75\n").unwrap();
    std::fs::set_permissions(&refusing, std::fs::Permissions::from_mode(0o755)).unwrap();
    for (binary, why) in [
        (refusing, "doesn't speak wire version"),
        (dir.path().join("missing"), "didn't start"),
    ] {
        // A jar that ended is seen at once: with an hour's hold, a tab that
        // waited for it to answer would still be waiting.
        let mut place = place_in(dir.path(), binary.clone());
        place.hold = Duration::from_secs(3600);
        let mark = dir.path().join("ran");
        let (done, failed) = mpsc::channel();
        {
            let (place, mark) = (place.clone(), mark.clone());
            std::thread::spawn(move || {
                let _ = done.send(
                    spawn_marking(&place, &mark)
                        .err()
                        .map(|e| (format!("{e:#}"), no_usable_jar(&e).is_some())),
                );
            });
        }
        let (e, unusable) = failed
            .recv_timeout(WAIT)
            .unwrap_or_else(|_| {
                panic!(
                    "{}: the tab waited on a jar that had ended",
                    binary.display()
                )
            })
            .unwrap_or_else(|| panic!("{}: the tab opened without a jar", binary.display()));
        assert!(e.contains(why), "{}: {e}", binary.display());
        assert!(
            unusable,
            "{}: said as no usable jar, which offers jarless tabs: {e}",
            binary.display()
        );
        std::thread::sleep(Duration::from_millis(300));
        assert!(!mark.exists(), "{}: the program ran", binary.display());
        let left: Vec<_> = std::fs::read_dir(&place.tokens).unwrap().collect();
        assert!(left.is_empty(), "a token outlived its jar: {left:?}");
    }
}

/// Until a tab can open before its jar answers, one whose jar doesn't
/// answer in time fails, says so, and runs nothing.
#[test]
fn a_tab_whose_jar_doesnt_answer_fails_and_says_so() {
    use std::os::unix::fs::PermissionsExt;
    let dir = short_dir();
    let silent = dir.path().join("silent-jar");
    std::fs::write(&silent, "#!/bin/sh\nexec sleep 60\n").unwrap();
    std::fs::set_permissions(&silent, std::fs::Permissions::from_mode(0o755)).unwrap();
    let place = place_in(dir.path(), silent);
    let mark = dir.path().join("ran");
    let e = spawn_marking(&place, &mark).err().expect("the tab failed");
    assert!(format!("{e:#}").contains("didn't answer within"), "{e:#}");
    assert!(!mark.exists(), "the program ran");
}

/// The rule's one door: the only launch on a PTY of spyc's own is a tab's
/// with jars off, and it asks first the jar the start's record names
/// (SPYC-TRAP `jar-never-twice`). Fails closed.
#[test]
fn with_jars_on_no_path_launches_the_program_outside_its_jar() {
    let host = crate::guard_support::production_half(include_str!("../pty_host.rs"));
    let callers = |name: &str| -> Vec<&str> {
        host.lines()
            .filter(|l| l.contains(&format!("{name}(")) && !l.trim_start().starts_with("fn "))
            .collect()
    };
    let calls = callers("spawn_unless_held");
    assert_eq!(calls.len(), 1, "spawn_unless_held's callers: {calls:?}");
    assert!(
        calls[0].trim_start().starts_with("None => "),
        "a PTY of spyc's only for a tab with no jar asked for: {calls:?}"
    );
    let native = callers("spawn_native");
    assert_eq!(native.len(), 1, "spawn_native's callers: {native:?}");
    let gate = host
        .split("fn spawn_unless_held(")
        .nth(1)
        .and_then(|body| body.split("\nfn ").next())
        .expect("premise: spawn_unless_held is there");
    let asked = gate.find("super::jar::held(spec.record)");
    let started = gate.find("spawn_native(");
    assert!(
        asked.is_some() && started.is_some() && asked < started,
        "spawn_native only after the jar the record names was asked: {gate}"
    );
    let jar = crate::guard_support::production_half(include_str!("mod.rs"));
    assert!(
        jar.contains(") -> anyhow::Result<Jarred> {"),
        "spawn returns a jar or an error, never a request to run the program here"
    );
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
/// too new to judge (a jar binds a moment before it listens), a live one, one
/// whose lock can't be read, or anything else.
#[test]
fn a_sweep_removes_only_old_sockets_whose_lock_is_free() {
    let dir = short_dir();
    let place = place_in(dir.path(), PathBuf::from("/nonexistent"));
    place.prepare().unwrap();
    let live = place.socket("aaaa");
    let dead = place.socket("bbbb");
    let unknown = place.socket("dddd");
    let other = place.sockets().join("notes.txt");
    let _listening = UnixListener::bind(&live).unwrap();
    drop(UnixListener::bind(&dead).unwrap());
    drop(UnixListener::bind(&unknown).unwrap());
    // A lock file nothing holds: the jar that held it is gone.
    std::fs::write(spyc_jar::lock::path(&dead), "").unwrap();
    std::fs::write(&other, "x").unwrap();
    tokens::save(&place, &dead, b"t").unwrap();
    tokens::save(&place, &live, b"t").unwrap();
    tokens::save(&place, &unknown, b"t").unwrap();
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
    assert!(!spyc_jar::lock::path(&dead).exists(), "with its lock file");
    assert!(unknown.exists(), "one with no lock file to read is kept");
    assert!(token_of(&place, &unknown).exists(), "with its token");
    assert!(other.exists(), "only sockets are swept");
    assert!(!token_of(&place, &dead).exists(), "with its token");
    assert!(token_of(&place, &live).exists());
}

#[test]
fn only_a_current_or_rc_build_reads_the_setting() {
    assert!(honoured_by("2.5.0-CURRENT"));
    assert!(honoured_by("3.0.0-rc.1"));
    assert!(!honoured_by("2.5.0"));
    assert!(wanted(false, "0123", None).unwrap().is_none(), "off is off");
}

#[test]
fn jars_are_on_by_default_but_never_for_a_unit_test() {
    assert!(by_default(false), "a spyc runs tabs in jars unasked");
    assert!(!by_default(true), "a unit test's tabs start no jar");
    const { assert!(!BY_DEFAULT) };
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
    warmed(
        exe.ancestors()
            .skip(1)
            .map(|d| d.join("spyc-jar"))
            .find(|j| j.is_file())
            .unwrap_or_else(|| {
                panic!(
                    "no spyc-jar above {}: `cargo build --bin spyc-jar` (make test builds it)",
                    exe.display()
                )
            }),
    )
}

/// A tab's program on a PTY of the real jar's, as `PtyHost` spawns it.
fn spawn_in(place: &Place, command: &str, cwd: &Path) -> anyhow::Result<PtyHost> {
    spawn_named(place, "0123456789abcdef", command, cwd)
}

/// [`spawn_in`], for the tab `name`.
fn spawn_named(place: &Place, name: &str, command: &str, cwd: &Path) -> anyhow::Result<PtyHost> {
    PtyHost::spawn(crate::pane::pty_host::PtySpec {
        project_env: &[],
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
            name: name.into(),
            adopt: None,
            retake: false,
            exit_linger: None,
            watch: None,
        }),
        record: None,
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
    assert_eq!(recorded_exit(&mut host).exit_code(), 3);
    let until = Instant::now() + WAIT;
    while socket.exists() {
        assert!(
            Instant::now() < until,
            "the jar outlived its acknowledged exit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A jar's command line, which `ps` shows anyone for the jar's whole life,
/// is `spyc-jar`, the program's name and the wire version:
/// never the program's arguments or where it runs, since a tab's command can
/// carry a prompt, and spyc's wrapper around it names files gone once spyc
/// is. Both still reach the program.
#[test]
fn a_jars_command_line_names_its_program_and_nothing_else() {
    let dir = short_dir();
    let place = place_in(dir.path(), built_jar());
    let cwd = dir.path().join("where-the-program-runs");
    std::fs::create_dir(&cwd).unwrap();
    let host = spawn_in(
        &place,
        "sh -c 'printf \"[%s]\" \"$PWD\"; exec cat' # a-prompt-of-the-users",
        &cwd,
    )
    .unwrap();
    read_until(&host, "where-the-program-runs]");
    let socket = place.socket("0123456789abcdef");
    let pid = describe_field(&socket, "pid");
    let out = std::process::Command::new("ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "spyc-jar sh --wire 1"
    );
}

/// A jar's name is the program a tab runs, past `exec`, `env` and variables
/// set for it, without its directory; a tab with no command runs the shell.
#[test]
fn a_jar_is_named_for_the_program_its_tab_runs() {
    for (command, name) in [
        ("claude --resume 0f3c 'fix the bug'", "spyc-jar claude"),
        ("exec env CODEX_HOME=/x /opt/bin/codex", "spyc-jar codex"),
        ("", "spyc-jar zsh"),
        ("FOO=1", "spyc-jar zsh"),
    ] {
        assert_eq!(jar_name(command, "/bin/zsh"), name, "{command:?}");
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
    let mut restarted = spawn_in(&place, "exec cat", dir.path()).unwrap();
    assert!(restarted.jarred(), "a restart gets a jar");
    restarted.shutdown(END);
}

/// How long an ended program's jar takes to go, at most, here: its exit
/// acked, then a debug build's end held for a socket check, every 2 s.
const JAR_GOES: Duration = Duration::from_secs(30);

/// How many ways `src` names to end a jar or take its address away: a path
/// that names `exit`, `_exit` or `abort` (`std::process`'s, `libc`'s),
/// `remove_file`, `lock::forget` or `lock::path`, called or bound to a name
/// (`let quit = std::process::exit`), and a `use` that brings one in, renamed
/// or by a glob of `process`, `lock` or `libc`. Parsed, test items left out.
/// Not seen: a path built inside a macro's tokens, a name brought in by a
/// `use` in another file and re-exported, and an end outside the files read.
fn jar_enders(src: &str) -> usize {
    use syn::visit::Visit;
    fn ends(names: &[String]) -> bool {
        match names {
            [.., m, star] if star == "*" => matches!(m.as_str(), "process" | "lock" | "libc"),
            [_, .., last] if matches!(last.as_str(), "exit" | "_exit" | "abort") => true,
            [.., last] if last == "remove_file" => true,
            [.., m, f] => m == "lock" && matches!(f.as_str(), "forget" | "path"),
            _ => false,
        }
    }
    fn test_only(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|a| {
            a.path().is_ident("cfg") && a.parse_args::<syn::Ident>().is_ok_and(|i| i == "test")
        })
    }
    /// Each path a `use` brings in, the name it is brought in by left out.
    fn flatten(tree: &syn::UseTree, at: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        let one = |at: &[String], last: String| [at, &[last]].concat();
        match tree {
            syn::UseTree::Path(p) => {
                at.push(p.ident.to_string());
                flatten(&p.tree, at, out);
                at.pop();
            }
            syn::UseTree::Name(n) => out.push(one(at, n.ident.to_string())),
            syn::UseTree::Rename(r) => out.push(one(at, r.ident.to_string())),
            syn::UseTree::Glob(_) => out.push(one(at, "*".into())),
            syn::UseTree::Group(g) => g.items.iter().for_each(|t| flatten(t, at, out)),
        }
    }
    struct Enders(usize);
    impl<'a> Visit<'a> for Enders {
        fn visit_item(&mut self, item: &'a syn::Item) {
            let attrs = match item {
                syn::Item::Fn(i) => &i.attrs,
                syn::Item::Mod(i) => &i.attrs,
                syn::Item::Impl(i) => &i.attrs,
                syn::Item::Use(i) => &i.attrs,
                syn::Item::Const(i) => &i.attrs,
                syn::Item::Static(i) => &i.attrs,
                _ => return syn::visit::visit_item(self, item),
            };
            if !test_only(attrs) {
                syn::visit::visit_item(self, item);
            }
        }
        fn visit_impl_item_fn(&mut self, f: &'a syn::ImplItemFn) {
            if !test_only(&f.attrs) {
                syn::visit::visit_impl_item_fn(self, f);
            }
        }
        fn visit_item_use(&mut self, u: &'a syn::ItemUse) {
            let mut out = Vec::new();
            flatten(&u.tree, &mut Vec::new(), &mut out);
            self.0 += out.iter().filter(|p| ends(p)).count();
        }
        fn visit_path(&mut self, path: &'a syn::Path) {
            let names: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
            if ends(&names) {
                self.0 += 1;
            }
            syn::visit::visit_path(self, path);
        }
    }
    let file = syn::parse_file(src).unwrap();
    let mut enders = Enders(0);
    enders.visit_file(&file);
    enders.0
}

/// The guard's reading: each way to end a jar counts, renamed or bound.
#[test]
fn every_way_to_end_a_jar_is_counted() {
    for (src, n) in [
        ("fn f() { std::process::exit(0); }", 1),
        ("fn f() { let quit = std::process::exit; quit(0); }", 1),
        ("use std::process::exit; fn f() { exit(0); }", 1),
        ("use std::process::exit as quit; fn f() { quit(0); }", 1),
        (
            "use std::{fs, process::{self, exit}}; fn f() { exit(0); }",
            1,
        ),
        ("use std::process::*; fn f() { exit(0); }", 1),
        (
            "fn f() { let _ = std::fs::remove_file(crate::lock::path(s)); }",
            2,
        ),
        (
            "fn f() { crate::lock::forget(s); unsafe { libc::_exit(0) } }",
            2,
        ),
        ("fn f() { std::process::abort(); }", 1),
        (
            "fn f(st: &S) { if st.exit.is_some() { let exit = 1; drop(exit); } std::mem::forget(x); }",
            0,
        ),
        (
            "#[cfg(test)] mod tests { fn f() { std::process::exit(0); } }",
            0,
        ),
    ] {
        assert_eq!(jar_enders(src), n, "{src}");
    }
}

/// SPYC-TRAP `jar-end-holds-its-address`: a jar ends only through
/// `jar/end.rs`, which holds the jar's address from its socket's going to
/// the process's exit. Nothing else under `crates/spyc-jar/src/jar/` names a
/// way to end a jar or remove its address ([`jar_enders`], which says what it
/// can't see); the upgrade containment (`held.rs`, outside it) runs no socket
/// check. Fails closed: `end.rs` must name its three.
#[test]
fn a_jar_ends_only_through_its_end() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/spyc-jar/src/jar");
    let mut found = std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        let n = jar_enders(&std::fs::read_to_string(&path).unwrap());
        if n > 0 {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            found.insert(name, n);
        }
    }
    assert_eq!(
        found,
        [("end.rs".to_string(), 3)].into(),
        "a jar that ends without holding its address can leave its socket behind"
    );
}

/// An ending jar leaves nothing at its address, whenever its socket check
/// runs. That check binds the socket again when a cleaner has removed it,
/// and it can run while the jar ends, after its socket has gone and before
/// its lock file goes. Then a socket stays with no lock file and nothing
/// listening, which no spyc can tell from a running jar's: the tab's restart
/// is refused, and a sweep never removes it. A debug build's
/// `SPYC_JAR_END_GAP` holds the jar's end there until a socket check has run
/// (the flake in `a_tab_whose_jar_still_runs_never_starts_its_program_twice`).
#[test]
fn an_ending_jar_leaves_no_socket_for_its_tab_to_be_refused_on() {
    let dir = short_dir();
    let held = dir.path().join("held-jar");
    std::fs::write(
        &held,
        format!(
            "#!/bin/sh\nSPYC_JAR_END_GAP=1 exec '{}' \"$@\"\n",
            built_jar().display()
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&held, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let place = place_in(dir.path(), held);
    let socket = place.socket("0123456789abcdef");
    // The program prints this itself, once it runs in its own process group
    // (a tty echoes what's typed before then), so the kill below reaches it.
    let mut first = spawn_in(&place, "printf started; exec cat", dir.path()).unwrap();
    assert!(first.jarred());
    read_until(&first, "started");

    // As a tab's restart does: the kill, and the jar ends.
    first.child.kill().unwrap();
    let until = Instant::now() + JAR_GOES;
    while socket_state(&socket) == SocketState::Live {
        assert!(Instant::now() < until, "never: the jar went");
        std::thread::sleep(Duration::from_millis(20));
    }
    let left = socket_state(&socket);
    place::sweep(&place, Duration::ZERO);
    let restarted = spawn_in(&place, "exec cat", dir.path());
    let refused = restarted.as_ref().err().map(|e| format!("{e:#}"));
    if let Ok(mut restarted) = restarted {
        restarted.shutdown(END);
    }
    assert_eq!(
        refused,
        None,
        "the tab's restart was refused, after a sweep too: the jar left {left:?} at {}",
        socket.display()
    );
    assert_eq!(left, SocketState::Missing, "the jar left its socket");
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
    // A snapshot goes out on the connection's one queue as the tick takes
    // it: what's typed after the tick comes after any snapshot it took.
    let mut host = host;
    host.write_all(b"after the tick").unwrap();
    let next = jar.next();
    assert!(
        matches!(next, Msg::Input { .. }),
        "a second snapshot of nothing new: {next:?}"
    );
}

/// A jar that never reports the exit can't hold spyc's shutdown.
#[test]
fn a_jar_that_never_reports_an_exit_cannot_hold_a_shutdown() {
    let (jar, mut host) = host_in_fake_jar(std::process::id());
    let (done, shut) = mpsc::channel();
    std::thread::spawn(move || {
        host.shutdown(Duration::from_millis(100));
        let _ = done.send(());
    });
    assert!(
        shut.recv_timeout(WAIT).is_ok(),
        "the shutdown waited on an exit the jar never reports"
    );
    drop(jar);
}

/// The exit `host`'s jar connection recorded, however long a loaded machine
/// takes to record it. Never `wait()`, whose bound (`EXIT_WAIT`) is how long
/// a quit waits for an exit, not how long one takes to arrive.
fn recorded_exit(host: &mut PtyHost) -> portable_pty::ExitStatus {
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = host.child.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < until, "the connection recorded no exit");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Decision G's labels: a connection the jar ended while it still runs
/// leaves the tab `[detached]`; one whose jar is gone, `[jar lost]`.
#[test]
fn a_lost_connection_says_whether_the_jar_still_runs() {
    let (jar, mut host) = host_in_fake_jar(std::process::id());
    jar.to_owner.shutdown(std::net::Shutdown::Both).unwrap();
    let status = recorded_exit(&mut host);
    assert_eq!(status.signal(), Some(Lost::Held.label()));

    let (mut jar, mut host) = host_in_fake_jar(std::process::id());
    let socket = jar.socket.clone();
    drop(jar.listener.take());
    std::fs::remove_file(&socket).unwrap();
    jar.to_owner.shutdown(std::net::Shutdown::Both).unwrap();
    let status = recorded_exit(&mut host);
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
            project_env: &[],
            ssh_agent: None,
            command: "sh -c 'printf hello; exec cat'",
            rows: 24,
            cols: 80,
            cwd: dir.path(),
            context_path: &dir.path().join("context"),
            extra_env: &[],
            jar: Some(Spec {
                place: place.clone(),
                name: "0123456789abcdef".into(),
                adopt: None,
                retake: false,
                exit_linger: None,
                watch: None,
            }),
            record: None,
        },
        std::sync::Arc::new(|| {}),
    )
    .unwrap();
    assert!(pane.jarred());
    let socket = std::fs::read_dir(place.sockets())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "sock"))
        .unwrap();
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
    let mut pane = pane;
    pane.shutdown_reporting(END, || {});
}

/// R17b: the newest snapshot this build can use, binary before portable at
/// one offset; a binary one it can't decode, or one past the jar's head,
/// never; past a gap (R12c), the newest anyway.
#[test]
fn an_adoption_starts_from_the_newest_snapshot_this_build_can_use() {
    let snap = |format, offset, blob: &[u8]| Snapshot {
        format,
        offset,
        rows: 24,
        cols: 80,
        blob: blob.to_vec(),
    };
    let ours = |b: &[u8]| b == b"ours";
    let pick = |all: &[Snapshot], head| choose(all, head, ours).map(|s| (s.format, s.offset));
    let both = [
        snap(proto::SNAPSHOT_VT, 100, b"vt"),
        snap(proto::SNAPSHOT_BINARY, 100, b"ours"),
    ];
    assert_eq!(pick(&both, 200), Some((proto::SNAPSHOT_BINARY, 100)));
    let foreign = [
        snap(proto::SNAPSHOT_VT, 100, b"vt"),
        snap(proto::SNAPSHOT_BINARY, 100, b"another build's"),
    ];
    assert_eq!(pick(&foreign, 200), Some((proto::SNAPSHOT_VT, 100)));
    let newer_vt = [
        snap(proto::SNAPSHOT_BINARY, 100, b"ours"),
        snap(proto::SNAPSHOT_VT, 150, b"vt"),
    ];
    assert_eq!(pick(&newer_vt, 200), Some((proto::SNAPSHOT_VT, 150)));
    assert_eq!(
        pick(&newer_vt, 120),
        Some((proto::SNAPSHOT_BINARY, 100)),
        "past the head"
    );
    assert_eq!(pick(&[], 200), None);
}

/// R15a: while a replayed tail is in the parser, its queries go unanswered;
/// a grace after it's handed over, answers flow again.
#[test]
fn replies_wait_until_the_replayed_tail_is_parsed() {
    let gate = link::ReplayGate::until(100);
    assert!(!gate.is_open());
    gate.handed_over(50);
    std::thread::sleep(Duration::from_millis(600));
    assert!(!gate.is_open(), "half the tail");
    gate.handed_over(100);
    assert!(!gate.is_open(), "the parser may still hold its queries");
    std::thread::sleep(Duration::from_millis(600));
    assert!(gate.is_open());
    assert!(link::ReplayGate::open().is_open(), "nothing to replay");
}

const TAB: &str = "fedcba9876543210";

/// A pane tab in a real jar, taking back the jar at `adopt` when it runs.
fn pane_in(place: &Place, command: &str, cwd: &Path, adopt: Option<PathBuf>) -> crate::pane::Pane {
    crate::pane::Pane::launch(
        crate::pane::Launch {
            project_env: &[],
            ssh_agent: None,
            command,
            rows: 24,
            cols: 80,
            cwd,
            context_path: &cwd.join("context"),
            extra_env: &[],
            jar: Some(Spec {
                place: place.clone(),
                name: TAB.into(),
                adopt,
                retake: false,
                exit_linger: None,
                watch: None,
            }),
            record: None,
        },
        std::sync::Arc::new(|| {}),
    )
    .unwrap()
}

fn screen(pane: &crate::pane::Pane) -> String {
    pane.with_screen(crate::pane::engine::TerminalScreen::contents)
}

/// Wait until the pane's screen holds `needle`.
fn shows(pane: &crate::pane::Pane, needle: &str) {
    let until = Instant::now() + WAIT;
    while !screen(pane).contains(needle) {
        assert!(
            Instant::now() < until,
            "{needle:?} never showed: {:?}",
            screen(pane)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Wait until the jar at `socket` holds both snapshot forms at its head.
fn snapshotted(socket: &Path) {
    let until = Instant::now() + WAIT;
    loop {
        let fields = spyc_jar::client::describe(socket).unwrap();
        let get = |k: &str| {
            fields
                .iter()
                .find(|(key, _)| key == k)
                .map_or_else(String::new, |(_, v)| v.clone())
        };
        let printed = get("log")
            .split(" printed")
            .next()
            .and_then(|s| s.rsplit(' ').next())
            .map(str::to_string)
            .unwrap();
        if get("snapshots") == format!("1@{printed} 2@{printed}") {
            return;
        }
        assert!(Instant::now() < until, "snapshots {:?}", get("snapshots"));
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A restored tab whose jar still runs takes it back: the same program,
/// its screen from the snapshot and the replay, nothing in it twice, and
/// what's typed reaching it.
#[test]
fn a_restored_tab_takes_its_running_jar_back() {
    let dir = short_dir();
    let place = place_in(dir.path(), built_jar());
    let socket = place.socket(TAB);
    let first = pane_in(
        &place,
        "sh -c 'printf before; read x; printf \"got-%s\" \"$x\"; exec cat'",
        dir.path(),
        None,
    );
    shows(&first, "before");
    snapshotted(&socket);
    let mut first = first;
    first.send_bytes(b"zz\n").unwrap();
    shows(&first, "got-zz");
    let pid = first.process_id();
    first.host.sever_jar();
    drop(first);

    let second = pane_in(&place, "echo started-again", dir.path(), Some(socket));
    assert!(second.adopted(), "the running jar was taken back");
    assert_eq!(second.process_id(), pid, "the same program");
    shows(&second, "got-zz");
    let text = screen(&second);
    assert_eq!(
        text.matches("before").count(),
        1,
        "replayed twice: {text:?}"
    );
    assert_eq!(
        text.matches("got-zz").count(),
        1,
        "replayed twice: {text:?}"
    );
    assert!(!text.contains("started-again"), "{text:?}");
    let mut second = second;
    second.send_bytes(b"typed\n").unwrap();
    shows(&second, "typed");
    second.shutdown_reporting(END, || {});
}

/// How many bytes the jar at `socket` has printed, and the first it still
/// holds.
fn log_printed_and_base(socket: &Path) -> (u64, u64) {
    let log = spyc_jar::client::describe(socket)
        .unwrap()
        .into_iter()
        .find(|(k, _)| k == "log")
        .map(|(_, v)| v)
        .unwrap();
    let number = |after: &str| -> u64 {
        log.split(after)
            .nth(1)
            .unwrap()
            .split([' ', ','])
            .next()
            .unwrap()
            .parse()
            .unwrap()
    };
    let printed = log
        .split(" printed")
        .next()
        .unwrap()
        .rsplit(' ')
        .next()
        .unwrap();
    (printed.parse().unwrap(), number("from "))
}

/// R12c, finding 9: a tab taken back across a gap, whose jar dropped bytes
/// while no spyc owned it, has its program draw its screen again: the
/// program is given its size a row off, then its own size back.
#[test]
fn a_tab_taken_back_across_a_gap_has_its_program_repaint() {
    let dir = short_dir();
    let place = place_in(dir.path(), built_jar());
    let socket = place.socket(TAB);
    let go = dir.path().join("go");
    let program = dir.path().join("program.sh");
    std::fs::write(
        &program,
        format!(
            "trap 'stty size' WINCH\n\
             printf 'ready\\n'\n\
             while [ ! -e {go} ]; do sleep 0.05; done\n\
             head -c 6000000 /dev/zero | tr '\\0' x\n\
             printf '\\nflooded\\n'\n\
             while :; do sleep 0.05; done\n",
            go = go.display()
        ),
    )
    .unwrap();
    let command = format!("sh {}", program.display());
    let first = pane_in(&place, &command, dir.path(), None);
    shows(&first, "ready");
    first.host.sever_jar();
    drop(first);
    std::fs::write(&go, "").unwrap();
    // Taken back once the flood is printed: a shell runs its trap only
    // between commands.
    // The wait bounds each stretch with no growth, never the whole flood: a
    // loaded machine slows it, and only a program that stops short fails.
    let mut printed = log_printed_and_base(&socket).0;
    let mut grew = Instant::now();
    while printed < 6_000_000 {
        assert!(
            Instant::now() < grew + WAIT,
            "premise: the program floods its jar (stopped at {printed})"
        );
        std::thread::sleep(Duration::from_millis(50));
        let now = log_printed_and_base(&socket).0;
        if now != printed {
            grew = Instant::now();
            printed = now;
        }
    }
    assert!(
        log_printed_and_base(&socket).1 > 0,
        "premise: the jar dropped bytes"
    );

    let mut second = pane_in(&place, "echo started-again", dir.path(), Some(socket));
    assert!(second.adopted(), "premise: the running jar was taken back");
    shows(&second, "flooded");
    shows(&second, "23 80");
    shows(&second, "24 80");
    let text = screen(&second);
    assert!(
        text.find("23 80") < text.find("24 80"),
        "a row off first, then its own size: {text:?}"
    );
    second.shutdown_reporting(END, || {});
}

/// Finding 7: a jar another spyc still owns, or one this spyc has no token
/// for, isn't taken, and nothing is started in its place: the tab is
/// detached.
#[test]
fn a_restored_tab_whose_jar_wont_take_it_is_detached_and_starts_nothing() {
    let dir = short_dir();
    let place = place_in(dir.path(), built_jar());
    let socket = place.socket(TAB);
    let mark = dir.path().join("ran");
    let owner = pane_in(&place, "cat", dir.path(), None);
    let command = format!("touch {}", mark.display());

    let held = pane_in(&place, &command, dir.path(), Some(socket.clone()));
    assert!(held.detached(), "another spyc owns it");
    assert!(!held.adopted());
    assert_eq!(
        held.jar_socket(),
        Some(socket.as_path()),
        "kept for a later restore"
    );

    owner.host.sever_jar();
    drop(owner);
    std::fs::remove_file(token_of(&place, &socket)).unwrap();
    let tokenless = pane_in(&place, &command, dir.path(), Some(socket.clone()));
    assert!(tokenless.detached(), "no token for it");

    std::thread::sleep(Duration::from_millis(300));
    assert!(!mark.exists(), "the program was started twice");
    assert_eq!(socket_state(&socket), SocketState::Live, "the jar runs on");
    let mut held = held;
    let until = Instant::now() + WAIT;
    while !held.is_closed() {
        held.drain_output();
        assert!(Instant::now() < until, "a detached tab never closed");
        std::thread::sleep(Duration::from_millis(20));
    }
    let status = held.exit_status().map(portable_pty::ExitStatus::signal);
    assert_eq!(status, Some(Some(Lost::Held.label())));
    // Nothing here owns the jar: end it and its program by hand.
    for key in ["program_pid", "pid"] {
        let _ = rustix::process::kill_process_group(
            rustix::process::Pid::from_raw(describe_field(&socket, key)).unwrap(),
            rustix::process::Signal::KILL,
        );
    }
}

fn describe_field(socket: &Path, key: &str) -> i32 {
    spyc_jar::client::describe(socket)
        .unwrap()
        .into_iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.parse().ok())
        .unwrap()
}

/// A restored tab whose jar is gone starts its program again, in a jar.
#[test]
fn a_restored_tab_whose_jar_is_gone_starts_again() {
    let dir = short_dir();
    let place = place_in(dir.path(), built_jar());
    let gone = place.sockets().join("gone.sock");
    let pane = pane_in(
        &place,
        "sh -c 'printf fresh; exec cat'",
        dir.path(),
        Some(gone),
    );
    assert!(pane.jarred() && !pane.adopted() && !pane.detached());
    shows(&pane, "fresh");
    let mut pane = pane;
    pane.shutdown_reporting(END, || {});
}

/// R15a end to end: a query the program made before the crash is replayed
/// into the new pane's engine and goes unanswered. `cat -v` shows every
/// answer it reads on the line `done` ends: one answer, the first spyc's.
#[test]
fn a_replayed_query_is_not_answered_again() {
    let dir = short_dir();
    let place = place_in(dir.path(), built_jar());
    let socket = place.socket(TAB);
    let first = pane_in(
        &place,
        "sh -c 'printf \"\\033[c\"; exec cat -v'",
        dir.path(),
        None,
    );
    shows(&first, "?62");
    first.host.sever_jar();
    drop(first);

    let second = pane_in(&place, "exec cat", dir.path(), Some(socket));
    assert!(second.adopted());
    // Typed once the replay shows the first answer's echo, which the log
    // holds after the query: the engine has parsed the query by then, so an
    // answer to it is already queued for the program, ahead of what's typed.
    shows(&second, "?62");
    let mut second = second;
    second.send_bytes(b"done\n").unwrap();
    shows(&second, "cdone");
    let text = screen(&second);
    let line = text.lines().find(|l| l.ends_with("cdone")).unwrap();
    assert_eq!(
        line.matches("?62").count(),
        1,
        "a replayed query was answered again: {line:?}"
    );
    second.shutdown_reporting(END, || {});
}

/// The SHA-256 of `file`, in hex.
fn sha256_of(file: &Path) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    sha2::Sha256::digest(std::fs::read(file).unwrap())
        .iter()
        .fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// The build the built `spyc-jar` says it is. A test built apart from it
/// (`cargo test --lib`, in a worktree, where `build.rs` runs every time) has
/// another build time, as a spyc would whose `spyc-jar` an upgrade replaced.
fn built_jars_build() -> store::Build<'static> {
    let out = std::process::Command::new(built_jar())
        .arg("build-info")
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let mut lines = text
        .lines()
        .map(|l| &*Box::leak(l.to_string().into_boxed_str()));
    store::Build {
        commit: lines.next().unwrap(),
        built: lines.next().unwrap(),
    }
}

fn inode(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).unwrap().ino()
}

/// Decision L: this build's `spyc-jar` goes into the store under its version
/// and the first 12 hex digits of its SHA-256,
/// written whole, `0555`, in a marked private directory a jar knows as a
/// store; a second start uses that entry again, never writing it twice.
#[test]
fn the_store_holds_this_build_under_its_hash() {
    use std::os::unix::fs::PermissionsExt;
    let dir = short_dir();
    let root = dir.path().join("jar-bin");
    let source = built_jar();
    let got = store::install(&root, &source, built_jars_build());
    let store::Binary::Entry(entry) = got else {
        panic!("this build's spyc-jar wasn't taken: {got:?}");
    };
    let name = format!(
        "{}-{}",
        spyc_jar::PACKAGE_VERSION,
        &sha256_of(&source)[..12]
    );
    assert_eq!(entry, root.join(name).join("spyc-jar"));
    assert_eq!(sha256_of(&entry), sha256_of(&source));
    let mode = std::fs::metadata(&entry).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o555);
    assert_eq!(
        spyc_jar::store::store_of(&entry),
        Some(root.clone()),
        "a jar knows its store"
    );
    let first = inode(&entry);
    assert_eq!(
        store::install(&root, &source, built_jars_build()),
        store::Binary::Entry(entry.clone())
    );
    assert_eq!(inode(&entry), first, "the entry was written again");
    let strays: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".incoming"))
        .collect();
    assert!(strays.is_empty(), "{strays:?}");
}

/// A `spyc-jar` that isn't this build's (a package upgrade landed while spyc
/// started) starts no jars this session.
#[test]
fn another_builds_spyc_jar_starts_no_jars() {
    let dir = short_dir();
    let other = store::Build {
        commit: "0000000",
        built: "another time",
    };
    let got = store::install(&dir.path().join("jar-bin"), &built_jar(), other);
    assert!(matches!(got, store::Binary::None(_)), "{got:?}");
}

/// A store that can't take a copy (its directory can't be written, or is
/// mounted `noexec`) leaves jars to start from the `spyc-jar` beside spyc;
/// an entry that doesn't hash to its name isn't used either.
#[test]
fn a_store_copy_that_cant_be_used_falls_back_to_the_binary_beside_spyc() {
    use std::os::unix::fs::PermissionsExt;
    let dir = short_dir();
    let unwritable = dir.path().join("not-a-directory");
    std::fs::write(&unwritable, "").unwrap();
    let got = store::install(&unwritable, &built_jar(), built_jars_build());
    assert!(
        matches!(&got, store::Binary::Beside(at, _) if *at == built_jar()),
        "{got:?}"
    );

    let root = dir.path().join("jar-bin");

    let source = built_jar();
    let store::Binary::Entry(entry) = store::install(&root, &source, built_jars_build()) else {
        panic!("premise: the entry is made");
    };
    // Changed under its name into something that answers as this build
    // would: only its hash gives it away.
    let build = built_jars_build();
    std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        &entry,
        format!(
            "#!/bin/sh\necho '{}'\necho '{}'\n",
            build.commit, build.built
        ),
    )
    .unwrap();
    let got = store::install(&root, &source, built_jars_build());
    assert!(matches!(got, store::Binary::Beside(..)), "{got:?}");
}

/// macOS scans a new executable before its first run and scans one at a
/// time, so a fresh store entry's first `build-info` waits behind every new
/// executable started meanwhile: 72 copies of `spyc-jar` run together took up
/// to 15.8 s here. A `spyc-jar` that answers that late is still this
/// session's jar, never the "no usable spyc-jar" a broken install gets.
#[test]
fn a_spyc_jar_slow_to_answer_its_first_run_is_still_this_sessions_jar() {
    use std::os::unix::fs::PermissionsExt;
    let dir = short_dir();
    let build = store::Build {
        commit: "c0ffee0",
        built: "a slow first run",
    };
    let slow = dir.path().join("spyc-jar");
    std::fs::write(
        &slow,
        format!(
            "#!/bin/sh\nsleep 15\necho '{}'\necho '{}'\n",
            build.commit, build.built
        ),
    )
    .unwrap();
    std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
    let got = store::install(&dir.path().join("jar-bin"), &slow, build);
    assert!(matches!(got, store::Binary::Entry(_)), "{got:?}");
}

/// No usable jar: a `spyc-jar` missing from beside spyc, or one there that
/// can't run, is no jar this session, never one every tab then fails to
/// start.
#[test]
fn a_spyc_jar_missing_or_unable_to_run_is_no_usable_jar() {
    let dir = short_dir();
    let root = dir.path().join("jar-bin");
    let garbage = dir.path().join("spyc-jar");
    std::fs::write(&garbage, b"\x7fELF not really").unwrap();
    for source in [dir.path().join("missing").join("spyc-jar"), garbage] {
        let got = store::install(&root, &source, built_jars_build());
        assert!(
            matches!(&got, store::Binary::None(why) if why.contains(&*source.to_string_lossy())),
            "{}: {got:?}",
            source.display()
        );
    }
}

/// Set `dir`'s modification time two days back.
fn age(dir: &Path) {
    use rustix::fs::{AtFlags, CWD, Timespec, Timestamps};
    let then = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 2 * 24 * 3600;
    let t = Timespec {
        tv_sec: i64::try_from(then).unwrap(),
        tv_nsec: 0,
    };
    rustix::fs::utimensat(
        CWD,
        dir,
        &Timestamps {
            last_access: t,
            last_modification: t,
        },
        AtFlags::empty(),
    )
    .unwrap();
}

/// The sweep removes an old entry no live jar reports, and keeps one a live
/// jar runs from, spyc's own, and a new one; when a live jar won't say what
/// it runs, it removes nothing.
#[test]
fn the_store_sweep_removes_only_old_entries_nothing_runs() {
    let dir = short_dir();
    let root = dir.path().join("jar-bin");
    let store::Binary::Entry(running) = store::install(&root, &built_jar(), built_jars_build())
    else {
        panic!("premise: the entry is made");
    };
    let mut place = place_in(dir.path(), running.clone());
    place.binary = running.clone();
    let mut host = spawn_in(&place, "exec cat", dir.path()).unwrap();
    let entry = |name: &str| {
        let d = root.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("spyc-jar"), b"x").unwrap();
        d
    };
    let ours = entry("ours");
    let stale = entry("stale");
    let fresh = entry("fresh");
    for d in [running.parent().unwrap(), &ours, &stale] {
        age(d);
    }
    let aged = running
        .parent()
        .unwrap()
        .symlink_metadata()
        .unwrap()
        .modified()
        .unwrap();

    // A jar that answers a connection but never describes itself.
    let mute = std::os::unix::net::UnixListener::bind(place.sockets().join("mute.sock")).unwrap();
    assert!(
        store::sweep(
            &root,
            place.sockets(),
            &ours.join("spyc-jar"),
            store::UNTOUCHED
        )
        .is_empty(),
        "a sweep went ahead with a jar it couldn't ask"
    );
    drop(mute);
    let _ = std::fs::remove_file(place.sockets().join("mute.sock"));

    let removed = store::sweep(
        &root,
        place.sockets(),
        &ours.join("spyc-jar"),
        store::UNTOUCHED,
    );
    assert_eq!(removed, std::slice::from_ref(&stale));
    assert!(running.exists(), "the entry a live jar runs from");
    assert!(ours.exists() && fresh.exists());
    let touched = running
        .parent()
        .unwrap()
        .symlink_metadata()
        .unwrap()
        .modified()
        .unwrap();
    assert!(touched > aged, "a reported entry is touched");
    host.shutdown(END);
}

/// Decision M: a pane's jar holds its program until spyc, its first owner,
/// connects, so the program's start-up probes are answered by an owner, and
/// for as long as spyc waits for it to answer. Read from what spyc hands the
/// real jar, through a wrapper that keeps a copy.
#[test]
fn a_panes_jar_holds_its_program_for_its_first_owner() {
    use std::os::unix::fs::PermissionsExt;
    let dir = short_dir();
    let (copy, wrapper) = (dir.path().join("start"), dir.path().join("jar"));
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\ntee {} | exec {} \"$@\"\n",
            copy.display(),
            built_jar().display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let place = place_in(dir.path(), wrapper);
    let host = spawn_in(&place, "printf up; exec cat", dir.path()).unwrap();
    read_until(&host, "up");
    let sent = spyc_jar::cli::Start::parse(&std::fs::read(&copy).unwrap()).unwrap();
    assert_eq!(
        sent.hold,
        Some(place::HOLD),
        "the jar holds the program for as long as spyc waits"
    );
    assert_eq!((sent.rows, sent.cols), (24, 80), "at the tab's size");
}

// The ownership timers on spyc's side, each asked what a starved but live
// jar sees when it fires: a jar that is slow, parked or not accepting is
// never taken for gone, and its tab neither fails to open nor detaches.

/// A listener whose backlog is full, as a jar's is when it isn't accepting
/// (parked for an upgrade, stopped or starved) and connections queue.
fn not_accepting(path: &Path) -> (rustix::fd::OwnedFd, Vec<rustix::fd::OwnedFd>) {
    use rustix::net::{AddressFamily, SocketAddrUnix, SocketType};
    let addr = SocketAddrUnix::new(path).unwrap();
    let listener = rustix::net::socket(AddressFamily::UNIX, SocketType::STREAM, None).unwrap();
    rustix::net::bind(&listener, &addr).unwrap();
    rustix::net::listen(&listener, 1).unwrap();
    let mut queued = Vec::new();
    for _ in 0..64 {
        let c = rustix::net::socket(AddressFamily::UNIX, SocketType::STREAM, None).unwrap();
        rustix::io::ioctl_fionbio(&c, true).unwrap();
        if rustix::net::connect(&c, &addr).is_err() {
            break;
        }
        queued.push(c);
    }
    assert!(queued.len() < 64, "premise: the backlog fills");
    (listener, queued)
}

/// `socket_state` and a sweep, from another thread, answered within `WAIT`
/// or `None`: a probe that blocks is a failure too.
fn judged(place: &Place, socket: &Path) -> Option<(SocketState, usize)> {
    let (tx, rx) = mpsc::channel();
    let place = place.clone();
    let socket = socket.to_path_buf();
    std::thread::spawn(move || {
        let state = socket_state(&socket);
        let _ = tx.send((state, place::sweep(&place, Duration::ZERO)));
    });
    rx.recv_timeout(WAIT).ok()
}

/// R32b, finding 4: a socket counts as gone only when its jar's lock is
/// free. A real jar that isn't accepting (stopped, its backlog full, so
/// macOS refuses a connection) holds its lock: live, and never swept.
#[test]
fn a_jar_holding_its_lock_is_never_judged_gone_however_busy() {
    use rustix::process::{Pid, Signal, kill_process};
    let dir = short_dir();
    let place = place_in(dir.path(), built_jar());
    let mut host = spawn_in(&place, "exec sleep 100", dir.path()).unwrap();
    let socket = place.socket("0123456789abcdef");
    let jar = describe_field(&socket, "pid");
    let _ends = Ends(vec![jar]);
    kill_process(Pid::from_raw(jar).unwrap(), Signal::STOP).unwrap();
    let mut queued = Vec::new();
    for _ in 0..200 {
        let c = rustix::net::socket(
            rustix::net::AddressFamily::UNIX,
            rustix::net::SocketType::STREAM,
            None,
        )
        .unwrap();
        rustix::io::ioctl_fionbio(&c, true).unwrap();
        let addr = rustix::net::SocketAddrUnix::new(&socket).unwrap();
        if rustix::net::connect(&c, &addr).is_err() {
            break;
        }
        queued.push(c);
    }
    assert_eq!(
        judged(&place, &socket),
        Some((SocketState::Live, 0)),
        "a stopped jar, {} connections queued, holds its lock",
        queued.len()
    );
    assert!(socket.exists(), "its socket is kept");
    assert!(token_of(&place, &socket).exists(), "and its token");
    drop(queued);
    kill_process(Pid::from_raw(jar).unwrap(), Signal::CONT).unwrap();
    host.shutdown(END);
}

/// R32b: with no lock file to read, a socket nothing accepts at once is
/// unknown: answered for at once, never a blocking connect, and never
/// swept with its token.
#[test]
fn a_socket_without_a_lock_file_is_never_judged_gone() {
    let dir = short_dir();
    let place = place_in(dir.path(), PathBuf::from("/nonexistent"));
    place.prepare().unwrap();
    let socket = place.socket("cccc");
    let _held = not_accepting(&socket);
    tokens::save(&place, &socket, b"t").unwrap();
    assert_eq!(
        judged(&place, &socket),
        Some((SocketState::Unknown, 0)),
        "a full backlog and no lock file: nothing can be told, at once"
    );
    assert!(socket.exists(), "its socket is kept");
    assert!(token_of(&place, &socket).exists(), "and its token");
}

/// Ends the processes a test started in a jar, however the test ends.
struct Ends(Vec<i32>);

impl Drop for Ends {
    fn drop(&mut self) {
        for &p in &self.0 {
            if let Some(pid) = rustix::process::Pid::from_raw(p) {
                let _ = rustix::process::kill_process(pid, rustix::process::Signal::CONT);
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
                let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            }
        }
    }
}

/// Test 8 of the timer brief: a jar slow to start (its binary's first run
/// scanned under load) still gives the tab its program, run once.
#[test]
fn a_tab_whose_jar_is_slow_to_start_opens_with_its_program_run_once() {
    use std::os::unix::fs::PermissionsExt;
    let dir = short_dir();
    let slow = dir.path().join("slow-jar");
    std::fs::write(
        &slow,
        format!(
            "#!/bin/sh\nsleep 6\nexec '{}' \"$@\"\n",
            built_jar().display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
    let place = place_in(dir.path(), slow);
    let ran = dir.path().join("ran");
    let host = spawn_in(
        &place,
        &format!("echo once >> {}; exec cat", ran.display()),
        dir.path(),
    );
    let mut host = host.unwrap_or_else(|e| panic!("the tab didn't open: {e:#}"));
    host.write_all(b"typed\n").unwrap();
    read_until(&host, "typed");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        std::fs::read_to_string(&ran).unwrap(),
        "once\n",
        "the program ran once"
    );
    host.shutdown(END);
}

/// Test 8 of the timer brief: a jar stopped for six seconds while a restored
/// tab takes it back is still taken back, the same program, typing reaching
/// it, and nothing started twice.
#[test]
fn a_restored_tab_takes_back_a_jar_that_stalls_while_it_attaches() {
    use rustix::process::{Pid, Signal, kill_process};
    let dir = short_dir();
    let place = place_in(dir.path(), built_jar());
    let socket = place.socket(TAB);
    let first = pane_in(&place, "cat", dir.path(), None);
    let jar = describe_field(&socket, "pid");
    let program = first.process_id();
    let _ends = Ends(vec![
        program.and_then(|p| i32::try_from(p).ok()).unwrap(),
        jar,
    ]);
    first.host.sever_jar();
    drop(first);
    let until = Instant::now() + WAIT;
    while !spyc_jar::client::describe(&socket)
        .unwrap()
        .iter()
        .any(|(k, v)| k == "owner" && v.starts_with("none"))
    {
        assert!(Instant::now() < until, "premise: the jar saw its owner go");
        std::thread::sleep(Duration::from_millis(20));
    }

    kill_process(Pid::from_raw(jar).unwrap(), Signal::STOP).unwrap();
    let resume = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(6));
        kill_process(Pid::from_raw(jar).unwrap(), Signal::CONT).unwrap();
    });
    let second = pane_in(&place, "echo started-again", dir.path(), Some(socket));
    resume.join().unwrap();
    assert!(
        second.adopted() && !second.detached(),
        "a jar that answers late is taken back, not left detached"
    );
    assert_eq!(second.process_id(), program, "the same program");
    let mut second = second;
    second.send_bytes(b"typed\n").unwrap();
    shows(&second, "typed");
    assert!(!screen(&second).contains("started-again"));
    second.shutdown_reporting(END, || {});
}

/// A jar parked for an upgrade reads nothing from its owner for up to
/// [`spyc_jar::limits::PARKED_AT_MOST`]. spyc's writer, holding more than the socket takes
/// (typing, a snapshot), waits it out and delivers every byte, rather than
/// end a connection to a jar that is only busy.
#[test]
fn an_owner_outwaits_a_jar_parked_for_its_longest_upgrade() {
    use spyc_jar::limits::PARKED_AT_MOST;
    let dir = short_dir();
    let socket = dir.path().join("parked.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let (tx, got) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let Ok(Some(Msg::Hello { .. })) = proto::read_msg(&mut conn) else {
            panic!("no hello");
        };
        let welcome = Msg::Welcome {
            version: proto::VERSION,
            child_pid: std::process::id(),
            head: 0,
            base: 0,
            rows: 24,
            cols: 80,
        };
        proto::write_msg(&mut conn, &welcome).unwrap();
        std::thread::sleep(PARKED_AT_MOST);
        let mut input = 0;
        while let Ok(Some(m)) = proto::read_msg(&mut conn) {
            if let Msg::Input { bytes } = m {
                input += bytes.len();
                let _ = tx.send(input);
            }
        }
    });
    let (client, welcome) = Client::connect(&socket, Role::Owner, WAIT, b"token").unwrap();
    let Msg::Welcome { child_pid, .. } = welcome else {
        panic!("no welcome: {welcome:?}");
    };
    let fresh = Resume {
        pid: child_pid,
        replay: 0,
        offsets: offsets::Offsets::new(0),
        gate: ReplayGate::open(),
    };
    let jarred = connect(client, fresh, &socket, (24, 80), Duration::from_secs(2)).unwrap();
    let mut host = PtyHost::around_jar(jarred, (24, 80)).unwrap();
    // Keystroke-sized batches: once the socket is full, the next one makes no
    // progress at all, where one large frame's partial write would restart
    // the writer's clock.
    let batches = 400;
    for _ in 0..batches {
        host.write_all(&[b'x'; 256]).unwrap();
    }
    let pasted = batches * 256;
    let until = Instant::now() + PARKED_AT_MOST + WAIT;
    let mut through = 0;
    while through < pasted {
        match got.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(n) => through = n,
            Err(e) => panic!(
                "spyc gave up on a parked jar with {through} of {pasted} typed bytes through: {e}"
            ),
        }
    }
}

/// Every tab has a socket of its own, though tabs made together share their
/// ids' leading digits: a v7 UUID's are its millisecond and a counter.
#[test]
fn tabs_made_together_have_sockets_of_their_own() {
    let place = place_in(Path::new("/x"), PathBuf::from("/j"));
    let sockets: std::collections::HashSet<PathBuf> = (0..1000)
        .map(|_| place.socket(&crate::pane::tabs::TabInfo::new("zsh", "/").id))
        .collect();
    assert_eq!(sockets.len(), 1000, "tabs made together shared a socket");
}

/// SPYC-TRAP `jar-outwait`: every time constant in spyc's jar code is either
/// a wait asserted to outlast a parked jar, or here with the reason it may
/// be shorter. A new one fails this until it is one or the other.
#[test]
fn every_wait_on_a_jar_outlasts_it_or_says_why_not() {
    const OUTWAITS: [&str; 5] = [
        "ADOPT_WAIT",
        "END_HELD_WAIT",
        "LINK_WAIT",
        "TAKEOVER_WAIT",
        "WRITE_STALL",
    ];
    const SHORTER: [(&str, &str); 26] = [
        (
            "LET_GO_POLL",
            "how often a retake asks a jar whether it let a connection go, not a wait",
        ),
        (
            "LET_GO_RESTORING",
            "a restore's wait for a connection of this spyc's to go, past the jar's \
             deadline for a silent owner: running out leaves the tab detached, nothing \
             started, and a parked jar, silent, is asked again",
        ),
        (
            "SHUTDOWN_WAIT",
            "a session's spyc quitting, not a wait on a jar: running out says it hasn't ended, \
             and nothing is lost",
        ),
        (
            "SHUTDOWN_POLL",
            "how often `spyc shutdown` looks for the session's spyc to end, not a wait",
        ),
        (
            "NO_REPAINT_DUE",
            "a link's reader's idle wait: it wakes on a frame, and the connection's deadline ends it",
        ),
        (
            "ENDING_POLL",
            "how often a restart looks for the last jar to end, not a wait",
        ),
        (
            "ANSWER_TRY",
            "each try to reach a starting jar, within the start's hold, not a wait",
        ),
        (
            "DISCARD_POLL",
            "how often a discard looks for the program's exit, not a wait",
        ),
        (
            "BUILD_INFO_POLL",
            "how often a store binary's `build-info` is looked at, not a wait",
        ),
        (
            "WATCH_IDLE",
            "the own-jar watcher's wait between frames: the jar holds no deadline over a program",
        ),
        (
            "UPGRADE_WAIT",
            "a session's spyc restarting, not a wait on a jar: running out attaches anyway",
        ),
        (
            "START_POLL",
            "how often a handover looks for its session jar, not a wait",
        ),
        (
            "TAKEOVER_POLL",
            "how often a takeover looks at the jar, not a wait",
        ),
        (
            "DISCARD_WAIT",
            "a discard's wait on a jar no spyc owns, which no upgrade parks: running \
             out fails the discard, the jar left running, nothing lost",
        ),
        (
            "HANGUP_GRACE",
            "how long a discarded program has to end on a hangup: not a wait on the jar",
        ),
        (
            "PING_EVERY",
            "how often spyc speaks: asserted inside the jar's deadline",
        ),
        (
            "HOLD",
            "the jar's own hold, which spyc's answer waits out (decision M)",
        ),
        (
            "ENDING_WAIT",
            "a restart's wait for the last jar to end: running out fails the start, \
             `[not started]`, nothing lost, and `^a R` tries again",
        ),
        (
            "EXIT_WAIT",
            "shutdown's wait for an exit report: running out leaves the jar to end \
             its program alone, and quitting doesn't hang",
        ),
        ("REPLY_GRACE", "the engine's parser, not a wait on a jar"),
        (
            "REPAINT_HOLD",
            "how long a repaint holds a size a row off, not a wait on a jar",
        ),
        ("QUIET", "when a snapshot is taken, not a wait"),
        ("WHILE_FLOWING", "when a snapshot is taken, not a wait"),
        ("UNTOUCHED", "the store's sweep age, not a wait"),
        (
            "SWEEP_AFTER",
            "how young a socket is left unjudged; its lock judges it, not a wait",
        ),
        (
            "BUILD_INFO_WAIT",
            "a store binary's `build-info`, not a jar",
        ),
    ];
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pane/jar");
    let mut found = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|x| x != "rs") || path.ends_with("tests.rs") {
            continue;
        }
        let src = crate::guard_support::production_half(&std::fs::read_to_string(&path).unwrap());
        for line in src.lines() {
            let Some((head, rest)) = line.split_once(": ") else {
                continue;
            };
            if let Some(name) = head.trim_start().split("const ").nth(1)
                && rest.contains("Duration =")
            {
                found.insert(name.to_string());
            }
        }
    }
    assert!(
        found.contains("WRITE_STALL"),
        "premise: the scan finds waits"
    );
    let listed: std::collections::BTreeSet<String> = OUTWAITS
        .iter()
        .copied()
        .chain(SHORTER.iter().map(|(n, _)| *n))
        .map(String::from)
        .collect();
    assert_eq!(
        found, listed,
        "a time constant in src/pane/jar outlasts a parked jar, or says why it needn't"
    );
    let asserted = std::fs::read_to_string(dir.join("mod.rs")).unwrap();
    for name in OUTWAITS {
        assert!(
            asserted.contains(&format!("assert!(outwaits_a_parked_jar({name}))")),
            "{name} is asserted to outlast a parked jar"
        );
    }
}

/// Review finding B1: no thread in the `spyc-jar` crate is started so that
/// the OS's refusal panics (ending the jar on its main thread, or leaving a
/// lease or an upgrade claimed on another). Every one goes through
/// `threads::start`, which hands the refusal back for the site to fail what
/// wanted the thread.
///
/// Read from each file's syntax (`syn`), production code only. It refuses,
/// anywhere but `threads.rs`:
/// - std's `thread::spawn`, `thread::Builder` and `thread::scope`, called or
///   imported, `std::thread` renamed included (`use std::thread as t;
///   t::spawn(..)`), and a glob of `std::thread`;
/// - the same names inside a macro's tokens (`vec![std::thread::spawn(..)]`),
///   which `syn` leaves unparsed;
/// - `.expect(..)` or `.unwrap()` taken on a `threads::start(..)` (or a
///   `use` of it under another name) or on any `.spawn(..)`: directly, at
///   the end of a chain of method calls from one (`.ok().unwrap()`,
///   `.map(..).expect(..)`), or on a `let` binding such a start or chain
///   was put in. A `?` ends the chain: the refusal has been handed back;
/// - the same in the function-call form, `Result::unwrap(threads::start(..))`,
///   `Option::expect(..)` and the like, on a start, a chain or a binding.
///
/// It can't see: a refusal panicked on some other way (a `match` whose `Err`
/// arm panics, `unwrap_or_else(|e| panic!(..))`); a start's handle unwrapped
/// after it is passed on (to a function, a field, a tuple, an array or an
/// iterator: `[threads::start(..)].into_iter().next().map(Result::unwrap)`);
/// `unwrap` passed as a function (`.map(Result::unwrap)`); `.expect` taken on
/// a start inside a macro's tokens; and a thread a dependency starts.
#[test]
fn every_thread_the_jar_starts_can_be_refused() {
    use std::collections::HashSet;
    use syn::visit::Visit;

    const STARTERS: [&str; 3] = ["spawn", "Builder", "scope"];

    /// What a file's `use` items call std's `thread` module and
    /// `threads::start`.
    #[derive(Default)]
    struct Names {
        thread: HashSet<String>,
        start: HashSet<String>,
    }

    impl Names {
        fn uses(&mut self, prefix: &mut Vec<String>, tree: &syn::UseTree) {
            match tree {
                syn::UseTree::Path(p) => {
                    prefix.push(p.ident.to_string());
                    self.uses(prefix, &p.tree);
                    prefix.pop();
                }
                syn::UseTree::Name(n) => self.name(prefix, &n.ident, &n.ident),
                syn::UseTree::Rename(r) => self.name(prefix, &r.ident, &r.rename),
                syn::UseTree::Group(g) => {
                    for t in &g.items {
                        self.uses(prefix, t);
                    }
                }
                syn::UseTree::Glob(_) => {}
            }
        }

        fn name(&mut self, prefix: &[String], ident: &syn::Ident, called: &syn::Ident) {
            let last = prefix.last().map(String::as_str);
            if ident == "thread" || (ident == "self" && last == Some("thread")) {
                self.thread.insert(called.to_string());
            }
            if ident == "start" && last == Some("threads") {
                self.start.insert(called.to_string());
            }
        }
    }

    impl<'ast> Visit<'ast> for Names {
        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            self.uses(&mut Vec::new(), &item.tree);
        }
    }

    struct Scan {
        file: String,
        in_helper: bool,
        names: Names,
        bound: HashSet<String>,
        unhandled: Vec<String>,
        started: usize,
    }

    impl Scan {
        /// Whether `names` holds std's thread starters, under any name the
        /// file gives `thread`.
        fn std_starter(&self, names: &[String]) -> bool {
            names.windows(2).any(|w| {
                (w[0] == "thread" || self.names.thread.contains(&w[0]))
                    && STARTERS.contains(&w[1].as_str())
            })
        }

        /// Whether `path` names `threads::start`, under any name.
        fn is_start(&self, path: &syn::Path) -> bool {
            let names: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
            names
                .windows(2)
                .any(|w| w[0] == "threads" && w[1] == "start")
                || (names.len() == 1 && self.names.start.contains(&names[0]))
        }

        /// Whether `expr` starts a thread (or a process) and gives back its
        /// refusal.
        fn starts(&self, expr: &syn::Expr) -> bool {
            match expr {
                syn::Expr::Call(call) => {
                    matches!(&*call.func, syn::Expr::Path(p) if self.is_start(&p.path))
                }
                syn::Expr::MethodCall(m) => m.method == "spawn",
                syn::Expr::Path(p) => p
                    .path
                    .get_ident()
                    .is_some_and(|i| self.bound.contains(&i.to_string())),
                _ => false,
            }
        }

        /// Whether `expr` is a start, or a chain of method calls from one
        /// that still carries its refusal.
        fn chained_from_start(&self, expr: &syn::Expr) -> bool {
            match expr {
                syn::Expr::MethodCall(m) if !self.starts(expr) => {
                    self.chained_from_start(&m.receiver)
                }
                syn::Expr::Paren(p) => self.chained_from_start(&p.expr),
                _ => self.starts(expr),
            }
        }

        fn refuse(&mut self, what: String) {
            if !self.in_helper {
                self.unhandled.push(format!("{}: {what}", self.file));
            }
        }

        fn uses(&mut self, prefix: &mut Vec<String>, tree: &syn::UseTree) {
            match tree {
                syn::UseTree::Path(p) => {
                    prefix.push(p.ident.to_string());
                    self.uses(prefix, &p.tree);
                    prefix.pop();
                }
                syn::UseTree::Name(n) => self.used(prefix, &n.ident),
                syn::UseTree::Rename(r) => self.used(prefix, &r.ident),
                syn::UseTree::Group(g) => {
                    for t in &g.items {
                        self.uses(prefix, t);
                    }
                }
                syn::UseTree::Glob(_) => {
                    if prefix.last().is_some_and(|l| l == "thread") {
                        self.refuse(format!("use {}::*", prefix.join("::")));
                    }
                }
            }
        }

        fn used(&mut self, prefix: &[String], ident: &syn::Ident) {
            let mut names = prefix.to_vec();
            names.push(ident.to_string());
            if self.std_starter(&names) {
                self.refuse(format!("use {}", names.join("::")));
            }
        }
    }

    impl<'ast> Visit<'ast> for Scan {
        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            self.uses(&mut Vec::new(), &item.tree);
        }

        fn visit_path(&mut self, path: &'ast syn::Path) {
            let names: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
            if self.std_starter(&names) {
                self.refuse(names.join("::"));
            }
            if self.is_start(path) {
                self.started += 1;
            }
            syn::visit::visit_path(self, path);
        }

        fn visit_local(&mut self, local: &'ast syn::Local) {
            if let (syn::Pat::Ident(p), Some(init)) = (&local.pat, &local.init)
                && self.chained_from_start(&init.expr)
            {
                self.bound.insert(p.ident.to_string());
            }
            syn::visit::visit_local(self, local);
        }

        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            if (call.method == "expect" || call.method == "unwrap")
                && self.chained_from_start(&call.receiver)
            {
                self.refuse(format!(
                    ".{}() on a start or a chain from one, which panics when the OS refuses it",
                    call.method
                ));
            }
            syn::visit::visit_expr_method_call(self, call);
        }

        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if let syn::Expr::Path(p) = &*call.func
                && let Some(last) = p.path.segments.last()
                && (last.ident == "expect" || last.ident == "unwrap")
                && p.path.segments.len() >= 2
                && call
                    .args
                    .first()
                    .is_some_and(|a| self.chained_from_start(a))
            {
                self.refuse(format!(
                    "{}(..) on a start or a chain from one, which panics when the OS refuses it",
                    p.path
                        .segments
                        .iter()
                        .map(|s| s.ident.to_string())
                        .collect::<Vec<_>>()
                        .join("::")
                ));
            }
            syn::visit::visit_expr_call(self, call);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            let tokens: String = mac
                .tokens
                .to_string()
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            let thread: Vec<String> = std::iter::once("thread".to_string())
                .chain(self.names.thread.iter().cloned())
                .collect();
            for name in thread {
                for starter in STARTERS {
                    if tokens.contains(&format!("{name}::{starter}")) {
                        self.refuse(format!("{name}::{starter} in a macro's tokens"));
                    }
                }
            }
            syn::visit::visit_macro(self, mac);
        }
    }

    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/spyc-jar/src");
    let mut unhandled = Vec::new();
    let mut started = 0;
    let mut files = 0;
    let mut dirs = vec![src];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().is_none_or(|x| x != "rs") {
                continue;
            }
            let text =
                crate::guard_support::production_half(&std::fs::read_to_string(&path).unwrap());
            let file = syn::parse_file(&text)
                .unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
            let mut names = Names::default();
            names.visit_file(&file);
            let mut scan = Scan {
                file: path.display().to_string(),
                in_helper: path.ends_with("threads.rs"),
                names,
                bound: HashSet::new(),
                unhandled: Vec::new(),
                started: 0,
            };
            scan.visit_file(&file);
            unhandled.extend(scan.unhandled);
            started += scan.started;
            files += 1;
        }
    }
    assert!(
        files > 10 && started >= 15,
        "premise: the scan reads the jar's {files} files and finds its threads, {started} of them"
    );
    assert!(
        unhandled.is_empty(),
        "a thread whose refusal panics; start it with threads::start and fail what wanted it: {unhandled:#?}"
    );
}

/// Review finding A1: a jar whose upgrade `exec` failed has `SIGPIPE` at its
/// default, so no send of the jar's may raise it (SPYC-TRAP `jar-no-sigpipe`):
/// every `sendmsg` carries `NO_SIGNAL`, every socketpair comes from
/// `jar::pair`, and nothing writes a socket with `writev`, which std sends
/// without `MSG_NOSIGNAL`.
#[test]
fn every_send_the_jar_makes_answers_a_gone_peer_with_epipe() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/spyc-jar/src");
    let mut wrong = Vec::new();
    let mut sends = 0;
    let mut dirs = vec![src];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().is_none_or(|x| x != "rs") {
                continue;
            }
            let text =
                crate::guard_support::production_half(&std::fs::read_to_string(&path).unwrap());
            let lines: Vec<&str> = text
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect();
            let defines = path.ends_with("jar/accept.rs");
            for (i, line) in lines.iter().enumerate() {
                let at = || format!("{}: {}", path.display(), line.trim());
                if line.contains("sendmsg(") {
                    sends += 1;
                    let call = lines[i..lines.len().min(i + 8)].join(" ");
                    if !call.contains("NO_SIGNAL") {
                        wrong.push(at());
                    }
                }
                let flags = line.contains("SendFlags::") && !defines;
                let pair = line.contains("UnixStream::pair(") && !defines;
                if flags || pair || line.contains("write_vectored") {
                    wrong.push(at());
                }
            }
        }
    }
    assert!(
        sends >= 2,
        "premise: the scan finds the jar's sendmsg calls ({sends})"
    );
    assert!(
        wrong.is_empty(),
        "a send that may raise SIGPIPE ends a jar whose exec failed: {wrong:#?}"
    );
}

/// Every time constant in the `spyc-jar` crate, by what running out costs: a
/// new timer fails this until it is classified. A running jar keeps its
/// compiled-in timers for its whole life, so each is sized for its kind
/// before it ships. A parked one is part of `limits::PARKED_AT_MOST`, which
/// clients outwait (SPYC-TRAP `jar-outwait`).
#[test]
fn every_timer_in_the_jar_is_classified_by_what_running_out_costs() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Kind {
        /// Before an upgrade's `exec`, the program served: nothing is lost,
        /// so a long backstop.
        Served,
        /// Before the `exec`, the program parked: the jar unparks and reports
        /// busy, so short.
        Parked,
        /// After the `exec`: running out loses something, so it ends on an
        /// event, and the clock is only a backstop.
        AfterExec,
        /// A peer's deadline: running out ends a lease or a connection, so
        /// sized for a starved but live peer.
        Peer,
        /// Running out fails a jar's start; nothing running is lost.
        Start,
        /// Running out ends a program that is being ended anyway.
        Ending,
        /// How often something is looked at, not a wait.
        Poll,
        /// A limit published for clients, made from the others.
        Limit,
        /// Never runs out: what it keeps ends on an event (a capture's exit
        /// collected by its owner).
        Unbounded,
        /// A command-line client's own wait (`attach`, `upgrade`, a lookup):
        /// running out ends that command with an error; nothing in a jar is lost.
        Command,
        /// A floor that keeps a computed wait from being zero, which a socket
        /// takes as none, waiting for ever.
        Floor,
        /// How long a jar keeps something for an owner that isn't there:
        /// running out drops it.
        Keep,
        /// A debug build's stretch of a deadline, for a test that tells "at
        /// once" from "at the deadline"; a release build has none.
        DebugStretch,
    }
    use Kind::{
        AfterExec, Command, DebugStretch, Ending, Floor, Keep, Limit, Parked, Peer, Poll, Served,
        Start, Unbounded,
    };
    const TIMERS: [(&str, Kind); 47] = [
        ("ATTACH_WAIT", Command),
        ("ATTACH_PING", Poll),
        ("SIZE_POLL", Poll),
        ("WRITE_STALL", Peer),
        ("RECONNECT_RETRY", Poll),
        ("SHORTEST_WAIT", Floor),
        ("PARK_POLL", Poll),
        ("VALIDATOR_POLL", Poll),
        ("CONNECT_RETRY", Poll),
        ("DESCRIBE_WAIT", Command),
        ("DESCRIBE_POLL", Poll),
        ("ACCEPT_RETRY", Poll),
        ("LOCK_RETRY", Poll),
        ("EXIT_LINGER", Keep),
        ("COMMAND_WAIT", Command),
        ("UPGRADE_ANSWER_WAIT", Command),
        ("PROGRAM_EVENTS_TICK", Poll),
        ("WRITER_TICK", Poll),
        ("WAITER_POLL", Poll),
        ("REAPER_TICK", Poll),
        ("END_GRACE", Ending),
        ("END_POLL", Poll),
        ("FIRST_VALIDATION", Served),
        ("UNTIL_READ", Unbounded),
        ("PARK_DEADLINE", Parked),
        ("FLUSH_DEADLINE", Parked),
        ("PARKED_VALIDATION", Parked),
        ("OUTPUT_GRACE", Parked),
        ("HOLDER_ANSWER", Parked),
        ("HOLD_BACKSTOP", AfterExec),
        ("OWNER_DEADLINE", Peer),
        ("FIRST_OWNER_GRACE", Peer),
        ("FRAME_DEADLINE", Peer),
        ("LOOKUP_DEADLINE", Peer),
        ("WELCOME_WAIT", Peer),
        ("REST_OF_FRAME", Peer),
        ("WAIT", Peer),
        ("READER_GRACE", Start),
        ("HANGUP_GRACE", Ending),
        ("PARENT_CHECK", Poll),
        ("HOLDER_POLL", Poll),
        ("TICK", Poll),
        ("SOCKET_CHECK", Poll),
        ("GAP_POLL", Poll),
        ("PARKED_AT_MOST", Limit),
        ("UPGRADE_AT_MOST", Limit),
        ("STRETCHED", DebugStretch),
    ];
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/spyc-jar/src");
    let mut found = std::collections::BTreeSet::new();
    let mut dirs = vec![src.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().is_none_or(|x| x != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for line in crate::guard_support::production_half(&text).lines() {
                let Some((head, rest)) = line.split_once(": ") else {
                    continue;
                };
                if let Some(name) = head.trim_start().split("const ").nth(1)
                    && rest.contains("Duration =")
                {
                    found.insert(name.to_string());
                }
            }
        }
    }
    assert!(
        found.contains("FIRST_VALIDATION"),
        "premise: the scan finds timers"
    );
    let listed: std::collections::BTreeSet<String> =
        TIMERS.iter().map(|(n, _)| (*n).to_string()).collect();
    assert_eq!(
        found, listed,
        "every time constant in spyc-jar is classified by what running out costs"
    );
    let limits = std::fs::read_to_string(src.join("limits.rs")).unwrap();
    let parked_at_most = crate::guard_support::production_half(&limits)
        .split("pub const PARKED_AT_MOST")
        .nth(1)
        .and_then(|rest| rest.split(';').next())
        .map(str::to_string)
        .expect("PARKED_AT_MOST's definition");
    for (name, kind) in TIMERS {
        if kind == Parked {
            assert!(
                parked_at_most.contains(name),
                "{name} parks the jar, so it is part of PARKED_AT_MOST"
            );
        }
    }
}

/// No timer in jar code is written inline. A `Duration` given as a number,
/// rather than a named constant, is in neither guard above, so it ships
/// without anyone having said what its running out costs. A conversion of a
/// value read at run time (`from_millis(ms)`) isn't a timer and doesn't count.
#[test]
fn no_timer_in_jar_code_is_written_inline() {
    /// Durations written as numbers that aren't timers, by file and line.
    const NOT_TIMERS: [(&str, &str, &str); 3] = [
        (
            "handover.rs",
            "deadline: Duration::from_secs(10),",
            "`sample_blob`'s config, data for the blob's tests and fuzz seeds",
        ),
        (
            "handover.rs",
            "linger_after_exit: Duration::from_secs(60),",
            "`sample_blob`'s config, data for the blob's tests and fuzz seeds",
        ),
        (
            "handover.rs",
            "linger: Some(Duration::from_secs(300)),",
            "`sample_blob`'s config, data for the blob's tests and fuzz seeds",
        ),
    ];
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let inline =
        regex::Regex::new(r"Duration::from_(secs|millis|micros|nanos)\(\s*[0-9][0-9_]*\s*\)")
            .expect("regex");
    let mut scanned = 0;
    let mut named = 0;
    let mut written_inline = Vec::new();
    for top in ["crates/spyc-jar/src", "src/pane/jar"] {
        let mut dirs = vec![root.join(top)];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if path.extension().is_none_or(|x| x != "rs")
                    || name == "tests.rs"
                    || name == "test_dir.rs"
                {
                    continue;
                }
                scanned += 1;
                let text = std::fs::read_to_string(&path).unwrap();
                let mut declaring = false;
                for (n, line) in crate::guard_support::production_half(&text)
                    .lines()
                    .enumerate()
                {
                    let constant = line.contains("const ") && line.contains("Duration");
                    let exempt = NOT_TIMERS
                        .iter()
                        .any(|(file, text, _)| name == *file && line.trim() == *text);
                    if inline.is_match(line) && !exempt {
                        if constant || declaring {
                            named += 1;
                        } else {
                            written_inline.push(format!(
                                "{}:{}: {}",
                                path.display(),
                                n + 1,
                                line.trim()
                            ));
                        }
                    }
                    declaring = constant && line.trim_end().ends_with('=');
                }
            }
        }
    }
    assert!(
        scanned > 20 && named > 20,
        "premise: the scan reads jar code ({scanned} files, {named} named timers)"
    );
    assert!(
        written_inline.is_empty(),
        "name each timer as a constant and classify it:\n{}",
        written_inline.join("\n")
    );
}

/// A jar left by a crash of its spyc: a tab's, told what it holds, its
/// connection severed. Its host is forgotten, as a crash forgets it.
fn left_by_a_crash(place: &Place, dir: &Path) -> (PathBuf, u32) {
    let host = spawn_named(place, "left-by-a-crash", "exec cat", dir).unwrap();
    let socket = host.jar_socket().unwrap().to_path_buf();
    let program = host.process_id().unwrap();
    host.tell_jar(&[
        ("is", "pane"),
        ("pane_id", "tab-1"),
        ("label", "orphaned"),
        ("command", "cat"),
        ("agent", "other"),
        ("project", "work"),
        ("project_home", "/home/work"),
    ]);
    let until = Instant::now() + WAIT;
    while !spyc_jar::client::describe(&socket).is_ok_and(|f| {
        f.iter()
            .any(|(k, v)| k == "project_home" && v == "/home/work")
    }) {
        assert!(Instant::now() < until, "the jar was never told");
        std::thread::sleep(Duration::from_millis(20));
    }
    host.sever_jar();
    std::mem::forget(host);
    // The jar hears its owner went in its own time; until then it refuses one.
    while !spyc_jar::client::describe(&socket)
        .is_ok_and(|f| f.iter().any(|(k, v)| k == "owner" && v.starts_with("none")))
    {
        assert!(Instant::now() < until, "the jar never heard its owner go");
        std::thread::sleep(Duration::from_millis(20));
    }
    (socket, program)
}

fn running(pid: u32) -> bool {
    rustix::process::Pid::from_raw(i32::try_from(pid).unwrap())
        .is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
}

/// S2c: a live jar with no owner, whose token this spyc keeps, is found by
/// what it holds; one owned, or whose token isn't here, isn't.
#[test]
fn jars_no_spyc_owns_are_found_by_what_they_hold() {
    let dir = short_dir();
    let place = place_in(dir.path(), built_jar());
    let mut owned = spawn_in(&place, "exec cat", dir.path()).unwrap();
    assert!(
        orphans::find(Some(place.clone())).is_empty(),
        "an owned jar"
    );
    // A host dropped leaves its jar running, as a spyc that exits does.
    let owned_socket = owned.jar_socket().unwrap().to_path_buf();
    owned.shutdown(Duration::from_secs(2));
    drop(owned);
    let until = Instant::now() + WAIT;
    while owned_socket.exists() {
        assert!(
            Instant::now() < until,
            "the owned jar ran on after its tab closed"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let (socket, program) = left_by_a_crash(&place, dir.path());
    let mut found = Vec::new();
    let until = Instant::now() + WAIT;
    while found.is_empty() {
        assert!(Instant::now() < until, "the orphan was never found");
        found = orphans::find(Some(place.clone()));
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(found.len(), 1, "{found:?}");
    let orphan = &found[0];
    assert_eq!(orphan.socket, socket);
    assert_eq!(
        (
            orphan.is.as_str(),
            orphan.label.as_str(),
            orphan.command.as_str()
        ),
        ("pane", "orphaned", "cat")
    );
    assert_eq!(orphan.project_home, "/home/work");
    assert_eq!(orphan.pane_id, "tab-1");

    let token = token_of(&place, &socket);
    let kept = std::fs::read(&token).unwrap();
    std::fs::remove_file(&token).unwrap();
    assert!(
        orphans::find(Some(place.clone())).is_empty(),
        "a jar whose token isn't here is another spyc's"
    );
    std::fs::write(&token, kept).unwrap();
    orphans::discard(Some(place), &socket).unwrap();
    let until = Instant::now() + WAIT;
    while running(program) {
        assert!(Instant::now() < until, "the discarded jar's program ran on");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// S2c: a discarded jar ends, its program with it, its socket and token
/// gone after.
#[test]
fn a_discarded_jar_ends_with_its_program() {
    let dir = short_dir();
    let place = place_in(dir.path(), built_jar());
    let (socket, program) = left_by_a_crash(&place, dir.path());
    orphans::discard(Some(place.clone()), &socket).unwrap();
    let until = Instant::now() + WAIT;
    while running(program) || socket.exists() {
        assert!(Instant::now() < until, "the jar or its program ran on");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!token_of(&place, &socket).exists(), "its token was kept");
}
