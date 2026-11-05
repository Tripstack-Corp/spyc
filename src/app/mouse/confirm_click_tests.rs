//! A confirm prompt answered by a click. A left click on the `y` or the `n`
//! of a prompt's `[y/N]`, `(y/N)` or `[Y/n]` answers as that key does, and so
//! does one on a consent pop-up's `[y]` or `[n]`; anywhere else the prompt
//! stays up. Each case raises the prompt as the app does, goes through
//! `handle_mouse`, and aims at what the frame draws.

use super::menu_tests::{app_over_entries, app_with_tab, ctrl_a, cursor_on, flash};
use super::tests::{FRAME, centre, click, drawn, press};
use crate::app::archive_ops::{ArchiveOp, ArchiveOutcome};
use crate::app::{App, Effect, Mode, Prompt, PromptKind};
use crate::state::sessions::AgentKind;
use std::path::Path;

/// Run `body` over a fresh state root, with mouse reporting on, as a click
/// needs it.
fn with_mouse(body: impl FnOnce(&Path)) {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        crate::set_mouse_capture_for_test(true);
        body(tmp.path());
        crate::set_mouse_capture_for_test(false);
    });
}

/// The open prompt's kind, if a prompt is open.
fn prompting(app: &App) -> Option<&PromptKind> {
    match &app.state.mode {
        Mode::Prompting(p) => Some(&p.kind),
        Mode::Normal => None,
    }
}

/// Where `text` is drawn on screen, searched from the bottom row up so the
/// prompt row is met first.
fn drawn_from_the_bottom(app: &mut App, text: &str) -> Option<(u16, u16)> {
    let buf = drawn(app);
    for y in (0..FRAME.height).rev() {
        let cells: Vec<String> = (0..FRAME.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect();
        let Some(byte) = cells.concat().find(text) else {
            continue;
        };
        let mut at = 0;
        for (i, cell) in cells.iter().enumerate() {
            if at == byte {
                return Some((u16::try_from(i).unwrap(), y));
            }
            at += cell.len();
        }
    }
    None
}

/// Where the prompt row draws the `y` (`answer` `'y'`) or the `n` of its
/// choice.
fn choice(app: &mut App, answer: char) -> (u16, u16) {
    let (x, y) = ["[y/N]", "(y/N)", "[Y/n]"]
        .iter()
        .find_map(|c| drawn_from_the_bottom(app, c))
        .expect("a y/N choice is drawn");
    let prompt = app.frame_layout(FRAME).prompt;
    assert!(
        prompt.contains(ratatui::layout::Position::new(x, y)),
        "premise: the choice is on the prompt row"
    );
    (if answer == 'y' { x + 1 } else { x + 3 }, y)
}

/// The active tab's pane id: a restart gives it a new one.
fn pane_id(app: &App) -> String {
    app.runtime
        .project()
        .pane_tabs
        .as_ref()
        .expect("a tab")
        .active_info()
        .id
        .clone()
}

fn tab_count(app: &App) -> usize {
    app.runtime
        .project()
        .pane_tabs
        .as_ref()
        .map_or(0, crate::pane::tabs::PaneTabs::len)
}

/// `^a R` on the live tab: its restart confirm is up.
fn ask_to_restart(app: &mut App) {
    ctrl_a(app);
    press(app, 'R');
    assert!(
        matches!(prompting(app), Some(PromptKind::RestartPane)),
        "premise: `^a R` asks first"
    );
}

/// A zip at `tmp/pkg.zip`, whose mount asks first: its `[Y/n]` is up.
fn ask_to_mount(app: &mut App, tmp: &Path) {
    use std::io::Write as _;
    let archive = tmp.join("pkg.zip");
    if !archive.exists() {
        let mut w = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
        w.start_file("README.md", opts).unwrap();
        w.write_all(b"# pkg\n").unwrap();
        w.finish().unwrap();
    }
    app.file_archive_outcome(ArchiveOutcome::NeedsConfirm {
        path: archive,
        question: "pkg.zip unpacks to 9 GB. Read it?".to_string(),
        then: None,
    });
    app.apply_archive_outcomes();
    assert!(
        matches!(prompting(app), Some(PromptKind::ArchiveMountConfirm { .. })),
        "premise: the mount asks first"
    );
}

// ── the prompt row's choice ──

/// `^a R` on a live tab, then a click on `y`, restarts it, as `y` does.
#[test]
#[ignore = "red: a click on a prompt's y is swallowed"]
fn a_click_on_y_restarts_the_tab() {
    with_mouse(|tmp| {
        let mut app = app_with_tab(tmp, "cat");
        let before = pane_id(&app);
        ask_to_restart(&mut app);

        let at = choice(&mut app, 'y');
        click(&mut app, at);

        assert!(prompting(&app).is_none(), "the prompt is answered");
        assert_ne!(pane_id(&app), before, "and the tab restarted");
    });
}

/// `^a x` on a live tab, then a click on `n`, keeps it, as `n` does.
#[test]
#[ignore = "red: a click on a prompt's n is swallowed"]
fn a_click_on_n_keeps_the_tab() {
    with_mouse(|tmp| {
        let mut app = app_with_tab(tmp, "cat");
        let before = pane_id(&app);
        ctrl_a(&mut app);
        press(&mut app, 'x');
        assert!(
            matches!(prompting(&app), Some(PromptKind::ClosePane)),
            "premise: `^a x` asks first"
        );

        let at = choice(&mut app, 'n');
        click(&mut app, at);

        assert!(prompting(&app).is_none(), "the prompt is answered");
        assert_eq!(tab_count(&app), 1, "the tab stays");
        assert_eq!(pane_id(&app), before, "the same tab");
    });
}

/// A `(y/N):` prompt takes the click too: `dd`, then `y`, removes the file.
#[test]
#[ignore = "red: a click on a prompt's y is swallowed"]
fn a_click_on_y_answers_the_remove_confirm() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        cursor_on(&mut app, "notes.txt");
        press(&mut app, 'd');
        press(&mut app, 'd');
        assert!(
            matches!(prompting(&app), Some(PromptKind::RemoveConfirm)),
            "premise: `dd` asks first"
        );

        let at = choice(&mut app, 'y');
        let fx = click(&mut app, at);

        assert!(prompting(&app).is_none(), "the prompt is answered");
        assert!(
            fx.iter().any(|e| matches!(e, Effect::Graveyard(_))),
            "the file goes to the graveyard: {fx:?}"
        );
    });
}

