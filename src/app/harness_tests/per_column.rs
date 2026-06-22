use super::*;

/// A `!` capture runs in the FOCUSED column's dir — `!touch foo` with `b`
/// focused lands in `b`, not `a`/PROJECT_HOME. Regression: `start_capture`
/// hardcoded `state.left.listing.dir`.
#[test]
fn capture_runs_in_the_focused_columns_dir() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let a = std::fs::canonicalize(tmp.path()).unwrap().join("a");
        let b = std::fs::canonicalize(tmp.path()).unwrap().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let mut app = App::test_app(a.clone());
        app.open_second_commander_at(&b); // b focused
        assert_eq!(app.focused_side(), state::Side::Right);

        app.start_capture("touch marker.txt", "touch", "!touch marker.txt");

        // `touch` is near-instant; poll briefly for the spawned subprocess.
        let in_b = b.join("marker.txt");
        for _ in 0..200 {
            if in_b.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(in_b.exists(), "! ran in b (focused) → b/marker.txt");
        assert!(
            !a.join("marker.txt").exists(),
            "did NOT run in a / PROJECT_HOME"
        );
    });
}

/// `:;<cmd>` (typed foreground shell) runs in the FOCUSED column's dir, like
/// the `;` prompt and `!` capture — `:;touch foo` with `b` focused lands in
/// `b`. Regression: the typed `:;` path hardcoded `state.left.listing.dir`
/// while the other shell-spawn paths were converted to follow `cur()`.
#[test]
fn typed_shell_command_runs_in_the_focused_columns_dir() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let a = std::fs::canonicalize(tmp.path()).unwrap().join("a");
        let b = std::fs::canonicalize(tmp.path()).unwrap().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let mut app = App::test_app(a.clone());
        app.open_second_commander_at(&b); // b focused
        assert_eq!(app.focused_side(), state::Side::Right);

        app.dispatch_command(";touch marker.txt");

        // `touch` is near-instant; poll briefly for the spawned subprocess.
        let in_b = b.join("marker.txt");
        for _ in 0..200 {
            if in_b.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(in_b.exists(), ":; ran in b (focused) → b/marker.txt");
        assert!(
            !a.join("marker.txt").exists(),
            "did NOT run in a / PROJECT_HOME"
        );
    });
}

/// The COMPOSED gitignore-drop (`drop_gitignored_fs_events`): across a column in
/// a repo, FSEvents under a gitignored build subtree (`target/`) are dropped so
/// cargo-build churn can't spam refresh — but three exemptions SURVIVE the drop:
/// a cwd-level gitignored file (a visible row changed), a normal source edit,
/// and the open vertical-split preview file even when it lives under the
/// gitignored subtree (else its save never reaches the live reload). The pure
/// pieces (`is_cwd_level`, `excludes::with_checker`) are tested in isolation;
/// this pins their composition in the retain_mut drain.
#[test]
fn gitignored_fs_event_drop_keeps_cwd_level_source_and_preview() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let repo = std::fs::canonicalize(tmp.path()).unwrap();
        gix::init(&repo).unwrap();
        std::fs::write(repo.join(".gitignore"), "target/\n*.log\n").unwrap();

        let mut app = App::test_app(repo.clone());
        app.state.update_repo_root(state::Side::Left, &repo);
        // The open preview points at a file under the gitignored `target/`.
        let preview = repo.join("target/preview.md");
        let mut pv = crate::ui::pager::PagerView::new_plain("preview", vec!["x".to_string()]);
        pv.source_path = Some(preview.clone());
        app.view.right_pager = Some(pv);

        let ev = |p: &std::path::Path| {
            notify::Event::new(notify::EventKind::Any).add_path(p.to_path_buf())
        };
        let log = repo.join("debug.log"); // cwd-level + gitignored → keep
        let src = repo.join("src/main.rs"); // ordinary edit → keep
        let obj = repo.join("target/debug/foo.o"); // gitignored subtree churn → drop
        let mut pending = vec![ev(&log), ev(&src), ev(&obj), ev(&preview)];

        app.drop_gitignored_fs_events(&mut pending);

        let survived: std::collections::HashSet<std::path::PathBuf> = pending
            .iter()
            .flat_map(|e| e.paths.iter().cloned())
            .collect();
        assert!(survived.contains(&log), "cwd-level gitignored file kept");
        assert!(survived.contains(&src), "ordinary source edit kept");
        assert!(
            survived.contains(&preview),
            "open preview file kept even under target/"
        );
        assert!(
            !survived.contains(&obj),
            "gitignored build-subtree churn dropped"
        );
    });
}

