//! Tests for the diff/show renderers (`super::render_diff` / `render_show`).
//! Split out of `diff_render.rs` verbatim during the 800-LoC decomposition.

use super::{DiffLayout, TEST_TAB_WIDTH, render_diff, render_show};
use crate::git::model::{
    CommitMeta, DiffKind, DiffLine, DiffModel, FileDiff, FileStatus, Hunk, LineOrigin,
};
use crate::ui::theme::Theme;
use ratatui::text::Line;

fn ctx(text: &str) -> DiffLine {
    DiffLine {
        origin: LineOrigin::Context,
        text: text.to_string(),
    }
}
fn add(text: &str) -> DiffLine {
    DiffLine {
        origin: LineOrigin::Add,
        text: text.to_string(),
    }
}
fn rem(text: &str) -> DiffLine {
    DiffLine {
        origin: LineOrigin::Remove,
        text: text.to_string(),
    }
}

/// The substrings a set of highlight ranges selects, for readable assertions.
fn picks<'a>(text: &'a str, ranges: &[std::ops::Range<usize>]) -> Vec<&'a str> {
    ranges.iter().map(|r| &text[r.start..r.end]).collect()
}

/// A one-file modify diff (`c` → `C`) with surrounding context, in `f.txt`.
fn modify_model() -> DiffModel {
    DiffModel {
        files: vec![FileDiff {
            old_path: Some("f.txt".into()),
            new_path: Some("f.txt".into()),
            status: FileStatus::Modified,
            lang_hint: "txt".into(),
            kind: DiffKind::Text(vec![Hunk {
                old_start: 1,
                old_lines: 5,
                new_start: 1,
                new_lines: 5,
                lines: vec![ctx("a"), ctx("b"), rem("c"), add("C"), ctx("d"), ctx("e")],
            }]),
        }],
        truncated: false,
    }
}

/// Flatten styled lines to their glyph text (the layout/structure view).
/// One added file (`added.txt`, two all-add lines).
fn added_model() -> DiffModel {
    DiffModel {
        files: vec![FileDiff {
            old_path: None,
            new_path: Some("added.txt".into()),
            status: FileStatus::Added,
            lang_hint: "txt".into(),
            kind: DiffKind::Text(vec![Hunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 2,
                lines: vec![add("new1"), add("new2")],
            }]),
        }],
        truncated: false,
    }
}

/// A one-file modify diff whose hunk sits at 5-digit line numbers
/// (`12340…`), to exercise the dynamic side-by-side line-number field.
fn big_lnum_model() -> DiffModel {
    single_file(
        FileStatus::Modified,
        DiffKind::Text(vec![Hunk {
            old_start: 12_340,
            old_lines: 4,
            new_start: 12_340,
            new_lines: 4,
            lines: vec![ctx("a"), ctx("b"), rem("c"), add("C"), ctx("d")],
        }]),
        Some("f.txt"),
        Some("f.txt"),
    )
}

fn single_file(
    status: FileStatus,
    kind: DiffKind,
    old: Option<&str>,
    new: Option<&str>,
) -> DiffModel {
    DiffModel {
        files: vec![FileDiff {
            old_path: old.map(Into::into),
            new_path: new.map(Into::into),
            status,
            lang_hint: String::new(),
            kind,
        }],
        truncated: false,
    }
}

