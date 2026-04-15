//! Default color palette. Centralized so a future `.cspyrc` `color`
//! directive has a single place to override.
//!
//! The values are inspired by the Tokyo Night palette — tuned for dark
//! terminal backgrounds. We use true-color RGB values; ratatui will map
//! them to the closest 256-color on terminals that don't speak 24-bit.

use ratatui::style::{Color, Modifier, Style};

// ---- Palette ---------------------------------------------------------------

// Accents for entry kinds.
pub const DIR: Color = Color::Rgb(0x7a, 0xa2, 0xf7); // soft blue
pub const EXEC: Color = Color::Rgb(0x9e, 0xce, 0x6a); // soft green
pub const SYMLINK: Color = Color::Rgb(0xbb, 0x9a, 0xf7); // lavender
pub const FILE: Color = Color::Rgb(0xc0, 0xca, 0xf5); // near-white
pub const OTHER: Color = Color::Rgb(0x54, 0x5c, 0x7e); // dim slate

// Cursor + state markers. The cursor bar is intentionally warm and bright:
// it's the one thing in the UI we want you to see from across the room,
// and a warm hue pops cleanly against the cool blues/greens that dominate
// the rest of the palette.
pub const CURSOR_BG: Color = Color::Rgb(0xb8, 0x5c, 0x4a); // warm terracotta
pub const CURSOR_FG: Color = Color::Rgb(0xff, 0xff, 0xff); // pure white for max legibility
pub const PICK: Color = Color::Rgb(0xe0, 0xaf, 0x68); // amber
pub const TAKE: Color = Color::Rgb(0x73, 0xda, 0xca); // teal

// Chrome: status bar, prompt, hints.
pub const STATUS_USER: Color = Color::Rgb(0xbb, 0x9a, 0xf7); // lavender
pub const STATUS_PATH: Color = Color::Rgb(0xc0, 0xca, 0xf5);
pub const STATUS_SUFFIX: Color = Color::Rgb(0x56, 0x5f, 0x89);
pub const PROMPT_PREFIX: Color = Color::Rgb(0xe0, 0xaf, 0x68);
pub const EMPTY_MARKER: Color = Color::Rgb(0x56, 0x5f, 0x89);

// ---- Helpers ---------------------------------------------------------------

pub fn dir_style() -> Style {
    Style::default().fg(DIR).add_modifier(Modifier::BOLD)
}

pub fn exec_style() -> Style {
    Style::default().fg(EXEC).add_modifier(Modifier::BOLD)
}

pub fn symlink_style() -> Style {
    Style::default().fg(SYMLINK)
}

pub fn file_style() -> Style {
    Style::default().fg(FILE)
}

pub fn other_style() -> Style {
    Style::default().fg(OTHER)
}

pub fn pick_style() -> Style {
    Style::default().fg(PICK).add_modifier(Modifier::BOLD)
}

pub fn take_style() -> Style {
    Style::default().fg(TAKE).add_modifier(Modifier::BOLD)
}
