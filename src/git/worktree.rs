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

    // The main worktree isn't in `repo.worktrees()` (linked only) but git lists
    // it first. Resolve it from the shared common dir, NOT `repo.workdir()`:
    // discovery from inside a linked worktree opens that worktree, so workdir()
    // would list the current linked worktree as main and drop the real one.
    // Canonicalize first — gix can hand back a CWD-relative `common_dir`.
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

/// Serialize worktree mutations across the whole test process.
///
/// gix acquires `<admin>/index.lock` and loose-ref locks with
/// `Fail::Immediately` while building or tearing down a worktree. The parallel
/// test runner starts many worktree-mutating tests at once, and on a loaded CI
/// filesystem their lock lifecycles overlap enough that an acquire fails
/// outright — an intermittent flake with no production analogue, since spyc
/// drives worktree ops one at a time through its single update loop + the
/// off-thread worktree worker. Holding this mutex for the whole mutation means
/// at most one worktree is built or removed at a time under test,
/// deterministically. Compiled out of release builds (`cfg(test)`), so
/// production keeps its lock-free path plus the transient-contention retry in
/// [`write_index_with_lock_retry`]. Poison is recovered so one panicking test
/// can't wedge every other worktree test behind it.
#[cfg(test)]
fn serialize_worktree_mutation() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Create a new worktree under a per-repo `<repo>.worktrees/` dir next to
/// the working-tree root (`<repo_parent>/<repo>.worktrees/<branch>`): use an
/// EXISTING branch `<branch>` if present, else create it at `base` (a ref/rev,
/// typically the repo's default branch), or HEAD when `base` is `None`.
/// Returns the new worktree's path.
pub fn add(dir: &Path, branch: &str, base: Option<&str>) -> std::io::Result<PathBuf> {
    #[cfg(test)]
    let _serial = serialize_worktree_mutation();
    // `gix::discover` (not `gix::open`) so adding from a repo subdirectory
    // resolves the enclosing repo and anchors the new worktree on its root,
    // matching `git worktree add` from anywhere in the tree.
    let repo = gix::discover(dir).map_err(|e| std::io::Error::other(format!("open repo: {e}")))?;

    // Canonicalize up front: gix returns paths relative to the CWD it captured
    // at open, and a relative `gitdir:` in the persisted worktree gitfile makes
    // git resolve it against the worktree and miss.
    let common_dir = std::fs::canonicalize(repo.common_dir())?;
    // Anchor `.worktrees/` on the MAIN worktree root, not whatever enclosing
    // worktree `dir` was discovered from — else adding from inside a linked
    // worktree nests the new one under it (#511). The common dir is shared by
    // every worktree, so opening it always yields the main repo view.
    let root = if let Some(workdir) = gix::open(&common_dir)
        .ok()
        .and_then(|main| main.workdir().map(std::path::Path::to_path_buf))
    {
        // Non-bare: the main worktree root, canonicalized (absolute persisted paths).
        std::fs::canonicalize(workdir)?
    } else {
        // Bare repo: derive the root from the (canonical) git dir and place
        // worktrees beside it, matching `git worktree add`. Strip a `.git`
        // basename or `.git` extension; otherwise the git dir is the root.
        let cd = common_dir.as_path();
        if cd.file_name() == Some(std::ffi::OsStr::new(".git")) {
            cd.parent().unwrap_or(cd).to_path_buf()
        } else if cd.extension() == Some(std::ffi::OsStr::new("git")) {
            cd.with_extension("")
        } else {
            cd.to_path_buf()
        }
    };

    // Group worktrees under a per-repo `<repo>.worktrees/` sibling dir so they
    // don't clutter the parent or collide with same-named siblings.
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
    let commit_id = resolve_or_create_branch(&repo, &common_dir, &full_ref, branch, base)?;

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

    materialize_worktree(repo, tree_id, &target, &admin_dir, branch)?;

    Ok(target)
}

