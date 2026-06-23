//! Extract file-path references from terminal output.
//!
//! When Claude (or any tool) prints paths like `src/app.rs:172` in the
//! pane, `gf` / `gF` need to find and parse them. This module scans
//! lines of text for path-like tokens and returns the best candidate.
//!
//! Recognized patterns:
//!   - `path/to/file.rs`
//!   - `path/to/file.rs:42`         (path:line)
//!   - `path/to/file.rs:42:5`       (path:line:col — col ignored)
//!   - `./relative/path`
//!   - Paths inside backticks, quotes, or after common prefixes
//!     (Reading, Editing, Created, →, etc.)

use std::path::{Path, PathBuf};

/// A path reference extracted from terminal output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRef {
    pub path: PathBuf,
    pub line: Option<usize>,
}

/// Scan recent terminal lines for the most relevant path reference.
///
/// `lines` should be the visible screen lines (and optionally a few
/// scrollback lines), ordered top-to-bottom. We scan **bottom-up** so
/// the most recent output wins. `resolve_base` is the directory to
/// resolve relative paths against (typically the pane tab's cwd or the
/// project root).
pub fn extract_path_ref(lines: &[String], resolve_base: &Path) -> Option<PathRef> {
    // Scan bottom-to-top: most recent output is most relevant.
    for line in lines.iter().rev() {
        if let Some(pr) = extract_from_line(line, resolve_base) {
            return Some(pr);
        }
    }
    None
}

/// Extract a path reference from a single line of text.
///
/// Tries each candidate token on the line and returns the first one
/// that resolves to an existing file or directory.
pub fn extract_from_line(line: &str, resolve_base: &Path) -> Option<PathRef> {
    for candidate in candidates(line) {
        let (raw_path, line_num) = split_path_line(&candidate);
        if raw_path.is_empty() || !looks_like_path(raw_path) {
            continue;
        }
        let resolved = resolve_path(raw_path, resolve_base);
        if resolved.exists() {
            return Some(PathRef {
                path: resolved,
                line: line_num,
            });
        }
    }
    None
}

/// Split `path:line` or `path:line:col` into (path, Option<line>).
fn split_path_line(s: &str) -> (&str, Option<usize>) {
    // A *trailing* colon is never a separator — it's the punctuation
    // gcc/clang put after the location in a diagnostic
    // (`file.c:3:7: error: …`). Left in place, the `rsplitn(3, ':')` below
    // sees an empty final field and folds the real line number into the
    // path (`file.c:3`, line 7), which then fails to resolve and `gf`
    // misses the file. Strip it first.
    let s = s.trim_end_matches(':');
    // Try splitting from the right to handle `path:line:col` and `path:line`.
    // Be careful not to split Windows drive letters (C:\...) — but we don't
    // support Windows, so this is safe.
    let mut parts = s.rsplitn(3, ':');
    let _col = parts.next().unwrap_or("");
    let mid = parts.next();
    let first = parts.next();

    // path:line:col
    if let (Some(path), Some(line_str)) = (first, mid)
        && let Ok(line) = line_str.parse::<usize>()
        && line > 0
    {
        return (path, Some(line));
    }

    // path:line (two parts)
    let mut parts = s.rsplitn(2, ':');
    let maybe_line = parts.next().unwrap_or("");
    let maybe_path = parts.next();
    if let Some(path) = maybe_path
        && let Ok(line) = maybe_line.parse::<usize>()
        && line > 0
    {
        return (path, Some(line));
    }

    // No line number — whole string is the path.
    (s, None)
}

/// Generate candidate path tokens from a line of text.
///
/// Strips common decorations (backticks, quotes, ANSI prefixes,
/// leading/trailing punctuation) and yields each plausible token.
fn candidates(line: &str) -> Vec<String> {
    let mut result = Vec::new();
    let stripped = strip_ansi(line);
    let cleaned = strip_prefixes(&stripped);

    // Strategy: split on whitespace, then for each token strip decorations.
    for token in cleaned.split_whitespace() {
        let clean = strip_decorations(token);
        if !clean.is_empty() {
            result.push(clean.to_string());
        }
        // Also try extracting a path from inside parentheses/brackets
        // anywhere in the token — handles `Update(path/to/file)`,
        // `[path/to/file]`, etc.
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
            if let Some(start) = token.find(open)
                && let Some(end) = token[start..].find(close)
            {
                let inner = &token[start + 1..start + end];
                let inner = strip_decorations(inner);
                if !inner.is_empty() && !result.contains(&inner.to_string()) {
                    result.push(inner.to_string());
                }
            }
        }
    }

    result
}

