//! Off-thread filesystem-watch control worker.
//!
//! `notify`'s inotify backend (Linux) does a *synchronous* per-subdir
//! `inotify_add_watch` walk on the calling thread — many milliseconds on a
//! `$HOME`-shaped tree, enough to stall the event loop. So the watcher lives on
//! this worker; the main loop only sends [`WatchCommand`]s and the blocking
//! (un)watch syscalls happen here. (macOS FSEvents / Windows are OS-level and
//! don't pay this cost, but the seam is uniform.)
//!
//! Trade-off on Linux: a recursive watch of a huge tree registers an inotify
//! watch per subdir; if it hits `fs.inotify.max_user_watches` the dir is left
//! unwatched and the 1 Hz git poll covers marker refresh. Events still arrive
//! on the unified channel as [`Message::FsEvent`].

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use notify::event::{AccessKind, AccessMode};
use notify::{EventKind, RecursiveMode, Watcher};

use super::{App, Message, RunCtx};

/// True for a pure read/open watcher event — one that never changes a file's
/// content or the directory structure, so a file manager has nothing to react
/// to. Dropped at the callback so it never reaches the loop.
///
/// This is load-bearing on Linux: `notify`'s inotify mask includes `IN_OPEN`
/// (→ `Access(Open)`) and `IN_CLOSE_NOWRITE` (→ `Access(Close(Read))`). spyc's
/// own git-status / gitignore-excludes machinery opens every `.gitignore` in
/// the tree to build the ignore stack; under the recursive watch each open
/// fires an `Access(Open)` event, which drives a refresh that opens them
/// again — a self-sustaining storm (observed at ~21 k events/s in a small
/// repo). macOS FSEvents never reports opens, so it's invisible in dev.
/// `Access(Close(Write))` (a real write completion) and every Create / Modify
/// / Remove event pass through.
// SPYC-TRAP(fs-watch-readonly-access): dropping open/read events breaks a
// self-sustaining inotify storm invisible on macOS (FSEvents has no opens).
const fn is_readonly_access(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::Access(
            AccessKind::Open(_) | AccessKind::Read | AccessKind::Close(AccessMode::Read)
        )
    )
}

/// Watch-topology change requested by the event loop. The worker owns the
/// actual watch state; the main loop just describes the target topology.
pub enum WatchCommand {
    /// Re-point the recursive listing watch onto `dir`, the non-recursive
    /// gitdir watch onto `gitdir`, and the non-recursive preview-parent watch
    /// onto `preview`'s parent dir (unwatching the previous targets). Sent
    /// whenever the listing cwd OR the open vertical-split preview changes.
    SyncListing {
        dir: PathBuf,
        gitdir: Option<PathBuf>,
        /// The **second** commander's (column `b`) listing dir + gitdir, watched
        /// the same way as the primary's so `b`'s git markers refresh on
        /// fs-events too (not just the ≤1 s poll PR E left). `None` when no
        /// second commander is open.
        dir_right: Option<PathBuf>,
        gitdir_right: Option<PathBuf>,
        /// The vertical-split preview's source file (`None` when no split is
        /// open). Its *parent* dir is watched non-recursively so a
        /// replace-on-save survives (file-level watches go deaf on the rename)
        /// — but only when that parent lies OUTSIDE the recursive listing watch,
        /// which already delivers events for files beneath it.
        preview: Option<PathBuf>,
    },
    /// Watch exactly these config-file parent dirs, non-recursively: the
    /// candidates of every open project (`App::config_parents`). Sent when
    /// the set changes, as a start dir moves or a project opens or closes.
    SyncConfigParents(Vec<PathBuf>),
}

