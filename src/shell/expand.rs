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

/// Quote `s` as a single POSIX-shell word, via `shlex`.
///
/// Not always literally quoted: `shlex` leaves a word bare when every byte is
/// already safe (`/usr/bin/ls`) and switches to double quotes when the content
/// holds a single quote, concatenating segments (`"it's "'$HOME'`) so `$`,
/// backtick and `!` still land inside single quotes. All three forms are one
/// shell word — what callers depend on — not a fixed output shape.
///
/// # Panics
///
/// If `s` contains an interior NUL, the one input `shlex::try_quote` rejects.
/// No POSIX path can contain one, and every caller derives `s` from a path, so
/// reaching this means a deliberately synthesized string. `sh -c` takes a C
/// string and would truncate at the NUL, so no quoting can represent it:
/// panicking loudly beats handing the shell a silently shortened command.
pub fn shell_quote(s: &str) -> String {
    shlex::try_quote(s)
        // Invariant: callers pass path-derived strings; POSIX paths hold no NUL.
        .expect("shell_quote input contains an interior NUL")
        .into_owned()
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

    /// The words a shell would parse the built command into. These tests assert
    /// *this* rather than the exact quoted bytes: `shlex` leaves already-safe
    /// words bare and uses double quotes around an embedded `'`, so pinning the
    /// output shape would test the quoter's style instead of the requirement —
    /// that each path arrives as exactly one word, whatever it contains.
    fn words(cmd: &str) -> Vec<String> {
        shlex::split(cmd).expect("built command must parse as shell words")
    }

    #[test]
    fn quotes_plain_name() {
        assert_eq!(words(&shell_quote("foo.txt")), vec!["foo.txt"]);
    }

    #[test]
    fn quotes_spaces() {
        // The load-bearing case: a space must not split the path into two args.
        assert_eq!(words(&shell_quote("two words")), vec!["two words"]);
    }

    #[test]
    fn escapes_embedded_single_quote() {
        assert_eq!(words(&shell_quote("it's")), vec!["it's"]);
        // A quote must not let the rest of the name escape into shell syntax.
        assert_eq!(
            words(&shell_quote("it's; rm -rf /")),
            vec!["it's; rm -rf /"]
        );
    }

    #[test]
    fn expands_single_file() {
        let files = [p("foo bar.txt")];
        let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        assert_eq!(
            words(&expand_percent("ls -la %", &refs).unwrap()),
            vec!["ls", "-la", "foo bar.txt"]
        );
    }

    #[test]
    fn expands_multiple_files() {
        let files = [p("a.txt"), p("b c.txt")];
        let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        assert_eq!(
            words(&expand_percent("cat %", &refs).unwrap()),
            vec!["cat", "a.txt", "b c.txt"]
        );
    }

    #[test]
    fn literal_percent_with_double() {
        let files = [p("x")];
        let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        assert_eq!(
            words(&expand_percent("printf '%%s\\n' %", &refs).unwrap()),
            vec!["printf", "%s\\n", "x"]
        );
    }

    #[test]
    fn multiple_occurrences() {
        let files = [p("x")];
        let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        // `%.bak` appends OUTSIDE the quoting, so the suffix joins the same word.
        assert_eq!(
            words(&expand_percent("cp % %.bak", &refs).unwrap()),
            vec!["cp", "x", "x.bak"]
        );
    }

    /// The suffix-joins-the-word property with a path that must be quoted — the
    /// case where the old always-quote and shlex's bare form could diverge.
    #[test]
    fn a_suffix_after_a_quoted_path_stays_one_word() {
        let files = [p("a b")];
        let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        assert_eq!(
            words(&expand_percent("cp % %.bak", &refs).unwrap()),
            vec!["cp", "a b", "a b.bak"]
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
    // Round-trip: for any input `s`, splitting `shell_quote(s)` back into words
    // must yield exactly one word equal to `s`. That is the property a shell
    // observes when invoked with the quoted form.
    //
    // The decoder used to be a hand-written POSIX single-quoted-string parser,
    // valid only because the old `shell_quote` ALWAYS wrapped in `'…'`. `shlex`
    // emits three shapes (bare / single-quoted / double-quoted-and-concatenated),
    // so re-implementing a shell word-splitter to decode them would be a bigger
    // hand-roll than the one this PR removes. `shlex::split` is that parser.

    /// One shell word, or `None` if the quoting didn't survive splitting.
    fn parse_shell_quoted(encoded: &str) -> Option<String> {
        match shlex::split(encoded)?.as_slice() {
            [only] => Some(only.clone()),
            _ => None,
        }
    }

    proptest::proptest! {
        #[test]
        // `[^\x00]` rather than `.`: an interior NUL is the one input
        // `shell_quote` refuses (see its docs — `sh -c` takes a C string and
        // would truncate there, so no quoting represents it). Its domain is
        // path-derived strings and no POSIX path holds a NUL; the refusal is
        // pinned by `a_nul_is_refused_not_silently_truncated` below rather than
        // smuggled into this property's alphabet.
        fn shell_quote_round_trips(s in proptest::string::string_regex("[^\u{0}]{0,40}").unwrap()) {
            let encoded = shell_quote(&s);
            let decoded = parse_shell_quoted(&encoded);
            proptest::prop_assert_eq!(decoded.as_deref(), Some(s.as_str()));
        }
    }

    /// A NUL must fail loudly. The old hand-rolled quoter accepted it and
    /// embedded it, producing a command `sh` silently truncated at the NUL —
    /// i.e. it ran against a *different, shorter* path than the one selected.
    #[test]
    #[should_panic(expected = "interior NUL")]
    fn a_nul_is_refused_not_silently_truncated() {
        let _ = shell_quote("safe\u{0}; rm -rf /");
    }

    /// The assertion that actually pins the contract: hand the quoted form to a
    /// real `sh` and require the original bytes back. `shlex::split` agreeing
    /// with `shlex::try_quote` only proves they are mutual inverses; this proves
    /// the quoting means what we need it to mean to the program that runs it.
    #[test]
    #[cfg(unix)]
    fn shell_quote_survives_a_real_sh() {
        let cases = [
            "/usr/bin/ls",
            "plain",
            "two words",
            "it's",
            "it's $HOME",
            "it's `id`",
            "it's $(id)",
            "don't; rm -rf /",
            "a'b'c",
            "$HOME",
            "`id`",
            "$(id)",
            "a|b",
            "a&b",
            "a;b",
            "a>b",
            "*",
            "?",
            "~/x",
            "#h",
            "a!b",
            "a{b}",
            "a\"b",
            "a\\b",
            "a\nb",
            "\t",
            "  lead",
            "trail  ",
            "",
            "«ü»",
        ];
        for s in cases {
            let quoted = shell_quote(s);
            // `printf %s` so no trailing newline and no escape interpretation of
            // our own is folded into the comparison.
            let out = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("printf %s {quoted}"))
                .output()
                .expect("spawn sh");
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                s,
                "sh did not round-trip {s:?} quoted as {quoted}"
            );
        }
    }
}
