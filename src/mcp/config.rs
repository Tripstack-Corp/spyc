//! Managing the client-side MCP config (.mcp.json / codex config.toml) and
//! detecting/handing off existing spyc instances. Split out of mcp.rs verbatim.
use std::io::{self};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::server::{notify_disconnect, pid_from_sock_path};
use super::{mcp_log, socket_path};
pub enum McpConfigStatus {
    /// .mcp.json written/updated to point at our socket.
    Configured,
    /// Took over from another instance (notified it). PID of old instance.
    TookOver { old_pid: u32 },
    /// Detected another live instance and the caller asked us not to
    /// take over — `.mcp.json` left pointing at the old PID.
    SkippedTakeover { old_pid: u32 },
    /// Enterprise managed-settings.json blocks spyc.
    BlockedByEnterprise,
    /// Enterprise managed-mcp.json already defines spyc — Claude
    /// resolves through the org config; we run the socket server but
    /// skip writing local `.mcp.json` (and clean up any prior write).
    ManagedByEnterprise,
}

/// Detect a live spyc instance currently owning MCP for `dir` without
/// modifying `.mcp.json`. Returns the old instance's PID if its socket
/// is reachable, else None. Used by the startup takeover prompt so we
/// can ask the user before clobbering another instance's registration.
pub fn detect_existing_spyc(dir: &Path) -> Option<u32> {
    let our_sock = socket_path();
    let path = dir.join(".mcp.json");
    let text = std::fs::read_to_string(&path).ok()?;
    let parsed: Value = serde_json::from_str(&text).ok()?;
    let old_sock_str = parsed
        .pointer("/mcpServers/spyc/env/SPYC_MCP_SOCK")
        .and_then(|v| v.as_str())?;
    let old_sock = PathBuf::from(old_sock_str);
    if old_sock == our_sock {
        return None;
    }
    UnixStream::connect(&old_sock).ok()?;
    pid_from_sock_path(old_sock_str)
}

/// Well-known paths for Claude Code enterprise managed settings.
const MANAGED_SETTINGS_PATHS: &[&str] = &[
    // macOS system-wide
    "/Library/Application Support/ClaudeCode/managed-settings.json",
    // Linux / WSL system-wide
    "/etc/claude-code/managed-settings.json",
];

/// Well-known paths for Claude Code enterprise-deployed MCP definitions.
/// When this file exists and defines a server named "spyc", the org has
/// already wired Claude → spyc and our per-project `.mcp.json` writes
/// are redundant (and just collide on the server name).
const MANAGED_MCP_PATHS: &[&str] = &[
    "/Library/Application Support/ClaudeCode/managed-mcp.json",
    "/etc/claude-code/managed-mcp.json",
];

/// Check whether enterprise managed-settings.json blocks "spyc".
/// Checks `deniedMcpServers` (by serverName) and `allowedMcpServers`.
/// Returns `None` if no enterprise config exists or if there's no
/// restriction. Returns `Some(false)` if spyc is denied or not in
/// an allowlist.
fn enterprise_allows_spyc() -> Option<bool> {
    for path in MANAGED_SETTINGS_PATHS {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        // Denylist takes absolute precedence.
        if let Some(denied) = parsed.get("deniedMcpServers")
            && let Some(arr) = denied.as_array()
            && arr
                .iter()
                .any(|entry| entry["serverName"].as_str() == Some("spyc"))
        {
            return Some(false);
        }
        // Allowlist: if present, spyc must be in it.
        if let Some(allowed) = parsed.get("allowedMcpServers")
            && let Some(arr) = allowed.as_array()
        {
            let ok = arr
                .iter()
                .any(|entry| entry["serverName"].as_str() == Some("spyc"));
            return Some(ok);
        }
        return None; // Enterprise config exists but no MCP restrictions.
    }
    None // No enterprise config found.
}

/// True when an enterprise-deployed `managed-mcp.json` defines a
/// server named "spyc". In that case Claude already knows how to
/// reach us and we should not also write per-project `.mcp.json`
/// files (a name collision results in Claude picking the org
/// definition, with the per-project entry only adding noise).
pub fn enterprise_defines_spyc() -> bool {
    for path in MANAGED_MCP_PATHS {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if parsed.pointer("/mcpServers/spyc").is_some() {
            return true;
        }
    }
    false
}

