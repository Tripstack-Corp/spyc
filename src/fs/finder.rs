//! Filename finder backing the `F` picker in the file list.
//!
//! Walks the project tree (PROJECT_HOME or the listing dir as
//! fallback) honoring `.gitignore` via the `ignore` crate, then
//! scores candidates against a fuzzy query with `nucleo-matcher`.
//! No persistent index -- the walk runs lazily on F open and the
//! result is cached on `App` for the lifetime of the picker.
//!
//! Scope is bounded:
//!
//! - Files only (directories are skipped from results; hierarchy
//!   navigation is `J` / `gh`'s job, not `F`'s).
//! - Hard cap of `MAX_CANDIDATES` (default 100K) so `F` in a
//!   monorepo doesn't load the entire kernel into RAM.
//! - Returned paths are repo-relative for display; full absolute
//!   path is reconstructed by the caller via `root.join(rel)`.
//!
//! Performance: walking + scoring on a 30K-file repo is well under
//! 100ms cold, and matching is ~1us per candidate, so per-keystroke
//! re-rank stays interactive without a worker thread. If a user
//! ever hits the cap or a slow walk, we can add a background-walk
//! worker later (the `nucleo` crate, parent of `nucleo-matcher`,
//! ships exactly that, but for v1 the synchronous path is plenty).

use std::path::{Path, PathBuf};

use nucleo_matcher::{Config, Matcher, Utf32Str, pattern::Pattern};

/// Soft cap on candidate set. A monorepo with 200K files would
/// blow past spyc's interactive ceiling; we'd rather show a
/// truncated list than freeze the UI.
pub const MAX_CANDIDATES: usize = 100_000;

/// Walk `root` honoring gitignore + standard hidden-file rules,
/// returning repo-relative paths for every regular file. Symlinks
/// are followed only at the root (gitignore convention). Hidden
/// files are excluded -- the user's `a` mask toggle is for the
/// listing, not the finder; the finder is for "find any project
/// file by fragment of name."
pub fn walk(root: &Path) -> Vec<PathBuf> {
    let walker = ignore::WalkBuilder::new(root)
        .standard_filters(true) // gitignore + hidden + .git/
        .max_filesize(None)
        .build();
    let mut out = Vec::new();
    for entry in walker {
        let Ok(entry) = entry else { continue };
        // Skip directories themselves; `ignore` yields both dirs
        // and files when descending. We only want files.
        if entry.file_type().is_some_and(|t| t.is_file()) {
            let path = entry.path();
            // Strip the root prefix for compact display. If for
            // some reason stripping fails (shouldn't, but
            // belt-and-suspenders), fall back to the raw path.
            let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
            out.push(rel);
            if out.len() >= MAX_CANDIDATES {
                break;
            }
        }
    }
    out
}

/// Rank `candidates` against `query`, returning the top `limit`
/// paths in score order (best first). An empty query returns the
/// candidates in their natural walk order, truncated. Scoring is
/// path-aware: matches that hit the basename score higher than
/// matches buried in a parent dir, which mirrors fzf-style intent.
pub fn rank(candidates: &[PathBuf], query: &str, limit: usize) -> Vec<(PathBuf, u32)> {
    if query.is_empty() {
        return candidates
            .iter()
            .take(limit)
            .map(|p| (p.clone(), 0))
            .collect();
    }
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::parse(
        query,
        nucleo_matcher::pattern::CaseMatching::Smart,
        nucleo_matcher::pattern::Normalization::Smart,
    );
    let mut buf: Vec<char> = Vec::new();
    let mut scored: Vec<(PathBuf, u32)> = candidates
        .iter()
        .filter_map(|path| {
            let s = path.to_string_lossy();
            buf.clear();
            let haystack = Utf32Str::new(&s, &mut buf);
            pattern
                .score(haystack, &mut matcher)
                .map(|sc| (path.clone(), sc))
        })
        .collect();
    // Highest score first; tie-break by shorter path (more specific
    // typically beats deeply-nested in user intent).
    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.as_os_str().len().cmp(&b.0.as_os_str().len()))
    });
    scored.truncate(limit);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write as _;
    use tempfile::tempdir;

    #[test]
    fn walk_skips_gitignored_files() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        // Make it a git repo so `ignore` sees the gitignore.
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        File::create(root.join("kept.txt")).unwrap();
        File::create(root.join("ignored.txt")).unwrap();
        let paths: Vec<String> = walk(root)
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert!(paths.iter().any(|s| s == "kept.txt"));
        assert!(!paths.iter().any(|s| s == "ignored.txt"));
        // .gitignore itself is also a file under the repo. ignore
        // crate's standard filters keep it (it's not in any
        // gitignore), and that's fine for the finder.
    }

    #[test]
    fn walk_returns_files_not_directories() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("subdir")).unwrap();
        File::create(root.join("subdir/inner.txt"))
            .unwrap()
            .write_all(b"x")
            .unwrap();
        let paths: Vec<String> = walk(root)
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert!(paths.iter().any(|s| s == "subdir/inner.txt"));
        assert!(!paths.iter().any(|s| s == "subdir")); // dir excluded
    }

    #[test]
    fn rank_empty_query_returns_natural_order_truncated() {
        let candidates: Vec<PathBuf> = (0..5)
            .map(|i| PathBuf::from(format!("file{i}.txt")))
            .collect();
        let result = rank(&candidates, "", 3);
        assert_eq!(result.len(), 3);
        // Order preserved, all scores 0.
        assert_eq!(result[0].0, PathBuf::from("file0.txt"));
        assert_eq!(result[2].0, PathBuf::from("file2.txt"));
    }

    #[test]
    fn rank_scores_basename_match_higher_than_dir_match() {
        let candidates = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("main/src/lib.rs"),
        ];
        let result = rank(&candidates, "main", 10);
        // Both should match; basename hit ("main.rs") should
        // outscore parent-dir hit ("main/...lib.rs").
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, PathBuf::from("src/main.rs"));
    }

    #[test]
    fn rank_filters_non_matches() {
        let candidates = vec![
            PathBuf::from("foo.rs"),
            PathBuf::from("bar.rs"),
            PathBuf::from("baz.rs"),
        ];
        let result = rank(&candidates, "foo", 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, PathBuf::from("foo.rs"));
    }
}
