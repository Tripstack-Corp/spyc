//! Quick Select — a labeled-overlay picker for URLs / paths / git
//! SHAs / IPs / custom-regex matches in the bottom pane's visible
//! output. Borrowed wholesale from WezTerm's "Quick Select" mode
//! (https://wezterm.org/quickselect.html); the user-visible model
//! is identical:
//!
//!   1. Press `^a u` to enter the mode.
//!   2. Spyc scans the visible viewport, finds matches for each
//!      registered pattern, and overlays a 1- or 2-letter label on
//!      each match.
//!   3. Press the label letters → match is yanked to the clipboard,
//!      mode exits.
//!   4. Press the **uppercase** form of the label letters → "open"
//!      intent: URLs hand off to `open`/`xdg-open`, paths
//!      cursor-jump in spyc, SHAs `git show` into the pager.
//!      Other kinds fall back to yank with a flash hint.
//!   5. `q` or `Esc` exits without action.
//!
//! Why this lives under `pane/`: Quick Select is only meaningful
//! when there's a pty pane to scan, and its input is the pane's
//! visible text grid — co-located with the rest of the pane code.
//!
//! Scope: always scans `visible_lines()` (exactly what's on screen
//! at the moment of `^a u`). That means scroll mode "just works" —
//! scroll up to a Claude reply, hit `^a u`, the URLs in *that*
//! reply get labels. We don't dip into off-screen scrollback; we
//! couldn't draw labels there anyway.

use regex::Regex;

/// Reserved keys we mustn't generate as labels: `q`/`Q` exits the
/// mode; the alphabet skips both. Keeping it lowercase-only keeps
/// the uppercase forms free for the "open" intent (`A` opens `a`,
/// etc.). We also skip `j`/`k` so that someone who reflexively
/// reaches for vi motions during the mode just gets ignored input
/// instead of an accidental action.
const ALPHABET: &[u8] = b"abcdefghilmnoprstuvwxyz";

/// Hard cap. With a 23-letter alphabet, 23² = 529 two-letter
/// labels. If a viewport produces more matches than that, we trim
/// to this many (oldest-first): they're the most likely to scroll
/// past anyway, and a forest of 3-letter labels would be unreadable.
pub const MAX_MATCHES: usize = ALPHABET.len() * ALPHABET.len();

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchKind {
    Url,
    Path,
    GitSha,
    Ipv4,
    /// User-defined pattern. `url_template` (if set) is the
    /// `https://.../{}` string that turns the matched text into
    /// an openable URL; `{}` is replaced with the match.
    Custom {
        name: String,
        url_template: Option<String>,
    },
}