impl App {
    /// Re-point the worker when the open projects' config dirs changed, or a
    /// column's listing dir or the open vertical-split preview moved; the (un)watch syscalls run on the worker,
    /// and `ctx` keeps the send-dedup keys. Names `a` and `b` by field rather
    /// than by focus: each column's tree and gitdir are watched under their own
    /// key whichever one has the keyboard.
    pub(super) fn sync_watch(&self, ctx: &mut RunCtx) {
        let config = self.config_parents();
        if config != ctx.watched_config
            && let Some(tx) = ctx.watch_tx.as_ref()
        {
            let _ = tx.send(WatchCommand::SyncConfigParents(config.clone()));
            ctx.watched_config = config;
        }
        let preview = self
            .view
            .project()
            .right_pager
            .as_ref()
            .and_then(|v| v.source_path.clone());
        let dir = &self.state.active_project().left.listing.dir;
        let dir_right = self
            .state
            .active_project()
            .right
            .as_ref()
            .map(|c| c.listing.dir.clone());
        let unchanged = ctx.watched_listing.as_ref() == Some(dir)
            && ctx.watched_preview == preview
            && ctx.watched_listing_right == dir_right;
        let Some(tx) = ctx.watch_tx.as_ref().filter(|_| !unchanged) else {
            return;
        };
        let _ = tx.send(WatchCommand::SyncListing {
            dir: dir.clone(),
            gitdir: self
                .state
                .active_project()
                .left
                .git_cache
                .current_gitdir
                .clone(),
            dir_right: dir_right.clone(),
            gitdir_right: self
                .state
                .active_project()
                .right
                .as_ref()
                .and_then(|c| c.git_cache.current_gitdir.clone()),
            preview: preview.clone(),
        });
        ctx.watched_listing = Some(dir.clone());
        ctx.watched_preview = preview;
        ctx.watched_listing_right = dir_right;
    }
}

/// Spawn the watch-control worker. Returns the command sender, or `None` if
/// the watcher couldn't be created (degrades to poll-only, same as before).
///
/// The worker builds and owns the `RecommendedWatcher` so neither the
/// watcher nor its blocking (un)watch syscalls ever touch the main thread.
/// `config_parents` are the parent directories of the config files, watched
/// non-recursively up front. The worker terminates (and drops the watcher,
/// stopping `notify`'s thread) when the returned sender drops at teardown —
/// the same detached-thread lifecycle as the git/MCP forwarders.
pub fn spawn_watch_worker(
    msg_tx: &Sender<Message>,
    config_parents: Vec<PathBuf>,
) -> Option<Sender<WatchCommand>> {
    // The watcher posts each `Ok(Event)` onto the unified channel as
    // `Message::FsEvent`, dropping `Err` at the boundary (preserving the
    // prior Ok-only drain contract).
    let watcher_tx = msg_tx.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res
            && !is_readonly_access(ev.kind)
        {
            let _ = watcher_tx.send(Message::FsEvent(ev));
        }
    })
    .ok()?;

    // Config files are watched via their *parent* directories, not the files,
    // because editors that replace-on-save (vim, VS Code, nvim) remove the old
    // inode before creating the new one. Non-recursive; small set.
    let mut slots = Slots::default();
    slots.sync_config(&mut watcher, config_parents);

    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<WatchCommand>();
    std::thread::spawn(move || {
        // Owned by the worker: the dirs the watcher currently holds. The main
        // loop tracks the last-requested targets separately, only to avoid
        // sending redundant commands.
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                WatchCommand::SyncListing {
                    dir,
                    gitdir,
                    dir_right,
                    gitdir_right,
                    preview,
                } => slots.sync_listing(
                    &mut watcher,
                    &dir,
                    gitdir.as_deref(),
                    dir_right.as_deref(),
                    gitdir_right.as_deref(),
                    preview.as_deref(),
                ),
                WatchCommand::SyncConfigParents(parents) => {
                    slots.sync_config(&mut watcher, parents);
                }
            }
        }
        // cmd_tx dropped at teardown → recv errs → drop `watcher` here, which
        // stops notify's internal thread.
    });
    Some(cmd_tx)
}

