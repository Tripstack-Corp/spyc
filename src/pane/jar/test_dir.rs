//! A test's directory for its jars. A pane's jar waits for an owner for
//! ever, so a test that leaves one, by cutting it off from its owner as a
//! crash would or by failing first, would leave it and its program running
//! after the test, each holding a pty. Whatever jar still answers in the
//! directory when it goes is ended with its program, however the test ended.

use std::path::{Path, PathBuf};

/// A directory under `/tmp`, short enough for a socket's path on macOS.
pub struct TestDir(tempfile::TempDir);

impl TestDir {
    pub fn new(prefix: &str) -> Self {
        Self(
            tempfile::Builder::new()
                .prefix(prefix)
                .tempdir_in("/tmp")
                .expect("a directory under /tmp"),
        )
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        use rustix::process::{Pid, Signal, kill_process, kill_process_group};
        for socket in sockets(self.path()) {
            let Ok(said) = spyc_jar::client::describe(&socket) else {
                continue;
            };
            let pid = |key: &str| {
                said.iter()
                    .find(|(k, _)| k == key)
                    .and_then(|(_, v)| v.parse().ok())
                    .and_then(Pid::from_raw)
            };
            // The program's group first: a jar ended first would leave it
            // a moment to outlive its terminal.
            if let Some(program) = pid("program_pid") {
                let _ = kill_process(program, Signal::CONT);
                let _ = kill_process_group(program, Signal::KILL);
            }
            if let Some(jar) = pid("pid") {
                let _ = kill_process(jar, Signal::CONT);
                let _ = kill_process(jar, Signal::KILL);
            }
        }
    }
}

/// Every socket under `dir`, however deep.
fn sockets(dir: &Path) -> Vec<PathBuf> {
    use std::os::unix::fs::FileTypeExt;
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            found.extend(sockets(&entry.path()));
        } else if kind.is_socket() {
            found.push(entry.path());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::jar::place::Place;
    use crate::pane::pty_host::{PtyHost, PtySpec};
    use std::time::{Duration, Instant};

    fn running(pid: u32) -> bool {
        rustix::process::Pid::from_raw(i32::try_from(pid).unwrap())
            .is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
    }

    fn field(socket: &Path, key: &str) -> u32 {
        spyc_jar::client::describe(socket)
            .unwrap()
            .into_iter()
            .find(|(k, _)| k == key)
            .unwrap()
            .1
            .parse()
            .unwrap()
    }

    /// A jar cut off from its owner, as a crash leaves it, is ended with its
    /// program once the test's directory goes.
    #[test]
    fn a_jar_left_in_a_tests_directory_ends_with_it() {
        let dir = TestDir::new("jd");
        let exe = std::env::current_exe().unwrap();
        let binary = crate::pane::jar::warmed(
            exe.ancestors()
                .skip(1)
                .map(|d| d.join("spyc-jar"))
                .find(|j| j.is_file())
                .expect("a built spyc-jar: make test builds it"),
        );
        let place = Place {
            binary,
            dirs: vec![
                dir.path().join("spyc"),
                dir.path().join("spyc").join("jars"),
            ],
            tokens: dir.path().join("tokens"),
            hold: crate::pane::jar::place::HOLD,
            repaint_hold: Duration::from_secs(2),
        };
        let host = PtyHost::spawn(PtySpec {
            command: "exec cat",
            rows: 24,
            cols: 80,
            cwd: dir.path(),
            env: &[],
            term: "dumb",
            nudge_winch: false,
            debug_dump: false,
            exec_replace: false,
            capture_launch_env: false,
            jar: Some(crate::pane::jar::Spec {
                place,
                name: "left-behind".into(),
                adopt: None,
                retake: false,
                exit_linger: None,
            }),
        })
        .unwrap();
        let socket = host.jar_socket().unwrap().to_path_buf();
        let (jar, program) = (field(&socket, "pid"), field(&socket, "program_pid"));
        host.sever_jar();
        std::mem::forget(host);
        drop(dir);
        let until = Instant::now() + Duration::from_secs(5);
        while running(jar) || running(program) {
            assert!(
                Instant::now() < until,
                "the jar ran on: {}, its program: {}",
                running(jar),
                running(program)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
