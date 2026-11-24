//! An upgrade's validation (`JAR_PROTOCOL.md` → "Upgrading: where each check
//! runs", part 2): the new binary is run as `validate-handover` with the blob
//! on its standard input, and must print the proof line for it within a
//! deadline. What its failures answer: only what the binary did rejects it
//! (reason 6); the machine's shortages and running out of time are busy
//! (reason 5), review finding A2. Its process group ends with it, whatever
//! the outcome (finding A9).

use std::io::{BufRead, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};

use crate::proto::reason;
use crate::upgrade::{OUTPUT_GRACE, PROOF};

/// How often an upgrade looks for the new binary's validation to end.
const VALIDATOR_POLL: Duration = Duration::from_millis(5);

/// Whether `e`, from copying or starting the new binary, is the binary's
/// own doing: it isn't there, can't be read or run, or isn't a file at all
/// (reason 6). Anything else (no descriptor, no process, no room on the
/// disk, no memory) is the machine's for now, and only defers the upgrade
/// (reason 5): review finding A2. A refusal that is no `io::Error` (not a
/// regular file, too big) is the binary's.
pub fn the_binarys_fault(e: &std::io::Error) -> bool {
    use rustix::io::Errno;
    use std::io::ErrorKind::{
        InvalidFilename, IsADirectory, NotADirectory, NotFound, PermissionDenied,
    };
    matches!(
        e.kind(),
        NotFound | PermissionDenied | NotADirectory | IsADirectory | InvalidFilename
    ) || matches!(Errno::from_io_error(e), Some(Errno::NOEXEC | Errno::LOOP))
        || e.raw_os_error().is_some_and(cant_run_here)
}

/// macOS's own refusals of a binary that can never run on this machine:
/// `EBADEXEC` (85), `EBADARCH` (86), `ESHLIBVERS` (87) and `EBADMACHO` (88).
/// std reads them as no kind it knows.
#[cfg(target_os = "macos")]
fn cant_run_here(errno: i32) -> bool {
    (85..=88).contains(&errno)
}

/// Linux says `ENOEXEC` for any binary it can't run.
#[cfg(not(target_os = "macos"))]
const fn cant_run_here(_errno: i32) -> bool {
    false
}

/// Have `binary` check the blob before anything is handed to it: it must
/// print the proof line for this blob within `deadline`, which covers its
/// exit and its output. The blob goes over a socketpair, not a pipe: a
/// validator that refuses without reading it leaves a write that answers
/// `EPIPE`, never `SIGPIPE`.
///
/// Only what the binary did rejects it (reason 6): it isn't there or can't
/// run, it exits non-zero, or it exits 0 without the proof. Running out of
/// time, its proof included, and a validator the machine can't start now,
/// make the upgrade busy (reason 5): a loaded machine defers an upgrade and
/// never fails a good binary (review finding A2). Whatever the outcome, the
/// validator's whole process group is ended (finding A9): a child it left
/// would hold a copy of the blob for as long as it lives.
pub fn validate(
    binary: &Path,
    blob: &[u8],
    instance: u64,
    deadline: Duration,
) -> Result<(), (u16, anyhow::Error)> {
    let busy = |e: anyhow::Error| (reason::UPGRADE_BUSY, e);
    let rejected = |e: anyhow::Error| (reason::UPGRADE_REJECTED, e);
    let (ours, theirs) = crate::jar::pair().context("socketpair").map_err(busy)?;
    let mut child = Command::new(binary)
        .arg("validate-handover")
        .process_group(0)
        .stdin(Stdio::from(OwnedFd::from(theirs)))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| unstarted(binary, e))?;
    let group = i32::try_from(child.id())
        .ok()
        .and_then(rustix::process::Pid::from_raw);
    let end_group = || {
        if let Some(g) = group {
            let _ = rustix::process::kill_process_group(g, rustix::process::Signal::KILL);
        }
    };
    // A thread the OS refuses (review finding B1) makes the upgrade busy,
    // the validator ended with its group.
    let no_thread = |child: &mut std::process::Child, e: std::io::Error| {
        end_group();
        let _ = child.wait();
        busy(anyhow!("no thread to validate {}: {e}", binary.display()))
    };
    let blob_owned = blob.to_vec();
    if let Err(e) = crate::threads::start("jar-blob-in", move || {
        let _ = (&ours).write_all(&blob_owned);
        let _ = ours.shutdown(std::net::Shutdown::Write);
    }) {
        return Err(no_thread(&mut child, e));
    }
    // Read on threads of their own: a child of the validator may hold either
    // stream open long after the validator has gone.
    let (line_tx, line_rx) = std::sync::mpsc::channel();
    if let Some(out) = child.stdout.take()
        && let Err(e) = crate::threads::start("jar-proof", move || {
            let mut first = String::new();
            let _ = std::io::BufReader::new(out.take(4096)).read_line(&mut first);
            let _ = line_tx.send(first);
        })
    {
        return Err(no_thread(&mut child, e));
    }
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    if let Some(err) = child.stderr.take()
        && let Err(e) = crate::threads::start("jar-why", move || {
            let mut why = String::new();
            let _ = err.take(4096).read_to_string(&mut why);
            let _ = err_tx.send(why);
        })
    {
        return Err(no_thread(&mut child, e));
    }
    let until = Instant::now() + deadline;
    let left = || until.saturating_duration_since(Instant::now());
    // Its exit is seen without reaping it: until it is reaped, its pid can't
    // name another process group, so ending its group reaches no stranger.
    let pid = rustix::process::Pid::from_child(&child);
    let succeeded = loop {
        use rustix::process::{WaitId, WaitIdOptions};
        let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
        match rustix::process::waitid(WaitId::Pid(pid), options) {
            Ok(Some(s)) => break s.exit_status() == Some(0),
            Ok(None) | Err(rustix::io::Errno::INTR) => {}
            Err(e) => {
                end_group();
                let _ = child.wait();
                return Err(busy(anyhow!("wait for {}: {e}", binary.display())));
            }
        }
        if left().is_zero() {
            end_group();
            let _ = child.wait();
            return Err(busy(anyhow!(
                "{} didn't answer within {deadline:?}",
                binary.display()
            )));
        }
        std::thread::sleep(VALIDATOR_POLL);
    };
    let answer = if succeeded {
        proved(binary, &line_rx, left(), (instance, blob.len()), deadline)
    } else {
        // Its reason, if it has gone with it: a child of the validator may
        // keep the stream open.
        let why = err_rx
            .recv_timeout(left().min(OUTPUT_GRACE))
            .unwrap_or_default();
        Err(rejected(anyhow!(
            "{} rejected the handover: {}",
            binary.display(),
            why.trim()
        )))
    };
    end_group();
    let _ = child.wait();
    answer
}

