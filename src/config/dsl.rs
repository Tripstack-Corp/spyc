//! Keymap DSL parser.
//!
//! Grammar (one `map` / `unmap` line per entry in the TOML `keymap` array):
//!
//! ```text
//! map  KEY  ACTION  [ARGS...]
//! unmap  KEY
//! ```
//!
//! - Lines whose first non-whitespace char is `#` are comments (the TOML
//!   surrounds with a string, so comments are stripped per entry).
//! - `KEY` is one of:
//!     - A single printable character: `f`, `;`, `!`, `H`.
//!     - Control notation: `^P`, `^W`.
//!     - Named keys: `<Enter>`, `<Space>`, `<F1>`, `<Up>`, `<PageDown>`.
//! - `ACTION` is one of the identifiers below. Most take no args.
//! - For actions that take a preset argument the syntax is `=value`
//!   (e.g. `map h jump =$HFS/houdini`).
//! - For `unix`, **the rest of the line after `unix`** is taken verbatim
//!   as a shell command template (with `%` expanded to the selection at
//!   run time).
//!
//! Supported actions (M4 subset):
//!
//! - quit, redraw, help
//! - up, down, left, right, pageup, pagedown, home, climb
//! - enter (display), edit (enter_or_edit)
//! - pick, unpick, take, drop, inventory, empty
//! - search, next, previous
//! - startshell, unix, unix_cmd
//! - longlist, file, copy, move, remove, makedirs
//! - ignoretoggle =N
//! - patternpick =PATTERN
//! - jump =PATH
//! - togglepane (rebind the pane toggle if `^\` / `F10` are
//!   intercepted by the host terminal / window manager)

use crate::keymap::action::Action;
use crate::keymap::user::{BoundAction, KeyChord, NamedKey, UserBinding};

/// Parse a single `map`/`unmap` line. Returns `Ok(None)` for blank/comment
/// lines and `Ok(Some(binding))` for real rules. `unmap` returns Ok(None)
/// today because we don't model removals yet; when we do we'll return a
/// separate variant.
pub fn parse(line: &str) -> Result<Option<UserBinding>, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Ok(None);
    }
    let (verb, rest) = split_once_ws(trimmed);
    match verb {
        "map" => parse_map(rest),
        "unmap" => Ok(None), // TODO: represent unbind.
        other => Err(format!("unknown directive `{other}` (expected `map`)")),
    }
}

fn parse_map(rest: &str) -> Result<Option<UserBinding>, String> {
    let (key_tok, rest) = split_once_ws(rest);
    if key_tok.is_empty() {
        return Err("missing KEY after `map`".to_string());
    }
    let chord = parse_key(key_tok)?;

    let rest = rest.trim_start();
    if rest.is_empty() {
        return Err("missing action after key".to_string());
    }
    let (action_tok, tail) = split_once_ws(rest);
    let tail = tail.trim_start();

    let action = parse_action(action_tok, tail)?;
    Ok(Some(UserBinding { chord, action }))
}

fn parse_key(tok: &str) -> Result<KeyChord, String> {
    // Control notation: ^X
    if let Some(rest) = tok.strip_prefix('^') {
        let mut chars = rest.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return Err(format!("bad control key `{tok}` (expected `^X`)"));
        };
        return Ok(KeyChord::Ctrl(c.to_ascii_lowercase()));
    }
    // Named: <Enter>, <F1>, <Up>, ...
    if let Some(inner) = tok.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        return parse_named(inner).map(KeyChord::Named);
    }
    // Plain char.
    let mut chars = tok.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return Err(format!("unrecognized key `{tok}`"));
    };
    Ok(KeyChord::Char(c))
}

