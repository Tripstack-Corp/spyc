//! `D` on a member of a mounted archive opens what `D` opens on a file on disk
//! holding the same bytes: the top-pane pager, read from the member's
//! extracted copy, or `$PAGER` for one too big to load. The archive file
//! itself still pages as its raw bytes.

use super::*;

use crate::app::harness_tests::process_cwd::wait_until_in;
use crate::ui::pager::Mount;
use ratatui::text::Line;

const NOTES: (&str, &[u8]) = ("notes.txt", b"first line\nsecond line\n");
const BLOB: (&str, &[u8]) = (
    "blob.bin",
    &[
        0x00, 0x9f, 0xff, 0x01, 0x00, 0xfe, 0x80, 0x00, 0x7f, 0x10, 0x00, 0xc3,
    ],
);
const GUIDE: (&str, &[u8]) = (
    "guide.md",
    b"# Guide\n\nThis paragraph runs well past the width of one column of a \
      split, so where it wraps depends on the width the pager was given.\n",
);

/// `pkg.zip` holding `files` as members, the same files beside it on disk,
/// an app whose column is inside the archive, and one whose column lists the
/// directory holding it.
struct Fixture {
    root: PathBuf,
    archive: PathBuf,
    staging: PathBuf,
    inside: App,
    beside: App,
}

fn with_files(files: &[(&str, &[u8])], test: impl FnOnce(&mut Fixture)) {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    crate::state::with_state_root(&root.join("state"), || {
        let archive = root.join("pkg.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
        for (name, body) in files {
            w.start_file(*name, opts).unwrap();
            w.write_all(body).unwrap();
            std::fs::write(root.join(name), body).unwrap();
        }
        w.finish().unwrap();
        let staging = root.join("staging");
        let mut inside = App::test_app(root.clone());
        mount_inline(&mut inside, &archive, &staging);
        assert_eq!(inside.state.cur().listing.dir, archive, "in the mount");
        let mut beside = App::test_app(root.clone());
        beside.state.refresh_listing();
        test(&mut Fixture {
            root,
            archive,
            staging,
            inside,
            beside,
        });
    });
}

/// `D` on `path`, a row of the column, run to the end.
fn press_d_on(app: &mut App, path: &Path) {
    app.state.focus_on_path(path);
    let col = app.state.cur();
    assert_eq!(
        col.rows.get(col.cursor.index).map(|r| r.path.as_path()),
        Some(path),
        "the cursor is on {}",
        path.display()
    );
    let fx = app.handle_key(key('D')).unwrap();
    settle(app, fx);
}

/// `D` on `path`; the pager it opened in the top pane, which has the keys.
fn page_with_d<'a>(app: &'a mut App, path: &Path) -> &'a PagerView {
    press_d_on(app, path);
    assert_eq!(
        app.state.active_project().focus,
        state::Focus::Pager(Mount::TopPane),
        "`D` on {} put no pager in the top pane: {:?}",
        path.display(),
        app.flash_text()
    );
    let view = app
        .view
        .project()
        .pager
        .as_ref()
        .expect("the focused pager");
    assert!(matches!(view.mount, Mount::TopPane), "{:?}", view.mount);
    view
}

type Shown = (String, Vec<Line<'static>>, Option<Vec<Line<'static>>>);

/// What a pager shows: its title and lines, in both of its views.
fn shown(view: &PagerView) -> Shown {
    (
        view.title.clone(),
        view.lines.clone(),
        view.alt_lines.clone(),
    )
}

/// `D` on the member `name` and on the file of the same bytes beside the
/// archive: what each pager shows.
fn member_and_file(fx: &mut Fixture, name: &str) -> (Shown, Shown) {
    let member = shown(page_with_d(&mut fx.inside, &fx.archive.join(name)));
    let file = shown(page_with_d(&mut fx.beside, &fx.root.join(name)));
    (member, file)
}

/// Guard: `D` on a file on disk pages that file in the top pane.
#[test]
fn d_on_a_file_on_disk_pages_it_in_the_top_pane() {
    with_files(&[NOTES], |fx| {
        let path = fx.root.join(NOTES.0);
        let view = page_with_d(&mut fx.beside, &path);
        assert_eq!(view.source_path.as_deref(), Some(path.as_path()));
        let text: Vec<String> = view.lines.iter().map(ToString::to_string).collect();
        assert_eq!(text, ["first line", "second line"]);
    });
}

/// `D` on a text member pages its extracted copy, as `D` pages the same
/// file on disk.
#[test]
#[ignore = "red: `D` reads the member's archive path, which names no file, and flashes `hex: Not a directory`"]
fn d_on_a_text_member_pages_its_extracted_copy() {
    with_files(&[NOTES], |fx| {
        let (member, file) = member_and_file(fx, NOTES.0);
        assert_eq!(member, file);

        let view = fx.inside.view.project().pager.as_ref().expect("paged");
        let source = view.source_path.clone().expect("a file-backed pager");
        assert!(
            source.starts_with(&fx.staging),
            "the extracted copy: {}",
            source.display()
        );
        assert_eq!(std::fs::read(&source).unwrap(), NOTES.1);
    });
}

