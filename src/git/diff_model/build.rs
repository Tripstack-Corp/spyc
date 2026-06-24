//! Per-change `FileDiff` builders + the status-walk reduction.
//!
//! The scope entry points in [`super`] decide *what* to compare; this module
//! turns each individual change (a status item, a `tree_index` change, or a
//! `diff_tree_to_tree` change) into one [`FileDiff`], delegating the blob line
//! diff to [`super::blob`]. It also reduces the working-tree status walk into a
//! per-path plan ([`collect_worktree_plan`]) with rename detection.

use super::blob::{
    blob_similarity, diff_kind_from_cache, set_blob, tree_blob_at, worktree_similarity,
};
use crate::git::model::{DiffKind, FileDiff, FileStatus, lang_hint_for};

/// One unit of work for the working-tree (`gd`) diff: either a detected
/// rename/copy pair, or a single path whose HEAD blob is diffed against its
/// worktree file (covering modifications, additions, deletions, and untracked
/// files).
pub enum WorkItem {
    Rename {
        source: String,
        dest: String,
        copy: bool,
    },
    Path {
        path: String,
    },
}

impl WorkItem {
    /// The destination/own path used to sort and path-filter the plan.
    pub fn key(&self) -> &str {
        match self {
            Self::Rename { dest, .. } | Self::Path { path: dest } => dest,
        }
    }
}

/// Walk the repo status once and reduce it to the per-path plan the
/// working-tree diff needs: rename/copy pairs (from the staged `tree_index`
/// and unstaged `index_worktree` rewrite arms) and otherwise one
/// `WorkItem::Path` per changed/untracked path. Untracked files are requested
/// individually (`UntrackedFiles::Files`) so each becomes its own addition,
/// not a collapsed `dir/` entry. `None` if the status walk can't run.
pub fn collect_worktree_plan(repo: &gix::Repository) -> Option<Vec<WorkItem>> {
    use gix::bstr::ByteSlice;
    use gix::diff::index::ChangeRef;
    use gix::status::index_worktree::Item as IwItem;
    use gix::status::{Item, UntrackedFiles};
    use std::collections::BTreeSet;

    // `UntrackedFiles::Files`: each untracked file becomes its own WorkItem
    // so the diff plan sees individual additions. Staged-rename detection and
    // the deliberate omission of index↔worktree rewrite detection are shared
    // with `repo_status` via `crate::git::status::make_status_platform`.
    let platform = crate::git::status::make_status_platform(repo, UntrackedFiles::Files)?;

    // Rename pairs override the per-path treatment of their endpoints, so we
    // remember which paths are consumed by a rename and skip emitting a plain
    // Path item for them.
    let mut renames: Vec<WorkItem> = Vec::new();
    let mut consumed: BTreeSet<String> = BTreeSet::new();
    let mut plain: BTreeSet<String> = BTreeSet::new();

    for item in platform.into_iter(None).ok()? {
        let item = item.ok()?;
        match item {
            Item::TreeIndex(change) => match change {
                ChangeRef::Rewrite {
                    source_location,
                    location,
                    copy,
                    ..
                } => {
                    let source = source_location.to_str_lossy().into_owned();
                    let dest = location.to_str_lossy().into_owned();
                    consumed.insert(source.clone());
                    consumed.insert(dest.clone());
                    renames.push(WorkItem::Rename { source, dest, copy });
                }
                ChangeRef::Addition { location, .. }
                | ChangeRef::Deletion { location, .. }
                | ChangeRef::Modification { location, .. } => {
                    plain.insert(location.to_str_lossy().into_owned());
                }
            },
            Item::IndexWorktree(iw) => match iw {
                IwItem::Rewrite {
                    source,
                    dirwalk_entry,
                    ..
                } => {
                    // With `index_worktree_rewrites` left disabled (above) gix
                    // doesn't emit this for the unstaged half — the rename
                    // decomposes into a Modification{Removed} (source) plus a
                    // DirectoryContents{Untracked} (dest), which the arms below
                    // turn into a deletion + an addition. Decode it the same way
                    // defensively (BOTH sides, not just the dest) so the deletion
                    // half survives even if rewrite detection is ever re-enabled.
                    plain.insert(source.rela_path().to_str_lossy().into_owned());
                    plain.insert(dirwalk_entry.rela_path.to_str_lossy().into_owned());
                }
                IwItem::Modification { rela_path, .. } => {
                    plain.insert(rela_path.to_str_lossy().into_owned());
                }
                IwItem::DirectoryContents {
                    entry: dir_entry, ..
                } => {
                    if dir_entry.status == gix::dir::entry::Status::Untracked
                        && dir_entry.disk_kind != Some(gix::dir::entry::Kind::Directory)
                    {
                        plain.insert(dir_entry.rela_path.to_str_lossy().into_owned());
                    }
                }
            },
        }
    }

    let mut out = renames;
    for p in plain {
        if !consumed.contains(&p) {
            out.push(WorkItem::Path { path: p });
        }
    }
    Some(out)
}

