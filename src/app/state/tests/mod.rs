//! Unit tests for `AppState`, split out of `state` thematically.
#![allow(clippy::wildcard_imports)]

use super::*;
use crate::app::PromptKind;
use crate::fs::entry::EntryKind;
use crate::fs::listing::SortMode;
use crate::keymap::Action;
use crate::state::Mark;

mod apply;
mod dispatch;
mod navigation;

/// Build a minimal `AppState` for testing. Uses an empty listing
/// and sensible defaults — no disk I/O, no terminal.
fn test_state() -> AppState {
    AppState::test_default(PathBuf::from("/tmp/test"))
}

/// Build a test state with named rows (simulating a directory listing).
fn state_with_rows(names: &[&str]) -> AppState {
    let mut s = test_state();
    s.rows = names
        .iter()
        .map(|n| RowData {
            path: PathBuf::from(format!("/tmp/test/{n}")),
            display: n.to_string(),
            kind: EntryKind::File,
        })
        .collect();
    s
}

// ── focus accessor ────────────────────────────────────────────

fn state_with_real_files(tmp: &std::path::Path, names: &[&str]) -> AppState {
    let mut s = test_state();
    for name in names {
        std::fs::write(tmp.join(name), format!("content of {name}")).unwrap();
    }
    s.rows = names
        .iter()
        .map(|n| RowData {
            path: tmp.join(n),
            display: n.to_string(),
            kind: EntryKind::File,
        })
        .collect();
    s
}

fn dirty_state(names: &[&str], dirty: &[&str]) -> AppState {
    use crate::ui::list_view::{GitChange, GitFileStatus};
    let mut s = state_with_rows(names);
    for d in dirty {
        s.git.files.insert(
            (*d).to_string(),
            GitFileStatus::unstaged(GitChange::Modified),
        );
    }
    s
}

#[test]
fn apply_result_into_update() {
    assert!(matches!(Update::from(ApplyResult::Handled), Update::Handled(ref fx) if fx.is_empty()));
    assert!(matches!(
        Update::from(ApplyResult::NotHandled),
        Update::Defer
    ));
    // Post carries its effects through unchanged (count preserved).
    let fx = vec![Effect::SetTerminalTitle { title: "x".into() }];
    assert!(matches!(Update::from(ApplyResult::Post(fx)), Update::Handled(ref f) if f.len() == 1));
    // OpenPager passes the request through verbatim.
    let req = PagerRequest {
        title: "T".into(),
        lines: vec!["a".into()],
        columns: 2,
        fit_to_content: true,
    };
    assert!(matches!(
        Update::from(ApplyResult::OpenPager(req)),
        Update::OpenPager(r) if r.title == "T" && r.columns == 2 && r.fit_to_content
    ));
}

#[test]
fn command_result_into_update() {
    assert!(
        matches!(Update::from(CommandResult::Handled), Update::Handled(ref fx) if fx.is_empty())
    );
    assert!(matches!(
        Update::from(CommandResult::NotHandled),
        Update::Defer
    ));
    assert!(matches!(Update::from(CommandResult::Quit), Update::Quit));
    let fx = vec![Effect::SetTerminalTitle { title: "x".into() }];
    assert!(
        matches!(Update::from(CommandResult::Post(fx)), Update::Handled(ref f) if f.len() == 1)
    );
    // OpenPager{title,lines} normalizes to the `new_plain`-equivalent
    // PagerRequest (columns = 1, no fit-to-content).
    let u = Update::from(CommandResult::OpenPager {
        title: "marks".into(),
        lines: vec!["m1".into(), "m2".into()],
    });
    match u {
        Update::OpenPager(r) => {
            assert_eq!(r.title, "marks");
            assert_eq!(
                r.columns, 1,
                "command-path pager keeps new_plain's 1-column default"
            );
            assert!(!r.fit_to_content);
            assert_eq!(r.lines, vec!["m1".to_string(), "m2".to_string()]);
        }
        _ => panic!("expected OpenPager"),
    }
}

#[test]
fn prompt_result_into_update() {
    assert!(
        matches!(Update::from(PromptResult::Handled), Update::Handled(ref fx) if fx.is_empty())
    );
    assert!(matches!(
        Update::from(PromptResult::NotHandled),
        Update::Defer
    ));
}

#[test]
fn flash_info_sets_message() {
    let mut s = test_state();
    s.flash_info("hello");
    let flash = s.flash.as_ref().unwrap();
    assert_eq!(flash.text, "hello");
    assert!(matches!(flash.kind, FlashKind::Info));
}

#[test]
fn flash_error_sets_message() {
    let mut s = test_state();
    s.flash_error("oops");
    let flash = s.flash.as_ref().unwrap();
    assert_eq!(flash.text, "oops");
    assert!(matches!(flash.kind, FlashKind::Error));
}

// ── selection_paths ───────────────────────────────────────────

#[test]
fn selection_returns_cursor_item_when_no_picks() {
    let s = state_with_rows(&["a.txt", "b.txt"]);
    let paths = s.selection_paths();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].ends_with("a.txt"));
}

