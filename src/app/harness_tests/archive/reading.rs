//! Reading members out: the effect screen that extracts a member before an
//! effect reads it, where a streamed mount's bytes live, and what a yanked
//! member is remembered as.

use super::*;

// ── reading members out (the effect screen) ──────────────────────────────

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

// ── an extracted member's bytes are in staging, never at its mount path ───

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
