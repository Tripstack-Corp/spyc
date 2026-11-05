//! One archive, one mount, however its path is spelled: through a symlinked
//! directory, or with a `..` on the way. A mount is keyed by the archive's
//! canonical path, which is also the path its row has in the directory it
//! lives in, so climbing out and coming back finds the mount the user was
//! working in.

use super::*;

/// What every case here works in: `real/` holding `a.txt`, `pkg.zip` and an
/// empty `sub/`, and `link`, a symlink to `real/`. The column starts in
/// `real/`. `a.txt` sorts before `pkg.zip`, so a cursor that loses its place
/// lands on the wrong row.
struct Tree {
    app: App,
    real: PathBuf,
    link: PathBuf,
}

impl Tree {
    fn archive(&self) -> PathBuf {
        self.real.join("pkg.zip")
    }

    /// `pkg.zip` named another way: through the symlink, or through `sub/..`.
    fn spellings(&self) -> [PathBuf; 2] {
        [
            self.link.join("pkg.zip"),
            self.real.join("sub").join("..").join("pkg.zip"),
        ]
    }
}

/// Run `test` once per spelling of the archive's path, each in a fresh tree.
fn for_each_spelling(test: impl Fn(Tree, PathBuf)) {
    for which in 0..2 {
        in_a_tree(|tree| {
            let spelling = tree.spellings()[which].clone();
            test(tree, spelling);
        });
    }
}

fn in_a_tree(test: impl FnOnce(Tree)) {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        // Canonical, so the only indirection on the way is the one made here.
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let real = base.join("real");
        std::fs::create_dir_all(real.join("sub")).unwrap();
        std::fs::write(real.join("a.txt"), b"a\n").unwrap();
        build_zip(&real.join("pkg.zip"));
        let link = base.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut app = App::test_app(real.clone());
        app.state.refresh_listing();
        test(Tree { app, real, link });
    });
}

/// Type `:command` and Enter, as the user would.
fn colon(app: &mut App, command: &str) -> Vec<Effect> {
    let mut fx = app.handle_key(key(':')).unwrap();
    for c in command.chars() {
        fx.extend(app.handle_key(key(c)).unwrap());
    }
    fx.extend(
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .unwrap(),
    );
    fx
}

/// `:cd` to `path`, typed, and carried out. A relative path is anchored on
/// the column first, as the executor anchors it before the archive screen
/// sees it.
fn cd(app: &mut App, path: &Path) {
    let dir = app.state.cur().listing.dir.clone();
    let fx = colon(app, &format!("cd {}", path.display()))
        .into_iter()
        .map(|effect| match effect {
            Effect::ChangeDir {
                path,
                focus,
                on_ok,
                err_prefix,
            } if path.is_relative() => Effect::ChangeDir {
                path: dir.join(path),
                focus,
                on_ok,
                err_prefix,
            },
            other => other,
        })
        .collect();
    settle(app, fx);
}

/// `-`, the climb to the parent, carried out.
fn climb(app: &mut App) {
    let fx = app.handle_key(key('-')).unwrap();
    settle(app, fx);
}

/// `Enter` on the row named `name`, carried out.
fn enter(app: &mut App, name: &str) {
    let at = app.state.cur().listing.dir.join(name);
    app.state.focus_on_path(&at);
    assert_eq!(cursor_row(app), name, "the cursor is on {name}");
    let fx = app
        .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
        .unwrap();
    settle(app, fx);
}

/// Delete `README.md` from the mount the column is in, which leaves it with
/// a pending change.
fn delete_the_readme(app: &mut App) {
    let at = app.state.cur().listing.dir.join("README.md");
    app.state.focus_on_path(&at);
    assert_eq!(cursor_row(app), "README.md");
    let fx = app.apply(&Action::RemovePrompt(None)).unwrap();
    let fx = if fx.is_empty() {
        app.handle_remove_confirm_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::empty()))
    } else {
        fx
    };
    settle(app, fx);
    assert!(
        !row_names(app).iter().any(|r| r == "README.md"),
        "the delete is pending: {:?}",
        row_names(app)
    );
}

/// The name of the row under the cursor.
fn cursor_row(app: &App) -> String {
    let c = app.state.cur();
    c.rows
        .get(c.cursor.index)
        .map(|r| r.display.clone())
        .unwrap_or_default()
}

/// The painted line of the listing row that shows `name`.
fn painted_row(app: &mut App, name: &str) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 12)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .find(|line| line.contains(name))
        .unwrap_or_default()
}

