//! #99 step 3: an MCP connection answers for, and acts on, its own project.
//!
//! Two projects in one `App`: `a` is active, the one the user sees, and `b` is
//! another, whose agent must neither read `a` nor move it. Most tests speak the
//! wire protocol to the server's own connection handler, and its commands land
//! in the loop's own drain, so they hold whatever routes a call to its project;
//! the rest hand the drain's router a request directly.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::*;
use crate::mcp::test_support::Conn;
use crate::mcp_cmd::{McpCommand, McpResponse};
use crate::pane::tabs::AgentActivity;

/// Two projects in sibling directories, each with a real listing, a written
/// context file and one `cat` tab. `a` is active.
struct Projects {
    app: App,
    b: state::ProjectId,
    base: PathBuf,
    a_home: PathBuf,
    b_home: PathBuf,
    a_pane: String,
    b_pane: String,
    /// How many `Bind`s the connections [`Projects::wire_from`] served asked
    /// the loop for.
    binds: usize,
}

fn with_projects(test: impl FnOnce(&mut Projects)) {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let (state_dir, a_home, b_home) = (base.join("state"), base.join("a"), base.join("b"));
    for d in [&state_dir, &a_home, &b_home] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(a_home.join("in_a.rs"), "").unwrap();
    std::fs::write(b_home.join("in_b.rs"), "").unwrap();
    crate::state::with_state_root(&state_dir, || {
        let mut app = App::test_app(a_home.clone());
        let a = app.state.active;
        app.ready_test_project(a, &a_home);
        let b = app.add_test_project(&b_home);
        let a_pane = app.open_tab_in_project(a, &a_home);
        let b_pane = app.open_tab_in_project(b, &b_home);
        test(&mut Projects {
            app,
            b,
            base,
            a_home,
            b_home,
            a_pane,
            b_pane,
            binds: 0,
        });
    });
}

impl Projects {
    /// Drive `client` over one real connection, serving its commands as the
    /// loop does: through `drain_mcp_pending`, with worktree jobs landed and
    /// scope waits settled between requests. The server's own context file is
    /// `a`'s, the one it starts with.
    fn wire<T: Send>(&mut self, client: impl FnOnce(&mut Conn) -> T + Send) -> T {
        let default = self.app.view.project().context_path.clone();
        self.wire_from(default, client)
    }

    /// [`Self::wire`], with the server started on the context file `default`.
    fn wire_from<T: Send>(
        &mut self,
        default: PathBuf,
        client: impl FnOnce(&mut Conn) -> T + Send,
    ) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut conn = Conn::open(default, tx);
        let mut ctx = RunCtx::for_test();
        let app = &mut self.app;
        let binds = &mut self.binds;
        std::thread::scope(|s| {
            let client = s.spawn(move || client(&mut conn));
            while !client.is_finished() {
                if let Ok(req) = rx.recv_timeout(Duration::from_millis(5)) {
                    *binds += usize::from(matches!(req.command, McpCommand::Bind { .. }));
                    ctx.mcp_pending.push(req);
                    app.drain_mcp_pending(&mut ctx);
                }
                app.apply_worktree_outcomes();
                app.settle_scope_waiters(Instant::now(), &mut ctx);
            }
            // A command nobody answers (`spyc/statusline`) is sent before its
            // reply, so it can still be queued once the client has finished,
            // when the client ran between a timed-out wait and the check.
            while let Ok(req) = rx.try_recv() {
                *binds += usize::from(matches!(req.command, McpCommand::Bind { .. }));
                ctx.mcp_pending.push(req);
                app.drain_mcp_pending(&mut ctx);
            }
            client.join().expect("the client finishes")
        })
    }

    /// One call from the agent in `pane`, over its own connection.
    fn call_from(&mut self, pane: &str, tool: &'static str, args: Value) -> Value {
        let pane = pane.to_string();
        self.wire(move |c| {
            c.initialize(Some(&pane));
            c.tool(tool, args)
        })
    }

    fn in_b<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> R {
        let b = self.b;
        self.app.as_project(b, f)
    }

    /// `b`'s focused column's directory.
    fn b_dir(&mut self) -> PathBuf {
        self.in_b(|app| app.state.cur().listing.dir.clone())
    }

    /// What `b`'s context file says.
    fn b_context(&mut self) -> Value {
        let path = self.in_b(|app| app.view.project().context_path.clone());
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// The last status the tab `pane` in the active project was told.
    fn reported(app: &App, pane: &str) -> Option<AgentActivity> {
        let tabs = app.runtime.project().pane_tabs.as_ref()?;
        let tab = tabs.tabs().iter().find(|t| t.info.id == pane)?;
        tab.info.last_reported.as_ref().map(|r| r.status)
    }
}

