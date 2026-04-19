//! Minimal MCP (Model Context Protocol) server.
//!
//! Two transports:
//!   - **stdio** (`spyc --mcp`): JSON-RPC over Content-Length framed
//!     stdin/stdout, for manual testing or when the pty pipe issue is
//!     resolved.
//!   - **HTTP** (`start_http_server`): Spawns a background thread with
//!     a `TcpListener` on an OS-assigned port. Claude Code connects
//!     via `--mcp-config` with the assigned URL.
//!
//! Both transports share the same JSON-RPC dispatch logic.

use std::io::{self, BufRead, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::context;

const SERVER_NAME: &str = "spyc";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: &str = "2024-11-05";
const CONTEXT_URI: &str = "spyc://context";

/// Log to /tmp/spyc-mcp.log for debugging MCP connection issues.
fn mcp_log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/spyc-mcp.log")
    {
        let _ = writeln!(f, "spyc-mcp: {msg}");
    }
}

// ── Shared JSON-RPC dispatch ────────────────────────────────────

/// Dispatch a JSON-RPC request and write the response to `w`.
fn dispatch(w: &mut impl Write, msg: &str, ctx_path: &Path) -> io::Result<()> {
    let parsed: Value = match serde_json::from_str(msg) {
        Ok(v) => v,
        Err(_) => return send_error(w, Value::Null, -32700, "Parse error"),
    };

    // Notifications (no "id") — acknowledge silently.
    if parsed.get("id").is_none() {
        return Ok(());
    }

    let id = parsed["id"].clone();
    let method = parsed["method"].as_str().unwrap_or("");

    match method {
        "initialize" => handle_initialize(w, &id, &parsed["params"]),
        "resources/list" => handle_resources_list(w, &id),
        "resources/read" => handle_resources_read(w, &id, &parsed["params"], ctx_path),
        "tools/list" => handle_tools_list(w, &id),
        "tools/call" => handle_tools_call(w, &id, &parsed["params"], ctx_path),
        "ping" => send_result(w, &id, json!({})),
        _ => send_error(w, id, -32601, &format!("Method not found: {method}")),
    }
}

// ── Stdio transport (spyc --mcp) ────────────────────────────────

/// Resolve context path from env var or project root.
fn resolve_context_path(project_root: &Path) -> PathBuf {
    if let Ok(p) = std::env::var(context::CONTEXT_ENV_VAR) {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    context::context_path(project_root)
}

/// Run the stdio MCP server loop. Blocks until stdin closes.
pub fn run(project_root: PathBuf) -> anyhow::Result<()> {
    let context_path = resolve_context_path(&project_root);
    mcp_log(&format!("stdio: starting, context_path={}", context_path.display()));
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    loop {
        let msg = match read_lsp_message(&mut reader) {
            Ok(msg) => msg,
            Err(e) => {
                mcp_log(&format!("stdio: read error: {e}"));
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    break;
                }
                return Err(e.into());
            }
        };
        dispatch(&mut writer, &msg, &context_path)?;
    }
    Ok(())
}

// ── HTTP transport (background thread) ──────────────────────────

/// Start the HTTP MCP server on an OS-assigned port. Returns the port
/// number. The server runs on a background thread and reads context
/// from `ctx_path`.
pub fn start_http_server(ctx_path: PathBuf) -> anyhow::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let ctx_path = Arc::new(ctx_path);

    mcp_log(&format!("http: listening on 127.0.0.1:{port}"));

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let ctx = Arc::clone(&ctx_path);
            // Handle each connection in a thread (Claude Code keeps one
            // connection open, but we handle concurrent requests safely).
            std::thread::spawn(move || {
                if let Err(e) = handle_http_connection(stream, &ctx) {
                    mcp_log(&format!("http: connection error: {e}"));
                }
            });
        }
    });

    Ok(port)
}

/// Return the `--mcp-config` JSON string for this server.
pub fn mcp_config_json(port: u16) -> String {
    json!({
        "mcpServers": {
            "spyc": {
                "type": "http",
                "url": format!("http://127.0.0.1:{port}/mcp")
            }
        }
    })
    .to_string()
}

