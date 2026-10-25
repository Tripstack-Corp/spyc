//! Pane attribution: a connection whose `initialize` names a live pane answers
//! for that pane, and every other connection is served exactly as before.

use std::sync::mpsc::{self, Receiver, Sender};

use super::*;
use crate::mcp::protocol::{CWD_META, PANE_ID_META};
use crate::mcp::server::annotate_initialize;
use crate::mcp::test_support::Conn;
use crate::mcp_cmd::{McpRequest, McpResponse};

/// The one project [`fake_loop`] binds every connection to.
const PROJECT: &str = "0192f0e4-7c3a-7b2e-9a51-3c1d2e4f5a6b";

/// A stand-in for the main loop that knows the pane ids in `live`, all in
/// [`PROJECT`]. Every command it receives is recorded on the returned channel
/// before it replies, so a caller that has its answer can read what the loop
/// saw.
fn fake_loop(live: &'static [&'static str]) -> (Sender<McpRequest>, Receiver<McpCommand>) {
    let (tx, rx) = mpsc::channel::<McpRequest>();
    let (seen_tx, seen_rx) = mpsc::channel();
    let is_live = move |p: &Option<String>| p.as_deref().is_some_and(|p| live.contains(&p));
    std::thread::spawn(move || {
        for McpRequest { command, reply, .. } in rx {
            let message = match &command {
                McpCommand::Bind { pane_id, .. } => {
                    json!({"pane": is_live(pane_id), "project": PROJECT}).to_string()
                }
                McpCommand::CallerContext { pane_id, .. } if is_live(pane_id) => {
                    let pane_id = pane_id.as_deref().unwrap_or_default();
                    json!({"pane": {"id": pane_id, "cwd": format!("/wt/{pane_id}")}}).to_string()
                }
                McpCommand::CallerContext { .. } => "{}".into(),
                _ => "ok".into(),
            };
            let resp = McpResponse::Ok { message };
            let _ = seen_tx.send(command);
            let _ = reply.send(resp);
        }
    });
    (tx, seen_rx)
}

/// A context file saying the user is browsing `/user/y`.
fn user_browsing_y(tmp: &tempfile::TempDir) -> PathBuf {
    let ctx = context::SpycContext {
        root: tmp.path().to_path_buf(),
        cwd: PathBuf::from("/user/y"),
        cursor_file: None,
        picks: vec![],
        inventory: vec![],
        filter: None,
        git_branch: None,
        project_home: None,
        search_root: None,
        session_name: String::new(),
        pid: 0,
        version: String::new(),
        archive_mounts: Vec::new(),
    };
    let ctx_path = context::context_path(tmp.path());
    context::write_context_file(&ctx_path, &ctx).unwrap();
    ctx_path
}

/// A connection over [`user_browsing_y`], and the temp dir its context lives in.
fn open(cmd_tx: Sender<McpRequest>) -> (Conn, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let conn = Conn::open(user_browsing_y(&tmp), cmd_tx);
    (conn, tmp)
}

/// What a `report_status` targeted: its `pane_id`, then its `pane` index.
type Target = (Option<String>, Option<usize>);

/// The target of each `report_status` the loop received, and whether any
/// lookup of the caller (`Bind`, `CallerContext`) reached it.
fn reports(seen: &Receiver<McpCommand>) -> (Vec<Target>, bool) {
    let mut out = Vec::new();
    let mut looked_up = false;
    for c in seen.try_iter() {
        match c {
            McpCommand::ReportStatus { pane_id, pane, .. } => out.push((pane_id, pane)),
            McpCommand::Bind { .. } | McpCommand::CallerContext { .. } => looked_up = true,
            _ => {}
        }
    }
    (out, looked_up)
}

/// The exit criterion: an agent in worktree X gets X while the user browses Y.
/// The user's view keeps its fields; the caller's own tab arrives beside them.
#[test]
fn a_connection_naming_a_live_pane_answers_for_that_pane() {
    let (tx, seen) = fake_loop(&["abc"]);
    let (mut conn, _tmp) = open(tx);
    conn.initialize(Some("abc"));

    let ctx = conn.tool("get_spyc_context", json!({}));
    assert_eq!(ctx["cwd"], "/user/y", "cwd is still what the user browses");
    assert_eq!(
        ctx["pane"]["cwd"], "/wt/abc",
        "pane is the caller's own: {ctx}"
    );

    conn.tool("report_status", json!({"status": "working"}));
    assert_eq!(reports(&seen).0, [(Some("abc".into()), None)]);
}

