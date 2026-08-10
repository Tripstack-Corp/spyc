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
        then: None,
        address: None,
        depth: 0,
        reset_staging: false,
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
                    depth: 0,
                    editing: Vec::new(),
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
                // A mount re-issues the `ChangeDir` that was waiting for it.
                Effect::ChangeDir {
                    path,
                    focus,
                    on_ok,
                    err_prefix,
                } => app
                    .state
                    .change_dir(&path, focus.as_deref(), on_ok.as_deref(), err_prefix),
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
                sources: vec![archive.join("README.md")]
                    .into_iter()
                    .map(crate::app::inventory_ops::YankSource::plain)
                    .collect(),
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
                sources: vec![dir.join("real.txt")]
                    .into_iter()
                    .map(crate::app::inventory_ops::YankSource::plain)
                    .collect(),
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
                    sources: vec![source]
                        .into_iter()
                        .map(crate::app::inventory_ops::YankSource::plain)
                        .collect(),
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

// ── an extracted member's bytes are in staging, never at its mount path ───

/// Same three members, as a `.tar.gz` — a *streamed* mount, which extracts every
/// member as it reads because a compressed tar can't be listed any other way.
fn build_tar_gz(path: &Path) {
    let enc = flate2::write::GzEncoder::new(
        std::fs::File::create(path).unwrap(),
        flate2::Compression::default(),
    );
    let mut b = tar::Builder::new(enc);
    for (name, body) in [
        ("README.md", "# pkg\n"),
        ("src/main.rs", "fn main() {}\n"),
        ("src/deep/mod.rs", "pub fn helper() {}\n"),
    ] {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(1_000_000);
        h.set_entry_type(tar::EntryType::Regular);
        b.append_data(&mut h, name, body.as_bytes()).unwrap();
    }
    b.into_inner().unwrap().finish().unwrap();
}

/// A `.tar.gz` with `count` small members, for the staged-set bound.
fn build_tar_gz_with(path: &Path, count: usize) {
    let enc = flate2::write::GzEncoder::new(
        std::fs::File::create(path).unwrap(),
        flate2::Compression::default(),
    );
    let mut b = tar::Builder::new(enc);
    for i in 0..count {
        let body = format!("member {i}\n");
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(1_000_000);
        h.set_entry_type(tar::EntryType::Regular);
        b.append_data(&mut h, format!("file-{i}.txt"), body.as_bytes())
            .unwrap();
    }
    b.into_inner().unwrap().finish().unwrap();
}

/// The reported bug: `y` inside a `.tar.gz` failed with
/// `demo.tar.gz/src/main.rs: Not a directory`.
///
/// A streamed mount stages every member up front, so nothing needed extracting —
/// and the screen took "nothing to extract" to mean "these paths are real", when
/// a member's bytes are in the staging tree and its mount path is never a file.
#[test]
fn yanking_a_member_of_a_streamed_archive_captures_its_contents() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.tar.gz");
        build_tar_gz(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::Take).unwrap();
        settle(&mut app, fx);

        let item = app
            .state
            .inventory
            .items()
            .find(|i| i.filename == "README.md")
            .unwrap_or_else(|| {
                panic!(
                    "the member should be in the inventory; flash: {:?}",
                    app.state.flash.as_ref().map(|f| f.text.clone())
                )
            });
        assert_eq!(
            app.state.inventory.read_content(&item.id).as_deref(),
            Some(b"# pkg\n".as_slice()),
            "and it carries the member's real bytes"
        );
    });
}

/// The same hole with a seekable archive: reading a member stages it, so the
/// *second* read is the one that finds nothing to extract.
#[test]
fn reading_a_member_twice_still_reads_its_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        app.apply(&Action::Down(1)).unwrap();

        // First read extracts it; second finds it staged.
        let fx = app.apply(&Action::Take).unwrap();
        settle(&mut app, fx);
        app.state.flash = None;
        let fx = app.apply(&Action::Take).unwrap();
        settle(&mut app, fx);

        // The second yank is the one under test, so its *outcome* is what has to
        // be clean — an inventory item from the first read would hide a failure.
        assert!(
            !matches!(
                app.state.flash.as_ref().map(|f| f.kind),
                Some(crate::app::FlashKind::Error)
            ),
            "the second yank failed: {:?}",
            app.state.flash.as_ref().map(|f| f.text.clone())
        );
        let held: Vec<_> = app
            .state
            .inventory
            .items()
            .filter(|i| i.filename == "README.md")
            .map(|i| app.state.inventory.read_content(&i.id))
            .collect();
        assert!(!held.is_empty(), "and the member is in the inventory");
        for content in held {
            assert_eq!(
                content.as_deref(),
                Some(b"# pkg\n".as_slice()),
                "every copy carries the member's bytes"
            );
        }
    });
}

/// A mixed selection is the subtle half: extracting only what's missing means the
/// worker's own result can't be the whole substitution, so the already-staged
/// member would keep its mount path.
#[test]
fn a_mixed_selection_rewrites_the_staged_member_too() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Stage `README.md` by yanking it, then yank it together with a member
        // that is still only an index entry.
        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::Take).unwrap();
        settle(&mut app, fx);

        let readme = archive.join("README.md");
        let main = archive.join("src/main.rs");
        let fx = app.screen_archive_effect(Effect::Inventory(
            crate::app::inventory_ops::InventoryOp::Yank {
                sources: vec![readme.clone(), main.clone()]
                    .into_iter()
                    .map(crate::app::inventory_ops::YankSource::plain)
                    .collect(),
            },
        ));
        let Some(Effect::Archive(ArchiveOp::MaterializeMany { then, .. })) = fx else {
            panic!("expected the extraction to be held back, got {fx:?}");
        };
        let crate::app::archive_ops::MaterializeThen::Retry(effect) = then else {
            panic!("a held-back read retries the original effect");
        };
        let Effect::Inventory(crate::app::inventory_ops::InventoryOp::Yank { sources }) = *effect
        else {
            panic!("shape preserved");
        };
        let reads: Vec<PathBuf> = sources.iter().map(|s| s.read.clone()).collect();
        assert!(
            !reads.contains(&readme),
            "the staged member is already pointed at staging: {reads:?}"
        );
        assert!(
            reads.contains(&main),
            "and the unextracted one still waits for the worker: {reads:?}"
        );
        assert!(
            sources.iter().all(|s| s.record_as.starts_with(&archive)),
            "both are still remembered by their member paths"
        );
    });
}

