//! The jar's ownership timers, each asked what a starved but live peer sees
//! when it fires: an owner that is slow but there keeps its lease, and a jar
//! waiting for its first owner waits without spending the machine's time.
//! Slowness is injected (a client that pauses mid-frame or stops reading,
//! while it pings), never measured.

use std::time::{Duration, Instant};

use spyc_jar::proto::{self, Msg, Role};

use super::*;

/// A jar holding its program for its first owner, with a finite linger,
/// waits past the first-owner grace without spinning, and the owner that
/// comes inside its start wait is still given the program.
#[test]
fn a_jar_holding_its_program_for_a_first_owner_waits_idle_past_the_grace() {
    let mut jar = Jar::holding(30000, &["--linger-ownerless-ms", "0"], &["/bin/cat"]);
    eventually("the jar's socket", || jar.socket.exists());
    // Past the jar's ten-second first-owner grace, inside its start wait.
    std::thread::sleep(Duration::from_secs(13));
    let spent = cpu_seconds(jar.process.id());
    assert!(
        spent < 1.0,
        "a jar waiting for its first owner spent {spent} s of CPU in 13 s"
    );
    let (_owner, welcome) = jar.welcome(Role::Owner);
    let Msg::Welcome { child_pid, .. } = welcome else {
        panic!("{welcome:?}");
    };
    assert_ne!(
        child_pid, 0,
        "the owner inside the start wait gets the program"
    );
}

/// R21: a lease ends when its connection drops or its owner misses the
/// liveness deadline. An owner that pings on time but has stopped reading
/// (its spyc's pane is busy) keeps the lease, its program paced meanwhile,
/// and reads on with nothing lost.
#[test]
fn an_owner_that_pings_keeps_its_lease_while_it_stops_reading() {
    let deadline = Duration::from_millis(1000);
    let mut jar = Jar::start(
        &["--deadline-ms", "1000"],
        &sh("i=0; while :; do i=$((i+1)); echo n=$i; done"),
    );
    let owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let (mut rx, tx) = owner.split();
    // Four deadlines of pings and no reads: far more is printed meanwhile
    // than the flow-control window and the socket hold.
    let until = Instant::now() + deadline * 4;
    let mut n = 0;
    while Instant::now() < until {
        n += 1;
        tx.send(&Msg::Ping { n }).unwrap_or_else(|e| {
            panic!("the jar ended a live owner's connection while it pinged: {e}")
        });
        std::thread::sleep(deadline / 5);
    }
    let mut seen = Seen::default();
    let mut heard = Instant::now();
    while counts(&seen).last().is_none_or(|n| *n < 50_000) {
        assert!(
            Instant::now() < heard + WAIT,
            "nothing for {WAIT:?}; saw {} bytes",
            seen.text.len()
        );
        let msg = rx.recv(Duration::from_millis(200));
        if matches!(msg, Ok(Some(_))) {
            heard = Instant::now();
        }
        let msg = msg.unwrap_or_else(|e| {
            panic!(
                "the jar ended a live owner's connection after {} bytes: {e:#}",
                seen.text.len()
            )
        });
        if let Some(msg) = msg {
            seen.take(msg);
            if let Some(next) = seen.next_offset {
                tx.send(&Msg::Ack { offset: next }).expect("ack");
            }
        }
    }
    assert_eq!(seen.first_offset, Some(0), "the stream starts at the start");
    assert_unbroken(&seen);
}

/// An owner paused partway through a frame (a large paste or snapshot, its
/// spyc stopped or starved) is waited for as long as its liveness deadline,
/// not cut sooner by the frame deadline that bounds a tokenless trickle.
#[test]
fn an_owner_paused_mid_frame_inside_its_deadline_keeps_its_lease() {
    let mut jar = Jar::start(&["--deadline-ms", "10000"], &["/bin/cat"]);
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut line = vec![b'a'; 512];
    line.extend(b" paused-mid-frame\n");
    let frame = proto::encode(&Msg::Input { bytes: line });
    let (first, rest) = frame.split_at(frame.len() / 2);
    owner.send_raw(first).expect("the frame's first half");
    // Past the five-second frame deadline, inside the ten-second lease.
    std::thread::sleep(Duration::from_secs(7));
    owner.send_raw(rest).expect("the rest of the frame");
    let seen = read_until(&mut owner, true, |s| s.contains("paused-mid-frame"));
    assert!(seen.contains("paused-mid-frame"));
}

