//! Harness tests for archive mounts: `Enter` on a container, browsing what it
//! holds, and climbing back out.
//!
//! The mount itself is driven by running the worker inline and applying its
//! outcome, which is exactly what the event loop does — just without the thread,
//! so the assertions are deterministic.

use super::*;
use crate::app::archive_ops::{ArchiveOp, ArchiveOutcome, run_archive_op};
use crate::keymap::Action;

use std::io::Write as _;
use std::path::{Path, PathBuf};

fn build_zip(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut w = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
    for (name, body) in [
        ("README.md", "# pkg\n"),
        ("src/main.rs", "fn main() {}\n"),
        ("src/deep/mod.rs", "pub fn helper() {}\n"),
    ] {
        w.start_file(name, opts).unwrap();
        w.write_all(body.as_bytes()).unwrap();
    }
    w.finish().unwrap();
}

/// Run the mount the way the loop would — worker first, then the drain that
/// applies the outcome — and hand back the effects the drain produced.
fn mount_inline(app: &mut App, archive: &Path, staging: &Path) -> Vec<Effect> {
    let outcome = run_archive_op(ArchiveOp::Mount {
        path: archive.to_path_buf(),
        staging_root: staging.to_path_buf(),
        limits: app.state.config.archive.limits(),
        max_entries: 1000,
        confirmed: false,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    });
    assert!(
        matches!(outcome, ArchiveOutcome::Mounted { .. }),
        "fixture should mount: {outcome:?}"
    );
    app.runtime.archive_results.lock().unwrap().push(outcome);
    let (_, fx) = app.apply_archive_outcomes();
    fx
}

/// Puts the process cwd back on drop.
///
/// `change_dir` moves the *process* cwd, and a test that points it at a tempdir
/// leaves it dangling the moment that tempdir is cleaned up — after which every
/// later test in this binary that needs a working directory fails with
/// "Could not obtain the current working directory". Restoring to the crate root
/// is unconditional because it is the one path guaranteed to still exist.
struct CwdGuard;

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(env!("CARGO_MANIFEST_DIR"));
    }
}

/// Apply the effects a handler returned, for the two kinds these tests produce.
/// The real executor spawns threads; running them inline keeps the assertions
/// deterministic.
fn apply_effects(app: &mut App, effects: Vec<Effect>) {
    for effect in effects {
        match effect {
            Effect::ChangeDir {
                path,
                focus,
                on_ok,
                err_prefix,
            } => app
                .state
                .change_dir(&path, focus.as_deref(), on_ok.as_deref(), err_prefix),
            Effect::Archive(op) => {
                let outcome = run_archive_op(op);
                app.runtime.archive_results.lock().unwrap().push(outcome);
                app.apply_archive_outcomes();
            }
            _ => {}
        }
    }
}

fn row_names(app: &App) -> Vec<String> {
    app.state
        .cur()
        .rows
        .iter()
        .map(|r| r.display.clone())
        .collect()
}

/// The headline: entering an archive leaves the column browsing its members,
/// addressed under the archive's own path.
#[test]
fn mounting_an_archive_lists_its_members() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);

        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        assert_eq!(app.state.cur().listing.dir, archive);
        assert_eq!(row_names(&app), ["src/", "README.md"]);
        assert!(app.state.mounts.contains(&archive));
    });
}

/// A mount is virtual: the column is browsing a location that is not a directory
/// on disk, which is why nothing tries to `chdir` into it and why the orphaned-
/// column heal has to know about mounts.
#[test]
fn a_mounted_column_sits_somewhere_that_is_not_a_directory() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);

        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let here = app.state.cur().listing.dir.clone();
        assert!(here.is_file(), "the mount root is the archive file itself");
        assert!(!here.is_dir(), "there is no directory to chdir into");
        assert!(!app.state.cur().rows.is_empty(), "and yet it lists members");
    });
}

#[test]
fn descending_inside_a_mount_lists_the_subdirectory() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Cursor is on `src/` (dirs sort first); Enter descends.
        app.apply(&Action::EnterOrDisplay).unwrap();
        assert_eq!(app.state.cur().listing.dir, archive.join("src"));
        assert_eq!(row_names(&app), ["deep/", "main.rs"]);
    });
}