/// The two calls the worker makes on its watcher, so the bookkeeping in
/// [`Slots`] can be tested against a stand-in.
trait Watches {
    /// Watch `dir`; whether it took.
    fn watch(&mut self, dir: &Path, mode: RecursiveMode) -> bool;
    fn unwatch(&mut self, dir: &Path);
}

impl Watches for notify::RecommendedWatcher {
    fn watch(&mut self, dir: &Path, mode: RecursiveMode) -> bool {
        Watcher::watch(self, dir, mode).is_ok()
    }

    fn unwatch(&mut self, dir: &Path) {
        let _ = Watcher::unwatch(self, dir);
    }
}

/// What the watcher holds, slot by slot, and the config dirs it watches.
/// `notify` keeps one watch per path, and on inotify unwatching a dir a
/// recursive watch covers drops every watch beneath it, so a dir a slot
/// covers is never watched or unwatched on its own, and what a slot lets go
/// of is put back beneath it ([`Slots::restore_under`]).
#[derive(Default)]
struct Slots {
    listing: Option<PathBuf>,
    git: Option<PathBuf>,
    listing_right: Option<PathBuf>,
    git_right: Option<PathBuf>,
    preview: Option<PathBuf>,
    /// The config files' parent dirs.
    config: Vec<PathBuf>,
    /// Those of them that didn't exist when last synced, so not watched: a
    /// resend tries them again.
    config_missing: Vec<PathBuf>,
}

impl Slots {
    /// Reconcile every listing-side slot to the requested topology. Runs on
    /// the watch worker, so the blocking recursive `inotify_add_watch` walk
    /// never stalls the event loop.
    fn sync_listing(
        &mut self,
        w: &mut impl Watches,
        dir: &Path,
        gitdir: Option<&Path>,
        dir_right: Option<&Path>,
        gitdir_right: Option<&Path>,
        preview: Option<&Path>,
    ) {
        // Recursive: catches changes anywhere below the listing dir so git
        // status markers update on the parent directory row when a file is
        // added/modified in a subdirectory (e.g. touching `docs/foo.md` while
        // sitting at the repo root). Events under `.git/` are filtered to
        // specific files (`index`, `HEAD`) by `is_listing_path` to avoid
        // `.git/objects` / pack / lockfile churn cascading into needless `git
        // status` calls. On `Err` (e.g. Linux inotify watch-limit on a huge
        // tree) the dir is left unwatched and the 1 Hz git poll carries marker
        // refresh.
        self.sync_slot(w, |s| &mut s.listing, Some(dir), RecursiveMode::Recursive);
        // Watch the repo's *resolved* gitdir non-recursively. For a normal repo
        // that's `<root>/.git`; for a linked worktree it's
        // `<main>/.git/worktrees/<name>/` (resolved from the `.git` *file*),
        // which lives OUTSIDE the working tree — without watching it, a
        // worktree's index/HEAD changes (stage, commit, checkout, branch
        // switch) never fire the watcher and markers only refresh on the slower
        // periodic poll. We can't watch the `index` *file* directly: git
        // commits via atomic rename (write `index.lock`, rename to `index`),
        // which replaces the inode — a file-level watch follows the *old* inode
        // and goes deaf. A directory watch sees the rename land. NonRecursive
        // bounds the noise even with huge `.git/objects` trees. `gitdir` is
        // resolved + cached on chdir (`current_gitdir`).
        self.sync_slot(w, |s| &mut s.git, gitdir, RecursiveMode::NonRecursive);
        // Watch the vertical-split preview's *parent* dir non-recursively (same
        // replace-on-save rationale as the config + gitdir watches: a
        // file-level watch follows the old inode through an editor's atomic
        // rename and goes deaf).
        let want_preview = preview_parent_to_watch(preview, dir);
        self.sync_slot(
            w,
            |s| &mut s.preview,
            want_preview.as_deref(),
            RecursiveMode::NonRecursive,
        );
        // Column `b`: same recursive-tree + non-recursive-gitdir watches as the
        // primary, so `b`'s working-tree edits and index/HEAD changes fire
        // events too. `None` dir → unwatch.
        self.sync_slot(
            w,
            |s| &mut s.listing_right,
            dir_right,
            RecursiveMode::Recursive,
        );
        self.sync_slot(
            w,
            |s| &mut s.git_right,
            gitdir_right,
            RecursiveMode::NonRecursive,
        );
    }

