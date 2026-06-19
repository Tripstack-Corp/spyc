//! Off-thread filesystem-watch control worker.
//!
//! `notify`'s inotify backend (Linux) registers one watch per directory, and
//! a `RecursiveMode::Recursive` `watch()` does a *synchronous* per-subdir
//! `inotify_add_watch` walk on the calling thread. On a `$HOME`-shaped tree
//! (`anaconda3/`, multiple `node_modules/`, `.cache/`, …) that walk runs for
//! many milliseconds — long enough to stall the event loop if it ran there.
//! macOS FSEvents and Windows `ReadDirectoryChangesW` are OS-level and don't
//! pay this cost, but the seam is uniform across platforms.
//!
//! So the `RecommendedWatcher` (and thus `notify`'s internal thread) lives on
//! this worker, never on the event-loop/input thread. The main loop only
//! sends [`WatchCommand`]s; the blocking (un)watch syscalls happen here. This
//! is what let us delete the old Linux `MAX_RECURSIVE_WATCH_DIRS` cap — the
//! cap existed solely to bound that on-thread walk, and off-thread there's
//! nothing to bound. The trade-off it leaves: on Linux a recursive watch of a
//! genuinely huge tree registers an inotify watch per subdir (real kernel
//! memory, and `watch()` returns `Err` if it hits `fs.inotify.max_user_watches`,
//! in which case the dir is simply left unwatched and the 1 Hz git poll
//! covers marker refresh).
//!
//! Events still arrive on the unified channel as [`Message::FsEvent`] exactly
//! as before — only the watch *control* moved off-thread, not the delivery.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use notify::{RecursiveMode, Watcher};

use super::Message;

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
        /// The vertical-split preview's source file (`None` when no split is
        /// open). Its *parent* dir is watched non-recursively so a
        /// replace-on-save survives (file-level watches go deaf on the rename)
        /// — but only when that parent lies OUTSIDE the recursive listing watch,
        /// which already delivers events for files beneath it.
        preview: Option<PathBuf>,
    },
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
        if let Ok(ev) = res {
            let _ = watcher_tx.send(Message::FsEvent(ev));
        }
    })
    .ok()?;

    // Config files are watched via their *parent* directories, not the files,
    // because editors that replace-on-save (vim, VS Code, nvim) remove the old
    // inode before creating the new one. Non-recursive; small set.
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for parent in config_parents {
        if parent.is_dir() && seen.insert(parent.clone()) {
            let _ = watcher.watch(&parent, RecursiveMode::NonRecursive);
        }
    }

    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<WatchCommand>();
    std::thread::spawn(move || {
        // Owned by the worker: the dirs the watcher currently holds. The main
        // loop tracks the last-requested listing dir separately, only to avoid
        // sending redundant commands.
        let mut active_listing: Option<PathBuf> = None;
        let mut active_git: Option<PathBuf> = None;
        let mut active_preview: Option<PathBuf> = None;
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                WatchCommand::SyncListing {
                    dir,
                    gitdir,
                    preview,
                } => sync_listing(
                    &mut watcher,
                    &mut active_listing,
                    &mut active_git,
                    &mut active_preview,
                    &dir,
                    gitdir.as_deref(),
                    preview.as_deref(),
                ),
            }
        }
        // cmd_tx dropped at teardown → recv errs → drop `watcher` here, which
        // stops notify's internal thread.
    });
    Some(cmd_tx)
}

/// Reconcile the recursive listing watch and the non-recursive gitdir watch
/// to the requested topology. Runs on the watch worker, so the blocking
/// recursive `inotify_add_watch` walk never stalls the event loop.
fn sync_listing(
    watcher: &mut notify::RecommendedWatcher,
    active: &mut Option<PathBuf>,
    active_git: &mut Option<PathBuf>,
    active_preview: &mut Option<PathBuf>,
    new_dir: &Path,
    gitdir: Option<&Path>,
    preview: Option<&Path>,
) {
    if active.as_deref() != Some(new_dir) {
        if let Some(old) = active.as_ref() {
            let _ = watcher.unwatch(old);
        }
        // Recursive: catches changes anywhere below the listing dir so git
        // status markers update on the parent directory row when a file is
        // added/modified in a subdirectory (e.g. touching `docs/foo.md` while
        // sitting at the repo root). Events under `.git/` are filtered to
        // specific files (`index`, `HEAD`) by `is_listing_path` to avoid
        // `.git/objects` / pack / lockfile churn cascading into needless `git
        // status` calls. On `Err` (e.g. Linux inotify watch-limit on a huge
        // tree) the dir is left unwatched and the 1 Hz git poll carries
        // marker refresh.
        *active = watcher
            .watch(new_dir, RecursiveMode::Recursive)
            .is_ok()
            .then(|| new_dir.to_path_buf());
    }
    // Watch the repo's *resolved* gitdir non-recursively. For a normal repo
    // that's `<root>/.git`; for a linked worktree it's
    // `<main>/.git/worktrees/<name>/` (resolved from the `.git` *file*), which
    // lives OUTSIDE the working tree — without watching it, a worktree's
    // index/HEAD changes (stage, commit, checkout, branch switch) never fire
    // the watcher and markers only refresh on the slower periodic poll. We
    // can't watch the `index` *file* directly: git commits via atomic rename
    // (write `index.lock`, rename to `index`), which replaces the inode — a
    // file-level watch follows the *old* inode and goes deaf. A directory
    // watch sees the rename land. NonRecursive bounds the noise even with huge
    // `.git/objects` trees. `gitdir` is resolved + cached on chdir
    // (`current_gitdir`).
    if active_git.as_deref() != gitdir {
        if let Some(old) = active_git.take() {
            let _ = watcher.unwatch(&old);
        }
        if let Some(gd) = gitdir
            && watcher.watch(gd, RecursiveMode::NonRecursive).is_ok()
        {
            *active_git = Some(gd.to_path_buf());
        }
    }
    // Watch the vertical-split preview's *parent* dir non-recursively (same
    // replace-on-save rationale as the config + gitdir watches: a file-level
    // watch follows the old inode through an editor's atomic rename and goes
    // deaf).
    let want_preview = preview_parent_to_watch(preview, new_dir);
    if active_preview.as_deref() != want_preview.as_deref() {
        if let Some(old) = active_preview.take() {
            let _ = watcher.unwatch(&old);
        }
        if let Some(pp) = want_preview
            && watcher.watch(&pp, RecursiveMode::NonRecursive).is_ok()
        {
            *active_preview = Some(pp);
        }
    }
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
    use super::preview_parent_to_watch;
    use std::path::{Path, PathBuf};

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
