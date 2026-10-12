#![cfg(unix)]
//! `spyc --mcp` as a stdio proxy: the pane it names in `initialize`.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// Run `spyc --mcp`, under a parent whose argv is `parent_args`, with
/// `SPYC_PANE_ID=tab-1`; send it an `initialize` and return what it forwards.
fn forwarded_initialize(parent_args: &[&str]) -> Value {
    let temp = tempfile::Builder::new()
        .prefix("spyc-proxy-")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = temp.path().join("mcp.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let mut script = vec!["-c", "\"$0\" --mcp; :", env!("CARGO_BIN_EXE_spyc")];
    script.extend_from_slice(parent_args);
    let mut parent = Command::new("sh")
        .args(&script)
        .env("SPYC_MCP_SOCK", &socket)
        .env("SPYC_PANE_ID", "tab-1")
        .env("XDG_STATE_HOME", temp.path().join("state"))
        .env("HOME", temp.path())
        .env_remove("SPYC_MCP_DEBUG")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = parent.stdin.take().unwrap();
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n")
        .unwrap();
    let (stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut length = None;
    loop {
        let mut header = String::new();
        assert!(reader.read_line(&mut header).unwrap() > 0, "proxy closed");
        if header == "\r\n" {
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length:") {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
    }
    let mut body = vec![0; length.unwrap()];
    reader.read_exact(&mut body).unwrap();
    drop(stdin);
    drop(reader);
    let deadline = Instant::now() + Duration::from_secs(30);
    while parent.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = parent.kill();
    let _ = parent.wait();
    serde_json::from_slice(&body).unwrap()
}

/// The proxy names its pane so the connection binds to its tab, unless it runs
/// under codex's shared daemon: there `SPYC_PANE_ID` is whichever tab started
/// the daemon, and binding to it would answer for that tab.
#[test]
fn the_proxy_names_its_pane_except_under_codex_shared_daemon() {
    let named = forwarded_initialize(&["agent"]);
    assert_eq!(named["params"]["_meta"]["spyc/paneId"], "tab-1", "{named}");

    let under_daemon = forwarded_initialize(&["codex", "app-server", "--managed-daemon"]);
    assert!(
        under_daemon["params"]["_meta"]["spyc/paneId"].is_null(),
        "{under_daemon}"
    );
}
