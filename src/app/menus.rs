//! The menu of an object: `.` opens the entry under the cursor's, `^a .` the
//! active pane tab's, and a right-click the menu of the row, pane or tab label
//! it lands on (`mouse/`). The rows come from [`crate::keymap::menu::menu_for`];
//! this module gathers the facts it decides from and opens the menu as a
//! which-key popup that no chord armed.
//!
//! While a menu is open it takes the keys first (`InputSink::Menu`): `↑`/`↓`
//! move a highlight, `Enter` fires it as a click on the row would, and `Esc`
//! closes. Any other key closes the menu and goes where it would have gone
//! with none open, so a row's own key does what the row says.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::view_state::MenuState;
use super::{App, ChordHint, Effect, View};
use crate::keymap::ChordEntry;
use crate::keymap::menu::{EntryFacts, TabFacts, Target, menu_for};

impl App {
    /// The focused column's cursor entry as a menu target, with its name for
    /// the title. `None` on an empty listing, and outside the directory view,
    /// where the keys a row shows mean something else.
    fn entry_target(&self) -> Option<(Target, String)> {
        let c = self.state.cur();
        if c.view != View::Dir {
            return None;
        }
        let row = c.rows.get(c.cursor.index)?;
        let git = c.git.files.get(row.git_key()).copied().unwrap_or_default();
        let facts = EntryFacts {
            kind: row.kind,
            git_changed: !git.is_clean(),
            git_tracked: c.git.info.is_some() && !git.untracked,
            deleted: row.deleted,
            pane_open: self.runtime.project().pane_tabs.is_some(),
        };
        let name = row
            .path
            .file_name()
            .map_or_else(|| row.display.clone(), |n| n.to_string_lossy().into_owned());
        Some((Target::Entry(facts), name))
    }

    /// The active pane tab as a menu target, with its label for the title.
    fn tab_target(&self) -> Option<(Target, String)> {
        let tabs = self.runtime.project().pane_tabs.as_ref()?;
        let info = tabs.active_info();
        let facts = TabFacts {
            is_shell: crate::shell::command_is_shell(&info.command),
            can_fork: super::pane_fork::forks(info),
            exited: tabs.active().is_closed(),
        };
        Some((Target::PaneTab(facts), info.label.clone()))
    }

    /// Open `target`'s menu, titled `title`. Inside a mounted archive the rows
    /// whose action the mount refuses are left out, as `apply_inner` would
    /// refuse them.
    fn open_menu(&mut self, target: Target, title: String) {
        let c = self.state.cur();
        let in_mount = self.state.mounts.contains(&c.listing.dir);
        let entries: Vec<ChordEntry> = menu_for(target)
            .into_iter()
            .filter(|row| {
                !(in_mount
                    && matches!(row, ChordEntry::Act(_, a)
                        if super::state::archive::refusal(a).is_some()))
            })
            .collect();
        let rows = entries.iter().map(|e| (e.keys(), e.label())).collect();
        self.view.chord_hint = Some(ChordHint {
            title,
            entries,
            rows,
            menu: Some(MenuState::default()),
        });
        self.view.chord_hint_due = None;
    }

    /// `.`: the menu of the entry under the cursor.
    pub(super) fn open_entry_menu(&mut self) -> Vec<Effect> {
        match self.entry_target() {
            Some((target, title)) => self.open_menu(target, title),
            None if self.state.cur().view != View::Dir => self
                .state
                .flash_info("menu: an entry has a menu in a directory listing only"),
            None => self.state.flash_info("menu: no entry under the cursor"),
        }
        Vec::new()
    }

    /// `^a .`: the menu of the active pane tab.
    pub(super) fn open_tab_menu(&mut self) -> Vec<Effect> {
        match self.tab_target() {
            Some((target, title)) => self.open_menu(target, title),
            None => self.state.flash_info("menu: no pane tab is open"),
        }
        Vec::new()
    }

