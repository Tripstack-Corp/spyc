//! What an upgrade answers, and what it leaves, when it doesn't happen:
//! review findings A2 (running out of time, and a transient error, is busy,
//! reason 5, never rejected, reason 6: spyc stops upgrading for the session
//! on a 6), A3 (the copy step takes only a regular file, without blocking,
//! and removes what it refused) and A9 (a validator's process group ends
//! with it, whatever the outcome).

use super::*;
use spyc_jar::proto::reason;

/// A new binary at `name` in the jar's directory: `case` does `arm` for its
/// first argument and runs the real jar for anything else. `$DIR` is the
/// jar's directory and `$REAL` the real jar.
fn script(jar: &Jar, name: &str, arm: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = jar.dir.path().join(name);
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nREAL='{}'\nDIR='{}'\ncase \"$1\" in\n{arm}\n*) exec \"$REAL\" \"$@\" ;;\nesac\n",
            env!("CARGO_BIN_EXE_spyc-jar"),
            jar.dir.path().display()
        ),
    )
    .expect("write the binary");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

/// Ask for an upgrade to `binary` on `owner` and take the answer.
fn upgrade_to(owner: &mut Client, binary: &std::path::Path) -> Msg {
    owner
        .send(&Msg::Upgrade {
            binary: binary.display().to_string(),
        })
        .expect("upgrade");
    upgrade_answer_pinging(owner, |m| {
        matches!(m, Msg::Refused { .. } | Msg::Upgraded { .. })
    })
}

/// The reason in a refusal, or what came instead.
fn reason_of(m: &Msg) -> Result<u16, String> {
    match m {
        Msg::Refused { reason, .. } => Ok(*reason),
        other => Err(format!("{other:?}")),
    }
}

/// The copies the jar keeps beside its socket.
fn copies(jar: &Jar) -> Vec<String> {
    std::fs::read_dir(jar.dir.path().join(spyc_jar::store::COPIES)).map_or_else(
        |_| Vec::new(),
        |d| {
            d.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        },
    )
}

