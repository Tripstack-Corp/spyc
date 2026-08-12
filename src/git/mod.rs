//! Git integration facade.
//!
//! The single boundary between spyc and git. Every operation here runs
//! **in-process via gix (gitoxide)** — no `git` subprocess: discovery
//! ([`discovery`]), status ([`status`]), worktrees ([`worktree`]), and
//! diff / show / blame models ([`diff_model`], [`blame`], [`model`]). The
//! facade is pure infrastructure: paths in, owned `Send` data out. It has no
//! `App` dependency and never touches ratatui, so `app` depends on `git` and
//! never the reverse (the AGENTS.md one-way dependency rule).
//!
//! Production code no longer shells out to the `git` binary at all; the only
//! remaining git-subprocess usages are `#[cfg(test)]` fixtures that
//! *construct* throwaway repos to test the gix code against. The
//! [`no_subprocess_git_in_production`] guard test enforces that.

pub mod blame;
pub mod branch;
pub mod diff_model;
pub mod discovery;
pub mod excludes;
pub mod log;
pub mod model;
pub mod restore;
pub mod status;
pub mod worktree;

#[cfg(test)]
pub mod test_support;

/// Render an error with every `source()` behind it, so a wrapper whose Display
/// omits the cause still reports one.
///
/// gix's error types nest several layers deep and the outermost Display is
/// routinely the useless half ("could not open repository"), which is why the
/// facade formats causes rather than passing `{e}` up. The app-layer equivalent
/// is anyhow's `{e:#}` (see the `flashed_errors_render_their_whole_chain`
/// guard); this is the same rule for the non-anyhow half.
pub fn error_chain(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut src = e.source();
    while let Some(cause) = src {
        out.push_str(" -> ");
        out.push_str(&cause.to_string());
        src = cause.source();
    }
    out
}

#[cfg(test)]
mod commit_hook_is_current {
    //! The other half of the 2026-08-07 defenses (see
    //! [`super::no_subprocess_git_in_production`]): the installed pre-commit
    //! hook is what `unset`s `GIT_REDIRECT_ENV` before `exec make check`, so
    //! *every* tool the gate invokes is covered — not just spyc's own git
    //! spawns. A checkout whose installed copy predates that line has no
    //! protection and no way to tell.
    //!
    //! Nothing compared the two: `make install-hooks` is a bare `install -m
    //! 755` with no stamp, `make check` didn't look, and the only instruction
    //! was a sentence of prose in AGENTS.md. The drift was live in this
    //! repository's own checkout when this test was written — comment-only, and
    //! therefore harmless, but a missing `unset` would have looked identical.
    use std::path::{Path, PathBuf};

    /// Where git keeps this repository's hooks, or `None` when that can't be
    /// answered — an unpacked source tree with no `.git`, or a `core.hooksPath`
    /// pointing somewhere else. Resolved through the **common** dir so the test
    /// finds the one installed hook from a linked worktree too.
    fn hooks_dir() -> Option<PathBuf> {
        let repo = gix::discover(Path::new(env!("CARGO_MANIFEST_DIR"))).ok()?;
        Some(repo.common_dir().join("hooks"))
    }

    /// A hook is installed and is **byte-identical** to the tracked template.
    ///
    /// Skipped when none is installed: a fresh clone and CI are both legitimate
    /// states, and failing there would make `make check` unrunnable before the
    /// first `make install-hooks`. What it catches is the state that has no
    /// other signal — an installed hook that has since fallen behind.
    #[test]
    fn the_installed_hook_matches_its_template() {
        let template = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/git-hooks/pre-commit");
        let Some(installed) = hooks_dir().map(|d| d.join("pre-commit")) else {
            return;
        };
        let Ok(have) = std::fs::read(&installed) else {
            return; // no hook installed — see the doc comment
        };
        let want = std::fs::read(&template).expect("the tracked hook template");
        assert!(
            have == want,
            "{} has drifted from scripts/git-hooks/pre-commit — re-run \
             `make install-hooks` (from the main checkout or any worktree). \
             The hook is what strips the ambient git environment before the \
             gate runs; a stale copy silently removes that protection.",
            installed.display()
        );
    }
}

#[cfg(test)]
mod no_subprocess_git_in_production {
    //! Strangler-fig closing guard: production code must never spawn the `git`
    //! binary — every git operation runs in-process via gix. Test fixtures may
    //! still use `git` to build scratch repos, so we scan only the portion of
    //! each source file *before* its first `#[cfg(test)]` marker (tests live at
    //! the bottom of the file, the house convention).
    use std::path::Path;

