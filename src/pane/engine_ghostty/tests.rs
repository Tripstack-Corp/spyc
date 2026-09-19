//! Tests relocated from `engine_ghostty/mod.rs` to keep the engine itself under
//! the 800-line guideline. They reach the engine's private internals -- the raw
//! terminal handle, the mode reader -- so they are a child of it rather than a
//! sibling the way `app/mod_tests.rs` is; nested modules use `super::super::` to
//! get back to `engine_ghostty`.

/// The seam's contract, run against this engine.
#[cfg(test)]
mod seam_contract {
    use super::super::*;
    use crate::pane::engine::conformance;

    fn fed(rows: u16, cols: u16, bytes: &[u8]) -> GhosttyEngine {
        let mut e = <GhosttyEngine as Engine>::new(rows, cols, 10_000);
        e.process(bytes);
        e
    }

    // The seam's contract suite, against the shipped engine. The same five
    // run against vt100 in `engine_vt100`; one contract, both impls.
    #[test]
    fn reports_what_the_engine_holds() {
        conformance::reports_what_the_engine_holds::<GhosttyEngine>();
    }

    #[test]
    fn past_the_edge_is_absence_not_blankness() {
        conformance::past_the_edge_is_absence_not_blankness::<GhosttyEngine>();
    }

    #[test]
    fn mouse_protocol_maps_through_the_model_enums() {
        conformance::mouse_protocol_maps_through_the_model_enums::<GhosttyEngine>();
    }

    #[test]
    fn set_scrollback_clamps_to_the_real_length() {
        conformance::set_scrollback_clamps_to_the_real_length::<GhosttyEngine>();
    }

    #[test]
    fn reports_the_modes_the_pane_branches_on() {
        conformance::reports_the_modes_the_pane_branches_on::<GhosttyEngine>();
    }

    /// A frame filled by the render state and one filled by coordinate must be
    /// the same frame.
    ///
    /// Two fill paths exist because the render state only ever presents the
    /// live viewport, so history reads cannot use it. Two paths that drift
    /// would give the pane one grid and the scrollback pager another, and
    /// nothing else in the tree would notice — the render is snapshot-tested
    /// against ITSELF. This is the check that does not share their model.
    #[test]
    fn both_fills_agree_on_the_live_viewport() {
        let e = fed(
            6,
            24,
            // The last line is a background-colour ERASE: a row painted to the
            // edge by `ESC[K` under SGR, which stores its colour in the cell's
            // content tag rather than a style. Added after that shape shipped
            // a bug this corpus was too narrow to see.
            "\x1b[31mred\x1b[0m \x1b[1;4mbold-u\x1b[0m\r\n\
             wide \u{3042}\u{3044}\u{3046} tail\r\n\
             \x1b[2mdim\x1b[0m \x1b[7mrev\x1b[0m\r\n\
             \x1b[30;42mHDR\x1b[K\x1b[0m\r\n\
             plain\r\n"
                .as_bytes(),
        );
        let s = e.screen();

        let mut via_rs = Frame::default();
        assert!(
            s.fill_from_render_state(&mut via_rs),
            "the render state fill must succeed on a live viewport"
        );
        let mut via_grid = Frame::default();
        s.fill_from_grid_ref(&mut via_grid);

        assert_eq!(
            (via_rs.rows, via_rs.cols),
            (via_grid.rows, via_grid.cols),
            "geometry"
        );
        assert_eq!(via_rs.styles, via_grid.styles, "cell styles");
        assert_eq!(via_rs.wrapped, via_grid.wrapped, "row wrap flags");
        let text_of = |f: &Frame| {
            (0..f.rows)
                .map(|r| row_string(f, r))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(text_of(&via_rs), text_of(&via_grid), "cell text");
    }

    /// A scrolled-back view reads history, and the offset clamps to what
    /// exists — `ui::scrollback` discovers the length by asking for
    /// `usize::MAX` and reading back.
    #[test]
    fn the_view_offset_walks_history_and_clamps() {
        let mut e = <GhosttyEngine as Engine>::new(3, 12, 10_000);
        for i in 0..20 {
            e.process(format!("L{i:02}\r\n").as_bytes());
        }
        let s = e.screen_mut();
        assert_eq!(s.scrollback(), 0, "starts at the live edge");
        s.set_scrollback(usize::MAX);
        let len = s.scrollback();
        assert!(len > 0 && len <= 20, "clamped to real history, got {len}");
        s.set_scrollback(2);
        assert_eq!(s.scrollback(), 2);
        let text = s.contents();
        assert!(
            text.contains("L16") || text.contains("L15"),
            "a 2-row scrollback shows older rows, got {text:?}"
        );
        s.set_scrollback(0);
        assert!(s.contents().contains("L19"), "back at the live edge");
    }
}

/// The four engine-side defects [#34](https://github.com/Tripstack-Corp/spyc/issues/34)
/// turned out to be, each pinned against the shipped engine.
///
/// The spike found these by differential against the incumbent and named them
/// in `docs/drafts/VT_ENGINE_SPIKE.md`; the bytes here are its fixtures. They
/// are asserted in-crate rather than in `spikes/vt-engine/`, which is excluded
/// from the workspace and so runs only when someone remembers — a defect that
/// took an engine swap to fix deserves a test that runs on every push.
#[cfg(test)]
mod issue_34_engine_defects {
    use super::super::*;

