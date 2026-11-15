//! The loader a git view shows while it builds: a large pepper drawn in half
//! blocks, a band of heat running from its stem to its tip, steam rising
//! above it, and under it what the build is doing and how far it has got.
//!
//! Pure: a [`Loader`] and the body's size → centred lines for the pager. The
//! frame comes from the loop's cooking clock (`app::cooking`), so the wave
//! advances only while the build runs.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::theme::{SPICE_HEAT, Theme};

/// What the loader says this frame.
pub struct Loader<'a> {
    /// The cooking clock's frame.
    pub frame: u64,
    /// What is being built, e.g. `building the diff`.
    pub heading: &'a str,
    /// What the build is doing now, e.g. `highlighting 312 / 1,204 files`.
    pub detail: &'a str,
    /// How much of the build is done, 0 to 1, when it can tell.
    pub done: Option<f64>,
    /// How long the build has run, e.g. `2.4s`.
    pub elapsed: &'a str,
}

/// The pepper, two pixels to a row: `G` its stem, `g` the stem's light side,
/// `R` its body, `H` the shine on it.
const PEPPER: [&str; 16] = [
    "......................GG..",
    ".....................GG...",
    "..................GGGGGg..",
    "...............GGgRRRRRGg.",
    ".............RRRRRRRRRRRR.",
    "...........RRRHHHHRRRRRRR.",
    "..........RRHHRRRRRRRRRRR.",
    ".........RRHRRRRRRRRRRRR..",
    "........RRRRRRRRRRRRRRRR..",
    ".......RRRRRRRRRRRRRRR....",
    "......RRRRRRRRRRRRRR......",
    ".....RRRRRRRRRRRRR........",
    "...RRRRRRRRRRRR...........",
    ".RRRRRRRRRR...............",
    "RRRRRRR...................",
    ".RRR......................",
];

/// Columns the pepper takes.
const PEPPER_W: usize = PEPPER[0].len();

/// Frames for the heat to run the pepper's length and start again: 2 s at
/// the cooking clock's 100 ms.
const WAVE_FRAMES: u64 = 20;

/// How far the band of heat reaches either side of its centre, along the
/// pepper's 0-to-1 length.
const WAVE_REACH: f64 = 0.22;

/// The progress bar's cells.
const BAR_W: usize = 24;

/// The steam's two rows: a wisp rises through them over [`STEAM_FRAMES`].
const STEAM_ROWS: usize = 2;
const STEAM_FRAMES: u64 = 6;

/// The columns a wisp rises from, over the pepper's shoulder.
const WISPS: [usize; 3] = [11, 15, 19];

const STEM: (u8, u8, u8) = (0x3A, 0x7D, 0x2A);
const STEM_LIGHT: (u8, u8, u8) = (0x6B, 0xB3, 0x3C);
const SHINE: (u8, u8, u8) = (0xF2, 0x6B, 0x5A);

/// The loader as the pager's lines, centred in a `width` × `height` body.
pub fn render(
    loader: &Loader<'_>,
    width: usize,
    height: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut art = steam(loader.frame, theme);
    art.extend(pepper(loader.frame, theme));
    let mut text = vec![
        Line::default(),
        Line::from(Span::styled(
            loader.heading.to_string(),
            Style::default().fg(theme.file).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            loader.detail.to_string(),
            Style::default().fg(theme.file),
        )),
    ];
    if let Some(done) = loader.done {
        text.push(bar(done, theme));
    }
    text.push(Line::from(Span::styled(
        format!("{} \u{b7} Esc cancels", loader.elapsed),
        Style::default().fg(theme.status_suffix),
    )));
    let art_pad = width.saturating_sub(PEPPER_W) / 2;
    let mut lines: Vec<Line<'static>> = art.into_iter().map(|l| indent(l, art_pad)).collect();
    lines.extend(text.into_iter().map(|l| {
        let pad = width.saturating_sub(l.width()) / 2;
        indent(l, pad)
    }));
    let top = height.saturating_sub(lines.len()) / 2;
    let mut centred = vec![Line::default(); top];
    centred.extend(lines);
    centred
}

