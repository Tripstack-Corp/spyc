//! The tree of sizes `L` shows for a directory.
//!
//! [`scan`] walks a directory on several threads, never following a symbolic
//! link or crossing onto another file system, and feeds a [`DiskTree`]. A file
//! with several hard links counts once, under the first name the walk reaches.
//! The tree keeps every directory, but of each one's own files only the
//! largest [`SHOWN`], with sums for the rest, so a tree of millions of files
//! stays small. [`DiskTree::rows`] lays it out as deep as [`Folds`] says.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

/// How many children a directory shows before the rest fold into one row.
pub const SHOWN: usize = 20;

/// How often [`scan`] reports progress.
pub const TICK: Duration = Duration::from_millis(150);

/// Sizes summed over a set of entries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// The entries' lengths in bytes.
    pub apparent: u64,
    /// The bytes the file system allocated them.
    pub disk: u64,
    /// How many of them are not directories.
    pub files: u64,
}

impl Usage {
    const fn add(&mut self, other: Self) {
        self.apparent += other.apparent;
        self.disk += other.disk;
        self.files += other.files;
    }

    const fn minus(self, other: Self) -> Self {
        Self {
            apparent: self.apparent.saturating_sub(other.apparent),
            disk: self.disk.saturating_sub(other.disk),
            files: self.files.saturating_sub(other.files),
        }
    }
}

/// One entry the walk reached.
#[derive(Debug, Clone)]
pub struct Found {
    pub path: PathBuf,
    pub is_dir: bool,
    pub apparent: u64,
    pub disk: u64,
    /// Device and inode of a file with more than one link.
    pub link: Option<(u64, u64)>,
}

impl Found {
    /// What `md`, read without following a link, says about `path`.
    pub fn of(path: PathBuf, md: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        let is_dir = md.is_dir();
        Self {
            path,
            is_dir,
            apparent: md.len(),
            disk: md.blocks().saturating_mul(512),
            link: (!is_dir && md.nlink() > 1).then(|| (md.dev(), md.ino())),
        }
    }
}

#[derive(Debug, Clone)]
struct Leaf {
    name: String,
    usage: Usage,
}

#[derive(Debug)]
struct Dir {
    name: String,
    parent: Option<usize>,
    depth: usize,
    subdirs: Vec<usize>,
    /// The directory's own entry.
    own: Usage,
    /// Its own files, not its subdirectories'.
    files: Usage,
    /// The largest of its own files by disk, at most [`SHOWN`].
    largest: Vec<Leaf>,
    /// Everything beneath it, its own entry included.
    total: Usage,
}

impl Dir {
    fn new(name: String, parent: Option<usize>, depth: usize) -> Self {
        Self {
            name,
            parent,
            depth,
            subdirs: Vec::new(),
            own: Usage::default(),
            files: Usage::default(),
            largest: Vec::new(),
            total: Usage::default(),
        }
    }
}

/// The sizes beneath one directory, as far as the walk has got.
#[derive(Debug)]
pub struct DiskTree {
    root: PathBuf,
    dirs: Vec<Dir>,
    index: HashMap<PathBuf, usize>,
    links: HashSet<(u64, u64)>,
    deepest: usize,
    /// Entries the walk couldn't read.
    pub unreadable: u64,
}

impl DiskTree {
    /// An empty tree for `root`, its top row named `label`.
    pub fn new(root: PathBuf, label: String) -> Self {
        let index = HashMap::from([(root.clone(), 0)]);
        Self {
            root,
            dirs: vec![Dir::new(label, None, 0)],
            index,
            links: HashSet::new(),
            deepest: 0,
            unreadable: 0,
        }
    }

    /// Everything counted so far.
    pub fn total(&self) -> Usage {
        self.dirs[0].total
    }

    /// The depth of the deepest directory found, the root's being 0.
    pub const fn deepest(&self) -> usize {
        self.deepest
    }

    /// The directory `id` is in; `None` for the root.
    pub fn parent_of(&self, id: usize) -> Option<usize> {
        self.dirs.get(id)?.parent
    }

