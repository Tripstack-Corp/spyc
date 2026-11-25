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
fn an_upgrade_with_a_refused_token_never_reaches_the_loop() {
    let msg = json!({"jsonrpc":"2.0", "method":"spyc/upgrade", "params":{"token":"nope"}});
    let got = heard_for(&msg);
    assert!(
        !got.iter().any(|c| c == "Upgrade"),
        "told to upgrade: {got:?}"
    );
}

#[test]
fn a_shutdown_with_a_refused_token_never_reaches_the_loop() {
    let msg = json!({"jsonrpc":"2.0", "id":1, "method":"spyc/shutdown", "params":{"token":"nope"}});
    let got = heard_for(&msg);
    assert!(
        !got.iter().any(|c| c == "Shutdown"),
        "told to shut down: {got:?}"
    );
}

#[test]
fn a_release_with_a_refused_token_never_reaches_the_loop() {
    let msg = json!({"jsonrpc":"2.0", "id":1, "method":"spyc/release", "params":{"token":"nope"}});
    let got = heard_for(&msg);
    assert!(
        !got.iter().any(|c| c == "Release"),
        "told to release: {got:?}"
    );
}

/// What `dispatch` answered `msg` with, given `loop_answer` as the loop's
/// reply to everything it is asked.
fn answer_to(msg: &Value, loop_answer: impl Fn(&str) -> McpResponse + Send + 'static) -> Value {
    let (sender, receiver) = mpsc::channel::<McpRequest>();
    std::thread::spawn(move || {
        for request in receiver {
            let said = format!("{:?}", request.command);
            let _ = request.reply.send(loop_answer(&said));
        }
    });
    let mut output = Vec::new();
    dispatch(
        &mut output,
        &msg.to_string(),
        Path::new("/tmp"),
        Some(&sender),
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    let body = output.split("\r\n\r\n").nth(1).unwrap_or("null");
    serde_json::from_str(body).unwrap_or(Value::Null)
}

/// `spyc/upgrade` is a request (H7): one whose token is refused is answered
/// with why, so `spyc attach` hears the refusal rather than nothing.
#[test]
fn an_upgrade_request_with_a_refused_token_is_answered_why() {
    let msg = json!({"jsonrpc":"2.0", "id":1, "method":"spyc/upgrade", "params":{"token":"nope"}});
    let reply = answer_to(&msg, |said| {
        if said.starts_with("Authorize") {
            McpResponse::Error {
                message: "refused spyc/upgrade: its token isn't this session's".into(),
            }
        } else {
            McpResponse::Ok {
                message: "{}".into(),
            }
        }
    });
    let why = reply["error"]["message"].as_str().unwrap_or("");
    assert!(why.contains("token"), "{reply}");
}

/// One the loop refuses for want of what an upgrade needs is answered with
/// the loop's reason.
#[test]
fn an_upgrade_request_the_loop_refuses_is_answered_with_its_reason() {
    let (_dir, socket) = jar_with_token(b"tok");
    let named = json!({ "socket": socket }).to_string();
    let msg = json!({"jsonrpc":"2.0", "id":1, "method":"spyc/upgrade", "params":{"token":"tok"}});
    let reply = answer_to(&msg, move |said| {
        if said.starts_with("Authorize") {
            McpResponse::Ok {
                message: named.clone(),
            }
        } else if said == "Upgrade" {
            McpResponse::Error {
                message: "no newer spyc is installed".into(),
            }
        } else {
            McpResponse::Ok {
                message: "{}".into(),
            }
        }
    });
    let why = reply["error"]["message"].as_str().unwrap_or("");
    assert!(why.contains("no newer spyc"), "{reply}");
}

/// One the loop accepts is answered `{"upgrading": true}`.
#[test]
fn an_upgrade_request_the_loop_accepts_is_answered_upgrading() {
    let (_dir, socket) = jar_with_token(b"tok");
    let named = json!({ "socket": socket }).to_string();
    let msg = json!({"jsonrpc":"2.0", "id":1, "method":"spyc/upgrade", "params":{"token":"tok"}});
    let reply = answer_to(&msg, move |said| McpResponse::Ok {
        message: if said.starts_with("Authorize") {
            named.clone()
        } else if said == "Upgrade" {
            r#"{"upgrading": true}"#.into()
        } else {
            "{}".into()
        },
    });
    assert_eq!(reply["result"]["upgrading"], json!(true), "{reply}");
}

/// A session jar of the test's own, at `<tmp>/spyc/sessions/T.sock` as spyc
/// lays them out, that welcomes a connection whose token is `token` and
/// refuses any other, as a jar does.
fn jar_with_token(
    token: &'static [u8],
) -> (crate::pane::jar::test_dir::TestDir, std::path::PathBuf) {
    use spyc_jar::proto::{self, Msg};
    use std::os::unix::fs::DirBuilderExt;
    let dir = crate::pane::jar::test_dir::TestDir::new("ct");
    let sessions = dir.path().join("spyc").join("sessions");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&sessions)
        .unwrap();
    let socket = sessions.join("T.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { return };
            let Ok(Some(Msg::Hello { token: got, .. })) = proto::read_msg(&mut conn) else {
                continue;
            };
            let answer = if got == token {
                Msg::Welcome {
                    version: proto::VERSION,
                    child_pid: std::process::id(),
                    head: 0,
                    base: 0,
                    rows: 24,
                    cols: 80,
                }
            } else {
                Msg::Refused {
                    reason: 3,
                    why: "bad token".into(),
                }
            };
            let _ = proto::write_msg(&mut conn, &answer);
        }
    });
    (dir, socket)
}

/// The jar, not the loop, decides a token (the refuter on E1): one the jar
/// refuses is refused, though the loop named the jar, and the loop is told.
#[test]
fn a_token_the_jar_refuses_is_refused_and_the_loop_told() {
    let (_dir, socket) = jar_with_token(b"the-jars-own-token");
    let named = json!({ "socket": socket }).to_string();
    let (told_tx, told) = mpsc::channel::<String>();
    let msg = json!({"jsonrpc":"2.0", "id":1, "method":"spyc/shutdown", "params":{"token":"written-in-the-file"}});
    let reply = answer_to(&msg, move |said| {
        let _ = told_tx.send(said.to_string());
        McpResponse::Ok {
            message: if said.starts_with("Authorize") {
                named.clone()
            } else {
                "{}".into()
            },
        }
    });
    assert!(reply.get("error").is_some(), "{reply}");
    let heard: Vec<String> = told.try_iter().collect();
    assert!(!heard.iter().any(|c| c == "Shutdown"), "{heard:?}");
    assert!(
        heard.iter().any(|c| c.contains("refused: Some")),
        "the loop wasn't told: {heard:?}"
    );
}
