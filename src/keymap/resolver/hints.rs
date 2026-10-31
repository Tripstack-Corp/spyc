//! The which-key popup's rows: for each pending chord, the keys that complete
//! it and what each does. One table, read by the popup, by a click on one of
//! its rows, and by the guards that feed every listed key back through the
//! resolver.

use super::{BRACKET_JUMPS, PendingSeq, Resolver};
use crate::keymap::action::{Action, ProjectAction};

/// One row of the which-key chord-hint popup for a pending chord: a key that
/// fires an `Action`, a range of keys that each fire their own, or a key that
/// opens a nested submenu (shown with its own label). Returned by
/// [`Resolver::continuations`].
#[derive(Debug, Clone)]
pub enum ChordEntry {
    /// `key` fires `action`; the popup renders `key → action.describe()`.
    Act(&'static str, Action),
    /// `keys` name a range (`1-9`, `a-z`) whose every member fires its own
    /// instance of `action`, which is the first member's and stands for the
    /// rest in the popup. A click can't choose a member, so it fires none.
    Range(&'static str, Action),
    /// `key` opens the submenu `seq`, which the popup labels `label` (e.g.
    /// `"worktree"`). A click enters `seq` as the key does; the display
    /// string can't be fed back, since it may list alternatives.
    Sub(&'static str, &'static str, PendingSeq),
}

impl ChordEntry {
    /// The keys the popup shows for this row.
    pub const fn keys(&self) -> &'static str {
        match self {
            Self::Act(keys, _) | Self::Range(keys, _) | Self::Sub(keys, _, _) => keys,
        }
    }

    /// The row's label: its action's `describe()`, or the submenu's name.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Act(_, action) | Self::Range(_, action) => action.describe(),
            Self::Sub(_, label, _) => label,
        }
    }
}

/// The chord hint's rows for a pending `]` (`next`) or `[`.
fn bracket_rows(next: bool) -> Vec<ChordEntry> {
    BRACKET_JUMPS
        .into_iter()
        .map(|(key, fwd, back)| ChordEntry::Act(key, if next { fwd } else { back }))
        .collect()
}