// ── coming back to a place inside an archive ──────────────────────────────

/// The smallest session that names a cwd — nothing else about it is under test.
fn session_at(cwd: PathBuf) -> crate::state::sessions::Session {
    crate::state::sessions::Session {
        id: 1,
        saved_at: String::new(),
        epoch_secs: 0,
        cwd,
        tabs: Vec::new(),
        active_tab: 0,
        pane_height_pct: 30,
        pane_focused: false,
        name: String::new(),
        project_home: None,
        vsplit: None,
        scope_claims: Vec::new(),
    }
}

/// A mark inside an archive used to be refused, because the mount it pointed into
/// was gone by the time you came back. Now the jump mounts it again.
#[test]
fn a_mark_inside_an_archive_mounts_it_again_on_the_way_back() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Into `src/`, mark it, then leave the archive entirely.
        app.apply(&Action::EnterOrDisplay).unwrap();
        assert_eq!(app.state.cur().listing.dir, archive.join("src"));
        app.apply(&Action::SetMark('a')).unwrap();

        // `:archive unmount` steps the column out itself, then drops the mount.
        let fx = app.cmd_archive("unmount");
        settle(&mut app, fx);
        assert!(!app.state.mounts.contains(&archive), "unmounted");
        assert_eq!(app.state.cur().listing.dir, dir, "and out of it");

        // The jump names a path inside a file. Nothing is mounted, so the effect
        // screen has to mount it before the chdir can mean anything.
        let fx = app.apply(&Action::JumpMark('a')).unwrap();
        settle(&mut app, fx);

        assert!(app.state.mounts.contains(&archive), "re-mounted");
        assert_eq!(
            app.state.cur().listing.dir,
            archive.join("src"),
            "and landed where the mark pointed: {:?}",
            app.state.flash.as_ref().map(|f| f.text.clone())
        );
        assert_eq!(row_names(&app), ["deep/", "main.rs"]);
    });
}

/// Quitting while browsing an archive and restoring the session puts you back
/// inside it. The saved cwd is not a directory and never was, so the restore path
/// can't `chdir` to it — it hands the effect screen a `ChangeDir` instead.
#[test]
fn restoring_a_session_that_ended_inside_an_archive_mounts_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);

        let session = session_at(archive.join("src"));
        let fx = app.restore_session(&session);
        settle(&mut app, fx);

        assert!(
            app.state.mounts.contains(&archive),
            "the archive is mounted"
        );
        assert_eq!(
            app.state.cur().listing.dir,
            archive.join("src"),
            "back where the session ended: {:?}",
            app.state.flash.as_ref().map(|f| f.text.clone())
        );
    });
}

/// A session whose cwd is simply gone still reports that, rather than being
/// mistaken for an archive.
#[test]
fn restoring_a_session_whose_directory_is_gone_still_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = App::test_app(dir.clone());

        let session = session_at(dir.join("no-such-dir"));
        let fx = app.restore_session(&session);

        assert!(fx.is_empty(), "nothing to mount");
        let flash = app.state.flash.as_ref().map(|f| f.text.clone());
        assert!(
            flash.as_deref().is_some_and(|t| t.contains("gone")),
            "{flash:?}"
        );
    });
}

/// Harpoon, the other bookmark: appending inside a mount is allowed now, and the
/// slot jump comes back the same way a mark does.
#[test]
fn a_harpoon_slot_inside_an_archive_comes_back_to_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir.clone());
        // Harpoon is keyed on the column's repo root or PROJECT_HOME; inside a
        // mount there is no repo, so the archive's own project is the key.
        app.state.project_home = Some(dir);
        app.reconcile_harpoon();
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        app.apply(&Action::EnterOrDisplay).unwrap();
        assert_eq!(app.state.cur().listing.dir, archive.join("src"));
        app.apply(&Action::HarpoonAppend).unwrap();
        assert!(
            app.state
                .cur()
                .harpoon
                .as_ref()
                .is_some_and(|h| h.get(1).is_some()),
            "the slot took: {:?}",
            app.state.flash.as_ref().map(|f| f.text.clone())
        );

        let fx = app.cmd_archive("unmount");
        settle(&mut app, fx);
        assert!(!app.state.mounts.contains(&archive), "unmounted");

        let fx = app.apply(&Action::HarpoonJump(1)).unwrap();
        settle(&mut app, fx);
        assert!(app.state.mounts.contains(&archive), "re-mounted");
        assert_eq!(
            app.state.cur().listing.dir,
            archive.join("src"),
            "{:?}",
            app.state.flash.as_ref().map(|f| f.text.clone())
        );
    });
}

// ── an archive inside an archive ──────────────────────────────────────────

