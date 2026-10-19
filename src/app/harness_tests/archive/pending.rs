//! Pending changes: an edit or an added member is recorded against the
//! container, shown as such, and carried into the next write.

use super::*;

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
            .project()
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
            .project()
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

/// A put file's row must describe the file, not a placeholder.
///
/// The row's size/mtime come from `mount.staged`, which is filled from the index
/// (an addition isn't in it) and from the materialize outcome (a put goes through
/// `Effect::FileOp(Copy)` and never produces one). So every put member listed as
/// `size = 0`, `mtime = epoch` — indistinguishable from an empty file, in the one
/// listing where the user has no on-disk row to compare against.
#[test]
fn a_put_member_lists_its_real_size_and_mtime() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let staging = tmp.path().join("staging");
        let mut app = App::test_app(dir);
        let body = b"payload with a length worth reporting";
        put_a_file_into(&mut app, &archive, &staging, "brought.txt", body);

        let entry = app
            .state
            .cur()
            .listing
            .entries
            .iter()
            .find(|e| e.name == "brought.txt")
            .expect("the put file lists");
        assert_eq!(entry.size, body.len() as u64, "size");
        assert_ne!(
            entry.mtime,
            std::time::SystemTime::UNIX_EPOCH,
            "mtime is the epoch placeholder"
        );
    });
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
                .project()
                .pager
                .as_ref()
                .expect("the put member is paged")
                .source_path
                .as_deref(),
            Some(staging.join("brought.txt").as_path()),
            "reading the staged copy the put wrote"
        );
        app.view.project_mut().pager = None;

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
            app.file_archive_outcome(outcome);
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

// ── a write in flight owns its archive ───────────────────────────────────────

fn writes(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::Archive(ArchiveOp::Write { .. })))
}

fn being_written(app: &App) -> bool {
    app.flash_text()
        .is_some_and(|f| f.contains("being written"))
}

/// A zip mounted with the column inside and `README.md` marked for removal,
/// and the write of that change started but not landed: its op is returned.
fn writing(test: impl FnOnce(&mut App, &Path, Vec<Effect>)) {
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
        let write = app.cmd_archive("write");
        assert!(writes(&write), "{write:?}");
        test(&mut app, &archive, write);
    });
}

/// A second `:archive write` while the first runs is refused. Each write
/// re-reads the archive once it lands, and the second re-read empties the
/// staging tree after the first has landed, taking an edit saved in between
/// with it.
#[test]
fn a_second_write_while_one_runs_is_refused() {
    writing(|app, archive, write| {
        let second = app.cmd_archive("write");
        assert!(!writes(&second), "a second write started: {second:?}");
        assert!(being_written(app), "{:?}", app.flash_text());
        settle(app, write);
        assert!(
            !member_names(archive).contains(&"README.md".to_string()),
            "the first write never landed: {:?}",
            member_names(archive)
        );
    });
}

/// A change to an archive while it is written is refused: the write lands by
/// clearing the journal, so a change recorded after it started would vanish
/// with no word.
#[test]
fn a_change_while_the_archive_is_written_is_refused() {
    writing(|app, archive, write| {
        settle(
            app,
            vec![Effect::Graveyard(
                crate::app::graveyard_ops::GraveyardOp::Archive {
                    paths: vec![archive.join("src").join("main.rs")],
                },
            )],
        );
        assert!(being_written(app), "{:?}", app.flash_text());
        settle(app, write);
    });
}

/// Discarding an archive's changes while they are written is refused: it would
/// delete staged copies the write is reading.
#[test]
fn discarding_while_the_archive_is_written_is_refused() {
    writing(|app, archive, write| {
        let _ = app.cmd_archive("discard");
        assert!(being_written(app), "{:?}", app.flash_text());
        assert!(app.mount_is_dirty(archive), "the changes were discarded");
        settle(app, write);
    });
}

/// Unmounting an archive while it is written is refused before it asks
/// anything: either answer to "write it first?" acts on an archive whose
/// write has not landed.
#[test]
fn unmounting_while_the_archive_is_written_is_refused() {
    writing(|app, archive, write| {
        let _ = app.cmd_archive("unmount");
        assert!(
            !matches!(app.state.mode, Mode::Prompting(_)),
            "it asked to write or discard"
        );
        assert!(being_written(app), "{:?}", app.flash_text());
        assert!(app.state.mounts.contains(archive));
        settle(app, write);
    });
}

