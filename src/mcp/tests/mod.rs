//! Tests for the MCP server (dispatch, socket transport, client config).
//! Split out of mcp.rs during the 800-LoC decomposition; kept as one file
//! (the suite is ~660 lines, under the limit).

#![allow(clippy::wildcard_imports)]

use std::io::{self, Cursor};
use std::os::unix::net::UnixListener;

use serde_json::{Value, json};

use crate::mcp_cmd::McpCommand;

use super::protocol::{dispatch, read_lsp_message, send_message};
use super::server::{
    collect_project_pids_in, discover_live_socket, handle_socket_connection, pid_from_sock_path,
    read_context_pids_in_dir,
};
use super::*;

fn make_request(id: u64, method: &str, params: Value) -> String {
    let body = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
    .to_string();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
}

fn parse_response(raw: &[u8]) -> Value {
    let s = std::str::from_utf8(raw).unwrap();
    let body_start = s.find("\r\n\r\n").unwrap() + 4;
    serde_json::from_str(&s[body_start..]).unwrap()
}

#[test]
fn initialize_response() {
    let input = make_request(1, "initialize", json!({}));
    let mut reader = Cursor::new(input.as_bytes().to_vec());
    let msg = read_lsp_message(&mut reader).unwrap();

    let mut output = Vec::new();
    dispatch(&mut output, &msg, Path::new("/tmp"), None).unwrap();
    let resp = parse_response(&output);
    assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(resp["result"]["serverInfo"]["name"], "spyc");
    // The `instructions` field steers a launched agent toward spyc's tools.
    let instructions = resp["result"]["instructions"].as_str().unwrap();
    assert!(
        instructions.contains("search_content"),
        "names the search tool"
    );
    assert!(
        instructions.contains("get_spyc_context"),
        "tells it to ground first"
    );
    assert!(
        instructions.contains("spyc"),
        "frames the agent as running inside spyc"
    );
    assert!(
        instructions.contains("clean_worktree"),
        "names the full worktree lifecycle, not just create/remove"
    );
}

#[test]
fn resources_list_response() {
    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc":"2.0","id":2,"method":"resources/list"}).to_string(),
        Path::new("/tmp"),
        None,
    )
    .unwrap();
    let resp = parse_response(&output);
    let resources = resp["result"]["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0]["uri"], CONTEXT_URI);
}

#[test]
fn resources_read_with_context_file() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = context::SpycContext {
        cwd: PathBuf::from("/home/user/project"),
        cursor_file: Some("main.rs".into()),
        picks: vec![],
        inventory: vec![],
        filter: None,
        git_branch: Some("develop".into()),
        project_home: None,
        search_root: None,
        session_name: String::new(),
        pid: 0,
        version: String::new(),
    };
    let ctx_path = context::context_path(tmp.path());
    context::write_context_file(&ctx_path, &ctx).unwrap();

    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":CONTEXT_URI}})
            .to_string(),
        &ctx_path,
        None,
    )
    .unwrap();
    let resp = parse_response(&output);
    let text = resp["result"]["contents"][0]["text"].as_str().unwrap();
    let inner: Value = serde_json::from_str(text).unwrap();
    assert_eq!(inner["cwd"], "/home/user/project");
    assert_eq!(inner["git_branch"], "develop");
}

#[test]
fn tools_list_response() {
    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc":"2.0","id":4,"method":"tools/list"}).to_string(),
        Path::new("/tmp"),
        None,
    )
    .unwrap();
    let resp = parse_response(&output);
    let tools = resp["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 19);
    assert_eq!(tools[0]["name"], "get_spyc_context");
    assert_eq!(tools[1]["name"], "navigate_to");
    assert_eq!(tools[2]["name"], "set_filter");
    assert_eq!(tools[3]["name"], "pick_files");
    assert_eq!(tools[4]["name"], "clear_picks");
    assert_eq!(tools[5]["name"], "create_worktree");
    assert_eq!(tools[6]["name"], "remove_worktree");
    assert_eq!(tools[7]["name"], "clean_worktree");
    assert_eq!(tools[8]["name"], "open_worktree");
    assert_eq!(tools[9]["name"], "get_file_content");
    assert_eq!(tools[10]["name"], "search_paths");
    assert_eq!(tools[11]["name"], "search_content");
    assert_eq!(tools[12]["name"], "search_picks");
    assert_eq!(tools[13]["name"], "search_inventory");
    assert_eq!(tools[14]["name"], "list_worktrees");
    assert_eq!(tools[15]["name"], "claim_worktree");
    assert_eq!(tools[16]["name"], "release_worktree");
    assert_eq!(tools[17]["name"], "git_status");
    assert_eq!(tools[18]["name"], "git_log");
}