/// Build a `FileDiff` for a detected working-tree rename/copy: old side is the
/// HEAD-tree blob at `source`, new side is the worktree file at `dest`.
pub fn build_worktree_rename(
    rc: &mut gix::diff::blob::Platform,
    repo: &gix::Repository,
    source: &str,
    dest: &str,
    copy: bool,
) -> Option<FileDiff> {
    use gix::bstr::BStr;
    use gix::object::tree::EntryKind;

    let head_tree = repo.head_tree().ok()?;
    let (old_id, _kind) = tree_blob_at(&head_tree, source)?;
    let null = gix::ObjectId::null(repo.object_hash());

    // Similarity is computed against the actual worktree content of `dest`.
    let similarity = worktree_similarity(repo, old_id, &repo.workdir()?.join(dest));
    let status = if copy {
        FileStatus::Copied { similarity }
    } else {
        FileStatus::Renamed { similarity }
    };

    set_blob(
        rc,
        repo,
        old_id,
        EntryKind::Blob,
        BStr::new(source.as_bytes()),
        false,
    );
    set_blob(
        rc,
        repo,
        null,
        EntryKind::Blob,
        BStr::new(dest.as_bytes()),
        true,
    );

    Some(FileDiff {
        old_path: Some(source.to_string()),
        new_path: Some(dest.to_string()),
        status,
        kind: diff_kind_from_cache(rc),
        lang_hint: lang_hint_for(dest),
    })
}

/// Build a `FileDiff` for a working-tree (`gd`) path: old side is the HEAD
/// blob (if any), new side is the worktree file (read via the worktree root
/// in `rc`). `None` if there's nothing to diff. The status is `Added` (no old
/// blob), `Deleted` (no worktree file), or `Modified`.
pub fn build_worktree_file(
    rc: &mut gix::diff::blob::Platform,
    repo: &gix::Repository,
    path: &str,
    old_blob: Option<gix::ObjectId>,
    worktree_present: bool,
) -> Option<FileDiff> {
    use gix::bstr::BStr;
    use gix::object::tree::EntryKind;

    let rela = BStr::new(path.as_bytes());
    let null = gix::ObjectId::null(repo.object_hash());

    let (status, old_path, new_path) = match (&old_blob, worktree_present) {
        (None, true) => (FileStatus::Added, None, Some(path.to_string())),
        (Some(_), false) => (FileStatus::Deleted, Some(path.to_string()), None),
        (Some(_), true) => (
            FileStatus::Modified,
            Some(path.to_string()),
            Some(path.to_string()),
        ),
        // Neither present (e.g. a path that vanished between status + diff):
        // nothing to show.
        (None, false) => return None,
    };

    // Old side: the HEAD blob (object id, no worktree root) or a null id for an
    // addition. New side: a null id with the worktree root set, so the cache
    // reads the live file by `rela_path` (a missing file is treated as a
    // deletion by the pipeline, which is what we want).
    let old_id = old_blob.unwrap_or(null);
    set_blob(rc, repo, old_id, EntryKind::Blob, rela, false);
    set_blob(rc, repo, null, EntryKind::Blob, rela, true);

    Some(FileDiff {
        old_path,
        new_path,
        status,
        kind: diff_kind_from_cache(rc),
        lang_hint: lang_hint_for(path),
    })
}

/// An owned copy of one `tree_index` (staged) change, decoupled from the
/// transient `ChangeRef` borrow so the diff resource cache (which also borrows
/// the repo's object store) can be built afterwards.
pub enum OwnedIndexChange {
    Addition {
        path: String,
        id: gix::ObjectId,
        kind: gix::object::tree::EntryKind,
    },
    Deletion {
        path: String,
        id: gix::ObjectId,
        kind: gix::object::tree::EntryKind,
    },
    Modification {
        path: String,
        old_id: gix::ObjectId,
        old_kind: gix::object::tree::EntryKind,
        new_id: gix::ObjectId,
        new_kind: gix::object::tree::EntryKind,
    },
    Rewrite {
        source_path: String,
        source_id: gix::ObjectId,
        source_kind: gix::object::tree::EntryKind,
        dest_path: String,
        dest_id: gix::ObjectId,
        dest_kind: gix::object::tree::EntryKind,
        copy: bool,
    },
}

impl OwnedIndexChange {
    /// The destination/own path used to sort and path-filter staged changes.
    pub fn path(&self) -> &str {
        match self {
            Self::Addition { path, .. }
            | Self::Deletion { path, .. }
            | Self::Modification { path, .. } => path,
            Self::Rewrite { dest_path, .. } => dest_path,
        }
    }
}

