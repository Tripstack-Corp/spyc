//! Pure frame partitions; every renderer and input router shares these offsets.

use super::{App, FrameLayout};
use crate::config::StatusPosition;
use ratatui::layout::Rect;

impl App {
    /// Partition the frame into status/list/prompt rects — plus, when
    /// the pane is open, a divider row and the pane rect below it.
    ///
    /// The **entire spyc unit** (status, list, prompt) lives above the
    /// divider. That way the prompt row sits with the file list it's
    /// about rather than attached to the bottom of the screen where the
    /// pane's subprocess is typing.
    pub fn compute_layout(
        area: ratatui::layout::Rect,
        pane_open: bool,
        pane_pct: u16,
        status_position: StatusPosition,
    ) -> FrameLayout {
        use ratatui::layout::Rect;
        let w = area.width;
        let h = area.height;
        let bottom_status = matches!(status_position, StatusPosition::Bottom);

        if !pane_open {
            return no_pane_layout(area, bottom_status);
        }

        // With pane: top unit holds list+prompt(+status if top).
        // Pane and divider sit below; if status is bottom, status is the
        // very last row, prompt one above, pane above that.
        let usable = h.saturating_sub(1); // minus divider
        let pane_h = (u32::from(usable) * u32::from(pane_pct) / 100) as u16;
        let top_h = usable.saturating_sub(pane_h);

        if bottom_status {
            return bottom_pane_layout(area, pane_pct);
        }

        // BottomPane zoom (`pane_pct >= 100` — the only value `effective_pane_pct`
        // ever returns ≥ 100, and `pane_height_pct` is clamped to ≤ 90): the
        // pane fills everything below a single spyc status line. That one row
        // carries the status bar normally and flips to flash / chord-arming /
        // prompt when active (they share the row — `render_inner` draws one or
        // the other), so a zoomed session still surfaces arming + messages
        // while leaving the pane's own input line uncluttered at the bottom.
        // (The bottom-status path reserves its chrome even at pct 100, so only
        // this top-status path collapsed it — hence the targeted branch.)
        if pane_pct >= 100 {
            return zoomed_pane_layout(area);
        }

        // Top status (default): [status][list…][prompt][divider][pane]
        let status = Rect {
            x: area.x,
            y: area.y,
            width: w,
            height: 1.min(top_h),
        };
        let list_h = top_h.saturating_sub(2);
        let list = Rect {
            x: area.x,
            y: area.y + status.height,
            width: w,
            height: list_h,
        };
        let prompt = Rect {
            x: area.x,
            y: area.y + top_h.saturating_sub(1),
            width: w,
            height: u16::from(top_h >= 2),
        };

        let divider = Rect {
            x: area.x,
            y: area.y + top_h,
            width: w,
            height: 1,
        };
        // No pane rect when it has no height — `TopList` zoom (pct 0) leaves
        // only the bottom tab-bar divider visible; the pty runs off-screen.
        let pane = (pane_h > 0).then_some(Rect {
            x: area.x,
            y: divider.y + 1,
            width: w,
            height: pane_h,
        });

        FrameLayout {
            status,
            list,
            divider: Some(divider),
            pane,
            prompt,
            // Top status + pane: status+list+prompt are contiguous above the
            // divider (== top_h rows starting at area.y).
            top_unit: Rect {
                x: area.x,
                y: area.y,
                width: w,
                height: top_h,
            },
            agent_hud: None,
            right: None,
            vdivider: None,
        }
    }
}

fn no_pane_layout(area: Rect, bottom_status: bool) -> FrameLayout {
    let (w, h) = (area.width, area.height);
    // Top:    [status][list…][prompt]
    // Bottom: [list…][prompt][status]   (vim-style)
    let (status_y, list_y, prompt_y) = if bottom_status {
        (
            area.y + h.saturating_sub(1),
            area.y,
            area.y + h.saturating_sub(2),
        )
    } else {
        (area.y, area.y + 1.min(h), area.y + h.saturating_sub(1))
    };
    let status = Rect {
        x: area.x,
        y: status_y,
        width: w,
        height: 1.min(h),
    };
    let list = Rect {
        x: area.x,
        y: list_y,
        width: w,
        height: h.saturating_sub(2),
    };
    let prompt = Rect {
        x: area.x,
        y: prompt_y,
        width: w,
        height: u16::from(h != 0),
    };
    FrameLayout {
        status,
        list,
        divider: None,
        pane: None,
        prompt,
        // No pane: the spyc unit is the whole frame, regardless of
        // where the status row sits within it.
        top_unit: Rect {
            x: area.x,
            y: area.y,
            width: w,
            height: h,
        },
        agent_hud: None,
        right: None,
        vdivider: None,
    }
}

