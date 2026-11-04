//! The status bar's segments under the pointer. A click on a project's label
//! goes to that project as `Space N` does, and one on the collapsed count
//! opens the project list as `Space j` does; a right-click on a label, the
//! count, the lone project, the git, agent or session segment opens that
//! segment's menu, and anywhere else on the bar still opens the leader. Each
//! case goes through `handle_mouse` and aims at the bar as the frame draws it.

use super::MouseDragTarget;
use super::menu_tests::{offers, right_click};
use super::tests::{at, click, menu_in};
use crate::app::state::{Focus, ProjectId, Side, ZoomTarget};
use crate::app::{App, Effect};
use crate::pane::AgentActivity;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use std::path::{Path, PathBuf};

const HEIGHT: u16 = 24;

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

/// An App `width` columns wide with a project homed at each of `names`
/// under `base`, open in that order, the first in view.
fn projects(base: &Path, names: &[&str], width: u16) -> (App, Vec<ProjectId>) {
    let homes: Vec<PathBuf> = names
        .iter()
        .map(|name| {
            let home = base.join(name);
            std::fs::create_dir_all(&home).unwrap();
            home
        })
        .collect();
    let mut app = App::test_app(homes[0].clone());
    app.view.term_size = (width, HEIGHT);
    let first = app.state.active;
    app.ready_test_project(first, &homes[0]);
    let mut ids = vec![first];
    for home in &homes[1..] {
        ids.push(app.add_test_project(home));
    }
    (app, ids)
}

/// One frame at the App's own size.
fn draw(app: &mut App) -> Buffer {
    let (width, height) = app.view.term_size;
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    terminal.backend().buffer().clone()
}

/// The status bar's row, and its cells as drawn.
fn bar(app: &mut App) -> (u16, Vec<Cell>) {
    let buf = draw(app);
    let (width, height) = app.view.term_size;
    let y = app.frame_layout(Rect::new(0, 0, width, height)).status.y;
    (y, (0..width).map(|x| buf[(x, y)].clone()).collect())
}

/// The bar's text, one cell's symbol after another.
fn bar_text(cells: &[Cell]) -> String {
    cells.iter().map(Cell::symbol).collect()
}

/// Where `text` is first drawn on the status bar.
fn on_bar(app: &mut App, text: &str) -> (u16, u16) {
    let (y, cells) = bar(app);
    let line = bar_text(&cells);
    let byte = line
        .find(text)
        .unwrap_or_else(|| panic!("{text:?} is not on the bar: {line:?}"));
    let mut at = 0;
    for (x, cell) in cells.iter().enumerate() {
        if at == byte {
            return (u16::try_from(x).unwrap(), y);
        }
        at += cell.symbol().len();
    }
    panic!("{text:?} starts inside a cell");
}

/// The body lines of the menu titled `title`, as drawn.
fn menu(app: &mut App, title: &str) -> Vec<String> {
    let buf = draw(app);
    let menu = menu_in(&buf, title);
    (menu.y + 1..menu.bottom() - 1)
        .map(|y| {
            (menu.x + 1..menu.right() - 1)
                .map(|x| buf[(x, y)].symbol())
                .collect()
        })
        .collect()
}

/// Where the row reading `text` is drawn in the menu titled `title`.
fn menu_row(app: &mut App, title: &str, text: &str) -> (u16, u16) {
    let buf = draw(app);
    let menu = menu_in(&buf, title);
    for y in menu.y + 1..menu.bottom() - 1 {
        let cells: Vec<&str> = (menu.x..menu.right())
            .map(|x| buf[(x, y)].symbol())
            .collect();
        let Some(byte) = cells.concat().find(text) else {
            continue;
        };
        let mut at = 0;
        for (i, cell) in cells.iter().enumerate() {
            if at == byte {
                return (menu.x + u16::try_from(i).unwrap(), y);
            }
            at += cell.len();
        }
    }
    panic!("no row {text:?} in the menu titled {title:?}");
}

fn press(app: &mut App, (col, row): (u16, u16)) -> Vec<Effect> {
    app.handle_mouse(at(MouseEventKind::Down(MouseButton::Left), col, row))
}

fn release(app: &mut App, (col, row): (u16, u16)) -> Vec<Effect> {
    app.handle_mouse(at(MouseEventKind::Up(MouseButton::Left), col, row))
}

fn esc(app: &mut App) {
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
}

