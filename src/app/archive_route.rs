//! The one place that knows an effect might be carrying a path inside an archive.
//!
//! Effects are already the sole route to the OS, so screening them here covers
//! every op at once instead of teaching each handler about mounts. An effect
//! whose paths are all real passes straight through; one that names a member gets
//! held back until those bytes exist, then re-run against the extracted copies;
//! one that would *write* into a container is refused.
//!
//! The match over path-bearing effects is exhaustive on purpose. A future effect
//! that carries a path has to be classified here or the build fails, which is the
//! difference between a considered decision and a silent hole.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::archive::Mounts;

use super::Effect;
use super::file_ops::FileOp;
use super::inventory_ops::{InventoryOp, YankSource as InventoryYankSource};

/// What to do with an outgoing effect. Takes ownership so a held-back effect can
/// be handed straight back later without the enum needing to be cloneable.
#[derive(Debug)]
pub enum ArchiveSink {
    /// Nothing in it touches a mount — run it as-is.
    PassThrough(Effect),
    /// Extract these members first, then run the effect again — by then its
    /// paths resolve to real files.
    Materialize {
        members: Vec<PathBuf>,
        then: Box<Effect>,
    },
    /// Can't be done inside an archive; the string is shown to the user.
    Refuse(&'static str),
    /// The op takes the archive *file* — deleting, moving or renaming it. Drop
    /// these mounts first, then run it: a mount keyed on a path that no longer
    /// holds that archive would resolve a future file's members against a stale
    /// index.
    UnmountFirst {
        archives: Vec<PathBuf>,
        effect: Box<Effect>,
    },
    /// The op means something different inside a container: record it as a pending
    /// change rather than touching the filesystem. Deleting or renaming a member
    /// is an index edit, which is why either costs nothing however large it is.
    Record(Vec<PendingChange>),
    /// Both: run `effect` — already pointed at the staging tree — and record what
    /// it will have added. Bringing a file *in* genuinely needs its bytes copied
    /// somewhere, and staging is where the repack reads them from.
    RewriteAndRecord {
        effect: Box<Effect>,
        changes: Vec<PendingChange>,
    },
}

/// A filesystem op reinterpreted as a change to a container.
#[derive(Debug, PartialEq, Eq)]
pub enum PendingChange {
    Delete {
        archive: PathBuf,
        inner: String,
    },
    Rename {
        archive: PathBuf,
        from: String,
        to: String,
    },
    Add {
        archive: PathBuf,
        inner: String,
    },
}

/// Classify `effect` against the live mounts.
///
/// `is_staged` answers "are this member's bytes already on disk?" — passed in
/// rather than checked here so the routing itself stays testable without a
/// filesystem.
pub fn route_archive_effect(
    effect: Effect,
    mounts: &Mounts,
    is_staged: &dyn Fn(&Path) -> bool,
    inventory_names: &dyn Fn(&[String]) -> Vec<String>,
) -> ArchiveSink {
    if mounts.is_empty() {
        return ArchiveSink::PassThrough(effect);
    }
    match classify(&effect, mounts, is_staged, inventory_names) {
        Verdict::Pass => ArchiveSink::PassThrough(effect),
        // Substitute what's already on disk now, and let the extraction's own
        // result substitute the rest — an op must never see a mount path.
        Verdict::Need { members, staged } => ArchiveSink::Materialize {
            members,
            then: Box::new(rewrite_paths(effect, &staged)),
        },
        Verdict::Staged(staged) => ArchiveSink::PassThrough(rewrite_paths(effect, &staged)),
        Verdict::Refuse(why) => ArchiveSink::Refuse(why),
        Verdict::TakesContainer(archives) => ArchiveSink::UnmountFirst {
            archives,
            effect: Box::new(effect),
        },
        Verdict::Record(changes) => ArchiveSink::Record(changes),
        Verdict::RewriteAndRecord { effect, changes } => {
            ArchiveSink::RewriteAndRecord { effect, changes }
        }
    }
}

/// The decision, before the effect is moved into a sink.
enum Verdict {
    Pass,
    /// Extract `members`; `staged` names the ones already on disk, whose paths are
    /// substituted before the effect is held back.
    Need {
        members: Vec<PathBuf>,
        staged: HashMap<PathBuf, PathBuf>,
    },
    /// Every member named is on disk already — only the paths need substituting.
    Staged(HashMap<PathBuf, PathBuf>),
    Refuse(&'static str),
    TakesContainer(Vec<PathBuf>),
    Record(Vec<PendingChange>),
    RewriteAndRecord {
        effect: Box<Effect>,
        changes: Vec<PendingChange>,
    },
}

fn classify(
    effect: &Effect,
    mounts: &Mounts,
    is_staged: &dyn Fn(&Path) -> bool,
    inventory_names: &dyn Fn(&[String]) -> Vec<String>,
) -> Verdict {
    match effect {
        // --- reads: extract, then run ---
        // A yank names each file twice — where to read it, and what to remember it
        // as — and only the former can be a member path needing extraction.
        Effect::Inventory(InventoryOp::Yank { sources }) => {
            let reads: Vec<PathBuf> = sources.iter().map(|s| s.read.clone()).collect();
            need(&reads, mounts, is_staged)
        }
        Effect::FileOp(
            FileOp::PipeContent { paths, .. }
            | FileOp::LongList { paths, .. }
            | FileOp::FileType { paths },
        ) => need(paths, mounts, is_staged),

        // Copying *out* is a read of the sources. Copying *in* would rewrite the
        // container, which is a different feature.
        Effect::FileOp(FileOp::Copy { paths, dest }) => {
            if mounts.contains(dest) {
                if paths.iter().any(|p| mounts.holds_member(p)) {
                    return Verdict::Refuse("archive: copy out first, then in");
                }
                return bring_in(paths, dest, mounts);
            }
            need(paths, mounts, is_staged)
        }

        Effect::FileOp(FileOp::OpenSpecialFile { path, .. }) => {
            need(std::slice::from_ref(path), mounts, is_staged)
        }

        // --- writes: not this feature ---
        // A move out of an archive removes the member, and a rename inside it
        // rewrites the container.
        Effect::FileOp(FileOp::Move { paths, dest }) => {
            if !mounts.contains(dest)
                && let Some(verdict) = takes_container(paths, mounts)
            {
                return verdict;
            }
            let sources_in = paths.iter().filter(|p| mounts.holds_member(p)).count();
            let dest_in = mounts.contains(dest);
            if sources_in == 0 && !dest_in {
                return Verdict::Pass;
            }
            if sources_in == paths.len() && dest_in {
                return rename_within(paths, dest, mounts);
            }
            // Moving across the boundary is a copy plus a delete, in one of two
            // orders, with two different failure halves. The user can do both
            // steps deliberately.
            Verdict::Refuse("archive: move in or out isn't a single step — copy, then delete")
        }
        Effect::FileOp(FileOp::RenameEach { pairs, .. }) => {
            // Renaming the archive file itself is an ordinary rename, once its
            // mount stops claiming the old name.
            if !pairs.iter().any(|(_, dst)| mounts.contains(dst)) {
                let srcs: Vec<PathBuf> = pairs.iter().map(|(src, _)| src.clone()).collect();
                if let Some(verdict) = takes_container(&srcs, mounts) {
                    return verdict;
                }
            }
            let touching = pairs
                .iter()
                .filter(|(src, dst)| mounts.holds_member(src) || mounts.contains(dst))
                .count();
            if touching == 0 {
                return Verdict::Pass;
            }
            let changes: Vec<PendingChange> = pairs
                .iter()
                .filter_map(|(src, dst)| rename_change(src, dst, mounts))
                .collect();
            if changes.len() == pairs.len() {
                Verdict::Record(changes)
            } else {
                Verdict::Refuse("archive: rename members within the archive, not across it")
            }
        }
        Effect::Inventory(InventoryOp::Put { dest_dir, ids }) => {
            if !mounts.contains(dest_dir) {
                return Verdict::Pass;
            }
            let Some((mount, inner_dir)) = mounts.resolve(dest_dir) else {
                return Verdict::Pass;
            };
            // The inventory worker copies from its own cache into a directory, so
            // pointing it at the staging tree is all it takes; the names it will
            // write are the yanked files' own.
            let names = inventory_names(ids);
            let changes: Vec<PendingChange> = names
                .iter()
                .map(|name| PendingChange::Add {
                    archive: mount.archive().to_path_buf(),
                    inner: join_inner(&inner_dir, name),
                })
                .collect();
            if changes.is_empty() {
                return Verdict::Refuse("archive: nothing to put");
            }
            Verdict::RewriteAndRecord {
                effect: Box::new(Effect::Inventory(InventoryOp::Put {
                    dest_dir: mount.staging_root.join(&inner_dir),
                    ids: ids.clone(),
                })),
                changes,
            }
        }
        // Deleting a member is an edit to the container's index, not a file
        // removal — recorded now, applied by the next write-back. A mixed
        // selection is refused rather than half-done: the two halves have
        // different recovery stories (the graveyard vs. the journal).
        Effect::Graveyard(op) => {
            let paths = graveyard_paths(op);
            // A restore writes a new file, which is a write into the container.
            if matches!(op, super::graveyard_ops::GraveyardOp::Restore { .. }) {
                return if paths.iter().any(|p| mounts.contains(p)) {
                    Verdict::Refuse("archive: restore outside the archive, then copy it in")
                } else {
                    Verdict::Pass
                };
            }
            if let Some(verdict) = takes_container(&paths, mounts) {
                return verdict;
            }
            let (members, outside): (Vec<&PathBuf>, Vec<&PathBuf>) =
                paths.iter().partition(|p| mounts.holds_member(p));
            if members.is_empty() {
                return Verdict::Pass;
            }
            if !outside.is_empty() {
                return Verdict::Refuse(
                    "archive: select members or files, not both — they delete differently",
                );
            }
            let changes = members
                .iter()
                .filter_map(|p| {
                    let (mount, _) = mounts.member_of(p)?;
                    let entry = mount.entry_at(p)?;
                    Some(PendingChange::Delete {
                        archive: mount.archive().to_path_buf(),
                        inner: entry.inner.clone(),
                    })
                })
                .collect::<Vec<_>>();
            if changes.is_empty() {
                Verdict::Refuse("archive: no such member")
            } else {
                Verdict::Record(changes)
            }
        }
        Effect::FileOp(FileOp::GitRestore { repo_root, .. }) => {
            if mounts.contains(repo_root) {
                Verdict::Refuse("archive: no git inside an archive")
            } else {
                Verdict::Pass
            }
        }

        // A listing refresh inside a mount is served from the index, never from a
        // directory read, so this worker must not be pointed at one.
        Effect::FileOp(FileOp::RefreshListing { dir, .. }) => {
            if mounts.contains(dir) {
                Verdict::Refuse("archive: listing is served from the index")
            } else {
                Verdict::Pass
            }
        }

        // Everything else carries no filesystem path we could be wrong about:
        // terminal writes, signals, clipboard, pane IO, timers, the archive ops
        // themselves. `ChangeDir` is deliberately here — navigating *into* a mount
        // is the whole point, and `chdir_into_mount` handles it.
        _ => Verdict::Pass,
    }
}

/// Copying real files into a container: point the op at the staging tree and
/// record what it will have added.
fn bring_in(paths: &[PathBuf], dest: &Path, mounts: &Mounts) -> Verdict {
    let Some((mount, inner_dir)) = mounts.resolve(dest) else {
        return Verdict::Pass;
    };
    let changes: Vec<PendingChange> = paths
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .map(|name| PendingChange::Add {
            archive: mount.archive().to_path_buf(),
            inner: join_inner(&inner_dir, &name),
        })
        .collect();
    if changes.is_empty() {
        return Verdict::Refuse("archive: nothing to copy in");
    }
    Verdict::RewriteAndRecord {
        effect: Box::new(Effect::FileOp(FileOp::Copy {
            paths: paths.to_vec(),
            dest: mount.staging_root.join(&inner_dir),
        })),
        changes,
    }
}

/// Moving members inside one container is a rename — no bytes move, which is why
/// it costs nothing even for a huge member.
fn rename_within(paths: &[PathBuf], dest: &Path, mounts: &Mounts) -> Verdict {
    let Some((mount, dest_inner)) = mounts.resolve(dest) else {
        return Verdict::Pass;
    };
    // A destination the archive already holds as a directory takes each source
    // under its own name; anything else is a single rename to that exact name.
    let into_dir = mount.index.is_dir(&dest_inner);
    if !into_dir && paths.len() > 1 {
        return Verdict::Refuse("archive: rename one member at a time, or into a directory");
    }
    let mut changes = Vec::new();
    for path in paths {
        let Some((src_mount, from)) = mounts.member_of(path) else {
            return Verdict::Refuse("archive: rename members within the archive, not across it");
        };
        if src_mount.archive() != mount.archive() {
            return Verdict::Refuse("archive: rename members within one archive");
        }
        let to = if into_dir {
            let name = from.rsplit_once('/').map_or(from.as_str(), |(_, n)| n);
            join_inner(&dest_inner, name)
        } else {
            dest_inner.clone()
        };
        changes.push(PendingChange::Rename {
            archive: mount.archive().to_path_buf(),
            from,
            to,
        });
    }
    Verdict::Record(changes)
}

/// The archive an *unmounted* path is inside, if any.
///
/// A mount is addressed under the archive's own path, so `…/pkg.zip/src` names a
/// place inside `pkg.zip` whether or not it happens to be mounted right now. That
/// is what turns a path saved earlier — a mark, a harpoon slot, a restored session
/// cwd — back into "mount this, then go there", and it's the same walk that will
/// find a nested container.
///
/// `is_container` is injected because deciding it means asking the filesystem
/// whether an ancestor is a file; this half stays pure and testable.
pub fn archive_ancestor(
    path: &Path,
    is_container: &dyn Fn(&Path) -> bool,
) -> Option<(PathBuf, String)> {
    // Deepest first: with a nested container the inner one owns the path.
    for ancestor in path.ancestors() {
        if !is_container(ancestor) {
            continue;
        }
        let rel = path.strip_prefix(ancestor).ok()?;
        if rel.as_os_str().is_empty() {
            // The archive itself — mount it and land at its root.
            return Some((ancestor.to_path_buf(), String::new()));
        }
        // Normalized for the same reason a member name is: a `..` between here
        // and the archive would name somewhere outside it.
        let inner = crate::archive::index::normalize(&rel.to_string_lossy()).ok()?;
        return Some((ancestor.to_path_buf(), inner.inner));
    }
    None
}

/// The mounted archive *files* among `paths`, when the op would move or remove
/// them — a container going away has to take its mount with it.
///
/// `None` when nothing in `paths` is a mount root, which is the common case.
fn takes_container(paths: &[PathBuf], mounts: &Mounts) -> Option<Verdict> {
    let archives: Vec<PathBuf> = paths
        .iter()
        .filter(|p| mounts.get(p).is_some())
        .cloned()
        .collect();
    if archives.is_empty() {
        return None;
    }
    if paths.iter().any(|p| mounts.holds_member(p)) {
        // One is a change to a container, the other a change to what's inside
        // one: different recovery stories, so not in a single step.
        return Some(Verdict::Refuse(
            "archive: select members or whole archives, not both",
        ));
    }
    Some(Verdict::TakesContainer(archives))
}

/// One `(src, dst)` pair as a rename, when both sides are the same container.
fn rename_change(src: &Path, dst: &Path, mounts: &Mounts) -> Option<PendingChange> {
    let (src_mount, from) = mounts.member_of(src)?;
    let (dst_mount, to) = mounts.member_of(dst)?;
    (src_mount.archive() == dst_mount.archive()).then(|| PendingChange::Rename {
        archive: src_mount.archive().to_path_buf(),
        from,
        to,
    })
}

fn join_inner(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Sort the members among `paths` into "needs extracting" and "already on disk,
/// here is where".
///
/// A member's bytes are **never** at its mount path — extracted, they live in the
/// staging tree, so an op naming one has to be pointed there whether or not
/// anything had to be extracted first. Skipping the substitution because nothing
/// needed extracting is how a yank inside a streamed `.tar.gz` (where the mount
/// stages every member up front) reached the OS as
/// `demo.tar.gz/src/main.rs: Not a directory`.
///
/// A mounted archive named here is a real file already and needs neither: a read
/// of `pkg.zip` itself is a read of `pkg.zip`, mounted or not.
fn need(paths: &[PathBuf], mounts: &Mounts, is_staged: &dyn Fn(&Path) -> bool) -> Verdict {
    let mut wanted: Vec<PathBuf> = Vec::new();
    let mut staged: HashMap<PathBuf, PathBuf> = HashMap::new();
    for path in paths.iter().filter(|p| mounts.holds_member(p)) {
        match staging_path(path, mounts).filter(|_| is_staged(path)) {
            Some(real) => {
                staged.insert(path.clone(), real);
            }
            // Not extracted — or extracted somewhere we can't name, in which case
            // extracting it again is how it gets a path we can.
            None => wanted.push(path.clone()),
        }
    }
    if wanted.is_empty() {
        if staged.is_empty() {
            // Nothing in `paths` is in an archive.
            Verdict::Pass
        } else {
            Verdict::Staged(staged)
        }
    } else {
        Verdict::Need {
            members: wanted,
            staged,
        }
    }
}

/// Where a member's extracted bytes live, or would.
fn staging_path(member: &Path, mounts: &Mounts) -> Option<PathBuf> {
    let (mount, _) = mounts.member_of(member)?;
    mount.entry_at(member).map(|e| mount.staging_path(e))
}

fn graveyard_paths(op: &super::graveyard_ops::GraveyardOp) -> Vec<PathBuf> {
    match op {
        super::graveyard_ops::GraveyardOp::Archive { paths } => paths.clone(),
        // Restoring or purging targets the graveyard's own store, never a mount.
        super::graveyard_ops::GraveyardOp::Restore { dest, .. } => vec![dest.clone()],
        super::graveyard_ops::GraveyardOp::PurgeAll { .. } => Vec::new(),
    }
}

/// Rewrite every mount path in `effect` to where its bytes actually live.
///
/// Applied when a held-back effect is re-run: the op itself never learns that its
/// inputs came out of an archive, which is why nothing downstream needed changing.
pub fn rewrite_paths(effect: Effect, staged: &HashMap<PathBuf, PathBuf>) -> Effect {
    let map = |p: &PathBuf| staged.get(p).cloned().unwrap_or_else(|| p.clone());
    let map_all = |paths: &[PathBuf]| paths.iter().map(map).collect::<Vec<_>>();
    match effect {
        // Only `read` moves to the staging tree; `record_as` stays the member path,
        // which is the whole point of carrying both.
        Effect::Inventory(InventoryOp::Yank { sources }) => Effect::Inventory(InventoryOp::Yank {
            sources: sources
                .iter()
                .map(|s| InventoryYankSource {
                    read: map(&s.read),
                    record_as: s.record_as.clone(),
                })
                .collect(),
        }),
        Effect::FileOp(FileOp::Copy { paths, dest }) => Effect::FileOp(FileOp::Copy {
            paths: map_all(&paths),
            dest,
        }),
        Effect::FileOp(FileOp::PipeContent {
            use_inventory,
            inventory_ids,
            paths,
        }) => Effect::FileOp(FileOp::PipeContent {
            use_inventory,
            inventory_ids,
            paths: map_all(&paths),
        }),
        Effect::FileOp(FileOp::LongList { paths, title }) => Effect::FileOp(FileOp::LongList {
            paths: map_all(&paths),
            title,
        }),
        Effect::FileOp(FileOp::FileType { paths }) => Effect::FileOp(FileOp::FileType {
            paths: map_all(&paths),
        }),
        Effect::FileOp(FileOp::OpenSpecialFile {
            path,
            theme,
            open_as_rendered,
            wrap,
            dest,
        }) => Effect::FileOp(FileOp::OpenSpecialFile {
            path: map(&path),
            theme,
            open_as_rendered,
            wrap,
            dest,
        }),
        // Only the effects `route_archive_effect` can hold back need rewriting;
        // anything else reaches here unchanged by construction.
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::file_ops::PagerDest;
    use crate::archive::index::{Draft, IndexBuilder, Locator};
    use crate::archive::journal::{Journal, StagedStats};
    use crate::archive::mount::ArchiveMount;
    use crate::archive::{ArchiveFormat, Capability};

    fn mounts_with(members: &[&str]) -> Mounts {
        let mut b = IndexBuilder::new(100);
        for (i, name) in members.iter().enumerate() {
            b.push(name, Draft::file(1, Locator::Zip { index: i }));
        }
        let (index, _) = b.finish(PathBuf::from("/src/pkg.zip"), ArchiveFormat::Zip, 10);
        let mut mounts = Mounts::default();
        mounts.insert(
            ArchiveMount {
                index,
                journal: Journal::default(),
                staged: StagedStats::new(),
                capability: Capability::ReadWrite,
                warnings: Vec::new(),
                staging_root: PathBuf::from("/staging"),
                last_used: 0,
                depth: 0,
                editing: Vec::new(),
            },
            &[],
        );
        mounts
    }

    fn nothing_staged(_: &Path) -> bool {
        false
    }

    /// The inventory lookup the Put arm needs; these tests don't exercise it.
    fn no_names(_: &[String]) -> Vec<String> {
        Vec::new()
    }

    fn all_staged(_: &Path) -> bool {
        true
    }

    fn member(name: &str) -> PathBuf {
        PathBuf::from("/src/pkg.zip").join(name)
    }

    #[test]
    fn with_nothing_mounted_every_effect_passes_through() {
        let effect = Effect::Inventory(InventoryOp::Yank {
            sources: vec![PathBuf::from("/real/file.txt")]
                .into_iter()
                .map(crate::app::inventory_ops::YankSource::plain)
                .collect(),
        });
        assert!(matches!(
            route_archive_effect(effect, &Mounts::default(), &nothing_staged, &no_names),
            ArchiveSink::PassThrough(_)
        ));
    }

    #[test]
    fn an_effect_over_real_files_passes_through_even_with_a_mount_open() {
        let mounts = mounts_with(&["a.txt"]);
        let effect = Effect::Inventory(InventoryOp::Yank {
            sources: vec![PathBuf::from("/real/file.txt")]
                .into_iter()
                .map(crate::app::inventory_ops::YankSource::plain)
                .collect(),
        });
        assert!(matches!(
            route_archive_effect(effect, &mounts, &nothing_staged, &no_names),
            ArchiveSink::PassThrough(_)
        ));
    }

    /// The headline: yanking a member holds the op back until its bytes exist.
    #[test]
    fn yanking_a_member_is_held_back_for_extraction() {
        let mounts = mounts_with(&["a.txt", "d/b.txt"]);
        let effect = Effect::Inventory(InventoryOp::Yank {
            sources: vec![member("a.txt"), member("d/b.txt")]
                .into_iter()
                .map(crate::app::inventory_ops::YankSource::plain)
                .collect(),
        });
        let ArchiveSink::Materialize { members, then } =
            route_archive_effect(effect, &mounts, &nothing_staged, &no_names)
        else {
            panic!("a member yank must be materialized first");
        };
        assert_eq!(members, [member("a.txt"), member("d/b.txt")]);
        assert!(matches!(*then, Effect::Inventory(InventoryOp::Yank { .. })));
    }

    /// Already-extracted members need no round trip — that's what makes a second
    /// read free.
    #[test]
    fn staged_members_pass_straight_through() {
        let mounts = mounts_with(&["a.txt"]);
        let effect = Effect::Inventory(InventoryOp::Yank {
            sources: vec![member("a.txt")]
                .into_iter()
                .map(crate::app::inventory_ops::YankSource::plain)
                .collect(),
        });
        assert!(matches!(
            route_archive_effect(effect, &mounts, &all_staged, &no_names),
            ArchiveSink::PassThrough(_)
        ));
    }

    /// A mixed selection extracts only the members, and only the unstaged ones.
    #[test]
    fn only_the_unstaged_members_are_extracted() {
        let mounts = mounts_with(&["a.txt", "b.txt"]);
        let staged = |p: &Path| p == member("b.txt");
        let effect = Effect::Inventory(InventoryOp::Yank {
            sources: vec![
                PathBuf::from("/real/x.txt"),
                member("a.txt"),
                member("b.txt"),
            ]
            .into_iter()
            .map(crate::app::inventory_ops::YankSource::plain)
            .collect(),
        });
        let ArchiveSink::Materialize { members, .. } =
            route_archive_effect(effect, &mounts, &staged, &no_names)
        else {
            panic!("expected a materialize");
        };
        assert_eq!(members, [member("a.txt")]);
    }

    #[test]
    fn copying_out_of_an_archive_is_a_read() {
        let mounts = mounts_with(&["a.txt"]);
        let effect = Effect::FileOp(FileOp::Copy {
            paths: vec![member("a.txt")],
            dest: PathBuf::from("/home/me"),
        });
        assert!(matches!(
            route_archive_effect(effect, &mounts, &nothing_staged, &no_names),
            ArchiveSink::Materialize { .. }
        ));
    }

    /// Copying *into* a container points the op at the staging tree and records
    /// the addition — the bytes have to exist somewhere for the repack to read.
    #[test]
    fn copying_into_an_archive_stages_the_file_and_records_it() {
        let mounts = mounts_with(&["a.txt", "d/keep.txt"]);
        let effect = Effect::FileOp(FileOp::Copy {
            paths: vec![PathBuf::from("/real/x.txt")],
            dest: member("d"),
        });
        let ArchiveSink::RewriteAndRecord { effect, changes } =
            route_archive_effect(effect, &mounts, &nothing_staged, &no_names)
        else {
            panic!("a copy in must be staged and recorded");
        };
        let Effect::FileOp(FileOp::Copy { dest, .. }) = *effect else {
            panic!("still a copy");
        };
        assert_eq!(
            dest,
            PathBuf::from("/staging/d"),
            "the copy is redirected into the staging tree"
        );
        assert_eq!(
            changes,
            [PendingChange::Add {
                archive: PathBuf::from("/src/pkg.zip"),
                inner: "d/x.txt".to_string(),
            }]
        );
    }

    /// Copying a member into the same archive would be a rename dressed as a
    /// copy, with the source's bytes not yet on disk. Two steps, deliberately.
    #[test]
    fn copying_a_member_into_its_own_archive_is_refused() {
        let mounts = mounts_with(&["a.txt", "d/keep.txt"]);
        let effect = Effect::FileOp(FileOp::Copy {
            paths: vec![member("a.txt")],
            dest: member("d"),
        });
        assert!(matches!(
            route_archive_effect(effect, &mounts, &nothing_staged, &no_names),
            ArchiveSink::Refuse(_)
        ));
    }

    /// Moving members inside one container is a rename: no bytes move, so it is
    /// recorded rather than performed.
    #[test]
    fn moving_members_within_an_archive_is_recorded_as_a_rename() {
        let mounts = mounts_with(&["a.txt", "d/keep.txt"]);
        let effect = Effect::FileOp(FileOp::Move {
            paths: vec![member("a.txt")],
            dest: member("d"),
        });
        let ArchiveSink::Record(changes) =
            route_archive_effect(effect, &mounts, &nothing_staged, &no_names)
        else {
            panic!("a move within the archive is a rename");
        };
        assert_eq!(
            changes,
            [PendingChange::Rename {
                archive: PathBuf::from("/src/pkg.zip"),
                from: "a.txt".to_string(),
                to: "d/a.txt".to_string(),
            }]
        );
    }

    /// Moving across the boundary is a copy plus a delete, with two failure
    /// halves — the user can do both steps and see each land.
    #[test]
    fn moving_across_the_archive_boundary_is_refused() {
        let mounts = mounts_with(&["a.txt"]);
        for effect in [
            Effect::FileOp(FileOp::Move {
                paths: vec![member("a.txt")],
                dest: PathBuf::from("/home/me"),
            }),
            Effect::FileOp(FileOp::Move {
                paths: vec![PathBuf::from("/real/x.txt")],
                dest: member("d"),
            }),
        ] {
            let ArchiveSink::Refuse(why) =
                route_archive_effect(effect, &mounts, &nothing_staged, &no_names)
            else {
                panic!("a move across the boundary must be refused");
            };
            assert!(why.contains("copy, then delete"), "{why}");
        }
    }

    /// Deleting a member is an index edit, not a file removal — so it is recorded
    /// against the container rather than sent to the graveyard, and a 500 MB
    /// member can be dropped without ever extracting it.
    #[test]
    fn deleting_a_member_is_recorded_not_unlinked() {
        let mounts = mounts_with(&["a.txt"]);
        let effect = Effect::Graveyard(crate::app::graveyard_ops::GraveyardOp::Archive {
            paths: vec![member("a.txt")],
        });
        let ArchiveSink::Record(changes) =
            route_archive_effect(effect, &mounts, &nothing_staged, &no_names)
        else {
            panic!("a member delete must be recorded");
        };
        assert_eq!(
            changes,
            [PendingChange::Delete {
                archive: PathBuf::from("/src/pkg.zip"),
                inner: "a.txt".to_string(),
            }]
        );
    }

    /// A selection spanning both a member and a real file would delete two ways at
    /// once — the graveyard for one, the journal for the other — with different
    /// recovery stories. Refusing beats doing half of each.
    #[test]
    fn a_mixed_delete_selection_is_refused() {
        let mounts = mounts_with(&["a.txt"]);
        let effect = Effect::Graveyard(crate::app::graveyard_ops::GraveyardOp::Archive {
            paths: vec![member("a.txt"), PathBuf::from("/real/x.txt")],
        });
        let ArchiveSink::Refuse(why) =
            route_archive_effect(effect, &mounts, &nothing_staged, &no_names)
        else {
            panic!("a mixed selection must be refused");
        };
        assert!(why.contains("not both"), "{why}");
    }

    /// Deleting a real file while an archive happens to be mounted is untouched.
    #[test]
    fn deleting_a_real_file_is_unaffected_by_an_open_mount() {
        let mounts = mounts_with(&["a.txt"]);
        let effect = Effect::Graveyard(crate::app::graveyard_ops::GraveyardOp::Archive {
            paths: vec![PathBuf::from("/real/x.txt")],
        });
        assert!(matches!(
            route_archive_effect(effect, &mounts, &nothing_staged, &no_names),
            ArchiveSink::PassThrough(_)
        ));
    }

    /// A listing refresh inside a mount comes from the index; pointing the
    /// directory-reading worker at one would return an empty listing.
    #[test]
    fn a_listing_refresh_inside_a_mount_is_refused() {
        let mounts = mounts_with(&["a.txt"]);
        let effect = Effect::FileOp(FileOp::RefreshListing {
            side: crate::app::state::Side::Left,
            dir: PathBuf::from("/src/pkg.zip"),
            generation: 1,
        });
        assert!(matches!(
            route_archive_effect(effect, &mounts, &nothing_staged, &no_names),
            ArchiveSink::Refuse(_)
        ));
    }

    /// Navigating into a mount is the feature working, not something to screen.
    #[test]
    fn changing_directory_into_a_mount_passes_through() {
        let mounts = mounts_with(&["a.txt"]);
        let effect = Effect::ChangeDir {
            path: member("d"),
            focus: None,
            on_ok: None,
            err_prefix: "chdir",
        };
        assert!(matches!(
            route_archive_effect(effect, &mounts, &nothing_staged, &no_names),
            ArchiveSink::PassThrough(_)
        ));
    }

    #[test]
    fn rewriting_points_an_effect_at_the_extracted_copies() {
        let mut staged = HashMap::new();
        staged.insert(member("a.txt"), PathBuf::from("/staging/a.txt"));

        let rewritten = rewrite_paths(
            Effect::Inventory(InventoryOp::Yank {
                sources: vec![member("a.txt"), PathBuf::from("/real/x.txt")]
                    .into_iter()
                    .map(crate::app::inventory_ops::YankSource::plain)
                    .collect(),
            }),
            &staged,
        );
        let Effect::Inventory(InventoryOp::Yank { sources }) = rewritten else {
            panic!("shape preserved");
        };
        assert_eq!(
            sources.iter().map(|s| s.read.clone()).collect::<Vec<_>>(),
            [
                PathBuf::from("/staging/a.txt"),
                PathBuf::from("/real/x.txt")
            ],
            "members are redirected to read from staging, real paths left alone"
        );
        assert_eq!(
            sources
                .iter()
                .map(|s| s.record_as.clone())
                .collect::<Vec<_>>(),
            [member("a.txt"), PathBuf::from("/real/x.txt")],
            "and each is still remembered by the path the user sees"
        );
    }

    #[test]
    fn rewriting_preserves_everything_except_the_paths() {
        let mut staged = HashMap::new();
        staged.insert(member("a.txt"), PathBuf::from("/staging/a.txt"));

        let rewritten = rewrite_paths(
            Effect::FileOp(FileOp::Copy {
                paths: vec![member("a.txt")],
                dest: PathBuf::from("/home/me"),
            }),
            &staged,
        );
        let Effect::FileOp(FileOp::Copy { paths, dest }) = rewritten else {
            panic!("shape preserved");
        };
        assert_eq!(paths, [PathBuf::from("/staging/a.txt")]);
        assert_eq!(
            dest,
            PathBuf::from("/home/me"),
            "the destination is untouched"
        );
    }

    #[test]
    fn rewriting_a_pipe_keeps_its_inventory_selection() {
        let staged = HashMap::new();
        let rewritten = rewrite_paths(
            Effect::FileOp(FileOp::PipeContent {
                use_inventory: true,
                inventory_ids: vec!["id-1".to_string()],
                paths: vec![],
            }),
            &staged,
        );
        let Effect::FileOp(FileOp::PipeContent {
            use_inventory,
            inventory_ids,
            ..
        }) = rewritten
        else {
            panic!("shape preserved");
        };
        assert!(use_inventory);
        assert_eq!(inventory_ids, ["id-1"]);
    }

    #[test]
    fn rewriting_leaves_an_unrelated_effect_alone() {
        let staged = HashMap::new();
        let rewritten = rewrite_paths(
            Effect::FileOp(FileOp::OpenSpecialFile {
                path: PathBuf::from("/dev/zero"),
                theme: crate::ui::theme::Theme::default(),
                open_as_rendered: false,
                wrap: None,
                dest: PagerDest::Overlay { scroll: None },
            }),
            &staged,
        );
        let Effect::FileOp(FileOp::OpenSpecialFile { path, .. }) = rewritten else {
            panic!("shape preserved");
        };
        assert_eq!(path, PathBuf::from("/dev/zero"));
    }

    // ── the container itself ─────────────────────────────────────────────

    /// The archive file is a real file: reading it is not a member read, so it
    /// passes straight through however it's named.
    #[test]
    fn reading_the_archive_file_itself_passes_through() {
        let mounts = mounts_with(&["a.txt"]);
        let container = PathBuf::from("/src/pkg.zip");
        for effect in [
            Effect::Inventory(InventoryOp::Yank {
                sources: vec![container.clone()]
                    .into_iter()
                    .map(crate::app::inventory_ops::YankSource::plain)
                    .collect(),
            }),
            Effect::FileOp(FileOp::FileType {
                paths: vec![container.clone()],
            }),
            Effect::FileOp(FileOp::Copy {
                paths: vec![container],
                dest: PathBuf::from("/elsewhere"),
            }),
        ] {
            assert!(
                matches!(
                    route_archive_effect(effect, &mounts, &nothing_staged, &no_names),
                    ArchiveSink::PassThrough(_)
                ),
                "a mounted archive is still an ordinary file to read"
            );
        }
    }

    /// Deleting, moving or renaming the archive takes the mount with it — left
    /// behind, it would resolve a future file at that path against a stale index.
    #[test]
    fn an_op_that_takes_the_container_unmounts_it_first() {
        let mounts = mounts_with(&["a.txt"]);
        let container = PathBuf::from("/src/pkg.zip");
        for effect in [
            Effect::Graveyard(crate::app::graveyard_ops::GraveyardOp::Archive {
                paths: vec![container.clone()],
            }),
            Effect::FileOp(FileOp::Move {
                paths: vec![container.clone()],
                dest: PathBuf::from("/elsewhere"),
            }),
            Effect::FileOp(FileOp::RenameEach {
                pairs: vec![(container.clone(), PathBuf::from("/src/renamed.zip"))],
                is_move: false,
            }),
        ] {
            let sink = route_archive_effect(effect, &mounts, &nothing_staged, &no_names);
            let ArchiveSink::UnmountFirst { archives, .. } = sink else {
                panic!("expected an unmount, got {sink:?}");
            };
            assert_eq!(archives, std::slice::from_ref(&container));
        }
    }

    /// One is a change to a container, the other a change to what's inside one.
    /// They recover differently, so they aren't done in a single step.
    #[test]
    fn deleting_a_container_and_a_member_together_is_refused() {
        let mounts = mounts_with(&["a.txt"]);
        let sink = route_archive_effect(
            Effect::Graveyard(crate::app::graveyard_ops::GraveyardOp::Archive {
                paths: vec![PathBuf::from("/src/pkg.zip"), member("a.txt")],
            }),
            &mounts,
            &nothing_staged,
            &no_names,
        );
        assert!(matches!(sink, ArchiveSink::Refuse(_)), "got {sink:?}");
    }

    // ── reaching a place inside an unmounted archive ──────────────────────

    /// The predicate the App supplies from the filesystem; here it's a set.
    fn containers(names: &[&str]) -> impl Fn(&Path) -> bool + use<> {
        let set: Vec<PathBuf> = names.iter().map(PathBuf::from).collect();
        move |p: &Path| set.iter().any(|c| c == p)
    }

    #[test]
    fn a_path_inside_an_unmounted_archive_names_the_archive_and_the_way_in() {
        let is_container = containers(&["/src/pkg.zip"]);
        assert_eq!(
            archive_ancestor(Path::new("/src/pkg.zip/a/b.txt"), &is_container),
            Some((PathBuf::from("/src/pkg.zip"), "a/b.txt".to_string()))
        );
        // The archive itself: mount it and land at its root.
        assert_eq!(
            archive_ancestor(Path::new("/src/pkg.zip"), &is_container),
            Some((PathBuf::from("/src/pkg.zip"), String::new()))
        );
    }

    #[test]
    fn an_ordinary_path_names_no_archive() {
        let is_container = containers(&["/src/pkg.zip"]);
        assert_eq!(
            archive_ancestor(Path::new("/src/main.rs"), &is_container),
            None
        );
        assert_eq!(archive_ancestor(Path::new("/src"), &is_container), None);
        // Named like one but not a file on disk — the predicate is what decides.
        assert_eq!(
            archive_ancestor(Path::new("/src/other.zip/a"), &is_container),
            None
        );
    }

    /// With one archive inside another, the deepest owns the path — the same rule
    /// `Mounts::resolve` applies to a live mount.
    #[test]
    fn the_innermost_container_wins() {
        let is_container = containers(&["/src/outer.zip", "/src/outer.zip/inner.zip"]);
        assert_eq!(
            archive_ancestor(Path::new("/src/outer.zip/inner.zip/a.txt"), &is_container),
            Some((
                PathBuf::from("/src/outer.zip/inner.zip"),
                "a.txt".to_string()
            ))
        );
    }

    /// A `..` between the archive and the target names somewhere outside it, so
    /// there is nothing here to mount into.
    #[test]
    fn a_traversing_path_reaches_no_archive() {
        let is_container = containers(&["/src/pkg.zip"]);
        assert_eq!(
            archive_ancestor(Path::new("/src/pkg.zip/../escape"), &is_container),
            None
        );
    }
}
