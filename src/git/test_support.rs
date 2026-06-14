//! Shared `#[cfg(test)]` git fixtures for the git module's unit tests.
//!
//! Production git is pure gix (enforced by `no_subprocess_git_in_production`);
//! these spawn the `git` binary only to build scratch repos for tests.
//! Consolidated from five near-identical per-module copies — two of which
//! (`status`, `discovery`) predated the CWD-thrash hardening below, so they
//! were the flaky ones under the parallel suite.

use std::path::Path;

/// Run `git` against `dir` with a hermetic config (no user/system
/// `.gitconfig`), returning stdout.
///
/// Hardening for the parallel test suite: pass the operation dir via
/// `-C <dir>` and pin the *process* cwd to a stable, never-deleted
/// `temp_dir()`. Sibling tests `set_current_dir` and drop their tempdirs,
/// which can transiently invalidate an inherited cwd mid-spawn; retry a few
/// times with backoff to ride out that thrash.
pub fn run_git(dir: &Path, args: &[&str]) -> String {
    let dir_str = dir.to_str().expect("utf8 dir");
    let mut last_err = String::new();
    for attempt in 0..3u32 {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir_str)
            .args(args)
            .current_dir(std::env::temp_dir())
            // Fixed author/committer so tests that assert on commit metadata
            // (blame, diff-model `show`) get a stable name/email.
            .env("GIT_AUTHOR_NAME", "Ada")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_NAME", "Ada")
            .env("GIT_COMMITTER_EMAIL", "ada@example.com")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("spawn git");
        if out.status.success() {
            return String::from_utf8(out.stdout).expect("utf8 stdout");
        }
        last_err = String::from_utf8_lossy(&out.stderr).into_owned();
        std::thread::sleep(std::time::Duration::from_millis(
            50 * u64::from(attempt + 1),
        ));
    }
    panic!("git {args:?} failed after 3 attempts: {last_err}");
}
