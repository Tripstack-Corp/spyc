//! Pure bookkeeping for tabs a session restore could not open.

use crate::state::sessions::SavedTab;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone)]
pub struct DeferredTab {
    pub saved_index: usize,
    pub tab: SavedTab,
    pub reason: String,
}

/// Restore the selected saved tab, or the next successful tab (then previous).
/// The indices are in spawn order, which is the saved order with gaps.
pub fn restored_active_index(restored: &[usize], saved_active: usize) -> usize {
    restored
        .iter()
        .position(|index| *index >= saved_active)
        .unwrap_or_else(|| restored.len().saturating_sub(1))
}

/// Put unopened records back at their saved positions where possible. Closing
/// live tabs can shorten the list; those records then append, never disappear.
/// Map the selected live tab through each insertion so it stays selected.
pub fn merge_saved_tabs(
    mut live: Vec<SavedTab>,
    live_active: usize,
    deferred: &[DeferredTab],
) -> (Vec<SavedTab>, usize) {
    let has_live = !live.is_empty();
    let mut active = live_active.min(live.len().saturating_sub(1));
    let mut deferred: Vec<_> = deferred.iter().collect();
    deferred.sort_by_key(|tab| tab.saved_index);
    for tab in deferred {
        let index = tab.saved_index.min(live.len());
        if has_live && index <= active {
            active += 1;
        }
        live.insert(index, tab.tab.clone());
    }
    (live, active)
}

/// Hash the metadata that the snapshot persists without resolving conversations
/// or allocating serialized copies during the event loop's autosave check.
pub fn hash_deferred_tabs(deferred: &[DeferredTab], hasher: &mut impl Hasher) {
    deferred.len().hash(hasher);
    for deferred in deferred {
        deferred.saved_index.hash(hasher);
        let tab = &deferred.tab;
        tab.command.hash(hasher);
        tab.label.hash(hasher);
        tab.cwd.hash(hasher);
        std::mem::discriminant(&tab.agent_kind).hash(hasher);
        tab.agent_session_id.hash(hasher);
        tab.agent_session_name.hash(hasher);
        tab.claim_owner.hash(hasher);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(label: &str) -> SavedTab {
        serde_json::from_value(serde_json::json!({"command": label, "label": label, "cwd": "/tmp"}))
            .expect("fixture")
    }

    #[test]
    fn active_tab_maps_past_refusals_and_falls_forward_then_back() {
        assert_eq!(restored_active_index(&[1, 3, 4], 1), 0);
        assert_eq!(restored_active_index(&[1, 3, 4], 2), 1);
        assert_eq!(restored_active_index(&[1, 3, 4], 9), 2);
        assert_eq!(restored_active_index(&[], 0), 0);
    }

    #[test]
    fn saved_tabs_keep_gaps_and_active_identity_without_live_index_drift() {
        let deferred = vec![
            DeferredTab {
                saved_index: 3,
                tab: tab("d"),
                reason: "refused".into(),
            },
            DeferredTab {
                saved_index: 0,
                tab: tab("a"),
                reason: "refused".into(),
            },
        ];
        let (tabs, active) = merge_saved_tabs(vec![tab("b"), tab("c"), tab("e")], 1, &deferred);
        assert_eq!(
            tabs.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c", "d", "e"]
        );
        assert_eq!(active, 2);
        let (tabs, active) = merge_saved_tabs(vec![], 0, &deferred);
        assert_eq!(
            tabs.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(),
            ["a", "d"]
        );
        assert_eq!(active, 0);
        let (tabs, active) = merge_saved_tabs(vec![tab("new")], 0, &deferred);
        assert_eq!(
            tabs.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(),
            ["a", "new", "d"]
        );
        assert_eq!(active, 1);
    }
}
