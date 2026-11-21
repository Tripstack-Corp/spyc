//! The panes whose projects left this spyc (`:detach project`), as the MCP
//! socket threads see them. A connection bound to one is closed on its next
//! message, unanswered, so its proxy follows the pane's jar to the spyc that
//! has it now (R30a) and sends the message there. A pane that comes back
//! (`:attach project`) is answered here again.

/// The panes whose projects moved to a session of their own (`:detach
/// project`). A connection bound to one is closed on its next message,
/// unanswered, so its proxy follows the pane's jar to the spyc that has it now
/// (R30a) and sends the message there.
static MOVED_PANES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Mark `panes` moved ([`MOVED_PANES`]).
pub fn panes_moved(panes: impl IntoIterator<Item = String>) {
    MOVED_PANES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .extend(panes);
}

/// Unmark `panes`, back in this spyc (`:attach project`).
pub fn panes_returned(panes: &[String]) {
    MOVED_PANES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|id| !panes.contains(id));
}

/// Whether pane `id` has left this spyc.
pub(super) fn is_moved(id: &str) -> bool {
    MOVED_PANES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .any(|p| p == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_that_moved_and_came_back_is_answered_again() {
        let id = "moved-test-pane-0123".to_string();
        assert!(!is_moved(&id));
        panes_moved([id.clone()]);
        assert!(is_moved(&id));
        panes_returned(std::slice::from_ref(&id));
        assert!(!is_moved(&id));
    }
}