/// Flatten styled lines to their glyph text (the layout/structure view),
/// trailing whitespace trimmed per line.
fn text(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The concatenated glyph text of one rendered line.
fn row_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn unified_modify_glyph_and_colors() {
    let theme = Theme::default();
    let out = render_diff(&modify_model(), &theme, DiffLayout::Unified, 80);
    assert_eq!(
        text(&out),
        "modified   f.txt\n@@ -1,5 +1,5 @@\n a\n b\n-c\n+C\n d\n e"
    );
    // Row 4 is `-c`, row 5 is `+C`; their gutter + content carry the tint.
    assert_eq!(out[4].spans[0].content.as_ref(), "-");
    assert_eq!(out[4].spans[0].style.bg, Some(theme.diff_del_bg));
    assert_eq!(
        out[4].spans.last().unwrap().style.bg,
        Some(theme.diff_del_bg)
    );
    assert_eq!(out[5].spans[0].content.as_ref(), "+");
    assert_eq!(out[5].spans[0].style.bg, Some(theme.diff_add_bg));
    // Context rows are untinted.
    assert_eq!(out[2].spans[0].style.bg, None);
}

#[test]
fn split_modify_layout_and_colors() {
    let theme = Theme::default();
    let out = render_diff(&modify_model(), &theme, DiffLayout::SideBySide, 80);
    // header + hunk header + 5 rows: ctx a, ctx b, the paired `-c`/`+C`
    // change row (removes pair with adds side-by-side, unlike unified's two
    // separate rows), ctx d, ctx e.
    assert_eq!(out.len(), 7);
    // Every data row has the column separator.
    for row in &out[2..] {
        assert!(
            row_text(row).contains('│'),
            "row missing separator: {row:?}"
        );
    }
    // The change row pairs `-c` (left) with `+C` (right).
    let change = &out[4];
    let joined = row_text(change);
    assert!(
        joined.contains("-c") && joined.contains("+C"),
        "got: {joined}"
    );
    // Left gutter marker is `-` with the remove tint…
    assert_eq!(change.spans[1].content.as_ref(), "-");
    assert_eq!(change.spans[1].style.bg, Some(theme.diff_del_bg));
    // …and the right gutter marker (two spans past the separator) is `+`.
    let sep = change
        .spans
        .iter()
        .position(|s| s.content.contains('│'))
        .unwrap();
    assert_eq!(change.spans[sep + 2].content.as_ref(), "+");
    assert_eq!(change.spans[sep + 2].style.bg, Some(theme.diff_add_bg));
}

#[test]
fn mono_drops_backgrounds_keeps_markers() {
    let theme = Theme::default().toggled(); // mono = true
    assert!(theme.mono);
    let out = render_diff(&modify_model(), &theme, DiffLayout::Unified, 80);
    // Glyphs (and so the +/- markers) are unchanged…
    assert_eq!(
        text(&out),
        "modified   f.txt\n@@ -1,5 +1,5 @@\n a\n b\n-c\n+C\n d\n e"
    );
    // …but the row backgrounds are gone.
    assert_eq!(out[4].spans[0].style.bg, None);
    assert_eq!(out[5].spans[0].style.bg, None);
}

#[test]
fn added_file_is_all_adds() {
    let theme = Theme::default();
    let out = render_diff(&added_model(), &theme, DiffLayout::Unified, 80);
    assert_eq!(
        text(&out),
        "added      added.txt\n@@ -0,0 +1,2 @@\n+new1\n+new2"
    );
}

#[test]
fn unknown_language_falls_back_to_plus_minus_color() {
    let theme = Theme::default();
    // `.xyzzy` isn't a syntect-known extension → flat fallback styling.
    let model = single_file(
        FileStatus::Modified,
        DiffKind::Text(vec![Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: vec![rem("old"), add("new")],
        }]),
        Some("f.xyzzy"),
        Some("f.xyzzy"),
    );
    let out = render_diff(&model, &theme, DiffLayout::Unified, 80);
    // The `+new` content span uses the add foreground (no syntax colors).
    let add_row = out.iter().find(|l| row_text(l).starts_with('+')).unwrap();
    assert_eq!(
        add_row.spans.last().unwrap().style.fg,
        Some(theme.diff_add_fg)
    );
}

#[test]
fn binary_file_line() {
    let theme = Theme::default();
    let model = single_file(
        FileStatus::Modified,
        DiffKind::Binary,
        Some("b.bin"),
        Some("b.bin"),
    );
    let out = render_diff(&model, &theme, DiffLayout::Unified, 80);
    assert_eq!(text(&out), "modified   b.bin\nBinary file differs.");
}

#[test]
fn submodule_line_rendered() {
    let theme = Theme::default();
    let model = single_file(
        FileStatus::Modified,
        DiffKind::Submodule {
            old: "1111111aaa".into(),
            new: "2222222bbb".into(),
        },
        Some("dep"),
        Some("dep"),
    );
    let out = render_diff(&model, &theme, DiffLayout::Unified, 80);
    assert_eq!(text(&out), "modified   dep\nSubmodule 1111111 → 2222222");
}

#[test]
fn error_diff_renders_explicit_message_not_clean_file() {
    // A failed diff (e.g. a resource that couldn't be loaded) must NOT look
    // like an unchanged file — it gets an explicit "diff unavailable" line in
    // both layouts, styled with the error (red) style so it stands out.
    let theme = Theme::default();
    let model = single_file(
        FileStatus::Modified,
        DiffKind::Error("object 0badc0de missing".into()),
        Some("f.rs"),
        Some("f.rs"),
    );
    for layout in [DiffLayout::Unified, DiffLayout::SideBySide] {
        let out = render_diff(&model, &theme, layout, 80);
        let rendered = text(&out);
        assert!(
            rendered.contains("diff unavailable: object 0badc0de missing"),
            "{layout:?} layout missing error line; got {rendered:?}"
        );
        let err_row = out
            .iter()
            .find(|l| row_text(l).contains("diff unavailable"))
            .unwrap();
        assert_eq!(
            err_row.style.fg,
            Some(theme.diff_del_fg),
            "error line should use the error (red) style in {layout:?}"
        );
    }
}

