//! Every relative link in the repo's markdown resolves: the file it names
//! exists inside the repo, and a `#fragment` into a markdown file names one of
//! its headings or anchors.
//!
//! Moving or splitting a doc strands the links into it, and nothing else in
//! the gate reads a link. A prose reference to a section, in a doc or in code,
//! is checked the same way: `ARCHITECTURE.md → "MCP server"` names a heading (or
//! a bold rule) in that file, and `RELEASE_ENGINEERING.md §13.8` a numbered
//! section and, after the dot, an item in it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

/// Not scanned for links, though still valid targets: an archive records what
/// was written when (the prose guard's reason), and test data is data.
const NOT_SCANNED: &[&str] = &["docs/archive/", "tests/fixtures/", "fuzz/"];

#[derive(Debug, Default)]
struct Report {
    files: usize,
    links: usize,
    fragments: usize,
    broken: Vec<String>,
}

/// Lines outside fenced code blocks, numbered from 1.
fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut fence: Option<(char, usize)> = None;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        let marker = t
            .chars()
            .next()
            .filter(|c| matches!(c, '`' | '~'))
            .map(|c| (c, t.chars().take_while(|x| *x == c).count()));
        match (fence, marker) {
            (None, Some((c, n))) if n >= 3 => fence = Some((c, n)),
            (Some((c, n)), Some((c2, n2))) if c == c2 && n2 >= n && t.chars().all(|x| x == c) => {
                fence = None;
            }
            (None, _) => out.push((i + 1, line)),
            _ => {}
        }
    }
    out
}

/// `line` with its code spans removed, so a link shown as code is inert.
fn strip_code_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        out.push_str(&rest[..start]);
        let run = rest[start..].chars().take_while(|c| *c == '`').count();
        let fence = &rest[start..start + run];
        let after = &rest[start + run..];
        // A closing run must be exactly as long as the opening one.
        let close = after.match_indices(fence).find(|(i, _)| {
            !after[i + run..].starts_with('`') && (*i == 0 || !after[..*i].ends_with('`'))
        });
        let Some((i, _)) = close else {
            out.push_str(&rest[start..]);
            return out;
        };
        rest = &after[i + run..];
    }
    out.push_str(rest);
    out
}

/// Every link target on the prose lines of `text`: inline links and images,
/// reference definitions, and `src`/`href` attributes.
fn link_targets(text: &str) -> Vec<(usize, String)> {
    static INLINE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\]\(\s*(<[^>]*>|[^)\s]+)(?:\s+(?:"[^"]*"|'[^']*'))?\s*\)"#).expect("regex")
    });
    // `[^1]:` is a footnote, not a reference definition.
    static REFDEF: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s{0,3}\[[^\]^][^\]]*\]:\s*(<[^>]*>|\S+)").expect("regex"));
    static ATTR: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"\b(?:src|href)="([^"]*)""#).expect("regex"));
    let mut out = Vec::new();
    for (n, line) in prose_lines(text) {
        let line = strip_code_spans(line);
        for re in [&*INLINE, &*REFDEF, &*ATTR] {
            for c in re.captures_iter(&line) {
                let t = c[1].trim_start_matches('<').trim_end_matches('>');
                out.push((n, t.to_string()));
            }
        }
    }
    out
}

/// GitHub's anchor for a heading: lowercased, punctuation and symbols dropped,
/// each space a hyphen (so `A — B` is `a--b`).
fn slug(heading: &str) -> String {
    static LINK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\[([^\]]*)\]\([^)]*\)").expect("regex"));
    LINK.replace_all(heading, "$1")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' '))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// The fragments that land somewhere in `text`: each heading's slug (a repeat