#[test]
fn tools_call_returns_context() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = context::SpycContext {
        cwd: PathBuf::from("/projects/spyc"),
        cursor_file: Some("Cargo.toml".into()),
        picks: vec![PathBuf::from("src/main.rs")],
        inventory: vec![],
        filter: Some("*.rs".into()),
        git_branch: Some("feature".into()),
        project_home: None,
        search_root: None,
        session_name: String::new(),
        pid: 0,
        version: String::new(),
    };
    let ctx_path = context::context_path(tmp.path());
    context::write_context_file(&ctx_path, &ctx).unwrap();

    let mut output = Vec::new();
    dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"get_spyc_context","arguments":{}}}).to_string(),
            &ctx_path,
            None,
        )
        .unwrap();
    let resp = parse_response(&output);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let inner: Value = serde_json::from_str(text).unwrap();
    assert_eq!(inner["cursor_file"], "Cargo.toml");
    assert_eq!(inner["filter"], "*.rs");
}

/// Every `tools/call` forwards a fire-and-forget `ToolCalled{name}` down the
/// command channel (when a live spyc is connected) so the `A` overlay can tally
/// per-tool usage — even for read tools the socket thread serves inline.
#[test]
fn tools_call_forwards_tool_called_telemetry() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"get_spyc_context","arguments":{}}}).to_string(),
        Path::new("/tmp"),
        Some(&tx),
    )
    .unwrap();
    let req = rx.try_recv().expect("a ToolCalled was forwarded");
    match req.command {
        McpCommand::ToolCalled { name } => assert_eq!(name, "get_spyc_context"),
        other => panic!("expected ToolCalled, got {other:?}"),
    }
}

#[test]
fn unknown_resource_returns_error() {
    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"spyc://bogus"}})
            .to_string(),
        Path::new("/tmp"),
        None,
    )
    .unwrap();
    let resp = parse_response(&output);
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unknown resource")
    );
}

#[test]
fn notification_is_silent() {
    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string(),
        Path::new("/tmp"),
        None,
    )
    .unwrap();
    assert!(output.is_empty());
}

#[test]
fn search_paths_tool_walks_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join("alpha.rs"), "").unwrap();
    std::fs::write(root.join("beta.rs"), "").unwrap();
    std::fs::write(root.join("gamma.txt"), "").unwrap();
    let ctx = context::SpycContext {
        cwd: root.to_path_buf(),
        cursor_file: None,
        picks: vec![],
        inventory: vec![],
        filter: None,
        git_branch: None,
        project_home: Some(root.to_path_buf()),
        search_root: Some(root.to_path_buf()),
        session_name: String::new(),
        pid: 0,
        version: String::new(),
    };
    let ctx_path = context::context_path(tmp.path());
    context::write_context_file(&ctx_path, &ctx).unwrap();

    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc":"2.0","id":7,"method":"tools/call",
                "params":{"name":"search_paths","arguments":{"query":"alpha"}}})
        .to_string(),
        &ctx_path,
        None,
    )
    .unwrap();
    let resp = parse_response(&output);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let arr: Value = serde_json::from_str(text).unwrap();
    let paths: Vec<&str> = arr
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(paths.contains(&"alpha.rs"));
    assert!(!paths.contains(&"gamma.txt"));
}