    /// Count `found`, unless it is a file already counted under another name or
    /// lies outside the root.
    pub fn add(&mut self, found: Found) {
        if let Some(key) = found.link
            && !self.links.insert(key)
        {
            return;
        }
        let usage = Usage {
            apparent: found.apparent,
            disk: found.disk,
            files: u64::from(!found.is_dir),
        };
        let at = if found.is_dir {
            let Some(id) = self.dir(&found.path) else {
                return;
            };
            self.dirs[id].own.add(usage);
            id
        } else {
            let Some(id) = found.path.parent().and_then(|p| self.dir(p)) else {
                return;
            };
            let name = found
                .path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let dir = &mut self.dirs[id];
            dir.files.add(usage);
            keep_largest(&mut dir.largest, Leaf { name, usage });
            id
        };
        let mut next = Some(at);
        while let Some(id) = next {
            self.dirs[id].total.add(usage);
            next = self.dirs[id].parent;
        }
    }

    /// The directory at `path`, made with its missing ancestors.
    fn dir(&mut self, path: &Path) -> Option<usize> {
        if let Some(&id) = self.index.get(path) {
            return Some(id);
        }
        if !path.starts_with(&self.root) {
            return None;
        }
        let parent = self.dir(path.parent()?)?;
        let id = self.dirs.len();
        let depth = self.dirs[parent].depth + 1;
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.dirs.push(Dir::new(name, Some(parent), depth));
        self.dirs[parent].subdirs.push(id);
        self.index.insert(path.to_path_buf(), id);
        self.deepest = self.deepest.max(depth);
        Some(id)
    }

    /// The tree laid out row by row: the root, then each open directory's
    /// children, largest first by disk.
    pub fn rows(&self, folds: &Folds) -> Vec<Row> {
        let root = &self.dirs[0];
        let open = folds.is_open(0, 0);
        let mut out = vec![Row {
            rails: Vec::new(),
            last: true,
            parent: None,
            kind: RowKind::Dir {
                id: 0,
                open,
                empty: root.subdirs.is_empty() && root.files.files == 0,
            },
            name: root.name.clone(),
            usage: root.total,
            share: 1000,
        }];
        if open {
            self.children_rows(0, &mut Vec::new(), folds, &mut out);
        }
        out
    }

    fn children_rows(&self, id: usize, rails: &mut Vec<bool>, folds: &Folds, out: &mut Vec<Row>) {
        let dir = &self.dirs[id];
        let children = self.children(id);
        let count = children.len();
        for (k, child) in children.into_iter().enumerate() {
            let last = k + 1 == count;
            let (kind, name, usage, sub) = match child {
                Child::Sub(sub) => {
                    let d = &self.dirs[sub];
                    let open = folds.is_open(sub, d.depth);
                    let empty = d.subdirs.is_empty() && d.files.files == 0;
                    let kind = RowKind::Dir {
                        id: sub,
                        open,
                        empty,
                    };
                    (kind, d.name.clone(), d.total, open.then_some(sub))
                }
                Child::File(leaf) => (RowKind::File, leaf.name.clone(), leaf.usage, None),
                Child::More { entries, usage } => {
                    (RowKind::More { entries }, String::new(), usage, None)
                }
            };
            out.push(Row {
                rails: rails.clone(),
                last,
                parent: Some(id),
                kind,
                name,
                usage,
                share: share(usage, dir.total),
            });
            if let Some(sub) = sub {
                rails.push(!last);
                self.children_rows(sub, rails, folds, out);
                rails.pop();
            }
        }
    }