/// Remove ANSI escape sequences from a string. Delegates to the
/// `strip-ansi-escapes` crate (already a dependency) rather than the former
/// handrolled CSI/OSC scanner — same intent, broader and battle-tested
/// sequence coverage.
fn strip_ansi(s: &str) -> String {
    strip_ansi_escapes::strip_str(s)
}

/// Strip common prefixes that tools emit before paths.
fn strip_prefixes(s: &str) -> &str {
    let s = s.trim();
    // Common patterns from Claude Code and other tools:
    //   "Reading src/foo.rs"
    //   "Editing src/foo.rs"
    //   "Created src/foo.rs"
    //   "Modified src/foo.rs"
    //   "Deleted src/foo.rs"
    //   "  → src/foo.rs"
    //   "--- a/src/foo.rs"
    //   "+++ b/src/foo.rs"
    for prefix in &[
        // Claude Code output patterns
        "Read ",
        "Reading ",
        "Editing ",
        "Created ",
        "Modified ",
        "Deleted ",
        "Wrote ",
        "Updated ",
        "Update ",
        "Write ",
        "create mode 100644 ",
        "create mode 100755 ",
        // Arrow/pointer prefixes
        "→ ",
        "⎿ ",
        "=> ",
        "-> ",
        // Diff headers
        "--- a/",
        "+++ b/",
        "--- ",
        "+++ ",
        "diff --git a/",
        "rename from ",
        "rename to ",
        "copy from ",
        "copy to ",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return rest.trim();
        }
    }
    s
}

/// Strip backticks, quotes, parentheses, trailing punctuation from a token.
fn strip_decorations(s: &str) -> &str {
    let mut s = s;
    // Strip matched wrapping characters.
    for (open, close) in &[('`', '`'), ('"', '"'), ('\'', '\''), ('(', ')'), ('<', '>')] {
        if s.starts_with(*open) && s.ends_with(*close) && s.len() > 1 {
            s = &s[open.len_utf8()..s.len() - close.len_utf8()];
        }
    }
    // Strip trailing punctuation that's not part of a path.
    s = s.trim_end_matches([',', ';', '.', ')', ']', '}', '>', '\'']);
    // Strip leading punctuation.
    s = s.trim_start_matches(['(', '[', '{', '<']);
    s
}

/// Heuristic: does this string look like a file path?
fn looks_like_path(s: &str) -> bool {
    // Too short to be a meaningful path.
    if s.len() < 2 {
        return false;
    }
    // Must contain a slash or a dot (to have an extension).
    // Single words like "error" or "warning" should not match.
    if s.contains('/') {
        // Reject bare "/" or strings that are just slashes/dots
        let meaningful = s.trim_matches('/');
        return !meaningful.is_empty();
    }
    // Dotfile or file with extension: ".gitignore", "foo.rs", "Cargo.toml"
    if s.contains('.') && !s.starts_with("..") {
        // Exclude things like "1.5" (pure numbers with dots)
        let has_alpha = s.chars().any(|c| c.is_ascii_alphabetic());
        return has_alpha;
    }
    false
}

