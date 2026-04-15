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

    pub fn move_down(&mut self, n: usize, len: usize) {
        if len == 0 {
            return;
        }
        self.index = (self.index + n).min(len - 1);
    }

    pub fn move_up(&mut self, n: usize) {
        self.index = self.index.saturating_sub(n);
    }

    /// Move across columns in a grid of `columns` columns laid out row-major.
    /// Moving right by n advances index by n; moving left retreats by n;
    /// cap at listing bounds.
    pub fn move_right(&mut self, n: usize, len: usize) {
        self.move_down(n, len);
    }

    pub fn move_left(&mut self, n: usize) {
        self.move_up(n);
    }

    pub fn goto_first(&mut self) {
        self.index = 0;
    }

    pub fn goto_last(&mut self, len: usize) {
        if len == 0 {
            self.index = 0;
        } else {
            self.index = len - 1;
        }
    }
}
