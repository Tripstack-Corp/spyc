//! Word-level (intraline) highlight ranges for a modified line pair.
//!
//! A removed line and its added counterpart are tokenized into word /
//! whitespace / punctuation runs and diffed with the same `imara-diff` engine
//! that built the hunks upstream ([`crate::git::diff_model`]), so **every**
//! changed region is reported. Trimming a common prefix and suffix — the cheap
//! approach — yields exactly one region, which collapses
//! `foo(a, b, c)` → `foo(x, b, y)` into the single blob `a, b, c` / `x, b, y`
//! and washes the unchanged `b` in the highlight color.
//!
//! **Word tokens, not chars.** Char-level diffs of code lines scatter into
//! confetti. Punctuation is one token per char, so the argument list above
//! breaks at every delimiter and highlights `a`→`x` and `c`→`y` separately;
//! that puts the tokenizer between git's whitespace-only `--word-diff` default
//! and `--word-diff-regex=.`, matching `--word-diff-regex='\w+|[^[:space:]]'`.

use gix::diff::blob::{Algorithm, Diff, InternedInput};
use std::ops::Range;

/// Token ceiling per side, above which a line pair gets no highlight. Bounds
/// the per-pair diff cost for minified/generated lines, where a word highlight
/// is unreadable noise anyway — every paired line in a hunk pays this, and a
/// hunk set runs to [`MAX_DIFF_LINES`](crate::git::diff_model::MAX_DIFF_LINES).
const MAX_TOKENS: usize = 512;

/// Character class driving tokenization.
#[derive(PartialEq, Eq, Clone, Copy)]
enum Class {
    Word,
    Space,
    Punct,
}

fn class_of(c: char) -> Class {
    if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else if c.is_whitespace() {
        Class::Space
    } else {
        Class::Punct
    }
}

/// Byte ranges of `s`'s tokens: runs of word chars, runs of whitespace, and one
/// range per punctuation char. Contiguous and covering, so a token-index range
/// maps back to a byte range exactly.
fn tokenize(s: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut it = s.char_indices().peekable();
    while let Some((start, c)) = it.next() {
        let class = class_of(c);
        let mut end = start + c.len_utf8();
        if class != Class::Punct {
            while let Some(&(j, next)) = it.peek() {
                if class_of(next) != class {
                    break;
                }
                end = j + next.len_utf8();
                it.next();
            }
        }
        out.push(start..end);
    }
    out
}

/// The changed byte ranges between a removed line `old` and its added
/// counterpart `new`, as disjoint ascending ranges into each line's own text.
///
/// Empty on both sides for identical lines, and when *everything* changed — a
/// uniformly brighter line adds nothing over the row wash.
pub(super) fn intra_change_ranges(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let empty = || (Vec::new(), Vec::new());
    if old == new {
        return empty();
    }
    let (old_toks, new_toks) = (tokenize(old), tokenize(new));
    if old_toks.len() > MAX_TOKENS || new_toks.len() > MAX_TOKENS {
        return empty();
    }

    let mut input: InternedInput<&str> = InternedInput::default();
    for r in &old_toks {
        let tok = input.interner.intern(&old[r.start..r.end]);
        input.before.push(tok);
    }
    for r in &new_toks {
        let tok = input.interner.intern(&new[r.start..r.end]);
        input.after.push(tok);
    }
    let diff = Diff::compute(Algorithm::Histogram, &input);

    let (mut old_out, mut new_out) = (Vec::new(), Vec::new());
    for h in diff.hunks() {
        push_span(&mut old_out, &old_toks, &h.before, old);
        push_span(&mut new_out, &new_toks, &h.after, new);
    }
    // Nothing shared at either end on both sides ⇒ the whole pair changed.
    if spans_whole(&old_out, old.len()) && spans_whole(&new_out, new.len()) {
        return empty();
    }
    (old_out, new_out)
}

/// Translate one hunk's token-index `span` into a byte range, trim
/// whitespace-only tokens off both ends, and fuse it with the previous range
/// when they touch. Trimming keeps the highlight on the changed word rather
/// than trailing into the space after it — except when the change *is*
/// whitespace, where the highlight is the only way to see it at all.
fn push_span(out: &mut Vec<Range<usize>>, toks: &[Range<usize>], span: &Range<u32>, text: &str) {
    let (mut lo, mut hi) = (span.start as usize, span.end as usize);
    if lo >= hi {
        return;
    }
    let is_blank = |t: &Range<usize>| text[t.start..t.end].trim().is_empty();
    while toks[lo..hi].iter().any(|t| !is_blank(t)) {
        if is_blank(&toks[lo]) {
            lo += 1;
        } else if is_blank(&toks[hi - 1]) {
            hi -= 1;
        } else {
            break;
        }
    }
    let range = toks[lo].start..toks[hi - 1].end;
    match out.last_mut() {
        Some(prev) if prev.end == range.start => prev.end = range.end,
        _ => out.push(range),
    }
}

/// Whether `ranges` (disjoint, ascending, already fused where touching) cover
/// all of `0..len`. An empty side is trivially covered.
fn spans_whole(ranges: &[Range<usize>], len: usize) -> bool {
    len == 0 || matches!(ranges, [only] if only.start == 0 && only.end == len)
}

#[cfg(test)]
mod tests {
    use super::{intra_change_ranges, tokenize};

