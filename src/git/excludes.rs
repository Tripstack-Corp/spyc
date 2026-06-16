//! Gitignore-aware path exclusion via gix's worktree exclude stack.
//!
//! Used to drop FSEvents under gitignored build/cache dirs (`target/`,
//! `fuzz/target`, `node_modules/`, `.claude/`, …) so their churn never
//! triggers a listing / git-status refresh. The recursive FSEvents watch
//! can't skip those subtrees itself (macOS hands us one stream for the whole
//! tree), so we filter at ingest time. git-faithful: honors nested
//! `.gitignore`, `.git/info/exclude`, and `core.excludesFile`. Built once per
//! event batch (the exclude stack borrows the opened repo, so it can't be
//! cached across calls).

use std::path::Path;

/// Open the repo at `repo_root`, build its exclude stack once, and hand a
/// `is_excluded(absolute_path) -> bool` checker to `f`.
///
/// Returns `None` (without calling `f`) when the repo / worktree / exclude
/// stack can't be built — callers treat that as "don't filter" (fail open:
/// better to over-refresh than to silently drop a real change).
pub fn with_checker<R>(
    repo_root: &Path,
    f: impl FnOnce(&mut dyn FnMut(&Path) -> bool) -> R,
) -> Option<R> {
    let repo = gix::open(repo_root).ok()?;
    // `index_or_empty` tolerates a fresh repo with no `.git/index` yet (the
    // `Worktree::excludes()` shortcut errors there). Gitignore rules come from
    // the `.gitignore` files + `.git/info/exclude` + `core.excludesFile`
    // regardless of the index, so an empty one is fine.
    let index = repo.index_or_empty().ok()?;
    let mut stack = repo
        .excludes(
            &index,
            None,
            gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
        )
        .ok()?;
    let mut is_excluded = |abs: &Path| -> bool {
        let Ok(rel) = abs.strip_prefix(repo_root) else {
            return false; // outside the repo — not ours to ignore
        };
        if rel.as_os_str().is_empty() {
            return false; // the repo root itself
        }
        // Pass the leaf's mode so a bare ignored *directory* (`target/`) is
        // matched, not just files under it. A missing path (a delete event)
        // → `None` (assume file); files under an ignored dir are still
        // reported excluded because `at_path` matches ancestor rules on the
        // way down, regardless of the leaf's mode.
        let mode = std::fs::symlink_metadata(abs).ok().map(|m| {
            if m.is_dir() {
                gix::index::entry::Mode::DIR
            } else {
                gix::index::entry::Mode::FILE
            }
        });
        matches!(stack.at_path(rel, mode), Ok(platform) if platform.is_excluded())
    };
    Some(f(&mut is_excluded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_top_level_and_nested_gitignored_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        gix::init(root).unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n.claude\nnode_modules/\n").unwrap();
        std::fs::create_dir_all(root.join("fuzz")).unwrap();
        // NESTED ignore — the case a root-only matcher misses.
        std::fs::write(root.join("fuzz/.gitignore"), "target/\n").unwrap();
        std::fs::create_dir_all(root.join("target/deps")).unwrap();
        std::fs::create_dir_all(root.join("fuzz/target/x")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();

        with_checker(root, |is_excluded| {
            // Top-level gitignored dir + a file deep under it.
            assert!(is_excluded(&root.join("target")));
            assert!(is_excluded(&root.join("target/deps/foo.o")));
            assert!(is_excluded(&root.join(".claude/history.jsonl")));
            assert!(is_excluded(&root.join("node_modules/pkg/index.js")));
            // NESTED gitignored dir (via fuzz/.gitignore) — the bug case.
            assert!(is_excluded(&root.join("fuzz/target")));
            assert!(is_excluded(&root.join("fuzz/target/x/y.bin")));
            // Real source — NOT excluded.
            assert!(!is_excluded(&root.join("src/main.rs")));
            assert!(!is_excluded(&root.join("Cargo.toml")));
            assert!(!is_excluded(&root.join("fuzz/fuzz_targets/a.rs")));
        })
        .expect("exclude checker should build for a fresh repo");
    }
}
