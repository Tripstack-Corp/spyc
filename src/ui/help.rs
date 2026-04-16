//! Help content — the `?` screen. Rendered via the pager so it scrolls
//! and supports search. Content is hardcoded so it always reflects what
//! the resolver actually binds; user bindings from `.cspyrc` are appended.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::keymap::UserKeymap;
use crate::ui::theme::Theme;

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
            ("M", "move selection to a destination (prompt)"),
            ("R", "remove selection (confirm with y)"),
            ("+", "make a new directory (prompt)"),
            ("L", "long listing through $PAGER"),
            ("f", "file(1) on selection"),
            ("^X", "chmod +x on selection"),
        ],
    },
    Section {
        title: "Marks",
        rows: &[
            ("m{a-z}", "set mark at current dir + cursor"),
            ("'{a-z}", "jump to mark (chdir + focus)"),
            ("''", "jump back to previous directory (cd -)"),
            ("`", "jump to starting directory (project root)"),
        ],
    },
    Section {
        title: "Info",
        rows: &[
            ("D", "show date/time (UTC)"),
            ("V", "show cspy version"),
            ("I", "session info (pid, rss, counts)"),
            ("C", "toggle colors / mono"),
            ("s", "set environment variable (NAME=VALUE)"),
        ],
    },
    Section {
        title: "Git",
        rows: &[
            ("g d", "git diff (unstaged changes)"),
            ("g D", "git diff --cached (staged changes)"),
            ("W l", "list worktrees (1-9 to switch)"),
            ("W n", "new worktree (prompt for branch)"),
            ("W d", "delete current worktree (confirm)"),
        ],
    },
    Section {
        title: "Split pane (multi-tab)",
        rows: &[
            ("^\\  F10  ^W \\  ^W c", "toggle the bottom pane (claude)"),
            ("F9", "open pane with claude --resume"),
            ("^W j  ^W k", "switch focus between list and pane"),
            ("^W s", "send selection paths to pane stdin"),
            ("^W p", "pipe file contents of selection to pane"),
            ("^W i", "pipe inventory file contents to pane"),
            ("^W +  ^W -", "grow / shrink the pane"),
            ("^W v", "scroll pane history"),
            ("^W n", "new pane tab (prompt for command + cwd)"),
            ("^W x", "close active pane tab"),
            ("^W 1..9", "switch to tab N"),
            ("^W [  ^W ]", "prev / next tab"),
            ("^W r", "rename active tab"),
            ("Alt+Enter", "newline in pane (multi-line input)"),
            ("CSPY_PANE_CMD", "env var for pane command (default claude)"),
        ],
    },
    Section {
        title: "Shell-out",
        rows: &[
            ("!", "capture output → pager (colors preserved)"),
            (";", "interactive → runs in top pane (top, vim, htop)"),
            ("$", "drop into $SHELL in current dir"),
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
            ("^R", "reload config (auto-reloads on save)"),
            ("q  Q  ^D", "quit"),
            ("Esc (×2)", "cancel prompt (Esc→Normal→Esc→cancel)"),
        ],
    },
];

pub fn build_lines(theme: &Theme, user_keymap: &UserKeymap) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let key_style = Style::default().fg(theme.pick).add_modifier(Modifier::BOLD);
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
