//! Entering a container, browsing what it holds, climbing back out, and the
//! messages a mount puts on screen while it works.

use super::*;

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
            .active_project_mut()
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
        assert!(
            app.view.project().pager.is_none(),
            "nothing is paged synchronously"
        );

        // Run the extraction and its follow-up the way the loop does.
        for effect in fx {
            let Effect::Archive(op) = effect else {
                continue;
            };
            let outcome = run_archive_op(op);
            app.file_archive_outcome(outcome);
        }
        app.apply_archive_outcomes();

        let pager = app
            .view
            .project()
            .pager
            .as_ref()
            .expect("the member is paged");
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
        assert!(
            app.view.project().pager.is_some(),
            "the pager opens immediately"
        );
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
            app.file_archive_outcome(outcome);
        }
        app.apply_archive_outcomes();

        assert!(app.state.mounts.is_empty(), "nothing was mounted");
        let pager = app
            .view
            .project()
            .pager
            .as_ref()
            .expect("it opens as a file");
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
        app.state.active_project_mut().config.archive.enable = false;
        app.state.refresh_listing();

        let fx = app.apply(&Action::EnterOrDisplay).unwrap();
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::Archive(_))),
            "no mount is attempted: {fx:?}"
        );
        assert!(
            app.view.project().pager.is_some(),
            "the archive is paged as bytes"
        );
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
        assert!(matches!(flash.kind, crate::app::FlashKind::Progress(_)));

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
            !matches!(flash.kind, crate::app::FlashKind::Progress(_)),
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

/// Copying a directory into a mount used to report a successful write and lose
/// everything inside it: the journal recorded one `Added { inner: "docs" }` for
/// the whole tree, so the repack emitted a member named `docs` and dropped
/// `docs/a.md` and `docs/b.md`. The verify pass couldn't catch it either — it
/// compares the new archive against the same plan the children were missing from.
#[test]
fn copying_a_directory_into_an_archive_is_refused_rather_than_flattened() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let before = member_names(&archive);
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let tree = dir.join("docs");
        std::fs::create_dir(&tree).unwrap();
        std::fs::write(tree.join("a.md"), b"# a\n").unwrap();

        app.state.flash = None;
        settle(
            &mut app,
            vec![Effect::FileOp(crate::app::file_ops::FileOp::Copy {
                paths: vec![tree],
                dest: archive.clone(),
            })],
        );

        let flash = app.state.flash.as_ref().map(|f| f.text.clone());
        assert!(
            flash
                .as_deref()
                .is_some_and(|f| f.contains("not directories")),
            "the user is told why, not left with a silent loss: {flash:?}"
        );
        assert!(
            !app.mount_is_dirty(&archive),
            "and nothing was recorded to write back"
        );
        assert!(
            !row_names(&app).contains(&"docs".to_string()),
            "no phantom row: {:?}",
            row_names(&app)
        );

        // A write now has nothing to do, and the archive is untouched.
        let fx = app.cmd_archive("write");
        settle(&mut app, fx);
        assert_eq!(member_names(&archive), before);
    });
}

// ── One read of an archive at a time ─────────────────────────────────────────

fn starts_a_read(effects: &[Effect]) -> bool {
    effects.iter().any(|e| matches!(e, Effect::Archive(_)))
}

/// `Enter` on an archive whose read is still running, a second time or from
/// the other column, starts no second read: it joins the one running, and the
/// column is in the archive once that lands.
#[test]
fn entering_an_archive_being_read_joins_the_read() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        let first = app.request_mount(&archive, false);
        assert!(starts_a_read(&first), "{first:?}");
        let second = app.request_mount(&archive, false);
        assert!(!starts_a_read(&second), "a second read started: {second:?}");
        settle(&mut app, first);
        assert_eq!(app.state.cur().listing.dir, archive);
        assert!(
            !app.flash_text().is_some_and(|f| f.starts_with("reading")),
            "{:?}",
            app.flash_text()
        );
    });
}

/// An archive inside another is read from its staged copy, and only once, the
/// same way.
#[test]
fn entering_a_nested_archive_being_read_joins_the_read() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let staged = dir.join("inner.zip");
        build_zip(&staged);
        let at = dir.join("outer.zip").join("inner.zip");
        let mut app = App::test_app(dir);
        let first = app.request_nested_mount(&at, &staged, None);
        assert!(starts_a_read(&first), "{first:?}");
        let second = app.request_nested_mount(&at, &staged, None);
        assert!(!starts_a_read(&second), "a second read started: {second:?}");
    });
}