impl Resolver {
    /// The built-in keys that complete the current pending chord, each paired
    /// with the `Action` it fires. This is the data behind the which-key hint
    /// popup (rendered as `keys → action.describe()`); user-keymap bindings
    /// under the prefix are merged on top by the popup builder.
    ///
    /// Authored alongside `feed`'s chord arms — the exhaustive match means a
    /// new `PendingSeq` variant *must* get an entry here (a build error
    /// otherwise), and `chord_continuations_resolve_to_their_actions` guards
    /// that every listed single-byte key actually resolves to its action, so
    /// the popup can't silently drift from the real bindings.
    ///
    /// A display string may name several keys for one action (`"a h"`,
    /// `"\\ C"`); each single-byte ASCII one among them is feed-verified, so an
    /// alias listed in a shared row is checked like any other key. A
    /// [`ChordEntry::Range`] (`"1-9"`, `"a-z"`) has its first member fed; a
    /// non-char key (`"↓"`) names no key that can be fed and is not. A
    /// [`ChordEntry::Sub`] names a key that opens a nested submenu rather than
    /// firing an action.
    pub fn continuations(&self) -> Vec<ChordEntry> {
        use Action as A;
        use ChordEntry::{Act, Range, Sub};
        match self.pending {
            PendingSeq::Normal => Vec::new(),
            // Leader (global / workspace): `Space` in list focus, `^a Space`
            // from pane focus.
            PendingSeq::Leader => vec![
                Sub("w", "worktree", PendingSeq::Worktree),
                Act("p", A::Project(ProjectAction::JumpHome)),
                Act("P", A::Project(ProjectAction::SetHomeHere)),
                Act("n", A::Project(ProjectAction::Open)),
                Act("x", A::Project(ProjectAction::Close)),
                Act("j", A::Project(ProjectAction::List)),
                Act("Space", A::Project(ProjectAction::Last)),
                Range("1-9", A::Project(ProjectAction::ByIndex(1))),
                Act("]", A::Project(ProjectAction::Next)),
                Act("[", A::Project(ProjectAction::Prev)),
                Act("!", A::Project(ProjectAction::Attention)),
                Act("s", A::ShowMemory),
                Act("?", A::Help),
                Act("a", A::About),
            ],
            PendingSeq::G => vec![
                Act("g", A::GotoFirst),
                Act("d", A::GitDiff),
                Act("D", A::GitDiffCached),
                Act("u", A::GitDiffUnstaged),
                Act("b", A::GitBlame),
                Act("r", A::GitRestore),
                Act("f", A::GotoFile),
                Act("F", A::GotoFileLine),
                Act("w", A::JumpWorktreeRoot),
                Act("P", A::Project(ProjectAction::SetHomeHere)),
                Act("S", A::SetStartDirHere),
                Act("p", A::ReopenLastBuffer),
                Act("B", A::OpenTaskViewer),
                Act("V", A::Version),
            ],
            PendingSeq::W => vec![
                Act("j", A::PaneFocusDown),
                Act("k", A::PaneFocusUp),
                Act("a h", A::VsplitFocusLeft),
                Act("b l", A::VsplitFocusRight),
                Act("|", A::VsplitToggle),
                Act("f", A::VsplitToggleHeight),
                Act("d", A::ToggleDim),
                Act("n ]", A::PaneNextTab),
                Act("p [", A::PanePrevTab),
                Act("c", A::PaneNewTab),
                Range("1-9", A::PaneTabByIndex(1)),
                Act("x", A::PaneCloseTab),
                Act("r", A::PaneRenameTab),
                Act("R", A::PaneRestartTab),
                Act("F", A::PaneForkTab),
                Act("z", A::TogglePaneZoom),
                Act("v", A::PaneScrollEnter),
                Act("t", A::OpenThinkingHistory),
                Act("+", A::PaneGrow),
                Act("-", A::PaneShrink),
                Act("s", A::PaneSendSelection),
                Act("↓", A::PaneSendPrefix),
                Act("P", A::PanePipeContent),
                Act("i", A::PanePipeInventory),
                Act("u", A::QuickSelectOpen),
                Act(".", A::PaneContextMenu),
                Act("g", A::OpenImageGallery),
                Act("\\ C", A::TogglePane),
                Act("!", A::Project(ProjectAction::Attention)),
                Sub("Space", "global menu", PendingSeq::Leader),
            ],
            PendingSeq::CtrlS => vec![
                Act("|", A::VsplitToggle),
                Act("f", A::VsplitToggleHeight),
                Act("n", A::OpenSecondCommander),
                Act("x", A::CloseSecondCommander),
            ],
            PendingSeq::Worktree => vec![
                Act("l", A::WorktreeList),
                Act("n", A::WorktreeNew),
                Act("d", A::WorktreeDelete),
            ],
            PendingSeq::Harpoon => vec![
                Range("1-9", A::HarpoonJump(1)),
                Act("a", A::HarpoonAppend),
                Act("x", A::HarpoonRemove),
                Act("h", A::HarpoonOpenMenu),
            ],
            PendingSeq::Yank => vec![
                Act("y", A::Take),
                Act("f", A::YankPaths),
                Act("p", A::YankPrompt),
                Act("P", A::YankLastPrompt),
                Act("a", A::YankScrollback),
            ],
            PendingSeq::Mark => vec![Range("a-z", A::SetMark('a'))],
            PendingSeq::JumpMark => vec![Range("a-z", A::JumpMark('a')), Act("'", A::JumpPrevDir)],
            PendingSeq::NextBracket => bracket_rows(true),
            PendingSeq::PrevBracket => bracket_rows(false),
            PendingSeq::D => vec![Act("d", A::RemovePrompt(None))],
            PendingSeq::Z => vec![Act("Z", A::Quit)],
        }
    }
}