#[test]
fn rename_header_shows_similarity() {
    let theme = Theme::default();
    let model = single_file(
        FileStatus::Renamed { similarity: 87 },
        DiffKind::Text(Vec::new()),
        Some("old.rs"),
        Some("new.rs"),
    );
    let out = render_diff(&model, &theme, DiffLayout::Unified, 80);
    assert_eq!(text(&out), "renamed    old.rs → new.rs (87%)");
}

#[test]
fn truncated_appends_banner() {
    let theme = Theme::default();
    let mut model = added_model();
    model.truncated = true;
    let rendered = text(&render_diff(&model, &theme, DiffLayout::Unified, 80));
    assert!(rendered.ends_with("… diff truncated (too large to display in full) …"));
}

#[test]
fn empty_model_says_no_changes() {
    let theme = Theme::default();
    let out = render_diff(&DiffModel::default(), &theme, DiffLayout::Unified, 80);
    assert_eq!(text(&out), "No changes.");
}

#[test]
fn side_by_side_rows_never_exceed_width() {
    // The pager must not wrap side-by-side rows — so every rendered row's
    // display width must be ≤ the width it was rendered for. (A row wider
    // than the pager body wraps, and the wrapped padding tail shows as a
    // stray tinted bar — the bug this guards against.)
    //
    // Measured POST tab-expansion, on the flattened row. Summing per span over
    // the raw text measures neither what the user sees (the pager expands tabs
    // to `tab_width` first) nor the width the layout budgeted against, and it
    // splits grapheme clusters at span boundaries — so it would pass while the
    // drawn row overran.
    let theme = Theme::default();
    for width in [40usize, 60, 80, 81, 100, 137] {
        let out = render_diff(&modify_model(), &theme, DiffLayout::SideBySide, width);
        for line in &out {
            let flat: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            let expanded = flat.replace('\t', &" ".repeat(TEST_TAB_WIDTH));
            let w = crate::ui::display_width(&expanded);
            assert!(w <= width, "row width {w} exceeds {width}: {line:?}");
        }
    }
}

#[test]
fn side_by_side_five_digit_line_numbers_stay_aligned() {
    // A file with 5-digit line numbers must not overflow the gutter and
    // shove the separator / right column out of alignment. (The bug: the
    // fixed 4-wide LNUM_W field made a 5-digit number's cell one column too
    // wide, so the row exceeded `width` and wrapped — exactly what the
    // ≤-width assertion below catches.)
    let theme = Theme::default();
    for width in [40usize, 60, 80, 100] {
        let out = render_diff(&big_lnum_model(), &theme, DiffLayout::SideBySide, width);
        for line in &out {
            let w: usize = line
                .spans
                .iter()
                .map(|s| crate::ui::display_width(s.content.as_ref()))
                .sum();
            assert!(
                w <= width,
                "5-digit row width {w} exceeds {width}: {line:?}"
            );
        }
        // The full 5-digit numbers must survive — not be truncated to fit a
        // 4-wide field.
        let body = text(&out);
        assert!(
            body.contains("12340") && body.contains("12343"),
            "5-digit line numbers missing/truncated at width {width}:\n{body}"
        );
    }
}

#[test]
fn lnum_width_floors_at_four_and_grows_with_digits() {
    use crate::git::model::{DiffLine, Hunk, LineOrigin};
    let hunk = |start: u32| Hunk {
        old_start: start,
        old_lines: 1,
        new_start: start,
        new_lines: 1,
        lines: vec![DiffLine {
            origin: LineOrigin::Context,
            text: "x".into(),
        }],
    };
    // Small numbers keep the stable 4-wide gutter…
    assert_eq!(super::lnum_width(&[hunk(1)]), 4);
    assert_eq!(super::lnum_width(&[hunk(9999)]), 4);
    // …5- and 6-digit numbers widen to fit.
    assert_eq!(super::lnum_width(&[hunk(10_000)]), 5);
    assert_eq!(super::lnum_width(&[hunk(123_456)]), 6);
}

#[test]
fn intra_change_ranges_marks_only_the_changed_token() {
    // Only the digit differs; "let x = " and ";" are shared.
    let (old_r, new_r) = super::intraline::intra_change_ranges("let x = 1;", "let x = 2;");
    assert_eq!(picks("let x = 1;", &old_r), vec!["1"]);
    assert_eq!(picks("let x = 2;", &new_r), vec!["2"]);
}

