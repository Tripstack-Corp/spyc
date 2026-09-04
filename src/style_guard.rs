//! Guard: prose in this repo is Canadian English.
//!
//! Canadian English is not simply British English. It takes the `-our`, `-re`,
//! `-ce` and doubled-consonant spellings, but keeps the American `-ize` / `-yze`
//! endings — so `colour`, `centre`, `licence`, `travelled`, and also `organize`
//! and `analyze`. `-ise` is wrong here in both directions, which is why no
//! `-ise` form appears below.
//!
//! Only PROSE is governed: markdown body text and comment bodies. Identifiers,
//! string literals, CLI flags, config keys and inline-code spans are code, and
//! code is whatever upstream named it — `Color::Rgb`, `color_depth.rs`,
//! `$COLORTERM=truecolor` and `--color` all stay exactly as they are. The
//! protections below exist to keep this guard from demanding otherwise.

/// US → Canadian, restricted to forms with no ambiguity.
///
/// Deliberately absent: `meter` (a gauge is a meter; only the SI unit is a
/// metre), `practice`/`licence` (the noun/verb split needs a human), and every
/// `-ise`/`-yse` form (Canadian keeps `-ize`/`-yze`).
const BANNED: &[(&str, &str)] = &[
    ("color", "colour"),
    ("colors", "colours"),
    ("colored", "coloured"),
    ("coloring", "colouring"),
    ("colorful", "colourful"),
    ("colorless", "colourless"),
    ("colorize", "colourize"),
    ("colorized", "colourized"),
    ("behavior", "behaviour"),
    ("behaviors", "behaviours"),
    ("behavioral", "behavioural"),
    ("favor", "favour"),
    ("favors", "favours"),
    ("favored", "favoured"),
    ("favorable", "favourable"),
    ("favorite", "favourite"),
    ("favorites", "favourites"),
    ("flavor", "flavour"),
    ("flavors", "flavours"),
    ("honor", "honour"),
    ("honors", "honours"),
    ("honored", "honoured"),
    ("honoring", "honouring"),
    ("labor", "labour"),
    ("neighbor", "neighbour"),
    ("neighbors", "neighbours"),
    ("humor", "humour"),
    ("rumor", "rumour"),
    ("vapor", "vapour"),
    ("endeavor", "endeavour"),
    ("harbor", "harbour"),
    ("armor", "armour"),
    ("center", "centre"),
    ("centers", "centres"),
    ("centered", "centred"),
    ("centering", "centring"),
    ("fiber", "fibre"),
    ("caliber", "calibre"),
    ("luster", "lustre"),
    ("somber", "sombre"),
    ("specter", "spectre"),
    ("theater", "theatre"),
    ("defense", "defence"),
    ("offense", "offence"),
    ("pretense", "pretence"),
    ("gray", "grey"),
    ("grays", "greys"),
    ("grayed", "greyed"),
    ("grayscale", "greyscale"),
    ("traveled", "travelled"),
    ("traveling", "travelling"),
    ("canceled", "cancelled"),
    ("canceling", "cancelling"),
    ("labeled", "labelled"),
    ("labeling", "labelling"),
    ("modeled", "modelled"),
    ("modeling", "modelling"),
    ("signaled", "signalled"),
    ("signaling", "signalling"),
    ("marvelous", "marvellous"),
    ("counselor", "counsellor"),
    ("fueled", "fuelled"),
    ("totaled", "totalled"),
    ("catalog", "catalogue"),
    ("catalogs", "catalogues"),
];

/// Whether the character *after* a match makes it part of an identifier.
///
/// `.` and `:` only bind when something follows them — `color.rs`, `Color::Rgb`.
/// A sentence-final "colour." is prose, and treating that period as an
/// identifier join silently exempts every word that ends a sentence.
///
/// `/` binds nothing. It was in this set for bare paths, but AGENTS.md's
/// backtick contract already requires those to be backticked, and a bare
/// `color.rs` is still covered by the extension rule above. What the slash
/// actually exempted was ordinary prose — "cosmetic/behavioural",
/// "centred/fit", "Colour/style" — four of them, in the tree, silently.
fn binds_after(text: &str, end: usize) -> bool {
    let mut it = text[end..].chars();
    let Some(c) = it.next() else { return false };
    if c.is_alphanumeric() || matches!(c, '_' | '\\') {
        return true;
    }
    if matches!(c, '.' | ':') {
        return it.next().is_some_and(|n| n.is_alphanumeric() || n == c);
    }
    false
}