impl MatchKind {
    /// Short tag for status flashes ("yanked URL", "yanked path").
    pub fn label(&self) -> &str {
        match self {
            Self::Url => "URL",
            Self::Path => "path",
            Self::GitSha => "SHA",
            Self::Ipv4 => "IP",
            Self::Custom { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Match {
    pub text: String,
    pub kind: MatchKind,
    /// 1- or 2-letter label assigned during scan. Lowercase only;
    /// the dispatch layer interprets uppercase keystrokes as
    /// "open" intent for the same label.
    pub label: String,
    /// 0-based row in the snapshot (= row in the visible pane).
    pub row: usize,
    /// 0-based terminal column where the match begins — the display
    /// width of the line content preceding it, not a byte offset, so a
    /// label over a line with wide/multibyte chars still lands on the
    /// match (the overlay renders at `pane_rect.x + col`).
    pub col: usize,
}

/// Parsed/compiled Quick Select state, held on `App` while the
/// mode is active. The snapshot of visible text isn't retained —
/// we extract matches with their coordinates and let the live pane
/// keep rendering underneath. If the pane scrolls during the mode
/// the labels go stale; the simplest correct behavior is to close
/// the picker on the next event tick that detects pane growth.
pub struct QuickSelect {
    pub matches: Vec<Match>,
    /// First keystroke buffered when labels are 2-letter. `None`
    /// at entry; set to `Some(c)` after the first label keystroke
    /// matches a known prefix; commits on the second keystroke.
    pub pending_first: Option<char>,
    /// True iff every label is 2-letter (i.e. >|ALPHABET| matches).
    /// Determines whether the first keystroke can commit (1-letter
    /// case) or always narrows (2-letter case).
    pub all_two_letter: bool,
    /// "Open intent" sticky bit. Set by an *uppercase* first
    /// keystroke in the 2-letter case (so `Ab` opens, same as `aB`
    /// or `AB`). In the 1-letter case it's unused (uppercase
    /// commits directly).
    pub open_intent: bool,
}

/// A single user-defined pattern from `.spycrc.toml`. Compiled
/// once at config load; bad regexes are dropped with a warning
/// and don't fail the whole config.
#[derive(Debug, Clone)]
pub struct CustomPattern {
    pub name: String,
    pub regex: Regex,
    pub url_template: Option<String>,
}

/// Built-in URL pattern. `\S+` reaches everything up to whitespace
/// (so `?foo=bar&baz=qux#anchor` is captured); we trim trailing
/// punctuation in `trim_url` before yielding the match because
/// `Look at https://example.com.` shouldn't include the period.
const URL_PATTERN: &str = r"https?://\S+";
/// 7-40 hex chars, word-bounded. 7 is `git --short`'s default;
/// the upper bound rules out 64-hex SHA-256 hashes that aren't
/// SHAs (commonly seen in Cargo.lock checksums).
const GIT_SHA_PATTERN: &str = r"\b[0-9a-f]{7,40}\b";
/// IPv4 — naïve but good enough for picker UX. Doesn't validate
/// that octets are 0-255 (would just refuse 256.256.256.256 etc.,
/// which never appear in real output anyway).
const IPV4_PATTERN: &str = r"\b\d{1,3}(?:\.\d{1,3}){3}\b";
/// Filesystem-ish paths. Conservative: must contain a `/` (no
/// bare filenames) and stop at whitespace, quote, or `>`. Lets
/// `gf`/`gF` keep their dedicated muscle-memory binding while
/// also surfacing paths via the picker.
const PATH_PATTERN: &str = r"[\w./~][\w./~+\-]*/[\w./~+\-]+";

/// Trim trailing punctuation that's almost always *sentence*
/// punctuation rather than part of the URL. Matches the trim
/// WezTerm uses internally. Note: parens are intentionally NOT
/// trimmed — many real URLs have balanced parens (Wikipedia,
/// MSDN). This errs on capturing slightly too much rather than
/// silently dropping a URL char.
pub fn trim_url(s: &str) -> &str {
    s.trim_end_matches(['.', ',', ';', ':', '!', '?', ']', '}', '>'])
}

/// Compile every built-in and user-defined regex into one
/// dispatch table. Returns one `(Regex, MatchKind)` per pattern;
/// the caller iterates each line through each regex.
///
/// We deliberately don't use `RegexSet`: it tells us *which*
/// regexes matched but not *where*, and we need the spans to
/// place labels. With the small fixed pattern count, individual
/// `Regex::find_iter` is plenty fast (microseconds over a 24×200
/// viewport).
pub fn build_patterns(custom: &[CustomPattern]) -> Vec<(Regex, MatchKind)> {
    let mut out: Vec<(Regex, MatchKind)> = Vec::new();
    // Built-in order matters for overlap resolution: when two
    // regexes match overlapping text on the same line, we keep
    // the earlier pattern. URL first because it's the most
    // common reason to use the picker; path last (broadest, most
    // likely to over-match other things if it ran first).
    if let Ok(r) = Regex::new(URL_PATTERN) {
        out.push((r, MatchKind::Url));
    }
    if let Ok(r) = Regex::new(GIT_SHA_PATTERN) {
        out.push((r, MatchKind::GitSha));
    }
    if let Ok(r) = Regex::new(IPV4_PATTERN) {
        out.push((r, MatchKind::Ipv4));
    }
    for p in custom {
        out.push((
            p.regex.clone(),
            MatchKind::Custom {
                name: p.name.clone(),
                url_template: p.url_template.clone(),
            },
        ));
    }
    if let Ok(r) = Regex::new(PATH_PATTERN) {
        out.push((r, MatchKind::Path));
    }
    out
}

/// Find every match for every pattern in `lines`, dropping
/// overlapping matches (earlier pattern wins). Returns matches in
/// scan order — top-to-bottom, left-to-right.
pub fn scan(lines: &[String], patterns: &[(Regex, MatchKind)]) -> Vec<Match> {
    let mut out: Vec<Match> = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        // Per-line overlap rejection: track byte ranges already
        // claimed so a path regex can't double up on the URL it
        // already matched.
        let mut claimed: Vec<(usize, usize)> = Vec::new();
        for (regex, kind) in patterns {
            for m in regex.find_iter(line) {
                let (start, end) = (m.start(), m.end());
                if claimed.iter().any(|&(s, e)| start < e && end > s) {
                    continue;
                }
                let raw = m.as_str();
                let text = if matches!(kind, MatchKind::Url) {
                    trim_url(raw).to_string()
                } else {
                    raw.to_string()
                };
                if text.is_empty() {
                    continue;
                }
                claimed.push((start, start + text.len()));
                out.push(Match {
                    text,
                    kind: kind.clone(),
                    label: String::new(), // assigned below
                    row,
                    // Display column, not byte offset: `start` is a char
                    // boundary (regex match), so the width of everything
                    // before it is where the match sits on screen.
                    col: crate::ui::display_width(&line[..start]),
                });
            }
        }
        // Re-sort matches on this line by column so the global
        // scan order is "reading order" — important so labels go
        // a, b, c, ... in the order the user's eye traverses the
        // screen. Without this, URL matches would all bunch first.
        let line_start = out.len() - out.iter().rev().take_while(|mm| mm.row == row).count();
        out[line_start..].sort_by_key(|m| m.col);
    }
    out
}

/// Assign labels to a slice of matches. Mutates `matches` in place,
/// truncating to `MAX_MATCHES` if there are too many. Labels use
/// 1-letter when possible, 2-letter otherwise. Returns
/// `all_two_letter` so the caller knows how to interpret keystrokes.
pub fn assign_labels(matches: &mut Vec<Match>) -> bool {
    if matches.len() > MAX_MATCHES {
        matches.truncate(MAX_MATCHES);
    }
    let n = ALPHABET.len();
    if matches.len() <= n {
        for (i, m) in matches.iter_mut().enumerate() {
            m.label = (ALPHABET[i] as char).to_string();
        }
        false
    } else {
        for (i, m) in matches.iter_mut().enumerate() {
            let first = ALPHABET[i / n];
            let second = ALPHABET[i % n];
            m.label = format!("{}{}", first as char, second as char);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Vec<(Regex, MatchKind)> {
        build_patterns(&[])
    }

    #[test]
    fn scans_url_path_sha_ip() {
        let lines = vec![
            "see https://example.com/foo?bar=baz for details".to_string(),
            "  edited src/app/mod.rs:5520".to_string(),
            "commit 0691666cafe1234".to_string(),
            "ping 192.168.1.1 ok".to_string(),
        ];
        let pats = defaults();
        let matches = scan(&lines, &pats);
        let kinds: Vec<&MatchKind> = matches.iter().map(|m| &m.kind).collect();
        assert!(kinds.contains(&&MatchKind::Url));
        assert!(kinds.contains(&&MatchKind::Path));
        assert!(kinds.contains(&&MatchKind::GitSha));
        assert!(kinds.contains(&&MatchKind::Ipv4));
    }

    #[test]
    fn url_trailing_period_is_trimmed() {
        let lines = vec!["check https://example.com/foo. okay".to_string()];
        let pats = defaults();
        let matches = scan(&lines, &pats);
        let url = matches
            .iter()
            .find(|m| matches!(m.kind, MatchKind::Url))
            .expect("url match");
        assert_eq!(url.text, "https://example.com/foo");
    }

    #[test]
    fn url_query_string_kept() {
        let lines =
            vec!["see https://example.com/foo?bar=baz&qux=1#anchor for details".to_string()];
        let pats = defaults();
        let matches = scan(&lines, &pats);
        let url = matches
            .iter()
            .find(|m| matches!(m.kind, MatchKind::Url))
            .expect("url match");
        assert_eq!(url.text, "https://example.com/foo?bar=baz&qux=1#anchor");
    }

    #[test]
    fn path_doesnt_double_match_url() {
        // "https://example.com/foo" contains slashes; the path
        // pattern would gladly match `example.com/foo` if we let
        // it. Overlap rejection should prevent that.
        let lines = vec!["https://example.com/foo".to_string()];
        let pats = defaults();
        let matches = scan(&lines, &pats);
        let path_count = matches
            .iter()
            .filter(|m| matches!(m.kind, MatchKind::Path))
            .count();
        assert_eq!(
            path_count, 0,
            "path pattern bled into URL match: {matches:?}"
        );
    }

    #[test]
    fn matches_sorted_in_reading_order() {
        let lines = vec!["first src/a.rs second https://x.com third 192.168.1.1".to_string()];
        let pats = defaults();
        let matches = scan(&lines, &pats);
        let cols: Vec<usize> = matches.iter().map(|m| m.col).collect();
        let mut sorted = cols.clone();
        sorted.sort_unstable();
        assert_eq!(cols, sorted, "matches not in reading order: {matches:?}");
    }

    #[test]
    fn col_is_display_column_not_byte_offset() {
        // "日本 " is 5 columns but 7 bytes. The match must report the
        // column (5) so its label lands on the URL, not 2 cells too far
        // right (the byte-offset bug).
        let lines = vec!["日本 https://example.com".to_string()];
        let matches = scan(&lines, &defaults());
        let url = matches
            .iter()
            .find(|m| matches!(m.kind, MatchKind::Url))
            .expect("url match");
        assert_eq!(url.col, 5, "col should be a display column: {url:?}");
    }

    #[test]
    fn labels_one_letter_when_few() {
        let mut matches = vec![
            Match {
                text: "a".into(),
                kind: MatchKind::Url,
                label: String::new(),
                row: 0,
                col: 0,
            },
            Match {
                text: "b".into(),
                kind: MatchKind::Url,
                label: String::new(),
                row: 0,
                col: 1,
            },
        ];
        let two = assign_labels(&mut matches);
        assert!(!two);
        assert_eq!(matches[0].label, "a");
        assert_eq!(matches[1].label, "b");
    }

    #[test]
    fn labels_two_letter_when_many() {
        let mut matches: Vec<Match> = (0..ALPHABET.len() + 5)
            .map(|i| Match {
                text: i.to_string(),
                kind: MatchKind::Url,
                label: String::new(),
                row: 0,
                col: i,
            })
            .collect();
        let two = assign_labels(&mut matches);
        assert!(two);
        assert!(matches.iter().all(|m| m.label.len() == 2));
    }

    #[test]
    fn labels_truncate_at_max() {
        let mut matches: Vec<Match> = (0..MAX_MATCHES + 10)
            .map(|i| Match {
                text: i.to_string(),
                kind: MatchKind::Url,
                label: String::new(),
                row: 0,
                col: i,
            })
            .collect();
        assign_labels(&mut matches);
        assert_eq!(matches.len(), MAX_MATCHES);
    }

    #[test]
    fn alphabet_excludes_q_jk() {
        // q is the exit key; j/k are reflexive vi motions we want
        // to leave inert during the mode. Verify they aren't in
        // the label alphabet.
        for c in [b'q', b'j', b'k'] {
            assert!(
                !ALPHABET.contains(&c),
                "{} is in the label alphabet",
                c as char
            );
        }
    }

    #[test]
    fn custom_pattern_round_trips_kind() {
        let custom = vec![CustomPattern {
            name: "jira".into(),
            regex: Regex::new(r"[A-Z]+-\d+").unwrap(),
            url_template: Some("https://example.atlassian.net/browse/{}".into()),
        }];
        let pats = build_patterns(&custom);
        let lines = vec!["fixed in PROJ-123".to_string()];
        let matches = scan(&lines, &pats);
        let m = matches
            .iter()
            .find(|m| matches!(m.kind, MatchKind::Custom { .. }))
            .expect("custom match");
        assert_eq!(m.text, "PROJ-123");
        if let MatchKind::Custom {
            ref name,
            ref url_template,
        } = m.kind
        {
            assert_eq!(name, "jira");
            assert_eq!(
                url_template.as_deref(),
                Some("https://example.atlassian.net/browse/{}")
            );
        }
    }
}
