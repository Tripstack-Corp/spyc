use super::*;
use crate::mcp_cmd::{McpRequest, McpResponse};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn status_hook_metadata_is_validated_at_socket_dispatch_without_rejecting_legacy_reports() {
    let (sender, receiver) = mpsc::channel::<McpRequest>();
    let (seen_sender, seen_receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        for request in receiver {
            if let McpCommand::ReportStatus {
                pane_id,
                status,
                hook_event,
                ..
            } = &request.command
            {
                seen_sender
                    .send((pane_id.clone(), status.clone(), hook_event.clone()))
                    .unwrap();
            }
            let _ = request.reply.send(McpResponse::Ok {
                message: "ok".into(),
            });
        }
    });
    for (metadata, expected) in [
        (
            json!({"hook_event_name":"PermissionRequest", "tool_name":"Bash", "turn_id":"turn-1", "tool_input":"private-command"}),
            Some("PermissionRequest"),
        ),
        (json!({"hook_event_name":"Stop\u{1b}[31m"}), None),
        (json!(null), None),
    ] {
        let request = json!({"jsonrpc":"2.0", "id":1, "method":"tools/call",
        "params":{"name":"report_status", "arguments":{
            "status":"blocked", "pane_id":"pane-1", "hook_event":metadata
        }}});
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &request.to_string(),
            Path::new("/tmp"),
            Some(&sender),
        )
        .unwrap();
        assert_ne!(parse_response(&output)["result"]["isError"], json!(true));
        let (pane, status, event) = seen_receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(pane.as_deref(), Some("pane-1"));
        assert_eq!(status, "blocked");
        assert_eq!(
            event.as_ref().map(|event| event.hook_event_name.as_str()),
            expected
        );
        assert!(
            !serde_json::to_string(&event)
                .unwrap()
                .contains("private-command")
        );
    }
    drop(sender);
    worker.join().unwrap();
}

#[test]
fn correlated_question_reports_cross_the_protocol_boundary() {
    let (sender, receiver) = mpsc::channel::<McpRequest>();
    let worker = std::thread::spawn(move || {
        for (signal, event_name) in [
            ("codex-question-start", "PreToolUse"),
            ("codex-question-end", "PostToolUse"),
        ] {
            let request = loop {
                let request = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
                if matches!(&request.command, McpCommand::ReportStatus { .. }) {
                    break request;
                }
                let _ = request.reply.send(McpResponse::Ok {
                    message: "ok".into(),
                });
            };
            let McpCommand::ReportStatus {
                pane_id,
                status,
                session_id,
                hook_event,
                ..
            } = request.command
            else {
                panic!("expected question report")
            };
            assert_eq!(status, signal);
            assert_eq!(pane_id.as_deref(), Some("pane-1"));
            assert_eq!(session_id.as_deref(), Some("session-1"));
            let event = hook_event.unwrap();
            assert_eq!(event.hook_event_name, event_name);
            assert_eq!(event.tool_name.as_deref(), Some("request_user_input"));
            assert_eq!(event.turn_id.as_deref(), Some("turn-1"));
            assert_eq!(event.tool_use_id.as_deref(), Some("call-1"));
            request
                .reply
                .send(McpResponse::Ok {
                    message: "ok".into(),
                })
                .unwrap();
        }
    });
    for (signal, event_name) in [
        ("codex-question-start", "PreToolUse"),
        ("codex-question-end", "PostToolUse"),
    ] {
        let request = json!({"jsonrpc":"2.0", "id":1, "method":"tools/call",
        "params":{"name":"report_status", "arguments":{
            "status":signal, "pane_id":"pane-1", "session_id":"session-1",
            "hook_event":{"hook_event_name":event_name,
                "tool_name":"request_user_input", "turn_id":"turn-1",
                "tool_use_id":"call-1"}
        }}});
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &request.to_string(),
            Path::new("/tmp"),
            Some(&sender),
        )
        .unwrap();
        assert_ne!(
            parse_response(&output)["result"]["isError"],
            json!(true),
            "question lifecycle must reach the App: {}",
            String::from_utf8_lossy(&output)
        );
    }
    drop(sender);
    worker.join().unwrap();
}

