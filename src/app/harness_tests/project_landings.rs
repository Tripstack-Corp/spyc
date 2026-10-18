//! #99 step 5: work started in one project lands in that project, whichever
//! project the user is looking at when it finishes.
//!
//! Two projects in one `App`, `x` (where the work starts) and `y` (where the
//! user has gone by the time it lands). The work runs on the real worker
//! (`spawn_file_op`), and the test switches projects before draining it, which
//! is the order a background landing takes once switching exists.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;

struct Two {
    app: App,
    x: state::ProjectId,
    y: state::ProjectId,
    x_home: PathBuf,
    y_home: PathBuf,
}

/// `x` is active and lists `xproj/`, which holds `one.txt` and an empty
/// `dest/`; `y` lists `yproj/`, which holds `two.txt`.
fn with_two(test: impl FnOnce(&mut Two)) {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let (state_dir, x_home, y_home) = (base.join("state"), base.join("xproj"), base.join("yproj"));
    for d in [&state_dir, &x_home.join("dest"), &y_home] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(x_home.join("one.txt"), "one\n").unwrap();
    std::fs::write(y_home.join("two.txt"), "two\n").unwrap();
    crate::state::with_state_root(&state_dir, || {
        let mut app = App::test_app(x_home.clone());
        let x = app.state.active;
        app.ready_test_project(x, &x_home);
        let y = app.add_test_project(&y_home);
        test(&mut Two {
            app,
            x,
            y,
            x_home,
            y_home,
        });
    });
}

/// Hand every file op among `effects` to the real worker.
fn spawn(app: &App, effects: Vec<Effect>) {
    assert!(!effects.is_empty(), "the handler produced the op");
    for effect in effects {
        let Effect::FileOp(op) = effect else {
            panic!("expected a file op, got {effect:?}");
        };
        app.spawn_file_op(op);
    }
}

/// Wait for the worker's result, then drain it as the loop does.
fn land(app: &mut App) -> Vec<Effect> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.runtime.file_results.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the file op never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
    let (_, effects) = app.apply_file_outcomes();
    effects
}

fn pick(app: &mut App, path: &Path) {
    app.state.cur_mut().picks.insert(path);
}

/// The control: a copy that lands in the project it started in reads as it
/// always has, with no project named.
#[test]
fn a_copy_that_lands_where_it_started_reads_as_before() {
    with_two(|t| {
        let app = &mut t.app;
        pick(app, &t.x_home.join("one.txt"));
        let dest = t.x_home.join("dest");
        let effects = app.run_selection_to(dest.to_str().unwrap(), false);
        spawn(app, effects);
        land(app);
        assert!(dest.join("one.txt").exists(), "the copy ran");
        assert!(app.state.cur().picks.is_empty(), "its picks are cleared");
        assert_eq!(
            app.flash_text(),
            Some(format!("copied 1 item(s) to {}", dest.display()).as_str())
        );
    });
}

/// Landing in the project the user is in is a plain apply: the flash is
/// theirs, with no project named.
#[test]
fn landing_in_the_active_project_names_no_project() {
    with_two(|t| {
        let app = &mut t.app;
        let active = app.state.active;
        assert_eq!(active, t.x);
        let out = app.land_in(active, |app| {
            app.state.flash_info("done");
            7
        });
        assert_eq!(out, Some(7));
        assert_eq!(app.flash_text(), Some("done"));
    });
}

/// A watcher-driven read of X's column, landing after the user moved to Y.
/// The read's staleness tag is a side and a generation, and Y's column can
/// carry the same pair: both start at the same generation.
#[test]
fn a_listing_refresh_from_x_never_replaces_ys() {
    with_two(|t| {
        let app = &mut t.app;
        let generation = app.state.col(state::Side::Left).list_generation;
        app.spawn_listing_refresh();
        app.make_active(t.y);
        app.state.col_mut(state::Side::Left).list_generation = generation;
        land(app);
        assert_eq!(
            app.state.col(state::Side::Left).listing.dir,
            t.y_home,
            "X's listing replaced Y's column"
        );
        assert!(
            !app.runtime.listing_refresh_inflight,
            "the read is no longer in flight"
        );
    });
}

/// A copy started in X finishes while the user is in Y: X's picks clear, Y's
/// stay, and the flash says which project it was.
#[test]
fn a_copy_finished_after_leaving_x_clears_xs_picks_and_names_x() {
    with_two(|t| {
        let app = &mut t.app;
        let y_file = t.y_home.join("two.txt");
        app.as_project(t.y, |app| pick(app, &y_file));
        pick(app, &t.x_home.join("one.txt"));
        let dest = t.x_home.join("dest");
        let effects = app.run_selection_to(dest.to_str().unwrap(), false);
        spawn(app, effects);
        app.make_active(t.y);
        land(app);
        assert!(dest.join("one.txt").exists(), "the copy ran");
        assert!(!app.state.cur().picks.is_empty(), "Y's picks survive");
        assert!(
            app.as_project(t.x, |app| app.state.cur().picks.is_empty()),
            "X's picks are cleared"
        );
        assert_eq!(
            app.flash_text(),
            Some(format!("xproj: copied 1 item(s) to {}", dest.display()).as_str())
        );
    });
}

/// A pipe to X's agent finishes while the user is in Y. `PaneTarget::Active`
/// names no tab, so delivering it would type into Y's agent; it is dropped,
/// and the flash says so.
#[test]
fn a_pipe_kicked_in_x_never_types_into_ys_agent() {
    with_two(|t| {
        let app = &mut t.app;
        app.open_tab_in_project(t.x, &t.x_home);
        app.open_tab_in_project(t.y, &t.y_home);
        pick(app, &t.x_home.join("one.txt"));
        let effects = app.pipe_content_to_pane(false);
        spawn(app, effects);
        app.make_active(t.y);
        let effects = land(app);
        assert!(
            effects.is_empty(),
            "nothing is sent to Y's agent: {effects:?}"
        );
        assert!(
            matches!(app.state.mode, Mode::Normal),
            "no prompt opens in Y"
        );
        assert!(
            app.flash_text().is_some_and(|f| f.starts_with("xproj: ")),
            "the flash names X: {:?}",
            app.flash_text()
        );
    });
}
