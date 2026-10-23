#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

fn run_reporter(payload: &str) -> (Value, String) {
    run_reporter_for("blocked", payload)
}

/// How long to wait on the reporter before failing instead of hanging. A bound
/// for a broken reporter, not a measure of a working one: starting a process on
/// a loaded machine has no useful upper limit.
const HANG_GUARD: Duration = Duration::from_secs(30);

/// The reporter is the freshly built `spyc`. macOS scans an executable on its
/// first run, which takes seconds for a debug build this size, and every rebuild
/// (any worktree sharing the target dir) makes it new again. Run it once,
/// untimed, so the waits below are spent on the reporter rather than the scan.
fn warm_binary() {
    static WARM: std::sync::Once = std::sync::Once::new();
    WARM.call_once(|| {
        let _ = Command::new(env!("CARGO_BIN_EXE_spyc"))
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    });
}

fn run_reporter_for(state: &str, payload: &str) -> (Value, String) {
    run_reporter_traced(state, payload, Trace::Arg)
}

/// How a reporter run is told to trace.
enum Trace {
    /// The `--status-trace` argument hooks installed by older spycs carry.
    Arg,
    /// `<state>/spyc/status-trace` naming this pid, as `spyc --status-trace`
    /// writes it.
    FlagFile(u32),
}

fn run_reporter_traced(state: &str, payload: &str, trace: Trace) -> (Value, String) {
    warm_binary();
    let temp = tempfile::Builder::new()
        .prefix("spyc-hook-")
        .tempdir_in("/tmp")
        .unwrap();
    let mut args = vec!["--report-status", state];
    match trace {
        Trace::Arg => args.push("--status-trace"),
        Trace::FlagFile(pid) => {
            let dir = temp.path().join("state/spyc");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("status-trace"), pid.to_string()).unwrap();
        }
    }
    let socket = temp.path().join("hook.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + HANG_GUARD;
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
        // macOS hands the accepted socket the listener's O_NONBLOCK (Linux, and
        // so CI, doesn't), and a non-blocking socket ignores the read timeout
        // below: a read that beats the reporter's write fails at once with
        // WouldBlock, which a loaded machine makes likely.
        stream.set_nonblocking(false).unwrap();
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
        .args(&args)
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
    let deadline = Instant::now() + HANG_GUARD;
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
    let log = std::fs::read_to_string(temp.path().join("state/spyc/mcp.log")).unwrap_or_default();
    (request, log)
}