    /// A directory's children to show, largest first, the rest summed into one.
    fn children(&self, id: usize) -> Vec<Child<'_>> {
        let dir = &self.dirs[id];
        let mut all: Vec<(Usage, &str, Child<'_>)> = dir
            .subdirs
            .iter()
            .map(|&sub| {
                (
                    self.dirs[sub].total,
                    self.dirs[sub].name.as_str(),
                    Child::Sub(sub),
                )
            })
            .chain(
                dir.largest
                    .iter()
                    .map(|leaf| (leaf.usage, leaf.name.as_str(), Child::File(leaf))),
            )
            .collect();
        all.sort_by(|a, b| {
            b.0.disk
                .cmp(&a.0.disk)
                .then(b.0.apparent.cmp(&a.0.apparent))
                .then_with(|| a.1.cmp(b.1))
        });
        all.truncate(SHOWN);
        let mut shown = Usage::default();
        for (usage, _, _) in &all {
            shown.add(*usage);
        }
        let entries = dir.subdirs.len() as u64 + dir.files.files;
        let rest = entries.saturating_sub(all.len() as u64);
        let mut children: Vec<Child<'_>> = all.into_iter().map(|(_, _, c)| c).collect();
        if rest > 0 {
            children.push(Child::More {
                entries: rest,
                usage: dir.total.minus(dir.own).minus(shown),
            });
        }
        children
    }
}

enum Child<'a> {
    Sub(usize),
    File(&'a Leaf),
    More { entries: u64, usage: Usage },
}

/// Insert `leaf` among the largest by disk, keeping at most [`SHOWN`].
fn keep_largest(largest: &mut Vec<Leaf>, leaf: Leaf) {
    if largest.len() == SHOWN
        && largest
            .last()
            .is_some_and(|smallest| smallest.usage.disk >= leaf.usage.disk)
    {
        return;
    }
    let at = largest.partition_point(|l| l.usage.disk >= leaf.usage.disk);
    largest.insert(at, leaf);
    largest.truncate(SHOWN);
}

/// `part`'s share of `whole` in thousandths, by disk, or by length when
/// nothing is allocated.
fn share(part: Usage, whole: Usage) -> u32 {
    let (p, w) = if whole.disk > 0 {
        (part.disk, whole.disk)
    } else {
        (part.apparent, whole.apparent)
    };
    if w == 0 {
        return 0;
    }
    u32::try_from(u128::from(p) * 1000 / u128::from(w)).unwrap_or(1000)
}

/// One line of the laid-out tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// For each level between the root and this row, whether that level's
    /// branch goes on below it.
    pub rails: Vec<bool>,
    /// The last of its parent's children shown.
    pub last: bool,
    /// The directory it is listed in; `None` for the root.
    pub parent: Option<usize>,
    pub kind: RowKind,
    pub name: String,
    pub usage: Usage,
    /// Its share of its parent, in thousandths.
    pub share: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Dir {
        id: usize,
        open: bool,
        empty: bool,
    },
    File,
    /// The children too small to show, summed.
    More {
        entries: u64,
    },
}

/// How deep the tree is open, and the directories opened or closed against
/// that by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folds {
    /// Levels open below the root; `None` opens every one.
    pub depth: Option<usize>,
    flipped: HashSet<usize>,
}

impl Folds {
    /// Open `depth` levels below the root.
    pub fn new(depth: usize) -> Self {
        Self {
            depth: Some(depth.max(1)),
            flipped: HashSet::new(),
        }
    }

    /// Whether the directory `id`, `depth` below the root, shows its children.
    pub fn is_open(&self, id: usize, depth: usize) -> bool {
        self.depth.is_none_or(|limit| depth < limit) != self.flipped.contains(&id)
    }

    /// Open one more level, every level once none is left closed in a tree
    /// whose deepest directory is `deepest` below the root.
    pub fn deeper(&mut self, deepest: usize) {
        if let Some(limit) = self.depth {
            self.depth = (limit < deepest).then_some(limit + 1);
        }
        self.flipped.clear();
    }

    /// Close the deepest open level, keeping the root's children shown.
    pub fn shallower(&mut self, deepest: usize) {
        let limit = self.depth.unwrap_or(deepest + 1);
        self.depth = Some(limit.saturating_sub(1).max(1));
        self.flipped.clear();
    }

    /// Open every level.
    pub fn open_all(&mut self) {
        self.depth = None;
        self.flipped.clear();
    }

    /// Show the root's children only.
    pub fn close_all(&mut self) {
        self.depth = Some(1);
        self.flipped.clear();
    }

    /// Open the directory `id` if it is closed, close it if it is open.
    pub fn flip(&mut self, id: usize) {
        if !self.flipped.remove(&id) {
            self.flipped.insert(id);
        }
    }
}

