//! `c` and `M` with a `%` destination, which names each picked file's own
//! target, in and around a mounted archive. A copy leaves its sources where they
//! are, so the journal never records one as a rename, whatever the pick count.
//!
//! And `c` of real files into a mount, held against the same `c` into a
//! directory holding the archive's tree: whatever lands, or is refused, on disk
//! lands or is refused in the archive.

use super::*;
use crate::archive::journal::Change;

/// What `pkg.zip` holds: two top-level files beside `src/main.rs`.
const MEMBERS: [(&str, &str); 3] = [
    ("notes.txt", "notes\n"),
    ("todo.txt", "todo\n"),
    ("src/main.rs", "fn main() {}\n"),
];

/// `pkg.zip` holding [`MEMBERS`].
fn build_two_file_zip(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut w = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
    for (name, body) in MEMBERS {
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

// ── real files in, as on disk ────────────────────────────────────────────

/// The real files a test copies in, beside the archive.
const BROUGHT: [(&str, &str); 3] = [
    ("brought.txt", "brought\n"),
    ("a.txt", "a\n"),
    ("b.txt", "b\n"),
];

/// Files by `/`-separated path under their root, with their bodies, in name
/// order: a directory and an archive read alike, so the two compare.
type Tree = Vec<(String, String)>;

fn file(name: &str, body: &str) -> (String, String) {
    (name.to_string(), body.to_string())
}

/// Every file under `root`.
fn files_under(root: &Path) -> Tree {
    fn walk(root: &Path, dir: &Path, out: &mut Tree) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy();
                out.push(file(&rel, &std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    let mut out = Tree::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// Every file `archive` holds, read off disk.
fn files_in(archive: &Path) -> Tree {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive).unwrap()).unwrap();
    let mut out = Tree::new();
    for i in 0..zip.len() {
        let mut member = zip.by_index(i).unwrap();
        if member.is_dir() {
            continue;
        }
        let name = member.name().unwrap().into_owned();
        let mut body = String::new();
        std::io::Read::read_to_string(&mut member, &mut body).unwrap();
        out.push(file(&name, &body));
    }
    out.sort();
    out
}

/// What `c` of the real files `sources` to `dest` did, typed once under
/// `mirror`, a directory holding the archive's tree, and once under the
/// archive: both trees afterwards, and what the mount said and recorded.
struct BothWays {
    on_disk_before: Tree,
    on_disk: Tree,
    in_archive: Tree,
    told: String,
    journal: Vec<Change>,
}

/// `c` from beside the archive, with the column climbed out of it.
fn copy_both_ways(sources: &[&str], dest: &str) -> BothWays {
    mounted(|mut fx| {
        let _cwd = CwdGuard;
        let dir = fx.dir.clone();
        let effects = press(&mut fx.app, "-");
        settle(&mut fx.app, effects);
        assert_eq!(fx.app.state.cur().listing.dir, dir, "out of the archive");
        let mirror = dir.join("mirror");
        std::fs::create_dir_all(mirror.join("src")).unwrap();
        for (name, body) in MEMBERS {
            std::fs::write(mirror.join(name), body).unwrap();
        }
        for (name, body) in BROUGHT {
            std::fs::write(dir.join(name), body).unwrap();
        }
        let on_disk_before = files_under(&mirror);
        for root in ["mirror", "pkg.zip"] {
            fx.app.state.refresh_listing();
            fx.app.state.cur_mut().picks.clear();
            pick(&mut fx.app, &dir, sources);
            let typed = if dest.is_empty() {
                root.to_string()
            } else {
                format!("{root}/{dest}")
            };
            run_to(&mut fx.app, "c", &typed);
        }
        let told = told(&fx.app);
        let journal = journal(&fx);
        write_back(&mut fx);
        BothWays {
            on_disk_before,
            on_disk: files_under(&mirror),
            in_archive: files_in(&fx.archive),
            told,
            journal,
        }
    })
}

/// `c` of `sources` to `dest` leaves the archive holding what it leaves on disk.
fn copied_as_on_disk(sources: &[&str], dest: &str) -> BothWays {
    let both = copy_both_ways(sources, dest);
    assert_eq!(
        both.in_archive, both.on_disk,
        "c {sources:?} to {dest:?}: {}",
        both.told
    );
    both
}

/// A copy that fails on disk is refused in the mount, `because` of what made
/// it fail there, before anything is staged or recorded.
fn assert_refused(both: &BothWays, what: &str, because: &str) {
    assert_eq!(both.on_disk, both.on_disk_before, "{what}: refused on disk");
    assert!(
        both.told.starts_with("archive:") && both.told.contains(because),
        "{what}: refused up front, {because:?}: {}",
        both.told
    );
    assert!(both.journal.is_empty(), "{what}: {:?}", both.journal);
}

/// Into a directory the archive holds, each file keeps its own name, as on disk.
#[test]
fn copying_files_into_a_directory_of_a_mount_keeps_their_names() {
    for (sources, dest, landed) in [
        (
            &["brought.txt"][..],
            "",
            &[file("brought.txt", "brought\n")][..],
        ),
        (
            &["brought.txt"],
            "src",
            &[file("src/brought.txt", "brought\n")],
        ),
        (
            &["a.txt", "b.txt"],
            "src",
            &[file("src/a.txt", "a\n"), file("src/b.txt", "b\n")],
        ),
    ] {
        let both = copied_as_on_disk(sources, dest);
        for one in landed {
            assert!(
                both.in_archive.contains(one),
                "{one:?}: {:?}",
                both.in_archive
            );
        }
    }
}

/// The report: one file copied in to a name the archive doesn't hold was
/// staged as a directory of that name with the file inside, so the write added
/// `new.txt/brought.txt`. On disk it's a copy named `new.txt`, and a `%` name
/// on one file is that same copy.
#[test]
#[ignore = "red: the file is staged inside a new directory named after the destination"]
fn copying_one_file_into_a_mount_under_a_new_name_names_it_so() {
    for (dest, landed) in [
        ("new.txt", "new.txt"),
        ("src/new.rs", "src/new.rs"),
        ("%.bak", "brought.txt.bak"),
    ] {
        let both = copied_as_on_disk(&["brought.txt"], dest);
        assert!(
            both.in_archive.contains(&file(landed, "brought\n")),
            "{dest}: {:?}",
            both.in_archive
        );
    }
}

/// Only a directory takes several files: on disk, several to one new name is
/// refused. In a mount it staged a directory of that name nobody asked for.
#[test]
#[ignore = "red: the files are staged inside a new directory named after the destination"]
fn copying_several_files_into_a_mount_under_one_new_name_is_refused() {
    let both = copied_as_on_disk(&["a.txt", "b.txt"], "new");
    assert_refused(&both, "several to one name", "copy into a directory");
}

/// A destination that is neither a directory nor a free name: a file the
/// archive holds, which a copy on disk doesn't overwrite, and a directory it
/// doesn't hold, which a copy on disk doesn't make.
#[test]
#[ignore = "red: each is staged as a new directory named after the destination"]
fn copying_into_a_mount_where_a_copy_on_disk_fails_is_refused() {
    for (dest, because) in [
        ("notes.txt", "one file to a new name"),
        ("nodir/new.txt", "no such directory"),
        ("newdir/", "no such directory"),
    ] {
        let both = copied_as_on_disk(&["brought.txt"], dest);
        assert_refused(&both, dest, because);
    }
}

/// A name a pending change still claims: a deleted member's, which the delete
/// would hide an addition under, and a renamed member's old one, whose staged
/// bytes an addition would land on. The journal can't hold either, so the copy
/// is refused with the way round it, and the archive keeps only the change
/// already made.
#[test]
#[ignore = "red: the file is staged inside a new directory named after the destination"]
fn copying_in_under_a_name_a_pending_change_claims_is_refused() {
    type Setup = fn(&mut crate::archive::Journal);
    let cases: [(&str, Setup, Change, Tree); 2] = [
        (
            "todo.txt",
            |journal| journal.delete("todo.txt"),
            Change::Deleted {
                inner: "todo.txt".to_string(),
            },
            vec![
                file("notes.txt", "notes\n"),
                file("src/main.rs", "fn main() {}\n"),
            ],
        ),
        (
            "notes.txt",
            |journal| journal.rename("notes.txt", "kept.txt"),
            Change::Renamed {
                from: "notes.txt".to_string(),
                to: "kept.txt".to_string(),
            },
            vec![
                file("kept.txt", "notes\n"),
                file("src/main.rs", "fn main() {}\n"),
                file("todo.txt", "todo\n"),
            ],
        ),
    ];
    for (dest, setup, made, kept) in cases {
        mounted(|mut fx| {
            let _cwd = CwdGuard;
            let dir = fx.dir.clone();
            let archive = fx.archive.clone();
            setup(&mut fx.app.state.mounts.get_mut(&archive).unwrap().journal);
            let effects = press(&mut fx.app, "-");
            settle(&mut fx.app, effects);
            assert_eq!(fx.app.state.cur().listing.dir, dir, "out of the archive");
            std::fs::write(dir.join("brought.txt"), b"brought\n").unwrap();
            fx.app.state.refresh_listing();
            pick(&mut fx.app, &dir, &["brought.txt"]);
            run_to(&mut fx.app, "c", &format!("pkg.zip/{dest}"));
            let said = told(&fx.app);
            assert!(
                said.contains("copy files in under their own names, then rename"),
                "{dest}: {said}"
            );
            assert_eq!(journal(&fx), [made], "{dest}");
            write_back(&mut fx);
            assert_eq!(files_in(&archive), kept, "{dest}");
        });
    }
}
