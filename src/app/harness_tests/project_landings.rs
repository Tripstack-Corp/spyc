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
use crate::app::View;

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
    while app.runtime.file_results.is_empty() {
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
        if app.runtime.archive_results.is_empty() {
            // A worker the drain started itself may still be running.
            std::thread::sleep(Duration::from_millis(50));
            if app.runtime.archive_results.is_empty() {
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
fn a_reread_after_a_write_from_x_lands_in_xs_column() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.x_home.join("pkg.zip");
        build_zip(&zip);
        kick_mount(app, &zip);
        settle_archive(app);
        // Back out of the archive without `chdir`, which would move the
        // process cwd into a temp dir this test deletes.
        app.state.cur_mut().listing = crate::fs::listing::Listing::read(&t.x_home).unwrap();
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

fn written(zip: &Path) -> ArchiveOutcome {
    ArchiveOutcome::Written {
        archive: zip.to_path_buf(),
        report: crate::archive::write::RepackReport {
            members: 2,
            bytes: 1,
            snapshot: None,
        },
    }
}

/// Drain archive outcomes until `done`, for at most five seconds: a re-read
/// that a landing starts itself runs on a worker.
fn land_until(app: &mut App, done: impl Fn(&App) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        settle_archive(app);
        if done(app) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// X writes an archive back and closes before the write lands. The write
/// still counts: the journal clears, so nothing reads as unwritten, and the
/// archive is read again from clean staging, as after any write, so a member
/// extracted before it never stands in for what is on disk now. Nothing is
/// entered: Y's column never moves.
#[test]
fn a_write_that_lands_after_its_project_closed_still_counts() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.x_home.join("pkg.zip");
        build_zip(&zip);
        kick_mount(app, &zip);
        settle_archive(app);
        app.state.cur_mut().listing = crate::fs::listing::Listing::read(&t.x_home).unwrap();
        let stale = app
            .state
            .mounts
            .get(&zip)
            .unwrap()
            .staging_root
            .join("README.md");
        // A zip mount stages nothing until a member is read.
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::write(&stale, "stale\n").unwrap();
        app.state
            .mounts
            .get_mut(&zip)
            .unwrap()
            .journal
            .delete("README.md");
        assert_eq!(app.dirty_mounts(), std::slice::from_ref(&zip));
        app.file_archive_outcome(written(&zip));
        app.make_active(t.y);
        app.remove_test_project(t.x);
        assert!(
            land_until(app, |app| app.state.mounts.get(&zip).is_some()
                && !stale.exists()),
            "the archive is never read again from clean staging"
        );
        assert!(
            app.dirty_mounts().is_empty(),
            "the written changes read as pending"
        );
        assert_eq!(app.state.cur().listing.dir, t.y_home, "Y's column moved");
        assert!(
            app.flash_text()
                .is_some_and(|f| f.starts_with("xproj: wrote pkg.zip")),
            "{:?}",
            app.flash_text()
        );
    });
}

/// A failed write that lands after its project closed still says so: the
/// user's changes are not on disk.
#[test]
fn a_failed_write_that_lands_after_its_project_closed_says_so() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.x_home.join("pkg.zip");
        app.file_archive_outcome(ArchiveOutcome::WriteFailed {
            archive: zip,
            error: "disk full".into(),
        });
        app.make_active(t.y);
        app.remove_test_project(t.x);
        settle_archive(app);
        assert!(
            app.flash_text().is_some_and(|f| f.contains("NOT written")),
            "{:?}",
            app.flash_text()
        );
    });
}

/// X starts reading an archive and closes before the read lands. The mount
/// table is the process's, so the archive is recorded there, its staging
/// with it for clean-up, but it is entered nowhere: Y's column never moves.
#[test]
fn a_mount_that_lands_after_its_project_closed_is_recorded_not_entered() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.x_home.join("pkg.zip");
        build_zip(&zip);
        kick_mount(app, &zip);
        app.make_active(t.y);
        app.remove_test_project(t.x);
        settle_archive(app);
        assert!(
            app.state.mounts.get(&zip).is_some(),
            "the read was thrown away, and its staging with it"
        );
        assert_eq!(app.state.cur().listing.dir, t.y_home, "Y's column moved");
    });
}

