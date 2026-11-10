//! Real PTY output, application mouse routing, rendered text and clipboard payloads.

use crate::app::{App, Effect};
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use std::time::{Duration, Instant};

const COMMAND: &str = "cargo test --locked";
const LAST_COL: u16 = 2 + COMMAND.len() as u16 - 1;

struct Capture;
impl Capture {
    fn new() -> Self {
        crate::set_mouse_capture_for_test(true);
        Self
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        crate::set_mouse_capture_for_test(false);
    }
}

fn fixture(dir: &std::path::Path, mouse: bool) -> App {
    let script = dir.join("question.sh");
    let modes = if mouse { r"\033[?1002h\033[?1006h" } else { "" };
    std::fs::write(&script, format!(r"stty -echo -icanon min 1 time 0
printf '\033[?1049h\033[?25l{modes}\033[2J\033[HQueued follow-up inputs\033[3;3H{COMMAND}\033[5;3H1. Continue\033[6;3H2. Other'
dd bs=1 count=1 >/dev/null 2>&1
printf '\033[3;3H\033[2Kreplacement command'
dd bs=1 count=1 >/dev/null 2>&1
")).unwrap();
    let mut app = App::test_app(dir.to_path_buf());
    app.view.term_size = (60, 24);
    app.open_pane_tab(&format!("/bin/sh {}", script.display()));
    wait_for(&mut app, COMMAND);
    app
}

fn wait_for(app: &mut App, text: &str) {
    use crate::pane::engine::TerminalScreen as _;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let found = app
            .runtime
            .project()
            .pane_tabs
            .as_ref()
            .unwrap()
            .active()
            .with_screen(|s| s.contents().contains(text));
        app.drain_pane_output();
        if found {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("PTY never painted {text:?}");
}

fn render(app: &mut App) -> (Rect, String) {
    let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    let pane = app.frame_layout(Rect::new(0, 0, 60, 24)).pane.unwrap();
    let row = (0..pane.width)
        .map(|x| {
            terminal
                .backend()
                .buffer()
                .cell((pane.x + x, pane.y + 2))
                .unwrap()
                .symbol()
        })
        .collect();
    (pane, row)
}

fn event(pane: Rect, kind: MouseEventKind, col: u16, modifiers: KeyModifiers) -> MouseEvent {
    MouseEvent {
        kind,
        column: pane.x + col,
        row: pane.y + 2,
        modifiers,
    }
}

fn copied(effects: &[Effect]) -> Option<&str> {
    effects.iter().find_map(|e| match e {
        Effect::CopyToClipboard { text, .. } => Some(text.as_str()),
        _ => None,
    })
}

fn repaint(app: &mut App) {
    app.runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .unwrap()
        .active_mut()
        .send_bytes(b"r")
        .unwrap();
    wait_for(app, "replacement command");
}

#[test]
fn ctrl_drag_copies_question_text_without_forwarding_even_during_redraw() {
    let _lock = crate::mouse_test_lock();
    let _capture = Capture::new();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_state_root(dir.path(), || {
        let mut app = fixture(dir.path(), true);
        let (pane, row) = render(&mut app);
        assert!(row.contains(COMMAND));
        assert!(
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active()
                .wants_mouse()
        );
        let press = app.handle_mouse(event(
            pane,
            MouseEventKind::Down(MouseButton::Left),
            2,
            KeyModifiers::CONTROL,
        ));
        assert!(
            press.is_empty(),
            "Ctrl+press must belong to spyc, not the child: {press:?}"
        );
        app.handle_mouse(event(
            pane,
            MouseEventKind::Drag(MouseButton::Left),
            LAST_COL,
            KeyModifiers::NONE,
        ));
        repaint(&mut app);
        assert!(
            render(&mut app).1.contains(COMMAND),
            "a held drag must still show the command it selects"
        );
        let release = app.handle_mouse(event(
            pane,
            MouseEventKind::Up(MouseButton::Left),
            LAST_COL,
            KeyModifiers::NONE,
        ));
        assert_eq!(copied(&release), Some(COMMAND));
        assert!(
            render(&mut app).1.contains("replacement command"),
            "release resumes the live view"
        );
    });
}

#[test]
fn ordinary_spyc_drag_copies_the_text_it_held_before_redraw() {
    let _lock = crate::mouse_test_lock();
    let _capture = Capture::new();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_state_root(dir.path(), || {
        let mut app = fixture(dir.path(), false);
        let (pane, _) = render(&mut app);
        app.handle_mouse(event(
            pane,
            MouseEventKind::Down(MouseButton::Left),
            2,
            KeyModifiers::NONE,
        ));
        app.handle_mouse(event(
            pane,
            MouseEventKind::Drag(MouseButton::Left),
            LAST_COL,
            KeyModifiers::NONE,
        ));
        repaint(&mut app);
        let release = app.handle_mouse(event(
            pane,
            MouseEventKind::Up(MouseButton::Left),
            LAST_COL,
            KeyModifiers::NONE,
        ));
        assert_eq!(copied(&release), Some(COMMAND));
    });
}

#[test]
fn normal_drag_still_forwards_the_whole_gesture_to_a_mouse_child() {
    let _lock = crate::mouse_test_lock();
    let _capture = Capture::new();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_state_root(dir.path(), || {
        let mut app = fixture(dir.path(), true);
        let (pane, _) = render(&mut app);
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            let effects = app.handle_mouse(event(pane, kind, 2, KeyModifiers::NONE));
            assert!(matches!(effects.as_slice(), [Effect::SendToPane { .. }]));
            assert!(copied(&effects).is_none());
        }
    });
}

/// The mouse fixture's child, hosted as a Claude tab.
fn agent_fixture(dir: &std::path::Path) -> App {
    let mut app = fixture(dir, true);
    app.runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .expect("a tab")
        .active_info_mut()
        .agent_hint = Some(crate::state::sessions::AgentKind::Claude);
    app
}

fn sent(effects: &[Effect]) -> Vec<u8> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::SendToPane {
                input: crate::app::effect::PaneInput::Bytes(bytes),
                ..
            } => Some(bytes.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect()
}

/// In an agent tab a plain drag is spyc's, as Ctrl+drag is elsewhere: the
/// child hears none of it, the pane holds the text under the pointer while
/// its output moves on, release copies what was there at the press, and the
/// live grid comes back.
#[test]
#[ignore = "red: a plain press in an agent tab is forwarded to the child"]
fn a_plain_drag_in_an_agent_tab_holds_and_copies_the_press_time_text() {
    let _lock = crate::mouse_test_lock();
    let _capture = Capture::new();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_state_root(dir.path(), || {
        let mut app = agent_fixture(dir.path());
        let (pane, _) = render(&mut app);
        let mut heard = Vec::new();
        for (kind, col) in [
            (MouseEventKind::Down(MouseButton::Left), 2),
            (MouseEventKind::Drag(MouseButton::Left), LAST_COL),
        ] {
            heard.extend(sent(&app.handle_mouse(event(
                pane,
                kind,
                col,
                KeyModifiers::NONE,
            ))));
        }
        repaint(&mut app);
        assert!(
            render(&mut app).1.contains(COMMAND),
            "the held drag shows the text it selects, not the new output"
        );
        let release = app.handle_mouse(event(
            pane,
            MouseEventKind::Up(MouseButton::Left),
            LAST_COL,
            KeyModifiers::NONE,
        ));
        heard.extend(sent(&release));
        assert!(heard.is_empty(), "the child heard the drag: {heard:?}");
        assert_eq!(copied(&release), Some(COMMAND));
        assert!(
            render(&mut app).1.contains("replacement command"),
            "release resumes the live view"
        );
    });
}

/// A plain click that never moves still reaches an agent tab's child, its
/// press and its release both, once the release shows it wasn't a drag.
#[test]
#[ignore = "red: the press reaches the child at once, before the release decides"]
fn a_plain_click_in_an_agent_tab_reaches_the_child_on_release() {
    let _lock = crate::mouse_test_lock();
    let _capture = Capture::new();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_state_root(dir.path(), || {
        let mut app = agent_fixture(dir.path());
        let (pane, _) = render(&mut app);
        let press = app.handle_mouse(event(
            pane,
            MouseEventKind::Down(MouseButton::Left),
            2,
            KeyModifiers::NONE,
        ));
        assert!(sent(&press).is_empty(), "the press waits: {press:?}");
        let release = app.handle_mouse(event(
            pane,
            MouseEventKind::Up(MouseButton::Left),
            2,
            KeyModifiers::NONE,
        ));
        assert_eq!(
            String::from_utf8(sent(&release)).unwrap(),
            "\x1b[<0;3;3M\x1b[<0;3;3m",
            "the click reaches the child whole"
        );
        assert!(copied(&release).is_none());
    });
}

#[test]
fn ctrl_click_copies_nothing_and_resumes_the_live_grid() {
    let _lock = crate::mouse_test_lock();
    let _capture = Capture::new();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_state_root(dir.path(), || {
        let mut app = fixture(dir.path(), true);
        let (pane, _) = render(&mut app);
        app.handle_mouse(event(
            pane,
            MouseEventKind::Down(MouseButton::Left),
            2,
            KeyModifiers::CONTROL,
        ));
        repaint(&mut app);
        let release = app.handle_mouse(event(
            pane,
            MouseEventKind::Up(MouseButton::Left),
            2,
            KeyModifiers::NONE,
        ));
        assert!(release.is_empty());
        assert!(render(&mut app).1.contains("replacement command"));
    });
}

#[test]
fn a_key_cancels_the_held_grid_and_a_late_release_does_not_copy() {
    let _lock = crate::mouse_test_lock();
    let _capture = Capture::new();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_state_root(dir.path(), || {
        let mut app = fixture(dir.path(), true);
        let (pane, _) = render(&mut app);
        app.handle_mouse(event(
            pane,
            MouseEventKind::Down(MouseButton::Left),
            2,
            KeyModifiers::CONTROL,
        ));
        app.handle_mouse(event(
            pane,
            MouseEventKind::Drag(MouseButton::Left),
            LAST_COL,
            KeyModifiers::CONTROL,
        ));
        repaint(&mut app);
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            KeyModifiers::NONE,
        ))
        .unwrap();
        assert!(render(&mut app).1.contains("replacement command"));
        let release = app.handle_mouse(event(
            pane,
            MouseEventKind::Up(MouseButton::Left),
            LAST_COL,
            KeyModifiers::NONE,
        ));
        assert!(copied(&release).is_none());
    });
}

