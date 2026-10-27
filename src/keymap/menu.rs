//! The menu of the object under the cursor or the pointer: the rows `.`, `^a .`
//! and a right-click open, each naming the key that does the same and the
//! action a click fires. Pure, and decided from facts the app layer gathers,
//! so a row appears only where its action applies.

use super::action::Action;
use super::resolver::ChordEntry;
use crate::fs::EntryKind;

/// What a menu is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A file-list entry: the one under the cursor, or the row right-clicked.
    Entry(EntryFacts),
    /// A pane tab: the active one, or the label right-clicked.
    PaneTab(TabFacts),
}

/// What an entry's menu depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryFacts {
    pub kind: EntryKind,
    /// The row carries a git mark: staged, modified, untracked or deleted.
    pub git_changed: bool,
    /// The entry is inside a repository and not untracked.
    pub git_tracked: bool,
    /// A git-deleted file, drawn struck through: gone from disk, so only its
    /// history applies, and `gr` brings it back.
    pub deleted: bool,
    /// A pane tab is open to send the entry to.
    pub pane_open: bool,
}

/// What a pane tab's menu depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabFacts {
    /// A shell keeps `^z` for its own job control.
    pub is_shell: bool,
    /// `^a F` forks this kind of tab (`src/app/pane_fork.rs`).
    pub can_fork: bool,
    /// The child has exited: nothing to suspend or move to the tasks.
    pub exited: bool,
}

/// The rows of `target`'s menu, in the order they are drawn.
pub fn menu_for(target: Target) -> Vec<ChordEntry> {
    match target {
        Target::Entry(facts) => entry_rows(facts),
        Target::PaneTab(facts) => tab_rows(facts),
    }
}