/// The project list `Space j` opens is in view.
fn project_list_open(app: &App) -> bool {
    app.view
        .project()
        .pager
        .as_ref()
        .is_some_and(|v| v.picker == Some(crate::ui::pager::PickerKind::Projects))
}

/// The rows every project's menu and the projects menu end with: the list,
/// a new project, and next and previous while several are open.
const PROJECTS_ROWS: [&str; 4] = [
    "Space j  list the open projects",
    "Space n  open a directory as a project",
    "Space ]  go to the next project",
    "Space [  go to the previous project",
];

// ── a project's label ──

/// With two projects open, a click on the second's label goes to it, as
/// `Space 2` does.
#[test]
fn a_click_on_a_project_label_goes_to_that_project() {
    with_mouse(|base| {
        let (mut app, ids) = projects(base, &["alpha", "bravo"], 120);
        let label = on_bar(&mut app, "bravo");

        click(&mut app, label);

        assert_eq!(app.state.active, ids[1], "bravo is in view");
    });
}

/// A right-click on a label goes to that project first, as a click on it
/// would, then opens its menu: no switch, since it is in view now, but close,
/// and the projects rows. Its close closes the project the label named.
#[test]
fn a_right_click_on_a_label_goes_there_and_its_close_closes_that_project() {
    with_mouse(|base| {
        let (mut app, ids) = projects(base, &["alpha", "bravo"], 120);
        let label = on_bar(&mut app, "bravo");

        right_click(&mut app, label);

        assert_eq!(app.state.active, ids[1], "bravo is in view");
        let rows = menu(&mut app, "bravo");
        assert!(!offers(&rows, "go to project N"), "{rows:#?}");
        assert!(
            offers(&rows, "Space x  close the project in view"),
            "{rows:#?}"
        );
        for row in PROJECTS_ROWS {
            assert!(offers(&rows, row), "{row:?}: {rows:#?}");
        }
        assert!(!offers(&rows, "Space !"), "nobody waits: {rows:#?}");

        let close = menu_row(&mut app, "bravo", "close the project in view");
        click(&mut app, close);

        assert!(!app.state.has_project(ids[1]), "bravo closed");
        assert!(app.state.has_project(ids[0]), "alpha is still open");
        assert_eq!(app.state.active, ids[0], "and in view");
    });
}

/// While an agent is waiting on the user, the menu offers going to it.
#[test]
fn a_projects_menu_offers_the_agent_that_needs_you_while_one_waits() {
    with_mouse(|base| {
        let (mut app, ids) = projects(base, &["alpha", "bravo"], 120);
        let home = base.join("bravo");
        app.open_tab_in_project(ids[1], &home);
        app.as_project(ids[1], |app| {
            let tabs = app.runtime.project_mut().pane_tabs.as_mut().expect("a tab");
            tabs.active_info_mut().activity = AgentActivity::Blocked;
        });
        let label = on_bar(&mut app, "alpha");

        right_click(&mut app, label);

        let rows = menu(&mut app, "alpha");
        assert!(
            offers(&rows, "Space !  go to the agent that needs you"),
            "{rows:#?}"
        );
    });
}

/// A label drawn for a project that has closed since names none: a
/// right-click on it is refused as `Space N` would be, and opens no menu, so
/// no row acts on the project in view instead.
#[test]
fn a_label_for_a_project_closed_since_it_was_drawn_opens_nothing() {
    with_mouse(|base| {
        let (mut app, ids) = projects(base, &["alpha", "bravo", "charlie"], 160);
        let label = on_bar(&mut app, "charlie");
        app.remove_test_project(ids[1]);

        right_click(&mut app, label);

        assert_eq!(app.state.active, ids[0], "alpha stays in view");
        assert!(app.view.chord_hint.is_none(), "no menu opened");
        assert!(
            app.flash_text()
                .is_some_and(|f| f.starts_with("no project 3")),
            "{:?}",
            app.flash_text()
        );
    });
}

