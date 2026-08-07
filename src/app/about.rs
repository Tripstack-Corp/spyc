//! The `about` page — spyc's origin story / implementation notes / the name,
//! rendered through the pager's markdown mode. Sibling of the `?` help screen
//! (`App::open_help`), which is why it follows the same shape: content built in
//! memory at open time for the current width, installed with `set_pager`, kept
//! out of the buffer history, and rebuilt on resize.

use crate::ui::pager::{self, PagerView};

/// The page copy, bundled into the binary — an installed spyc has no repo
/// checkout to read `docs/ABOUT.md` from at runtime.
const ABOUT_MD: &str = include_str!("../../docs/ABOUT.md");

impl super::App {
    /// Title used for the about pager. Also the resize handler's "about is
    /// open" probe, same as [`Self::HELP_TITLE`].
    const ABOUT_TITLE: &'static str = "spyc — about";

    /// Build and show the about pager. Called from `Action::About`, the
    /// `:about` command, and on terminal resize (markdown wrap points are baked
    /// at render time, so a resize needs a rebuild).
    pub(super) fn open_about(&mut self) {
        let (term_w, _) = self.view.term_size;
        // Match `build_pager_view`'s markdown width math: the centered overlay
        // body minus the block borders and the line-number gutter, so prose
        // reflows to the space it will actually be drawn in.
        let body_w = pager::centered_body_width(term_w) as usize;
        let source_line_count = ABOUT_MD.lines().count().max(1);
        let gutter_w = (source_line_count.saturating_mul(4)).max(1).ilog10() as usize + 2;
        let wrap_w = body_w.saturating_sub(2 + gutter_w);
        let doc = crate::ui::markdown::render_doc(ABOUT_MD, &self.view.theme, Some(wrap_w));
        let mut view = PagerView::new_styled(Self::ABOUT_TITLE, doc.lines);
        // `m` toggles to the raw markdown, as it does for a markdown file.
        view.alt_lines = Some(
            ABOUT_MD
                .lines()
                .map(|l| ratatui::text::Line::from(l.to_string()))
                .collect(),
        );
        view.markdown_rendered = true;
        view.mermaid_blocks = doc.mermaid_blocks;
        view.no_history = true;
        self.set_pager(view);
    }

    /// True when the about pager is the currently-open pager view.
    pub(super) fn about_is_open(&self) -> bool {
        self.view
            .pager
            .as_ref()
            .is_some_and(|v| v.title == Self::ABOUT_TITLE)
    }
}

#[cfg(test)]
mod tests {
    use super::ABOUT_MD;
    use crate::app::App;
    use crate::keymap::Action;

    /// Content-integrity guard: the bundled copy is prose the author wrote, not
    /// generated text. Pin distinctive sentences from each section so a rewrite
    /// that guts `docs/ABOUT.md` fails here instead of shipping an empty page.
    #[test]
    fn bundled_copy_keeps_its_load_bearing_lines() {
        for sentinel in [
            "Claude: spy + c == spyc.",
            "Git is in-process via gitoxide",
            "BSD-3-Clause. No telemetry. No accounts.",
            "Most of this code was written by agents.",
        ] {
            assert!(
                ABOUT_MD.contains(sentinel),
                "docs/ABOUT.md no longer contains {sentinel:?} — the about page copy is \
                 hand-written; restore it rather than updating this assertion"
            );
        }
        // Headings the markdown render turns into the page's structure.
        for heading in ["# spyc — about", "## Implementation", "## The name"] {
            assert!(ABOUT_MD.contains(heading), "missing heading {heading:?}");
        }
    }

    /// `Action::About` opens the pager on the about page, rendered (not raw
    /// source) so the markdown mode is what the user lands in.
    #[test]
    fn about_action_opens_the_rendered_page() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            assert!(app.view.pager.is_none());
            let fx = app.apply(&Action::About).unwrap();
            assert!(fx.is_empty(), "about is pure view state, no effects");
            let view = app.view.pager.as_ref().expect("about opened a pager");
            assert_eq!(view.title, App::ABOUT_TITLE);
            assert!(view.markdown_rendered, "lands on the rendered view");
            assert!(view.alt_lines.is_some(), "`m` can toggle to the source");
            assert!(view.no_history, "stays out of the [b/]b buffer history");
            assert!(app.about_is_open());
        });
    }

    /// `lines` holds the markdown *render*, `alt_lines` the source. spyc's
    /// renderer keeps the `##` sigil, so the discriminator is styling: a
    /// rendered heading is bold, the same text on the source side is a plain
    /// unstyled span.
    #[test]
    fn rendered_lines_are_styled_and_the_source_side_is_not() {
        use ratatui::style::Modifier;
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            app.apply(&Action::About).unwrap();
            let view = app.view.pager.as_ref().expect("about opened a pager");

            // The render splits a heading across spans (`"## "` then the text),
            // so match on the line's joined content, not a single span.
            let has_text = |l: &ratatui::text::Line<'_>| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .contains("## Implementation")
            };
            let rendered = view
                .lines
                .iter()
                .find(|l| has_text(l))
                .expect("the Implementation heading is in the rendered view");
            assert!(
                rendered
                    .spans
                    .iter()
                    .any(|s| s.style.add_modifier.contains(Modifier::BOLD)),
                "rendered heading is unstyled — the page is showing raw source"
            );

            let source = view
                .alt_lines
                .as_ref()
                .expect("source side present")
                .iter()
                .find(|l| has_text(l))
                .expect("the Implementation heading is in the source view");
            assert!(
                source.spans.iter().all(|s| s.style.add_modifier.is_empty()),
                "source side is styled — `m` would toggle between two renders"
            );
        });
    }

    /// The page opens on its own title, not mid-prose.
    #[test]
    fn the_page_leads_with_its_h1() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            app.apply(&Action::About).unwrap();
            let view = app.view.pager.as_ref().expect("about opened a pager");
            let first = view
                .lines
                .iter()
                .map(|l| {
                    l.spans
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect::<String>()
                })
                .find(|t| !t.trim().is_empty())
                .expect("the rendered page is not blank");
            assert!(
                first.contains("spyc") && first.contains("about"),
                "first rendered line is {first:?}, not the page's H1"
            );
        });
    }
}