/// The schema an agent reads, what the socket accepts and what the loop applies
/// are one vocabulary: every reportable state is advertised and accepted, a
/// note reaches the loop already cleaned, and `unknown` (a dot state, not a
/// report) is refused.
#[test]
fn report_status_accepts_waiting_with_a_cleaned_note() {
    let tools = crate::mcp::tool_schemas::tools();
    let schema = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "report_status")
        .unwrap();
    let advertised: Vec<&str> = schema["inputSchema"]["properties"]["status"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|state| state.as_str().unwrap())
        .collect();
    let reportable = |name: &str| crate::pane::AgentActivity::reportable(name).is_some();
    assert!(
        advertised.iter().all(|name| reportable(name)),
        "{advertised:?}"
    );
    for state in [
        crate::pane::AgentActivity::Working,
        crate::pane::AgentActivity::Idle,
        crate::pane::AgentActivity::Blocked,
        crate::pane::AgentActivity::Done,
        crate::pane::AgentActivity::Waiting,
    ] {
        assert!(
            advertised.contains(&state.name()),
            "{state:?} not advertised"
        );
    }

    let (sender, receiver) = mpsc::channel::<McpRequest>();
    let (seen_sender, seen_receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        for request in receiver {
            if let McpCommand::ReportStatus { status, note, .. } = &request.command {
                seen_sender.send((status.clone(), note.clone())).unwrap();
            }
            let _ = request.reply.send(McpResponse::Ok {
                message: "ok".into(),
            });
        }
    });
    let call = |arguments: Value| {
        let request = json!({"jsonrpc":"2.0", "id":1, "method":"tools/call",
            "params":{"name":"report_status", "arguments":arguments}});
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &request.to_string(),
            Path::new("/tmp"),
            Some(&sender),
        )
        .unwrap();
        parse_response(&output)
    };

    let accepted = call(json!({"status":"waiting", "note":"CI on\n#641\u{1b}[2J"}));
    assert_ne!(accepted["result"]["isError"], json!(true), "{accepted}");
    let (status, note) = seen_receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(status, "waiting");
    assert_eq!(note.as_deref(), Some("CI on #641 [2J"));

    let refused = call(json!({"status":"unknown"}));
    assert_eq!(refused["result"]["isError"], json!(true), "{refused}");
    assert!(
        seen_receiver.try_recv().is_err(),
        "a refused report reached the loop"
    );
    drop(sender);
    worker.join().unwrap();
}

/// The statusline forwarder's method: a report reaches the loop held to the
/// projection's bounds, the reply comes at once, and no agent can list it.
#[test]
fn a_statusline_report_reaches_the_loop_bounded_and_unlisted() {
    let (sender, receiver) = mpsc::channel::<McpRequest>();
    let send = |params: Value| {
        let request = json!({"jsonrpc":"2.0", "id":7, "method":"spyc/statusline", "params":params});
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &request.to_string(),
            Path::new("/tmp"),
            Some(&sender),
        )
        .unwrap();
        parse_response(&output)
    };
    let reply = send(json!({"pane_id":"pane-1", "statusline":{
        "context":{"size":1_000_000, "used_percent":34},
        "five_hour":{"used_percent":250, "resets_at":5},
        "seven_day":{"used_percent":61, "resets_at":1_791_900_000}
    }}));
    assert_eq!(reply["result"], json!({}));
    match receiver.try_recv().unwrap().command {
        McpCommand::Statusline {
            pane_id,
            statusline,
        } => {
            assert_eq!(pane_id, "pane-1");
            assert_eq!(statusline.context.unwrap().used_percent, Some(34));
            assert_eq!(statusline.five_hour, None, "out of range is dropped");
            assert_eq!(statusline.seven_day.unwrap().used_percent, 61);
        }
        other => panic!("{other:?}"),
    }
    for params in [
        json!({"pane_id":"pane-1", "statusline":{}}),
        json!({"statusline":{"context":{"size":5, "used_percent":1}}}),
        json!({"pane_id":"pane-1", "statusline":"not an object"}),
        json!({"pane_id":"pane-1", "statusline":{"vim_mode":"REPLACE"}}),
    ] {
        assert_eq!(send(params.clone())["result"], json!({}), "{params}");
        assert!(receiver.try_recv().is_err(), "{params} is never queued");
    }
    send(json!({"pane_id":"pane-1", "statusline":{"vim_mode":"NORMAL"}}));
    match receiver.try_recv().unwrap().command {
        McpCommand::Statusline { statusline, .. } => assert_eq!(
            statusline.vim_mode,
            Some(crate::agent::claude_statusline::VimMode::Normal),
            "a mode alone is a report"
        ),
        other => panic!("{other:?}"),
    }
    let mut output = Vec::new();
    let list = json!({"jsonrpc":"2.0", "id":8, "method":"tools/list"});
    dispatch(
        &mut output,
        &list.to_string(),
        Path::new("/tmp"),
        Some(&sender),
    )
    .unwrap();
    assert!(!String::from_utf8(output).unwrap().contains("statusline"));
}

