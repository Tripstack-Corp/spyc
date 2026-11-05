//! `c` and `M` with a `%` destination, which names each picked file's own
//! target, in and around a mounted archive. A copy leaves its sources where they
//! are, so the journal never records one as a rename, whatever the pick count.
//!
//! And `c` and `p` of real files into a mount, held against the same key into
//! a directory holding the archive's tree: whatever lands, or is refused, on
//! disk lands or is refused in the archive, and is told where it landed.

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

/// The real files a test copies in, beside the archive: three names the
/// archive doesn't hold, two of its files' names with other bodies, and a file
/// named as its directory is.
const BROUGHT: [(&str, &str); 6] = [
    ("brought.txt", "brought\n"),
    ("a.txt", "a\n"),
    ("b.txt", "b\n"),
    ("notes.txt", "notes, again\n"),
    ("main.rs", "fn main() { again() }\n"),
    ("src", "a file named src\n"),
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
/// archive: both trees afterwards, what each copy said, and what the mount
/// listed and recorded before the write.
struct BothWays {
    on_disk_before: Tree,
    on_disk: Tree,
    in_archive: Tree,
    told: String,
    told_on_disk: String,
    shown: Vec<String>,
    journal: Vec<Change>,
}

/// One `c` of a run: rewrite these real files beside the archive, then copy
/// `sources` to `dest`.
struct Step<'a> {
    rewrite: &'a [(&'a str, &'a str)],
    sources: &'a [&'a str],
    dest: &'a str,
}

/// `c` from beside the archive, with the column climbed out of it.
fn copy_both_ways(sources: &[&str], dest: &str) -> BothWays {
    copy_steps_both_ways(&[Step {
        rewrite: &[],
        sources,
        dest,
    }])
}

/// Each of `steps` in turn, under `mirror` and then under the archive, from the
/// same real files each time.
fn copy_steps_both_ways(steps: &[Step]) -> BothWays {
    mounted(|mut fx| {
        let _cwd = CwdGuard;
        let dir = fx.dir.clone();
        let effects = press(&mut fx.app, "-");
        settle(&mut fx.app, effects);
        assert_eq!(fx.app.state.cur().listing.dir, dir, "out of the archive");
        let mirror = mirror_beside(&dir);
        let on_disk_before = files_under(&mirror);
        let mut told_on_disk = String::new();
        for root in ["mirror", "pkg.zip"] {
            for (name, body) in BROUGHT {
                std::fs::write(dir.join(name), body).unwrap();
            }
            for step in steps {
                for (name, body) in step.rewrite {
                    std::fs::write(dir.join(name), body).unwrap();
                }
                fx.app.state.refresh_listing();
                fx.app.state.cur_mut().picks.clear();
                pick(&mut fx.app, &dir, step.sources);
                let typed = if step.dest.is_empty() {
                    root.to_string()
                } else {
                    format!("{root}/{}", step.dest)
                };
                run_to(&mut fx.app, "c", &typed);
            }
            if root == "mirror" {
                told_on_disk = told(&fx.app);
            }
        }
        let told = told(&fx.app);
        let journal = journal(&fx);
        let shown = shown_files(&fx);
        write_back(&mut fx);
        BothWays {
            on_disk_before,
            on_disk: files_under(&mirror),
            in_archive: files_in(&fx.archive),
            told,
            told_on_disk,
            shown,
            journal,
        }
    })
}

/// `mirror` in `dir`: a directory holding the archive's tree.
fn mirror_beside(dir: &Path) -> PathBuf {
    let mirror = dir.join("mirror");
    std::fs::create_dir_all(mirror.join("src")).unwrap();
    for (name, body) in MEMBERS {
        std::fs::write(mirror.join(name), body).unwrap();
    }
    mirror
}

