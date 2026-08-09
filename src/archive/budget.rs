//! Whether to mount at all, and whether to ask first.
//!
//! The disk cost of a mount is knowable *before* paying it — a zip's central
//! directory and a tar's headers both carry uncompressed sizes — so this decides
//! from the numbers rather than discovering trouble halfway through an
//! extraction. Pure: free space is passed in, never queried here.

use crate::fs::ops::format_size;

/// Ceilings a mount is judged against. The App layer fills these from
/// `[archive]` config; the defaults are the shipped ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetLimits {
    /// Hard ceiling on what a streaming (compressed-tar) mount may extract.
    pub extract_budget: u64,
    /// Confirm before a mount that would extract more than this.
    pub warn_over: u64,
    /// Confirm when the uncompressed total exceeds the archive by this factor —
    /// the zip-bomb shape.
    pub ratio_limit: u64,
    /// Headroom left on the staging filesystem after an extraction.
    pub free_space_margin: u64,
}

const MIB: u64 = 1024 * 1024;

impl Default for BudgetLimits {
    fn default() -> Self {
        Self {
            extract_budget: 512 * MIB,
            warn_over: 128 * MIB,
            ratio_limit: 200,
            free_space_margin: 64 * MIB,
        }
    }
}

/// What the indexer learned, reduced to what the decision needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MountFacts {
    /// Exact for a seekable container, whose index carries every member's
    /// uncompressed size. For a compressed tar this is the *lower bound* — the
    /// archive's own size, since a stream never shrinks — because the real total
    /// isn't knowable until the whole thing has been decompressed, which is the
    /// very pass we're deciding whether to run.
    pub total_uncompressed: u64,
    /// Whether `total_uncompressed` is the real number or a lower bound.
    pub size_is_exact: bool,
    pub compressed_size: u64,
    /// Members that made it into the index.
    pub entries: usize,
    /// Members dropped for unsafe names.
    pub skipped: usize,
    /// Free bytes on the staging filesystem, when known.
    pub free_space: Option<u64>,
    /// True when mounting means extracting the whole archive (a compressed tar,
    /// which can't be read piecemeal). False for a seekable container, where
    /// mounting costs no disk at all.
    pub needs_extraction: bool,
}

impl MountFacts {
    /// Facts for the check that runs *before* a streamed mount decompresses
    /// anything.
    ///
    /// A compressed tar reveals its member count and real size only to the pass
    /// that extracts it, so the only figure available up front is the archive's
    /// own size — a floor, since a stream never shrinks. `entries` is 1 because
    /// the count is genuinely unknown here and an empty archive is caught after
    /// the pass, where it can be counted rather than guessed.
    pub const fn preflight_streamed(compressed_size: u64, free_space: Option<u64>) -> Self {
        Self {
            total_uncompressed: compressed_size,
            size_is_exact: false,
            compressed_size,
            entries: 1,
            skipped: 0,
            free_space,
            needs_extraction: true,
        }
    }

