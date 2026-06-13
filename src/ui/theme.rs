//! Runtime color theme.
//!
//! The default palette is inspired by Tokyo Night — tuned for dark terminal
//! backgrounds. We use true-color RGB values; ratatui will map them to the
//! closest 256-color on terminals that don't speak 24-bit.
//!
//! Users can override individual colors via `[colors]` in `.spycrc.toml`.
//! Invalid values are silently ignored so a bad rc file degrades rather than
//! crashing.

use ratatui::style::{Color, Modifier, Style};

use crate::config::ColorOverrides;

/// Runtime color theme. Every field corresponds to a named color that can be
/// overridden by the user's `.spycrc.toml` `[colors]` table.
#[derive(Debug, Clone)]
pub struct Theme {
    pub dir: Color,
    pub exec: Color,
    pub symlink: Color,
    pub file: Color,
    pub other: Color,
    pub cursor_bg: Color,
    pub cursor_bg_dim: Color,
    pub cursor_fg: Color,
    pub pick: Color,
    pub take: Color,
    pub status_user: Color,
    pub status_path: Color,
    pub status_suffix: Color,
    pub prompt_prefix: Color,
    pub empty_marker: Color,
    /// Background tint for rows that are about to be deleted by an
    /// active `RemoveConfirm` prompt. Stronger than the cursor /
    /// pick tints so it reads as "consequence" — the user can see
    /// exactly which files the next `y` keystroke will affect.
    /// Drawn over cursor / pick / mark styling.
    pub delete_warning: Color,
    /// Foreground for added (`+`) lines in the diff/show renderer (the
    /// gutter glyph + the unknown-language fallback text). Syntax-highlighted
    /// lines keep their language colors; only the row background is tinted.
    pub diff_add_fg: Color,
    /// Foreground for removed (`-`) lines in the diff/show renderer.
    pub diff_del_fg: Color,
    /// Row background tint behind added lines (overlaid on the syntect fg, so
    /// language colors survive). A dim wash — the brighter `diff_add_word_bg`
    /// marks the actually-changed tokens. Dropped in `mono`.
    pub diff_add_bg: Color,
    /// Row background tint behind removed lines. Dropped in `mono`.
    pub diff_del_bg: Color,
    /// Brighter background for the *changed tokens* within a modified added
    /// line (word-level highlight over the dim `diff_add_bg` wash). Dropped in
    /// `mono`.
    pub diff_add_word_bg: Color,
    /// Brighter background for the changed tokens within a modified removed
    /// line. Dropped in `mono`.
    pub diff_del_word_bg: Color,
    /// Foreground for hunk headers (`@@ -a,b +c,d @@`).
    pub diff_hunk_fg: Color,
    /// Foreground for per-file headers (the `status  path` line).
    pub diff_file_fg: Color,
    /// Foreground for low-emphasis metadata (binary/submodule notes, the
    /// `show` commit header's Author/Date labels, truncation banner).
    pub diff_meta_fg: Color,
    /// When true, all colors fall back to terminal defaults. Used by the
    /// `C` (colortoggle) action — the cursor row falls back to reverse
    /// video so the selection is still visible.
    pub mono: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            dir: Color::Rgb(0x7a, 0xa2, 0xf7),           // soft blue
            exec: Color::Rgb(0x9e, 0xce, 0x6a),          // soft green
            symlink: Color::Rgb(0xbb, 0x9a, 0xf7),       // lavender
            file: Color::Rgb(0xc0, 0xca, 0xf5),          // near-white
            other: Color::Rgb(0x54, 0x5c, 0x7e),         // dim slate
            cursor_bg: Color::Rgb(0xb8, 0x5c, 0x4a),     // warm terracotta
            cursor_bg_dim: Color::Rgb(0x3b, 0x40, 0x61), // muted indigo (unfocused)
            cursor_fg: Color::Rgb(0xff, 0xff, 0xff),     // pure white
            pick: Color::Rgb(0xe0, 0xaf, 0x68),          // amber
            take: Color::Rgb(0x73, 0xda, 0xca),          // teal
            status_user: Color::Rgb(0xbb, 0x9a, 0xf7),   // lavender
            status_path: Color::Rgb(0xc0, 0xca, 0xf5),
            status_suffix: Color::Rgb(0x56, 0x5f, 0x89),
            prompt_prefix: Color::Rgb(0xe0, 0xaf, 0x68),
            empty_marker: Color::Rgb(0x56, 0x5f, 0x89),
            delete_warning: Color::Rgb(0x80, 0x1e, 0x1e), // deep crimson
            diff_add_fg: Color::Rgb(0x9e, 0xce, 0x6a),    // green (matches exec)
            diff_del_fg: Color::Rgb(0xf7, 0x76, 0x8e),    // soft red
            diff_add_bg: Color::Rgb(0x12, 0x26, 0x19),    // dim green line wash
            diff_del_bg: Color::Rgb(0x2a, 0x16, 0x1b),    // dim red line wash
            diff_add_word_bg: Color::Rgb(0x2b, 0x60, 0x3a), // bright green changed-token
            diff_del_word_bg: Color::Rgb(0x6b, 0x27, 0x33), // bright red changed-token
            diff_hunk_fg: Color::Rgb(0x7d, 0xcf, 0xff),   // cyan
            diff_file_fg: Color::Rgb(0x7a, 0xa2, 0xf7),   // soft blue
            diff_meta_fg: Color::Rgb(0x56, 0x5f, 0x89),   // dim slate
            mono: false,
        }
    }
}