/// Puts the process cwd back on drop, for a test whose failure would move it.
struct CwdGuard;

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(env!("CARGO_MANIFEST_DIR"));
    }
}

fn init_repo(dir: &Path) {
    let run_git = crate::git::test_support::run_git;
    run_git(dir, &["init", "-q", "--initial-branch=main"]);
    run_git(dir, &["add", "in_b.rs"]);
    run_git(dir, &["commit", "-q", "-m", "v1"]);
}

/// The harness itself, on the project the user is in: what every red test
/// below relies on to fail for its own reason and not the harness's.
#[test]
fn an_agent_in_the_active_project_is_served_over_the_wire() {
    with_projects(|p| {
        let a_pane = p.a_pane.clone();
        let ctx = p.wire(move |c| {
            c.initialize(Some(&a_pane));
            c.tool("pick_files", json!({"patterns": ["in_a.rs"]}));
            c.tool("get_spyc_context", json!({}))
        });
        assert_eq!(ctx["pane"]["id"], p.a_pane.as_str(), "{ctx}");
        assert_eq!(ctx["cwd"], p.a_home.to_str().unwrap(), "{ctx}");
        assert!(
            p.app.state.cur().picks.contains(&p.a_home.join("in_a.rs")),
            "pick_files picked in a"
        );
    });
}

#[test]
fn an_agent_in_another_project_binds_to_its_pane() {
    with_projects(|p| {
        let b_pane = p.b_pane.clone();
        let ctx = p.call_from(&b_pane, "get_spyc_context", json!({}));
        assert_eq!(ctx["pane"]["id"], p.b_pane.as_str(), "{ctx}");
    });
}

#[test]
fn an_agent_in_another_project_reads_its_own_project() {
    with_projects(|p| {
        let b_pane = p.b_pane.clone();
        let (ctx, found) = p.wire(move |c| {
            c.initialize(Some(&b_pane));
            (
                c.tool("get_spyc_context", json!({})),
                c.tool("search_paths", json!({"query": "in_b"})),
            )
        });
        let b_home = p.b_home.to_str().unwrap();
        assert_eq!(ctx["cwd"], b_home, "{ctx}");
        assert_eq!(ctx["root"], b_home, "{ctx}");
        assert!(found.to_string().contains("in_b.rs"), "{found}");
    });
}

/// An agent can tell whether the user is in its project, and which one it is.
#[test]
fn get_spyc_context_names_the_callers_project() {
    with_projects(|p| {
        let (a_pane, b_pane) = (p.a_pane.clone(), p.b_pane.clone());
        let b = p.call_from(&b_pane, "get_spyc_context", json!({}));
        let a = p.call_from(&a_pane, "get_spyc_context", json!({}));
        assert_eq!(b["project"]["home"], p.b_home.to_str().unwrap(), "{b}");
        assert_eq!(b["project"]["label"], "b", "{b}");
        assert_eq!(b["project"]["active"], false, "{b}");
        assert_eq!(a["project"]["home"], p.a_home.to_str().unwrap(), "{a}");
        assert_eq!(a["project"]["active"], true, "{a}");
        assert!(b["project"]["id"].is_string(), "{b}");
        assert_ne!(b["project"]["id"], a["project"]["id"]);
    });
}

