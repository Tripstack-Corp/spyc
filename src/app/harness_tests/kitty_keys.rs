//! The kitty keyboard protocol, end to end through a real pane (#1176): a
//! program that pushes the flags gets Shift+Enter as `CSI 13;2u`, where the
//! legacy encoding spyc sends everyone else would give it a newline.

use super::*;
use crate::app::effect::{PaneInput, PaneTarget};
use std::time::{Duration, Instant};

fn contents(app: &App) -> String {
    app.runtime.project().pane_tabs.as_ref().unwrap().tabs()[0]
        .pane
        .with_screen(crate::pane::engine::TerminalScreen::contents)
}

fn wait_for(app: &mut App, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        app.drain_pane_output();
        if contents(app).contains(needle) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "never saw {needle:?}: {:?}",
            contents(app)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn press(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    app.execute_pane_input(
        PaneTarget::Active,
        PaneInput::Key(KeyEvent::new(code, mods)),
        None,
        None,
    );
}

#[test]
fn a_program_that_pushes_the_flags_gets_kitty_keys() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let script = dir.join("kitty.sh");
        // `exec` keeps the pushing process the pane's foreground group.
        std::fs::write(&script, "printf '\\033[>1uready\\n'\nexec cat -v\n").unwrap();
        let mut app = App::test_app(dir.clone());
        assert!(app.open_pane_tab_in(&format!("sh {}", script.display()), &dir));
        wait_for(&mut app, "ready");
        press(&mut app, KeyCode::Enter, KeyModifiers::SHIFT);
        // The terminal's echo shows the escape as `^[[13;2u`.
        wait_for(&mut app, "[13;2u");
    });
}