/// Climbing out of a mount root lands where the archive lives, with the cursor
/// on it — the same thing climbing out of a directory does, and the reason the
/// mount root is the archive's own path.
#[test]
fn climbing_out_of_a_mount_returns_to_the_archive() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let fx = app.apply(&Action::Climb).unwrap();
        apply_effects(&mut app, fx);

        assert_eq!(app.state.cur().listing.dir, dir);
        let cursor_path = app
            .state
            .cur()
            .rows
            .get(app.state.cur().cursor.index)
            .map(|r| r.path.clone());
        assert_eq!(
            cursor_path,
            Some(archive),
            "the cursor lands back on the archive"
        );
    });
}

/// A refresh must not eject the column: a mount path is *supposed* to fail
/// `is_dir`, which is exactly the shape the orphaned-column heal looks for.
#[test]
fn a_refresh_inside_a_mount_keeps_the_column_there() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        app.state.refresh_listing();

        assert_eq!(
            app.state.cur().listing.dir,
            archive,
            "the heal must not treat a mount as an orphaned directory"
        );
        assert_eq!(row_names(&app), ["src/", "README.md"]);
    });
}

/// The cursor survives a refresh, so a watcher tick can't move it under the user.
#[test]
fn a_refresh_inside_a_mount_keeps_the_cursor() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        app.apply(&Action::Down(1)).unwrap();
        let before = app.state.cur().cursor.index;
        app.state.refresh_listing();
        assert_eq!(app.state.cur().cursor.index, before);
    });
}

/// Git markers inside an archive would be nonsense — a member has no history and
/// discovery would climb out into whatever repository holds the archive.
#[test]
fn a_mount_carries_no_git_state() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        app.state
            .left
            .git
            .set(Some("main".to_string()), std::collections::HashMap::new());

        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        assert_eq!(app.state.cur().git.info, None);
        assert!(app.state.cur().git.files.is_empty());
    });
}

/// The refusal gate: an action with no meaning inside an archive is stopped
/// before dispatch, with a message rather than a filesystem error.
#[test]
fn an_unsupported_action_is_refused_inside_a_mount() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let fx = app.apply(&Action::MakeDirPrompt).unwrap();
        assert!(fx.is_empty(), "nothing is attempted");
        let flash = app.flash_text().unwrap_or_default();
        assert!(flash.contains("archive:"), "{flash}");
    });
}

/// The gate is scoped to where the rows aren't real files.
#[test]
fn the_gate_does_not_leak_outside_a_mount() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        std::fs::create_dir(dir.join("sub")).unwrap();
        let mut app = App::test_app(dir);
        app.state.refresh_listing();

        app.apply(&Action::MakeDirPrompt).unwrap();
        assert!(
            matches!(app.state.mode, Mode::Prompting(_)),
            "outside an archive the prompt still opens"
        );
    });
}

/// Reading a member goes through the mount: the row's path doesn't exist yet, so
/// the open has to extract first.
#[test]
fn opening_a_member_extracts_it_then_pages_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        let staging = tmp.path().join("staging");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &staging);

        // Move to `README.md` (after the `src/` directory row) and open it.
        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Archive(ArchiveOp::Materialize { .. }))),
            "an unextracted member is materialized first: {fx:?}"
        );
        assert!(app.view.pager.is_none(), "nothing is paged synchronously");

        // Run the extraction and its follow-up the way the loop does.
        for effect in fx {
            let Effect::Archive(op) = effect else {
                continue;
            };
            let outcome = run_archive_op(op);
            app.runtime.archive_results.lock().unwrap().push(outcome);
        }
        app.apply_archive_outcomes();

        let pager = app.view.pager.as_ref().expect("the member is paged");
        assert_eq!(
            pager.source_path.as_deref(),
            Some(staging.join("README.md").as_path()),
            "the pager reads the extracted copy"
        );
    });
}

/// A second read costs nothing: the bytes are already staged, so no worker round
/// trip and the pager opens straight away.
#[test]
fn re_opening_a_member_skips_the_worker() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        let staging = tmp.path().join("staging");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &staging);

        // Pre-stage the member, as a previous read would have.
        let entry = app
            .state
            .mounts
            .get(&archive)
            .unwrap()
            .index
            .get("README.md")
            .unwrap()
            .clone();
        crate::archive::read::materialize(&archive, &entry, &staging).unwrap();

        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        assert!(fx.is_empty(), "no worker round trip: {fx:?}");
        assert!(app.view.pager.is_some(), "the pager opens immediately");
    });
}

