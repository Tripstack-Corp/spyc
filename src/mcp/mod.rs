//! Minimal MCP (Model Context Protocol) server.
//!
//! Two transports:
//!   - **stdio** (`spyc --mcp`): Spawned by Claude Code. Reads
//!     JSON-RPC from stdin, proxies to the running spyc instance
//!     via a Unix domain socket, and writes responses to stdout.
//!   - **Unix socket** (`start_socket_server`): Spawns a background
//!     thread listening on `~/.local/state/spyc/mcp-<PID>.sock`.
//!     The stdio process connects here; writable actions go through
//!     the command channel to the main event loop.
//!
//! Multiple spyc instances coexist via PID-scoped sockets. The
//! `.mcp.json` carries `SPYC_MCP_SOCK` in its `env` block so the
//! stdio proxy connects to the right instance.
//!
//! Both transports share the same JSON-RPC dispatch logic.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use crate::context;

const SERVER_NAME: &str = "spyc";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: &str = "2024-11-05";
const CONTEXT_URI: &str = "spyc://context";

/// Socket IO deadline for the stdio proxy. Bounds how long it waits on a
/// server response (and on a write) so a wedged / silent / panicked server
/// thread surfaces as a clean JSON-RPC error to the agent instead of
/// hanging it indefinitely. Generous — well above the server's own 5 s
/// writable-action timeout — so a legitimately slow reply isn't cut off;
/// only a genuine indefinite stall trips it.
const PROXY_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

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

/// Socket path for a given PID: `~/.local/state/spyc/mcp-<pid>.sock`.
pub fn socket_path_for(pid: u32) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let state_dir = PathBuf::from(home).join(".local/state/spyc");
    state_dir.join(format!("mcp-{pid}.sock"))
}

/// Socket path for the current process.
pub fn socket_path() -> PathBuf {
    socket_path_for(std::process::id())
}

/// Dispatch a JSON-RPC request and write the response to `w`.
/// `cmd_tx` is `Some` when running as the socket server
/// (writable actions available), `None` for read-only fallback.
mod config;
mod protocol;
mod readers;
mod server;

pub use config::{
    McpConfigStatus, detect_existing_spyc, detect_existing_spyc_codex, ensure_codex_config_toml,
    ensure_mcp_json, enterprise_defines_spyc,
};
pub use server::{cleanup_socket, start_socket_server};

use server::{discover_live_socket, run_direct, run_proxy};

fn resolve_context_path(project_root: &Path) -> PathBuf {
    if let Ok(p) = std::env::var(context::CONTEXT_ENV_VAR)
        && !p.is_empty()
    {
        return PathBuf::from(p);
    }
    context::context_path(project_root)
}

/// Run the stdio MCP server. Reads JSONL from stdin, dispatches
/// locally, writes JSONL to stdout. If the running spyc instance's
/// Unix socket is available, proxies through it for writable access.
///
/// Socket resolution order:
/// 1. `$SPYC_MCP_SOCK` (set in `.mcp.json`'s `env` block) — exact match
/// 2. Project-scoped discovery: walk `caller_cwd` upward looking for
///    `.spyc-context-<pid>.json` markers; map those PIDs to live
///    sockets. Refuses cross-project attachment (a spyc running in
///    a different project tree can no longer be picked up).
/// 3. Falls back to read-only direct mode if nothing matches.
pub fn run(project_root: PathBuf) -> anyhow::Result<()> {
    // Try explicit socket path from env first.
    if let Ok(p) = std::env::var("SPYC_MCP_SOCK")
        && !p.is_empty()
    {
        let sock = PathBuf::from(&p);
        mcp_log(&format!("stdio: trying env socket {}", sock.display()));
        if let Ok(stream) = UnixStream::connect(&sock) {
            mcp_log("stdio: connected via env socket, proxying");
            return run_proxy(stream);
        }
    }

    // Discovery: only consider spyc instances rooted in this project
    // tree (caller's cwd or any ancestor). See `discover_live_socket`.
    if let Some(stream) = discover_live_socket(&project_root) {
        return run_proxy(stream);
    }

    // Direct mode: read-only local dispatch (no writable actions).
    mcp_log("stdio: no live socket found, running direct JSONL");
    run_direct(project_root)
}

#[cfg(test)]
mod tests;
