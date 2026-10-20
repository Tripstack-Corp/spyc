//! `spyc --statusline`: what Claude Code's `statusLine` runs once spyc's
//! wiring wraps it. It forwards the projection of its input to the spyc pane it
//! runs in, then runs the user's own statusline command on the same input, so
//! what the user sees is unchanged. Outside spyc it only runs that command.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

use super::framing::{send_message, send_result};
use crate::agent::claude_statusline::{self, Statusline};
use crate::mcp_cmd::{McpCommand, McpRequest};

/// The JSON-RPC method the forwarder calls. Not an MCP tool: no agent sees it.
pub(super) const METHOD: &str = "spyc/statusline";

/// The statusline runs on every message, so the forward never holds it up long.
const FORWARD_TIMEOUT: Duration = Duration::from_millis(500);

/// Run as Claude Code's statusline: forward, then run `wrapped`. The exit
/// status is 0 once `wrapped` has run, whatever its own: the wrapper's fallback
/// is for a spyc that can't do this, and must not run the command a second time.
pub fn run(wrapped: Option<&str>) -> i32 {
    let input = {
        use std::io::IsTerminal;
        let mut input = Vec::new();
        let stdin = std::io::stdin();
        if !stdin.is_terminal() {
            let _ = stdin.lock().read_to_end(&mut input);
        }
        input
    };
    // Started first, so the user's line never waits on spyc.
    let child = wrapped.and_then(|command| start(command, &input));
    forward(&claude_statusline::project_bytes(&input));
    if let Some(mut child) = child {
        let _ = child.wait();
    }
    0
}

/// The user's statusline command, through `sh` as Claude Code runs it, fed the
/// same input on a thread so a command that never reads it can't block us.
fn start(command: &str, input: &[u8]) -> Option<std::process::Child> {
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let input = input.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    Some(child)
}

/// Send the projection to the spyc this pane's environment names. Best-effort.
fn forward(statusline: &Statusline) {
    if statusline.is_empty() {
        return;
    }
    let sock = std::env::var("SPYC_MCP_SOCK").unwrap_or_default();
    let pane_id = std::env::var("SPYC_PANE_ID").unwrap_or_default();
    if sock.is_empty() || pane_id.is_empty() || super::under_codex_shared_daemon() {
        return;
    }
    forward_to(&sock, &pane_id, statusline);
}

fn forward_to(sock: &str, pane_id: &str, statusline: &Statusline) {
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(sock) else {
        return;
    };
    let _ = stream.set_read_timeout(Some(FORWARD_TIMEOUT));
    let _ = stream.set_write_timeout(Some(FORWARD_TIMEOUT));
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": METHOD,
        "params": { "pane_id": pane_id, "statusline": statusline },
    })
    .to_string();
    if send_message(&mut stream, &request).is_ok() {
        // Read the reply so the server has the report before the socket closes.
        let mut reply = [0u8; 256];
        let _ = stream.read(&mut reply);
    }
}

/// The server's half: queue the report for the loop and reply at once. A report
/// that names no pane, or carries nothing, is dropped.
pub(super) fn handle(
    w: &mut impl Write,
    id: &Value,
    params: &Value,
    cmd_tx: Option<&std::sync::mpsc::Sender<McpRequest>>,
) -> std::io::Result<()> {
    let pane_id = params["pane_id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 128)
        .map(str::to_owned);
    let statusline = serde_json::from_value::<Statusline>(params["statusline"].clone())
        .ok()
        .map(Statusline::bounded)
        .filter(|s| !s.is_empty());
    if let (Some(pane_id), Some(statusline), Some(tx)) = (pane_id, statusline, cmd_tx) {
        let (reply, _) = std::sync::mpsc::channel();
        let _ = tx.send(McpRequest {
            command: McpCommand::Statusline {
                pane_id,
                statusline,
            },
            reply,
            project: None,
        });
    }
    send_result(w, id, json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::claude_statusline::RateWindow;

    /// What the forwarder puts on the socket is the framed method call, with
    /// the pane it runs in and the projection.
    #[test]
    fn the_forwarder_sends_its_pane_and_projection_on_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("spyc.sock");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let body = super::super::framing::read_lsp_message(&mut reader).unwrap();
            let mut writer = stream;
            // The forwarder stops waiting at its timeout, and a loaded runner
            // can get here later than that: the reply is not what's under test.
            let _ = send_result(&mut writer, &json!(1), json!({}));
            serde_json::from_str::<Value>(&body).unwrap()
        });
        let statusline = Statusline {
            five_hour: Some(RateWindow {
                used_percent: 23,
                resets_at: None,
            }),
            ..Statusline::default()
        };
        forward_to(sock.to_str().unwrap(), "pane-7", &statusline);
        let sent = server.join().unwrap();
        assert_eq!(sent["method"], METHOD);
        assert_eq!(sent["params"]["pane_id"], "pane-7");
        assert_eq!(
            serde_json::from_value::<Statusline>(sent["params"]["statusline"].clone()).unwrap(),
            statusline
        );
    }

    /// A spyc that never answers, or none at all, holds the statusline up for
    /// at most the forward's timeout. Waited on generously, so only a missing
    /// timeout fails it, never a slow runner.
    #[test]
    fn the_forwarder_never_holds_the_statusline_past_its_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let silent = dir.path().join("silent.sock");
        let _never_accepts = std::os::unix::net::UnixListener::bind(&silent).unwrap();
        let statusline = Statusline {
            five_hour: Some(RateWindow {
                used_percent: 23,
                resets_at: None,
            }),
            ..Statusline::default()
        };
        for sock in [silent, dir.path().join("gone.sock")] {
            let (done, finished) = std::sync::mpsc::channel();
            let (path, statusline) = (sock.clone(), statusline.clone());
            std::thread::spawn(move || {
                forward_to(path.to_str().unwrap(), "pane-7", &statusline);
                let _ = done.send(());
            });
            assert!(
                finished.recv_timeout(FORWARD_TIMEOUT * 10).is_ok(),
                "{sock:?} held the statusline"
            );
        }
    }
}
