//! Pure focus decision: which [`Focus`] does a directional focus change
//! (`^W j` / `^W k`) select?
//!
//! Mirrors [`super::route`] — a pure function of a small `Copy` snapshot, no
//! `&App` and no side effects, so every branch is unit-testable without a TUI.
//! The test module doubles as the regression pin for the Phase-0 invariant
//! that *every* non-`Pane` arm collapses to `pane_focused() == false`.

use crate::ui::pager::Mount;

use super::state::Focus;

/// The App-state bits the focus decision reads. `Copy` so tests construct one
/// inline.
#[derive(Debug, Clone, Copy)]
pub(super) struct FocusSnapshot {
    /// A top overlay (`V` editor / `;cmd`) pty is alive.
    pub has_top_overlay: bool,
    /// The in-app pager's mount slot, if a pager is open.
    pub pager_mount: Option<Mount>,
}

/// Decide the [`Focus`] for a directional focus change. **Pure** — no mutation,
/// no I/O.
///
/// `want_pane` always selects [`Focus::Pane`]. Otherwise the "non-pane" side is
/// the front-most surface: a top overlay, else the pager (tagged with its
/// mount), else the file list. Branch order is the Phase-0 contract — every
/// non-`Pane` arm yields `Focus::pane_focused() == false`, so the
/// Overlay/Pager distinction is invisible to current consumers (router, render
/// DIM, flash, `^C` gate) and carried only for later MVU phases.
pub(super) const fn decide_focus(snap: FocusSnapshot, want_pane: bool) -> Focus {
    if want_pane {
        Focus::Pane
    } else if snap.has_top_overlay {
        Focus::Overlay
    } else if let Some(mount) = snap.pager_mount {
        Focus::Pager(mount)
    } else {
        Focus::FileList
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(has_top_overlay: bool, pager_mount: Option<Mount>) -> FocusSnapshot {
        FocusSnapshot {
            has_top_overlay,
            pager_mount,
        }
    }

    #[test]
    fn want_pane_always_selects_pane() {
        assert_eq!(decide_focus(snap(false, None), true), Focus::Pane);
        assert_eq!(decide_focus(snap(true, None), true), Focus::Pane);
        assert_eq!(
            decide_focus(snap(true, Some(Mount::TopPane)), true),
            Focus::Pane
        );
    }

    #[test]
    fn non_pane_prefers_overlay_then_pager_then_file_list() {
        // Overlay wins even when a pager is also mounted.
        assert_eq!(
            decide_focus(snap(true, Some(Mount::TopPane)), false),
            Focus::Overlay
        );
        // Pager (tagged with its mount) when no overlay.
        assert_eq!(
            decide_focus(snap(false, Some(Mount::LowerPane)), false),
            Focus::Pager(Mount::LowerPane)
        );
        // File list when nothing else is up.
        assert_eq!(decide_focus(snap(false, None), false), Focus::FileList);
    }

    #[test]
    fn every_non_pane_decision_is_not_pane_focused() {
        for has_top_overlay in [false, true] {
            for pager_mount in [None, Some(Mount::TopPane), Some(Mount::LowerPane)] {
                let decided = decide_focus(snap(has_top_overlay, pager_mount), false);
                assert_ne!(
                    decided,
                    Focus::Pane,
                    "non-pane decision (overlay={has_top_overlay}, pager={pager_mount:?}) \
                     must collapse to pane_focused() == false",
                );
            }
        }
    }
}