#[test]
fn selection_returns_picks_when_present() {
    let mut s = state_with_rows(&["a.txt", "b.txt", "c.txt"]);
    s.picks.toggle(Path::new("/tmp/test/b.txt"));
    s.picks.toggle(Path::new("/tmp/test/c.txt"));
    let paths = s.selection_paths();
    assert_eq!(paths.len(), 2);
}

#[test]
fn selection_empty_when_no_rows() {
    let s = test_state();
    assert!(s.selection_paths().is_empty());
}

// ── toggle_pick_cursor ────────────────────────────────────────

#[test]
fn toggle_pick_adds_and_removes() {
    let mut s = state_with_rows(&["a.txt", "b.txt"]);
    s.toggle_pick_cursor();
    assert!(s.picks.contains(Path::new("/tmp/test/a.txt")));
    s.toggle_pick_cursor();
    assert!(!s.picks.contains(Path::new("/tmp/test/a.txt")));
}

#[test]
fn toggle_pick_noop_in_inventory_view() {
    let mut s = state_with_rows(&["a.txt"]);
    s.view = View::Inventory;
    s.toggle_pick_cursor();
    assert!(s.picks.is_empty());
}

// ── toggle_all_picks ──────────────────────────────────────────

#[test]
fn toggle_all_picks_selects_then_clears() {
    let mut s = state_with_rows(&["a", "b", "c"]);
    s.toggle_all_picks();
    assert_eq!(s.picks.len(), 3);
    s.toggle_all_picks();
    assert!(s.picks.is_empty());
}

// ── take / drop / inventory ───────────────────────────────────

#[test]
fn take_cursor_item_to_inventory() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut s = state_with_real_files(tmp.path(), &["a.txt", "b.txt"]);
        s.take();
        assert_eq!(s.inventory.len(), 1);
        assert!(s.inventory.contains(&tmp.path().join("a.txt")));
    });
}

#[test]
fn take_picks_to_inventory() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut s = state_with_real_files(tmp.path(), &["a.txt", "b.txt"]);
        s.picks.toggle(&tmp.path().join("a.txt"));
        s.picks.toggle(&tmp.path().join("b.txt"));
        s.take();
        assert_eq!(s.inventory.len(), 2);
    });
}

#[test]
fn drop_removes_from_inventory() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut s = state_with_real_files(tmp.path(), &["a.txt"]);
        s.take(); // yank it first
        assert_eq!(s.inventory.len(), 1);
        // Switch to inventory view to drop
        s.toggle_inventory_view();
        s.drop_cursor();
        assert!(s.inventory.is_empty());
    });
}

// ── toggle_inventory_view ─────────────────────────────────────

#[test]
fn toggle_inventory_switches_view() {
    let mut s = test_state();
    assert_eq!(s.view, View::Dir);
    s.toggle_inventory_view();
    assert_eq!(s.view, View::Inventory);
    s.toggle_inventory_view();
    assert_eq!(s.view, View::Dir);
}

// ── focus_on_path ─────────────────────────────────────────────

#[test]
fn focus_on_path_sets_cursor() {
    let mut s = state_with_rows(&["a", "b", "c"]);
    s.focus_on_path(Path::new("/tmp/test/c"));
    assert_eq!(s.cursor.index, 2);
}

#[test]
fn focus_on_missing_path_is_noop() {
    let mut s = state_with_rows(&["a", "b"]);
    s.cursor.index = 1;
    s.focus_on_path(Path::new("/tmp/test/nope"));
    assert_eq!(s.cursor.index, 1); // unchanged
}

// ── dispatch_command ──────────────────────────────────────────

#[test]
fn climb_from_inventory_exits_to_dir_view_no_effect() {
    let mut s = test_state();
    s.view = View::Inventory;
    let fx = s.climb();
    assert!(fx.is_empty(), "inventory exit emits no effect");
    assert_eq!(s.view, View::Dir);
}

