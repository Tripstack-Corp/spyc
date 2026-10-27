//! While the TUI owns the screen, spyc's stderr (fd 2) points at a per-run log
//! file instead of the terminal. Whatever writes there — a library's warning,
//! macOS's malloc notice from a child between `fork` and `exec`, a helper
//! spawned with an inherited stderr — lands in the `:stderr` log rather than
//! over the frame.
//!
//! A file, not a pipe: a write never blocks however far behind the reader is,
//! what a crash prints on its way down is still on disk afterwards, and a child
//! that outlives spyc never writes into a closed pipe. [`crate::terminal`] hands
//! the real stderr back whenever spyc hands over the terminal.

use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use rustix::fs::{Mode, OFlags};

/// The run's log, and the stderr spyc started with.
struct Redirect {
    path: PathBuf,
    log: OwnedFd,
    original: OwnedFd,
}

static REDIRECT: OnceLock<Redirect> = OnceLock::new();

/// How long a finished run's log is kept.
const KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Where the logs live: `<state_root>/stderr`, one `<pid>.log` per run.
fn dir() -> Option<PathBuf> {
    crate::state::state_root().map(|root| root.join("stderr"))
}

/// Point fd 2 at this run's log. Its path; `None` when there is no state
/// directory or the log can't be opened, and stderr stays the terminal.
pub fn start() -> Option<PathBuf> {
    start_in(&dir()?).ok()
}

fn start_in(dir: &Path) -> io::Result<PathBuf> {
    if let Some(redirect) = REDIRECT.get() {
        return Ok(redirect.path.clone());
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.log", std::process::id()));
    let log = rustix::fs::open(
        &path,
        OFlags::CREATE
            | OFlags::WRONLY
            | OFlags::APPEND
            | OFlags::TRUNC
            | OFlags::NOFOLLOW
            | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )?;
    let original = io::stderr().as_fd().try_clone_to_owned()?;
    rustix::stdio::dup2_stderr(&log)?;
    Ok(REDIRECT
        .get_or_init(|| Redirect {
            path,
            log,
            original,
        })
        .path
        .clone())
}

/// Hand fd 2 back to the stderr spyc started with, for a child taking the
/// terminal over, a panic's message, or what spyc prints as it quits.
pub fn release() {
    if let Some(redirect) = REDIRECT.get() {
        // Best-effort: a failed dup2 leaves stderr where it was, and there is
        // nowhere left to report it.
        let _ = rustix::stdio::dup2_stderr(&redirect.original);
    }
}

/// Point fd 2 back at the log once spyc has the terminal again.
pub fn reclaim() {
    if let Some(redirect) = REDIRECT.get() {
        let _ = rustix::stdio::dup2_stderr(&redirect.log);
    }
}

/// At a clean exit: hand stderr back and drop the log if nothing was written
/// to it. The path of one that holds something, for spyc to point at.
pub fn finish() -> Option<PathBuf> {
    let redirect = REDIRECT.get()?;
    release();
    kept(&redirect.path)
}

/// `path`, unless it is empty, in which case it is deleted.
fn kept(path: &Path) -> Option<PathBuf> {
    if std::fs::metadata(path).ok()?.len() > 0 {
        return Some(path.to_path_buf());
    }
    let _ = std::fs::remove_file(path);
    None
}

/// A log in the log directory: its path, when it was last written, its size.
type Entry = (PathBuf, SystemTime, u64);

/// The logs in `entries` to delete: another run's, once that run has ended,
/// straight away when it is empty and after [`KEEP`] when it isn't, which
/// leaves a crashed run's last words there for a week.
fn stale(
    entries: &[Entry],
    own: u32,
    now: SystemTime,
    alive: impl Fn(u32) -> bool,
) -> Vec<PathBuf> {
    entries
        .iter()
        .filter(|(path, modified, len)| {
            let Some(pid) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<u32>().ok())
            else {
                return false;
            };
            let old = now.duration_since(*modified).is_ok_and(|age| age > KEEP);
            pid != own && !alive(pid) && (*len == 0 || old)
        })
        .map(|(path, ..)| path.clone())
        .collect()
}