/// Every file the mount lists, by its path in the archive, walked from its
/// root: what the user sees row by row, a name listed twice included.
fn shown_files(fx: &Fixture) -> Vec<String> {
    let mount = fx.app.state.mounts.get(&fx.archive).expect("still mounted");
    let mut out = Vec::new();
    let mut dirs = vec![String::new()];
    while let Some(dir) = dirs.pop() {
        for entry in mount.listing_for(&dir).entries {
            let path = if dir.is_empty() {
                entry.name.clone()
            } else {
                format!("{dir}/{}", entry.name)
            };
            if entry.kind == crate::fs::EntryKind::Dir {
                dirs.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The names in `tree`.
fn names(tree: &Tree) -> Vec<String> {
    tree.iter().map(|(name, _)| name.clone()).collect()
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
fn copying_several_files_into_a_mount_under_one_new_name_is_refused() {
    let both = copied_as_on_disk(&["a.txt", "b.txt"], "new");
    assert_refused(&both, "several to one name", "copy into a directory");
}

/// A destination that is neither a directory nor a free name: a file the
/// archive holds, which a copy on disk doesn't overwrite, and a directory it
/// doesn't hold, which a copy on disk doesn't make.
#[test]
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

// ── real files in, over a name the archive holds ─────────────────────────

/// The report: a file copied into a directory of the mount that holds a file
/// of its name was added beside it, so the directory listed the name twice and
/// `:archive write` failed with `Duplicate filename`. On disk the copy
/// replaces that file's bytes.
#[test]
fn copying_a_file_into_a_mount_over_one_of_that_name_replaces_it() {
    for (sources, dest) in [
        (&["notes.txt"][..], ""),
        (&["main.rs"], "src"),
        (&["notes.txt", "brought.txt"], ""),
    ] {
        let both = copied_as_on_disk(sources, dest);
        assert_ne!(both.on_disk, both.on_disk_before, "{sources:?}: copied");
        assert_eq!(
            both.shown,
            names(&both.on_disk),
            "{sources:?} to {dest:?}: each name listed once: {}",
            both.told
        );
    }
}

/// The same file copied in again, changed in between, over a file copied in
/// before and over a file the archive held: one pending change each, and the
/// archive takes the second copy, as the directory does.
#[test]
fn copying_a_file_into_a_mount_twice_keeps_the_second_copy() {
    for name in ["brought.txt", "notes.txt"] {
        let again = [(name, "changed in between\n")];
        let sources = [name];
        let both = copy_steps_both_ways(&[
            Step {
                rewrite: &[],
                sources: &sources,
                dest: "",
            },
            Step {
                rewrite: &again,
                sources: &sources,
                dest: "",
            },
        ]);
        assert!(
            both.on_disk.contains(&file(name, "changed in between\n")),
            "{name}: {:?}",
            both.on_disk
        );
        assert_eq!(both.in_archive, both.on_disk, "{name}: {}", both.told);
        assert_eq!(both.shown, names(&both.on_disk), "{name}: listed once");
        assert_eq!(both.journal.len(), 1, "{name}: {:?}", both.journal);
    }
}

/// A member read before the copy has its bytes staged already, and the copy
/// has to land on them rather than beside them.
#[test]
fn copying_a_file_over_a_member_already_extracted_replaces_it() {
    mounted(|mut fx| {
        let _cwd = CwdGuard;
        let dir = fx.dir.clone();
        let archive = fx.archive.clone();
        settle(
            &mut fx.app,
            vec![Effect::Inventory(
                crate::app::inventory_ops::InventoryOp::Yank {
                    sources: vec![crate::app::inventory_ops::YankSource::plain(
                        archive.join("notes.txt"),
                    )],
                },
            )],
        );
        let mount = fx.app.state.mounts.get(&archive).expect("mounted");
        let entry = mount.index.get("notes.txt").expect("a member");
        assert!(mount.is_materialized(entry), "the yank extracted it");
        let effects = press(&mut fx.app, "-");
        settle(&mut fx.app, effects);
        assert_eq!(fx.app.state.cur().listing.dir, dir, "out of the archive");
        std::fs::write(dir.join("notes.txt"), b"notes, again\n").unwrap();
        fx.app.state.refresh_listing();
        pick(&mut fx.app, &dir, &["notes.txt"]);
        run_to(&mut fx.app, "c", "pkg.zip");
        let said = told(&fx.app);
        assert_eq!(
            shown_files(&fx),
            ORIGINALS,
            "listed once, where it was: {said}"
        );
        write_back(&mut fx);
        assert_eq!(
            files_in(&archive),
            [
                file("notes.txt", "notes, again\n"),
                file("src/main.rs", "fn main() {}\n"),
                file("todo.txt", "todo\n"),
            ],
            "{said}"
        );
    });
}

/// A file copied over a directory fails on disk, and is refused in the mount
/// before anything is staged.
#[test]
fn copying_a_file_into_a_mount_over_a_directory_of_that_name_is_refused() {
    let both = copied_as_on_disk(&["src"], "");
    assert_refused(
        &both,
        "a file over a directory",
        "a directory has that name",
    );
}

/// A file copied over a link writes, on disk, wherever the link points, which
/// in a mount is another member's staged bytes. Refused before anything is
/// staged.
#[test]
fn copying_a_file_into_a_mount_over_a_link_of_that_name_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("links.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
        w.start_file("notes.txt", opts).unwrap();
        w.write_all(b"notes\n").unwrap();
        w.add_symlink("link.txt", "notes.txt", opts).unwrap();
        w.finish().unwrap();
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        let effects = press(&mut app, "-");
        settle(&mut app, effects);
        assert_eq!(app.state.cur().listing.dir, dir, "out of the archive");
        std::fs::write(dir.join("link.txt"), b"brought\n").unwrap();
        app.state.refresh_listing();
        pick(&mut app, &dir, &["link.txt"]);
        run_to(&mut app, "c", "links.zip");
        let said = told(&app);
        assert!(
            said.starts_with("archive:") && said.contains("a link has that name"),
            "{said}"
        );
        let mount = app.state.mounts.get(&archive).expect("still mounted");
        assert!(!mount.is_dirty(), "{:?}", mount.journal.changes());
    });
}

/// Of two members whose names differ only in case, the second is staged apart
/// from its name, so a copy writing at that name would land on the first.
/// Refused before anything is staged.
#[test]
fn copying_a_file_into_a_mount_over_a_member_named_but_for_case_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let _cwd = CwdGuard;
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let archive = dir.join("cases.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
        for name in ["Notes.txt", "notes.txt"] {
            w.start_file(name, opts).unwrap();
            w.write_all(name.as_bytes()).unwrap();
        }
        w.finish().unwrap();
        let mut app = App::test_app(dir.clone());
        mount_inline(&mut app, &archive, &tmp.path().join("staging"));
        let effects = press(&mut app, "-");
        settle(&mut app, effects);
        assert_eq!(app.state.cur().listing.dir, dir, "out of the archive");
        std::fs::write(dir.join("notes.txt"), b"brought\n").unwrap();
        app.state.refresh_listing();
        pick(&mut app, &dir, &["notes.txt"]);
        run_to(&mut app, "c", "cases.zip");
        let said = told(&app);
        assert!(
            said.starts_with("archive:") && said.contains("share that name but for case"),
            "{said}"
        );
        let mount = app.state.mounts.get(&archive).expect("still mounted");
        assert!(!mount.is_dirty(), "{:?}", mount.journal.changes());
    });
}

/// A name a pending change holds, met copying into a directory: a deleted
/// member's, which the delete would hide the copy under; a renamed member's old
/// one, which the rename would carry the copy along from; and a renamed
/// member's new one, or one under a renamed directory, whose bytes live under
/// the name the archive stores. The copy is refused with the way round it, and
/// the archive keeps only the change already made.
#[test]
fn copying_into_a_directory_of_a_mount_over_a_name_a_pending_change_holds_is_refused() {
    type Setup = fn(&mut crate::archive::Journal);
    let renamed = |from: &str, to: &str| Change::Renamed {
        from: from.to_string(),
        to: to.to_string(),
    };
    let cases: [(&str, &str, Setup, Change, Tree); 4] = [
        (
            "todo.txt",
            "",
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
            "",
            |journal| journal.rename("notes.txt", "kept.txt"),
            renamed("notes.txt", "kept.txt"),
            vec![
                file("kept.txt", "notes\n"),
                file("src/main.rs", "fn main() {}\n"),
                file("todo.txt", "todo\n"),
            ],
        ),
        (
            "brought.txt",
            "",
            |journal| journal.rename("todo.txt", "brought.txt"),
            renamed("todo.txt", "brought.txt"),
            vec![
                file("brought.txt", "todo\n"),
                file("notes.txt", "notes\n"),
                file("src/main.rs", "fn main() {}\n"),
            ],
        ),
        (
            "main.rs",
            "source",
            |journal| journal.rename("src", "source"),
            renamed("src", "source"),
            vec![
                file("notes.txt", "notes\n"),
                file("source/main.rs", "fn main() {}\n"),
                file("todo.txt", "todo\n"),
            ],
        ),
    ];
    for (name, dest, setup, made, kept) in cases {
        mounted(|mut fx| {
            let _cwd = CwdGuard;
            let dir = fx.dir.clone();
            let archive = fx.archive.clone();
            setup(&mut fx.app.state.mounts.get_mut(&archive).unwrap().journal);
            let effects = press(&mut fx.app, "-");
            settle(&mut fx.app, effects);
            assert_eq!(fx.app.state.cur().listing.dir, dir, "out of the archive");
            std::fs::write(dir.join(name), b"brought\n").unwrap();
            fx.app.state.refresh_listing();
            pick(&mut fx.app, &dir, &[name]);
            let typed = if dest.is_empty() {
                "pkg.zip".to_string()
            } else {
                format!("pkg.zip/{dest}")
            };
            run_to(&mut fx.app, "c", &typed);
            let said = told(&fx.app);
            assert!(
                said.starts_with("archive:")
                    && said.contains("a pending change holds that name — :archive write first"),
                "{name}: {said}"
            );
            assert_eq!(journal(&fx), [made], "{name}");
            write_back(&mut fx);
            assert_eq!(files_in(&archive), kept, "{name}");
        });
    }
}

// ── what a copy in says ──────────────────────────────────────────────────

/// The report: a copy in said `copied N item(s) to` the staging directory
/// spyc copied the bytes to. It names the place in the archive, as the copy
/// on disk names the directory.
#[test]
fn copying_into_a_mount_says_where_in_the_archive_it_went() {
    for (sources, dest) in [
        (&["brought.txt"][..], ""),
        (&["brought.txt"], "src"),
        (&["a.txt", "b.txt"], "src"),
        (&["brought.txt"], "new.txt"),
    ] {
        let both = copy_both_ways(sources, dest);
        assert!(
            both.told_on_disk.starts_with("copied "),
            "{sources:?}: {}",
            both.told_on_disk
        );
        assert_eq!(
            both.told,
            both.told_on_disk.replace("/mirror", "/pkg.zip"),
            "{sources:?} to {dest:?}"
        );
    }
}

// ── a put in ─────────────────────────────────────────────────────────────

/// What `p` of a real file yanked as `name` said and did: once into `mirror`,
/// once into the archive's root, each from an inventory holding only it. Each
/// runs in a directory of its own, so what each says names it `<dir>`.
struct PutBothWays {
    on_disk_before: Tree,
    on_disk: Tree,
    in_archive: Tree,
    told: String,
    told_on_disk: String,
    shown: Vec<String>,
    journal: Vec<Change>,
}

fn put_both_ways(name: &str) -> PutBothWays {
    let (on_disk_before, on_disk, told_on_disk) = mounted(|mut fx| {
        let mirror = mirror_beside(&fx.dir);
        let before = files_under(&mirror);
        put_into(&mut fx, name, &mirror);
        (before, files_under(&mirror), told_in(&fx))
    });
    mounted(|mut fx| {
        let archive = fx.archive.clone();
        put_into(&mut fx, name, &archive);
        let told = told_in(&fx);
        let journal = journal(&fx);
        let shown = shown_files(&fx);
        write_back(&mut fx);
        PutBothWays {
            on_disk_before,
            on_disk,
            in_archive: files_in(&archive),
            told,
            told_on_disk,
            shown,
            journal,
        }
    })
}

/// What the fixture's app said, its directory written `<dir>`.
fn told_in(fx: &Fixture) -> String {
    told(&fx.app).replace(&fx.dir.display().to_string(), "<dir>")
}

/// Yank the real file `name` beside the archive, then `p` it into `root`.
fn put_into(fx: &mut Fixture, name: &str, root: &Path) {
    let source = fx.dir.join(name);
    std::fs::write(&source, b"put\n").unwrap();
    settle(
        &mut fx.app,
        vec![Effect::Inventory(
            crate::app::inventory_ops::InventoryOp::Yank {
                sources: vec![crate::app::inventory_ops::YankSource::plain(source)],
            },
        )],
    );
    fx.app.state.change_dir(root, None, None, "cd");
    assert_eq!(fx.app.state.cur().listing.dir, root, "{}", told(&fx.app));
    let effects = press(&mut fx.app, "p");
    settle(&mut fx.app, effects);
}

/// A put doesn't overwrite on disk; in the mount it was added beside the file
/// of its name, and the write failed with `Duplicate filename`.
#[test]
fn putting_a_file_into_a_mount_over_one_of_that_name_is_refused() {
    let both = put_both_ways("notes.txt");
    assert_eq!(both.on_disk, both.on_disk_before, "{}", both.told_on_disk);
    assert!(
        both.told.starts_with("archive:") && both.told.contains("that name is taken"),
        "refused up front: {}",
        both.told
    );
    assert!(both.journal.is_empty(), "{:?}", both.journal);
    assert_eq!(both.shown, ORIGINALS);
    assert_eq!(
        both.in_archive, both.on_disk_before,
        "the archive as it was"
    );
}

/// A deleted member's name: the delete hid the put file, and the write
/// dropped it.
#[test]
fn putting_a_file_into_a_mount_under_a_deleted_members_name_is_refused() {
    mounted(|mut fx| {
        let archive = fx.archive.clone();
        if let Some(mount) = fx.app.state.mounts.get_mut(&archive) {
            mount.journal.delete("todo.txt");
        }
        put_into(&mut fx, "todo.txt", &archive);
        let said = told(&fx.app);
        assert!(
            said.contains("a pending change holds that name — :archive write first"),
            "{said}"
        );
        assert_eq!(
            journal(&fx),
            [Change::Deleted {
                inner: "todo.txt".to_string()
            }]
        );
    });
}

/// Two yanked files of one name: on disk the second put fails, as the name is
/// taken by then. In a mount both were added under it; the put is refused
/// whole, as every change to an archive is.
#[test]
fn putting_two_files_of_one_name_into_a_mount_is_refused() {
    mounted(|mut fx| {
        let archive = fx.archive.clone();
        let mut sources = Vec::new();
        for from in ["x", "y"] {
            let source = fx.dir.join(from).join("twin.txt");
            std::fs::create_dir_all(source.parent().unwrap()).unwrap();
            std::fs::write(&source, from).unwrap();
            sources.push(crate::app::inventory_ops::YankSource::plain(source));
        }
        settle(
            &mut fx.app,
            vec![Effect::Inventory(
                crate::app::inventory_ops::InventoryOp::Yank { sources },
            )],
        );
        assert_eq!(fx.app.state.inventory.items().count(), 2, "both yanked");
        fx.app.state.change_dir(&archive, None, None, "cd");
        let effects = press(&mut fx.app, "p");
        settle(&mut fx.app, effects);
        let said = told(&fx.app);
        assert!(
            said.starts_with("archive:") && said.contains("that name is taken"),
            "{said}"
        );
        assert!(journal(&fx).is_empty(), "{:?}", journal(&fx));
        assert_eq!(shown_files(&fx), ORIGINALS);
    });
}

/// The put flash named the staging directory, as the copy's did.
#[test]
fn putting_into_a_mount_says_where_in_the_archive_it_went() {
    let both = put_both_ways("brought.txt");
    assert!(
        both.in_archive.contains(&file("brought.txt", "put\n")),
        "{:?}",
        both.in_archive
    );
    assert!(
        both.told_on_disk.starts_with("put "),
        "{}",
        both.told_on_disk
    );
    assert_eq!(both.told, both.told_on_disk.replace("/mirror", "/pkg.zip"));
}
