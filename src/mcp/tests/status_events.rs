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