/// `Enter` on something that merely *looks* like an archive must still open the
/// file — the name filter is a pre-filter, not a verdict.
#[test]
fn a_file_that_only_looks_like_an_archive_still_opens_as_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let fake = dir.join("notes.zip");
        std::fs::write(&fake, b"just text, not a container\n").unwrap();
        let mut app = App::test_app(dir);
        app.state.refresh_listing();

        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        // The mount is attempted (the name matched), and comes back as a file.
        for effect in fx {
            let Effect::Archive(op) = effect else {
                continue;
            };
            let outcome = run_archive_op(op);
            assert!(
                matches!(outcome, ArchiveOutcome::NotAnArchive { .. }),
                "{outcome:?}"
            );
            app.runtime.archive_results.lock().unwrap().push(outcome);
        }
        app.apply_archive_outcomes();

        assert!(app.state.mounts.is_empty(), "nothing was mounted");
        let pager = app.view.pager.as_ref().expect("it opens as a file");
        assert_eq!(pager.source_path.as_deref(), Some(fake.as_path()));
    });
}

/// With `[archive] enable = false`, `Enter` pages the archive's bytes as before.
#[test]
fn disabling_the_feature_pages_the_archive_instead() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        app.state.config.archive.enable = false;
        app.state.refresh_listing();

        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::Archive(_))),
            "no mount is attempted: {fx:?}"
        );
        assert!(app.view.pager.is_some(), "the archive is paged as bytes");
        assert!(app.state.mounts.is_empty());
    });
}

/// Unmounting while a column is inside would strand it on a path with nothing
/// behind it.
#[test]
fn unmounting_is_refused_while_a_column_is_inside() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let fx = app.unmount_archive(&archive);
        assert!(fx.is_empty());
        assert!(app.state.mounts.contains(&archive), "still mounted");
        let flash = app.flash_text().unwrap_or_default();
        assert!(flash.contains("climb out"), "{flash}");
    });
}

/// `:archive unmount` climbs out first, so the same command works from inside.
#[test]
fn the_unmount_command_climbs_out_and_drops_the_mount() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let fx = app.cmd_archive("unmount");
        apply_effects(&mut app, fx);

        assert!(!app.state.mounts.contains(&archive), "the mount is gone");
        assert_eq!(
            app.state.cur().listing.dir,
            dir,
            "and the column is back outside"
        );
    });
}

/// The status suffix names the container, so browsing one never looks like an
/// ordinary directory that happens to sit under a file.
#[test]
fn the_status_suffix_names_the_container() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 24)).unwrap();
        terminal.draw(|f| app.render(f)).unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(rendered.contains("zip"), "the suffix carries the format");
    });
}

/// A mount is dropped when nothing is standing in it and something has to give,
/// but the staging bytes are only removed through the cleanup effect — never
/// silently by the registry.
#[test]
fn eviction_hands_back_the_staging_tree_to_clean() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut mounts = crate::archive::Mounts::default();
        let mut roots: Vec<PathBuf> = Vec::new();
        for i in 0..=crate::archive::mount::MAX_MOUNTS {
            let archive = tmp.path().join(format!("a{i}.zip"));
            build_zip(&archive);
            let staging = tmp.path().join(format!("staging{i}"));
            std::fs::create_dir_all(&staging).unwrap();
            let indexed = crate::archive::read::index_seekable(
                &archive,
                crate::archive::ArchiveFormat::Zip,
                100,
            )
            .unwrap();
            roots.extend(mounts.insert(
                crate::archive::ArchiveMount {
                    index: indexed.index,
                    journal: crate::archive::Journal::default(),
                    staged: crate::archive::journal::StagedStats::new(),
                    capability: crate::archive::Capability::ReadWrite,
                    warnings: Vec::new(),
                    staging_root: staging,
                    last_used: 0,
                },
                &[],
            ));
        }
        assert_eq!(roots.len(), 1, "exactly one mount was evicted");
        assert!(
            roots[0].exists(),
            "its bytes are still there for the cleanup op"
        );
    });
}