/// Resolve a path string against a base directory.
fn resolve_path(raw: &str, base: &Path) -> PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn setup_tree() -> tempfile::TempDir {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("src/app")).unwrap();
        fs::write(tmp.path().join("src/app/state.rs"), "// state").unwrap();
        fs::write(tmp.path().join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]").unwrap();
        fs::write(tmp.path().join("README.md"), "# readme").unwrap();
        tmp
    }

    // ── split_path_line ───────────────────────────────────────────

    #[test]
    fn split_bare_path() {
        assert_eq!(split_path_line("src/main.rs"), ("src/main.rs", None));
    }

    #[test]
    fn split_path_with_line() {
        assert_eq!(split_path_line("src/main.rs:42"), ("src/main.rs", Some(42)));
    }

    #[test]
    fn split_path_with_line_and_col() {
        assert_eq!(
            split_path_line("src/main.rs:42:5"),
            ("src/main.rs", Some(42))
        );
    }

    #[test]
    fn split_path_zero_line_ignored() {
        assert_eq!(split_path_line("src/main.rs:0"), ("src/main.rs:0", None));
    }

    #[test]
    fn split_strips_trailing_colon_from_diagnostics() {
        // gcc/clang emit "file.c:3:7: error: …"; the path token carries the
        // trailing colon. It must not fold the line number into the path.
        assert_eq!(
            split_path_line("src/main.rs:3:7:"),
            ("src/main.rs", Some(3))
        );
        assert_eq!(
            split_path_line("src/main.rs:42:"),
            ("src/main.rs", Some(42))
        );
    }

    // ── strip_ansi ────────────────────────────────────────────────

    #[test]
    fn strips_color_codes() {
        assert_eq!(strip_ansi("\x1b[32msrc/main.rs\x1b[0m"), "src/main.rs");
    }

    #[test]
    fn strips_bold_and_reset() {
        assert_eq!(
            strip_ansi("\x1b[1;33mwarning\x1b[0m: in \x1b[36msrc/app.rs\x1b[0m"),
            "warning: in src/app.rs"
        );
    }

    // ── strip_prefixes ────────────────────────────────────────────

    #[test]
    fn strips_reading_prefix() {
        assert_eq!(strip_prefixes("Reading src/app.rs"), "src/app.rs");
    }

    #[test]
    fn strips_arrow_prefix() {
        assert_eq!(strip_prefixes("  → src/main.rs:42"), "src/main.rs:42");
    }

    #[test]
    fn strips_diff_prefix() {
        assert_eq!(strip_prefixes("--- a/src/main.rs"), "src/main.rs");
        assert_eq!(strip_prefixes("+++ b/src/main.rs"), "src/main.rs");
    }

    // ── strip_decorations ─────────────────────────────────────────

    #[test]
    fn strips_backticks() {
        assert_eq!(strip_decorations("`src/main.rs`"), "src/main.rs");
    }

    #[test]
    fn strips_quotes() {
        assert_eq!(strip_decorations("\"src/main.rs\""), "src/main.rs");
    }

    #[test]
    fn strips_trailing_punctuation() {
        assert_eq!(strip_decorations("src/main.rs,"), "src/main.rs");
        assert_eq!(strip_decorations("src/main.rs."), "src/main.rs");
    }

    // ── looks_like_path ───────────────────────────────────────────

    #[test]
    fn path_with_slash() {
        assert!(looks_like_path("src/main.rs"));
        assert!(looks_like_path("./foo"));
    }

    #[test]
    fn file_with_extension() {
        assert!(looks_like_path("Cargo.toml"));
        assert!(looks_like_path("README.md"));
    }

    #[test]
    fn plain_word_is_not_path() {
        assert!(!looks_like_path("error"));
        assert!(!looks_like_path("warning"));
    }

    #[test]
    fn number_with_dot_is_not_path() {
        assert!(!looks_like_path("1.5"));
        assert!(!looks_like_path("3.14"));
    }

    // ── extract_from_line (integration) ───────────────────────────

    #[test]
    fn extracts_bare_path() {
        let tmp = setup_tree();
        let pr = extract_from_line("src/main.rs", tmp.path()).unwrap();
        assert_eq!(pr.path, tmp.path().join("src/main.rs"));
        assert_eq!(pr.line, None);
    }

    #[test]
    fn extracts_path_with_line() {
        let tmp = setup_tree();
        let pr = extract_from_line("src/main.rs:42", tmp.path()).unwrap();
        assert_eq!(pr.path, tmp.path().join("src/main.rs"));
        assert_eq!(pr.line, Some(42));
    }

    #[test]
    fn extracts_path_in_sentence() {
        let tmp = setup_tree();
        let pr = extract_from_line("Reading src/app/state.rs", tmp.path()).unwrap();
        assert_eq!(pr.path, tmp.path().join("src/app/state.rs"));
    }

    #[test]
    fn extracts_backticked_path() {
        let tmp = setup_tree();
        let pr = extract_from_line("I modified `src/main.rs:10`", tmp.path()).unwrap();
        assert_eq!(pr.path, tmp.path().join("src/main.rs"));
        assert_eq!(pr.line, Some(10));
    }

    #[test]
    fn extracts_from_ansi_colored_output() {
        let tmp = setup_tree();
        let line = "\x1b[32mReading\x1b[0m src/main.rs:55".to_string();
        let pr = extract_from_line(&line, tmp.path()).unwrap();
        assert_eq!(pr.path, tmp.path().join("src/main.rs"));
        assert_eq!(pr.line, Some(55));
    }

    #[test]
    fn returns_none_for_no_path() {
        let tmp = setup_tree();
        assert!(extract_from_line("just some text", tmp.path()).is_none());
    }

    #[test]
    fn returns_none_for_nonexistent_path() {
        let tmp = setup_tree();
        assert!(extract_from_line("nope/not/here.rs:5", tmp.path()).is_none());
    }

    // ── extract_path_ref (multi-line) ─────────────────────────────

    #[test]
    fn prefers_most_recent_line() {
        let tmp = setup_tree();
        let lines = vec![
            "Reading src/main.rs:10".to_string(),
            "some other text".to_string(),
            "Editing src/app/state.rs:99".to_string(),
        ];
        let pr = extract_path_ref(&lines, tmp.path()).unwrap();
        // Bottom line wins (most recent)
        assert_eq!(pr.path, tmp.path().join("src/app/state.rs"));
        assert_eq!(pr.line, Some(99));
    }

    #[test]
    fn skips_lines_without_paths() {
        let tmp = setup_tree();
        let lines = vec![
            "Reading src/main.rs".to_string(),
            "Done!".to_string(),
            "All good.".to_string(),
        ];
        let pr = extract_path_ref(&lines, tmp.path()).unwrap();
        assert_eq!(pr.path, tmp.path().join("src/main.rs"));
    }

    #[test]
    fn diff_header_paths() {
        let tmp = setup_tree();
        let pr = extract_from_line("--- a/src/main.rs", tmp.path()).unwrap();
        assert_eq!(pr.path, tmp.path().join("src/main.rs"));
    }

    #[test]
    fn path_with_line_col() {
        let tmp = setup_tree();
        let pr = extract_from_line("src/main.rs:42:5", tmp.path()).unwrap();
        assert_eq!(pr.path, tmp.path().join("src/main.rs"));
        assert_eq!(pr.line, Some(42));
    }

    #[test]
    fn gcc_clang_diagnostic_with_trailing_colon() {
        let tmp = setup_tree();
        // The classic compiler diagnostic shape — `gf` must land on the file
        // at the diagnostic's line, not miss it because of the trailing colon.
        let pr = extract_from_line("src/main.rs:3:7: error: expected ';'", tmp.path()).unwrap();
        assert_eq!(pr.path, tmp.path().join("src/main.rs"));
        assert_eq!(pr.line, Some(3));
    }

    // ── Claude CLI output patterns ────────────────────────────────

    #[test]
    fn claude_update_parens() {
        let tmp = setup_tree();
        let pr = extract_from_line("⏺ Update(src/main.rs)", tmp.path()).unwrap();
        assert_eq!(pr.path, tmp.path().join("src/main.rs"));
    }

    #[test]
    fn claude_create_mode() {
        let tmp = setup_tree();
        let pr = extract_from_line("     create mode 100644 src/main.rs", tmp.path()).unwrap();
        assert_eq!(pr.path, tmp.path().join("src/main.rs"));
    }

    #[test]
    fn claude_read_file() {
        let tmp = setup_tree();
        let pr = extract_from_line("  Read src/main.rs", tmp.path()).unwrap();
        assert_eq!(pr.path, tmp.path().join("src/main.rs"));
    }

    #[test]
    fn claude_path_in_sentence() {
        let tmp = setup_tree();
        let pr = extract_from_line(
            "  Claude outputs src/app/state.rs:172 in the pane",
            tmp.path(),
        )
        .unwrap();
        assert_eq!(pr.path, tmp.path().join("src/app/state.rs"));
        assert_eq!(pr.line, Some(172));
    }

    #[test]
    fn claude_prefixed_path_with_slash() {
        let tmp = setup_tree();
        // "spyc/Cargo.toml" should match if spyc/ subdir exists
        fs::create_dir_all(tmp.path().join("spyc")).unwrap();
        fs::write(tmp.path().join("spyc/Cargo.toml"), "").unwrap();
        let pr = extract_from_line("⏺ Update(spyc/Cargo.toml)", tmp.path()).unwrap();
        assert_eq!(pr.path, tmp.path().join("spyc/Cargo.toml"));
    }

    #[test]
    fn bare_slash_is_not_a_path() {
        assert!(!looks_like_path("/"));
        assert!(!looks_like_path("//"));
    }

    #[test]
    fn short_tokens_rejected() {
        assert!(!looks_like_path("a"));
        assert!(!looks_like_path("."));
    }

    #[test]
    fn no_path_returns_none_not_root() {
        let tmp = setup_tree();
        // These lines have no valid paths — should return None
        assert!(extract_from_line("just some text with no paths", tmp.path()).is_none());
        assert!(extract_from_line("  ⎿  Added 1 line, removed 1 line", tmp.path()).is_none());
        assert!(extract_from_line("      3 -version = \"1.3.1\"", tmp.path()).is_none());
    }
}
