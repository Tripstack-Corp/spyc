//! Each of a jar's upgrade timers sized by what its expiry costs (the
//! owner's decision of 2026-11-15, before `spyc-jar` 0.1.0 is published): a
//! jar starved by load defers an upgrade, and never loses its program, a
//! connection or a byte, nor reports a good binary as rejected. Slowness is
//! injected, never measured: a wrapper binary stalls at a chosen step of a
//! chosen call, or a client stops reading.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use spyc_jar::client::Client;
use spyc_jar::proto::{self, Msg, Role, reason};

use super::upgrade::{
    assert_program_survives, count_on, first_validation_ended, held_until, jar_script,
    upgrade_answer,
};
use super::*;

/// A shell step that runs `then` on the `n`th time it runs, counting in
/// `counter`.
pub fn on_call(counter: &Path, n: u32, then: &str) -> String {
    format!(
        "c=$(cat {f} 2>/dev/null || echo 0); c=$((c+1)); echo $c > {f}; [ $c -ne {n} ] || {{ {then}; }}",
        f = counter.display()
    )
}

/// A jar running [`counter`], its owner having seen the count start.
pub fn counting() -> (Jar, Client, Seen) {
    let mut jar = Jar::start(&[], &counter());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let seen = read_while_it_comes(&mut owner, true, |s| {
        counts(s).last().is_some_and(|n| *n >= 5)
    });
    (jar, owner, seen)
}

