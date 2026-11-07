//! A read-only mount refuses an edit when it is made, with the reason the mount
//! can't be written, and journals nothing; reading, extracting and copying out
//! still work. A writable mount takes the same edits.

use super::*;

/// `pkg.tar` holding `README.md` twice beside `src/main.rs`. The duplicate
/// makes the mount read-only: a repack would drop the shadowed copy.
pub(super) fn build_tar_with_a_duplicate(path: &Path) {
    let mut b = tar::Builder::new(std::fs::File::create(path).unwrap());
    for (name, body) in [
        ("README.md", "# first\n"),
        ("README.md", "# second\n"),
        ("src/main.rs", "fn main() {}\n"),
    ] {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(1_000_000);
        h.set_entry_type(tar::EntryType::Regular);
        b.append_data(&mut h, name, body.as_bytes()).unwrap();
    }
    b.into_inner().unwrap();
}

/// The words the capability gives as its reason, which every refusal carries.
const REASON: &str = "duplicate member name";

/// What a mount fixture hands its test: the app with the column at the
/// archive's root and the cursor on `README.md`, the archive, and its staging
/// tree.
struct Fixture {
    app: App,
    archive: PathBuf,
    staging: PathBuf,
}

/// Run `test` against a mount of the read-only tar.
fn read_only(test: impl FnOnce(Fixture)) {
    mounted("pkg.tar", build_tar_with_a_duplicate, |fx| {
        let mount = fx.app.state.mounts.get(&fx.archive).expect("mounted");
        assert!(
            mount
                .capability
                .reason()
                .is_some_and(|why| why.contains(REASON)),
            "the fixture must mount read-only for its duplicate: {:?}",
            mount.capability
        );
        test(fx);
    });
}

/// Run `test` against a mount of an ordinary, writable zip.
fn writable(test: impl FnOnce(Fixture)) {
    mounted("pkg.zip", build_zip, |fx| {
        let mount = fx.app.state.mounts.get(&fx.archive).expect("mounted");
        assert!(mount.capability.is_writable(), "{:?}", mount.capability);
        test(fx);
    });
}

fn mounted(name: &str, build: fn(&Path), test: impl FnOnce(Fixture)) {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        // Canonical, as a climb out of the archive lands on the canonical path.
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join(name);
        build(&archive);
        let staging = tmp.path().join("staging");
        let mut app = App::test_app(dir);
        mount_inline(&mut app, &archive, &staging);
        app.state.focus_on_path(&archive.join("README.md"));
        test(Fixture {
            app,
            archive,
            staging,
        });
    });
}

/// Type `keys` as the user would, one key at a time, gathering the effects.
/// The record of which keys were used is left out: it changes nothing here.
fn press(app: &mut App, keys: &str) -> Vec<Effect> {
    keys.chars()
        .flat_map(|c| app.handle_key(key(c)).unwrap())
        .filter(|e| !matches!(e, Effect::SaveLearned(_)))
        .collect()
}

fn press_enter(app: &mut App) -> Vec<Effect> {
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
        .unwrap()
        .into_iter()
        .filter(|e| !matches!(e, Effect::SaveLearned(_)))
        .collect()
}

/// What the user was told last: the status bar's flash, or an open pager's.
fn told(app: &App) -> String {
    app.flash_text()
        .map(str::to_string)
        .or_else(|| {
            app.view
                .project()
                .pager
                .as_ref()
                .and_then(|v| v.flash.clone())
        })
        .unwrap_or_default()
}

/// The keys of the menu `.` opens on the cursor's entry.
fn menu_keys(app: &mut App) -> Vec<String> {
    let fx = press(app, ".");
    assert!(fx.is_empty(), "{fx:?}");
    let hint = app.view.chord_hint.as_ref().expect("the menu is open");
    hint.rows.iter().map(|(keys, _)| keys.clone()).collect()
}

/// Yank the real file `name` beside the archive into the inventory.
fn yank_a_real_file(app: &mut App, archive: &Path, name: &str) {
    let source = archive.parent().unwrap().join(name);
    std::fs::write(&source, b"brought in\n").unwrap();
    settle(
        app,
        vec![Effect::Inventory(
            crate::app::inventory_ops::InventoryOp::Yank {
                sources: vec![crate::app::inventory_ops::YankSource::plain(source)],
            },
        )],
    );
    assert_eq!(app.state.inventory.items().count(), 1, "the yank landed");
}

