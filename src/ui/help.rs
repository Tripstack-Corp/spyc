//! Help overlay — the `?` screen. Hardcoded so it always reflects what
//! the resolver actually binds. When we implement `.cspyrc` in M4 we can
//! regenerate this from the live keymap.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::keymap::UserKeymap;
use crate::ui::theme::Theme;

pub fn render(frame: &mut Frame, area: Rect, theme: &Theme, user_keymap: &UserKeymap) {
    let inner_area = centered_rect(area, 78, 92);

    // Clear the region below the overlay so text beneath doesn't bleed
    // through wherever our content is shorter than the frame.
    frame.render_widget(Clear, inner_area);

    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        "  cspy — key bindings  (press any key to close)  ",
        Style::default()
            .fg(theme.prompt_prefix)
            .add_modifier(Modifier::BOLD),
    ));

    let body_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    let lines = build_lines(theme, user_keymap);
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, body_area);
}

const fn centered_rect(area: Rect, percent_w: u16, percent_h: u16) -> Rect {
    let w = area.width * percent_w / 100;
    let h = area.height * percent_h / 100;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

struct Section {
    title: &'static str,
    rows: &'static [(&'static str, &'static str)],
}

const SECTIONS: &[Section] = &[
    Section {
        title: "Navigation",
        rows: &[
            ("h  ←", "move left one column"),
            ("j  ↓", "move down one entry"),
            ("k  ↑", "move up one entry"),
            ("l  →  Space", "move right one column"),
            ("gg", "first entry"),
            ("G", "last entry"),
            ("^B  PageUp", "previous page"),
            ("^F  PageDown", "next page"),
            ("0–9 <motion>", "count prefix (e.g. 5j, 10k)"),
        ],
    },
    Section {
        title: "Directories & files",
        rows: &[
            ("d  Enter", "enter dir / pager ($PAGER) on text file"),
            ("e  v", "enter dir / editor ($EDITOR) on file"),
            ("u  -", "climb to parent"),
            ("H  ~  Home", "go to home directory"),
            ("J", "jump to a path (~, $VAR expanded)"),
        ],
    },
    Section {
        title: "Picks (per-directory)",
        rows: &[
            ("t", "toggle pick on cursor entry"),
            ("T", "pick by glob pattern (prompt)"),
            ("^T", "pick all / clear all"),
        ],
    },
    Section {
        title: "Inventory (cross-directory)",
        rows: &[
            ("y  Y", "take selection into inventory"),
            ("p", "drop cursor item from inventory"),
            ("i", "toggle inventory view"),
            ("z", "empty inventory"),
        ],
    },
    Section {
        title: "Ignore masks",
        rows: &[
            ("a", "toggle mask 1 (dotfiles by default)"),
            ("o", "toggle mask 2 (build artifacts by default)"),
        ],
    },
    Section {
        title: "File operations",
        rows: &[
            ("c", "copy selection to a destination (prompt)"),
            ("m", "move selection to a destination (prompt)"),
            ("R", "remove selection (confirm with y/yes)"),
            ("M", "make a new directory (prompt)"),
            ("L", "ls -lh on selection through $PAGER"),
            ("f", "file(1) on selection"),
        ],
    },
    Section {
        title: "Shell-out",
        rows: &[
            ("!  ;", "prompt shell command (% = selection)"),
            ("$", "drop into $SHELL in current dir"),
            ("^W", "chmod +w on selection"),
            ("^X", "chmod +x on selection"),
        ],
    },
    Section {
        title: "Search",
        rows: &[
            ("/", "incremental search (prefix, or glob if * ? [)"),
            ("n", "repeat search forward"),
            ("N", "repeat search backward"),
        ],
    },
    Section {
        title: "Meta",
        rows: &[
            ("?  F1", "this help"),
            ("^L", "redraw"),
            ("q  Q  ^D", "quit"),
            ("Esc  Backspace (empty)", "cancel an open prompt"),
        ],
    },
];

fn build_lines(theme: &Theme, user_keymap: &UserKeymap) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let key_style = Style::default()
        .fg(theme.pick)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(theme.status_path);
    let section_style = Style::default()
        .fg(theme.status_user)
        .add_modifier(Modifier::BOLD);

    for (i, section) in SECTIONS.iter().enumerate() {
        if i > 0 {
            out.push(Line::from(""));
        }
        out.push(Line::from(Span::styled(section.title, section_style)));
        for (keys, desc) in section.rows {
            out.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{keys:<24}"), key_style),
                Span::raw("  "),
                Span::styled((*desc).to_string(), desc_style),
            ]));
        }
    }

    // Per-user bindings from .cspyrc.toml, if any.
    let user_bindings: Vec<_> = user_keymap.iter().collect();
    if !user_bindings.is_empty() {
        out.push(Line::from(""));
        out.push(Line::from(Span::styled(
            "Custom (.cspyrc.toml)",
            section_style,
        )));
        for binding in user_bindings {
            out.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{:<24}", binding.chord.display()), key_style),
                Span::raw("  "),
                Span::styled(binding.action.describe(), desc_style),
            ]));
        }
    }

    out
}
