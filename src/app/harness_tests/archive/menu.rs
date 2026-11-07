//! The entry menu inside a mounted archive ends with the archive's own rows:
//! `:archive write` while the mount has changes it can write, `:archive
//! discard` while it has changes, and `:archive unmount`, each run as typing
//! it does. Each case opens the menu with `.` or a right-click and fires a row
//! with `Enter` on its highlight or a click, as the user would.

use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

const WRITE: &str = ":archive write";
const DISCARD: &str = ":archive discard";
const UNMOUNT: &str = ":archive unmount";
const ARCHIVE_ROWS: [&str; 3] = [WRITE, DISCARD, UNMOUNT];

/// What a mount fixture hands its test: the app with the column at the
/// archive's root and the cursor on `README.md`, the directory holding the
/// archive, and the archive.
struct Fixture {
    app: App,
    dir: PathBuf,
    archive: PathBuf,
}

fn mounted(name: &str, build: fn(&Path), test: impl FnOnce(Fixture)) {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        // Canonical, as a climb out of the archive lands on the canonical path.
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join(name);
        build(&archive);
        let mut app = App::test_app(dir.clone());
        app.view.term_size = (120, 24);
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        app.state.focus_on_path(&archive.join("README.md"));
        test(Fixture { app, dir, archive });
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

/// Run `test` against a mount of a tar that can't be written back.
fn read_only(test: impl FnOnce(Fixture)) {
    mounted(
        "pkg.tar",
        super::read_only::build_tar_with_a_duplicate,
        |fx| {
            let mount = fx.app.state.mounts.get(&fx.archive).expect("mounted");
            assert!(!mount.capability.is_writable(), "{:?}", mount.capability);
            test(fx);
        },
    );
}

fn special(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Type `keys` as the user would, one key at a time, and run what they start.
fn type_keys(app: &mut App, keys: &str) {
    let effects: Vec<Effect> = keys
        .chars()
        .flat_map(|c| app.handle_key(key(c)).unwrap())
        .collect();
    settle(app, effects);
}

/// Type `:line` and `Enter`, and hand back what it started, unrun.
fn typed_command(app: &mut App, line: &str) -> Vec<Effect> {
    let mut effects: Vec<Effect> = format!(":{line}")
        .chars()
        .flat_map(|c| app.handle_key(key(c)).unwrap())
        .collect();
    effects.extend(app.handle_key(special(KeyCode::Enter)).unwrap());
    effects
}

/// Delete `README.md` with `dd`, answered `y`: a pending change.
fn delete_the_readme(fx: &mut Fixture) {
    fx.app.state.focus_on_path(&fx.archive.join("README.md"));
    type_keys(&mut fx.app, "ddy");
    assert!(
        fx.app.mount_is_dirty(&fx.archive),
        "premise: the delete is pending: {:?}",
        fx.app.flash_text()
    );
    assert!(!listed(&fx.app, "README.md"), "premise: the row is gone");
}

fn listed(app: &App, name: &str) -> bool {
    app.state
        .cur()
        .rows
        .iter()
        .any(|r| r.path.file_name().is_some_and(|n| n == name))
}

/// The keys of the open menu's rows, in the order they are drawn.
fn open_menu_keys(app: &App) -> Vec<String> {
    let hint = app.view.chord_hint.as_ref().expect("a menu is open");
    assert!(hint.menu.is_some(), "an object's menu, not a chord's");
    hint.rows.iter().map(|(keys, _)| keys.clone()).collect()
}

/// `.` on the cursor's entry: the keys of the menu it opens.
fn menu_keys(app: &mut App) -> Vec<String> {
    app.handle_key(key('.')).unwrap();
    open_menu_keys(app)
}

/// Which of the archive's rows `keys` holds, in its order.
fn archive_rows(keys: &[String]) -> Vec<&str> {
    keys.iter()
        .map(String::as_str)
        .filter(|k| ARCHIVE_ROWS.contains(k))
        .collect()
}

/// Highlight the open menu's row `keys` with the arrows and fire it with
/// `Enter`, then run what it started.
fn enter_on(app: &mut App, keys: &str) {
    let rows = open_menu_keys(app);
    let at = rows
        .iter()
        .position(|k| k == keys)
        .unwrap_or_else(|| panic!("no row {keys} in the menu: {rows:?}"));
    for _ in 0..=at {
        app.handle_key(special(KeyCode::Down)).unwrap();
    }
    let effects = app.handle_key(special(KeyCode::Enter)).unwrap();
    assert!(
        app.view.chord_hint.is_none(),
        "the row fired and the menu closed"
    );
    settle(app, effects);
}

fn drawn(app: &mut App) -> ratatui::buffer::Buffer {
    let (width, height) = app.view.term_size;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    terminal.backend().buffer().clone()
}

/// Where `text` is first drawn, top to bottom.
fn drawn_at(app: &mut App, text: &str) -> (u16, u16) {
    let buf = drawn(app);
    for y in 0..buf.area.height {
        let cells: Vec<&str> = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        let Some(byte) = cells.concat().find(text) else {
            continue;
        };
        let mut at = 0;
        for (x, cell) in cells.iter().enumerate() {
            if at == byte {
                return (u16::try_from(x).unwrap(), y);
            }
            at += cell.len();
        }
    }
    panic!("{text:?} is not drawn");
}

fn mouse(app: &mut App, kind: MouseEventKind, (column, row): (u16, u16)) -> Vec<Effect> {
    app.handle_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// A press and release of `button` at `at`, and what they started.
fn click(app: &mut App, button: MouseButton, at: (u16, u16)) -> Vec<Effect> {
    let mut effects = mouse(app, MouseEventKind::Down(button), at);
    effects.extend(mouse(app, MouseEventKind::Up(button), at));
    effects
}

/// Deleting a member leaves the mount with a change it can write: its menu
/// ends with all three rows, after the entry's own, and `:archive write`
/// there writes the delete into the archive on disk as typing it does.
#[test]
fn a_changed_mounts_menu_ends_with_its_rows_and_write_writes_it_back() {
    writable(|mut fx| {
        delete_the_readme(&mut fx);

        let keys = menu_keys(&mut fx.app);

        assert!(
            keys.iter().any(|k| k == "Enter"),
            "the entry's own: {keys:?}"
        );
        let tail = &keys[keys.len().saturating_sub(3)..];
        assert_eq!(tail, ARCHIVE_ROWS, "after the entry's own rows: {keys:?}");

        enter_on(&mut fx.app, WRITE);

        assert_eq!(
            member_names(&fx.archive),
            ["src/deep/mod.rs", "src/main.rs"],
            "the delete is written: {:?}",
            fx.app.flash_text()
        );
        assert!(!fx.app.mount_is_dirty(&fx.archive), "clean after the write");
    });
}

/// A clean mount has nothing to write or discard, so its menu offers only
/// `:archive unmount`; a click on it, in the menu a right-click opened,
/// leaves the archive for the directory holding it, the cursor on it.
#[test]
fn a_clean_mounts_menu_offers_only_unmount_and_a_click_on_it_leaves() {
    let _lock = crate::mouse_test_lock();
    writable(|mut fx| {
        crate::set_mouse_capture_for_test(true);
        let readme = drawn_at(&mut fx.app, "README.md");
        click(&mut fx.app, MouseButton::Right, readme);
        let keys = open_menu_keys(&fx.app);
        let unmount = (archive_rows(&keys) == [UNMOUNT]).then(|| drawn_at(&mut fx.app, UNMOUNT));
        let effects = unmount.map(|at| click(&mut fx.app, MouseButton::Left, at));
        crate::set_mouse_capture_for_test(false);

        assert_eq!(archive_rows(&keys), [UNMOUNT], "{keys:?}");
        settle(&mut fx.app, effects.unwrap_or_default());
        assert!(
            !fx.app.state.mounts.contains(&fx.archive),
            "unmounted: {:?}",
            fx.app.flash_text()
        );
        let c = fx.app.state.cur();
        assert_eq!(c.listing.dir, fx.dir, "beside the archive");
        assert_eq!(
            c.rows.get(c.cursor.index).map(|r| &r.path),
            Some(&fx.archive),
            "the cursor on it"
        );
    });
}

/// A read-only mount carries a change only from an edit saved to an
/// extracted copy outside spyc, here not yet noticed by any scan. Its menu
/// offers to discard that change and to unmount, but not to write, which
/// the mount refuses.
#[test]
fn a_changed_read_only_mounts_menu_offers_discard_and_unmount_but_not_write() {
    read_only(|mut fx| {
        type_keys(&mut fx.app, "yy");
        let mount = fx.app.state.mounts.get(&fx.archive).unwrap();
        let staged = mount.staging_path(mount.index.get("README.md").unwrap());
        std::fs::write(&staged, b"# changed by an agent\n").unwrap();
        assert!(fx.app.mount_is_dirty(&fx.archive), "premise: a change");

        let keys = menu_keys(&mut fx.app);

        assert_eq!(archive_rows(&keys), [DISCARD, UNMOUNT], "{keys:?}");
    });
}

/// `:archive discard` from the menu drops the pending delete: the member is
/// listed again, and the archive on disk is as it was.
#[test]
fn discard_on_the_menu_drops_the_change() {
    writable(|mut fx| {
        let before = std::fs::read(&fx.archive).unwrap();
        delete_the_readme(&mut fx);
        menu_keys(&mut fx.app);

        enter_on(&mut fx.app, DISCARD);

        assert!(!fx.app.mount_is_dirty(&fx.archive), "the change is gone");
        assert!(listed(&fx.app, "README.md"), "the member is back");
        assert_eq!(std::fs::read(&fx.archive).unwrap(), before);
    });
}

/// `:archive unmount` from the menu of a changed mount, under `write_back =
/// "ask"`, asks to write the changes first, as typing it does, and keeps the
/// mount until answered.
#[test]
fn unmount_on_the_menu_of_a_changed_mount_asks_to_write_first() {
    writable(|mut fx| {
        fx.app.state.active_project_mut().config.archive.write_back = crate::config::WriteBack::Ask;
        delete_the_readme(&mut fx);
        menu_keys(&mut fx.app);

        enter_on(&mut fx.app, UNMOUNT);

        assert!(
            matches!(
                &fx.app.state.mode,
                Mode::Prompting(p) if matches!(
                    &p.kind,
                    PromptKind::ArchiveWriteConfirm { archive } if *archive == fx.archive
                )
            ),
            "the write prompt is up: {:?}",
            fx.app.flash_text()
        );
        assert!(fx.app.state.mounts.contains(&fx.archive), "still mounted");
    });
}

/// Outside a mount the entry menu has none of the archive's rows, even on an
/// archive that is mounted and carries a change.
#[test]
fn outside_a_mount_the_entry_menu_has_none_of_the_archives_rows() {
    writable(|mut fx| {
        delete_the_readme(&mut fx);
        type_keys(&mut fx.app, "-");
        assert_eq!(fx.app.state.cur().listing.dir, fx.dir, "premise: out");
        fx.app.state.focus_on_path(&fx.archive);
        assert!(
            fx.app.state.mounts.contains(&fx.archive),
            "premise: mounted"
        );

        let keys = menu_keys(&mut fx.app);

        assert!(archive_rows(&keys).is_empty(), "{keys:?}");
    });
}

/// While a write of the mount runs, `:archive write`, `discard` and
/// `unmount` each refuse, so its menu offers none of them; once the write
/// lands, the clean mount's menu offers `:archive unmount` again.
#[test]
fn while_its_write_runs_the_menu_has_none_of_the_archives_rows() {
    writable(|mut fx| {
        delete_the_readme(&mut fx);
        let write = typed_command(&mut fx.app, "archive write");
        assert!(
            write
                .iter()
                .any(|e| matches!(e, Effect::Archive(ArchiveOp::Write { .. }))),
            "premise: a write starts: {write:?}"
        );

        let keys = menu_keys(&mut fx.app);
        fx.app.handle_key(special(KeyCode::Esc)).unwrap();

        assert!(archive_rows(&keys).is_empty(), "while written: {keys:?}");
        settle(&mut fx.app, write);
        assert!(!fx.app.mount_is_dirty(&fx.archive), "premise: written");
        let keys = menu_keys(&mut fx.app);
        assert_eq!(archive_rows(&keys), [UNMOUNT], "once landed: {keys:?}");
    });
}

/// "Has changes", for the menu and for `:archive write` alike, compares each
/// member spyc extracted against what it wrote there: a stat per extracted
/// member, where a stat per member of the archive made the menu of a zip of
/// 200,000 members, one of them read, cost 200,000. A file at the place of a
/// member spyc never extracted is none of those, so the menu offers only
/// `:archive unmount` and `:archive write` has nothing to write; an edit to
/// the member spyc did extract is a change both see.
#[test]
fn only_the_members_spyc_extracted_are_checked_for_changes() {
    writable(|mut fx| {
        type_keys(&mut fx.app, "yy");
        let mount = fx.app.state.mounts.get(&fx.archive).unwrap();
        let readme = mount.staging_path(mount.index.get("README.md").unwrap());
        let unread = mount.staging_path(mount.index.get("src/main.rs").unwrap());
        assert!(readme.is_file(), "premise: `yy` extracted README.md");
        assert!(!unread.exists(), "premise: src/main.rs never extracted");
        std::fs::create_dir_all(unread.parent().unwrap()).unwrap();
        std::fs::write(&unread, b"fn main() { planted() }\n").unwrap();

        let keys = menu_keys(&mut fx.app);
        fx.app.handle_key(special(KeyCode::Esc)).unwrap();
        let write = typed_command(&mut fx.app, "archive write");
        let wrote = write
            .iter()
            .any(|e| matches!(e, Effect::Archive(ArchiveOp::Write { .. })));
        settle(&mut fx.app, write);

        assert_eq!(archive_rows(&keys), [UNMOUNT], "{keys:?}");
        assert!(!wrote, "nothing to write: {:?}", fx.app.flash_text());
        std::fs::write(&readme, b"# edited by an agent\n").unwrap();
        let keys = menu_keys(&mut fx.app);
        assert_eq!(
            archive_rows(&keys),
            ARCHIVE_ROWS,
            "an extracted edit: {keys:?}"
        );
    });
}