/// The user's view stays where they left it, and so does the process cwd:
/// both belong to the project they're looking at. Run it alone: until it
/// passes, the agent's `navigate_to` moves the user's column and the process.
#[test]
fn navigate_to_moves_the_callers_project_not_the_users_view() {
    with_projects(|p| {
        let _cwd = CwdGuard;
        let target = p.b_home.join("sub");
        std::fs::create_dir(&target).unwrap();
        let b_pane = p.b_pane.clone();
        p.call_from(
            &b_pane,
            "navigate_to",
            json!({"path": target.to_str().unwrap()}),
        );
        assert_eq!(p.app.state.cur().listing.dir, p.a_home, "the user's view");
        assert_eq!(p.b_dir(), target, "the caller's own project moved");
        assert_eq!(p.b_context()["cwd"], target.to_str().unwrap());
        assert_ne!(
            std::env::current_dir().ok(),
            Some(target),
            "the process cwd follows the user's view"
        );
    });
}

#[test]
fn set_filter_and_pick_files_act_on_the_callers_project() {
    with_projects(|p| {
        let b_pane = p.b_pane.clone();
        p.wire(move |c| {
            c.initialize(Some(&b_pane));
            c.tool("set_filter", json!({"pattern": "*.rs"}));
            c.tool("pick_files", json!({"patterns": ["in_b.rs"]}));
        });
        assert_eq!(p.app.state.cur().temp_filter, None, "the user's filter");
        assert!(p.app.state.cur().picks.is_empty(), "the user's picks");
        let b_home = p.b_home.clone();
        let (filter, picked) = p.in_b(|app| {
            let col = app.state.cur();
            (
                col.temp_filter.clone(),
                col.picks.contains(&b_home.join("in_b.rs")),
            )
        });
        assert_eq!(filter.as_deref(), Some("*.rs"));
        assert!(picked, "pick_files picked in b");
    });
}

#[test]
fn open_worktree_opens_b_in_the_callers_project() {
    with_projects(|p| {
        let tree = p.b_home.join("tree");
        std::fs::create_dir(&tree).unwrap();
        let b_pane = p.b_pane.clone();
        p.call_from(
            &b_pane,
            "open_worktree",
            json!({"path": tree.to_str().unwrap()}),
        );
        assert!(
            p.app.state.active_project().right.is_none(),
            "the user's project opens no b"
        );
        let opened = p.in_b(|app| {
            let right = app.state.active_project().right.as_ref();
            right.map(|c| c.listing.dir.clone())
        });
        assert_eq!(opened, Some(tree));
    });
}

/// A session codex's shared daemon started has no pane. Its proxy sends its
/// cwd, and the connection binds to the project whose root contains it, else
/// to the active project.
#[test]
fn an_agent_with_no_pane_binds_by_its_cwd() {
    with_projects(|p| {
        let deep = p.b_home.join("deep");
        let outside = p.base.join("elsewhere");
        for d in [&deep, &outside] {
            std::fs::create_dir(d).unwrap();
        }
        let context_from = |p: &mut Projects, cwd: &Path| {
            let meta = json!({"spyc/cwd": cwd.to_str().unwrap()});
            p.wire(move |c| {
                c.initialize_with(meta);
                c.tool("get_spyc_context", json!({}))
            })
        };
        let ctx = context_from(p, &deep);
        assert_eq!(ctx["cwd"], p.b_home.to_str().unwrap(), "{ctx}");
        let ctx = context_from(p, &outside);
        assert_eq!(ctx["cwd"], p.a_home.to_str().unwrap(), "{ctx}");
    });
}

/// A tab number is a tab of the caller's own project, like its default.
#[test]
fn report_status_from_another_project_lands_on_its_tab() {
    with_projects(|p| {
        let b_pane = p.b_pane.clone();
        p.call_from(&b_pane, "report_status", json!({"status": "working"}));
        assert_eq!(Projects::reported(&p.app, &p.a_pane), None, "a's tab");
        let pane = b_pane.clone();
        let b_status = p.in_b(|app| Projects::reported(app, &pane));
        assert_eq!(b_status, Some(AgentActivity::Working));

        p.call_from(
            &b_pane,
            "report_status",
            json!({"status": "done", "pane": 1}),
        );
        assert_eq!(Projects::reported(&p.app, &p.a_pane), None, "a's tab");
        let b_status = p.in_b(|app| Projects::reported(app, &b_pane));
        assert_eq!(b_status, Some(AgentActivity::Done));
    });
}

