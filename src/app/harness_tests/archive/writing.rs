//! Writing changes back into the container, from inside the mount or from
//! the directory it sits in.

use super::*;

// ── writing back ─────────────────────────────────────────────────────────

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

/// **An archive that genuinely earns read-only gets it, end to end.**
///
/// The sibling test below hand-writes the demotion into the mount, so it proves
/// the refusal honours a `ReadOnly` capability but not that anything ever *sets*
/// one. Here the archive earns it: a `..` member is skipped on the way in, and a
/// repack that silently dropped it would hand the user a lossy archive with no
/// warning. The reason string is asserted as the one `assess` produces, and the
/// status bar's `ro` marker with it — a capability nothing surfaces is one the
/// user can't act on.
#[test]
fn an_archive_with_a_skipped_member_mounts_read_only_and_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        {
            let file = std::fs::File::create(&archive).unwrap();
            let mut w = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
            w.start_file("README.md", opts).unwrap();
            w.write_all(b"# pkg\n").unwrap();
            // Rejected by `index::normalize` on the way in — the member exists in
            // the container and cannot exist in the mount.
            w.start_file("../escape.txt", opts).unwrap();
            w.write_all(b"nope\n").unwrap();
            w.finish().unwrap();
        }
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));

        let mount = app.state.mounts.get(&archive).expect("mounted");
        assert!(
            !mount.capability.is_writable(),
            "a skipped member must demote the mount, got {:?}",
            mount.capability
        );
        match &mount.capability {
            crate::archive::Capability::ReadOnly(why) => assert!(
                why.contains("unsafe paths"),
                "the reason has to be the one assess derived: {why}"
            ),
            crate::archive::Capability::ReadWrite => panic!("expected ReadOnly, got ReadWrite"),
        }

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
        assert!(
            rendered.contains("zip ro"),
            "the status bar has to show the mount is read-only"
        );

        // And the demotion is load-bearing: the write is refused.
        if let Some(mount) = app.state.mounts.get_mut(&archive) {
            mount.journal.delete("README.md");
        }
        let before = std::fs::read(&archive).unwrap();
        assert!(app.cmd_archive("write").is_empty(), "no write is attempted");
        assert_eq!(std::fs::read(&archive).unwrap(), before);
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
