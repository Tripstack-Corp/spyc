use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub struct Panels {
    pub status: Rect,
    pub list: Rect,
    /// Always-reserved single-row slot at the bottom for prompts. Keeping
    /// it permanently allocated (even when no prompt is active) means the
    /// list never reflows when `/`, `!`, etc. is pressed.
    pub prompt: Rect,
}

pub fn split(area: Rect) -> Panels {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);
    Panels {
        status: chunks[0],
        list: chunks[1],
        prompt: chunks[2],
    }
}