/// Dual fs-watch: with a second commander open, the fs-event path predicates
/// recognize column `b`'s tree + gitdir too — so `b`'s working-tree edits and
/// index/HEAD changes drive a refresh, not just the ≤1 s poll PR E left.
#[test]
fn fs_predicates_recognize_the_second_column() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let a = std::fs::canonicalize(tmp.path()).unwrap().join("a");
        let b = std::fs::canonicalize(tmp.path()).unwrap().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let mut app = App::test_app(a.clone());
        app.state.left.git_cache.current_gitdir = Some(a.join(".git"));
        app.open_second_commander_at(&b);
        app.state.right.as_mut().unwrap().git_cache.current_gitdir = Some(b.join(".git"));

        // b's tree + index/HEAD are recognized…
        assert!(app.is_listing_path(&b.join("src/main.rs")), "b tree file");
        assert!(app.is_gitdir_status_path(&b.join(".git/index")), "b index");
        assert!(app.is_gitdir_status_path(&b.join(".git/HEAD")), "b HEAD");
        // …and a's still are (both columns active)…
        assert!(app.is_listing_path(&a.join("lib.rs")), "a tree file");
        assert!(app.is_gitdir_status_path(&a.join(".git/index")), "a index");
        // …while b's .git housekeeping is still rejected.
        assert!(
            !app.is_gitdir_status_path(&b.join(".git/objects/ab/cd")),
            "b objects churn rejected"
        );
    });
}

/// `/` incremental search moves the FOCUSED column's cursor — searching in `b`
/// must not scroll `a`. Regression: the match application hardcoded `left`.
#[test]
fn slash_search_targets_the_focused_column() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let left_dir = tmp.path().join("left");
        let right_dir = tmp.path().join("right");
        std::fs::create_dir(&left_dir).unwrap();
        std::fs::create_dir(&right_dir).unwrap();
        let mut app = App::test_app(left_dir);
        app.open_second_commander_at(&right_dir); // b focused

        // Deterministic rows for b; a starts at cursor 0.
        let r = app.state.right.as_mut().unwrap();
        r.rows = vec![
            RowData {
                path: right_dir.join("alpha"),
                display: "alpha".to_string(),
                kind: EntryKind::File,
            },
            RowData {
                path: right_dir.join("zebra"),
                display: "zebra".to_string(),
                kind: EntryKind::File,
            },
        ];
        r.cursor.index = 0;

        app.apply(&Action::SearchPrompt).unwrap(); // saves b's cursor (0)
        for c in "zeb".chars() {
            app.handle_key(key(c)).unwrap();
        }
        assert_eq!(
            app.state.right.as_ref().unwrap().cursor.index,
            1,
            "/ moved b's cursor to the match"
        );
        assert_eq!(
            app.state.left.cursor.index, 0,
            "a's cursor is untouched by a search in b"
        );
    });
}