/// Convert a borrowed `tree_index` `ChangeRef` into an owned `OwnedIndexChange`,
/// or `None` for an entry kind we can't represent as a blob (e.g. a tree mode).
pub fn own_index_change(change: &gix::diff::index::ChangeRef<'_, '_>) -> Option<OwnedIndexChange> {
    use gix::bstr::ByteSlice;
    use gix::diff::index::ChangeRef;

    let mode_kind = |m: gix::index::entry::Mode| m.to_tree_entry_mode().map(|tm| tm.kind());

    match change {
        ChangeRef::Addition {
            location,
            entry_mode,
            id,
            ..
        } => Some(OwnedIndexChange::Addition {
            path: location.to_str_lossy().into_owned(),
            id: id.as_ref().to_owned(),
            kind: mode_kind(*entry_mode)?,
        }),
        ChangeRef::Deletion {
            location,
            entry_mode,
            id,
            ..
        } => Some(OwnedIndexChange::Deletion {
            path: location.to_str_lossy().into_owned(),
            id: id.as_ref().to_owned(),
            kind: mode_kind(*entry_mode)?,
        }),
        ChangeRef::Modification {
            location,
            previous_entry_mode,
            previous_id,
            entry_mode,
            id,
            ..
        } => Some(OwnedIndexChange::Modification {
            path: location.to_str_lossy().into_owned(),
            old_id: previous_id.as_ref().to_owned(),
            old_kind: mode_kind(*previous_entry_mode)?,
            new_id: id.as_ref().to_owned(),
            new_kind: mode_kind(*entry_mode)?,
        }),
        ChangeRef::Rewrite {
            source_location,
            source_entry_mode,
            source_id,
            location,
            entry_mode,
            id,
            copy,
            ..
        } => Some(OwnedIndexChange::Rewrite {
            source_path: source_location.to_str_lossy().into_owned(),
            source_id: source_id.as_ref().to_owned(),
            source_kind: mode_kind(*source_entry_mode)?,
            dest_path: location.to_str_lossy().into_owned(),
            dest_id: id.as_ref().to_owned(),
            dest_kind: mode_kind(*entry_mode)?,
            copy: *copy,
        }),
    }
}

/// Build a `FileDiff` for one staged (`tree_index`) change, running the blob
/// line diff via the resource cache.
pub fn build_index_change_file(
    rc: &mut gix::diff::blob::Platform,
    repo: &gix::Repository,
    change: &OwnedIndexChange,
) -> FileDiff {
    use gix::bstr::BStr;

    match change {
        OwnedIndexChange::Addition { path, id, kind } => {
            set_blob(
                rc,
                repo,
                gix::ObjectId::null(repo.object_hash()),
                *kind,
                BStr::new(path.as_bytes()),
                false,
            );
            set_blob(rc, repo, *id, *kind, BStr::new(path.as_bytes()), true);
            FileDiff {
                old_path: None,
                new_path: Some(path.clone()),
                status: FileStatus::Added,
                kind: diff_kind_from_cache(rc),
                lang_hint: lang_hint_for(path),
            }
        }
        OwnedIndexChange::Deletion { path, id, kind } => {
            set_blob(rc, repo, *id, *kind, BStr::new(path.as_bytes()), false);
            set_blob(
                rc,
                repo,
                gix::ObjectId::null(repo.object_hash()),
                *kind,
                BStr::new(path.as_bytes()),
                true,
            );
            FileDiff {
                old_path: Some(path.clone()),
                new_path: None,
                status: FileStatus::Deleted,
                kind: diff_kind_from_cache(rc),
                lang_hint: lang_hint_for(path),
            }
        }
        OwnedIndexChange::Modification {
            path,
            old_id,
            old_kind,
            new_id,
            new_kind,
        } => {
            let status = if old_kind == new_kind {
                FileStatus::Modified
            } else {
                FileStatus::TypeChange
            };
            set_blob(
                rc,
                repo,
                *old_id,
                *old_kind,
                BStr::new(path.as_bytes()),
                false,
            );
            set_blob(
                rc,
                repo,
                *new_id,
                *new_kind,
                BStr::new(path.as_bytes()),
                true,
            );
            FileDiff {
                old_path: Some(path.clone()),
                new_path: Some(path.clone()),
                status,
                kind: diff_kind_from_cache(rc),
                lang_hint: lang_hint_for(path),
            }
        }
        OwnedIndexChange::Rewrite {
            source_path,
            source_id,
            source_kind,
            dest_path,
            dest_id,
            dest_kind,
            copy,
        } => {
            let similarity = blob_similarity(repo, *source_id, *dest_id);
            let status = if *copy {
                FileStatus::Copied { similarity }
            } else {
                FileStatus::Renamed { similarity }
            };
            set_blob(
                rc,
                repo,
                *source_id,
                *source_kind,
                BStr::new(source_path.as_bytes()),
                false,
            );
            set_blob(
                rc,
                repo,
                *dest_id,
                *dest_kind,
                BStr::new(dest_path.as_bytes()),
                true,
            );
            FileDiff {
                old_path: Some(source_path.clone()),
                new_path: Some(dest_path.clone()),
                status,
                kind: diff_kind_from_cache(rc),
                lang_hint: lang_hint_for(dest_path),
            }
        }
    }
}