/// The answer to an upgrade `owner` has asked for, acknowledging output
/// meanwhile, and how long it took; `None` when none comes within `wait`.
fn answer_to_the_upgrade(
    owner: &mut Client,
    seen: &mut Seen,
    wait: Duration,
) -> (Option<Msg>, Duration) {
    let at = Instant::now();
    if let Some(m) = seen
        .frames
        .iter()
        .find(|f| matches!(f, Msg::Upgraded { .. } | Msg::Refused { .. }))
    {
        return (Some(m.clone()), at.elapsed());
    }
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

fn reason_of(answer: Option<&Msg>) -> Option<u16> {
    match answer {
        Some(Msg::Refused { reason, .. }) => Some(*reason),
        _ => None,
    }
}

/// The pid a wrapper step wrote to `path`.
fn pid_in(path: &Path) -> u32 {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .trim()
        .parse()
        .expect("a pid")
}

/// Wait up to 2 s for `pid` to be gone; kill it if it isn't, and say so.
fn gone_soon(pid: u32) -> bool {
    let until = Instant::now() + Duration::from_secs(2);
    while running(pid) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    let gone = !running(pid);
    if !gone
        && let Some(p) = i32::try_from(pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
    {
        let _ = rustix::process::kill_process(p, rustix::process::Signal::KILL);
    }
    gone
}

/// The descriptors process `pid` has open.
fn open_fds(pid: u32) -> usize {
    if cfg!(target_os = "linux") {
        std::fs::read_dir(format!("/proc/{pid}/fd")).map_or(0, Iterator::count)
    } else {
        let out = Command::new("lsof")
            .args(["-n", "-P", "-p", &pid.to_string()])
            .output()
            .expect("lsof");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .skip(1)
            .filter(|l| {
                l.split_whitespace()
                    .nth(3)
                    .is_some_and(|fd| fd.starts_with(|c: char| c.is_ascii_digit()))
            })
            .count()
    }
}

/// (1) A new binary's first run that takes longer than a parked validation
/// may (a cold file under load, a jar with no store) is paid with the
/// program still served: the upgrade goes ahead, and the program counts on
/// while that first run is past a parked validation's 5 s and still going.
#[test]
fn a_first_run_slower_than_a_parked_validation_still_upgrades() {
    let (mut jar, mut owner, mut seen) = counting();
    let calls = jar.dir.path().join("validations");
    let slept = jar.dir.path().join("first-run-slept");
    let go = jar.dir.path().join("go");
    let slow_first = jar_script(
        jar.dir.path(),
        "jar-slow-first",
        &format!(
            "validate-handover) {};;",
            on_call(
                &calls,
                1,
                &format!("sleep 6; touch {}; {}", slept.display(), held_until(&go))
            )
        ),
    );
    owner
        .send(&Msg::Upgrade {
            binary: slow_first.display().to_string(),
        })
        .expect("upgrade");
    let until = Instant::now() + Duration::from_secs(40);
    while !slept.exists() {
        assert!(Instant::now() < until, "the first run never got past 6 s");
        count_on(&mut owner, &mut seen, 1, "while the first run is paid");
    }
    count_on(&mut owner, &mut seen, 20, "with the first run past 6 s");
    std::fs::write(&go, "").expect("let the first run end");
    let (answer, _) = answer_to_the_upgrade(&mut owner, &mut seen, UPGRADE_AT_MOST);
    assert!(
        matches!(answer, Some(Msg::Upgraded { .. })),
        "a good binary, only slow to start, takes over: {answer:?}"
    );
    assert_unbroken(&seen);
    assert_program_survives(&mut jar, owner, "a slow first run");
}

/// (2) A parked validation that outlasts its limit refuses the upgrade as
/// busy (worth asking again), not rejected: the binary is good, the jar
/// only couldn't wait for it parked. The refusal comes while the stalled
/// validator would still be running (it never ends by itself), the program
/// counts on after it with nothing lost or doubled, and the next attempt
/// goes through.
#[test]
fn a_parked_validation_past_its_limit_is_busy_and_a_retry_succeeds() {
    let (mut jar, mut owner, mut seen) = counting();
    let calls = jar.dir.path().join("validations");
    let go = jar.dir.path().join("go");
    let slow_parked = jar_script(
        jar.dir.path(),
        "jar-slow-parked",
        &format!(
            "validate-handover) {};;",
            on_call(&calls, 2, &held_until(&go))
        ),
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &slow_parked, Duration::from_secs(60));
    assert_eq!(
        reason_of(answer.as_ref()),
        Some(reason::UPGRADE_BUSY),
        "a stalled parked validation is busy: {answer:?}"
    );
    count_on(&mut owner, &mut seen, 20, "after the busy refusal");
    std::fs::write(&go, "").expect("let a stalled validator go");
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &slow_parked, Duration::from_secs(60));
    assert!(
        matches!(answer, Some(Msg::Upgraded { .. })),
        "asked again, the upgrade goes through: {answer:?}"
    );
    assert_unbroken(&seen);
    assert_program_survives(&mut jar, owner, "a stalled parked validation");
}

/// (3) A parked validator that never exits: busy, its whole process group
/// ended, and the jar serves on.
#[test]
fn a_parked_validator_that_never_answers_is_busy_and_ended_with_its_group() {
    let (mut jar, mut owner, mut seen) = counting();
    let calls = jar.dir.path().join("validations");
    let pidfile = jar.dir.path().join("validator-child.pid");
    let hung = jar_script(
        jar.dir.path(),
        "jar-hung",
        &format!(
            "validate-handover) {};;",
            on_call(
                &calls,
                2,
                &format!("sleep 300 & echo $! > {}; sleep 300", pidfile.display())
            )
        ),
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &hung, Duration::from_secs(60));
    assert_eq!(
        reason_of(answer.as_ref()),
        Some(reason::UPGRADE_BUSY),
        "a hung parked validator is busy: {answer:?}"
    );
    assert!(
        gone_soon(pid_in(&pidfile)),
        "the validator's child outlived its deadline"
    );
    assert_program_survives(&mut jar, owner, "a hung parked validator");
}

/// (4) Positive evidence still rejects, on that evidence and no deadline: a
/// validator that exits non-zero, one that proves another blob, one that
/// exits 0 and proves nothing.
#[test]
fn a_binary_that_refuses_is_rejected_for_what_it_did() {
    let (mut jar, mut owner, mut seen) = counting();
    let dir = jar.dir.path().to_path_buf();
    for (name, step) in [
        ("jar-exits-3", "exit 3"),
        (
            "jar-wrong-proof",
            "cat >/dev/null; echo spyc-jar-handover-ok 0000000000000000 1; exit 0",
        ),
        ("jar-no-proof", "cat >/dev/null; exit 0"),
    ] {
        let binary = jar_script(&dir, name, &format!("validate-handover) {step};;"));
        let (answer, _) = upgrade_answer(&mut owner, &mut seen, &binary, UPGRADE_AT_MOST);
        let Some(Msg::Refused { reason, why }) = &answer else {
            panic!("{name}: refused: {answer:?}");
        };
        assert_eq!(*reason, reason::UPGRADE_REJECTED, "{name}: rejected: {why}");
        assert!(
            why.contains("rejected the handover") || why.contains("without proving"),
            "{name}: rejected for what the binary did, not for a deadline: {why}"
        );
    }
    assert_program_survives(&mut jar, owner, "three refusals");
}

/// (5) An owner that can't take its last frames before the handover (a
/// spyc starved by load) keeps its connection: the upgrade is refused as
/// busy rather than going ahead without it, and every frame still comes.
///
/// The owner's thread guesses the park from a lookup's silence and must read
/// within the jar's own 3 s park deadline; when it doesn't, the jar says so
/// (busy: a thread didn't reach a safe point), the scenario never came
/// about, and it is set up again, at most three times.
#[test]
fn an_owner_that_cant_take_its_last_frames_keeps_its_connection() {
    let mut missed = Vec::new();
    for _ in 0..3 {
        match owner_cant_flush() {
            None => return,
            Some(why) => missed.push(why),
        }
    }
    panic!("the owner never held its last frames through a park: {missed:?}");
}

/// One go at (5): `None` when the jar kept the owner and refused the
/// upgrade for its last frames, the reason the jar gave when the park the
/// scenario needs never came, and a panic for anything else.
fn owner_cant_flush() -> Option<String> {
    let mut jar = Jar::start(
        &["--deadline-ms", "60000"],
        &sh("yes aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | head -c 2000000; exec cat"),
    );
    // The owner: replays the whole log a second time and queues answers to
    // its pings behind it, then reads only the log's batch once the jar
    // parks, so its writer parks with the answers waiting, and reads nothing
    // more until the upgrade has been answered.
    // Its socket's file exists from its `bind`, a moment before it listens:
    // wait for a connection it takes.
    let mut sock = None;
    eventually("the jar to take a connection", || {
        sock = UnixStream::connect(&jar.socket).ok();
        sock.is_some()
    });
    let mut sock = sock.expect("connected");
    proto::write_msg(
        &mut sock,
        &Msg::Hello {
            version: proto::VERSION,
            role: Role::Owner,
            token: TOKEN.to_vec(),
        },
    )
    .expect("hello");
    assert!(
        matches!(proto::read_msg(&mut sock), Ok(Some(Msg::Welcome { .. }))),
        "premise: the owner is welcomed"
    );
    proto::write_msg(&mut sock, &Msg::Replay { from: 0 }).expect("replay");
    // All of it first, acknowledged, so the program has printed everything
    // and nothing paces it; then the whole log again, unread.
    let mut head = 0;
    while head < 2_000_000 {
        if let Ok(Some(Msg::Output { offset, bytes })) = proto::read_msg(&mut sock) {
            head = offset + bytes.len() as u64;
            proto::write_msg(&mut sock, &Msg::Ack { offset: head }).expect("ack");
        }
    }
    proto::write_msg(&mut sock, &Msg::Replay { from: 0 }).expect("replay");
    let mut requester = jar.connect(Role::Reader);
    let mut seen = Seen::default();
    let pings: Vec<u8> = (0..20_000u64)
        .flat_map(|n| proto::encode(&Msg::Ping { n }))
        .collect();
    (&sock).write_all(&pings).expect("pings");
    // The jar reads a connection's frames in order: once it has stored what
    // the owner set after its pings, every ping's answer is queued.
    proto::write_msg(
        &mut sock,
        &Msg::SetInfo {
            key: "pings".into(),
            value: "read".into(),
        },
    )
    .expect("set info");
    eventually("the jar to read every ping", || {
        described(&jar, "pings") == "read"
    });
    let answered = Arc::new(AtomicBool::new(false));
    let first = jar.dir.path().join("first-run-paid");
    let owner = {
        let answered = Arc::clone(&answered);
        let first = first.clone();
        std::thread::spawn(move || {
            let until = Instant::now() + UPGRADE_AT_MOST;
            while !first_validation_ended(&first) {
                if Instant::now() > until {
                    return Err("the jar never parked".to_string());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let mut sock = sock;
            loop {
                match proto::read_msg(&mut sock) {
                    Ok(Some(Msg::Pong { .. })) => break,
                    Ok(_) => {}
                    Err(e) => return Err(format!("the owner's connection ended: {e:?}")),
                }
            }
            while !answered.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(50));
            }
            // Now it reads again: every answer still comes, and the
            // connection lives.
            let _ = sock.set_read_timeout(Some(WAIT));
            loop {
                match proto::read_msg(&mut sock) {
                    Ok(Some(Msg::Pong { n: 19_999 })) => return Ok(()),
                    Ok(_) => {}
                    Err(e) => return Err(format!("the owner's connection ended: {e:?}")),
                }
            }
        })
    };
    let next = jar_script(
        jar.dir.path(),
        "jar-next",
        &format!(
            "validate-handover) [ -e {first} ] || {{ sleep 1; echo $$ > {first}; }};;",
            first = first.display()
        ),
    );
    let (answer, _) = upgrade_answer(&mut requester, &mut seen, &next, UPGRADE_AT_MOST);
    answered.store(true, Ordering::SeqCst);
    let kept = owner.join().expect("the owner");
    if let Some(Msg::Refused { why, .. }) = &answer
        && why.contains("safe point")
    {
        return Some(why.clone());
    }
    assert_eq!(
        reason_of(answer.as_ref()),
        Some(reason::UPGRADE_BUSY),
        "an owner that can't flush makes the upgrade busy: {answer:?}; the owner: {kept:?}"
    );
    // Busy for the flush, as the jar says.
    assert!(
        matches!(&answer, Some(Msg::Refused { why, .. }) if why.contains("last frames")),
        "busy because the owner didn't take its last frames: {answer:?}"
    );
    kept.unwrap_or_else(|e| panic!("{e}"));
    None
}

/// (6) A holder slow to say it has the sockets: busy, the holder ended with
/// everything it started (which would otherwise keep the jar's sockets in
/// flight), no descriptor left open in the jar, and the next upgrade works.
#[test]
fn a_slow_holder_is_busy_leaks_nothing_and_the_next_upgrade_works() {
    let (mut jar, mut owner, mut seen) = counting();
    let calls = jar.dir.path().join("holds");
    let pidfile = jar.dir.path().join("holder-child.pid");
    let slow_holder = jar_script(
        jar.dir.path(),
        "jar-slow-holder",
        &format!(
            "hold-handover) {};;",
            on_call(
                &calls,
                1,
                &format!("sleep 300 & echo $! > {}; wait", pidfile.display())
            )
        ),
    );
    let jar_pid = jar.process.id();
    let before = open_fds(jar_pid);
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &slow_holder, Duration::from_secs(60));
    assert_eq!(
        reason_of(answer.as_ref()),
        Some(reason::UPGRADE_BUSY),
        "a holder that doesn't answer in time is busy: {answer:?}"
    );
    assert!(
        gone_soon(pid_in(&pidfile)),
        "the holder's child outlived it, holding the jar's sockets in flight"
    );
    let after = open_fds(jar_pid);
    assert!(
        after <= before,
        "the jar has {after} descriptors open after the failed hold, {before} before"
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &slow_holder, Duration::from_secs(60));
    assert!(
        matches!(answer, Some(Msg::Upgraded { .. })),
        "the next upgrade goes through: {answer:?}"
    );
    assert_program_survives(&mut jar, owner, "a slow holder");
}

