/// Cursor position within a listing: the selected index and the viewport's top row.
#[derive(Debug, Clone, Copy, Default)]
pub struct Cursor {
    /// Flat index into the listing.
    pub index: usize,
    /// Top row shown in the viewport (for scroll).
    pub view_top: usize,
}

impl Cursor {
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn clamp(&mut self, len: usize) {
        if len == 0 {
            self.index = 0;
            self.view_top = 0;
            return;
        }
        if self.index >= len {
            self.index = len - 1;
        }
    }

    // All motion now lives in `App` because it is grid-aware:
    // - `j` / `k` wrap around the end of the flat list
    // - `l` / `h` preserve the row across columns and wrap at the edges
    // - `gg` / `G` jump to the top / bottom of the current column
    //
    // Keeping the math next to the `grid_dims` value means we never use
    // stale geometry to compute a motion.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_zero() {
        let c = Cursor::new();
        assert_eq!(c.index, 0);
        assert_eq!(c.view_top, 0);
    }

    #[test]
    fn clamp_to_empty_list() {
        let mut c = Cursor {
            index: 5,
            view_top: 3,
        };
        c.clamp(0);
        assert_eq!(c.index, 0);
        assert_eq!(c.view_top, 0);
    }

    #[test]
    fn clamp_within_bounds_is_noop() {
        let mut c = Cursor {
            index: 2,
            view_top: 0,
        };
        c.clamp(10);
        assert_eq!(c.index, 2);
    }

    #[test]
    fn clamp_past_end_snaps_to_last() {
        let mut c = Cursor {
            index: 15,
            view_top: 0,
        };
        c.clamp(10);
        assert_eq!(c.index, 9);
    }

    #[test]
    fn clamp_exactly_at_len_snaps() {
        let mut c = Cursor {
            index: 5,
            view_top: 0,
        };
        c.clamp(5);
        assert_eq!(c.index, 4);
    }
}