fn parse_named(name: &str) -> Result<NamedKey, String> {
    let lower = name.to_ascii_lowercase();
    Ok(match lower.as_str() {
        "enter" | "return" | "cr" => NamedKey::Enter,
        "space" | "sp" => NamedKey::Space,
        "tab" => NamedKey::Tab,
        "backspace" | "bs" => NamedKey::Backspace,
        "esc" | "escape" => NamedKey::Esc,
        "up" => NamedKey::Up,
        "down" => NamedKey::Down,
        "left" => NamedKey::Left,
        "right" => NamedKey::Right,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        "pageup" | "pgup" => NamedKey::PageUp,
        "pagedown" | "pgdn" => NamedKey::PageDown,
        other => {
            if let Some(n) = other
                .strip_prefix('f')
                .filter(|s| s.chars().all(|c| c.is_ascii_digit()))
                .and_then(|s| s.parse::<u8>().ok())
            {
                NamedKey::Fn(n)
            } else {
                return Err(format!("unknown named key `<{name}>`"));
            }
        }
    })
}

fn parse_action(name: &str, tail: &str) -> Result<BoundAction, String> {
    // Helpers for the "=value" argument convention.
    fn arg_value(tail: &str) -> Option<&str> {
        tail.strip_prefix('=').map(str::trim)
    }

    match name {
        "quit" => Ok(BoundAction::Plain(Action::Quit)),
        "redraw" => Ok(BoundAction::Plain(Action::Redraw)),
        "help" | "keys" => Ok(BoundAction::Plain(Action::Help)),

        "up" | "previous" => Ok(BoundAction::Plain(Action::Up(1))),
        "down" | "nextfile" => Ok(BoundAction::Plain(Action::Down(1))),
        "left" => Ok(BoundAction::Plain(Action::Left(1))),
        "right" => Ok(BoundAction::Plain(Action::Right(1))),
        "pageup" => Ok(BoundAction::Plain(Action::PageUp)),
        "pagedown" => Ok(BoundAction::Plain(Action::PageDown)),

        "home" => Ok(BoundAction::Plain(Action::Home)),
        "climb" => Ok(BoundAction::Plain(Action::Climb)),
        "enter" | "edit" => Ok(BoundAction::Plain(Action::EnterOrEdit)),
        "display" => Ok(BoundAction::Plain(Action::EnterOrDisplay)),

        "pick" => Ok(BoundAction::Plain(Action::TogglePick)),
        "unpick" => Ok(BoundAction::Plain(Action::PickToggleAll)),
        "take" => Ok(BoundAction::Plain(Action::Take)),
        "drop" => Ok(BoundAction::Plain(Action::Drop)),
        "inventory" => Ok(BoundAction::Plain(Action::ToggleInventoryView)),
        "empty" => Ok(BoundAction::Plain(Action::EmptyInventory)),

        "search" => Ok(BoundAction::Plain(Action::SearchPrompt)),
        "next" => Ok(BoundAction::Plain(Action::SearchNext)),

        "startshell" => Ok(BoundAction::Plain(Action::StartShell)),
        // `unix_cmd` in spy was a prompted shell command. In spyc, `!`
        // captures output into the pager while `;` runs foreground. DSL
        // defaults to the captured variant; use `foreground_cmd` for `;`.
        "unix_cmd" => Ok(BoundAction::Plain(Action::ShellCapturedPrompt)),
        "foreground_cmd" => Ok(BoundAction::Plain(Action::ShellForegroundPrompt)),
        "unix" => {
            if tail.is_empty() {
                Err("`unix` needs a shell command (e.g. `unix ls %`)".to_string())
            } else {
                Ok(BoundAction::UnixCmd(tail.to_string()))
            }
        }

        "longlist" => Ok(BoundAction::Plain(Action::LongList)),
        "file" => Ok(BoundAction::Plain(Action::FileType)),
        "copy" => Ok(BoundAction::Plain(Action::CopyPrompt)),
        "move" => Ok(BoundAction::Plain(Action::MovePrompt)),
        "remove" => Ok(BoundAction::Plain(Action::RemovePrompt)),
        "makedirs" => Ok(BoundAction::Plain(Action::MakeDirPrompt)),

        "ignoretoggle" => {
            let Some(v) = arg_value(tail) else {
                return Err("`ignoretoggle` needs `=N` (1 or 2)".to_string());
            };
            let n = v
                .parse::<u8>()
                .map_err(|_| format!("ignoretoggle expects a number, got `{v}`"))?;
            Ok(BoundAction::ToggleMaskFixed(n))
        }

        "patternpick" => {
            let Some(pat) = arg_value(tail) else {
                return Err("`patternpick` needs `=GLOB`".to_string());
            };
            Ok(BoundAction::PatternPick(pat.to_string()))
        }

        "jump" => {
            let Some(path) = arg_value(tail) else {
                return Err("`jump` needs `=PATH`".to_string());
            };
            Ok(BoundAction::Jump(path.to_string()))
        }

        "panescroll" => Ok(BoundAction::Plain(Action::PaneScrollEnter)),
        "panesave" => Ok(BoundAction::Plain(Action::PaneScrollSave)),
        // Lets a user rebind the pane toggle when a host terminal /
        // window manager has grabbed the built-in `^\` and `F10`.
        // Example: `map ^p togglepane`.
        "togglepane" => Ok(BoundAction::Plain(Action::TogglePane)),

        other => Err(format!("unknown action `{other}`")),
    }
}