/// A binary member gets the hex view a binary file on disk gets.
#[test]
#[ignore = "red: `D` reads the member's archive path, which names no file, and flashes `hex: Not a directory`"]
fn d_on_a_binary_member_shows_the_hex_view_of_its_bytes() {
    with_files(&[BLOB], |fx| {
        let (member, file) = member_and_file(fx, BLOB.0);
        assert!(member.0.ends_with("[hex]"), "a hex view: {}", member.0);
        assert_eq!(member, file);
    });
}

/// In a split, `D` wraps a markdown member to the focused column's width, as
/// it wraps the file on disk.
#[test]
#[ignore = "red: `D` reads the member's archive path, which names no file, and flashes `hex: Not a directory`"]
fn d_on_a_markdown_member_in_a_split_wraps_to_the_column() {
    with_files(&[GUIDE], |fx| {
        for app in [&mut fx.inside, &mut fx.beside] {
            app.view.term_size = (64, 24);
            app.state.active_project_mut().vsplit = Some(state::VSplit {
                width_pct: 50,
                mode: state::VsplitMode::TopOnly,
                focus: state::Side::Left,
            });
        }
        let (member, file) = member_and_file(fx, GUIDE.0);
        assert_eq!(member, file);
    });
}

/// A member too big for the in-app pager goes to `$PAGER` in the top overlay,
/// as a file that size does, started beside the archive.
#[test]
#[ignore = "red: `D` reads the member's archive path, which names no file, and flashes `hex: Not a directory`"]
fn d_on_a_member_too_big_for_the_pager_hands_it_to_dollar_pager() {
    let line = [b'x'; 79].iter().chain(b"\n").copied().collect::<Vec<u8>>();
    let rows = usize::try_from(crate::fs::ops::MAX_PAGER_BYTES).unwrap() / line.len() + 1;
    let big = line.repeat(rows);
    with_files(&[("big.log", &big)], |fx| {
        crate::shell::with_pager("tail -f", || {
            press_d_on(&mut fx.beside, &fx.root.join("big.log"));
            press_d_on(&mut fx.inside, &fx.archive.join("big.log"));
        });
        for (app, which) in [(&fx.beside, "the file"), (&fx.inside, "the member")] {
            assert!(
                app.view.project().pager.is_none(),
                "{which}: no in-app pager"
            );
            assert_eq!(
                app.state.active_project().focus,
                state::Focus::Overlay,
                "{which}: `$PAGER` has the keys: {:?}",
                app.flash_text()
            );
        }
        let pid = fx
            .inside
            .runtime
            .project()
            .top_overlay
            .as_ref()
            .expect("the member's `$PAGER`")
            .process_id()
            .expect("it runs");
        wait_until_in(pid, &fx.root);
    });
}

/// The pager's `v` on a member `D` paged edits the extracted copy in the top
/// overlay, started beside the archive, and the mount watches the copy.
#[test]
#[ignore = "red: `D` reads the member's archive path, which names no file, and flashes `hex: Not a directory`"]
fn v_in_the_pager_d_opened_on_a_member_edits_its_extracted_copy() {
    with_files(&[NOTES], |fx| {
        page_with_d(&mut fx.inside, &fx.archive.join(NOTES.0));
        crate::shell::with_editor("tail -f", || {
            fx.inside.handle_key(key('v')).unwrap();
        });

        let pid = fx
            .inside
            .runtime
            .project()
            .top_overlay
            .as_ref()
            .unwrap_or_else(|| panic!("no editor opened: {:?}", fx.inside.flash_text()))
            .process_id()
            .expect("the editor runs");
        wait_until_in(pid, &fx.root);
        let mount = fx.inside.state.mounts.get(&fx.archive).expect("mounted");
        assert_eq!(mount.editing, [NOTES.0], "the copy is watched for edits");
    });
}

/// Guard: `D` on the mounted archive itself, from the directory holding it,
/// pages the archive's own bytes.
#[test]
fn d_on_a_mounted_archive_pages_its_raw_bytes() {
    with_files(&[NOTES], |fx| {
        let effects = fx.inside.apply(&Action::Climb).unwrap();
        apply_effects(&mut fx.inside, effects);
        assert_eq!(fx.inside.state.cur().listing.dir, fx.root);
        assert!(
            fx.inside.state.mounts.contains(&fx.archive),
            "still mounted"
        );

        let view = page_with_d(&mut fx.inside, &fx.archive);
        assert_eq!(view.title, "pkg.zip [hex]");
    });
}
