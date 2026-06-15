//! Git worktree list / add / remove on gix (gitoxide) plumbing + `std::fs`.
//!
//! gix has no high-level `worktree add` / `worktree remove`, so create and
//! remove are hand-rolled to reproduce exactly what `git worktree` writes:
//! the admin dir under `<common_dir>/worktrees/<name>/` (with `gitdir`,
//! `commondir`, `HEAD`, `index`), the worktree's `.git` *gitfile*, the
//! branch ref, and the checkout of the branch's tree into the new worktree.
//! `list` walks `repo.worktrees()` and emits the MAIN worktree first to
//! match `git worktree list --porcelain` ordering (the `git_state.rs`
//! "← current" marker lines up against that order).
//!
//! The `Worktree` shape is the facade's public output so the `W l` pager
//! formatting and the `W d` removal flow in `app/` stay untouched.
//!
//! ## Correctness
//!
//! This is data-affecting (it writes linked worktrees into real repos), so
//! the tests verify the hand-built worktree two ways: the real `git` binary
//! must register + list it (`git worktree list --porcelain` against the main
//! repo), and gix must open it and report a clean status + the right HEAD.
//! (The worktree *internals* are checked via gix in-process rather than a
//! spawned `git`: a child `git` needs a valid process CWD at startup, which
//! the parallel test suite's global-CWD thrash can leave deleted — gix
//! tolerates that, a spawned `git` does not.)

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// A parsed git worktree entry.
pub struct Worktree {
    pub path: PathBuf,
    /// Short commit hash.
    pub head: String,
    /// Branch name, "(detached)", or "(bare)".
    pub branch: String,
}

/// Short-7 commit prefix for a repo's HEAD, or empty if HEAD can't resolve
/// (an unborn HEAD on a fresh worktree, say). Matches `git`'s `--short`
/// default width used by the porcelain `HEAD` line truncation.
fn head_short(repo: &gix::Repository) -> String {
    repo.head_id()
        .map(|id| id.to_hex_with_len(7).to_string())
        .unwrap_or_default()
}

/// Branch display for a repo's HEAD: the short branch name (`main`) for an
/// attached HEAD, or `"(detached)"` when HEAD points straight at a commit.
fn head_branch_label(repo: &gix::Repository) -> String {
    match repo.head_name() {
        Ok(Some(name)) => name.shorten().to_string(),
        _ => "(detached)".to_string(),
    }
}

/// List the repo's worktrees, MAIN first then linked ones (matching
/// `git worktree list --porcelain` ordering so the "← current" marker in
/// `git_state.rs` lines up). `None` if `dir` isn't inside a repository.
///
/// Uses `gix::discover` (walks up to the enclosing repo) rather than
/// `gix::open` (strict — repo must be *at* `dir`): callers pass the user's
/// listing dir, so browsing `repo/src/` must still resolve the repo, matching
/// the upward discovery the prior `git worktree list` subprocess inherited.
pub fn list(dir: &Path) -> Option<Vec<Worktree>> {
    let repo = gix::discover(dir).ok()?;
    let mut worktrees = Vec::new();

    // The main worktree is NOT in `repo.worktrees()` (which enumerates the
    // LINKED worktrees only), and `git worktree list` shows it first. Resolve
    // it from the shared COMMON dir, *not* from `repo.workdir()`: when the
    // user has switched INTO a linked worktree, discovery opened that linked
    // worktree, whose `workdir()` is the linked dir — emitting it here
    // listed the *current* linked worktree as the `[1]` "main" entry and
    // dropped the real main, so the user could no longer switch back to it
    // (Spencer's "[1] shows where I currently am, can't get back to my git
    // home"). The common dir (`<main>/.git`) is identical for every worktree;
    // opening it yields the main worktree's repo view regardless of `dir`.
    // Canonicalize first — gix can hand back a CWD-relative `common_dir`
    // (see `add`), and the second open must not depend on the process CWD.
    let common_dir = std::fs::canonicalize(repo.common_dir())
        .unwrap_or_else(|_| repo.common_dir().to_path_buf());
    match gix::open(&common_dir)
        .ok()
        .and_then(|main| main.workdir().map(|w| (main.clone(), w.to_path_buf())))
    {
        Some((main, workdir)) => worktrees.push(Worktree {
            path: workdir,
            head: head_short(&main),
            branch: head_branch_label(&main),
        }),
        // Bare repo: no main working tree, but git still lists the bare
        // entry first. Keep the "(bare)" label the callers expect.
        None => worktrees.push(Worktree {
            path: common_dir,
            head: head_short(&repo),
            branch: "(bare)".to_string(),
        }),
    }

    for proxy in repo.worktrees().ok()? {
        let path = proxy
            .base()
            .unwrap_or_else(|_| proxy.git_dir().to_path_buf());
        // Open the linked worktree's own repo view for its HEAD/branch.
        // `into_repo_with_possibly_inaccessible_worktree` still resolves
        // HEAD/branch even if the checkout dir was moved away.
        let (head, branch) = match proxy.into_repo_with_possibly_inaccessible_worktree() {
            Ok(wt_repo) => (head_short(&wt_repo), head_branch_label(&wt_repo)),
            Err(_) => (String::new(), "(detached)".to_string()),
        };
        worktrees.push(Worktree { path, head, branch });
    }

    if worktrees.is_empty() {
        None
    } else {
        Some(worktrees)
    }
}

