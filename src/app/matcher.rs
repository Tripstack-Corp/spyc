//! The `/` search and `=` limit-filter matcher — a pure text leaf with no `App`
//! dependency, extracted from `mod.rs` under the anti-monolith ceiling.

use glob::Pattern;

/// Search / filter matcher: case-insensitive substring for plain
/// text, glob for anything with `*`, `?`, or `[`, and a leading `^` /
/// trailing `$` as regex-style anchors. Used by `/` (search) and `=`
/// (limit filter). Substring (not anchored at the
/// start) so `/env` finds `.env`, `.envrc`, and `environment.toml`
/// — anchored prefix mode hid dot-prefixed files behind their
/// leading `.` and was consistently surprising. Anchor explicitly with
/// `^env` (or the equivalent glob `env*`) when you want starts-with.
pub enum Matcher {
    Substring(String),
    Glob(Pattern),
    /// A `^`/`$`-anchored query, desugared to a glob. Matched like [`Self::Glob`]
    /// except that a row's kind decoration — `/` on a directory, `*` on an
    /// executable, added by `Entry::display_name` — doesn't count as part of the
    /// name. Without that, an end anchor could never match either: `/src$` would
    /// miss the directory `src/`, and `/sh$` would miss `deploy.sh*`.
    AnchoredGlob(Pattern),
    /// An invalid glob produced by a malformed pattern. Matches nothing.
    Never,
}

impl Matcher {
    pub fn build(query: &str) -> Self {
        let lower = query.to_lowercase();
        // `^`/`$` anchors desugar into the glob engine — the whole-string glob
        // match IS the anchoring, so `^env` is `env*` and `env$` is `*env`.
        // Costs no new matching code and composes with `*`/`?`/`[` (`^a?c$`).
        //
        // Without this, `^env` was a literal substring no filename contains, so
        // the regex reflex silently matched NOTHING rather than starts-with.
        if let Some(anchored) = desugar_anchors(&lower) {
            return match Pattern::new(&anchored) {
                Ok(p) => Self::AnchoredGlob(p),
                Err(_) => Self::Never,
            };
        }
        if lower.contains(['*', '?', '[']) {
            match Pattern::new(&lower) {
                Ok(p) => Self::Glob(p),
                Err(_) => Self::Never,
            }
        } else {
            Self::Substring(lower)
        }
    }

    pub fn matches(&self, name: &str) -> bool {
        match self {
            Self::Substring(q) => ascii_or_lower_contains(name, q),
            Self::Glob(p) => glob_matches(p, name),
            Self::AnchoredGlob(p) => {
                glob_matches(p, name) || undecorated(name).is_some_and(|n| glob_matches(p, n))
            }
            Self::Never => false,
        }
    }
}

/// Case-insensitive whole-string glob test. Glob matching needs an owned `&str`;
/// skip the lowercasing allocation for the common case of an already-lowercase
/// ASCII name. Non-ASCII (or any uppercase) names fall back to `to_lowercase` to
/// preserve Unicode case-folding semantics.
fn glob_matches(p: &Pattern, name: &str) -> bool {
    if name.is_ascii() && !name.bytes().any(|b| b.is_ascii_uppercase()) {
        p.matches(name)
    } else {
        p.matches(&name.to_lowercase())
    }
}

/// A row's name with its kind decoration removed (`Entry::display_name` appends
/// `/` to a directory and `*` to an executable), or `None` when it carries none.
fn undecorated(name: &str) -> Option<&str> {
    name.strip_suffix('/').or_else(|| name.strip_suffix('*'))
}

