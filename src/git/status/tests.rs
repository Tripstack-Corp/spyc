//! Unit tests split out of `git/status.rs`: the stable-walk guard
//! (`stable_walk_tests`), the `decode_porcelain` → `map_to_listing` path-mapping
//! rules (`map_tests`), and the gix-vs-porcelain parity cross-checks
//! (`parity_tests`). Relocated verbatim; `super::` references became
//! `super::super::` one level deeper — these submodules now sit under
//! `status::tests`, so `super` is `tests` and `super::super` is `status` (the
//! `decode_porcelain`/`decode_half` test scaffolding stays in `status.rs`,
//! adjacent to the production it mirrors). Same convention as `pane/tests.rs`.

#[cfg(test)]
mod stable_walk_tests {
    use super::super::stable_walk;
    use std::cell::Cell;

    /// Key stable across the walk → the first walk is trusted (one walk only).
    #[test]
    fn trusts_first_walk_when_key_stable() {
        let walks = Cell::new(0u32);
        let (res, key) = stable_walk(
            || 7u32,
            || {
                walks.set(walks.get() + 1);
                "entries"
            },
            3,
        );
        assert_eq!(res, "entries");
        assert_eq!(key, 7);
        assert_eq!(walks.get(), 1, "stable on the first try → exactly one walk");
    }

    /// Key moves across the first walk, then settles → the racy first result is
    /// discarded and the second (stable) walk is returned, stamped with its key.
    #[test]
    fn retries_until_key_is_stable() {
        let n = Cell::new(0u32);
        // stat sequence: before1=1, after1=2 (differ) → retry; before2=9,
        // after2=9 (same) → trust the second walk.
        let key = || {
            let i = n.get();
            n.set(i + 1);
            match i {
                0 => 1,
                1 => 2,
                _ => 9,
            }
        };
        let walks = Cell::new(0u32);
        let (_res, k) = stable_walk(
            key,
            || {
                walks.set(walks.get() + 1);
                walks.get()
            },
            3,
        );
        assert_eq!(k, 9, "stamped with the stable key");
        assert_eq!(walks.get(), 2, "one retry after the racy first walk");
    }

    /// Persistent churn (key changes across every walk) terminates after
    /// `max_tries` and falls back to the last walk's *before* key — never the
    /// post-walk key, so a stale snapshot can't be stamped as current.
    #[test]
    fn persistent_churn_falls_back_to_before_key() {
        let n = Cell::new(0u32);
        let key = || {
            let i = n.get();
            n.set(i + 1);
            i // strictly increasing → before != after on every iteration
        };
        let (res, k) = stable_walk(key, || "e", 3);
        assert_eq!(res, "e");
        // 3 iterations consume keys 0..6; befores are 0, 2, 4 → last before = 4.
        assert_eq!(
            k, 4,
            "fallback stamps the last walk's BEFORE key, not its after"
        );
    }
}

#[cfg(test)]
mod map_tests {
    //! Path-mapping rules: `decode_porcelain` → `map_to_listing`. Relocated
    //! from `sysinfo::tests` when the porcelain parser was split into the
    //! shared decode + map stages (gix flip, PR 5). These pin the
    //! prefix/basename/parent-dir-aggregation behavior that both backends
    //! share.
    use super::super::{decode_porcelain, map_to_listing};
    use crate::ui::list_view::{GitChange, GitFileStatus};
    use std::collections::HashMap;

    /// The production decode→map composition the old `parse_porcelain_statuses`
    /// performed, so the test bodies read unchanged.
    fn parse(porcelain: &str, prefix: &str) -> HashMap<String, GitFileStatus> {
        map_to_listing(&decode_porcelain(porcelain), prefix)
    }

    #[test]
    fn deep_modification_does_not_dirty_same_basename_at_root() {
        // Regression: a root listing of `git status` showing
        // `content-acquisition/AGENTS.md` modified must NOT mark a
        // separate root-level `AGENTS.md` as modified.
        let porcelain = " M content-acquisition/AGENTS.md\n";
        let map = parse(porcelain, "");
        // The deep file's basename is NOT a root entry.
        assert!(!map.contains_key("AGENTS.md"));
        // The parent dir IS marked dirty (unstaged-Modified).
        let dir_status = map.get("content-acquisition/").unwrap();
        assert_eq!(dir_status.unstaged, Some(GitChange::Modified));
        assert!(dir_status.staged.is_none());
        assert!(!dir_status.untracked);
    }

