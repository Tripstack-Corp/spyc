//! The jar's lock (finding 4 of the ownership-timer audit): a jar holds an
//! `fcntl` lock on a file beside its socket for as long as it lives, across
//! every upgrade's `exec`, and nothing it started holds it once it's gone.
//! A client reads it as spyc does, taking it without waiting and letting go
//! at once: held, the jar is alive, however busy; free, the jar is gone.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rustix::fs::FlockOperation;
use spyc_jar::proto::{Msg, Role};

use super::timers::{counting, on_call};
use super::upgrade::{assert_program_survives, jar_script, upgrade_answer};
use super::*;

/// The jar's lock file: its socket's path with `.lock` added.
fn lock_path(socket: &Path) -> PathBuf {
    let mut p = socket.as_os_str().to_owned();
    p.push(".lock");
    p.into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lock {
    Held,
    Free,
    Missing,
}

/// The jar's lock as spyc reads it.
fn lock_of(socket: &Path) -> Lock {
    let Ok(f) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path(socket))
    else {
        return Lock::Missing;
    };
    // Dropping the file lets go of a lock taken here.
    match rustix::fs::fcntl_lock(&f, FlockOperation::NonBlockingLockShared) {
        Ok(()) => Lock::Free,
        Err(_) => Lock::Held,
    }
}

fn lock_becomes(socket: &Path, want: Lock, what: &str) {
    let until = Instant::now() + WAIT;
    while lock_of(socket) != want {
        assert!(
            Instant::now() < until,
            "{what}: the lock stayed {:?}",
            lock_of(socket)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A program that outlives its jar: it ignores the hangup the jar's end
/// sends its terminal.
fn outliving() -> Vec<&'static str> {
    sh("trap '' HUP; echo hangups-ignored; exec sleep 120")
}

/// Held while the jar runs; free within a moment of the jar being killed,
/// though the program it started runs on.
#[test]
#[ignore = "red until a jar holds a lock beside its socket for its life"]
fn a_jar_holds_its_lock_while_it_runs_and_its_program_never_does() {
    let mut jar = Jar::start(&[], &outliving());
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    read_until(&mut owner, true, |s| s.contains("hangups-ignored"));
    let program = jar.program.expect("the program's pid");
    lock_becomes(&jar.socket, Lock::Held, "a running jar");
    jar.process.kill().expect("kill the jar");
    jar.process.wait().expect("reap the jar");
    lock_becomes(&jar.socket, Lock::Free, "a killed jar");
    assert!(running(program), "premise: the program outlived its jar");
}

/// The lock is held at every moment across consecutive upgrades, a parked
/// validation included, and the validators and holders they started don't
/// keep it once the jar is killed.
#[test]
#[ignore = "red until a jar holds a lock beside its socket for its life"]
fn the_lock_is_held_through_every_upgrade() {
    let (mut jar, mut owner, mut seen) = counting();
    let socket = jar.socket.clone();
    lock_becomes(&socket, Lock::Held, "a running jar");
    let stop = Arc::new(AtomicBool::new(false));
    let (probes, misses) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
    let watcher = {
        let (stop, probes, misses) = (Arc::clone(&stop), Arc::clone(&probes), Arc::clone(&misses));
        let socket = socket.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                probes.fetch_add(1, Ordering::SeqCst);
                if lock_of(&socket) != Lock::Held {
                    misses.fetch_add(1, Ordering::SeqCst);
                }
            }
        })
    };
    let calls = jar.dir.path().join("validations");
    let slow_parked = jar_script(
        jar.dir.path(),
        "jar-slow-parked",
        &format!("validate-handover) {};;", on_call(&calls, 2, "sleep 2")),
    );
    let plain = jar_script(jar.dir.path(), "jar-plain", "");
    for binary in [&plain, &slow_parked, &plain] {
        let (answer, _) = upgrade_answer(&mut owner, &mut seen, binary, Duration::from_secs(60));
        assert!(
            matches!(answer, Some(Msg::Upgraded { .. })),
            "the upgrade to {} went through: {answer:?}",
            binary.display()
        );
    }
    stop.store(true, Ordering::SeqCst);
    watcher.join().expect("the watcher");
    let (probes, misses) = (probes.load(Ordering::SeqCst), misses.load(Ordering::SeqCst));
    assert!(
        probes > 100,
        "premise: the lock was watched ({probes} probes)"
    );
    assert_eq!(
        misses, 0,
        "the lock was not held in {misses} of {probes} probes"
    );
    assert_program_survives(&mut jar, owner, "three upgrades");
    jar.process.kill().expect("kill the jar");
    jar.process.wait().expect("reap the jar");
    lock_becomes(&socket, Lock::Free, "a killed jar, after upgrades");
}

/// A cleaner may delete the lock file as it may the socket: the jar makes
/// it again and holds the new one.
#[test]
#[ignore = "red until a jar holds a lock beside its socket for its life"]
fn a_jar_whose_lock_file_was_deleted_makes_and_holds_it_again() {
    let mut jar = Jar::start(&[], &["/bin/cat"]);
    let _owner = jar.connect(Role::Owner);
    lock_becomes(&jar.socket, Lock::Held, "a running jar");
    std::fs::remove_file(lock_path(&jar.socket)).expect("delete the lock file");
    lock_becomes(&jar.socket, Lock::Held, "a jar whose lock file was deleted");
}
