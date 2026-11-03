//! `[ui]`: how the menus teach the keys. A menu row fired by a click or `Enter`
//! names the key that does the same, and `menu_hints` says for how long: until
//! the user has fired the action by its key `menu_hints_fade_after` times
//! (`"fade"`), always, or never (`"off"`, which also counts nothing and hides
//! the status bar's `. menu`).

use std::num::NonZeroU32;

use serde::Deserialize;

/// `[ui] menu_hints`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuHints {
    /// Name the key until the user has fired the action by it often enough.
    #[default]
    Fade,
    Always,
    /// Name no key and count nothing.
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiConfig {
    pub menu_hints: MenuHints,
    /// Key presses of an action before its hint stops; at least 1.
    pub menu_hints_fade_after: u32,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            menu_hints: MenuHints::Fade,
            menu_hints_fade_after: 3,
        }
    }
}

impl UiConfig {
    /// Whether a menu click names the key of an action whose key has fired it
    /// `count` times.
    pub const fn names_the_key(self, count: u32) -> bool {
        match self.menu_hints {
            MenuHints::Fade => count < self.menu_hints_fade_after,
            MenuHints::Always => true,
            MenuHints::Off => false,
        }
    }

    /// The most a key press raises an action's count to; `None` when nothing
    /// is counted. `"always"` counts too, so a switch back to `"fade"` finds
    /// what the user has learned.
    pub const fn count_cap(self) -> Option<u32> {
        match self.menu_hints {
            MenuHints::Off => None,
            MenuHints::Fade | MenuHints::Always => Some(self.menu_hints_fade_after),
        }
    }

    /// Whether the menus teach at all: the hints, and the status bar's `. menu`.
    pub const fn teaches(self) -> bool {
        !matches!(self.menu_hints, MenuHints::Off)
    }

    pub(super) const fn merge(&mut self, file: &FileUi) {
        if let Some(mode) = file.menu_hints {
            self.menu_hints = mode;
        }
        if let Some(n) = file.menu_hints_fade_after {
            self.menu_hints_fade_after = n.get();
        }
    }
}

/// On-disk shape of `[ui]`. A `menu_hints_fade_after` of 0 fails the file, as
/// a value of the wrong type does.
#[derive(Debug, Default, Deserialize)]
pub(super) struct FileUi {
    #[serde(default)]
    menu_hints: Option<MenuHints>,
    #[serde(default)]
    menu_hints_fade_after: Option<NonZeroU32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_names_the_key_below_the_threshold_only() {
        let ui = UiConfig::default();
        assert!(ui.names_the_key(2));
        assert!(!ui.names_the_key(3));
        let always = UiConfig {
            menu_hints: MenuHints::Always,
            ..ui
        };
        assert!(always.names_the_key(99));
        let off = UiConfig {
            menu_hints: MenuHints::Off,
            ..ui
        };
        assert!(!off.names_the_key(0));
        assert_eq!(off.count_cap(), None);
    }
}
