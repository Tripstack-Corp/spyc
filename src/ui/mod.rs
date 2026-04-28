//! Rendering. Layout decisions live in `layout`; individual widgets in
//! `list_view`, `status`, and `prompt`. Shared colors in `theme`.
//!
//! Width helpers: all UI code should use `display_width()` and
//! `display_truncate()` instead of `chars().count()` or `.len()`.

use unicode_width::UnicodeWidthStr;

pub mod help;
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
}
