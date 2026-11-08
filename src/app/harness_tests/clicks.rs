//! What the mouse tests share: a listing in a temporary directory with the
//! mouse on, the frame as drawn, and presses where something is drawn,
//! through `handle_mouse`.

use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use std::path::{Path, PathBuf};

/// `root/work`, canonical, holding `docs/` (with `docs/inner/` inside) and
/// `notes.txt`.
pub(super) fn work_dir(root: &Path) -> PathBuf {
    let work = std::fs::canonicalize(root).unwrap().join("work");
    std::fs::create_dir_all(work.join("docs").join("inner")).unwrap();
    std::fs::write(work.join("notes.txt"), "first line\nsecond line\n").unwrap();
    work
}

/// Run `test` against an app whose one column lists `work_dir`, with the
/// mouse on.
pub(super) fn listing(test: impl FnOnce(&mut App, &Path)) {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let work = work_dir(tmp.path());
        let mut app = App::test_app(work.clone());
        app.view.term_size = (100, 24);
        app.state.chdir(&work).unwrap();
        crate::set_mouse_capture_for_test(true);
        test(&mut app, &work);
        crate::set_mouse_capture_for_test(false);
    });
}

pub(super) fn drawn(app: &mut App) -> ratatui::buffer::Buffer {
    let (width, height) = app.view.term_size;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    terminal.backend().buffer().clone()
}

/// Where `text` is first drawn, top to bottom and left to right.
pub(super) fn drawn_at(app: &mut App, text: &str) -> (u16, u16) {
    let buf = drawn(app);
    for y in 0..buf.area.height {
        let cells: Vec<&str> = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        let Some(byte) = cells.concat().find(text) else {
            continue;
        };
        let mut at = 0;
        for (x, cell) in cells.iter().enumerate() {
            if at == byte {
                return (u16::try_from(x).unwrap(), y);
            }
            at += cell.len();
        }
    }
    panic!("{text:?} is not drawn");
}

pub(super) fn mouse(app: &mut App, kind: MouseEventKind, (column, row): (u16, u16)) -> Vec<Effect> {
    app.handle_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// A left press and release at `at`, and what they started.
pub(super) fn click(app: &mut App, at: (u16, u16)) -> Vec<Effect> {
    let mut effects = mouse(app, MouseEventKind::Down(MouseButton::Left), at);
    effects.extend(mouse(app, MouseEventKind::Up(MouseButton::Left), at));
    effects
}

/// Two clicks at `at`, as quick as a double-click is.
pub(super) fn double_click(app: &mut App, at: (u16, u16)) -> Vec<Effect> {
    let mut effects = click(app, at);
    effects.extend(click(app, at));
    effects
}

/// The file the pager over the list shows, if one is open.
pub(super) fn paged(app: &App) -> Option<PathBuf> {
    app.view.project().pager.as_ref()?.source_path.clone()
}

/// Run the directory changes in `effects` inline, as the executor does.
pub(super) fn change_dirs(app: &mut App, effects: Vec<Effect>) {
    for effect in effects {
        if let Effect::ChangeDir {
            path,
            focus,
            on_ok,
            err_prefix,
        } = effect
        {
            app.state
                .change_dir(&path, focus.as_deref(), on_ok.as_deref(), err_prefix);
        }
    }
}