    /// Reconcile one slot to `want` in `mode`, letting go of its old dir
    /// first. On a `watch` error the slot is left empty (the poll carries
    /// refresh).
    fn sync_slot(
        &mut self,
        w: &mut impl Watches,
        slot: fn(&mut Self) -> &mut Option<PathBuf>,
        want: Option<&Path>,
        mode: RecursiveMode,
    ) {
        if slot(self).as_deref() == want {
            return;
        }
        if let Some(old) = slot(self).take()
            && !self.covers(&old)
        {
            w.unwatch(&old);
            self.restore_under(w, &old, mode);
        }
        if let Some(dir) = want
            && w.watch(dir, mode)
        {
            *slot(self) = Some(dir.to_path_buf());
        }
    }

    /// Put back what still wants a watch after a slot let go of `dir`, held in
    /// `mode`. On inotify, unwatching a dir watched recursively drops every
    /// watch beneath it too, so each slot and config dir beneath it is
    /// watched again; a non-recursive watch takes only its own dir, a config
    /// dir there included. Each goes through [`rewatch`], so FSEvents, which
    /// dropped nothing beneath, isn't handed a second copy of a path.
    fn restore_under(&self, w: &mut impl Watches, dir: &Path, mode: RecursiveMode) {
        let recursive = matches!(mode, RecursiveMode::Recursive);
        if recursive {
            for kept in self.recursive().filter(|r| r.starts_with(dir)) {
                rewatch(w, kept, RecursiveMode::Recursive);
            }
            for kept in self.flat().filter(|f| f.starts_with(dir)) {
                rewatch(w, kept, RecursiveMode::NonRecursive);
            }
        }
        for kept in self.config.iter().filter(|c| {
            (if recursive {
                c.starts_with(dir)
            } else {
                *c == dir
            }) && !self.covers(c)
        }) {
            rewatch(w, kept, RecursiveMode::NonRecursive);
        }
    }

    /// Watch exactly `parents` as config dirs. One no longer named is
    /// unwatched, and a new one watched, unless a slot covers it either way;
    /// one that didn't exist last time is tried again.
    fn sync_config(&mut self, w: &mut impl Watches, parents: Vec<PathBuf>) {
        let mut wanted: Vec<PathBuf> = Vec::new();
        for parent in parents {
            if !wanted.contains(&parent) {
                wanted.push(parent);
            }
        }
        let old = std::mem::replace(&mut self.config, wanted);
        for gone in old.iter().filter(|p| !self.config.contains(p)) {
            if !self.covers(gone) {
                w.unwatch(gone);
            }
        }
        let missing = std::mem::take(&mut self.config_missing);
        for new in self
            .config
            .iter()
            .filter(|p| !old.contains(p) || missing.contains(p))
        {
            if self.covers(new) {
                continue;
            }
            if new.is_dir() {
                w.watch(new, RecursiveMode::NonRecursive);
            } else {
                self.config_missing.push(new.clone());
            }
        }
    }

    /// The dirs the recursive slots hold.
    fn recursive(&self) -> impl Iterator<Item = &Path> {
        [&self.listing, &self.listing_right]
            .into_iter()
            .filter_map(Option::as_deref)
    }

    /// The dirs the non-recursive slots hold.
    fn flat(&self) -> impl Iterator<Item = &Path> {
        [&self.git, &self.git_right, &self.preview]
            .into_iter()
            .filter_map(Option::as_deref)
    }

