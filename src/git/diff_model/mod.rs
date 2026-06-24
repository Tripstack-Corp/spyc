//! Structured diff/show model builders on gix (no `git` subprocess).
//!
//! These produce a pure-data [`DiffModel`](crate::git::model) /
//! [`CommitMeta`](crate::git::model::CommitMeta) straight from gix and feed
//! the in-house renderer in [`crate::ui::diff_render`]. They are the live
//! diff/show path — production no longer shells out to git.
//!
//! Three scopes mirror spyc's git keys:
//! * [`diff_head_to_worktree`] — `gd`: `HEAD` vs the working tree
//!   (staged + unstaged + untracked).
//! * [`diff_cached`] — `gD`: `HEAD` tree vs the index (staged only).
//! * [`show_model`] — `git show <rev>`: a commit vs its first parent
//!   (root commit → vs the empty tree), plus commit metadata.
//!
//! The line-level hunks come from gix's blob diff (see [`blob`]): each changed
//! file's old + new blob are set as resources on a `gix::diff::blob::Platform`,
//! then `prepare_diff()` either flags the pair binary or yields an
//! `InternedInput` we run through `imara-diff` and reassemble into git-style
//! hunks with `CONTEXT_LINES` of context. Per-change → `FileDiff` translation
//! lives in [`build`].
//!
//! `app/git_view_session.rs` builds these models off-thread and renders
//! them in-house.

mod blob;
mod build;

pub use blob::{commit_meta, format_git_time};

use crate::git::model::DiffModel;
use std::path::Path;

/// Decoded-object LRU budget for the `gd` per-path HEAD-tree lookups. Tree
/// objects are small (sub-KB), so a few MB caches every directory a realistic
/// diff touches — shared parent trees get decoded once instead of once per
/// changed file. Bounded LRU, lives only for the off-thread model build.
const OBJECT_CACHE_BYTES: usize = 8 * 1024 * 1024;

use blob::{MAX_DIFF_LINES, account_file, path_selected, tree_blob_at};
use build::{
    OwnedIndexChange, WorkItem, build_index_change_file, build_tree_change_file,
    build_worktree_file, build_worktree_rename, collect_worktree_plan, own_index_change,
};

/// `gd` scope: working tree (staged + unstaged) vs `HEAD`. For every changed
/// tracked path the old side is the `HEAD`-tree blob and the new side is the
/// worktree file; untracked files come back as all-addition `FileDiff`s
/// (replacing the old `git diff --no-index /dev/null` trick). `None` if
/// `repo_root` can't be opened as a repository.
///
/// `paths` (repo-relative, forward-slash) optionally restricts the result to
/// matching files/subtrees; empty means "everything".
pub fn diff_head_to_worktree(repo_root: &Path, paths: &[String]) -> Option<DiffModel> {
    let mut repo = gix::open(repo_root).ok()?;
    // Cache decoded objects so the per-changed-path HEAD-tree lookups below
    // (`tree_blob_at` → root-down `lookup_entry_by_path`, and the same call in
    // `build_worktree_rename`) reuse shared parent trees instead of re-decoding
    // them from the pack for every path. Without it a large diff over a deep
    // tree pays O(N·depth) tree decodes, re-walking the same directories once
    // per changed file. Adaptive LRU (only caches trees actually touched);
    // `_if_unset` respects a user-configured `core.deltaBaseCacheLimit`-style
    // size if one is already set.
    repo.object_cache_size_if_unset(OBJECT_CACHE_BYTES);
    let workdir = repo.workdir()?.to_path_buf();
    let head_tree = repo.head_tree().ok();

    // Worktree-rooted resource cache so the "new" side can be read straight
    // from the working tree (null id + rela_path) for tracked + untracked.
    let mut rc = repo
        .diff_resource_cache(
            gix::diff::blob::pipeline::Mode::ToGit,
            gix::diff::blob::pipeline::WorktreeRoots {
                old_root: None,
                new_root: Some(workdir.clone()),
            },
        )
        .ok()?;

    // Reduce the status walk to a per-path plan of what to diff against the
    // worktree, with rename pairs detected and untracked files individual.
    let mut plan = collect_worktree_plan(&repo)?;
    plan.retain(|w| path_selected(w.key(), paths));
    plan.sort_by(|a, b| a.key().cmp(b.key()));

    let mut budget = MAX_DIFF_LINES;
    let mut files = Vec::new();
    let mut truncated = false;

    for item in plan {
        if truncated {
            break;
        }
        let file = match item {
            WorkItem::Rename {
                source, dest, copy, ..
            } => build_worktree_rename(&mut rc, &repo, &source, &dest, copy),
            WorkItem::Path { path } => {
                let old_blob = head_tree
                    .as_ref()
                    .and_then(|t| tree_blob_at(t, &path).map(|(id, _kind)| id));
                let worktree_present = workdir.join(&path).is_file();
                build_worktree_file(&mut rc, &repo, &path, old_blob, worktree_present)
            }
        };
        if let Some(file) = file {
            account_file(&file, &mut budget, &mut truncated);
            files.push(file);
        }
    }

    Some(DiffModel { files, truncated })
}