impl Theme {
    /// Return a copy with `mono` flipped — flipping between the colored
    /// palette and a terminal-defaults look.
    pub fn toggled(&self) -> Self {
        let mut copy = self.clone();
        copy.mono = !self.mono;
        copy
    }

    /// Apply user overrides from the config. Invalid color strings are
    /// silently ignored (the default stays in place).
    pub fn with_overrides(mut self, overrides: &ColorOverrides) -> Self {
        macro_rules! apply {
            ($field:ident) => {
                if let Some(ref s) = overrides.$field {
                    if let Some(c) = parse_color(s) {
                        self.$field = c;
                    }
                }
            };
        }
        apply!(dir);
        apply!(exec);
        apply!(symlink);
        apply!(file);
        apply!(other);
        apply!(cursor_bg);
        apply!(cursor_fg);
        apply!(pick);
        apply!(take);
        apply!(status_user);
        apply!(status_path);
        apply!(status_suffix);
        apply!(prompt_prefix);
        apply!(delete_warning);
        apply!(diff_add_fg);
        apply!(diff_del_fg);
        apply!(diff_add_bg);
        apply!(diff_del_bg);
        apply!(diff_add_word_bg);
        apply!(diff_del_word_bg);
        apply!(diff_hunk_fg);
        apply!(diff_file_fg);
        apply!(diff_meta_fg);
        self
    }

    pub fn dir_style(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.dir).add_modifier(Modifier::BOLD)
        }
    }

    pub fn exec_style(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.exec).add_modifier(Modifier::BOLD)
        }
    }

    pub fn symlink_style(&self) -> Style {
        if self.mono {
            Style::default()
        } else {
            Style::default().fg(self.symlink)
        }
    }

    pub fn file_style(&self) -> Style {
        if self.mono {
            Style::default()
        } else {
            Style::default().fg(self.file)
        }
    }

    pub fn other_style(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            Style::default().fg(self.other)
        }
    }

    pub fn pick_style(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.pick).add_modifier(Modifier::BOLD)
        }
    }

    pub fn take_style(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.take).add_modifier(Modifier::BOLD)
        }
    }
}

/// Diff / show / blame renderer styles. In their own impl block grouping the
/// diff-view color logic; consumed by the `diff_render` and `blame_render`
/// modules (driven into the pager by the git-view session).
/// All mono-aware: in `mono` the +/- distinction is carried by the gutter
/// glyph + BOLD rather than color, and row backgrounds are dropped (honoring
/// the `C` colortoggle).
impl Theme {
    /// Style for a per-file header line (`status  path`).
    pub fn diff_file_style(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(self.diff_file_fg)
                .add_modifier(Modifier::BOLD)
        }
    }

    /// Style for a hunk header (`@@ -a,b +c,d @@`).
    pub fn diff_hunk_style(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            Style::default().fg(self.diff_hunk_fg)
        }
    }

    /// Style for low-emphasis metadata (binary/submodule notes, the `show`
    /// commit header's Author/Date labels, the truncation banner).
    pub fn diff_meta_style(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            Style::default().fg(self.diff_meta_fg)
        }
    }

    /// Style for a diff that *failed* to build (a [`DiffKind::Error`] line) —
    /// the soft-red removal color plus bold so a broken diff stands out from
    /// the dim metadata of a binary/submodule note. Bold-only in `mono`.
    ///
    /// [`DiffKind::Error`]: crate::git::model::DiffKind::Error
    pub fn diff_error_style(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(self.diff_del_fg)
                .add_modifier(Modifier::BOLD)
        }
    }

    /// Gutter-glyph style for an added (`is_add`) or removed line. The glyph
    /// carries the meaning in `mono`; in color it also gets the +/- color.
    pub fn diff_gutter_style(&self, is_add: bool) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(if is_add {
                self.diff_add_fg
            } else {
                self.diff_del_fg
            })
        }
    }

    /// Row-background tint for an added (`is_add`) or removed line, or `None`
    /// in `mono` (where the gutter glyph carries the distinction).
    pub const fn diff_row_bg(&self, is_add: bool) -> Option<Color> {
        if self.mono {
            None
        } else if is_add {
            Some(self.diff_add_bg)
        } else {
            Some(self.diff_del_bg)
        }
    }

    /// Brighter background for the changed tokens within a modified added
    /// (`is_add`) or removed line — the word-level highlight over the dim
    /// [`Self::diff_row_bg`] wash. `None` in `mono` (no word highlight).
    pub const fn diff_word_bg(&self, is_add: bool) -> Option<Color> {
        if self.mono {
            None
        } else if is_add {
            Some(self.diff_add_word_bg)
        } else {
            Some(self.diff_del_word_bg)
        }
    }

    /// Fallback text style for a +/- line when syntax highlighting is
    /// unavailable (unknown language). Context falls back to the terminal
    /// default (`Style::default()`).
    pub fn diff_text_style(&self, is_add: bool) -> Style {
        if self.mono {
            Style::default()
        } else {
            Style::default().fg(if is_add {
                self.diff_add_fg
            } else {
                self.diff_del_fg
            })
        }
    }
}

