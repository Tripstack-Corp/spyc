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
fn jar_script(dir: &std::path::Path, name: &str, cases: &str) -> PathBuf {
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
/// or `Refused`) and how long it took, acknowledging output meanwhile. `None`
/// when the jar answers neither within `wait`, or the connection ends.
fn upgrade_answer(
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
    while at.elapsed() < wait {
        match owner.recv(Duration::from_millis(200)) {
            Ok(Some(m @ (Msg::Upgraded { .. } | Msg::Refused { .. }))) => {
                return (Some(m), at.elapsed());
            }
            Ok(Some(m)) => {
                seen.take(m);
                if let Some(next) = seen.next_offset {
                    let _ = owner.send(&Msg::Ack { offset: next });
                }
            }
            Ok(None) => {}
            Err(_) => return (None, at.elapsed()),
        }
    }
    (None, at.elapsed())
}

/// The program still counts and reads input, on `owner`'s connection or, if
/// that one ended, a new owner's.
fn assert_program_survives(jar: &mut Jar, owner: Client, what: &str) {
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
    let mut seen = read_until(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let fake = jar_script(
        jar.dir.path(),
        "jar-fake",
        "validate-handover) cat >/dev/null; exit 0;;\nresume-handover) exit 1;;",
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &fake, WAIT);
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
    let mut seen = read_until(&mut owner, true, |s| {
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
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &named, WAIT);
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
    let mut seen = read_until(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let silent = jar_script(
        jar.dir.path(),
        "jar-silent",
        "validate-handover) cat >/dev/null; exit 0;;",
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &silent, WAIT);
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
/// refusal still comes within the validator's deadline, and the program isn't
/// held up meanwhile. Today the jar reads that standard error to its end,
/// past the deadline, with every thread parked.
#[test]
fn a_validators_child_holding_its_stderr_cannot_hold_the_jar() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_until(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let holding = jar_script(
        jar.dir.path(),
        "jar-holding",
        "validate-handover) sleep 12 & exit 1;;",
    );
    let (answer, took) = upgrade_answer(&mut owner, &mut seen, &holding, Duration::from_secs(20));
    assert!(matches!(answer, Some(Msg::Refused { .. })), "{answer:?}");
    assert!(
        took < Duration::from_secs(7),
        "the refusal waited on the validator's child: {took:?}"
    );
    assert_program_survives(&mut jar, owner, "a held standard error");
}

/// A validator that outruns its deadline is ended with everything it started:
/// its whole process group, not only the direct child.
#[test]
fn a_validator_past_its_deadline_is_ended_with_its_children() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_until(&mut owner, true, |s| {
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
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &slow, Duration::from_secs(20));
    assert!(matches!(answer, Some(Msg::Refused { .. })), "{answer:?}");
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
    let mut seen = read_until(&mut owner, true, |s| {
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
        let (answer, _) = upgrade_answer(&mut owner, &mut seen, &liar, WAIT);
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
    let mut seen = read_until(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let (answer, _) = upgrade_answer(
        &mut owner,
        &mut seen,
        std::path::Path::new("relative/spyc-jar"),
        WAIT,
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
    let mut seen = read_until(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let told = jar.dir.path().join("previous");
    let telling = jar_script(
        jar.dir.path(),
        "jar-telling",
        &format!("resume-handover) printf %s \"$2\" > {};;", told.display()),
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &telling, WAIT);
    assert!(matches!(answer, Some(Msg::Upgraded { .. })), "{answer:?}");
    assert_eq!(
        std::fs::read_to_string(&told).expect("the new image was told"),
        env!("CARGO_BIN_EXE_spyc-jar")
    );
    assert_program_survives(&mut jar, owner, "an upgrade");
}

/// A new binary's first run is slow (about 263 ms on macOS for a new file):
/// the jar pays it while it still serves the program, validating once before
/// anything parks, and again, parked, on the real blob (decision L, part 7).
#[test]
fn a_slow_first_validation_never_stalls_the_program() {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_until(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    let first = jar.dir.path().join("first-run-paid");
    let slow_once = jar_script(
        jar.dir.path(),
        "jar-slow-once",
        &format!(
            "validate-handover) [ -e {first} ] || {{ sleep 2; touch {first}; }};;",
            first = first.display()
        ),
    );
    owner
        .send(&Msg::Upgrade {
            binary: slow_once.display().to_string(),
        })
        .expect("upgrade");
    let mut last_output = Instant::now();
    let mut longest = Duration::ZERO;
    let until = Instant::now() + WAIT;
    let answer = loop {
        assert!(Instant::now() < until, "no answer to the upgrade");
        match owner.recv(Duration::from_millis(50)).expect("recv") {
            Some(m @ (Msg::Upgraded { .. } | Msg::Refused { .. })) => break m,
            Some(m @ Msg::Output { .. }) => {
                longest = longest.max(last_output.elapsed());
                last_output = Instant::now();
                seen.take(m);
                if let Some(next) = seen.next_offset {
                    owner.send(&Msg::Ack { offset: next }).expect("ack");
                }
            }
            Some(m) => seen.take(m),
            None => {}
        }
    };
    assert!(matches!(answer, Msg::Upgraded { .. }), "{answer:?}");
    // The silence ends with the next output, which the new image sends after
    // its notice.
    loop {
        assert!(Instant::now() < until, "no output after the upgrade");
        if let Some(m) = owner.recv(Duration::from_millis(50)).expect("recv")
            && let Msg::Output { offset, bytes } = m
        {
            longest = longest.max(last_output.elapsed());
            owner
                .send(&Msg::Ack {
                    offset: offset + bytes.len() as u64,
                })
                .expect("ack");
            break;
        }
    }
    assert!(
        longest < Duration::from_secs(1),
        "the program went quiet for {longest:?} while the new binary's first run was paid"
    );
    assert_program_survives(&mut jar, owner, "a slow first validation");
}

/// A reader that takes a little every 0.9 s while the jar flushes its last
/// frames before a handover gets one deadline for the whole flush, not one
/// for each write or each wait: it can't keep the jar parked. It takes 4 KiB
/// at a time, which makes the socket writable each time (a byte at a time
/// never does on macOS, which wakes a writer only once 2 KiB are free).
#[test]
fn a_reader_trickling_during_the_flush_cannot_keep_the_jar_parked() {
    use spyc_jar::proto::{self, VERSION};
    let mut jar = Jar::start(
        &["--deadline-ms", "60000"],
        &sh("yes aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | head -c 2000000; exec cat"),
    );
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_until(&mut owner, true, |s| s.text.len() >= 2_000_000);
    // A reader replaying the whole log, reading none of it: its writer blocks
    // in the middle of that batch, and the answers to its pings queue in its
    // outbox meanwhile.
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
    let pings: Vec<u8> = (0..20_000u64)
        .flat_map(|n| proto::encode(&Msg::Ping { n }))
        .collect();
    (&sock).write_all(&pings).expect("pings");
    std::thread::sleep(Duration::from_millis(300));
    let stop = Arc::new(AtomicBool::new(false));
    let first = jar.dir.path().join("first-run-paid");
    let trickle = {
        let stop = Arc::clone(&stop);
        let (first, socket) = (first.clone(), jar.socket.clone());
        std::thread::spawn(move || {
            // Once the jar has validated the new binary the first time and
            // begun to park (its acceptor parks too, so a lookup goes
            // unanswered): read the rest of the log's batch, which lets the
            // writer park with its outbox full, until the first answer to a
            // ping shows the flush has begun; then take 4 KiB every 0.9 s.
            let until = Instant::now() + WAIT;
            while !first.exists() || !parking(&socket) {
                if Instant::now() > until {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let mut sock = sock;
            let mut flushing = false;
            while !flushing {
                match proto::read_msg(&mut sock) {
                    Ok(Some(Msg::Pong { .. })) => flushing = true,
                    Ok(_) => {}
                    Err(_) => return false,
                }
            }
            let mut some = [0u8; 4096];
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(900));
                let _ = (&sock).read(&mut some);
            }
            true
        })
    };
    let next = jar_script(
        jar.dir.path(),
        "jar-next",
        &format!(
            "validate-handover) [ -e {first} ] || {{ sleep 1; touch {first}; }};;",
            first = first.display()
        ),
    );
    let (answer, took) = upgrade_answer(&mut owner, &mut seen, &next, Duration::from_secs(20));
    stop.store(true, Ordering::SeqCst);
    assert!(
        trickle.join().expect("the trickle"),
        "premise: the reader saw the flush begin"
    );
    assert!(
        matches!(answer, Some(Msg::Upgraded { .. })),
        "the upgrade goes ahead without the trickling reader, in {took:?}: {answer:?}"
    );
    assert!(
        took < Duration::from_secs(8),
        "the trickling reader held the jar parked for {took:?}"
    );
}

/// Whether the jar at `socket` has parked for a handover: a lookup's `hello`
/// goes unanswered, its acceptor being parked with everything else.
fn parking(socket: &std::path::Path) -> bool {
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
// is accepted (`--start-on-owner MS`), so a program's start-up probes are
// answered by an owner that is there, never lost before one connects.

/// Until its first owner, a jar started with `--start-on-owner` runs nothing
/// and says so (`program_pid` 0); the owner's welcome carries the program it
/// started, and the program's first output is there to replay.
#[test]
fn a_jar_started_on_owner_runs_its_program_once_the_owner_is_there() {
    let dir = socket_dir();
    let mark = dir.path().join("started");
    let mut jar = Jar::start(
        &["--start-on-owner", "10000"],
        &sh(&format!(
            "touch {}; printf first-words; exec cat",
            mark.display()
        )),
    );
    eventually("the jar's socket", || jar.socket.exists());
    std::thread::sleep(Duration::from_millis(300));
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
    let mut jar = Jar::start(
        &["--start-on-owner", "400"],
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
    let mut jar = Jar::start(
        &["--start-on-owner", "10000"],
        &sh(&format!("touch {}; exec cat", mark.display())),
    );
    let mut reader = jar.connect(Role::Reader);
    let next = next_binary(jar.dir.path());
    reader
        .send(&Msg::Upgrade {
            binary: next.display().to_string(),
        })
        .expect("upgrade");
    let m = recv_matching(&mut reader, |m| {
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
