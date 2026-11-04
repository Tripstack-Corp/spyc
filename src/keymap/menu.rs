//! The menu of the object under the cursor or the pointer: the rows `.`, `^a .`
//! and a right-click open, each naming the key that does the same and the
//! action a click fires. Pure, and decided from facts the app layer gathers,
//! so a row appears only where its action applies.

use super::action::{Action, ProjectAction};
use super::resolver::ChordEntry;
use crate::fs::EntryKind;

/// What a menu is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A file-list entry: the one under the cursor, or the row right-clicked.
    Entry(EntryFacts),
    /// A pane tab: the active one, or the label right-clicked.
    PaneTab(TabFacts),
    /// The pane's tabs as a whole: the divider, right-clicked off its labels.
    Divider(DividerFacts),
    /// The vertical split: its divider, right-clicked.
    VDivider(SplitFacts),
    /// The project in view: its label on the status bar, or the lone
    /// project's segment, right-clicked.
    Project(ProjectFacts),
    /// The open projects as a whole: the status bar's collapsed count.
    Projects(ProjectsFacts),
    /// The focused column's repo: the status bar's git segment.
    Git(GitFacts),
    /// The session: the status bar's session segment.
    Session,
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
    /// A file named as make names a Makefile, whose targets `Space m` lists.
    pub makefile: bool,
    /// The column has a harpoon list for `Ha` to append to: its repo's, or
    /// the project home's.
    pub harpoon: bool,
}

/// What a pane tab's menu depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabFacts {
    /// A shell keeps `^z` for its own job control.
    pub is_shell: bool,
    /// `^a F` forks this kind of tab (`src/app/pane_fork.rs`).
    pub can_fork: bool,
    /// What `^a F` forks from is known: a claude or codex tab has a
    /// conversation pinned to it, codex's with its launch home.
    pub fork_source: bool,
    /// The child has exited: nothing to suspend or move to the tasks.
    pub exited: bool,
}

/// What the pane divider's menu depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DividerFacts {
    /// How many pane tabs are open: stepping between them takes two.
    pub tabs: usize,
}

/// What the split's menu depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitFacts {
    /// The right column holds a second commander, not a preview: `^s x`
    /// closes it, and `^a |` refuses.
    pub commander: bool,
}

/// What the open projects' rows depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectsFacts {
    /// Another project is open to step to: with one, `Space ]` and `Space [`
    /// do nothing.
    pub several: bool,
    /// An agent in some project is waiting on the user, for `Space !`.
    pub waiting: bool,
}

/// What the project in view's menu depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectFacts {
    pub projects: ProjectsFacts,
    /// `Space x` closes it: another project is open, and no editor runs in
    /// its view.
    pub closable: bool,
    /// It has a home for `Space p` to jump to.
    pub home: bool,
    /// The focused column's directory is no other open project's home or
    /// anchor, so `Space P` can make it this one's.
    pub rehome: bool,
}

/// What the git segment's menu depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitFacts {
    /// The repo has changes for `gd` to show.
    pub changes: bool,
    /// The focused column is at the root of a clean linked worktree that the
    /// other column isn't inside: `Space w d` can remove it.
    pub removable: bool,
}

/// The key a menu row stands for, as it is typed: the chord's `prefix` and the
/// row's first key, joined the way they are written (`gg`, `Wl`, `^a z`,
/// `Space p`). An object's menu has no prefix, and its rows' keys are already
/// whole (`^a R`, `:exec`).
pub fn key_hint(prefix: &str, keys: &str) -> String {
    if prefix.is_empty() {
        return keys.to_string();
    }
    let key = keys.split_whitespace().next().unwrap_or(keys);
    if prefix.chars().count() == 1 {
        format!("{prefix}{key}")
    } else {
        format!("{prefix} {key}")
    }
}