/// takes `-1`, `-2`, …) and every HTML `id` or `<a name>`.
fn anchors(text: &str) -> HashSet<String> {
    static HEADING: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^ {0,3}#{1,6}\s+(.*?)(?:\s+#+)?\s*$").expect("regex"));
    static ID: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?:\bid|<a\s[^>]*\bname)="([^"]+)""#).expect("regex"));
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out = HashSet::new();
    for (_, line) in prose_lines(text) {
        if let Some(c) = HEADING.captures(line) {
            let s = slug(&c[1]);
            let n = seen.entry(s.clone()).or_insert(0);
            out.insert(if *n == 0 { s } else { format!("{s}-{n}") });
            *n += 1;
        }
        out.extend(ID.captures_iter(line).map(|c| c[1].to_string()));
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        if let Some(Ok(byte)) = hex
            .filter(|_| b[i] == b'%')
            .map(|h| u8::from_str_radix(h, 16))
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `target` read from the doc at `from`, as a repo-relative path. `None` when
/// it climbs out of the repo.
fn resolve(from: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = match target.strip_prefix('/') {
        Some(_) => Vec::new(),
        None => from
            .rsplit_once('/')
            .map_or_else(Vec::new, |(dir, _)| dir.split('/').collect()),
    };
    for seg in target.trim_start_matches('/').split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// Checks every link in the scanned `docs` (path → contents). `exists` answers
/// for a target that isn't one of `docs`.
fn check(docs: &BTreeMap<String, String>, exists: impl Fn(&str) -> bool) -> Report {
    let anchor_sets: HashMap<&str, HashSet<String>> =
        docs.iter().map(|(p, t)| (p.as_str(), anchors(t))).collect();
    let mut r = Report::default();
    let scheme = Regex::new(r"^[A-Za-z][A-Za-z0-9+.-]*:").expect("regex");
    for (from, text) in docs {
        if NOT_SCANNED.iter().any(|skip| from.starts_with(skip)) {
            continue;
        }
        r.files += 1;
        for (n, target) in link_targets(text) {
            if target.is_empty() || scheme.is_match(&target) {
                continue;
            }
            r.links += 1;
            let (path, fragment) = target.split_once('#').unwrap_or((target.as_str(), ""));
            let path = percent_decode(path);
            let resolved = if path.is_empty() {
                Some(from.clone())
            } else {
                resolve(from, &path)
            };
            let Some(resolved) = resolved else {
                r.broken
                    .push(format!("{from}:{n}: `{target}` leaves the repo"));
                continue;
            };
            let in_docs = anchor_sets.get(resolved.as_str());
            if in_docs.is_none() && !exists(&resolved) {
                r.broken
                    .push(format!("{from}:{n}: `{target}` names no file"));
                continue;
            }
            // A fragment into anything but a doc (`main.rs#L10`) isn't checked.
            let Some(anchors) = in_docs.filter(|_| !fragment.is_empty()) else {
                continue;
            };
            r.fragments += 1;
            let fragment = percent_decode(fragment);
            let lands = anchors.contains(&fragment) || anchors.contains(&fragment.to_lowercase());
            if !lands {
                r.broken.push(format!(
                    "{from}:{n}: `{target}` names no heading or anchor in {resolved}"
                ));
            }
        }
    }
    r
}

/// Where a reference may point: a heading by name, or a numbered section and
/// optionally a numbered item in it.
#[derive(Debug, PartialEq, Eq)]
enum Section {
    Named(String),
    Numbered(String, Option<String>),
}

impl std::fmt::Display for Section {
    /// The reference as it is written: `"Name"`, or `§N` / `§N.M`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named(name) => write!(f, "\"{name}\""),
            Self::Numbered(num, None) => write!(f, "§{num}"),
            Self::Numbered(num, Some(item)) => write!(f, "§{num}.{item}"),
        }
    }
}

/// The section references on `line`: `FILE.md → "Name"` (the quotes may be
/// escaped, as in a Rust string) and `FILE.md §N` / `§N.M`.
fn section_refs(line: &str) -> Vec<(String, Section)> {
    static NAMED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"`?([A-Za-z0-9_./-]+\.md)`?\s*→\s*\\?["“]([^"”\\]+)\\?["”]"#).expect("regex")
    });
    static NUMBERED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"`?([A-Za-z0-9_./-]+\.md)`?\s*(?:→\s*)?§\s*(\d+)(?:\.(\d+))?").expect("regex")
    });
    let named = NAMED
        .captures_iter(line)
        .map(|c| (c[1].to_string(), Section::Named(c[2].to_string())));
    let numbered = NUMBERED.captures_iter(line).map(|c| {
        let item = c.get(3).map(|m| m.as_str().to_string());
        (c[1].to_string(), Section::Numbered(c[2].to_string(), item))
    });
    named.chain(numbered).collect()
}

/// A heading or rule name as a reference spells it: no code or bold marks,
/// spaces collapsed, no trailing punctuation, lowercase.
fn normalize(s: &str) -> String {
    let plain: String = s.chars().filter(|c| !matches!(c, '`' | '*')).collect();
    plain
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', ':', ',', ';'])
        .to_lowercase()
}