/// The status hook never initializes, so its connection belongs to no project:
/// the pane id it names is what finds the tab, in whichever project holds it.
#[test]
fn a_hook_report_for_another_projects_pane_lands_on_its_tab() {
    with_projects(|p| {
        let b_pane = p.b_pane.clone();
        let args = json!({"status": "working", "pane_id": b_pane});
        p.wire(move |c| c.tool("report_status", args));
        assert_eq!(Projects::reported(&p.app, &p.a_pane), None, "a's tab");
        let b_status = p.in_b(|app| Projects::reported(app, &b_pane));
        assert_eq!(b_status, Some(AgentActivity::Working));
    });
}

#[test]
fn register_scope_from_another_project_claims_for_its_tab() {
    with_projects(|p| {
        let b_pane = p.b_pane.clone();
        p.call_from(
            &b_pane,
            "register_scope",
            json!({"paths": ["src/x.rs"], "intent": "editing"}),
        );
        let owner = p.in_b(|app| {
            let tabs = app.runtime.project().pane_tabs.as_ref().unwrap();
            let tab = tabs.tabs().iter().find(|t| t.info.id == b_pane).unwrap();
            tab.info.claim_owner.clone()
        });
        let claim = p.app.state.scope_registry.last().expect("a claim");
        assert_eq!(claim.owner, owner);
    });
}

/// `b`'s agent waits on `a`'s merge: its owner is its own tab, so `a`'s claim
/// is someone else's.
#[test]
fn wait_for_scope_clear_from_another_project_waits_on_others_claims() {
    with_projects(|p| {
        let (a_pane, b_pane) = (p.a_pane.clone(), p.b_pane.clone());
        p.call_from(
            &a_pane,
            "register_scope",
            json!({"paths": ["src/x.rs"], "intent": "merging"}),
        );
        let out = p.call_from(
            &b_pane,
            "wait_for_scope_clear",
            json!({"paths": ["src/x.rs"], "timeout_ms": 300}),
        );
        assert_eq!(out["outcome"], "timed_out", "{out}");
    });
}

#[test]
fn a_statusline_from_another_projects_claude_tab_reaches_it() {
    fn info<'a>(app: &'a mut App, pane: &str) -> &'a mut crate::pane::tabs::TabInfo {
        let tabs = app.runtime.project_mut().pane_tabs.as_mut().unwrap();
        let tab = tabs.tabs_mut().iter_mut().find(|t| t.info.id == pane);
        &mut tab.unwrap().info
    }
    /// Marks the tab `pane` in `project` as Claude Code, sends it a statusline
    /// over the wire, and says whether the tab kept it.
    fn lands(p: &mut Projects, project: state::ProjectId, pane: &str) -> bool {
        p.app.as_project(project, |app| {
            info(app, pane).agent_hint = Some(crate::state::sessions::AgentKind::Claude);
        });
        let params = json!({
            "pane_id": pane,
            "statusline": {"five_hour": {"used_percent": 10, "resets_at": null}},
        });
        p.wire(move |c| c.call("spyc/statusline", params));
        p.app
            .as_project(project, |app| info(app, pane).claude_statusline.is_some())
    }
    with_projects(|p| {
        let (a, a_pane, b, b_pane) = (p.app.state.active, p.a_pane.clone(), p.b, p.b_pane.clone());
        assert!(lands(p, a, &a_pane), "the report is one a Claude tab keeps");
        assert!(lands(p, b, &b_pane), "b's Claude tab kept its statusline");
    });
}

#[test]
fn create_worktree_from_another_project_opens_b_there() {
    with_projects(|p| {
        init_repo(&p.b_home);
        let b_pane = p.b_pane.clone();
        let made = p.call_from(
            &b_pane,
            "create_worktree",
            json!({"branch": "wt", "open": true}),
        );
        let tree = p.base.join("b.worktrees/wt");
        assert!(
            p.app.state.active_project().right.is_none(),
            "the user's project opens no b: {made}"
        );
        let opened = p.in_b(|app| {
            let right = app.state.active_project().right.as_ref();
            right.map(|c| c.listing.dir.clone())
        });
        assert_eq!(opened, Some(tree), "{made}");
    });
}

