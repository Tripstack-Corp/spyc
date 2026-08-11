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
    // Prefer the `envset` overlay for HOME (the same source used for every
    // other variable via the `lookup` below), falling back to the process
    // environment — otherwise an overridden HOME applied everywhere else was
    // silently ignored by tilde expansion.
    let home = crate::envset::var("HOME")
        .or_else(|| std::env::var_os("HOME").map(|h| h.to_string_lossy().into_owned()));
    expand_with(input, home.as_deref(), crate::envset::var)
}

/// Pure variant of `expand` that takes the HOME value and an env
/// lookup function as parameters. Tests use this directly so they
/// don't need to mutate the process-global env.
fn expand_with(
    input: &str,
    home: Option<&str>,
    lookup: impl Fn(&str) -> Option<String>,
) -> PathBuf {
    let tilde_done = expand_tilde(input, home);
    PathBuf::from(expand_env_vars(&tilde_done, lookup))
}

/// Inverse of `expand_tilde` for *display*: if `path` starts with `$HOME`,
/// replace that prefix with `~`. Otherwise return the path verbatim.
///
/// Matches at directory boundaries so `/Users/xx` is not rewritten
/// when `$HOME` is `/Users/x`.
pub fn display_tilde(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    let Some(home) = std::env::var_os("HOME") else {
        return s.into_owned();
    };
    let home = home.to_string_lossy();
    if home.is_empty() {
        return s.into_owned();
    }
    let home = home.trim_end_matches('/');
    if let Some(rest) = s.strip_prefix(home) {
        if rest.is_empty() {
            return "~".to_string();
        }
        if rest.starts_with('/') {
            return format!("~{rest}");
        }
    }
    s.into_owned()
}

fn expand_tilde(s: &str, home: Option<&str>) -> String {
    let Some(rest) = s.strip_prefix('~') else {
        return s.to_string();
    };
    // Only a bare `~` or `~/…` expands to $HOME. `~user` (tilde followed by
    // anything other than `/`) names *another* user's home, which we don't
    // resolve — leave it verbatim rather than mangling it into `$HOME/user`.
    if !rest.is_empty() && !rest.starts_with('/') {
        return s.to_string();
    }
    let Some(home) = home else {
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

fn expand_env_vars(input: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
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
                if let Some(val) = lookup(&name) {
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
            if let Some(val) = lookup(&name) {
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

/// Whether `candidate` resolves to `root` or somewhere inside it.
///
/// `None` when either side can't be canonicalized, because that question has
/// two right answers and only the caller knows which it wants. A reader
/// checking a path the user typed can fall back to a literal compare — a
/// non-existent path fails at `open` anyway. An *extractor* cannot: its
/// destination doesn't exist yet by definition, and a lexical compare on an
/// unresolved path is what let a symlink chain out of the archive staging root
/// (`starts_with` is happy to call `root/x/..` contained). Callers that must
/// judge a path which doesn't exist yet resolve its parent first and ask about
/// that.
///
/// Canonical on both sides so a symlinked root doesn't false-reject, and
/// component-wise via `starts_with` so `/a/bc` is not inside `/a/b`.
pub fn canonical_contains(root: &std::path::Path, candidate: &std::path::Path) -> Option<bool> {
    let root = std::fs::canonicalize(root).ok()?;
    let candidate = std::fs::canonicalize(candidate).ok()?;
    Some(candidate == root || candidate.starts_with(&root))
}

#[cfg(test)]
mod containment_tests {
    use super::canonical_contains;

    #[test]
    fn a_path_inside_the_root_is_contained() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let inner = tmp.path().join("a/b");
        std::fs::create_dir_all(&inner).expect("mkdir");
        assert_eq!(canonical_contains(tmp.path(), &inner), Some(true));
        assert_eq!(canonical_contains(tmp.path(), tmp.path()), Some(true));
    }

    #[test]
    fn a_sibling_sharing_a_name_prefix_is_not_contained() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("b");
        let sibling = tmp.path().join("bc");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::create_dir_all(&sibling).expect("mkdir");
        assert_eq!(canonical_contains(&root, &sibling), Some(false));
    }

    /// The property the archive extractor depends on: a `..` that climbs out is
    /// caught even though the unresolved path lexically starts with the root.
    #[test]
    fn a_symlink_climbing_out_is_not_contained() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("d")).expect("mkdir");
        std::fs::create_dir_all(tmp.path().join("outside")).expect("mkdir");
        #[cfg(unix)]
        std::os::unix::fs::symlink("../../outside", root.join("d/out")).expect("symlink");
        #[cfg(unix)]
        {
            let escaped = root.join("d/out");
            assert!(escaped.starts_with(&root), "lexically it looks contained");
            assert_eq!(canonical_contains(&root, &escaped), Some(false));
        }
    }

    #[test]
    fn an_unresolvable_path_is_none_not_a_guess() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            canonical_contains(tmp.path(), &tmp.path().join("does/not/exist")),
            None
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_tilde_collapses_home_prefix() {
        if std::env::var_os("HOME").is_none() {
            return;
        }
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            display_tilde(&PathBuf::from(format!("{home}/src/spyc"))),
            "~/src/spyc"
        );
        assert_eq!(display_tilde(&PathBuf::from(&home)), "~");
    }

    #[test]
    fn display_tilde_only_at_directory_boundary() {
        if std::env::var_os("HOME").is_none() {
            return;
        }
        let home = std::env::var("HOME").unwrap();
        // A sibling directory whose name starts with HOME's basename
        // must NOT be rewritten.
        let sibling = format!("{home}_other/foo");
        assert_eq!(display_tilde(&PathBuf::from(&sibling)), sibling);
    }

    #[test]
    fn display_tilde_passes_through_non_home_paths() {
        assert_eq!(display_tilde(&PathBuf::from("/etc/hosts")), "/etc/hosts");
    }

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
    fn tilde_user_is_left_verbatim_not_mangled() {
        // `~user` names another user's home, which we don't resolve — it must
        // be returned unchanged, never rewritten to `$HOME/user`.
        assert_eq!(expand_tilde("~alice/foo", Some("/home/me")), "~alice/foo");
        assert_eq!(expand_tilde("~root", Some("/home/me")), "~root");
        // Bare `~` and `~/…` still expand.
        assert_eq!(expand_tilde("~", Some("/home/me")), "/home/me");
        assert_eq!(expand_tilde("~/foo", Some("/home/me")), "/home/me/foo");
    }

    #[test]
    fn env_var_brace_form() {
        let lookup = |name: &str| -> Option<String> {
            (name == "BRACE").then(|| "/tmp/spyc-brace".to_string())
        };
        assert_eq!(
            expand_with("${BRACE}/sub", None, lookup),
            PathBuf::from("/tmp/spyc-brace/sub")
        );
    }

    #[test]
    fn env_var_bare_form() {
        let lookup = |name: &str| -> Option<String> {
            (name == "BARE").then(|| "/tmp/spyc-bare".to_string())
        };
        assert_eq!(
            expand_with("$BARE/x", None, lookup),
            PathBuf::from("/tmp/spyc-bare/x")
        );
    }

    #[test]
    fn unset_var_passes_through() {
        // Lookup returns None for every name — the literal `$VAR` form
        // must survive verbatim.
        let lookup = |_: &str| -> Option<String> { None };
        assert_eq!(
            expand_with("/prefix/$NEVER_SET/suffix", None, lookup),
            PathBuf::from("/prefix/$NEVER_SET/suffix")
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