/// The rows whose condition holds, each `(shown, keys, action)`.
fn shown<const N: usize>(table: [(bool, &'static str, Action); N]) -> Vec<ChordEntry> {
    table
        .into_iter()
        .filter(|(shown, ..)| *shown)
        .map(|(_, keys, action)| ChordEntry::Act(keys, action))
        .collect()
}

fn entry_rows(e: EntryFacts) -> Vec<ChordEntry> {
    use Action as A;
    if e.deleted {
        return shown([(true, "gd", A::GitDiff), (true, "gr", A::GitRestore)]);
    }
    let file = e.kind != EntryKind::Dir;
    shown([
        (true, "Enter", A::EnterOrDisplay),
        (file, "e", A::EnterOrEdit),
        (file, "V", A::EditInPane),
        (file, "D", A::DisplayInPane),
        (e.kind == EntryKind::Executable, ":exec", A::ExecuteEntry),
        (true, "t", A::TogglePick),
        (file, "yy", A::Take),
        (true, "yf", A::YankPaths),
        (true, "c", A::CopyPrompt),
        (true, "M", A::MovePrompt),
        (true, "dd", A::RemovePrompt(None)),
        (e.git_changed, "gd", A::GitDiff),
        (e.git_tracked && file, "gb", A::GitBlame),
        (e.kind == EntryKind::File, ":chmod", A::ChmodAdd('x')),
        (true, "Ha", A::HarpoonAppend),
        (e.pane_open, "^a s", A::PaneSendSelection),
        (file && e.pane_open, "^a P", A::PanePipeContent),
        (true, "O", A::NewFilePrompt),
        (true, "+", A::MakeDirPrompt),
    ])
}

/// A key that works only from the file list says so: from the pane it would be
/// typed into the child.
fn tab_rows(t: TabFacts) -> Vec<ChordEntry> {
    use Action as A;
    shown([
        (true, "^a R", A::PaneRestartTab),
        (!t.is_shell && !t.exited, "^z", A::PaneToggleSuspend),
        (t.can_fork, "^a F", A::PaneForkTab),
        (true, "^a r", A::PaneRenameTab),
        (true, "^a x", A::PaneCloseTab),
        (true, "^a z", A::TogglePaneZoom),
        (true, "^a v", A::PaneScrollEnter),
        (true, "^a u", A::QuickSelectOpen),
        (true, "^a c", A::PaneNewTab),
        (!t.exited, ":pane-to-task", A::PaneToTask),
        (true, "yp (list)", A::YankPrompt),
        (true, "ya (list)", A::YankScrollback),
        (true, "^a k", A::PaneFocusUp),
    ])
}

/// One target for every combination of the facts a row depends on: what the
/// guards walk.
#[cfg(test)]
pub fn every_target() -> Vec<Target> {
    let mut out = Vec::new();
    for kind in [
        EntryKind::Dir,
        EntryKind::Executable,
        EntryKind::File,
        EntryKind::Symlink,
        EntryKind::Other,
    ] {
        for bits in 0..16u8 {
            let bit = |n: u8| bits & (1 << n) != 0;
            out.push(Target::Entry(EntryFacts {
                kind,
                git_changed: bit(0),
                git_tracked: bit(1),
                deleted: bit(2),
                pane_open: bit(3),
            }));
        }
    }
    for bits in 0..8u8 {
        let bit = |n: u8| bits & (1 << n) != 0;
        out.push(Target::PaneTab(TabFacts {
            is_shell: bit(0),
            can_fork: bit(1),
            exited: bit(2),
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Target, every_target, menu_for};
    use crate::keymap::action::{Action, ProjectAction, every_action};
    use crate::keymap::resolver::tests::prefixes::chord_states;
    use crate::keymap::{ChordEntry, Resolver, Tier, UserKeymap};
    use std::mem::{Discriminant, discriminant};

    type Kind = (Discriminant<Action>, Option<Discriminant<ProjectAction>>);

    /// Which action `a` is, its payload aside.
    fn kind(a: &Action) -> Kind {
        let nested = match a {
            Action::Project(p) => Some(discriminant(p)),
            _ => None,
        };
        (discriminant(a), nested)
    }

    /// Frame and pane actions that no menu offers, each for its reason.
    const EXEMPT: &[Action] = &[
        // Cursor motion: where the cursor is, not what is under it.
        Action::Up(0),
        Action::Down(0),
        Action::Left(0),
        Action::Right(0),
        Action::PageUp,
        Action::PageDown,
        Action::GotoLast,
        // Moving between directories.
        Action::Climb,
        Action::Home,
        Action::JumpPrompt,
        Action::JumpStartDir,
        // Search and the finder.
        Action::SearchPrompt,
        Action::SearchNext,
        Action::SearchPrev,
        Action::FindFile,
        // Sort, filter and view settings: the listing's, not an entry's.
        Action::SortCycle,
        Action::ToggleMask(0),
        Action::LimitPrompt,
        Action::ToggleInventoryView,
        Action::OpenGraveyardView,
        // Picks and the inventory as a whole.
        Action::PickPatternPrompt,
        Action::PickToggleAll,
        Action::Untake,
        Action::Drop,
        Action::EmptyInventory,
        // Prompts for a command or a shell, about no object.
        Action::CommandPrompt,
        Action::ShellCapturedPrompt,
        Action::ShellForegroundPrompt,
        Action::StartShell,
        Action::SetEnvPrompt,
        // Reports over the selection, reached by `L` and `:filetype`.
        Action::LongList,
        Action::FileType,
        // Menu openers.
        Action::ContextMenu,
        // Pane keys with a home outside any menu: `F9`, scroll mode's `s`, and
        // `^a ^a`, which a chord row can't name.
        Action::ResumePane,
        Action::PaneScrollSave,
        Action::PaneLastTab,
    ];

    /// Every action a menu row fires: an object menu's, or a which-key popup's,
    /// walked through every chord state as the popup guards walk them.
    fn offered() -> Vec<Kind> {
        let mut out: Vec<Kind> = every_target()
            .into_iter()
            .flat_map(menu_for)
            .filter_map(|row| match row {
                ChordEntry::Act(_, a) | ChordEntry::Range(_, a) => Some(kind(&a)),
                ChordEntry::Sub(..) => None,
            })
            .collect();
        for (_, entry) in chord_states() {
            let mut r = Resolver::new();
            for ev in entry {
                r.feed(ev, &UserKeymap::default());
            }
            out.extend(r.continuations().iter().filter_map(|row| match row {
                ChordEntry::Act(_, a) | ChordEntry::Range(_, a) => Some(kind(a)),
                ChordEntry::Sub(..) => None,
            }));
        }
        out
    }

    /// A frame or pane action is offered by some menu, so a user who reaches for
    /// the pointer can find it, or is exempt for a reason [`EXEMPT`] gives. No
    /// second registry: a new action fails here until a menu offers it or the
    /// list says why none does.
    #[test]
    fn every_frame_and_pane_action_has_a_menu() {
        let offered = offered();
        let exempt: Vec<Kind> = EXEMPT.iter().map(kind).collect();
        let mut checked = 0;
        for action in every_action() {
            if !matches!(action.tier(), Tier::Frame | Tier::Pane) {
                continue;
            }
            checked += 1;
            let k = kind(&action);
            assert!(
                offered.contains(&k) || exempt.contains(&k),
                "{action:?} is a {:?} action no menu offers: add a row to `menu_for`, \
                 or exempt it with its reason",
                action.tier()
            );
        }
        assert!(
            checked > 80,
            "only {checked} frame and pane actions checked"
        );
        for action in EXEMPT {
            assert!(
                matches!(action.tier(), Tier::Frame | Tier::Pane),
                "{action:?} is {:?}, outside the guard: drop its exemption",
                action.tier()
            );
            assert!(
                !offered.contains(&kind(action)),
                "{action:?} is offered by a menu now: drop its exemption"
            );
        }
    }

    fn keys(target: Target) -> Vec<&'static str> {
        menu_for(target).iter().map(ChordEntry::keys).collect()
    }

    /// A git-deleted file is gone from disk, so its menu offers only its
    /// history: what it was, and bringing it back.
    #[test]
    fn a_deleted_files_menu_offers_its_history_only() {
        for target in every_target() {
            if let Target::Entry(facts) = target {
                let rows = keys(target);
                if facts.deleted {
                    assert_eq!(rows, ["gd", "gr"], "{facts:?}");
                } else {
                    assert!(!rows.contains(&"gr"), "{facts:?}: nothing to restore");
                }
            }
        }
    }
}