/// Whether the character *before* a match makes it part of an identifier.
fn binds_before(text: &str, start: usize) -> bool {
    let mut it = text[..start].chars().rev();
    let Some(c) = it.next() else { return false };
    if c.is_alphanumeric() || matches!(c, '_' | '\\') {
        return true;
    }
    if matches!(c, '.' | ':') {
        return it
            .next()
            .is_some_and(|pch| pch.is_alphanumeric() || pch == c);
    }
    false
}

/// Replace inline-code spans with spaces, preserving byte offsets.
fn blank_code_spans(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_span = false;
    for ch in s.chars() {
        if ch == '`' {
            in_span = !in_span;
            out.push(' ');
        } else if in_span {
            out.extend(std::iter::repeat_n(' ', ch.len_utf8()));
        } else {
            out.push(ch);
        }
    }
    out
}

/// Every banned word in one line of prose, with its suggested replacement.
fn offenders(line: &str) -> Vec<(&'static str, &'static str)> {
    let text = blank_code_spans(line);
    let bytes = text.as_bytes();
    let lower = text.to_lowercase();
    let mut found = Vec::new();
    for (bad, good) in BANNED {
        let mut from = 0;
        while let Some(rel) = lower[from..].find(bad) {
            let start = from + rel;
            let end = start + bad.len();
            from = end;
            if binds_before(&text, start) || binds_after(&text, end) {
                continue;
            }
            // `--flag`
            if text[..start].ends_with('-') && start >= 2 && bytes[start - 2] == b'-' {
                continue;
            }
            found.push((*bad, *good));
        }
    }
    found
}

/// Comment bodies in a Rust source file.
fn rust_prose(src: &str) -> Vec<(usize, String)> {
    src.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let at = line.find("//")?;
            // A `//` inside a string literal is not a comment.
            if line[..at].matches('"').count() % 2 == 1 {
                return None;
            }
            Some((i + 1, line[at..].to_string()))
        })
        .collect()
}

/// Blank HTML tags and markdown link targets, preserving byte offsets.
///
/// `<p align="center">` is an HTML attribute value with one legal spelling, and
/// a link target is a URL or a path. Both are code wearing prose clothes.
fn blank_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth_tag = false;
    let mut depth_link = false;
    let mut prev = '\0';
    for ch in s.chars() {
        let opening_link = ch == '(' && prev == ']';
        if ch == '<' {
            depth_tag = true;
        } else if opening_link {
            depth_link = true;
        }
        if depth_tag || depth_link {
            out.extend(std::iter::repeat_n(' ', ch.len_utf8()));
        } else {
            out.push(ch);
        }
        if ch == '>' {
            depth_tag = false;
        } else if ch == ')' {
            depth_link = false;
        }
        prev = ch;
    }
    out
}

/// Body text of a markdown file: no fenced or indented code, no blockquotes
/// (which carry verbatim quotes), no table rows (which carry names and quoted
/// signal text).
fn markdown_prose(src: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut fenced = false;
    for (i, line) in src.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || line.starts_with("    ") || t.starts_with('>') || line.starts_with('|') {
            continue;
        }
        out.push((i + 1, blank_markup(line)));
    }
    out
}