/// A jar in any idle state spends next to no CPU: waiting for its first
/// owner past the first-owner grace, ownerless with no linger, ownerless
/// within a finite linger, and holding an exit nobody has collected.
#[test]
fn a_jar_sits_idle_in_every_idle_state() {
    let waiting = Jar::holding(60000, &["--linger-ownerless-ms", "0"], &["/bin/cat"]);
    let mut ownerless = Jar::start(&[], &["/bin/sleep", "120"]);
    drop(ownerless.connect(Role::Owner));
    let mut lingering = Jar::start(&["--linger-ownerless-ms", "60000"], &["/bin/sleep", "120"]);
    drop(lingering.connect(Role::Owner));
    let mut exited = Jar::start(&["--linger-ms", "60000"], &["/bin/sh", "-c", "exit 3"]);
    {
        let mut owner = exited.connect(Role::Owner);
        owner.send(&Msg::Replay { from: 0 }).expect("replay");
        // Seen, and never collected.
        recv_matching(&mut owner, |m| matches!(m, Msg::Exited { .. }));
    }
    eventually("every jar's socket", || waiting.socket.exists());
    let jars = [
        ("waiting for its first owner", &waiting),
        ("ownerless", &ownerless),
        ("lingering", &lingering),
        ("holding an exit", &exited),
    ];
    let before: Vec<f64> = jars
        .iter()
        .map(|(_, j)| cpu_seconds(j.process.id()))
        .collect();
    // The window ends past the waiting jar's ten-second first-owner grace.
    std::thread::sleep(Duration::from_secs(12));
    for ((state, jar), at) in jars.iter().zip(before) {
        assert!(
            jar.process.id() > 0 && running(jar.process.id()),
            "premise: the jar {state} still runs"
        );
        let spent = cpu_seconds(jar.process.id()) - at;
        assert!(spent < 0.5, "a jar {state} spent {spent} s of CPU in 12 s");
    }
}

/// An owner paused mid-frame keeps the jar's reader from a safe point, so an
/// upgrade asked meanwhile is busy (reason 5), never cut short or failed; once
/// the frame is whole, the next upgrade goes through.
#[test]
fn an_owner_paused_mid_frame_makes_an_upgrade_busy() {
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let frame = proto::encode(&Msg::Input {
        bytes: b"whole-at-last\n".to_vec(),
    });
    let (first, rest) = frame.split_at(frame.len() / 2);
    owner.send_raw(first).expect("the frame's first half");
    let mut reader = jar.connect(Role::Reader);
    let next = next_binary(jar.dir.path());
    let ask = |reader: &mut spyc_jar::client::Client| {
        reader
            .send(&Msg::Upgrade {
                binary: next.display().to_string(),
            })
            .expect("upgrade");
        recv_matching_within(reader, UPGRADE_AT_MOST, |m| {
            matches!(m, Msg::Refused { .. } | Msg::Upgraded { .. })
        })
    };
    let answer = ask(&mut reader);
    assert!(
        matches!(
            &answer,
            Msg::Refused { reason, why }
                if *reason == spyc_jar::proto::reason::UPGRADE_BUSY && why.contains("safe point")
        ),
        "an owner mid-frame makes the upgrade busy, its reader short of a safe point: {answer:?}"
    );
    owner.send_raw(rest).expect("the rest of the frame");
    let seen = read_until(&mut owner, true, |s| s.contains("whole-at-last"));
    assert!(seen.contains("whole-at-last"), "the owner's lease held");
    let answer = ask(&mut reader);
    assert!(
        matches!(answer, Msg::Upgraded { .. }),
        "with the frame whole, the upgrade goes through: {answer:?}"
    );
}