/// A member X extracted lands after X closed. The mount outlives X, so the
/// extracted copy is recorded as spyc's own rather than read as an edit:
/// nothing waits to be written, and the archive can still be moved or
/// deleted. No pager opens over Y.
#[test]
fn a_member_extracted_after_its_project_closed_is_not_an_edit() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.x_home.join("pkg.zip");
        build_zip(&zip);
        kick_mount(app, &zip);
        settle_archive(app);
        app.state.cur_mut().listing = crate::fs::listing::Listing::read(&t.x_home).unwrap();
        let real = app
            .state
            .mounts
            .get(&zip)
            .unwrap()
            .staging_root
            .join("README.md");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "# pkg\n").unwrap();
        assert_eq!(
            app.dirty_mounts(),
            std::slice::from_ref(&zip),
            "an extracted copy reads as an edit until it is recorded"
        );
        app.file_archive_outcome(ArchiveOutcome::Materialized {
            real,
            then: MaterializeThen::OpenPager(PagerDest::Overlay { scroll: None }),
        });
        app.make_active(t.y);
        app.remove_test_project(t.x);
        settle_archive(app);
        assert!(
            app.dirty_mounts().is_empty(),
            "the extracted copy reads as an edit"
        );
        assert!(app.view.project().pager.is_none(), "a pager opened over Y");
    });
}

/// Run the one archive op among `effects` and hand back its outcome unfiled,
/// to land later as the project that started it.
fn run_unfiled(effects: Vec<Effect>) -> ArchiveOutcome {
    let op = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::Archive(op) => Some(op),
            _ => None,
        })
        .expect("the handler produced an archive op");
    run_archive_op(op)
}

/// File `outcome` as project `origin`'s, open or closed.
fn file_as(app: &App, origin: state::ProjectId, outcome: ArchiveOutcome) {
    app.runtime.archive_results.land((origin, outcome));
}

fn starts_a_read(effects: &[Effect]) -> bool {
    effects.iter().any(|e| matches!(e, Effect::Archive(_)))
}

/// Y asks for an archive X is still reading. Y joins X's read rather than
/// reading it again, and once it lands both columns are in the archive, X's
/// in the background; or only Y's, when X closed first.
fn y_joins_xs_read(close_x: bool) {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.x_home.join("pkg.zip");
        build_zip(&zip);
        let xs = app.request_mount(&zip, false);
        app.make_active(t.y);
        let ys = app.request_mount(&zip, false);
        assert!(!starts_a_read(&ys), "Y read it again: {ys:?}");
        if close_x {
            app.remove_test_project(t.x);
        }
        file_as(app, t.x, run_unfiled(xs));
        settle_archive(app);
        assert_eq!(app.state.cur().listing.dir, zip, "Y's column is not in it");
        if !close_x {
            assert_eq!(
                app.as_project(t.x, |app| app.state.cur().listing.dir.clone()),
                zip,
                "X's column is not in it"
            );
        }
        assert!(
            !app.flash_text().is_some_and(|f| f.starts_with("reading")),
            "Y's in-flight message outlived the read: {:?}",
            app.flash_text()
        );
    });
}

#[test]
fn a_read_y_asks_for_while_xs_runs_joins_it() {
    y_joins_xs_read(false);
}

#[test]
fn a_read_y_asks_for_while_a_closed_projects_runs_joins_it() {
    y_joins_xs_read(true);
}

/// What Y holds back for an archive X is reading, a `navigate_to` into it,
/// joins X's read and runs in Y once it lands.
#[test]
fn work_y_holds_back_for_xs_read_runs_when_it_lands() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.x_home.join("pkg.zip");
        build_zip(&zip);
        let xs = app.request_mount(&zip, false);
        app.make_active(t.y);
        let into = Effect::ChangeDir {
            path: zip.join("src"),
            focus: None,
            on_ok: None,
            err_prefix: "cd",
        };
        let ys = app.request_mount_then(&zip, false, Some(Box::new(into)));
        assert!(!starts_a_read(&ys), "Y read it again: {ys:?}");
        file_as(app, t.x, run_unfiled(xs));
        let effects = settle_archive(app);
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::ChangeDir { path, .. } if *path == zip.join("src"))),
            "Y's navigation never ran: {effects:?}"
        );
    });
}

