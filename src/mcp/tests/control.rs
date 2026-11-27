//! The control requests on spyc's MCP socket reach the loop only once it has
//! said the token they carry is the session's (the 3.0 review, E1). Driven
//! through `dispatch` with a loop of the test's own, which refuses every
//! token, and a marker request sent after each, so what the loop heard
//! before the marker is everything the request asked of it.

use super::*;
use crate::mcp_cmd::{McpRequest, McpResponse};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

/// A loop that refuses any authorization and says what it heard.
fn refusing_loop() -> (mpsc::Sender<McpRequest>, mpsc::Receiver<String>) {
    let (sender, receiver) = mpsc::channel::<McpRequest>();
    let (heard_tx, heard) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for request in receiver {
            let said = format!("{:?}", request.command);
            let reply = if said.starts_with("Authorize") {
                McpResponse::Error {
                    message: "refused: its token isn't this session's".into(),
                }
            } else {
                McpResponse::Ok {
                    message: "{}".into(),
                }
            };
            let _ = heard_tx.send(said);
            let _ = request.reply.send(reply);
        }
    });
    (sender, heard)
}

/// What the loop heard until the marker the test sends after `msg`.
fn heard_for(msg: &Value) -> Vec<String> {
    let (sender, heard) = refusing_loop();
    let mut output = Vec::new();
    dispatch(
        &mut output,
        &msg.to_string(),
        Path::new("/tmp"),
        Some(&sender),
    )
    .unwrap();
    sender
        .send(McpRequest {
            command: McpCommand::Behind,
            reply: mpsc::channel().0,
            project: None,
            pane_id: None,
            conn: None,
        })
        .unwrap();
    let mut got = Vec::new();
    loop {
        let one = heard.recv_timeout(Duration::from_secs(5)).unwrap();
        if one == "Behind" {
            return got;
        }
        got.push(one);
    }
}

#[test]
#[ignore = "red until E1: fix/control-token"]
fn an_upgrade_with_a_refused_token_never_reaches_the_loop() {
    let msg = json!({"jsonrpc":"2.0", "method":"spyc/upgrade", "params":{"token":"nope"}});
    let got = heard_for(&msg);
    assert!(
        !got.iter().any(|c| c == "Upgrade"),
        "told to upgrade: {got:?}"
    );
}

#[test]
#[ignore = "red until E1: fix/control-token"]
fn a_shutdown_with_a_refused_token_never_reaches_the_loop() {
    let msg = json!({"jsonrpc":"2.0", "id":1, "method":"spyc/shutdown", "params":{"token":"nope"}});
    let got = heard_for(&msg);
    assert!(
        !got.iter().any(|c| c == "Shutdown"),
        "told to shut down: {got:?}"
    );
}

#[test]
#[ignore = "red until E1: fix/control-token"]
fn a_release_with_a_refused_token_never_reaches_the_loop() {
    let msg = json!({"jsonrpc":"2.0", "id":1, "method":"spyc/release", "params":{"token":"nope"}});
    let got = heard_for(&msg);
    assert!(
        !got.iter().any(|c| c == "Release"),
        "told to release: {got:?}"
    );
}