fn assert_refused_and_clean(fx: &Fixture, what: &str) {
    let said = told(&fx.app);
    assert!(
        said.contains("read-only") && said.contains(REASON),
        "{what} must be refused with the mount's reason, said {said:?}"
    );
    assert!(
        !fx.app.mount_is_dirty(&fx.archive),
        "{what} must journal nothing"
    );
    assert!(
        !matches!(fx.app.state.mode, Mode::Prompting(_)),
        "{what} must be refused before any prompt"
    );
}

// ── the keys ─────────────────────────────────────────────────────────────

#[test]
fn deleting_a_member_of_a_read_only_mount_is_refused_with_the_reason() {
    for keys in ["dd", "R"] {
        read_only(|mut fx| {
            let effects = press(&mut fx.app, keys);
            assert!(effects.is_empty(), "{keys}: {effects:?}");
            assert_refused_and_clean(&fx, keys);
        });
    }
}

#[test]
fn moving_or_renaming_a_member_of_a_read_only_mount_is_refused_with_the_reason() {
    read_only(|mut fx| {
        let effects = press(&mut fx.app, "M");
        assert!(effects.is_empty(), "{effects:?}");
        assert_refused_and_clean(&fx, "M");
    });
}

#[test]
fn putting_into_a_read_only_mount_is_refused_with_the_reason() {
    read_only(|mut fx| {
        let archive = fx.archive.clone();
        yank_a_real_file(&mut fx.app, &archive, "brought.txt");
        let effects = press(&mut fx.app, "p");
        assert!(effects.is_empty(), "{effects:?}");
        assert_refused_and_clean(&fx, "p");
    });
}

/// Copying a file in from outside the mount: `c` on a real file, the archive
/// typed as its destination.
#[test]
fn copying_into_a_read_only_mount_is_refused_with_the_reason() {
    read_only(|mut fx| {
        let _cwd = CwdGuard;
        let dir = fx.archive.parent().unwrap().to_path_buf();
        std::fs::write(dir.join("brought.txt"), b"brought in\n").unwrap();
        let effects = press(&mut fx.app, "-");
        settle(&mut fx.app, effects);
        assert_eq!(fx.app.state.cur().listing.dir, dir, "out of the archive");
        fx.app.state.refresh_listing();
        fx.app.state.focus_on_path(&dir.join("brought.txt"));

        let mut effects = press(&mut fx.app, "cpkg.tar");
        effects.extend(press_enter(&mut fx.app));
        settle(&mut fx.app, effects);

        assert_refused_and_clean(&fx, "c into the archive");
        assert!(
            !fx.staging.join("brought.txt").exists(),
            "nothing is staged for a refused copy"
        );
    });
}

/// `e` and `V` hand a member to an editor, whose save the journal would record.
#[test]
fn editing_a_member_of_a_read_only_mount_is_refused_with_the_reason() {
    for keys in ["e", "v", "V"] {
        read_only(|mut fx| {
            let effects = press(&mut fx.app, keys);
            assert!(effects.is_empty(), "{keys}: {effects:?}");
            assert_refused_and_clean(&fx, keys);
            let mount = fx.app.state.mounts.get(&fx.archive).unwrap();
            assert!(mount.editing.is_empty(), "{keys}: nothing is watched");
        });
    }
}

/// The pager's `v` edits the extracted copy it is showing.
#[test]
fn editing_from_the_pager_in_a_read_only_mount_is_refused_with_the_reason() {
    read_only(|mut fx| {
        let effects = press_enter(&mut fx.app);
        settle(&mut fx.app, effects);
        assert!(
            fx.app.view.project().pager.is_some(),
            "Enter reads the member: {:?}",
            told(&fx.app)
        );

        let effects = press(&mut fx.app, "v");
        assert!(effects.is_empty(), "{effects:?}");
        assert!(
            fx.app.view.project().pager.is_some(),
            "the pager stays open"
        );
        assert_refused_and_clean(&fx, "the pager's v");
        let mount = fx.app.state.mounts.get(&fx.archive).unwrap();
        assert!(mount.editing.is_empty(), "nothing is watched");
    });
}

