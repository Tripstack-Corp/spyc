//! Help content — the `?` screen. Rendered via the pager so it scrolls
//! and supports search. Content is hardcoded so it always reflects what
//! the resolver actually binds; user bindings from `.spycrc` are appended.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::display_pad_right;
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
            (
                "e  v",
                "enter dir / editor ($EDITOR) on file (suspends TUI)",
            ),
            ("V", "open $EDITOR in top pane (Claude pane stays visible)"),
            ("u  -", "climb to parent"),
            ("~  Home", "go to home directory ($HOME)"),
            ("J", "jump to a path (~, $VAR expanded)"),
            (
                "F",
                "find file (project-wide fuzzy: gitignore-aware walk, type to filter)",
            ),
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
        title: "Inventory & yank",
        rows: &[
            ("y y", "yank file(s) into inventory cache"),
            ("y p", "yank visible pane output to clipboard"),
            ("y P", "yank last typed prompt to clipboard"),
            ("y a", "yank full pane scrollback to clipboard"),
            ("Y", "remove cursor file from inventory"),
            ("p", "put inventory files to current dir"),
            ("i", "toggle inventory view"),
            ("z", "clear inventory (moves to graveyard)"),
        ],
    },
    Section {
        title: "Inventory view (inside i)",
        rows: &[
            ("t  Space", "tag/untag items for partial put"),
            ("p", "put tagged (or all) to cwd"),
            ("x  d", "remove item (to graveyard)"),
            ("ESC  i", "return to directory view"),
        ],
    },
    Section {
        title: "Ignore masks & filtering",
        rows: &[
            ("a", "toggle mask 1 (dotfiles by default)"),
            ("o", "toggle mask 2 (build artifacts by default)"),
            (
                "=",
                "limit — glob (! picks, git/g git changes, h harpoon, empty clears)",
            ),
        ],
    },
    Section {
        title: "File operations",
        rows: &[
            ("c", "copy selection to a destination (prompt)"),
            ("M", "move selection to a destination (prompt)"),
            ("R", "remove selection (confirm with y)"),
            ("+", "make a new directory (prompt)"),
            ("O", "create new file in $EDITOR (prompt)"),
            ("L", "long listing (wide aligned table)"),
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
            ("`", "jump to starting dir (set with gS or :startdir)"),
        ],
    },
    Section {
        title: "Harpoon (per-project pinned files, max 9)",
        rows: &[
            ("Ha", "append cursor file/dir to harpoon"),
            ("Hx", "remove cursor file/dir from harpoon"),
            ("H1..H9", "jump to harpoon slot N (chdir + focus)"),
            ("Hh", "open harpoon menu (j/k, K/J reorder, dd delete)"),
            ("=h", "limit listing to harpoon (incl. ancestor dirs)"),
        ],
    },
    Section {
        title: "Project home & session",
        rows: &[
            ("g h", "jump to PROJECT_HOME"),
            ("g P", "set PROJECT_HOME to current directory"),
            ("g S", "set start dir (target of `) to current directory"),
            ("g U", "flash user@host in status line"),
            (":project [.|<path>|clear]", "manage PROJECT_HOME"),
            (":startdir [.|<path>]", "manage start directory"),
            (
                ":name <NEW>",
                "rename the active session (SPICE_SPICE style)",
            ),
            (":whoami", "show user@host"),
        ],
    },
    Section {
        title: "Info",
        rows: &[
            ("D", "show date/time (UTC)"),
            ("g V", "show spyc version (also :version)"),
            ("I", "session info (pid, rss, counts)"),
            ("C", "toggle colors / mono"),
            ("A", "toggle activity monitor (draws/sec, cells/sec)"),
            ("s", "set environment variable (NAME=VALUE)"),
        ],
    },
    Section {
        title: "Pane path references",
        rows: &[
            ("g f", "jump to file path in pane output"),
            ("g F", "jump + open pager at line number"),
        ],
    },
    Section {
        title: "Git",
        rows: &[
            ("g d", "git diff (unstaged + new files)"),
            ("g D", "git diff --cached (staged changes)"),
            ("g b", "git blame (cursor file)"),
            ("W l", "list worktrees (1-9 to switch)"),
            ("W n", "new worktree (prompt for branch)"),
            ("W d", "delete current worktree (confirm)"),
        ],
    },
    Section {
        title: "Split pane (^a prefix, ^w also works)",
        rows: &[
            ("^\\  F10  ^a \\", "toggle the bottom pane (claude)"),
            ("F9", "open pane with claude --resume"),
            ("^a j / ^a k", "focus pane (down) / list (up)"),
            ("^a n  ^a ]", "next tab"),
            ("^a p  ^a [", "prev tab"),
            ("^a c", "new pane tab (prompt for command + cwd)"),
            ("^a K  ^a x", "close active pane tab"),
            ("^a 1..9", "switch to tab N"),
            ("^a r", "rename active tab"),
            ("^a R", "restart active tab command"),
            ("^a +  ^a -", "grow / shrink the pane"),
            ("^a z", "zoom pane (fullscreen toggle)"),
            ("^a v", "scroll pane history"),
            ("^a s", "send selection paths to pane stdin"),
            ("^a P", "pipe file contents of selection to pane"),
            ("^a i", "pipe inventory file contents to pane"),
            ("Ctrl+J", "newline in pane (multi-line input)"),
            ("SPYC_PANE_CMD", "env var for pane command (default claude)"),
        ],
    },
    Section {
        title: "Shell-out & commands",
        rows: &[
            (
                "!",
                "capture output → pager (PTY-backed; sudo/ssh prompts work)",
            ),
            ("!!", "repeat last captured command"),
            (
                "  while running",
                "keys → child, ^C interrupt, ^\\ kill, ^Z background",
            ),
            ("!?", "history editor — vi-edit, /search, :N jump, ^D del"),
            (";", "interactive → runs in top pane (top, vim, htop)"),
            ("$", "drop into $SHELL in current dir"),
            (
                ":",
                "command line (:cd, :sort, :grep, :limit, :set, :!, :;, :q)",
            ),
        ],
    },
    Section {
        title: "Background tasks & buffer history",
        rows: &[
            ("^Z", "(in ! pager) send running task to the background"),
            (":fg", "resume the most-recent backgrounded task"),
            (":fg N", "resume task #N specifically"),
            (
                "g B",
                "open task viewer for the most-recent task (peek mode)",
            ),
            (":task N", "open task viewer for task #N"),
            (
                "[t  ]t",
                "(in pager) cycle task viewer prev/next by id (chord)",
            ),
            (":pause / :pause N", "pause a backgrounded task (SIGSTOP)"),
            (":resume / :resume N", "resume a paused task (SIGCONT)"),
            (
                "S / C",
                "(in task viewer) pause / continue the underlying task",
            ),
            (
                "  divider glyphs",
                "[N+] new output, [N●] running, [N⏸] paused, [N✓] exit 0, [N✗] error",
            ),
            ("g p", "reopen the most-recently-closed pager buffer"),
            (":bprev / :bnext", "walk pager buffer history back/forward"),
            (
                "[b  ]b",
                "(in pager) walk buffer history back/forward (chord)",
            ),
        ],
    },
    Section {
        title: "Search",
        rows: &[
            ("/", "incremental search (prefix, or glob if * ? [)"),
            ("n", "repeat search forward"),
            ("N", "repeat search backward"),
            ("F", "project-wide fuzzy filename finder (gitignore-aware)"),
            (
                ":grep <pat>",
                "project-wide content search (embedded ripgrep, gf jumps)",
            ),
        ],
    },
    Section {
        title: "Meta",
        rows: &[
            ("?  F1", "this help"),
            ("^L", "redraw"),
            ("^R", "reload config (auto-reloads on save)"),
            ("Q  ^D  :q", "quit (q reserved for future macros)"),
            ("Esc (×2)", "cancel prompt (Esc→Normal→Esc→cancel)"),
        ],
    },
    Section {
        title: "CLI Flags",
        rows: &[
            ("-r --resume", "open pane with claude --resume"),
            ("-d --debug", "write debug log to /tmp/spyc-debug-<ts>.log"),
            ("-h --help", "show usage"),
            ("-v --version", "show version"),
        ],
    },
];

