//! The fade on whatever doesn't have the keys (#1175): the `^a v` scrollback
//! and a `D` pager with no split fade like the list and the pane do, and `^a d`
//! turns the fade off for the pane as well as the columns, without giving an
//! unfocused pane a cursor. Rendered whole frames over a real `cat` pane.

use std::path::Path;

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

use crate::app::{App, state};
use crate::pane::Pane;
use crate::pane::tabs::{PaneTabs, TabEntry, TabInfo};
use crate::ui::pager::{Mount, PagerView};

const AREA: Rect = Rect {
    x: 0,
    y: 0,
    width: 80,
    height: 24,
};

/// An `App` with a `cat` pane open, its keys on the file list.
fn with_pane(test: impl FnOnce(&mut App)) {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        app.state.learned.menu_seen = true;
        let wake = app.make_pane_wake();
        let root = Path::new("/");
        let pane =
            Pane::spawn("cat", 24, 80, root, &app.view.project().context_path, wake).unwrap();
        app.runtime.project_mut().pane_tabs = Some(PaneTabs::new(TabEntry::new(
            pane,
            TabInfo::new("cat", root),
        )));
        test(&mut app);
    });
}

fn pager(mount: Mount) -> PagerView {
    let mut view = PagerView::new_plain("page", (0..40).map(|i| format!("line {i}")).collect());
    view.mount = mount;
    view
}

fn focus(app: &mut App, focus: state::Focus) {
    app.state.active_project_mut().focus = focus;
}

fn draw(app: &mut App) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(AREA.width, AREA.height)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    terminal.backend().buffer().clone()
}

/// Whether every cell of `rect` carries `modifier`.
fn all(buf: &Buffer, rect: Rect, modifier: Modifier) -> bool {
    rect.positions()
        .all(|p| buf.cell(p).is_some_and(|c| c.modifier.contains(modifier)))
}

/// Whether any cell of `rect` carries `modifier`.
fn any(buf: &Buffer, rect: Rect, modifier: Modifier) -> bool {
    rect.positions()
        .any(|p| buf.cell(p).is_some_and(|c| c.modifier.contains(modifier)))
}

fn pane_rect(app: &App) -> Rect {
    app.frame_layout(AREA).pane.expect("the pane is open")
}

#[test]
fn the_scrollback_fades_while_the_pane_lacks_the_keys() {
    with_pane(|app| {
        app.view.project_mut().scroll_pager = Some(pager(Mount::LowerPane));
        let rect = pane_rect(app);

        focus(app, state::Focus::FileList);
        assert!(
            all(&draw(app), rect, Modifier::DIM),
            "the list has the keys"
        );

        focus(app, state::Focus::Pane);
        assert!(
            !all(&draw(app), rect, Modifier::DIM),
            "the scrollback has the keys"
        );

        focus(app, state::Focus::FileList);
        app.view.dim_inactive = false;
        assert!(
            !all(&draw(app), rect, Modifier::DIM),
            "`^a d` turned the fade off"
        );
    });
}

#[test]
fn a_d_pager_with_no_split_fades_while_the_pane_has_the_keys() {
    with_pane(|app| {
        app.view.project_mut().pager = Some(pager(Mount::TopPane));
        let rect = app.frame_layout(AREA).top_unit;

        focus(app, state::Focus::Pane);
        assert!(
            all(&draw(app), rect, Modifier::DIM),
            "the pane has the keys"
        );

        focus(app, state::Focus::Pager(Mount::TopPane));
        assert!(
            !all(&draw(app), rect, Modifier::DIM),
            "the pager has the keys"
        );

        focus(app, state::Focus::Pane);
        app.view.dim_inactive = false;
        assert!(
            !all(&draw(app), rect, Modifier::DIM),
            "`^a d` turned the fade off"
        );
    });
}

#[test]
fn ctrl_a_d_keeps_an_unfocused_pane_bright_and_cursorless() {
    with_pane(|app| {
        let rect = pane_rect(app);
        focus(app, state::Focus::FileList);
        assert!(all(&draw(app), rect, Modifier::DIM), "faded by default");

        app.view.dim_inactive = false;
        let buf = draw(app);
        assert!(
            !any(&buf, rect, Modifier::DIM),
            "`^a d` turned the fade off"
        );
        assert!(
            !any(&buf, rect, Modifier::REVERSED),
            "a pane without the keys draws no cursor block"
        );

        focus(app, state::Focus::Pane);
        assert!(
            any(&draw(app), rect, Modifier::REVERSED),
            "the focused pane draws its cursor block, so the check above can fail"
        );
    });
}
