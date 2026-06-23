use super::*;

/// The context snapshot announces the running spyc's identity — its pid and
/// build (version + git SHA) — so an MCP client can detect a stale server and
/// name the process to restart.
#[test]
fn snapshot_context_announces_pid_and_version() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let app = App::test_app(tmp.path().to_path_buf());
        let ctx = app.snapshot_context();
        assert_eq!(ctx.pid, std::process::id(), "reports our own pid");
        assert_eq!(
            ctx.version,
            crate::VERSION,
            "reports the baked build string"
        );
        assert!(
            ctx.version.contains('(') && ctx.version.contains(')'),
            "version carries the git SHA in parens: {}",
            ctx.version
        );
    });
}

/// MCP telemetry: each `ToolCalled` bumps the cumulative per-tool tally the
/// `A` overlay renders, plus the aggregate `mcp:N/s` rate. A read tool the
/// socket thread serves still flows through here, so reads are counted too.
#[test]
fn mcp_tool_called_tallies_per_tool_counts() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        for name in ["search_content", "search_content", "navigate_to"] {
            app.execute_mcp_command(crate::mcp_cmd::McpCommand::ToolCalled {
                name: name.to_string(),
            });
        }
        let calls = &app.view.activity.mcp_tool_calls;
        assert_eq!(calls.get("search_content"), Some(&2), "per-tool count");
        assert_eq!(calls.get("navigate_to"), Some(&1));
        assert_eq!(
            app.view.activity.live.mcp_reqs, 3,
            "aggregate rate counts every tools/call"
        );
    });
}

/// MCP follows focus: with a second commander focused, the context snapshot
/// the agent reads reports `b`'s cwd, and a mutating MCP command (set_filter)
/// targets `b` — not the left/primary column. (PR F: MCP focus-aware.)
#[test]
fn mcp_context_and_mutations_follow_the_focused_column() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let left_dir = tmp.path().join("left");
        let right_dir = tmp.path().join("right");
        std::fs::create_dir(&left_dir).unwrap();
        std::fs::create_dir(&right_dir).unwrap();
        std::fs::write(right_dir.join("a.rs"), "").unwrap();
        let mut app = App::test_app(left_dir);
        app.open_second_commander_at(&right_dir); // b open + focused

        // Read side: the snapshot the agent reads reports b's cwd.
        let want = std::fs::canonicalize(&right_dir).unwrap();
        assert_eq!(
            app.snapshot_context().cwd,
            want,
            "get_spyc_context reports the focused (b) column's cwd"
        );

        // Mutate side: set_filter targets b, leaving a untouched.
        app.execute_mcp_command(crate::mcp_cmd::McpCommand::SetFilter {
            pattern: Some("*.rs".to_string()),
        });
        assert_eq!(
            app.state.right.as_ref().unwrap().temp_filter.as_deref(),
            Some("*.rs"),
            "set_filter applies to the focused (b) column"
        );
        assert!(
            app.state.left.temp_filter.is_none(),
            "the left column's filter is untouched"
        );
    });
}

/// MCP `create_worktree` makes a git worktree off the FOCUSED column's repo
/// and replies `{branch, path}` — the entry point a skill uses to spin up a
/// worktree to work in `b`.
#[test]
fn mcp_create_worktree_makes_a_worktree_off_the_focused_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let run_git = |dir: &std::path::Path, args: &[&str]| {
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
        assert!(ok, "git {args:?} failed");
    };
    crate::state::with_state_root(tmp.path(), || {
        let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(repo.join("f.txt"), "v1\n").unwrap();
        run_git(&repo, &["add", "f.txt"]);
        run_git(&repo, &["commit", "-q", "-m", "v1"]);

        let mut app = App::test_app(repo.clone());
        app.state.update_repo_root(state::Side::Left, &repo);

        let resp = app.execute_mcp_command(crate::mcp_cmd::McpCommand::CreateWorktree {
            branch: "mcp-wt".to_string(),
        });
        match resp {
            crate::mcp_cmd::McpResponse::Ok { message } => {
                let v: serde_json::Value = serde_json::from_str(&message).unwrap();
                assert_eq!(v["branch"], "mcp-wt");
                let path = std::path::PathBuf::from(v["path"].as_str().unwrap());
                assert!(path.is_dir(), "worktree dir created: {path:?}");
                assert!(path.join("f.txt").exists(), "branch tree checked out");
            }
            crate::mcp_cmd::McpResponse::Error { message } => {
                panic!("create_worktree errored: {message}")
            }
        }
    });
}

