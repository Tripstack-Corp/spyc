//! A new image that can't resume contains it (`JAR_UPGRADE_BRIEF.md` → "4.
//! The new image's parts"): it holds the program it was handed, keeps its
//! PTY drained and its connections answered, and ends the holder at once.
//! `SPYC_JAR_RESUME_FAULT` makes a debug build's resume fail as asked.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use spyc_jar::client::Client;
use spyc_jar::proto::{Msg, Role, reason};

use super::upgrade::{jar_script, upgrade_answer};
use super::*;

/// A jar whose upgrade went to a new image that couldn't resume.
struct Held {
    jar: Jar,
    owner: Client,
    /// What the upgrade's requester heard.
    answer: Option<Msg>,
    /// The holder's pid.
    holder: u32,
    /// The program's rounds, and the file that stops it.
    rounds: PathBuf,
    stop: PathBuf,
    _marks: tempfile::TempDir,
}

/// The rounds the program has printed so far.
fn rounds(path: &Path) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Upgrade a jar to its own binary, resumed with `SPYC_JAR_RESUME_FAULT` set
/// to `fault`. Its program prints as fast as the PTY takes it, counting its
/// rounds (written down every hundredth, so a loaded disk doesn't pace it),
/// until a stop file appears: a PTY nobody reads holds a few hundred of its
/// rounds at most. It ignores `SIGHUP`, so only a kill ends it early.
fn held_by(fault: &str) -> Held {
    let marks = tempfile::tempdir().expect("tempdir");
    let rounds_at = marks.path().join("rounds");
    let stop = marks.path().join("stop");
    let program = format!(
        "trap '' HUP; i=0; while [ ! -e {stop} ]; do i=$((i+1)); [ $((i % 100)) -ne 0 ] || echo $i > {rounds}; printf '%0500d\\n' 0; done",
        stop = stop.display(),
        rounds = rounds_at.display()
    );
    let mut jar = Jar::start(&[], &sh(&program));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let mut seen = read_while_it_comes(&mut owner, true, |s| s.text.len() >= 4096);
    let holder_pid = jar.dir.path().join("holder.pid");
    let faulty = jar_script(
        jar.dir.path(),
        "jar-faulty",
        &format!(
            "hold-handover) echo $$ > {};;\nresume-handover) export SPYC_JAR_RESUME_FAULT={fault};;",
            holder_pid.display()
        ),
    );
    let (answer, _) = upgrade_answer(&mut owner, &mut seen, &faulty, UPGRADE_AT_MOST);
    let holder = std::fs::read_to_string(&holder_pid)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or_else(|| panic!("the holder never ran; the upgrade: {answer:?}"));
    Held {
        jar,
        owner,
        answer,
        holder,
        rounds: rounds_at,
        stop,
        _marks: marks,
    }
}

fn assert_failed(h: &Held, what: &str) {
    assert!(
        matches!(
            h.answer,
            Some(Msg::Refused {
                reason: reason::UPGRADE_FAILED,
                ..
            })
        ),
        "{what}: the requester hears the upgrade failed (reason 7): {:?}",
        h.answer
    );
}

/// What a held jar does: the holder goes at once, the program prints on past
/// anything a PTY buffers, and every connection is answered, the requester's
/// and new ones alike.
fn assert_held(h: &mut Held, what: &str) {
    assert_failed(h, what);
    let until = Instant::now() + Duration::from_secs(3);
    while running(h.holder) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !running(h.holder),
        "{what}: the holder outlived the failed resume"
    );
    let from = rounds(&h.rounds);
    while_it_grows(
        &format!("{what}: the program printing on"),
        || rounds(&h.rounds),
        |n| n >= from + 2000,
    );
    h.owner.send(&Msg::Ping { n: 9 }).expect("ping");
    recv_matching(&mut h.owner, |m| matches!(m, Msg::Pong { n: 9 }));
    h.owner
        .send(&Msg::Input {
            bytes: b"x".to_vec(),
        })
        .expect("input");
    recv_matching(&mut h.owner, |m| {
        matches!(
            m,
            Msg::Refused {
                reason: reason::UPGRADE_FAILED,
                ..
            }
        )
    });
    let fields = describe_within(&h.jar.socket)
        .unwrap_or_else(|e| panic!("{what}: a lookup isn't answered: {e}"));
    assert!(
        !field(&fields, "held").is_empty(),
        "{what}: the description says the jar is held: {fields:?}"
    );
    assert_eq!(
        refusal(try_connect(&h.jar, Role::Reader, TOKEN)),
        reason::UPGRADE_FAILED,
        "{what}: a new reader is told why it can't read"
    );
    assert!(
        h.jar.process.try_wait().expect("try_wait").is_none(),
        "{what}: the jar lives"
    );
}

/// The holder has nothing left to do once the new image has its sockets,
/// whether or not the blob came: it goes at once, not after its 30 seconds.
#[test]
fn a_new_image_that_never_reads_the_blob_holds_the_program() {
    let mut h = held_by("blob");
    assert_held(&mut h, "an unread blob");
}

#[test]
fn a_new_image_that_cant_decode_the_blob_holds_the_program() {
    let mut h = held_by("error");
    assert_held(&mut h, "a decoding error");
}

#[test]
fn a_new_image_that_panics_as_it_starts_holds_the_program() {
    let mut h = held_by("panic");
    assert_held(&mut h, "a panic");
}

/// A held jar has nothing left to keep once its program ends: it goes, and
/// takes its socket with it.
#[test]
fn a_held_jar_ends_with_its_program() {
    let mut h = held_by("error");
    assert_failed(&h, "held");
    std::fs::write(&h.stop, "").expect("stop the program");
    assert!(
        h.jar.ended_within(WAIT),
        "the held jar outlived its program"
    );
    assert!(!h.jar.socket.exists(), "the held jar left its socket");
}

/// Nothing can attach to a held jar, so once its last connection has gone
/// it ends its program, as a closed terminal would, and goes. The program
/// ignores the hangup, so it takes the kill that follows.
#[test]
fn a_held_jar_ends_its_program_once_nobody_is_connected() {
    let h = held_by("error");
    assert_failed(&h, "held");
    let Held { mut jar, owner, .. } = h;
    let program = jar.program.expect("the program's pid");
    drop(owner);
    assert!(
        jar.ended_within(WAIT),
        "the held jar outlived its last connection"
    );
    eventually("the program's end", || !running(program));
}