// ── reading members out (the effect screen) ──────────────────────────────

/// Run whatever the screen produced, then whatever the drain produced, until the
/// effects settle — the loop's behaviour, minus the threads.
fn settle(app: &mut App, effects: Vec<Effect>) {
    let mut queue = effects;
    for _ in 0..4 {
        if queue.is_empty() {
            return;
        }
        let mut next = Vec::new();
        for effect in queue {
            let Some(effect) = app.screen_archive_effect(effect) else {
                continue;
            };
            match effect {
                Effect::Archive(op) => {
                    let outcome = run_archive_op(op);
                    app.runtime.archive_results.lock().unwrap().push(outcome);
                }
                Effect::Inventory(op) => {
                    let outcome = crate::app::inventory_ops::run_inventory_op(op);
                    app.runtime.inventory_results.lock().unwrap().push(outcome);
                }
                Effect::FileOp(op) => {
                    let outcome = crate::app::file_ops::run_file_op(op);
                    app.runtime.file_results.lock().unwrap().push(outcome);
                }
                Effect::Graveyard(op) => {
                    let outcome = crate::app::graveyard_ops::run_graveyard_op(op);
                    app.runtime.graveyard_results.lock().unwrap().push(outcome);
                }
                _ => {}
            }
        }
        let (_, fx) = app.apply_archive_outcomes();
        next.extend(fx);
        app.apply_graveyard_outcomes();
        app.apply_inventory_outcomes();
        let (_, file_fx) = app.apply_file_outcomes();
        next.extend(file_fx);
        queue = next;
    }
}

/// The issue's headline for this PR: `y` on a member puts its *contents* in the
/// inventory, so `p` outside the archive writes a real file.
#[test]
fn yanking_a_member_captures_its_contents() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Cursor onto `README.md` (after the `src/` row), then yank.
        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::Take).unwrap();
        assert!(!fx.is_empty(), "the yank is attempted, not refused: {fx:?}");
        settle(&mut app, fx);

        let item = app
            .state
            .inventory
            .items()
            .find(|i| i.filename == "README.md")
            .expect("the member is in the inventory");
        assert_eq!(
            app.state.inventory.read_content(&item.id).as_deref(),
            Some(b"# pkg\n".as_slice()),
            "and it carries the member's real bytes"
        );
    });
}

/// The screen has to hold the op back *before* it runs: a yank of an unextracted
/// member must not reach the inventory worker with a path that doesn't exist.
#[test]
fn a_yank_is_held_back_until_the_member_is_extracted() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        app.apply(&Action::Down(1)).unwrap();

        let fx = app.apply(&Action::Take).unwrap();
        let screened: Vec<Effect> = fx
            .into_iter()
            .filter_map(|e| app.screen_archive_effect(e))
            .collect();

        assert!(
            !screened.iter().any(|e| matches!(e, Effect::Inventory(_))),
            "the inventory op must not reach the worker with a path that doesn't \
             exist yet: {screened:?}"
        );
        let carried = screened.iter().find_map(|e| match e {
            Effect::Archive(crate::app::archive_ops::ArchiveOp::MaterializeMany {
                then, ..
            }) => Some(then),
            _ => None,
        });
        assert!(
            matches!(
                carried,
                Some(crate::app::archive_ops::MaterializeThen::Retry(effect))
                    if matches!(**effect, Effect::Inventory(_))
            ),
            "an extraction goes instead, carrying the yank to re-run: {screened:?}"
        );
    });
}

/// Copying a member out lands a real file with the archived contents.
#[test]
fn copying_a_member_out_writes_the_real_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        let dest = tmp.path().join("out");
        std::fs::create_dir(&dest).unwrap();
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let member = archive.join("src/main.rs");
        settle(
            &mut app,
            vec![Effect::FileOp(crate::app::file_ops::FileOp::Copy {
                paths: vec![member],
                dest: dest.clone(),
            })],
        );

        assert_eq!(
            std::fs::read_to_string(dest.join("main.rs")).unwrap(),
            "fn main() {}\n"
        );
    });
}