fn bottom_pane_layout(area: Rect, pane_pct: u16) -> FrameLayout {
    let (w, h) = (area.width, area.height);
    // Layout (top → bottom): [list…][divider][pane…][prompt][status]
    // Reserve: 1 divider + 1 prompt + 1 status = 3 rows of chrome.
    // The remainder splits between list and pane by `pane_pct`.
    let chrome = 3u16;
    let usable_b = h.saturating_sub(chrome);
    let pane_h_b = (u32::from(usable_b) * u32::from(pane_pct) / 100) as u16;
    let list_h = usable_b.saturating_sub(pane_h_b);

    let list = Rect {
        x: area.x,
        y: area.y,
        width: w,
        height: list_h,
    };
    let divider = Rect {
        x: area.x,
        y: area.y + list_h,
        width: w,
        height: 1,
    };
    // No pane rect when it has no height — `TopList` zoom (pct 0)
    // leaves only the tab-bar divider; the pty runs off-screen.
    let pane = (pane_h_b > 0).then_some(Rect {
        x: area.x,
        y: divider.y + 1,
        width: w,
        height: pane_h_b,
    });
    let prompt = Rect {
        x: area.x,
        y: area.y + h.saturating_sub(2),
        width: w,
        height: u16::from(h >= 2),
    };
    let status = Rect {
        x: area.x,
        y: area.y + h.saturating_sub(1),
        width: w,
        height: 1.min(h),
    };
    FrameLayout {
        status,
        list,
        divider: Some(divider),
        pane,
        prompt,
        // Bottom status + pane: only the list sits above the divider;
        // prompt and status are below the pane. A top overlay occupies
        // just the list region.
        top_unit: Rect {
            x: area.x,
            y: area.y,
            width: w,
            height: list_h,
        },
        agent_hud: None,
        right: None,
        vdivider: None,
    }
}

fn zoomed_pane_layout(area: Rect) -> FrameLayout {
    let (w, h) = (area.width, area.height);
    let status = Rect {
        x: area.x,
        y: area.y,
        width: w,
        height: 1.min(h),
    };
    let divider = Rect {
        x: area.x,
        y: area.y + status.height,
        width: w,
        height: 1.min(h.saturating_sub(status.height)),
    };
    let pane = Rect {
        x: area.x,
        y: divider.y + divider.height,
        width: w,
        height: h.saturating_sub(status.height + divider.height),
    };
    FrameLayout {
        status,
        // No file list while the pane is zoomed.
        list: Rect {
            x: area.x,
            y: pane.y,
            width: w,
            height: 0,
        },
        divider: Some(divider),
        pane: Some(pane),
        // The flash / arming / prompt line shares the single top row.
        prompt: status,
        top_unit: status,
        agent_hud: None,
        right: None,
        vdivider: None,
    }
}

/// The rows of the vertical split's divider that border where the keys go, as
/// a range of `y`. `focus` is the focused column, or `None` while the pane has
/// the keys; `pane_top` is the pane's first row, its rule included.
///
/// A column owns every row it touches: the right column runs beside the whole
/// divider in either mode, and so does the left one when only the list is
/// split. In a full-height split the pane sits under the left column, so the
/// left column owns the rows above `pane_top` and the pane those from it down.
/// A pane below a top-only split touches no row of the divider.
pub(super) fn vsplit_divider_lit_rows(
    mode: crate::app::state::VsplitMode,
    divider: Rect,
    pane_top: Option<u16>,
    focus: Option<crate::app::state::Side>,
) -> std::ops::Range<u16> {
    use crate::app::state::{Side, VsplitMode};
    let (top, bottom) = (divider.y, divider.y.saturating_add(divider.height));
    let split_at = pane_top.map_or(bottom, |y| y.clamp(top, bottom));
    match (focus, mode) {
        (Some(Side::Right), _) | (Some(Side::Left), VsplitMode::TopOnly) => top..bottom,
        (Some(Side::Left), VsplitMode::FullHeight) => top..split_at,
        (None, VsplitMode::FullHeight) => split_at..bottom,
        (None, VsplitMode::TopOnly) => top..top,
    }
}

#[cfg(test)]
mod vsplit_divider_tests {
    use super::vsplit_divider_lit_rows as lit;
    use crate::app::state::{Side, VsplitMode};
    use ratatui::layout::Rect;

    /// A divider from row 0 to row 23, the pane's rule at row 14.
    const FULL: Rect = Rect {
        x: 40,
        y: 0,
        width: 1,
        height: 24,
    };
    /// A divider beside the list only: rows 1 to 12.
    const TOP: Rect = Rect {
        x: 40,
        y: 1,
        width: 1,
        height: 12,
    };

    #[test]
    fn the_right_column_lights_the_whole_divider() {
        assert_eq!(
            lit(VsplitMode::FullHeight, FULL, Some(14), Some(Side::Right)),
            0..24
        );
        assert_eq!(
            lit(VsplitMode::TopOnly, TOP, Some(14), Some(Side::Right)),
            1..13
        );
    }

    #[test]
    fn a_full_height_split_shares_the_divider_between_the_left_column_and_the_pane() {
        assert_eq!(
            lit(VsplitMode::FullHeight, FULL, Some(14), Some(Side::Left)),
            0..14
        );
        assert_eq!(lit(VsplitMode::FullHeight, FULL, Some(14), None), 14..24);
    }

    #[test]
    fn with_no_pane_the_left_column_lights_the_whole_divider() {
        assert_eq!(
            lit(VsplitMode::FullHeight, FULL, None, Some(Side::Left)),
            0..24
        );
        assert_eq!(lit(VsplitMode::TopOnly, TOP, None, Some(Side::Left)), 1..13);
    }

    #[test]
    fn a_pane_below_a_top_only_split_lights_nothing() {
        assert!(lit(VsplitMode::TopOnly, TOP, Some(14), None).is_empty());
    }

    #[test]
    fn a_pane_top_outside_the_divider_is_clamped_to_it() {
        assert_eq!(
            lit(VsplitMode::FullHeight, FULL, Some(30), Some(Side::Left)),
            0..24
        );
        assert!(lit(VsplitMode::FullHeight, FULL, Some(30), None).is_empty());
    }
}