    /// Message prefix that keeps a lower-bound size from reading as the real one.
    const fn at_least(&self) -> &'static str {
        if self.size_is_exact { "" } else { "at least " }
    }

    /// A bomb ratio can only be judged from a real uncompressed total.
    const fn ratio_is_knowable(&self) -> bool {
        self.size_is_exact && self.compressed_size > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountDecision {
    /// Mount straight away.
    Proceed,
    /// Ask first, showing this reason.
    Confirm(String),
    /// Don't mount; say why.
    Refuse(String),
}

/// Decide whether to mount, given what the index says it would cost.
///
/// Refusals win over confirmations, so an archive that is both a bomb and over
/// budget is refused rather than offered. A seekable container reaches
/// [`MountDecision::Proceed`] however large it is — nothing is extracted until
/// something is read, which is the whole point of indexing instead of unpacking.
pub fn decide_mount(facts: &MountFacts, limits: &BudgetLimits) -> MountDecision {
    if facts.entries == 0 {
        return MountDecision::Refuse(if facts.skipped > 0 {
            format!("every member ({}) has an unsafe path", facts.skipped)
        } else {
            "archive is empty".to_string()
        });
    }

    if facts.needs_extraction {
        if facts.total_uncompressed > limits.extract_budget {
            return MountDecision::Refuse(format!(
                "{}{} to extract, over the {} budget — raise [archive] extract_budget_mb to mount it",
                facts.at_least(),
                format_size(facts.total_uncompressed),
                format_size(limits.extract_budget),
            ));
        }
        if let Some(free) = facts.free_space
            && facts
                .total_uncompressed
                .saturating_add(limits.free_space_margin)
                > free
        {
            return MountDecision::Refuse(format!(
                "{}{} to extract, only {} free",
                facts.at_least(),
                format_size(facts.total_uncompressed),
                format_size(free),
            ));
        }
    }

    if is_bomb(facts, limits) {
        return MountDecision::Confirm(format!(
            "expands {}× to {} — mount anyway?",
            facts.total_uncompressed / facts.compressed_size.max(1),
            format_size(facts.total_uncompressed),
        ));
    }

    if facts.needs_extraction && facts.total_uncompressed > limits.warn_over {
        return MountDecision::Confirm(format!(
            "extracting {}{} — mount anyway?",
            facts.at_least(),
            format_size(facts.total_uncompressed)
        ));
    }

    MountDecision::Proceed
}

/// A suspicious expansion ratio, ignored for archives too small to matter — a
/// 40-byte zip of 40 KB of zeroes is a 1000× ratio and completely harmless.
const fn is_bomb(facts: &MountFacts, limits: &BudgetLimits) -> bool {
    facts.ratio_is_knowable()
        && facts.total_uncompressed > limits.warn_over
        && facts.total_uncompressed / facts.compressed_size > limits.ratio_limit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> MountFacts {
        MountFacts {
            total_uncompressed: 10 * MIB,
            size_is_exact: true,
            compressed_size: 5 * MIB,
            entries: 100,
            skipped: 0,
            free_space: Some(100 * 1024 * MIB),
            needs_extraction: false,
        }
    }

    #[test]
    fn an_ordinary_archive_mounts_without_asking() {
        assert_eq!(
            decide_mount(&facts(), &BudgetLimits::default()),
            MountDecision::Proceed
        );
    }

    /// The payoff for indexing instead of unpacking: a seekable container mounts
    /// instantly at any size, because mounting costs no disk.
    #[test]
    fn a_huge_seekable_archive_still_mounts_immediately() {
        let f = MountFacts {
            total_uncompressed: 40 * 1024 * MIB,
            compressed_size: 20 * 1024 * MIB,
            needs_extraction: false,
            ..facts()
        };
        assert_eq!(
            decide_mount(&f, &BudgetLimits::default()),
            MountDecision::Proceed
        );
    }

    #[test]
    fn a_streaming_mount_confirms_once_it_is_big() {
        let f = MountFacts {
            total_uncompressed: 200 * MIB,
            compressed_size: 100 * MIB,
            needs_extraction: true,
            ..facts()
        };
        let d = decide_mount(&f, &BudgetLimits::default());
        assert!(
            matches!(d, MountDecision::Confirm(ref m) if m.contains("extracting")),
            "{d:?}"
        );
    }

    #[test]
    fn a_streaming_mount_over_budget_is_refused_with_the_knob_named() {
        let f = MountFacts {
            total_uncompressed: 900 * MIB,
            compressed_size: 400 * MIB,
            needs_extraction: true,
            ..facts()
        };
        let MountDecision::Refuse(msg) = decide_mount(&f, &BudgetLimits::default()) else {
            panic!("over-budget streaming mount must be refused");
        };
        assert!(msg.contains("extract_budget_mb"), "{msg}");
    }

    #[test]
    fn a_streaming_mount_is_refused_when_the_disk_cannot_hold_it() {
        let f = MountFacts {
            total_uncompressed: 200 * MIB,
            compressed_size: 100 * MIB,
            needs_extraction: true,
            free_space: Some(210 * MIB), // fits, but not with the margin
            ..facts()
        };
        let MountDecision::Refuse(msg) = decide_mount(&f, &BudgetLimits::default()) else {
            panic!("must refuse when free space is short");
        };
        assert!(msg.contains("free"), "{msg}");
    }

    #[test]
    fn unknown_free_space_does_not_block_a_mount() {
        let f = MountFacts {
            total_uncompressed: 10 * MIB,
            needs_extraction: true,
            free_space: None,
            ..facts()
        };
        assert_eq!(
            decide_mount(&f, &BudgetLimits::default()),
            MountDecision::Proceed
        );
    }

    #[test]
    fn a_bomb_ratio_asks_first_even_when_seekable() {
        let f = MountFacts {
            total_uncompressed: 4096 * MIB,
            compressed_size: 4 * MIB,
            needs_extraction: false,
            ..facts()
        };
        let d = decide_mount(&f, &BudgetLimits::default());
        assert!(
            matches!(d, MountDecision::Confirm(ref m) if m.contains('×')),
            "{d:?}"
        );
    }

    /// A tiny archive with an absurd ratio is not a bomb — it's a file of zeroes.
    #[test]
    fn a_small_archive_with_a_wild_ratio_is_left_alone() {
        let f = MountFacts {
            total_uncompressed: 40 * 1024,
            compressed_size: 40,
            ..facts()
        };
        assert_eq!(
            decide_mount(&f, &BudgetLimits::default()),
            MountDecision::Proceed
        );
    }

    /// Refusal outranks confirmation: an over-budget bomb is not something to
    /// offer the user.
    #[test]
    fn over_budget_beats_the_bomb_confirmation() {
        let f = MountFacts {
            total_uncompressed: 4096 * MIB,
            compressed_size: 4 * MIB,
            needs_extraction: true,
            ..facts()
        };
        assert!(matches!(
            decide_mount(&f, &BudgetLimits::default()),
            MountDecision::Refuse(_)
        ));
    }

    #[test]
    fn an_empty_archive_is_refused_and_says_so() {
        let f = MountFacts {
            entries: 0,
            ..facts()
        };
        assert_eq!(
            decide_mount(&f, &BudgetLimits::default()),
            MountDecision::Refuse("archive is empty".to_string())
        );
    }

    /// An archive whose every member was a traversal attempt would mount as an
    /// empty tree, which tells the user nothing. Name the real reason.
    #[test]
    fn an_all_unsafe_archive_is_refused_for_the_right_reason() {
        let f = MountFacts {
            entries: 0,
            skipped: 12,
            ..facts()
        };
        let MountDecision::Refuse(msg) = decide_mount(&f, &BudgetLimits::default()) else {
            panic!("must refuse");
        };
        assert!(msg.contains("unsafe path"), "{msg}");
        assert!(msg.contains("12"), "{msg}");
    }

    /// A compressed tar's real size isn't knowable before the pass that would
    /// extract it, so the decision runs on the archive's own size as a floor —
    /// and the wording has to say so rather than state a number as fact.
    #[test]
    fn a_lower_bound_size_is_reported_as_a_lower_bound() {
        let f = MountFacts {
            total_uncompressed: 200 * MIB,
            size_is_exact: false,
            compressed_size: 200 * MIB,
            needs_extraction: true,
            ..facts()
        };
        let MountDecision::Confirm(msg) = decide_mount(&f, &BudgetLimits::default()) else {
            panic!("a big streamed mount asks first");
        };
        assert!(msg.contains("at least"), "{msg}");
    }

    /// An expansion ratio computed from a lower bound would be meaningless, so
    /// the bomb check simply doesn't run for a streamed mount.
    #[test]
    fn a_bomb_ratio_is_not_guessed_from_an_inexact_size() {
        let f = MountFacts {
            total_uncompressed: 300 * MIB,
            size_is_exact: false,
            compressed_size: MIB,
            needs_extraction: true,
            ..facts()
        };
        let d = decide_mount(&f, &BudgetLimits::default());
        assert!(
            matches!(d, MountDecision::Confirm(ref m) if m.contains("at least")),
            "{d:?}"
        );
    }

    /// The pre-flight is a floor, so it must refuse only what is *certainly* too
    /// big and let the in-pass ceiling catch the rest.
    #[test]
    fn the_streamed_preflight_judges_what_it_can_and_defers_the_rest() {
        let limits = BudgetLimits::default();
        // 900 MB compressed cannot possibly extract to less, so it is refused
        // before a single byte is decompressed.
        let big = MountFacts::preflight_streamed(900 * MIB, Some(100 * 1024 * MIB));
        assert!(matches!(
            decide_mount(&big, &limits),
            MountDecision::Refuse(_)
        ));

        // A small archive says nothing about what it expands to; the budget
        // inside the pass is what stops a bomb.
        let small = MountFacts::preflight_streamed(2 * MIB, Some(100 * 1024 * MIB));
        assert_eq!(decide_mount(&small, &limits), MountDecision::Proceed);
    }

    #[test]
    fn a_zero_length_archive_does_not_divide_by_zero() {
        let f = MountFacts {
            total_uncompressed: 200 * MIB,
            compressed_size: 0,
            ..facts()
        };
        // No ratio can be computed, so the bomb check simply doesn't fire.
        assert_eq!(
            decide_mount(&f, &BudgetLimits::default()),
            MountDecision::Proceed
        );
    }
}
