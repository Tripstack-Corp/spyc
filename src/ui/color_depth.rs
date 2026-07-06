//! Truecolor → 256-color auto-degrade.
//!
//! spyc's theme (and syntect highlighting, diffs, the spice gradient, and ANSI
//! passthrough in the pager) emit 24-bit `Color::Rgb`, which crossterm writes as
//! `\x1b[38;2;r;g;bm`. Terminals that can't parse that SGR — notably macOS's
//! bundled GNU screen 4.00.03 (Oct 2006), frozen at pre-GPLv3 and truecolor-blind
//! — drop the whole attribute, so every color, background, and highlight vanishes.
//!
//! The fix is a single choke point: after the pure render pass fills the frame
//! buffer, [`downgrade_buffer`] rewrites every `Rgb` cell to the nearest xterm
//! 256-color index when the resolved [`ColorDepth`] isn't `TrueColor`. Because it
//! runs on the finished buffer it catches *all* color sources, not just the theme.
//! `TrueColor` is a no-op, so capable terminals pay nothing.

use ratatui::buffer::Buffer;
use ratatui::style::Color;

use crate::config::ColorMode;

/// The concrete color depth spyc renders at — resolved once at startup from the
/// [`ColorMode`] preference (CLI > config > `$COLORTERM` auto-detect).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    /// 24-bit RGB — emit `Color::Rgb` unchanged (the native path).
    TrueColor,
    /// xterm 256-color — quantize every `Color::Rgb` to a `Color::Indexed`.
    Ansi256,
}

/// Resolve the effective depth. Precedence: an explicit `cli` (`--color`) wins,
/// else the config `[layout] color_depth`, else — for `Auto`:
///
/// - **inside GNU screen** (`in_screen`) → 256, *ignoring* `$COLORTERM`. Screen
///   inherits `COLORTERM=truecolor` from the outer terminal but does not itself
///   render 24-bit SGR (the ancient macOS 4.00.03 can't; 5.x needs `truecolor on`
///   and is off by default), so the claim is a lie. Its 256-color support is
///   solid, so 256 is the reliable default. Explicit `--color truecolor` still
///   forces it for a screen configured to pass RGB through.
/// - otherwise → truecolor when `$COLORTERM` advertises `truecolor`/`24bit`, else
///   256. This mirrors the de-facto `COLORTERM` convention.
pub fn resolve(
    cli: ColorMode,
    config: ColorMode,
    colorterm: Option<&str>,
    in_screen: bool,
) -> ColorDepth {
    let mode = if cli == ColorMode::Auto { config } else { cli };
    match mode {
        ColorMode::TrueColor => ColorDepth::TrueColor,
        ColorMode::Ansi256 => ColorDepth::Ansi256,
        ColorMode::Auto => {
            let truecolor = !in_screen
                && colorterm.is_some_and(|c| c.contains("truecolor") || c.contains("24bit"));
            if truecolor {
                ColorDepth::TrueColor
            } else {
                ColorDepth::Ansi256
            }
        }
    }
}

/// Rewrite every truecolor cell in `buf` to the resolved depth in place.
/// No-op for `TrueColor`. Runs once per frame on the diffed buffer (only changed
/// cells are present), after the pure `&self` render pass.
pub fn downgrade_buffer(buf: &mut Buffer, depth: ColorDepth) {
    if depth == ColorDepth::TrueColor {
        return;
    }
    for cell in &mut buf.content {
        cell.fg = downgrade(cell.fg, depth);
        cell.bg = downgrade(cell.bg, depth);
        cell.underline_color = downgrade(cell.underline_color, depth);
    }
}

/// Map one color to the target depth. Only `Rgb` is touched; named / indexed
/// colors already render everywhere and pass through untouched.
pub fn downgrade(color: Color, depth: ColorDepth) -> Color {
    match (depth, color) {
        (ColorDepth::Ansi256, Color::Rgb(r, g, b)) => Color::Indexed(rgb_to_ansi256(r, g, b)),
        _ => color,
    }
}

/// The six component levels of the xterm 6×6×6 color cube (indices 16..=231).
const CUBE_STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Nearest xterm 256-color index for an RGB triple. Considers both the 6×6×6
/// color cube and the 24-step grayscale ramp (232..=255) and picks whichever is
/// closer by squared Euclidean distance — so neutral grays land on the gray ramp
/// (finer than the cube) and saturated colors land in the cube.
pub fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    // Color-cube candidate: snap each channel to its nearest cube level.
    let ri = nearest_cube_level(r);
    let gi = nearest_cube_level(g);
    let bi = nearest_cube_level(b);
    let cube_idx = 16 + 36 * ri + 6 * gi + bi;
    let cube_rgb = (CUBE_STEPS[ri], CUBE_STEPS[gi], CUBE_STEPS[bi]);

    // Grayscale-ramp candidate: value 8 + 10*n for n in 0..=23 (232..=255).
    let avg = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let n = (avg.saturating_sub(8) + 5) / 10; // round to nearest ramp step
    let n = n.min(23);
    let gray_val = (8 + 10 * n) as u8;
    let gray_idx = 232 + n as usize;

    if dist2((r, g, b), cube_rgb) <= dist2((r, g, b), (gray_val, gray_val, gray_val)) {
        cube_idx as u8
    } else {
        gray_idx as u8
    }
}