    /// Whether a slot covers `dir`: holds it, or holds a dir above it
    /// recursively. A covered dir is never watched or unwatched on its own:
    /// a second watch on one path replaces the first's mode, and unwatching a
    /// dir a recursive watch walked into cuts a hole in it.
    fn covers(&self, dir: &Path) -> bool {
        self.recursive().any(|r| dir.starts_with(r)) || self.flat().any(|f| f == dir)
    }
}

/// Watch `dir` afresh: dropped first, so a backend that kept it (FSEvents,
/// which appends a path on each `watch`) holds one copy, not two.
fn rewatch(w: &mut impl Watches, dir: &Path, mode: RecursiveMode) {
    w.unwatch(dir);
    w.watch(dir, mode);
}

/// The parent dir to watch for a vertical-split `preview` file, or `None` when
/// no separate watch is needed. Returns the preview's parent — but only when it
/// lies OUTSIDE the recursive listing watch (`new_dir`): a parent at or under
/// the listing dir already gets the file's events from that recursive watch, and
/// double-watching the same dir then unwatching one of them can interfere on
/// inotify. So this covers only a preview whose file lives outside the cwd (e.g.
/// after browsing away with the split still open).
fn preview_parent_to_watch(preview: Option<&Path>, new_dir: &Path) -> Option<PathBuf> {
    preview
        .and_then(Path::parent)
        .filter(|pp| !pp.starts_with(new_dir))
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::{Slots, Watches, is_readonly_access, preview_parent_to_watch};
    use notify::event::{AccessKind, AccessMode, CreateKind, DataChange, MetadataKind, ModifyKind};
    use notify::{EventKind, RecursiveMode};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// A watcher that keeps one watch per path, as `notify` does.
    #[derive(Default)]
    struct OnePerPath(HashMap<PathBuf, RecursiveMode>);

    impl Watches for OnePerPath {
        fn watch(&mut self, dir: &Path, mode: RecursiveMode) -> bool {
            self.0.insert(dir.to_path_buf(), mode);
            true
        }

        fn unwatch(&mut self, dir: &Path) {
            self.0.remove(dir);
        }
    }

    fn listing_at(slots: &mut Slots, w: &mut impl Watches, dir: &Path) {
        slots.sync_listing(w, dir, None, None, None, None);
    }

    /// A watcher with inotify's semantics, as `notify` drives it: one watch
    /// per path, a recursive watch standing for every dir beneath it, and
    /// unwatching a dir watched recursively, or one a recursive watch walked
    /// into, dropping every watch at or beneath it.
    #[derive(Default)]
    struct Inotify {
        watches: HashMap<PathBuf, RecursiveMode>,
        /// Dirs beneath a recursive watch that lost theirs.
        holes: Vec<PathBuf>,
    }

    impl Inotify {
        fn walked_into(&self, dir: &Path) -> bool {
            self.watches
                .iter()
                .any(|(d, m)| *m == RecursiveMode::Recursive && dir.starts_with(d) && d != dir)
        }

        /// Whether an event in `dir` reaches the watcher.
        fn hears(&self, dir: &Path) -> bool {
            self.watches.contains_key(dir)
                || (self
                    .watches
                    .iter()
                    .any(|(d, m)| *m == RecursiveMode::Recursive && dir.starts_with(d))
                    && !self.holes.iter().any(|h| dir.starts_with(h)))
        }
    }

    impl Watches for Inotify {
        fn watch(&mut self, dir: &Path, mode: RecursiveMode) -> bool {
            self.holes.retain(|h| match mode {
                RecursiveMode::Recursive => !h.starts_with(dir),
                RecursiveMode::NonRecursive => h != dir,
            });
            self.watches.insert(dir.to_path_buf(), mode);
            true
        }

        fn unwatch(&mut self, dir: &Path) {
            let walked_into = self.walked_into(dir);
            if walked_into || self.watches.get(dir) == Some(&RecursiveMode::Recursive) {
                self.watches.retain(|d, _| !d.starts_with(dir));
                if walked_into {
                    self.holes.push(dir.to_path_buf());
                }
            } else {
                self.watches.remove(dir);
            }
        }
    }

    /// A start dir dropped from the config dirs while a column's recursive
    /// watch covers it (`h` up from it, then `gS` there) is left to that
    /// watch: unwatching it would cut a hole in the column's tree.
    #[test]
    fn a_config_dir_a_listing_covers_is_never_unwatched_on_its_own() {
        let tmp = tempfile::tempdir().unwrap();
        let (src, repo) = (tmp.path().to_path_buf(), tmp.path().join("repo"));
        std::fs::create_dir(&repo).unwrap();
        let (mut slots, mut w) = (Slots::default(), Inotify::default());
        slots.sync_config(&mut w, vec![repo.clone()]);
        listing_at(&mut slots, &mut w, &repo);
        listing_at(&mut slots, &mut w, &src);
        slots.sync_config(&mut w, vec![src.clone()]);
        assert!(w.hears(&repo.join("deep")), "a hole in the column's tree");
    }

    /// Leaving a dir watched recursively (`~`) puts back every config watch
    /// beneath it, not only one on the dir itself.
    #[test]
    fn leaving_a_dir_puts_back_the_config_watches_beneath_it() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let (lua, proj, elsewhere) = (
            home.join("config"),
            home.join("proj"),
            tmp.path().join("elsewhere"),
        );
        for d in [&lua, &proj, &elsewhere] {
            std::fs::create_dir_all(d).unwrap();
        }
        let (mut slots, mut w) = (Slots::default(), Inotify::default());
        slots.sync_config(&mut w, vec![home.clone(), lua.clone(), proj.clone()]);
        listing_at(&mut slots, &mut w, &home);
        listing_at(&mut slots, &mut w, &elsewhere);
        for dir in [&home, &lua, &proj] {
            assert!(w.hears(dir), "{} went unwatched", dir.display());
        }
    }

    /// The listing leaving the repo root (`l` into `src`) unwatches it, which
    /// on inotify takes the gitdir's watch beneath it: the gitdir is watched
    /// again.
    #[test]
    fn a_gitdir_beneath_a_dir_the_listing_leaves_is_watched_again() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().to_path_buf();
        let (git, src) = (repo.join(".git"), repo.join("src"));
        for d in [&git, &src] {
            std::fs::create_dir(d).unwrap();
        }
        let (mut slots, mut w) = (Slots::default(), Inotify::default());
        slots.sync_listing(&mut w, &repo, Some(&git), None, None, None);
        slots.sync_listing(&mut w, &src, Some(&git), None, None, None);
        assert!(w.hears(&git), "index and HEAD changes go unheard");
    }

    /// Column `a` leaving a dir column `b` stands beneath takes `b`'s
    /// watches on inotify: `b`'s tree is watched again.
    #[test]
    fn a_column_beneath_a_dir_another_column_leaves_is_watched_again() {
        let tmp = tempfile::tempdir().unwrap();
        let (outer, inner) = (tmp.path().to_path_buf(), tmp.path().join("inner"));
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::create_dir(&inner).unwrap();
        let (mut slots, mut w) = (Slots::default(), Inotify::default());
        slots.sync_listing(&mut w, &outer, None, Some(&inner), None, None);
        slots.sync_listing(&mut w, elsewhere.path(), None, Some(&inner), None, None);
        assert!(w.hears(&inner.join("deep")), "column b's tree went deaf");
    }

    /// A watcher with FSEvents' list semantics, as `notify` drives it: each
    /// `watch` appends its path, and `unwatch` drops every copy. `calls`
    /// logs every `watch`, kept or not.
    #[derive(Default)]
    struct FsEvents {
        paths: Vec<(PathBuf, RecursiveMode)>,
        calls: Vec<PathBuf>,
    }

    impl Watches for FsEvents {
        fn watch(&mut self, dir: &Path, mode: RecursiveMode) -> bool {
            self.paths.push((dir.to_path_buf(), mode));
            self.calls.push(dir.to_path_buf());
            true
        }

        fn unwatch(&mut self, dir: &Path) {
            self.paths.retain(|(d, _)| d != dir);
        }
    }

    /// Leaving and coming back to a dir over and over hands FSEvents one copy
    /// of each path beneath it, however many times it was put back.
    #[test]
    fn putting_watches_back_hands_fsevents_no_second_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().to_path_buf();
        let (git, src) = (repo.join(".git"), repo.join("src"));
        for d in [&git, &src] {
            std::fs::create_dir(d).unwrap();
        }
        let (mut slots, mut w) = (Slots::default(), FsEvents::default());
        for _ in 0..3 {
            slots.sync_listing(&mut w, &repo, Some(&git), None, None, None);
            slots.sync_listing(&mut w, &src, Some(&git), None, None, None);
        }
        let copies = w.paths.iter().filter(|(d, _)| *d == git).count();
        assert_eq!(copies, 1, "{:?}", w.paths);
    }

    /// A non-recursive slot letting go of a dir (the preview's parent, above
    /// the listing) takes no watch beneath it, so the listing isn't walked
    /// again.
    #[test]
    fn a_flat_slot_letting_go_walks_no_listing_again() {
        let tmp = tempfile::tempdir().unwrap();
        let (parent, listing) = (tmp.path().to_path_buf(), tmp.path().join("listing"));
        std::fs::create_dir(&listing).unwrap();
        let preview = parent.join("doc.md");
        let (mut slots, mut w) = (Slots::default(), FsEvents::default());
        slots.sync_listing(&mut w, &listing, None, None, None, Some(&preview));
        slots.sync_listing(&mut w, &listing, None, None, None, None);
        let walks = w.calls.iter().filter(|d| **d == listing).count();
        assert_eq!(walks, 1, "the listing was walked again: {:?}", w.calls);
    }

    /// Column `b` in a dir inside column `a`'s tree: when `b` leaves, `a`'s
    /// tree stays whole, since unwatching a dir a recursive watch walked into
    /// cuts a hole in it.
    #[test]
    fn a_column_leaving_a_dir_inside_another_columns_tree_leaves_it_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let (outer, inner) = (tmp.path().to_path_buf(), tmp.path().join("inner"));
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::create_dir(&inner).unwrap();
        let (mut slots, mut w) = (Slots::default(), Inotify::default());
        slots.sync_listing(&mut w, &outer, None, Some(&inner), None, None);
        slots.sync_listing(&mut w, &outer, None, Some(elsewhere.path()), None, None);
        assert!(w.hears(&inner.join("deep")), "column a's tree has a hole");
    }

    /// A project's start dir is a config dir, and a column stands in it at
    /// first. Moving the column away unwatches the dir, which takes the config
    /// watch with it, so the config watch is put back.
    #[test]
    fn a_config_dir_a_listing_leaves_is_watched_again() {
        let tmp = tempfile::tempdir().unwrap();
        let (start, elsewhere) = (tmp.path().join("start"), tmp.path().join("elsewhere"));
        for d in [&start, &elsewhere] {
            std::fs::create_dir(d).unwrap();
        }
        let (mut slots, mut w) = (Slots::default(), OnePerPath::default());
        slots.sync_config(&mut w, vec![start.clone()]);
        listing_at(&mut slots, &mut w, &start);
        listing_at(&mut slots, &mut w, &elsewhere);
        assert_eq!(w.0.get(&start), Some(&RecursiveMode::NonRecursive));
        assert_eq!(w.0.get(&elsewhere), Some(&RecursiveMode::Recursive));
    }

    /// A config dir a column stands in keeps the column's recursive watch: a
    /// non-recursive config watch over it would stop events from below.
    #[test]
    fn a_config_dir_a_listing_holds_keeps_its_recursive_watch() {
        let tmp = tempfile::tempdir().unwrap();
        let start = tmp.path().to_path_buf();
        let (mut slots, mut w) = (Slots::default(), OnePerPath::default());
        listing_at(&mut slots, &mut w, &start);
        slots.sync_config(&mut w, vec![start.clone()]);
        assert_eq!(w.0.get(&start), Some(&RecursiveMode::Recursive));
    }

    /// A config dir that doesn't exist yet is watched on the first resend
    /// after it does, though the set sent is the same.
    #[test]
    fn a_config_dir_made_later_is_watched_on_a_resend() {
        let tmp = tempfile::tempdir().unwrap();
        let later = tmp.path().join("later");
        let (mut slots, mut w) = (Slots::default(), OnePerPath::default());
        slots.sync_config(&mut w, vec![later.clone()]);
        assert_eq!(w.0.get(&later), None);
        std::fs::create_dir(&later).unwrap();
        slots.sync_config(&mut w, vec![later.clone()]);
        assert_eq!(w.0.get(&later), Some(&RecursiveMode::NonRecursive));
    }

    /// A config dir no longer named is unwatched, unless a column stands in it.
    #[test]
    fn a_config_dir_no_longer_named_is_unwatched_unless_a_listing_holds_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (gone, held) = (tmp.path().join("gone"), tmp.path().join("held"));
        for d in [&gone, &held] {
            std::fs::create_dir(d).unwrap();
        }
        let (mut slots, mut w) = (Slots::default(), OnePerPath::default());
        slots.sync_config(&mut w, vec![gone.clone(), held.clone()]);
        listing_at(&mut slots, &mut w, &held);
        slots.sync_config(&mut w, Vec::new());
        assert_eq!(w.0.get(&gone), None);
        assert_eq!(w.0.get(&held), Some(&RecursiveMode::Recursive));
    }
    #[test]
    fn readonly_access_events_are_dropped() {
        // The inotify `IN_OPEN` / `IN_CLOSE_NOWRITE` storm: pure reads.
        assert!(is_readonly_access(EventKind::Access(AccessKind::Open(
            AccessMode::Any
        ))));
        assert!(is_readonly_access(EventKind::Access(AccessKind::Read)));
        assert!(is_readonly_access(EventKind::Access(AccessKind::Close(
            AccessMode::Read
        ))));
    }

    #[test]
    fn mutating_events_pass_through() {
        // `Access(Close(Write))` is a real write completion — it must survive.
        assert!(!is_readonly_access(EventKind::Access(AccessKind::Close(
            AccessMode::Write
        ))));
        assert!(!is_readonly_access(EventKind::Create(CreateKind::File)));
        assert!(!is_readonly_access(EventKind::Modify(ModifyKind::Data(
            DataChange::Any
        ))));
        assert!(!is_readonly_access(EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::Any)
        )));
        assert!(!is_readonly_access(EventKind::Modify(ModifyKind::Name(
            notify::event::RenameMode::Both
        ))));
    }

    #[test]
    fn no_preview_means_no_watch() {
        assert_eq!(preview_parent_to_watch(None, Path::new("/repo")), None);
    }

    #[test]
    fn preview_under_listing_dir_is_covered_by_recursive_watch() {
        // File directly in the cwd, or in a subdir of it → the recursive
        // listing watch already delivers its events, so no separate watch.
        assert_eq!(
            preview_parent_to_watch(Some(Path::new("/repo/doc.md")), Path::new("/repo")),
            None
        );
        assert_eq!(
            preview_parent_to_watch(Some(Path::new("/repo/docs/sub/doc.md")), Path::new("/repo")),
            None
        );
    }

    #[test]
    fn preview_outside_listing_dir_watches_its_parent() {
        // Browsed away with the split still open: the file's dir is no longer
        // under the listing watch, so watch its parent non-recursively.
        assert_eq!(
            preview_parent_to_watch(Some(Path::new("/other/place/doc.md")), Path::new("/repo")),
            Some(PathBuf::from("/other/place"))
        );
    }
}