/// A hook's command is the same whether or not spyc traces, because codex
/// hashes the command to decide the hook is still the one the user trusted.
/// So `spyc --status-trace` reaches the reporter through a flag file naming
/// the tracing spyc, and tracing stops when that spyc is gone.
#[test]
fn the_reporter_traces_while_the_spyc_that_asked_lives() {
    let (_, log) = run_reporter_traced("working", "{}", Trace::FlagFile(std::process::id()));
    assert!(log.contains("report-status: state=working"), "{log}");

    let mut gone = Command::new("true").spawn().unwrap();
    let dead = gone.id();
    gone.wait().unwrap();
    let (_, log) = run_reporter_traced("working", "{}", Trace::FlagFile(dead));
    assert!(!log.contains("report-status:"), "{log}");
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

#[test]
fn large_hook_arguments_do_not_erase_permission_or_question_metadata() {
    // Normalized metadata recorded from native CLI runs; oversized body content
    // is synthetic edge-case data, not a claimed raw native hook capture.
    let fixtures: Value =
        serde_json::from_str(include_str!("fixtures/codex-hook-metadata.json")).unwrap();
    for (state, fixture, body_field) in [
        ("blocked", "permission", "tool_input"),
        ("codex-question-start", "question_start", "tool_input"),
        ("codex-question-end", "question_end", "tool_response"),
    ] {
        let metadata = &fixtures[fixture];
        // Put all correlation fields after a body larger than the old cutoff.
        let payload = format!(
            r#"{{"{body_field}":{{"private-content":"{}"}},{}}}"#,
            "private-content-".repeat(1600),
            metadata
                .to_string()
                .trim_start_matches('{')
                .trim_end_matches('}')
        );
        assert!(payload.len() > 8192);
        let (request, log) = run_reporter_for(state, &payload);
        let args = &request["params"]["arguments"];
        let mut expected = metadata.clone();
        expected.as_object_mut().unwrap().remove("session_id");
        assert_eq!(args["hook_event"], expected);
        assert_eq!(args["session_id"], metadata["session_id"]);
        assert_eq!(args["status"], state);
        assert!(!request.to_string().contains("private-content"));
        assert!(!log.contains("private-content"));
    }
}

#[test]
fn large_hook_body_preserves_legacy_idle_remap_and_camel_case_session() {
    let payload = format!(
        r#"{{"tool_response":"{}","hook_event_name":"Notification","notification_type":"idle_prompt","conversationId":"session-agy"}}"#,
        "private-content-".repeat(1600)
    );
    let (request, log) = run_reporter(&payload);
    let args = &request["params"]["arguments"];
    assert_eq!(args["status"], "done");
    assert_eq!(args["session_id"], "session-agy");
    assert!(!request.to_string().contains("private-content"));
    assert!(!log.contains("private-content"));
}

#[test]
fn agy_hook_reporter_forwards_event_metadata_and_remaps_stop_states() {
    let huge = "private-content-".repeat(1600);
    let question = format!(
        r#"{{"conversationId":"conv-agy","toolCall":{{"name":"ask_question","args":{{"questions":["{huge}"]}}}},"stepIdx":7}}"#
    );
    let (request, log) = run_reporter_for("blocked", &question);
    let args = &request["params"]["arguments"];
    assert_eq!(args["status"], "blocked");
    assert_eq!(args["session_id"], "conv-agy");
    assert_eq!(
        args["hook_event"],
        json!({"hook_event_name": "PreToolUse", "tool_name": "ask_question"})
    );
    assert!(!request.to_string().contains("private-content"));
    assert!(!log.contains("private-content"));

    let waiting = format!(
        r#"{{"conversationId":"conv-agy","executionNum":1,"terminationReason":"NO_TOOL_CALL","error":"{huge}","fullyIdle":false}}"#
    );
    let (request, _) = run_reporter_for("done", &waiting);
    let args = &request["params"]["arguments"];
    assert_eq!(args["status"], "waiting");
    assert_eq!(
        args["hook_event"],
        json!({
            "hook_event_name": "Stop",
            "termination_reason": "NO_TOOL_CALL",
            "fully_idle": false
        })
    );

    let cancelled = r#"{"conversationId":"conv-agy","executionNum":2,"terminationReason":"USER_CANCELED","error":"cancelled","fullyIdle":true}"#;
    let (request, _) = run_reporter_for("done", cancelled);
    let args = &request["params"]["arguments"];
    assert_eq!(args["status"], "idle");
    assert_eq!(
        args["hook_event"]["termination_reason"],
        json!("USER_CANCELED")
    );
}

/// Under codex's shared daemon, the reporter's `SPYC_PANE_ID` is whichever tab
/// started the daemon, not its own session's, so it reports nothing rather
/// than light that tab. The stand-in daemon's argv is the shared server's, and
/// it runs the reporter as its child, as the daemon runs a session's hooks.
#[test]
fn status_hook_reporter_under_codex_shared_daemon_reports_nothing() {
    warm_binary();
    let temp = tempfile::Builder::new()
        .prefix("spyc-hook-")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = temp.path().join("hook.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut daemon = Command::new("sh")
        .args([
            "-c",
            "\"$0\" --report-status working --status-trace </dev/null; :",
            env!("CARGO_BIN_EXE_spyc"),
            "app-server",
            "--managed-daemon",
        ])
        .env("SPYC_MCP_SOCK", &socket)
        .env("SPYC_PANE_ID", "starter-tab")
        .env("XDG_STATE_HOME", temp.path().join("state"))
        .env("HOME", temp.path())
        .env_remove("SPYC_MCP_DEBUG")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + HANG_GUARD;
    while daemon.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            daemon.kill().unwrap();
            let _ = daemon.wait();
            panic!("reporter did not exit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(daemon.wait().unwrap().success());
    let log = std::fs::read_to_string(temp.path().join("state/spyc/mcp.log")).unwrap_or_default();
    // The reporter has exited, so a connection it didn't make never will.
    match listener.accept() {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        Ok(_) => panic!("reported under codex's shared daemon; trace:\n{log}"),
        Err(error) => panic!("accept: {error}"),
    }
    assert!(log.contains("under codex's shared daemon"), "{log}");
}