/// Create the worktree + admin directories, check the branch tree out, and
/// write the admin files. On **any** failure, remove the partial directories
/// so a retry with the same branch name is possible — the non-empty-dir guard
/// in [`add`] would otherwise block it forever. (The branch ref, created
/// earlier, is intentionally left in place; an existing branch is reused on
/// retry.)
fn materialize_worktree(
    repo: gix::Repository,
    tree_id: gix::ObjectId,
    target: &Path,
    admin_dir: &Path,
    branch: &str,
) -> std::io::Result<()> {
    std::fs::create_dir_all(target)?;
    std::fs::create_dir_all(admin_dir)?;
    if let Err(e) = checkout_and_write(repo, tree_id, target, admin_dir, branch) {
        let _ = std::fs::remove_dir_all(target);
        let _ = std::fs::remove_dir_all(admin_dir);
        return Err(e);
    }
    Ok(())
}

/// Build the worktree index from `tree_id`, check it out into `target`, write
/// the index to `admin_dir/index`, then write the four admin files git expects.
/// Separated from [`materialize_worktree`] so partial state can be cleaned up
/// if any step fails.
fn checkout_and_write(
    repo: gix::Repository,
    tree_id: gix::ObjectId,
    target: &Path,
    admin_dir: &Path,
    branch: &str,
) -> std::io::Result<()> {
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
        target,
        objects,
        &gix::progress::Discard,
        &gix::progress::Discard,
        &should_interrupt,
        checkout_opts,
    )
    .map_err(|e| std::io::Error::other(format!("checkout: {e}")))?;
    write_index_with_lock_retry(&mut index)?;

    write_admin_files(admin_dir, target, branch)
}

