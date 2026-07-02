//! Shared `#[cfg(test)]` git fixtures for the git module's unit tests.
//!
//! Production git is pure gix (enforced by `no_subprocess_git_in_production`);
//! these spawn the `git` binary only to build scratch repos for tests.
//! Consolidated from five near-identical per-module copies — two of which
//! (`status`, `discovery`) predated the CWD-thrash hardening below, so they
//! were the flaky ones under the parallel suite.

use std::path::Path;

/// Retry budget for [`run_git`]. Observed failure (2026-07-02, `make check`'s
/// pre-commit hook): `git add` on a just-materialized worktree hit `Unable to
/// create '.../.git/index.lock': Not a directory` after exhausting the old
/// 3-attempt/300ms budget — did not reproduce over 5 clean re-runs at normal
/// parallelism, so it's genuine transient contention (this codebase's other
/// `set_current_dir` call sites — `app/bootstrap.rs`, `app/state/listing.rs`
/// — mutate the process-wide cwd from sibling test threads; under heavy
/// parallel `cargo test`, plus any concurrent `cargo build`/`test` from a
/// sibling worktree on the same machine, macOS's temp-volume metadata cache
/// can transiently misreport on an unrelated, otherwise-valid path), not a
/// logic bug — widen the budget rather than chase an unreproducible one-off.
const RUN_GIT_MAX_ATTEMPTS: u32 = 6;

/// Run `git` against `dir` with a hermetic config (no user/system
/// `.gitconfig`), returning stdout.
///
/// Hardening for the parallel test suite: pass the operation dir via
/// `-C <dir>` and pin the *process* cwd to a stable, never-deleted
/// `temp_dir()`. Sibling tests `set_current_dir` and drop their tempdirs,
/// which can transiently invalidate an inherited cwd mid-spawn, or (rarer)
/// heavy concurrent filesystem churn on the temp volume can transiently
/// misreport on an unrelated path; retry with backoff to ride out either.
pub fn run_git(dir: &Path, args: &[&str]) -> String {
    let dir_str = dir.to_str().expect("utf8 dir");
    let mut last_err = String::new();
    for attempt in 0..RUN_GIT_MAX_ATTEMPTS {
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
    panic!("git {args:?} failed after {RUN_GIT_MAX_ATTEMPTS} attempts: {last_err}");
}