/// `search_root` (the focused column's worktree root) takes precedence over
/// `project_home`. With a context whose `project_home` points at one tree but
/// `search_root` at a separate worktree, the walk scopes to `search_root` —
/// this is what makes MCP search follow the focused worktree (column `b` in a
/// different worktree searches its own tree, not the overall project anchor).
#[test]
fn search_root_overrides_project_home() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("main"); // PROJECT_HOME (column a)
    let worktree = tmp.path().join("wt"); // focused column b's worktree
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(home.join("in_home.rs"), "").unwrap();
    std::fs::write(worktree.join("in_worktree.rs"), "").unwrap();
    let ctx = context::SpycContext {
        cwd: worktree.clone(),
        cursor_file: None,
        picks: vec![],
        inventory: vec![],
        filter: None,
        git_branch: None,
        project_home: Some(home),
        search_root: Some(worktree),
        session_name: String::new(),
        pid: 0,
        version: String::new(),
    };
    let ctx_path = context::context_path(tmp.path());
    context::write_context_file(&ctx_path, &ctx).unwrap();

    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc":"2.0","id":17,"method":"tools/call",
                "params":{"name":"search_paths","arguments":{"query":"in_"}}})
        .to_string(),
        &ctx_path,
        None,
    )
    .unwrap();
    let resp = parse_response(&output);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let arr: Value = serde_json::from_str(text).unwrap();
    let paths: Vec<&str> = arr
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(paths.contains(&"in_worktree.rs"), "walks search_root");
    assert!(!paths.contains(&"in_home.rs"), "ignores project_home");
}

#[test]
fn search_content_tool_returns_matches() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join("a.txt"), "hello world\n").unwrap();
    let ctx = context::SpycContext {
        cwd: root.to_path_buf(),
        cursor_file: None,
        picks: vec![],
        inventory: vec![],
        filter: None,
        git_branch: None,
        project_home: Some(root.to_path_buf()),
        search_root: Some(root.to_path_buf()),
        session_name: String::new(),
        pid: 0,
        version: String::new(),
    };
    let ctx_path = context::context_path(tmp.path());
    context::write_context_file(&ctx_path, &ctx).unwrap();

    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc":"2.0","id":8,"method":"tools/call",
                "params":{"name":"search_content","arguments":{"pattern":"hello"}}})
        .to_string(),
        &ctx_path,
        None,
    )
    .unwrap();
    let resp = parse_response(&output);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let arr: Value = serde_json::from_str(text).unwrap();
    assert_eq!(arr.as_array().unwrap().len(), 1);
    assert_eq!(arr[0]["path"], "a.txt");
    assert_eq!(arr[0]["line"], 1);
}

#[test]
fn search_picks_tool_only_picked_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("picked.txt"), "TARGET in picked\n").unwrap();
    std::fs::write(root.join("unpicked.txt"), "TARGET in unpicked\n").unwrap();
    let ctx = context::SpycContext {
        cwd: root.to_path_buf(),
        cursor_file: None,
        // Picks stored as relative paths in spyc's UI; resolved
        // against cwd by the tool.
        picks: vec![PathBuf::from("picked.txt")],
        inventory: vec![],
        filter: None,
        git_branch: None,
        project_home: Some(root.to_path_buf()),
        search_root: Some(root.to_path_buf()),
        session_name: String::new(),
        pid: 0,
        version: String::new(),
    };
    let ctx_path = context::context_path(tmp.path());
    context::write_context_file(&ctx_path, &ctx).unwrap();

    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc":"2.0","id":9,"method":"tools/call",
                "params":{"name":"search_picks","arguments":{"pattern":"TARGET"}}})
        .to_string(),
        &ctx_path,
        None,
    )
    .unwrap();
    let resp = parse_response(&output);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let arr: Value = serde_json::from_str(text).unwrap();
    let paths: Vec<&str> = arr
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v["path"].as_str())
        .collect();
    assert!(paths.contains(&"picked.txt"));
    assert!(
        !paths.iter().any(|p| p.contains("unpicked")),
        "unpicked file leaked into results: {paths:?}"
    );
}

#[test]
fn read_lsp_message_parses() {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    let framed = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    let mut reader = Cursor::new(framed.as_bytes().to_vec());
    let msg = read_lsp_message(&mut reader).unwrap();
    assert_eq!(msg, body);
}