/// A zip holding a text file and a second zip.
fn build_nested_zip(path: &Path) {
    let inner_bytes = {
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
        w.start_file("deep/note.txt", opts).unwrap();
        w.write_all(b"from the inner archive\n").unwrap();
        w.start_file("inner.txt", opts).unwrap();
        w.write_all(b"second member\n").unwrap();
        w.finish().unwrap();
        buf.into_inner()
    };
    let file = std::fs::File::create(path).unwrap();
    let mut w = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
    w.start_file("README.md", opts).unwrap();
    w.write_all(b"# outer\n").unwrap();
    // Stored, not deflated: a nested archive compresses badly anyway, and this
    // keeps the fixture's own bytes recognisable.
    w.start_file(
        "bundle.zip",
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .unwrap();
    w.write_all(&inner_bytes).unwrap();
    w.finish().unwrap();
}

/// `Enter` on a member that is itself an archive walks into it. The mount is
/// **addressed** at the member path — what the user sees — while its bytes are
/// read from the staged copy, which is the whole trick.
#[test]
fn entering_an_archive_inside_an_archive_browses_it_at_its_member_path() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("outer.zip");
        build_nested_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Cursor onto `bundle.zip` and enter it.
        let idx = app
            .state
            .cur()
            .rows
            .iter()
            .position(|r| r.display.starts_with("bundle.zip"))
            .expect("the nested archive is listed");
        app.state.cur_mut().cursor.index = idx;
        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        settle(&mut app, fx);

        let inner = archive.join("bundle.zip");
        assert_eq!(
            app.state.cur().listing.dir,
            inner,
            "browsing the inner archive at its member path: {:?}",
            app.state.flash.as_ref().map(|f| f.text.clone())
        );
        assert_eq!(row_names(&app), ["deep/", "inner.txt"]);

        let mount = app.state.mounts.get(&inner).expect("mounted");
        assert_eq!(mount.depth, 1, "recorded as one level deep");
        assert_ne!(
            mount.source(),
            inner,
            "and read from the staged copy, not its address"
        );
        assert!(mount.source().is_file(), "which is a real file");
    });
}

/// Reading a member of the inner archive gets the inner archive's bytes — the
/// read follows `source`, not the address.
#[test]
fn a_member_of_a_nested_archive_reads_its_own_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("outer.zip");
        build_nested_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let idx = app
            .state
            .cur()
            .rows
            .iter()
            .position(|r| r.display.starts_with("bundle.zip"))
            .unwrap();
        app.state.cur_mut().cursor.index = idx;
        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        settle(&mut app, fx);

        // Yank `inner.txt` out of the inner archive.
        let idx = app
            .state
            .cur()
            .rows
            .iter()
            .position(|r| r.display.starts_with("inner.txt"))
            .expect("listed");
        app.state.cur_mut().cursor.index = idx;
        let fx = app.apply(&Action::Take).unwrap();
        settle(&mut app, fx);

        let item = app
            .state
            .inventory
            .items()
            .find(|i| i.filename == "inner.txt")
            .unwrap_or_else(|| {
                panic!(
                    "the inner member should be in the inventory; flash: {:?}",
                    app.state.flash.as_ref().map(|f| f.text.clone())
                )
            });
        assert_eq!(
            app.state.inventory.read_content(&item.id).as_deref(),
            Some(b"second member\n".as_slice())
        );
    });
}

/// The cap is what stops each level from costing another full copy in staging.
#[test]
fn nesting_past_max_depth_is_refused_and_names_the_knob() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("outer.zip");
        build_nested_zip(&archive);
        let mut app = App::test_app(dir);
        app.state.config.archive.max_depth = 0;
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let idx = app
            .state
            .cur()
            .rows
            .iter()
            .position(|r| r.display.starts_with("bundle.zip"))
            .unwrap();
        app.state.cur_mut().cursor.index = idx;
        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        settle(&mut app, fx);

        // `get`, not `contains`: the inner archive's path is inside the outer
        // mount either way, so only "is there a mount rooted here" answers this.
        assert!(
            app.state.mounts.get(&archive.join("bundle.zip")).is_none(),
            "not mounted"
        );
        let flash = app.state.flash.as_ref().map(|f| f.text.clone());
        assert!(
            flash.as_deref().is_some_and(|t| t.contains("max_depth")),
            "the refusal names the knob: {flash:?}"
        );
    });
}

/// Unmounting the outer archive would delete the staged copy the inner one is
/// reading from, so it says which to unmount first.
#[test]
fn unmounting_the_outer_archive_is_refused_while_the_inner_is_mounted() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("outer.zip");
        build_nested_zip(&archive);
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let idx = app
            .state
            .cur()
            .rows
            .iter()
            .position(|r| r.display.starts_with("bundle.zip"))
            .unwrap();
        app.state.cur_mut().cursor.index = idx;
        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        settle(&mut app, fx);

        // Step right out of both, then try to drop the outer.
        let fx = app.apply(&Action::Climb).unwrap();
        apply_effects(&mut app, fx);
        let fx = app.apply(&Action::Climb).unwrap();
        apply_effects(&mut app, fx);
        assert_eq!(app.state.cur().listing.dir, dir, "outside both");

        let fx = app.unmount_archive(&archive);
        settle(&mut app, fx);

        assert!(app.state.mounts.contains(&archive), "still mounted");
        let flash = app.state.flash.as_ref().map(|f| f.text.clone());
        assert!(
            flash.as_deref().is_some_and(|t| t.contains("bundle.zip")),
            "and it says which one is in the way: {flash:?}"
        );
    });
}

/// A repack renumbers a zip's entries, so the index built before it describes a
/// file that no longer exists. The mount has to be re-read, not re-entered —
/// otherwise a later read follows a stale locator to the wrong member's bytes.
#[test]
fn a_write_re_reads_the_archive_rather_than_trusting_the_old_index() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Delete the FIRST stored member, which shifts every later member's
        // position in the central directory.
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
        let fx = app.cmd_archive("write");
        settle(&mut app, fx);

        // Read every surviving member through the index the mount now holds. A
        // repack writes members in index order, which is not the order they were
        // stored in, so a stale locator points at a *different* member's bytes —
        // checking them all is what makes that impossible to pass by luck.
        let mount = app.state.mounts.get(&archive).expect("re-mounted");
        for (inner, want) in [
            ("src/main.rs", "fn main() {}\n"),
            ("src/deep/mod.rs", "pub fn helper() {}\n"),
        ] {
            let entry = mount
                .index
                .get(inner)
                .unwrap_or_else(|| panic!("{inner} is still in the archive"))
                .clone();
            let bytes = crate::archive::read::member_bytes(mount.source(), &entry)
                .unwrap_or_else(|e| panic!("reading {inner} after the write: {e:#}"));
            assert_eq!(
                String::from_utf8_lossy(&bytes),
                want,
                "{inner}: the index has to describe the archive as it is now"
            );
        }
    });
}