/// `text`'s headings, each with its level, and its bold leads (`**Rule.**`
/// opening a line or a bullet, level 0): what a section reference may name.
fn section_names(text: &str) -> Vec<(usize, usize, String)> {
    static HEADING: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^ {0,3}(#{1,6})\s+(.*?)\s*$").expect("regex"));
    static LEAD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(?:[-*]\s+)?\*\*(.+?)\*\*").expect("regex"));
    let mut out = Vec::new();
    for (n, line) in prose_lines(text) {
        if let Some(c) = HEADING.captures(line) {
            out.push((n, c[1].len(), normalize(&c[2])));
        } else if let Some(c) = LEAD.captures(line) {
            out.push((n, 0, normalize(&c[1])));
        }
    }
    out
}

/// Whether `text` has the section `want` names.
fn has_section(text: &str, want: &Section) -> bool {
    let names = section_names(text);
    match want {
        Section::Named(name) => {
            let name = normalize(name);
            names.iter().any(|(.., n)| n.starts_with(&name))
        }
        Section::Numbered(num, item) => {
            let Some(&(line, level, _)) = names.iter().find(|(_, level, n)| {
                *level > 0
                    && (n.starts_with(&format!("{num}.")) || n.starts_with(&format!("{num} ")))
            }) else {
                return false;
            };
            let Some(item) = item else { return true };
            let end = names
                .iter()
                .find(|(l, lv, _)| *l > line && *lv > 0 && *lv <= level)
                .map_or(usize::MAX, |&(l, ..)| l);
            prose_lines(text)
                .into_iter()
                .filter(|(n, _)| *n > line && *n < end)
                .any(|(_, l)| l.trim_start().starts_with(&format!("{item}. ")))
        }
    }
}

/// The doc a reference written in `from` names: as a repo path, relative to
/// `from`, or by a file name only one live doc has.
fn resolve_doc(from: &str, name: &str, docs: &BTreeMap<String, String>) -> Option<String> {
    if docs.contains_key(name) {
        return Some(name.to_string());
    }
    if let Some(rel) = resolve(from, name).filter(|r| docs.contains_key(r)) {
        return Some(rel);
    }
    let base = name.rsplit('/').next().unwrap_or(name);
    let mut hits = docs.keys().filter(|p| {
        p.rsplit('/').next() == Some(base) && !NOT_SCANNED.iter().any(|s| p.starts_with(s))
    });
    let first = hits.next()?;
    hits.next().is_none().then(|| first.clone())
}

/// Each directory of pages and the hub that must link every one of them: a page
/// nobody links to is found only by someone already looking for it.
const HUBS: &[(&str, &str)] = &[
    ("docs/config/", "CONFIGURATION.md"),
    ("docs/features/", "FEATURES.md"),
];

/// The repo-relative files `text`, the doc at `from`, links to.
fn linked_files(from: &str, text: &str) -> HashSet<String> {
    let scheme = Regex::new(r"^[A-Za-z][A-Za-z0-9+.-]*:").expect("regex");
    link_targets(text)
        .into_iter()
        .filter(|(_, t)| !scheme.is_match(t))
        .filter_map(|(_, t)| {
            let path = t.split('#').next().unwrap_or_default();
            (!path.is_empty()).then(|| resolve(from, &percent_decode(path)))?
        })
        .collect()
}

/// Not read for section references: history, test data, and this file's own
/// fixtures, which name sections that don't exist on purpose.
const REFS_NOT_SCANNED: &[&str] = &[
    "CHANGELOG.md",
    "docs/archive/",
    "docs/changelog/",
    "fuzz/",
    "src/link_guard.rs",
    "tests/fixtures/",
];

#[test]
fn every_section_reference_resolves() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |f: &String| {
        std::fs::read_to_string(root.join(f))
            .ok()
            .map(|t| (f.clone(), t))
    };
    let docs: BTreeMap<String, String> = crate::guard_support::markdown_files(root)
        .iter()
        .filter_map(read)
        .collect();
    let sources = crate::guard_support::repo_files(root, &["md", "rs", "toml", "yml", "sh"]);
    let mut checked = 0;
    let mut broken = Vec::new();
    for (from, text) in sources
        .iter()
        .filter(|f| !REFS_NOT_SCANNED.iter().any(|s| f.starts_with(s)))
        .filter_map(read)
    {
        let markdown = Path::new(&from)
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("md"));
        let lines: Vec<(usize, &str)> = if markdown {
            prose_lines(&text)
        } else {
            text.lines().enumerate().map(|(i, l)| (i + 1, l)).collect()
        };
        for (n, line) in lines {
            for (doc, section) in section_refs(line) {
                checked += 1;
                match resolve_doc(&from, &doc, &docs) {
                    None => broken.push(format!("{from}:{n}: `{doc}` names no doc")),
                    Some(target) if !has_section(&docs[&target], &section) => {
                        broken.push(format!("{from}:{n}: `{doc}` has no section {section}"));
                    }
                    Some(_) => {}
                }
            }
        }
    }
    assert!(
        checked > 40,
        "checked only {checked} references; the reader is broken"
    );
    assert!(
        broken.is_empty(),
        "{} section reference(s) name nothing — a moved or renamed section takes its \
         references with it:\n{}",
        broken.len(),
        broken.join("\n")
    );
}