fn flashed(app: &App) -> String {
    app.state
        .flash
        .as_ref()
        .map(|f| f.text.clone())
        .unwrap_or_default()
}

/// Climbing out of an archive entered by another spelling of its path puts
/// the cursor on the archive, as climbing out of any directory does.
#[test]
fn climbing_out_lands_on_the_archive_however_its_path_was_spelled() {
    for_each_spelling(|mut tree, spelling| {
        cd(&mut tree.app, &spelling);
        assert_eq!(
            row_names(&tree.app),
            ["src/", "README.md"],
            "inside {}: {}",
            spelling.display(),
            flashed(&tree.app)
        );

        climb(&mut tree.app);
        assert_eq!(tree.app.state.cur().listing.dir, tree.real, "out of it");
        assert_eq!(
            cursor_row(&tree.app),
            "pkg.zip",
            "the cursor is on the archive just left, entered as {}",
            spelling.display()
        );
    });
}

/// The row of an archive holding an unwritten change says so, from the
/// directory it lives in, whichever way the user went in.
#[test]
fn the_archive_row_shows_its_unwritten_change_however_its_path_was_spelled() {
    for_each_spelling(|mut tree, spelling| {
        cd(&mut tree.app, &spelling);
        delete_the_readme(&mut tree.app);
        climb(&mut tree.app);
        assert_eq!(tree.app.state.cur().listing.dir, tree.real, "out of it");

        let row = painted_row(&mut tree.app, "pkg.zip");
        assert!(
            row.contains('~'),
            "the archive's row marks its pending change, entered as {}: {row:?}",
            spelling.display()
        );
    });
}

/// Coming back by the archive's own row finds the mount the user left, with
/// its pending change, rather than mounting the archive a second time with a
/// clean journal.
#[test]
fn coming_back_by_its_row_finds_the_mount_entered_by_another_spelling() {
    for_each_spelling(|mut tree, spelling| {
        cd(&mut tree.app, &spelling);
        delete_the_readme(&mut tree.app);
        climb(&mut tree.app);
        enter(&mut tree.app, "pkg.zip");

        assert_eq!(tree.app.state.cur().listing.dir, tree.archive(), "back in");
        assert_eq!(
            tree.app.state.mounts.len(),
            1,
            "one mount for one archive, entered as {}",
            spelling.display()
        );
        assert_eq!(
            row_names(&tree.app),
            ["src/"],
            "the pending delete is still there"
        );
        assert!(tree.app.mount_is_dirty(&tree.archive()));
    });
}

/// The other way round: an archive mounted from its row, then reached by
/// another spelling of its path, is the same mount.
#[test]
fn another_spelling_finds_the_archive_already_mounted() {
    for_each_spelling(|mut tree, spelling| {
        enter(&mut tree.app, "pkg.zip");
        delete_the_readme(&mut tree.app);
        climb(&mut tree.app);

        cd(&mut tree.app, &spelling);
        assert_eq!(
            tree.app.state.mounts.len(),
            1,
            "one mount for one archive, reached as {}",
            spelling.display()
        );
        assert_eq!(
            row_names(&tree.app),
            ["src/"],
            "the pending delete is still there: {}",
            flashed(&tree.app)
        );
    });
}

/// `:cd ..` climbs inside an archive as it does anywhere else: from a
/// directory in it to its root, and from its root to where it lives; and
/// `:cd ../..` from a directory in it climbs out in one go.
#[test]
fn cd_dot_dot_climbs_inside_an_archive() {
    in_a_tree(|mut tree| {
        enter(&mut tree.app, "pkg.zip");
        enter(&mut tree.app, "src/");
        assert_eq!(tree.app.state.cur().listing.dir, tree.archive().join("src"));

        cd(&mut tree.app, Path::new(".."));
        assert_eq!(
            tree.app.state.cur().listing.dir,
            tree.archive(),
            "up to the archive's root: {}",
            flashed(&tree.app)
        );
        assert_eq!(row_names(&tree.app), ["src/", "README.md"]);

        cd(&mut tree.app, Path::new(".."));
        assert_eq!(
            tree.app.state.cur().listing.dir,
            tree.real,
            "up to where it lives: {}",
            flashed(&tree.app)
        );

        enter(&mut tree.app, "pkg.zip");
        enter(&mut tree.app, "src/");
        cd(&mut tree.app, Path::new("../.."));
        assert_eq!(
            tree.app.state.cur().listing.dir,
            tree.real,
            "out of it from inside: {}",
            flashed(&tree.app)
        );
    });
}
