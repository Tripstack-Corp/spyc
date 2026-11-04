//! The menu of an object: `.` opens the entry under the cursor's, `^a .` the
//! active pane tab's, and a right-click the menu of the row, pane, tab label,
//! divider or open pager it lands on (`mouse/`): off the pane divider's labels,
//! the tabs' menu, and on the split's divider, the split's. The rows come from
//! [`crate::keymap::menu::menu_for`], or for a pager from
//! `pager_handler::pager_menu`; this module gathers the facts they decide from
//! and opens the menu as a which-key popup that no chord armed.
//!
//! While a menu is open it takes the keys first (`InputSink::Menu`): `↑`/`↓`
//! move a highlight, `Enter` fires it as a click on the row would, and `Esc`
//! closes. Any other key closes the menu and goes where it would have gone
//! with none open, so a row's own key does what the row says.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::pager_handler::pager_menu;
use super::view_state::MenuState;
use super::{App, ChordHint, Effect, HintEntry, View};
use crate::keymap::ChordEntry;
use crate::keymap::menu::{DividerFacts, EntryFacts, SplitFacts, TabFacts, Target, menu_for};

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
            makefile: crate::taskfile::is_makefile(&row.path),
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
    fn open_menu(&mut self, target: Target, title: String) -> Option<Effect> {
        let c = self.state.cur();
        let in_mount = self.state.mounts.contains(&c.listing.dir);
        let rows = menu_for(target)
            .into_iter()
            .filter(|row| {
                !(in_mount
                    && matches!(row, ChordEntry::Act(_, a)
                        if super::state::archive::refusal(a).is_some()))
            })
            .map(|e| {
                let label = e.label().to_string();
                (e.keys().to_string(), label, HintEntry::Chord(e))
            })
            .collect();
        self.show_menu(format!("{title} · ? all keys"), rows)
    }

    /// Show a menu of `rows`, each `(keys, label, entry)`, titled `title`.
    /// The first menu ever opened retires the status bar's `. menu`, and
    /// returns the write that records it.
    pub(super) fn show_menu(
        &mut self,
        title: String,
        rows: Vec<(String, String, HintEntry)>,
    ) -> Option<Effect> {
        let (rows, entries) = rows
            .into_iter()
            .map(|(keys, label, entry)| ((keys, label), entry))
            .unzip();
        self.view.chord_hint = Some(ChordHint {
            title,
            entries,
            rows,
            prefix: "",
            menu: Some(MenuState::default()),
        });
        self.view.chord_hint_due = None;
        self.learn_menu()
    }

    /// A right-click on an open pager: its menu, titled with its title. Its
    /// keys are the pager's, so `?` is too, and the title doesn't offer it.
    pub(super) fn open_pager_menu(&mut self) -> Vec<Effect> {
        let Some((facts, title)) = self.pager_facts() else {
            return Vec::new();
        };
        let task = facts.task.map(|t| t.id);
        let rows = pager_menu(facts)
            .into_iter()
            .map(|a| (a.key(task), a.describe().to_string(), HintEntry::Pager(a)))
            .collect();
        self.show_menu(title, rows).into_iter().collect()
    }

    /// `.`: the menu of the entry under the cursor.
    pub(super) fn open_entry_menu(&mut self) -> Vec<Effect> {
        match self.entry_target() {
            Some((target, title)) => return self.open_menu(target, title).into_iter().collect(),
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
            Some((target, title)) => return self.open_menu(target, title).into_iter().collect(),
            None => self.state.flash_info("menu: no pane tab is open"),
        }
        Vec::new()
    }

    /// A right-click on the pane divider, off its tab labels: the menu of the
    /// tabs as a whole.
    pub(super) fn open_divider_menu(&mut self) -> Vec<Effect> {
        let tabs = self
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .map_or(0, crate::pane::tabs::PaneTabs::len);
        let target = Target::Divider(DividerFacts { tabs });
        self.open_menu(target, "pane tabs".to_string())
            .into_iter()
            .collect()
    }

    /// A right-click on the split's vertical divider: the split's menu, as
    /// it applies to a preview or to a second commander.
    pub(super) fn open_split_menu(&mut self) -> Vec<Effect> {
        let commander = self.state.has_col(super::state::Side::Right);
        let target = Target::VDivider(SplitFacts { commander });
        self.open_menu(target, "split".to_string())
            .into_iter()
            .collect()
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
        let pager = hint.is_pager_menu();
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
            // The key reference, from the pane too, where `?` would otherwise
            // be typed into the agent. A pager's `?` is its own.
            (KeyCode::Char('?'), _) if plain && !pager => {
                self.clear_chord_hint();
                self.update(super::update::UiMsg::Action(crate::keymap::Action::Help))
            }
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
    /// then `X`, as `^s X` is `^s` then `X`, `Space X` is the leader then `X`,
    /// a ` (list)` note is the focus the keys work from, and each other
    /// character is one key.
    fn keystrokes(keys: &str) -> Vec<KeyEvent> {
        let keys = keys.trim_end_matches(" (list)");
        if keys == "Enter" {
            return vec![KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)];
        }
        if let Some(rest) = keys.strip_prefix("Space ") {
            return std::iter::once(' ')
                .chain(rest.chars())
                .map(|c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
                .collect();
        }
        let prefix = ['a', 's']
            .into_iter()
            .find_map(|c| Some((c, keys.strip_prefix(&format!("^{c} "))?)));
        let (mut out, rest) = match prefix {
            Some((c, rest)) => (
                vec![KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)],
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
