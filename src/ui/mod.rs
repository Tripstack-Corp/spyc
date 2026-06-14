//! Rendering. Layout decisions live in `layout`; individual widgets in
//! `list_view`, `status`, and `prompt`. Shared colors in `theme`.
//!
//! Width helpers: all UI code should use `display_width()` and
//! `display_truncate()` instead of `chars().count()` or `.len()`.

use unicode_width::UnicodeWidthStr;

pub mod blame_render;
pub mod diff_render;
pub mod help;
pub mod hex;
pub mod json;
#[allow(
    dead_code,
    clippy::unnested_or_patterns,
    clippy::missing_const_for_fn,
    clippy::match_same_arms
)]
pub mod line_edit;
pub mod list_view;
pub mod markdown;
pub mod pager;
pub mod prompt;
pub mod scrollback;
pub mod status;
pub mod syntax;
pub mod theme;

/// Display width of a string in terminal columns. CJK characters and
/// some emoji count as 2 columns; most Latin/symbol characters as 1.
pub fn display_width(s: &str) -> usize {
    s.width()
}

/// Truncate a string to at most `max` display columns. Returns the
/// truncated slice (no allocation when the string already fits).
pub fn display_truncate(s: &str, max: usize) -> &str {
    if s.width() <= max {
        return s;
    }
    let mut width = 0;
    for (i, c) in s.char_indices() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if width + cw > max {
            return &s[..i];
        }
        width += cw;
    }
    s
}

/// Truncate a string to at most `max` display columns, keeping the
/// **tail** (rightmost columns) — the mirror of [`display_truncate`].
/// Walks characters from the end on `char` boundaries, so a multi-byte
/// path is never sliced mid-codepoint. Used for path displays that elide
/// the head (`…/deep/leaf`).
pub fn display_truncate_tail(s: &str, max: usize) -> &str {
    if s.width() <= max {
        return s;
    }
    let mut width = 0;
    for (i, c) in s.char_indices().rev() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if width + cw > max {
            // `i` is the byte offset of the char that would overflow;
            // the kept tail starts at the next char boundary.
            return &s[i + c.len_utf8()..];
        }
        width += cw;
    }
    s
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

#[cfg(test)]
mod tests {
    use super::*;

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