/// Work an agent in Y holds back for an archive X is reading joins X's read.
/// Y is out of view when it lands, and the work would act on the view in front
/// of the user, so it is dropped and said so, named for Y.
#[test]
fn work_held_back_out_of_view_for_a_read_is_dropped_and_said_so() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.x_home.join("pkg.zip");
        build_zip(&zip);
        let xs = app.request_mount(&zip, false);
        let into = Effect::ChangeDir {
            path: zip.join("src"),
            focus: None,
            on_ok: None,
            err_prefix: "cd",
        };
        let ys = app.in_project(t.y, |app| {
            app.request_mount_then(&zip, false, Some(Box::new(into)))
        });
        assert!(!starts_a_read(&ys), "Y read it again: {ys:?}");
        file_as(app, t.x, run_unfiled(xs));
        let effects = settle_archive(app);
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::ChangeDir { .. })),
            "Y's navigation ran in X: {effects:?}"
        );
        assert!(
            app.flash_text()
                .is_some_and(|f| f.starts_with("yproj:") && f.contains("dropped")),
            "{:?}",
            app.flash_text()
        );
    });
}

/// `:archive cancel` stops the read the user started in Y. A re-read that
/// X's write starts, X open or closed, runs where the user can't see it and
/// never takes the cancel from the read in front of them.
fn archive_cancel_stops_ys_read_not_xs(close_x: bool) {
    use std::sync::atomic::Ordering;
    with_two(|t| {
        let app = &mut t.app;
        let (xzip, yzip) = (t.x_home.join("pkg.zip"), t.y_home.join("pkg.zip"));
        build_zip(&xzip);
        build_zip(&yzip);
        app.make_active(t.y);
        let ys = app.request_mount(&yzip, false);
        let Some(Effect::Archive(crate::app::archive_ops::ArchiveOp::Mount { cancel, .. })) =
            ys.first()
        else {
            panic!("expected Y's mount op: {ys:?}");
        };
        let ys_cancel = std::sync::Arc::clone(cancel);
        if close_x {
            app.remove_test_project(t.x);
        }
        file_as(app, t.x, written(&xzip));
        app.apply_archive_outcomes();
        app.cancel_archive_mount();
        let stopped = ys_cancel.load(Ordering::Relaxed);
        settle_archive(app);
        assert!(stopped, "the cancel went to X's re-read, not Y's read");
    });
}

#[test]
fn archive_cancel_stops_the_users_read_not_a_closed_projects() {
    archive_cancel_stops_ys_read_not_xs(true);
}

#[test]
fn archive_cancel_stops_the_users_read_not_xs() {
    archive_cancel_stops_ys_read_not_xs(false);
}

/// Y's "reading…" announces Y's read, still running when X's work lands, so
/// X's landing leaves it, X open or closed. X's own in-flight message going
/// when X's work lands is `an_archive_entered_from_x_lands_in_xs_column`.
#[test]
fn ys_reading_flash_outlives_xs_landings() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.y_home.join("pkg.zip");
        build_zip(&zip);
        app.make_active(t.y);
        let _held = app.request_mount(&zip, false);
        let reading = |app: &App| app.flash_text().is_some_and(|f| f.starts_with("reading"));
        assert!(reading(app), "{:?}", app.flash_text());
        file_as(app, t.x, ArchiveOutcome::Cleaned);
        app.apply_archive_outcomes();
        assert!(
            reading(app),
            "X's landing cleared it: {:?}",
            app.flash_text()
        );
        app.remove_test_project(t.x);
        file_as(app, t.x, ArchiveOutcome::Cleaned);
        app.apply_archive_outcomes();
        assert!(
            reading(app),
            "the closed project's landing cleared it: {:?}",
            app.flash_text()
        );
    });
}