/// Extract the PID from a socket path like `mcp-12345.sock`.
pub fn ensure_mcp_json(dir: &Path, takeover_allowed: bool) -> Result<McpConfigStatus, io::Error> {
    if enterprise_allows_spyc() == Some(false) {
        return Ok(McpConfigStatus::BlockedByEnterprise);
    }

    if enterprise_defines_spyc() {
        // Org config (managed-mcp.json) is the source of truth for the
        // "spyc" server identifier — anything we write to .mcp.json just
        // collides with it. Remove any prior spyc entry we (or an older
        // spyc) wrote, preserving any other servers the user has defined.
        // If the file only contained spyc, remove it entirely.
        clean_local_mcp_entry(dir);
        return Ok(McpConfigStatus::ManagedByEnterprise);
    }

    let our_sock = socket_path();
    let our_pid = std::process::id();
    let path = dir.join(".mcp.json");
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("spyc"));

    // Check for an existing live spyc instance in this directory.
    let mut took_over: Option<u32> = None;
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(parsed) = serde_json::from_str::<Value>(&text)
        && let Some(old_sock_str) = parsed
            .pointer("/mcpServers/spyc/env/SPYC_MCP_SOCK")
            .and_then(|v| v.as_str())
    {
        let old_sock = PathBuf::from(old_sock_str);
        if old_sock != our_sock {
            // Another instance — check if it's still alive.
            if UnixStream::connect(&old_sock).is_ok() {
                let old_pid = pid_from_sock_path(old_sock_str).unwrap_or(0);
                if !takeover_allowed {
                    mcp_log(&format!(
                        "skipped takeover from PID {old_pid} ({})",
                        old_sock.display()
                    ));
                    return Ok(McpConfigStatus::SkippedTakeover { old_pid });
                }
                notify_disconnect(&old_sock, our_pid);
                took_over = Some(old_pid);
                mcp_log(&format!(
                    "taking over from PID {old_pid} ({})",
                    old_sock.display()
                ));
            }
        }
    }

    let spyc_entry = json!({
        "command": exe.to_string_lossy(),
        "args": ["--mcp"],
        "env": {
            "SPYC_MCP_SOCK": our_sock.to_string_lossy()
        }
    });

    // Default content when there's no existing file or we can't safely
    // splice into it (parse error, top-level not an object, mcpServers
    // present but not an object). In all those cases we overwrite with
    // a clean shape rather than panicking on `.as_object_mut().unwrap()`.
    let fresh =
        || serde_json::to_string_pretty(&json!({ "mcpServers": { "spyc": spyc_entry } })).unwrap();
    let content = match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(mut parsed) => {
                let top = parsed.as_object_mut();
                let servers = top.and_then(|t| {
                    let entry = t.entry("mcpServers").or_insert_with(|| json!({}));
                    entry.as_object_mut()
                });
                match servers {
                    Some(map) => {
                        map.insert("spyc".to_string(), spyc_entry);
                        serde_json::to_string_pretty(&parsed)
                            .expect("serializing a serde_json::Value cannot fail")
                    }
                    None => fresh(),
                }
            }
            Err(_) => fresh(),
        },
        Err(_) => fresh(),
    };

    std::fs::write(&path, content + "\n")?;
    mcp_log(&format!(
        "wrote .mcp.json (sock={}, exe={})",
        our_sock.display(),
        exe.display()
    ));

    match took_over {
        Some(old_pid) => Ok(McpConfigStatus::TookOver { old_pid }),
        None => Ok(McpConfigStatus::Configured),
    }
}