/// Index (0..=5) of the cube level nearest to `v`.
fn nearest_cube_level(v: u8) -> usize {
    let mut best = 0;
    let mut best_dist = u16::MAX;
    for (i, &step) in CUBE_STEPS.iter().enumerate() {
        let d = u16::from(v.abs_diff(step));
        if d < best_dist {
            best_dist = d;
            best = i;
        }
    }
    best
}

/// Squared Euclidean distance between two RGB triples.
fn dist2(a: (u8, u8, u8), b: (u8, u8, u8)) -> i32 {
    let dr = i32::from(a.0) - i32::from(b.0);
    let dg = i32::from(a.1) - i32::from(b.1);
    let db = i32::from(a.2) - i32::from(b.2);
    dr * dr + dg * dg + db * db
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_precedence_cli_beats_config_beats_env() {
        // CLI wins over everything.
        assert_eq!(
            resolve(
                ColorMode::Ansi256,
                ColorMode::TrueColor,
                Some("truecolor"),
                false
            ),
            ColorDepth::Ansi256
        );
        // CLI Auto → fall back to config.
        assert_eq!(
            resolve(ColorMode::Auto, ColorMode::TrueColor, None, false),
            ColorDepth::TrueColor
        );
        // Both Auto → env decides.
        assert_eq!(
            resolve(ColorMode::Auto, ColorMode::Auto, Some("truecolor"), false),
            ColorDepth::TrueColor
        );
        assert_eq!(
            resolve(ColorMode::Auto, ColorMode::Auto, Some("24bit"), false),
            ColorDepth::TrueColor
        );
        // Auto with no/other COLORTERM → 256.
        assert_eq!(
            resolve(ColorMode::Auto, ColorMode::Auto, None, false),
            ColorDepth::Ansi256
        );
        assert_eq!(
            resolve(ColorMode::Auto, ColorMode::Auto, Some(""), false),
            ColorDepth::Ansi256
        );
    }

    #[test]
    fn resolve_ignores_lying_colorterm_inside_screen() {
        // Screen inherits COLORTERM=truecolor from the outer terminal but can't
        // render 24-bit SGR → auto must drop to 256 despite the claim.
        assert_eq!(
            resolve(ColorMode::Auto, ColorMode::Auto, Some("truecolor"), true),
            ColorDepth::Ansi256
        );
        // But an explicit request is still honored (screen configured for RGB).
        assert_eq!(
            resolve(
                ColorMode::TrueColor,
                ColorMode::Auto,
                Some("truecolor"),
                true
            ),
            ColorDepth::TrueColor
        );
        assert_eq!(
            resolve(ColorMode::Auto, ColorMode::TrueColor, None, true),
            ColorDepth::TrueColor
        );
    }

    #[test]
    fn ansi256_maps_only_rgb() {
        // Pure black / white land on the cube endpoints.
        assert_eq!(rgb_to_ansi256(0, 0, 0), 16);
        assert_eq!(rgb_to_ansi256(255, 255, 255), 231);
        // Mid gray prefers the finer grayscale ramp over the cube.
        assert_eq!(rgb_to_ansi256(128, 128, 128), 244);
        // A saturated color lands in the cube (16..=231), not the gray ramp.
        let idx = rgb_to_ansi256(0x7a, 0xa2, 0xf7);
        assert!((16..=231).contains(&idx), "expected cube index, got {idx}");
    }

    #[test]
    fn downgrade_passthrough_and_quantize() {
        // TrueColor is always a no-op.
        let rgb = Color::Rgb(10, 20, 30);
        assert_eq!(downgrade(rgb, ColorDepth::TrueColor), rgb);
        // Ansi256 quantizes Rgb but leaves named / indexed / reset alone.
        assert!(matches!(
            downgrade(rgb, ColorDepth::Ansi256),
            Color::Indexed(_)
        ));
        assert_eq!(downgrade(Color::Red, ColorDepth::Ansi256), Color::Red);
        assert_eq!(
            downgrade(Color::Indexed(42), ColorDepth::Ansi256),
            Color::Indexed(42)
        );
        assert_eq!(downgrade(Color::Reset, ColorDepth::Ansi256), Color::Reset);
    }

    #[test]
    fn downgrade_buffer_rewrites_rgb_cells() {
        use ratatui::layout::Rect;
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        buf.content[0].fg = Color::Rgb(0x7a, 0xa2, 0xf7);
        buf.content[0].bg = Color::Rgb(0x1a, 0x1b, 0x26);
        buf.content[1].fg = Color::Red; // named — untouched

        // No-op path leaves everything as-is.
        let mut same = buf.clone();
        downgrade_buffer(&mut same, ColorDepth::TrueColor);
        assert_eq!(same.content[0].fg, Color::Rgb(0x7a, 0xa2, 0xf7));

        downgrade_buffer(&mut buf, ColorDepth::Ansi256);
        assert!(matches!(buf.content[0].fg, Color::Indexed(_)));
        assert!(matches!(buf.content[0].bg, Color::Indexed(_)));
        assert_eq!(buf.content[1].fg, Color::Red);
    }
}