/// What a closed project's write says is named for it, as a landing from a
/// project out of view is: the user is in Y, and Y wrote nothing.
#[test]
fn a_closed_projects_write_is_named_for_it() {
    with_two(|t| {
        let app = &mut t.app;
        let zip = t.x_home.join("pkg.zip");
        build_zip(&zip);
        app.make_active(t.y);
        app.remove_test_project(t.x);
        file_as(app, t.x, written(&zip));
        settle_archive(app);
        assert!(
            app.flash_text()
                .is_some_and(|f| f.starts_with("xproj: wrote pkg.zip")),
            "{:?}",
            app.flash_text()
        );
        file_as(
            app,
            t.x,
            ArchiveOutcome::WriteFailed {
                archive: zip,
                error: "disk full".into(),
            },
        );
        settle_archive(app);
        assert!(
            app.flash_text()
                .is_some_and(|f| f.starts_with("xproj: pkg.zip NOT written")),
            "{:?}",
            app.flash_text()
        );
    });
}

/// Work held back for a member a closed project extracted is dropped, and
/// said so: the yank or copy-out the user asked for never happened.
#[test]
fn work_held_back_for_a_closed_project_says_it_was_dropped() {
    with_two(|t| {
        let app = &mut t.app;
        let real = t.x_home.join("README.md");
        std::fs::write(&real, "# pkg\n").unwrap();
        app.make_active(t.y);
        app.remove_test_project(t.x);
        let yank = Effect::CopyToClipboard {
            text: real.display().to_string(),
            ok: crate::app::effect::ClipMsg::SinglePath,
        };
        file_as(
            app,
            t.x,
            ArchiveOutcome::Materialized {
                real,
                then: MaterializeThen::Retry(Box::new(yank)),
            },
        );
        let effects = settle_archive(app);
        assert!(effects.is_empty(), "the yank ran in Y: {effects:?}");
        let dropped = |app: &App| {
            app.flash_text()
                .is_some_and(|f| f.starts_with("xproj:") && f.contains("dropped"))
        };
        assert!(dropped(app), "{:?}", app.flash_text());
        // The same for an effect held back for a mount.
        app.state.flash = None;
        let zip = t.y_home.join("pkg.zip");
        build_zip(&zip);
        let into = Effect::ChangeDir {
            path: zip.join("src"),
            focus: None,
            on_ok: None,
            err_prefix: "cd",
        };
        let mounted = run_unfiled(app.request_mount_then(&zip, false, Some(Box::new(into))));
        app.state.flash = None;
        file_as(app, t.x, mounted);
        let effects = settle_archive(app);
        assert!(effects.is_empty(), "Y's column moved: {effects:?}");
        assert!(dropped(app), "{:?}", app.flash_text());
    });
}

/// A mount big enough to ask first, landing after the user moved to Y. The
/// question is about X's archive, so it never opens over Y.
#[test]
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

/// The mount table holds `MAX_MOUNTS` archives and evicts the least recently
/// used. One Y's column stands in is never the one evicted, however many X
/// opens after it.
#[test]
fn an_archive_ys_column_stands_in_is_never_evicted_by_xs_mounts() {
    with_two(|t| {
        let app = &mut t.app;
        let ys = t.y_home.join("pkg.zip");
        build_zip(&ys);
        app.make_active(t.y);
        kick_mount(app, &ys);
        settle_archive(app);
        assert_eq!(app.state.cur().listing.dir, ys, "Y's column is inside it");
        app.make_active(t.x);
        for i in 0..crate::archive::mount::MAX_MOUNTS {
            let zip = t.x_home.join(format!("x{i}.zip"));
            build_zip(&zip);
            kick_mount(app, &zip);
            settle_archive(app);
        }
        assert!(
            app.state.mounts.get(&ys).is_some(),
            "the archive Y's column stands in survives X's mounts"
        );
    });
}

/// A member extracted for editing in the pane, landing after the user moved
/// to Y: no editor overlay opens, neither over Y nor unseen in X.
#[test]
fn a_member_edited_in_the_pane_from_x_opens_no_overlay() {
    with_two(|t| {
        let app = &mut t.app;
        let x = t.x;
        app.file_archive_outcome(ArchiveOutcome::Materialized {
            real: t.x_home.join("one.txt"),
            then: MaterializeThen::Edit { in_pane: true },
        });
        app.make_active(t.y);
        settle_archive(app);
        assert!(
            app.runtime.project().top_overlay.is_none(),
            "no editor overlay opens over Y"
        );
        assert!(
            app.as_project(x, |app| app.runtime.project().top_overlay.is_none()),
            "none waits unseen in X"
        );
        assert!(
            app.flash_text().is_some_and(|f| f.starts_with("xproj: ")),
            "the flash names X: {:?}",
            app.flash_text()
        );
    });
}