/// A holder that never reads the blob, where the blob is far bigger than a
/// socket's buffer: taking it and answering share one deadline, so the jar
/// isn't held parked by the write, and the upgrade is busy.
#[test]
fn a_holder_that_never_takes_the_blob_cannot_hold_the_jar_parked() {
    let mut jar = Jar::start(
        &[],
        &sh(
            "yes aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | head -c 1000000; while read l; do echo got=$l; done",
        ),
    );
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_while_it_comes(&mut owner, true, |s| s.text.len() >= 1_000_000);
    let deaf = jar_script(
        jar.dir.path(),
        "jar-deaf-holder",
        &format!(
            "hold-handover) {};;",
            held_until(&jar.dir.path().join("go"))
        ),
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &deaf, Duration::from_secs(60));
    assert_eq!(
        reason_of(answer.as_ref()),
        Some(reason::UPGRADE_BUSY),
        "a holder that doesn't take the blob is busy: {answer:?}"
    );
    assert_program_survives(&mut jar, owner, "a deaf holder");
}

/// (7) A new image that takes longer to take its descriptors than the
/// holder's old 30 s: the holder keeps every socket until the new image has
/// them, so the owner's connection and the listener survive, and so does
/// the program. A holder that let go on a clock would leave them only in
/// flight, and macOS flushes those.
#[test]
fn a_new_image_slower_than_the_holders_old_limit_keeps_every_connection() {
    let (mut jar, mut owner, mut seen) = counting();
    let calls = jar.dir.path().join("resumes");
    let slow_resume = jar_script(
        jar.dir.path(),
        "jar-slow-resume",
        &format!("resume-handover) {};;", on_call(&calls, 1, "sleep 35")),
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &slow_resume, Duration::from_secs(60));
    assert!(
        matches!(answer, Some(Msg::Upgraded { .. })),
        "the owner's connection crosses the slow resume: {answer:?}"
    );
    owner.send(&Msg::Ping { n: 3 }).expect("ping");
    recv_matching(&mut owner, |m| matches!(m, Msg::Pong { n: 3 }));
    let _ = jar.connect(Role::Reader);
    assert_program_survives(&mut jar, owner, "a slow resume");
}

