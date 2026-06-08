//! Git integration facade.
//!
//! The single boundary between spyc and git. Every operation here runs
//! **in-process via gix (gitoxide)** — no `git` subprocess: discovery
//! ([`discovery`]), status ([`status`]), worktrees ([`worktree`]), and
//! diff / show / blame models ([`diff_model`], [`blame`], [`model`]). The
//! facade is pure infrastructure: paths in, owned `Send` data out. It has no
//! `App` dependency and never touches ratatui, so `app` depends on `git` and
//! never the reverse (the CLAUDE.md one-way dependency rule).
//!
//! Production code no longer shells out to the `git` binary at all; the only
//! remaining git-subprocess usages are `#[cfg(test)]` fixtures that
//! *construct* throwaway repos to test the gix code against. The
//! [`no_subprocess_git_in_production`] guard test enforces that.

pub mod blame;
pub mod diff_model;
pub mod discovery;
pub mod model;
pub mod status;
pub mod worktree;

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
                // file under a `tests/` directory. Test fixtures may use `git`.
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let in_tests_dir = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    == Some("tests");
                if name == "tests.rs" || name.ends_with("_tests.rs") || in_tests_dir {
                    continue;
                }
                let src = std::fs::read_to_string(&path).expect("read .rs");
                // Production portion = everything before the first cfg(test).
                let production = src.split("#[cfg(test)]").next().unwrap_or("");
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
}
