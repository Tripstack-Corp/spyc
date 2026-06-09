//! Integration tests for the markdown renderer (`super::render`).
//! Split out of `markdown.rs` verbatim during the 800-LoC decomposition.

use super::*;
use crate::ui::theme::Theme;

fn render_plain(src: &str) -> Vec<String> {
    let theme = Theme::default();
    render(src, &theme, None)
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn renders_heading_with_hash_prefix() {
    let lines = render_plain("# Title\n");
    assert!(lines.iter().any(|l| l == "# Title"));
}

#[test]
fn renders_paragraph_text_inline() {
    let lines = render_plain("hello world\n");
    assert!(lines.iter().any(|l| l == "hello world"));
}

#[test]
fn renders_bullet_list_with_indent() {
    let lines = render_plain("- alpha\n- beta\n");
    assert!(lines.iter().any(|l| l == "\u{2022} alpha"));
    assert!(lines.iter().any(|l| l == "\u{2022} beta"));
}

/// Regression: a *loose* list (blank lines between items) wraps
/// each item in a Paragraph at the pulldown-cmark event level.
/// Before the `just_started_item` guard, the paragraph-start
/// flush would dump the bullet glyph onto its own line and leave
/// the item's text on the next line — visible as `•` + newline +
/// `text` in the pager (reported against BUGS.md when viewed via
/// the markdown viewer).
#[test]
fn loose_list_keeps_bullet_attached_to_item_text() {
    let src = "- alpha\n\n- beta\n";
    let lines = render_plain(src);
    // First and second item content must be on the same row as
    // their bullet — not orphaned to its own row.
    assert!(
        lines.iter().any(|l| l == "\u{2022} alpha"),
        "expected `• alpha` together on one line; got: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l == "\u{2022} beta"),
        "expected `• beta` together on one line; got: {lines:?}"
    );
    // And the bullet glyph must NOT appear as a standalone line.
    assert!(
        !lines.iter().any(|l| l == "\u{2022} " || l == "\u{2022}"),
        "bullet glyph should not be on its own line; got: {lines:?}"
    );
}

#[test]
fn renders_blockquote_with_left_rule() {
    let lines = render_plain("> quoted\n");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("\u{2503} ") && l.contains("quoted"))
    );
}

#[test]
fn fenced_code_block_emits_fence_lines() {
    let lines = render_plain("```\nfoo\n```\n");
    // Top + bottom fence rows, plus body.
    assert!(lines.iter().filter(|l| l.starts_with("\u{2500}")).count() >= 2);
    assert!(lines.iter().any(|l| l == "foo"));
}

#[test]
fn link_renders_text_with_url_after() {
    let lines = render_plain("see [docs](https://example.com)\n");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("docs") && l.contains("https://example.com"))
    );
}

#[test]
fn keyed_metadata_lines_stack() {
    // Lines that start with `**Word:**` should each render on
    // their own row, even without trailing two-space hard
    // breaks or blank lines between them. CommonMark would
    // collapse them into a single wrapped paragraph; our
    // `force_hard_breaks_before_keyed_lines` preprocessor
    // opts each such line into a markdown hard break.
    let src = "**To:** Alice\n**From:** Bob\n**Status:** Draft\n";
    let lines = render_plain(src);
    let non_empty: Vec<&String> = lines.iter().filter(|l| !l.is_empty()).collect();
    assert_eq!(non_empty.len(), 3, "got lines: {lines:?}");
    assert!(non_empty[0].contains("To:"), "{:?}", non_empty[0]);
    assert!(non_empty[1].contains("From:"), "{:?}", non_empty[1]);
    assert!(non_empty[2].contains("Status:"), "{:?}", non_empty[2]);
}

#[test]
fn prose_reflows_across_source_line_breaks() {
    // Source authored with 80-col wrap should reflow at the
    // pager's width, not stick to the awkward source break
    // points. (Regression for "soft-breaks-as-hard-breaks"
    // which faithfully reproduced the source's 80-col splits
    // and broke at "...using a / new / Facade API ...".)
    let src = "Build direction §3.1 names Option A as \"build inside the IBE perimeter\n\
                   using a new Facade API and partner-facing GraphQL.\" A natural reading\n\
                   is that IBE's existing WEB GraphQL endpoint is the foundation to extend.";
    let lines = render_plain(src);
    // The whole paragraph reflows as one — no source line
    // ending with "a" stranded on its own row, etc. With a
    // 200-col hint we'd get 1-2 long rows; with the default
    // 80-col target we get a few rows but each ending at a
    // word boundary, not at the source's split points.
    for l in &lines {
        // The mid-paragraph fragments from the source ("using",
        // "is that") should never appear on their own line.
        assert_ne!(l.trim(), "using", "stranded source fragment: {lines:?}");
        assert_ne!(l.trim(), "is that", "stranded source fragment: {lines:?}");
    }
}