// ── Images (step 5b) ─────────────────────────────────────────────────────────
//
// The image worker runs inline and its outcome is filed for the drain
// (`file_image_outcome`) while X is in view, as `run_effects`' worker does.

use crate::app::image_ops::{self, ImageOutcome};

/// A 2×3 PNG in `dir`, encoded at test time.
fn write_png(dir: &Path) -> PathBuf {
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(2, 3)
        .write_to(&mut buf, image::ImageFormat::Png)
        .unwrap();
    let path = dir.join("shot.png");
    std::fs::write(&path, buf.into_inner()).unwrap();
    path
}

/// Open `path` from the project in view, leaving the worker's outcome filed.
/// `picker: None` is a terminal with no image protocol.
fn kick_image(app: &App, path: &Path, picker: Option<ratatui_image::picker::Picker>) {
    let Some(Effect::OpenImage(op)) = app.plan_image_open(path) else {
        panic!("{} opens as an image", path.display());
    };
    app.file_image_outcome(image_ops::open_image_file(op, picker));
}

/// The control: an image opened where it started is shown.
#[test]
fn an_image_opened_where_it_started_is_shown() {
    with_two(|t| {
        let app = &mut t.app;
        let png = write_png(&t.x_home);
        kick_image(app, &png, Some(ratatui_image::picker::Picker::halfblocks()));
        app.apply_image_outcomes();
        assert!(app.view.image_view.is_some(), "the image is shown");
    });
}

/// The worker's own tag: work started in X lands in X when it finishes after
/// the user moved to Y.
#[test]
fn an_image_worker_started_in_x_lands_in_x() {
    with_two(|t| {
        let app = &mut t.app;
        app.spawn_image_work(|_| ImageOutcome::Failed("boom".into()));
        app.make_active(t.y);
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.runtime.image_results.is_empty() {
            assert!(Instant::now() < deadline, "the image worker never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
        app.apply_image_outcomes();
        assert_eq!(app.flash_text(), Some("xproj: boom"));
    });
}

/// An image opened in X, decoded after the user moved to Y. The overlay
/// covers the whole screen and its verbs act on the project in view, so it is
/// not shown over Y, and the flash says so.
#[test]
fn an_image_opened_in_x_is_never_shown_over_y() {
    with_two(|t| {
        let app = &mut t.app;
        let png = write_png(&t.x_home);
        kick_image(app, &png, Some(ratatui_image::picker::Picker::halfblocks()));
        app.make_active(t.y);
        app.apply_image_outcomes();
        assert!(app.view.image_view.is_none(), "X's image covers Y");
        assert!(
            app.flash_text().is_some_and(|f| f.starts_with("xproj: ")),
            "the flash names X: {:?}",
            app.flash_text()
        );
    });
}

/// An image X failed to open, while the user, now in Y, was last reading a
/// pager in X. The failure reaches the user, named for X, rather than X's
/// pager, which they can't see.
#[test]
fn an_image_failure_from_x_reaches_the_user_named_for_x() {
    with_two(|t| {
        let app = &mut t.app;
        let png = write_png(&t.x_home);
        app.open_help();
        kick_image(app, &png, None);
        app.make_active(t.y);
        app.apply_image_outcomes();
        assert!(
            app.flash_text()
                .is_some_and(|f| f.starts_with("xproj: ") && f.contains("no image protocol")),
            "the failure reaches the user, named for X: {:?}",
            app.flash_text()
        );
    });
}

/// An image pasted into X's agent, captured after the user moved to Y. It is
/// kept for X's tab: that tab's next prompt carries it.
#[test]
fn a_paste_captured_in_x_is_kept_for_xs_tab() {
    with_two(|t| {
        let app = &mut t.app;
        let x = t.x;
        let tab = app.open_tab_in_project(x, &t.x_home);
        app.open_tab_in_project(t.y, &t.y_home);
        let epoch = app
            .state
            .active_project()
            .pane
            .image_capture_epochs
            .get(&tab)
            .copied()
            .unwrap_or(0);
        app.file_image_outcome(ImageOutcome::Captured {
            tab_id: tab.clone(),
            epoch,
            encoded: vec![1, 2, 3],
            dims: (1, 1),
        });
        app.make_active(t.y);
        app.apply_image_outcomes();
        let kept = app.as_project(x, |app| {
            app.state
                .active_project()
                .pane
                .pending_images
                .get(&tab)
                .map_or(0, Vec::len)
        });
        assert_eq!(kept, 1, "X's tab keeps the pasted image");
        assert!(
            app.flash_text().is_some_and(|f| f.starts_with("xproj: ")),
            "the flash names X: {:?}",
            app.flash_text()
        );
    });
}

// ── Lua (step 5b) ────────────────────────────────────────────────────────────
//
// A script runs on the real worker. It is started in X, and its requests are
// drained (`handle_lua_done`) once the worker wakes the loop.

/// Give `app` a Lua worker whose wakes arrive on the returned receiver.
fn attach_lua(app: &mut App) -> std::sync::mpsc::Receiver<Message> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.runtime.pane_wake_tx = Some(tx.clone());
    app.runtime.lua = Some(crate::lua::LuaWorker::spawn(move || {
        let _ = tx.send(Message::Wake(Wake::Lua));
    }));
    rx
}

/// Run `<config>/lua/<name>.lua`, holding `body`, from the project in view.
fn kick_lua(app: &mut App, config: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(config.join("lua")).unwrap();
    std::fs::write(config.join("lua").join(format!("{name}.lua")), body).unwrap();
    crate::state::with_config_root(config, || app.apply_lua_binding(name));
}

/// Wait for the script to finish, then drain it as the loop does.
fn finish_lua(app: &mut App, rx: &std::sync::mpsc::Receiver<Message>) -> Vec<Effect> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (landed, effects) = app.handle_lua_done();
        if landed {
            return effects;
        }
        assert!(Instant::now() < deadline, "the script never finished");
        let _ = rx.recv_timeout(Duration::from_millis(50));
    }
}

