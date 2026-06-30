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
//! - For `unix` and `command`, **the rest of the line after the verb** is
//!   taken verbatim — a shell command template for `unix` (with `%` expanded
//!   to the selection at run time), or a `:` command line for `command` (e.g.
//!   `command graveyard`). Both are `is_executing`, so only `$HOME` config
//!   may bind them.
//!
//! Supported action verbs (the identifiers accepted after `map KEY`):
//!
//! - quit, redraw, help / keys
//! - up / previous, down / nextfile, left, right, pageup, pagedown
//! - home, climb
//! - enter / edit (open-or-edit), display (open-or-display)
//! - pick, unpick, take, drop, inventory, empty
//! - search; next / searchnext (repeat search forward); searchprev (repeat
//!   search backward). NB: `previous` above is a legacy alias for *cursor-up*,
//!   not search-previous — use `searchprev` to rebind backward search.
//! - startshell; unix CMD (verbatim template); unix_cmd (prompted, captured
//!   into the pager); foreground_cmd (prompted, run in the foreground like `;`)
//! - command CMD ($HOME only) — bind a key to a `:` command (e.g.
//!   `command graveyard`, `command activity`)
//! - longlist, file, copy, move, remove, makedirs
//! - ignoretoggle =N, patternpick =GLOB, jump =PATH
//! - panescroll, panesave
//! - togglepane (rebind the pane toggle if `^\` / `F10` are
//!   intercepted by the host terminal / window manager)

use crate::keymap::action::Action;
use crate::keymap::user::{BoundAction, KeyChord, NamedKey, UserBinding};

/// Parse a single `map`/`unmap` line. Returns `Ok(None)` for blank/comment
/// lines and `Ok(Some(binding))` for real rules; `unmap` currently parses to
/// `Ok(None)` (removals aren't modeled).
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

