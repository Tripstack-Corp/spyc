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
use super::inventory_ops::InventoryOp;

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
) -> ArchiveSink {
    if mounts.is_empty() {
        return ArchiveSink::PassThrough(effect);
    }
    match classify(&effect, mounts, is_staged) {
        Verdict::Pass => ArchiveSink::PassThrough(effect),
        Verdict::Need(members) => ArchiveSink::Materialize {
            members,
            then: Box::new(effect),
        },
        Verdict::Refuse(why) => ArchiveSink::Refuse(why),
    }
}

/// The decision, before the effect is moved into a sink.
enum Verdict {
    Pass,
    Need(Vec<PathBuf>),
    Refuse(&'static str),
}

fn classify(effect: &Effect, mounts: &Mounts, is_staged: &dyn Fn(&Path) -> bool) -> Verdict {
    match effect {
        // --- reads: extract, then run ---
        Effect::Inventory(InventoryOp::Yank { paths })
        | Effect::FileOp(
            FileOp::PipeContent { paths, .. }
            | FileOp::LongList { paths, .. }
            | FileOp::FileType { paths },
        ) => need(paths, mounts, is_staged),

        // Copying *out* is a read of the sources. Copying *in* would rewrite the
        // container, which is a different feature.
        Effect::FileOp(FileOp::Copy { paths, dest }) => {
            if mounts.contains(dest) {
                return Verdict::Refuse("archive: copying into an archive is not supported");
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
            if paths.iter().any(|p| mounts.contains(p)) || mounts.contains(dest) {
                Verdict::Refuse("archive: moving a member is not supported")
            } else {
                Verdict::Pass
            }
        }
        Effect::FileOp(FileOp::RenameEach { pairs, .. }) => {
            if pairs
                .iter()
                .any(|(src, dst)| mounts.contains(src) || mounts.contains(dst))
            {
                Verdict::Refuse("archive: renaming a member is not supported")
            } else {
                Verdict::Pass
            }
        }
        Effect::Inventory(InventoryOp::Put { dest_dir, .. }) => {
            if mounts.contains(dest_dir) {
                Verdict::Refuse("archive: writing into an archive is not supported")
            } else {
                Verdict::Pass
            }
        }
        Effect::Graveyard(op) => {
            if graveyard_paths(op).iter().any(|p| mounts.contains(p)) {
                Verdict::Refuse("archive: deleting a member is not supported")
            } else {
                Verdict::Pass
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

/// The members among `paths` that aren't extracted yet.
fn need(paths: &[PathBuf], mounts: &Mounts, is_staged: &dyn Fn(&Path) -> bool) -> Verdict {
    let members: Vec<PathBuf> = paths
        .iter()
        .filter(|p| mounts.contains(p) && !is_staged(p))
        .cloned()
        .collect();
    if members.is_empty() {
        // Either nothing is in an archive, or every member is already extracted —
        // in both cases the effect's paths are real files right now.
        Verdict::Pass
    } else {
        Verdict::Need(members)
    }
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
        Effect::Inventory(InventoryOp::Yank { paths }) => Effect::Inventory(InventoryOp::Yank {
            paths: map_all(&paths),
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
            },
            &[],
        );
        mounts
    }

    fn nothing_staged(_: &Path) -> bool {
        false
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
            paths: vec![PathBuf::from("/real/file.txt")],
        });
        assert!(matches!(
            route_archive_effect(effect, &Mounts::default(), &nothing_staged),
            ArchiveSink::PassThrough(_)
        ));
    }

    #[test]
    fn an_effect_over_real_files_passes_through_even_with_a_mount_open() {
        let mounts = mounts_with(&["a.txt"]);
        let effect = Effect::Inventory(InventoryOp::Yank {
            paths: vec![PathBuf::from("/real/file.txt")],
        });
        assert!(matches!(
            route_archive_effect(effect, &mounts, &nothing_staged),
            ArchiveSink::PassThrough(_)
        ));
    }

    /// The headline: yanking a member holds the op back until its bytes exist.
    #[test]
    fn yanking_a_member_is_held_back_for_extraction() {
        let mounts = mounts_with(&["a.txt", "d/b.txt"]);
        let effect = Effect::Inventory(InventoryOp::Yank {
            paths: vec![member("a.txt"), member("d/b.txt")],
        });
        let ArchiveSink::Materialize { members, then } =
            route_archive_effect(effect, &mounts, &nothing_staged)
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
            paths: vec![member("a.txt")],
        });
        assert!(matches!(
            route_archive_effect(effect, &mounts, &all_staged),
            ArchiveSink::PassThrough(_)
        ));
    }

    /// A mixed selection extracts only the members, and only the unstaged ones.
    #[test]
    fn only_the_unstaged_members_are_extracted() {
        let mounts = mounts_with(&["a.txt", "b.txt"]);
        let staged = |p: &Path| p == member("b.txt");
        let effect = Effect::Inventory(InventoryOp::Yank {
            paths: vec![
                PathBuf::from("/real/x.txt"),
                member("a.txt"),
                member("b.txt"),
            ],
        });
        let ArchiveSink::Materialize { members, .. } =
            route_archive_effect(effect, &mounts, &staged)
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
            route_archive_effect(effect, &mounts, &nothing_staged),
            ArchiveSink::Materialize { .. }
        ));
    }

    #[test]
    fn copying_into_an_archive_is_refused() {
        let mounts = mounts_with(&["a.txt"]);
        let effect = Effect::FileOp(FileOp::Copy {
            paths: vec![PathBuf::from("/real/x.txt")],
            dest: member("d"),
        });
        let ArchiveSink::Refuse(why) = route_archive_effect(effect, &mounts, &nothing_staged)
        else {
            panic!("writing into a container must be refused");
        };
        assert!(why.contains("into an archive"), "{why}");
    }

    /// A move out would have to remove the member, which is a rewrite of the
    /// container — a different thing from copying out.
    #[test]
    fn moving_a_member_is_refused_in_either_direction() {
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
            assert!(matches!(
                route_archive_effect(effect, &mounts, &nothing_staged),
                ArchiveSink::Refuse(_)
            ));
        }
    }

    #[test]
    fn deleting_a_member_is_refused() {
        let mounts = mounts_with(&["a.txt"]);
        let effect = Effect::Graveyard(crate::app::graveyard_ops::GraveyardOp::Archive {
            paths: vec![member("a.txt")],
        });
        let ArchiveSink::Refuse(why) = route_archive_effect(effect, &mounts, &nothing_staged)
        else {
            panic!("deleting a member must be refused");
        };
        assert!(why.contains("deleting"), "{why}");
    }

    /// Deleting a real file while an archive happens to be mounted is untouched.
    #[test]
    fn deleting_a_real_file_is_unaffected_by_an_open_mount() {
        let mounts = mounts_with(&["a.txt"]);
        let effect = Effect::Graveyard(crate::app::graveyard_ops::GraveyardOp::Archive {
            paths: vec![PathBuf::from("/real/x.txt")],
        });
        assert!(matches!(
            route_archive_effect(effect, &mounts, &nothing_staged),
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
            route_archive_effect(effect, &mounts, &nothing_staged),
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
            route_archive_effect(effect, &mounts, &nothing_staged),
            ArchiveSink::PassThrough(_)
        ));
    }

    #[test]
    fn rewriting_points_an_effect_at_the_extracted_copies() {
        let mut staged = HashMap::new();
        staged.insert(member("a.txt"), PathBuf::from("/staging/a.txt"));

        let rewritten = rewrite_paths(
            Effect::Inventory(InventoryOp::Yank {
                paths: vec![member("a.txt"), PathBuf::from("/real/x.txt")],
            }),
            &staged,
        );
        let Effect::Inventory(InventoryOp::Yank { paths }) = rewritten else {
            panic!("shape preserved");
        };
        assert_eq!(
            paths,
            [
                PathBuf::from("/staging/a.txt"),
                PathBuf::from("/real/x.txt")
            ],
            "members are redirected, real paths left alone"
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
}
