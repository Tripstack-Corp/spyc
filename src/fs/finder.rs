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
/// Cancellation: when the receiver is dropped (e.g. user closes
/// the picker), `tx.send` fails and we exit cleanly without
/// finishing the walk -- no lingering threads.
///
/// Returns when: the walk completes, the cap is hit, or the
/// receiver disconnects. The sender drops on return; the receiver
/// sees `TryRecvError::Disconnected` and knows the walk is done.
pub fn walk_streaming(root: &Path, tx: std::sync::mpsc::Sender<Vec<PathBuf>>) {
    let walker = ignore::WalkBuilder::new(root)
        .standard_filters(true) // gitignore + hidden + .git/
        .max_filesize(None)
        .build();
    let mut batch: Vec<PathBuf> = Vec::with_capacity(STREAM_BATCH);
    let mut count = 0usize;
    for entry in walker {
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_some_and(|t| t.is_file()) {
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
            batch.push(rel);
            count += 1;
            if batch.len() >= STREAM_BATCH {
                if tx.send(std::mem::take(&mut batch)).is_err() {
                    return; // receiver dropped -- picker closed
                }
                batch = Vec::with_capacity(STREAM_BATCH);
            }
            if count >= MAX_CANDIDATES {
                break;
            }
        }
    }
    if !batch.is_empty() {
        let _ = tx.send(batch);
    }
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
