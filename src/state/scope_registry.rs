//! P2 merge/scope coordination (`docs/AGENT_AWARENESS_PLAN.md`): the in-memory,
//! session-persisted registry an agent uses to declare what it's touching before
//! a merge, so a concurrent agent can see the overlap and `wait_for_scope_clear`
//! instead of colliding. Lives flat on `AppState` (`Vec<ScopeClaim>`, the same
//! shape as `graveyard`/`marks`) — no daemon, no files, no locking: every agent
//! pane in one spyc shares that spyc's socket, so one registry coordinates them
//! all. **Advisory only** — nothing here blocks a merge; it's data a `list_scopes`
//! read or a `wait_for_scope_clear` call can act on.

use serde::{Deserialize, Serialize};

/// What a claim is for. `Merging` is the state another agent's
/// `wait_for_scope_clear` blocks on; `Editing` is informational (visible on
/// `list_scopes` / the orchestration screen, doesn't block anyone).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScopeIntent {
    Editing,
    Merging,
}

impl ScopeIntent {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "editing" => Some(Self::Editing),
            "merging" => Some(Self::Merging),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Editing => "editing",
            Self::Merging => "merging",
        }
    }
}

/// One agent's declared scope. `owner` is the claiming tab's stable
/// [`crate::pane::tabs::TabInfo::claim_owner`] key (survives `-r`, unlike the
/// pane's ephemeral `id`), so a restored session's claims re-bind to the right
/// respawned tab. `owner_label` is denormalized (the tab's label at claim time)
/// so the registry still reads sensibly if the owning tab has since closed.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScopeClaim {
    pub id: u64,
    pub owner: String,
    pub owner_label: String,
    /// Glob patterns (`glob::Pattern` syntax) or literal paths — see
    /// [`paths_conflict`] for the (deliberately approximate) overlap rule.
    pub paths: Vec<String>,
    pub intent: ScopeIntent,
    pub pr: Option<String>,
    pub note: Option<String>,
    pub claimed_at_secs: u64,
}

/// Whether two path/glob sets overlap. Exact-string equality always counts;
/// beyond that, a pattern in one side matching a literal string on the other
/// (via `glob::Pattern`, either direction) counts too. This is a deliberate
/// approximation — true glob-vs-glob intersection is a much heavier problem —
/// appropriate for an **advisory** coordination signal, not a correctness gate.
/// An unparseable pattern (e.g. malformed glob syntax) never matches anything.
pub fn paths_conflict(a: &[String], b: &[String]) -> bool {
    a.iter().any(|pa| {
        b.iter().any(|pb| {
            pa == pb
                || glob::Pattern::new(pa).is_ok_and(|p| p.matches(pb))
                || glob::Pattern::new(pb).is_ok_and(|p| p.matches(pa))
        })
    })
}

/// The claims that would block `owner` from proceeding on `paths` — every
/// `Merging` claim from a **different** owner whose paths overlap. `owner`'s
/// own claims never conflict with themselves (re-registering / widening your
/// own scope isn't a collision).
pub fn conflicts<'a>(
    claims: &'a [ScopeClaim],
    owner: &str,
    paths: &[String],
) -> Vec<&'a ScopeClaim> {
    claims
        .iter()
        .filter(|c| c.owner != owner && c.intent == ScopeIntent::Merging)
        .filter(|c| paths_conflict(&c.paths, paths))
        .collect()
}

/// The outcome of a `wait_for_scope_clear` check at one instant — the pure core
/// of `settle_scope_waiters`, kept over `bool`s so it's trivially testable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitOutcome {
    /// No blocking conflict remains — resume the caller.
    Cleared,
    /// The deadline passed while a conflict still stood.
    TimedOut,
}

