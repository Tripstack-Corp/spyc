//! An agent's MCP proxy follows its pane's jar (R30a). The proxy reaches the
//! spyc that launched the agent; when that spyc goes (a crash, the handover
//! `^d` makes, an upgrade), the agent's tab runs on in its jar, which a new
//! spyc takes back. The proxy then asks the jar, named by `SPYC_JAR_SOCKET`,
//! where its owner's MCP socket is now, connects there, and replays the
//! `initialize` and `initialized` it began with, so the new spyc knows the
//! pane. While no spyc owns the jar, a request waits for one within its own
//! wait, then gets an error, and the proxy carries on for the next.

use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::framing::{read_lsp_message, send_message};
use super::mcp_log;

/// How often a proxy whose spyc has gone asks its jar for the next one.
const FOLLOW_POLL: Duration = Duration::from_millis(250);

/// The proxy's connection to a spyc.
pub(super) struct Upstream {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

/// Why an exchange with a spyc failed.
pub(super) enum Failure {
    /// The spyc went: the connection ended or broke.
    Gone(io::Error),
    /// The spyc is there and didn't answer within the wait.
    Silent(io::Error),
}

impl Upstream {
    pub(super) fn new(stream: UnixStream, io_timeout: Duration) -> io::Result<Self> {
        let reader = stream.try_clone()?;
        // Bound socket IO so a wedged server can't hang the agent; each
        // request's read is bounded again by what the server may take over it.
        reader.set_read_timeout(Some(io_timeout))?;
        stream.set_write_timeout(Some(io_timeout))?;
        Ok(Self {
            reader: BufReader::new(reader),
            writer: stream,
        })
    }

    /// Send `msg`, and for a request, read its reply within `wait`.
    pub(super) fn exchange(
        &mut self,
        msg: &str,
        is_request: bool,
        wait: Duration,
    ) -> Result<Option<String>, Failure> {
        send_message(&mut self.writer, msg).map_err(Failure::Gone)?;
        if !is_request {
            return Ok(None);
        }
        let _ = self.reader.get_ref().set_read_timeout(Some(wait));
        match read_lsp_message(&mut self.reader) {
            Ok(reply) => Ok(Some(reply)),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Err(Failure::Silent(e))
            }
            Err(e) => Err(Failure::Gone(e)),
        }
    }
}

/// Whether `msg` binds a session to its pane, to be replayed to the next spyc.
pub(super) fn binds(msg: &Value) -> bool {
    matches!(
        msg["method"].as_str(),
        Some("initialize" | "notifications/initialized")
    )
}

// SPYC-TRAP(mcp-follows-jar): an agent finds spyc through its pane's jar
// after the spyc that launched it goes; a proxy that ends with its first spyc
// leaves the agent without spyc's tools, and nothing says so.
/// The spyc that owns the jar at `jar` now, connected and bound by replaying
/// `binding`; asked until `until`.
pub(super) fn follow(
    jar: &Path,
    binding: &[String],
    io_timeout: Duration,
    until: Instant,
) -> Option<Upstream> {
    loop {
        if let Some(upstream) = crate::pane::jar::owner_mcp(jar)
            .and_then(|path| UnixStream::connect(&path).ok())
            .and_then(|stream| Upstream::new(stream, io_timeout).ok())
            .and_then(|up| bind(up, binding, io_timeout))
        {
            mcp_log("proxy: following the pane's jar to the spyc that owns it now");
            return Some(upstream);
        }
        if Instant::now() >= until {
            mcp_log("proxy: the pane's jar names no spyc that answers");
            return None;
        }
        std::thread::sleep(FOLLOW_POLL);
    }
}

/// Replay `binding` on `up`, its replies read and dropped.
fn bind(mut up: Upstream, binding: &[String], io_timeout: Duration) -> Option<Upstream> {
    for msg in binding {
        let is_request = serde_json::from_str::<Value>(msg).is_ok_and(|v| v.get("id").is_some());
        up.exchange(msg, is_request, io_timeout).ok()?;
    }
    Some(up)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_and_initialized_bind_a_session_and_nothing_else_does() {
        let msg = |m: &str| serde_json::json!({ "jsonrpc": "2.0", "method": m });
        assert!(binds(&msg("initialize")));
        assert!(binds(&msg("notifications/initialized")));
        assert!(!binds(&msg("tools/call")));
        assert!(!binds(&msg("notifications/cancelled")));
    }
}
