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
use std::sync::mpsc::Sender;

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
    tx: Sender<Vec<GrepMatch>>,
) -> Result<(), String> {
    let matcher = RegexMatcherBuilder::new()
        .case_smart(true)
        .build(pattern)
        .map_err(|e| format!("invalid regex: {e}"))?;

    let mut count = 0usize;

    if !search_one(root, root, &matcher, &tx, &mut count) {
        return Ok(());
    }

    if root.join(".git").is_dir() {
        for extra in find_nested_git_repos(root) {
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
    tx: &Sender<Vec<GrepMatch>>,
    count: &mut usize,
) -> bool {
    let walker = ignore::WalkBuilder::new(walk_root)
        .standard_filters(true)
        .max_filesize(None)
        .parents(false)
        .build();
    // Match ripgrep's default binary handling: stop searching a
    // file on the first NUL byte. Without this, .docx / .pdf /
    // .dll / .jar files (which `.gitignore` doesn't exclude when
    // they're tracked) emit raw bytes that wreck the terminal.
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
    tx: &'a Sender<Vec<GrepMatch>>,
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
/// control bytes (NUL, ESC, BEL, etc.) with `·` so they can't
/// move the cursor or set colors, expands tabs to spaces (ratatui
/// counts `\t` as zero-width but terminals expand it to 8 columns,
/// which scrambles tab-separated content like TSV postcode tables),
/// and truncates absurdly long lines with an ellipsis.
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
        } else if (ch as u32) < 0x20 || ch == '\u{7f}' {
            out.push('·');
            written += 1;
        } else {
            out.push(ch);
            written += 1;
        }
    }
    out
}

/// Same scan as `finder::find_nested_git_repos` -- find sibling-clone
/// `.git` directories the outer gitignore excluded. Duplicated rather
/// than shared so finder and grep can evolve independently if their
/// skip lists diverge (e.g. grep might want to skip lockfiles
/// someday).
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

#[cfg(test)]
fn search(root: &Path, pattern: &str) -> Vec<GrepMatch> {
    let (tx, rx) = std::sync::mpsc::channel();
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
    fn invalid_regex_returns_err() {
        let tmp = tempdir().unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
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