fn handle_http_connection(
    mut stream: std::net::TcpStream,
    ctx_path: &Path,
) -> io::Result<()> {
    use std::io::BufReader;
    let mut reader = BufReader::new(stream.try_clone()?);

    // Read HTTP requests in a loop (HTTP/1.1 keep-alive).
    loop {
        // Read request line.
        let mut request_line = String::new();
        let n = reader.read_line(&mut request_line)?;
        if n == 0 {
            break; // Connection closed.
        }

        // Read headers.
        let mut content_length: usize = 0;
        let mut header = String::new();
        loop {
            header.clear();
            reader.read_line(&mut header)?;
            if header.trim().is_empty() {
                break;
            }
            if let Some(val) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = val.trim().parse().unwrap_or(0);
            }
        }

        // Read body.
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body)?;
        }
        let body_str = String::from_utf8_lossy(&body);

        // Route.
        let is_post = request_line.starts_with("POST");
        let is_mcp = request_line.contains("/mcp");

        if is_post && is_mcp {
            // JSON-RPC request.
            let mut response_buf = Vec::new();
            dispatch(&mut response_buf, &body_str, ctx_path)?;

            // The dispatch writes Content-Length framed output (for stdio).
            // For HTTP, we need to extract just the JSON body.
            let response_body = extract_json_body(&response_buf);

            write!(
                stream,
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {}\r\n\
                 \r\n\
                 {}",
                response_body.len(),
                response_body
            )?;
            stream.flush()?;
        } else {
            // Health check or unknown route.
            let body = json!({"status": "ok", "server": SERVER_NAME}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {}\r\n\
                 \r\n\
                 {}",
                body.len(),
                body
            )?;
            stream.flush()?;
        }
    }
    Ok(())
}

/// Extract the JSON body from a Content-Length framed message.
fn extract_json_body(framed: &[u8]) -> String {
    let s = String::from_utf8_lossy(framed);
    if let Some(pos) = s.find("\r\n\r\n") {
        s[pos + 4..].to_string()
    } else {
        s.to_string()
    }
}

// ── Protocol handlers ────────────────────────────────────────────

fn handle_initialize(w: &mut impl Write, id: &Value, params: &Value) -> io::Result<()> {
    let version = params["protocolVersion"]
        .as_str()
        .unwrap_or(PROTOCOL_VERSION);
    send_result(
        w,
        id,
        json!({
            "protocolVersion": version,
            "capabilities": {
                "resources": {},
                "tools": {}
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION
            }
        }),
    )
}

fn handle_resources_list(w: &mut impl Write, id: &Value) -> io::Result<()> {
    send_result(
        w,
        id,
        json!({
            "resources": [
                {
                    "uri": CONTEXT_URI,
                    "name": "spyc context",
                    "description": "Current spyc state: working directory, cursor position, picks, inventory, filter, git branch.",
                    "mimeType": "application/json"
                }
            ]
        }),
    )
}

fn handle_resources_read(
    w: &mut impl Write,
    id: &Value,
    params: &Value,
    ctx_path: &Path,
) -> io::Result<()> {
    let uri = params["uri"].as_str().unwrap_or("");
    if uri != CONTEXT_URI {
        return send_error(w, id.clone(), -32602, &format!("Unknown resource: {uri}"));
    }

    let text = read_context_or_empty(ctx_path);
    send_result(
        w,
        id,
        json!({
            "contents": [
                {
                    "uri": CONTEXT_URI,
                    "mimeType": "application/json",
                    "text": text
                }
            ]
        }),
    )
}

fn handle_tools_list(w: &mut impl Write, id: &Value) -> io::Result<()> {
    send_result(
        w,
        id,
        json!({
            "tools": [
                {
                    "name": "get_spyc_context",
                    "description": "Get the current spyc file manager state: working directory, cursor position, picked files, inventory, active filter, and git branch. Use this to understand what the user is looking at in their file manager.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                }
            ]
        }),
    )
}

fn handle_tools_call(
    w: &mut impl Write,
    id: &Value,
    params: &Value,
    ctx_path: &Path,
) -> io::Result<()> {
    let name = params["name"].as_str().unwrap_or("");
    if name != "get_spyc_context" {
        return send_result(
            w,
            id,
            json!({
                "isError": true,
                "content": [{"type": "text", "text": format!("Unknown tool: {name}")}]
            }),
        );
    }

    let text = read_context_or_empty(ctx_path);
    send_result(
        w,
        id,
        json!({
            "content": [{"type": "text", "text": text}]
        }),
    )
}

fn read_context_or_empty(ctx_path: &Path) -> String {
    std::fs::read_to_string(ctx_path).unwrap_or_else(|_| {
        json!({
            "cwd": null,
            "cursor_file": null,
            "picks": [],
            "inventory": [],
            "filter": null,
            "git_branch": null
        })
        .to_string()
    })
}

// ── Framing helpers ─────────────────────────────────────────────

fn read_lsp_message(reader: &mut impl BufRead) -> io::Result<String> {
    let mut content_length: Option<usize> = None;
    let mut header = String::new();
    loop {
        header.clear();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stdin closed"));
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(val) = trimmed.strip_prefix("Content-Length:") {
            content_length = val.trim().parse().ok();
        }
    }

    let len = content_length.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length")
    })?;

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    String::from_utf8(buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn send_message(w: &mut impl Write, body: &str) -> io::Result<()> {
    write!(w, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    w.flush()
}

fn send_result(w: &mut impl Write, id: &Value, result: Value) -> io::Result<()> {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    });
    send_message(w, &msg.to_string())
}

