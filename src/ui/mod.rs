//! Rendering. Layout decisions live in `layout`; individual widgets in
//! `list_view`, `status`, and `prompt`. Shared colors in `theme`.
//!
//! Width helpers: all UI code should use `display_width()` and
//! `display_truncate()` instead of `chars().count()` or `.len()`.

use unicode_truncate::UnicodeTruncateStr;
use unicode_width::UnicodeWidthStr;

pub mod blame_render;
pub mod color_depth;
pub mod diff_render;
pub mod help;
pub mod hex;
pub mod json;
pub mod line_edit;
pub mod line_select;
pub mod list_view;
pub mod markdown;
pub mod pager;
pub mod prompt;
pub mod scrollback;
pub mod status;
pub mod syntax;
pub mod theme;
pub mod wrap;

/// Display width of a string in terminal columns. CJK characters and
/// some emoji count as 2 columns; most Latin/symbol characters as 1.
pub fn display_width(s: &str) -> usize {
    s.width()
}

/// Truncate a string to at most `max` display columns. Returns the
/// truncated slice (no allocation when the string already fits).
///
/// Cuts on **grapheme-cluster** boundaries, not char boundaries: a cluster can
/// score a different width than the sum of its chars — `"❤️"` (U+2764 U+FE0F)
/// is 2 columns where its chars sum to 1, and a ZWJ emoji sums to 6 where the
/// terminal draws 2 — so a per-char walk both mismeasures the budget and is
/// free to slice a cluster in half.
pub fn display_truncate(s: &str, max: usize) -> &str {
    if s.width() <= max {
        return s;
    }
    s.unicode_truncate(max).0
}

/// Truncate a string to at most `max` display columns, keeping the
/// **tail** (rightmost columns) — the mirror of [`display_truncate`], with the
/// same cluster-boundary guarantee. Used for path displays that elide the head
/// (`…/deep/leaf`).
pub fn display_truncate_tail(s: &str, max: usize) -> &str {
    if s.width() <= max {
        return s;
    }
    s.unicode_truncate_start(max).0
}

/// Pad a string with spaces to `width` display columns.
pub fn display_pad_right(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - w))
    }
}