#[test]
fn a_section_reference_names_a_heading_a_rule_or_a_numbered_item() {
    let doc = "# Guide\n## Mouse routing\n- **Never pipe the gate.** Run it bare.\n## 8. CI\n1. lint\n2. test\n## 9. Release\n3. tag\n";
    let named = |n: &str| Section::Named(n.to_string());
    let num = |n: &str, i: Option<&str>| Section::Numbered(n.to_string(), i.map(str::to_string));
    assert!(has_section(doc, &named("Mouse routing")));
    assert!(has_section(doc, &named("mouse ROUTING.")));
    assert!(has_section(doc, &named("Never pipe the gate")));
    assert!(!has_section(doc, &named("Keyboard routing")));
    assert!(has_section(doc, &num("8", None)) && has_section(doc, &num("8", Some("2"))));
    assert!(
        !has_section(doc, &num("8", Some("3"))),
        "item 3 is in section 9"
    );
    assert!(!has_section(doc, &num("10", None)));
}

#[test]
fn section_references_are_read_in_every_written_form() {
    assert_eq!(
        section_refs(r#"see `ARCHITECTURE.md` → "Mouse routing", and RELEASE.md §13.8"#),
        [
            (
                "ARCHITECTURE.md".to_string(),
                Section::Named("Mouse routing".to_string())
            ),
            (
                "RELEASE.md".to_string(),
                Section::Numbered("13".to_string(), Some("8".to_string()))
            ),
        ]
    );
    assert_eq!(
        section_refs(r#""see AGENTS.md → \"Architecture\".""#),
        [(
            "AGENTS.md".to_string(),
            Section::Named("Architecture".to_string())
        )]
    );
    assert!(
        section_refs("AGENTS.md → Conventions").is_empty(),
        "an unquoted name isn't read"
    );
}

#[test]
fn every_feature_and_config_page_is_linked_from_its_hub() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let docs = crate::guard_support::markdown_files(root);
    let mut orphans = Vec::new();
    for &(dir, hub) in HUBS {
        let text = std::fs::read_to_string(root.join(hub)).expect("read the hub");
        let linked = linked_files(hub, &text);
        let pages: Vec<&String> = docs.iter().filter(|f| f.starts_with(dir)).collect();
        assert!(
            !pages.is_empty(),
            "no pages under {dir}; the walk is broken"
        );
        orphans.extend(
            pages
                .into_iter()
                .filter(|page| !linked.contains(*page))
                .map(|page| format!("{page}: not linked from {hub}")),
        );
    }
    assert!(
        orphans.is_empty(),
        "{}\nGive each page a summary and a link in its hub.",
        orphans.join("\n")
    );
}

#[test]
fn every_relative_markdown_link_resolves() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let docs: BTreeMap<String, String> = crate::guard_support::markdown_files(root)
        .into_iter()
        .map(|f| {
            let text =
                std::fs::read_to_string(root.join(&f)).unwrap_or_else(|e| panic!("{f}: {e}"));
            (f, text)
        })
        .collect();
    let r = check(&docs, |p| root.join(p).exists());
    // A walk or a reader that stops matching checks nothing and passes.
    assert!(
        r.files > 40,
        "scanned only {} docs — the walk is broken",
        r.files
    );
    assert!(
        r.links > 100,
        "checked only {} links — the reader is broken",
        r.links
    );
    assert!(
        r.fragments > 15,
        "checked only {} fragments — the reader is broken",
        r.fragments
    );
    assert!(
        r.broken.is_empty(),
        "{} broken link(s) — a moved doc's inbound links move with it:\n{}",
        r.broken.len(),
        r.broken.join("\n")
    );
}

