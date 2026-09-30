//! Pane attribution: a connection whose `initialize` names a live pane answers
//! for that pane, and every other connection is served exactly as before.

use std::sync::mpsc::{self, Receiver, Sender};

use super::*;
use crate::mcp::protocol::PANE_ID_META;
use crate::mcp::server::annotate_initialize;
use crate::mcp_cmd::{McpRequest, McpResponse};

/// A stand-in for the main loop that knows the pane ids in `live`. Every
/// command it receives is recorded on the returned channel before it replies,
/// so a caller that has its answer can read what the loop saw.
fn fake_loop(live: &'static [&'static str]) -> (Sender<McpRequest>, Receiver<McpCommand>) {
    let (tx, rx) = mpsc::channel::<McpRequest>();
    let (seen_tx, seen_rx) = mpsc::channel();
    std::thread::spawn(move || {
        for McpRequest { command, reply } in rx {
            let resp = match &command {
                McpCommand::PaneContext { pane_id } if live.contains(&pane_id.as_str()) => {
                    McpResponse::Ok {
                        message: json!({"id": pane_id, "cwd": format!("/wt/{pane_id}")})
                            .to_string(),
                    }
                }
                McpCommand::PaneContext { pane_id } => McpResponse::Error {
                    message: format!("no pane with id {pane_id} (closed?)"),
                },
                _ => McpResponse::Ok {
                    message: "ok".into(),
                },
            };
            let _ = seen_tx.send(command);
            let _ = reply.send(resp);
        }
    });
    (tx, seen_rx)
}

/// A context file saying the user is browsing `/user/y`.
fn user_browsing_y(tmp: &tempfile::TempDir) -> PathBuf {
    let ctx = context::SpycContext {
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

/// One socket connection to `handle_socket_connection`, over
/// [`user_browsing_y`].
struct Conn {
    reader: io::BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
    _tmp: tempfile::TempDir,
}

impl Conn {
    fn open(cmd_tx: Sender<McpRequest>) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let ctx_path = user_browsing_y(&tmp);
        let sock = tmp.path().join("attr.sock");
        let listener = bind_test_socket(&sock);
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_socket_connection(stream, &ctx_path, &cmd_tx).unwrap_or(());
        });
        let stream = UnixStream::connect(&sock).unwrap();
        Self {
            reader: io::BufReader::new(stream.try_clone().unwrap()),
            writer: stream,
            next_id: 1,
            _tmp: tmp,
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let body =
            json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params});
        self.next_id += 1;
        send_message(&mut self.writer, &body.to_string()).unwrap();
        serde_json::from_str(&read_lsp_message(&mut self.reader).unwrap()).unwrap()
    }

    fn initialize(&mut self, pane_id: Option<&str>) {
        let params = pane_id.map_or_else(|| json!({}), |p| json!({"_meta": {PANE_ID_META: p}}));
        let resp = self.call("initialize", params);
        assert!(resp["result"]["serverInfo"].is_object(), "{resp}");
    }

    /// A tool's text result, parsed when it is JSON.
    fn tool(&mut self, name: &str, args: Value) -> Value {
        let resp = self.call("tools/call", json!({"name": name, "arguments": args}));
        let text = resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{resp}"))
            .to_string();
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    }
}

/// What a `report_status` targeted: its `pane_id`, then its `pane` index.
type Target = (Option<String>, Option<usize>);

/// The target of each `report_status` the loop received, and whether any
/// `PaneContext` lookup reached it.
fn reports(seen: &Receiver<McpCommand>) -> (Vec<Target>, bool) {
    let mut out = Vec::new();
    let mut looked_up = false;
    for c in seen.try_iter() {
        match c {
            McpCommand::ReportStatus { pane_id, pane, .. } => out.push((pane_id, pane)),
            McpCommand::PaneContext { .. } => looked_up = true,
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
    let mut conn = Conn::open(tx);
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
    let mut conn = Conn::open(tx);
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
    let mut conn = Conn::open(tx);
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
    let mut conn = Conn::open(tx);
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
    let mut conn = Conn::open(tx);
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

    crate::mcp::server::proxy_io(Cursor::new(stdin), &mut stdout, agent_side, Some("abc")).unwrap();

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
    let out: Value = serde_json::from_str(&annotate_initialize(msg, Some("abc"))).unwrap();
    assert_eq!(out["params"]["_meta"][PANE_ID_META], "abc");
    assert_eq!(
        out["params"]["_meta"]["k"], 1,
        "the client's own _meta survives"
    );
    assert_eq!(out["params"]["protocolVersion"], "2025-06-18");
    assert_eq!(out["id"], 0);

    let bare = r#"{"jsonrpc":"2.0","id":0,"method":"initialize"}"#;
    let out: Value = serde_json::from_str(&annotate_initialize(bare, Some("abc"))).unwrap();
    assert_eq!(out["params"]["_meta"][PANE_ID_META], "abc");
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
        assert_eq!(annotate_initialize(msg, pane), msg, "{msg} / {pane:?}");
    }
}