/// `gD` scope: `HEAD` tree vs the index (the "what would be committed" /
/// staged view). `None` if `repo_root` can't be opened.
///
/// Uses `tree_index_status` (HEAD tree diffed against the current worktree
/// index) with git's default rename detection (50%), then runs a blob diff
/// per change. `paths` restricts the result as in [`diff_head_to_worktree`].
pub fn diff_cached(repo_root: &Path, paths: &[String]) -> Option<DiffModel> {
    let repo = gix::open(repo_root).ok()?;
    // The HEAD tree id (empty tree on an unborn HEAD → everything staged is an
    // addition).
    let head_tree_id = match repo.head_tree_id() {
        Ok(id) => id.detach(),
        Err(_) => repo.empty_tree().id,
    };
    let index = repo.index_or_load_from_head_or_empty().ok()?;

    // Collect owned changes first so the diff resource cache (also borrowing
    // the repo's object store) isn't borrowed at the same time.
    let mut owned: Vec<OwnedIndexChange> = Vec::new();
    repo.tree_index_status(
        &head_tree_id,
        &index,
        None,
        gix::status::tree_index::TrackRenames::Given(gix::diff::Rewrites::default()),
        |change,
         _tree_index,
         _worktree_index|
         -> Result<gix::diff::index::Action, std::convert::Infallible> {
            if let Some(oc) = own_index_change(&change) {
                owned.push(oc);
            }
            Ok(std::ops::ControlFlow::Continue(()))
        },
    )
    .ok()?;

    let mut rc = repo.diff_resource_cache_for_tree_diff().ok()?;
    let mut budget = MAX_DIFF_LINES;
    let mut files = Vec::new();
    let mut truncated = false;

    owned.sort_by(|a, b| a.path().cmp(b.path()));
    for oc in owned {
        if truncated {
            break;
        }
        if !path_selected(oc.path(), paths) {
            continue;
        }
        let file = build_index_change_file(&mut rc, &repo, &oc);
        account_file(&file, &mut budget, &mut truncated);
        files.push(file);
    }

    Some(DiffModel { files, truncated })
}

/// `git show <rev>`: resolve `rev` to a commit, diff its tree against its
/// first-parent tree (root commit → vs the empty tree, so every line is an
/// addition), and return the commit metadata alongside the structured diff.
/// `None` if `repo_root` can't be opened or `rev` can't be resolved.
pub fn show_model(
    repo_root: &Path,
    rev: &str,
) -> Option<(crate::git::model::CommitMeta, DiffModel)> {
    use gix::bstr::BStr;

    let repo = gix::open(repo_root).ok()?;
    let id = repo.rev_parse_single(BStr::new(rev.as_bytes())).ok()?;
    let commit = repo.find_commit(id).ok()?;
    let meta = commit_meta(&commit)?;

    let new_tree = commit.tree().ok()?;
    // First parent's tree, or the empty tree for a root commit.
    let parent_tree = commit
        .parent_ids()
        .next()
        .and_then(|pid| repo.find_commit(pid.detach()).ok())
        .and_then(|p| p.tree().ok());

    let opts = gix::diff::Options::default().with_rewrites(Some(gix::diff::Rewrites::default()));
    let changes = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&new_tree), opts)
        .ok()?;

    let mut rc = repo.diff_resource_cache_for_tree_diff().ok()?;
    let mut budget = MAX_DIFF_LINES;
    let mut files = Vec::new();
    let mut truncated = false;

    // `show` has no path filter — every change in the commit is included.
    for change in changes {
        if truncated {
            break;
        }
        if let Some(file) = build_tree_change_file(&mut rc, &repo, change) {
            account_file(&file, &mut budget, &mut truncated);
            files.push(file);
        }
    }

    Some((meta, DiffModel { files, truncated }))
}