#[test]
fn socket_server_responds() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = context::SpycContext {
        cwd: PathBuf::from("/test"),
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
    };
    let ctx_path = context::context_path(tmp.path());
    context::write_context_file(&ctx_path, &ctx).unwrap();

    // Use a temp socket path to avoid interfering with a running instance.
    let sock_path = tmp.path().join("test-mcp.sock");
    let listener = bind_test_socket(&sock_path);

    let (cmd_tx, _cmd_rx) = std::sync::mpsc::channel();
    let ctx_for_thread = ctx_path;
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        handle_socket_connection(stream, &ctx_for_thread, &cmd_tx).unwrap_or(());
    });

    // Give the server thread a moment to start.
    std::thread::sleep(std::time::Duration::from_millis(50));

    let stream = UnixStream::connect(&sock_path).unwrap();
    let mut reader = io::BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;

    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}).to_string();
    send_message(&mut writer, &body).unwrap();

    let response = read_lsp_message(&mut reader).unwrap();
    assert!(response.contains("get_spyc_context"));
}

#[test]
fn disconnect_notification_routes_through_channel() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = context::SpycContext {
        cwd: PathBuf::from("/test"),
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
    };
    let ctx_path = context::context_path(tmp.path());
    context::write_context_file(&ctx_path, &ctx).unwrap();

    let sock_path = tmp.path().join("test-disconnect.sock");
    let listener = bind_test_socket(&sock_path);

    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let ctx_for_thread = ctx_path;
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        handle_socket_connection(stream, &ctx_for_thread, &cmd_tx).unwrap_or(());
    });

    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut stream = UnixStream::connect(&sock_path).unwrap();
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "spyc/disconnected",
        "params": { "new_pid": 99999 }
    });
    send_message(&mut stream, &notification.to_string()).unwrap();
    drop(stream); // close connection so handler exits

    let req = cmd_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    match req.command {
        McpCommand::Disconnected { new_pid } => assert_eq!(new_pid, 99999),
        other => panic!("expected Disconnected, got {other:?}"),
    }
}

#[test]
fn pid_from_sock_path_parses() {
    assert_eq!(
        pid_from_sock_path("/home/user/.local/state/spyc/mcp-12345.sock"),
        Some(12345)
    );
    assert_eq!(pid_from_sock_path("mcp-1.sock"), Some(1));
    assert_eq!(pid_from_sock_path("mcp.sock"), None);
    assert_eq!(pid_from_sock_path("mcp-.sock"), None);
}

#[test]
fn socket_path_contains_pid() {
    let path = socket_path();
    let pid = std::process::id();
    assert!(path.to_string_lossy().contains(&format!("mcp-{pid}.sock")));
}

// ── Project-scoped discovery ──────────────────────────────

fn touch_context(dir: &Path, pid: u32) {
    std::fs::write(dir.join(format!(".spyc-context-{pid}.json")), b"{}").unwrap();
}

/// Write the trusted-root sidecar attesting `pid` is rooted at `root`,
/// mirroring what a live spyc writes next to its socket. Without this a
/// marker is untrusted, so the genuine-discovery tests must lay one down.
fn touch_root_marker(state_dir: &Path, pid: u32, root: &Path) {
    std::fs::create_dir_all(state_dir).unwrap();
    let canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    std::fs::write(
        state_dir.join(format!("mcp-{pid}.root")),
        canon.to_string_lossy().as_bytes(),
    )
    .unwrap();
}

#[test]
fn read_context_pids_finds_markers() {
    let tmp = tempfile::tempdir().unwrap();
    touch_context(tmp.path(), 100);
    touch_context(tmp.path(), 200);
    // Decoy: not our prefix.
    std::fs::write(tmp.path().join("not-spyc-300.json"), "{}").unwrap();
    // Decoy: malformed PID.
    std::fs::write(tmp.path().join(".spyc-context-abc.json"), "{}").unwrap();
    let mut pids = read_context_pids_in_dir(tmp.path());
    pids.sort_unstable();
    assert_eq!(pids, vec![100, 200]);
}