/// Codex's equivalent of `ensure_mcp_json`. Writes a stdio MCP entry
/// for spyc into `<dir>/.codex/config.toml` so the codex CLI discovers
/// us automatically, the same way claude does via `.mcp.json`. Codex
/// reads both `~/.codex/config.toml` (user-scope) and
/// `<cwd>/.codex/config.toml` (project-scope); we only ever write the
/// project file to mirror claude's project-scoped behavior and avoid
/// touching the user's main config.
///
/// Codex's TOML schema is `[mcp_servers.<name>]` with `command`,
/// `args`, and `env` keys for stdio servers (parallel to
/// claude's `.mcp.json` shape):
///
/// ```toml
/// [mcp_servers.spyc]
/// command = "spyc"
/// args = ["--mcp"]
///
/// [mcp_servers.spyc.env]
/// SPYC_MCP_SOCK = "/Users/x/.local/state/spyc/mcp-12345.sock"
/// ```
///
/// Takeover semantics match `ensure_mcp_json`: an existing live spyc
/// socket in another PID gets a `spyc/disconnected` notification and
/// we replace the entry. Enterprise policies are claude-specific and
/// don't apply here.
pub fn ensure_codex_config_toml(
    dir: &Path,
    takeover_allowed: bool,
) -> Result<McpConfigStatus, io::Error> {
    let our_sock = socket_path();
    let our_pid = std::process::id();
    let path = dir.join(".codex").join("config.toml");
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("spyc"));

    // Takeover detection: existing entry pointing at a different
    // live socket means another spyc instance owns this directory.
    let mut took_over: Option<u32> = None;
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(parsed) = toml::from_str::<toml::Value>(&text)
        && let Some(old_sock_str) = parsed
            .get("mcp_servers")
            .and_then(|m| m.get("spyc"))
            .and_then(|s| s.get("env"))
            .and_then(|e| e.get("SPYC_MCP_SOCK"))
            .and_then(toml::Value::as_str)
    {
        let old_sock = PathBuf::from(old_sock_str);
        if old_sock != our_sock && UnixStream::connect(&old_sock).is_ok() {
            let old_pid = pid_from_sock_path(old_sock_str).unwrap_or(0);
            if !takeover_allowed {
                mcp_log(&format!(
                    "codex: skipped takeover from PID {old_pid} ({})",
                    old_sock.display()
                ));
                return Ok(McpConfigStatus::SkippedTakeover { old_pid });
            }
            notify_disconnect(&old_sock, our_pid);
            took_over = Some(old_pid);
            mcp_log(&format!(
                "codex: taking over from PID {old_pid} ({})",
                old_sock.display()
            ));
        }
    }

    // Build a fresh `[mcp_servers.spyc]` table — used both as the
    // splice target and as the whole-file fallback when the existing
    // file is malformed or has the wrong shape (top-level not a
    // table, mcp_servers not a table, etc.).
    let build_entry = || {
        let mut env_table = toml::Table::new();
        env_table.insert(
            "SPYC_MCP_SOCK".into(),
            toml::Value::String(our_sock.to_string_lossy().into_owned()),
        );
        let mut entry = toml::Table::new();
        entry.insert(
            "command".into(),
            toml::Value::String(exe.to_string_lossy().into_owned()),
        );
        entry.insert(
            "args".into(),
            toml::Value::Array(vec![toml::Value::String("--mcp".into())]),
        );
        entry.insert("env".into(), toml::Value::Table(env_table));
        entry
    };
    let fresh = || {
        let mut servers = toml::Table::new();
        servers.insert("spyc".into(), toml::Value::Table(build_entry()));
        let mut root = toml::Table::new();
        root.insert("mcp_servers".into(), toml::Value::Table(servers));
        toml::to_string_pretty(&toml::Value::Table(root)).unwrap_or_default()
    };

    let content = match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<toml::Value>(&text) {
            Ok(mut parsed) => {
                let top = parsed.as_table_mut();
                let servers_ok = top.and_then(|t| {
                    let entry = t
                        .entry("mcp_servers")
                        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
                    entry.as_table_mut()
                });
                match servers_ok {
                    Some(map) => {
                        map.insert("spyc".to_string(), toml::Value::Table(build_entry()));
                        toml::to_string_pretty(&parsed).unwrap_or_else(|_| fresh())
                    }
                    None => fresh(),
                }
            }
            Err(_) => fresh(),
        },
        Err(_) => fresh(),
    };

    // Create the `.codex/` parent directory if missing.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    mcp_log(&format!(
        "wrote .codex/config.toml (sock={}, exe={})",
        our_sock.display(),
        exe.display()
    ));

    match took_over {
        Some(old_pid) => Ok(McpConfigStatus::TookOver { old_pid }),
        None => Ok(McpConfigStatus::Configured),
    }
}

/// Detect a live spyc instance currently owning codex MCP for `dir`
/// without modifying `.codex/config.toml`. Mirrors
/// `detect_existing_spyc` for the codex side; used by startup so a
/// single takeover prompt covers both claude and codex.
pub fn detect_existing_spyc_codex(dir: &Path) -> Option<u32> {
    let our_sock = socket_path();
    let path = dir.join(".codex").join("config.toml");
    let text = std::fs::read_to_string(&path).ok()?;
    let parsed: toml::Value = toml::from_str(&text).ok()?;
    let old_sock_str = parsed
        .get("mcp_servers")?
        .get("spyc")?
        .get("env")?
        .get("SPYC_MCP_SOCK")?
        .as_str()?;
    let old_sock = PathBuf::from(old_sock_str);
    if old_sock == our_sock {
        return None;
    }
    UnixStream::connect(&old_sock).ok()?;
    pid_from_sock_path(old_sock_str)
}

/// Remove just the "spyc" entry from `<dir>/.mcp.json` if present,
/// preserving any other servers the user (or another tool) may have
/// added. If after removal `mcpServers` is empty *and* no other
/// top-level keys remain, delete the file. All errors are best-effort
/// — this is cleanup, not load-bearing.
fn clean_local_mcp_entry(dir: &Path) {
    let path = dir.join(".mcp.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut parsed) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let Some(root) = parsed.as_object_mut() else {
        return;
    };
    let Some(servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return;
    };
    if servers.remove("spyc").is_none() {
        return;
    }
    let servers_empty = servers.is_empty();
    let only_servers = root.len() == 1; // i.e. just `mcpServers`
    if only_servers && servers_empty {
        let _ = std::fs::remove_file(&path);
        mcp_log(&format!(
            "removed empty .mcp.json after cleaning spyc entry ({})",
            path.display()
        ));
        return;
    }
    if let Ok(out) = serde_json::to_string_pretty(&parsed) {
        let _ = std::fs::write(&path, out + "\n");
        mcp_log(&format!(
            "cleaned spyc entry from .mcp.json (preserved other servers, {})",
            path.display()
        ));
    }
}