/// The tree under `b`'s column goes, so `b`'s column goes home: the orphan
/// reset covers the caller's project, not only the user's.
#[test]
fn remove_worktree_from_another_project_resets_its_columns() {
    with_projects(|p| {
        let _cwd = CwdGuard;
        init_repo(&p.b_home);
        let tree = p.base.join("b.worktrees/wt");
        let run_git = crate::git::test_support::run_git;
        run_git(
            &p.b_home,
            &["worktree", "add", "-q", "-b", "wt", tree.to_str().unwrap()],
        );
        let listing = crate::fs::listing::Listing::read(&tree).unwrap();
        p.in_b(|app| app.state.cur_mut().listing = listing);

        let b_pane = p.b_pane.clone();
        let removed = p.call_from(
            &b_pane,
            "remove_worktree",
            json!({"path": tree.to_str().unwrap()}),
        );
        assert!(!tree.exists(), "{removed}");
        assert_eq!(p.b_dir(), p.b_home, "b's column went home: {removed}");
        assert_eq!(p.app.state.cur().listing.dir, p.a_home, "the user's view");
    });
}

#[test]
fn the_activity_dump_names_the_tab_of_a_connection_in_another_project() {
    with_projects(|p| {
        p.app
            .execute_mcp_command(McpCommand::ConnectionInitialized {
                conn: 7,
                pane_id: Some(p.b_pane.clone()),
            });
        p.app.dispatch_command("activity dump");
        let pager = p.app.view.project().pager.as_ref().expect("dump opened");
        let seven = pager
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .find(|l| l.trim_start().starts_with("#7"))
            .expect("connection #7 is listed");
        assert!(
            !seven.contains("closed") && seven.contains("\"cat\""),
            "{seven}"
        );
    });
}

/// Discovery finds this spyc from under any project it has open: the sidecar
/// holds every project's root, and `start_dir` moving rewrites all of them.
#[test]
fn the_sidecar_lists_every_projects_root() {
    with_projects(|p| {
        p.app.view.mcp_running = true;
        let restored = p.base.join("restored");
        std::fs::create_dir(&restored).unwrap();
        p.app
            .state
            .active_project_mut()
            .start_dir
            .clone_from(&restored);
        p.app.write_context();

        let state_dir = crate::state::state_root().expect("the test's state root");
        let sidecar = crate::mcp::root_marker_path_in(&state_dir, std::process::id());
        let recorded = std::fs::read_to_string(sidecar).unwrap_or_default();
        let mut roots: Vec<&str> = recorded.lines().collect();
        roots.sort_unstable();
        assert_eq!(
            roots,
            [p.b_home.to_str().unwrap(), restored.to_str().unwrap()]
        );
    });
}

/// Serve `command` as the loop does for a connection bound to `project`, and
/// return its inline reply.
fn serve(app: &mut App, command: McpCommand, project: Option<state::ProjectId>) -> McpResponse {
    let (reply, rx) = std::sync::mpsc::channel();
    app.route_mcp_request(crate::mcp_cmd::McpRequest {
        command,
        reply,
        project,
    });
    rx.try_recv().expect("answered inline")
}

/// A call from a project that has since closed is refused, and moves nothing,
/// least of all the user's view.
#[test]
fn a_closed_projects_driving_call_is_refused() {
    with_projects(|p| {
        let filter = McpCommand::SetFilter {
            pattern: Some("*.rs".into()),
        };
        let reply = serve(&mut p.app, filter, Some(state::ProjectId::new()));
        assert!(matches!(reply, McpResponse::Error { .. }), "{reply:?}");
        assert_eq!(p.app.state.cur().temp_filter, None);
    });
}