fn docs(files: &[(&str, &str)]) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|(p, t)| ((*p).to_string(), (*t).to_string()))
        .collect()
}

#[test]
fn slug_matches_github_for_this_repos_headings() {
    assert_eq!(
        slug("Agent HUD — `[pane] agent_hud`"),
        "agent-hud--pane-agent_hud"
    );
    assert_eq!(
        slug("Git views — diff / show / blame"),
        "git-views--diff--show--blame"
    );
    assert_eq!(slug("The `spyc.*` API"), "the-spyc-api");
    assert_eq!(slug("See [the plan](x.md) first"), "see-the-plan-first");
}

#[test]
fn a_repeated_heading_takes_a_numeric_suffix() {
    let a = anchors("## Notes\n\n## Notes\n\n## Notes\n");
    assert!(a.contains("notes") && a.contains("notes-1") && a.contains("notes-2"));
}

#[test]
fn html_ids_are_anchors_and_fenced_headings_are_not() {
    let a = anchors("<a name=\"top\"></a>\n<h2 id=\"x\">X</h2>\n```\n# not a heading\n```\n");
    assert!(a.contains("top") && a.contains("x"));
    assert!(!a.contains("not-a-heading"));
}

#[test]
fn code_and_fences_hide_links() {
    let text = "see `[a](gone.md)` and ``[b](`gone.md`)``\n```\n[c](gone.md)\n```\n~~~~\n```\n[d](gone.md)\n~~~~\n";
    assert!(link_targets(text).is_empty(), "{:?}", link_targets(text));
}

#[test]
fn every_link_shape_is_read() {
    let text = "[a](a.md) ![b](b.png \"title\") [c](<c d.md>)\n[ref]: e.md\n[^1]: a footnote\n<img src=\"f.svg\">\n";
    let got: Vec<String> = link_targets(text).into_iter().map(|(_, t)| t).collect();
    assert_eq!(got, ["a.md", "b.png", "c d.md", "e.md", "f.svg"]);
}

#[test]
fn a_missing_file_a_missing_heading_and_an_escape_are_reported() {
    let r = check(
        &docs(&[
            (
                "README.md",
                "# Top\n[ok](docs/A.md#usage) [self](#top) [img](logo.png)\n",
            ),
            (
                "docs/A.md",
                "## Usage\n[gone](B.md) [stale](../README.md#install) [out](../../x.md)\n",
            ),
        ]),
        |p| p == "logo.png",
    );
    assert_eq!(r.links, 6);
    assert_eq!(r.broken.len(), 3, "{:#?}", r.broken);
    assert!(r.broken[0].starts_with("docs/A.md:2: `B.md` names no file"));
    assert!(r.broken[1].contains("names no heading or anchor in README.md"));
    assert!(r.broken[2].contains("leaves the repo"));
}

#[test]
fn urls_are_not_checked_and_history_is_not_scanned() {
    let r = check(
        &docs(&[
            (
                "README.md",
                "[w](https://x.dev/a.md) [m](mailto:a@b.c) [arc](docs/archive/old.md#gone)\n",
            ),
            ("docs/archive/old.md", "# Old\n[dead](nowhere.md)\n"),
        ]),
        |_| false,
    );
    assert_eq!(r.files, 1);
    assert_eq!(r.broken.len(), 1, "{:#?}", r.broken);
    assert!(r.broken[0].contains("`docs/archive/old.md#gone` names no heading"));
}

#[test]
fn a_hub_links_a_page_however_the_link_is_written() {
    let linked = linked_files(
        "FEATURES.md",
        "[a](docs/features/a.md) [b](./docs/features/b.md#top) <img src=\"docs/c.png\"> [w](https://x.dev/d.md)\n",
    );
    assert!(linked.contains("docs/features/a.md") && linked.contains("docs/features/b.md"));
    assert!(linked.contains("docs/c.png"));
    assert_eq!(linked.len(), 3, "{linked:?}");
}

#[test]
fn percent_encoded_paths_resolve() {
    let r = check(
        &docs(&[
            ("README.md", "[s](docs/a%20b.md)\n"),
            ("docs/a b.md", "# A\n"),
        ]),
        |_| false,
    );
    assert!(r.broken.is_empty(), "{:#?}", r.broken);
}