#[test]
fn long_paragraph_wraps_at_content_width() {
    // Build a paragraph whose source is one line of >100 chars;
    // pulldown joins it as one logical paragraph, the renderer
    // should wrap at CONTENT_WIDTH (80) at word boundaries.
    let src = format!("{} word.\n", "lorem ".repeat(20));
    let lines = render_plain(&src);
    // Every non-empty body line should be <= CONTENT_WIDTH.
    for l in &lines {
        assert!(
            l.chars().count() <= super::CONTENT_WIDTH,
            "line {l:?} exceeded CONTENT_WIDTH"
        );
    }
    // And the paragraph should produce more than one line of
    // content (proves wrap actually happened).
    let body_lines = lines.iter().filter(|l| !l.is_empty()).count();
    assert!(
        body_lines >= 2,
        "expected wrap to produce multiple lines, got {lines:?}"
    );
}

#[test]
fn wrapped_list_item_indents_continuation() {
    // List item whose content overflows 80 cols should wrap with
    // 2-space hanging indent so the continuation aligns under
    // the bullet's text.
    let src = format!("- {}\n", "alpha ".repeat(20));
    let lines = render_plain(&src);
    let body: Vec<&String> = lines.iter().filter(|l| !l.is_empty()).collect();
    assert!(body.len() >= 2, "expected wrap on long list item");
    // First line starts with "• ".
    assert!(
        body[0].starts_with("\u{2022} "),
        "first line: {:?}",
        body[0]
    );
    // Continuation starts with two spaces (matches bullet width).
    assert!(body[1].starts_with("  "), "continuation: {:?}", body[1]);
}
#[test]
fn renders_simple_table_with_borders() {
    // Standard GFM table: header row + separator + data rows.
    // Should render with box-drawing borders and the header
    // text appearing somewhere inside the table.
    let src = "| H1 | H2 |\n|----|----|\n| a  | b  |\n| c  | d  |\n";
    let lines = render_plain(src);
    // Top border with corner glyphs.
    assert!(
        lines
            .iter()
            .any(|l| l.contains('\u{250c}') && l.contains('\u{2510}')),
        "missing top border in {lines:?}"
    );
    // Bottom border.
    assert!(
        lines
            .iter()
            .any(|l| l.contains('\u{2514}') && l.contains('\u{2518}')),
        "missing bottom border in {lines:?}"
    );
    // Header separator with cross.
    assert!(
        lines.iter().any(|l| l.contains('\u{253c}')),
        "missing header separator in {lines:?}"
    );
    // Header and data text appear.
    assert!(lines.iter().any(|l| l.contains("H1") && l.contains("H2")));
    assert!(lines.iter().any(|l| l.contains('a') && l.contains('b')));
}

#[test]
fn table_fences_each_body_row_with_a_separator() {
    // Every body row is fenced by a `├─┼─┤` separator (not just the
    // header), so rows read as distinct cells. A two-row body yields two
    // separator lines: one after the header, one between the body rows.
    let src = "| H1 | H2 |\n|----|----|\n| a | b |\n| c | d |\n";
    let lines = render_plain(src);
    let separators = lines.iter().filter(|l| l.contains('\u{253c}')).count();
    assert_eq!(
        separators, 2,
        "expected a separator after the header AND between the two body rows; got {lines:?}"
    );
}

#[test]
fn table_wraps_overlong_cells_to_multiple_visual_rows() {
    // A cell long enough that wrapping at column width produces
    // multiple visual rows. We should see the same column-border
    // glyph (`│`) on more than one line below the header
    // separator -- proving the cell spans multiple visual rows
    // rather than being truncated with `…`.
    let long = "alpha bravo ".repeat(20);
    let src = format!("| H |\n|---|\n| {long} |\n");
    let lines = render_plain(&src);
    // No truncation marker should appear (we wrap, not truncate).
    assert!(
        !lines.iter().any(|l| l.contains('\u{2026}')),
        "expected NO ellipsis (wrap, don't truncate); got {lines:?}"
    );
    // At least 3 rows of body content (the long string at narrow
    // width must wrap to multiple visual rows). Each body row
    // has a leading `│ `.
    let body_rows = lines.iter().filter(|l| l.starts_with("\u{2502} ")).count();
    assert!(
        body_rows >= 3,
        "expected ≥3 body rows from wrap, got {body_rows} in {lines:?}"
    );
}

#[test]
fn is_markdown_path_matches_md_and_markdown() {
    use std::path::Path;
    assert!(is_markdown_path(Path::new("README.md")));
    assert!(is_markdown_path(Path::new("notes.markdown")));
    assert!(!is_markdown_path(Path::new("main.rs")));
}