/// `line` moved `pad` columns right.
fn indent(line: Line<'static>, pad: usize) -> Line<'static> {
    let mut spans = vec![Span::raw(" ".repeat(pad))];
    spans.extend(line.spans);
    Line::from(spans)
}

/// The pepper in half blocks, each body pixel heated by the wave.
fn pepper(frame: u64, theme: &Theme) -> Vec<Line<'static>> {
    let rows: Vec<&[u8]> = PEPPER.iter().map(|r| r.as_bytes()).collect();
    rows.chunks(2)
        .enumerate()
        .map(|(pair, rows)| {
            let spans = (0..PEPPER_W)
                .map(|x| {
                    let top = pixel(rows[0][x], x, pair * 2, frame, theme);
                    let bottom = rows
                        .get(1)
                        .and_then(|row| pixel(row[x], x, pair * 2 + 1, frame, theme));
                    half_block(top, bottom, theme.mono)
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

/// One cell from its top and bottom pixels' colours.
fn half_block(top: Option<Color>, bottom: Option<Color>, mono: bool) -> Span<'static> {
    match (top, bottom) {
        (None, None) => Span::raw(" "),
        (Some(_), Some(_)) if mono => Span::raw("\u{2588}"),
        (Some(top), Some(bottom)) => Span::styled("\u{2580}", Style::default().fg(top).bg(bottom)),
        (Some(top), None) => Span::styled("\u{2580}", tint(top, mono)),
        (None, Some(bottom)) => Span::styled("\u{2584}", tint(bottom, mono)),
    }
}

fn tint(colour: Color, mono: bool) -> Style {
    if mono {
        Style::default()
    } else {
        Style::default().fg(colour)
    }
}

/// The colour of the pixel `kind` at `x`, `y` this frame; `None` off the pepper.
fn pixel(kind: u8, x: usize, y: usize, frame: u64, theme: &Theme) -> Option<Color> {
    let base = match kind {
        b'G' => return Some(rgb(STEM)),
        b'g' => return Some(rgb(STEM_LIGHT)),
        b'R' => SPICE_HEAT[0],
        b'H' => SHINE,
        _ => return None,
    };
    if theme.mono {
        return Some(theme.file);
    }
    let heat = wave(along(x, y), frame);
    let spark = SPICE_HEAT[3];
    Some(rgb((
        mix(base.0, spark.0, heat),
        mix(base.1, spark.1, heat),
        mix(base.2, spark.2, heat),
    )))
}

/// How far along the pepper `x`, `y` lies: 0 at the stem, top right, to 1 at
/// the tip, bottom left.
fn along(x: usize, y: usize) -> f64 {
    let span = (PEPPER_W - 1 + PEPPER.len() - 1) as f64;
    ((PEPPER_W - 1 - x) + y) as f64 / span
}

/// The heat at `t` along the pepper this frame, 0 to 1: a band that enters
/// past the stem and leaves past the tip, easing in and out.
fn wave(t: f64, frame: u64) -> f64 {
    let near = 1.0 - ((t - wave_centre(frame)).abs() / WAVE_REACH).min(1.0);
    near * near * 2.0f64.mul_add(-near, 3.0)
}

/// Where the band of heat is centred this frame: from just before the stem
/// (`-WAVE_REACH`) round to just past the tip (`1 + WAVE_REACH`).
fn wave_centre(frame: u64) -> f64 {
    let phase = (frame % WAVE_FRAMES) as f64 / WAVE_FRAMES as f64;
    phase.mul_add(2.0f64.mul_add(WAVE_REACH, 1.0), -WAVE_REACH)
}

/// `from` toward `to` by `by`, 0 to 1.
fn mix(from: u8, to: u8, by: f64) -> u8 {
    (f64::from(to) - f64::from(from))
        .mul_add(by, f64::from(from))
        .round()
        .clamp(0.0, 255.0) as u8
}

const fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(r, g, b)
}

/// Wisps of steam rising over the pepper's shoulder, each a frame apart.
fn steam(frame: u64, theme: &Theme) -> Vec<Line<'static>> {
    const RISE: [char; STEAM_FRAMES as usize] = [' ', '\u{b7}', '\u{2d9}', '\u{b0}', ' ', ' '];
    let style = Style::default().fg(theme.status_suffix);
    (0..STEAM_ROWS)
        .map(|row| {
            let mut cells = vec![' '; PEPPER_W];
            for (i, &col) in WISPS.iter().enumerate() {
                // A wisp in the lower row now is in the upper row a frame on.
                let at = frame + (i as u64) * 2 + (STEAM_ROWS - 1 - row) as u64;
                cells[col] = RISE[(at % STEAM_FRAMES) as usize];
            }
            Line::from(Span::styled(cells.into_iter().collect::<String>(), style))
        })
        .collect()
}

/// A bar of [`BAR_W`] cells, `done` of it filled with the pepper's heat.
fn bar(done: f64, theme: &Theme) -> Line<'static> {
    let filled = ((done.clamp(0.0, 1.0) * BAR_W as f64).round() as usize).min(BAR_W);
    let (r, g, b) = SPICE_HEAT[2];
    let fill = if theme.mono {
        Style::default()
    } else {
        Style::default().fg(Color::Rgb(r, g, b))
    };
    Line::from(vec![
        Span::styled("\u{25b0}".repeat(filled), fill),
        Span::styled(
            "\u{25b1}".repeat(BAR_W - filled),
            Style::default().fg(theme.status_suffix),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loader(frame: u64, done: Option<f64>) -> Loader<'static> {
        Loader {
            frame,
            heading: "building the diff",
            detail: "highlighting 3 / 12 files",
            done,
            elapsed: "2.4s",
        }
    }

    fn text(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The loader says what is building, how far it has got and how to stop
    /// it, centred in the body.
    #[test]
    fn the_loader_says_what_builds_and_how_far_centred() {
        let theme = Theme::default();
        let lines = render(&loader(0, Some(0.25)), 100, 40, &theme);
        let all = text(&lines);
        for said in [
            "building the diff",
            "highlighting 3 / 12 files",
            "2.4s",
            "Esc cancels",
        ] {
            assert!(all.contains(said), "{said:?} in {all}");
        }
        assert!(
            all.contains(&"\u{25b0}".repeat(6)),
            "a quarter of the bar: {all}"
        );
        assert!(!all.contains(&"\u{25b0}".repeat(7)), "{all}");
        let top = lines.iter().take_while(|l| l.width() == 0).count();
        let bottom = 40 - lines.len();
        assert!(top.abs_diff(bottom) <= 1, "top {top}, bottom {bottom}");
        for line in &lines {
            assert!(line.width() <= 100, "{line:?}");
        }
        let heading = lines
            .iter()
            .find(|l| text(std::slice::from_ref(l)).contains("building"))
            .unwrap();
        let pad = heading.spans[0].content.len();
        assert_eq!(pad, (100 - "building the diff".len()) / 2);
    }

    /// Without a measure of progress, no bar.
    #[test]
    fn no_bar_until_the_build_can_tell() {
        let all = text(&render(&loader(0, None), 80, 30, &Theme::default()));
        assert!(!all.contains('\u{25b1}'), "{all}");
    }

    /// The heat moves: the same pepper pixel takes another colour a few
    /// frames on, and comes back once the wave has gone round.
    #[test]
    fn the_heat_runs_along_the_pepper() {
        let theme = Theme::default();
        let colours = |frame| -> Vec<Option<Color>> {
            render(&loader(frame, None), 80, 30, &theme)
                .iter()
                .flat_map(|l| l.spans.iter().map(|s| s.style.fg))
                .collect()
        };
        assert_ne!(colours(0), colours(5));
        assert_eq!(colours(3), colours(3 + WAVE_FRAMES));
    }

    #[test]
    fn the_wave_peaks_at_its_centre_and_fades_out_of_reach() {
        let frame = WAVE_FRAMES / 2;
        let centre = wave_centre(frame);
        assert!((centre - 0.5).abs() < 1e-9, "half way round, mid-pepper");
        assert!((wave(centre, frame) - 1.0).abs() < 1e-9);
        assert!(wave(centre + WAVE_REACH, frame).abs() < 1e-9);
        assert!(wave(0.0, frame) < wave(centre - 0.05, frame));
        assert!((0.0..=1.0).contains(&along(0, 15)));
        assert!(along(PEPPER_W - 1, 0).abs() < 1e-9, "the stem end is 0");
    }

    /// Mono draws the pepper's shape uncoloured.
    #[test]
    fn mono_draws_the_shape_without_colour() {
        let theme = Theme {
            mono: true,
            ..Theme::default()
        };
        let lines = render(&loader(4, Some(0.5)), 80, 30, &theme);
        let all = text(&lines);
        assert!(all.contains('\u{2588}'), "{all}");
        for span in lines.iter().flat_map(|l| l.spans.iter()) {
            assert!(
                !matches!(span.style.bg, Some(Color::Rgb(..))),
                "mono paints no backgrounds: {span:?}"
            );
        }
    }
}