#[test]
fn intra_change_ranges_pure_insertion_is_empty_on_old_side() {
    // "a b" → "a X b": nothing removed, so the old side carries no highlight.
    // (Was "ab" → "aXb"; with word tokens that pair is a single changed token
    // spanning both whole lines, which is the deliberate no-highlight case.)
    let (old_r, new_r) = super::intraline::intra_change_ranges("a b", "a X b");
    assert!(old_r.is_empty());
    assert_eq!(picks("a X b", &new_r), vec!["X"]);
}

#[test]
fn intra_change_ranges_empty_when_identical_or_wholly_different() {
    assert_eq!(
        super::intraline::intra_change_ranges("same", "same"),
        (vec![], vec![])
    );
    // Nothing shared at either end → uniform wash, no word highlight.
    assert_eq!(
        super::intraline::intra_change_ranges("abc", "xyz"),
        (vec![], vec![])
    );
}

/// The #179 class, asserted through the real renderer: an unchanged token
/// between two changed ones must not carry the word-highlight bg.
#[test]
fn word_highlight_does_not_bleed_across_an_unchanged_token() {
    let theme = Theme::default();
    let model = single_file(
        FileStatus::Modified,
        DiffKind::Text(vec![Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: vec![rem("foo(a, b, c)"), add("foo(x, b, y)")],
        }]),
        Some("f.rs"),
        Some("f.rs"),
    );
    let out = render_diff(&model, &theme, DiffLayout::Unified, 80);
    for (prefix, bg, expect) in [
        ('-', theme.diff_del_word_bg, vec!["a", "c"]),
        ('+', theme.diff_add_word_bg, vec!["x", "y"]),
    ] {
        let row = out
            .iter()
            .find(|l| row_text(l).starts_with(prefix))
            .expect("row present");
        let marked: Vec<&str> = row
            .spans
            .iter()
            .filter(|s| s.style.bg == Some(bg))
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(marked, expect, "{prefix} row highlights each changed token");
        // The shared `b` keeps the dim row wash, never the bright word bg.
        assert!(
            !marked.iter().any(|s| s.contains('b')),
            "{prefix} row bled the unchanged `b` into the highlight"
        );
    }
}

#[test]
fn word_highlight_brightens_only_the_changed_token() {
    let theme = Theme::default();
    let model = single_file(
        FileStatus::Modified,
        DiffKind::Text(vec![Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: vec![rem("let x = 1;"), add("let x = 2;")],
        }]),
        Some("f.rs"),
        Some("f.rs"),
    );
    let out = render_diff(&model, &theme, DiffLayout::Unified, 80);
    let add_row = out.iter().find(|l| row_text(l).starts_with('+')).unwrap();
    let rem_row = out.iter().find(|l| row_text(l).starts_with('-')).unwrap();
    // The changed token carries the bright word bg…
    let add_word = add_row
        .spans
        .iter()
        .find(|s| s.style.bg == Some(theme.diff_add_word_bg))
        .expect("add row highlights the changed token");
    assert_eq!(add_word.content.as_ref(), "2");
    let rem_word = rem_row
        .spans
        .iter()
        .find(|s| s.style.bg == Some(theme.diff_del_word_bg))
        .expect("remove row highlights the changed token");
    assert_eq!(rem_word.content.as_ref(), "1");
    // …while the unchanged part keeps the dim wash.
    assert!(
        add_row
            .spans
            .iter()
            .any(|s| s.style.bg == Some(theme.diff_add_bg))
    );
}

#[test]
fn show_renders_commit_header_then_diff() {
    let theme = Theme::default();
    let meta = CommitMeta {
        id: "a".repeat(40),
        short_id: "aaaaaaa".into(),
        author: "Ada".into(),
        email: "ada@example.com".into(),
        time: "2026-06-06 10:00:00 -04:00".into(),
        subject: "tweak c".into(),
        body: "body line one\nbody line two".into(),
    };
    let out = render_show(&meta, &modify_model(), &theme, DiffLayout::Unified, 80);
    let rendered = text(&out);
    assert!(rendered.starts_with(&format!("commit {}", "a".repeat(40))));
    assert!(rendered.contains("Author: Ada <ada@example.com>"));
    assert!(rendered.contains("Date:   2026-06-06 10:00:00 -04:00"));
    assert!(rendered.contains("\n    tweak c\n"));
    assert!(rendered.contains("\n    body line one\n    body line two\n"));
    // The diff body follows.
    assert!(rendered.contains("@@ -1,5 +1,5 @@"));
}

