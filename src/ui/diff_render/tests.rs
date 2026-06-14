//! Tests for the diff/show renderers (`super::render_diff` / `render_show`).
//! Split out of `diff_render.rs` verbatim during the 800-LoC decomposition.

use super::{DiffLayout, render_diff, render_show};
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
    let theme = Theme::default();
    for width in [40usize, 60, 80, 81, 100, 137] {
        let out = render_diff(&modify_model(), &theme, DiffLayout::SideBySide, width);
        for line in &out {
            let w: usize = line
                .spans
                .iter()
                .map(|s| crate::ui::display_width(s.content.as_ref()))
                .sum();
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
fn intra_change_range_trims_common_prefix_and_suffix() {
    // Only the digit differs; prefix "let x = " + suffix ";" are shared.
    let (old_r, new_r) = super::intra_change_range("let x = 1;", "let x = 2;").unwrap();
    assert_eq!(&"let x = 1;"[old_r], "1");
    assert_eq!(&"let x = 2;"[new_r], "2");
}

#[test]
fn intra_change_range_pure_insertion_is_empty_on_old_side() {
    // "ab" → "aXb": shared prefix "a" + suffix "b"; "X" inserted.
    let (old_r, new_r) = super::intra_change_range("ab", "aXb").unwrap();
    assert!(old_r.is_empty());
    assert_eq!(&"aXb"[new_r], "X");
}

#[test]
fn intra_change_range_none_when_identical_or_disjoint() {
    assert!(super::intra_change_range("same", "same").is_none());
    // No shared prefix or suffix → uniform wash, no word highlight.
    assert!(super::intra_change_range("abc", "xyz").is_none());
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