/// A holder ends with the jar's process: a new image that dies before
/// taking its descriptors leaves nothing to hold them for, and the holder
/// mustn't keep them, and itself, for its whole backstop.
#[test]
fn a_holder_ends_when_the_jar_does() {
    let (mut jar, mut owner, mut seen) = counting();
    let holder_pid = jar.dir.path().join("holder.pid");
    let resuming = jar.dir.path().join("resuming");
    let stalled = jar_script(
        jar.dir.path(),
        "jar-stalled-resume",
        &format!(
            "hold-handover) echo $$ > {holder};;\nresume-handover) touch {resuming}; exec sleep 300;;",
            holder = holder_pid.display(),
            resuming = resuming.display()
        ),
    );
    owner
        .send(&Msg::Upgrade {
            binary: stalled.display().to_string(),
        })
        .expect("upgrade");
    // Read on meanwhile, as an owner does: one that stops reading can't let
    // the jar park.
    let until = Instant::now() + Duration::from_secs(60);
    while !resuming.exists() {
        assert!(Instant::now() < until, "the new image never started");
        if let Ok(Some(m)) = owner.recv(Duration::from_millis(50)) {
            assert!(
                !matches!(m, Msg::Refused { .. }),
                "premise: the upgrade goes ahead: {m:?}"
            );
            seen.take(m);
            if let Some(next) = seen.next_offset {
                let _ = owner.send(&Msg::Ack { offset: next });
            }
        }
    }
    let holder = pid_in(&holder_pid);
    assert!(running(holder), "premise: the holder holds");
    jar.process.kill().expect("kill the jar");
    let _ = jar.process.wait();
    assert!(gone_soon(holder), "the holder outlived the jar");
}