/// Build a `FileDiff` for one `diff_tree_to_tree` change (used by `show`),
/// using gix's `set_resource_by_change` to set the resources from the blob ids.
/// Submodule (commit-mode) entries become [`DiffKind::Submodule`] rather than a
/// blob diff.
pub fn build_tree_change_file(
    rc: &mut gix::diff::blob::Platform,
    repo: &gix::Repository,
    change: gix::object::tree::diff::ChangeDetached,
) -> Option<FileDiff> {
    use gix::bstr::ByteSlice;
    use gix::object::tree::EntryMode;
    use gix::object::tree::diff::ChangeDetached as Change;

    let is_submodule = |m: EntryMode| m.is_commit();

    // Detect submodule changes (commit-mode entries) before running a blob
    // diff, which would otherwise try to read a non-existent blob.
    match &change {
        Change::Addition {
            location,
            entry_mode,
            id,
            ..
        } if is_submodule(*entry_mode) => {
            let path = location.to_str_lossy().into_owned();
            return Some(FileDiff {
                old_path: None,
                new_path: Some(path.clone()),
                status: FileStatus::Added,
                kind: DiffKind::Submodule {
                    old: String::new(),
                    new: id.to_hex().to_string(),
                },
                lang_hint: lang_hint_for(&path),
            });
        }
        Change::Deletion {
            location,
            entry_mode,
            id,
            ..
        } if is_submodule(*entry_mode) => {
            let path = location.to_str_lossy().into_owned();
            return Some(FileDiff {
                old_path: Some(path.clone()),
                new_path: None,
                status: FileStatus::Deleted,
                kind: DiffKind::Submodule {
                    old: id.to_hex().to_string(),
                    new: String::new(),
                },
                lang_hint: lang_hint_for(&path),
            });
        }
        Change::Modification {
            location,
            previous_id,
            id,
            entry_mode,
            previous_entry_mode,
            ..
        } if is_submodule(*entry_mode) || is_submodule(*previous_entry_mode) => {
            let path = location.to_str_lossy().into_owned();
            return Some(FileDiff {
                old_path: Some(path.clone()),
                new_path: Some(path.clone()),
                status: FileStatus::Modified,
                kind: DiffKind::Submodule {
                    old: previous_id.to_hex().to_string(),
                    new: id.to_hex().to_string(),
                },
                lang_hint: lang_hint_for(&path),
            });
        }
        _ => {}
    }

    // Determine the model status + paths from the change shape.
    let (old_path, new_path, status) = match &change {
        Change::Addition { location, .. } => (
            None,
            Some(location.to_str_lossy().into_owned()),
            FileStatus::Added,
        ),
        Change::Deletion { location, .. } => (
            Some(location.to_str_lossy().into_owned()),
            None,
            FileStatus::Deleted,
        ),
        Change::Modification {
            location,
            entry_mode,
            previous_entry_mode,
            ..
        } => {
            let status = if entry_mode.kind() == previous_entry_mode.kind() {
                FileStatus::Modified
            } else {
                FileStatus::TypeChange
            };
            let p = location.to_str_lossy().into_owned();
            (Some(p.clone()), Some(p), status)
        }
        Change::Rewrite {
            source_location,
            location,
            copy,
            source_id,
            id,
            ..
        } => {
            let similarity = blob_similarity(repo, *source_id, *id);
            let status = if *copy {
                FileStatus::Copied { similarity }
            } else {
                FileStatus::Renamed { similarity }
            };
            (
                Some(source_location.to_str_lossy().into_owned()),
                Some(location.to_str_lossy().into_owned()),
                status,
            )
        }
    };

    let lang = lang_hint_for(new_path.as_deref().or(old_path.as_deref()).unwrap_or(""));

    // Set both resources from the blob ids, then read the resulting hunks.
    rc.set_resource_by_change(change.to_ref(), &repo.objects)
        .ok()?;
    let kind = diff_kind_from_cache(rc);

    Some(FileDiff {
        old_path,
        new_path,
        status,
        kind,
        lang_hint: lang,
    })
}