/// The control: a script's request applies in the project it started in.
#[test]
fn a_lua_script_run_where_it_started_applies() {
    with_two(|t| {
        let app = &mut t.app;
        let rx = attach_lua(app);
        let config = t.x_home.with_file_name("config");
        kick_lua(app, &config, "narrow", "spyc.filter('*.txt')");
        finish_lua(app, &rx);
        assert_eq!(app.state.cur().temp_filter.as_deref(), Some("*.txt"));
    });
}

/// A script started in a project the user switched to applies there.
#[test]
fn a_lua_script_run_after_a_switch_applies_where_it_started() {
    with_two(|t| {
        let app = &mut t.app;
        let rx = attach_lua(app);
        app.make_active(t.y);
        let config = t.y_home.with_file_name("config");
        kick_lua(app, &config, "narrow", "spyc.filter('*.txt')");
        finish_lua(app, &rx);
        assert_eq!(app.state.cur().temp_filter.as_deref(), Some("*.txt"));
    });
}

/// A script started in X, finished after the user moved to Y. What it asks
/// for would act on Y's view, so it is dropped, and the flash says so.
#[test]
fn a_lua_script_run_from_x_never_acts_on_y() {
    with_two(|t| {
        let app = &mut t.app;
        let rx = attach_lua(app);
        let config = t.x_home.with_file_name("config");
        kick_lua(app, &config, "narrow", "spyc.filter('*.txt')");
        app.make_active(t.y);
        let effects = finish_lua(app, &rx);
        assert!(effects.is_empty(), "nothing runs in Y: {effects:?}");
        assert_eq!(
            app.state.cur().temp_filter,
            None,
            "X's script narrowed Y's column"
        );
        assert!(
            app.flash_text().is_some_and(|f| f.starts_with("xproj: ")),
            "the flash names X: {:?}",
            app.flash_text()
        );
    });
}

