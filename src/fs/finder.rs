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
//! 100ms cold, and matching is ~1us per candidate. The walk runs on a
//! background thread via `walk_streaming` (spawned in
//! app/find_picker.rs), streaming batches to the picker so it stays
//! live on a large monorepo; only the per-keystroke re-rank (`rank`)
//! is synchronous.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::fs::WakingSender;
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
pub fn walk_streaming(root: &Path, tx: WakingSender<Vec<PathBuf>>) {
    let mut count = 0usize;

    // Pass 1: standard walk from the requested root.
    if !walk_one(root, root, &tx, &mut count) {
        return;
    }

    // Pass 2: only meaningful when root is itself a git repo --
    // find sibling-clone-style nested .git directories that pass 1's
    // gitignore would have masked, and walk each as its own root.
    // `.exists()` (not `.is_dir()`) so a worktree/submodule checkout,
    // where `.git` is a *gitdir-pointer file* rather than a directory,
    // still qualifies as a repo root -- otherwise running `F` from
    // inside a worktree silently skips every nested sibling clone.
    // Matches the nested-repo detector below, which already uses
    // `.exists()`.
    if root.join(".git").exists() {
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
    tx: &WakingSender<Vec<PathBuf>>,
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
    // Synchronous collector — no event loop to wake (rx drained right here).
    let tx = WakingSender::new(tx, Arc::new(|| {}));
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
    let tx = WakingSender::new(tx, Arc::new(|| {}));
    let walk_root = root.to_path_buf();
    std::thread::spawn(move || walk_streaming(&walk_root, tx));
    let mut out = Vec::new();
    while let Ok(batch) = rx.recv() {
        out.extend(batch);
    }
    out
}

thread_local! {
    /// Reused nucleo scratch for `rank`. A fresh `Matcher` allocates internal
    /// scratch slabs per construction; `rank` is the synchronous per-keystroke
    /// (and per-walk-batch) path, so rebuilding it each call burned allocation
    /// on the input thread. `rank` only ever runs on the main event loop, so a
    /// thread-local is the simplest safe hoist — and if it were ever called off
    /// that thread, each thread just gets its own scratch.
    static RANK_SCRATCH: std::cell::RefCell<(Matcher, Vec<char>)> =
        std::cell::RefCell::new((Matcher::new(Config::DEFAULT.match_paths()), Vec::new()));
}

/// Rank `candidates` against `query`, returning the top `limit`
/// paths in score order (best first). An empty query returns the
/// candidates in their natural walk order, truncated. Scoring is
/// path-aware: matches that hit the basename score higher than
/// matches buried in a parent dir, which mirrors fzf-style intent.
pub fn rank(candidates: &[PathBuf], query: &str, limit: usize) -> Vec<(PathBuf, u32)> {
    if query.is_empty() || limit == 0 {
        return candidates
            .iter()
            .take(limit)
            .map(|p| (p.clone(), 0))
            .collect();
    }
    let pattern = Pattern::parse(
        query,
        nucleo_matcher::pattern::CaseMatching::Smart,
        nucleo_matcher::pattern::Normalization::Smart,
    );
    // Score into (score, candidate_index): no PathBuf clone for non-winners.
    // At the 100K-candidate cap with a short query this avoids ~100K heap
    // clones per keystroke; we clone only the surviving `limit` at the end.
    let mut scored: Vec<(u32, usize)> = RANK_SCRATCH.with_borrow_mut(|(matcher, buf)| {
        candidates
            .iter()
            .enumerate()
            .filter_map(|(i, path)| {
                let s = path.to_string_lossy();
                buf.clear();
                let haystack = Utf32Str::new(&s, buf);
                pattern.score(haystack, matcher).map(|sc| (sc, i))
            })
            .collect()
    });
    // Highest score first; tie-break by shorter path (more specific
    // typically beats deeply-nested in user intent). Captures only the
    // candidates slice by shared ref, so the closure is `Copy` and can be
    // reused for both the partial-select and the final sort.
    let by = |a: &(u32, usize), b: &(u32, usize)| {
        b.0.cmp(&a.0).then_with(|| {
            candidates[a.1]
                .as_os_str()
                .len()
                .cmp(&candidates[b.1].as_os_str().len())
        })
    };
    // Keep the top `limit` without a full sort of the whole match set:
    // `select_nth_unstable_by` partitions in O(n), then we sort only the
    // `limit` survivors (O(limit log limit)) instead of O(n log n).
    if scored.len() > limit {
        scored.select_nth_unstable_by(limit - 1, by);
        scored.truncate(limit);
    }
    scored.sort_by(by);
    scored
        .into_iter()
        .map(|(sc, i)| (candidates[i].clone(), sc))
        .collect()
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
    fn rank_partial_select_keeps_global_best_under_limit() {
        // limit < match count exercises the select_nth_unstable_by partition.
        // The best basename match must survive truncation and land at the
        // front — i.e. the partial-select keeps the *globally* top-`limit`,
        // not just whatever happened to fall in the first `limit` candidates.
        let mut candidates: Vec<PathBuf> = (0..50)
            .map(|i| PathBuf::from(format!("deep/nested/dir/zzz_{i}_match.rs")))
            .collect();
        // The strongest match is a bare basename, placed last so a naive
        // truncate-before-rank would drop it.
        candidates.push(PathBuf::from("match.rs"));
        let result = rank(&candidates, "match", 5);
        assert_eq!(result.len(), 5);
        assert_eq!(
            result[0].0,
            PathBuf::from("match.rs"),
            "the global best match must survive the top-K partial select"
        );
        // Returned slice is sorted best-first.
        assert!(result.windows(2).all(|w| w[0].1 >= w[1].1));
    }

    #[test]
    fn rank_limit_zero_returns_empty() {
        let candidates = vec![PathBuf::from("foo.rs")];
        assert!(rank(&candidates, "foo", 0).is_empty());
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
        let tx = WakingSender::new(tx, Arc::new(|| {}));
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
    fn walk_descends_into_sibling_clone_when_root_is_a_worktree() {
        // A git worktree (or submodule) checkout stores `.git` as a
        // gitdir-pointer *file*, not a directory. Pass 2 used to gate on
        // `.git` being a dir, so running `F` from inside a worktree
        // silently skipped every nested sibling clone. The outer-repo
        // detection must accept a `.git` file too.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        // Outer repo at root, but as a worktree: `.git` is a file.
        std::fs::write(root.join(".git"), "gitdir: /somewhere/.git/worktrees/wt\n").unwrap();
        std::fs::write(root.join(".gitignore"), "sibling/\n").unwrap();
        File::create(root.join("outer_kept.txt")).unwrap();
        // Sibling clone -- its own git repo, gitignored out of the outer tree.
        std::fs::create_dir_all(root.join("sibling/.git")).unwrap();
        File::create(root.join("sibling/inner_kept.txt")).unwrap();

        let paths: Vec<String> = walk(root)
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert!(paths.iter().any(|s| s == "outer_kept.txt"));
        assert!(
            paths.iter().any(|s| s == "sibling/inner_kept.txt"),
            "worktree root's nested sibling clone missed; got {paths:?}"
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
        let tx = WakingSender::new(tx, Arc::new(|| {}));
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