    fn screen_of(rows: u16, cols: u16, bytes: &[u8]) -> GhosttyEngine {
        let mut e = <GhosttyEngine as Engine>::new(rows, cols, 10_000);
        e.process(bytes);
        e
    }

    /// `ESC ( 0` selects DEC special graphics, so `lqqqk` is a box, not
    /// letters. vt100 does not implement SCS at all and renders the literal
    /// text — garbage in any pane whose child draws boxes.
    #[test]
    fn scs_box_drawing_draws_boxes() {
        let e = screen_of(5, 20, b"\x1b(0lqqqk\r\nx  x\r\nmqqqj\x1b(B\r\n");
        let text = e.screen().contents();
        let row0 = text.lines().next().unwrap_or_default();
        assert_eq!(row0, "┌───┐", "SCS box drawing, got {row0:?}");
        assert!(
            text.lines().nth(2).unwrap_or_default().starts_with('└'),
            "the bottom edge too, got {text:?}"
        );
    }

    /// A row written before a DECSTBM scroll region is set must survive it.
    /// vt100 loses it.
    #[test]
    fn a_row_written_before_decstbm_survives() {
        let e = screen_of(
            8,
            20,
            b"\x1b[2J\x1b[H\x1b[3;6rheader\r\n\x1b[3Hone\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\nseven\r\neight\r\n",
        );
        let text = e.screen().contents();
        assert!(
            text.contains("header"),
            "the pre-region row must survive, got {text:?}"
        );
    }

    /// Content scrolled out of a TOP-ANCHORED DECSTBM region reaches history.
    /// vt100 retains zero rows, which is why a codex pane's scrollback was
    /// always empty — codex confines its transcript to a scroll region.
    ///
    /// Top-anchored is the whole point, and the first version of this test got
    /// it wrong by reusing the spike's `3;6` fixture. A region that does not
    /// start at row 1 scrolls its lines within the screen, so none of them
    /// leave it and history correctly stays empty — measured at `2;6` and
    /// `3;6`, both 0 rows, against 15 at `1;6`. The engine was right and the
    /// test was wrong.
    #[test]
    fn a_top_anchored_scroll_region_accumulates_scrollback() {
        let mut e = <GhosttyEngine as Engine>::new(8, 20, 10_000);
        e.process(b"\x1b[2J\x1b[H\x1b[1;6r");
        for i in 0..20 {
            e.process(format!("line {i:02}\r\n").as_bytes());
        }
        let s = e.screen_mut();
        s.set_scrollback(usize::MAX);
        assert!(
            s.scrollback() > 0,
            "a scroll-region child must accumulate scrollback, got {}",
            s.scrollback()
        );
    }