/// While an editor runs in a project's view `Space x` refuses to close it,
/// so its menu doesn't offer close.
#[test]
fn a_project_with_an_editor_open_is_not_offered_close() {
    with_mouse(|base| {
        let (mut app, ids) = projects(base, &["alpha", "bravo"], 120);
        app.as_project(ids[1], |app| {
            app.state.active_project_mut().focus = Focus::Overlay;
        });
        let label = on_bar(&mut app, "bravo");

        right_click(&mut app, label);

        assert_eq!(app.state.active, ids[1], "bravo is in view");
        let rows = menu(&mut app, "bravo");
        assert!(offers(&rows, PROJECTS_ROWS[0]), "premise: {rows:#?}");
        assert!(!offers(&rows, "close the project in view"), "{rows:#?}");
    });
}

/// A project with no home has none for `Space p` to jump to, and one whose
/// column is in another open project's home can't be re-homed there by
/// `Space P`, so its menu offers neither.
#[test]
fn a_project_is_offered_only_the_homes_its_keys_would_take() {
    with_mouse(|base| {
        let (mut app, _) = projects(base, &["alpha", "bravo"], 120);
        let bravo = base.join("bravo");
        let project = app.state.active_project_mut();
        project.home = None;
        project.left.listing = crate::fs::listing::Listing::read(&bravo).expect("bravo lists");
        app.state.rebuild_rows();
        let label = on_bar(&mut app, "alpha");

        right_click(&mut app, label);

        let rows = menu(&mut app, "alpha");
        assert!(offers(&rows, PROJECTS_ROWS[0]), "premise: {rows:#?}");
        assert!(!offers(&rows, "jump to PROJECT_HOME"), "{rows:#?}");
        assert!(!offers(&rows, "set PROJECT_HOME"), "{rows:#?}");
    });
}

/// With one project open, its segment's menu has neither switch nor close,
/// and a click on it still selects its text.
#[test]
fn the_lone_projects_menu_has_no_switch_or_close() {
    with_mouse(|base| {
        let (mut app, _) = projects(base, &["alpha"], 120);
        let segment = on_bar(&mut app, "alpha");

        right_click(&mut app, segment);

        let rows = menu(&mut app, "alpha");
        assert!(offers(&rows, "Space p  jump to PROJECT_HOME"), "{rows:#?}");
        assert!(
            offers(&rows, "Space P  set PROJECT_HOME to current dir"),
            "{rows:#?}"
        );
        assert!(offers(&rows, PROJECTS_ROWS[0]), "{rows:#?}");
        assert!(offers(&rows, PROJECTS_ROWS[1]), "{rows:#?}");
        for absent in [
            "go to project N",
            "close the project in view",
            "go to the next project",
            "go to the previous project",
        ] {
            assert!(!offers(&rows, absent), "{absent:?}: {rows:#?}");
        }

        esc(&mut app);
        press(&mut app, segment);
        assert_eq!(
            app.view.mouse_selection,
            Some(MouseDragTarget::Chrome),
            "a press on it starts a selection, as before"
        );
        release(&mut app, segment);
    });
}

