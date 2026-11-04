//! A chord's second key while a column shows the inventory or the graveyard
//! (#963). The inventory answers bare `x`, `d`, `p`, `t`, `Space` and `Esc`
//! itself, and those keys also end chords: `^a x` closes a tab, `^a p` steps
//! to the previous one, `^a Esc` sends `Esc`, `yp` yanks the pane. A chord
//! already armed finishes as that chord, and the view's own keys act only when
//! none is.

use super::*;
use crate::app::View;
use crate::app::effect::{PaneInput, PaneTarget};
use crate::state::graveyard::Graveyard;
use std::path::Path;

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn esc() -> KeyEvent {
    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
}

/// Press `keys` through `handle_key`, returning every effect they emitted.
fn press(app: &mut App, keys: &[KeyEvent]) -> Vec<Effect> {
    keys.iter()
        .flat_map(|key| app.handle_key(*key).unwrap())
        .collect()
}

fn touches_the_inventory(fx: &[Effect]) -> bool {
    fx.iter().any(|e| matches!(e, Effect::Inventory(_)))
}

fn active_tab(app: &App) -> usize {
    app.runtime
        .project()
        .pane_tabs
        .as_ref()
        .expect("pane tabs")
        .active_index()
}

fn closing_a_tab(app: &App) -> bool {
    matches!(&app.state.mode, Mode::Prompting(p) if matches!(p.kind, PromptKind::ClosePane))
}

/// A spyc in `dir` with two running pane tabs, the second active, and
/// `one.txt` and `two.txt` in the inventory.
fn spyc_with_two_tabs(dir: &Path) -> App {
    let mut app = App::test_app(dir.to_path_buf());
    app.state.refresh_listing();
    for name in ["one.txt", "two.txt"] {
        let file = dir.join(name);
        std::fs::write(&file, name).unwrap();
        app.state.inventory.yank(&file).unwrap();
    }
    assert!(app.open_pane_tab_in("sleep 30", dir));
    assert!(app.open_pane_tab_in("sleep 30", dir));
    assert_eq!(active_tab(&app), 1);
    app
}

/// The inventory view open in the column, the keyboard in the pane.
fn inventory_open(dir: &Path) -> App {
    let mut app = spyc_with_two_tabs(dir);
    app.state.toggle_inventory_view();
    assert_eq!(app.state.cur().view, View::Inventory);
    assert_eq!(app.state.cur().rows.len(), 2);
    app.set_pane_focus(true);
    app
}

/// `^a x` asks to close the running tab and `^a p` steps to the previous tab,
/// rather than dropping the cursor's item or putting the inventory here.
#[test]
#[ignore = "red: the inventory's `x` and `p` take a pane chord's second key"]
fn pane_chords_close_and_step_tabs_with_the_inventory_open() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = inventory_open(tmp.path());

        let fx = press(&mut app, &[ctrl('a'), key('x')]);
        assert!(
            !touches_the_inventory(&fx),
            "^a x acted on the inventory: {fx:?}"
        );
        assert!(closing_a_tab(&app), "^a x asks to close the running tab");
        app.state.mode = Mode::Normal;

        let fx = press(&mut app, &[ctrl('a'), key('p')]);
        assert!(
            !touches_the_inventory(&fx),
            "^a p acted on the inventory: {fx:?}"
        );
        assert_eq!(active_tab(&app), 0, "^a p steps to the previous tab");
        assert_eq!(app.state.cur().view, View::Inventory);
    });
}

/// `^a Esc` sends a plain `Esc` to the pane and leaves the inventory open.
#[test]
#[ignore = "red: the inventory's `Esc` takes `^a Esc` and closes the view"]
fn ctrl_a_esc_sends_esc_with_the_inventory_open() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = inventory_open(tmp.path());
        let fx = press(&mut app, &[ctrl('a'), esc()]);
        assert!(
            fx.iter().any(|e| matches!(
                e,
                Effect::SendToPane {
                    target: PaneTarget::Active,
                    input: PaneInput::Bytes(bytes),
                    ..
                } if bytes == &[0x1b]
            )),
            "^a Esc sends Esc to the pane: {fx:?}"
        );
        assert_eq!(app.state.cur().view, View::Inventory, "the view stays open");
    });
}