/// The rows of `target`'s menu, in the order they are drawn.
pub fn menu_for(target: Target) -> Vec<ChordEntry> {
    match target {
        Target::Entry(facts) => entry_rows(facts),
        Target::PaneTab(facts) => tab_rows(facts),
        Target::Divider(facts) => divider_rows(facts),
        Target::VDivider(facts) => split_rows(facts),
        Target::Project(facts) => project_rows(facts),
        Target::Projects(facts) => projects_rows(facts),
        Target::Git(facts) => git_rows(facts),
        Target::Session => shown([(true, "Space s", Action::ShowMemory)]),
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
    // A FIFO or a device named `Makefile` is no Makefile.
    let regular = matches!(
        e.kind,
        EntryKind::File | EntryKind::Executable | EntryKind::Symlink
    );
    shown([
        (e.makefile && regular, "Space m", A::MakeMenu),
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
        (e.harpoon, "Ha", A::HarpoonAppend),
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
        (t.can_fork && t.fork_source, "^a F", A::PaneForkTab),
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

fn divider_rows(d: DividerFacts) -> Vec<ChordEntry> {
    use Action as A;
    let several = d.tabs > 1;
    shown([
        (true, "^a c", A::PaneNewTab),
        (several, "^a n", A::PaneNextTab),
        (several, "^a p", A::PanePrevTab),
        (true, "^a z", A::TogglePaneZoom),
        (true, "^a +", A::PaneGrow),
        (true, "^a -", A::PaneShrink),
    ])
}

/// The `^a` keys work from the pane and the list alike; `^s` typed in the
/// pane reaches the child.
fn split_rows(s: SplitFacts) -> Vec<ChordEntry> {
    use Action as A;
    shown([
        (!s.commander, "^a |", A::VsplitToggle),
        (true, "^a f", A::VsplitToggleHeight),
        (true, "^a h", A::VsplitFocusLeft),
        (true, "^a l", A::VsplitFocusRight),
        (true, "^a d", A::ToggleDim),
        (s.commander, "^s x (list)", A::CloseSecondCommander),
    ])
}

/// The project in view's own rows, then the open projects'. No row goes to
/// it: its menu opens once it is in view.
fn project_rows(p: ProjectFacts) -> Vec<ChordEntry> {
    use Action as A;
    use ProjectAction as P;
    let mut rows = shown([
        (p.closable, "Space x", A::Project(P::Close)),
        (p.home, "Space p", A::Project(P::JumpHome)),
        (p.rehome, "Space P", A::Project(P::SetHomeHere)),
    ]);
    rows.extend(projects_rows(p.projects));
    rows
}

fn projects_rows(p: ProjectsFacts) -> Vec<ChordEntry> {
    use Action as A;
    use ProjectAction as P;
    shown([
        (true, "Space j", A::Project(P::List)),
        (true, "Space n", A::Project(P::Open)),
        (p.several, "Space ]", A::Project(P::Next)),
        (p.several, "Space [", A::Project(P::Prev)),
        (p.waiting, "Space !", A::Project(P::Attention)),
    ])
}

/// The worktree keys are named on the leader, where the workspace's keys
/// live; `W` is the file list's alias for them.
fn git_rows(g: GitFacts) -> Vec<ChordEntry> {
    use Action as A;
    shown([
        (true, "Space w l", A::WorktreeList),
        (true, "Space w n", A::WorktreeNew),
        (g.removable, "Space w d", A::WorktreeDelete),
        (g.changes, "gd", A::GitDiff),
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
        for bits in 0..64u8 {
            let bit = |n: u8| bits & (1 << n) != 0;
            out.push(Target::Entry(EntryFacts {
                kind,
                git_changed: bit(0),
                git_tracked: bit(1),
                deleted: bit(2),
                pane_open: bit(3),
                makefile: bit(4),
                harpoon: bit(5),
            }));
        }
    }
    for bits in 0..16u8 {
        let bit = |n: u8| bits & (1 << n) != 0;
        out.push(Target::PaneTab(TabFacts {
            is_shell: bit(0),
            can_fork: bit(1),
            fork_source: bit(3),
            exited: bit(2),
        }));
    }
    for tabs in 0..3 {
        out.push(Target::Divider(DividerFacts { tabs }));
    }
    for commander in [false, true] {
        out.push(Target::VDivider(SplitFacts { commander }));
    }
    for bits in 0..32u8 {
        let bit = |n: u8| bits & (1 << n) != 0;
        let projects = ProjectsFacts {
            several: bit(0),
            waiting: bit(1),
        };
        if bits < 4 {
            out.push(Target::Projects(projects));
            out.push(Target::Git(GitFacts {
                changes: bit(0),
                removable: bit(1),
            }));
        }
        out.push(Target::Project(ProjectFacts {
            projects,
            closable: bit(2),
            home: bit(3),
            rehome: bit(4),
        }));
    }
    out.push(Target::Session);
    out
}

#[cfg(test)]
mod tests {
    use super::{Target, every_target, key_hint, menu_for};

    /// A one-character prefix joins its row's key as typed; a longer one takes
    /// a space; an object menu's whole keys pass through; of several keys on a
    /// row, the first is named.
    #[test]
    fn a_rows_key_is_named_as_it_is_typed() {
        assert_eq!(key_hint("g", "g"), "gg");
        assert_eq!(key_hint("W", "l"), "Wl");
        assert_eq!(key_hint("^a", "z"), "^a z");
        assert_eq!(key_hint("Space", "p"), "Space p");
        assert_eq!(key_hint("^a", "n ]"), "^a n");
        assert_eq!(key_hint("", "^a R"), "^a R");
        assert_eq!(key_hint("", "yp (list)"), "yp (list)");
    }
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
