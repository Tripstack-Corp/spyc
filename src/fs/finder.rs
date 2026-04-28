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

/// Batch size for the streaming walker. Tuned for "rare enough that
/// channel overhead is negligible, frequent enough that the picker
/// updates feel live on a 100K-file repo."
const STREAM_BATCH: usize = 256;

/// Walk `root` honoring gitignore + standard hidden-file rules,
/// streaming repo-relative paths through `tx` in batches. Designed
/// to run in a background thread so the picker stays interactive
/// while a large monorepo is being enumerated.
///
/// Multi-repo handling: if `root` is itself a git repo, the walk
/// runs in two passes. Pass 1 is a standard gitignore-aware walk
/// from `root`. Pass 2 looks for *nested* git repos under `root`
/// that pass 1 skipped because the outer repo's `.gitignore`
/// excluded them (the common "sibling clones living inside a
/// parent dir" layout, e.g. `tripstack_platform/.gitignore`
/// excluding `book-org/` even though `book-org/` is a separate
/// checkout the user wants to find files in). Each found subrepo
/// is then walked with its own gitignore context. Without pass 2,
/// `F` in such a parent dir misses everything outside the outer
/// repo's tracked tree.
///
/// Cancellation: when the receiver is dropped (e.g. user closes
/// the picker), `tx.send` fails and we exit cleanly without
/// finishing the walk -- no lingering threads.
pub fn walk_streaming(root: &Path, tx: std::sync::mpsc::Sender<Vec<PathBuf>>) {
    let mut count = 0usize;

    // Pass 1: standard walk from the requested root.
    if !walk_one(root, root, &tx, &mut count) {
        return;
    }

    // Pass 2: only meaningful when root is itself a git repo --
    // find sibling-clone-style nested .git directories that pass 1's
    // gitignore would have masked, and walk each as its own root.
    if root.join(".git").is_dir() {
        for extra in find_nested_git_repos(root) {
            if !walk_one(&extra, root, &tx, &mut count) {
                return;
            }
        }
    }
}

/// Single-repo walk loop, factored out so `walk_streaming` can run
/// it on the original root and on each nested-repo root. Reports
/// paths *relative to* `display_root` so the picker UI shows
/// consistent prefixes regardless of which pass produced them.
/// Returns false on receiver disconnect or cap-hit so the caller
/// knows to bail.
fn walk_one(
    walk_root: &Path,
    display_root: &Path,
    tx: &std::sync::mpsc::Sender<Vec<PathBuf>>,
    count: &mut usize,
) -> bool {
    let walker = ignore::WalkBuilder::new(walk_root)
        .standard_filters(true) // gitignore + hidden + .git/
        .max_filesize(None)
        .parents(false) // don't pull in gitignores from above walk_root
        .build();
    let mut batch: Vec<PathBuf> = Vec::with_capacity(STREAM_BATCH);
    for entry in walker {
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_some_and(|t| t.is_file()) {
            let path = entry.path();
            let rel = path
                .strip_prefix(display_root)
                .unwrap_or(path)
                .to_path_buf();
            batch.push(rel);
            *count += 1;
            if batch.len() >= STREAM_BATCH {
                if tx.send(std::mem::take(&mut batch)).is_err() {
                    return false;
                }
                batch = Vec::with_capacity(STREAM_BATCH);
            }
            if *count >= MAX_CANDIDATES {
                if !batch.is_empty() {
                    let _ = tx.send(batch);
                }
                return false;
            }
        }
    }
    if !batch.is_empty() && tx.send(batch).is_err() {
        return false;
    }
    true
}

/// Scan `root`'s subtree (without gitignore filtering) for
/// directories that contain a `.git/`. Returns the parent paths.
/// Stops descending once a `.git/` is found (anything below it
/// belongs to that repo, which the caller will walk with proper
/// gitignore context). Skips `root` itself (already walked in
/// pass 1) plus a small set of well-known noise dirs to avoid
/// pointless descent into build/dependency trees.
fn find_nested_git_repos(root: &Path) -> Vec<PathBuf> {
    const SKIP: &[&str] = &[
        "node_modules",
        "target",
        "build",
        "dist",
        "_build",
        ".next",
        ".cache",
        "__pycache__",
        "venv",
        ".venv",
    ];
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        // Don't re-add the original root -- pass 1 already covered it.
        if dir != root && dir.join(".git").exists() {
            found.push(dir);
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') || SKIP.contains(&name_str.as_ref()) {
                continue;
            }
            stack.push(entry.path());
        }
    }
    found
}