/// Write the worktree index, riding out transient contention on its lock file.
///
/// gix acquires `<admin>/index.lock` with `Fail::Immediately` — no internal
/// wait — so a contender that holds the lock for even a moment (a concurrent
/// gix operation in production, or the heavily-parallel test runner) makes a
/// `write` fail instantly with `AcquireLock` rather than blocking. A bounded
/// exponential backoff (10 attempts, capped at 300ms/step, ~1.8s total) rides
/// out that transient contention on a loaded CI filesystem, while a genuinely
/// stuck lock still fails in bounded time instead of hanging (it is NOT
/// stomped — see `write_index_gives_up_on_a_stuck_lock_instead_of_hanging`).
/// Re-issuing `write` is safe: it re-serializes the same in-memory index and
/// re-acquires from scratch, so no partial state carries across attempts.
fn write_index_with_lock_retry(index: &mut gix::index::File) -> std::io::Result<()> {
    const ATTEMPTS: u32 = 10;
    const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_millis(300);
    let mut backoff = std::time::Duration::from_millis(20);
    for attempt in 1..=ATTEMPTS {
        match index.write(gix::index::write::Options::default()) {
            Ok(()) => return Ok(()),
            Err(gix::index::file::write::Error::AcquireLock(_)) if attempt < ATTEMPTS => {
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
            Err(e) => return Err(std::io::Error::other(format!("write index: {e}"))),
        }
    }
    // The `attempt < ATTEMPTS` guard routes the final attempt's errors to the
    // catch-all above, so every path through the loop has already returned.
    unreachable!("write_index_with_lock_retry loop always returns")
}

/// Resolve `full_ref` (`refs/heads/<branch>`) to its commit id. If the branch
/// doesn't exist yet, create it at `base` (a ref/rev — typically the repo's
/// default branch, so a new worktree starts off the trunk, NOT the asking
/// column's HEAD), falling back to HEAD when `base` is `None` or unresolvable.
/// `common_dir` is the absolute shared git dir the loose ref is written under.
fn resolve_or_create_branch(
    repo: &gix::Repository,
    common_dir: &Path,
    full_ref: &str,
    branch: &str,
    base: Option<&str>,
) -> std::io::Result<gix::ObjectId> {
    // `find_reference` checks both loose and packed refs. An existing branch is
    // checked out as-is — `base` only matters when CREATING the branch.
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
    // Start the new branch at `base` (the repo's default branch) when given and
    // resolvable, else the current HEAD — the pre-POLA behavior, and the
    // fallback when there's no resolvable default (origin-less / unborn repo).
    let start_commit = match base.and_then(|b| {
        repo.rev_parse_single(gix::bstr::BStr::new(b.as_bytes()))
            .ok()
    }) {
        Some(id) => id.detach(),
        None => repo
            .head_id()
            .map_err(|e| std::io::Error::other(format!("resolve HEAD: {e}")))?
            .detach(),
    };
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
    std::fs::write(&ref_path, format!("{}\n", start_commit.to_hex()))?;
    Ok(start_commit)
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
    remove_inner(path, false)
}

/// Like [`remove`], but skips the *dirty* refusal — for safe-remove, which has
/// already archived the worktree's uncommitted/untracked content to the
/// graveyard. A *lease* (lock) is still honored: `force` forces past dirt, not
/// past another session's claim — release it first.
pub fn remove_force(path: &Path) -> std::io::Result<()> {
    remove_inner(path, true)
}

fn remove_inner(path: &Path, force_dirty: bool) -> std::io::Result<()> {
    #[cfg(test)]
    let _serial = serialize_worktree_mutation();
    // Resolve the admin dir from the worktree's `.git` gitfile.
    let admin_dir = admin_dir_of(path)?;

    // SAFETY (always, even under force): refuse if locked (git refuses a locked
    // worktree). spyc's `claim_worktree` lease writes this file with an owner
    // reason, so surface it — a cooperating session sees WHO claimed it and why.
    let locked = admin_dir.join("locked");
    if locked.is_file() {
        let reason = std::fs::read_to_string(&locked).unwrap_or_default();
        let reason = reason.trim();
        return Err(std::io::Error::other(if reason.is_empty() {
            "worktree is locked (claimed) — release it to remove".to_string()
        } else {
            format!("worktree is locked (claimed): {reason} — release it to remove")
        }));
    }

    // SAFETY: refuse a dirty worktree (uncommitted or untracked changes),
    // matching `git worktree remove` — unless the caller forces (safe-remove,
    // which archived the dirt first). An empty status Vec is clean.
    if !force_dirty {
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
    }

    if path.exists() {
        std::fs::remove_dir_all(path)?;
    }
    if admin_dir.exists() {
        std::fs::remove_dir_all(&admin_dir)?;
    }
    Ok(())
}

/// Lock a worktree using git's native mechanism: write `<admin>/locked` with
/// `reason`. `remove` (and `git worktree remove`) refuse a locked worktree, so
/// this is spyc's worktree "claim/lease" — an agent locks the worktree it's
/// working in so a cooperating session won't tear it down. Errors if `path`
/// isn't a linked worktree (the main worktree can't be locked, like in git).
pub fn lock(path: &Path, reason: &str) -> std::io::Result<()> {
    let admin_dir = admin_dir_of(path)?;
    std::fs::write(admin_dir.join("locked"), reason)
}

/// Release a worktree lock (clear `<admin>/locked`). No-op if already unlocked.
pub fn unlock(path: &Path) -> std::io::Result<()> {
    let locked = admin_dir_of(path)?.join("locked");
    if locked.is_file() {
        std::fs::remove_file(locked)?;
    }
    Ok(())
}

/// The lock reason if the worktree is locked, else `None`. `Some(String::new())`
/// for a worktree locked without a recorded reason. `None` for the main
/// worktree (no gitfile) or any non-worktree path.
pub fn lock_reason(path: &Path) -> Option<String> {
    let locked = admin_dir_of(path).ok()?.join("locked");
    locked.is_file().then(|| {
        std::fs::read_to_string(&locked)
            .unwrap_or_default()
            .trim()
            .to_string()
    })
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
    use super::{add, list, lock, lock_reason, remove, unlock};
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
    fn new_branch_starts_at_base_not_head() {
        let (_tmp, main) = init_repo();
        // HEAD moves to an OLDER commit (a feature branch off `main~1`), while
        // `main` stays at the newer tip — so "base vs HEAD" is observable.
        run_git(&main, &["checkout", "-q", "-b", "feature", "HEAD~1"]);
        let main_tip = rev_parse(&main, "main");
        let head_tip = rev_parse(&main, "HEAD");
        assert_ne!(main_tip, head_tip, "precondition: HEAD != main");
        // Create a worktree for a NEW branch based on `main`, not HEAD.
        add(&main, "wt", Some("main")).expect("add with base");
        assert_eq!(
            rev_parse(&main, "wt"),
            main_tip,
            "new branch must start at the base (main), not the checked-out HEAD"
        );
    }

    #[test]
    fn add_from_inside_linked_worktree_anchors_on_main() {
        // The nesting bug: when the asking dir sits INSIDE an existing linked
        // worktree (e.g. a spyc column navigated into one), the new worktree
        // must still land as a SIBLING of the MAIN repo
        // (`<repo>.worktrees/<branch>`), not nested under the linked one
        // (`<linked>.worktrees/<branch>`).
        let (_tmp, main) = init_repo();
        let group = main.parent().unwrap().join("repo.worktrees");
        let wt1 = add(&main, "feat-a", None).expect("first worktree");
        assert!(
            wt1.starts_with(&group),
            "precondition: first worktree under repo.worktrees/, got {wt1:?}"
        );

        // Add a SECOND worktree, asking from INSIDE the first one.
        let wt2 = add(&wt1, "feat-b", None).expect("second worktree from inside the first");

        assert_eq!(
            wt2,
            group.join("feat-b"),
            "worktree must anchor on the main repo, got {wt2:?}"
        );
        assert!(
            !wt2.starts_with(&wt1),
            "worktree must NOT nest under the asking linked worktree: {wt2:?}"
        );
    }

    #[test]
    fn add_to_bare_repo_anchors_beside_repo_root() {
        // A bare main repo (no working tree) must still accept `add()`:
        // `git worktree add` supports it, but the gix path used to bail with
        // "cannot add a worktree to a bare repository" because a bare repo has
        // no `workdir()` — which broke `create_worktree` on spyc's own canonical
        // layout (a bare main checkout with sibling worktrees). The new worktree
        // must land beside the repo root (`<root>.worktrees/<branch>`), derived
        // from the `<root>/.git` git dir.
        let (_tmp, main) = init_repo();
        // Flip the repo bare so gix reports no workdir, exercising the
        // bare-fallback path. The `.git` dir and its objects stay in place.
        run_git(&main, &["config", "core.bare", "true"]);
        let base_head = gix_head_sha(&main); // a bare repo's HEAD still resolves

        let target = add(&main, "wt-bare", None).expect("add to bare repo must succeed");

        let group = main.parent().unwrap().join("repo.worktrees");
        assert_eq!(
            target,
            group.join("wt-bare"),
            "worktree must anchor beside the bare repo root, got {target:?}"
        );
        assert!(target.is_dir(), "worktree dir must be materialized");
        assert_eq!(
            gix_head_sha(&target),
            base_head,
            "new worktree HEAD must match the bare repo's base commit"
        );
    }

    #[test]
    fn add_new_branch_real_git_accepts_worktree() {
        let (_tmp, main) = init_repo();
        let main_head = gix_head_sha(&main);

        let target = add(&main, "feature", None).expect("add worktree");

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

        let target = add(&main, "existing", None).expect("add worktree for existing branch");

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
        let target = add(&main, "feature", None).expect("add");

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
        let target = add(&main, "feature", None).expect("add");

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
        let target = add(&main, "feature", None).expect("add");
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
        let target = add(&main, "feature", None).expect("add");

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
        let target = add(&main, "feature", None).expect("add");
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
        let target = add(&sub, "feature", None).expect("add from a repo subdirectory");
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

    #[test]
    fn lock_claims_a_worktree_and_remove_refuses_until_released() {
        let (_tmp, main) = init_repo();
        let wt = add(&main, "feature", None).expect("add worktree");
        assert_eq!(lock_reason(&wt), None, "fresh worktree is unlocked");

        // Claim it; the reason round-trips and remove refuses, surfacing it.
        lock(&wt, "agent A: working").expect("lock");
        assert_eq!(lock_reason(&wt).as_deref(), Some("agent A: working"));
        let err = remove(&wt).expect_err("remove must refuse a locked worktree");
        assert!(
            err.to_string().contains("agent A: working"),
            "lock reason should be in the refusal: {err}"
        );

        // Release, then a clean remove proceeds.
        unlock(&wt).expect("unlock");
        assert_eq!(lock_reason(&wt), None);
        remove(&wt).expect("remove after release");
        assert!(!wt.exists());
    }

    #[test]
    fn materialize_worktree_cleans_up_partial_dirs_on_failure() {
        // The cleanup-on-failure fix: when checkout fails partway, both the
        // target and admin directories `materialize_worktree` created must be
        // removed so a retry with the same branch name isn't blocked by the
        // non-empty-dir guard. Force a deterministic failure by passing a null
        // ObjectId as the tree id — `index_from_tree` can't resolve it, so
        // checkout_and_write errors *after* the directories are created.
        let (_tmp, main) = init_repo();
        let group = main.parent().unwrap().join("repo.worktrees");
        let target = group.join("boom");
        let admin_dir = main.join(".git").join("worktrees").join("boom");
        assert!(
            !target.exists() && !admin_dir.exists(),
            "clean precondition"
        );

        let repo = gix::open(&main).expect("open repo");
        let bogus_tree = gix::ObjectId::null(repo.object_hash());

        let res = super::materialize_worktree(repo, bogus_tree, &target, &admin_dir, "boom");
        assert!(res.is_err(), "null tree id must fail the checkout");

        // Both directories the helper created must be gone — without the
        // cleanup, the empty `target` would block a same-name retry.
        assert!(!target.exists(), "partial target dir not cleaned up");
        assert!(!admin_dir.exists(), "partial admin dir not cleaned up");
    }

    #[test]
    fn add_succeeds_after_a_prior_failed_attempt() {
        // End-to-end companion: a failed materialize leaves nothing behind, so
        // a subsequent real `add` with the SAME branch name succeeds (the bug
        // was that leftover dirs tripped the "already exists and not empty"
        // guard forever).
        let (_tmp, main) = init_repo();
        let group = main.parent().unwrap().join("repo.worktrees");
        let target = group.join("feature");
        let admin_dir = main.join(".git").join("worktrees").join("feature");

        let repo = gix::open(&main).expect("open repo");
        let bogus_tree = gix::ObjectId::null(repo.object_hash());
        assert!(
            super::materialize_worktree(repo, bogus_tree, &target, &admin_dir, "feature").is_err(),
            "forced failure"
        );

        // Same branch name now adds cleanly.
        let added = add(&main, "feature", None).expect("retry add after failure");
        assert_eq!(added, target);
        assert!(added.is_dir());
    }

    #[test]
    fn write_index_gives_up_on_a_stuck_lock_instead_of_hanging() {
        // A permanently-held `index.lock` must make the write FAIL (surfacing
        // the lock error) after the bounded retries — never hang, never skip
        // the write. The complementary recover-from-a-TRANSIENT-lock path is
        // covered by add_succeeds_after_a_prior_failed_attempt, which now runs
        // through the same retry helper.
        let (_tmp, main) = init_repo();
        let repo = gix::open(&main).expect("open repo");
        let tree = repo
            .head_commit()
            .expect("head commit")
            .tree_id()
            .expect("tree id")
            .detach();
        let mut index = repo.index_from_tree(&tree).expect("index from tree");

        let index_path = main.join(".git").join("wtidx-stuck-lock-test");
        index.set_path(&index_path);
        // Plant the lock gix acquires (`<resource>.lock`) so every attempt
        // fails immediately.
        let lock_path = index_path.with_extension("lock");
        std::fs::write(&lock_path, b"").expect("plant stuck lock");

        let err = super::write_index_with_lock_retry(&mut index)
            .expect_err("a permanently-held lock must surface an error");
        assert!(
            err.to_string().contains("lock"),
            "error should name the lock, got: {err}"
        );
        // The write never landed while the lock was held.
        assert!(
            !index_path.exists(),
            "index must not be written out under a held lock"
        );
    }

    #[test]
    fn worktree_mutations_are_serialized_under_test() {
        // The cfg(test) mutex must give strict mutual exclusion so parallel
        // worktree tests never overlap their gix lock lifecycles — the concurrency
        // that produced the CI "could not acquire lock for index file" flake.
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let inside = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let inside = Arc::clone(&inside);
                let max_seen = Arc::clone(&max_seen);
                std::thread::spawn(move || {
                    let _serial = super::serialize_worktree_mutation();
                    let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    inside.fetch_sub(1, Ordering::SeqCst);
                })
            })
            .collect();
        for h in handles {
            h.join().expect("serialized worker joins");
        }
        assert_eq!(
            max_seen.load(Ordering::SeqCst),
            1,
            "at most one worktree mutation may hold the serialization lock at a time"
        );
    }
}