/// Putting a file into an archive while it is written is refused, as any
/// change to it is.
#[test]
fn putting_a_file_into_the_archive_while_it_is_written_is_refused() {
    writing(|app, archive, write| {
        let staging = app.state.mounts.get(archive).unwrap().staging_root.clone();
        put_a_file_into(app, archive, &staging, "brought.txt", b"brought\n");
        assert!(being_written(app), "{:?}", app.flash_text());
        settle(app, write);
    });
}

/// Once a write has landed the archive is free again: the next change and
/// write go ahead.
#[test]
fn a_write_that_has_landed_lets_the_next_one_start() {
    writing(|app, archive, write| {
        settle(app, write);
        settle(
            app,
            vec![Effect::Graveyard(
                crate::app::graveyard_ops::GraveyardOp::Archive {
                    paths: vec![archive.join("src").join("main.rs")],
                },
            )],
        );
        let next = app.cmd_archive("write");
        assert!(writes(&next), "the next write was refused: {next:?}");
    });
}

/// An edit saved to an extracted copy while the archive is written, by an
/// editor left open or an agent, isn't in that write, and can't be refused:
/// spyc never sees it coming. A zip's untouched members are carried across
/// from the archive itself, and the write's re-read empties the staging tree,
/// so the edited copy is kept aside and put back, pending, once the archive
/// has been read again.
#[test]
#[ignore = "red: an edit saved while the archive is written is lost when the write lands"]
fn an_edit_saved_while_the_archive_is_written_is_kept_pending() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.zip");
        build_zip(&archive);
        let mut app = App::test_app(dir);
        let fx = app.request_mount(&archive, false);
        settle(&mut app, fx);
        let readme = archive.join("README.md");
        let fx = app.open_member(
            &readme,
            crate::app::archive_ops::MaterializeThen::OpenPager(
                crate::app::file_ops::PagerDest::Overlay { scroll: None },
            ),
        );
        settle(&mut app, fx);
        settle(
            &mut app,
            vec![Effect::Graveyard(
                crate::app::graveyard_ops::GraveyardOp::Archive {
                    paths: vec![archive.join("src").join("main.rs")],
                },
            )],
        );
        let write = app.cmd_archive("write");
        assert!(writes(&write), "{write:?}");
        let edited = "# edited while the archive was written\n";
        let staged = app
            .state
            .mounts
            .get(&archive)
            .unwrap()
            .staging_root
            .join("README.md");
        assert!(staged.exists(), "README.md was never extracted");
        std::fs::write(&staged, edited).unwrap();
        settle(&mut app, write);
        assert!(
            !member_names(&archive).contains(&"src/main.rs".to_string()),
            "the write itself never landed: {:?}",
            member_names(&archive)
        );
        let mount = app.state.mounts.get(&archive).expect("read again");
        assert_eq!(
            std::fs::read_to_string(mount.staging_root.join("README.md")).ok(),
            Some(edited.to_string()),
            "the edit was lost"
        );
        assert!(app.mount_is_dirty(&archive), "the edit isn't pending");
        assert!(
            app.flash_text().is_some_and(|f| f.contains("README.md")),
            "{:?}",
            app.flash_text()
        );
    });
}

/// A compressed tar's untouched members are repacked from their staged
/// copies, so an edit saved before the write read it is in the archive: it is
/// written, and is not pending again once the archive has been read back.
#[test]
fn an_edit_a_streamed_write_carried_is_not_pending_again() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().to_path_buf();
        let archive = dir.join("pkg.tar.gz");
        build_tar_gz(&archive);
        let mut app = App::test_app(dir);
        let fx = app.request_mount(&archive, false);
        settle(&mut app, fx);
        settle(
            &mut app,
            vec![Effect::Graveyard(
                crate::app::graveyard_ops::GraveyardOp::Archive {
                    paths: vec![archive.join("src").join("main.rs")],
                },
            )],
        );
        let write = app.cmd_archive("write");
        assert!(writes(&write), "{write:?}");
        let edited = "# edited before the write read it\n";
        let staged = app
            .state
            .mounts
            .get(&archive)
            .unwrap()
            .staging_root
            .join("README.md");
        std::fs::write(&staged, edited).unwrap();
        settle(&mut app, write);
        let mount = app.state.mounts.get(&archive).expect("read again");
        assert_eq!(
            std::fs::read_to_string(mount.staging_root.join("README.md")).ok(),
            Some(edited.to_string()),
            "the write didn't carry the edit"
        );
        assert!(
            !app.mount_is_dirty(&archive),
            "an edit the write carried is pending again"
        );
    });
}