/// Empty branch is rejected before touching git.
#[test]
fn mcp_create_worktree_rejects_empty_branch() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        let resp = app.execute_mcp_command(crate::mcp_cmd::McpCommand::CreateWorktree {
            branch: "   ".to_string(),
        });
        assert!(
            matches!(resp, crate::mcp_cmd::McpResponse::Error { .. }),
            "blank branch is an error"
        );
    });
}

/// MCP `remove_worktree` tears down a clean worktree — and when a column is open
/// inside it, removal still proceeds and the column is snapped back to
/// PROJECT_HOME (rather than refused and left stranded on a deleted dir).
#[test]
fn mcp_remove_worktree_tears_down_and_resets_occupied_column() {
    use crate::mcp_cmd::{McpCommand, McpResponse};
    let tmp = tempfile::tempdir().unwrap();
    let run_git = |dir: &std::path::Path, args: &[&str]| {
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
        assert!(ok, "git {args:?} failed");
    };
    crate::state::with_state_root(tmp.path(), || {
        let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(repo.join("f.txt"), "v1\n").unwrap();
        run_git(&repo, &["add", "f.txt"]);
        run_git(&repo, &["commit", "-q", "-m", "v1"]);

        let mut app = App::test_app(repo.clone());
        app.state.update_repo_root(state::Side::Left, &repo);

        // Create, then capture its path.
        let created = app.execute_mcp_command(McpCommand::CreateWorktree {
            branch: "teardown-wt".to_string(),
        });
        let path = match created {
            McpResponse::Ok { message } => {
                let v: serde_json::Value = serde_json::from_str(&message).unwrap();
                std::path::PathBuf::from(v["path"].as_str().unwrap().to_string())
            }
            McpResponse::Error { message } => panic!("create failed: {message}"),
        };
        assert!(path.is_dir(), "worktree created");

        // Open `b` inside it, then refocus `a` (so the post-removal reset of the
        // background column `b` doesn't move the process cwd during the test).
        app.state.project_home = Some(repo.clone());
        app.open_second_commander_at(&path);
        app.state.vsplit.as_mut().unwrap().focus = state::Side::Left;

        // Removal PROCEEDS even with `b` inside it (the occupied-column refuse is
        // gone) and snaps `b` back to PROJECT_HOME instead of stranding it.
        let removed = app.execute_mcp_command(McpCommand::RemoveWorktree {
            path: path.display().to_string(),
        });
        assert!(
            matches!(removed, McpResponse::Ok { .. }),
            "removes a worktree even with a column inside it: {removed:?}"
        );
        assert!(!path.exists(), "worktree dir removed");
        assert_eq!(
            app.state.right.as_ref().unwrap().listing.dir,
            repo,
            "column b reset to PROJECT_HOME after its worktree was removed"
        );
    });
}

/// MCP `open_worktree` opens column `b` at the given dir (the "work in it in b"
/// step), so `cur()` then resolves to `b`.
#[test]
fn mcp_open_worktree_opens_column_b() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let a = std::fs::canonicalize(tmp.path()).unwrap().join("a");
        let wt = std::fs::canonicalize(tmp.path()).unwrap().join("wt");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&wt).unwrap();
        let mut app = App::test_app(a);
        assert!(app.state.right.is_none(), "single column to start");

        let resp = app.execute_mcp_command(crate::mcp_cmd::McpCommand::OpenWorktree {
            path: wt.display().to_string(),
        });
        assert!(
            matches!(resp, crate::mcp_cmd::McpResponse::Ok { .. }),
            "open ok: {resp:?}"
        );
        let b = app.state.right.as_ref().expect("column b opened");
        assert_eq!(b.listing.dir, wt, "b opened at the worktree");
        assert_eq!(app.focused_side(), state::Side::Right, "focus moved to b");
    });
}

/// `resolve_worktree_arg` (shared by remove/clean_worktree) — two arms the
/// existing remove test never hits (it feeds an absolute created-worktree path):
/// the empty-path error, and the relative-path resolution joined onto the
/// FOCUSED column's cwd before the occupied check.
#[test]
fn worktree_arg_empty_errors_and_relative_resolves_against_cwd() {
    use crate::mcp_cmd::{McpCommand, McpResponse};
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let sub = base.join("sub");
        std::fs::create_dir(&sub).unwrap();
        let mut app = App::test_app(base.clone());
        app.open_second_commander_at(&sub); // b inside base/sub
        // Resolve relative paths against `a` (base) → "sub" means base/sub.
        app.state.vsplit.as_mut().unwrap().focus = state::Side::Left;
        assert_eq!(app.focused_side(), state::Side::Left);

        // Empty path → missing-parameter error (before any path logic).
        let empty = app.execute_mcp_command(McpCommand::RemoveWorktree {
            path: String::new(),
        });
        assert!(
            matches!(&empty, McpResponse::Error { message } if message.contains("missing required parameter")),
            "empty path → missing-parameter error"
        );

        // Relative "sub" joins onto the focused column's cwd (base) → base/sub.
        // (The occupied-column refuse was removed — removal now resets the
        // column instead — so assert the path resolution directly.)
        let resolved = app
            .resolve_worktree_arg("sub")
            .expect("relative path resolves");
        assert_eq!(
            resolved,
            base.join("sub"),
            "relative path resolved against the focused column's cwd"
        );
    });
}