/// An older `spyc --mcp` proxy sends no pane id. Its connection is served as
/// every connection was before attribution: no `pane`, no lookup, and a
/// `report_status` left to the focused tab.
#[test]
fn an_initialize_without_a_pane_id_is_served_as_before() {
    let (tx, seen) = fake_loop(&["abc"]);
    let (mut conn, _tmp) = open(tx);
    conn.initialize(None);

    let ctx = conn.tool("get_spyc_context", json!({}));
    assert_eq!(ctx["cwd"], "/user/y");
    assert!(ctx.get("pane").is_none(), "{ctx}");

    conn.tool("report_status", json!({"status": "working"}));
    let (targets, looked_up) = reports(&seen);
    assert_eq!(targets, [(None, None)]);
    assert!(
        !looked_up,
        "an unattributed connection never asks for a pane"
    );
}

/// An id no live tab carries — a closed tab, a stale env — degrades to
/// unattributed. Keeping it would aim `report_status` at a tab that's gone.
#[test]
fn a_pane_id_no_live_tab_has_is_dropped() {
    let (tx, seen) = fake_loop(&["abc"]);
    let (mut conn, _tmp) = open(tx);
    conn.initialize(Some("gone"));

    let ctx = conn.tool("get_spyc_context", json!({}));
    assert!(ctx.get("pane").is_none(), "{ctx}");
    conn.tool("report_status", json!({"status": "working"}));
    assert_eq!(reports(&seen).0, [(None, None)]);
}

/// The binding is the default, never an override: a call that names its own
/// target keeps it.
#[test]
fn a_call_that_names_its_target_keeps_it() {
    let (tx, seen) = fake_loop(&["abc"]);
    let (mut conn, _tmp) = open(tx);
    conn.initialize(Some("abc"));

    conn.tool(
        "report_status",
        json!({"status": "done", "pane_id": "other"}),
    );
    conn.tool("report_status", json!({"status": "done", "pane": 2}));
    assert_eq!(
        reports(&seen).0,
        [(Some("other".into()), None), (None, Some(2))]
    );
}

/// Bound once: a later `initialize` on the same connection can't move it.
#[test]
fn a_second_initialize_does_not_rebind() {
    let (tx, seen) = fake_loop(&["abc", "def"]);
    let (mut conn, _tmp) = open(tx);
    conn.initialize(Some("abc"));
    conn.initialize(Some("def"));

    conn.tool("report_status", json!({"status": "idle"}));
    assert_eq!(reports(&seen).0, [(Some("abc".into()), None)]);
}

/// The whole path: an agent's newline-delimited `initialize` and
/// `get_spyc_context` through the real proxy, into a real connection.
#[test]
fn the_proxy_carries_its_pane_to_the_server() {
    let (tx, _seen) = fake_loop(&["abc"]);
    let tmp = tempfile::tempdir().unwrap();
    let ctx_path = user_browsing_y(&tmp);
    let (agent_side, server_side) = UnixStream::pair().unwrap();
    std::thread::spawn(move || {
        handle_socket_connection(server_side, &ctx_path, &tx).unwrap_or(());
    });
    let stdin = [
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {}}),
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
               "params": {"name": "get_spyc_context", "arguments": {}}}),
    ]
    .map(|m| m.to_string() + "\n")
    .concat();
    let mut stdout = Vec::new();

    crate::mcp::server::proxy_io(
        Cursor::new(stdin),
        &mut stdout,
        agent_side,
        Some("abc"),
        None,
        PROXY_IO_TIMEOUT,
    )
    .unwrap();

    let replies: Vec<Value> = std::str::from_utf8(&stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let text = replies[1]["result"]["content"][0]["text"].as_str().unwrap();
    let ctx: Value = serde_json::from_str(text).unwrap();
    assert_eq!(ctx["pane"]["cwd"], "/wt/abc", "{ctx}");
}

