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

// ── Archive mounts (step 5b) ─────────────────────────────────────────────────
//
// The mount worker runs inline (`run_archive_op`) and its outcome is filed for
// the drain (`file_archive_outcome`), as `run_effects`' worker does. A remount
// the drain kicks itself may instead run on the real worker, so the tests wait
// for the slot (`settle_archive`) rather than assuming either.

use crate::app::archive_ops::{ArchiveOutcome, MaterializeThen, run_archive_op};
use crate::app::file_ops::PagerDest;

fn build_zip(path: &Path) {
    use std::io::Write as _;
    let file = std::fs::File::create(path).unwrap();
    let mut w = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
    for (name, body) in [("README.md", "# pkg\n"), ("src/main.rs", "fn main() {}\n")] {
        w.start_file(name, opts).unwrap();
        w.write_all(body.as_bytes()).unwrap();
    }
    w.finish().unwrap();
}

/// Run every archive op among `effects` now, filing each outcome.
fn run_archive_ops(app: &App, effects: Vec<Effect>) {
    for effect in effects {
        if let Effect::Archive(op) = effect {
            app.file_archive_outcome(run_archive_op(op));
        }
    }
}

/// Drain landed archive outcomes as the loop does: run the archive ops the
/// drain hands back (in the project in view, as `run_effects` would), wait
/// for anything still on a worker, and drain again until nothing is pending.
fn settle_archive(app: &mut App) -> Vec<Effect> {
    let mut rest = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (_, effects) = app.apply_archive_outcomes();
        let (ops, others): (Vec<Effect>, Vec<Effect>) = effects
            .into_iter()
            .partition(|e| matches!(e, Effect::Archive(_)));
        rest.extend(others);
        run_archive_ops(app, ops);
        if app.runtime.archive_results.lock().unwrap().is_empty() {
            // A worker the drain started itself may still be running.
            std::thread::sleep(Duration::from_millis(50));
            if app.runtime.archive_results.lock().unwrap().is_empty() {
                return rest;
            }
        }
        assert!(Instant::now() < deadline, "the archive work never settled");
    }
}

/// Mount `zip` from the project in view, leaving its outcome filed.
fn kick_mount(app: &mut App, zip: &Path) {
    let effects = app.request_mount(zip, false);
    run_archive_ops(app, effects);
}

/// The control: an archive entered where it started reads as before.
#[test]
fn an_archive_entered_where_it_started_reads_as_before() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.x_home.join("pkg.zip");
        build_zip(&zip);
        kick_mount(app, &zip);
        settle_archive(app);
        assert_eq!(app.state.cur().listing.dir, zip, "the column entered it");
        assert!(
            !app.flash_text().is_some_and(|f| f.starts_with("reading")),
            "the progress flash cleared: {:?}",
            app.flash_text()
        );
    });
}

/// `Enter` on an archive in X; the mount lands after the user moved to Y.
#[test]
#[ignore = "red: entering an archive moves whichever project's column is active"]
fn an_archive_entered_from_x_lands_in_xs_column() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.x_home.join("pkg.zip");
        build_zip(&zip);
        kick_mount(app, &zip);
        app.make_active(t.y);
        settle_archive(app);
        assert_eq!(
            app.state.cur().listing.dir,
            t.y_home,
            "Y's column stayed put"
        );
        assert!(
            app.state.mounts.get(&zip).is_some(),
            "the archive is mounted"
        );
        assert_eq!(
            app.as_project(t.x, |app| app.state.cur().listing.dir.clone()),
            zip,
            "X's column entered it"
        );
        assert!(
            !app.flash_text().is_some_and(|f| f.starts_with("reading")),
            "X's progress flash cleared once its mount landed: {:?}",
            app.flash_text()
        );
    });
}

