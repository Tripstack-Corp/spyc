//! Moving across the boundary: the container as a file in its own
//! directory, coming back inside by mark, session or harpoon, and an
//! archive inside an archive.

use super::*;

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