/// An `init.lua` reload from Y, queued behind a script X started, never
/// claims that script's requests for Y.
#[test]
fn a_reload_queued_behind_xs_script_never_claims_its_requests() {
    with_two(|t| {
        let app = &mut t.app;
        let rx = attach_lua(app);
        let config = t.x_home.with_file_name("config");
        kick_lua(app, &config, "narrow", "spyc.filter('*.txt')");
        app.make_active(t.y);
        std::fs::write(config.join("init.lua"), "").unwrap();
        crate::state::with_config_root(&config, || app.reload_init_lua(false));
        finish_lua(app, &rx);
        assert_eq!(
            app.state.cur().temp_filter,
            None,
            "X's script narrowed Y's column"
        );
    });
}

/// A script started in X fails after the user moved to Y: the error is news,
/// named for X.
#[test]
fn a_lua_error_from_x_is_named_for_x() {
    with_two(|t| {
        let app = &mut t.app;
        let rx = attach_lua(app);
        let config = t.x_home.with_file_name("config");
        kick_lua(app, &config, "broken", "error('boom')");
        app.make_active(t.y);
        finish_lua(app, &rx);
        assert!(
            app.flash_text()
                .is_some_and(|f| f.starts_with("xproj: ") && f.contains("boom")),
            "the error names X: {:?}",
            app.flash_text()
        );
    });
}

// ── Git views (step 5b) ──────────────────────────────────────────────────────

use crate::app::git_view_session::{DiffScope, GitViewKind};

/// Commit `one.txt` in `dir`, then change it.
fn changed_repo(dir: &Path) {
    let run_git = crate::git::test_support::run_git;
    run_git(dir, &["init", "-q", "--initial-branch=main"]);
    run_git(dir, &["add", "one.txt"]);
    run_git(dir, &["commit", "-q", "-m", "v1"]);
    std::fs::write(dir.join("one.txt"), "one\nmore\n").unwrap();
}

