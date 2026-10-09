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
//! - lua NAME ($HOME only) — run `<config_root>/lua/NAME.lua`
//! - prompt NAME ($HOME only) — type the `[prompts]` template NAME into the
//!   active pane tab
//! - longlist, file, copy, move, remove, makedirs
//! - ignoretoggle =N, patternpick =GLOB, jump =PATH
//! - panescroll, panesave
//! - togglepane (rebind the pane toggle if `^\` / `F10` are
//!   intercepted by the host terminal / window manager)
//! - any other action by its canonical name (`git_blame`, `worktree_list`, …),
//!   the names `spyc.action` takes; a parametric one needs `=value`
//!   (`harpoon_jump =3`, `set_mark =a`)
//!
//! `unmap KEY` binds `KEY` to `noop`.

use crate::keymap::action::Action;
use crate::keymap::user::{BoundAction, KeyChord, NamedKey, UserBinding};

/// Parse a single `map`/`unmap` line. Returns `Ok(None)` for blank/comment
/// lines and `Ok(Some(binding))` for real rules.
pub fn parse(line: &str) -> Result<Option<UserBinding>, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Ok(None);
    }
    let (verb, rest) = split_once_ws(trimmed);
    match verb {
        "map" => parse_map(rest),
        "unmap" => parse_unmap(rest),
        other => Err(format!(
            "unknown directive `{other}` (expected `map` or `unmap`)"
        )),
    }
}