#[test]
fn side_by_side_long_lines_wrap_not_truncate() {
    // A line longer than the column content width must wrap into extra visual
    // rows rather than being silently clipped. The full text must survive in
    // the rendered output.
    let long = "a".repeat(200);
    let theme = Theme::default();
    let model = single_file(
        FileStatus::Modified,
        DiffKind::Text(vec![Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: vec![rem(&long), add(&long)],
        }]),
        Some("f.txt"),
        Some("f.txt"),
    );
    let width = 80usize;
    let out = render_diff(&model, &theme, DiffLayout::SideBySide, width);

    // Every row still fits within `width`.
    for line in &out {
        let w: usize = line
            .spans
            .iter()
            .map(|s| crate::ui::display_width(s.content.as_ref()))
            .sum();
        assert!(
            w <= width,
            "wrapped row width {w} exceeds {width}: {line:?}"
        );
    }

    // The full content must appear across the wrapped rows (not truncated).
    let all_text: String = out
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.content.as_ref())
        .collect();
    let count = all_text.matches('a').count();
    // Both remove and add sides carry 200 'a's — expect ≥ 400.
    assert!(
        count >= 400,
        "expected ≥400 'a' chars but got {count}; content was truncated"
    );

    // More than one visual row must have been emitted per diff line.
    // header + hunk-header + at least 2 content rows = > 3 total.
    assert!(
        out.len() > 3,
        "expected wrapped output but got only {} rows",
        out.len()
    );
}

#[test]
fn wrap_spans_splits_at_width_boundary() {
    use ratatui::text::Span;
    let spans = vec![Span::raw("hello world")];
    // Width 5: "hello" | " worl" | "d"
    let rows = super::wrap_spans(&spans, 5, 4);
    assert_eq!(rows.len(), 3, "expected 3 rows, got {rows:?}");
    assert_eq!(rows[0][0].content.as_ref(), "hello");
    assert_eq!(rows[1][0].content.as_ref(), " worl");
    assert_eq!(rows[2][0].content.as_ref(), "d");
}

/// Encode styled lines as one debug string per line — glyphs plus every span's
/// fg/bg — so a cache mismatch in colors (not just glyphs) is caught by a plain
/// string compare.
fn styled_fingerprint(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| format!("{:?}|{:?}|{:?}", s.content, s.style.fg, s.style.bg))
                .collect::<Vec<_>>()
                .join("⟂")
        })
        .collect()
}

#[test]
fn cached_highlight_render_matches_inline_render() {
    // `render_diff_highlighted` with a precomputed highlight must be byte-for-
    // byte identical to `render_diff` (which highlights inline) — the resize/
    // toggle cache is a pure optimization, not a behavior change. Exercise a
    // syntect-known language (.rs) in both layouts so the highlight cache is
    // actually populated and flows through identically.
    let theme = Theme::default();
    let model = single_file(
        FileStatus::Modified,
        DiffKind::Text(vec![Hunk {
            old_start: 1,
            old_lines: 2,
            new_start: 1,
            new_lines: 2,
            lines: vec![
                ctx("fn main() {"),
                rem("    let x = 1;"),
                add("    let x = 2;"),
                ctx("}"),
            ],
        }]),
        Some("f.rs"),
        Some("f.rs"),
    );
    let hl = super::highlight_diff(&model);
    for layout in [DiffLayout::Unified, DiffLayout::SideBySide] {
        for width in [40usize, 80, 137] {
            let inline = render_diff(&model, &theme, layout, width);
            let cached = super::render_diff_highlighted(&model, &hl, &theme, layout, width, 4);
            assert_eq!(
                styled_fingerprint(&inline),
                styled_fingerprint(&cached),
                "cached render diverged from inline at {layout:?} width {width}"
            );
        }
    }
}

#[test]
fn cached_highlight_relayout_reflows_at_new_width() {
    // The whole point of the cache: re-lay-out the SAME highlight at a new
    // width and the side-by-side rows must reflow (column width changes), not
    // stay frozen at the first width.
    let theme = Theme::default();
    let model = single_file(
        FileStatus::Modified,
        DiffKind::Text(vec![Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: vec![rem(&"x".repeat(120)), add(&"y".repeat(120))],
        }]),
        Some("f.txt"),
        Some("f.txt"),
    );
    let hl = super::highlight_diff(&model);
    let widest = |lines: &[Line]| {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| crate::ui::display_width(s.content.as_ref()))
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(0)
    };
    let narrow = super::render_diff_highlighted(&model, &hl, &theme, DiffLayout::SideBySide, 80, 4);
    let wide = super::render_diff_highlighted(&model, &hl, &theme, DiffLayout::SideBySide, 160, 4);
    // Rows are sized to the body width, so a wider render produces wider rows…
    assert!(
        widest(&wide) > widest(&narrow),
        "wider re-layout must widen rows: {} → {}",
        widest(&narrow),
        widest(&wide)
    );
    // …and a 120-col line fits in fewer wrapped rows when the column is wider.
    assert!(
        wide.len() < narrow.len(),
        "wider re-layout must wrap into fewer rows: {} → {}",
        narrow.len(),
        wide.len()
    );
}