/// Parse a `.spycrc` DSL key token (a plain char, `^X` control notation, or a
/// `<Named>` key) into a [`KeyChord`]. `pub` so `spyc.map(key, fn)` can reuse
/// the exact same key grammar the config DSL uses.
pub fn parse_key(tok: &str) -> Result<KeyChord, String> {
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

pub fn parse_action(name: &str, tail: &str) -> Result<BoundAction, String> {
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
        "next" | "searchnext" => Ok(BoundAction::Plain(Action::SearchNext)),
        // Backward search repeat (the default `N`). Previously unbindable —
        // `next` had no symmetric verb, and `previous` is cursor-up, not
        // search-prev.
        "searchprev" => Ok(BoundAction::Plain(Action::SearchPrev)),

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
        // `command <name [args]>` — bind a key to a `:` command (the rest of
        // the line is the command, e.g. `command graveyard`). Only honored in
        // $HOME config (it's `is_executing` — it can reach `:!`/`:;` shell).
        "command" => {
            if tail.is_empty() {
                Err("`command` needs a : command (e.g. `command graveyard`)".to_string())
            } else {
                Ok(BoundAction::Command(tail.to_string()))
            }
        }
        // `lua <name>` — bind a key to a $HOME Lua script
        // (`~/.config/spyc/lua/<name>.lua`). `is_executing` (runs arbitrary
        // code), so only `$HOME` config may bind it.
        "lua" => {
            if tail.is_empty() {
                Err("`lua` needs a script name (e.g. `lua mymacro`)".to_string())
            } else {
                Ok(BoundAction::Lua(tail.to_string()))
            }
        }

        "longlist" => Ok(BoundAction::Plain(Action::LongList)),
        "file" => Ok(BoundAction::Plain(Action::FileType)),
        "copy" => Ok(BoundAction::Plain(Action::CopyPrompt)),
        "move" => Ok(BoundAction::Plain(Action::MovePrompt)),
        "remove" => Ok(BoundAction::Plain(Action::RemovePrompt(None))),
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
    fn parses_command_verb_with_rest_of_line() {
        let b = parse("map A command activity").unwrap().unwrap();
        match &b.action {
            BoundAction::Command(s) => assert_eq!(s, "activity"),
            other => panic!("expected Command, got {other:?}"),
        }
        // The whole tail is the command line, args included.
        let b = parse("map ^G command project .").unwrap().unwrap();
        match &b.action {
            BoundAction::Command(s) => assert_eq!(s, "project ."),
            other => panic!("expected Command, got {other:?}"),
        }
    }

    #[test]
    fn command_verb_is_executing() {
        // So the project-config security gate ($HOME-only) covers it.
        let b = parse("map A command activity").unwrap().unwrap();
        assert!(b.action.is_executing());
    }

    #[test]
    fn lua_verb_parses_and_is_executing() {
        let b = parse("map z lua mymacro").unwrap().unwrap();
        match &b.action {
            BoundAction::Lua(s) => assert_eq!(s, "mymacro"),
            other => panic!("expected Lua, got {other:?}"),
        }
        // The $HOME-only gate keys off this — a project `.spycrc.toml` can't
        // bind Lua (it runs arbitrary code).
        assert!(b.action.is_executing());
    }

    #[test]
    fn empty_lua_is_an_error() {
        assert!(parse("map z lua").is_err());
    }

    #[test]
    fn empty_command_is_an_error() {
        assert!(parse("map A command").is_err());
    }

    #[test]
    fn parses_named_key() {
        let b = parse("map <F1> help").unwrap().unwrap();
        assert_eq!(b.chord, KeyChord::Named(NamedKey::Fn(1)));
        assert!(matches!(b.action, BoundAction::Plain(Action::Help)));
    }

    #[test]
    fn binds_search_repeat_verbs() {
        // `searchprev` was previously unbindable — only `next` → SearchNext
        // existed, with no symmetric verb for backward search.
        assert!(matches!(
            parse("map p searchprev").unwrap().unwrap().action,
            BoundAction::Plain(Action::SearchPrev)
        ));
        assert!(matches!(
            parse("map n searchnext").unwrap().unwrap().action,
            BoundAction::Plain(Action::SearchNext)
        ));
        // `next` stays an alias for SearchNext (back-compat).
        assert!(matches!(
            parse("map n next").unwrap().unwrap().action,
            BoundAction::Plain(Action::SearchNext)
        ));
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

    // ── parser fuzzing via proptest (testing campaign, cluster 8) ──
    // cargo-fuzz would need a lib target (spyc is bin-only); proptest gets the
    // same panic-freedom property in-crate — no nightly, runs in `make check`.
    proptest::proptest! {
        /// The DSL parser must never panic on *any* input — it returns
        /// Ok(binding) / Ok(None) / Err for everything, never an unwrap or an
        /// index/slice panic.
        #[test]
        fn parse_never_panics_on_arbitrary_input(line in ".{0,64}") {
            let _ = parse(&line);
        }

        /// Biased toward the `map` grammar so the key / action sub-parsers
        /// (`^X` control, plain char, `=value` args) are exercised — not just
        /// the unknown-directive early-out.
        #[test]
        fn parse_never_panics_on_map_like_input(
            key in "\\^?.{0,4}",
            action in "[a-z_]{0,16}",
            arg in "(=.{0,12})?",
        ) {
            let _ = parse(&format!("map {key} {action} {arg}"));
        }

        /// A well-formed `map ^<letter> <known-verb>` always parses to a
        /// binding — the happy path holds for any letter × known verb.
        #[test]
        fn parse_well_formed_ctrl_map_binds(c in "[a-z]") {
            for verb in ["quit", "redraw", "help", "up", "down", "pageup"] {
                let line = format!("map ^{c} {verb}");
                proptest::prop_assert!(
                    matches!(parse(&line), Ok(Some(_))),
                    "well-formed map should bind: {line:?}"
                );
            }
        }
    }
}