/// A write-back in X lands after the user moved to Y. The write re-reads the
/// archive, and that re-read enters the archive's column: X's, not Y's.
#[test]
#[ignore = "red: the re-read after a write lands in whichever project is active"]
fn a_reread_after_a_write_from_x_lands_in_xs_column() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.x_home.join("pkg.zip");
        build_zip(&zip);
        kick_mount(app, &zip);
        settle_archive(app);
        app.state.chdir(&t.x_home).unwrap();
        app.file_archive_outcome(ArchiveOutcome::Written {
            archive: zip.clone(),
            report: crate::archive::write::RepackReport {
                members: 2,
                bytes: 1,
                snapshot: None,
            },
        });
        app.make_active(t.y);
        settle_archive(app);
        assert_eq!(
            app.state.cur().listing.dir,
            t.y_home,
            "Y's column stayed put"
        );
        assert_eq!(
            app.as_project(t.x, |app| app.state.cur().listing.dir.clone()),
            zip,
            "the re-read entered X's column"
        );
        assert!(
            app.flash_text()
                .is_some_and(|f| f.starts_with("xproj: wrote")),
            "the write names X: {:?}",
            app.flash_text()
        );
    });
}

/// A mount big enough to ask first, landing after the user moved to Y. The
/// question is about X's archive, so it never opens over Y.
#[test]
#[ignore = "red: a mount's size question opens in whichever project is active"]
fn a_mount_that_asks_first_from_x_never_prompts_in_y() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.x_home.join("pkg.zip");
        build_zip(&zip);
        app.file_archive_outcome(ArchiveOutcome::NeedsConfirm {
            path: zip,
            question: "pkg.zip unpacks to 9 GB. Read it?".to_string(),
            then: None,
        });
        app.make_active(t.y);
        settle_archive(app);
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

/// A member opened from X lands after the user moved to Y: it opens in X's
/// pager, not over Y.
#[test]
#[ignore = "red: an extracted member opens in whichever project's pager is active"]
fn a_member_opened_from_x_opens_in_xs_pager() {
    with_two(|t| {
        let app = &mut t.app;
        app.file_archive_outcome(ArchiveOutcome::Materialized {
            real: t.x_home.join("one.txt"),
            then: MaterializeThen::OpenPager(PagerDest::Overlay { scroll: None }),
        });
        app.make_active(t.y);
        settle_archive(app);
        assert!(app.view.project().pager.is_none(), "no pager opens over Y");
        assert!(
            app.as_project(t.x, |app| app.view.project().pager.is_some()),
            "X's pager holds the member"
        );
    });
}

/// A member extracted for editing in X lands after the user moved to Y. An
/// editor would take the terminal away from Y, so the edit is dropped and the
/// flash says so.
#[test]
#[ignore = "red: an extracted member's editor starts over whichever project is active"]
fn a_member_edit_from_x_never_takes_the_terminal_from_y() {
    with_two(|t| {
        let app = &mut t.app;
        app.file_archive_outcome(ArchiveOutcome::Materialized {
            real: t.x_home.join("one.txt"),
            then: MaterializeThen::Edit { in_pane: false },
        });
        app.make_active(t.y);
        let effects = settle_archive(app);
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::ForegroundExec { .. })),
            "no editor takes the terminal: {effects:?}"
        );
        assert!(
            app.flash_text().is_some_and(|f| f.starts_with("xproj: ")),
            "the flash names X: {:?}",
            app.flash_text()
        );
    });
}

/// The mount table is global: an archive Y's column stands in can't be
/// unmounted from X, or Y's column is left inside an archive that no longer
/// exists.
#[test]
#[ignore = "red: unmounting checks only the active project's columns"]
fn an_archive_ys_column_stands_in_is_never_unmounted_from_x() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.y_home.join("pkg.zip");
        build_zip(&zip);
        app.make_active(t.y);
        kick_mount(app, &zip);
        settle_archive(app);
        assert_eq!(app.state.cur().listing.dir, zip, "Y's column is inside it");
        app.make_active(t.x);
        let effects = app.unmount_archive(&zip);
        assert!(
            app.state.mounts.get(&zip).is_some(),
            "the archive stays mounted"
        );
        assert!(effects.is_empty(), "no staging is cleaned: {effects:?}");
        assert!(
            app.flash_text().is_some_and(|f| f.contains("yproj")),
            "the refusal names the project standing in it: {:?}",
            app.flash_text()
        );
    });
}