/// A compressed tar is extracted as it is read, into a staging tree every
/// read of it shares. A second read, landing after an edit the user saved to
/// a member, would rewrite the member with the archive's bytes while the edit
/// still reads as pending, and `:archive write` would pack the old bytes.
#[test]
fn an_edit_saved_while_a_compressed_tar_is_read_twice_survives() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.tar.gz");
        build_tar_gz(&archive);
        let mut app = App::test_app(dir);
        let first = app.request_mount(&archive, false);
        let second = app.request_mount(&archive, false);
        settle(&mut app, first);
        assert_eq!(app.state.cur().listing.dir, archive);
        let staged = app
            .state
            .mounts
            .get(&archive)
            .unwrap()
            .staging_root
            .join("README.md");
        std::fs::write(&staged, "# edited, and longer\n").unwrap();
        app.scan_archive_edits();
        settle(&mut app, second);
        assert_eq!(
            std::fs::read_to_string(&staged).unwrap(),
            "# edited, and longer\n",
            "a second read rewrote the edit"
        );
        assert_eq!(app.dirty_mounts(), std::slice::from_ref(&archive));
    });
}

/// A read that ends without a mount, here because the bytes are no archive,
/// is over: the next request for the file reads it again.
#[test]
fn a_read_that_ends_without_a_mount_lets_the_next_request_read() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("notes.zip");
        std::fs::write(&archive, "not a zip\n").unwrap();
        let mut app = App::test_app(dir);
        let first = app.request_mount(&archive, false);
        settle(&mut app, first);
        assert!(!app.state.mounts.contains(&archive));
        let again = app.request_mount(&archive, false);
        assert!(starts_a_read(&again), "{again:?}");
    });
}

/// The re-read after a write is a read like any other: a request for the
/// archive while it runs joins it rather than reading it alongside.
#[test]
fn a_request_while_a_write_is_reread_joins_the_reread() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir.clone());
        let fx = app.request_mount(&archive, false);
        settle(&mut app, fx);
        // Back out without `chdir`, which would move the process cwd.
        app.state.cur_mut().listing = crate::fs::listing::Listing::read(&dir).unwrap();
        app.file_archive_outcome(ArchiveOutcome::Written {
            archive: archive.clone(),
            report: crate::archive::write::RepackReport {
                members: 3,
                bytes: 1,
                snapshot: None,
            },
        });
        let (_, reread) = app.apply_archive_outcomes();
        assert!(starts_a_read(&reread), "{reread:?}");
        let again = app.request_mount(&archive, false);
        assert!(!starts_a_read(&again), "a second read started: {again:?}");
        settle(&mut app, reread);
        assert_eq!(app.state.cur().listing.dir, archive);
    });
}

// ── a clean-up takes only the tree it was handed ─────────────────────────────

/// Read `archive` and land the read, handing back what the landing asked for
/// (a clean-up of what it evicted) without running it.
fn land_read(app: &mut App, archive: &Path) -> Vec<Effect> {
    for effect in app.request_mount(archive, false) {
        if let Effect::Archive(op) = effect {
            app.file_archive_outcome(run_archive_op(op));
        }
    }
    let (_, follow_on) = app.apply_archive_outcomes();
    follow_on
}

fn cleans(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::Archive(ArchiveOp::Clean { .. })))
}

/// Unmounting hands the staging tree to a clean-up on its own thread, and
/// entering the archive again at once reads into a tree at the same path. The
/// clean-up takes only the tree it was handed: what the new read extracted
/// survives it.
#[test]
#[ignore = "red: the clean-up after an unmount deletes what a read right after it extracts"]
fn a_read_right_after_an_unmount_keeps_what_it_extracts() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.tar.gz");
        build_tar_gz(&archive);
        let mut app = App::test_app(dir.clone());
        assert!(land_read(&mut app, &archive).is_empty());
        app.state.cur_mut().listing = crate::fs::listing::Listing::read(&dir).unwrap();
        let clean = app.unmount_archive(&archive);
        assert!(cleans(&clean), "{clean:?}");
        assert!(land_read(&mut app, &archive).is_empty());
        let staged = app
            .state
            .mounts
            .get(&archive)
            .unwrap()
            .staging_root
            .join("README.md");
        assert!(staged.exists(), "the new read extracted nothing");
        settle(&mut app, clean);
        assert!(staged.exists(), "the clean-up took the new read's files");
    });
}

/// The same for the clean-up of a mount evicted to make room: reading the
/// evicted archive again at once keeps what that read extracts.
#[test]
#[ignore = "red: the clean-up after an eviction deletes what a read right after it extracts"]
fn a_read_right_after_an_eviction_keeps_what_it_extracts() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let first = dir.join("first.tar.gz");
        build_tar_gz(&first);
        let mut app = App::test_app(dir.clone());
        assert!(land_read(&mut app, &first).is_empty());
        let mut clean = Vec::new();
        for i in 0..crate::archive::mount::MAX_MOUNTS {
            let zip = dir.join(format!("z{i}.zip"));
            build_zip(&zip);
            clean.extend(land_read(&mut app, &zip));
        }
        assert!(
            !app.state.mounts.contains(&first),
            "the first archive was never evicted"
        );
        assert!(cleans(&clean), "{clean:?}");
        let _ = land_read(&mut app, &first);
        let staged = app
            .state
            .mounts
            .get(&first)
            .unwrap()
            .staging_root
            .join("README.md");
        assert!(staged.exists(), "the new read extracted nothing");
        settle(&mut app, clean);
        assert!(staged.exists(), "the clean-up took the new read's files");
    });
}
