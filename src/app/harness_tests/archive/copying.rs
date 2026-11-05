//! `c` and `M` with a `%` destination, which names each picked file's own
//! target, in and around a mounted archive. A copy leaves its sources where they
//! are, so the journal never records one as a rename, whatever the pick count.

use super::*;
use crate::archive::journal::Change;

/// `pkg.zip` holding two top-level files beside `src/main.rs`.
fn build_two_file_zip(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut w = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
    for (name, body) in [
        ("notes.txt", "notes\n"),
        ("todo.txt", "todo\n"),
        ("src/main.rs", "fn main() {}\n"),
    ] {
        w.start_file(name, opts).unwrap();
        w.write_all(body.as_bytes()).unwrap();
    }
    w.finish().unwrap();
}

/// What a test gets: the app with the column at the archive's root, the
/// directory the archive sits in, and the archive.
struct Fixture {
    app: App,
    dir: PathBuf,
    archive: PathBuf,
}

/// Run `test` against a writable mount of `pkg.zip`.
fn mounted<R>(test: impl FnOnce(Fixture) -> R) -> R {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        // Canonical, as a climb out of the archive lands on the canonical path.
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("pkg.zip");
        build_two_file_zip(&archive);
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        let mount = app.state.mounts.get(&archive).expect("mounted");
        assert!(mount.capability.is_writable(), "{:?}", mount.capability);
        test(Fixture { app, dir, archive })
    })
}

/// Type `keys` as the user would, one key at a time, gathering the effects.
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

fn told(app: &App) -> String {
    app.flash_text().unwrap_or_default().to_string()
}

/// Pick each of `names` in `dir`'s listing with `t`.
fn pick(app: &mut App, dir: &Path, names: &[&str]) {
    for name in names {
        app.state.focus_on_path(&dir.join(name));
        let effects = press(app, "t");
        assert!(effects.is_empty(), "{effects:?}");
    }
    assert_eq!(
        app.state.selection_paths().len(),
        names.len(),
        "every name is picked"
    );
}

/// `op` (`c` or `M`) with `dest` typed at its prompt, run to completion.
fn run_to(app: &mut App, op: &str, dest: &str) {
    let mut effects = press(app, op);
    assert!(
        matches!(app.state.mode, Mode::Prompting(_)),
        "{op} asks where to: {:?}",
        told(app)
    );
    effects.extend(press(app, dest));
    effects.extend(press_enter(app));
    settle(app, effects);
}

fn journal(fx: &Fixture) -> Vec<Change> {
    let mount = fx.app.state.mounts.get(&fx.archive).expect("still mounted");
    mount.journal.changes().to_vec()
}

/// `:archive write`, run to completion.
fn write_back(fx: &mut Fixture) {
    let effects = fx.app.cmd_archive("write");
    settle(&mut fx.app, effects);
}

/// What `op` `%.bak` on `picked`, inside the mount, left behind: what the user
/// was told, what the journal holds, and the members once written back.
struct Outcome {
    told: String,
    journal: Vec<Change>,
    written: Vec<String>,
}

fn percent_bak_inside(op: &str, picked: &[&str]) -> Outcome {
    mounted(|mut fx| {
        let archive = fx.archive.clone();
        pick(&mut fx.app, &archive, picked);
        run_to(&mut fx.app, op, "%.bak");
        let told = told(&fx.app);
        let journal = journal(&fx);
        write_back(&mut fx);
        Outcome {
            told,
            journal,
            written: member_names(&archive),
        }
    })
}

const ORIGINALS: [&str; 3] = ["notes.txt", "src/main.rs", "todo.txt"];

// ── inside the mount ─────────────────────────────────────────────────────

/// A copy of one member to a new name inside its own archive is refused, and
/// the archive is left as it was.
#[test]
fn a_percent_copy_of_one_member_inside_its_mount_is_refused() {
    let one = percent_bak_inside("c", &["notes.txt"]);
    assert!(one.told.contains("copy out first"), "{}", one.told);
    assert!(one.journal.is_empty(), "{:?}", one.journal);
    assert_eq!(one.written, ORIGINALS);
}

/// The data-loss case: several members copied to `%` names inside their own
/// archive were journaled as renames, so `:archive write` moved them and the
/// originals were gone. They're answered as a single member is.
#[test]
#[ignore = "red: a multi-pick `%` copy is journaled as renames and the write moves the originals"]
fn a_percent_copy_of_several_members_inside_their_mount_keeps_the_originals() {
    let several = percent_bak_inside("c", &["notes.txt", "todo.txt"]);
    assert!(
        !several
            .journal
            .iter()
            .any(|c| matches!(c, Change::Renamed { .. })),
        "a copy is never journaled as a rename: {:?}",
        several.journal
    );
    for original in ORIGINALS {
        assert!(
            several.written.iter().any(|n| n == original),
            "{original} lost by the write: {:?}",
            several.written
        );
    }
    let one = percent_bak_inside("c", &["notes.txt"]);
    assert_eq!(
        several.told, one.told,
        "several picks are answered as one is"
    );
}