/// Parse a color string. Accepts:
/// - Hex `#rrggbb` (6 hex digits) -> `Color::Rgb`
/// - Short hex `#rgb` (3 hex digits) -> expand each digit, e.g. `#abc` = `#aabbcc`
/// - Named colors (case-insensitive): `black`, `red`, `green`, `yellow`,
///   `blue`, `magenta`/`purple`, `cyan`, `white`, `gray`/`grey`/`dark_gray`,
///   and `light_*` versions.
pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    parse_named(&s.to_ascii_lowercase())
}

fn parse_hex(hex: &str) -> Option<Color> {
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::Rgb(r, g, b))
        }
        3 => {
            let chars: Vec<u8> = hex
                .chars()
                .map(|c| {
                    let mut buf = [0u8; 4];
                    let s = c.encode_utf8(&mut buf);
                    u8::from_str_radix(s, 16).ok()
                })
                .collect::<Option<Vec<_>>>()?;
            Some(Color::Rgb(chars[0] * 17, chars[1] * 17, chars[2] * 17))
        }
        _ => None,
    }
}

fn parse_named(name: &str) -> Option<Color> {
    Some(match name {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" | "purple" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" | "dark_gray" | "dark_grey" => Color::DarkGray,
        "light_red" => Color::LightRed,
        "light_green" => Color::LightGreen,
        "light_yellow" => Color::LightYellow,
        "light_blue" => Color::LightBlue,
        "light_magenta" | "light_purple" => Color::LightMagenta,
        "light_cyan" => Color::LightCyan,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_long() {
        assert_eq!(parse_color("#ff8800"), Some(Color::Rgb(0xff, 0x88, 0x00)));
    }

    #[test]
    fn hex_short() {
        // #abc -> #aabbcc
        assert_eq!(parse_color("#abc"), Some(Color::Rgb(0xaa, 0xbb, 0xcc)));
    }

    #[test]
    fn hex_short_digits() {
        // #123 -> #112233
        assert_eq!(parse_color("#123"), Some(Color::Rgb(0x11, 0x22, 0x33)));
    }

    #[test]
    fn named_colors() {
        assert_eq!(parse_color("red"), Some(Color::Red));
        assert_eq!(parse_color("magenta"), Some(Color::Magenta));
        assert_eq!(parse_color("purple"), Some(Color::Magenta));
        assert_eq!(parse_color("gray"), Some(Color::DarkGray));
        assert_eq!(parse_color("grey"), Some(Color::DarkGray));
        assert_eq!(parse_color("dark_gray"), Some(Color::DarkGray));
        assert_eq!(parse_color("light_red"), Some(Color::LightRed));
        assert_eq!(parse_color("light_cyan"), Some(Color::LightCyan));
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(parse_color("RED"), Some(Color::Red));
        assert_eq!(parse_color("Light_Blue"), Some(Color::LightBlue));
        assert_eq!(parse_color("#AABBCC"), Some(Color::Rgb(0xaa, 0xbb, 0xcc)));
    }

    #[test]
    fn unknown_returns_none() {
        assert_eq!(parse_color("chartreuse"), None);
        assert_eq!(parse_color("#zzzzzz"), None);
        assert_eq!(parse_color("#12"), None);
        assert_eq!(parse_color(""), None);
    }

    #[test]
    fn with_overrides_applies_valid_and_ignores_invalid() {
        let overrides = ColorOverrides {
            dir: Some("#ff0000".to_string()),
            exec: Some("not_a_color".to_string()),
            ..Default::default()
        };
        let theme = Theme::default().with_overrides(&overrides);
        assert_eq!(theme.dir, Color::Rgb(0xff, 0x00, 0x00));
        // exec should remain at default because the override was invalid.
        assert_eq!(theme.exec, Theme::default().exec);
    }
}