/// Rewrite a `^`/`$`-anchored query as the equivalent glob, or `None` when the
/// query carries no anchor and the caller should use its normal path.
///
/// Only a LEADING `^` and a TRAILING `$` are anchors — `$RECYCLE.BIN` is a real
/// filename, so a `$` anywhere else stays literal, and a trailing `$` with
/// nothing before it is a search FOR `$` rather than a useless empty anchor.
/// A bare `^` matches everything, which is what an incremental search wants
/// after the first keystroke of `^env`.
fn desugar_anchors(query: &str) -> Option<String> {
    let at_start = query.starts_with('^');
    let body = query.strip_prefix('^').unwrap_or(query);
    let at_end = body.len() > 1 && body.ends_with('$');
    let body = if at_end {
        body.strip_suffix('$').unwrap_or(body)
    } else {
        body
    };
    match (at_start, at_end) {
        (false, false) => None,
        (true, true) => Some(body.to_string()),
        // Don't double up an existing `*` — `**` is its own thing in a glob.
        (true, false) if body.ends_with('*') => Some(body.to_string()),
        (true, false) => Some(format!("{body}*")),
        (false, true) if body.starts_with('*') => Some(body.to_string()),
        (false, true) => Some(format!("*{body}")),
    }
}

/// Case-insensitive substring test that avoids allocating a lowercased copy of
/// `name` on the filter/search hot path (called once per listing row per
/// keystroke). `needle` is already lowercased by `Matcher::build`. The ASCII
/// fast path is allocation-free; non-ASCII names fall back to `to_lowercase`
/// so Unicode case folding stays identical to the old behaviour.
fn ascii_or_lower_contains(name: &str, needle: &str) -> bool {
    if name.is_ascii() && needle.is_ascii() {
        let (h, n) = (name.as_bytes(), needle.as_bytes());
        if n.is_empty() {
            return true;
        }
        if n.len() > h.len() {
            return false;
        }
        h.windows(n.len())
            .any(|w| w.iter().zip(n).all(|(&a, &b)| a.to_ascii_lowercase() == b))
    } else {
        name.to_lowercase().contains(needle)
    }
}

#[cfg(test)]
mod matcher_tests {
    use super::Matcher;

    // The allocation-free ASCII fast path in `Matcher::matches` must stay
    // behaviorally identical to the old `name.to_lowercase().contains(q)`.
    #[test]
    fn substring_is_case_insensitive_both_directions() {
        let m = Matcher::build("env");
        assert!(m.matches(".ENV"));
        assert!(m.matches(".envrc"));
        assert!(m.matches("Environment.toml"));
        assert!(!m.matches("readme.md"));

        // Uppercase query lowercases at build time.
        let m = Matcher::build("ENV");
        assert!(m.matches(".env"));
    }

    #[test]
    fn substring_empty_query_matches_everything() {
        let m = Matcher::build("");
        assert!(m.matches("anything"));
        assert!(m.matches(""));
    }

    #[test]
    fn substring_unicode_falls_back_to_lowercase() {
        // Non-ASCII names take the to_lowercase path; case folding must hold.
        let m = Matcher::build("café");
        assert!(m.matches("CAFÉ.txt"));
        assert!(m.matches("le-café"));
        assert!(!m.matches("coffee"));
    }

    #[test]
    fn substring_needle_longer_than_name_is_no_match() {
        let m = Matcher::build("readme");
        assert!(!m.matches("rd"));
    }

    /// Issue #199: `^` is the reflex for starts-with, and it used to be a
    /// literal substring no filename contains — so the query silently matched
    /// NOTHING instead of anchoring. Glob mode already anchored (`env*`); `^`
    /// desugars into it.
    #[test]
    fn caret_anchors_at_the_start() {
        let m = Matcher::build("^env");
        assert!(m.matches("environment.toml"));
        assert!(m.matches("env"));
        assert!(m.matches("ENV_FILE")); // still case-insensitive
        assert!(!m.matches(".envrc")); // the leading `.` is not a match
        assert!(!m.matches("MY_ENV.txt"));
    }

