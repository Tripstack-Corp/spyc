//! Path expansion: `~` and `$VAR` / `${VAR}` substitution.
//!
//! Scoped deliberately narrow — we do *not* try to emulate `sh -c`. Anyone
//! who needs more power can invoke the shell (`$`, `!`). This just makes
//! jump targets ergonomic (`~/src`, `$HOME/bin`).

use std::fmt::Write as _;
use std::path::PathBuf;

/// Expand `~` at the start and `$VAR` / `${VAR}` everywhere, then return
/// the result as a `PathBuf`.
///
/// - `~` at the very start expands to `$HOME` (followed by `/rest` if any).
/// - `$VAR` and `${VAR}` expand to the corresponding environment value;
///   unset vars are left as-is so the user sees what they typed.
pub fn expand(input: &str) -> PathBuf {
    let tilde_done = expand_tilde(input);
    PathBuf::from(expand_env_vars(&tilde_done))
}

fn expand_tilde(s: &str) -> String {
    let Some(rest) = s.strip_prefix('~') else {
        return s.to_string();
    };
    let Some(home) = std::env::var_os("HOME") else {
        return s.to_string();
    };
    let mut out = PathBuf::from(home);
    // Strip the separator — PathBuf::push replaces its argument when it
    // starts with `/`.
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    if !rest.is_empty() {
        out.push(rest);
    }
    out.to_string_lossy().into_owned()
}

fn expand_env_vars(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '$' {
            out.push(ch);
            continue;
        }
        // `${VAR}` form.
        if chars.peek() == Some(&'{') {
            chars.next();
            let mut name = String::new();
            let mut closed = false;
            while let Some(&nc) = chars.peek() {
                if nc == '}' {
                    chars.next();
                    closed = true;
                    break;
                }
                name.push(nc);
                chars.next();
            }
            if closed {
                if let Ok(val) = std::env::var(&name) {
                    out.push_str(&val);
                } else {
                    // Unset — keep the literal so the user sees the typo.
                    let _ = write!(out, "${{{name}}}");
                }
            } else {
                // Unterminated — emit literally.
                let _ = write!(out, "${{{name}");
            }
            continue;
        }
        // `$VAR` form — consume [A-Za-z_][A-Za-z0-9_]*.
        if chars
            .peek()
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')
        {
            let mut name = String::new();
            while let Some(&nc) = chars.peek() {
                if nc.is_ascii_alphanumeric() || nc == '_' {
                    name.push(nc);
                    chars.next();
                } else {
                    break;
                }
            }
            if let Ok(val) = std::env::var(&name) {
                out.push_str(&val);
            } else {
                out.push('$');
                out.push_str(&name);
            }
            continue;
        }
        // Lone `$` — keep as-is.
        out.push('$');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_alone_expands_to_home() {
        // Guard against hosts where HOME is not set.
        if std::env::var_os("HOME").is_none() {
            return;
        }
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand("~"), PathBuf::from(home));
    }

    #[test]
    fn tilde_with_subpath() {
        if std::env::var_os("HOME").is_none() {
            return;
        }
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            expand("~/foo/bar"),
            PathBuf::from(format!("{home}/foo/bar"))
        );
    }

    #[test]
    fn env_var_brace_form() {
        // SAFETY: scoped to this test thread; single-threaded test run avoids
        // interference with other tests that read env.
        unsafe {
            std::env::set_var("CSPY_TEST_BRACE", "/tmp/cspy-brace");
        }
        assert_eq!(
            expand("${CSPY_TEST_BRACE}/sub"),
            PathBuf::from("/tmp/cspy-brace/sub")
        );
    }

    #[test]
    fn env_var_bare_form() {
        unsafe {
            std::env::set_var("CSPY_TEST_BARE", "/tmp/cspy-bare");
        }
        assert_eq!(
            expand("$CSPY_TEST_BARE/x"),
            PathBuf::from("/tmp/cspy-bare/x")
        );
    }

    #[test]
    fn unset_var_passes_through() {
        // Ensure the var really isn't set.
        unsafe {
            std::env::remove_var("CSPY_NEVER_SET_PROBABLY");
        }
        assert_eq!(
            expand("/prefix/$CSPY_NEVER_SET_PROBABLY/suffix"),
            PathBuf::from("/prefix/$CSPY_NEVER_SET_PROBABLY/suffix")
        );
    }

    #[test]
    fn lone_dollar_preserved() {
        assert_eq!(expand("price-$-5"), PathBuf::from("price-$-5"));
    }

    #[test]
    fn literal_without_expansion() {
        assert_eq!(expand("/a/b/c"), PathBuf::from("/a/b/c"));
    }
}