/// Copying a file into an archive stages it and records the addition, so the
/// next write-back includes it.
#[test]
fn copying_a_file_into_an_archive_then_writing_adds_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        std::fs::write(dir.join("outside.txt"), b"brought in").unwrap();
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        settle(
            &mut app,
            vec![Effect::FileOp(crate::app::file_ops::FileOp::Copy {
                paths: vec![dir.join("outside.txt")],
                dest: archive.join("src"),
            })],
        );
        assert!(app.mount_is_dirty(&archive), "the addition is pending");

        let fx = app.cmd_archive("write");
        settle(&mut app, fx);

        assert!(
            member_names(&archive).contains(&"src/outside.txt".to_string()),
            "{:?}",
            member_names(&archive)
        );
    });
}

/// A second read of the same member skips the extraction entirely.
#[test]
fn an_already_extracted_member_passes_straight_through_the_screen() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        let staging = tmp.path().join("staging");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &staging);

        let entry = app
            .state
            .mounts
            .get(&archive)
            .unwrap()
            .index
            .get("README.md")
            .unwrap()
            .clone();
        crate::archive::read::materialize(&archive, &entry, &staging).unwrap();

        let held = app.screen_archive_effect(Effect::Inventory(
            crate::app::inventory_ops::InventoryOp::Yank {
                paths: vec![archive.join("README.md")],
            },
        ));
        assert!(held.is_some(), "no round trip for bytes already on disk");
    });
}

/// Effects that have nothing to do with archives are untouched by the screen,
/// even with a mount open.
#[test]
fn the_screen_leaves_unrelated_effects_alone() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        std::fs::write(dir.join("real.txt"), b"hi").unwrap();
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let held = app.screen_archive_effect(Effect::Inventory(
            crate::app::inventory_ops::InventoryOp::Yank {
                paths: vec![dir.join("real.txt")],
            },
        ));
        assert!(held.is_some(), "a real file's yank runs as normal");
    });
}

// ── writing back ─────────────────────────────────────────────────────────

/// Read an archive's member names back off disk.
fn member_names(archive: &Path) -> Vec<String> {
    let indexed =
        crate::archive::read::index_seekable(archive, crate::archive::ArchiveFormat::Zip, 1000)
            .unwrap();
    let mut names: Vec<String> = indexed
        .index
        .entries
        .iter()
        .filter(|e| e.locator != crate::archive::Locator::Implied)
        .map(|e| e.inner.clone())
        .collect();
    names.sort();
    names
}

/// The whole delete-then-write story: `R` marks a member, the archive on disk is
/// untouched until `:archive write`, and afterwards the member is gone.
#[test]
fn deleting_a_member_then_writing_removes_it_from_the_archive() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        let before = std::fs::read(&archive).unwrap();

        // Mark `README.md` (the row after `src/`) for removal.
        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::RemovePrompt(None)).unwrap();
        // The remove prompt confirms first; answer it.
        let fx = if fx.is_empty() {
            app.handle_remove_confirm_key(crossterm::event::KeyEvent::new(
                KeyCode::Char('y'),
                KeyModifiers::empty(),
            ))
        } else {
            fx
        };
        settle(&mut app, fx);

        assert_eq!(
            std::fs::read(&archive).unwrap(),
            before,
            "nothing is written until asked"
        );
        assert!(
            !app.state
                .cur()
                .rows
                .iter()
                .any(|r| r.display.contains("README")),
            "but the row is gone from the listing"
        );
        assert!(app.mount_is_dirty(&archive), "and the mount reads as dirty");

        let fx = app.cmd_archive("write");
        settle(&mut app, fx);

        assert_eq!(member_names(&archive), ["src/deep/mod.rs", "src/main.rs"]);
        assert!(!app.mount_is_dirty(&archive), "clean again after the write");
    });
}

/// Discarding throws the pending change away and leaves the archive alone.
#[test]
fn discarding_pending_changes_restores_the_listing() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        let before = std::fs::read(&archive).unwrap();

        if let Some(mount) = app.state.mounts.get_mut(&archive) {
            mount.journal.delete("README.md");
        }
        app.state.refresh_listing();
        assert!(app.mount_is_dirty(&archive));

        let fx = app.cmd_archive("discard");
        settle(&mut app, fx);

        assert!(!app.mount_is_dirty(&archive), "the change is gone");
        assert_eq!(std::fs::read(&archive).unwrap(), before);
        assert!(
            app.state
                .cur()
                .rows
                .iter()
                .any(|r| r.display.contains("README")),
            "and the member is back in the listing"
        );
    });
}

