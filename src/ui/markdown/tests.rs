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

#[test]
fn nested_bold_in_heading_keeps_rest_bold() {
    use ratatui::style::Modifier;
    let theme = Theme::default();
    // A heading is bold; a `**strong**` span inside it must not un-bold the
    // text that follows when the inner Strong closes (the style_mods bitflag
    // regression — now reference-counted).
    let lines = render("# Title **mid** tail", &theme, None);
    let heading = lines
        .iter()
        .find(|l| !l.spans.is_empty())
        .expect("a heading line");
    let tail = heading
        .spans
        .iter()
        .find(|s| s.content.contains("tail"))
        .expect("a span containing `tail`");
    assert!(
        tail.style.add_modifier.contains(Modifier::BOLD),
        "text after a nested **bold** in a heading must stay bold; style={:?}",
        tail.style
    );
}

// --- mermaid block detection (docs/archive/MERMAID_PAGER_PLAN.md, Phase 1) ---

fn doc(src: &str) -> MarkdownDoc {
    render_doc(src, &Theme::default(), None)
}

#[test]
fn mermaid_block_recorded_with_source_and_line_range() {
    let d = doc("intro\n\n```mermaid\nflowchart LR\n  A-->B\n```\n\nafter\n");
    assert_eq!(d.mermaid_blocks.len(), 1, "exactly one mermaid block");
    let b = &d.mermaid_blocks[0];
    assert_eq!(b.source, "flowchart LR\n  A-->B");
    // The recorded range must point at real placeholder rows, and the header
    // row must be the first line of that range.
    assert!(b.line_range.start < b.line_range.end);
    assert!(b.line_range.end <= d.lines.len());
    let header: String = d.lines[b.line_range.start]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(
        header.contains("mermaid diagram"),
        "first line of the block range is the header, got {header:?}"
    );
    // The source is shown within the block range (open-MVP keeps it visible).
    let block_text: String = d.lines[b.line_range.clone()]
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();
    assert!(block_text.contains("flowchart LR"));
}

#[test]
fn multiple_mermaid_blocks_are_each_recorded_and_disjoint() {
    let d =
        doc("```mermaid\nflowchart LR\n  A-->B\n```\n\n```mermaid\nflowchart TD\n  X-->Y\n```\n");
    assert_eq!(d.mermaid_blocks.len(), 2);
    assert_eq!(d.mermaid_blocks[0].source, "flowchart LR\n  A-->B");
    assert_eq!(d.mermaid_blocks[1].source, "flowchart TD\n  X-->Y");
    // Ranges don't overlap and are in document order.
    assert!(d.mermaid_blocks[0].line_range.end <= d.mermaid_blocks[1].line_range.start);
}

#[test]
fn non_mermaid_code_block_records_nothing() {
    let d = doc("```rust\nfn main() {}\n```\n");
    assert!(
        d.mermaid_blocks.is_empty(),
        "a ```rust block is code, not a diagram"
    );
}

#[test]
fn no_code_block_records_nothing() {
    let d = doc("# just prose\n\nno diagrams here\n");
    assert!(d.mermaid_blocks.is_empty());
}

// --- the document ends on its last line of content ---

/// The pager reads `lines.len()` as the document's length: it numbers rows from
/// it, prints it as `(N lines)`, and puts `[EOF]` on the row after the last one.
/// Every block end emitting its separator blank unconditionally left a trailing
/// row with no content, so `[EOF]` sat a row below the text it was marking the
/// end of. One case per block kind that can close on a blank — a blockquote
/// closes on two (its inner paragraph, then the quote).
#[test]
fn rendered_doc_ends_on_content_not_a_separator_blank() {
    for (kind, src) in [
        ("paragraph", "hello\n"),
        ("heading", "# Title\n"),
        ("list", "- a\n- b\n"),
        ("nested list", "- a\n  - b\n"),
        ("code block", "```rust\nfn main() {}\n```\n"),
        ("table", "|a|b|\n|-|-|\n|1|2|\n"),
        ("blockquote", "> quoted\n"),
        ("mermaid", "```mermaid\nflowchart LR\n  A-->B\n```\n"),
        ("trailing blanks in source", "hello\n\n\n\n"),
        (
            "paragraph after block",
            "```rust\nfn f() {}\n```\n\ntail text\n",
        ),
    ] {
        let lines = render_plain(src);
        assert!(
            !lines.is_empty(),
            "{kind}: rendered to nothing, expected content"
        );
        assert!(
            !lines[lines.len() - 1].is_empty(),
            "{kind}: last rendered line is blank — [EOF] would land past the \
             content:\n{lines:#?}"
        );
    }
}

/// An empty (or blank-only) source has no content to end on, and must not
/// underflow its way to a panic while the trailing blanks are trimmed.
#[test]
fn empty_source_renders_no_lines() {
    for src in ["", "\n", "\n\n\n", "   \n"] {
        let lines = render_plain(src);
        assert!(
            lines.iter().all(|l| l.trim().is_empty()),
            "blank source rendered content: {lines:#?}"
        );
    }
}

/// A diagram's `line_range` runs from its header row *through* the separator
/// blank that follows it, so trimming that blank off the end of the document
/// leaves the range pointing one row past the end — the pager slices `lines`
/// with it to find the source under the cursor.
#[test]
fn mermaid_range_stays_in_bounds_when_the_diagram_ends_the_doc() {
    let d = doc("intro\n\n```mermaid\nflowchart LR\n  A-->B\n```\n");
    assert_eq!(d.mermaid_blocks.len(), 1);
    let b = &d.mermaid_blocks[0];
    assert!(
        b.line_range.end <= d.lines.len(),
        "range {:?} escapes the {} rendered lines",
        b.line_range,
        d.lines.len()
    );
    // Still a usable slice: the header is its first row and the source is inside.
    let block_text: String = d.lines[b.line_range.clone()]
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
        .collect();
    assert!(block_text.contains("mermaid diagram"));
    assert!(block_text.contains("flowchart LR"));
}