    /// A tag-sequence grapheme survives past 18 bytes. vt100's `Cell` has
    /// `CONTENT_BYTES = 22` and drops silently at 18; the Scotland flag needs
    /// 28, so it lost two of its six tag characters.
    #[test]
    fn a_tag_sequence_grapheme_survives_past_eighteen_bytes() {
        let flag = "\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}";
        assert!(flag.len() > 18, "the fixture must exceed the old limit");
        let e = screen_of(3, 20, format!("{flag}|end\r\n").as_bytes());
        let mut got = String::new();
        e.screen().cell_text(0, 0, &mut got);
        assert_eq!(
            got.chars().count(),
            flag.chars().count(),
            "every codepoint of the cluster survives: {got:?}"
        );
        assert_eq!(got, flag);
    }
}

/// The shipped engine against the reference engine, over a real captured
/// terminal stream, through the same widget.
///
/// This is the only place spyc compares the two engines' *rendering* rather
/// than their seam answers, and it is what caught the background-colour-erase
/// bug that `both_fills_agree_on_the_live_viewport` could not: both fills were
/// wrong the same way, so they agreed. An instrument that shares the subject's
/// model inherits its blind spots — the reference engine does not share it.
///
/// It dies with `engine_vt100.rs` after 2.2 tags ([#453](https://github.com/Tripstack-Corp/spyc/issues/453)).
/// Until then it is the strongest thing the fallback buys.
#[cfg(test)]
mod against_the_reference_engine {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget as _;

    use crate::pane::PaneWidget;
    use crate::pane::engine::Engine as EngineT;

    fn render<E: EngineT>(bytes: &[u8], rows: u16, cols: u16) -> Buffer {
        let mut e = E::new(rows, cols, 10_000);
        e.process(bytes);
        let area = Rect::new(0, 0, cols, rows);
        let mut buf = Buffer::empty(area);
        PaneWidget {
            screen: e.screen(),
            focused: true,
            selection: None,
        }
        .render(area, &mut buf);
        buf
    }

    /// htop is the case that exercises meters, a full-width header bar, a
    /// selected row and the function-key footer — every one of which is a
    /// coloured run that ends in erased cells.
    #[test]
    fn htop_renders_identically_through_both_engines() {
        // Absolute, via the manifest dir: tests share a process, so a
        // relative path breaks the moment another test changes the cwd.
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("spikes/vt-engine/fixtures/htop.bin");
        let bytes = std::fs::read(&fixture)
            .unwrap_or_else(|e| panic!("the spike's htop capture at {}: {e}", fixture.display()));
        let (rows, cols) = (24u16, 80);
        let a = render::<vt100::Parser>(&bytes, rows, cols);
        let b = render::<super::super::GhosttyEngine>(&bytes, rows, cols);

        let mut wrong = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                let (Some(x), Some(y)) = (a.cell((c, r)), b.cell((c, r))) else {
                    continue;
                };
                if x.symbol() != y.symbol() {
                    wrong.push(format!(
                        "({r},{c}) symbol {:?} vs {:?}",
                        x.symbol(),
                        y.symbol()
                    ));
                    continue;
                }
                let (sx, sy) = (x.style(), y.style());
                if sx.bg != sy.bg {
                    wrong.push(format!("({r},{c}) bg {:?} vs {:?}", sx.bg, sy.bg));
                }
                if sx.add_modifier != sy.add_modifier {
                    wrong.push(format!(
                        "({r},{c}) modifiers {:?} vs {:?}",
                        sx.add_modifier, sy.add_modifier
                    ));
                }
                // Foreground is compared only where there is a glyph to
                // colour. A cell erased under SGR keeps the foreground of the
                // moment in vt100 and carries none in ghostty, which stores
                // such a cell as a background-only content tag — invisible
                // either way, since a blank has nothing to paint.
                if x.symbol().trim().is_empty() {
                    continue;
                }
                if sx.fg != sy.fg {
                    wrong.push(format!("({r},{c}) fg {:?} vs {:?}", sx.fg, sy.fg));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} cells differ between the engines:\n  {}",
            wrong.len(),
            wrong[..wrong.len().min(12)].join("\n  ")
        );
    }
}

/// Grapheme clustering (DEC mode 2027), pinned against `ui::display_width`.
///
/// Ghostty-scoped rather than part of the `E: Engine` conformance suite, for
/// the reason that suite states about `issue_34_engine_defects`: vt100 fails
/// this by design — it lays the flag out as two narrow cells, the ZWJ family
/// across six columns and the VS16 heart in one — and pinning a capability the
/// escape hatch ([#453](https://github.com/Tripstack-Corp/spyc/issues/453))
/// cannot have would assert only that it is still broken.
#[cfg(test)]
mod grapheme_cluster_width {
    use super::super::*;

