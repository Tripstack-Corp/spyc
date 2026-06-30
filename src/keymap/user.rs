//! User-supplied keymap bindings — the target of `.spycrc` parsing.
//!
//! A user binding marries a `KeyChord` (how the binding is triggered) to a
//! `BoundAction` (what to do). The `Resolver` consults the user table
//! first; if nothing matches, it falls back to the built-in defaults.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::action::Action;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyChord {
    /// Plain character: `f`, `;`, `!`, `H`.
    Char(char),
    /// Control + letter: `^P`, `^W`.
    Ctrl(char),
    /// Named keys: `<Enter>`, `<F1>`, `<Up>`, …
    Named(NamedKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedKey {
    Enter,
    Space,
    Tab,
    Backspace,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Fn(u8),
}

impl NamedKey {
    fn display(self) -> String {
        match self {
            Self::Enter => "Enter".into(),
            Self::Space => "Space".into(),
            Self::Tab => "Tab".into(),
            Self::Backspace => "Backspace".into(),
            Self::Esc => "Esc".into(),
            Self::Up => "Up".into(),
            Self::Down => "Down".into(),
            Self::Left => "Left".into(),
            Self::Right => "Right".into(),
            Self::Home => "Home".into(),
            Self::End => "End".into(),
            Self::PageUp => "PageUp".into(),
            Self::PageDown => "PageDown".into(),
            Self::Fn(n) => format!("F{n}"),
        }
    }
}

impl KeyChord {
    /// Human-readable rendering, suitable for help output. Matches the
    /// DSL `map` syntax the user wrote in `.spycrc.toml`.
    pub fn display(&self) -> String {
        match self {
            Self::Char(c) => c.to_string(),
            Self::Ctrl(c) => format!("^{}", c.to_ascii_uppercase()),
            Self::Named(n) => format!("<{}>", n.display()),
        }
    }

    /// Returns true iff `ev` would normally trigger this chord.
    pub fn matches(&self, ev: &KeyEvent) -> bool {
        match self {
            Self::Char(c) => {
                !ev.modifiers.contains(KeyModifiers::CONTROL) && ev.code == KeyCode::Char(*c)
            }
            Self::Ctrl(c) => {
                ev.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(ev.code,
                        KeyCode::Char(k) if k.eq_ignore_ascii_case(c))
            }
            Self::Named(n) => {
                let expected = match n {
                    NamedKey::Enter => KeyCode::Enter,
                    NamedKey::Space => KeyCode::Char(' '),
                    NamedKey::Tab => KeyCode::Tab,
                    NamedKey::Backspace => KeyCode::Backspace,
                    NamedKey::Esc => KeyCode::Esc,
                    NamedKey::Up => KeyCode::Up,
                    NamedKey::Down => KeyCode::Down,
                    NamedKey::Left => KeyCode::Left,
                    NamedKey::Right => KeyCode::Right,
                    NamedKey::Home => KeyCode::Home,
                    NamedKey::End => KeyCode::End,
                    NamedKey::PageUp => KeyCode::PageUp,
                    NamedKey::PageDown => KeyCode::PageDown,
                    NamedKey::Fn(n) => KeyCode::F(*n),
                };
                ev.code == expected
            }
        }
    }
}

/// What happens when a user binding fires.
///
/// Simple actions (quit, up, pick) just carry the `Action` enum. Actions
/// with inline data — `unix "cmd %"`, `patternpick =*.rs`, `jump =~/src` —
/// carry the string payload so the App can dispatch appropriately without
/// re-parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundAction {
    Plain(Action),
    UnixCmd(String),
    PatternPick(String),
    Jump(String),
    ToggleMaskFixed(u8),
    /// `map KEY command <name [args]>` — run a `:` command on keypress (e.g.
    /// `command graveyard`). The string is dispatched exactly like typed `:`
    /// input, so it can reach the `:!`/`:;` shell symbols — which is why this
    /// is [`is_executing`](Self::is_executing) (only `$HOME/.spycrc.toml` may
    /// bind it). Lets a user re-bind any feature that ships only as a `:`
    /// command (graveyard, activity, …).
    Command(String),
    /// `map KEY lua <name>` — run the Lua script `<config_root>/lua/<name>.lua`
    /// on keypress. Runs arbitrary code, so it's
    /// [`is_executing`](Self::is_executing) (only `$HOME` config may bind it; a
    /// project `.spycrc.toml` cannot).
    Lua(String),
}

impl BoundAction {
    /// Short human description for the help overlay.
    pub fn describe(&self) -> String {
        match self {
            Self::Plain(a) => a.describe().to_string(),
            Self::UnixCmd(cmd) => format!("shell: {cmd}"),
            Self::PatternPick(pat) => format!("pick pattern {pat}"),
            Self::Jump(path) => format!("jump to {path}"),
            Self::ToggleMaskFixed(n) => format!("toggle mask {n}"),
            Self::Command(cmd) => format!(":{cmd}"),
            Self::Lua(name) => format!("lua: {name}"),
        }
    }

    /// True for bindings that, on a single keypress, run a shell command or
    /// act on an arbitrary baked-in path — the capabilities an untrusted
    /// project-local `.spycrc.toml` must not be able to introduce (see
    /// `Config::load_default`). `Plain` built-in actions (incl. the
    /// copy/move/remove *prompts*, which carry no payload — the user still
    /// types the target) and the harmless `PatternPick`/`ToggleMaskFixed`
    /// are not executing. `Command` is — it dispatches arbitrary `:` input,
    /// including the `:!`/`:;` shell symbols.
    pub const fn is_executing(&self) -> bool {
        matches!(
            self,
            Self::UnixCmd(_) | Self::Jump(_) | Self::Command(_) | Self::Lua(_)
        )
    }
}

#[derive(Debug, Clone)]
pub struct UserBinding {
    pub chord: KeyChord,
    pub action: BoundAction,
}

/// A lookup table used by the resolver. Later entries override earlier
/// ones (so a project config can rebind a key the user config already set).
#[derive(Debug, Default, Clone)]
pub struct UserKeymap {
    entries: Vec<UserBinding>,
}

impl UserKeymap {
    pub const fn from_bindings(bindings: Vec<UserBinding>) -> Self {
        Self { entries: bindings }
    }

    /// Iterate over the bindings in the order they were declared. Used by
    /// the help overlay so the user sees their own `.spycrc` entries.
    pub fn iter(&self) -> std::slice::Iter<'_, UserBinding> {
        self.entries.iter()
    }

    pub fn find(&self, ev: &KeyEvent) -> Option<&BoundAction> {
        // Iterate in reverse so that later bindings win on conflicts.
        for b in self.entries.iter().rev() {
            if b.chord.matches(ev) {
                return Some(&b.action);
            }
        }
        None
    }
}