/// Delete the logs [`stale`] picks. Directory IO, so it runs on the log's
/// follower thread, never the loop.
pub fn prune() {
    let Some(dir) = dir() else { return };
    let Ok(read) = std::fs::read_dir(&dir) else {
        return;
    };
    let entries: Vec<Entry> = read
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "log"))
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            Some((entry.path(), meta.modified().ok()?, meta.len()))
        })
        .collect();
    let alive = |pid: u32| {
        i32::try_from(pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
            .is_some_and(|pid| rustix::process::test_kill_process(pid).is_ok())
    };
    for path in stale(&entries, std::process::id(), SystemTime::now(), alive) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set in the copy of the test binary [`what_spyc_and_its_children_print_lands_in_the_log`]
    /// runs, naming the directory its log goes in.
    const CHILD: &str = "SPYC_STDERR_CAPTURE_CHILD";

    /// fd 2 is the process's, so the redirect is tested in a copy of the test
    /// binary rather than in this one, whose stderr the harness reports through.
    #[test]
    fn what_spyc_and_its_children_print_lands_in_the_log() {
        if let Some(dir) = std::env::var_os(CHILD) {
            let path = start_in(Path::new(&dir)).unwrap();
            eprintln!("spyc, captured");
            std::process::Command::new("/bin/sh")
                .args(["-c", "echo a child, captured >&2"])
                .status()
                .unwrap();
            release();
            eprintln!("spyc, released");
            reclaim();
            eprintln!("spyc, reclaimed");
            assert_eq!(finish(), Some(path));
            eprintln!("spyc, finished");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "stderr_capture::tests::what_spyc_and_its_children_print_lands_in_the_log",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, dir.path())
            .output()
            .unwrap();
        let terminal = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "the child failed: {terminal}");
        let logs: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(logs.len(), 1, "one log per run");
        let log = std::fs::read_to_string(logs[0].as_ref().unwrap().path()).unwrap();
        assert_eq!(
            log, "spyc, captured\na child, captured\nspyc, reclaimed\n",
            "the log holds what was written while captured, and nothing else"
        );
        for line in ["spyc, released", "spyc, finished"] {
            assert!(
                terminal.contains(line),
                "{line:?} missing from {terminal:?}"
            );
        }
        for line in ["captured", "reclaimed"] {
            assert!(
                !terminal.contains(line),
                "{line:?} leaked onto {terminal:?}"
            );
        }
    }

    #[test]
    fn an_empty_log_is_deleted_and_a_written_one_kept() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("1.log");
        std::fs::write(&empty, "").unwrap();
        assert_eq!(kept(&empty), None);
        assert!(!empty.exists());

        let written = dir.path().join("2.log");
        std::fs::write(&written, "boom\n").unwrap();
        assert_eq!(kept(&written), Some(written.clone()));
        assert!(written.exists());
    }

    #[test]
    fn only_an_ended_runs_log_is_pruned_and_a_written_one_only_after_a_week() {
        let now = SystemTime::now();
        let fresh = now - Duration::from_secs(60);
        let old = now - KEEP - Duration::from_secs(60);
        let entries: Vec<Entry> = vec![
            ("/s/10.log".into(), fresh, 0),
            ("/s/11.log".into(), fresh, 5),
            ("/s/12.log".into(), old, 5),
            ("/s/13.log".into(), old, 5),
            ("/s/14.log".into(), old, 0),
            ("/s/notes.log".into(), old, 0),
        ];
        // 13 is still running; 14 is this run.
        let picked = stale(&entries, 14, now, |pid| pid == 13);
        assert_eq!(
            picked,
            vec![PathBuf::from("/s/10.log"), PathBuf::from("/s/12.log")]
        );
    }
}