// ── an edit is a pending change, and says so ──────────────────────────────

/// A streamed mount writes every member during the pass, so what's on disk at
/// mount time is what spyc put there. Recording nothing is how a `.tar.gz` came
/// out of its own mount already reading as edited — and the only sign was a
/// warning at quit about changes nobody made.
#[test]
fn a_streamed_mount_is_not_dirty_the_moment_it_opens() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.tar.gz");
        build_tar_gz(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        assert!(
            !app.state.cur().rows.is_empty(),
            "the mount really did extract"
        );
        assert!(
            !app.mount_is_dirty(&archive),
            "a freshly opened archive has no pending changes"
        );
        assert!(app.dirty_mounts().is_empty(), "and quitting says nothing");
    });
}

/// The user's question: with an edit staged and nothing written, what says so?
/// The badge reads the journal, so the edit has to be *in* it — the draw pass
/// can't go and stat the staging tree.
#[test]
fn an_edit_becomes_a_pending_change_the_badge_can_show() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // `e` on `README.md`: extracts it and hands the copy to an editor.
        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::EnterOrEdit).unwrap();
        settle(&mut app, fx);
        let mount = app.state.mounts.get(&archive).expect("mounted");
        assert_eq!(
            mount.editing,
            ["README.md"],
            "the member is watched from the moment it's handed over"
        );
        let staged = mount.staging_path(mount.index.get("README.md").unwrap());
        assert!(!mount.journal.is_dirty(), "nothing pending yet");

        // The editor saves.
        std::fs::write(&staged, b"# edited by hand\n").unwrap();
        assert!(app.settle_archive_edits(), "the change is noticed");

        let mount = app.state.mounts.get(&archive).expect("mounted");
        assert!(mount.journal.is_dirty(), "and recorded as pending");
        assert_eq!(
            mount.journal.counts().badge(),
            "~1",
            "which is what the status suffix shows"
        );
        assert!(
            mount.editing.is_empty(),
            "and it stops being watched once recorded"
        );
    });
}

/// The route that actually bit: the pager's `v` edits its own `source_path`,
/// which for a member *is* the staged copy — so spyc never handed the member to
/// an editor through `open_member`. An agent in the pane writing the same file
/// looks identical. Neither can be caught by watching only what spyc knows it
/// handed over, which is why the scan covers everything staged.
#[test]
fn an_edit_spyc_did_not_make_shows_up_without_being_asked() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Extract a member the way a *read* does — nothing is handed to an editor.
        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::Take).unwrap();
        settle(&mut app, fx);
        let mount = app.state.mounts.get(&archive).expect("mounted");
        assert!(mount.editing.is_empty(), "spyc handed nothing to an editor");
        let staged = mount.staging_path(mount.index.get("README.md").unwrap());

        std::fs::write(&staged, b"# changed by something else\n").unwrap();

        assert!(
            app.settle_archive_edits(),
            "and it is still noticed, with no command run"
        );
        assert_eq!(
            app.state
                .mounts
                .get(&archive)
                .unwrap()
                .journal
                .counts()
                .badge(),
            "~1"
        );
    });
}

/// The bound that keeps that scan affordable. A streamed mount extracts every
/// member, so its staged set is the whole archive — statting it on every keypress
/// would be visible jank, and those fall back to the scan at reporting time.
#[test]
fn a_huge_staged_set_falls_back_to_the_reporting_scan() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("many.tar.gz");
        build_tar_gz_with(&archive, 300);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let mount = app.state.mounts.get(&archive).expect("mounted");
        assert!(
            mount.staged.len() > 256,
            "the fixture is past the bound: {}",
            mount.staged.len()
        );
        let staged = mount.staging_path(mount.index.get("file-7.txt").unwrap());
        std::fs::write(&staged, b"edited\n").unwrap();

        assert!(
            !app.settle_archive_edits(),
            "too many to stat on every wake"
        );
        assert!(app.scan_archive_edits(), "but reporting still catches it");
        assert_eq!(
            app.state
                .mounts
                .get(&archive)
                .unwrap()
                .journal
                .counts()
                .badge(),
            "~1"
        );
    });
}

/// A staging directory's mtime moves whenever any member inside it is written, so
/// counting directories reported a change to a member nobody touched.
#[test]
fn writing_one_member_does_not_mark_its_neighbours() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Extract two members that share a directory.
        for inner in ["src/main.rs", "src/deep/mod.rs"] {
            let fx = app.open_member(
                &archive.join(inner),
                crate::app::archive_ops::MaterializeThen::OpenPager(
                    crate::app::file_ops::PagerDest::Overlay { scroll: None },
                ),
            );
            settle(&mut app, fx);
        }
        assert!(app.dirty_mounts().is_empty(), "reading changes nothing");
        assert!(
            !app.scan_archive_edits(),
            "and neither does the directory mtime that moved with them"
        );
    });
}

/// From outside a mount, `:archive info` used to refuse — which is exactly where
/// an archive holding unwritten changes is hardest to notice.
#[test]
fn archive_info_outside_a_mount_reports_what_is_mounted() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Delete a member, then climb out of the archive entirely.
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
        assert_eq!(app.state.cur().listing.dir, dir, "outside the archive");

        let fx = app.cmd_archive("info");
        settle(&mut app, fx);

        let shown = app
            .view
            .pager
            .as_ref()
            .map(|p| {
                p.lines
                    .iter()
                    .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
            .join("");
        assert!(shown.contains("pkg.zip"), "names the archive: {shown}");
        assert!(shown.contains("-1"), "and its pending change: {shown}");
    });
}

