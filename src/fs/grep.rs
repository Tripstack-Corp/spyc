//! Project-wide content search backing the `:grep` command.
//!
//! Embedded ripgrep matcher: `grep-regex` for the regex,
//! `grep-searcher` for the line-oriented file search, `ignore` for
//! the gitignore-aware walk. No subprocess, no external rg
//! dependency. Power users with custom `~/.ripgreprc` or fancy flag
//! combinations can still drop down to `! rg foo`.
//!
//! Output shape: each hit is rendered as `path:line:col: matched
//! text` (one line per match). That's deliberately the same shape
//! `gf` / `gF` already understand from pane output, so jumping from
//! a `:grep` result into the file is free.
//!
//! Multi-repo handling mirrors `finder.rs`: pass 1 walks the start
//! root with its own gitignore, pass 2 looks for sibling-clone
//! `.git` directories the outer ignore excluded and walks each as
//! its own root. Without pass 2, `:grep foo` inside a workspace
//! parent dir would miss everything outside the outer repo's
//! tracked tree -- same bug the F finder had.
//!
//! Cancellation: when the receiver is dropped (user closes the
//! grep pager), the next batch send fails and the worker exits.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::fs::WakingSender;
use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkError, SinkMatch};

/// Hard cap on total matches before the searcher bails. A
/// pathological pattern (`.`) on a 100K-file repo would otherwise
/// stream until OOM. 5000 is plenty for navigation; the user can
/// refine the pattern if they hit the cap.
pub const MAX_MATCHES: usize = 5000;

/// Batch size for the streaming searcher. Same rationale as
/// `finder::STREAM_BATCH` -- frequent enough to feel live, rare
/// enough that channel overhead is noise.
const STREAM_BATCH: usize = 64;

/// One match. `path` is repo-relative for display; `line` and
/// `col` are 1-indexed (matches the path:line:col convention).
/// `text` is the matching line with trailing newline stripped.
#[derive(Debug, Clone)]
pub struct GrepMatch {
    pub path: PathBuf,
    pub line: u64,
    pub col: u64,
    pub text: String,
}

impl GrepMatch {
    /// Render as `path:line:col: text` -- the canonical form
    /// recognized by spyc's `gf`/`gF` pane-reference jump.
    pub fn render(&self) -> String {
        let path = self.path.display();
        format!("{path}:{}:{}: {}", self.line, self.col, self.text)
    }
}

