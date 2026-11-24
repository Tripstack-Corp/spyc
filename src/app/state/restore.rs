//! Pure bookkeeping for a session restore: the tabs it could not open, and
//! what a column showed in its directory (its cursor, picks and filter).

use crate::app::View;
use crate::state::sessions::{SavedColumn, SavedTab};
use std::hash::{Hash, Hasher};

use super::{AppState, Side};

impl AppState {
    /// What column `side` shows in its directory, as a save keeps it: the file
    /// under its cursor, its picks, sorted, and its filter. `None` when there
    /// is no such column, or nothing to keep. A name or pick that isn't UTF-8
    /// is left out, since the session file can't hold it.
    pub fn saved_column(&self, side: Side) -> Option<SavedColumn> {
        let col = self.get_col(side)?;
        let cursor = (col.view == View::Dir)
            .then(|| col.rows.get(col.cursor.index))
            .flatten()
            .and_then(|row| row.path.file_name()?.to_str())
            .map(str::to_owned);
        let mut picks: Vec<_> = col
            .picks
            .iter()
            .filter(|p| p.to_str().is_some())
            .cloned()
            .collect();
        picks.sort();
        let saved = SavedColumn {
            cursor,
            picks,
            filter: col.temp_filter.clone(),
        };
        (saved != SavedColumn::default()).then_some(saved)
    }

    /// Put back what column `side` showed in its directory, once it is there
    /// again: the picks still listed, the filter, and the cursor on its file
    /// while that file is still listed.
    pub fn restore_column(&mut self, side: Side, saved: &SavedColumn) {
        let col = self.col_mut(side);
        for pick in &saved.picks {
            if col.listing.entries.iter().any(|e| e.path == *pick) {
                col.picks.insert(pick);
            }
        }
        col.temp_filter.clone_from(&saved.filter);
        self.rebuild_rows_for(side);
        let col = self.col_mut(side);
        if let Some(index) = saved.cursor.as_deref().and_then(|name| {
            col.rows
                .iter()
                .position(|row| row.path.file_name().is_some_and(|n| n == name))
        }) {
            col.cursor.index = index;
        }
    }

    /// What of column `side` a save keeps that the autosave watches for: its
    /// picks and filter. Not its cursor, which moves with every key.
    pub fn hash_column(&self, side: Side, hasher: &mut impl Hasher) {
        if let Some(col) = self.get_col(side) {
            col.picks
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .hash(hasher);
            col.temp_filter.hash(hasher);
        }
    }
}

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
        live.insert(
            index,
            SavedTab {
                unopened: true,
                ..tab.tab.clone()
            },
        );
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
        tab.pane_id.hash(hasher);
        tab.agent_hint
            .as_ref()
            .map(std::mem::discriminant)
            .hash(hasher);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(label: &str) -> SavedTab {
        serde_json::from_value(serde_json::json!({"command": label, "label": label, "cwd": "/tmp"}))
            .expect("fixture")
    }

    /// A record put back is marked unopened: no agent runs it, so it holds no
    /// conversation against another spyc's restore.
    #[test]
    fn records_put_back_are_marked_unopened() {
        let deferred = vec![DeferredTab {
            saved_index: 0,
            tab: tab("kept"),
            reason: "refused".into(),
        }];
        let (tabs, _) = merge_saved_tabs(vec![tab("live")], 0, &deferred);
        let marked: Vec<(&str, bool)> = tabs
            .iter()
            .map(|tab| (tab.label.as_str(), tab.unopened))
            .collect();
        assert_eq!(marked, [("kept", true), ("live", false)]);
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