/// End to end, in the terms the complaint was made in: after an edit, the badge is
/// **painted**.
///
/// Every other test here asserts the *journal*. That's one layer short of "I get
/// no visual indicator", and the gap between the two is where a blind badge hid
/// twice — once because the edit never reached the journal, once because the tag
/// was resolved against the wrong column.
#[test]
fn the_status_bar_paints_the_badge_after_an_edit() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let paint = |app: &mut App| -> String {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 24)).unwrap();
            terminal.draw(|f| app.render(f)).unwrap();
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect()
        };

        let before = paint(&mut app);
        assert!(before.contains("zip"), "inside a mount, the bar says so");
        assert!(
            !before.contains("~1"),
            "and nothing is pending yet: {before}"
        );

        // Read a member (staging it), then change that copy the way the pager's
        // `v` — or an agent in the pane — would, without telling spyc.
        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::Take).unwrap();
        settle(&mut app, fx);
        let staged = {
            let mount = app.state.mounts.get(&archive).unwrap();
            mount.staging_path(mount.index.get("README.md").unwrap())
        };
        std::fs::write(&staged, b"# edited\n").unwrap();
        assert!(app.settle_archive_edits(), "the change is noticed");

        let after = paint(&mut app);
        assert!(
            after.contains("~1"),
            "the status bar is what the user reads: {after}"
        );
    });
}

/// The reported flow, keys and all — `Enter` a member to page it, `v` to edit it,
/// save — and the thing the user is looking at while they do it: **the row**.
///
/// The aggregate badge in the status suffix was there all along; what wasn't was
/// any mark on the member itself, so an edited file looked exactly like an
/// untouched one. Inside a mount the gutter answers the archive's question rather
/// than git's.
#[test]
fn an_edited_member_is_marked_in_the_listing() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Painted, not inspected: the gutter's unstaged column is what the user
        // is looking at, and `~` is its modified glyph.
        let readme_row = |app: &mut App| -> String {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 12)).unwrap();
            terminal.draw(|f| app.render(f)).unwrap();
            let buf = terminal.backend().buffer().clone();
            (0..buf.area.height)
                .map(|y| {
                    (0..buf.area.width)
                        .map(|x| buf[(x, y)].symbol())
                        .collect::<String>()
                })
                .find(|line| line.contains("README"))
                .unwrap_or_default()
        };
        let before = readme_row(&mut app);
        assert!(!before.contains('~'), "untouched to begin with: {before:?}");

        // Enter it (pager on the staged copy), then `v` to hand it to an editor.
        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        settle(&mut app, fx);
        let staged = app
            .view
            .pager
            .as_ref()
            .and_then(|p| p.source_path.clone())
            .expect("paged from the staging tree");
        let _ = app.handle_pager_key(crossterm::event::KeyEvent::new(
            KeyCode::Char('v'),
            KeyModifiers::empty(),
        ));

        // The editor saves.
        std::fs::write(&staged, b"# edited in the editor\n").unwrap();
        assert!(app.settle_archive_edits(), "the edit is noticed");

        let after = readme_row(&mut app);
        assert!(
            after.contains('~'),
            "and the row is marked: {after:?} (was {before:?})"
        );
    });
}

/// The place the indicator was missing that mattered most: standing in the
/// directory the archive lives in, looking at the archive file, with an unwritten
/// change inside it.
///
/// The suffix badge only shows while you're *in* the mount, and the members aren't
/// listed out here — so without a mark on the container's own row there is nothing
/// at all to see.
#[test]
fn a_dirty_archive_is_marked_in_the_directory_it_lives_in() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let archive_row = |app: &mut App| -> String {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 12)).unwrap();
            terminal.draw(|f| app.render(f)).unwrap();
            let buf = terminal.backend().buffer().clone();
            (0..buf.area.height)
                .map(|y| {
                    (0..buf.area.width)
                        .map(|x| buf[(x, y)].symbol())
                        .collect::<String>()
                })
                .find(|line| line.contains("pkg.zip"))
                .unwrap_or_default()
        };

        // Delete a member, then climb out to where the archive lives.
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
        assert_eq!(app.state.cur().listing.dir, dir, "outside the archive");

        let row = archive_row(&mut app);
        assert!(
            row.contains('~'),
            "the archive says it holds unwritten changes: {row:?}"
        );

        // And once written it stops saying so. (The write runs from *inside* the
        // mount: `:archive write` resolves which archive it means from the cwd.)
        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        settle(&mut app, fx);
        assert_eq!(app.state.cur().listing.dir, archive, "back inside");
        let fx = app.cmd_archive("write");
        settle(&mut app, fx);
        assert!(!app.mount_is_dirty(&archive), "the write landed");

        let fx = app.apply(&Action::Climb).unwrap();
        apply_effects(&mut app, fx);
        let row = archive_row(&mut app);
        assert!(!row.contains('~'), "clean again after the write: {row:?}");
    });
}

// ── what a yanked member is remembered as ─────────────────────────────────

/// A yanked member is remembered by its path *into the archive*, not by the
/// staged copy the bytes came from.
///
/// The staging path is a per-process cache location: recording it meant the row
/// never showed as taken (its path is the member's), a re-yank in a later session
/// made a second entry instead of refreshing the first, and the inventory kept a
/// path that stopped existing when the session did.
#[test]
fn a_yanked_member_is_remembered_by_its_path_in_the_archive() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // `y` on `README.md`.
        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::Take).unwrap();
        settle(&mut app, fx);

        let member = archive.join("README.md");
        let item = app
            .state
            .inventory
            .items()
            .find(|i| i.filename == "README.md")
            .expect("in the inventory");
        assert_eq!(
            item.orig_path, member,
            "remembered as the member path, not the staging copy"
        );
        assert_eq!(
            app.state.inventory.read_content(&item.id).as_deref(),
            Some(b"# pkg\n".as_slice()),
            "with the member's real bytes, which came from staging"
        );
        // Which is what makes the row's take-check work: it compares the row's own
        // path against the inventory.
        assert!(
            app.state.inventory.contains(&member),
            "so the member row reads as taken"
        );
    });
}

