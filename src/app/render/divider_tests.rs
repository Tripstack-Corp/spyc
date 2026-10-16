//! The pane divider (`render_pane_status_line`), rendered one row wide: tab
//! cells, the active tab's cwd, background-task tags and the mode tags (#175).
//! Panes are real `cat` children started at `/`, so the cwd text is the same on
//! every machine.
#![allow(clippy::unwrap_used)] // test fixtures; the module deny is for production

use std::path::{Path, PathBuf};

use ratatui::style::{Color, Modifier};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

use crate::app::{App, TaskStatus, state};
use crate::pane::{
    AgentActivity, Pane,
    tabs::{PaneTabs, TabEntry, TabInfo},
};

/// The pale red the divider draws a blocked agent and a failed task in.
const ALARM: Color = Color::Rgb(0xF7, 0x76, 0x8E);

/// An `App` with a tab per label, all at `/`, the first one active.
fn with_tabs(labels: &[&str], test: impl FnOnce(&mut App)) {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        for label in labels {
            let wake = app.make_pane_wake();
            let root = Path::new("/");
            let pane =
                Pane::spawn("cat", 24, 80, root, &app.view.project().context_path, wake).unwrap();
            let entry = TabEntry::new(pane, TabInfo::new(*label, root));
            match app.runtime.project_mut().pane_tabs.as_mut() {
                Some(tabs) => tabs.push(entry),
                None => app.runtime.project_mut().pane_tabs = Some(PaneTabs::new(entry)),
            }
        }
        tabs(&mut app).switch_to(0);
        test(&mut app);
    });
}

fn tabs(app: &mut App) -> &mut PaneTabs {
    app.runtime.project_mut().pane_tabs.as_mut().unwrap()
}

fn info(app: &mut App, tab: usize) -> &mut TabInfo {
    &mut tabs(app).tabs_mut()[tab].info
}

fn draw(app: &App, width: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
    terminal
        .draw(|f| app.render_pane_status_line(f, f.area()))
        .unwrap();
    terminal.backend().buffer().clone()
}

/// The row's text. A wide glyph's second cell reads as a space.
fn row(app: &App, width: u16) -> String {
    let buf = draw(app, width);
    (0..width)
        .map(|x| buf.cell((x, 0)).map_or(" ", |c| c.symbol()))
        .collect()
}

/// The column where `needle` starts in the row.
fn column_of(app: &App, width: u16, needle: &str) -> u16 {
    let text = row(app, width);
    let byte = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} in {text:?}"));
    u16::try_from(text[..byte].chars().count()).unwrap()
}

fn style_at(app: &App, width: u16, needle: &str) -> ratatui::style::Style {
    let x = column_of(app, width, needle);
    draw(app, width).cell((x, 0)).unwrap().style()
}

/// `count` background tasks, `cat`s sent to the background with `^Z`.
fn add_tasks(app: &mut App, count: usize) {
    for _ in 0..count {
        app.start_capture("cat", "cat", "cat");
        app.background_capture();
    }
}

#[test]
fn each_tab_shows_its_number_a_status_cell_and_its_label() {
    with_tabs(&["cat", "zsh"], |app| {
        assert_eq!(
            row(app, 40),
            "─[1]  cat ─[2]  zsh ── / ───────────────",
            "a quiet tab's cell is a blank, so the bar keeps its width"
        );
        assert!(
            style_at(app, 40, "[1]")
                .add_modifier
                .contains(Modifier::REVERSED),
            "the active tab is drawn in reverse"
        );

        info(app, 1).has_activity = true;
        assert_eq!(row(app, 40), "─[1]  cat ─[2]+ zsh ── / ───────────────");
        let unseen = style_at(app, 40, "[2]");
        assert_eq!(
            unseen.fg,
            Some(app.view.theme.pick),
            "unseen output stands out"
        );
        assert!(!unseen.add_modifier.contains(Modifier::REVERSED));
    });
}

#[test]
fn an_agent_tab_shows_its_activity_and_a_suspended_tab_sleeps() {
    with_tabs(&["cat", "claude"], |app| {
        for (activity, dot) in [
            (AgentActivity::Working, "●"),
            (AgentActivity::Idle, "·"),
            (AgentActivity::Blocked, "■"),
            (AgentActivity::Done, "■"),
        ] {
            info(app, 1).activity = activity;
            let text = row(app, 30);
            assert!(
                text.starts_with(&format!("─[1]  cat ─[2]{dot} claude ")),
                "{activity:?}: {text:?}"
            );
        }
        info(app, 1).activity = AgentActivity::Blocked;
        assert_eq!(style_at(app, 30, "■").fg, Some(ALARM), "blocked: needs you");
        info(app, 1).activity = AgentActivity::Done;
        assert_eq!(
            style_at(app, 30, "■").fg,
            Some(app.view.theme.take),
            "done: calm"
        );

        info(app, 1).suspended = true;
        let text = row(app, 30);
        assert!(
            text.starts_with("─[1]  cat ─[2]💤  claude "),
            "a suspended tab sleeps, whatever its agent was doing: {text:?}"
        );
    });
}