#[test]
fn changing_tabs_cancels_the_old_panes_drag() {
    let _lock = crate::mouse_test_lock();
    let _capture = Capture::new();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_state_root(dir.path(), || {
        let mut app = fixture(dir.path(), true);
        let (pane, _) = render(&mut app);
        app.handle_mouse(event(
            pane,
            MouseEventKind::Down(MouseButton::Left),
            2,
            KeyModifiers::CONTROL,
        ));
        app.handle_mouse(event(
            pane,
            MouseEventKind::Drag(MouseButton::Left),
            LAST_COL,
            KeyModifiers::CONTROL,
        ));
        app.open_pane_tab("/bin/sh -c 'exec sleep 30'");
        render(&mut app);
        let release = app.handle_mouse(event(
            pane,
            MouseEventKind::Up(MouseButton::Left),
            LAST_COL,
            KeyModifiers::NONE,
        ));
        assert!(
            copied(&release).is_none(),
            "the old tab's grid must not be copied from the new tab"
        );
    });
}

/// A change to the pane's rect while a drag is held waits for the release, so
/// the text under the pointer stays put; the new rect follows it.
#[test]
#[ignore = "red: a geometry change cancels the held drag instead of waiting for it"]
fn changing_pane_geometry_waits_for_the_drag() {
    let _lock = crate::mouse_test_lock();
    let _capture = Capture::new();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_state_root(dir.path(), || {
        let mut app = fixture(dir.path(), true);
        let (pane, _) = render(&mut app);
        app.handle_mouse(event(
            pane,
            MouseEventKind::Down(MouseButton::Left),
            2,
            KeyModifiers::CONTROL,
        ));
        app.handle_mouse(event(
            pane,
            MouseEventKind::Drag(MouseButton::Left),
            LAST_COL,
            KeyModifiers::CONTROL,
        ));
        app.state.active_project_mut().pane.pane_height_pct = 75;
        let (held, row) = render(&mut app);
        assert_eq!(held, pane, "the pane keeps its rect while the drag is held");
        assert!(row.contains(COMMAND), "and its text: {row:?}");
        let release = app.handle_mouse(event(
            pane,
            MouseEventKind::Up(MouseButton::Left),
            LAST_COL,
            KeyModifiers::NONE,
        ));
        assert_eq!(copied(&release), Some(COMMAND));
        let (after, _) = render(&mut app);
        assert_ne!(after, pane, "the deferred geometry lands on release");
    });
}