    #[test]
    fn root_modification_marks_basename() {
        // ` M` = unstaged-only modify.
        let map = parse(" M AGENTS.md\n", "");
        let s = map.get("AGENTS.md").unwrap();
        assert_eq!(s.unstaged, Some(GitChange::Modified));
        assert!(s.staged.is_none());
        assert!(!s.untracked);
    }

    #[test]
    fn root_and_deep_same_basename_uses_root_status() {
        // Both a root file and a sibling-named deep file are dirty.
        // The root entry must reflect the root file's actual status,
        // not the deep file's.
        let porcelain = "?? new.md\n M sub/new.md\n";
        let map = parse(porcelain, "");
        let new_md = map.get("new.md").unwrap();
        assert!(new_md.untracked);
        assert!(new_md.staged.is_none() && new_md.unstaged.is_none());
        assert_eq!(map.get("sub/").unwrap().unstaged, Some(GitChange::Modified));
    }

    #[test]
    fn prefix_strips_listing_dir() {
        // Listing `sub/` under a repo root: only entries under `sub/`
        // contribute, and they're keyed relative to the listing dir.
        let porcelain = " M sub/foo.txt\n M other/bar.txt\n";
        let map = parse(porcelain, "sub");
        assert_eq!(
            map.get("foo.txt").unwrap().unstaged,
            Some(GitChange::Modified)
        );
        assert!(!map.contains_key("bar.txt"));
    }

    #[test]
    fn untracked_surfaces_in_subdirectory_listing() {
        // Viewing `docs/` with untracked files in it must surface them
        // as basename-keyed untracked entries.
        let porcelain = "?? docs/PATH_HANDOFF_PLAN.md\n?? docs/TEST_IMPROVEMENT_PLAN.md\n";
        let map = parse(porcelain, "docs");
        let a = map.get("PATH_HANDOFF_PLAN.md").unwrap();
        assert!(a.untracked);
        assert!(a.staged.is_none() && a.unstaged.is_none());
        assert!(map.get("TEST_IMPROVEMENT_PLAN.md").unwrap().untracked);
    }

    #[test]
    fn untracked_only_subdir_collapses_to_untracked_dir() {
        // A subtree whose only change is untracked content marks the
        // intermediate directory `?` (untracked), not `~` (modified).
        let map = parse("?? docs/drafts/notes.md\n", "docs");
        let dir = map.get("drafts/").unwrap();
        assert!(dir.untracked);
        assert!(dir.staged.is_none() && dir.unstaged.is_none());
        assert!(!map.contains_key("notes.md"));
    }

    #[test]
    fn mixed_subdir_prefers_modified_over_untracked() {
        // A dir containing both a tracked modification and an untracked
        // file reads as changed (`~`), regardless of which row git
        // emits first — tracked outranks untracked and never downgrades.
        let untracked_first = parse("?? sub/new.md\n M sub/old.md\n", "");
        let modified_first = parse(" M sub/old.md\n?? sub/new.md\n", "");
        for map in [untracked_first, modified_first] {
            let dir = map.get("sub/").unwrap();
            assert_eq!(dir.unstaged, Some(GitChange::Modified));
            assert!(!dir.untracked);
        }
    }

    #[test]
    fn rename_takes_new_name() {
        // `R ` = staged rename, working tree clean.
        let porcelain = "R  old.md -> new.md\n";
        let map = parse(porcelain, "");
        let s = map.get("new.md").unwrap();
        assert_eq!(s.staged, Some(GitChange::Renamed));
        assert!(s.unstaged.is_none());
        assert!(!map.contains_key("old.md"));
    }

    #[test]
    fn staged_only_modify() {
        let map = parse("M  foo.rs\n", "");
        let s = map.get("foo.rs").unwrap();
        assert_eq!(s.staged, Some(GitChange::Modified));
        assert!(s.unstaged.is_none());
    }

    #[test]
    fn partially_staged_modify() {
        // `MM` — staged modify + further unstaged edits. Both halves set.
        let map = parse("MM foo.rs\n", "");
        let s = map.get("foo.rs").unwrap();
        assert_eq!(s.staged, Some(GitChange::Modified));
        assert_eq!(s.unstaged, Some(GitChange::Modified));
    }

