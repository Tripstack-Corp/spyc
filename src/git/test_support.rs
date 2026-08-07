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

/// Environment variables through which an ambient git process redirects a
/// child at a different repository. **`GIT_DIR` overrides `-C`**, so a test
/// that carefully passes `-C <tempdir>` still operates on whatever these name.
///
/// This is not theoretical. `git` exports `GIT_DIR` and `GIT_INDEX_FILE` into
/// hook processes, so a `cargo test` launched from the pre-commit hook has
/// every one of its scratch-repo commands silently retargeted at the real
/// repository — writing its config, and staging into its index. Observed
/// 2026-08-07: a full suite run under the hook wrote `core.bare = true` into
/// the developer's checkout (breaking `git status` outright, which in turn
/// blanked spyc's own git markers), corrupted the worktree index, and failed
/// 99 tests. The 2026-07-02 `index.lock: Not a directory` failure recorded
/// above was almost certainly the same leak, misread as contention and papered
/// over by widening the retry budget.
const GIT_REDIRECT_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_PREFIX",
    "GIT_CEILING_DIRECTORIES",
];

/// A `git` command targeting `dir`, with the ambient repo environment stripped
/// and a hermetic config. Every test-side git spawn must go through this —
/// `-C` alone is not enough (see [`GIT_REDIRECT_ENV`]).
pub fn git_command(dir: &Path) -> std::process::Command {
    // GIT-ENV-EXEMPT: this IS the stripping site — it clears GIT_REDIRECT_ENV in a
    // loop below rather than naming GIT_DIR literally.
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        // Pin the process cwd to a stable, never-deleted dir: sibling tests
        // `set_current_dir` and drop their tempdirs, which can transiently
        // invalidate an inherited cwd mid-spawn.
        .current_dir(std::env::temp_dir())
        // Fixed author/committer so tests that assert on commit metadata
        // (blame, diff-model `show`) get a stable name/email.
        .env("GIT_AUTHOR_NAME", "Ada")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_NAME", "Ada")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    for var in GIT_REDIRECT_ENV {
        cmd.env_remove(var);
    }
    cmd
}

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
    let mut last_err = String::new();
    for attempt in 0..RUN_GIT_MAX_ATTEMPTS {
        let out = git_command(dir).args(args).output().expect("spawn git");
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

#[cfg(test)]
mod tests {
    use super::{GIT_REDIRECT_ENV, git_command};

    #[test]
    fn git_command_strips_the_ambient_repo_redirect() {
        // `GIT_DIR` overrides `-C`, so a hook-launched `cargo test` would
        // retarget every scratch-repo command at the real repository. Asserted
        // on the built command rather than by mutating the process
        // environment: `std::env::set_var` is unsafe in edition 2024, and the
        // parallel suite shares one environment anyway.
        let cmd = git_command(std::path::Path::new("/tmp"));
        let removed: Vec<&str> = cmd
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .filter_map(|(k, _)| k.to_str())
            .collect();
        for var in GIT_REDIRECT_ENV {
            assert!(
                removed.contains(var),
                "{var} must be cleared — `-C` does not override it; got {removed:?}"
            );
        }
    }

    #[test]
    fn git_dir_really_does_beat_dash_c() {
        // Characterizes the hazard the removal above defends against, so the
        // reason for it can't be lost. Two throwaway repos: `-C` names one,
        // `GIT_DIR` names the other, and `GIT_DIR` wins.
        let tmp = tempfile::tempdir().unwrap();
        let decoy = tmp.path().join("decoy");
        let target = tmp.path().join("target");
        for p in [&decoy, &target] {
            std::fs::create_dir(p).unwrap();
            super::run_git(p, &["init", "-q", "--initial-branch=main"]);
        }
        // GIT-ENV-EXEMPT: deliberately unstripped — this reproduces the ambient leak
        // in order to prove it exists.
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(&target)
            .args(["config", "spyc.probe", "leaked"])
            .env("GIT_DIR", decoy.join(".git"))
            .status()
            .expect("spawn git")
            .success();
        assert!(ok, "probe write must succeed");
        assert_eq!(
            super::run_git(&decoy, &["config", "--get", "spyc.probe"]).trim(),
            "leaked",
            "GIT_DIR should have captured the write"
        );
        // GIT-ENV-EXEMPT: the read-back of the probe above; stripping is irrelevant
        // here and adding it would obscure what the test demonstrates.
        let target_val = std::process::Command::new("git")
            .arg("-C")
            .arg(&target)
            .args(["config", "--get", "spyc.probe"])
            .output()
            .expect("spawn git");
        assert!(
            !target_val.status.success(),
            "the -C target must NOT have been written — that is the whole hazard"
        );
    }
}