/// Re-yanking the same member refreshes its entry rather than adding a second
/// one. Keyed on the staging path it couldn't: that path carries the pid.
#[test]
fn re_yanking_a_member_refreshes_one_entry() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        app.apply(&Action::Down(1)).unwrap();

        let fx = app.apply(&Action::Take).unwrap();
        settle(&mut app, fx);
        // Re-mount into a *different* staging tree, as a later session would, and
        // yank the same member again.
        app.state.mounts.remove(&archive);
        mount_inline(&mut app, &archive, &tmp.path().join("staging-2"));
        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::Take).unwrap();
        settle(&mut app, fx);

        let held: Vec<_> = app
            .state
            .inventory
            .items()
            .filter(|i| i.filename == "README.md")
            .collect();
        assert_eq!(held.len(), 1, "one entry, refreshed: {held:?}");
    });
}

// ── writing without being inside the archive ──────────────────────────────

/// Delete a member, then climb out, leaving the cursor on the archive.
fn dirty_then_climb_out(app: &mut App, dir: &Path) {
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
    settle(app, fx);
    let fx = app.apply(&Action::Climb).unwrap();
    apply_effects(app, fx);
    assert_eq!(app.state.cur().listing.dir, dir, "outside the archive");
}

/// The dead end the container marker created: you can see `demo.zip` holds
/// unwritten changes from the directory it lives in, and `:archive write` there
/// did nothing because it resolved the archive from the cwd. The cursor is on the
/// row that told you, so that's what it means.
#[test]
fn writing_from_outside_uses_the_archive_under_the_cursor() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        dirty_then_climb_out(&mut app, &dir);

        // Climbing out leaves the cursor on the archive it came from.
        let cursor = app
            .state
            .cur()
            .rows
            .get(app.state.cur().cursor.index)
            .map(|r| r.path.clone());
        assert_eq!(
            cursor.as_deref(),
            Some(archive.as_path()),
            "cursor is on it"
        );

        let fx = app.cmd_archive("write");
        settle(&mut app, fx);

        assert_eq!(
            member_names(&archive),
            ["src/deep/mod.rs", "src/main.rs"],
            "the delete was applied to the archive on disk"
        );
        assert!(!app.mount_is_dirty(&archive), "and nothing is pending");
    });
}

/// Cursor elsewhere: one archive has changes, so there's nothing to guess at.
#[test]
fn writing_from_outside_falls_back_to_the_only_dirty_archive() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        std::fs::write(dir.join("aaa.txt"), b"not an archive\n").unwrap();
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        dirty_then_climb_out(&mut app, &dir);

        // Move the cursor off the archive entirely.
        app.state.cur_mut().cursor.index = 0;
        let cursor = app
            .state
            .cur()
            .rows
            .first()
            .map(|r| r.path.clone())
            .unwrap();
        assert_ne!(cursor, archive, "cursor is not on the archive");

        let fx = app.cmd_archive("write");
        settle(&mut app, fx);
        assert!(!app.mount_is_dirty(&archive), "written anyway");
    });
}

/// Two dirty archives and no cursor to disambiguate: say so rather than pick.
#[test]
fn writing_from_outside_refuses_to_guess_between_two() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = App::test_app(dir.clone());
        for (name, staging) in [("one.zip", "s1"), ("two.zip", "s2")] {
            let archive = dir.join(name);
            build_zip(&archive);
            mount_inline(&mut app, &archive, &tmp.path().join(staging));
            dirty_then_climb_out(&mut app, &dir);
        }
        assert_eq!(app.dirty_mounts().len(), 2, "both are dirty");

        // Cursor on neither.
        std::fs::write(dir.join("aaa.txt"), b"x\n").unwrap();
        app.state.refresh_listing();
        app.state.cur_mut().cursor.index = 0;

        let fx = app.cmd_archive("write");
        settle(&mut app, fx);

        let flash = app.state.flash.as_ref().map(|f| f.text.clone());
        assert!(
            flash
                .as_deref()
                .is_some_and(|t| t.contains('2') && t.contains("write")),
            "names the count and the verb: {flash:?}"
        );
        assert_eq!(app.dirty_mounts().len(), 2, "and wrote neither");
    });
}

#[test]
fn writing_with_nothing_pending_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = App::test_app(dir);

        let fx = app.cmd_archive("write");
        settle(&mut app, fx);
        let flash = app.state.flash.as_ref().map(|f| f.text.clone());
        assert_eq!(flash.as_deref(), Some("archive: nothing to write"));
    });
}

/// `:archive discard` has to drop the staged copies too, not just the journal.
///
/// Clearing the journal alone left an edited copy on disk for the next scan to
/// re-record — so the badge came back and the "discarded" edit was still pending.
#[test]
fn discarding_an_edit_drops_the_staged_copy_too() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Read a member (staging it), then change that copy behind spyc's back.
        app.apply(&Action::Down(1)).unwrap();
        let fx = app.apply(&Action::Take).unwrap();
        settle(&mut app, fx);
        let staged = {
            let mount = app.state.mounts.get(&archive).unwrap();
            mount.staging_path(mount.index.get("README.md").unwrap())
        };
        std::fs::write(&staged, b"# edited\n").unwrap();
        assert!(app.settle_archive_edits(), "noticed as pending");
        assert_eq!(
            app.state
                .mounts
                .get(&archive)
                .unwrap()
                .journal
                .counts()
                .badge(),
            "~1"
        );

        let fx = app.cmd_archive("discard");
        settle(&mut app, fx);

        assert!(!staged.exists(), "the edited copy is gone");
        assert!(
            !app.mount_is_dirty(&archive),
            "and nothing is pending any more"
        );
        assert!(
            !app.settle_archive_edits(),
            "including on the next scan — this is where it used to come back"
        );

        // Reading it again gets the archive's own bytes.
        let fx = app.apply(&Action::Take).unwrap();
        settle(&mut app, fx);
        let item = app
            .state
            .inventory
            .items()
            .find(|i| i.filename == "README.md")
            .unwrap();
        assert_eq!(
            app.state.inventory.read_content(&item.id).as_deref(),
            Some(b"# pkg\n".as_slice()),
            "the archive's version, not the discarded edit"
        );
    });
}