/// `M` with a `%` destination is the rename the copy must not become.
#[test]
fn a_percent_move_of_several_members_inside_their_mount_renames_each() {
    let moved = percent_bak_inside("M", &["notes.txt", "todo.txt"]);
    // Picks are a set, so the renames come in no particular order.
    let mut renames: Vec<(&str, &str)> = moved
        .journal
        .iter()
        .filter_map(|c| match c {
            Change::Renamed { from, to } => Some((from.as_str(), to.as_str())),
            _ => None,
        })
        .collect();
    renames.sort_unstable();
    assert_eq!(
        renames,
        [("notes.txt", "notes.txt.bak"), ("todo.txt", "todo.txt.bak")],
        "{}",
        moved.told
    );
    assert_eq!(moved.journal.len(), renames.len(), "{:?}", moved.journal);
    assert_eq!(
        moved.written,
        ["notes.txt.bak", "src/main.rs", "todo.txt.bak"]
    );
}

// ── across the boundary ──────────────────────────────────────────────────

/// Copying several members out under `%` names is a read of each, as copying
/// one out is.
#[test]
#[ignore = "red: a multi-pick `%` copy out of a mount is refused as a rename across it"]
fn a_percent_copy_of_several_members_out_of_their_mount_extracts_each() {
    mounted(|mut fx| {
        let archive = fx.archive.clone();
        let out = fx.dir.join("out");
        std::fs::create_dir(&out).unwrap();
        pick(&mut fx.app, &archive, &["notes.txt", "todo.txt"]);
        run_to(&mut fx.app, "c", &format!("{}/%.bak", out.display()));
        for (name, body) in [("notes.txt.bak", "notes\n"), ("todo.txt.bak", "todo\n")] {
            assert_eq!(
                std::fs::read_to_string(out.join(name)).ok().as_deref(),
                Some(body),
                "{name}: {}",
                told(&fx.app)
            );
        }
        assert!(
            journal(&fx).is_empty(),
            "copying out changes nothing inside"
        );
    });
}

/// Real files copied in under `%` names are refused, and nothing is journaled.
#[test]
fn a_percent_copy_of_several_files_into_a_mount_is_refused() {
    mounted(|mut fx| {
        let _cwd = CwdGuard;
        let dir = fx.dir.clone();
        let effects = press(&mut fx.app, "-");
        settle(&mut fx.app, effects);
        assert_eq!(fx.app.state.cur().listing.dir, dir, "out of the archive");
        for name in ["a.txt", "b.txt"] {
            std::fs::write(dir.join(name), b"real\n").unwrap();
        }
        fx.app.state.refresh_listing();
        pick(&mut fx.app, &dir, &["a.txt", "b.txt"]);
        run_to(&mut fx.app, "c", "pkg.zip/%.bak");
        let said = told(&fx.app);
        assert!(
            said.starts_with("archive:"),
            "refused before any file is touched: {said}"
        );
        assert!(journal(&fx).is_empty(), "{:?}", journal(&fx));
        write_back(&mut fx);
        assert_eq!(member_names(&fx.archive), ORIGINALS);
    });
}

/// A `%` copy of the archive file itself is a read of a real file: the mount
/// stays, and so do its unwritten changes.
#[test]
#[ignore = "red: a multi-pick `%` copy takes the archive file as a move would and refuses a dirty mount"]
fn a_percent_copy_of_a_mounted_archive_keeps_its_mount_and_its_changes() {
    mounted(|mut fx| {
        let _cwd = CwdGuard;
        let dir = fx.dir.clone();
        let archive = fx.archive.clone();
        if let Some(mount) = fx.app.state.mounts.get_mut(&archive) {
            mount.journal.delete("todo.txt");
        }
        let effects = press(&mut fx.app, "-");
        settle(&mut fx.app, effects);
        assert_eq!(fx.app.state.cur().listing.dir, dir, "out of the archive");
        std::fs::write(dir.join("other.txt"), b"other\n").unwrap();
        fx.app.state.refresh_listing();
        pick(&mut fx.app, &dir, &["pkg.zip", "other.txt"]);
        run_to(&mut fx.app, "c", "%.orig");
        assert_eq!(
            std::fs::read(dir.join("pkg.zip.orig")).ok(),
            std::fs::read(&archive).ok(),
            "the archive is copied: {}",
            told(&fx.app)
        );
        assert!(dir.join("other.txt.orig").is_file());
        assert!(
            fx.app.mount_is_dirty(&archive),
            "the mount keeps its change"
        );
    });
}