/// Resolve a parked waiter: `Cleared` once its scope has no blocking conflict,
/// `TimedOut` if the deadline passed first, else `None` (keep waiting). A scope
/// that clears exactly as the deadline hits reports `Cleared` — the good outcome
/// wins the tie (conflict is checked first).
pub const fn wait_outcome(has_conflict: bool, past_deadline: bool) -> Option<WaitOutcome> {
    if !has_conflict {
        Some(WaitOutcome::Cleared)
    } else if past_deadline {
        Some(WaitOutcome::TimedOut)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(owner: &str, paths: &[&str], intent: ScopeIntent) -> ScopeClaim {
        ScopeClaim {
            id: 1,
            owner: owner.to_string(),
            owner_label: owner.to_string(),
            paths: paths.iter().copied().map(String::from).collect(),
            intent,
            pr: None,
            note: None,
            claimed_at_secs: 0,
        }
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().copied().map(String::from).collect()
    }

    #[test]
    fn scope_intent_parses_known_strings_and_rejects_unknown() {
        assert_eq!(ScopeIntent::parse("editing"), Some(ScopeIntent::Editing));
        assert_eq!(ScopeIntent::parse("merging"), Some(ScopeIntent::Merging));
        assert_eq!(ScopeIntent::parse("MERGING"), None);
        assert_eq!(ScopeIntent::parse(""), None);
    }

    #[test]
    fn paths_conflict_on_exact_match() {
        assert!(paths_conflict(
            &strs(&["src/app/session.rs"]),
            &strs(&["src/app/session.rs", "Cargo.toml"])
        ));
        assert!(!paths_conflict(
            &strs(&["src/app/session.rs"]),
            &strs(&["src/app/mcp.rs"])
        ));
    }

    #[test]
    fn paths_conflict_on_glob_either_direction() {
        // A's glob matches one of B's literals.
        assert!(paths_conflict(
            &strs(&["src/app/*.rs"]),
            &strs(&["src/app/session.rs"])
        ));
        // B's glob matches one of A's literals (symmetric).
        assert!(paths_conflict(
            &strs(&["src/app/session.rs"]),
            &strs(&["src/app/*.rs"])
        ));
        assert!(!paths_conflict(
            &strs(&["src/app/*.rs"]),
            &strs(&["src/state/marks.rs"])
        ));
    }

    #[test]
    fn paths_conflict_empty_sets_never_conflict() {
        assert!(!paths_conflict(&[], &strs(&["anything"])));
        assert!(!paths_conflict(&strs(&["anything"]), &[]));
    }

    #[test]
    fn conflicts_ignores_own_claims_and_editing_intent() {
        let claims = vec![
            // Same owner, overlapping, Merging — not a conflict (it's yours).
            claim("agent-a", &["src/app/session.rs"], ScopeIntent::Merging),
            // Different owner, overlapping, but only Editing — not blocking.
            claim("agent-b", &["src/app/session.rs"], ScopeIntent::Editing),
            // Different owner, overlapping, Merging — THE conflict.
            claim("agent-c", &["src/app/session.rs"], ScopeIntent::Merging),
            // Different owner, Merging, no overlap — not a conflict.
            claim("agent-d", &["src/state/marks.rs"], ScopeIntent::Merging),
        ];
        let hits = conflicts(&claims, "agent-a", &strs(&["src/app/session.rs"]));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].owner, "agent-c");
    }

    #[test]
    fn conflicts_empty_registry_is_empty() {
        assert!(conflicts(&[], "agent-a", &strs(&["x"])).is_empty());
    }

    #[test]
    fn wait_outcome_cleared_beats_timeout_and_keeps_waiting_otherwise() {
        // No conflict → cleared, regardless of the deadline.
        assert_eq!(wait_outcome(false, false), Some(WaitOutcome::Cleared));
        assert_eq!(wait_outcome(false, true), Some(WaitOutcome::Cleared));
        // Conflict persists past the deadline → timed out.
        assert_eq!(wait_outcome(true, true), Some(WaitOutcome::TimedOut));
        // Conflict, still within the window → keep waiting.
        assert_eq!(wait_outcome(true, false), None);
    }
}