/// An added member's staged bytes go too — nothing brought in survives a discard.
#[test]
fn discarding_an_added_member_removes_its_staged_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let brought = dir.join("extra.txt");
        std::fs::write(&brought, b"brought in\n").unwrap();
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        // Copy a real file into the mount, which records an Add and stages bytes.
        let fx = app.screen_archive_effect(Effect::FileOp(crate::app::file_ops::FileOp::Copy {
            paths: vec![brought],
            dest: archive.clone(),
        }));
        settle(&mut app, fx.into_iter().collect());
        let staged = app
            .state
            .mounts
            .get(&archive)
            .unwrap()
            .staging_root
            .join("extra.txt");
        assert!(staged.exists(), "the brought-in bytes are staged");

        let fx = app.cmd_archive("discard");
        settle(&mut app, fx);

        assert!(!staged.exists(), "and discarded with the change");
        assert!(!app.mount_is_dirty(&archive));
        assert!(
            !app.state
                .cur()
                .rows
                .iter()
                .any(|r| r.display.starts_with("extra.txt")),
            "the row is gone too"
        );
    });
}

// ── a member the user brought in is a member ──────────────────────────────

/// Yank a real file and put it into a mount, handing back the inventory ids in
/// case the test needs to put it again.
fn put_a_file_into(app: &mut App, archive: &Path, staging: &Path, name: &str, body: &[u8]) {
    let source = archive.parent().unwrap().join(name);
    std::fs::write(&source, body).unwrap();
    settle(
        app,
        vec![Effect::Inventory(
            crate::app::inventory_ops::InventoryOp::Yank {
                sources: vec![source]
                    .into_iter()
                    .map(crate::app::inventory_ops::YankSource::plain)
                    .collect(),
            },
        )],
    );
    if !app.state.mounts.contains(archive) {
        mount_inline(app, archive, staging);
    }
    let ids: Vec<String> = app.state.inventory.items().map(|i| i.id.clone()).collect();
    settle(
        app,
        vec![Effect::Inventory(
            crate::app::inventory_ops::InventoryOp::Put {
                dest_dir: archive.to_path_buf(),
                ids,
            },
        )],
    );
}

/// The bug this fixes: a put file listed fine but refused every read, edit and
/// delete with "no such member", because it lives in the journal and the staging
/// tree while `entry_at` only ever asked the index.
#[test]
fn a_put_member_can_be_opened_yanked_and_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let staging = tmp.path().join("staging");
        let mut app = App::test_app(dir);
        put_a_file_into(&mut app, &archive, &staging, "brought.txt", b"payload");

        let added = archive.join("brought.txt");
        assert!(
            row_names(&app).contains(&"brought.txt".to_string()),
            "the put file lists: {:?}",
            row_names(&app)
        );

        // Opening it reaches the staged bytes. Those are already on disk — a put
        // wrote them — so the pager opens with no worker round trip at all, which
        // is why the proof is the pager rather than an effect.
        app.state.flash = None;
        let fx = app.open_member(
            &added,
            crate::app::archive_ops::MaterializeThen::OpenPager(
                crate::app::file_ops::PagerDest::Overlay { scroll: None },
            ),
        );
        assert!(fx.is_empty(), "nothing to extract: {fx:?}");
        assert_eq!(
            app.state.flash.as_ref().map(|f| f.text.clone()),
            None,
            "and it is not refused"
        );
        assert_eq!(
            app.view
                .pager
                .as_ref()
                .expect("the put member is paged")
                .source_path
                .as_deref(),
            Some(staging.join("brought.txt").as_path()),
            "reading the staged copy the put wrote"
        );
        app.view.pager = None;

        // Reading it out — `y`, and by the same route `L` / `f` / copy-out.
        settle(
            &mut app,
            vec![Effect::Inventory(
                crate::app::inventory_ops::InventoryOp::Yank {
                    sources: vec![crate::app::inventory_ops::YankSource::plain(added.clone())],
                },
            )],
        );
        let cached = app
            .state
            .inventory
            .items()
            .find(|i| i.orig_path == added)
            .expect("the put member yanks, addressed by its path in the archive");
        assert_eq!(cached.filename, "brought.txt");
    });
}

/// Deleting a put member un-adds it: the row goes, the journal goes clean, and
/// the staged copy is unlinked so the same name can be put again.
#[test]
fn deleting_a_put_member_un_adds_it_and_frees_the_name() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let staging = tmp.path().join("staging");
        let mut app = App::test_app(dir);
        put_a_file_into(&mut app, &archive, &staging, "brought.txt", b"first");

        let added = archive.join("brought.txt");
        let staged_copy = staging.join("brought.txt");
        assert!(staged_copy.exists(), "the put staged its bytes");

        app.state.flash = None;
        settle(
            &mut app,
            vec![Effect::Graveyard(
                crate::app::graveyard_ops::GraveyardOp::Archive { paths: vec![added] },
            )],
        );
        assert!(
            !row_names(&app).contains(&"brought.txt".to_string()),
            "the row is gone: {:?}",
            row_names(&app)
        );
        assert!(
            !app.mount_is_dirty(&archive),
            "nothing is pending — the archive never held it"
        );
        assert!(
            !staged_copy.exists(),
            "and its staged copy went with it, so the name is free again"
        );

        // Free means free: the same file can be put back.
        put_a_file_into(&mut app, &archive, &staging, "brought.txt", b"second");
        assert!(
            row_names(&app).contains(&"brought.txt".to_string()),
            "a second put of the same name lands: {:?}",
            row_names(&app)
        );
        assert_eq!(std::fs::read(&staged_copy).unwrap(), b"second");
    });
}

/// An archived member is unaffected: deleting one is still a recorded removal
/// that the write-back applies.
#[test]
fn deleting_an_archived_member_is_still_recorded() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        settle(
            &mut app,
            vec![Effect::Graveyard(
                crate::app::graveyard_ops::GraveyardOp::Archive {
                    paths: vec![archive.join("README.md")],
                },
            )],
        );
        assert!(app.mount_is_dirty(&archive), "the removal is pending");
        let fx = app.cmd_archive("write");
        settle(&mut app, fx);
        assert!(
            !member_names(&archive).contains(&"README.md".to_string()),
            "{:?}",
            member_names(&archive)
        );
    });
}