#[test]
fn read_context_pids_empty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(read_context_pids_in_dir(tmp.path()).is_empty());
}

#[test]
fn collect_pids_finds_marker_in_caller_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    touch_context(tmp.path(), 42);
    touch_root_marker(state.path(), 42, tmp.path());
    assert_eq!(collect_project_pids_in(tmp.path(), state.path()), vec![42]);
}

#[test]
fn collect_pids_walks_up_to_ancestor_marker() {
    // spyc started at /tmp/.../proj; claude in /tmp/.../proj/src/sub.
    let tmp = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    let sub = proj.join("src").join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    touch_context(&proj, 7);
    touch_root_marker(state.path(), 7, &proj);
    assert_eq!(collect_project_pids_in(&sub, state.path()), vec![7]);
}

#[test]
fn collect_pids_first_ancestor_with_match_wins() {
    // A spyc at /proj and another at /proj/inner. Caller in
    // /proj/inner should NOT see /proj's spyc — locality
    // beats inheritance.
    let tmp = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    let inner = proj.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    touch_context(&proj, 1);
    touch_root_marker(state.path(), 1, &proj);
    touch_context(&inner, 2);
    touch_root_marker(state.path(), 2, &inner);
    assert_eq!(collect_project_pids_in(&inner, state.path()), vec![2]);
}

#[test]
fn collect_pids_returns_all_pids_at_same_dir() {
    // Two spyc instances rooted at the same dir → both candidates.
    let tmp = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    touch_context(tmp.path(), 100);
    touch_root_marker(state.path(), 100, tmp.path());
    touch_context(tmp.path(), 200);
    touch_root_marker(state.path(), 200, tmp.path());
    let mut pids = collect_project_pids_in(tmp.path(), state.path());
    pids.sort_unstable();
    assert_eq!(pids, vec![100, 200]);
}

#[test]
fn collect_pids_no_match_returns_empty() {
    // Cross-project case: caller's tree has no .spyc-context-*.json.
    // Sibling dir does, but that's deliberately invisible.
    let tmp = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let project_a = tmp.path().join("a");
    let project_b = tmp.path().join("b");
    std::fs::create_dir_all(&project_a).unwrap();
    std::fs::create_dir_all(&project_b).unwrap();
    touch_context(&project_b, 99);
    touch_root_marker(state.path(), 99, &project_b);
    // Walking up from project_a hits tmp.path() (no marker), then
    // ancestors of the temp dir which we can't predict — but the
    // test passes as long as none of THOSE happen to have a
    // spyc-context file. In CI / typical dev machines they won't.
    // To make the test deterministic we anchor at project_a only.
    let pids = collect_project_pids_in(&project_a, state.path());
    assert!(
        !pids.contains(&99),
        "must not pick up sibling project's spyc"
    );
}

#[test]
fn collect_pids_rejects_planted_marker_rooted_elsewhere() {
    // The attack: a malicious repo ships a `.spyc-context-<pid>.json`
    // whose pid is really a victim spyc rooted in a DIFFERENT project.
    // The trusted sidecar (which the attacker can't write — it lives in
    // the victim's private state dir) records that other root, so the
    // planted marker is refused. No cross-project attachment.
    let clone = tempfile::tempdir().unwrap(); // the cloned hostile repo
    let real = tempfile::tempdir().unwrap(); // victim's real project
    let state = tempfile::tempdir().unwrap();
    touch_context(clone.path(), 4242); // planted marker in the clone
    touch_root_marker(state.path(), 4242, real.path()); // genuine root ≠ clone
    assert!(
        collect_project_pids_in(clone.path(), state.path()).is_empty(),
        "a marker rooted elsewhere must be refused"
    );
}

#[test]
fn collect_pids_requires_a_trusted_root_sidecar() {
    // A marker with no sidecar at all (a stray or planted file with no
    // matching live spyc) is untrusted — fail safe, don't attach.
    let tmp = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    touch_context(tmp.path(), 55);
    assert!(
        collect_project_pids_in(tmp.path(), state.path()).is_empty(),
        "a marker without a trusted-root sidecar must be refused"
    );
}

