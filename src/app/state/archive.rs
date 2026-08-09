//! What the Model allows inside a mounted archive.
//!
//! A mount is a virtual tree: its rows carry paths that don't exist on disk until
//! a member is extracted, and its "directory" is a file. Most of spyc doesn't
//! care — sort, picks, the cursor, the `=` filter and the whole render path work
//! off rows.
//!
//! Anything that *reads* a member is no longer refused here: those ops route
//! through `archive_route`, which holds the effect back, extracts, and re-runs it.
//! What's left is the two things that can't be papered over — writing into a
//! container, and running something that has no meaning in here — plus the ops
//! whose effects never reach the screen because they don't carry a path.

use crate::keymap::Action;

/// Why an action can't run inside a mount. The string is what the user sees, so
/// it says what is true rather than what is missing.
pub const fn refusal(action: &Action) -> Option<&'static str> {
    use Action as A;
    Some(match action {
        // Writing into a container means rewriting it, which is not something
        // spyc does yet. (`Drop`, `CopyPrompt` and `MovePrompt` reach the effect
        // screen, which distinguishes copying *out* — allowed — from writing in;
        // these two never produce a path to screen.)
        A::MakeDirPrompt | A::NewFilePrompt => "archive: writing into an archive is not supported",
        A::ChmodAdd(_) => "archive: permissions are stored in the archive",

        // Editing a member would stage a change with nowhere to go: spyc can't
        // write the container back yet, so the edit would be silently discarded.
        A::EnterOrEdit | A::EditInPane => "archive: editing a member is not supported",

        // A shell inherits the process cwd, which stays *outside* the archive —
        // running one here would silently operate on the archive's directory.
        A::StartShell | A::ShellCapturedPrompt | A::ShellForegroundPrompt => {
            "archive: shell commands run outside the archive"
        }

        // Bookmarks that would persist a path with nothing behind it once the
        // mount is dropped. Jumping *out* by a mark or harpoon slot is fine.
        A::SetMark(_) => "archive: marks can't point inside an archive",
        A::HarpoonAppend | A::HarpoonRemove => "archive: harpoon can't point inside an archive",

        // A member has no history, and there is no worktree in here.
        A::GitDiff
        | A::GitDiffCached
        | A::GitDiffUnstaged
        | A::GitBlame
        | A::GitRestore
        | A::JumpNextGitChange
        | A::JumpPrevGitChange => "archive: no git inside an archive",
        A::WorktreeNew | A::WorktreeDelete => "archive: not inside a worktree",

        // The finder walks real directories.
        A::FindFile => "archive: find is not available inside an archive",

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_and_view_actions_are_untouched() {
        for action in [
            Action::Down(1),
            Action::Climb,
            Action::EnterOrDisplay,
            Action::TogglePick,
            Action::SortCycle,
            Action::LimitPrompt,
            Action::SearchPrompt,
            Action::Help,
            Action::CommandPrompt,
            Action::TogglePane,
            Action::JumpMark('a'),
            Action::HarpoonJump(1),
            Action::WorktreeList,
            Action::JumpProjectHome,
        ] {
            assert_eq!(refusal(&action), None, "{action:?} should work in a mount");
        }
    }

    #[test]
    fn creating_files_and_changing_modes_are_refused() {
        for action in [
            Action::MakeDirPrompt,
            Action::NewFilePrompt,
            Action::ChmodAdd('x'),
        ] {
            assert!(refusal(&action).is_some(), "{action:?} must be refused");
        }
    }

    /// Reads are not refused here — they route through the effect screen, which
    /// extracts first and re-runs the op. Refusing them twice would make the
    /// screen unreachable.
    #[test]
    fn reads_are_left_to_the_effect_screen() {
        for action in [
            Action::Take,
            Action::PanePipeContent,
            Action::PanePipeInventory,
            Action::LongList,
            Action::FileType,
            Action::CopyPrompt,
            Action::MovePrompt,
            Action::Drop,
            Action::RemovePrompt(None),
            Action::Take,
        ] {
            assert_eq!(
                refusal(&action),
                None,
                "{action:?} is the effect screen's call, not this gate's"
            );
        }
    }

    /// An edit has nowhere to go until the container can be written back, so it
    /// is refused rather than staged into a copy nobody will ever repack.
    #[test]
    fn editing_a_member_is_refused() {
        for action in [Action::EnterOrEdit, Action::EditInPane] {
            let why = refusal(&action).expect("refused");
            assert!(why.contains("editing"), "{why}");
        }
    }

    /// A shell would run in the process cwd, which stays outside the archive —
    /// the one case where doing nothing is safer than doing something plausible.
    #[test]
    fn shells_are_refused_and_say_why() {
        let why = refusal(&Action::StartShell).expect("refused");
        assert!(why.contains("outside"), "{why}");
    }

    #[test]
    fn bookmarks_that_would_dangle_are_refused_but_jumps_out_are_not() {
        assert!(refusal(&Action::SetMark('a')).is_some());
        assert!(refusal(&Action::HarpoonAppend).is_some());
        assert_eq!(refusal(&Action::JumpMark('a')), None);
        assert_eq!(refusal(&Action::HarpoonJump(2)), None);
    }

    #[test]
    fn git_actions_are_refused() {
        for action in [
            Action::GitDiff,
            Action::GitBlame,
            Action::GitRestore,
            Action::JumpNextGitChange,
        ] {
            let why = refusal(&action).expect("refused");
            assert!(why.contains("git"), "{why}");
        }
    }

    /// Every refusal is shown to the user, so each one has to name the archive
    /// and read as a statement rather than a stack trace.
    #[test]
    fn every_refusal_message_is_user_facing() {
        for action in [
            Action::MakeDirPrompt,
            Action::SetMark('a'),
            Action::GitDiff,
            Action::FindFile,
            Action::StartShell,
        ] {
            let why = refusal(&action).expect("refused");
            assert!(why.starts_with("archive: "), "{why}");
            assert!(!why.ends_with('.'), "flashes don't end in a period: {why}");
        }
    }
}