/// Dual git (PR E): each column resolves + shows its OWN repo's markers and
/// branch — `b` in a different repo doesn't inherit `a`'s state, and the two
/// don't collide on a single generation. (Sync walk: test harness has no
/// worker, so `git_file_statuses_cached` runs inline.)
#[test]
fn dual_git_each_column_shows_its_own_repo() {
    use std::path::Path;
    let tmp = tempfile::tempdir().unwrap();
    let run_git = |dir: &Path, args: &[&str]| {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@x")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@x")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .expect("spawn git")
            .success();
        assert!(ok, "git {args:?} failed in {dir:?}");
    };
    let mk_repo = |name: &str| {
        let d = std::fs::canonicalize(tmp.path()).unwrap().join(name);
        std::fs::create_dir(&d).unwrap();
        run_git(&d, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(d.join("file.txt"), "v1\n").unwrap();
        run_git(&d, &["add", "file.txt"]);
        run_git(&d, &["commit", "-q", "-m", "v1"]);
        d
    };
    crate::state::with_state_root(tmp.path(), || {
        let repo_a = mk_repo("a"); // stays clean
        let repo_b = mk_repo("b");
        std::fs::write(repo_b.join("file.txt"), "v2\n").unwrap(); // b is dirty

        let mut app = App::test_app(repo_a.clone());
        app.state.update_repo_root(state::Side::Left, &repo_a);
        app.state.refresh_git_state(); // a: clean
        app.open_second_commander_at(&repo_b); // resolves + refreshes b (dirty)

        assert!(
            app.state.left.git.files.is_empty(),
            "a (clean) has no markers: {:?}",
            app.state.left.git.files
        );
        assert_eq!(
            app.state.left.git.info.as_deref(),
            Some("main"),
            "a clean branch"
        );
        let b = app.state.right.as_ref().expect("b open");
        assert!(
            b.git.files.contains_key("file.txt"),
            "b shows ITS own M marker, not a's: {:?}",
            b.git.files
        );
        assert!(
            b.git.info.as_deref().is_some_and(|s| s.ends_with('*')),
            "b's branch shows dirty: {:?}",
            b.git.info
        );
    });
}

/// `O` (new file) creates the file in the FOCUSED column's dir — so a worktree
/// in `b` gets the file, not `a`'s dir. Regression: the create path joined
/// `state.left.listing.dir`.
#[test]
fn new_file_creates_in_the_focused_column() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let mut app = App::test_app(a.clone());
        app.open_second_commander_at(&b); // b focused
        assert_eq!(app.focused_side(), state::Side::Right);

        // O → NewFile prompt, type a name, submit.
        let mut p = Prompt::simple(PromptKind::NewFile, "new file: ");
        p.buffer = "made_in_b.txt".to_string();
        app.state.mode = Mode::Prompting(p);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .unwrap();

        assert!(
            std::fs::canonicalize(&b)
                .unwrap()
                .join("made_in_b.txt")
                .exists(),
            "O creates the file in b (the focused column)"
        );
        assert!(
            !a.join("made_in_b.txt").exists(),
            "the file is NOT created in a"
        );
    });
}

/// `g w` (JumpWorktreeRoot) targets the FOCUSED column's own repo/worktree
/// root — `b` jumps to b's root, not a's. (PROJECT_HOME, jumped by `g h`,
/// stays the overall anchor.)
#[test]
fn gw_targets_the_focused_columns_worktree_root() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir_a = tmp.path().join("a");
        let dir_b = tmp.path().join("b");
        std::fs::create_dir(&dir_a).unwrap();
        std::fs::create_dir(&dir_b).unwrap();
        let mut app = App::test_app(dir_a);
        app.open_second_commander_at(&dir_b); // b focused

        let root_a = tmp.path().join("repoA");
        let root_b = tmp.path().join("repoB");
        app.state.left.git_cache.current_repo_root = Some(root_a);
        app.state
            .right
            .as_mut()
            .unwrap()
            .git_cache
            .current_repo_root = Some(root_b.clone());

        let fx = app.apply(&Action::JumpWorktreeRoot).unwrap();
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::ChangeDir { path, .. } if *path == root_b)),
            "g w targets b's (focused) worktree root, not a's: {fx:?}"
        );
    });
}