    /// Whether the open popup is an object's menu rather than a chord's.
    pub(super) fn menu_is_open(&self) -> bool {
        self.view
            .chord_hint
            .as_ref()
            .is_some_and(|hint| hint.menu.is_some())
    }

    /// A key while a menu is open: the menu's own, or any other, which closes
    /// the menu and is dispatched as though none had been open.
    pub(super) fn handle_menu_key(&mut self, key: KeyEvent) -> Result<Vec<Effect>> {
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let Some(hint) = self.view.chord_hint.as_mut() else {
            return self.handle_key(key);
        };
        let len = hint.entries.len();
        let selected = hint.menu.and_then(|m| m.selected);
        match (key.code, selected) {
            (KeyCode::Down | KeyCode::Up, _) if plain && len > 0 => {
                let next = match (key.code, selected) {
                    (KeyCode::Down, Some(i)) => (i + 1) % len,
                    (KeyCode::Down, None) => 0,
                    (_, Some(i)) => (i + len - 1) % len,
                    (_, None) => len - 1,
                };
                hint.menu = Some(MenuState {
                    selected: Some(next),
                });
                Ok(Vec::new())
            }
            (KeyCode::Enter, Some(i)) if plain => Ok(self.fire_chord_row(i)),
            (KeyCode::Esc, _) => {
                self.clear_chord_hint();
                Ok(Vec::new())
            }
            _ => {
                self.clear_chord_hint();
                self.handle_key(key)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::command_table::COMMAND_TABLE;
    use super::super::key_dispatch::{PaneKeyAction, pane_suspend_key_action};
    use crate::keymap::menu::{every_target, menu_for};
    use crate::keymap::{ChordEntry, Resolver, ResolverOutcome, UserKeymap};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// The keystrokes a row's keys name, from the file list: `^a X` is `^a`
    /// then `X`, a ` (list)` note is the focus the keys work from, and each
    /// other character is one key.
    fn keystrokes(keys: &str) -> Vec<KeyEvent> {
        let keys = keys.trim_end_matches(" (list)");
        if keys == "Enter" {
            return vec![KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)];
        }
        let (mut out, rest) = match keys.strip_prefix("^a ") {
            Some(rest) => (
                vec![KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)],
                rest,
            ),
            None => (Vec::new(), keys),
        };
        out.extend(
            rest.chars()
                .map(|c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
        );
        out
    }

    /// Each menu row's key does what a click on the row does, so the menu
    /// teaches a key that works: fed to a fresh resolver from the file list, it
    /// fires the row's action. A `:name` row names a command that exists, and
    /// `^z`, which the pane's own key path reads, toggles a tab's suspend.
    #[test]
    fn every_menu_rows_key_fires_its_action() {
        let mut checked = 0;
        for target in every_target() {
            for row in menu_for(target) {
                let ChordEntry::Act(keys, action) = row else {
                    panic!("{target:?}: a menu row fires one action: {row:?}");
                };
                if let Some(name) = keys.strip_prefix(':') {
                    assert!(
                        COMMAND_TABLE.iter().any(|c| c.name == name),
                        "{keys}: no such command"
                    );
                } else if keys == "^z" {
                    let ctrl_z = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL);
                    assert_eq!(
                        pane_suspend_key_action(ctrl_z, false, false),
                        PaneKeyAction::Toggle,
                        "^z suspends a tab that isn't a shell"
                    );
                } else {
                    let mut resolver = Resolver::new();
                    let strokes = keystrokes(keys);
                    let (last, chord) = strokes.split_last().expect("a row names a key");
                    for ev in chord {
                        assert_eq!(
                            resolver.feed(*ev, &UserKeymap::default()),
                            ResolverOutcome::Pending,
                            "{keys}: {ev:?} arms the chord"
                        );
                    }
                    assert_eq!(
                        resolver.feed(*last, &UserKeymap::default()),
                        ResolverOutcome::Action(action.clone()),
                        "{keys} fires what its row does"
                    );
                }
                checked += 1;
            }
        }
        assert!(checked > 500, "only {checked} rows checked");
    }
}
