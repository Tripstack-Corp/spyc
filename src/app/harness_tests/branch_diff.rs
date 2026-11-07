//! A branch's changes since it left its base: `gm` over the selection and
//! `:diff [<base>]` over the whole repository, each pressed or typed as keys
//! and waited on until the diff lands as the loop's drain lands it.

use super::*;
use crate::app::FlashKind;
use crate::app::state::Side;
use crate::git::test_support::{branch_repo, run_git, stale_main_repo};

use std::path::Path;
use std::time::{Duration, Instant};

/// An App whose column lists `dir`, inside a repository, read as the git poll
/// reads it.
fn app_in(dir: &Path) -> App {
    let mut app = App::test_app(dir.to_path_buf());
    app.state.refresh_listing();
    app.state.update_repo_root(Side::Left, dir);
    app.state.refresh_git_state();
    assert!(app.state.cur().git.info.is_some(), "premise: in a repo");
    app
}

/// Put the cursor on the entry named `name`.
fn cursor_to(app: &mut App, name: &str) {
    let at = app
        .state
        .cur()
        .rows
        .iter()
        .position(|row| row.path.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("{name} is not listed"));
    app.state.cur_mut().cursor.index = at;
}

/// Press each of `keys` in turn.
fn press(app: &mut App, keys: &str) {
    for c in keys.chars() {
        app.handle_key(key(c)).unwrap();
    }
}

/// Type `line` at the `:` prompt and run it.
fn command(app: &mut App, line: &str) {
    press(app, ":");
    press(app, line);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
}

/// The pager title and text of the diff the last keys opened.
fn diff_opened(app: &mut App, what: &str) -> (String, String) {
    app.land_git_view()
        .unwrap_or_else(|| panic!("{what} opens a diff; the flash says {:?}", app.flash_text()))
}

/// The fixture's files a diff's text names, of `alpha`–`echo`.
fn files_in(text: &str) -> Vec<&'static str> {
    ["alpha", "bravo", "charlie", "delta", "echo"]
        .into_iter()
        .filter(|name| text.contains(&format!("{name}.txt")))
        .collect()
}

/// `git merge-base` of `a` and `b`, short as the title names it.
fn merge_base(repo: &Path, a: &str, b: &str) -> String {
    run_git(repo, &["merge-base", a, b]).trim()[..7].to_string()
}

/// The headline: over a directory holding everything, `gm` shows what the
/// branch committed and what it hasn't yet, and none of `main`'s own later
/// work. A committed change reads against the base, not against `HEAD`, where
/// it would be no change at all. The title names the base and the commit the
/// branch left it at, and says uncommitted work is in.
#[test]
fn gm_over_a_directory_shows_the_branchs_changes_and_not_its_bases() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let repo = branch_repo(tmp.path());
        let mut app = app_in(&repo);
        cursor_to(&mut app, "proj");

        press(&mut app, "gm");

        let (title, text) = diff_opened(&mut app, "gm");
        assert_eq!(
            files_in(&text),
            ["alpha", "bravo", "charlie", "delta"],
            "{text}"
        );
        assert!(text.contains("from the branch"), "alpha's commit: {text}");
        let fork = merge_base(&repo, "main", "HEAD");
        assert!(title.contains("main"), "names the base: {title}");
        assert!(title.contains(&fork), "names the fork, {fork}: {title}");
        assert!(title.contains("uncommitted"), "says so: {title}");
    });
}

/// Picks choose what `gm` shows, as they do for `gd`.
#[test]
fn gm_with_picks_shows_only_the_picks() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let repo = branch_repo(tmp.path());
        let mut app = app_in(&repo.join("proj"));
        for name in ["alpha.txt", "delta.txt"] {
            cursor_to(&mut app, name);
            press(&mut app, "t");
        }
        assert_eq!(app.state.cur().picks.len(), 2, "premise: two picks");

        press(&mut app, "gm");

        let (_, text) = diff_opened(&mut app, "gm");
        assert_eq!(files_in(&text), ["alpha", "delta"], "{text}");
    });
}

/// `:diff` answers for the whole branch, from a directory none of its changes
/// are in.
#[test]
fn diff_from_a_subdirectory_shows_the_whole_branch() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let repo = branch_repo(tmp.path());
        let mut app = app_in(&repo.join("other"));

        command(&mut app, "diff");

        let (title, text) = diff_opened(&mut app, ":diff");
        assert_eq!(
            files_in(&text),
            ["alpha", "bravo", "charlie", "delta"],
            "{text}"
        );
        assert!(title.contains("main"), "the default base: {title}");
    });
}

/// `:diff <sha>` takes the commit as given: from the branch's own commit, only
/// what is not committed yet.
#[test]
fn diff_with_a_sha_diffs_from_that_commits_merge_base() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let repo = branch_repo(tmp.path());
        let sha = run_git(&repo, &["rev-parse", "feature"]).trim().to_string();
        let mut app = app_in(&repo);

        command(&mut app, &format!("diff {sha}"));

        let (title, text) = diff_opened(&mut app, ":diff <sha>");
        assert_eq!(files_in(&text), ["charlie", "delta"], "{text}");
        assert!(title.contains(&sha[..7]), "names the fork: {title}");
    });
}

