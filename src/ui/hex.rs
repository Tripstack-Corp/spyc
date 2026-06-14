//! Hex-dump view: style a file's leading bytes into pager `Line`s.
//!
//! The byte read lives in the `fs` layer ([`crate::fs::ops::read_hex_window`]);
//! this module owns the *presentation* — theme colors and `ratatui` spans —
//! so those UI dependencies stay out of `fs`. `fs` reads bytes, `ui` paints
//! them.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::fs::ops::{HEX_CAP, read_hex_window};
use crate::ui::theme::Theme;

/// Read up to `HEX_CAP` bytes from `path` and format as a hex dump.
/// Returns styled lines ready for the pager: each line split into
/// offset (dim), hex bytes (default), and ASCII sidebar (warm).
pub fn hex_dump_lines(
    path: &std::path::Path,
    theme: &Theme,
) -> std::io::Result<Vec<Line<'static>>> {
    use pretty_hex::{HexConfig, config_hex};

    let (buf, truncated) = read_hex_window(path, HEX_CAP)?;

    let hex_str = config_hex(
        &buf,
        HexConfig {
            title: false,
            width: 16,
            group: 0,
            ..HexConfig::default()
        },
    );

    let offset_style = Style::default()
        .fg(theme.status_suffix)
        .add_modifier(Modifier::DIM);
    let hex_style = Style::default().fg(theme.file);
    let ascii_style = Style::default().fg(theme.pick).add_modifier(Modifier::DIM);
    let sep_style = Style::default().fg(theme.status_suffix);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for text_line in hex_str.lines() {
        // pretty-hex lines look like:
        //   00000000: 7f 45 4c 46 02 01 01 00 00 00 00 00 00 00 00 00  .ELF............
        // Split into offset (before ':'), hex (middle), ascii (after '  ').
        if let Some(colon) = text_line.find(':') {
            let offset_part = &text_line[..colon];
            let rest = &text_line[colon + 1..];
            // The ASCII sidebar is the last segment after "  " (double space).
            let (hex_part, ascii_part) = if let Some(sep) = rest.rfind("  ") {
                (&rest[..sep], rest[sep + 2..].trim())
            } else {
                (rest, "")
            };
            lines.push(Line::from(vec![
                Span::styled(offset_part.to_string(), offset_style),
                Span::styled(":", sep_style),
                Span::styled(hex_part.to_string(), hex_style),
                Span::styled("  ", sep_style),
                Span::styled(ascii_part.to_string(), ascii_style),
            ]));
        } else {
            // Fallback: unstyled.
            lines.push(Line::from(text_line.to_string()));
        }
    }

    if truncated {
        lines.push(Line::from(Span::styled(
            format!("... truncated at {HEX_CAP} bytes"),
            offset_style,
        )));
    }

    Ok(lines)
}