#[cfg(test)]
mod tests {
    //! Structural / parity tests for the gix model builders. The production
    //! builders are pure in-process gix; `git` is a test-only fixture used to
    //! *construct* the repos and cross-check ids.
    use super::{diff_cached, diff_head_to_worktree, show_model};
    use crate::git::model::{DiffKind, FileStatus, LineOrigin};
    use crate::git::test_support::run_git;
    use std::path::PathBuf;

    /// Fresh repo on `main`, canonicalized (macOS /var→/private/var so gix's
    /// absolute paths line up). No commits — callers add their own.
    fn empty_repo() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap_or_else(|_| tmp.path().to_path_buf());
        run_git(&root, &["init", "-q", "--initial-branch=main"]);
        (tmp, root)
    }

    /// `empty_repo` plus a committed `f.txt` of five numbered lines, so HEAD
    /// exists and there's a multi-line file to modify with surrounding context.
    fn repo_with_commit() -> (tempfile::TempDir, PathBuf) {
        let (tmp, root) = empty_repo();
        std::fs::write(root.join("f.txt"), "a\nb\nc\nd\ne\n").unwrap();
        run_git(&root, &["add", "f.txt"]);
        run_git(&root, &["commit", "-q", "-m", "c1"]);
        (tmp, root)
    }

    /// Pull the single text file matching `path` out of a model, asserting it's
    /// present and textual; returns its hunks.
    fn text_hunks<'a>(
        model: &'a crate::git::model::DiffModel,
        path: &str,
    ) -> &'a [crate::git::model::Hunk] {
        let file = model
            .files
            .iter()
            .find(|f| f.new_path.as_deref() == Some(path) || f.old_path.as_deref() == Some(path))
            .unwrap_or_else(|| panic!("no FileDiff for {path}: {:?}", model.files));
        match &file.kind {
            DiffKind::Text(h) => h,
            other => panic!("expected text diff for {path}, got {other:?}"),
        }
    }

    /// Collect the texts of lines of a given origin across all hunks.
    fn origin_texts(hunks: &[crate::git::model::Hunk], origin: LineOrigin) -> Vec<&str> {
        hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.origin == origin)
            .map(|l| l.text.as_str())
            .collect()
    }

    /// The `paths` filter restricts the working-tree diff to the selected
    /// file/subtree (what `gd` on a cursor file/folder relies on): a subtree
    /// filter keeps only files under it, a single-file filter keeps only that
    /// file, and an empty filter is "everything". (Guards against a regression
    /// in the scoping itself; the `gd`-shows-stale-diff bug was actually in the
    /// pager close path, not here — see `pager_stream` tests.)
    #[test]
    fn paths_filter_scopes_working_diff_to_selection() {
        let (_t, root) = repo_with_commit();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/inner.txt"), "x\n").unwrap();
        std::fs::write(root.join("top.txt"), "y\n").unwrap();
        std::fs::write(root.join("f.txt"), "a\nb\nC\nd\ne\n").unwrap();

        let names = |m: &crate::git::model::DiffModel| {
            let mut v: Vec<String> = m
                .files
                .iter()
                .map(|f| {
                    f.new_path
                        .clone()
                        .or_else(|| f.old_path.clone())
                        .unwrap_or_default()
                })
                .collect();
            v.sort();
            v
        };

        let all = diff_head_to_worktree(&root, &[]).expect("all");
        assert_eq!(
            names(&all),
            vec![
                "f.txt".to_string(),
                "sub/inner.txt".to_string(),
                "top.txt".to_string()
            ]
        );

        let scoped_sub = diff_head_to_worktree(&root, &["sub".to_string()]).expect("sub");
        assert_eq!(names(&scoped_sub), vec!["sub/inner.txt".to_string()]);

        let scoped_f = diff_head_to_worktree(&root, &["f.txt".to_string()]).expect("f");
        assert_eq!(names(&scoped_f), vec!["f.txt".to_string()]);
    }

    #[test]
    fn working_added_modified_deleted_structure() {
        let (_t, root) = repo_with_commit();
        // Add a new untracked file.
        std::fs::write(root.join("added.txt"), "new1\nnew2\n").unwrap();
        // Modify the tracked file: change line "c" → "C", around context.
        std::fs::write(root.join("f.txt"), "a\nb\nC\nd\ne\n").unwrap();
        // Delete another tracked file.
        std::fs::write(root.join("gone.txt"), "x\n").unwrap();
        run_git(&root, &["add", "gone.txt"]);
        run_git(&root, &["commit", "-q", "-m", "add gone"]);
        std::fs::remove_file(root.join("gone.txt")).unwrap();

        let model = diff_head_to_worktree(&root, &[]).expect("model");
        assert_eq!(model.files.len(), 3, "files: {:?}", model.files);
        assert!(!model.truncated);

        // Added file: status Added, all-add lines, no old path.
        let added = model
            .files
            .iter()
            .find(|f| f.new_path.as_deref() == Some("added.txt"))
            .unwrap();
        assert_eq!(added.status, FileStatus::Added);
        assert_eq!(added.old_path, None);
        assert_eq!(added.lang_hint, "txt");
        let DiffKind::Text(hunks) = &added.kind else {
            panic!("added not text");
        };
        assert_eq!(origin_texts(hunks, LineOrigin::Add), vec!["new1", "new2"]);
        assert!(origin_texts(hunks, LineOrigin::Remove).is_empty());
        assert!(origin_texts(hunks, LineOrigin::Context).is_empty());

        // Deleted file: status Deleted, no new path.
        let deleted = model
            .files
            .iter()
            .find(|f| f.old_path.as_deref() == Some("gone.txt"))
            .unwrap();
        assert_eq!(deleted.status, FileStatus::Deleted);
        assert_eq!(deleted.new_path, None);

        // Modified file: one remove "c", one add "C", surrounding context, and
        // a single hunk with the right line numbers.
        let modf = model
            .files
            .iter()
            .find(|f| f.new_path.as_deref() == Some("f.txt"))
            .unwrap();
        assert_eq!(modf.status, FileStatus::Modified);
        let DiffKind::Text(hunks) = &modf.kind else {
            panic!("modified not text");
        };
        assert_eq!(hunks.len(), 1, "expected one hunk");
        let h = &hunks[0];
        // file is only 5 lines, so context is clamped: a,b (context), -c, +C,
        // d,e (context). Both sides start at line 1.
        assert_eq!(h.old_start, 1);
        assert_eq!(h.new_start, 1);
        assert_eq!(origin_texts(hunks, LineOrigin::Remove), vec!["c"]);
        assert_eq!(origin_texts(hunks, LineOrigin::Add), vec!["C"]);
        assert_eq!(
            origin_texts(hunks, LineOrigin::Context),
            vec!["a", "b", "d", "e"]
        );
    }

    #[test]
    fn working_add_and_remove_lines() {
        // A modification that both adds and removes lines (not a 1:1 swap):
        // remove "b", add two lines after "a".
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("f.txt"), "a\nX\nY\nc\nd\ne\n").unwrap();
        let model = diff_head_to_worktree(&root, &[]).expect("model");
        let hunks = text_hunks(&model, "f.txt");
        assert_eq!(hunks.len(), 1);
        assert_eq!(origin_texts(hunks, LineOrigin::Remove), vec!["b"]);
        assert_eq!(origin_texts(hunks, LineOrigin::Add), vec!["X", "Y"]);
    }

    #[test]
    fn working_rename_detected_with_similarity() {
        let (_t, root) = empty_repo();
        // A file with enough content that a rename is >50% similar.
        let body = "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\n";
        std::fs::write(root.join("orig.txt"), body).unwrap();
        run_git(&root, &["add", "orig.txt"]);
        run_git(&root, &["commit", "-q", "-m", "add orig"]);
        // Rename via git so it's staged (working-tree scope sees staged too).
        run_git(&root, &["mv", "orig.txt", "renamed.txt"]);

        let model = diff_head_to_worktree(&root, &[]).expect("model");
        let renamed = model
            .files
            .iter()
            .find(|f| f.new_path.as_deref() == Some("renamed.txt"))
            .unwrap_or_else(|| panic!("no rename FileDiff: {:?}", model.files));
        match renamed.status {
            FileStatus::Renamed { similarity } => {
                assert!(
                    similarity >= 50,
                    "similarity should be >=50, got {similarity}"
                );
                assert_eq!(renamed.old_path.as_deref(), Some("orig.txt"));
            }
            other => panic!("expected Renamed, got {other:?}"),
        }
    }

    #[test]
    fn working_unstaged_rename_shows_both_delete_and_add() {
        let (_t, root) = empty_repo();
        let body = "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\n";
        std::fs::write(root.join("orig.txt"), body).unwrap();
        run_git(&root, &["add", "orig.txt"]);
        run_git(&root, &["commit", "-q", "-m", "add orig"]);
        // Filesystem rename WITHOUT staging. `git diff HEAD` (+ new) reports
        // orig.txt as DELETED and renamed.txt as ADDED — it never pairs an
        // untracked destination as a rename. The `gd` model must surface BOTH;
        // the old code dropped the deletion side (only the addition showed).
        std::fs::rename(root.join("orig.txt"), root.join("renamed.txt")).unwrap();

        let model = diff_head_to_worktree(&root, &[]).expect("model");
        let deleted = model
            .files
            .iter()
            .find(|f| f.old_path.as_deref() == Some("orig.txt"))
            .unwrap_or_else(|| panic!("deletion side dropped: {:?}", model.files));
        assert_eq!(deleted.status, FileStatus::Deleted);
        assert_eq!(deleted.new_path, None);

        let added = model
            .files
            .iter()
            .find(|f| f.new_path.as_deref() == Some("renamed.txt"))
            .unwrap_or_else(|| panic!("addition side missing: {:?}", model.files));
        assert_eq!(added.status, FileStatus::Added);
        assert_eq!(added.old_path, None);
    }

    #[test]
    fn working_binary_file_flagged() {
        let (_t, root) = repo_with_commit();
        // A file with NUL bytes → gix classifies it binary.
        std::fs::write(root.join("blob.bin"), [0u8, 159, 146, 150, 0, 1, 2]).unwrap();
        let model = diff_head_to_worktree(&root, &[]).expect("model");
        let bin = model
            .files
            .iter()
            .find(|f| f.new_path.as_deref() == Some("blob.bin"))
            .unwrap_or_else(|| panic!("no binary FileDiff: {:?}", model.files));
        assert_eq!(bin.kind, DiffKind::Binary, "should be binary");
    }

    #[test]
    fn working_empty_when_clean() {
        let (_t, root) = repo_with_commit();
        let model = diff_head_to_worktree(&root, &[]).expect("model");
        assert!(model.files.is_empty(), "clean tree: {:?}", model.files);
        assert!(!model.truncated);
    }

    #[test]
    fn cached_shows_staged_working_shows_staged_plus_unstaged() {
        let (_t, root) = repo_with_commit();
        // Stage one change.
        std::fs::write(root.join("f.txt"), "a\nb\nc\nd\nE\n").unwrap();
        run_git(&root, &["add", "f.txt"]);

        // diff_cached sees the staged "E".
        let cached = diff_cached(&root, &[]).expect("cached");
        assert_eq!(
            origin_texts(text_hunks(&cached, "f.txt"), LineOrigin::Add),
            vec!["E"],
            "cached should show staged E"
        );

        // diff_head_to_worktree also sees it (HEAD vs worktree).
        let working = diff_head_to_worktree(&root, &[]).expect("working");
        assert_eq!(
            origin_texts(text_hunks(&working, "f.txt"), LineOrigin::Add),
            vec!["E"]
        );

        // Add a further UNSTAGED edit on top: working reflects both (the new
        // worktree state vs HEAD), cached still only the staged "E".
        std::fs::write(root.join("f.txt"), "a\nB\nc\nd\nE\n").unwrap();
        let working2 = diff_head_to_worktree(&root, &[]).expect("working2");
        assert_eq!(
            origin_texts(text_hunks(&working2, "f.txt"), LineOrigin::Add),
            vec!["B", "E"],
            "working should reflect staged+unstaged"
        );

        let cached2 = diff_cached(&root, &[]).expect("cached2");
        assert_eq!(
            origin_texts(text_hunks(&cached2, "f.txt"), LineOrigin::Add),
            vec!["E"],
            "cached unaffected by later unstaged edit"
        );
    }

    #[test]
    fn show_head_commit_meta_and_diff() {
        let (_t, root) = repo_with_commit();
        // Second commit modifies f.txt: c → c2.
        std::fs::write(root.join("f.txt"), "a\nb\nc2\nd\ne\n").unwrap();
        run_git(&root, &["add", "f.txt"]);
        run_git(
            &root,
            &[
                "commit",
                "-q",
                "-m",
                "tweak c\n\nbody line one\nbody line two",
            ],
        );

        let (meta, model) = show_model(&root, "HEAD").expect("show HEAD");
        assert_eq!(meta.author, "Ada");
        assert_eq!(meta.email, "ada@example.com");
        assert_eq!(meta.subject, "tweak c");
        assert_eq!(meta.body, "body line one\nbody line two");
        assert_eq!(meta.id.len(), 40);
        assert_eq!(meta.short_id.len(), 7);
        assert!(meta.id.starts_with(&meta.short_id));
        assert!(!meta.time.is_empty());
        // Cross-check the full id against git.
        let git_head = run_git(&root, &["rev-parse", "HEAD"]).trim().to_string();
        assert_eq!(meta.id, git_head);

        // The diff has one file with the c → c2 swap.
        let hunks = text_hunks(&model, "f.txt");
        assert_eq!(hunks.len(), 1);
        assert_eq!(origin_texts(hunks, LineOrigin::Remove), vec!["c"]);
        assert_eq!(origin_texts(hunks, LineOrigin::Add), vec!["c2"]);
    }

    #[test]
    fn show_root_commit_is_all_additions() {
        let (_t, root) = repo_with_commit(); // single root commit creating f.txt
        let git_head = run_git(&root, &["rev-parse", "HEAD"]).trim().to_string();
        let (meta, model) = show_model(&root, &git_head).expect("show root");
        assert_eq!(meta.subject, "c1");

        let file = model
            .files
            .iter()
            .find(|f| f.new_path.as_deref() == Some("f.txt"))
            .unwrap();
        assert_eq!(file.status, FileStatus::Added, "root commit file is Added");
        let DiffKind::Text(hunks) = &file.kind else {
            panic!("root file not text");
        };
        // Every line is an addition (parent = empty tree).
        assert_eq!(
            origin_texts(hunks, LineOrigin::Add),
            vec!["a", "b", "c", "d", "e"]
        );
        assert!(origin_texts(hunks, LineOrigin::Remove).is_empty());
    }

    #[test]
    fn path_filter_restricts_files() {
        let (_t, root) = repo_with_commit();
        std::fs::write(root.join("f.txt"), "a\nb\nc\nd\nE\n").unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("g.txt"), "g\n").unwrap();

        // No filter: both the modified f.txt and the untracked sub/g.txt show.
        let all = diff_head_to_worktree(&root, &[]).expect("all");
        assert!(all.files.len() >= 2);

        // Filter to sub/: only the file under sub/.
        let only_sub = diff_head_to_worktree(&root, &["sub".to_string()]).expect("sub");
        assert!(
            only_sub
                .files
                .iter()
                .all(|f| f.new_path.as_deref().is_some_and(|p| p.starts_with("sub/"))),
            "filter leaked: {:?}",
            only_sub.files
        );
    }
}
