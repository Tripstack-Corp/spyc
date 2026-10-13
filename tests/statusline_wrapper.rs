#![cfg(unix)]

//! The statusline wrapper end to end: `sh -c` running the wrapper spyc's wiring
//! writes, the real `spyc --statusline`, and the user's command.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

/// macOS scans a new executable on its first run; spend that outside any wait.
fn warm_binary() {
    static WARM: std::sync::Once = std::sync::Once::new();
    WARM.call_once(|| {
        let _ = Command::new(env!("CARGO_BIN_EXE_spyc"))
            .arg("--version")
            .stdout(Stdio::null())
            .status();
    });
}

/// The wrapper as `wiring::statusline::wrap` writes it, around `original`.
fn wrapper(original: &str) -> String {
    let exe = shlex::try_quote(env!("CARGO_BIN_EXE_spyc"))
        .unwrap()
        .into_owned();
    let original = shlex::try_quote(original).unwrap().into_owned();
    format!(
        "o={original}; command -v {exe} >/dev/null 2>&1 && {exe} --statusline -- \"$o\" || exec sh -c \"$o\""
    )
}

fn input() -> String {
    json!({
        "session_id": "c1a0de00-0000-4000-8000-000000000001",
        "cwd": "/PRIVATE/SENTINEL",
        "context_window": {"context_window_size": 1_000_000, "used_percentage": 34},
        "rate_limits": {"five_hour": {"used_percentage": 23, "resets_at": 1_791_900_000}}
    })
    .to_string()
}

/// Run `command` through `sh -c` with the statusline input, as Claude Code does.
fn run(command: &str, env: &[(&str, &str)]) -> (String, i32) {
    warm_binary();
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(command)
        .env_remove("SPYC_MCP_SOCK")
        .env_remove("SPYC_PANE_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    for (key, value) in env {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8(output.stdout).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

/// The user's command sees the same input and prints once, even when it fails:
/// spyc exits 0 after running it, so the wrapper's fallback stays unused.
#[test]
fn the_users_statusline_runs_once_on_the_same_input() {
    let (out, code) = run(&wrapper("printf 'line:'; cat; exit 3"), &[]);
    assert_eq!(out, format!("line:{}", input()));
    assert_eq!(code, 0);
}

/// Inside a spyc pane the projection reaches that spyc, and nothing private
/// travels with it.
#[test]
fn a_pane_forwards_the_projection_to_its_spyc() {
    let dir = tempfile::Builder::new()
        .prefix("spyc-sl-")
        .tempdir_in("/tmp")
        .unwrap();
    let sock = dir.path().join("spyc.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut length = 0;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).unwrap();
            if header.trim().is_empty() {
                break;
            }
            if let Some(n) = header.trim().strip_prefix("Content-Length: ") {
                length = n.parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        let reply = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        let mut writer = stream;
        write!(writer, "Content-Length: {}\r\n\r\n{reply}", reply.len()).unwrap();
        String::from_utf8(body).unwrap()
    });
    let (out, code) = run(
        &wrapper("printf ok"),
        &[
            ("SPYC_MCP_SOCK", sock.to_str().unwrap()),
            ("SPYC_PANE_ID", "pane-9"),
        ],
    );
    assert_eq!((out.as_str(), code), ("ok", 0));
    let body = server.join().unwrap();
    assert!(!body.contains("SENTINEL"), "{body}");
    let sent: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(sent["method"], "spyc/statusline");
    assert_eq!(sent["params"]["pane_id"], "pane-9");
    assert_eq!(sent["params"]["statusline"]["context"]["used_percent"], 34);
    assert_eq!(
        sent["params"]["statusline"]["five_hour"]["used_percent"],
        23
    );
}
