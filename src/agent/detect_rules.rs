//! P1-2 scrape fallback (`docs/archive/AGENT_AWARENESS_PLAN.md`): a declarative,
//! data-driven detection ruleset for agents that don't self-report over the
//! MCP/hook channel — the tunable fallback spyc's P1-1 self-report path was
//! always meant to have, never the primary path (that's herdr's fragility
//! class; see the plan's "Explicitly OUT of scope").
//!
//! Kept deliberately small: one [`Region`] (the bottom of the pane's recent
//! text, where an interactive prompt renders) and one [`Matcher`] shape
//! (substring). Extend with `Regex` / composition (`Any`/`All`/`Not`) / more
//! regions only when a real rule needs one — no speculative surface.

use crate::pane::AgentActivity;

/// Which slice of a pane's recent text a [`DetectionRule`] inspects. Derived
/// from the same vt100 screen text `gf`/quick-select already read
/// (`Pane::recent_lines`) — no new OSC-sequence tracking.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum Region {
    /// The last `n` non-empty lines of the pane's recent output, newest first
    /// collapsed into one block — where a confirmation prompt almost always
    /// renders.
    BottomNonEmptyLines(usize),
}

/// Text matcher for a [`DetectionRule`]. An enum (not a bare `&str`) so it's the
/// extension point for `Regex` / `Any` / `All` / `Not` when a real rule needs
/// composition — none do today, so `Contains` is the only variant.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum Matcher {
    /// A plain substring match (case-sensitive — agent prompt text is stable
    /// enough that this beats a regex dependency for the rules that exist
    /// today).
    Contains(&'static str),
    /// Matches if the region ends with this string (ignoring trailing whitespace).
    EndsWith(&'static str),
}

impl Matcher {
    fn matches(self, haystack: &str) -> bool {
        match self {
            Self::Contains(needle) => haystack.contains(needle),
            Self::EndsWith(needle) => haystack.trim_end().ends_with(needle),
        }
    }
}

/// One priority-ordered rule in an [`crate::agent::AgentProfile::detection_rules`]
/// ruleset: scan `region`, and if `matcher` matches, the tab's scrape-inferred
/// status is `state`. `visible_blocker` is the human-readable reason surfaced
/// by `:why-status` / `:activity dump`.
#[derive(Debug, Clone, Copy)]
pub struct DetectionRule {
    pub region: Region,
    pub matcher: Matcher,
    pub state: AgentActivity,
    pub visible_blocker: Option<&'static str>,
}

fn region_text(lines: &[String], region: Region) -> String {
    match region {
        Region::BottomNonEmptyLines(n) => {
            let mut chunk: Vec<&str> = lines
                .iter()
                .rev()
                .map(String::as_str)
                .filter(|l| !l.trim().is_empty())
                .take(n)
                .collect();
            chunk.reverse();
            chunk.join("\n")
        }
    }
}

/// Scan `lines` (a pane's recent text) against `rules` in priority order,
/// returning the first match's `(state, visible_blocker)`. Pure — no I/O, no
/// clock, no hysteresis (the caller, [`crate::app::agent_status`], debounces
/// across calls so one transient line of scrolling text can't flip a dot).
pub fn scan(
    lines: &[String],
    rules: &[DetectionRule],
) -> Option<(AgentActivity, Option<&'static str>)> {
    rules.iter().find_map(|rule| {
        let text = region_text(lines, rule.region);
        rule.matcher
            .matches(&text)
            .then_some((rule.state, rule.visible_blocker))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &[&str]) -> Vec<String> {
        s.iter().copied().map(String::from).collect()
    }

    #[test]
    fn bottom_non_empty_lines_skips_blanks_and_caps_count() {
        let l = lines(&["one", "", "two", "", "", "three", "four"]);
        assert_eq!(
            region_text(&l, Region::BottomNonEmptyLines(2)),
            "three\nfour"
        );
        // Fewer non-empty lines than requested ⇒ just what's there, in order.
        assert_eq!(
            region_text(&l, Region::BottomNonEmptyLines(100)),
            "one\ntwo\nthree\nfour"
        );
    }

    #[test]
    fn contains_matches_substring_anywhere_in_region() {
        let m = Matcher::Contains("Do you want to proceed?");
        assert!(m.matches("blah\nDo you want to proceed?\nblah"));
        assert!(!m.matches("nothing interesting here"));
    }

    #[test]
    fn ends_with_matches_at_end_ignoring_whitespace() {
        let m = Matcher::EndsWith("esc to cancel");
        assert!(m.matches("blah\nesc to cancel"));
        assert!(m.matches("blah\nesc to cancel\n  \n"));
        assert!(!m.matches("esc to cancel\nblah"));
    }

    #[test]
    fn scan_returns_first_matching_rule_in_priority_order() {
        let rules = &[
            DetectionRule {
                region: Region::BottomNonEmptyLines(5),
                matcher: Matcher::Contains("Do you want to proceed?"),
                state: AgentActivity::Blocked,
                visible_blocker: Some("awaiting tool-execution approval"),
            },
            DetectionRule {
                region: Region::BottomNonEmptyLines(5),
                matcher: Matcher::Contains("anything"),
                state: AgentActivity::Working,
                visible_blocker: None,
            },
        ];
        let l = lines(&["Do you want to proceed?", "anything else"]);
        // First rule wins even though the second would also match.
        assert_eq!(
            scan(&l, rules),
            Some((
                AgentActivity::Blocked,
                Some("awaiting tool-execution approval")
            ))
        );
    }

    #[test]
    fn scan_returns_none_when_no_rule_matches() {
        let rules = &[DetectionRule {
            region: Region::BottomNonEmptyLines(5),
            matcher: Matcher::Contains("Do you want to proceed?"),
            state: AgentActivity::Blocked,
            visible_blocker: None,
        }];
        assert_eq!(scan(&lines(&["nothing relevant"]), rules), None);
    }

    #[test]
    fn scan_empty_ruleset_is_none() {
        assert_eq!(scan(&lines(&["Do you want to proceed?"]), &[]), None);
    }
}