/// Floor for the key column: keeps the description stripe visually
/// consistent even when every visible key is short (default bindings
/// only). Widens automatically when any row — user binding or built-in —
/// needs more room.
const MIN_KEY_W: usize = 24;

/// Build help content formatted to fit within `col_w` per column.
/// Descriptions longer than the available width are wrapped at word
/// boundaries with continuations indented to the description column.
pub fn build_lines(theme: &Theme, user_keymap: &UserKeymap, col_w: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let key_style = Style::default().fg(theme.pick).add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(theme.status_path);
    let section_style = Style::default()
        .fg(theme.status_user)
        .add_modifier(Modifier::BOLD);

    let user_bindings: Vec<_> = user_keymap.iter().collect();
    let key_w = max_key_width(&user_bindings);

    for (i, section) in SECTIONS.iter().enumerate() {
        if i > 0 {
            out.push(Line::from(""));
        }
        out.push(Line::from(Span::styled(section.title, section_style)));
        for (keys, desc) in section.rows {
            emit_row(&mut out, keys, desc, col_w, key_w, key_style, desc_style);
        }
    }

    if !user_bindings.is_empty() {
        out.push(Line::from(""));
        out.push(Line::from(Span::styled(
            "Custom (.spycrc.toml)",
            section_style,
        )));
        for binding in &user_bindings {
            let chord = binding.chord.display();
            let action = binding.action.describe();
            emit_row(
                &mut out, &chord, &action, col_w, key_w, key_style, desc_style,
            );
        }
    }

    out
}

