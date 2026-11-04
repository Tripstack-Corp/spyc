//! A menu offers only what spyc would do: a row whose action would be refused
//! in the state the menu opens in is left out. Each case right-clicks in the
//! refusing state and finds the row absent, then in the allowing state finds
//! it there, through `handle_mouse` and the frame as drawn.

use super::menu_tests::{app_over_entries, cursor_on, drawn_at, flash, offers, right_click};
use super::status_menu_tests::{esc, git_menu, menu, on_bar, read_git, repo_app};
use super::tests::{FRAME, drawn_menu, press};
use crate::app::App;
use crate::app::state::Side;
use crate::keymap::Action;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Run `body` over a fresh state root, in a canonical temp dir, with mouse
/// reporting on, as a click needs it.
fn with_mouse(body: impl FnOnce(&Path)) {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let state = base.join("state");
    std::fs::create_dir_all(&state).unwrap();
    crate::state::with_state_root(&state, || {
        crate::set_mouse_capture_for_test(true);
        body(&base);
        crate::set_mouse_capture_for_test(false);
    });
}

fn type_keys(app: &mut App, keys: &str) {
    for c in keys.chars() {
        press(app, c);
    }
}

fn enter(app: &mut App) {
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
}

// ── the git segment: a claimed worktree ──

/// A linked worktree `feature` of a fresh repo under `base`.
fn linked_worktree(base: &Path) -> PathBuf {
    use crate::git::test_support::run_git;
    let repo = base.join("repo");
    drop(repo_app(&repo));
    let wt = base.join("feature");
    run_git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            wt.to_str().unwrap(),
        ],
    );
    wt
}

/// A worktree another session has claimed is locked, and `Space w d` refuses
/// to remove it, so the git segment's menu offers no delete there.
#[test]
#[ignore = "red: the git menu offers `Space w d` on a locked worktree"]
fn a_claimed_worktrees_git_menu_offers_no_delete() {
    with_mouse(|base| {
        let wt = linked_worktree(base);
        crate::git::worktree::lock(&wt, "agent B: busy").unwrap();

        let rows = git_menu(&wt, false);

        assert!(offers(&rows, "new git worktree"), "premise: {rows:#?}");
        assert!(!offers(&rows, "delete git worktree"), "claimed: {rows:#?}");

        let mut app = App::test_app(wt.clone());
        app.view.term_size = (120, 24);
        read_git(&mut app, &wt);
        type_keys(&mut app, " wdy");
        enter(&mut app);
        let said = flash(&app).unwrap_or_default();
        assert!(
            said.contains("locked (claimed)"),
            "premise: the key refuses it: {said:?}"
        );
        assert!(wt.exists(), "premise: and it is still there");
    });
}

/// A claim that lands while the column is at the worktree's root is read by
/// the next git poll: before it the menu offers delete, after it not, and
/// once released, delete again.
#[test]
#[ignore = "red: the git cache holds no lock, so the menu offers delete on a claimed tree"]
fn a_claim_taken_after_the_column_arrived_is_read_by_the_git_poll() {
    with_mouse(|base| {
        let wt = linked_worktree(base);
        let mut app = App::test_app(wt.clone());
        app.view.term_size = (120, 24);
        read_git(&mut app, &wt);
        let segment = on_bar(&mut app, "\u{e0a0}");
        right_click(&mut app, segment);
        let rows = menu(&mut app, "git");
        assert!(offers(&rows, "Space w d  delete git worktree"), "{rows:#?}");
        esc(&mut app);

        crate::git::worktree::lock(&wt, "agent B: busy").unwrap();
        app.state.refresh_git_state();
        right_click(&mut app, segment);

        let rows = menu(&mut app, "git");
        assert!(offers(&rows, "new git worktree"), "premise: {rows:#?}");
        assert!(!offers(&rows, "delete git worktree"), "claimed: {rows:#?}");
        esc(&mut app);

        crate::git::worktree::unlock(&wt).unwrap();
        app.state.refresh_git_state();
        right_click(&mut app, segment);

        let rows = menu(&mut app, "git");
        assert!(offers(&rows, "delete git worktree"), "released: {rows:#?}");
    });
}

