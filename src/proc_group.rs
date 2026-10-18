//! The members of a process group, read from the OS: what a pane's foreground
//! job is running (#544). A job-control shell runs each job in its own group
//! and hands that group the terminal, so the group the pty's foreground points
//! at is what the user is running now.
//!
//! - **Membership**: macOS lists a group's pids through `libproc`
//!   (`pids_by_type(ByProgramGroup)`); Linux reads each `/proc/<pid>/stat`,
//!   whose fifth field is the group.
//! - **What each member runs**: `sysinfo`, scoped to those pids: the kernel's
//!   name for it, its argv and its start time.
//!
//! Empty on any failure (the group is gone, permission denied).

/// One process in a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub pid: u32,
    /// The kernel's name for it (`comm`).
    pub comm: String,
    pub argv: Vec<String>,
    /// When it started, in seconds since the epoch.
    pub started_epoch_secs: u64,
}

/// Every process in group `pgid`.
pub fn members(pgid: u32) -> Vec<Member> {
    use sysinfo_crate::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let pids: Vec<Pid> = group_pids(pgid).into_iter().map(Pid::from_u32).collect();
    if pids.is_empty() {
        return Vec::new();
    }
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    pids.iter()
        .filter_map(|pid| {
            let process = sys.process(*pid)?;
            Some(Member {
                pid: pid.as_u32(),
                comm: process.name().to_string_lossy().into_owned(),
                argv: process
                    .cmd()
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect(),
                started_epoch_secs: process.start_time(),
            })
        })
        .collect()
}

/// Whether `pid` or any process above it matches `matches`, given each one's
/// pid and argv. Walks parents until the root, or a process that can't be read.
pub fn has_ancestor(pid: u32, matches: impl Fn(u32, &[String]) -> bool) -> bool {
    let mut sys = sysinfo_crate::System::new();
    let mut current = Some(pid);
    // A cycle can't occur, but a pid reused mid-walk could make one look like it.
    for _ in 0..64 {
        let Some(pid) = current else {
            return false;
        };
        let Some((argv, parent)) = read_process(&mut sys, pid) else {
            return false;
        };
        if matches(pid, &argv) {
            return true;
        }
        current = parent.filter(|parent| *parent != pid);
    }
    false
}

/// `pid`'s argv and parent. A lookup can miss on a process that is busy, and
/// a miss ends the walk, so a live process of ours that can't be read yet is
/// tried again before the walk gives up on it.
fn read_process(sys: &mut sysinfo_crate::System, pid: u32) -> Option<(Vec<String>, Option<u32>)> {
    use sysinfo_crate::{Pid, ProcessRefreshKind, ProcessesToUpdate, UpdateKind};
    let target = Pid::from_u32(pid);
    for attempt in 0..3_u32 {
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[target]),
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
        );
        if let Some(process) = sys.process(target) {
            let argv = process
                .cmd()
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            return Some((argv, process.parent().map(Pid::as_u32)));
        }
        if !alive(pid) {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10 << attempt));
    }
    None
}

/// When `pid` started, in seconds since the epoch; `None` when it can't be read.
pub fn started_epoch_secs(pid: u32) -> Option<u64> {
    use sysinfo_crate::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let pid = Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys.process(pid).map(sysinfo_crate::Process::start_time)
}

/// When this process started, read once.
pub fn own_start_secs() -> Option<u64> {
    static OWN: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *OWN.get_or_init(|| started_epoch_secs(std::process::id()))
}

/// Whether `pid` is still a process.
pub fn alive(pid: u32) -> bool {
    i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .is_some_and(|pid| rustix::process::test_kill_process(pid).is_ok())
}

#[cfg(target_os = "macos")]
fn group_pids(pgid: u32) -> Vec<u32> {
    use libproc::processes::{ProcFilter, pids_by_type};
    pids_by_type(ProcFilter::ByProgramGroup { pgrpid: pgid })
        .unwrap_or_default()
        .into_iter()
        .filter(|pid| *pid != 0)
        .collect()
}

#[cfg(target_os = "linux")]
fn group_pids(pgid: u32) -> Vec<u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| stat_group(*pid) == Some(pgid))
        .collect()
}

/// The group in `/proc/<pid>/stat`. `comm` is parenthesized and may itself
/// hold spaces or parentheses, so the fields are counted from the last `)`:
/// state, ppid, then the group.
#[cfg(target_os = "linux")]
fn stat_group(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(2)?.parse().ok()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn group_pids(_pgid: u32) -> Vec<u32> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::{alive, has_ancestor, members, own_start_secs, started_epoch_secs};
    use std::os::unix::process::CommandExt;

    /// This process has a start time, it is in the past, and a pid no process
    /// holds has none.
    #[test]
    fn a_running_process_has_a_start_time() {
        let own = own_start_secs().expect("this process started");
        assert!(own <= crate::sysinfo::epoch_secs());
        assert_eq!(started_epoch_secs(std::process::id()), Some(own));
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn true");
        let pid = child.id();
        child.wait().expect("reap");
        assert_eq!(started_epoch_secs(pid), None);
    }

    /// A process started in a group of its own is that group's member, named
    /// for what it runs, and stops being alive once it is gone.
    #[test]
    fn a_group_lists_the_process_it_runs() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        let found = members(pid);
        let me = found
            .iter()
            .find(|m| m.pid == pid)
            .unwrap_or_else(|| panic!("{pid} not in its own group: {found:?}"));
        assert!(
            me.argv.first().is_some_and(|a| a.ends_with("sleep")),
            "{me:?}"
        );
        assert!(me.started_epoch_secs > 0);
        assert!(alive(pid));
        child.kill().expect("kill");
        child.wait().expect("reap");
        assert!(!alive(pid));
        assert!(members(pid).is_empty(), "the group is gone");
    }

    /// The walk climbs parents: a child's ancestors include this test
    /// process, and a predicate nothing matches is never satisfied.
    #[test]
    fn a_process_has_its_parent_as_an_ancestor() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let me = std::process::id();
        assert!(has_ancestor(child.id(), |pid, _| pid == me));
        assert!(!has_ancestor(child.id(), |_, argv| argv
            .iter()
            .any(|a| a == "no-such-marker")));
        child.kill().expect("kill");
        child.wait().expect("reap");
    }

    /// A process below one running as codex's shared daemon finds it: the
    /// shape `spyc --report-status` and `spyc --mcp` see when the daemon runs
    /// a session's hooks and MCP servers.
    #[test]
    fn a_process_under_a_daemon_shaped_parent_finds_it() {
        let mut daemon = std::process::Command::new("sh")
            .args([
                "-c",
                "sleep 30; :",
                "codex",
                "app-server",
                "--managed-daemon",
            ])
            .process_group(0)
            .spawn()
            .expect("spawn sh");
        let group = daemon.id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let sleeper = loop {
            if let Some(m) = members(group).into_iter().find(|m| m.pid != group) {
                break m.pid;
            }
            assert!(std::time::Instant::now() < deadline, "sleep never started");
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert!(has_ancestor(sleeper, |_, argv| {
            crate::agent::is_codex_shared_daemon(argv)
        }));
        daemon.kill().expect("kill");
        daemon.wait().expect("reap");
        let _ = std::process::Command::new("kill")
            .arg(sleeper.to_string())
            .status();
    }
}