// ── side-by-side column alignment ──────────────────────────────────

/// Separator column of every row that has one, measured as the user finally
/// sees it — i.e. after the pager has expanded tabs to `tab_width`
/// (`pager::render::expand_tabs`, which runs on this renderer's output).
fn separator_columns(lines: &[Line], tab_width: usize) -> Vec<(usize, usize, String)> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let flat: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            let expanded = flat.replace('\t', &" ".repeat(tab_width));
            let byte_idx = expanded.find('│')?;
            Some((i, crate::ui::display_width(&expanded[..byte_idx]), expanded))
        })
        .collect()
}

/// Every side-by-side row must put `│` at the SAME display column. That's the
/// entire contract of the layout — a separator that wanders line to line makes
/// the two columns un-scannable, and the diff reads as untrustworthy even when
/// the pairing underneath is correct.
///
/// Tab-indented source is what broke it, and it's not niche: gofmt uses tabs,
/// so does every Makefile and a lot of C. `unicode_width` scores `\t` as ONE
/// column, but the pager expands it to `tab_width` before drawing — so a cell
/// padded on the raw measurement rendered `tab_width - 1` columns too wide per
/// tab, shoving the separator right by an amount that varied with each line's
/// indent depth. Note the assertion is on the POST-expansion column: the raw
/// text is now deliberately narrower per tab, which is what makes it land flush.
#[test]
fn split_separator_column_is_identical_on_every_row() {
    let tabbed = DiffKind::Text(vec![Hunk {
        old_start: 1,
        old_lines: 8,
        new_start: 1,
        new_lines: 8,
        lines: vec![
            ctx("func f() {"),
            ctx("\tif ok {"),
            ctx("\t\tfor i := range xs {"),
            rem("\t\t\tkn.CPU = q.String()"),
            add("\t\t\tkn.CPUMilli = q.MilliValue()"),
            ctx("\t\t}"),
            ctx("\t}"),
            ctx("}"),
        ],
    }]);
    let model = single_file(FileStatus::Modified, tabbed, Some("k8s.go"), Some("k8s.go"));

    // Every tab_width a user can configure, at two widths — the bug scaled with
    // tab_width, so a fix that only works for the default isn't a fix.
    for tab_width in [1, 2, 4, 8] {
        for width in [100, 120] {
            let out = super::render_diff_tw(
                &model,
                &Theme::default(),
                DiffLayout::SideBySide,
                width,
                tab_width,
            );
            let cols = separator_columns(&out, tab_width);
            // 6 context rows + one paired remove|add row, plus any wrap
            // continuation rows (a large tab_width can push a line over the
            // column width) — those carry a separator too and must align just
            // the same, so they're deliberately included.
            assert!(
                cols.len() >= 7,
                "expected >=7 split rows, got {}",
                cols.len()
            );
            let first = cols[0].1;
            let bad: Vec<_> = cols.iter().filter(|(_, c, _)| *c != first).collect();
            assert!(
                bad.is_empty(),
                "tab_width={tab_width} width={width}: separator wanders — rows {:?} differ from column {first}.\n{}",
                bad.iter().map(|(i, c, _)| (*i, *c)).collect::<Vec<_>>(),
                cols.iter()
                    .map(|(i, c, f)| format!("  row {i:>2} sep@{c:>3}  {f:?}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }
}

/// The separator must also hold for content whose grapheme clusters are wider
/// than the sum of their chars. An emoji-presentation sequence (base char +
/// U+FE0F) is the common case: `unicode_width` scores the cluster as 2 columns —
/// which is what the terminal draws — while a per-`char` walk sees 1 + 0.
///
/// The wrap scan and the cell padding must therefore measure the SAME way. When
/// they disagree the padding believes the row is full, skips the pad, and the
/// cell runs over its budget — the separator shifts right by one column per
/// sequence, which is the same ragged-column symptom tab handling produced.
/// Lines are long enough to pack the column, since a short row pads either way.
#[test]
fn split_separator_holds_for_clusters_wider_than_their_chars() {
    // `⚠️` / `✔️` are base-char + VS16. The trailing text pushes each line up
    // against the column budget so the padding branch is actually exercised.
    let emoji = DiffKind::Text(vec![Hunk {
        old_start: 1,
        old_lines: 4,
        new_start: 1,
        new_lines: 4,
        lines: vec![
            ctx("// ⚠️ the checked path below is load-bearing, do not reorder it"),
            rem("let status = \"✔️ ok\"; // ⚠️ verified against the upstream fixture"),
            add("let status = \"✔️ done\"; // ⚠️ verified against the upstream table"),
            ctx("// ⚠️ ✔️ ⚠️ ✔️ trailing cluster run to fill out the column budget"),
        ],
    }]);
    let model = single_file(FileStatus::Modified, emoji, Some("a.rs"), Some("a.rs"));

    for width in [80, 100, 120] {
        let out =
            super::render_diff_tw(&model, &Theme::default(), DiffLayout::SideBySide, width, 4);
        let cols = separator_columns(&out, 4);
        assert!(
            cols.len() >= 3,
            "expected >=3 split rows, got {}",
            cols.len()
        );
        let first = cols[0].1;
        let bad: Vec<_> = cols.iter().filter(|(_, c, _)| *c != first).collect();
        assert!(
            bad.is_empty(),
            "width={width}: separator wanders on cluster-wide content — rows {:?} differ from column {first}.\n{}",
            bad.iter().map(|(i, c, _)| (*i, *c)).collect::<Vec<_>>(),
            cols.iter()
                .map(|(i, c, f)| format!("  row {i:>2} sep@{c:>3}  {f:?}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

// ── word-diff highlight clamping (#179) ─────────────────────────────────

#[test]
fn word_highlight_never_bleeds_into_same_row_padding() {
    // A changed word ending mid-line, with a fully-blank cell right after it
    // on the SAME (unwrapped) row — the padding must carry only the row
    // wash, never the word-highlight bg.
    let theme = Theme::default();
    let model = single_file(
        FileStatus::Modified,
        DiffKind::Text(vec![Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: vec![rem("foo bar"), add("foo baz")],
        }]),
        Some("f.txt"),
        Some("f.txt"),
    );
    let out = render_diff(&model, &theme, DiffLayout::SideBySide, 40);
    let change = out
        .iter()
        .find(|l| row_text(l).contains("baz"))
        .expect("paired change row");
    // Scope to the right (add) side only — the left (remove) side has its
    // own, differently-colored padding right next to it across the `│`.
    let sep_idx = change
        .spans
        .iter()
        .position(|s| s.content.contains('│'))
        .unwrap();
    let right = &change.spans[sep_idx + 1..];
    // Word-level tokens mark the whole changed word (`bar`→`baz`), the way
    // git's `--word-diff` does, not just the one differing char.
    let word = right
        .iter()
        .find(|s| s.content.as_ref() == "baz")
        .expect("changed word carries its own span");
    assert_eq!(word.style.bg, Some(theme.diff_add_word_bg));
    let pad = right
        .iter()
        .find(|s| s.content.len() > 1 && s.content.chars().all(|c| c == ' '))
        .expect("trailing padding span on the same row");
    assert_eq!(pad.style.bg, Some(theme.diff_add_bg));
}

#[test]
fn word_highlight_never_reaches_a_textless_continuation_row() {
    // The remove side wraps to more visual rows than its paired (much
    // shorter, unrelated) add side. The add side's leftover continuation row
    // carries no text at all — it must never carry the word-highlight bg,
    // only the add row's own wash (the actual fix here: it used to render
    // fully unstyled — see PR body for why that, not a literal bleed, is
    // what this file's code produced).
    let theme = Theme::default();
    let model = single_file(
        FileStatus::Modified,
        DiffKind::Text(vec![Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: vec![
                rem("and this second line carries far more words than its short replacement below"),
                add("short tail now"),
            ],
        }]),
        Some("f.txt"),
        Some("f.txt"),
    );
    let out = render_diff(&model, &theme, DiffLayout::SideBySide, 40);
    let mut found_textless_continuation = false;
    for line in &out {
        let Some(sep_idx) = line.spans.iter().position(|s| s.content.contains('│')) else {
            continue;
        };
        let right = &line.spans[sep_idx + 1..];
        if right.is_empty() || !right.iter().all(|s| s.content.chars().all(|c| c == ' ')) {
            continue;
        }
        found_textless_continuation = true;
        for s in right {
            assert_ne!(
                s.style.bg,
                Some(theme.diff_add_word_bg),
                "textless continuation row must never carry the word-highlight bg: {right:?}"
            );
            assert_eq!(
                s.style.bg,
                Some(theme.diff_add_bg),
                "textless continuation row should carry the add row's own wash: {right:?}"
            );
        }
    }
    assert!(
        found_textless_continuation,
        "expected the short add side to leave a textless continuation row"
    );
}

#[test]
fn word_highlight_unchanged_for_a_simple_single_row_split_change() {
    // Regression: the ordinary case — no wrap, no pair-count mismatch — must
    // render identically to before this fix. `modify_model()`'s `c`→`C` is a
    // wholly-changed single token (`intra_change_ranges` yields nothing for it,
    // no word highlight at all), so use a pair with a real word-level range.
    let theme = Theme::default();
    let model = single_file(
        FileStatus::Modified,
        DiffKind::Text(vec![Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: vec![rem("let x = 1;"), add("let x = 2;")],
        }]),
        Some("f.rs"),
        Some("f.rs"),
    );
    let out = render_diff(&model, &theme, DiffLayout::SideBySide, 80);
    let change = out
        .iter()
        .find(|l| row_text(l).contains("let x ="))
        .expect("paired change row");
    let sep_idx = change
        .spans
        .iter()
        .position(|s| s.content.contains('│'))
        .unwrap();
    let left_word = change.spans[..sep_idx]
        .iter()
        .find(|s| s.style.bg == Some(theme.diff_del_word_bg))
        .expect("left side still highlights its changed char");
    assert_eq!(left_word.content.as_ref(), "1");
    let right_word = change.spans[sep_idx + 1..]
        .iter()
        .find(|s| s.style.bg == Some(theme.diff_add_word_bg))
        .expect("right side still highlights its changed char");
    assert_eq!(right_word.content.as_ref(), "2");
}

proptest::proptest! {
    #[test]
    // Across arbitrary line pairs and widths, the total bytes carrying the
    // word-highlight bg in the rendered output must equal EXACTLY the byte
    // length of the computed changed range — never more (a bleed into
    // padding/wrap/blank cells) and never less (a dropped legitimate
    // highlight). General form of the three cases above.
    fn word_highlight_byte_count_matches_the_computed_range(
        old in proptest::string::string_regex(r"[ab \t,.(){}]{1,40}").unwrap(),
        new in proptest::string::string_regex(r"[ab \t,.(){}]{1,40}").unwrap(),
        width in 20usize..70,
    ) {
        proptest::prop_assume!(old != new);
        let theme = Theme::default();
        let model = single_file(
            FileStatus::Modified,
            DiffKind::Text(vec![Hunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
                lines: vec![rem(&old), add(&new)],
            }]),
            Some("f.txt"),
            Some("f.txt"),
        );
        let out = render_diff(&model, &theme, DiffLayout::SideBySide, width);
        let (o, n) = super::intraline::intra_change_ranges(&old, &new);
        let expected: usize = o.iter().chain(n.iter()).map(|r| r.end - r.start).sum();
        let add_bg = theme.diff_add_word_bg;
        let del_bg = theme.diff_del_word_bg;
        let actual: usize = out
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.bg == Some(add_bg) || s.style.bg == Some(del_bg))
            .map(|s| s.content.len())
            .sum();
        proptest::prop_assert_eq!(actual, expected);
    }
}

/// Space-indented content was never affected, so it must stay byte-identical —
/// the tab accounting has to be a no-op when there are no tabs.
#[test]
fn split_layout_unchanged_for_space_indented_content() {
    let spaced = DiffKind::Text(vec![Hunk {
        old_start: 1,
        old_lines: 4,
        new_start: 1,
        new_lines: 4,
        lines: vec![
            ctx("fn f() {"),
            ctx("    if ok {"),
            rem("        let x = old();"),
            add("        let x = new();"),
            ctx("    }"),
        ],
    }]);
    let model = single_file(FileStatus::Modified, spaced, Some("a.rs"), Some("a.rs"));
    let theme = Theme::default();
    let baseline = text(&super::render_diff_tw(
        &model,
        &theme,
        DiffLayout::SideBySide,
        100,
        1,
    ));
    for tab_width in [2, 4, 8] {
        assert_eq!(
            text(&super::render_diff_tw(
                &model,
                &theme,
                DiffLayout::SideBySide,
                100,
                tab_width
            )),
            baseline,
            "tab_width must not affect tab-free content (tab_width={tab_width})"
        );
    }
}