/// Create a new worktree under a per-repo `<repo>.worktrees/` dir next to
/// the working-tree root (`<repo_parent>/<repo>.worktrees/<branch>`): use an
/// EXISTING branch `<branch>` if present, else create it at the current HEAD.
/// Returns the new worktree's path.
pub fn add(dir: &Path, branch: &str) -> std::io::Result<PathBuf> {
    // `gix::discover` (not `gix::open`) so adding from a repo subdirectory
    // resolves the enclosing repo and anchors the new worktree on its root,
    // matching `git worktree add` from anywhere in the tree.
    let repo = gix::discover(dir).map_err(|e| std::io::Error::other(format!("open repo: {e}")))?;

    // Resolve the working-tree root and common git dir to ABSOLUTE,
    // fs-canonical paths up front. gix derives these relative to the
    // process CWD it captures at open (`gix_fs::current_dir`); under spyc's
    // own chdir (and, in tests, a parallel suite that thrashes the global
    // CWD) gix can hand back a CWD-relative path. Canonicalizing here makes
    // every path we *persist* (the worktree gitfile + admin `gitdir`)
    // absolute — a relative `gitdir:` would make `git` resolve the admin
    // dir against the worktree and fail to find it (the bug this guards).
    let root = std::fs::canonicalize(
        repo.workdir()
            .ok_or_else(|| std::io::Error::other("cannot add a worktree to a bare repository"))?,
    )?;
    let common_dir = std::fs::canonicalize(repo.common_dir())?;

    // Group worktrees under a per-repo sibling dir, so they don't clutter the
    // repo's parent or collide with unrelated dirs:
    //   <repo_parent>/<repo>.worktrees/<branch>
    // (The old subprocess path put a bare `<repo_parent>/<branch>`, which
    // collided with same-named siblings like an existing `~/src/test`.)
    let parent = root.parent().unwrap_or(&root);
    let repo_name = root.file_name().and_then(|n| n.to_str()).unwrap_or("repo");
    let target = parent.join(format!("{repo_name}.worktrees")).join(branch);

    if target.exists() && std::fs::read_dir(&target).is_ok_and(|mut d| d.next().is_some()) {
        return Err(std::io::Error::other(format!(
            "'{}' already exists and is not empty",
            target.display()
        )));
    }

    // Resolve / create the branch ref, yielding the commit to check out.
    let full_ref = format!("refs/heads/{branch}");
    let commit_id = resolve_or_create_branch(&repo, &common_dir, &full_ref, branch)?;

    // Refuse if the branch is already checked out in another worktree
    // (matches `git worktree add`).
    if let Some(existing) = list(dir) {
        for wt in &existing {
            if wt.branch == branch && wt.path != target {
                return Err(std::io::Error::other(format!(
                    "'{branch}' is already checked out at '{}'",
                    wt.path.display()
                )));
            }
        }
    }

    // The tree to materialize is the branch commit's tree.
    let tree_id = repo
        .find_commit(commit_id)
        .map_err(|e| std::io::Error::other(format!("find commit: {e}")))?
        .tree_id()
        .map_err(|e| std::io::Error::other(format!("commit tree: {e}")))?
        .detach();

    // Pick the admin dir name (dedupe like git: name, name1, name2, …).
    let worktrees_root = common_dir.join("worktrees");
    let name = unique_admin_name(&worktrees_root, branch);
    let admin_dir = worktrees_root.join(&name);

    std::fs::create_dir_all(&target)?;
    std::fs::create_dir_all(&admin_dir)?;

    // Build the worktree index from the branch tree, point it at the admin
    // `index` file, check the tree out into `target`, then persist the index.
    let mut index = repo
        .index_from_tree(&tree_id)
        .map_err(|e| std::io::Error::other(format!("index from tree: {e}")))?;
    index.set_path(admin_dir.join("index"));

    let checkout_opts = repo
        .checkout_options(gix::worktree::stack::state::attributes::Source::IdMapping)
        .map_err(|e| std::io::Error::other(format!("checkout options: {e}")))?;
    let objects = repo
        .objects
        .into_arc()
        .map_err(|e| std::io::Error::other(format!("object store: {e}")))?;
    let should_interrupt = AtomicBool::new(false);
    gix::worktree::state::checkout(
        &mut index,
        &target,
        objects,
        &gix::progress::Discard,
        &gix::progress::Discard,
        &should_interrupt,
        checkout_opts,
    )
    .map_err(|e| std::io::Error::other(format!("checkout: {e}")))?;
    index
        .write(gix::index::write::Options::default())
        .map_err(|e| std::io::Error::other(format!("write index: {e}")))?;

    // Write the admin files git expects, byte-for-byte.
    write_admin_files(&admin_dir, &target, branch)?;

    Ok(target)
}

