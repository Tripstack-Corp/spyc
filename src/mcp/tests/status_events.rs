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