/// Drain the git view until it mounts, for at most `limit`.
fn mount_git_view(app: &mut App, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if app.drain_pending_git_view() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A diff opened in X, built after the user moved to Y. It mounts in X's
/// pager when X is next in view, and never in Y's.
#[test]
fn a_git_view_opened_in_x_mounts_in_xs_pager() {
    with_two(|t| {
        let app = &mut t.app;
        changed_repo(&t.x_home);
        app.open_git_view(
            GitViewKind::Diff {
                repo_root: t.x_home.clone(),
                paths: vec!["one.txt".into()],
                scope: DiffScope::HeadToWorktree,
            },
            "git diff one.txt".into(),
        );
        app.make_active(t.y);
        // Long enough for the worker to finish: in Y nothing may mount.
        mount_git_view(app, Duration::from_secs(1));
        assert!(app.view.project().pager.is_none(), "X's diff mounted in Y");
        app.make_active(t.x);
        assert!(
            mount_git_view(app, Duration::from_secs(10)),
            "X's diff mounts once X is in view"
        );
        let pager = app.view.project().pager.as_ref().expect("X's pager");
        assert_eq!(pager.title, "git diff one.txt");
    });
}

fn lists(app: &App, name: &str) -> bool {
    app.state.cur().rows.iter().any(|row| row.display == name)
}

/// An `R` started in X finishes while the user is in Y: X's column re-lists
/// without the file, and the flash says which project it was. The op runs
/// inline, on the thread holding the test's state root, and lands as its
/// worker's would.
#[test]
fn a_delete_finished_after_leaving_x_relists_xs_column_and_names_x() {
    use crate::app::graveyard_ops::{GraveyardOp, run_graveyard_op};
    with_two(|t| {
        let app = &mut t.app;
        let one = t.x_home.join("one.txt");
        assert!(lists(app, "one.txt"));
        app.file_graveyard_outcome(run_graveyard_op(GraveyardOp::Archive {
            paths: vec![one.clone()],
        }));
        assert!(!one.exists(), "the delete ran");
        app.make_active(t.y);
        app.apply_graveyard_outcomes();
        let flash = app.flash_text().unwrap_or_default().to_owned();
        assert!(flash.starts_with("xproj: removed 1 item(s)"), "{flash}");
        app.make_active(t.x);
        assert!(
            !lists(app, "one.txt"),
            "X's column still lists the deleted file"
        );
    });
}

/// A delete whose project closed before it finished still says what happened
/// to its files, named for that project, in the one the user is in.
#[test]
fn a_delete_finished_after_its_project_closed_still_says_so() {
    use crate::app::graveyard_ops::{GraveyardOp, run_graveyard_op};
    with_two(|t| {
        let app = &mut t.app;
        app.file_graveyard_outcome(run_graveyard_op(GraveyardOp::Archive {
            paths: vec![t.x_home.join("one.txt")],
        }));
        app.make_active(t.y);
        app.remove_test_project(t.x);
        app.apply_graveyard_outcomes();
        let flash = app.flash_text().unwrap_or_default().to_owned();
        assert!(flash.starts_with("xproj: removed 1 item(s)"), "{flash}");
        assert_eq!(app.state.cur().listing.dir, t.y_home, "Y's column moved");
    });
}

/// A put into X from the inventory finishes while the user is in Y: X's
/// column re-lists with the file, and the flash says which project it was.
#[test]
fn a_put_finished_after_leaving_x_relists_xs_column_and_names_x() {
    use crate::app::inventory_ops::{InventoryOp, YankSource, run_inventory_op};
    with_two(|t| {
        let app = &mut t.app;
        run_inventory_op(InventoryOp::Yank {
            sources: vec![YankSource::plain(t.y_home.join("two.txt"))],
        });
        let ids: Vec<String> = crate::state::inventory::Inventory::load()
            .items()
            .map(|item| item.id.clone())
            .collect();
        assert_eq!(ids.len(), 1, "the yank ran");
        assert!(!lists(app, "two.txt"));
        app.file_inventory_outcome(run_inventory_op(InventoryOp::Put {
            dest_dir: t.x_home.clone(),
            ids,
        }));
        assert!(t.x_home.join("two.txt").exists(), "the put ran");
        app.make_active(t.y);
        app.apply_inventory_outcomes();
        let flash = app.flash_text().unwrap_or_default().to_owned();
        assert!(flash.starts_with("xproj: put 1 file(s)"), "{flash}");
        app.make_active(t.x);
        assert!(
            lists(app, "two.txt"),
            "X's column doesn't list the file put there"
        );
    });
}

/// The inventory is shared, so a yank from X that lands while Y shows the
/// inventory lists the item in Y too.
#[test]
fn a_yank_from_x_lists_in_ys_inventory_view() {
    use crate::app::inventory_ops::{InventoryOp, YankSource, run_inventory_op};
    with_two(|t| {
        let app = &mut t.app;
        app.file_inventory_outcome(run_inventory_op(InventoryOp::Yank {
            sources: vec![YankSource::plain(t.x_home.join("one.txt"))],
        }));
        app.make_active(t.y);
        app.state.cur_mut().view = View::Inventory;
        app.state.rebuild_rows();
        assert!(
            !app.state
                .cur()
                .rows
                .iter()
                .any(|r| r.display.starts_with("one.txt"))
        );
        app.apply_inventory_outcomes();
        assert!(
            app.state
                .cur()
                .rows
                .iter()
                .any(|r| r.display.starts_with("one.txt")),
            "Y's inventory view doesn't list the yank: {:?}",
            app.state
                .cur()
                .rows
                .iter()
                .map(|r| &r.display)
                .collect::<Vec<_>>()
        );
    });
}

/// The graveyard is shared, so a delete from X that lands while Y shows the
/// graveyard lists the entry in Y too.
#[test]
fn a_delete_from_x_lists_in_ys_graveyard_view() {
    use crate::app::graveyard_ops::{GraveyardOp, run_graveyard_op};
    with_two(|t| {
        let app = &mut t.app;
        app.file_graveyard_outcome(run_graveyard_op(GraveyardOp::Archive {
            paths: vec![t.x_home.join("one.txt")],
        }));
        app.make_active(t.y);
        app.state.cur_mut().view = View::Graveyard;
        app.state.rebuild_rows();
        assert!(
            !app.state
                .cur()
                .rows
                .iter()
                .any(|r| r.display.contains("one.txt"))
        );
        app.apply_graveyard_outcomes();
        assert!(
            app.state
                .cur()
                .rows
                .iter()
                .any(|r| r.display.contains("one.txt")),
            "Y's graveyard view doesn't list the delete: {:?}",
            app.state
                .cur()
                .rows
                .iter()
                .map(|r| &r.display)
                .collect::<Vec<_>>()
        );
    });
}