#[test]
fn discover_live_socket_returns_none_without_project_marker() {
    let tmp = tempfile::tempdir().unwrap();
    // No .spyc-context-*.json in the caller's tree → discovery
    // must NOT fall through to scanning every socket on the
    // host (the cross-project bug we're fixing).
    assert!(discover_live_socket(tmp.path()).is_none());
}

#[test]
fn get_file_content_blocks_path_traversal() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    // Create a file outside the project directory.
    let secret = tmp.path().join("secret.txt");
    std::fs::write(&secret, "top secret").unwrap();

    // Create a file inside the project directory.
    std::fs::write(project.join("ok.txt"), "public").unwrap();

    // Set up context with cwd = project.
    let ctx = context::SpycContext {
        cwd: project,
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
    };
    let ctx_path = context::context_path(tmp.path());
    context::write_context_file(&ctx_path, &ctx).unwrap();

    // Reading a file inside cwd should succeed.
    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
            "name":"get_file_content","arguments":{"path":"ok.txt"}
        }})
        .to_string(),
        &ctx_path,
        None,
    )
    .unwrap();
    let resp = parse_response(&output);
    assert!(
        resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("public")
    );

    // Traversal via ../secret.txt should be blocked.
    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name":"get_file_content","arguments":{"path":"../secret.txt"}
        }})
        .to_string(),
        &ctx_path,
        None,
    )
    .unwrap();
    let resp = parse_response(&output);
    assert!(
        resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("outside the project root")
    );
}

#[test]
fn get_file_content_resolves_relative_against_search_root() {
    // search_root (the repo root) differs from cwd (a subdir). A relative path
    // as `search_paths`/`search_content` return them (repo-root-relative) must
    // resolve against the search root, not cwd — so search results read back.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(root.join("top.txt"), "from-root").unwrap();
    let ctx = context::SpycContext {
        cwd: sub,
        cursor_file: None,
        picks: vec![],
        inventory: vec![],
        filter: None,
        git_branch: None,
        project_home: None,
        search_root: Some(root),
        session_name: String::new(),
        pid: 0,
        version: String::new(),
    };
    let ctx_path = context::context_path(tmp.path());
    context::write_context_file(&ctx_path, &ctx).unwrap();
    let mut output = Vec::new();
    dispatch(
        &mut output,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
            "name":"get_file_content","arguments":{"path":"top.txt"}
        }})
        .to_string(),
        &ctx_path,
        None,
    )
    .unwrap();
    let resp = parse_response(&output);
    assert!(
        resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("from-root"),
        "search-root-relative path should resolve against search_root, not cwd"
    );
}

// ---- ensure_codex_config_toml ------------------------------------

#[test]
fn codex_config_writes_fresh_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let status = ensure_codex_config_toml(tmp.path(), true).unwrap();
    assert!(matches!(status, McpConfigStatus::Configured));
    let written = std::fs::read_to_string(tmp.path().join(".codex").join("config.toml"))
        .expect("config.toml created");
    let parsed: toml::Value = toml::from_str(&written).unwrap();
    // Schema check: mcp_servers.spyc.{command,args,env.SPYC_MCP_SOCK}
    let spyc = &parsed["mcp_servers"]["spyc"];
    assert!(spyc["command"].as_str().unwrap_or("").contains("spyc"));
    assert_eq!(spyc["args"][0].as_str(), Some("--mcp"));
    assert!(
        spyc["env"]["SPYC_MCP_SOCK"]
            .as_str()
            .unwrap_or("")
            .contains("mcp-")
    );
}