/// Unmounting an archive with unwritten changes asks before dropping them.
#[test]
fn unmounting_a_changed_archive_offers_to_write_it_first() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        if let Some(mount) = app.state.mounts.get_mut(&archive) {
            mount.journal.delete("README.md");
        }

        let fx = app.cmd_archive("unmount");
        assert!(
            fx.is_empty(),
            "nothing happens until the question is answered"
        );
        assert!(
            matches!(app.state.mode, Mode::Prompting(_)),
            "the prompt is up"
        );
        assert!(app.state.mounts.contains(&archive), "still mounted");
    });
}

/// Declining that offer drops the mount and the changes deliberately.
#[test]
fn declining_the_write_unmounts_and_discards() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        if let Some(mount) = app.state.mounts.get_mut(&archive) {
            mount.journal.delete("README.md");
        }
        let before = std::fs::read(&archive).unwrap();

        let fx = app.finish_archive_write_confirm(&archive, false);
        settle(&mut app, fx);

        assert!(!app.state.mounts.contains(&archive), "the mount is gone");
        assert_eq!(
            std::fs::read(&archive).unwrap(),
            before,
            "and so is the change"
        );
        assert_eq!(app.state.cur().listing.dir, dir);
    });
}

/// A read-only archive refuses the write rather than producing a lossy one.
#[test]
fn a_read_only_mount_refuses_to_be_written() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        if let Some(mount) = app.state.mounts.get_mut(&archive) {
            mount.journal.delete("README.md");
            mount.capability =
                crate::archive::Capability::ReadOnly("2 duplicate member name(s)".to_string());
        }
        let before = std::fs::read(&archive).unwrap();

        let fx = app.cmd_archive("write");
        assert!(fx.is_empty(), "no write is attempted");
        let flash = app.flash_text().unwrap_or_default();
        assert!(flash.contains("read-only"), "{flash}");
        assert_eq!(std::fs::read(&archive).unwrap(), before);
    });
}

/// Quitting with pending changes says so on the first tap, where the existing
/// double-tap confirm already gives the user somewhere to stop.
#[test]
fn quitting_with_unwritten_changes_warns_first() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        if let Some(mount) = app.state.mounts.get_mut(&archive) {
            mount.journal.delete("README.md");
        }

        app.request_quit();

        assert!(!app.state.should_quit, "the first tap does not quit");
        let flash = app.flash_text().unwrap_or_default();
        assert!(flash.contains("unwritten changes"), "{flash}");
    });
}

/// Putting an inventory item into a mount stages its bytes and records the
/// addition, so the write-back carries the yanked file's contents in.
#[test]
fn putting_an_inventory_item_into_an_archive_adds_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let source = dir.join("brought.txt");
        std::fs::write(&source, b"from the inventory").unwrap();
        let mut app = App::test_app(dir);

        // Yank a real file first, then mount and put it inside.
        settle(
            &mut app,
            vec![Effect::Inventory(
                crate::app::inventory_ops::InventoryOp::Yank {
                    paths: vec![source],
                },
            )],
        );
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        let ids: Vec<String> = app.state.inventory.items().map(|i| i.id.clone()).collect();

        settle(
            &mut app,
            vec![Effect::Inventory(
                crate::app::inventory_ops::InventoryOp::Put {
                    dest_dir: archive.clone(),
                    ids,
                },
            )],
        );
        let fx = app.cmd_archive("write");
        settle(&mut app, fx);

        assert!(
            member_names(&archive).contains(&"brought.txt".to_string()),
            "{:?}",
            member_names(&archive)
        );
    });
}