/// `e` on a directory enters it, which is not an edit.
#[test]
fn entering_a_directory_of_a_read_only_mount_with_e_still_works() {
    read_only(|mut fx| {
        let src = fx.archive.join("src");
        fx.app.state.focus_on_path(&src);
        let effects = press(&mut fx.app, "e");
        settle(&mut fx.app, effects);
        assert_eq!(fx.app.state.cur().listing.dir, src, "{:?}", told(&fx.app));
    });
}

// ── the menu ─────────────────────────────────────────────────────────────

#[test]
fn the_entry_menu_in_a_read_only_mount_leaves_the_edits_out() {
    read_only(|mut fx| {
        let keys = menu_keys(&mut fx.app);
        for edit in ["e", "V", "M", "dd", "O", "+"] {
            assert!(!keys.iter().any(|k| k == edit), "{edit} offered: {keys:?}");
        }
        for read in ["Enter", "D", "yy", "c"] {
            assert!(keys.iter().any(|k| k == read), "{read} missing: {keys:?}");
        }
    });
}

#[test]
fn the_entry_menu_in_a_writable_mount_offers_the_edits() {
    writable(|mut fx| {
        let keys = menu_keys(&mut fx.app);
        for edit in ["e", "V", "M", "dd", "Enter", "c"] {
            assert!(keys.iter().any(|k| k == edit), "{edit} missing: {keys:?}");
        }
    });
}

// ── what still works, and what a writable mount still takes ──────────────

/// Reading, extracting and copying out are not edits.
#[test]
fn a_read_only_mount_still_reads_extracts_and_copies_out() {
    read_only(|mut fx| {
        let effects = press(&mut fx.app, "yy");
        settle(&mut fx.app, effects);
        let item = fx
            .app
            .state
            .inventory
            .items()
            .find(|i| i.filename == "README.md")
            .expect("yy extracts the member into the inventory");
        assert_eq!(
            fx.app.state.inventory.read_content(&item.id).as_deref(),
            Some(b"# second\n".as_slice()),
            "the copy the archive shows"
        );

        let out = fx.archive.parent().unwrap().join("out");
        std::fs::create_dir(&out).unwrap();
        let mut effects = press(&mut fx.app, "c");
        assert!(
            matches!(fx.app.state.mode, Mode::Prompting(_)),
            "c asks where to: {:?}",
            told(&fx.app)
        );
        effects.extend(press(&mut fx.app, &out.display().to_string()));
        effects.extend(press_enter(&mut fx.app));
        settle(&mut fx.app, effects);
        assert_eq!(
            std::fs::read(out.join("README.md")).unwrap(),
            b"# second\n",
            "c copies the member out"
        );
        assert!(!fx.app.mount_is_dirty(&fx.archive));
    });
}

/// `D` reads a member, so a read-only mount pages its extracted copy in the
/// top pane.
#[test]
fn a_read_only_mount_pages_a_member_with_d() {
    read_only(|mut fx| {
        let effects = press(&mut fx.app, "D");
        settle(&mut fx.app, effects);
        let view = fx
            .app
            .view
            .project()
            .pager
            .as_ref()
            .unwrap_or_else(|| panic!("`D` opened no pager: {:?}", told(&fx.app)));
        assert!(
            matches!(view.mount, crate::ui::pager::Mount::TopPane),
            "{:?}",
            view.mount
        );
        let source = view.source_path.clone().expect("a file-backed pager");
        assert!(source.starts_with(&fx.staging), "{}", source.display());
        assert_eq!(std::fs::read(source).unwrap(), b"# second\n");
        assert!(!fx.app.mount_is_dirty(&fx.archive));
    });
}

/// The same edits in a writable mount reach their prompts and the journal.
#[test]
fn a_writable_mount_takes_the_same_edits() {
    for keys in ["dd", "M"] {
        writable(|mut fx| {
            let effects = press(&mut fx.app, keys);
            assert!(effects.is_empty(), "{keys}: {effects:?}");
            assert!(
                matches!(fx.app.state.mode, Mode::Prompting(_)),
                "{keys} asks first: {:?}",
                told(&fx.app)
            );
        });
    }
    writable(|mut fx| {
        let effects = press(&mut fx.app, "e");
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::Archive(ArchiveOp::Materialize { .. }))),
            "e extracts the member for the editor: {effects:?}"
        );
    });
    writable(|mut fx| {
        let archive = fx.archive.clone();
        yank_a_real_file(&mut fx.app, &archive, "brought.txt");
        let effects = press(&mut fx.app, "p");
        settle(&mut fx.app, effects);
        assert!(
            fx.app.mount_is_dirty(&archive),
            "p puts: {:?}",
            told(&fx.app)
        );
        assert!(fx.staging.join("brought.txt").exists());
    });
}