#[test]
fn a_cwd_the_tab_has_moved_from_is_marked_and_a_long_one_keeps_its_tail() {
    with_tabs(&["cat"], |app| {
        assert_eq!(row(app, 30), "─[1]  cat ── / ───────────────");
        tabs(app).tabs_mut()[0].set_live_cwd(PathBuf::from("/usr/local/share/a/b/c/leaf"));
        assert_eq!(
            row(app, 50),
            "─[1]  cat ── ↪ /usr/local/share/a/b/c/leaf ───────"
        );
        assert!(
            style_at(app, 50, "↪")
                .add_modifier
                .contains(Modifier::REVERSED),
            "a moved cwd is drawn like the active tab"
        );
        assert_eq!(
            row(app, 34),
            "─[1]  cat ── ↪ …/a/b/c/leaf ──────",
            "the tail is kept"
        );
    });
}

#[test]
fn background_tasks_are_tagged_on_the_right_and_the_oldest_drop_first() {
    with_tabs(&["cat"], |app| {
        add_tasks(app, 5);
        let tasks = &mut app.runtime.background_tasks.tasks;
        tasks[1].has_unread_output = true;
        tasks[2].status = TaskStatus::Exited(0);
        tasks[3].status = TaskStatus::Exited(3);
        tasks[4].paused = true;
        tasks[4].has_unread_output = true;
        let text = row(app, 50);
        assert!(
            text.ends_with("─ [1●] [2+] [3✓] [4✗] [5⏸]"),
            "running, unread, clean exit, failed, paused: {text:?}"
        );
        assert_eq!(style_at(app, 50, "[2+]").fg, Some(app.view.theme.take));
        assert_eq!(style_at(app, 50, "[3✓]").fg, Some(app.view.theme.exec));
        assert_eq!(style_at(app, 50, "[4✗]").fg, Some(ALARM));
        let text = row(app, 36);
        assert!(
            text.ends_with("─ [3✓] [4✗] [5⏸]") && !text.contains("[2+]"),
            "out of room, the oldest go first: {text:?}"
        );
    });
}

#[test]
fn a_zoomed_pane_and_scroll_mode_are_tagged() {
    with_tabs(&["cat"], |app| {
        app.state.active_project_mut().pane.zoom = state::ZoomTarget::BottomPane;
        assert_eq!(row(app, 30), "─[1]  cat ── / ──────── [ZOOM]");
        app.state.active_project_mut().pane.zoom = state::ZoomTarget::None;

        tabs(app).active_mut().enter_scroll_mode();
        assert_eq!(
            row(app, 30),
            "─[1]  CAT ── / ────── [SCROLL]",
            "scrolling shouts the active tab's name"
        );
        assert_eq!(style_at(app, 30, "──────").fg, Some(app.view.theme.dir));
        assert_eq!(style_at(app, 30, "[1]").fg, Some(app.view.theme.dir));
    });
}

/// A tab that doesn't fit is left off rather than drawn half-way, and the
/// cwd goes before any tab does.
#[test]
fn a_narrow_divider_drops_the_cwd_first() {
    with_tabs(&["cat", "zsh"], |app| {
        assert_eq!(row(app, 26), "─[1]  cat ─[2]  zsh ──────");
    });
}

/// `[SCROLL]` is the cue that the pane isn't live, so a long cwd gives way to
/// it rather than pushing it off the end of the row.
#[test]
fn scroll_mode_keeps_its_tag_beside_a_long_cwd() {
    with_tabs(&["cat"], |app| {
        tabs(app).tabs_mut()[0].set_live_cwd(PathBuf::from("/usr/local/share/a/b/c/leaf"));
        tabs(app).active_mut().enter_scroll_mode();
        for width in 30..=60 {
            assert!(
                row(app, width).ends_with(" [SCROLL]"),
                "width {width}: {:?}",
                row(app, width)
            );
        }
        app.state.active_project_mut().pane.zoom = state::ZoomTarget::BottomPane;
        assert!(
            row(app, 50).ends_with(" [ZOOM] [SCROLL]"),
            "{:?}",
            row(app, 50)
        );
    });
}