/// grep `F` / find walk the FOCUSED column's worktree root via `tool_root`:
/// with `b` focused in a separate repo, the F-finder scopes to b's root — not
/// a's, not PROJECT_HOME. (MCP search shares the same root, exercised in
/// `mcp::tests::search_root_overrides_project_home`.)
#[test]
fn find_picker_walks_the_focused_columns_worktree_root() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir_a = tmp.path().join("a");
        let dir_b = tmp.path().join("b");
        std::fs::create_dir(&dir_a).unwrap();
        std::fs::create_dir(&dir_b).unwrap();
        let mut app = App::test_app(dir_a);
        app.open_second_commander_at(&dir_b); // b focused

        let root_a = tmp.path().join("repoA");
        let root_b = tmp.path().join("repoB");
        std::fs::create_dir(&root_a).unwrap();
        std::fs::create_dir(&root_b).unwrap();
        app.state.left.git_cache.current_repo_root = Some(root_a);
        app.state
            .right
            .as_mut()
            .unwrap()
            .git_cache
            .current_repo_root = Some(root_b.clone());

        app.open_find_picker();
        let root = app
            .runtime
            .find_picker
            .as_ref()
            .expect("finder open")
            .root
            .clone();
        assert_eq!(root, root_b, "F walks b's (focused) worktree root");
    });
}

/// Harpoon is per-column, pinned to each column's worktree: an entry appended
/// in `a` does NOT bleed into `b`'s list when `b` is a different worktree, and
/// vice versa. Harpoon stores ABSOLUTE paths, so a shared list would jump `b`
/// into `a`'s copy of a file — the wrong worktree/branch. Each column keeps
/// its own bookmarks keyed by `harpoon_root`.
#[test]
fn harpoon_is_per_column_pinned_to_each_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir_a = std::fs::canonicalize(tmp.path()).unwrap().join("a");
        let dir_b = std::fs::canonicalize(tmp.path()).unwrap().join("b");
        std::fs::create_dir(&dir_a).unwrap();
        std::fs::create_dir(&dir_b).unwrap();
        let fa = dir_a.join("fa.txt");
        let fb = dir_b.join("fb.txt");
        std::fs::write(&fa, "").unwrap();
        std::fs::write(&fb, "").unwrap();

        let mut app = App::test_app(dir_a.clone());
        // `a`'s worktree root enables + keys its harpoon.
        app.state.left.git_cache.current_repo_root = Some(dir_a);
        app.reconcile_harpoon();
        // Cursor on fa.txt → Ha appends it to a's list.
        app.state.left.rows = vec![RowData {
            path: fa.clone(),
            display: "fa.txt".into(),
            kind: EntryKind::File,
        }];
        app.state.left.cursor.index = 0;
        app.harpoon_append();

        // Open `b` in its own worktree, harpoon fb.txt there.
        app.open_second_commander_at(&dir_b);
        app.state
            .right
            .as_mut()
            .unwrap()
            .git_cache
            .current_repo_root = Some(dir_b.clone());
        app.reconcile_harpoon();
        app.state.right.as_mut().unwrap().rows = vec![RowData {
            path: fb.clone(),
            display: "fb.txt".into(),
            kind: EntryKind::File,
        }];
        app.state.right.as_mut().unwrap().cursor.index = 0;
        app.harpoon_append();

        let a = app.state.left.harpoon.as_ref().expect("a harpoon loaded");
        let b = app
            .state
            .right
            .as_ref()
            .unwrap()
            .harpoon
            .as_ref()
            .expect("b harpoon loaded");
        assert!(
            a.contains(&fa) && !a.contains(&fb),
            "a's list = a's file only"
        );
        assert!(
            b.contains(&fb) && !b.contains(&fa),
            "b's list = b's file only — no cross-worktree bleed"
        );
        assert_ne!(
            a.project, b.project,
            "each column's harpoon keys to its OWN worktree root"
        );
    });
}
