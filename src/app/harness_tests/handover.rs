//! `^d`'s handover, whose session jar fails to start (D2 of the 3.0
//! review): spyc stays with its tabs, each still linked to its jar, none let
//! go before the start has landed.

use super::jar_start::{
    WAIT, built_jar, jar_app, live_tab, script, short_dir, shows, tabs, type_into,
};

/// The session's jar fails to start: the tab still carries what's typed to
/// its program and back, its link never cut, so nothing let it go to be
/// taken back.
#[test]
fn a_handover_whose_jar_fails_to_start_leaves_each_tab_linked() {
    let dir = short_dir();
    crate::state::with_state_root(dir.path(), || {
        let mut app = jar_app(dir.path(), built_jar(), crate::pane::jar::place::HOLD);
        live_tab(&mut app, dir.path());
        app.state.session_name = Some("ANISE".into());
        let failing = script(dir.path(), "failing-jar", "exit 1\n");
        app.runtime
            .jar_place
            .as_mut()
            .expect("jar_app gives the app a place")
            .binary = failing;
        app.prepare_handover();
        assert!(
            app.runtime.handover_starts.wait(WAIT + WAIT),
            "the session jar's start never landed"
        );
        app.apply_handover_starts();
        assert!(!app.state.should_quit, "spyc left its loop");
        assert!(!app.runtime.released, "the session was let go");
        assert!(!tabs(&mut app).active().adopted(), "the tab was taken back");
        type_into(&mut app, b"still-linked\r");
        shows(&mut app, "still-linked");
    });
}