/// Walk `root` into `tree`, calling `tick` every [`TICK`] while it runs; the
/// walk stops early once `tick` returns false.
pub fn scan(root: &Path, tree: &Mutex<DiskTree>, mut tick: impl FnMut() -> bool) {
    let stop = AtomicBool::new(false);
    let (tx, rx) = mpsc::sync_channel::<Option<Found>>(4096);
    std::thread::scope(|s| {
        let stop = &stop;
        s.spawn(move || walk(root, &tx, stop));
        let mut last = Instant::now();
        let mut batch: Vec<Option<Found>> = Vec::new();
        loop {
            let ended = match rx.recv_timeout(TICK) {
                Ok(found) => {
                    batch.push(found);
                    batch.extend(rx.try_iter().take(8192));
                    false
                }
                Err(mpsc::RecvTimeoutError::Timeout) => false,
                Err(mpsc::RecvTimeoutError::Disconnected) => true,
            };
            if !batch.is_empty() {
                let mut tree = tree.lock().unwrap_or_else(PoisonError::into_inner);
                for found in std::mem::take(&mut batch) {
                    match found {
                        Some(found) => tree.add(found),
                        None => tree.unreadable += 1,
                    }
                }
            }
            if ended {
                break;
            }
            if last.elapsed() >= TICK {
                last = Instant::now();
                if !tick() {
                    stop.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
        drop(rx);
    });
}

/// The walk itself: each entry it reaches, or `None` for one it can't read.
fn walk(root: &Path, tx: &mpsc::SyncSender<Option<Found>>, stop: &AtomicBool) {
    use ignore::WalkState;
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .standard_filters(false)
        .follow_links(false)
        .same_file_system(true);
    builder.build_parallel().run(|| {
        let tx = tx.clone();
        Box::new(move |entry| {
            if stop.load(Ordering::Relaxed) {
                return WalkState::Quit;
            }
            let found = entry.ok().and_then(|e| {
                let md = e.metadata().ok()?;
                Some(Found::of(e.into_path(), &md))
            });
            if tx.send(found).is_err() {
                return WalkState::Quit;
            }
            WalkState::Continue
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, disk: u64) -> Found {
        Found {
            path: PathBuf::from(path),
            is_dir: false,
            apparent: disk / 2,
            disk,
            link: None,
        }
    }

    fn dir(path: &str) -> Found {
        Found {
            path: PathBuf::from(path),
            is_dir: true,
            apparent: 0,
            disk: 0,
            link: None,
        }
    }

    fn tree(found: Vec<Found>) -> DiskTree {
        let mut t = DiskTree::new(PathBuf::from("/r"), "r".into());
        for f in found {
            t.add(f);
        }
        t
    }

    fn names(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r.kind {
                RowKind::More { entries } => format!("+{entries}"),
                _ => r.name.clone(),
            })
            .collect()
    }

    #[test]
    fn a_directory_holds_everything_beneath_it() {
        let t = tree(vec![
            dir("/r"),
            dir("/r/a"),
            file("/r/a/x", 100),
            dir("/r/a/b"),
            file("/r/a/b/y", 40),
            file("/r/z", 8),
        ]);
        let rows = t.rows(&Folds::new(3));
        let a = rows.iter().find(|r| r.name == "a").unwrap();
        assert_eq!(
            a.usage,
            Usage {
                apparent: 70,
                disk: 140,
                files: 2
            }
        );
        assert_eq!(t.total().disk, 148);
        assert_eq!(t.total().files, 3);
    }

    #[test]
    fn a_file_with_several_links_counts_once() {
        let mut one = file("/r/a", 100);
        one.link = Some((1, 7));
        let mut two = file("/r/b", 100);
        two.link = Some((1, 7));
        let t = tree(vec![dir("/r"), one, two, file("/r/c", 10)]);
        assert_eq!(t.total().disk, 110);
        assert_eq!(t.total().files, 2);
    }

    #[test]
    fn a_child_found_before_its_directory_is_still_counted_there() {
        let t = tree(vec![file("/r/a/b/x", 64), dir("/r/a")]);
        let rows = t.rows(&Folds::new(5));
        assert_eq!(names(&rows), ["r", "a", "b", "x"]);
        assert_eq!(rows[1].usage.disk, 64);
        assert_eq!(t.deepest(), 2);
    }

    #[test]
    fn a_path_outside_the_root_is_not_counted() {
        let t = tree(vec![dir("/r"), file("/elsewhere/x", 64)]);
        assert_eq!(t.total(), Usage::default());
    }

    #[test]
    fn children_come_largest_first_and_the_rest_fold_into_one_row() {
        let mut found = vec![dir("/r")];
        for i in 0..SHOWN + 5 {
            found.push(file(&format!("/r/f{i:02}"), 10 + i as u64));
        }
        let t = tree(found);
        let rows = t.rows(&Folds::new(1));
        assert_eq!(rows.len(), 1 + SHOWN + 1);
        assert_eq!(rows[1].name, format!("f{:02}", SHOWN + 4));
        let more = rows.last().unwrap();
        assert_eq!(more.kind, RowKind::More { entries: 5 });
        // f00..f04: 10 + 11 + 12 + 13 + 14.
        assert_eq!(more.usage.disk, 60);
        assert_eq!(more.usage.files, 5);
        assert!(more.last);
    }

    #[test]
    fn depth_opens_levels_and_a_flip_opens_or_closes_one_directory() {
        let t = tree(vec![
            dir("/r"),
            dir("/r/a"),
            dir("/r/a/b"),
            file("/r/a/b/x", 8),
            file("/r/y", 4),
        ]);
        let mut folds = Folds::new(1);
        assert_eq!(names(&t.rows(&folds)), ["r", "a", "y"]);
        folds.deeper(t.deepest());
        assert_eq!(names(&t.rows(&folds)), ["r", "a", "b", "y"]);
        folds.deeper(t.deepest());
        assert_eq!(folds.depth, None, "every level is open");
        assert_eq!(names(&t.rows(&folds)), ["r", "a", "b", "x", "y"]);
        folds.shallower(t.deepest());
        assert_eq!(names(&t.rows(&folds)), ["r", "a", "b", "y"]);
        folds.close_all();
        let a = t.rows(&folds)[1].kind;
        let RowKind::Dir {
            id, open: false, ..
        } = a
        else {
            panic!("a is closed at depth 1: {a:?}");
        };
        folds.flip(id);
        assert_eq!(names(&t.rows(&folds)), ["r", "a", "b", "y"]);
        folds.flip(id);
        assert_eq!(names(&t.rows(&folds)), ["r", "a", "y"]);
        folds.shallower(t.deepest());
        assert_eq!(folds.depth, Some(1), "the root's children stay shown");
    }

    #[test]
    fn rails_mark_the_branches_that_go_on() {
        let t = tree(vec![
            dir("/r"),
            dir("/r/a"),
            file("/r/a/x", 8),
            file("/r/y", 4),
        ]);
        let rows = t.rows(&Folds::new(3));
        let x = rows.iter().find(|r| r.name == "x").unwrap();
        assert_eq!(x.rails, [true], "a has a sibling below it");
        assert!(x.last);
        let y = rows.iter().find(|r| r.name == "y").unwrap();
        assert_eq!(y.rails, [false; 0]);
        assert!(y.last);
    }

    #[test]
    fn a_scan_counts_hard_links_once_and_never_follows_a_link() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/data"), vec![7u8; 10_000]).unwrap();
        std::fs::hard_link(root.join("sub/data"), root.join("again")).unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("big"), vec![1u8; 50_000]).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let t = Mutex::new(DiskTree::new(root.clone(), "root".into()));
        scan(&root, &t, || true);
        let t = t.into_inner().unwrap();
        let total = t.total();
        assert_eq!(total.files, 2, "the data once and the link itself");
        assert!(
            total.apparent < 20_000,
            "nothing behind the link, and the data once: {total:?}"
        );
        assert!(total.apparent >= 10_000);
        assert_eq!(t.unreadable, 0);
    }
}
