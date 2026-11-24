//! The upgrade step (`JAR_PROTOCOL.md`, decisions L and M), in
//! `jar_process`'s harness: an upgrade never costs the program it exists to
//! keep, and a pane's jar starts its program for its first owner.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use spyc_jar::client::Client;
use spyc_jar::proto::{Msg, Role};

use super::*;

// The upgrade step's red tests (`JAR_UPGRADE_BRIEF.md` → "1. Red tests first";
// `JAR_PROTOCOL.md`, decision L): an upgrade must never cost the program it
// exists to keep, whatever binary it's handed.

/// A binary at `dir/name` that runs `cases` (a `case "$1" in` body) and hands
/// anything else to the real jar.
pub fn jar_script(dir: &std::path::Path, name: &str, cases: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nREAL={real}\ncase \"$1\" in\n{cases}\nesac\nexec \"$REAL\" \"$@\"\n",
            real = env!("CARGO_BIN_EXE_spyc-jar")
        ),
    )
    .expect("write the script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

/// Ask `owner` for an upgrade to `binary`; what the jar answers (`Upgraded`
/// or `Refused`) and how long it took, pinging and acknowledging output
/// meanwhile. `None`
/// when the jar answers neither within `wait`, or the connection ends.
pub fn upgrade_answer(
    owner: &mut Client,
    seen: &mut Seen,
    binary: &std::path::Path,
    wait: Duration,
) -> (Option<Msg>, Duration) {
    let at = Instant::now();
    owner
        .send(&Msg::Upgrade {
            binary: binary.display().to_string(),
        })
        .expect("upgrade");
    let mut n = 1 << 40;
    let mut pinged = at;
    while at.elapsed() < wait {
        // Pinging once a second, as spyc's owner does while it waits: one
        // silent past the jar's deadline loses its lease, and the answer.
        if pinged.elapsed() >= Duration::from_secs(1) {
            n += 1;
            let _ = owner.send(&Msg::Ping { n });
            pinged = Instant::now();
        }
        match owner.recv(Duration::from_millis(200)) {
            Ok(Some(m @ (Msg::Upgraded { .. } | Msg::Refused { .. }))) => {
                return (Some(m), at.elapsed());
            }
            Ok(Some(Msg::Pong { .. }) | None) => {}
            Ok(Some(m)) => {
                seen.take(m);
                if let Some(next) = seen.next_offset {
                    let _ = owner.send(&Msg::Ack { offset: next });
                }
            }
            Err(_) => return (None, at.elapsed()),
        }
    }
    (None, at.elapsed())
}

/// A shell loop that waits for `go` to exist: a step held until the test
/// lets it go. It ends with the test's directory too, so a test that fails
/// first leaves nothing waiting behind it.
pub fn held_until(go: &std::path::Path) -> String {
    format!(
        "while [ -d {dir} ] && [ ! -e {go} ]; do sleep 0.05; done",
        dir = go.parent().expect("a directory").display(),
        go = go.display()
    )
}

/// Read on `owner`, acknowledging, until `more` counts have come past the
/// last one `seen` holds: the program runs and the jar serves it.
pub fn count_on(owner: &mut Client, seen: &mut Seen, more: u64, what: &str) {
    let from = counts(seen).last().copied().unwrap_or(0);
    let until = Instant::now() + WAIT;
    while counts(seen).last().is_none_or(|n| *n < from + more) {
        assert!(
            Instant::now() < until,
            "{what}: the count stopped at {:?}, frames {:?}",
            counts(seen).last(),
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

/// The program still counts and reads input, on `owner`'s connection or, if
/// that one ended, a new owner's.
pub fn assert_program_survives(jar: &mut Jar, owner: Client, what: &str) {
    let mut owner = if owner.send(&Msg::Ping { n: 77 }).is_ok() {
        owner
    } else {
        jar.connect(Role::Owner)
    };
    let _ = owner.send(&Msg::Replay { from: 0 });
    owner
        .send(&Msg::Input {
            bytes: b"survived\n".to_vec(),
        })
        .unwrap_or_else(|e| panic!("{what}: the jar is gone: {e}"));
    let mut seen = Seen::default();
    let until = Instant::now() + WAIT;
    while !seen.contains("got=survived") {
        assert!(
            Instant::now() < until,
            "{what}: the program didn't survive; tail {:?}",
            String::from_utf8_lossy(&seen.text[seen.text.len().saturating_sub(120)..])
        );
        match owner.recv(Duration::from_millis(200)) {
            Ok(Some(Msg::Output { offset, bytes })) => {
                seen.text.extend(&bytes);
                let _ = owner.send(&Msg::Ack {
                    offset: offset + bytes.len() as u64,
                });
            }
            Ok(_) => {}
            Err(e) => panic!("{what}: the jar's connection ended: {e:#}"),
        }
    }
    assert!(
        jar.process.try_wait().expect("try_wait").is_none(),
        "{what}: the jar lives"
    );
}

/// `fake`: a binary that validates and holds but can't resume. Without a
/// proof line its validation proves nothing, and it must be refused before
/// anything is handed to it; today the jar `exec`s it and is lost, and its
/// program gets `SIGHUP`.
#[test]
fn an_upgrade_to_a_binary_that_cant_resume_leaves_the_program_running() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_while_it_comes(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let fake = jar_script(
        jar.dir.path(),
        "jar-fake",
        "validate-handover) cat >/dev/null; exit 0;;\nresume-handover) exit 1;;",
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &fake, UPGRADE_AT_MOST);
    assert!(
        matches!(answer, Some(Msg::Refused { .. })),
        "the requester hears a refusal: {answer:?}"
    );
    assert_program_survives(&mut jar, owner, "fake");
}

/// `swap`: the named file replaced while the holder starts. The jar must
/// `exec` the image it validated, never the path looked up again; today it
/// runs whatever is at the path by then.
#[test]
fn an_upgrade_runs_the_image_it_validated_though_the_file_is_replaced() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_while_it_comes(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let dir = jar.dir.path();
    let broken = jar_script(dir, "jar-broken", "resume-handover) exit 1;;");
    let named = dir.join("jar-swapped");
    jar_script(
        dir,
        "jar-swapped",
        &format!(
            "hold-handover) cp {broken} {named}.new && mv {named}.new {named};;",
            broken = broken.display(),
            named = named.display()
        ),
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &named, UPGRADE_AT_MOST);
    assert!(
        matches!(answer, Some(Msg::Upgraded { .. })),
        "the image that was validated takes over: {answer:?}"
    );
    assert_program_survives(&mut jar, owner, "swap");
}

/// A validator's exit status proves nothing (`/usr/bin/true` passes it): one
/// that exits 0 without its proof line is refused (reason 6), and the jar
/// runs on as it was.
#[test]
fn a_validator_that_prints_no_proof_is_refused() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_while_it_comes(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let silent = jar_script(
        jar.dir.path(),
        "jar-silent",
        "validate-handover) cat >/dev/null; exit 0;;",
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &silent, UPGRADE_AT_MOST);
    assert!(
        matches!(
            answer,
            Some(Msg::Refused {
                reason: spyc_jar::proto::reason::UPGRADE_REJECTED,
                ..
            })
        ),
        "refused with reason 6: {answer:?}"
    );
    assert_program_survives(&mut jar, owner, "a silent validator");
}

/// A validator that exits while a child of its keeps its standard error: the
/// refusal comes while the child still holds it, so the jar never waited for
/// that standard error's end, and the program runs on. The child holds it
/// until the test lets it go, after the answer.
#[test]
fn a_validators_child_holding_its_stderr_cannot_hold_the_jar() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_while_it_comes(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let go = jar.dir.path().join("go");
    let holding = jar_script(
        jar.dir.path(),
        "jar-holding",
        &format!("validate-handover) ( {} ) & exit 1;;", held_until(&go)),
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &holding, UPGRADE_AT_MOST);
    std::fs::write(&go, "").expect("let the validator's child go");
    assert!(
        matches!(answer, Some(Msg::Refused { .. })),
        "refused while the validator's child held its standard error: {answer:?}"
    );
    assert_program_survives(&mut jar, owner, "a held standard error");
}

/// A validator that outruns its deadline (here the first validation's long
/// backstop) is ended with everything it started, its whole process group
/// and not only the direct child, and the upgrade is busy.
#[test]
fn a_validator_past_its_deadline_is_ended_with_its_children() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_while_it_comes(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let pidfile = jar.dir.path().join("validator-child.pid");
    let slow = jar_script(
        jar.dir.path(),
        "jar-slow",
        &format!(
            "validate-handover) sleep 60 & echo $! > {}; sleep 60;;",
            pidfile.display()
        ),
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &slow, Duration::from_secs(60));
    assert!(
        matches!(
            answer,
            Some(Msg::Refused {
                reason: spyc_jar::proto::reason::UPGRADE_BUSY,
                ..
            })
        ),
        "a validator out of time makes the upgrade busy: {answer:?}"
    );
    let child: i32 = std::fs::read_to_string(&pidfile)
        .expect("the validator started a child")
        .trim()
        .parse()
        .expect("a pid");
    let pid = rustix::process::Pid::from_raw(child).expect("a pid");
    let gone = || rustix::process::test_kill_process(pid).is_err();
    let until = Instant::now() + Duration::from_secs(2);
    while !gone() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    let alive = !gone();
    if alive {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
    }
    assert!(!alive, "the validator's child outlived its deadline");
    assert_program_survives(&mut jar, owner, "a slow validator");
}

/// A proof line for some other blob proves nothing: the instance and the
/// length must be this blob's.
#[test]
fn a_proof_for_another_blob_is_refused() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_while_it_comes(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    // The right length with another instance, and the real validator's
    // line with another length.
    let liars = [
        (
            "jar-other-instance",
            "validate-handover) n=$(wc -c | tr -d ' '); echo spyc-jar-handover-ok 0123456789abcdef $n; exit 0;;",
        ),
        (
            "jar-other-length",
            "validate-handover) set -- $(\"$REAL\" validate-handover); echo $1 $2 1; exit 0;;",
        ),
    ];
    for (name, cases) in liars {
        let liar = jar_script(jar.dir.path(), name, cases);
        let (answer, _) = upgrade_answer(&mut owner, &mut seen, &liar, UPGRADE_AT_MOST);
        assert!(
            matches!(
                answer,
                Some(Msg::Refused {
                    reason: spyc_jar::proto::reason::UPGRADE_REJECTED,
                    ..
                })
            ),
            "{name}: {answer:?}"
        );
    }
    assert_program_survives(&mut jar, owner, "a proof for another blob");
}

/// A jar takes an absolute path only (reason 8): a relative one would be
/// looked up from wherever the jar happens to be.
#[test]
fn an_upgrade_to_a_relative_path_is_refused() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_while_it_comes(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let (answer, _) = upgrade_answer(
        &mut owner,
        &mut seen,
        std::path::Path::new("relative/spyc-jar"),
        UPGRADE_AT_MOST,
    );
    assert!(
        matches!(
            answer,
            Some(Msg::Refused {
                reason: spyc_jar::proto::reason::INVALID,
                ..
            })
        ),
        "{answer:?}"
    );
    assert_program_survives(&mut jar, owner, "a relative path");
}

/// The new image is told the image it replaced (`PREVIOUS`), the one it
/// would switch back to.
#[test]
fn a_new_image_is_told_the_image_it_replaced() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_while_it_comes(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let told = jar.dir.path().join("previous");
    let telling = jar_script(
        jar.dir.path(),
        "jar-telling",
        &format!("resume-handover) printf %s \"$2\" > {};;", told.display()),
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &telling, UPGRADE_AT_MOST);
    assert!(matches!(answer, Some(Msg::Upgraded { .. })), "{answer:?}");
    assert_eq!(
        std::fs::read_to_string(&told).expect("the new image was told"),
        env!("CARGO_BIN_EXE_spyc-jar")
    );
    assert_program_survives(&mut jar, owner, "an upgrade");
}

/// A jar's name, the `argv[0]` its starter gave it and `ps` shows, crosses an
/// upgrade's `exec`.
#[test]
fn a_jars_name_crosses_an_upgrade() {
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    let mut owner = jar.connect(Role::Owner);
    let pid = jar.process.id();
    assert_eq!(command_line(pid), format!("{NAME} --wire 1"), "premise");
    upgrade(&jar, &mut owner).expect("the jar upgrades");
    let line = command_line(pid);
    assert!(
        line.starts_with(&format!("{NAME} resume-handover ")),
        "{line:?}"
    );
}

/// R12b across an upgrade: a capture's jar still holds its program rather
/// than drop output, since the exit linger that says so crosses in the blob.
#[test]
fn a_captures_jar_still_loses_no_output_after_an_upgrade() {
    let mut jar = Jar::keeping_its_exit(
        &["--ring", "65536", "--window", "32768"],
        &sh(&format!("read go; {MILLION_BS}; echo; echo END")),
    );
    let mut owner = jar.connect(Role::Owner);
    upgrade(&jar, &mut owner).expect("the jar upgrades");
    owner
        .send(&Msg::Input {
            bytes: b"\n".to_vec(),
        })
        .unwrap();
    owner.send(&Msg::Replay { from: 0 }).unwrap();
    read_until(&mut owner, false, |s| !s.text.is_empty());
    owner.drop_connection();
    eventually("the owner's lease to end", || {
        described(&jar, "owner") != "connected"
    });
    assert_every_byte_replays(&mut jar);
}

/// A new binary's first run is slow (about 263 ms on macOS for a new file):
/// the jar pays it while it still serves the program, validating once before
/// anything parks, and again, parked, on the real blob (decision L, part 7).
/// The first validation is held until the program has counted on, so it is
/// paid with the program served however long it takes.
#[test]
fn a_slow_first_validation_never_stalls_the_program() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_while_it_comes(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let began = jar.dir.path().join("first-run-began");
    let go = jar.dir.path().join("go");
    let slow_once = jar_script(
        jar.dir.path(),
        "jar-slow-once",
        &format!(
            "validate-handover) [ -e {began} ] || {{ touch {began}; {held}; }};;",
            began = began.display(),
            held = held_until(&go)
        ),
    );
    owner
        .send(&Msg::Upgrade {
            binary: slow_once.display().to_string(),
        })
        .expect("upgrade");
    let until = Instant::now() + UPGRADE_AT_MOST;
    while !began.exists() {
        assert!(Instant::now() < until, "the first validation never began");
        count_on(&mut owner, &mut seen, 1, "before the first validation");
    }
    count_on(&mut owner, &mut seen, 20, "while the first validation runs");
    std::fs::write(&go, "").expect("let the first validation end");
    let answered = |s: &Seen| {
        s.frames
            .iter()
            .any(|f| matches!(f, Msg::Upgraded { .. } | Msg::Refused { .. }))
    };
    let until = Instant::now() + UPGRADE_AT_MOST;
    while !answered(&seen) {
        assert!(Instant::now() < until, "no answer to the upgrade");
        if let Some(m) = owner.recv(Duration::from_millis(50)).expect("recv") {
            seen.take(m);
            if let Some(next) = seen.next_offset {
                owner.send(&Msg::Ack { offset: next }).expect("ack");
            }
        }
    }
    assert!(
        seen.frames
            .iter()
            .any(|f| matches!(f, Msg::Upgraded { .. })),
        "{:?}",
        seen.frames
    );
    assert_unbroken(&seen);
    assert_program_survives(&mut jar, owner, "a slow first validation");
}

/// A reader that takes a little every 0.9 s while the jar flushes its last
/// frames before a handover gets one deadline for the whole flush, not one
/// for each write or each wait: it can't keep the jar parked. The jar leaves
/// it behind, closed, and the upgrade goes ahead without it. It takes 4 KiB
/// at a time, which makes the socket writable each time (a byte at a time
/// never does on macOS, which wakes a writer only once 2 KiB are free).
///
/// The reader guesses the park from a lookup's silence and must begin within
/// the jar's own 3 s park deadline; when it doesn't, the jar refuses the
/// upgrade as busy, the scenario never came about, and it is set up again,
/// at most three times.
#[test]
fn a_reader_trickling_during_the_flush_cannot_keep_the_jar_parked() {
    let mut missed = Vec::new();
    for _ in 0..3 {
        match trickling_reader() {
            None => return,
            Some(why) => missed.push(why),
        }
    }
    panic!("the reader never trickled through a flush: {missed:?}");
}

/// How a trickling reader's connection ended up.
#[derive(Debug)]
enum Trickled {
    /// The jar closed it: left behind.
    LeftBehind,
    /// Still open once the upgrade was answered: carried.
    Carried,
    /// The jar refused the upgrade: what it said.
    Refused(String),
    /// Something else went wrong.
    Broke(String),
}

/// What a reader's raw bytes say of a refusal of its upgrade, if anything.
fn refusal_in(bytes: &[u8]) -> Option<String> {
    let needle = b"upgrade refused";
    let at = bytes.windows(needle.len()).position(|w| w == needle)?;
    Some(String::from_utf8_lossy(&bytes[at..(at + 160).min(bytes.len())]).into_owned())
}

/// One go: `None` when the reader was left behind and the upgrade went
/// ahead, what the jar said when it refused the upgrade (the park the
/// scenario needs never came), and a panic for anything else.
fn trickling_reader() -> Option<String> {
    use spyc_jar::proto::{self, VERSION};
    let mut jar = Jar::start(
        &["--deadline-ms", "60000"],
        &sh("yes aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | head -c 2000000; exec cat"),
    );
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_while_it_comes(&mut owner, true, |s| s.text.len() >= 2_000_000);
    // A reader replaying the whole log, reading none of it: its writer blocks
    // in the middle of that batch, and the answers to its pings queue in its
    // outbox meanwhile. It asks for the upgrade after its pings, so every
    // answer is queued before the upgrade begins.
    let mut sock = UnixStream::connect(&jar.socket).expect("connect");
    proto::write_msg(
        &mut sock,
        &Msg::Hello {
            version: VERSION,
            role: Role::Reader,
            token: TOKEN.to_vec(),
        },
    )
    .expect("hello");
    assert!(
        matches!(proto::read_msg(&mut sock), Ok(Some(Msg::Welcome { .. }))),
        "premise: the reader is welcomed"
    );
    proto::write_msg(&mut sock, &Msg::Replay { from: 0 }).expect("replay");
    let first = jar.dir.path().join("first-run-paid");
    let next = jar_script(
        jar.dir.path(),
        "jar-next",
        &format!(
            "validate-handover) [ -e {first} ] || touch {first};;",
            first = first.display()
        ),
    );
    let mut frames: Vec<u8> = (0..20_000u64)
        .flat_map(|n| proto::encode(&Msg::Ping { n }))
        .collect();
    frames.extend(proto::encode(&Msg::Upgrade {
        binary: next.display().to_string(),
    }));
    (&sock).write_all(&frames).expect("pings, then the upgrade");
    let stop = Arc::new(AtomicBool::new(false));
    let trickle = {
        let stop = Arc::clone(&stop);
        let socket = jar.socket.clone();
        std::thread::spawn(move || -> Trickled {
            // Once the jar has validated the new binary the first time and
            // begun to park (its acceptor parks too, so a lookup goes
            // unanswered): read the rest of the log's batch, which lets the
            // writer park with its outbox full, until an answer to a ping
            // after the log shows the flush has begun; then take 4 KiB every
            // 0.9 s.
            let mut sock = sock;
            let mut taken = Vec::new();
            let mut some = [0u8; 4096];
            let until = Instant::now() + UPGRADE_AT_MOST;
            let mut parked = true;
            while !first.exists() || !parking(&socket) {
                if Instant::now() > until {
                    parked = false;
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if parked {
                let mut logged = 0;
                loop {
                    match proto::read_msg(&mut sock) {
                        Ok(Some(Msg::Pong { .. })) if logged >= 2_000_000 => break,
                        Ok(Some(Msg::Output { bytes, .. })) => logged += bytes.len(),
                        Ok(Some(Msg::Refused { why, .. })) => return Trickled::Refused(why),
                        Ok(Some(m @ Msg::Upgraded { .. })) => {
                            return Trickled::Broke(format!("{m:?} before the flush began"));
                        }
                        Ok(Some(_)) => {}
                        Ok(None) | Err(_) => {
                            return Trickled::Broke(format!(
                                "the connection ended after {logged} bytes of the log"
                            ));
                        }
                    }
                }
                while !stop.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(900));
                    if let Ok(n) = (&sock).read(&mut some) {
                        taken.extend_from_slice(&some[..n]);
                    }
                }
            } else {
                while !stop.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            // The upgrade is answered, or isn't coming: now read all that's
            // left at once. A reader the jar left behind reaches the end of
            // its connection; one it carried stays open, and goes quiet once
            // it has its `upgraded`; a refusal is in what it read.
            let _ = sock.set_read_timeout(Some(WAIT));
            let ended = loop {
                match (&sock).read(&mut some) {
                    Ok(0) => break true,
                    Ok(n) => taken.extend_from_slice(&some[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        break false;
                    }
                    Err(_) => break true,
                }
            };
            match (refusal_in(&taken), ended) {
                (Some(why), _) => Trickled::Refused(why),
                (None, true) => Trickled::LeftBehind,
                (None, false) => Trickled::Carried,
            }
        })
    };
    let until = Instant::now() + UPGRADE_AT_MOST;
    let answer = loop {
        if Instant::now() >= until {
            break None;
        }
        match owner.recv(Duration::from_millis(200)) {
            Ok(Some(m @ (Msg::Upgraded { .. } | Msg::Refused { .. }))) => break Some(m),
            Ok(Some(m)) => {
                seen.take(m);
                if let Some(next) = seen.next_offset {
                    let _ = owner.send(&Msg::Ack { offset: next });
                }
            }
            Ok(None) => {}
            Err(e) => panic!("the owner's connection ended: {e:#}"),
        }
    };
    stop.store(true, Ordering::SeqCst);
    let trickled = trickle.join().expect("the trickle");
    if answer.is_none()
        && let Trickled::Refused(why) = &trickled
        && why.contains("safe point")
    {
        return Some(why.clone());
    }
    if let Trickled::Broke(why) = &trickled {
        panic!("the trickling reader: {why}; the upgrade: {answer:?}");
    }
    assert!(
        matches!(answer, Some(Msg::Upgraded { .. })),
        "the upgrade goes ahead without the trickling reader: {answer:?}; the reader: {trickled:?}"
    );
    assert!(
        matches!(trickled, Trickled::LeftBehind),
        "the jar leaves the trickling reader behind: {trickled:?}"
    );
    None
}

/// Whether the jar at `socket` has parked for a handover: a lookup's `hello`
/// goes unanswered for 300 ms, its acceptor being parked with everything
/// else. A guess, which a test that acts on it must check afterwards by what
/// the jar did: the jar reports no park, and a reader must begin within the
/// jar's own 3 s park deadline.
pub fn parking(socket: &std::path::Path) -> bool {
    let Ok(mut s) = UnixStream::connect(socket) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
    if s.write_all(&LOOKUP_HELLO).is_err() {
        return false;
    }
    spyc_jar::proto::read_msg(&mut s).is_err()
}

// Decision M: a pane's jar starts its program when its first owner's `hello`
// is accepted (the start's hold), so a program's start-up probes are
// answered by an owner that is there, never lost before one connects.

/// Until its first owner, a jar whose start has a hold runs nothing
/// and says so (`program_pid` 0); the owner's welcome carries the program it
/// started, and the program's first output is there to replay.
#[test]
fn a_jar_started_on_owner_runs_its_program_once_the_owner_is_there() {
    let dir = socket_dir();
    let mark = dir.path().join("started");
    let mut jar = Jar::holding(
        10000,
        &[],
        &sh(&format!(
            "touch {}; printf first-words; exec cat",
            mark.display()
        )),
    );
    // A jar answers a lookup only once it serves, which is after it would
    // have started a program it doesn't hold: `program_pid` is the fact.
    eventually("the jar's socket", || jar.socket.exists());
    assert_eq!(
        described(&jar, "program_pid"),
        "0",
        "no program before an owner"
    );
    assert!(!mark.exists(), "the program started before an owner");

    let (mut owner, welcome) = jar.welcome(Role::Owner);
    let Msg::Welcome { child_pid, .. } = welcome else {
        panic!("{welcome:?}");
    };
    assert_ne!(child_pid, 0, "the welcome carries the program it started");
    assert_eq!(described(&jar, "program_pid"), child_pid.to_string());
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    read_until(&mut owner, true, |s| s.contains("first-words"));
    assert!(mark.exists());
}

/// No owner within the wait: the jar ends, having started nothing.
#[test]
fn a_jar_no_owner_comes_for_ends_having_started_nothing() {
    let dir = socket_dir();
    let mark = dir.path().join("started");
    let mut jar = Jar::holding(
        400,
        &[],
        &sh(&format!("touch {}; exec cat", mark.display())),
    );
    assert!(jar.ended_within(WAIT), "the jar waited on for an owner");
    assert!(!mark.exists(), "the program started with no owner");
    assert!(!jar.socket.exists(), "its socket was left behind");
}

/// The blob always has a program: an upgrade asked before the program has
/// started is refused as busy (reason 5), and asking starts nothing.
#[test]
fn an_upgrade_before_the_program_starts_is_refused() {
    let dir = socket_dir();
    let mark = dir.path().join("started");
    let mut jar = Jar::holding(
        10000,
        &[],
        &sh(&format!("touch {}; exec cat", mark.display())),
    );
    let mut reader = jar.connect(Role::Reader);
    let next = next_binary(jar.dir.path());
    reader
        .send(&Msg::Upgrade {
            binary: next.display().to_string(),
        })
        .expect("upgrade");
    let m = recv_matching_within(&mut reader, UPGRADE_AT_MOST, |m| {
        matches!(m, Msg::Refused { .. } | Msg::Upgraded { .. })
    });
    assert!(
        matches!(
            m,
            Msg::Refused {
                reason: spyc_jar::proto::reason::UPGRADE_BUSY,
                ..
            }
        ),
        "{m:?}"
    );
    assert!(!mark.exists(), "a reader started the program");
    assert_eq!(described(&jar, "program_pid"), "0");
}
