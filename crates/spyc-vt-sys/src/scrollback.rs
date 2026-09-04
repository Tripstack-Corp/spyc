//! Turning spyc's row budget into libghostty's two scrollback limits.
//!
//! **The contract: rows are the UX promise, bytes are a safety valve.** spyc
//! budgets scrollback in rows because that is what a user reasons about — `^a v`
//! scrolls back N lines. libghostty exposes both a line limit and a byte limit
//! and, per its header, "if both are set, the first-reached limit is used
//! first". So the byte limit must be set high enough that it never binds on
//! content a user could legitimately produce, and low enough to cap a
//! pathological stream's memory.
//!
//! Neither limit is left at its default. That is not tidiness: the default byte
//! limit truncates retained history to roughly 840 rows *irrespective of the
//! line limit*, so a 10,000-row budget silently delivers 8% of itself.
//!
//! If the derived ceiling ever comes out uncomfortable — north of ~50 MB per
//! pane at the default budget — that is a prompt to revisit the default **row**
//! budget in the open, not a licence to let the valve quietly bind first.

/// Retained bytes per row on the heavy end of realistic content, measured at
/// [`crate::pin::GHOSTTY_COMMIT`].
///
/// Derived from an agent-frame-shaped stream — several SGR runs per row
/// including a truecolour run, box drawing, a wide char, an emoji and a
/// combining mark — not from build-log spew. A ceiling sized for average
/// content binds on heavy-but-legitimate content, which is a silent contract
/// violation.
///
/// The measurement is indirect, because the C API has no "bytes retained"
/// readout: pin a byte ceiling, feed far more than fits, and read how many rows
/// survived. Across ceilings where the byte limit actually bound, the heavy
/// stream cost 767–929 B/row and the light stream 726–929. **The higher figure
/// is used deliberately.** The two corpora agreeing exactly at the tightest
/// ceiling is page-quantisation showing through rather than a content-driven
/// rate, so the conservative reading is the honest one for a safety valve.
pub const BYTES_PER_ROW_HEAVY: usize = 929;

/// libghostty's page size, the quantum its pruning works in. Its header puts a
/// page at "about 400KB", and says both limits are estimates because whole
/// pages are what get reclaimed.
pub const PAGE_BYTES: usize = 400 * 1024;

/// Pages of slack added on top of the derived requirement.
///
/// **Additive, not multiplicative.** Page granularity is a fixed quantum: it
/// does not scale with the row budget, so a percentage margin would
/// over-provision a large budget and under-provision a small one. Two pages
/// clears the granularity in both directions — one for the partially-filled
/// page at the head of history, one for the page pruning is about to reclaim.
pub const SLACK_PAGES: usize = 2;

/// The two limits to hand libghostty for a given row budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// `GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES` — the UX contract.
    pub max_lines: usize,
    /// `GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES` — the safety valve.
    pub max_bytes: usize,
}

/// Derive both limits from spyc's row budget.
///
/// The line limit *is* the budget. The byte ceiling is
/// `budget × heavy-rate + slack`, so it sits above anything realistic content
/// can reach at that budget and binds only on a pathological stream.
#[must_use]
pub const fn limits_for_row_budget(rows: usize) -> Limits {
    Limits {
        max_lines: rows,
        max_bytes: rows * BYTES_PER_ROW_HEAVY + SLACK_PAGES * PAGE_BYTES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// spyc's default budget (`Pane::spawn_with_env` passes 10_000) must not
    /// land in the range that would prompt revisiting the row budget instead.
    #[test]
    fn the_default_budget_stays_well_under_the_discomfort_threshold() {
        let l = limits_for_row_budget(10_000);
        assert_eq!(
            l.max_lines, 10_000,
            "the line limit is the budget, verbatim"
        );
        assert!(
            l.max_bytes < 50 * 1024 * 1024,
            "a ceiling north of ~50 MB/pane is a prompt to revisit the row budget \
             in the open, not to ship quietly; got {} bytes",
            l.max_bytes
        );
        // 10,109,200 B = 9.64 MiB at the default budget. Bounded on both sides
        // so a change to the rate or the slack has to be deliberate: widening
        // this window silently is how a derived constant stops being derived.
        assert_eq!(
            l.max_bytes, 10_109_200,
            "the derivation moved; update this and the addendum together"
        );
        assert!(
            l.max_bytes < 10 * 1024 * 1024,
            "under 10 MiB/pane: {}",
            l.max_bytes
        );
    }

    /// The ceiling must exceed what the heavy corpus actually needs, or the
    /// valve binds on legitimate content — the failure the derivation exists to
    /// avoid. Slack is what makes this strict rather than exactly equal.
    #[test]
    fn the_ceiling_clears_heavy_content_at_every_budget() {
        for rows in [24usize, 500, 1_000, 10_000, 100_000] {
            let l = limits_for_row_budget(rows);
            let needed = rows * BYTES_PER_ROW_HEAVY;
            assert!(
                l.max_bytes > needed,
                "budget {rows}: ceiling {} must exceed the heavy requirement {needed}",
                l.max_bytes
            );
            assert!(
                l.max_bytes - needed >= SLACK_PAGES * PAGE_BYTES,
                "budget {rows}: slack shrank below {SLACK_PAGES} pages"
            );
        }
    }

    /// Slack is absolute, so a tiny budget gets proportionally much more of it
    /// and a huge budget is dominated by the rate. Pinned because switching to
    /// a multiplicative margin would silently break the small-budget end.
    #[test]
    fn slack_is_absolute_not_proportional() {
        let small = limits_for_row_budget(24);
        let large = limits_for_row_budget(100_000);
        assert_eq!(
            small.max_bytes - 24 * BYTES_PER_ROW_HEAVY,
            large.max_bytes - 100_000 * BYTES_PER_ROW_HEAVY,
            "the slack term must not scale with the budget"
        );
    }
}
