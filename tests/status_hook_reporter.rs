#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

fn run_reporter(payload: &str) -> (Value, String) {
    run_reporter_for("blocked", payload)
}

fn run_reporter_for(state: &str, payload: &str) -> (Value, String) {
    let temp = tempfile::Builder::new()
        .prefix("spyc-hook-")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = temp.path().join("hook.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "reporter did not connect");
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept reporter: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut length = None;
        loop {
            let mut header = String::new();
            assert!(reader.read_line(&mut header).unwrap() > 0);
            if header == "\r\n" {
                break;
            }
            if let Some(value) = header.strip_prefix("Content-Length:") {
                length = Some(value.trim().parse::<usize>().unwrap());
            }
        }
        let length = length.unwrap();
        assert!(length < 16_384);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        reader.get_mut().write_all(b"{}\n").unwrap();
        serde_json::from_slice::<Value>(&body).unwrap()
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_spyc"))
        .args(["--report-status", state, "--status-trace"])
        .env("SPYC_MCP_SOCK", &socket)
        .env("SPYC_PANE_ID", "pane-1")
        .env("XDG_STATE_HOME", temp.path().join("state"))
        .env("HOME", temp.path())
        .env_remove("SPYC_MCP_DEBUG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("reporter did not exit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stdout.is_empty());
    let request = server.join().unwrap();
    let log = std::fs::read_to_string(temp.path().join("state/spyc/mcp.log")).unwrap();
    (request, log)
}

#[test]
fn status_hook_reporter_forwards_only_metadata_and_redacts_trace_at_the_call_site() {
    let payload = json!({
        "hook_event_name": "PermissionRequest", "session_id": "session-1",
        "turn_id": "turn-1", "tool_name": "Bash",
        "tool_input": {"command": "private-command"}, "prompt": "private-prompt",
        "tool_response": "private-output", "last_assistant_message": "private-response"
    });
    let (request, log) = run_reporter(&payload.to_string());
    let args = &request["params"]["arguments"];
    assert_eq!(
        args["hook_event"],
        json!({
            "hook_event_name": "PermissionRequest", "turn_id": "turn-1", "tool_name": "Bash"
        })
    );
    assert_eq!(args["status"], "blocked");
    assert_eq!(args["session_id"], "session-1");
    assert_eq!(args["pane_id"], "pane-1");
    assert!(log.contains("PermissionRequest"), "{log}");
    assert!(!log.contains("private-"), "trace leaked content: {log}");
    assert!(!request.to_string().contains("private-"));
}

#[test]
fn status_hook_reporter_trace_excludes_content_even_when_metadata_is_invalid() {
    let (_, log) = run_reporter("{private-partial");
    assert!(!log.contains("private-"), "trace leaked content: {log}");
}

#[test]
fn status_hook_reporter_preserves_legacy_remap_and_malformed_payload_fallback() {
    let (idle, _) =
        run_reporter(r#"{"hook_event_name":"Notification","notification_type":"idle_prompt"}"#);
    assert_eq!(idle["params"]["arguments"]["status"], "done");
    assert_eq!(
        idle["params"]["arguments"]["hook_event"]["notification_type"],
        "idle_prompt"
    );
    for payload in ["{private-partial", ""] {
        let (request, log) = run_reporter(payload);
        assert_eq!(request["params"]["arguments"]["status"], "blocked");
        assert!(request["params"]["arguments"]["hook_event"].is_null());
        assert!(!log.contains("private-"), "{log}");
    }
}

#[test]
fn question_hook_reporter_preserves_guarded_wire_values_and_correlation() {
    for (state, event) in [
        ("codex-question-start", "PreToolUse"),
        ("codex-question-end", "PostToolUse"),
    ] {
        let (request, log) = run_reporter_for(state, &json!({
            "hook_event_name":event, "tool_name":"request_user_input", "turn_id":"turn-1",
            "tool_use_id":"call-1", "session_id":"session-1", "tool_input":{"questions":"private-question"},
            "tool_response":"private-answer"
        }).to_string());
        let args = &request["params"]["arguments"];
        assert_eq!(
            args["status"], state,
            "older hosts must reject the guarded status"
        );
        assert_eq!(args["hook_event"]["tool_use_id"], "call-1");
        assert_eq!(args["hook_event"]["turn_id"], "turn-1");
        assert_eq!(args["session_id"], "session-1");
        assert!(!request.to_string().contains("private-"));
        assert!(!log.contains("private-"));
    }
}