/// Renaming a member inside an archive moves no bytes — it's an index edit — and
/// the write-back emits it under the new name.
#[test]
fn renaming_a_member_then_writing_moves_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        let staging = tmp.path().join("staging");
        mount_inline(&mut app, &archive, &staging);

        settle(
            &mut app,
            vec![Effect::FileOp(crate::app::file_ops::FileOp::RenameEach {
                pairs: vec![(archive.join("README.md"), archive.join("READ-ME.md"))],
                is_move: true,
            })],
        );
        assert!(app.mount_is_dirty(&archive), "the rename is pending");
        assert!(
            !staging.exists()
                || std::fs::read_dir(&staging).map_or(0, std::iter::Iterator::count) == 0,
            "and nothing was extracted to do it"
        );

        let fx = app.cmd_archive("write");
        settle(&mut app, fx);

        let names = member_names(&archive);
        assert!(names.contains(&"READ-ME.md".to_string()), "{names:?}");
        assert!(!names.contains(&"README.md".to_string()), "{names:?}");
    });
}

/// Editing a member extracts it first and points the editor at that copy — the
/// row's own path has no bytes behind it.
#[test]
fn editing_a_member_opens_the_extracted_copy() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        let staging = tmp.path().join("staging");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &staging);
        app.apply(&Action::Down(1)).unwrap();

        let fx = app.apply(&Action::EnterOrEdit).unwrap();
        assert!(
            fx.iter().any(|e| matches!(
                e,
                Effect::Archive(crate::app::archive_ops::ArchiveOp::Materialize { .. })
            )),
            "the member is extracted first: {fx:?}"
        );

        // Run the extraction; the follow-up is the editor spawn.
        let mut spawned: Vec<Effect> = Vec::new();
        for effect in fx {
            let Effect::Archive(op) = effect else {
                continue;
            };
            let outcome = run_archive_op(op);
            app.runtime.archive_results.lock().unwrap().push(outcome);
        }
        let (_, follow) = app.apply_archive_outcomes();
        spawned.extend(follow);

        let target = spawned.iter().find_map(|e| match e {
            Effect::ForegroundExec { args, .. } => args.last().cloned(),
            _ => None,
        });
        assert_eq!(
            target.as_deref(),
            Some(staging.join("README.md").to_string_lossy().as_ref()),
            "the editor opens the extracted copy, not the member path"
        );
    });
}

/// An edit spyc never performed is still noticed: the staged file no longer
/// matches what spyc wrote when it extracted it.
#[test]
fn an_external_edit_makes_the_mount_dirty_and_is_written_back() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        let staging = tmp.path().join("staging");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &staging);

        // Extract a member the way a read would, then edit it behind spyc's back.
        let entry = app
            .state
            .mounts
            .get(&archive)
            .unwrap()
            .index
            .get("README.md")
            .unwrap()
            .clone();
        let real = crate::archive::read::materialize(&archive, &entry, &staging).unwrap();
        app.state.mounts.get_mut(&archive).unwrap().staged.insert(
            "README.md".to_string(),
            crate::archive::journal::StagedStat {
                size: std::fs::metadata(&real).unwrap().len(),
                mtime: std::fs::metadata(&real).unwrap().modified().unwrap(),
                is_dir: false,
            },
        );
        assert!(
            !app.mount_is_dirty(&archive),
            "clean right after extraction"
        );

        std::fs::write(&real, b"edited outside spyc").unwrap();
        assert!(
            app.mount_is_dirty(&archive),
            "the changed staged copy is what makes it dirty"
        );

        let fx = app.cmd_archive("write");
        settle(&mut app, fx);

        // Read the member back out of the rewritten archive.
        let after =
            crate::archive::read::index_seekable(&archive, crate::archive::ArchiveFormat::Zip, 100)
                .unwrap();
        let entry = after.index.get("README.md").unwrap();
        let check = tmp.path().join("check");
        let real = crate::archive::read::materialize(&archive, entry, &check).unwrap();
        assert_eq!(std::fs::read(real).unwrap(), b"edited outside spyc");
    });
}

// ── the archive file itself is not a member of itself ─────────────────────

/// The reported bug: leaving an archive and coming back reported
/// "archive: no such member".
///
/// The cursor lands on the archive file, whose path is the live mount's *root* —
/// so asking "is this path in a mount?" said yes and the member branch asked the
/// index for the entry named `""`. A container is not a member of itself.
#[test]
fn leaving_an_archive_and_coming_back_re_enters_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let fx = app.apply(&Action::Climb).unwrap();
        apply_effects(&mut app, fx);
        assert_eq!(app.state.cur().listing.dir, dir, "out of the archive");

        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        settle(&mut app, fx);

        assert_eq!(
            app.state.cur().listing.dir,
            archive,
            "back inside: {:?}",
            app.state.flash.as_ref().map(|f| f.text.clone())
        );
        assert_eq!(row_names(&app), ["src/", "README.md"]);
    });
}