/// `spyc shutdown`'s request: a spyc that knows it asks its loop whether the
/// token it carries is the session's (the 3.0 review, E1) and, told yes,
/// answers and tells the loop; told no, answers why and tells it nothing. One
/// that predates it answers "Method not found", and the command says that
/// rather than waiting for a quit that never comes.
#[test]
fn a_shutdown_is_answered_and_an_old_spyc_says_it_cannot() {
    let (sender, receiver) = mpsc::channel::<McpRequest>();
    // The loop: it lets the session's token through and says what it heard.
    let (heard_tx, heard) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for request in receiver {
            let reply = match &request.command {
                McpCommand::Authorize { token, .. } if token.as_deref() == Some("tok") => {
                    McpResponse::Ok {
                        message: "{}".into(),
                    }
                }
                McpCommand::Authorize { .. } => McpResponse::Error {
                    message: "refused: its token isn't this session's".into(),
                },
                _ => McpResponse::Ok {
                    message: "{}".into(),
                },
            };
            let _ = heard_tx.send(format!("{:?}", request.command));
            let _ = request.reply.send(reply);
        }
    });
    let serve_once = |old: bool, token: &[u8]| {
        let dir = tempfile::Builder::new()
            .prefix("sd")
            .tempdir_in("/tmp")
            .unwrap();
        let sock = dir.path().join("s.sock");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let sender = sender.clone();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let request = super::super::framing::read_lsp_message(&mut reader).unwrap();
            let mut output = Vec::new();
            let request = if old {
                request.replace("spyc/shutdown", "spyc/not-yet")
            } else {
                request
            };
            dispatch(&mut output, &request, Path::new("/tmp"), Some(&sender)).unwrap();
            let mut writer = stream;
            std::io::Write::write_all(&mut writer, &output).unwrap();
        });
        let answered = super::super::request_shutdown(&sock, token);
        server.join().unwrap();
        answered
    };
    // Everything the loop heard up to a marker sent after the exchange.
    let heard_until_now = || {
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
    };
    serve_once(false, b"tok").unwrap();
    let got = heard_until_now();
    assert!(
        got.iter().any(|c| c == "Shutdown"),
        "the loop is told: {got:?}"
    );
    let refused = serve_once(false, b"nope").unwrap_err();
    assert!(format!("{refused:#}").contains("token"), "{refused:#}");
    let got = heard_until_now();
    assert!(
        !got.iter().any(|c| c == "Shutdown"),
        "told without the token: {got:?}"
    );
    let old = serve_once(true, b"tok").unwrap_err();
    assert!(format!("{old:#}").contains("predates"), "{old:#}");
    let got = heard_until_now();
    assert!(got.is_empty(), "an old spyc's loop hears nothing: {got:?}");
}
