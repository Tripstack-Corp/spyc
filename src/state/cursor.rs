/// Cursor position within a listing, plus the grid width used for h/l motion.
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

    pub fn clamp(&mut self, len: usize) {
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
    // Keeping the math next to the `last_grid` value means we never use
    // stale geometry to compute a motion.
}