/// A2: a validator that exits 0 at once while a child of it prints the proof
/// after the parked validation's deadline has run out of time: the upgrade is
/// busy, and the next one runs. The old image called it rejected (6), and
/// spyc then stopped upgrading every jar for the session.
#[test]
#[ignore = "red until A2, A3 and A9 are fixed (fix/jar-upgrade-answers)"]
fn a_proof_that_comes_after_the_deadline_is_busy_and_the_next_upgrade_runs() {
    let mut jar = Jar::start(&[], &sh("cat"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    // The second validation, the parked one, proves its blob only after
    // its deadline. The others are the real jar's.
    let late = script(
        &jar,
        "jar-late",
        "validate-handover)\n  n=$(cat \"$DIR/count\" 2>/dev/null || echo 0); n=$((n + 1)); echo $n > \"$DIR/count\"\n  if [ $n = 2 ]; then proof=$(\"$REAL\" validate-handover); ( sleep 8; echo \"$proof\" ) & exit 0; fi\n  exec \"$REAL\" \"$@\" ;;",
    );
    let answer = upgrade_to(&mut owner, &late);
    assert_eq!(
        reason_of(&answer),
        Ok(reason::UPGRADE_BUSY),
        "a proof after the deadline is busy: {answer:?}"
    );
    let again = upgrade_to(&mut owner, &late);
    assert!(
        matches!(again, Msg::Upgraded { .. }),
        "the next upgrade runs: {again:?}"
    );
}

/// A3: an upgrade naming a FIFO is refused at once, not left waiting in
/// `open` with the jar's upgrade claimed for as long as nothing writes to it;
/// the next upgrade runs.
#[test]
#[ignore = "red until A2, A3 and A9 are fixed (fix/jar-upgrade-answers)"]
fn an_upgrade_naming_a_fifo_is_refused_and_the_next_one_runs() {
    let mut jar = Jar::start(&[], &sh("cat"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let fifo = jar.dir.path().join("jar-fifo");
    let made = Command::new("mkfifo").arg(&fifo).status().expect("mkfifo");
    assert!(made.success(), "mkfifo");
    let answer = upgrade_to(&mut owner, &fifo);
    assert_eq!(
        reason_of(&answer),
        Ok(reason::UPGRADE_REJECTED),
        "a FIFO is no binary: {answer:?}"
    );
    let again = upgrade(&jar, &mut owner);
    assert!(again.is_ok(), "the next upgrade runs: {again:?}");
}

/// A3: a copy the jar made for an upgrade that didn't happen is removed.
#[test]
#[ignore = "red until A2, A3 and A9 are fixed (fix/jar-upgrade-answers)"]
fn a_refused_upgrade_leaves_no_copy_behind() {
    let mut jar = Jar::start(&[], &sh("cat"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let junk = script(&jar, "jar-junk", "validate-handover) exit 3 ;;");
    let answer = upgrade_to(&mut owner, &junk);
    assert_eq!(
        reason_of(&answer),
        Ok(reason::UPGRADE_REJECTED),
        "{answer:?}"
    );
    assert_eq!(
        copies(&jar),
        Vec::<String>::new(),
        "the refused copy is gone"
    );
}

/// Whether process `pid` runs: there, and not a zombie waiting to be reaped.
fn alive(pid: u32) -> bool {
    if !running(pid) {
        return false;
    }
    let out = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("ps");
    let stat = String::from_utf8_lossy(&out.stdout);
    !stat.trim().is_empty() && !stat.trim().starts_with('Z')
}

/// A9: a child a validator leaves behind is ended with the validator's
/// process group once the validation is over, however it ended: only a
/// validator that ran out of time had its group ended.
#[test]
#[ignore = "red until A2, A3 and A9 are fixed (fix/jar-upgrade-answers)"]
fn a_validators_children_end_with_the_validation() {
    let mut jar = Jar::start(&[], &sh("cat"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let leaving = script(
        &jar,
        "jar-leaving",
        "validate-handover) sleep 60 </dev/null >/dev/null 2>&1 & echo $! >> \"$DIR/children\"; exec \"$REAL\" \"$@\" ;;",
    );
    let answer = upgrade_to(&mut owner, &leaving);
    assert!(matches!(answer, Msg::Upgraded { .. }), "{answer:?}");
    let children: Vec<u32> = std::fs::read_to_string(jar.dir.path().join("children"))
        .expect("the validators' children")
        .lines()
        .map(|l| l.trim().parse().expect("a pid"))
        .collect();
    assert_eq!(children.len(), 2, "one child for each validation");
    let until = Instant::now() + WAIT;
    while children.iter().any(|c| alive(*c)) {
        if Instant::now() >= until {
            for c in &children {
                let _ = Command::new("kill").arg(c.to_string()).status();
            }
            panic!("a validator's child outlived its validation: {children:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The jar's soft `RLIMIT_NOFILE` set to `n` while this lives, put back
/// when it is dropped.
#[cfg(target_os = "linux")]
struct Descriptors {
    pid: rustix::process::Pid,
    was: rustix::process::Rlimit,
}

#[cfg(target_os = "linux")]
impl Descriptors {
    /// No descriptor the jar opens may be numbered `n` or above.
    fn below(jar: &Jar, n: u64) -> Self {
        use rustix::process::{Pid, Resource, Rlimit};
        let pid =
            Pid::from_raw(i32::try_from(jar.process.id()).expect("a pid")).expect("the jar's pid");
        let hard = rustix::process::getrlimit(Resource::Nofile).maximum;
        let was = rustix::process::prlimit(
            Some(pid),
            Resource::Nofile,
            Rlimit {
                current: Some(n),
                maximum: hard,
            },
        )
        .expect("set the jar's RLIMIT_NOFILE");
        Self { pid, was }
    }
}

#[cfg(target_os = "linux")]
impl Drop for Descriptors {
    fn drop(&mut self) {
        let _ =
            rustix::process::prlimit(Some(self.pid), rustix::process::Resource::Nofile, self.was);
    }
}

/// The `n` lowest descriptor numbers the jar has free: the next ones it
/// opens, since the kernel always hands out the lowest.
#[cfg(target_os = "linux")]
fn free_descriptors(jar: &Jar, n: usize) -> Vec<u64> {
    let open: std::collections::BTreeSet<u64> =
        std::fs::read_dir(format!("/proc/{}/fd", jar.process.id()))
            .expect("the jar's descriptors")
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
            .collect();
    (0..).filter(|fd| !open.contains(fd)).take(n).collect()
}

/// A2: an upgrade whose copy of the new binary can't open it for want of a
/// descriptor (`EMFILE`, as a disk that is full is `ENOSPC`) is busy, not
/// rejected; with descriptors again, the next one runs.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "red until A2, A3 and A9 are fixed (fix/jar-upgrade-answers)"]
fn an_upgrade_that_cant_copy_for_want_of_a_descriptor_is_busy() {
    let mut jar = Jar::start(&[], &sh("cat"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let next = next_binary(jar.dir.path());
    let answer = {
        let free = free_descriptors(&jar, 1);
        let _none = Descriptors::below(&jar, free[0]);
        upgrade_to(&mut owner, &next)
    };
    assert_eq!(
        reason_of(&answer),
        Ok(reason::UPGRADE_BUSY),
        "a copy short of descriptors is busy: {answer:?}"
    );
    let again = upgrade(&jar, &mut owner);
    assert!(again.is_ok(), "the next upgrade runs: {again:?}");
}

/// A2: an upgrade whose validator can't start for want of a descriptor is
/// busy, not rejected: the copy takes two descriptors and gives them back,
/// the socketpair to the validator takes the same two, and its pipes find
/// none.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "red until A2, A3 and A9 are fixed (fix/jar-upgrade-answers)"]
fn an_upgrade_whose_validator_cant_start_for_want_of_a_descriptor_is_busy() {
    let mut jar = Jar::start(&[], &sh("cat"));
    let mut owner = jar.connect(Role::Owner);
    owner.send(&Msg::Replay { from: 0 }).expect("replay");
    let next = next_binary(jar.dir.path());
    let answer = {
        let free = free_descriptors(&jar, 2);
        let _two = Descriptors::below(&jar, free[1] + 1);
        upgrade_to(&mut owner, &next)
    };
    assert_eq!(
        reason_of(&answer),
        Ok(reason::UPGRADE_BUSY),
        "a validator short of descriptors is busy: {answer:?}"
    );
    let again = upgrade(&jar, &mut owner);
    assert!(again.is_ok(), "the next upgrade runs: {again:?}");
}