/// `#`-comment bodies (TOML, YAML, shell). Shebangs are not prose.
fn hash_prose(src: &str) -> Vec<(usize, String)> {
    src.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            if line.trim_start().starts_with("#!") {
                return None;
            }
            let at = line.find('#')?;
            let head = &line[..at];
            if head.matches('"').count() % 2 == 1 || head.matches('\'').count() % 2 == 1 {
                return None;
            }
            Some((i + 1, line[at..].to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Exempt by file name, each for a reason that outranks house spelling.
    const SKIP_FILES: &[&str] = &[
        // Generated from commit subjects by git-cliff, and frozen verbatim
        // history before v1.57.0 — editing it here would be undone and would
        // rewrite quoted history.
        "CHANGELOG.md",
        // A vendored canonical document (Contributor Covenant). Its worth is
        // in being recognizably the standard text, so it is the
        // verbatim-import class — the same principle as skipping blockquotes.
        "CODE_OF_CONDUCT.md",
    ];

    /// Exempt by repo-relative path prefix.
    const SKIP_PATHS: &[&str] = &[
        // An archive records what was written when. Re-spelling one is
        // rewriting history, which the decisions log refuses to do by policy;
        // documents archived from here on are already Canadian, having been
        // written under the rule.
        "docs/archive/",
    ];

    const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".cache"];

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if p.is_dir() {
                if !SKIP_DIRS.contains(&name) {
                    walk(&p, out);
                }
            } else if !SKIP_FILES.contains(&name) {
                out.push(p);
            }
        }
    }

    /// Prose in this repo is Canadian English, and this is what makes that a
    /// rule rather than a preference. Drift is otherwise invisible: nothing
    /// else in the gate reads a comment.
    #[test]
    fn prose_is_canadian_english() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        walk(root, &mut files);

        let mut scanned = 0usize;
        let mut bad: Vec<String> = Vec::new();
        for path in files {
            let extract = match path.extension().and_then(|e| e.to_str()) {
                Some("rs") => rust_prose,
                Some("md") => markdown_prose,
                Some("toml" | "yml" | "yaml" | "sh") => hash_prose,
                _ => continue,
            };
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string()
                .replace('\\', "/");
            if SKIP_PATHS.iter().any(|skip| rel.starts_with(skip)) {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            scanned += 1;
            for (line_no, line) in extract(&src) {
                for (found, want) in offenders(&line) {
                    bad.push(format!("{rel}:{line_no}: `{found}` should be `{want}`"));
                }
            }
        }

        assert!(
            scanned > 100,
            "scanned only {scanned} files — the walk is broken"
        );
        assert!(
            bad.is_empty(),
            "prose must be Canadian English (see AGENTS.md). {} occurrence(s):\n{}",
            bad.len(),
            bad.join("\n")
        );
    }

    /// A Canadianized word inside a TOML-section shape is always a bug: the
    /// sweep read a bare `[colors]` as prose and produced a comment naming a
    /// section that does not exist. Backticks are the real contract
    /// (AGENTS.md) and this cannot police bare identifiers in general — but
    /// this one shape is cheap and, measured over the tree, matches nothing
    /// legitimate, so it is worth the twenty lines.
    #[test]
    fn no_canadianized_toml_section_names() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        walk(root, &mut files);
        let mut bad = Vec::new();
        for path in files {
            if !matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("rs" | "md" | "toml")
            ) {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string()
                .replace('\\', "/");
            // The pair table necessarily contains every Canadian spelling.
            if rel == "src/style_guard.rs" {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (i, line) in src.lines().enumerate() {
                for (_us, ca) in BANNED {
                    if line.contains(&format!("[{ca}]")) {
                        bad.push(format!("{rel}:{}: `[{ca}]` is not a config section", i + 1));
                    }
                }
            }
        }
        assert!(
            bad.is_empty(),
            "a config section is spelled the way the struct field is spelled, \
             and prose must backtick it (AGENTS.md):\n{}",
            bad.join("\n")
        );
    }

    #[test]
    fn identifiers_and_flags_are_left_alone() {
        for code in [
            "// ratatui's Color::Rgb is 24-bit",
            "// see src/ui/color_depth.rs for the remap",
            "// pass --color=never to disable",
            "// $COLORTERM=truecolor claims support",
            "// the `color` field of the theme table",
            // Still exempt: an extension or an underscore makes it a path or
            // an identifier even with a slash in front.
            "// see src/color.rs for the remap",
            "// see src/color_depth.rs for the remap",
            // The shape that bit on first run: an HTML attribute in README.md
            // whose value has exactly one legal spelling.
            "<p align=\"center\">",
            "see [the docs](https://example.com/color-guide)",
        ] {
            assert!(
                offenders(&blank_markup(code)).is_empty(),
                "identifier/flag/code-span context must not be flagged: {code}"
            );
        }
    }

    #[test]
    fn plain_prose_is_flagged() {
        for code in [
            "// the warning color row highlight",
            "// a 256-color terminal",
            "// centered in the frame",
            // Sentence-final: the trailing period is punctuation, not an
            // identifier join. This shape escaped the first guard entirely.
            "// sets the default color.",
            "# The status bar is centered.",
            // A slash between two words is prose, not a path. Treating it as
            // an identifier join exempted four real misses in this tree.
            "// cosmetic/behavioral settings and plain rebindings",
            "//! centered/fit rects and body widths",
            "/// Color/style overrides.",
        ] {
            assert!(!offenders(code).is_empty(), "prose must be flagged: {code}");
        }
    }
}
