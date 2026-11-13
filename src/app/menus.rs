//! The menu of an object: `.` opens the entry under the cursor's, `^a .` the
//! active pane tab's, and a right-click the menu of the row, pane, tab label,
//! divider, open pager or status-bar segment it lands on (`mouse/`): off the
//! pane divider's labels, the tabs' menu; on the split's divider, the split's;
//! on a project's label, that project's; on the collapsed count, the open
//! projects'; and on the git, agent or session segment, the repo's, the
//! active tab's or the session's; on the background tasks' indicator, the
//! tasks'. The rows come from [`crate::keymap::menu::menu_for`], or for a
//! pager from `pager_handler::pager_menu`; this module gathers the facts they
//! decide from and opens the menu as a which-key popup that no chord armed.
//! The git segment's adds a `:diff` row, run as typing `:diff` runs it;
//! an entry's, inside a mounted archive, the archive's `:archive write`,
//! `discard` and `unmount`; and the session segment's a `:name` row, which
//! opens the `:` line holding `name` and the session's name, for editing.
//!
//! While a menu is open it takes the keys first (`InputSink::Menu`): `↑`/`↓`
//! move a highlight, `←`/`→` move it to the entry drawn on the same line of
//! the previous or next column, over the columns `chord_hint_layout` draws,
//! `Enter` fires it as a click on the row would, and `Esc` closes. Any other
//! key closes the menu and goes where it would have gone with none open, so a
//! row's own key does what the row says. The make menu's filter takes the
//! characters first (`make_menu.rs`).

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use super::mouse::chord_hit::entry_across;
use super::pager_handler::pager_menu;
use super::state::archive::read_only_refusal;
use super::view_state::{CommandRow, MenuState};
use super::{App, ChordHint, Effect, HintEntry, View};
use crate::keymap::ChordEntry;
use crate::keymap::menu::{
    DividerFacts, EntryFacts, GitFacts, ProjectFacts, ProjectsFacts, SplitFacts, TabFacts, Target,
    menu_for,
};
use crate::ui::status_segments::StatusSegment;

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
            harpoon: c.harpoon.is_some(),
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
            fork_source: super::pane_fork::fork_source_known(info),
            exited: tabs.active().is_closed(),
        };
        Some((Target::PaneTab(facts), info.label.clone()))
    }

    /// Open `target`'s menu, titled `title`. An entry's ends with the
    /// archive's rows inside a mounted archive.
    fn open_menu(&mut self, target: Target, title: String) -> Option<Effect> {
        let mut rows = self.menu_rows(target);
        if matches!(target, Target::Entry(_)) {
            // What the staging watches saw since the loop last looked counts.
            self.note_staging_edits();
            rows.extend(self.archive_rows());
        }
        self.show_menu(format!("{title} · ? all keys"), rows)
    }

    /// `target`'s rows, each `(keys, label, entry)`. Inside a mounted archive
    /// the rows whose action the mount refuses are left out, as `apply_inner`
    /// would refuse them.
    fn menu_rows(&self, target: Target) -> Vec<(String, String, HintEntry)> {
        menu_for(target)
            .into_iter()
            .filter(|row| {
                !matches!(row, ChordEntry::Act(_, a) if self.state.mount_refusal(a).is_some())
            })
            .map(|e| {
                let label = e.label().to_string();
                (e.keys().to_string(), label, HintEntry::Chord(e))
            })
            .collect()
    }

    /// The git segment's menu: the repo's rows, `gM` among them.
    fn open_git_menu(&mut self) -> Option<Effect> {
        let rows = self.menu_rows(self.git_target());
        self.show_menu("git · ? all keys".to_string(), rows)
    }

    /// Inside a mounted archive, a row for each `:archive` verb that would
    /// act on it, run as typing it does: `write` while the mount has changes
    /// it can write, `discard` while it has changes, and `unmount`. Its
    /// changes are the ones `:archive write` finds. None while a write of it
    /// runs, which each verb refuses.
    fn archive_rows(&self) -> Vec<(String, String, HintEntry)> {
        let Some((mount, _)) = self.state.mounts.resolve(&self.state.cur().listing.dir) else {
            return Vec::new();
        };
        let archive = mount.archive();
        if self.runtime.archive_writes.contains_key(archive) {
            return Vec::new();
        }
        let changes = self.mount_is_dirty(archive);
        let writable = read_only_refusal(&mount.capability).is_none();
        [
            (changes && writable, "write", "write the changes back"),
            (changes, "discard", "discard the changes"),
            (true, "unmount", "unmount the archive"),
        ]
        .into_iter()
        .filter(|(shown, ..)| *shown)
        .map(|(_, verb, label)| {
            let line = format!("archive {verb}");
            let run = HintEntry::Command(Some(CommandRow::Typed(line.clone())));
            (format!(":{line}"), label.to_string(), run)
        })
        .collect()
    }

    /// The session segment's menu: the session's rows, then `:name`, which
    /// opens the `:` line holding `name` and the session's name, for editing.
    /// With no name, the line holds `name ` alone.
    fn open_session_menu(&mut self) -> Option<Effect> {
        let mut rows = self.menu_rows(Target::Session);
        let name = self.state.session_name.as_deref();
        rows.push((
            ":name".to_string(),
            "rename this session".to_string(),
            HintEntry::Command(Some(CommandRow::Edit(format!(
                "name {}",
                name.unwrap_or_default()
            )))),
        ));
        let title = name.unwrap_or("session");
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

    /// A right-click on the background tasks' indicator: `gB`, then a row per
    /// task that `:fg N` brings back, labelled with its command. `:fg` refuses
    /// every task while a capture is live, so then only `gB` is offered.
    pub(super) fn open_tasks_menu(&mut self) -> Vec<Effect> {
        let viewer = crate::keymap::Action::OpenTaskViewer;
        let key = super::mouse::TASK_VIEWER_KEY;
        let mut rows = vec![(
            key.to_string(),
            viewer.describe().to_string(),
            HintEntry::Chord(ChordEntry::Act(key, viewer)),
        )];
        if self.runtime.pending_capture.is_none() {
            rows.extend(self.runtime.background_tasks.tasks.iter().map(|task| {
                (
                    format!(":fg {} (list)", task.id),
                    task_label(&task.cmd_display),
                    HintEntry::Command(Some(CommandRow::Typed(format!("fg {}", task.id)))),
                )
            }));
        }
        self.show_menu("tasks".to_string(), rows)
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

    /// The menu of the status bar's `segment`, as the app stands: a project's
    /// label's is the project in view's, since a right-click has gone there.
    pub(super) fn open_status_menu(&mut self, segment: StatusSegment) -> Vec<Effect> {
        let (target, title) = match segment {
            StatusSegment::Project | StatusSegment::ProjectLabel(_) => {
                (self.project_target(), self.state.active_project().label())
            }
            StatusSegment::ProjectCount => {
                let projects = Target::Projects(self.projects_facts());
                (projects, "projects".to_string())
            }
            StatusSegment::Git => return self.open_git_menu().into_iter().collect(),
            StatusSegment::Session => return self.open_session_menu().into_iter().collect(),
            StatusSegment::Agent => return self.open_tab_menu(),
            StatusSegment::Projects
            | StatusSegment::Path
            | StatusSegment::PathUp(_)
            | StatusSegment::Suffix => {
                return Vec::new();
            }
        };
        self.open_menu(target, title).into_iter().collect()
    }

    /// What the open projects' rows depend on.
    fn projects_facts(&self) -> ProjectsFacts {
        ProjectsFacts {
            several: self.state.open_projects().nth(1).is_some(),
            waiting: self.someone_waits(),
        }
    }

    /// The project in view as a menu target, with what `Space x` and
    /// `Space P` would refuse.
    fn project_target(&self) -> Target {
        let projects = self.projects_facts();
        let project = self.state.active_project();
        let here = &self.state.cur().listing.dir;
        Target::Project(ProjectFacts {
            projects,
            closable: projects.several && !matches!(project.focus, super::state::Focus::Overlay),
            home: project.home.is_some(),
            rehome: self.state.home_taken(here, self.state.active).is_none(),
        })
    }

    /// The focused column's repo as a menu target. Its changes are the ones
    /// the bar's `*` counts; a worktree is removable as `Space w d` removes
    /// it, from its root, clean, unclaimed, with the other column outside it.
    fn git_target(&self) -> Target {
        let c = self.state.cur();
        let root = c.git_cache.current_repo_root.as_deref();
        let changes = c
            .git_cache
            .git_status_cache
            .as_ref()
            .is_some_and(|s| Some(s.repo_root.as_path()) == root && !s.entries.is_empty());
        // A linked worktree's gitdir lives in the main repo's, outside it.
        let linked_root = root.is_some_and(|root| {
            c.listing.dir == root
                && c.git_cache
                    .current_gitdir
                    .as_deref()
                    .is_some_and(|gitdir| !gitdir.starts_with(root))
        });
        let other_inside = self
            .state
            .get_col(self.state.focused_side().other())
            .is_some_and(|o| o.listing.dir.starts_with(&c.listing.dir));
        Target::Git(GitFacts {
            changes,
            removable: linked_root && !changes && !other_inside && !c.git_cache.worktree_locked,
        })
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
        if self.make_filter_key(key) {
            return Ok(Vec::new());
        }
        let Some(hint) = self.view.chord_hint.as_ref() else {
            return self.handle_key(key);
        };
        let len = hint.entries.len();
        let selected = hint.menu.as_ref().and_then(|m| m.selected);
        let pager = hint.is_pager_menu();
        match (key.code, selected) {
            (KeyCode::Down | KeyCode::Up, _) if plain && len > 0 => {
                let next = match (key.code, selected) {
                    (KeyCode::Down, Some(i)) => (i + 1) % len,
                    (KeyCode::Down, None) => 0,
                    (_, Some(i)) => (i + len - 1) % len,
                    (_, None) => len - 1,
                };
                self.select_menu_row(next);
                Ok(Vec::new())
            }
            (KeyCode::Left | KeyCode::Right, _) if plain => {
                let right = key.code == KeyCode::Right;
                if let Some(next) = self.menu_row_across(selected, right) {
                    self.select_menu_row(next);
                }
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

    /// Highlight the open menu's entry `index`.
    const fn select_menu_row(&mut self, index: usize) {
        if let Some(hint) = self.view.chord_hint.as_mut()
            && let Some(menu) = hint.menu.as_mut()
        {
            menu.selected = Some(index);
        }
    }

    /// The entry `→` (`right`) or `←` moves the open menu's highlight to from
    /// `selected`, over its columns as the frame draws them.
    fn menu_row_across(&self, selected: Option<usize>, right: bool) -> Option<usize> {
        let (cols, rows) = self.view.term_size;
        let frame = Rect::new(0, 0, cols, rows);
        let drawn = self.chord_hint_drawn(&self.frame_layout(frame), frame)?;
        entry_across(&drawn, selected, right)
    }
}

/// How wide a task's row label may be before it is cut.
const TASK_LABEL_WIDTH: usize = 40;

/// A task's row label: its command, without anything a terminal would read as
/// a control sequence, cut to [`TASK_LABEL_WIDTH`] columns.
fn task_label(command: &str) -> String {
    let clean: String = command.chars().filter(|c| !c.is_control()).collect();
    if crate::ui::display_width(&clean) <= TASK_LABEL_WIDTH {
        return clean;
    }
    let cut = crate::ui::display_truncate(&clean, TASK_LABEL_WIDTH - 1);
    format!("{cut}\u{2026}")
}

#[cfg(test)]
mod tests {
    use super::super::command_table::COMMAND_TABLE;
    use super::super::key_dispatch::{PaneKeyAction, pane_suspend_key_action};
    use crate::keymap::menu::{every_target, menu_for};
    use crate::keymap::{ChordEntry, Resolver, ResolverOutcome, UserKeymap};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// The keystrokes a row's keys name, from the file list: `^a X` is `^a`
    /// then `X`, as `^s X` is `^s` then `X`, `Space X` is the leader then `X`
    /// (`Space w l` the leader, `w`, then `l`), a ` (list)` note is the focus
    /// the keys work from, and each other character is one key.
    fn keystrokes(keys: &str) -> Vec<KeyEvent> {
        let keys = keys.trim_end_matches(" (list)");
        if keys == "Enter" {
            return vec![KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)];
        }
        if let Some(rest) = keys.strip_prefix("Space ") {
            return std::iter::once(' ')
                .chain(rest.split_whitespace().flat_map(str::chars))
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
                        ResolverOutcome::Action(action),
                        "{keys} fires what its row does"
                    );
                }
                checked += 1;
            }
        }
        assert!(checked > 500, "only {checked} rows checked");
    }
}