/// What belongs to the user's view stays as it was while an agent drives
/// another project: the flash they're reading, `J`'s frecency, an open
/// harpoon menu, and the process cwd.
#[test]
fn driving_another_project_leaves_the_users_view_alone() {
    with_projects(|p| {
        let _cwd = CwdGuard;
        let (sub, tree) = (p.b_home.join("sub"), p.b_home.join("tree"));
        for d in [&sub, &tree] {
            std::fs::create_dir(d).unwrap();
        }
        p.app.state.flash_info("the user's");
        p.app.view.harpoon_menu = Some(crate::app::view_state::HarpoonMenu {
            cursor: 0,
            delete_armed: false,
        });
        let b = Some(p.b);
        let path = sub.to_str().unwrap().to_string();
        serve(&mut p.app, McpCommand::NavigateTo { path }, b);
        let path = tree.to_str().unwrap().to_string();
        serve(&mut p.app, McpCommand::OpenWorktree { path }, b);

        assert_eq!(p.app.flash_text(), Some("the user's"));
        assert!(p.app.view.harpoon_menu.is_some(), "the user's harpoon menu");
        assert!(
            p.app.state.frecency.search("sub").is_empty(),
            "J learned another project's navigation"
        );
        assert_ne!(std::env::current_dir().ok(), Some(sub));
    });
}

impl Projects {
    /// Bind a connection by `meta`, run `close` in the loop once it is bound,
    /// then have the agent search for `in_`. The server starts with the
    /// context file `default`, which a connection bound to nothing reads.
    fn search_across_a_close(
        &mut self,
        default: PathBuf,
        meta: Value,
        close: impl FnOnce(&mut App),
    ) -> String {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut conn = Conn::open(default, tx);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let app = &mut self.app;
        let mut ctx = RunCtx::for_test();
        let mut close = Some(close);
        let reply = std::thread::scope(|s| {
            let client = s.spawn(move || {
                conn.initialize_with(meta);
                ready_tx.send(()).unwrap();
                go_rx.recv().unwrap();
                conn.tool("search_paths", json!({"query": "in_"}))
            });
            while !client.is_finished() {
                if let Ok(req) = rx.recv_timeout(Duration::from_millis(5)) {
                    ctx.mcp_pending.push(req);
                    app.drain_mcp_pending(&mut ctx);
                }
                if ready_rx.try_recv().is_ok()
                    && let Some(close) = close.take()
                {
                    close(app);
                    go_tx.send(()).unwrap();
                }
            }
            client.join().expect("the client finishes")
        });
        reply.to_string()
    }
}

/// An agent whose project has closed reads nothing. Its tab went with the
/// project, and answering from wherever spyc's process cwd is would hand it
/// another project's tree.
#[test]
fn a_read_from_a_closed_projects_agent_answers_closed() {
    with_projects(|p| {
        let b = p.b;
        let default = p.app.view.project().context_path.clone();
        let meta = json!({"spyc/paneId": p.b_pane});
        let text = p.search_across_a_close(default, meta, |app| app.remove_test_project(b));
        assert!(text.contains("has closed"), "{text}");
        assert!(!text.contains("in_a.rs"), "{text}");
    });
}

/// A connection bound by its cwd, as a session codex's shared daemon runs is,
/// belongs to no tab, so it outlives a project closing. It binds again from
/// its cwd, as it would at `initialize`: to the project holding it, else the
/// active one, never to nothing. The server's own file is a third project's,
/// so reading that instead is told apart.
#[test]
fn a_connection_bound_by_cwd_binds_again_when_its_project_closes() {
    with_projects(|p| {
        let c_home = p.base.join("c");
        std::fs::create_dir(&c_home).unwrap();
        std::fs::write(c_home.join("in_c.rs"), "").unwrap();
        let c = p.app.add_test_project(&c_home);
        let default = p
            .app
            .as_project(c, |app| app.view.project().context_path.clone());
        let b = p.b;
        let meta = json!({"spyc/cwd": p.b_home});
        let text = p.search_across_a_close(default, meta, |app| app.remove_test_project(b));
        assert!(text.contains("in_a.rs"), "{text}");
        assert!(!text.contains("in_c.rs"), "{text}");
    });
}