/// A `[Y/n]` prompt: a click on `n` declines the mount, and one on `Y`
/// re-issues it as confirmed, as the keys do.
#[test]
#[ignore = "red: a click on a prompt's Y or n is swallowed"]
fn a_mount_confirms_y_and_n_take_a_click() {
    with_mouse(|tmp| {
        let mut app = App::test_app(tmp.to_path_buf());
        app.view.term_size = (FRAME.width, FRAME.height);
        ask_to_mount(&mut app, tmp);

        let at = choice(&mut app, 'n');
        click(&mut app, at);
        assert!(prompting(&app).is_none(), "`n` answered it");
        assert_eq!(flash(&app).as_deref(), Some("archive: not mounted"));

        ask_to_mount(&mut app, tmp);
        let at = choice(&mut app, 'y');
        let fx = click(&mut app, at);
        assert!(prompting(&app).is_none(), "`Y` answered it");
        assert!(
            fx.iter().any(|e| matches!(
                e,
                Effect::Archive(ArchiveOp::Mount {
                    confirmed: true,
                    ..
                })
            )),
            "the mount goes ahead, confirmed: {fx:?}"
        );
    });
}

/// A click off the choice leaves the prompt up and the tab running: on the
/// question, the list and the pane.
#[test]
fn a_click_elsewhere_leaves_the_prompt_up() {
    with_mouse(|tmp| {
        let mut app = app_with_tab(tmp, "cat");
        let before = pane_id(&app);
        ask_to_restart(&mut app);
        let question = drawn_from_the_bottom(&mut app, "restart running tab").expect("drawn");
        let layout = app.frame_layout(FRAME);
        let pane = layout.pane.expect("a pane");

        for at in [question, centre(layout.list), centre(pane)] {
            click(&mut app, at);
            assert!(
                matches!(prompting(&app), Some(PromptKind::RestartPane)),
                "a click at {at:?} left the prompt up"
            );
        }
        assert_eq!(pane_id(&app), before, "and the tab running");
    });
}

// ── a consent's pop-up ──

/// The agent consent's pop-up takes a click on its `[n]` as `n`: the answer
/// is recorded and the pop-up closes.
#[test]
#[ignore = "red: a click on a consent's [n] is swallowed"]
fn a_click_on_a_consents_n_declines_it() {
    with_mouse(|tmp| {
        let mut app = app_over_entries(tmp);
        app.state.mode = Mode::Prompting(Prompt::simple(
            PromptKind::HookConsent {
                agent: AgentKind::Claude,
            },
            "Show claude's live status on its tab?",
        ));
        let at = drawn_from_the_bottom(&mut app, "[n] no").expect("the pop-up's [n]");

        click(&mut app, at);

        assert!(prompting(&app).is_none(), "the pop-up is answered");
        assert_eq!(
            crate::state::wiring_consent::answer(AgentKind::Claude),
            Some(false),
            "and the answer kept"
        );
    });
}