/// `^a d` and `^a t` are pane chords too: neither drops nor tags an item.
#[test]
#[ignore = "red: the inventory's `d` and `t` take a pane chord's second key"]
fn ctrl_a_d_and_t_never_drop_or_tag_an_item() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = inventory_open(tmp.path());
        for second in ['d', 't'] {
            let fx = press(&mut app, &[ctrl('a'), key(second)]);
            assert!(
                !touches_the_inventory(&fx),
                "^a {second} dropped an item: {fx:?}"
            );
            assert!(
                app.state.inventory.picks.is_empty(),
                "^a {second} tagged an item"
            );
        }
    });
}

/// With the keyboard in the column, a file chord ending in one of the
/// inventory's keys finishes as the chord: `yp` yanks the pane, it doesn't put
/// the inventory.
#[test]
#[ignore = "red: the inventory's `p` takes the second key of `yp`"]
fn a_file_chord_finishes_with_the_inventory_open() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = inventory_open(tmp.path());
        app.set_pane_focus(false);
        let fx = press(&mut app, &[key('y'), key('p')]);
        assert!(!touches_the_inventory(&fx), "yp put the inventory: {fx:?}");
    });
}

/// The inventory's own keys, with no chord armed, act on it as before.
#[test]
fn the_inventorys_own_keys_still_act_on_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = inventory_open(tmp.path());
        app.set_pane_focus(false);
        for drop in ['x', 'd'] {
            let fx = press(&mut app, &[key(drop)]);
            assert!(touches_the_inventory(&fx), "{drop} drops the cursor's item");
        }
        assert!(
            touches_the_inventory(&press(&mut app, &[key('p')])),
            "p puts"
        );
        press(&mut app, &[key('t')]);
        assert_eq!(app.state.inventory.picks.len(), 1, "t tags");
        press(&mut app, &[esc()]);
        assert_eq!(app.state.cur().view, View::Dir, "Esc closes the view");
    });
}

/// With the graveyard open in the column, a pane chord typed in the pane
/// reaches spyc: `^a` arms it, so `^a p` steps tabs and `^a x` asks to close
/// one, and neither key reaches the program.
#[test]
#[ignore = "red: the graveyard view swallows `^a`, so `p` and `x` go to the program"]
fn pane_chords_finish_while_the_graveyard_is_open() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let buried = tmp.path().join("buried");
        std::fs::create_dir_all(&buried).unwrap();
        let file = buried.join("gone.txt");
        std::fs::write(&file, "gone").unwrap();
        Graveyard::write_entry(&file).unwrap();
        let mut app = spyc_with_two_tabs(tmp.path());
        app.state.open_graveyard_view();
        assert!(matches!(app.state.cur().view, View::Graveyard));
        app.set_pane_focus(true);

        let fx = press(&mut app, &[ctrl('a'), key('p')]);
        assert_eq!(
            active_tab(&app),
            0,
            "^a p steps to the previous tab: {fx:?}"
        );
        let fx = press(&mut app, &[ctrl('a'), key('x')]);
        assert!(
            closing_a_tab(&app),
            "^a x asks to close the running tab: {fx:?}"
        );
        assert!(matches!(app.state.cur().view, View::Graveyard));
    });
}

/// A chord typed between the two `d`s of the graveyard's `dd` disarms it, as
/// any other key does, so a later `d` arms again rather than purging.
#[test]
fn a_chord_disarms_a_half_typed_graveyard_dd() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let buried = tmp.path().join("buried");
        std::fs::create_dir_all(&buried).unwrap();
        let file = buried.join("kept.txt");
        std::fs::write(&file, "kept").unwrap();
        Graveyard::write_entry(&file).unwrap();
        let mut app = spyc_with_two_tabs(tmp.path());
        app.state.open_graveyard_view();
        app.set_pane_focus(false);

        press(&mut app, &[key('d')]);
        assert!(app.view.graveyard_pending_d, "d arms dd");
        press(&mut app, &[ctrl('a')]);
        assert!(!app.view.graveyard_pending_d, "^a disarms it");
    });
}