/// A column that moves to a claimed worktree reads the claim as it arrives,
/// before any git poll: from the split's other column, the move and the
/// focus change read nothing else.
#[test]
#[ignore = "red: the git cache holds no lock, so the menu offers delete on a claimed tree"]
fn a_column_moving_to_a_claimed_worktree_reads_the_claim_on_arrival() {
    with_mouse(|base| {
        let wt = linked_worktree(base);
        let repo = base.join("repo");
        let mut app = App::test_app(repo.clone());
        app.view.term_size = (120, 24);
        read_git(&mut app, &repo);
        app.open_second_commander_at(&repo);
        super::menu_tests::ctrl_a(&mut app);
        press(&mut app, 'h');
        assert_eq!(app.state.focused_side(), Side::Left, "premise: `a` has it");
        crate::git::worktree::lock(&wt, "agent B: busy").unwrap();

        app.state.chdir_side(Side::Right, &wt).unwrap();
        super::menu_tests::ctrl_a(&mut app);
        press(&mut app, 'l');
        assert_eq!(app.state.focused_side(), Side::Right, "premise: `b` has it");
        let segment = on_bar(&mut app, "\u{e0a0}");
        right_click(&mut app, segment);

        let rows = menu(&mut app, "git");
        assert!(offers(&rows, "new git worktree"), "premise: {rows:#?}");
        assert!(!offers(&rows, "delete git worktree"), "claimed: {rows:#?}");
    });
}

// ── an entry: `Ha` with no harpoon list ──

/// The entry menu of `notes.txt`, opened by a right-click on its row.
fn notes_menu(app: &mut App) -> Vec<String> {
    let row = drawn_at(app, "notes.txt");
    right_click(app, row);
    drawn_menu(app, "notes.txt");
    super::menu_tests::menu_lines(app, "notes.txt")
}

/// `Ha` appends to the column's harpoon list, which its repo or the project's
/// home gives it. Outside a repo with no home there is none, `Ha` refuses, and
/// the entry's menu offers no harpoon; with a home it does.
#[test]
#[ignore = "red: the entry menu offers `Ha` with no harpoon list to append to"]
fn an_entrys_menu_offers_harpoon_only_with_a_list_to_append_to() {
    with_mouse(|base| {
        let mut app = app_over_entries(base);
        app.reconcile_harpoon();
        cursor_on(&mut app, "notes.txt");
        type_keys(&mut app, "Ha");
        assert_eq!(
            flash(&app).as_deref(),
            Some("harpoon: open a repo or set PROJECT_HOME (gP)"),
            "premise: the key refuses"
        );

        let rows = notes_menu(&mut app);

        assert!(offers(&rows, "yank"), "premise: {rows:#?}");
        assert!(!offers(&rows, "harpoon"), "no list: {rows:#?}");
        esc(&mut app);

        let home = app.state.cur().listing.dir.clone();
        app.state.active_project_mut().home = Some(home);
        app.reconcile_harpoon();
        let rows = notes_menu(&mut app);

        assert!(offers(&rows, "Ha  harpoon"), "a home: {rows:#?}");
    });
}

/// In a repo the column has its list with no home, and the menu offers `Ha`.
#[test]
fn an_entrys_menu_in_a_repo_offers_harpoon() {
    with_mouse(|base| {
        let dir = base.join("repo");
        let mut app = repo_app(&dir);
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        app.view.term_size = (FRAME.width, FRAME.height);
        read_git(&mut app, &dir);
        app.state.refresh_listing();
        app.reconcile_harpoon();
        assert!(
            app.state.active_project().home.is_none(),
            "premise: no home"
        );

        let rows = notes_menu(&mut app);

        assert!(offers(&rows, "Ha  harpoon"), "{rows:#?}");
    });
}

// ── a tab: `^a F` with no conversation ──