    /// Render `ranges` as the substrings they select, for readable assertions.
    fn picks<'a>(text: &'a str, ranges: &[std::ops::Range<usize>]) -> Vec<&'a str> {
        ranges.iter().map(|r| &text[r.start..r.end]).collect()
    }

    #[test]
    fn tokenize_splits_words_space_and_each_punct_char() {
        let s = "foo(a, b)";
        assert_eq!(
            picks(s, &tokenize(s)),
            vec!["foo", "(", "a", ",", " ", "b", ")"]
        );
    }

    #[test]
    fn tokenize_is_contiguous_and_covering() {
        for s in ["", "a", "  ", "a  b", "ünïcödé(x)", "\t\tif x:"] {
            let toks = tokenize(s);
            assert_eq!(toks.first().map_or(0, |t| t.start), 0, "{s:?} starts at 0");
            assert_eq!(
                toks.last().map_or(0, |t| t.end),
                s.len(),
                "{s:?} covers end"
            );
            for w in toks.windows(2) {
                assert_eq!(w[0].end, w[1].start, "{s:?} has a gap/overlap");
            }
        }
    }

    /// The #179 class: the old prefix/suffix trim reported ONE region spanning
    /// `a, b, c` → `x, b, y`, bleeding the unchanged `b` into the highlight.
    #[test]
    fn unchanged_middle_is_not_swept_into_the_highlight() {
        let (old, new) = ("foo(a, b, c)", "foo(x, b, y)");
        let (o, n) = intra_change_ranges(old, new);
        assert_eq!(picks(old, &o), vec!["a", "c"], "two regions, not one blob");
        assert_eq!(picks(new, &n), vec!["x", "y"]);
        // The unchanged token must fall outside every highlight range.
        let b = old.find(" b").expect("has b") + 1;
        assert!(
            !o.iter().any(|r| r.contains(&b)),
            "unchanged `b` must not be highlighted"
        );
    }

    #[test]
    fn changes_at_both_ends_keep_the_middle_clean() {
        let (old, new) = ("aaa keep zzz", "bbb keep yyy");
        let (o, n) = intra_change_ranges(old, new);
        assert_eq!(picks(old, &o), vec!["aaa", "zzz"]);
        assert_eq!(picks(new, &n), vec!["bbb", "yyy"]);
    }

    #[test]
    fn single_changed_token_is_the_only_highlight() {
        let (old, new) = ("let x = 1;", "let x = 2;");
        let (o, n) = intra_change_ranges(old, new);
        assert_eq!(picks(old, &o), vec!["1"]);
        assert_eq!(picks(new, &n), vec!["2"]);
    }

    #[test]
    fn pure_insertion_leaves_the_old_side_unhighlighted() {
        let (old, new) = ("a b", "a X b");
        let (o, n) = intra_change_ranges(old, new);
        assert!(o.is_empty(), "nothing was removed, so nothing to mark");
        assert_eq!(picks(new, &n), vec!["X"], "and the space is trimmed off");
    }

    #[test]
    fn identical_lines_get_no_highlight() {
        assert_eq!(intra_change_ranges("same", "same"), (vec![], vec![]));
    }

    #[test]
    fn wholly_different_lines_get_no_highlight() {
        // A uniformly brighter line adds nothing over the row wash.
        for (old, new) in [("abc", "xyz"), ("", "abc"), ("abc", "")] {
            let (o, n) = intra_change_ranges(old, new);
            assert!(
                o.is_empty() && n.is_empty(),
                "{old:?} -> {new:?} should not highlight"
            );
        }
    }

    #[test]
    fn a_whitespace_only_change_is_still_visible() {
        // Trailing-whitespace and indent changes are invisible without it, so
        // the edge-trim must not erase a change that is ENTIRELY whitespace.
        let (old, new) = ("x = 1", "x  = 1");
        let (o, n) = intra_change_ranges(old, new);
        assert!(!n.is_empty(), "the added whitespace must be marked");
        assert!(
            picks(new, &n).iter().all(|s| s.trim().is_empty()),
            "and only whitespace is marked: {:?}",
            picks(new, &n)
        );
        assert!(o.is_empty() || picks(old, &o).iter().all(|s| s.trim().is_empty()));
    }

    #[test]
    fn ranges_are_disjoint_ascending_and_on_char_boundaries() {
        // `overlay_range_bg` requires char boundaries and counts each range's
        // bytes once; overlapping or descending ranges would double-count.
        let cases = [
            ("ünïcödé(a, b)", "ünïcödé(x, b)"),
            ("日本 keep 語", "日本語 keep 語"),
            ("a(b)c(d)e", "a(z)c(w)e"),
        ];
        for (old, new) in cases {
            let (o, n) = intra_change_ranges(old, new);
            for (text, ranges) in [(old, &o), (new, &n)] {
                for w in ranges.windows(2) {
                    assert!(w[0].end < w[1].start, "{text:?} ranges touch/overlap");
                }
                for r in ranges {
                    assert!(text.is_char_boundary(r.start), "{text:?} start boundary");
                    assert!(text.is_char_boundary(r.end), "{text:?} end boundary");
                }
            }
        }
    }

    #[test]
    fn absurdly_long_lines_fall_back_to_no_highlight() {
        let old = "a,".repeat(super::MAX_TOKENS);
        let new = format!("{old}b");
        assert_eq!(intra_change_ranges(&old, &new), (vec![], vec![]));
    }
}