/// Why `binary`'s validator didn't start: its own fault (reason 6), or the
/// machine's for now (reason 5).
fn unstarted(binary: &Path, e: std::io::Error) -> (u16, anyhow::Error) {
    let why = if the_binarys_fault(&e) {
        reason::UPGRADE_REJECTED
    } else {
        reason::UPGRADE_BUSY
    };
    let e = anyhow::Error::new(e).context(format!("run {} validate-handover", binary.display()));
    (why, e)
}

/// Whether a validator that exited 0 proved the blob of `(instance, len)`:
/// its proof line, or the end of its output, read within `left`. A child
/// of the validator may hold its output open, and a line not there yet is no
/// line refused: running out of `deadline` is busy.
fn proved(
    binary: &Path,
    line: &std::sync::mpsc::Receiver<String>,
    left: Duration,
    (instance, len): (u64, usize),
    deadline: Duration,
) -> Result<(), (u16, anyhow::Error)> {
    match line.recv_timeout(left) {
        Ok(line) if proves(&line, instance, len) => Ok(()),
        Ok(line) => Err((
            reason::UPGRADE_REJECTED,
            anyhow!(
                "{} exited without proving it took the handover (it printed {:?})",
                binary.display(),
                line.trim()
            ),
        )),
        Err(_) => Err((
            reason::UPGRADE_BUSY,
            anyhow!(
                "{} didn't prove it took the handover within {deadline:?}",
                binary.display()
            ),
        )),
    }
}

/// Whether `line` is the proof line for the blob of `instance` and `len`
/// bytes. A later validator may add fields after these three.
fn proves(line: &str, instance: u64, len: usize) -> bool {
    let mut f = line.split_whitespace();
    f.next() == Some(PROOF)
        && f.next() == Some(format!("{instance:016x}").as_str())
        && f.next() == Some(len.to_string().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review finding A2: a start or copy short of a descriptor, a process,
    /// memory or room is the machine's for now (busy); a binary that isn't
    /// there or can never run is its own (rejected).
    #[test]
    fn a_shortage_is_the_machines_and_a_binary_that_cant_run_is_its_own() {
        use rustix::io::Errno;
        let e = |n: Errno| std::io::Error::from_raw_os_error(n.raw_os_error());
        for n in [
            Errno::MFILE,
            Errno::NFILE,
            Errno::AGAIN,
            Errno::NOMEM,
            Errno::NOSPC,
            Errno::IO,
        ] {
            assert!(!the_binarys_fault(&e(n)), "{n:?} is the machine's");
        }
        for n in [
            Errno::NOENT,
            Errno::ACCESS,
            Errno::PERM,
            Errno::NOEXEC,
            Errno::LOOP,
            Errno::NOTDIR,
            Errno::ISDIR,
            Errno::NAMETOOLONG,
        ] {
            assert!(the_binarys_fault(&e(n)), "{n:?} is the binary's");
        }
        #[cfg(target_os = "macos")]
        for n in 85..=88 {
            let err = std::io::Error::from_raw_os_error(n);
            assert!(the_binarys_fault(&err), "macOS's {n} is the binary's");
        }
    }
}