/// A base that names nothing is an error that says which, and no pager opens.
#[test]
fn diff_with_an_unknown_base_flashes_its_name_and_opens_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let repo = branch_repo(tmp.path());
        let mut app = app_in(&repo);

        command(&mut app, "diff nosuch");

        assert_eq!(app.land_git_view(), None, "no pager opens");
        assert!(app.view.project().pager.is_none(), "no pager opens");
        let flash = app.state.flash.as_ref().expect("a flash says why");
        assert!(matches!(flash.kind, FlashKind::Error), "{}", flash.text);
        assert!(flash.text.contains("nosuch"), "names it: {}", flash.text);
        assert!(
            !flash.text.contains("unknown command"),
            "`:diff` is a command: {}",
            flash.text
        );
    });
}

/// Local `main` lags `origin/main`, and the branch was rebased onto
/// `origin/main`: what `main` lacks is other people's merged work, not this
/// branch's, so `gm` leaves it out.
#[test]
fn gm_with_a_stale_local_main_shows_only_the_branchs_own_files() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let repo = stale_main_repo(tmp.path());
        let mut app = app_in(&repo);
        cursor_to(&mut app, "proj");

        press(&mut app, "gm");

        let (title, text) = diff_opened(&mut app, "gm");
        assert_eq!(files_in(&text), ["alpha"], "{text}");
        let fork = merge_base(&repo, "origin/main", "HEAD");
        assert!(
            title.contains(&fork),
            "forked at origin/main's {fork}: {title}"
        );
    });
}

/// The `g` popup lists `m` beside `gd`, `gD` and `gu`.
#[test]
fn the_g_popup_lists_gm() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        app.handle_key(key('g')).unwrap();
        app.settle_chord_hint(
            Instant::now() + Duration::from_secs(60),
            &mut RunCtx::for_test(),
        );

        let hint = app.view.chord_hint.as_ref().expect("the popup is up");
        assert!(
            hint.rows
                .iter()
                .any(|(keys, label)| keys == "m" && label.contains("base")),
            "{:?}",
            hint.rows
        );
    });
}

/// A file a branch commit changed and the working tree has since put back as
/// it was at the fork is no change since the fork: `:diff` leaves it out, as
/// `git diff $(git merge-base main HEAD)` does.
#[test]
fn diff_leaves_out_a_file_the_worktree_put_back_as_it_was_at_the_fork() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let repo = branch_repo(tmp.path());
        std::fs::write(repo.join("proj/alpha.txt"), "alpha\n").unwrap();
        let mut app = app_in(&repo);

        command(&mut app, "diff");

        let (_, text) = diff_opened(&mut app, ":diff");
        assert_eq!(files_in(&text), ["bravo", "charlie", "delta"], "{text}");
    });
}

/// The same for a binary file, which has no lines to compare: put back byte
/// for byte as it was at the fork, it is left out.
#[test]
fn diff_leaves_out_a_binary_file_the_worktree_put_back() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let repo = branch_repo(tmp.path());
        let blob = repo.join("proj/blob.bin");
        std::fs::write(&blob, b"\0first\0").unwrap();
        run_git(&repo, &["add", "proj/blob.bin"]);
        run_git(&repo, &["commit", "-q", "-m", "a binary file"]);
        let fork = run_git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
        std::fs::write(&blob, b"\0second\0").unwrap();
        run_git(&repo, &["add", "proj/blob.bin"]);
        run_git(&repo, &["commit", "-q", "-m", "the binary file changes"]);
        std::fs::write(&blob, b"\0first\0").unwrap();
        let mut app = app_in(&repo);

        command(&mut app, &format!("diff {fork}"));

        let (_, text) = diff_opened(&mut app, ":diff <sha>");
        assert_eq!(files_in(&text), ["charlie", "delta"], "{text}");
        assert!(!text.contains("blob.bin"), "{text}");
    });
}

/// A file whose content is as it was at the fork but whose mode a branch
/// commit changed is a change: it stays.
#[cfg(unix)]
#[test]
fn diff_keeps_a_file_whose_mode_the_branch_changed() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let repo = branch_repo(tmp.path());
        let keep = repo.join("other/keep.txt");
        std::fs::set_permissions(&keep, std::fs::Permissions::from_mode(0o755)).unwrap();
        run_git(&repo, &["add", "other/keep.txt"]);
        run_git(&repo, &["commit", "-q", "-m", "keep.txt runs"]);
        let mut app = app_in(&repo);

        command(&mut app, "diff");

        let (_, text) = diff_opened(&mut app, ":diff");
        assert!(text.contains("keep.txt"), "the mode change: {text}");
    });
}
