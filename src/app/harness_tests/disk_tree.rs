//! `L` on a directory, pressed as keys: the tree of sizes beneath it fills in
//! as the walk lands, and the `z` keys open and close it.

use super::*;

use std::path::Path;
use std::time::{Duration, Instant};

/// Press each of `keys`, running what each asks for as the loop would.
fn press(app: &mut App, keys: &str) {
    for c in keys.chars() {
        let effects = app.handle_key(key(c)).unwrap();
        super::process_cwd::run(app, effects);
    }
}

fn cursor_to(app: &mut App, name: &str) {
    let at = app
        .state
        .cur()
        .rows
        .iter()
        .position(|row| row.path.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("{name} is not listed"));
    app.state.cur_mut().cursor.index = at;
}

/// The pager's title and text once the walk has finished.
fn walked(app: &mut App) -> (String, String) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        app.drain_pager_stream();
        let pager = app.view.project().pager.as_ref().expect("a pager is open");
        if !pager.streaming {
            return shown(app);
        }
        assert!(Instant::now() < deadline, "the walk never finished");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn shown(app: &App) -> (String, String) {
    let pager = app.view.project().pager.as_ref().expect("a pager is open");
    let text = pager
        .lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (pager.title.clone(), text)
}

fn fixture(root: &Path) {
    std::fs::create_dir_all(root.join("a/b/c")).unwrap();
    std::fs::write(root.join("a/b/c/deep.bin"), vec![1u8; 64 * 1024]).unwrap();
    std::fs::write(root.join("a/near.txt"), b"near").unwrap();
    std::fs::write(root.join("top.txt"), b"top").unwrap();
}

#[test]
fn l_on_a_directory_shows_its_tree_and_z_keys_open_and_close_it() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fixture(&home.join("tree"));
    std::fs::write(home.join("plain.txt"), b"x").unwrap();
    let mut app = App::test_app(home);
    app.state.refresh_listing();
    cursor_to(&mut app, "tree");
    press(&mut app, "L");
    let (title, text) = walked(&mut app);
    assert!(title.contains("3 files"), "{title}");
    assert!(title.contains("depth 2"), "{title}");
    assert!(text.contains("a/") && text.contains("b/ ▸"), "{text}");
    assert!(!text.contains("c/"), "depth 2 leaves b closed:\n{text}");
    assert!(
        text.find("a/") < text.find("top.txt"),
        "largest first:\n{text}"
    );

    press(&mut app, "zr");
    let (title, text) = shown(&app);
    assert!(title.contains("depth 3"), "{title}");
    assert!(text.contains("c/") && !text.contains("deep.bin"), "{text}");

    press(&mut app, "zR");
    let (title, text) = shown(&app);
    assert!(title.contains("every level"), "{title}");
    assert!(text.contains("deep.bin"), "{text}");

    // A level closing over the row at the top of the view leaves its
    // directory there.
    let deep = text.lines().position(|l| l.contains("deep.bin")).unwrap();
    let pager = app.view.project_mut().pager.as_mut().unwrap();
    pager.last_viewport_h.set(2);
    pager.scroll = deep;
    press(&mut app, "zm");
    let (_, text) = shown(&app);
    let pager = app.view.project().pager.as_ref().unwrap();
    assert!(
        text.lines().nth(pager.scroll).unwrap().contains("c/"),
        "c takes deep.bin's place at the top:\n{text}"
    );

    press(&mut app, "zM");
    let (_, text) = shown(&app);
    assert!(text.contains("a/ ▸") && !text.contains("b/"), "{text}");

    // `za` opens the directory at the top of a view too short to hold it all.
    let line = text.lines().position(|l| l.contains("a/ ▸")).unwrap();
    let pager = app.view.project_mut().pager.as_mut().unwrap();
    pager.last_viewport_h.set(2);
    pager.scroll = line;
    press(&mut app, "za");
    let (_, text) = shown(&app);
    assert!(text.contains("b/ ▸") && text.contains("near.txt"), "{text}");
    let pager = app.view.project().pager.as_ref().unwrap();
    assert!(
        text.lines().nth(pager.scroll).unwrap().contains("a/"),
        "a stays at the top of the view"
    );
}