/// Walk `root` honoring gitignore, run `pattern` against each text
/// file, stream `GrepMatch` batches through `tx`. Designed to run
/// in a worker thread; the receiver drives a live pager view.
///
/// Returns `Err` only if the regex itself failed to compile -- per-
/// file errors (binary files, permissions) are silently skipped
/// like ripgrep's defaults.
pub fn search_streaming(
    root: &Path,
    pattern: &str,
    tx: WakingSender<Vec<GrepMatch>>,
) -> Result<(), String> {
    let matcher = RegexMatcherBuilder::new()
        .case_smart(true)
        .build(pattern)
        .map_err(|e| format!("invalid regex: {e}"))?;

    let mut count = 0usize;

    if !search_one(root, root, &matcher, &tx, &mut count) {
        return Ok(());
    }

    // `.exists()` (not `.is_dir()`): a worktree/submodule checkout has
    // `.git` as a gitdir-pointer file, but is still a repo root whose
    // pass-2 nested-clone scan should run. Mirrors finder.rs.
    if root.join(".git").exists() {
        for extra in crate::fs::finder::find_nested_git_repos(root) {
            if !search_one(&extra, root, &matcher, &tx, &mut count) {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// One gitignore-rooted walk + per-file searcher pass. Returns
/// false when the cap is hit or the receiver disconnects so the
/// caller can bail out of the multi-repo loop.
fn search_one(
    walk_root: &Path,
    display_root: &Path,
    matcher: &grep_regex::RegexMatcher,
    tx: &WakingSender<Vec<GrepMatch>>,
    count: &mut usize,
) -> bool {
    let walker = ignore::WalkBuilder::new(walk_root)
        .standard_filters(true)
        .max_filesize(None)
        .parents(false)
        .build();
    // Match ripgrep: stop on the first NUL byte, so tracked binaries
    // (.pdf, .jar, …) don't emit raw bytes that wreck the terminal.
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .line_number(true)
        .build();
    let mut batch: Vec<GrepMatch> = Vec::with_capacity(STREAM_BATCH);
    for entry in walker {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let abs_path = entry.path();
        let rel = abs_path
            .strip_prefix(display_root)
            .unwrap_or(abs_path)
            .to_path_buf();
        let mut sink = BatchSink {
            path: &rel,
            matcher,
            batch: &mut batch,
            tx,
            count,
            cap_hit: false,
            disconnected: false,
        };
        // Per-file errors (binary, permission, IO) are intentionally
        // dropped -- ripgrep does the same; we want grep to keep
        // making progress on a broken file.
        let _ = searcher.search_path(matcher, abs_path, &mut sink);
        if sink.disconnected || sink.cap_hit {
            if !batch.is_empty() {
                let _ = tx.send(batch);
            }
            return false;
        }
        if batch.len() >= STREAM_BATCH {
            if tx.send(std::mem::take(&mut batch)).is_err() {
                return false;
            }
            batch = Vec::with_capacity(STREAM_BATCH);
        }
    }
    if !batch.is_empty() && tx.send(batch).is_err() {
        return false;
    }
    true
}

/// Sink that pushes each match into a shared batch buffer. The
/// outer loop ships the batch when it crosses STREAM_BATCH or when
/// a file completes -- whichever comes first.
struct BatchSink<'a> {
    path: &'a Path,
    matcher: &'a grep_regex::RegexMatcher,
    batch: &'a mut Vec<GrepMatch>,
    tx: &'a WakingSender<Vec<GrepMatch>>,
    count: &'a mut usize,
    cap_hit: bool,
    disconnected: bool,
}

impl Sink for BatchSink<'_> {
    type Error = SinkErr;

    fn matched(
        &mut self,
        _searcher: &Searcher,
        sink_match: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        let line_no = sink_match.line_number().unwrap_or(0);
        let bytes = sink_match.bytes();
        // Compute the 1-indexed byte column of the first match on
        // this line. `find` returns None on transient matcher
        // errors -- fall back to col 1 rather than dropping the
        // hit. Note: byte column, not codepoint -- gf/gF accepts
        // any positive integer here.
        let col = match self.matcher.find(bytes) {
            Ok(Some(m)) => m.start() as u64 + 1,
            _ => 1,
        };
        let text = sanitize_line(bytes);

        self.batch.push(GrepMatch {
            path: self.path.to_path_buf(),
            line: line_no,
            col,
            text,
        });
        *self.count += 1;
        if *self.count >= MAX_MATCHES {
            self.cap_hit = true;
            return Ok(false);
        }
        if self.batch.len() >= STREAM_BATCH {
            let drained = std::mem::take(self.batch);
            if self.tx.send(drained).is_err() {
                self.disconnected = true;
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[derive(Debug)]
struct SinkErr(String);

impl std::fmt::Display for SinkErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SinkErr {}

impl SinkError for SinkErr {
    fn error_message<T: std::fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

/// Maximum match-line width before we truncate. Long minified or
/// generated lines (sourcemap blobs, base64 inlined assets) would
/// otherwise blow up the pager and force horizontal-scroll-only.
const MAX_LINE_DISPLAY: usize = 400;

/// Convert a matched line's raw bytes into a single-line string
/// safe to render in the pager. Drops trailing CR/LF, replaces
/// control characters (NUL, ESC, BEL, etc. — including the C1 range
/// U+0080..=U+009F, where U+009B is a CSI that many terminals honor
/// exactly like `ESC [`) with `·` so they can't move the cursor or
/// set colors, expands tabs to spaces (ratatui counts `\t` as
/// zero-width but terminals expand it to 8 columns, which scrambles
/// tab-separated content like TSV postcode tables), and truncates
/// absurdly long lines with an ellipsis.
fn sanitize_line(bytes: &[u8]) -> String {
    // Strip any trailing CR/LF (handles \n, \r, \r\n alike).
    let mut end = bytes.len();
    while end > 0 && matches!(bytes[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    let trimmed = &bytes[..end];
    let mut out = String::with_capacity(trimmed.len().min(MAX_LINE_DISPLAY));
    let mut written = 0usize;
    for ch in String::from_utf8_lossy(trimmed).chars() {
        if written >= MAX_LINE_DISPLAY {
            out.push('…');
            break;
        }
        if ch == '\t' {
            // Expand to next 4-column boundary. 4 (not 8) keeps
            // result lines compact since most paths are deep.
            let pad = 4 - (written % 4);
            for _ in 0..pad {
                if written >= MAX_LINE_DISPLAY {
                    break;
                }
                out.push(' ');
                written += 1;
            }
        } else if ch.is_control() {
            // `char::is_control` is exactly the Unicode Cc category:
            // C0 (U+0000..=U+001F), DEL (U+007F), and C1
            // (U+0080..=U+009F). The old `< 0x20 || == 0x7f` test let
            // the C1 range through — and a UTF-8-encoded U+009B (CSI)
            // in a matched line would reach the terminal as a live
            // control sequence introducer. (Tab is handled above.)
            out.push('·');
            written += 1;
        } else {
            out.push(ch);
            written += 1;
        }
    }
    out
}

/// Synchronous search across `root`. Same gitignore-aware walk +
/// binary-skipping searcher as `search_streaming`, but collects all
/// matches into a vec (capped at `limit`) and returns them. Used by
/// the MCP `search_content` tool which has a single-shot
/// request/response shape.
pub fn search_to_vec(root: &Path, pattern: &str, limit: usize) -> Result<Vec<GrepMatch>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let r = root.to_path_buf();
    let p = pattern.to_string();
    // Synchronous collector — no event loop to wake (rx is drained right
    // here), so a no-op wake.
    let tx = WakingSender::new(tx, Arc::new(|| {}));
    let handle = std::thread::spawn(move || search_streaming(&r, &p, tx));
    let mut out = Vec::new();
    while let Ok(batch) = rx.recv() {
        for m in batch {
            if out.len() >= limit {
                break;
            }
            out.push(m);
        }
        if out.len() >= limit {
            break;
        }
    }
    // Drop the receiver before joining. Once we've collected `limit` matches
    // the worker is still walking the tree, and the channel is unbounded — so
    // its sends never block and it would crawl the *entire* repo before
    // `join` returned. Dropping `rx` makes the next batch send fail, so the
    // worker bails early (see the module-level "Cancellation" note) and
    // `join` returns promptly. (When the search exhausts naturally the worker
    // has already finished and dropped its sender, so this is a no-op there.)
    drop(rx);
    // Surface a regex compile error from the worker; success/non-
    // regex errors are silent (per-file IO failures already are).
    if let Ok(Err(e)) = handle.join() {
        return Err(e);
    }
    Ok(out)
}

/// Synchronous search across an explicit list of files (no walker,
/// no gitignore -- the caller already chose the set). Used by the
/// MCP `search_picks` and `search_inventory` tools which run grep
/// over a known finite set of paths instead of a tree.
pub fn search_files(
    files: &[PathBuf],
    pattern: &str,
    display_root: Option<&Path>,
    limit: usize,
) -> Result<Vec<GrepMatch>, String> {
    let matcher = RegexMatcherBuilder::new()
        .case_smart(true)
        .build(pattern)
        .map_err(|e| format!("invalid regex: {e}"))?;
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .line_number(true)
        .build();
    let mut out = Vec::new();
    for path in files {
        if out.len() >= limit {
            break;
        }
        if !path.is_file() {
            continue;
        }
        let rel = display_root
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path)
            .to_path_buf();
        // Drain matches via a one-shot channel to reuse BatchSink's
        // formatting (column lookup, sanitize_line). Bounded buffer
        // keeps memory predictable per-file.
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = WakingSender::new(tx, Arc::new(|| {}));
        let mut count = 0usize;
        let mut batch: Vec<GrepMatch> = Vec::new();
        let mut sink = BatchSink {
            path: &rel,
            matcher: &matcher,
            batch: &mut batch,
            tx: &tx,
            count: &mut count,
            cap_hit: false,
            disconnected: false,
        };
        let _ = searcher.search_path(&matcher, path, &mut sink);
        // Pull anything the sink shipped via the channel, then the
        // tail still in `batch`.
        drop(tx);
        while let Ok(b) = rx.recv() {
            for m in b {
                if out.len() >= limit {
                    break;
                }
                out.push(m);
            }
        }
        for m in batch {
            if out.len() >= limit {
                break;
            }
            out.push(m);
        }
    }
    Ok(out)
}

#[cfg(test)]
fn search(root: &Path, pattern: &str) -> Vec<GrepMatch> {
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = WakingSender::new(tx, Arc::new(|| {}));
    let r = root.to_path_buf();
    let p = pattern.to_string();
    std::thread::spawn(move || {
        let _ = search_streaming(&r, &p, tx);
    });
    let mut out = Vec::new();
    while let Ok(batch) = rx.recv() {
        out.extend(batch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write as _;
    use tempfile::tempdir;

    #[test]
    fn finds_simple_pattern() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        File::create(root.join("a.txt"))
            .unwrap()
            .write_all(b"hello world\nfoo bar\nhello again\n")
            .unwrap();
        let hits = search(root, "hello");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].line, 1);
        assert_eq!(hits[1].line, 3);
        assert_eq!(hits[0].path, PathBuf::from("a.txt"));
        assert!(hits[0].text.contains("hello"));
    }

    #[test]
    fn skips_gitignored_files() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".gitignore"), "skip.txt\n").unwrap();
        File::create(root.join("kept.txt"))
            .unwrap()
            .write_all(b"needle\n")
            .unwrap();
        File::create(root.join("skip.txt"))
            .unwrap()
            .write_all(b"needle\n")
            .unwrap();
        let hits = search(root, "needle");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, PathBuf::from("kept.txt"));
    }

    #[test]
    fn smart_case_insensitive_when_lowercase() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        File::create(root.join("a.txt"))
            .unwrap()
            .write_all(b"FooBar\nfoobar\nFOOBAR\n")
            .unwrap();
        let hits = search(root, "foobar");
        // Smart case: all-lowercase query → case-insensitive.
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn smart_case_sensitive_when_mixed() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        File::create(root.join("a.txt"))
            .unwrap()
            .write_all(b"FooBar\nfoobar\nFOOBAR\n")
            .unwrap();
        let hits = search(root, "FooBar");
        // Smart case: mixed query → case-sensitive.
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "FooBar");
    }

    #[test]
    fn render_format_matches_pane_reference_shape() {
        let m = GrepMatch {
            path: PathBuf::from("src/main.rs"),
            line: 42,
            col: 1,
            text: "fn main() {".to_string(),
        };
        assert_eq!(m.render(), "src/main.rs:42:1: fn main() {");
    }

    #[test]
    fn descends_into_sibling_clone() {
        // Same shape as finder's multi-repo test: outer repo
        // gitignores a sibling clone. Grep should still find a
        // pattern inside the sibling repo's tracked files.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".gitignore"), "sibling/\n").unwrap();
        File::create(root.join("outer.txt"))
            .unwrap()
            .write_all(b"NEEDLE in outer\n")
            .unwrap();
        std::fs::create_dir_all(root.join("sibling/.git")).unwrap();
        File::create(root.join("sibling/inner.txt"))
            .unwrap()
            .write_all(b"NEEDLE in sibling\n")
            .unwrap();
        let hits = search(root, "NEEDLE");
        let paths: Vec<String> = hits
            .iter()
            .map(|h| h.path.to_string_lossy().into_owned())
            .collect();
        assert!(paths.iter().any(|p| p == "outer.txt"));
        assert!(
            paths.iter().any(|p| p == "sibling/inner.txt"),
            "sibling-clone match missed; got {paths:?}"
        );
    }

    #[test]
    fn skips_binary_files() {
        // ripgrep-default behavior: a NUL byte in the file means
        // "stop searching this file." Without this, tracked .docx /
        // .pdf / .dll matches dump raw bytes into the pager.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        // Text file with the needle.
        File::create(root.join("a.txt"))
            .unwrap()
            .write_all(b"NEEDLE in text\n")
            .unwrap();
        // "Binary" file: contains NUL but also has the needle.
        File::create(root.join("b.bin"))
            .unwrap()
            .write_all(b"NEEDLE\0junk\nNEEDLE again\n")
            .unwrap();
        let hits = search(root, "NEEDLE");
        let paths: Vec<String> = hits
            .iter()
            .map(|h| h.path.to_string_lossy().into_owned())
            .collect();
        assert!(paths.iter().any(|p| p == "a.txt"));
        assert!(
            !paths.iter().any(|p| p == "b.bin"),
            "binary file produced matches; got {paths:?}"
        );
    }

    #[test]
    fn sanitize_strips_control_bytes_and_caps_length() {
        // Control bytes → ·, CR/LF stripped, tabs expanded to next
        // 4-col boundary. "hello"(5) + ESC→·(6) + "[31mworld"(15) +
        // tab→pad-to-16(16) + BEL→·(17). End: "hello·[31mworld ·".
        let s = sanitize_line(b"hello\x1b[31mworld\t\x07\n");
        assert_eq!(s, "hello·[31mworld ·");
        // Tab at column 0 expands to 4 spaces.
        assert_eq!(sanitize_line(b"\tabc"), "    abc");
        // Long line truncated with ellipsis.
        let mut long = vec![b'x'; super::MAX_LINE_DISPLAY + 50];
        long.push(b'\n');
        let trimmed = sanitize_line(&long);
        assert_eq!(trimmed.chars().count(), super::MAX_LINE_DISPLAY + 1);
        assert!(trimmed.ends_with('…'));
    }

    #[test]
    fn sanitize_neutralizes_c1_control_chars() {
        // C1 controls (U+0080..=U+009F) are encoded in UTF-8 as two
        // bytes (0xC2 0x8x/0x9x). U+009B is the CSI — a terminal honors
        // it like `ESC [`, so a matched line carrying one must not pass
        // through. "a"(·)"b" → the CSI between them becomes `·`.
        let s = sanitize_line("a\u{009b}31mb".as_bytes());
        assert_eq!(s, "a·31mb");
        // The whole C1 range is neutralized, not just CSI.
        assert_eq!(sanitize_line("\u{0080}\u{009f}".as_bytes()), "··");
        // A printable high-Latin-1 char (U+00E9 = é) is NOT a control
        // and must survive untouched.
        assert_eq!(sanitize_line("caf\u{00e9}".as_bytes()), "café");
    }

    #[test]
    fn search_to_vec_caps_results() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let mut f = File::create(root.join("a.txt")).unwrap();
        for _ in 0..50 {
            writeln!(f, "needle").unwrap();
        }
        let hits = search_to_vec(root, "needle", 10).unwrap();
        assert_eq!(hits.len(), 10);
    }

    /// With matches spread across many files, hitting `limit` must still return
    /// exactly `limit` valid matches — the early-drop of the receiver (which
    /// makes the worker bail before walking the whole tree) must not truncate
    /// or corrupt the collected prefix.
    #[test]
    fn search_to_vec_caps_across_many_files() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        for i in 0..40 {
            let mut f = File::create(root.join(format!("f{i:03}.txt"))).unwrap();
            writeln!(f, "needle in file {i}").unwrap();
        }
        let hits = search_to_vec(root, "needle", 5).unwrap();
        assert_eq!(hits.len(), 5, "cap is honored across a multi-file walk");
        assert!(
            hits.iter().all(|m| m.text.contains("needle")),
            "every returned match is a real hit, not a partial/garbled batch"
        );
    }

    #[test]
    fn search_files_only_explicit_set() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        File::create(root.join("a.txt"))
            .unwrap()
            .write_all(b"NEEDLE here\n")
            .unwrap();
        File::create(root.join("b.txt"))
            .unwrap()
            .write_all(b"NEEDLE there\n")
            .unwrap();
        // Restrict to a.txt; b.txt should not appear.
        let only = vec![root.join("a.txt")];
        let hits = search_files(&only, "NEEDLE", Some(root), 100).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, PathBuf::from("a.txt"));
    }

    #[test]
    fn search_files_invalid_regex_errors() {
        let result = search_files(&[], "[unterminated", None, 10);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_regex_returns_err() {
        let tmp = tempdir().unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        let tx = WakingSender::new(tx, Arc::new(|| {}));
        let result = search_streaming(tmp.path(), "[unterminated", tx);
        assert!(result.is_err());
    }

    #[test]
    fn dropping_receiver_stops_searcher() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        // Generate enough matches to force several batches.
        for i in 0..200 {
            let mut f = File::create(root.join(format!("f{i:03}.txt"))).unwrap();
            for _ in 0..10 {
                writeln!(f, "needle line").unwrap();
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = WakingSender::new(tx, Arc::new(|| {}));
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = search_streaming(&root, "needle", tx);
            let _ = done_tx.send(());
        });
        drop(rx);
        let result = done_rx.recv_timeout(std::time::Duration::from_secs(5));
        assert!(result.is_ok(), "searcher did not exit after rx drop");
    }
}