#[test]
fn the_proxy_names_its_pane_in_initialize() {
    let msg = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","_meta":{"k":1}}}"#;
    let out: Value = serde_json::from_str(&annotate_initialize(msg, Some("abc"), None)).unwrap();
    assert_eq!(out["params"]["_meta"][PANE_ID_META], "abc");
    assert_eq!(
        out["params"]["_meta"]["k"], 1,
        "the client's own _meta survives"
    );
    assert_eq!(out["params"]["protocolVersion"], "2025-06-18");
    assert_eq!(out["id"], 0);

    let bare = r#"{"jsonrpc":"2.0","id":0,"method":"initialize"}"#;
    let out: Value = serde_json::from_str(&annotate_initialize(bare, Some("abc"), None)).unwrap();
    assert_eq!(out["params"]["_meta"][PANE_ID_META], "abc");
}

/// With no pane, the proxy names its cwd, so the server can bind the
/// connection to the project holding it (#99). With both, it names both.
#[test]
fn the_proxy_names_its_cwd_in_initialize() {
    let msg = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}"#;
    let cwd = Some(Path::new("/work/b"));
    let out: Value = serde_json::from_str(&annotate_initialize(msg, None, cwd)).unwrap();
    assert_eq!(out["params"]["_meta"][CWD_META], "/work/b");
    assert!(out["params"]["_meta"].get(PANE_ID_META).is_none(), "{out}");

    let out: Value = serde_json::from_str(&annotate_initialize(msg, Some("abc"), cwd)).unwrap();
    assert_eq!(out["params"]["_meta"][CWD_META], "/work/b");
    assert_eq!(out["params"]["_meta"][PANE_ID_META], "abc");
}

/// The whole path for a session with no pane: the proxy's cwd reaches the loop
/// as the cwd to bind by.
#[test]
fn the_proxy_carries_its_cwd_to_the_loop() {
    let (tx, seen) = fake_loop(&[]);
    let tmp = tempfile::tempdir().unwrap();
    let ctx_path = user_browsing_y(&tmp);
    let (agent_side, server_side) = UnixStream::pair().unwrap();
    std::thread::spawn(move || {
        handle_socket_connection(server_side, &ctx_path, &tx).unwrap_or(());
    });
    let stdin = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {}});
    let mut stdout = Vec::new();
    let cwd = Path::new("/work/b");

    crate::mcp::server::proxy_io(
        Cursor::new(stdin.to_string() + "\n"),
        &mut stdout,
        agent_side,
        None,
        Some(cwd),
        PROXY_IO_TIMEOUT,
    )
    .unwrap();

    let bound_by = seen.try_iter().find_map(|c| match c {
        McpCommand::Bind { cwd, .. } => cwd,
        _ => None,
    });
    assert_eq!(bound_by.as_deref(), Some(cwd));
}

#[test]
fn the_proxy_passes_everything_else_through_verbatim() {
    let init = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}"#;
    let call = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"x"}}"#;
    let note = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    for (msg, pane) in [
        (call, Some("abc")),
        (note, Some("abc")),
        ("not json", Some("abc")),
        (init, None),
        (init, Some("")),
    ] {
        assert_eq!(
            annotate_initialize(msg, pane, None),
            msg,
            "{msg} / {pane:?}"
        );
    }
}

/// The loop hears when an agent's connection starts, which pane it bound to,
/// which connection each call came over, and when it ends.
#[test]
fn the_loop_hears_a_connection_start_and_end() {
    let (tx, seen) = fake_loop(&["abc"]);
    let (mut conn, _tmp) = open(tx);
    conn.initialize(Some("abc"));
    conn.tool("git_status", json!({}));
    drop(conn);

    let mut started = None;
    let mut called = None;
    let mut ended = None;
    while ended.is_none() {
        match seen
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the close")
        {
            McpCommand::ConnectionInitialized { conn, pane_id } => started = Some((conn, pane_id)),
            McpCommand::ToolCalled { conn, .. } => called = conn,
            McpCommand::ConnectionClosed { conn } => ended = Some(conn),
            _ => {}
        }
    }
    let (id, pane) = started.expect("initialize was reported");
    assert_eq!(pane.as_deref(), Some("abc"));
    assert_eq!(called, Some(id), "the call names its connection");
    assert_eq!(ended, Some(id));
}