fn send_error(w: &mut impl Write, id: Value, code: i32, message: &str) -> io::Result<()> {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    });
    send_message(w, &msg.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make_request(id: u64, method: &str, params: Value) -> String {
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        })
        .to_string();
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
    }

    fn parse_response(raw: &[u8]) -> Value {
        let s = std::str::from_utf8(raw).unwrap();
        let body_start = s.find("\r\n\r\n").unwrap() + 4;
        serde_json::from_str(&s[body_start..]).unwrap()
    }

    #[test]
    fn initialize_response() {
        let input = make_request(1, "initialize", json!({}));
        let mut reader = Cursor::new(input.as_bytes().to_vec());
        let msg = read_lsp_message(&mut reader).unwrap();

        let mut output = Vec::new();
        dispatch(&mut output, &msg, Path::new("/tmp")).unwrap();
        let resp = parse_response(&output);
        assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(resp["result"]["serverInfo"]["name"], "spyc");
    }

    #[test]
    fn resources_list_response() {
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":2,"method":"resources/list"}).to_string(),
            Path::new("/tmp"),
        )
        .unwrap();
        let resp = parse_response(&output);
        let resources = resp["result"]["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0]["uri"], CONTEXT_URI);
    }

    #[test]
    fn resources_read_with_context_file() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = context::SpycContext {
            cwd: PathBuf::from("/home/user/project"),
            cursor_file: Some("main.rs".into()),
            picks: vec![],
            inventory: vec![],
            filter: None,
            git_branch: Some("develop".into()),
        };
        let ctx_path = context::context_path(tmp.path());
        context::write_context_file(&ctx_path, &ctx).unwrap();

        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":CONTEXT_URI}}).to_string(),
            &ctx_path,
        )
        .unwrap();
        let resp = parse_response(&output);
        let text = resp["result"]["contents"][0]["text"].as_str().unwrap();
        let inner: Value = serde_json::from_str(text).unwrap();
        assert_eq!(inner["cwd"], "/home/user/project");
        assert_eq!(inner["git_branch"], "develop");
    }

    #[test]
    fn tools_list_response() {
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/list"}).to_string(),
            Path::new("/tmp"),
        )
        .unwrap();
        let resp = parse_response(&output);
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "get_spyc_context");
    }

    #[test]
    fn tools_call_returns_context() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = context::SpycContext {
            cwd: PathBuf::from("/projects/spyc"),
            cursor_file: Some("Cargo.toml".into()),
            picks: vec![PathBuf::from("src/main.rs")],
            inventory: vec![],
            filter: Some("*.rs".into()),
            git_branch: Some("feature".into()),
        };
        let ctx_path = context::context_path(tmp.path());
        context::write_context_file(&ctx_path, &ctx).unwrap();

        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"get_spyc_context","arguments":{}}}).to_string(),
            &ctx_path,
        )
        .unwrap();
        let resp = parse_response(&output);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let inner: Value = serde_json::from_str(text).unwrap();
        assert_eq!(inner["cursor_file"], "Cargo.toml");
        assert_eq!(inner["filter"], "*.rs");
    }

    #[test]
    fn unknown_resource_returns_error() {
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"spyc://bogus"}}).to_string(),
            Path::new("/tmp"),
        )
        .unwrap();
        let resp = parse_response(&output);
        assert!(resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unknown resource"));
    }

    #[test]
    fn notification_is_silent() {
        let mut output = Vec::new();
        dispatch(
            &mut output,
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string(),
            Path::new("/tmp"),
        )
        .unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn read_lsp_message_parses() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let framed = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut reader = Cursor::new(framed.as_bytes().to_vec());
        let msg = read_lsp_message(&mut reader).unwrap();
        assert_eq!(msg, body);
    }

    #[test]
    fn http_server_responds() {
        use std::io::{Read, Write};
        let tmp = tempfile::tempdir().unwrap();
        let ctx = context::SpycContext {
            cwd: PathBuf::from("/test"),
            cursor_file: None,
            picks: vec![],
            inventory: vec![],
            filter: None,
            git_branch: None,
        };
        let ctx_path = context::context_path(tmp.path());
        context::write_context_file(&ctx_path, &ctx).unwrap();

        let port = start_http_server(ctx_path).unwrap();

        // Give the server thread a moment to start.
        std::thread::sleep(std::time::Duration::from_millis(50));

        let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}).to_string();
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );

        let mut stream = std::net::TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();

        let mut response = String::new();
        stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        let _ = stream.read_to_string(&mut response);

        assert!(response.contains("get_spyc_context"));
    }

    #[test]
    fn mcp_config_json_format() {
        let config = mcp_config_json(12345);
        let parsed: Value = serde_json::from_str(&config).unwrap();
        assert_eq!(parsed["mcpServers"]["spyc"]["type"], "http");
        assert_eq!(
            parsed["mcpServers"]["spyc"]["url"],
            "http://127.0.0.1:12345/mcp"
        );
    }
}