// ── render fidelity: what the parser knows but the renderer used to drop ──

/// A fence tag is a language, not a filename. Synthesizing `snippet.<lang>` and
/// going through syntect's EXTENSION table meant the spellings people actually
/// write missed entirely: spyc's own docs carry 28 ` ```rust ` blocks and zero
/// ` ```rs `, and every one of them rendered unhighlighted.
///
/// Asserts on span COUNT, not colours: a highlighted line is split into many
/// styled spans, a plain one is a single span. That distinguishes the two
/// without pinning syntect's palette.
#[test]
fn full_language_names_in_a_fence_highlight_not_just_extensions() {
    let theme = Theme::default();
    let spans_for = |lang: &str| {
        let src = format!("```{lang}\nfn main() {{ let x = 1; }}\n```\n");
        render(&src, &theme, None)
            .into_iter()
            .map(|l| l.spans.len())
            .max()
            .unwrap_or(0)
    };
    // The extension spelling always worked — it's the control.
    let by_ext = spans_for("rs");
    assert!(by_ext > 1, "```rs should highlight (got {by_ext} spans)");
    for lang in ["rust", "Rust", "RUST"] {
        assert!(
            spans_for(lang) > 1,
            "```{lang} must highlight like ```rs does"
        );
    }
    // mdBook and rustdoc write attributes into the info string; the language is
    // the first word, and the rest must not defeat the lookup.
    for lang in ["rust,ignore", "rust no_run"] {
        assert!(
            spans_for(lang) > 1,
            "```{lang} must highlight — attributes aren't part of the name"
        );
    }
    // An unknown language still falls back to plain rather than failing.
    assert_eq!(
        spans_for("nosuchlang"),
        1,
        "an unrecognized language renders plain, one span per line"
    );
}

/// YAML front matter is a data header. Without the metadata option the opening
/// `---` parsed as a thematic break and the closing one as a setext underline,
/// so the fields collapsed into a single run-together H2 — which is how
/// `SKILL.md`, the file spyc itself installs, looked in spyc's own pager.
#[test]
fn yaml_front_matter_renders_as_its_own_lines_not_a_heading() {
    let lines = render_plain("---\nname: spyc\ndescription: a thing\n---\n\n# Real Heading\n");
    assert!(
        lines.iter().any(|l| l == "name: spyc"),
        "each field keeps its own line: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l == "description: a thing"),
        "fields are not reflowed together: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("##")),
        "the closing `---` must not become a setext underline: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("\u{2500}\u{2500}")),
        "the opening `---` must not become a thematic break: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l == "# Real Heading"),
        "the document after the front matter still renders: {lines:?}"
    );
}

/// GFM alerts. Without `ENABLE_GFM` the tag stayed in the quote body and
/// rendered as the literal text `[!WARNING]` — which is how CONFIGURATION.md
/// and four archive docs looked.
#[test]
fn gfm_alerts_render_as_a_label_not_literal_tag_text() {
    for (tag, label) in [
        ("NOTE", "NOTE"),
        ("TIP", "TIP"),
        ("IMPORTANT", "IMPORTANT"),
        ("WARNING", "WARNING"),
        ("CAUTION", "CAUTION"),
    ] {
        let lines = render_plain(&format!("> [!{tag}]\n> Body text.\n"));
        assert!(
            lines.iter().any(|l| l == &format!("\u{2503} {label}")),
            "[!{tag}] should render as its own label row: {lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l.contains("[!")),
            "the raw tag must not survive into the body: {lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("Body text.")),
            "the quote body still renders: {lines:?}"
        );
    }
    // A plain blockquote is unaffected — no label row, no leading blank.
    let plain = render_plain("> just a quote\n");
    assert!(
        plain.iter().any(|l| l == "\u{2503} just a quote"),
        "an ordinary blockquote keeps its shape: {plain:?}"
    );
}

/// The delimiter row's alignment is author intent, and `Tag::Table` used to
/// throw the whole vector away — so a right-aligned numeric column rendered
/// ragged-left like everything else.
#[test]
fn table_columns_honor_the_delimiter_rows_alignment() {
    let lines = render_plain("| left | mid | right |\n|:-----|:---:|------:|\n| a | b | c |\n");
    let body = lines
        .iter()
        .find(|l| l.contains('a') && l.contains('b') && l.contains('c'))
        .unwrap_or_else(|| panic!("no body row in {lines:?}"));

    // Cell interiors, between the `│` separators and minus the framing spaces.
    let cells: Vec<&str> = body.split('\u{2502}').filter(|c| !c.is_empty()).collect();
    let [left, mid, right] = cells.as_slice() else {
        panic!("expected three cells, got {cells:?}");
    };
    assert!(
        left.starts_with(" a") && left.ends_with("  "),
        "`:---` pads on the right: {left:?}"
    );
    assert!(
        right.starts_with("  ") && right.trim_end().ends_with('c'),
        "`---:` pads on the left: {right:?}"
    );
    let m = mid.trim_matches(' ');
    assert_eq!(m, "b");
    let lead = mid.len() - mid.trim_start_matches(' ').len();
    let trail = mid.len() - mid.trim_end_matches(' ').len();
    assert!(
        lead > 1 && trail > 1 && lead.abs_diff(trail) <= 1,
        "`:--:` splits the slack both sides: lead={lead} trail={trail} in {mid:?}"
    );
}
