//! `%` substitution for shell commands, spy-style.
//!
//! `%` in a user-typed shell command is replaced with the current
//! selection's paths, each shell-quoted and separated by spaces. A literal
//! percent sign can be produced with `%%`.
//!
//! We only generate command *strings* here — execution happens through
//! `sh -c`, so the shell parses the result.

use std::path::Path;

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
pub fn expand_percent(template: &str, targets: &[&Path]) -> String {
    let joined: String = targets
        .iter()
        .map(|p| shell_quote(&p.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ");

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
    out
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
        let refs: Vec<&Path> = files.iter().map(|p| p.as_path()).collect();
        assert_eq!(expand_percent("ls -la %", &refs), "ls -la 'foo bar.txt'");
    }

    #[test]
    fn expands_multiple_files() {
        let files = [p("a.txt"), p("b c.txt")];
        let refs: Vec<&Path> = files.iter().map(|p| p.as_path()).collect();
        assert_eq!(expand_percent("cat %", &refs), "cat 'a.txt' 'b c.txt'");
    }

    #[test]
    fn literal_percent_with_double() {
        let files = [p("x")];
        let refs: Vec<&Path> = files.iter().map(|p| p.as_path()).collect();
        assert_eq!(
            expand_percent("printf '%%s\\n' %", &refs),
            "printf '%s\\n' 'x'"
        );
    }

    #[test]
    fn multiple_occurrences() {
        let files = [p("x")];
        let refs: Vec<&Path> = files.iter().map(|p| p.as_path()).collect();
        assert_eq!(expand_percent("cp % %.bak", &refs), "cp 'x' 'x'.bak");
    }

    #[test]
    fn no_percent_passes_through() {
        assert_eq!(expand_percent("date", &[]), "date");
    }
}