/// End to end: a put member survives the write-back and is a real archived
/// member afterwards — the point of making it actionable in the first place.
#[test]
fn a_put_member_edited_then_written_carries_its_edit_in() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let staging = tmp.path().join("staging");
        let mut app = App::test_app(dir);
        put_a_file_into(&mut app, &archive, &staging, "brought.txt", b"before");

        // Edit it the way an editor or an agent would — straight on the staged copy.
        std::fs::write(staging.join("brought.txt"), b"after the edit").unwrap();

        let fx = app.cmd_archive("write");
        settle(&mut app, fx);

        assert!(
            member_names(&archive).contains(&"brought.txt".to_string()),
            "{:?}",
            member_names(&archive)
        );
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&archive).unwrap()).unwrap();
        let mut body = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("brought.txt").unwrap(), &mut body).unwrap();
        assert_eq!(body, "after the edit");
    });
}

/// After a write-back the mount is rebuilt from the file on disk, and the stale
/// staging tree is emptied **by that same op** — never by a `Clean` issued
/// alongside it.
///
/// Every archive op gets its own thread, so a `Clean` and a `Mount` naming one
/// staging root run concurrently: `remove_dir_all` walking the tree while the
/// extraction refills it produced `creating <staging>/…: File exists (os error
/// 17)` against a real 459-member tarball, reproducible about one run in ten.
/// A race can't be proved absent by running it, so the guard is structural.
#[test]
fn a_write_back_re_reads_without_a_clean_racing_its_staging_tree() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        settle(
            &mut app,
            vec![Effect::Graveyard(
                crate::app::graveyard_ops::GraveyardOp::Archive {
                    paths: vec![archive.join("README.md")],
                },
            )],
        );

        // Run the write, then take exactly what its outcome asks for next.
        let write = app.cmd_archive("write");
        for effect in write {
            let Effect::Archive(op) = effect else {
                continue;
            };
            let outcome = run_archive_op(op);
            app.runtime.archive_results.lock().unwrap().push(outcome);
        }
        let (_, after_write) = app.apply_archive_outcomes();

        let mut mount_roots = Vec::new();
        let mut clean_roots = Vec::new();
        for effect in &after_write {
            match effect {
                Effect::Archive(ArchiveOp::Mount {
                    staging_root,
                    reset_staging,
                    ..
                }) => {
                    assert!(
                        *reset_staging,
                        "the re-read empties the tree itself, so no separate Clean is needed"
                    );
                    mount_roots.push(staging_root.clone());
                }
                Effect::Archive(ArchiveOp::Clean { staging_roots }) => {
                    clean_roots.extend(staging_roots.iter().cloned());
                }
                _ => {}
            }
        }
        assert_eq!(mount_roots.len(), 1, "one re-read: {after_write:?}");
        assert!(
            !clean_roots.contains(&mount_roots[0]),
            "a Clean on {:?} would race the re-read filling it — cleans: {clean_roots:?}",
            mount_roots[0]
        );
    });
}

// ── an in-flight message doesn't outlive its operation ────────────────────

/// The user's report: after entering an archive, `reading pkg.zip…` stayed on
/// the status bar, reading as though more were still to come.
///
/// A flash has no lifetime of its own, and the mount arm only speaks up when the
/// archive had something odd about it — so a clean mount left the in-flight
/// message as the last thing said.
#[test]
fn the_reading_message_goes_once_the_archive_is_mounted() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);

        let fx = app.request_mount(&archive, true);
        let flash = app.state.flash.as_ref().expect("the wait is announced");
        assert_eq!(flash.text, "reading pkg.zip…");
        assert!(matches!(flash.kind, crate::app::FlashKind::Progress));

        settle(&mut app, fx);

        assert_eq!(
            app.state.flash.as_ref().map(|f| f.text.clone()),
            None,
            "nothing is left claiming the read is still going"
        );
        assert!(app.state.mounts.contains(&archive), "and it did mount");
    });
}

/// The clear must not eat what the mount had to say. Two members differing only
/// by case is a real warning — and the reason the arm flashes at all.
///
/// Duplicate *identical* names would be the sharper fixture, but `ZipWriter`
/// refuses to write them, which is why the read-only test builds its capability
/// by hand.
#[test]
fn a_mount_with_something_to_report_still_reports_it() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("collide.zip");
        let f = std::fs::File::create(&archive).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default();
        for name in ["same.txt", "SAME.txt"] {
            w.start_file(name, opts).unwrap();
            w.write_all(b"body").unwrap();
        }
        w.finish().unwrap();
        let mut app = App::test_app(dir);

        let fx = app.request_mount(&archive, true);
        settle(&mut app, fx);

        let flash = app
            .state
            .flash
            .as_ref()
            .expect("the note survives the clear");
        assert!(
            flash.text.contains("differ only by case"),
            "it is the note, not the in-flight message: {}",
            flash.text
        );
        assert!(
            !matches!(flash.kind, crate::app::FlashKind::Progress),
            "a note is not progress"
        );
    });
}

/// An error is a real message too: a failed read replaces the in-flight one
/// rather than being cleared along with it.
#[test]
fn a_failed_read_leaves_its_error_on_screen() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let broken = dir.join("broken.zip");
        std::fs::write(&broken, b"PK\x03\x04 and then nothing usable").unwrap();
        let mut app = App::test_app(dir);

        let fx = app.request_mount(&broken, true);
        settle(&mut app, fx);

        let flash = app.state.flash.as_ref().expect("the failure is reported");
        assert!(
            matches!(flash.kind, crate::app::FlashKind::Error),
            "an error replaced the in-flight message rather than being cleared with it"
        );
        // Not a `contains("reading")` check: the error's own context chain says
        // "reading zip <path>", so the wording overlaps the progress message and
        // only the kind distinguishes them.
        assert!(
            flash.text.contains("Could not find EOCD"),
            "and it still names the cause: {}",
            flash.text
        );
    });
}