#[test]
fn a_paste_or_wheel_cancels_a_drag_without_copying() {
    let _lock = crate::mouse_test_lock();
    let _capture = Capture::new();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_state_root(dir.path(), || {
        for paste in [false, true] {
            let mut app = fixture(dir.path(), true);
            let (pane, _) = render(&mut app);
            app.handle_mouse(event(
                pane,
                MouseEventKind::Down(MouseButton::Left),
                2,
                KeyModifiers::CONTROL,
            ));
            app.handle_mouse(event(
                pane,
                MouseEventKind::Drag(MouseButton::Left),
                LAST_COL,
                KeyModifiers::CONTROL,
            ));
            repaint(&mut app);
            if paste {
                app.handle_paste("input".into());
            } else {
                app.handle_mouse(event(
                    pane,
                    MouseEventKind::ScrollDown,
                    2,
                    KeyModifiers::NONE,
                ));
            }
            assert!(render(&mut app).1.contains("replacement command"));
            let release = app.handle_mouse(event(
                pane,
                MouseEventKind::Up(MouseButton::Left),
                LAST_COL,
                KeyModifiers::NONE,
            ));
            assert!(copied(&release).is_none());
        }
    });
}

#[test]
fn a_backwards_drag_copies_the_same_command() {
    let _lock = crate::mouse_test_lock();
    let _capture = Capture::new();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_state_root(dir.path(), || {
        let mut app = fixture(dir.path(), true);
        let (pane, _) = render(&mut app);
        app.handle_mouse(event(
            pane,
            MouseEventKind::Down(MouseButton::Left),
            LAST_COL,
            KeyModifiers::CONTROL,
        ));
        let release = app.handle_mouse(event(
            pane,
            MouseEventKind::Up(MouseButton::Left),
            2,
            KeyModifiers::NONE,
        ));
        assert_eq!(copied(&release), Some(COMMAND));
        assert!(render(&mut app).1.contains(COMMAND));
    });
}