/// Human-readable size: `512B`, `4.0K`, `1.2M`, `3.4G`, etc. Shared by the
/// long-listing table, the file-type label, and the image-gallery popup —
/// a pure `u64 -> String`, so it lives with the renderers rather than in `fs`.
pub fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G", "T", "P"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes}B")
    } else if value >= 10.0 {
        format!("{value:.0}{}", UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_segmentation::UnicodeSegmentation;

    /// Clusters whose width is NOT the sum of their chars' widths — the whole
    /// reason these helpers advance by cluster rather than by char.
    const HEART: &str = "\u{2764}\u{FE0F}"; // ❤️ base + VS16: 2 cols, chars sum to 1
    const FAMILY: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"; // 👨‍👩‍👧: 2 cols, chars sum to 6
    const ACUTE: &str = "e\u{0301}"; // é base + combining mark: 1 col

    /// Byte offsets in `s` that sit on a grapheme-cluster boundary.
    fn cluster_bounds(s: &str) -> Vec<usize> {
        let mut v: Vec<usize> = s.grapheme_indices(true).map(|(i, _)| i).collect();
        v.push(s.len());
        v
    }

    /// `head` must be a whole-cluster prefix of `s`; `tail` a whole-cluster
    /// suffix. Catches a mid-cluster slice exactly, rather than approximately.
    fn is_cluster_prefix(s: &str, head: &str) -> bool {
        s.starts_with(head) && cluster_bounds(s).contains(&head.len())
    }
    fn is_cluster_suffix(s: &str, tail: &str) -> bool {
        s.ends_with(tail) && cluster_bounds(s).contains(&(s.len() - tail.len()))
    }

    #[test]
    fn a_variation_selector_cluster_is_measured_as_the_terminal_draws_it() {
        // ❤️ draws in 2 columns; its chars sum to 1. The old per-char walk let
        // BOTH hearts through a 2-column budget, overflowing the cell by 2.
        let s = HEART.repeat(2);
        assert_eq!(display_width(&s), 4);
        assert_eq!(display_truncate(&s, 2), HEART, "exactly one heart fits");
        assert_eq!(display_truncate_tail(&s, 2), HEART);
    }

    #[test]
    fn a_zwj_sequence_is_never_cut_into_fragments() {
        // The family emoji draws in 2 columns but its chars sum to 6. The old
        // walk sliced it after the first ZWJ, emitting a lone "👨\u{200d}".
        assert_eq!(display_width(FAMILY), 2);
        let s = FAMILY.repeat(2);
        assert_eq!(display_truncate(&s, 2), FAMILY);
        assert_eq!(display_truncate(&s, 3), FAMILY, "no room for a second");
        assert_eq!(display_truncate_tail(&s, 2), FAMILY);
    }

    #[test]
    fn truncation_holds_the_budget_and_cluster_boundary_at_every_width() {
        for unit in [HEART, FAMILY, ACUTE] {
            let s = unit.repeat(4);
            for max in 0..=display_width(&s) + 2 {
                let head = display_truncate(&s, max);
                let tail = display_truncate_tail(&s, max);
                assert!(
                    display_width(head) <= max,
                    "head {head:?} exceeds {max} cols"
                );
                assert!(
                    display_width(tail) <= max,
                    "tail {tail:?} exceeds {max} cols"
                );
                assert!(
                    is_cluster_prefix(&s, head),
                    "head split a cluster: {head:?}"
                );
                assert!(
                    is_cluster_suffix(&s, tail),
                    "tail split a cluster: {tail:?}"
                );
            }
        }
    }

    proptest::proptest! {
        /// The house-style property: truncation never splits a cluster and
        /// never exceeds the budget, over a mixed-width alphabet.
        #[test]
        fn truncate_respects_clusters_and_budget(
            s in proptest::string::string_regex(
                "(?:a|\u{2764}\u{FE0F}|\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}|e\u{0301}|\u{65E5}| ){0,10}"
            ).unwrap(),
            max in 0usize..14,
        ) {
            let head = display_truncate(&s, max);
            let tail = display_truncate_tail(&s, max);
            proptest::prop_assert!(display_width(head) <= max, "head {:?} > {}", head, max);
            proptest::prop_assert!(display_width(tail) <= max, "tail {:?} > {}", tail, max);
            proptest::prop_assert!(is_cluster_prefix(&s, head));
            proptest::prop_assert!(is_cluster_suffix(&s, tail));
        }
    }

    #[test]
    fn format_size_picks_appropriate_unit() {
        assert_eq!(format_size(0), "0B");
        assert_eq!(format_size(512), "512B");
        assert_eq!(format_size(1024), "1.0K");
        assert_eq!(format_size(1536), "1.5K");
        assert_eq!(format_size(10 * 1024), "10K");
        assert_eq!(format_size(1024 * 1024), "1.0M");
        assert_eq!(format_size(12 * 1024 * 1024), "12M");
    }

    #[test]
    fn ascii_width() {
        assert_eq!(display_width("hello"), 5);
    }

    #[test]
    fn cjk_width() {
        // Each CJK character is 2 columns wide.
        assert_eq!(display_width("日本語"), 6);
    }

    #[test]
    fn mixed_width() {
        assert_eq!(display_width("a日b"), 4); // 1 + 2 + 1
    }

    #[test]
    fn truncate_ascii() {
        assert_eq!(display_truncate("hello world", 5), "hello");
    }

    #[test]
    fn truncate_cjk_no_split() {
        // "日本語" is 6 cols. Truncating to 5 can't fit the 3rd char.
        assert_eq!(display_truncate("日本語", 5), "日本");
    }

    #[test]
    fn truncate_fits() {
        assert_eq!(display_truncate("abc", 10), "abc");
    }

    #[test]
    fn truncate_tail_ascii() {
        assert_eq!(display_truncate_tail("hello world", 5), "world");
    }

    #[test]
    fn truncate_tail_cjk_no_split() {
        // "日本語" is 6 cols; keeping 5 can only fit the last 2 (4 cols).
        assert_eq!(display_truncate_tail("日本語", 5), "本語");
    }

    #[test]
    fn truncate_tail_fits() {
        assert_eq!(display_truncate_tail("abc", 10), "abc");
    }

    #[test]
    fn truncate_tail_never_splits_codepoint() {
        // A byte-slice tail of width-1 on a multibyte path used to panic.
        let s = "/home/résumé/café";
        let out = display_truncate_tail(s, 6);
        assert!(s.ends_with(out));
        assert!(display_width(out) <= 6);
    }
}