/// Split off the first whitespace-separated token; return `(token, rest)`.
fn split_once_ws(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_map() {
        let b = parse("map f unix file %").unwrap().unwrap();
        assert_eq!(b.chord, KeyChord::Char('f'));
        match &b.action {
            BoundAction::UnixCmd(s) => assert_eq!(s, "file %"),
            _ => panic!("expected UnixCmd"),
        }
    }

    #[test]
    fn parses_control_key() {
        let b = parse("map ^P unix ps -u $USER").unwrap().unwrap();
        assert_eq!(b.chord, KeyChord::Ctrl('p'));
    }

    #[test]
    fn parses_named_key() {
        let b = parse("map <F1> help").unwrap().unwrap();
        assert_eq!(b.chord, KeyChord::Named(NamedKey::Fn(1)));
        assert!(matches!(b.action, BoundAction::Plain(Action::Help)));
    }

    #[test]
    fn parses_patternpick_arg() {
        let b = parse("map H patternpick =*.hip").unwrap().unwrap();
        match &b.action {
            BoundAction::PatternPick(s) => assert_eq!(s, "*.hip"),
            _ => panic!(),
        }
    }

    #[test]
    fn parses_jump_arg() {
        let b = parse("map h jump =$HFS/houdini").unwrap().unwrap();
        match &b.action {
            BoundAction::Jump(s) => assert_eq!(s, "$HFS/houdini"),
            _ => panic!(),
        }
    }

    #[test]
    fn ignoretoggle_group() {
        let b = parse("map 1 ignoretoggle =1").unwrap().unwrap();
        assert!(matches!(b.action, BoundAction::ToggleMaskFixed(1)));
    }

    #[test]
    fn blank_and_comment_lines_ignored() {
        assert!(parse("").unwrap().is_none());
        assert!(parse("   ").unwrap().is_none());
        assert!(parse("# hello").unwrap().is_none());
    }

    #[test]
    fn rejects_unknown_action() {
        let err = parse("map f banana").unwrap_err();
        assert!(err.contains("unknown action"), "got: {err}");
    }

    #[test]
    fn parses_togglepane() {
        // Escape hatch for users whose terminal grabs the built-in
        // pane-toggle keys (`^\` / `F10`).
        let b = parse("map ^p togglepane").unwrap().unwrap();
        assert_eq!(b.chord, KeyChord::Ctrl('p'));
        assert!(matches!(b.action, BoundAction::Plain(Action::TogglePane)));
    }
}
