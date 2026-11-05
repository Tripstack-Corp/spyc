//! The completeness guard for [`Action::writes_to_pane`], the list a project
//! `.spycrc.toml` may not bind (`BoundAction::is_executing`): a cloned repo's
//! config must not be able to type at an agent. Every action is bound to a key
//! and pressed in an App with a running pane tab and something to send, and
//! one whose effects type at a child must be on the list.

use super::*;
use crate::keymap::user::{BoundAction, KeyChord, NamedKey, UserBinding, UserKeymap};

/// The actions not pressed, each with why. Every other action is.
const SKIPPED: &[(Action, &str)] = &[
    (
        Action::ResumePane,
        "starts a real `claude --resume` in a new tab",
    ),
    (
        Action::EditInPane,
        "starts the `$VISUAL`/`$EDITOR` of the environment running the tests",
    ),
    (
        Action::PaneScrollSave,
        "writes `spyc_pane_*.txt` into the process cwd, the checkout under `cargo test`",
    ),
];

/// Fewer pressed than this means the walk lost actions: it presses every one
/// but [`SKIPPED`].
const MIN_CHECKED: usize = 130;

/// The key every action is bound to in turn: no built-in chord starts with it.
const KEY: KeyCode = KeyCode::F(12);

/// Bind `action` to [`KEY`] as a `.spycrc.toml` `map` does, press it in a
/// fresh App, and return the effects. The App lists `work/` with the cursor on
/// a picked file, holds a file in its inventory, and has a running `cat` tab,
/// with the file list focused.
fn press_bound(action: &Action) -> Vec<Effect> {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    let work = root.join("work");
    std::fs::create_dir(&work).unwrap();
    std::fs::write(work.join("notes.txt"), "notes\n").unwrap();
    std::fs::write(work.join("todo.txt"), "todo\n").unwrap();
    crate::state::with_state_root(&root.join("state"), || {
        crate::state::with_config_root(&root.join("config"), || {
            crate::config::with_home_root(&root.join("home"), || {
                let mut app = App::test_app(work.clone());
                app.state.refresh_listing();
                app.state.toggle_pick_cursor();
                app.state
                    .inventory
                    .yank(&work.join("todo.txt"))
                    .expect("the inventory takes a file");
                assert!(app.open_pane_tab_in("cat", &work), "the pane tab starts");
                app.state.active_project_mut().focus = state::Focus::FileList;
                app.state.active_project_mut().user_keymap =
                    UserKeymap::from_bindings(vec![UserBinding {
                        chord: KeyChord::Named(NamedKey::Fn(12)),
                        action: BoundAction::Plain(action.clone()),
                    }]);
                app.handle_key(KeyEvent::new(KEY, KeyModifiers::NONE))
                    .unwrap_or_else(|e| panic!("{action:?}: {e:#}"))
            })
        })
    })
}

/// An action that types at the pane, or at a `!` capture, is on
/// `writes_to_pane`, so a project `.spycrc.toml` can't bind it. The converse
/// proves the fixture: each listed action does type here, so a press that
/// reached nothing would fail rather than pass.
#[test]
fn every_action_that_types_at_a_pane_is_on_writes_to_pane() {
    for (action, why) in SKIPPED {
        assert!(
            crate::keymap::action::every_action().any(|a| a == *action),
            "skipped {action:?} is not an action"
        );
        assert!(!why.is_empty(), "skipped {action:?} needs its reason");
    }
    let mut unlisted = Vec::new();
    let mut silent = Vec::new();
    let mut checked = 0;
    for action in crate::keymap::action::every_action() {
        if SKIPPED.iter().any(|(skip, _)| *skip == action) {
            continue;
        }
        let effects = press_bound(&action);
        match effects.typed_input() {
            Some(effect) if !action.writes_to_pane() => {
                unlisted.push(format!("{action:?} ({effect:?})"));
            }
            None if action.writes_to_pane() => silent.push(action),
            _ => {}
        }
        checked += 1;
    }
    assert!(
        unlisted.is_empty(),
        "these actions type at a pane but aren't on `Action::writes_to_pane`, \
         so a project `.spycrc.toml` can bind them: {unlisted:#?}"
    );
    assert!(
        silent.is_empty(),
        "these `writes_to_pane` actions sent nothing: the fixture gives them \
         nothing to send, or the key never reached them: {silent:?}"
    );
    assert!(
        checked >= MIN_CHECKED,
        "pressed {checked} actions, fewer than {MIN_CHECKED}"
    );
}