/// Widest key string across all built-in rows plus the user's custom
/// bindings, clamped to `MIN_KEY_W` as a floor.
fn max_key_width(user_bindings: &[&crate::keymap::user::UserBinding]) -> usize {
    let builtin = SECTIONS
        .iter()
        .flat_map(|s| s.rows.iter())
        .map(|(k, _)| super::display_width(k));
    let user = user_bindings
        .iter()
        .map(|b| super::display_width(&b.chord.display()));
    builtin.chain(user).max().unwrap_or(0).max(MIN_KEY_W)
}

/// Push a help row (key + description) into `out`. Wraps the description
/// onto continuation lines when it would overflow `col_w`; continuations
/// indent to the description column so the table stays readable.
fn emit_row(
    out: &mut Vec<Line<'static>>,
    keys: &str,
    desc: &str,
    col_w: usize,
    key_w: usize,
    key_style: Style,
    desc_style: Style,
) {
    let prefix_w = 2 + key_w + 2;
    let desc_w = col_w.saturating_sub(prefix_w).max(10);
    let chunks = wrap_description(desc, desc_w);
    let mut iter = chunks.into_iter();
    let first = iter.next().unwrap_or_default();
    out.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(display_pad_right(keys, key_w), key_style),
        Span::raw("  "),
        Span::styled(first, desc_style),
    ]));
    for cont in iter {
        out.push(Line::from(vec![
            Span::raw(" ".repeat(prefix_w)),
            Span::styled(cont, desc_style),
        ]));
    }
}

/// Split `text` into chunks no wider than `max`. Breaks at word boundaries
/// when possible; a single word longer than `max` is hard-split.
fn wrap_description(text: &str, max: usize) -> Vec<String> {
    if crate::ui::display_width(text) <= max {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w = 0usize;
    for word in text.split_whitespace() {
        let w = crate::ui::display_width(word);
        let need = if current.is_empty() { w } else { w + 1 };
        if current_w + need > max && !current.is_empty() {
            out.push(std::mem::take(&mut current));
            current_w = 0;
        }
        if w > max {
            // Word alone exceeds the budget — hard-split by chars.
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
                current_w = 0;
            }
            let mut buf = String::new();
            let mut buf_w = 0usize;
            for ch in word.chars() {
                let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if buf_w + cw > max && !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                    buf_w = 0;
                }
                buf.push(ch);
                buf_w += cw;
            }
            if !buf.is_empty() {
                current = buf;
                current_w = buf_w;
            }
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
            current_w += 1;
        }
        current.push_str(word);
        current_w += w;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_description_not_wrapped() {
        assert_eq!(wrap_description("short", 40), vec!["short".to_string()]);
    }

    #[test]
    fn wraps_at_word_boundary() {
        let out = wrap_description("one two three four five six seven", 10);
        assert!(out.iter().all(|c| c.len() <= 10), "got {out:?}");
        assert_eq!(out.join(" "), "one two three four five six seven");
    }

    #[test]
    fn hard_splits_long_word() {
        let out = wrap_description("supercalifragilistic", 6);
        assert!(out.iter().all(|c| c.len() <= 6));
        assert_eq!(out.join(""), "supercalifragilistic");
    }

    #[test]
    fn preserves_single_word_exactly_fits() {
        let out = wrap_description("abcdef", 6);
        assert_eq!(out, vec!["abcdef".to_string()]);
    }
}