/// Inside a mounted archive a project can't be re-homed, so its menu doesn't
/// offer it.
#[test]
fn a_project_is_not_offered_a_home_inside_an_archive() {
    use crate::app::archive_ops::{ArchiveOp, ArchiveOutcome, run_archive_op};
    use std::io::Write as _;
    with_mouse(|base| {
        let (mut app, _) = projects(base, &["alpha"], 120);
        let archive = base.join("alpha").join("pkg.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        w.start_file("README.md", zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(b"# pkg\n").unwrap();
        w.finish().unwrap();
        let outcome = run_archive_op(ArchiveOp::Mount {
            path: archive.clone(),
            staging_root: base.join("staging"),
            limits: app.state.active_project().config.archive.limits(),
            max_entries: 1000,
            confirmed: false,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            then: None,
            address: None,
            depth: 0,
            reset_staging: false,
        });
        assert!(
            matches!(outcome, ArchiveOutcome::Mounted { .. }),
            "{outcome:?}"
        );
        app.file_archive_outcome(outcome);
        app.apply_archive_outcomes();
        assert_eq!(app.state.cur().listing.dir, archive, "premise: mounted");
        let segment = on_bar(&mut app, "alpha");

        right_click(&mut app, segment);

        let rows = menu(&mut app, "alpha");
        assert!(offers(&rows, "jump to PROJECT_HOME"), "premise: {rows:#?}");
        assert!(!offers(&rows, "set PROJECT_HOME"), "{rows:#?}");
    });
}

// ── the collapsed count ──

/// Where the bar is too short for every chip, the segment collapses to the
/// project in view and its place among them: a click on the place opens the
/// project list, and a right-click the projects menu.
#[test]
fn the_collapsed_count_opens_the_project_list_and_the_projects_menu() {
    with_mouse(|base| {
        let names = ["alpha", "bravo", "charlie", "delta"];
        let (mut app, ids) = projects(base, &names, 60);
        let count = on_bar(&mut app, "1/4");

        right_click(&mut app, count);

        assert_eq!(app.state.active, ids[0], "the project in view stays");
        let rows = menu(&mut app, "projects");
        for row in PROJECTS_ROWS {
            assert!(offers(&rows, row), "{row:?}: {rows:#?}");
        }
        for absent in ["close the project in view", "jump to PROJECT_HOME"] {
            assert!(!offers(&rows, absent), "{absent:?}: {rows:#?}");
        }

        esc(&mut app);
        click(&mut app, count);

        assert!(project_list_open(&app), "the project list is open");
    });
}

// ── the git segment ──

/// A repo at `dir` with one commit, listed in an App's left column.
fn repo_app(dir: &Path) -> App {
    use crate::git::test_support::run_git;
    std::fs::create_dir_all(dir).unwrap();
    run_git(dir, &["init", "-q", "--initial-branch=main"]);
    std::fs::write(dir.join("seed.txt"), "s\n").unwrap();
    run_git(dir, &["add", "seed.txt"]);
    run_git(dir, &["commit", "-q", "-m", "seed"]);
    let mut app = App::test_app(dir.to_path_buf());
    app.view.term_size = (120, HEIGHT);
    app
}

/// Read `app`'s left column's repo as the git poll does.
fn read_git(app: &mut App, dir: &Path) {
    app.state.update_repo_root(Side::Left, dir);
    app.state.refresh_git_state();
    assert!(app.state.cur().git.info.is_some(), "premise: in a repo");
}

/// The git segment's menu offers the worktrees, and `gd` only while the
/// repo has changes. In the main checkout there is no worktree to delete.
#[test]
fn the_git_segments_menu_offers_gd_only_with_changes() {
    with_mouse(|base| {
        let clean = base.join("clean");
        let mut app = repo_app(&clean);
        read_git(&mut app, &clean);
        let segment = on_bar(&mut app, "\u{e0a0}");

        right_click(&mut app, segment);

        let rows = menu(&mut app, "git");
        assert!(offers(&rows, "Space w l  list git worktrees"), "{rows:#?}");
        assert!(offers(&rows, "Space w n  new git worktree"), "{rows:#?}");
        assert!(!offers(&rows, "gd"), "a clean repo: {rows:#?}");
        assert!(!offers(&rows, "delete git worktree"), "main: {rows:#?}");

        let changed = base.join("changed");
        let mut app = repo_app(&changed);
        std::fs::write(changed.join("seed.txt"), "changed\n").unwrap();
        read_git(&mut app, &changed);
        assert!(
            app.state
                .cur()
                .git
                .info
                .as_deref()
                .is_some_and(|i| i.ends_with('*')),
            "premise: the repo has changes"
        );
        let segment = on_bar(&mut app, "\u{e0a0}");

        right_click(&mut app, segment);

        let rows = menu(&mut app, "git");
        assert!(offers(&rows, "gd  git diff HEAD"), "{rows:#?}");
    });
}

/// The git segment's menu of an App over `dir`'s repo, read afresh.
fn git_menu(dir: &Path, split: bool) -> Vec<String> {
    let mut app = App::test_app(dir.to_path_buf());
    app.view.term_size = (120, HEIGHT);
    read_git(&mut app, dir);
    if split {
        app.open_second_commander_at(dir);
    }
    let segment = on_bar(&mut app, "\u{e0a0}");
    right_click(&mut app, segment);
    menu(&mut app, "git")
}

/// At the root of a clean linked worktree, the git segment's menu offers to
/// delete it, as `Space w d` would. Not while the other column is inside it,
/// which the key refuses, nor while it has changes, which the removal
/// refuses.
#[test]
fn the_git_segments_menu_offers_delete_at_a_clean_linked_worktree() {
    use crate::git::test_support::run_git;
    with_mouse(|base| {
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

        let rows = git_menu(&wt, false);
        assert!(offers(&rows, "Space w d  delete git worktree"), "{rows:#?}");

        let rows = git_menu(&wt, true);
        assert!(offers(&rows, "new git worktree"), "premise: {rows:#?}");
        assert!(!offers(&rows, "delete git worktree"), "split: {rows:#?}");

        std::fs::write(wt.join("seed.txt"), "changed\n").unwrap();
        let rows = git_menu(&wt, false);
        assert!(offers(&rows, "gd  git diff HEAD"), "premise: {rows:#?}");
        assert!(!offers(&rows, "delete git worktree"), "changed: {rows:#?}");
    });
}

// ── the agent and session segments ──

/// An App over `work/` with a `cat` tab that spyc takes for claude, the
/// keyboard in the file list.
fn app_with_agent(base: &Path) -> App {
    let dir = base.join("work");
    std::fs::create_dir_all(&dir).unwrap();
    let mut app = App::test_app(dir);
    app.view.term_size = (120, HEIGHT);
    app.open_pane_tab("cat");
    app.runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .expect("a tab")
        .active_info_mut()
        .agent_hint = Some(crate::state::sessions::AgentKind::Claude);
    app.set_pane_focus(false);
    app
}

/// A right-click on the agent segment gives the pane the keyboard and opens
/// its active tab's menu: the rows `^a .` opens.
#[test]
fn a_right_click_on_the_agent_segment_opens_the_active_tabs_menu() {
    with_mouse(|base| {
        let mut app = app_with_agent(base);
        let label = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .expect("a tab")
            .active_info()
            .label
            .clone();
        super::menu_tests::ctrl_a(&mut app);
        super::tests::press(&mut app, '.');
        let keyed = menu(&mut app, &label);
        esc(&mut app);
        app.set_pane_focus(false);
        let segment = on_bar(&mut app, "\u{f120}");

        right_click(&mut app, segment);

        assert!(app.state.pane_focused(), "the pane has the keyboard");
        assert_eq!(menu(&mut app, &label), keyed, "the rows `^a .` opens");
    });
}

/// A click on the agent segment gives the pane the keyboard.
#[test]
fn a_click_on_the_agent_segment_focuses_the_pane() {
    with_mouse(|base| {
        let mut app = app_with_agent(base);
        let segment = on_bar(&mut app, "\u{f120}");

        click(&mut app, segment);

        assert!(app.state.pane_focused(), "the pane has the keyboard");
    });
}

/// A right-click on the session segment opens its menu: the session's info.
#[test]
fn a_right_click_on_the_session_segment_offers_its_info() {
    with_mouse(|base| {
        let dir = base.join("work");
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::test_app(dir);
        app.view.term_size = (120, HEIGHT);
        app.state.session_name = Some("SAFFRON_CUMIN".to_string());
        let segment = on_bar(&mut app, "SAFFRON_CUMIN");

        right_click(&mut app, segment);

        let rows = menu(&mut app, "SAFFRON_CUMIN");
        assert!(offers(&rows, "Space s  session info"), "{rows:#?}");
    });
}

// ── the rest of the bar ──

/// The path is no object, nor the parting between two labels: a
/// right-click on either still opens the leader, and a drag along the bar
/// still selects and copies its text.
#[test]
fn the_path_still_opens_the_leader_and_the_bar_still_selects() {
    with_mouse(|base| {
        let (mut app, _) = projects(base, &["alpha", "bravo"], 120);
        let parting = on_bar(&mut app, "\u{e0b1}");

        right_click(&mut app, parting);

        assert_eq!(
            app.state.resolver.pending_display().as_deref(),
            Some("leader-"),
            "the leader is armed"
        );
        esc(&mut app);
        let (col, row) = on_bar(&mut app, "/");

        right_click(&mut app, (col, row));

        assert_eq!(
            app.state.resolver.pending_display().as_deref(),
            Some("leader-"),
            "the leader is armed"
        );
        esc(&mut app);
        press(&mut app, (col, row));
        assert_eq!(app.view.mouse_selection, Some(MouseDragTarget::Chrome));
        app.handle_mouse(at(MouseEventKind::Drag(MouseButton::Left), col + 3, row));
        let fx = release(&mut app, (col + 3, row));
        let (_, cells) = bar(&mut app);
        let from = usize::from(col);
        let drawn = bar_text(&cells[from..from + 4]);
        assert!(
            fx.iter().any(
                |e| matches!(e, Effect::CopyToClipboard { text, .. } if *text == drawn.trim())
            ),
            "the drag copied {drawn:?}: {fx:?}"
        );
    });
}

/// Where the bar isn't drawn, nothing of it answers a click: with the pane
/// zoomed and a message on the top row, a click where a label was selects
/// the message and goes nowhere.
#[test]
fn a_label_no_longer_drawn_is_no_target() {
    with_mouse(|base| {
        let (mut app, ids) = projects(base, &["alpha", "bravo"], 120);
        app.open_pane_tab("cat");
        app.set_pane_focus(false);
        let label = on_bar(&mut app, "bravo");
        app.state.active_project_mut().pane.zoom = ZoomTarget::BottomPane;
        app.state
            .flash_info("a message on the top row of the zoomed pane");
        let buf = draw(&mut app);
        let top: String = (0..120).map(|x| buf[(x, label.1)].symbol()).collect();
        assert!(top.contains("a message"), "premise: {top:?}");

        click(&mut app, label);

        assert_eq!(app.state.active, ids[0], "alpha stays in view");
    });
}

/// Each project label as drawn: a run of bold or dim cells in the projects
/// segment, the cells before its powerline separator. `(first, end)` columns.
fn drawn_labels(cells: &[Cell]) -> (usize, Vec<(usize, usize)>) {
    let end = cells
        .iter()
        .position(|c| c.symbol() == "\u{e0b0}")
        .expect("the projects segment's separator");
    let label = |c: &Cell| c.modifier.intersects(Modifier::BOLD | Modifier::DIM);
    let mut runs = Vec::new();
    let mut x = 0;
    while x < end {
        if label(&cells[x]) {
            let from = x;
            while x < end && label(&cells[x]) {
                x += 1;
            }
            runs.push((from, x));
        } else {
            x += 1;
        }
    }
    (end, runs)
}

/// At every width the segment narrows through (every chip whole, the labels
/// cut, collapsed to `1/4`, the project in view alone), a press on any cell
/// of a drawn label goes to the project it names, a press on the count opens
/// the project list, and a press anywhere else in the segment starts a text
/// selection: where a click lands and what is drawn there agree, cell for
/// cell.
#[test]
fn every_cell_of_a_drawn_label_goes_to_its_project_and_no_other_does() {
    with_mouse(|base| {
        let names = ["alpha", "bravo", "charlie", "delta"];
        for (width, collapsed) in [(160, false), (120, false), (60, true), (30, false)] {
            let dir = base.join(width.to_string());
            let (mut app, ids) = projects(&dir, &names, width);
            let (y, cells) = bar(&mut app);
            let text = bar_text(&cells);
            let (end, labels) = drawn_labels(&cells);
            let count = text.find("1/4").filter(|_| collapsed).map(|byte| {
                let x = cells
                    .iter()
                    .scan(0, |at, c| {
                        let here = *at;
                        *at += c.symbol().len();
                        Some(here)
                    })
                    .position(|at| at == byte)
                    .expect("the count starts a cell");
                (x, x + 3)
            });
            assert_eq!(count.is_some(), collapsed, "width {width}: {text:?}");
            let several = labels.len() > 1;
            assert_eq!(
                several,
                width >= 120,
                "width {width}: every chip drawn: {text:?}"
            );
            for x in 0..end {
                let point = (u16::try_from(x).unwrap(), y);
                let on_label = labels.iter().position(|&(a, b)| (a..b).contains(&x));
                let on_count = count.is_some_and(|(a, b)| (a..b).contains(&x));
                press(&mut app, point);
                let to = on_label.map(|k| if several { ids[k] } else { ids[0] });
                match (to, on_count) {
                    (Some(id), _) => {
                        assert_eq!(app.state.active, id, "width {width}, column {x}: {text:?}");
                        assert_eq!(
                            app.view.mouse_selection, None,
                            "width {width}, column {x}: a label is no text to select"
                        );
                    }
                    (None, true) => {
                        assert!(project_list_open(&app), "width {width}, column {x}");
                    }
                    (None, false) => {
                        assert_eq!(app.state.active, ids[0], "width {width}, column {x}");
                        assert_eq!(
                            app.view.mouse_selection,
                            Some(MouseDragTarget::Chrome),
                            "width {width}, column {x}: {text:?}"
                        );
                    }
                }
                release(&mut app, point);
                if project_list_open(&app) {
                    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
                        .unwrap();
                }
                app.make_active(ids[0]);
            }
        }
    });
}
