//! A test client for one MCP connection: the server's own
//! `handle_socket_connection` on one end of a socket pair, JSON-RPC on the
//! other. The server's tests answer its commands with a stand-in loop; the
//! App's pump them into a real one.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::{Value, json};

use super::protocol::{PANE_ID_META, read_lsp_message, send_message};
use super::server::handle_socket_connection;
use crate::mcp_cmd::McpRequest;

/// How long a call waits for its reply. A loop that never answers fails the
/// test instead of hanging the run.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Conn {
    reader: io::BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

impl Conn {
    /// Serve one connection on its own thread. `ctx_path` is the context file
    /// the server was started with; `cmd_tx` is where it sends the loop's work.
    pub fn open(ctx_path: PathBuf, cmd_tx: Sender<McpRequest>) -> Self {
        let (client, server) = UnixStream::pair().expect("a socket pair");
        std::thread::spawn(move || {
            handle_socket_connection(server, &ctx_path, &cmd_tx).unwrap_or(());
        });
        client
            .set_read_timeout(Some(REPLY_TIMEOUT))
            .expect("a read timeout");
        Self {
            reader: io::BufReader::new(client.try_clone().expect("a second handle")),
            writer: client,
            next_id: 1,
        }
    }

    pub fn call(&mut self, method: &str, params: Value) -> Value {
        let body =
            json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params});
        self.next_id += 1;
        send_message(&mut self.writer, &body.to_string()).expect("the request is sent");
        let reply = read_lsp_message(&mut self.reader)
            .unwrap_or_else(|e| panic!("no reply to {method}: {e}"));
        serde_json::from_str(&reply).expect("a JSON reply")
    }

    /// `initialize`, naming `pane_id` as the `spyc --mcp` proxy does.
    pub fn initialize(&mut self, pane_id: Option<&str>) {
        self.initialize_with(pane_id.map_or(Value::Null, |p| json!({PANE_ID_META: p})));
    }

    /// `initialize` carrying `meta` as its `_meta`; `Null` sends none.
    pub fn initialize_with(&mut self, meta: Value) {
        let params = if meta.is_null() {
            json!({})
        } else {
            json!({"_meta": meta})
        };
        let resp = self.call("initialize", params);
        assert!(resp["result"]["serverInfo"].is_object(), "{resp}");
    }

    /// A tool's text result, parsed when it is JSON.
    pub fn tool(&mut self, name: &str, args: Value) -> Value {
        let resp = self.call("tools/call", json!({"name": name, "arguments": args}));
        let text = resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{resp}"))
            .to_string();
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    }
}