/// End-to-end-ish coverage of the git refresh pipeline. Edit a
/// tracked file → `refresh_listing` surfaces the `M` marker; commit
/// it → the next refresh clears it. Drives the real `refresh_listing`
/// → `git_file_statuses_cached` → `git status --porcelain` path on a
/// throwaway temp repo, so a regression in any of those (or in the
/// raw-cache / mtime-cache / row-rebuild plumbing) shows up here.
/// `git_worker_available` is false, so the sync spawn path runs — no
/// timing dependency, no real fs watcher.
#[test]
fn refresh_listing_picks_up_edit_and_clears_after_commit() {
    // Canonicalize so macOS `/var` → `/private/var` doesn't trip the
    // repo_root match inside the refresh path.
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap_or_else(|_| tmp.path().to_path_buf());

    let run_git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@x")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@x")
            // Suppress any user-level .gitconfig so the test is
            // hermetic on machines with unusual defaults.
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .expect("spawn git");
        assert!(status.success(), "git {args:?} failed");
    };
    run_git(&["init", "-q", "--initial-branch=main"]);
    std::fs::write(root.join("file.txt"), "v1\n").unwrap();
    run_git(&["add", "file.txt"]);
    run_git(&["commit", "-q", "-m", "v1"]);

    let mut s = test_state();
    s.listing.dir = root.clone();
    s.start_dir = root.clone();
    s.update_huge_tree(&root);
    s.git.info = s.compute_git_info_fast();

    // Clean repo: refresh sees no modifications.
    s.refresh_listing();
    assert!(
        s.git.files.is_empty(),
        "clean repo: no markers (got {:?})",
        s.git.files
    );

    // Working-tree edit → `M file.txt` should surface on next refresh.
    std::fs::write(root.join("file.txt"), "v2\n").unwrap();
    // Bypass the in-state 1 s invalidation throttle so this call
    // re-fetches instead of reusing the cached clean snapshot.
    s.git_cache.last_git_invalidation = None;
    s.refresh_listing();
    assert!(
        s.git.files.contains_key("file.txt"),
        "expected M marker for file.txt after edit; got {:?}",
        s.git.files
    );

    // Commit it → marker should clear (`.git/index` mtime moves, so
    // the mtime-cache invalidates on its own).
    run_git(&["add", "file.txt"]);
    run_git(&["commit", "-q", "-m", "v2"]);
    s.git_cache.last_git_invalidation = None;
    s.refresh_listing();
    assert!(
        !s.git.files.contains_key("file.txt"),
        "expected marker to clear after commit; got {:?}",
        s.git.files
    );
}

/// With a background worker wired (`git_worker_available = true`), a
/// cache-miss in the git-status path must NOT spawn `git status` inline.
/// It bumps the generation, enqueues exactly one request into the outbox
/// (`pending_git_requests`), stamps `last_git_request_at`, and returns an
/// empty map for this frame — the real markers arrive later via
/// `git_result_rx`. Locks the outbox contract `flush_git_requests`
/// (and the run loop) depend on after git_worker_tx moved to the Runtime.
#[test]
fn git_worker_available_enqueues_request_instead_of_spawning() {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap_or_else(|_| tmp.path().to_path_buf());
    let run_git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@x")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@x")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .expect("spawn git");
        assert!(status.success(), "git {args:?} failed");
    };
    run_git(&["init", "-q", "--initial-branch=main"]);
    std::fs::write(root.join("file.txt"), "v1\n").unwrap();
    run_git(&["add", "file.txt"]);
    run_git(&["commit", "-q", "-m", "v1"]);

    let mut s = test_state();
    s.listing.dir = root.clone();
    s.start_dir = root.clone();
    s.update_huge_tree(&root); // sets current_repo_root

    // Wire the "worker" and force a clean cache-miss baseline so the
    // asserted call takes the enqueue branch deterministically.
    s.git_cache.git_worker_available = true;
    s.git_cache.git_status_cache = None;
    s.git_cache.pending_git_requests.clear();
    let gen_before = s.git_cache.git_generation;

    let map = s.git_file_statuses_cached(&root);

    assert!(
        map.is_empty(),
        "worker path returns an empty map this frame (markers arrive async)"
    );
    assert_eq!(
        s.git_cache.pending_git_requests.len(),
        1,
        "exactly one request enqueued for the run loop to flush"
    );
    let req = &s.git_cache.pending_git_requests[0];
    assert_eq!(
        s.git_cache.current_repo_root.as_deref(),
        Some(req.repo_root.as_path()),
        "request carries the current repo root"
    );
    assert_eq!(
        s.git_cache.git_generation,
        gen_before.wrapping_add(1),
        "generation bumped once"
    );
    assert_eq!(
        req.generation, s.git_cache.git_generation,
        "enqueued request stamped with the bumped generation"
    );
    assert!(
        s.git_cache.last_git_request_at.is_some(),
        "request-sent timestamp stamped for the activity overlay"
    );
}

// ── count_files_in_dir_capped (R blast-radius walk, bounded) ──────
#[test]
fn count_files_capped_counts_under_cap_and_stops_at_cap() {
    use std::fs;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // 3 files at top + 2 in a subdir = 5 regular files (the dir itself
    // is not counted, matching what `remove_tree` unlinks).
    for i in 0..3 {
        fs::write(root.join(format!("f{i}")), "x").unwrap();
    }
    let sub = root.join("sub");
    fs::create_dir(&sub).unwrap();
    for i in 0..2 {
        fs::write(sub.join(format!("g{i}")), "x").unwrap();
    }

    // Under the cap → exact recursive count.
    assert_eq!(count_files_in_dir_capped(root, 100), 5);
    // Cap reached → walk stops; a return == cap means "at least cap"
    // (the prompt then shows `N+`).
    assert_eq!(count_files_in_dir_capped(root, 3), 3);
    // A zero cap walks nothing.
    assert_eq!(count_files_in_dir_capped(root, 0), 0);
}