/// A connection that named a tab that isn't live, and no cwd, is bound to
/// the project the user was in. It has no tab to lose with that project, so
/// when the project closes it binds again the same way, to the project the
/// user is in now.
#[test]
fn a_connection_bound_to_the_users_project_binds_again_when_it_closes() {
    with_projects(|p| {
        let (a, b) = (p.app.state.active, p.b);
        let default = p.app.view.project().context_path.clone();
        let meta = json!({"spyc/paneId": "no-such-pane"});
        let text = p.search_across_a_close(default, meta, move |app| {
            app.make_active(b);
            app.remove_test_project(a);
        });
        assert!(text.contains("in_b.rs"), "{text}");
    });
}

/// A connection that has not bound, one that sent no `initialize`, reads the
/// server's own context file, the launch project's. Once that project closes
/// it binds as an `initialize` naming nothing would, to the project the user
/// is in, rather than reading whatever directory spyc's process cwd is.
#[test]
fn a_connection_that_has_not_bound_follows_the_user_when_the_launch_project_closes() {
    with_projects(|p| {
        let (a, b) = (p.app.state.active, p.b);
        let default = p.app.view.project().context_path.clone();
        p.app.make_active(b);
        p.app.remove_test_project(a);
        let found = p.wire_from(default, |c| c.tool("search_paths", json!({"query": "in_"})));
        let text = found.to_string();
        assert!(text.contains("in_b.rs"), "{text}");
    });
}

/// A hook's one-shot `report_status`, which never sends `initialize`, names
/// its tab and reads no context file, so the launch project having closed
/// doesn't make it bind first: it fires on every turn of every agent.
#[test]
fn a_hook_report_after_the_launch_project_closes_binds_nothing() {
    with_projects(|p| {
        let (a, b) = (p.app.state.active, p.b);
        let default = p.app.view.project().context_path.clone();
        p.app.make_active(b);
        p.app.remove_test_project(a);
        let pane = p.b_pane.clone();
        let args = json!({"status": "working", "pane_id": pane});
        p.wire_from(default, move |c| c.tool("report_status", args));
        assert_eq!(p.binds, 0, "the report bound first");
        assert_eq!(
            Projects::reported(&p.app, &p.b_pane),
            Some(AgentActivity::Working)
        );
    });
}

/// An agent's `initialize`, arriving once the launch project has closed,
/// binds it to the pane it names, as it would with that project open: the
/// connection isn't bound to the user's project on the way.
#[test]
fn an_agent_initializing_after_the_launch_project_closes_binds_its_pane() {
    with_projects(|p| {
        let (a, b) = (p.app.state.active, p.b);
        let default = p.app.view.project().context_path.clone();
        p.app.make_active(b);
        p.app.remove_test_project(a);
        let pane = p.b_pane.clone();
        let context = p.wire_from(default, move |c| {
            c.initialize(Some(&pane));
            c.tool("get_spyc_context", json!({}))
        });
        assert_eq!(context["pane"]["id"], p.b_pane.as_str(), "{context}");
    });
}

#[test]
fn teardown_removes_every_projects_context_file() {
    with_projects(|p| {
        let files: Vec<PathBuf> = p.app.view.context_paths().map(Path::to_path_buf).collect();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.exists()), "{files:?}");
        p.app.run_teardown();
        assert!(files.iter().all(|f| !f.exists()), "{files:?}");
    });
}

/// A connection's reads answer from its project's file, so binding to a
/// project whose file was never written writes it.
#[test]
fn binding_writes_the_bound_projects_context_file() {
    with_projects(|p| {
        let path = p.in_b(|app| {
            app.view.project_mut().last_context = None;
            app.view.project().context_path.clone()
        });
        std::fs::remove_file(&path).unwrap();
        let reply = p.app.execute_mcp_command(McpCommand::Bind {
            pane_id: Some(p.b_pane.clone()),
            cwd: None,
        });
        let McpResponse::Ok { message } = reply else {
            panic!("{reply:?}");
        };
        let bound: Value = serde_json::from_str(&message).unwrap();
        assert_eq!(bound["pane"], true, "{bound}");
        assert_eq!(bound["context_file"], path.to_str().unwrap(), "{bound}");
        assert!(path.exists(), "the bound project's context is on disk");
    });
}
