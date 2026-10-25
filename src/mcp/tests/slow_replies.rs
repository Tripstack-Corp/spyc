//! A reply the server takes its time over still reaches the agent through the
//! stdio proxy, and the connection outlives it: `wait_for_scope_clear` blocks
//! for minutes by design, and a worktree removal can archive a large tree.
//! Only a server that never answers ends the connection.

use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::mcp_cmd::{McpRequest, McpResponse};

/// How long the proxy waits on the server here, in place of
/// [`PROXY_IO_TIMEOUT`]'s 20 s.
const IO_TIMEOUT: Duration = Duration::from_millis(200);
/// How long the fake loop takes over a slow command: well past [`IO_TIMEOUT`],
/// well inside the server's own budget for the command.
const SLOW: Duration = Duration::from_millis(800);
const PROJECT: &str = "0192f0e4-7c3a-7b2e-9a51-5d6e7f809a1b";
const CLEARED: &str = r#"{"outcome":"cleared","conflicts":[]}"#;

/// A stand-in for the main loop that answers every command at once, except
/// those `slow` picks, which it answers after [`SLOW`].
fn fake_loop(slow: fn(&McpCommand) -> bool) -> mpsc::Sender<McpRequest> {
    let (tx, rx) = mpsc::channel::<McpRequest>();
    std::thread::spawn(move || {
        for McpRequest { command, reply, .. } in rx {
            let message = match &command {
                McpCommand::Bind { .. } => json!({"pane": false, "project": PROJECT}).to_string(),
                McpCommand::WaitForScopeClear { .. } => CLEARED.to_owned(),
                McpCommand::ListScopes => "[]".to_owned(),
                _ => "ok".to_owned(),
            };
            let delay = if slow(&command) { SLOW } else { Duration::ZERO };
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                let _ = reply.send(McpResponse::Ok { message });
            });
        }
    });
    tx
}

fn call(id: u64, name: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
           "params": {"name": name, "arguments": arguments}})
}

/// The agent's `messages`, newline-delimited, through the real proxy into a
/// real connection served by `tx`'s loop: the replies the agent reads.
fn through_proxy(tx: mpsc::Sender<McpRequest>, messages: &[Value]) -> Vec<Value> {
    let tmp = tempfile::tempdir().unwrap();
    let ctx_path = tmp.path().join("context.json");
    let (agent_side, server_side) = UnixStream::pair().unwrap();
    std::thread::spawn(move || {
        handle_socket_connection(server_side, &ctx_path, &tx).unwrap_or(());
    });
    let initialize = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {}});
    let stdin: String = std::iter::once(&initialize)
        .chain(messages)
        .map(|m| m.to_string() + "\n")
        .collect();
    let mut stdout = Vec::new();
    crate::mcp::server::proxy_io(
        Cursor::new(stdin),
        &mut stdout,
        agent_side,
        None,
        None,
        IO_TIMEOUT,
    )
    .unwrap();
    std::str::from_utf8(&stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn text(reply: &Value) -> Option<&str> {
    reply["result"]["content"][0]["text"].as_str()
}

#[test]
fn a_scope_wait_longer_than_the_proxys_timeout_still_answers() {
    let tx = fake_loop(|c| matches!(c, McpCommand::WaitForScopeClear { .. }));
    let replies = through_proxy(
        tx,
        &[
            call(
                1,
                "wait_for_scope_clear",
                json!({"paths": ["src/**"], "timeout_ms": 5_000}),
            ),
            call(2, "list_scopes", json!({})),
        ],
    );
    assert_eq!(replies.len(), 3, "{replies:#?}");
    assert_eq!(text(&replies[1]), Some(CLEARED), "{replies:#?}");
    assert_eq!(
        text(&replies[2]),
        Some("[]"),
        "the connection ended: {replies:#?}"
    );
}

#[test]
fn a_slow_worktree_removal_still_answers() {
    let tx = fake_loop(|c| matches!(c, McpCommand::RemoveWorktree { .. }));
    let replies = through_proxy(
        tx,
        &[
            call(1, "remove_worktree", json!({"path": "/wt/feat"})),
            call(2, "list_scopes", json!({})),
        ],
    );
    assert_eq!(replies.len(), 3, "{replies:#?}");
    assert_eq!(text(&replies[1]), Some("ok"), "{replies:#?}");
    assert_eq!(
        text(&replies[2]),
        Some("[]"),
        "the connection ended: {replies:#?}"
    );
}

/// A server that never answers still can't hang the agent: the proxy answers
/// with an error after its timeout and ends.
#[test]
fn a_server_that_never_answers_ends_the_connection_with_an_error() {
    let (agent_side, server_side) = UnixStream::pair().unwrap();
    let silent = std::thread::spawn(move || {
        // Read whatever comes, answer nothing, until the proxy hangs up.
        let mut reader = io::BufReader::new(server_side);
        while read_lsp_message(&mut reader).is_ok() {}
    });
    let stdin = [
        json!({"jsonrpc": "2.0", "id": 7, "method": "ping"}),
        json!({"jsonrpc": "2.0", "id": 8, "method": "ping"}),
    ]
    .map(|m| m.to_string() + "\n")
    .concat();
    let mut stdout = Vec::new();
    let started = std::time::Instant::now();
    crate::mcp::server::proxy_io(
        Cursor::new(stdin),
        &mut stdout,
        agent_side,
        None,
        None,
        IO_TIMEOUT,
    )
    .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the proxy waited out the server"
    );
    let replies: Vec<Value> = std::str::from_utf8(&stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies.len(), 1, "{replies:#?}");
    assert_eq!(replies[0]["id"], 7);
    assert_eq!(replies[0]["error"]["code"], -32000);
    let _ = silent.join();
}