    /// One regional-indicator pair: 🇨🇦.
    const FLAG: &str = "\u{1f1e8}\u{1f1e6}";
    /// A ZWJ sequence: 👨‍👩‍👧.
    const FAMILY: &str = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
    /// Base plus skin-tone modifier: 👍🏽.
    const SKIN_TONE: &str = "\u{1f44d}\u{1f3fd}";
    /// Emoji presentation selector: ❤️. Widens a codepoint that is narrow bare.
    const HEART_VS16: &str = "\u{2764}\u{fe0f}";
    /// spyc's own status-bar logo: 🌶️ — the same VS16 shape.
    const CHILLI: &str = "\u{1f336}\u{fe0f}";

    /// Columns the engine spent laying `s` out on a fresh grid.
    ///
    /// A cell counts as occupied when it carries text or is a wide glyph's
    /// continuation; a trailing run of blank narrow cells is the unused rest
    /// of the row.
    fn columns_used(s: &str) -> usize {
        let mut e = <GhosttyEngine as Engine>::new(2, 40, 0);
        e.process(s.as_bytes());
        let screen = e.screen();
        let mut last = 0;
        let mut text = String::new();
        for col in 0..40u16 {
            let Some(st) = screen.cell_style(0, col) else {
                break;
            };
            text.clear();
            screen.cell_text(0, col, &mut text);
            if !text.is_empty() || st.wide == Wide::Tail {
                last = usize::from(col) + 1;
            }
        }
        last
    }

    /// The engine and `ui::display_width` are the two halves of one pipeline —
    /// a producer budgets columns the way `display_width` counts them, the
    /// engine lays the bytes out, ratatui re-emits the grid — so a shape they
    /// disagree on is drawn misaligned. Before mode 2027 was enabled the engine
    /// summed per-codepoint widths: the flag took 4 columns against a budget of
    /// 2, and the VS16 heart took 1.
    #[test]
    fn every_cluster_shape_costs_what_display_width_budgets() {
        for s in [
            FLAG,
            FAMILY,
            SKIN_TONE,
            HEART_VS16,
            CHILLI,
            "\u{2705}",  // ✅ — already wide bare
            "\u{3042}",  // CJK, the baseline wide case
            "\u{2764}",  // bare heart: narrow, and must STAY narrow
            "e\u{0301}", // combining acute: one column
            "ab",        // and clustering must not disturb plain ASCII
        ] {
            assert_eq!(
                columns_used(s),
                crate::ui::display_width(s),
                "engine vs display_width on {:?}",
                s.escape_unicode().to_string()
            );
        }
    }

    /// Agreeing on the total is not enough: the whole cluster must land in ONE
    /// head cell. Split across cells the column count could still come out
    /// right while `contents_between` (selection text) and `ui::scrollback`
    /// reassemble the codepoints by luck.
    #[test]
    fn a_cluster_lands_in_one_cell_not_one_per_codepoint() {
        for s in [FLAG, FAMILY, SKIN_TONE, HEART_VS16, CHILLI] {
            let mut e = <GhosttyEngine as Engine>::new(2, 40, 0);
            e.process(s.as_bytes());
            let screen = e.screen();
            let mut text = String::new();
            assert!(screen.cell_text(0, 0, &mut text));
            assert_eq!(
                text,
                s,
                "the head cell must carry the whole cluster, got {:?}",
                text.escape_unicode().to_string()
            );
            assert_eq!(screen.cell_style(0, 0).map(|c| c.wide), Some(Wide::Head));
            assert_eq!(screen.cell_style(0, 1).map(|c| c.wide), Some(Wide::Tail));
        }
    }

    /// A child running `reset` emits RIS, which restores every mode to its
    /// reset default. Clustering is set through `OPT_MODE_DEFAULT` precisely so
    /// it survives that; `OPT_MODE` alone would set the live value and let RIS
    /// silently drop the pane back to per-codepoint layout.
    #[test]
    fn a_child_reset_does_not_drop_clustering() {
        let mut e = <GhosttyEngine as Engine>::new(2, 40, 0);
        assert!(e.screen().mode(MODE_GRAPHEME_CLUSTER), "enabled at init");
        e.process(b"\x1bc");
        assert!(
            e.screen().mode(MODE_GRAPHEME_CLUSTER),
            "RIS must restore mode 2027 set, not clear it"
        );
        e.process(FLAG.as_bytes());
        assert_eq!(columns_used(FLAG), 2, "and layout still clusters after RIS");
    }
}