#[test]
fn disabling_mouse_capture_cancels_the_held_grid() {
    let _lock = crate::mouse_test_lock();
    let _capture = Capture::new();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_state_root(dir.path(), || {
        let mut app = fixture(dir.path(), true);
        let (pane, _) = render(&mut app);
        app.handle_mouse(event(
            pane,
            MouseEventKind::Down(MouseButton::Left),
            2,
            KeyModifiers::CONTROL,
        ));
        app.handle_mouse(event(
            pane,
            MouseEventKind::Drag(MouseButton::Left),
            LAST_COL,
            KeyModifiers::CONTROL,
        ));
        repaint(&mut app);
        crate::set_mouse_capture_for_test(false);
        assert!(render(&mut app).1.contains("replacement command"));
        crate::set_mouse_capture_for_test(true);
        let release = app.handle_mouse(event(
            pane,
            MouseEventKind::Up(MouseButton::Left),
            LAST_COL,
            KeyModifiers::NONE,
        ));
        assert!(copied(&release).is_none());
    });
}

fn command_is_highlighted(app: &mut App) -> bool {
    let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    let pane = app.frame_layout(Rect::new(0, 0, 60, 24)).pane.unwrap();
    (2..=LAST_COL).all(|col| {
        terminal
            .backend()
            .buffer()
            .cell((pane.x + col, pane.y + 2))
            .unwrap()
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    })
}

#[test]
fn a_released_highlight_does_not_move_to_different_live_text() {
    let _lock = crate::mouse_test_lock();
    let _capture = Capture::new();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_state_root(dir.path(), || {
        let mut app = fixture(dir.path(), true);
        let (pane, _) = render(&mut app);
        app.handle_mouse(event(
            pane,
            MouseEventKind::Down(MouseButton::Left),
            2,
            KeyModifiers::CONTROL,
        ));
        app.handle_mouse(event(
            pane,
            MouseEventKind::Drag(MouseButton::Left),
            LAST_COL,
            KeyModifiers::NONE,
        ));
        let release = app.handle_mouse(event(
            pane,
            MouseEventKind::Up(MouseButton::Left),
            LAST_COL,
            KeyModifiers::NONE,
        ));
        assert_eq!(copied(&release), Some(COMMAND));
        assert!(command_is_highlighted(&mut app));
        app.runtime
            .project_mut()
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_mut()
            .send_bytes(b"r")
            .unwrap();
        // Let the parser repaint before the application lands its output wake.
        // The render must validate the old band under the same lock as the grid.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            use crate::pane::engine::TerminalScreen as _;
            if app
                .runtime
                .project()
                .pane_tabs
                .as_ref()
                .unwrap()
                .active()
                .with_screen(|s| s.contents().contains("replacement command"))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "PTY did not repaint after release"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !command_is_highlighted(&mut app),
            "a released band must identify only the copied bytes"
        );
    });
}