    #[test]
    fn dollar_anchors_at_the_end() {
        let m = Matcher::build("env$");
        assert!(m.matches(".env"));
        assert!(m.matches("MY_ENV"));
        assert!(!m.matches(".envrc"));
        assert!(!m.matches("environment.toml"));
    }

    #[test]
    fn both_anchors_are_an_exact_match() {
        let m = Matcher::build("^env$");
        assert!(m.matches("env"));
        assert!(m.matches("ENV"));
        assert!(!m.matches(".env"));
        assert!(!m.matches("envrc"));
    }

    #[test]
    fn anchors_compose_with_glob_metacharacters() {
        let m = Matcher::build("^a?c$");
        assert!(m.matches("abc"));
        assert!(m.matches("axc"));
        assert!(!m.matches("abcd"));

        // An existing `*` on the anchored side isn't doubled into `**`.
        let m = Matcher::build("^src*");
        assert!(m.matches("src/lib.rs"));
        assert!(m.matches("srcfile"));
        assert!(!m.matches("mysrc"));
    }

    /// `$RECYCLE.BIN` is a real filename (Windows volumes mounted on a Mac), so
    /// a `$` that isn't trailing must stay literal — and a bare trailing `$`
    /// with no body is a search FOR `$`, not an empty end-anchor.
    #[test]
    fn dollar_is_literal_unless_it_trails_a_body() {
        let m = Matcher::build("$recycle");
        assert!(m.matches("$RECYCLE.BIN"));

        let m = Matcher::build("$");
        assert!(m.matches("$RECYCLE.BIN"));
        assert!(!m.matches("plain.txt"));

        // Mid-query `^` is literal too.
        let m = Matcher::build("a^b");
        assert!(m.matches("xa^by"));
        assert!(!m.matches("ab"));
    }

    /// A lone `^` is the first keystroke of `^env` under incremental search, so
    /// it must match everything rather than nothing.
    #[test]
    fn bare_caret_matches_everything_mid_typing() {
        let m = Matcher::build("^");
        assert!(m.matches("anything.txt"));
        assert!(m.matches(".hidden"));
    }

    /// Rows carry a kind decoration — `Entry::display_name` appends `/` to a
    /// directory and `*` to an executable — and that is what the matcher sees.
    /// An end anchor has to look past it, or `/src$` could never find `src/`:
    /// the same silent no-match that made `^` worth fixing in the first place.
    #[test]
    fn end_anchor_looks_past_the_row_kind_decoration() {
        let m = Matcher::build("src$");
        assert!(m.matches("src/"), "the directory row is `src/`");
        assert!(m.matches("src"));
        assert!(m.matches("mysrc/"));
        assert!(!m.matches("src/lib.rs"));

        let m = Matcher::build("^src$");
        assert!(m.matches("src/"));
        assert!(!m.matches("mysrc/"));

        // Executables display as `name*`.
        let m = Matcher::build("deploy.sh$");
        assert!(m.matches("deploy.sh*"));
        assert!(m.matches("deploy.sh"));
    }

    /// The decoration allowance is scoped to anchored queries — a plain glob
    /// keeps matching the decorated name exactly as it did before #199, so
    /// `=*/` still means "directories only".
    #[test]
    fn plain_glob_still_matches_the_decorated_name() {
        let m = Matcher::build("*/");
        assert!(m.matches("src/"));
        assert!(!m.matches("main.rs"));

        // `sr?` is whole-string, so the decorated `src/` (4 chars) is no match
        // while the bare `src` is — an unanchored glob gets no allowance.
        let m = Matcher::build("sr?");
        assert!(m.matches("src"));
        assert!(!m.matches("src/"));
    }

    #[test]
    fn glob_skips_alloc_for_lowercase_ascii_but_stays_case_insensitive() {
        let m = Matcher::build("*.RS");
        assert!(m.matches("main.rs")); // already-lowercase fast path
        assert!(m.matches("MAIN.RS")); // uppercase → lowercase fallback
        assert!(!m.matches("main.py"));
    }
}
