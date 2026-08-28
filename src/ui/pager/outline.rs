//! Markdown outline folding and heading motions for the pager.
//!
//! The pure half — which lines a section owns, and what the buffer looks like
//! with some of them gone — lives in [`crate::ui::markdown::outline`]. This is
//! the `PagerView` glue: hold the unfolded document, rebuild `lines` when the
//! fold set changes, and keep the reading position anchored across the rebuild.

use std::collections::BTreeSet;

use ratatui::text::Line;

use crate::ui::markdown::{Heading, MermaidBlock, outline};

use super::PagerView;

/// The unfolded rendered document, retained so a fold is a rebuild rather than
/// a re-parse.
///
/// Boxed onto `PagerView` as one field: four loose fields would all have to be
/// kept in step by hand, and three of them are meaningless without the fourth.
pub struct MarkdownFold {
    /// Every rendered line, folds ignored. The master copy.
    pub lines: Vec<Line<'static>>,
    pub headings: Vec<Heading>,
    /// Mermaid ranges against `lines` — i.e. unfolded indices.
    pub mermaid_blocks: Vec<MermaidBlock>,
    /// Indices into `headings` whose bodies are currently hidden.
    pub folded: BTreeSet<usize>,
    /// Style for the `▸ N lines` marker on a collapsed heading. Captured at
    /// construction, where the theme is already in hand — carrying it here
    /// keeps every fold operation off the theme, which the pager's key handler
    /// cannot lend out while it holds the view borrow.
    pub marker: ratatui::style::Style,
}

impl PagerView {
    /// Is there an outline to act on? False for a non-markdown pager, and while
    /// the `m` toggle is showing the source (whose line numbers are different).
    pub fn has_outline(&self) -> bool {
        self.markdown_rendered
            && self
                .md_fold
                .as_ref()
                .is_some_and(|f| !f.headings.is_empty())
    }

    /// Fold or unfold the section the viewport is sitting in. Returns false when
    /// there is nothing to act on, so the caller can flash instead.
    ///
    /// The target is the nearest heading at or above the top visible line — the
    /// pager scrolls rather than carrying a cursor, so "the section I am reading"
    /// is the one whose heading I last passed.
    pub fn toggle_fold(&mut self, viewport: u16) -> bool {
        if !self.has_outline() {
            return false;
        }
        // `scroll` indexes the FOLDED buffer; the outline indexes the unfolded
        // one. Translate before asking which heading owns the line, or every
        // fold past the first lands on the wrong section. Resolved BEFORE the
        // mutable borrow below, which would otherwise lock out the read.
        let Some(h) = self.anchor_heading() else {
            return false;
        };
        let Some(fold) = self.md_fold.as_mut() else {
            return false;
        };
        if !fold.folded.remove(&h) {
            fold.folded.insert(h);
        }
        self.reapply_folds(Some(h), viewport);
        true
    }

    /// Collapse every section (`zM`).
    pub fn fold_all(&mut self, viewport: u16) -> bool {
        if !self.has_outline() {
            return false;
        }
        let anchor = self.anchor_heading();
        if let Some(fold) = self.md_fold.as_mut() {
            fold.folded = (0..fold.headings.len()).collect();
        }
        self.reapply_folds(anchor, viewport);
        true
    }

    /// Expand everything (`zR`).
    pub fn unfold_all(&mut self, viewport: u16) -> bool {
        if !self.has_outline() {
            return false;
        }
        let anchor = self.anchor_heading();
        if let Some(fold) = self.md_fold.as_mut() {
            fold.folded.clear();
        }
        self.reapply_folds(anchor, viewport);
        true
    }

    /// Scroll to the next (`forward`) or previous heading. Returns false when
    /// there isn't one in that direction.
    pub fn goto_heading(&mut self, forward: bool, viewport: u16) -> bool {
        if !self.has_outline() {
            return false;
        }
        let top = self.scroll;
        // Heading rows in FOLDED coordinates — the only ones reachable.
        let Some(fold) = self.md_fold.as_ref() else {
            return false;
        };
        let rows: Vec<usize> = fold
            .headings
            .iter()
            .filter_map(|h| self.folded_row(h.line))
            .collect();
        let target = if forward {
            rows.into_iter().find(|&r| r > top)
        } else {
            rows.into_iter().rev().find(|&r| r < top)
        };
        let Some(target) = target else {
            return false;
        };
        self.scroll = target.min(self.scroll_max(viewport));
        true
    }

    /// The unfolded index of folded row `row`.
    ///
    /// `md_kept` is left EMPTY while nothing is folded rather than allocating a
    /// 1:1 table for every markdown pager, so the mapping is the identity then.
    /// Both directions go through these two so no call site can forget that —
    /// `goto_heading` did, and silently found no headings at all, because
    /// `position` over an empty map matches nothing.
    fn unfolded_index(&self, row: usize) -> usize {
        self.md_kept.get(row).copied().unwrap_or(row)
    }

    /// The folded row currently showing unfolded line `line`, if any.
    fn folded_row(&self, line: usize) -> Option<usize> {
        if self.md_kept.is_empty() {
            return Some(line);
        }
        self.md_kept.iter().position(|&k| k == line)
    }

    /// The heading the viewport currently sits in, in outline indices.
    fn anchor_heading(&self) -> Option<usize> {
        let fold = self.md_fold.as_ref()?;
        outline::heading_at_or_above(&fold.headings, self.unfolded_index(self.scroll))
    }

    /// Rebuild `lines` from the master copy and the fold set, then put `anchor`'s
    /// heading back under the top of the viewport.
    ///
    /// Everything that indexes `lines` is rebuilt in the same pass — the kept-line
    /// map, the mermaid ranges, and the scroll. Doing any of them lazily is how a
    /// fold would leave the diagram hit-test pointing at the wrong rows.
    fn reapply_folds(&mut self, anchor: Option<usize>, viewport: u16) {
        let Some(fold) = self.md_fold.as_ref() else {
            return;
        };
        let folded = outline::apply(&fold.lines, &fold.headings, &fold.folded, fold.marker);
        let mermaid: Vec<MermaidBlock> = fold
            .mermaid_blocks
            .iter()
            .filter_map(|b| {
                outline::remap_range(&folded.kept, &b.line_range).map(|line_range| MermaidBlock {
                    line_range,
                    source: b.source.clone(),
                })
            })
            .collect();
        let anchor_row = anchor.and_then(|h| {
            let line = self.md_fold.as_ref()?.headings.get(h)?.line;
            folded.kept.iter().position(|&k| k == line)
        });
        self.lines = folded.lines;
        self.md_kept = folded.kept;
        self.mermaid_blocks = mermaid;
        self.scroll = anchor_row
            .unwrap_or(self.scroll)
            .min(self.scroll_max(viewport));
    }
}