/// `unmap KEY` binds `KEY` to `noop`, so its built-in binding stops firing. A
/// later `map` of the same key still wins, as between two `map`s.
fn parse_unmap(rest: &str) -> Result<Option<UserBinding>, String> {
    let (key_tok, extra) = split_once_ws(rest);
    if key_tok.is_empty() {
        return Err("missing KEY after `unmap`".to_string());
    }
    if !extra.trim().is_empty() {
        return Err(format!("`unmap` takes only a key, got `{}`", extra.trim()));
    }
    Ok(Some(UserBinding {
        chord: parse_key(key_tok)?,
        action: BoundAction::Plain(Action::Noop),
    }))
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

/// The value of an `=value` argument.
fn arg_value(tail: &str) -> Option<&str> {
    tail.strip_prefix('=').map(str::trim)
}

pub fn parse_action(name: &str, tail: &str) -> Result<BoundAction, String> {
    match name {
        "quit" => Ok(BoundAction::Plain(Action::Quit)),
        "redraw" => Ok(BoundAction::Plain(Action::Redraw)),
        "help" | "keys" => Ok(BoundAction::Plain(Action::Help)),
        "about" => Ok(BoundAction::Plain(Action::About)),

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
        // Backward search repeat (default `N`); `previous` is cursor-up, not
        // search-prev.
        "searchprev" => Ok(BoundAction::Plain(Action::SearchPrev)),

        "startshell" => Ok(BoundAction::Plain(Action::StartShell)),
        // `!` captures output into the pager, `;` runs foreground; this
        // defaults to the captured variant, `foreground_cmd` is `;`.
        "unix_cmd" => Ok(BoundAction::Plain(Action::ShellCapturedPrompt)),
        "foreground_cmd" => Ok(BoundAction::Plain(Action::ShellForegroundPrompt)),
        "unix" => {
            if tail.is_empty() {
                Err("`unix` needs a shell command (e.g. `unix ls %`)".to_string())
            } else {
                Ok(BoundAction::UnixCmd(tail.to_string()))
            }
        }
        // `command <name>` — bind a key to a `:` command (e.g. `command
        // graveyard`). $HOME-only (`is_executing`; can reach `:!`/`:;`).
        "command" => {
            if tail.is_empty() {
                Err("`command` needs a : command (e.g. `command graveyard`)".to_string())
            } else {
                Ok(BoundAction::Command(tail.to_string()))
            }
        }
        // `lua <name>` — bind a key to a $HOME Lua script; `is_executing`
        // (runs arbitrary code), so $HOME config only.
        "lua" => {
            if tail.is_empty() {
                Err("`lua` needs a script name (e.g. `lua mymacro`)".to_string())
            } else {
                Ok(BoundAction::Lua(tail.to_string()))
            }
        }

        // `prompt <name>` — type a `[prompts]` template into the active pane
        // tab; `is_executing` (it types at an agent), so $HOME config only.
        "prompt" => {
            if tail.is_empty() {
                Err("`prompt` needs a template name (e.g. `prompt review`)".to_string())
            } else {
                Ok(BoundAction::Prompt(tail.to_string()))
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

        other => named_action(other, tail).map(BoundAction::Plain),
    }
}

/// Any action by its canonical name (`git_blame`, `worktree_list`, …), the
/// vocabulary `spyc.action` takes. A parametric action needs its parameter as
/// `=value`, since a default slot, tab or mark would bind the wrong one
/// silently; the rest refuse an argument.
fn named_action(name: &str, tail: &str) -> Result<Action, String> {
    let param = arg_value(tail);
    Ok(match name {
        "set_mark" => Action::SetMark(mark_letter(name, param)?),
        "jump_mark" => Action::JumpMark(mark_letter(name, param)?),
        "harpoon_jump" => Action::HarpoonJump(one_to_nine(name, param)?),
        "pane_tab_by_index" => Action::PaneTabByIndex(one_to_nine(name, param)?),
        "toggle_mask" => match param {
            Some("1") => Action::ToggleMask(1),
            Some("2") => Action::ToggleMask(2),
            _ => return Err("`toggle_mask` needs `=1` or `=2`".to_string()),
        },
        "chmod_add" => match param {
            Some("w") => Action::ChmodAdd('w'),
            Some("x") => Action::ChmodAdd('x'),
            _ => return Err("`chmod_add` needs `=w` or `=x`".to_string()),
        },
        _ => {
            let action = crate::keymap::action::action_from_name(name)
                .ok_or_else(|| format!("unknown action `{name}`"))?;
            if !tail.is_empty() {
                return Err(format!("`{name}` takes no argument, got `{tail}`"));
            }
            action
        }
    })
}

fn one_to_nine(name: &str, param: Option<&str>) -> Result<u8, String> {
    param
        .and_then(|v| v.parse::<u8>().ok())
        .filter(|n| (1..=9).contains(n))
        .ok_or_else(|| format!("`{name}` needs `=N`, 1 to 9"))
}

fn mark_letter(name: &str, param: Option<&str>) -> Result<char, String> {
    let mut chars = param.unwrap_or_default().chars();
    match (chars.next(), chars.next()) {
        (Some(c @ 'a'..='z'), None) => Ok(c),
        _ => Err(format!("`{name}` needs `=LETTER`, a to z")),
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

    /// Where spyc ships DSL lines a user may copy.
    const EXAMPLE_SOURCES: &[&str] = &[
        "src/config/default.spycrc.toml",
        "CONFIGURATION.md",
        "FEATURES.md",
        "README.md",
        "AGENTS.md",
        "DESIGN.md",
        "docs/KEYBINDINGS.md",
    ];

    /// How a DSL line appears in a doc: a quoted TOML array entry, a code
    /// span, or a template comment `# map <KEY> ... — does`, which is a form.
    /// An `unmap` line counts as one too.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Shape {
        Quoted,
        Span,
        Form,
    }

    /// Every DSL line the docs and the `--print-config` template show, with
    /// where it came from. A placeholder key or argument (`KEY`, `<glob>`)
    /// gets a real value, so a form is checked for its shape, `=` included.
    /// The grammar line itself (an `action` placeholder where the verb goes)
    /// has nothing to check.
    fn shipped_examples() -> Vec<(String, Shape, String)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let shapes = [
            (
                Shape::Quoted,
                regex::Regex::new(r#""((?:un)?map [^"]+)""#).expect("regex"),
            ),
            (
                Shape::Span,
                regex::Regex::new(r"`((?:un)?map [^`]+)`").expect("regex"),
            ),
            (
                Shape::Form,
                regex::Regex::new(r"^\s*#\s+((?:un)?map .+?)\s+—").expect("regex"),
            ),
        ];
        let mut out = Vec::new();
        for &file in EXAMPLE_SOURCES {
            let text = std::fs::read_to_string(root.join(file)).expect(file);
            for (n, line) in text.lines().enumerate() {
                for (shape, re) in &shapes {
                    for cap in re.captures_iter(line) {
                        let mut toks: Vec<String> =
                            cap[1].split_whitespace().map(String::from).collect();
                        if matches!(toks[1].as_str(), "KEY" | "<KEY>") {
                            toks[1] = "x".to_string();
                        }
                        if toks[0] == "map"
                            && toks
                                .get(2)
                                .is_none_or(|a| a == "action" || a.starts_with('<'))
                        {
                            continue;
                        }
                        for arg in toks.iter_mut().skip(3) {
                            let bare = arg.trim_start_matches('=');
                            if bare.starts_with('<') && bare.ends_with('>') {
                                *arg = arg.replace(bare, "x");
                            }
                        }
                        out.push((format!("{file}:{}", n + 1), *shape, toks.join(" ")));
                    }
                }
            }
        }
        out
    }

    /// `--print-config` prints the template for the user to uncomment, and the
    /// docs are what they copy from. One line the parser rejects fails the
    /// whole config load, so a broken example locks the user out of the rest
    /// of their config.
    #[test]
    fn every_shipped_keymap_example_parses() {
        let examples = shipped_examples();
        // A reader that stops matching checks nothing and passes, so each
        // source and each shape must still turn something up.
        for file in EXAMPLE_SOURCES {
            assert!(
                examples
                    .iter()
                    .any(|(at, ..)| at.starts_with(&format!("{file}:"))),
                "no example found in {file}"
            );
        }
        for shape in [Shape::Quoted, Shape::Span, Shape::Form] {
            assert!(
                examples.iter().any(|(at, s, _)| {
                    *s == shape && at.starts_with("src/config/default.spycrc.toml:")
                }),
                "no {shape:?} example found in the template"
            );
        }
        assert!(
            examples
                .iter()
                .any(|(_, _, line)| line.starts_with("unmap ")),
            "no unmap example found"
        );
        let broken: Vec<String> = examples
            .iter()
            .filter_map(|(at, _, line)| parse(line).err().map(|e| format!("{at}: `{line}`: {e}")))
            .collect();
        assert!(broken.is_empty(), "{}", broken.join("\n"));
    }

    #[test]
    fn prompt_verb_parses_and_is_executing() {
        let b = parse("map <F6> prompt review").unwrap().unwrap();
        match &b.action {
            BoundAction::Prompt(s) => assert_eq!(s, "review"),
            other => panic!("expected Prompt, got {other:?}"),
        }
        // A project `.spycrc.toml` binding one would let a repo decide what
        // gets typed at your agent.
        assert!(b.action.is_executing());
    }

    #[test]
    fn empty_prompt_is_an_error() {
        assert!(parse("map <F6> prompt").is_err());
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

    /// `about` is bindable from `.spycrc.toml` even though its default home is
    /// the leader (`Space a`) — the same deal `help`/`keys` gets.
    #[test]
    fn binds_about() {
        let b = parse("map ^A about").unwrap().unwrap();
        assert!(matches!(b.action, BoundAction::Plain(Action::About)));
        // Informational page, not a shell/lua escape hatch: bindable from a
        // project-local rc too.
        assert!(!b.action.is_executing());
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

    /// The `=value` a parametric action is bound with, and the action that
    /// binds; `None` for one that takes no parameter.
    fn sample_parameter(a: &Action) -> Option<(&'static str, Action)> {
        Some(match a {
            Action::SetMark(_) => ("=q", Action::SetMark('q')),
            Action::JumpMark(_) => ("=q", Action::JumpMark('q')),
            Action::HarpoonJump(_) => ("=3", Action::HarpoonJump(3)),
            Action::PaneTabByIndex(_) => ("=3", Action::PaneTabByIndex(3)),
            Action::ToggleMask(_) => ("=2", Action::ToggleMask(2)),
            Action::ChmodAdd(_) => ("=w", Action::ChmodAdd('w')),
            _ => return None,
        })
    }

    /// Every action binds by the name `spyc.action` takes. The curated verbs
    /// are matched first, so one spelling a canonical name for a different
    /// action fails here too.
    #[test]
    fn every_action_binds_by_its_canonical_name() {
        use strum::IntoEnumIterator;
        for variant in Action::iter() {
            let name = variant.canonical_name();
            let (line, want) = match sample_parameter(&variant) {
                Some((arg, want)) => (format!("map x {name} {arg}"), want),
                None => (
                    format!("map x {name}"),
                    crate::keymap::action::action_from_name(name).expect(name),
                ),
            };
            let got = parse(&line)
                .unwrap_or_else(|e| panic!("`{line}`: {e}"))
                .expect("a binding");
            assert_eq!(got.action, BoundAction::Plain(want), "`{line}`");
        }
    }

    #[test]
    fn a_parametric_action_needs_a_valid_parameter() {
        for line in [
            "map x harpoon_jump",
            "map x harpoon_jump =0",
            "map x harpoon_jump =10",
            "map x pane_tab_by_index =a",
            "map x set_mark",
            "map x set_mark =A",
            "map x jump_mark =ab",
            "map x toggle_mask =3",
            "map x chmod_add =r",
        ] {
            assert!(parse(line).is_err(), "`{line}` should be rejected");
        }
    }

    #[test]
    fn an_action_without_a_parameter_refuses_one() {
        for line in ["map x git_blame =1", "map x git_blame now"] {
            assert!(parse(line).is_err(), "`{line}` should be rejected");
        }
    }

    /// The actions that write to a pane's input put the repo's text in front
    /// of your agent, as a `prompt` does, so they share its `$HOME`-only gate.
    #[test]
    fn an_action_that_writes_to_a_pane_is_executing() {
        for name in [
            "pane_send_selection",
            "pane_send_prefix",
            "pane_pipe_content",
            "pane_pipe_inventory",
        ] {
            let b = parse(&format!("map x {name}")).unwrap().unwrap();
            assert!(b.action.is_executing(), "{name}");
        }
        assert!(
            !parse("map x git_blame")
                .unwrap()
                .unwrap()
                .action
                .is_executing()
        );
    }

    #[test]
    fn unmap_binds_the_key_to_nothing() {
        let b = parse("unmap q").unwrap().unwrap();
        assert_eq!(b.chord, KeyChord::Char('q'));
        assert_eq!(b.action, BoundAction::Plain(Action::Noop));
        assert!(!b.action.is_executing());
    }

    #[test]
    fn unmap_takes_exactly_one_key() {
        for line in ["unmap", "unmap q quit", "unmap ^"] {
            assert!(parse(line).is_err(), "`{line}` should be rejected");
        }
    }

    /// `unmap` silences a built-in key, and a `map` after it binds it again:
    /// the later line wins, as between two `map`s.
    #[test]
    fn unmap_silences_a_default_and_a_later_map_rebinds_it() {
        use crate::keymap::{Resolver, ResolverOutcome, UserKeymap};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let keymap = |lines: &[&str]| {
            UserKeymap::from_bindings(lines.iter().filter_map(|l| parse(l).unwrap()).collect())
        };
        let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let mut r = Resolver::new();
        assert_eq!(
            r.feed(j, &keymap(&[])),
            ResolverOutcome::Action(Action::Down(1))
        );
        assert_eq!(
            r.feed(j, &keymap(&["unmap j"])),
            ResolverOutcome::User(BoundAction::Plain(Action::Noop))
        );
        assert_eq!(
            r.feed(j, &keymap(&["unmap j", "map j up"])),
            ResolverOutcome::User(BoundAction::Plain(Action::Up(1)))
        );
    }

    /// `CONFIGURATION.md`'s action-name table is where a user finds a name to
    /// bind, so it lists every action and nothing else.
    #[test]
    fn the_action_name_table_lists_every_action() {
        use strum::IntoEnumIterator;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let doc = std::fs::read_to_string(root.join("CONFIGURATION.md")).expect("doc");
        let (_, section) = doc
            .split_once("\n### Action names\n")
            .expect("an `### Action names` section");
        let section = section.split("\n#").next().unwrap_or(section);
        let row = regex::Regex::new(r"^\| `([a-z_]+)[^`]*` \|").expect("regex");
        let listed: std::collections::BTreeSet<&str> = section
            .lines()
            .filter_map(|l| row.captures(l))
            .map(|c| c.get(1).expect("name").as_str())
            .collect();
        let names: std::collections::BTreeSet<&str> =
            Action::iter().map(|a| a.canonical_name()).collect();
        assert_eq!(listed, names);
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