    fn scan(dir: &Path, offenders: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read src dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                scan(&path, offenders);
            } else if path.extension().is_some_and(|e| e == "rs") {
                // Skip whole-file test modules — those reached via
                // `#[cfg(test)] mod …;` carry no in-file `#[cfg(test)]` marker,
                // so the split heuristic below would misread them as production.
                // The campaign's convention: `tests.rs`, `*_tests.rs`, or any
                // file under a `tests/` directory — or a `*_tests/` directory
                // (e.g. `harness_tests/` split into thematic submodules, whose
                // fixtures legitimately spawn `git`). Test fixtures may use `git`.
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let in_tests_dir = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n == "tests" || n.ends_with("_tests"));
                if name == "tests.rs"
                    || name == "test_support.rs"
                    || name.ends_with("_tests.rs")
                    || in_tests_dir
                {
                    continue;
                }
                let src = std::fs::read_to_string(&path).expect("read .rs");
                // Production portion = everything before the first cfg(test).
                let production = crate::guard_support::production_half(&src);
                if production.contains(GIT_SPAWN) {
                    offenders.push(path.display().to_string());
                }
            }
        }
    }

    // Split so this literal doesn't itself trip the scan if mod.rs ever moved
    // its tests; also keeps the intent obvious.
    const GIT_SPAWN: &str = concat!("Command::new(", "\"git\")");

    #[test]
    fn production_code_never_spawns_git() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        scan(&src, &mut offenders);
        assert!(
            offenders.is_empty(),
            "production code must use gix, not the `git` subprocess — offenders: {offenders:?}"
        );
    }

    /// A test-side git spawn must not be redirectable by the ambient
    /// environment. `GIT_DIR` overrides `-C`, so a `cargo test` launched from
    /// the pre-commit hook otherwise retargets every scratch-repo command at
    /// the developer's real repository — which is exactly what happened on
    /// 2026-08-07 (`core.bare = true` written into the checkout, the worktree
    /// index corrupted, 99 tests failed). Scans ALL source, tests included:
    /// unlike the guard above, test code is precisely the subject here.
    ///
    /// A site satisfies this by going through `test_support::git_command`, or by
    /// carrying `GIT-ENV-EXEMPT` with a reason.
    ///
    /// It used to accept a hand-rolled `env_remove("GIT_DIR")` instead, which is
    /// **one of the nine** variables `GIT_REDIRECT_ENV` names. Ten sites took it
    /// up on that and stripped three; the guard passed all ten while
    /// `GIT_OBJECT_DIRECTORY` and `GIT_ALTERNATE_OBJECT_DIRECTORIES` — which
    /// apply independently of `GIT_DIR` and would send a scratch repo's writes
    /// into the developer's real object store — went through untouched. Naming
    /// the list here would just be a tenth copy of it, so the requirement is now
    /// the *function* that owns it.
    fn spawn_sites_missing_env_hygiene(src_root: &Path) -> Vec<String> {
        const BEHIND: usize = 300;
        const EXEMPT: &str = concat!("GIT-ENV", "-EXEMPT");
        let mut offenders = Vec::new();
        let mut stack = vec![src_root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read src dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("read .rs");
                let mut from = 0;
                while let Some(rel) = text[from..].find(GIT_SPAWN) {
                    let at = from + rel;
                    // Look behind as well as ahead: an exemption is naturally
                    // written as a comment above the spawn, and the stripping
                    // calls come after it.
                    let start = at.saturating_sub(BEHIND);
                    let window = &text[start..at];
                    if !window.contains(EXEMPT) {
                        let line = text[..at].matches('\n').count() + 1;
                        offenders.push(format!("{}:{line}", path.display()));
                    }
                    from = at + GIT_SPAWN.len();
                }
            }
        }
        offenders.sort();
        offenders
    }

    #[test]
    fn every_test_git_spawn_resists_an_ambient_git_dir() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let offenders = spawn_sites_missing_env_hygiene(&src);
        assert!(
            offenders.is_empty(),
            "these git spawns can be retargeted by an inherited git environment — \
             build them with `git::test_support::git_command`, which strips all of \
             GIT_REDIRECT_ENV, or mark the site GIT-ENV-EXEMPT with a reason. \
             Offenders: {offenders:?}"
        );
    }
}
