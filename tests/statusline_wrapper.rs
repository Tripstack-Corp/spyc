#![cfg(unix)]
// Helpers outside `#[test]` fns, where `clippy.toml`'s test allowance stops.
#![allow(clippy::unwrap_used)]

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
        .env_remove("SPYC_JAR_SOCKET")
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

/// The one request a spyc listening at `sock` receives, once one arrives within
/// a generous wait: `None` when none does.
fn receive_one(sock: &std::path::Path) -> std::thread::JoinHandle<Option<String>> {
    let listener = UnixListener::bind(sock).unwrap();
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let until = std::time::Instant::now() + Duration::from_secs(30);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(_) if std::time::Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return None,
            }
        };
        stream.set_nonblocking(false).unwrap();
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
        // The forwarder stops waiting at its timeout, and a loaded runner
        // can get here later than that: the reply is not what's under test.
        let _ = write!(writer, "Content-Length: {}\r\n\r\n{reply}", reply.len());
        Some(String::from_utf8(body).unwrap())
    })
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
    let server = receive_one(&sock);
    let (out, code) = run(
        &wrapper("printf ok"),
        &[
            ("SPYC_MCP_SOCK", sock.to_str().unwrap()),
            ("SPYC_PANE_ID", "pane-9"),
        ],
    );
    assert_eq!((out.as_str(), code), ("ok", 0));
    let body = server.join().unwrap().expect("the projection reached spyc");
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

/// The spyc that launched an agent may be gone: a crash and `spyc -r`, `^d`'s
/// handover or an upgrade put another in its place. Its pane's jar names the
/// spyc that owns it now (R30a), and the projection goes there, so the HUD
/// keeps the tab's context and plan figures.
#[test]
fn a_pane_forwards_the_projection_to_the_spyc_that_owns_its_jar_now() {
    use std::os::unix::fs::PermissionsExt;

    use spyc_jar::client::Client;
    use spyc_jar::proto::{Msg, Role};
    // A jar's socket lives in a private directory, or spyc won't trust it.
    let dir = tempfile::Builder::new()
        .prefix("spyc-sl-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    let jar_socket = dir.path().join("j.sock");
    let token = b"test-token-0123456789";
    let mut jar = Command::new(env!("CARGO_BIN_EXE_spyc-jar"))
        .args(["--wire", "1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    jar.stdin
        .take()
        .unwrap()
        .write_all(
            &spyc_jar::cli::Start {
                token: token.to_vec(),
                socket: jar_socket.clone(),
                cwd: None,
                rows: 24,
                cols: 80,
                hold: None,
                exit_linger: None,
                argv: vec!["/bin/sleep".into(), "1000".into()],
            }
            .encode(),
        )
        .unwrap();
    let ended = Ends(jar);
    let (owner, _) =
        Client::connect(&jar_socket, Role::Owner, Duration::from_secs(30), token).unwrap();
    let now = dir.path().join("now.sock");
    owner
        .send(&Msg::SetInfo {
            key: "owner_mcp".into(),
            value: now.to_str().unwrap().into(),
        })
        .unwrap();
    let until = std::time::Instant::now() + Duration::from_secs(30);
    while !spyc_jar::client::describe(&jar_socket)
        .unwrap_or_default()
        .iter()
        .any(|(k, v)| k == "owner_mcp" && !v.is_empty())
    {
        assert!(
            std::time::Instant::now() < until,
            "the jar never stored its owner"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let server = receive_one(&now);
    let gone = dir.path().join("launcher.sock");
    let (out, code) = run(
        &wrapper("printf ok"),
        &[
            ("SPYC_MCP_SOCK", gone.to_str().unwrap()),
            ("SPYC_PANE_ID", "pane-9"),
            ("SPYC_JAR_SOCKET", jar_socket.to_str().unwrap()),
        ],
    );
    assert_eq!((out.as_str(), code), ("ok", 0));
    let body = server
        .join()
        .unwrap()
        .expect("the projection reached the spyc that owns the jar now");
    let sent: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(sent["params"]["pane_id"], "pane-9");
    assert_eq!(sent["params"]["statusline"]["context"]["used_percent"], 34);
    drop(owner);
    drop(ended);
}

/// A process killed when the test ends, passing or not.
struct Ends(std::process::Child);

impl Drop for Ends {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