/// Resolve `full_ref` (`refs/heads/<branch>`) to its commit id, creating the
/// branch at the current HEAD commit if it doesn't exist yet (replicating
/// `git worktree add` with vs without `-b`). `common_dir` is the absolute
/// shared git dir the loose ref is written under.
fn resolve_or_create_branch(
    repo: &gix::Repository,
    common_dir: &Path,
    full_ref: &str,
    branch: &str,
) -> std::io::Result<gix::ObjectId> {
    // `find_reference` checks both loose and packed refs.
    if let Ok(mut existing) = repo.find_reference(full_ref) {
        return existing
            .peel_to_id()
            .map(gix::Id::detach)
            .map_err(|e| std::io::Error::other(format!("peel branch '{branch}': {e}")));
    }
    // Validate the ref name via gix (rejects path-traversal / invalid names)
    // before writing the loose ref by hand.
    gix::refs::FullName::try_from(full_ref)
        .map_err(|e| std::io::Error::other(format!("invalid branch name '{branch}': {e}")))?;
    let head_commit = repo
        .head_id()
        .map_err(|e| std::io::Error::other(format!("resolve HEAD: {e}")))?
        .detach();
    // Write the loose ref directly (`<common_dir>/refs/heads/<branch>` = the
    // 40-hex commit + newline). We bypass gix's `repo.reference()` because it
    // writes a reflog with `force_create_reflog: false`, which fails to
    // *create* the new branch's reflog in some environments ("the reflog
    // could not be created or updated" — hit on CI but not locally). A loose
    // ref is just a file git + gix both read; the (optional) reflog is the
    // only thing skipped, and git recreates it on the next update.
    let ref_path = common_dir.join(full_ref);
    if let Some(parent) = ref_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&ref_path, format!("{}\n", head_commit.to_hex()))?;
    Ok(head_commit)
}