/// Re-entering must not re-read the archive: a fresh mount installs a fresh
/// journal, so a pending delete would vanish — and for a compressed tar it would
/// re-stream the whole thing to learn what it already knew.
#[test]
fn re_entering_keeps_unwritten_changes() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Delete `README.md`, then leave and come back.
        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::RemovePrompt(None)).unwrap();
        let fx = if fx.is_empty() {
            app.handle_remove_confirm_key(crossterm::event::KeyEvent::new(
                KeyCode::Char('y'),
                KeyModifiers::empty(),
            ))
        } else {
            fx
        };
        settle(&mut app, fx);
        assert!(app.mount_is_dirty(&archive), "dirty before leaving");

        let fx = app.apply(&Action::Climb).unwrap();
        apply_effects(&mut app, fx);
        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        settle(&mut app, fx);

        assert!(
            app.mount_is_dirty(&archive),
            "the pending delete survived the round trip"
        );
        assert!(
            !app.state
                .cur()
                .rows
                .iter()
                .any(|r| r.display.contains("README")),
            "and the listing still reflects it"
        );
    });
}

/// A mounted archive is still an ordinary file where it lives: reading it reads
/// the container's own bytes, not a member's.
#[test]
fn yanking_a_mounted_archive_takes_the_container_itself() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Step out so the cursor is on the archive file, and yank it.
        let fx = app.apply(&Action::Climb).unwrap();
        apply_effects(&mut app, fx);
        let fx = app.apply(&Action::Take).unwrap();
        settle(&mut app, fx);

        let held: Vec<String> = app
            .state
            .inventory
            .items()
            .map(|i| i.filename.clone())
            .collect();
        assert_eq!(held, ["pkg.zip"], "the archive itself is what got yanked");
    });
}

/// Deleting the archive drops its mount with it. Left registered, it would claim
/// the path — so a *new* file created there would be browsed through the old
/// archive's index.
#[test]
fn deleting_a_mounted_archive_drops_the_mount() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let fx = app.apply(&Action::Climb).unwrap();
        apply_effects(&mut app, fx);
        let fx = app.apply(&Action::RemovePrompt(None)).unwrap();
        let fx = if fx.is_empty() {
            app.handle_remove_confirm_key(crossterm::event::KeyEvent::new(
                KeyCode::Char('y'),
                KeyModifiers::empty(),
            ))
        } else {
            fx
        };
        settle(&mut app, fx);

        assert!(!archive.exists(), "the archive is gone from disk");
        assert!(
            !app.state.mounts.contains(&archive),
            "and so is its mount: {:?}",
            app.state.flash.as_ref().map(|f| f.text.clone())
        );
    });
}

/// ...unless it holds changes nobody wrote back. Those would go with it and have
/// nowhere to be put, so the delete is refused instead.
#[test]
fn deleting_a_dirty_mounted_archive_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Delete a member, climb out, then try to delete the archive.
        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::RemovePrompt(None)).unwrap();
        let fx = if fx.is_empty() {
            app.handle_remove_confirm_key(crossterm::event::KeyEvent::new(
                KeyCode::Char('y'),
                KeyModifiers::empty(),
            ))
        } else {
            fx
        };
        settle(&mut app, fx);
        let fx = app.apply(&Action::Climb).unwrap();
        apply_effects(&mut app, fx);

        let fx = app.apply(&Action::RemovePrompt(None)).unwrap();
        let fx = if fx.is_empty() {
            app.handle_remove_confirm_key(crossterm::event::KeyEvent::new(
                KeyCode::Char('y'),
                KeyModifiers::empty(),
            ))
        } else {
            fx
        };
        settle(&mut app, fx);

        assert!(archive.exists(), "the archive is still there");
        assert!(app.state.mounts.contains(&archive), "still mounted");
        let flash = app.state.flash.as_ref().map(|f| f.text.clone());
        assert!(
            flash.as_deref().is_some_and(|t| t.contains("unwritten")),
            "and the refusal says why: {flash:?}"
        );
    });
}