/// `W d` (worktree delete) refuses when the OTHER column is open inside the
/// worktree being removed — mirrors the MCP `remove_worktree` occupied guard,
/// so the in-app key is no less safe than the tool. Without it, deleting `a`'s
/// worktree while `b` sits in a subdir strands `b` on a removed directory.
#[test]
fn worktree_delete_refuses_when_other_column_inside() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let wt = root.join("wt");
        let sub = wt.join("sub");
        std::fs::create_dir_all(&sub).unwrap();

        let mut app = App::test_app(wt); // a in wt (the delete target)
        app.open_second_commander_at(&sub); // b in wt/sub
        // Focus a (the worktree we'll delete); mark it a git repo so the delete
        // path proceeds past the "not in a git repository" check.
        app.state.vsplit.as_mut().unwrap().focus = state::Side::Left;
        app.state.left.git.info = Some("main".to_string());
        assert_eq!(app.focused_side(), state::Side::Left);

        let _ = app.apply(&Action::WorktreeDelete);
        assert!(
            !matches!(app.state.mode, Mode::Prompting(_)),
            "refused: no confirm prompt shown"
        );
        assert!(
            app.state
                .flash
                .as_ref()
                .is_some_and(|f| f.text.contains("navigate it away")),
            "flashed the occupied-column refusal"
        );

        // Control: move b out of the worktree → the delete now prompts.
        app.state.right.as_mut().unwrap().listing.dir = root;
        let _ = app.apply(&Action::WorktreeDelete);
        assert!(
            matches!(
                &app.state.mode,
                Mode::Prompting(p) if matches!(p.kind, PromptKind::WorktreeDeleteConfirm)
            ),
            "b no longer inside → delete confirm prompt shown"
        );
    });
}

/// The off-thread MCP worktree path: plan_worktree_job (sync validate) →
/// spawn_worktree_job (worker does the gix add) → apply_worktree_outcomes (main
/// loop refresh+context) → the reply lands on the client's one-shot channel.
/// Guards that off-threading the heavy IO still answers the client correctly.
#[test]
fn mcp_create_worktree_runs_off_thread_and_replies() {
    use crate::mcp_cmd::{McpCommand, McpResponse};
    let tmp = tempfile::tempdir().unwrap();
    let run_git = |dir: &std::path::Path, args: &[&str]| {
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
        assert!(ok, "git {args:?} failed");
    };
    crate::state::with_state_root(tmp.path(), || {
        let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(repo.join("f.txt"), "v1\n").unwrap();
        run_git(&repo, &["add", "f.txt"]);
        run_git(&repo, &["commit", "-q", "-m", "v1"]);

        let mut app = App::test_app(repo.clone());
        app.state.update_repo_root(state::Side::Left, &repo);

        // Validate on the loop, then hand the gix add to a worker.
        let job = app
            .plan_worktree_job(&McpCommand::CreateWorktree {
                branch: "wt-async".into(),
            })
            .expect("a worktree command")
            .expect("validation passes");
        let (tx, rx) = std::sync::mpsc::channel();
        app.spawn_worktree_job(job, tx);

        // Worker runs off-thread (no wake tx in tests); poll the landing slot.
        for _ in 0..500 {
            if !app.runtime.worktree_results.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            app.apply_worktree_outcomes(),
            "landed outcome applied (refresh+context) + redraw requested"
        );
        let resp = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the worker replied on the client's channel");
        match resp {
            McpResponse::Ok { message } => {
                assert!(
                    message.contains("wt-async"),
                    "reply carries the branch: {message}"
                );
                assert!(
                    message.contains("path"),
                    "reply carries the new worktree path"
                );
            }
            McpResponse::Error { message } => panic!("create failed: {message}"),
        }
    });
}

/// A non-directory path is an error and leaves the layout unchanged.
#[test]
fn mcp_open_worktree_rejects_non_dir() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        let resp = app.execute_mcp_command(crate::mcp_cmd::McpCommand::OpenWorktree {
            path: tmp.path().join("does-not-exist").display().to_string(),
        });
        assert!(matches!(resp, crate::mcp_cmd::McpResponse::Error { .. }));
        assert!(app.state.right.is_none(), "b not opened on error");
    });
}