/// Pick the admin-dir name under `worktrees_root`: the basename, deduped the
/// way git does (`name`, then `name1`, `name2`, …) on collision.
fn unique_admin_name(worktrees_root: &Path, branch: &str) -> String {
    // Use the final path component of the branch as the base name, matching
    // git (a branch `feat/x` would key on `x`). For our callers `branch` is
    // already a leaf, but be defensive.
    let base = Path::new(branch)
        .file_name()
        .map_or(branch, |n| n.to_str().unwrap_or(branch));
    if !worktrees_root.join(base).exists() {
        return base.to_string();
    }
    let mut n = 1u32;
    loop {
        let candidate = format!("{base}{n}");
        if !worktrees_root.join(&candidate).exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Write the four admin files + the worktree's `.git` gitfile exactly as
/// `git worktree add` does (absolute paths, trailing newlines):
///   `<admin>/gitdir`   = `<target>/.git\n`
///   `<admin>/commondir`= `../..\n`
///   `<admin>/HEAD`     = `ref: refs/heads/<branch>\n`
///   `<target>/.git`    = `gitdir: <admin>\n`
/// (`<admin>/index` is written separately by the checkout step.)
fn write_admin_files(admin_dir: &Path, target: &Path, branch: &str) -> std::io::Result<()> {
    let target_dot_git = target.join(".git");
    std::fs::write(
        admin_dir.join("gitdir"),
        format!("{}\n", target_dot_git.display()),
    )?;
    std::fs::write(admin_dir.join("commondir"), "../..\n")?;
    std::fs::write(
        admin_dir.join("HEAD"),
        format!("ref: refs/heads/{branch}\n"),
    )?;
    std::fs::write(
        &target_dot_git,
        format!("gitdir: {}\n", admin_dir.display()),
    )?;
    Ok(())
}

/// Remove a worktree by path, replicating `git worktree remove <path>`:
/// refuse a dirty worktree (uncommitted/untracked changes), then delete both
/// the worktree dir and its admin dir under `<common_dir>/worktrees/<name>`.
/// The branch ref is left intact (git's `worktree remove` doesn't delete it).
pub fn remove(path: &Path) -> std::io::Result<()> {
    // Resolve the admin dir from the worktree's `.git` gitfile.
    let admin_dir = admin_dir_of(path)?;

    // SAFETY: refuse if locked (git refuses a locked worktree).
    if admin_dir.join("locked").is_file() {
        return Err(std::io::Error::other("worktree is locked; will not remove"));
    }

    // SAFETY: refuse a dirty worktree (uncommitted or untracked changes),
    // matching `git worktree remove`. An empty status Vec is clean.
    match crate::git::status::repo_status(path) {
        Some(entries) if !entries.is_empty() => {
            return Err(std::io::Error::other(
                "worktree contains modified or untracked files, use --force to delete it",
            ));
        }
        // `None` = couldn't open it as a repo; fall through to removal (the
        // gitfile resolved an admin dir, so it is a worktree — a status
        // failure shouldn't strand the user with an un-removable worktree).
        _ => {}
    }

    if path.exists() {
        std::fs::remove_dir_all(path)?;
    }
    if admin_dir.exists() {
        std::fs::remove_dir_all(&admin_dir)?;
    }
    Ok(())
}

/// Resolve a worktree's admin dir from its `<path>/.git` gitfile
/// (`gitdir: <admin>\n`). Errors if `<path>` isn't a linked worktree (no
/// gitfile, or a real `.git` directory).
fn admin_dir_of(path: &Path) -> std::io::Result<PathBuf> {
    let gitfile = path.join(".git");
    let contents = std::fs::read_to_string(&gitfile)?;
    let rest = contents
        .lines()
        .find_map(|l| l.strip_prefix("gitdir:"))
        .ok_or_else(|| {
            std::io::Error::other(format!(
                "'{}' is not a linked worktree (no gitdir gitfile)",
                path.display()
            ))
        })?;
    Ok(PathBuf::from(rest.trim()))
}

#[cfg(test)]
mod tests {
    use super::{add, list, remove};
    use crate::git::test_support::run_git;
    use std::path::{Path, PathBuf};

    /// Resolve a worktree's HEAD commit (full hex) via gix, IN-PROCESS.
    ///
    /// We validate the worktree *internals* (HEAD, clean status) through gix
    /// rather than a spawned `git`: a spawned `git` needs a valid process CWD
    /// at startup (`getcwd`), and under the suite's parallel global-CWD thrash
    /// (a sibling test's `set_current_dir` + tempdir-drop) that CWD can be
    /// *deleted*, breaking git ~1/9 of the time regardless of
    /// `current_dir`/`-C`/`GIT_DIR`. gix tolerates a deleted CWD (it's how
    /// `add()` itself opens the repo every run), so gix-based checks are
    /// reliable. Real-`git` acceptance is still cross-checked via
    /// `git worktree list` against the MAIN repo (a real `.git` directory,
    /// which never raced).
    fn gix_head_sha(dir: &Path) -> String {
        gix::open(dir)
            .expect("gix open worktree")
            .head_id()
            .expect("worktree HEAD")
            .to_hex()
            .to_string()
    }

    /// The per-worktree gitdir a worktree's `.git` gitfile points at.
    fn admin_of(target: &Path) -> PathBuf {
        let gf = std::fs::read_to_string(target.join(".git")).expect("read gitfile");
        PathBuf::from(
            gf.lines()
                .find_map(|l| l.strip_prefix("gitdir:"))
                .expect("gitdir gitfile")
                .trim(),
        )
    }

    /// Build a main repo with two commits, canonicalized (macOS
    /// /var→/private/var so gix's absolute paths line up). The repo lives in
    /// a `repo/` subdir of its own tempdir so the worktree target (a SIBLING
    /// of the repo root, `<tempdir>/<branch>`) is unique per test — otherwise
    /// every test's `feature` worktree would collide in the shared `$TMPDIR`.
    fn init_repo() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(tmp.path()).unwrap_or_else(|_| tmp.path().to_path_buf());
        let root = base.join("repo");
        std::fs::create_dir(&root).unwrap();
        run_git(&root, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(root.join("f.txt"), "v1\n").unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("g.txt"), "g1\n").unwrap();
        run_git(&root, &["add", "."]);
        run_git(&root, &["commit", "-q", "-m", "c1"]);
        std::fs::write(root.join("f.txt"), "v1\nv2\n").unwrap();
        run_git(&root, &["add", "f.txt"]);
        run_git(&root, &["commit", "-q", "-m", "c2"]);
        (tmp, root)
    }

    fn rev_parse(dir: &Path, rev: &str) -> String {
        run_git(dir, &["rev-parse", rev]).trim().to_string()
    }

    #[test]
    fn add_new_branch_real_git_accepts_worktree() {
        let (_tmp, main) = init_repo();
        let main_head = gix_head_sha(&main);

        let target = add(&main, "feature").expect("add worktree");

        // The worktree's gitfile must point to an ABSOLUTE, existing admin
        // dir — a relative `gitdir:` (which git resolves against the
        // worktree, not the repo) is the failure mode we're guarding.
        let gf = std::fs::read_to_string(target.join(".git")).unwrap_or_default();
        let admin = gf
            .strip_prefix("gitdir:")
            .map(str::trim)
            .unwrap_or_default();
        assert!(
            Path::new(admin).is_absolute() && Path::new(admin).is_dir(),
            "broken gitfile: {gf:?} (admin abs={}, exists={}); cwd={:?}",
            Path::new(admin).is_absolute(),
            Path::new(admin).is_dir(),
            std::env::current_dir(),
        );

        // Worktrees are grouped under `<repo>.worktrees/` (a sibling of the
        // repo), not bare in the repo's parent.
        assert_eq!(
            target
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str()),
            Some("repo.worktrees"),
            "worktree should be grouped under <repo>.worktrees/, got {target:?}"
        );

        // (a) The target exists with the committed files checked out.
        assert!(target.is_dir(), "target dir missing: {}", target.display());
        assert_eq!(
            std::fs::read_to_string(target.join("f.txt")).unwrap(),
            "v1\nv2\n"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("sub").join("g.txt")).unwrap(),
            "g1\n"
        );

        // (b) Real git lists the new worktree with branch refs/heads/feature.
        let porcelain = run_git(&main, &["worktree", "list", "--porcelain"]);
        let target_canon = std::fs::canonicalize(&target).unwrap_or_else(|_| target.clone());
        assert!(
            porcelain
                .lines()
                .any(|l| l == format!("worktree {}", target_canon.display())
                    || l == format!("worktree {}", target.display())),
            "porcelain missing worktree line:\n{porcelain}"
        );
        assert!(
            porcelain.contains("branch refs/heads/feature"),
            "porcelain missing branch:\n{porcelain}"
        );

        // (c) the worktree is clean and (d) its HEAD matches main — via gix
        // in-process (reliable under the suite's CWD thrash; see
        // `gix_head_sha`). The gitfile→admin chain was asserted above, and (b)
        // already had real `git` accept + list the worktree.
        assert!(
            crate::git::status::repo_status(&target).is_some_and(|e| e.is_empty()),
            "worktree should be clean, got {:?}",
            crate::git::status::repo_status(&target)
        );
        assert_eq!(
            gix_head_sha(&target),
            main_head,
            "worktree HEAD != main HEAD"
        );

        // (e) The branch exists in the repo.
        let branches = run_git(&main, &["branch", "--list", "feature"]);
        assert!(branches.contains("feature"), "branch missing: {branches:?}");
    }

    #[test]
    fn add_existing_branch_checks_out_that_branch() {
        let (_tmp, main) = init_repo();
        // Pre-create a branch at an earlier commit so it's distinguishable.
        run_git(&main, &["branch", "existing", "HEAD~1"]);
        let existing_head = rev_parse(&main, "existing");
        assert_ne!(existing_head, rev_parse(&main, "HEAD"));

        let target = add(&main, "existing").expect("add worktree for existing branch");

        // It checks out the existing branch's commit, not a fresh HEAD branch
        // (gix in-process — see `gix_head_sha`).
        assert_eq!(
            gix_head_sha(&target),
            existing_head,
            "worktree should be on the existing branch's commit"
        );
        let porcelain = run_git(&main, &["worktree", "list", "--porcelain"]);
        assert!(porcelain.contains("branch refs/heads/existing"));
        // f.txt at HEAD~1 had only "v1\n".
        assert_eq!(
            std::fs::read_to_string(target.join("f.txt")).unwrap(),
            "v1\n"
        );
    }

    #[test]
    fn list_orders_main_first_then_linked() {
        let (_tmp, main) = init_repo();
        let target = add(&main, "feature").expect("add");

        let worktrees = list(&main).expect("list");
        assert_eq!(worktrees.len(), 2, "expected main + linked");

        // Main worktree first.
        assert_eq!(worktrees[0].path, main);
        assert_eq!(worktrees[0].branch, "main");
        assert_eq!(worktrees[0].head.len(), 7);
        assert!(worktrees[0].head.chars().all(|c| c.is_ascii_hexdigit()));

        // Linked worktree second.
        assert_eq!(worktrees[1].path, target);
        assert_eq!(worktrees[1].branch, "feature");
        assert_eq!(worktrees[1].head.len(), 7);

        // Cross-check against real git porcelain: same order (main first).
        let porcelain = run_git(&main, &["worktree", "list", "--porcelain"]);
        let git_paths: Vec<&str> = porcelain
            .lines()
            .filter_map(|l| l.strip_prefix("worktree "))
            .collect();
        assert_eq!(git_paths.len(), 2, "git should list two worktrees");
        assert!(
            git_paths[0].ends_with("repo"),
            "git's first worktree should be main: {git_paths:?}"
        );
        assert!(
            git_paths[1].ends_with("feature"),
            "git's second worktree should be the linked one: {git_paths:?}"
        );
        // Our short head should match git's HEAD line truncated to 7.
        let git_heads: Vec<String> = porcelain
            .lines()
            .filter_map(|l| l.strip_prefix("HEAD "))
            .map(|h| h[..7].to_string())
            .collect();
        assert_eq!(worktrees[0].head, git_heads[0]);
        assert_eq!(worktrees[1].head, git_heads[1]);
    }

    #[test]
    fn list_from_linked_worktree_still_shows_main_first() {
        let (_tmp, main) = init_repo();
        let target = add(&main, "feature").expect("add");

        // Listing from INSIDE the linked worktree must still enumerate the
        // main worktree + the linked one, main first — NOT list the current
        // linked worktree as `[1]` and drop main. The old code emitted
        // `repo.workdir()`, which for a repo opened from a linked worktree is
        // the linked dir, so the user couldn't switch back to main (Spencer:
        // "[1] shows where I currently am, can't switch back to my git home").
        let from_linked = list(&target).expect("list from linked worktree");
        let paths: Vec<_> = from_linked.iter().map(|w| w.path.clone()).collect();
        assert!(
            paths.contains(&main),
            "main worktree missing when listing from a linked worktree: {paths:?}"
        );
        assert_eq!(
            from_linked[0].path, main,
            "main worktree must be listed first regardless of which worktree we list from: {paths:?}"
        );
        assert_eq!(
            from_linked.len(),
            2,
            "expected exactly main + one linked worktree (no duplicate of the current one): {paths:?}"
        );
        // And the order/labels match listing from main itself.
        let from_main = list(&main).expect("list from main");
        let main_paths: Vec<_> = from_main.iter().map(|w| w.path.clone()).collect();
        assert_eq!(
            paths, main_paths,
            "listing must be identical whether opened from main or a linked worktree"
        );
    }

    #[test]
    fn remove_clean_worktree_leaves_nothing() {
        let (_tmp, main) = init_repo();
        let target = add(&main, "feature").expect("add");
        let admin_dir = admin_of(&target);
        assert!(admin_dir.is_dir(), "admin dir should exist before remove");

        remove(&target).expect("remove clean worktree");

        // The worktree dir and admin dir are both gone.
        assert!(!target.exists(), "worktree dir still present");
        assert!(!admin_dir.exists(), "admin dir not pruned");

        // Real git no longer lists it — WITHOUT needing a prune first.
        let porcelain = run_git(&main, &["worktree", "list", "--porcelain"]);
        assert!(
            !porcelain.contains("feature")
                && porcelain
                    .lines()
                    .filter(|l| l.starts_with("worktree "))
                    .count()
                    == 1,
            "git still lists the removed worktree:\n{porcelain}"
        );
    }

    #[test]
    fn remove_dirty_worktree_refuses() {
        let (_tmp, main) = init_repo();
        let target = add(&main, "feature").expect("add");

        // Dirty a tracked file in the worktree.
        std::fs::write(target.join("f.txt"), "v1\nv2\nDIRTY\n").unwrap();

        let err = remove(&target).expect_err("dirty worktree should refuse removal");
        assert!(
            err.to_string().contains("modified or untracked"),
            "unexpected error: {err}"
        );
        // The worktree dir is still there.
        assert!(target.is_dir(), "dirty worktree was deleted!");
    }

    #[test]
    fn remove_dirty_with_untracked_refuses() {
        let (_tmp, main) = init_repo();
        let target = add(&main, "feature").expect("add");
        std::fs::write(target.join("brand_new.txt"), "untracked\n").unwrap();

        let err = remove(&target).expect_err("untracked content should refuse removal");
        assert!(err.to_string().contains("modified or untracked"));
        assert!(target.is_dir());
    }

    #[test]
    fn list_works_from_a_repo_subdirectory() {
        let (_tmp, main) = init_repo();
        // `init_repo` creates `<main>/sub/` with a committed file.
        let sub = main.join("sub");
        assert!(sub.is_dir(), "fixture should have a subdir");

        // `W l` passes the user's listing dir; browsing `repo/sub/` must still
        // find the repo (upward discovery), not fail "not a git repository"
        // the way the strict `gix::open(dir)` did.
        let from_sub = list(&sub).expect("list from a repo subdirectory");
        assert_eq!(
            from_sub[0].path,
            main,
            "subdir list should resolve the enclosing repo, main first: {:?}",
            from_sub.iter().map(|w| &w.path).collect::<Vec<_>>()
        );
    }

    #[test]
    fn add_works_from_a_repo_subdirectory() {
        let (_tmp, main) = init_repo();
        let sub = main.join("sub");

        // Adding from a subdir must anchor the worktree on the REPO root
        // (`<repo>.worktrees/<branch>`), not on the subdir.
        let target = add(&sub, "feature").expect("add from a repo subdirectory");
        assert_eq!(
            target
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str()),
            Some("repo.worktrees"),
            "worktree should be grouped under the repo's <repo>.worktrees/, got {target:?}"
        );
        assert!(
            target.is_dir(),
            "worktree dir missing: {}",
            target.display()
        );
        // Real git accepts it against the MAIN repo.
        let porcelain = run_git(&main, &["worktree", "list", "--porcelain"]);
        assert!(
            porcelain.contains("branch refs/heads/feature"),
            "porcelain missing the new worktree:\n{porcelain}"
        );
    }

    #[test]
    fn list_none_outside_repo() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(list(tmp.path()).is_none());
    }
}