    #[test]
    fn conflict_marks_both_halves() {
        // `UU` — both sides unmerged. We collapse to Conflicted on both.
        let map = parse("UU foo.rs\n", "");
        let s = map.get("foo.rs").unwrap();
        assert_eq!(s.staged, Some(GitChange::Conflicted));
        assert_eq!(s.unstaged, Some(GitChange::Conflicted));
    }
}

#[cfg(test)]
mod parity_tests {
    use super::super::{
        StatusEntry, decode_porcelain, map_to_listing, repo_status, repo_status_stable,
    };
    use crate::git::test_support::run_git;
    use std::path::{Path, PathBuf};

    /// Hermetic `git status --porcelain -unormal` stdout for `dir`, via the
    /// shared `run_git` fixture (so config — e.g. rename detection — matches
    /// the setup commands exactly).
    fn porcelain(dir: &Path) -> String {
        run_git(dir, &["status", "--porcelain", "-unormal"])
    }

    /// Fresh empty repo on `main`, canonicalized path (macOS /var→/private/var
    /// so gix's repo-relative paths line up). NO commit yet — callers add one.
    fn empty_repo() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap_or_else(|_| tmp.path().to_path_buf());
        run_git(&root, &["init", "-q", "--initial-branch=main"]);
        (tmp, root)
    }

    /// `empty_repo` plus one committed file `base.txt` so HEAD exists.
    fn repo_with_commit() -> (tempfile::TempDir, PathBuf) {
        let (tmp, root) = empty_repo();
        std::fs::write(root.join("base.txt"), "base\n").unwrap();
        run_git(&root, &["add", "base.txt"]);
        run_git(&root, &["commit", "-q", "-m", "c1"]);
        (tmp, root)
    }

    /// Sort entries by path for set-equality comparison (order from the two
    /// backends differs; gix runs producers in parallel threads).
    fn as_set(mut v: Vec<StatusEntry>) -> Vec<StatusEntry> {
        v.sort_by(|a, b| a.rela_path.cmp(&b.rela_path));
        v
    }

    /// The whole assertion bundle for one corpus case: gix decode == subprocess
    /// decode (as sets), AND `map_to_listing` agrees for prefix="" and a subdir
    /// prefix (isolating gix decode-correctness from the path mapping).
    fn assert_parity(root: &Path, subdir_prefix: &str) {
        let raw = porcelain(root);
        let subprocess = decode_porcelain(&raw);
        let gix = repo_status(root).expect("repo_status opens the repo");

        assert_eq!(
            as_set(gix.clone()),
            as_set(subprocess.clone()),
            "gix decode != subprocess decode\nporcelain:\n{raw}"
        );

        for prefix in ["", subdir_prefix] {
            let gix_map = map_to_listing(&gix, prefix);
            let sub_map = map_to_listing(&subprocess, prefix);
            assert_eq!(
                gix_map, sub_map,
                "map_to_listing diverged at prefix {prefix:?}\nporcelain:\n{raw}"
            );
        }
    }

    // --- Corpus: each case builds its OWN repo in its OWN tempdir. ----------

    #[test]
    fn case01_clean_repo() {
        let (_t, root) = repo_with_commit();
        let gix = repo_status(&root).expect("opens");
        assert!(gix.is_empty(), "clean repo has no entries, got {gix:?}");
        assert_parity(&root, "sub");
    }

    #[test]
    fn case02_modified_unstaged() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("base.txt"), "base\nmore\n").unwrap();
        assert_parity(&root, "sub");
    }

    #[test]
    fn case03_staged_only() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("base.txt"), "base\nstaged\n").unwrap();
        run_git(&root, &["add", "base.txt"]);
        assert_parity(&root, "sub");
    }

    #[test]
    fn case04_staged_then_modified() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("base.txt"), "base\nstaged\n").unwrap();
        run_git(&root, &["add", "base.txt"]);
        std::fs::write(root.join("base.txt"), "base\nstaged\nthen-more\n").unwrap();
        assert_parity(&root, "sub");
    }

    #[test]
    fn case05_added() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("new.txt"), "added\n").unwrap();
        run_git(&root, &["add", "new.txt"]);
        assert_parity(&root, "sub");
    }

    #[test]
    fn case06_staged_deletion() {
        let (_t, root) = repo_with_commit();
        run_git(&root, &["rm", "-q", "base.txt"]);
        assert_parity(&root, "sub");
    }

    #[test]
    fn case07_unstaged_deletion() {
        let (_t, root) = repo_with_commit();
        std::fs::remove_file(root.join("base.txt")).unwrap();
        assert_parity(&root, "sub");
    }

    #[test]
    fn case08_staged_rename() {
        let (_t, root) = repo_with_commit();
        // Give the rename real content so 50%-similarity detection fires.
        std::fs::write(root.join("orig.txt"), "line1\nline2\nline3\nline4\n").unwrap();
        run_git(&root, &["add", "orig.txt"]);
        run_git(&root, &["commit", "-q", "-m", "add orig"]);
        run_git(&root, &["mv", "orig.txt", "renamed.txt"]);
        assert_parity(&root, "sub");
    }

    #[test]
    fn case09_untracked_file() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("untracked.txt"), "u\n").unwrap();
        assert_parity(&root, "sub");
    }

    #[test]
    fn case10_fully_untracked_subdir() {
        // A wholly-untracked directory must collapse to a single `sub/` entry
        // (matching `-unormal`), not one entry per file. This requires
        // `UntrackedFiles::Collapsed` AND re-adding the trailing slash that
        // gix's `rela_path` drops for the collapsed directory — see
        // `repo_status`'s `DirectoryContents` arm.
        let (_t, root) = repo_with_commit();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("a.txt"), "a\n").unwrap();
        std::fs::write(root.join("sub").join("b.txt"), "b\n").unwrap();
        assert_parity(&root, "sub");
    }

    #[test]
    fn case11_ignored_file_absent() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join(".gitignore"), "ignored.log\n").unwrap();
        run_git(&root, &["add", ".gitignore"]);
        run_git(&root, &["commit", "-q", "-m", "gitignore"]);
        std::fs::write(root.join("ignored.log"), "noise\n").unwrap();
        let gix = repo_status(&root).expect("opens");
        assert!(
            !gix.iter().any(|e| e.rela_path == "ignored.log"),
            "ignored file must not appear: {gix:?}"
        );
        assert_parity(&root, "sub");
    }

    #[test]
    fn case12_deep_tracked_modified() {
        let (_t, root) = repo_with_commit();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("deep.txt"), "v1\n").unwrap();
        run_git(&root, &["add", "sub/deep.txt"]);
        run_git(&root, &["commit", "-q", "-m", "add deep"]);
        std::fs::write(root.join("sub").join("deep.txt"), "v1\nv2\n").unwrap();
        assert_parity(&root, "sub");
    }

    #[test]
    fn case13_empty_repo_unborn_head() {
        let (_t, root) = empty_repo();
        // No commits — unborn HEAD. An untracked file should still surface.
        std::fs::write(root.join("first.txt"), "first\n").unwrap();
        let gix = repo_status(&root).expect("repo_status works on unborn HEAD");
        assert!(
            gix.iter()
                .any(|e| e.rela_path == "first.txt" && e.untracked),
            "untracked file on unborn HEAD should surface: {gix:?}"
        );
        assert_parity(&root, "sub");
    }

    #[test]
    fn case14_detached_head_clean() {
        let (_t, root) = repo_with_commit();
        run_git(&root, &["checkout", "-q", "--detach"]);
        let gix = repo_status(&root).expect("opens detached");
        assert!(
            gix.is_empty(),
            "detached clean tree has no entries: {gix:?}"
        );
        assert_parity(&root, "sub");
    }

    /// `repo_status_stable` on a quiescent repo agrees with a direct
    /// `repo_status` and stamps the live cache-key mtimes (the happy path; the
    /// racy-snapshot handling that fixes the stale-marker bug is unit-tested in
    /// `stable_walk_tests`).
    #[test]
    fn repo_status_stable_agrees_with_repo_status() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("base.txt"), "base\nmod\n").unwrap();
        let (entries, index_mtime, head_mtime) = repo_status_stable(&root);
        assert_eq!(
            as_set(entries.expect("stable walk yields entries")),
            as_set(repo_status(&root).expect("opens")),
            "stable walk agrees with a direct walk on a quiescent repo"
        );
        assert!(
            index_mtime.is_some() && head_mtime.is_some(),
            "quiescent repo → both cache-key mtimes stamped"
        );
    }
}
