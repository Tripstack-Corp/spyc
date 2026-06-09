//! Pure modal-sink decision: which transient overlay, if any, is currently
//! swallowing input?
//!
//! Mirrors [`super::focus`] and [`super::route`] — a pure function of a small
//! `Copy` snapshot, no `&App` and no side effects, table-tested without a TUI.
//!
//! [`Modal`] is the **transient** axis of "what owns input", orthogonal to the
//! persistent [`super::state::Focus`] region: a modal eats input regardless of
//! which region is focused, and closing it returns to whatever focus was. The
//! modal's *data* stays bucket-locked (the finder/capture/overlay pty in
//! `Runtime`, the quick-select/harpoon menus in `ViewState`); this names which
//! one is live so the router consults a single typed value instead of five
//! scattered `Option::is_some()` reads. At most one is active at a time.

/// Which transient modal overlay is swallowing input, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Modal {
    /// `F` fuzzy-finder picker (`runtime.find_picker`).
    FindPicker,
    /// A `!` capture child is running (`runtime.pending_capture`).
    Capture,
    /// A top-overlay subprocess has exited and is held awaiting any input to
    /// dismiss it (`view.overlay_awaiting_dismiss` + `runtime.top_overlay`).
    OverlayDismiss,
    /// Quick-select label overlay (`view.quick_select`).
    QuickSelect,
    /// Harpoon menu (`view.harpoon_menu`).
    Harpoon,
}

/// The App-state bits the modal decision reads. `Copy` so tests construct one
/// inline. Each field is the `is_some()` / bool of a backing modal field.
#[derive(Debug, Clone, Copy)]
pub(super) struct ModalSnapshot {
    pub has_find_picker: bool,
    pub has_capture: bool,
    pub overlay_awaiting_dismiss: bool,
    pub has_quick_select: bool,
    pub has_harpoon: bool,
}

/// Decide which [`Modal`] (if any) owns input. **Pure** — no mutation, no I/O.
///
/// Branch order is the precedence the historical `handle_key` pre-check ladder
/// used: finder > capture > overlay-dismiss > quick-select > harpoon. In
/// practice at most one is ever live at once (a single pager slot + the modal
/// open/close discipline), but pinning the order keeps the decision total and
/// the routing deterministic if two were ever set.
pub(super) const fn active_modal(snap: ModalSnapshot) -> Option<Modal> {
    if snap.has_find_picker {
        Some(Modal::FindPicker)
    } else if snap.has_capture {
        Some(Modal::Capture)
    } else if snap.overlay_awaiting_dismiss {
        Some(Modal::OverlayDismiss)
    } else if snap.has_quick_select {
        Some(Modal::QuickSelect)
    } else if snap.has_harpoon {
        Some(Modal::Harpoon)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Snapshot with no modal active.
    const fn none() -> ModalSnapshot {
        ModalSnapshot {
            has_find_picker: false,
            has_capture: false,
            overlay_awaiting_dismiss: false,
            has_quick_select: false,
            has_harpoon: false,
        }
    }

    #[test]
    fn no_modal_is_none() {
        assert_eq!(active_modal(none()), None);
    }

    #[test]
    fn each_flag_maps_to_its_variant() {
        assert_eq!(
            active_modal(ModalSnapshot {
                has_find_picker: true,
                ..none()
            }),
            Some(Modal::FindPicker)
        );
        assert_eq!(
            active_modal(ModalSnapshot {
                has_capture: true,
                ..none()
            }),
            Some(Modal::Capture)
        );
        assert_eq!(
            active_modal(ModalSnapshot {
                overlay_awaiting_dismiss: true,
                ..none()
            }),
            Some(Modal::OverlayDismiss)
        );
        assert_eq!(
            active_modal(ModalSnapshot {
                has_quick_select: true,
                ..none()
            }),
            Some(Modal::QuickSelect)
        );
        assert_eq!(
            active_modal(ModalSnapshot {
                has_harpoon: true,
                ..none()
            }),
            Some(Modal::Harpoon)
        );
    }

    /// Precedence: finder > capture > dismiss > quick-select > harpoon. Set all
    /// flags, then peel them off one at a time and watch the winner advance.
    #[test]
    fn precedence_order() {
        let all = ModalSnapshot {
            has_find_picker: true,
            has_capture: true,
            overlay_awaiting_dismiss: true,
            has_quick_select: true,
            has_harpoon: true,
        };
        assert_eq!(active_modal(all), Some(Modal::FindPicker));
        let a = ModalSnapshot {
            has_find_picker: false,
            ..all
        };
        assert_eq!(active_modal(a), Some(Modal::Capture));
        let b = ModalSnapshot {
            has_capture: false,
            ..a
        };
        assert_eq!(active_modal(b), Some(Modal::OverlayDismiss));
        let c = ModalSnapshot {
            overlay_awaiting_dismiss: false,
            ..b
        };
        assert_eq!(active_modal(c), Some(Modal::QuickSelect));
        let d = ModalSnapshot {
            has_quick_select: false,
            ..c
        };
        assert_eq!(active_modal(d), Some(Modal::Harpoon));
    }
}
