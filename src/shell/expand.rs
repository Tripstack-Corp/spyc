//! `%` substitution for shell commands, spy-style.
//!
//! `%` in a user-typed shell command is replaced with the current
//! selection's paths, each shell-quoted and separated by spaces. A literal
//! percent sign can be produced with `%%`.
//!
//! We only generate command *strings* here — execution happens through
//! `sh -c`, so the shell parses the result.

use std::path::{Path, PathBuf};

/// A selected path whose bytes aren't valid UTF-8. We build the `sh -c`
/// command as a Rust `String` (UTF-8 by definition), so such a path
/// can't be embedded faithfully — a lossy conversion would substitute
/// U+FFFD and make `%` target a *different* file than the user picked.
/// `expand_percent` refuses rather than silently expand the wrong path.
#[derive(Debug)]
pub struct NonUtf8Path(pub PathBuf);

impl std::fmt::Display for NonUtf8Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `.display()` is itself lossy, but this is a human-facing
        // message only — the point is to name the offending entry.
        write!(
            f,
            "selection has a non-UTF-8 path ({}); can't expand %",
            self.0.display()
        )
    }
}

/// Single-quote a string for POSIX shells. Any embedded single quote is
/// escaped as `'\''` (close, escaped, reopen). Always safe.
pub fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Substitute `%` in `template` with a space-separated, shell-quoted list
/// of `targets`. `%%` is a literal percent.
///
/// Returns [`NonUtf8Path`] if any target isn't valid UTF-8: it can't be
/// represented in the `String` command we hand to `sh -c`, and a lossy
/// conversion would silently retarget `%` at a different file. The
/// happy path (valid UTF-8, i.e. every path on macOS and ~all on Linux)
/// is unchanged — `to_str()` matches the old `to_string_lossy()` there.
pub fn expand_percent(template: &str, targets: &[&Path]) -> Result<String, NonUtf8Path> {
    let mut joined = String::new();
    for (i, p) in targets.iter().enumerate() {
        let s = p.to_str().ok_or_else(|| NonUtf8Path(p.to_path_buf()))?;
        if i > 0 {
            joined.push(' ');
        }
        joined.push_str(&shell_quote(s));
    }

    // Walk the template, treating `%%` as an escape for a literal `%`.
    let mut out = String::with_capacity(template.len() + joined.len());
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            if chars.peek() == Some(&'%') {
                chars.next();
                out.push('%');
            } else {
                out.push_str(&joined);
            }
        } else {
            out.push(ch);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn quotes_plain_name() {
        assert_eq!(shell_quote("foo.txt"), "'foo.txt'");
    }

    #[test]
    fn quotes_spaces() {
        assert_eq!(shell_quote("two words"), "'two words'");
    }

    #[test]
    fn escapes_embedded_single_quote() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn expands_single_file() {
        let files = [p("foo bar.txt")];
        let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        assert_eq!(
            expand_percent("ls -la %", &refs).unwrap(),
            "ls -la 'foo bar.txt'"
        );
    }

    #[test]
    fn expands_multiple_files() {
        let files = [p("a.txt"), p("b c.txt")];
        let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        assert_eq!(
            expand_percent("cat %", &refs).unwrap(),
            "cat 'a.txt' 'b c.txt'"
        );
    }

    #[test]
    fn literal_percent_with_double() {
        let files = [p("x")];
        let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        assert_eq!(
            expand_percent("printf '%%s\\n' %", &refs).unwrap(),
            "printf '%s\\n' 'x'"
        );
    }

    #[test]
    fn multiple_occurrences() {
        let files = [p("x")];
        let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        assert_eq!(
            expand_percent("cp % %.bak", &refs).unwrap(),
            "cp 'x' 'x'.bak"
        );
    }

    #[test]
    fn no_percent_passes_through() {
        assert_eq!(expand_percent("date", &[]).unwrap(), "date");
    }

    #[test]
    #[cfg(unix)]
    fn rejects_non_utf8_path() {
        // On Unix a filename can be arbitrary bytes. A non-UTF-8 path
        // can't be embedded faithfully in the `sh -c` String, so `%`
        // expansion must refuse rather than silently target a
        // U+FFFD-mangled (and possibly different) file.
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let bad = PathBuf::from(OsStr::from_bytes(b"caf\xff"));
        let refs: Vec<&Path> = vec![bad.as_path()];
        let err = expand_percent("ls %", &refs).unwrap_err();
        assert!(
            err.to_string().contains("non-UTF-8"),
            "expected a non-UTF-8 refusal, got: {err}"
        );
        // A valid sibling in the same selection doesn't rescue it — the
        // whole expansion is refused so no command runs on a wrong path.
        let good = p("ok.txt");
        let mixed: Vec<&Path> = vec![good.as_path(), bad.as_path()];
        assert!(expand_percent("rm %", &mixed).is_err());
    }

    // ── property tests ────────────────────────────────────────────
    //
    // Round-trip: for any input string `s`, parsing the output of
    // `shell_quote(s)` back through a small POSIX single-quoted-string
    // parser must yield `s` exactly. This is the property a shell
    // would observe when invoked with the quoted form.

    /// Parse a string produced by `shell_quote` back into the original
    /// content. Returns `None` for malformed input — `shell_quote`'s
    /// output should never trigger that path. POSIX single-quoted-string
    /// rules: outer `'…'`, no escape inside *except* the four-char
    /// sequence `'\''` which encodes a literal single quote
    /// (close-quote, backslash, quote, reopen-quote).
    fn parse_shell_quoted(encoded: &str) -> Option<String> {
        let chars: Vec<char> = encoded.chars().collect();
        if chars.first() != Some(&'\'') || chars.last() != Some(&'\'') || chars.len() < 2 {
            return None;
        }
        let mut out = String::new();
        let mut i = 1;
        let end = chars.len() - 1;
        while i < end {
            if chars[i] == '\'' {
                // Must be the start of `'\''` — close, escaped, reopen.
                if chars.get(i + 1) == Some(&'\\')
                    && chars.get(i + 2) == Some(&'\'')
                    && chars.get(i + 3) == Some(&'\'')
                {
                    out.push('\'');
                    i += 4;
                    continue;
                }
                // Bare `'` inside the body is malformed for shell_quote output.
                return None;
            }
            out.push(chars[i]);
            i += 1;
        }
        Some(out)
    }

    proptest::proptest! {
        #[test]
        fn shell_quote_round_trips(s in proptest::string::string_regex(".{0,40}").unwrap()) {
            let encoded = shell_quote(&s);
            let decoded = parse_shell_quoted(&encoded);
            proptest::prop_assert_eq!(decoded.as_deref(), Some(s.as_str()));
        }
    }
}