// ── the screen every edit passes through ─────────────────────────────────

/// Each change the journal records, as the effect that would record it: a
/// delete, a rename, a put and a copy in. The screen refuses every one for a
/// read-only mount, whatever route the effect took to it.
fn journaled_edits(
    archive: &Path,
    real: &Path,
    inventory: Vec<String>,
) -> Vec<(&'static str, Effect)> {
    use crate::app::file_ops::FileOp;
    vec![
        (
            "delete",
            Effect::Graveyard(crate::app::graveyard_ops::GraveyardOp::Archive {
                paths: vec![archive.join("README.md")],
            }),
        ),
        (
            "rename",
            Effect::FileOp(FileOp::Move {
                paths: vec![archive.join("README.md")],
                dest: archive.join("src"),
            }),
        ),
        (
            "put",
            Effect::Inventory(crate::app::inventory_ops::InventoryOp::Put {
                dest_dir: archive.to_path_buf(),
                ids: inventory,
            }),
        ),
        (
            "copy in",
            Effect::FileOp(FileOp::Copy {
                paths: vec![real.to_path_buf()],
                dest: archive.join("src"),
            }),
        ),
    ]
}

#[test]
fn the_effect_screen_refuses_every_journaled_edit_to_a_read_only_mount() {
    for at in 0..4 {
        read_only(|mut fx| {
            let archive = fx.archive.clone();
            yank_a_real_file(&mut fx.app, &archive, "brought.txt");
            let ids = fx
                .app
                .state
                .inventory
                .items()
                .map(|i| i.id.clone())
                .collect();
            let real = archive.parent().unwrap().join("brought.txt");
            let (what, effect) = journaled_edits(&archive, &real, ids).swap_remove(at);
            fx.app.state.flash = None;
            settle(&mut fx.app, vec![effect]);
            assert_refused_and_clean(&fx, what);
        });
    }
}

#[test]
fn the_effect_screen_journals_the_same_edits_to_a_writable_mount() {
    for at in 0..4 {
        writable(|mut fx| {
            let archive = fx.archive.clone();
            yank_a_real_file(&mut fx.app, &archive, "brought.txt");
            let ids = fx
                .app
                .state
                .inventory
                .items()
                .map(|i| i.id.clone())
                .collect();
            let real = archive.parent().unwrap().join("brought.txt");
            let (what, effect) = journaled_edits(&archive, &real, ids).swap_remove(at);
            settle(&mut fx.app, vec![effect]);
            assert!(
                fx.app.mount_is_dirty(&archive),
                "{what}: {:?}",
                told(&fx.app)
            );
        });
    }
}

/// An edit saved to an extracted copy outside spyc, by an agent, can't be
/// refused when it is made. It stays pending, so it isn't lost unseen, and
/// `:archive write` refuses it with the mount's reason.
#[test]
fn an_edit_spyc_cannot_refuse_is_pending_and_the_write_refuses_it() {
    read_only(|mut fx| {
        let effects = press(&mut fx.app, "yy");
        settle(&mut fx.app, effects);
        let mount = fx.app.state.mounts.get(&fx.archive).unwrap();
        let staged = mount.staging_path(mount.index.get("README.md").unwrap());
        std::fs::write(&staged, b"# changed by an agent\n").unwrap();
        assert!(fx.app.settle_archive_edits(), "the edit is noticed");
        assert!(fx.app.mount_is_dirty(&fx.archive), "and kept pending");

        let before = std::fs::read(&fx.archive).unwrap();
        assert!(fx.app.cmd_archive("write").is_empty(), "no write starts");
        let said = told(&fx.app);
        assert!(
            said.contains("read-only") && said.contains(REASON),
            "{said}"
        );
        assert_eq!(std::fs::read(&fx.archive).unwrap(), before);
    });
}