#[test]
fn codex_config_preserves_other_servers() {
    // A pre-existing entry from another tool must survive the splice.
    let tmp = tempfile::tempdir().unwrap();
    let codex_dir = tmp.path().join(".codex");
    std::fs::create_dir_all(&codex_dir).unwrap();
    let path = codex_dir.join("config.toml");
    std::fs::write(
        &path,
        r#"
[mcp_servers.other]
command = "/usr/local/bin/other-mcp"
args = ["--stdio"]

[mcp_servers.other.env]
KEY = "val"
"#,
    )
    .unwrap();
    ensure_codex_config_toml(tmp.path(), true).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    let parsed: toml::Value = toml::from_str(&written).unwrap();
    // Both servers present.
    assert!(parsed["mcp_servers"].get("other").is_some());
    assert!(parsed["mcp_servers"].get("spyc").is_some());
    assert_eq!(
        parsed["mcp_servers"]["other"]["command"].as_str(),
        Some("/usr/local/bin/other-mcp")
    );
}

#[test]
fn codex_config_fresh_rewrite_on_malformed_input() {
    // Top-level array (not a table) must not panic — mirror the
    // mcp.json shape-check fix from v1.41.5.
    let tmp = tempfile::tempdir().unwrap();
    let codex_dir = tmp.path().join(".codex");
    std::fs::create_dir_all(&codex_dir).unwrap();
    let path = codex_dir.join("config.toml");
    std::fs::write(&path, "not_a_section = 1\nrandom = \"junk\"\n").unwrap();
    // Don't crash; either splice into the (now-empty) file or rewrite.
    let status = ensure_codex_config_toml(tmp.path(), true).unwrap();
    assert!(matches!(status, McpConfigStatus::Configured));
    let written = std::fs::read_to_string(&path).unwrap();
    let parsed: toml::Value = toml::from_str(&written).unwrap();
    assert!(parsed["mcp_servers"]["spyc"].is_table());
}

#[test]
fn codex_config_rewrites_completely_invalid_toml() {
    // Non-TOML content (e.g. corrupted file) falls back to a fresh
    // rewrite rather than failing.
    let tmp = tempfile::tempdir().unwrap();
    let codex_dir = tmp.path().join(".codex");
    std::fs::create_dir_all(&codex_dir).unwrap();
    let path = codex_dir.join("config.toml");
    std::fs::write(&path, "}}}}{{{ this is not toml").unwrap();
    let status = ensure_codex_config_toml(tmp.path(), true).unwrap();
    assert!(matches!(status, McpConfigStatus::Configured));
    let written = std::fs::read_to_string(&path).unwrap();
    let parsed: toml::Value = toml::from_str(&written).unwrap();
    assert!(parsed["mcp_servers"]["spyc"].is_table());
}

// ── socket-bind diagnostics (testing campaign, cluster 7) ──
// Make a sandboxed bind failure interpretable instead of an opaque
// "Operation not permitted" panic — without weakening the full-perms run
// (under normal permissions these still bind and exercise the real socket).

/// Bind a Unix socket for a roundtrip test, panicking with a clear message
/// (not a bare OS error) if a restricted sandbox refuses the bind.
fn bind_test_socket(path: &std::path::Path) -> UnixListener {
    UnixListener::bind(path).unwrap_or_else(|e| {
        panic!(
            "MCP socket test needs a real Unix socket; bind at {} failed ({e}). \
             If this is a restricted sandbox, rerun under normal permissions.",
            path.display()
        )
    })
}

/// A permission-denied bind is reported as a sandbox hint pointing at
/// rerunning under normal permissions, carrying the socket path.
#[test]
fn socket_bind_error_permission_denied_points_to_rerun() {
    let err = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    let msg =
        super::server::socket_bind_error(err, std::path::Path::new("/run/spyc-x.sock")).to_string();
    assert!(msg.contains("rerun under normal permissions"), "got: {msg}");
    assert!(msg.contains("/run/spyc-x.sock"), "got: {msg}");
}

/// A non-permission bind error keeps a plain path context (no misleading
/// sandbox hint).
#[test]
fn socket_bind_error_other_is_plain_context() {
    let err = std::io::Error::from(std::io::ErrorKind::AddrInUse);
    let msg =
        super::server::socket_bind_error(err, std::path::Path::new("/run/spyc-x.sock")).to_string();
    assert!(msg.contains("binding MCP socket"), "got: {msg}");
    assert!(
        !msg.contains("rerun under normal permissions"),
        "must not mislabel a non-permission error as a sandbox: {msg}"
    );
}