/// The loop hears which project a connection acts on: at its `initialize`,
/// and again when that project closes and it binds to another.
#[test]
fn the_loop_hears_each_project_a_connection_binds_to() {
    const OTHER: &str = "0192f0e4-7c3a-7b2e-9a51-3c1d2e4f5a6c";
    let tmp = tempfile::tempdir().unwrap();
    let user = user_browsing_y(&tmp);
    let first = tmp.path().join("first.json");
    std::fs::copy(&user, &first).unwrap();
    let (tx, rx) = mpsc::channel::<McpRequest>();
    let (seen_tx, seen) = mpsc::channel();
    let files = (first.clone(), user.clone());
    std::thread::spawn(move || {
        let mut binds = 0;
        for McpRequest { command, reply, .. } in rx {
            let message = match &command {
                McpCommand::Bind { .. } => {
                    binds += 1;
                    let (project, file) = if binds == 1 {
                        (PROJECT, &files.0)
                    } else {
                        (OTHER, &files.1)
                    };
                    json!({"pane": false, "project": project, "context_file": file}).to_string()
                }
                _ => "ok".into(),
            };
            let _ = seen_tx.send(command);
            let _ = reply.send(McpResponse::Ok { message });
        }
    });
    let mut conn = Conn::open(user, tx);
    conn.initialize_with(json!({CWD_META: "/w/b"}));
    std::fs::remove_file(&first).unwrap();
    conn.tool("git_status", json!({}));
    drop(conn);
    let mut bound = Vec::new();
    let mut initialized = false;
    loop {
        match seen
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the close")
        {
            McpCommand::ConnectionInitialized { .. } => initialized = true,
            McpCommand::ConnectionBound { project, .. } => {
                // The loop keeps a project only on a connection it has heard of.
                assert!(initialized, "a project before its connection");
                bound.push(project.to_string());
            }
            McpCommand::ConnectionClosed { .. } => break,
            _ => {}
        }
    }
    assert_eq!(bound, [PROJECT, OTHER]);
}

/// An older proxy's connection is reported too, as unattributed.
#[test]
fn an_unattributed_connection_is_reported_without_a_pane() {
    let (tx, seen) = fake_loop(&["abc"]);
    let (mut conn, _tmp) = open(tx);
    conn.initialize(None);
    drop(conn);
    let reported = std::iter::from_fn(|| seen.recv_timeout(std::time::Duration::from_secs(5)).ok())
        .find_map(|c| match c {
            McpCommand::ConnectionInitialized { pane_id, .. } => Some(pane_id),
            _ => None,
        });
    assert_eq!(reported, Some(None));
}

/// The status hook's one-shot `report_status` never initializes. It is not an
/// agent's session, so it never shows up as a connection.
#[test]
fn a_connection_that_never_initializes_is_not_listed() {
    let (tx, seen) = fake_loop(&["abc"]);
    let (mut conn, _tmp) = open(tx);
    conn.tool("report_status", json!({"status": "working"}));
    drop(conn);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let listed = seen.try_iter().any(|c| {
        matches!(
            c,
            McpCommand::ConnectionInitialized { .. } | McpCommand::ConnectionClosed { .. }
        )
    });
    assert!(!listed);
}

/// `create_worktree` carries the connection's own pane, so the loop can find the
/// agent's repo when the user is browsing outside one. An unattributed
/// connection carries none.
#[test]
fn create_worktree_carries_the_callers_pane() {
    let created = |pane: Option<&str>| {
        let (tx, seen) = fake_loop(&["abc"]);
        let (mut conn, _tmp) = open(tx);
        conn.initialize(pane);
        conn.tool("create_worktree", json!({"branch": "wt"}));
        seen.try_iter()
            .find_map(|c| match c {
                McpCommand::CreateWorktree { pane_id, .. } => Some(pane_id),
                _ => None,
            })
            .expect("create_worktree reached the loop")
    };
    assert_eq!(created(Some("abc")).as_deref(), Some("abc"));
    assert_eq!(created(None), None);
}