/// Synchronous wrapper: walk + rank in one call. Used by the MCP
/// `search_paths` tool which has a single-shot request/response
/// shape (no streaming UI). Returns the top `limit` paths, scored
/// against `query` (empty query = natural walk order, truncated).
pub fn find_paths(root: &Path, query: &str, limit: usize) -> Vec<PathBuf> {
    let (tx, rx) = std::sync::mpsc::channel();
    let walk_root = root.to_path_buf();
    std::thread::spawn(move || walk_streaming(&walk_root, tx));
    let mut all = Vec::new();
    while let Ok(batch) = rx.recv() {
        all.extend(batch);
    }
    rank(&all, query, limit)
        .into_iter()
        .map(|(p, _)| p)
        .collect()
}

/// Synchronous wrapper around `walk_streaming` -- spawns a thread,
/// drains the channel, returns the full list. Test-only; the
/// production picker uses `walk_streaming` directly so it can
/// progressively render results as the walk runs.
#[cfg(test)]
fn walk(root: &Path) -> Vec<PathBuf> {
    let (tx, rx) = std::sync::mpsc::channel();
    let walk_root = root.to_path_buf();
    std::thread::spawn(move || walk_streaming(&walk_root, tx));
    let mut out = Vec::new();
    while let Ok(batch) = rx.recv() {
        out.extend(batch);
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

    #[test]
    fn walk_streaming_emits_at_least_one_batch_and_disconnects() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        // Create enough files to ensure at least one batch ships.
        for i in 0..STREAM_BATCH + 5 {
            File::create(root.join(format!("f{i:04}"))).unwrap();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || walk_streaming(&root, tx));
        let mut total = 0;
        let mut batch_count = 0;
        while let Ok(batch) = rx.recv() {
            total += batch.len();
            batch_count += 1;
        }
        // Channel closed once walk completed -- thread dropped tx.
        assert!(batch_count >= 2, "expected ≥2 batches, got {batch_count}");
        assert_eq!(total, STREAM_BATCH + 5);
    }

    #[test]
    fn walk_descends_into_sibling_clone_under_gitignored_dir() {
        // Real-world repro: a parent repo whose .gitignore excludes
        // a sibling-clone subdir (the dir contains its own .git).
        // Pass 1 alone would skip everything under `sibling/` because
        // the outer .gitignore says so; pass 2 should pick it up by
        // detecting the nested .git boundary.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        // Outer repo at root.
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".gitignore"), "sibling/\n").unwrap();
        File::create(root.join("outer_kept.txt")).unwrap();
        // Sibling clone -- its own git repo, not part of outer's tracked tree.
        std::fs::create_dir_all(root.join("sibling/.git")).unwrap();
        File::create(root.join("sibling/inner_kept.txt")).unwrap();
        // Sibling has its own gitignore; should still be honored
        // within its own tree.
        std::fs::write(root.join("sibling/.gitignore"), "inner_skip.txt\n").unwrap();
        File::create(root.join("sibling/inner_skip.txt")).unwrap();

        let paths: Vec<String> = walk(root)
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert!(paths.iter().any(|s| s == "outer_kept.txt"));
        // The sibling repo's tracked file shows up via pass 2.
        assert!(
            paths.iter().any(|s| s == "sibling/inner_kept.txt"),
            "sibling clone's tracked file missed; got {paths:?}"
        );
        // The sibling's *own* gitignore still kicks in within its tree.
        assert!(
            !paths.iter().any(|s| s == "sibling/inner_skip.txt"),
            "sibling's own gitignore was not honored; got {paths:?}"
        );
    }

    #[test]
    fn walk_streaming_stops_when_receiver_drops() {
        // Cancellation contract: dropping the receiver makes the
        // walker thread exit on its next `tx.send` attempt without
        // finishing the walk. We can't directly observe the thread
        // exiting, but we can verify it doesn't hang: spawn it,
        // drop the receiver immediately, join with a timeout via
        // a side channel.
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        for i in 0..STREAM_BATCH * 4 {
            File::create(root.join(format!("f{i:04}"))).unwrap();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            walk_streaming(&root, tx);
            let _ = done_tx.send(());
        });
        drop(rx); // immediate cancel
        // Walker should exit promptly. Allow generous slack since
        // ignore's threadpool startup adds latency.
        let result = done_rx.recv_timeout(std::time::Duration::from_secs(5));
        assert!(result.is_ok(), "walker did not exit after rx drop");
    }
}
