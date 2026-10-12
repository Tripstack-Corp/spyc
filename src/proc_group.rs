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
    use super::{alive, members};
    use std::os::unix::process::CommandExt;

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
}