/// A program called `name` in `base/bin` that does nothing for half a
/// minute, so a tab running it is taken for that agent.
fn fake_agent(base: &Path, name: &str) -> String {
    let bin = base.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join(name);
    std::fs::write(&exe, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    exe.display().to_string()
}

/// An App over `work/` with one tab running a fake `name` agent, labelled
/// `agent`, the keyboard in the file list; codex with its launch home.
fn app_with_agent_tab(base: &Path, name: &str) -> App {
    let dir = base.join("work");
    std::fs::create_dir_all(&dir).unwrap();
    let cmd = fake_agent(base, name);
    let mut app = App::test_app(dir.clone());
    app.view.term_size = (FRAME.width, FRAME.height);
    let home = base.join("codex-home");
    let home = (name == "codex").then_some(home.as_path());
    assert!(
        app.open_pane_tab_in_home(&cmd, &dir, home),
        "premise: it runs"
    );
    let info = active_info(&mut app);
    info.label = "agent".to_string();
    assert_eq!(
        info.agent().name(),
        name,
        "premise: the tab is taken for {name}"
    );
    app.set_pane_focus(false);
    app
}

fn active_info(app: &mut App) -> &mut crate::pane::TabInfo {
    app.runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .expect("a tab")
        .active_info_mut()
}

/// The active tab's menu, opened by a right-click on the pane.
fn tab_menu(app: &mut App) -> Vec<String> {
    let pane = app.frame_layout(FRAME).pane.expect("a pane is open");
    right_click(app, super::tests::centre(pane));
    let rows = menu(app, "agent");
    esc(app);
    app.set_pane_focus(false);
    rows
}

const SESSION: &str = "019e8b21-9e7c-7553-a118-d1cdada725fd";
const FORK: &str = "^a F  fork pane tab's conversation";

/// A claude tab with no conversation pinned to it has nothing for `^a F` to
/// branch, so its menu offers no fork; once its hook has reported the
/// session, it does.
#[test]
#[ignore = "red: the tab menu offers `^a F` with no conversation pinned to the tab"]
fn a_claude_tabs_menu_offers_fork_only_with_a_conversation() {
    with_mouse(|base| {
        let mut app = app_with_agent_tab(base, "claude");

        let rows = tab_menu(&mut app);

        assert!(offers(&rows, "restart pane tab"), "premise: {rows:#?}");
        assert!(!offers(&rows, "fork"), "no conversation: {rows:#?}");

        active_info(&mut app).live_session_id = Some(SESSION.to_string());
        let rows = tab_menu(&mut app);

        assert!(offers(&rows, FORK), "a conversation: {rows:#?}");
    });
}

/// A codex tab forks the rollout claimed for it, from its launch home: with
/// no rollout `^a F` refuses, and so does the menu; with one but no home it
/// still refuses; with both it forks.
#[test]
#[ignore = "red: the tab menu offers `^a F` with no rollout claimed or no launch home"]
fn a_codex_tabs_menu_offers_fork_only_with_a_rollout_and_its_home() {
    with_mouse(|base| {
        let mut app = app_with_agent_tab(base, "codex");
        app.apply(&Action::PaneForkTab).unwrap();
        let said = flash(&app).unwrap_or_default();
        assert!(
            said.contains("nothing to branch yet"),
            "premise: the key refuses: {said:?}"
        );

        let rows = tab_menu(&mut app);

        assert!(offers(&rows, "restart pane tab"), "premise: {rows:#?}");
        assert!(!offers(&rows, "fork"), "no rollout: {rows:#?}");

        let home = active_info(&mut app).codex_home.take();
        assert!(home.is_some(), "premise: launched with its home");
        active_info(&mut app).codex_session_id = Some(SESSION.to_string());
        app.apply(&Action::PaneForkTab).unwrap();
        let said = flash(&app).unwrap_or_default();
        assert!(
            said.contains("launch home is not identified"),
            "premise: the key refuses: {said:?}"
        );
        let rows = tab_menu(&mut app);
        assert!(!offers(&rows, "fork"), "no home: {rows:#?}");

        active_info(&mut app).codex_home = home;
        let rows = tab_menu(&mut app);

        assert!(offers(&rows, FORK), "a rollout and its home: {rows:#?}");
    });
}
