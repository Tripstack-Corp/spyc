//! Managing the client-side MCP config (.mcp.json / codex config.toml) and
//! detecting/handing off existing spyc instances. Split out of mcp.rs verbatim.
use std::io::{self};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::server::{notify_disconnect, pid_from_sock_path};
use super::{mcp_log, socket_path};

/// Status of MCP configuration for this directory.
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

/// Given the `SPYC_MCP_SOCK` value parsed out of an existing client config,
/// return the live owner's PID — or `None` if it points at our own socket,
/// isn't reachable (stale registration), or has no parseable PID. The shared
/// tail of both `detect_existing_spyc*` functions; only the per-format
/// parsing of `old_sock_str` differs between them.
fn live_owner_pid(old_sock_str: &str, our_sock: &Path) -> Option<u32> {
    let old_sock = PathBuf::from(old_sock_str);
    if old_sock == *our_sock {
        return None;
    }
    UnixStream::connect(&old_sock).ok()?;
    pid_from_sock_path(old_sock_str)
}

/// Outcome of the takeover check shared by `ensure_mcp_json` and
/// `ensure_codex_config_toml`.
enum TakeoverDecision {
    /// No live conflicting instance (it's us, stale, or unreachable) —
    /// write our entry normally.
    Proceed,
    /// A live instance was found and we took it over (already notified).
    TookOver(u32),
    /// A live instance was found but takeover wasn't allowed — caller bails,
    /// leaving the old registration in place.
    Skipped(u32),
}

/// Decide whether to take over from the instance an existing config points
/// at. `old_sock_str` is the `SPYC_MCP_SOCK` already parsed out of the config
/// (JSON or TOML — the parsing differs per format, this decision does not).
/// On a live takeover this sends the disconnect notification as a side effect;
/// `log_prefix` (`""` / `"codex: "`) distinguishes the two in the debug log.
fn decide_takeover(
    old_sock_str: &str,
    our_sock: &Path,
    our_pid: u32,
    takeover_allowed: bool,
    log_prefix: &str,
) -> TakeoverDecision {
    let old_sock = PathBuf::from(old_sock_str);
    if old_sock == *our_sock || UnixStream::connect(&old_sock).is_err() {
        // Our own socket, or a dead one (stale registration) — no conflict.
        return TakeoverDecision::Proceed;
    }
    let old_pid = pid_from_sock_path(old_sock_str).unwrap_or(0);
    if !takeover_allowed {
        mcp_log(&format!(
            "{log_prefix}skipped takeover from PID {old_pid} ({})",
            old_sock.display()
        ));
        return TakeoverDecision::Skipped(old_pid);
    }
    notify_disconnect(&old_sock, our_pid);
    mcp_log(&format!(
        "{log_prefix}taking over from PID {old_pid} ({})",
        old_sock.display()
    ));
    TakeoverDecision::TookOver(old_pid)
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
    live_owner_pid(old_sock_str, &our_sock)
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

/// Ensure `.mcp.json` has the spyc entry using stdio transport.
/// Checks enterprise policy first. If another spyc instance owns
/// the entry, sends it a disconnect notification and takes over.
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
        match decide_takeover(old_sock_str, &our_sock, our_pid, takeover_allowed, "") {
            TakeoverDecision::Proceed => {}
            TakeoverDecision::TookOver(old_pid) => took_over = Some(old_pid),
            TakeoverDecision::Skipped(old_pid) => {
                return Ok(McpConfigStatus::SkippedTakeover { old_pid });
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

    crate::fs::write_atomic(&path, (content + "\n").as_bytes())?;
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
        match decide_takeover(
            old_sock_str,
            &our_sock,
            our_pid,
            takeover_allowed,
            "codex: ",
        ) {
            TakeoverDecision::Proceed => {}
            TakeoverDecision::TookOver(old_pid) => took_over = Some(old_pid),
            TakeoverDecision::Skipped(old_pid) => {
                return Ok(McpConfigStatus::SkippedTakeover { old_pid });
            }
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
    crate::fs::write_atomic(&path, content.as_bytes())?;
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
    live_owner_pid(old_sock_str, &our_sock)
}

/// Remove just the "spyc" entry from `<dir>/.mcp.json`. Enterprise path:
/// removes *any* spyc entry unconditionally (org config owns the name),
/// ignoring ownership and git-tracking.
fn clean_local_mcp_entry(dir: &Path) {
    let _ = remove_spyc_from_mcp_json(dir, |_| true, false);
}

/// Outcome of a teardown cleanup attempt for one client-config file.
pub enum ConfigCleanup {
    /// Our entry was removed (and the file/dir deleted if left empty).
    Cleaned,
    /// Nothing of ours to clean — no file, or the entry isn't ours.
    NothingToDo,
    /// The file is ours but tracked in git, so it was left untouched; the
    /// caller should warn the user rather than dirty a committed file.
    SkippedTracked,
}

/// True when `sock_str` is *our* PID-scoped MCP socket — i.e. this entry was
/// written by this running spyc, not a different (possibly successor) instance.
/// Our socket path embeds our pid, so this is a sound "did we write it" proxy.
fn sock_is_ours(sock_str: &str) -> bool {
    socket_path().to_string_lossy() == sock_str
}

/// Shared core for `.mcp.json` spyc-entry removal. `should_remove` is given the
/// entry's `SPYC_MCP_SOCK` value (`None` if absent) and decides whether to
/// remove it — `sock_is_ours` for teardown (never disturb a successor's entry),
/// dead-PID for the orphan sweep, unconditional for the enterprise path.
/// `guard_tracked` refuses to touch a git-tracked file (never dirty/delete
/// something the user committed). On removal, if `mcpServers` is empty *and* no
/// other top-level keys remain, the file is deleted. All errors are
/// best-effort: this is cleanup, not load-bearing.
fn remove_spyc_from_mcp_json(
    dir: &Path,
    should_remove: impl Fn(Option<&str>) -> bool,
    guard_tracked: bool,
) -> ConfigCleanup {
    let path = dir.join(".mcp.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return ConfigCleanup::NothingToDo;
    };
    let Ok(mut parsed) = serde_json::from_str::<Value>(&text) else {
        return ConfigCleanup::NothingToDo;
    };
    let sock = parsed
        .pointer("/mcpServers/spyc/env/SPYC_MCP_SOCK")
        .and_then(Value::as_str);
    if !should_remove(sock) {
        return ConfigCleanup::NothingToDo;
    }
    if guard_tracked && crate::git::discovery::is_tracked(&path) {
        return ConfigCleanup::SkippedTracked;
    }
    let Some(root) = parsed.as_object_mut() else {
        return ConfigCleanup::NothingToDo;
    };
    let Some(servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return ConfigCleanup::NothingToDo;
    };
    if servers.remove("spyc").is_none() {
        return ConfigCleanup::NothingToDo;
    }
    let servers_empty = servers.is_empty();
    let only_servers = root.len() == 1; // i.e. just `mcpServers`
    if only_servers && servers_empty {
        let _ = std::fs::remove_file(&path);
        mcp_log(&format!(
            "removed empty .mcp.json after cleaning spyc entry ({})",
            path.display()
        ));
        return ConfigCleanup::Cleaned;
    }
    if let Ok(out) = serde_json::to_string_pretty(&parsed) {
        let _ = crate::fs::write_atomic(&path, (out + "\n").as_bytes());
        mcp_log(&format!(
            "cleaned spyc entry from .mcp.json (preserved other servers, {})",
            path.display()
        ));
    }
    ConfigCleanup::Cleaned
}

/// Teardown counterpart to [`ensure_mcp_json`]: remove the spyc entry *we*
/// wrote from `<dir>/.mcp.json`, deleting the file if it's left empty. Leaves a
/// successor instance's entry and any git-tracked file untouched.
pub fn cleanup_mcp_json(dir: &Path) -> ConfigCleanup {
    remove_spyc_from_mcp_json(dir, |sock| sock.is_some_and(sock_is_ours), true)
}

/// Teardown counterpart to [`ensure_codex_config_toml`]: remove the spyc entry
/// *we* wrote from `<dir>/.codex/config.toml`, preserving any other codex
/// config the user has. If that empties the file, delete it and then the
/// `.codex/` directory too (only when it's now empty — `remove_dir` is a no-op
/// otherwise, so a `.codex/` holding other files is left alone). Leaves a
/// successor's entry and any git-tracked file untouched.
pub fn cleanup_codex_config(dir: &Path) -> ConfigCleanup {
    remove_spyc_from_codex_config(dir, |sock| sock.is_some_and(sock_is_ours), true)
}

/// Shared core for `.codex/config.toml` spyc-entry removal — the codex
/// counterpart of [`remove_spyc_from_mcp_json`]. `should_remove` / `guard_tracked`
/// as there. Preserves any other codex config; deletes `config.toml` only when
/// nothing else remains, and the `.codex/` dir only via `remove_dir` — a no-op
/// unless it's now empty, so a `.codex/` holding other files is always left
/// alone (the "only delete if empty, no non-spyc config" rule).
fn remove_spyc_from_codex_config(
    dir: &Path,
    should_remove: impl Fn(Option<&str>) -> bool,
    guard_tracked: bool,
) -> ConfigCleanup {
    let codex_dir = dir.join(".codex");
    let path = codex_dir.join("config.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return ConfigCleanup::NothingToDo;
    };
    let Ok(mut parsed) = toml::from_str::<toml::Value>(&text) else {
        return ConfigCleanup::NothingToDo;
    };
    let sock = parsed
        .get("mcp_servers")
        .and_then(|m| m.get("spyc"))
        .and_then(|s| s.get("env"))
        .and_then(|e| e.get("SPYC_MCP_SOCK"))
        .and_then(toml::Value::as_str);
    if !should_remove(sock) {
        return ConfigCleanup::NothingToDo;
    }
    if guard_tracked && crate::git::discovery::is_tracked(&path) {
        return ConfigCleanup::SkippedTracked;
    }
    let Some(root) = parsed.as_table_mut() else {
        return ConfigCleanup::NothingToDo;
    };
    let mut removed = false;
    if let Some(servers) = root
        .get_mut("mcp_servers")
        .and_then(toml::Value::as_table_mut)
    {
        removed = servers.remove("spyc").is_some();
        if servers.is_empty() {
            root.remove("mcp_servers");
        }
    }
    if !removed {
        return ConfigCleanup::NothingToDo;
    }
    if root.is_empty() {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&codex_dir);
        mcp_log(&format!(
            "removed empty .codex/config.toml after cleaning spyc entry ({})",
            path.display()
        ));
    } else if let Ok(out) = toml::to_string_pretty(&parsed) {
        let _ = crate::fs::write_atomic(&path, out.as_bytes());
        mcp_log(&format!(
            "cleaned spyc entry from .codex/config.toml (preserved other config, {})",
            path.display()
        ));
    }
    ConfigCleanup::Cleaned
}

/// Startup orphan sweep: reap dead-PID spyc MCP entries that instances killed
/// without running teardown left behind in `dir`. Removes the `spyc` entry from
/// `.mcp.json` and `.codex/config.toml` ONLY when its socket PID is dead and
/// isn't `our_pid` — never a live owner's entry — reusing the conservative
/// removal (preserve any other config/servers, delete an emptied file / `.codex`
/// dir, skip a git-tracked file). Returns how many entries it cleaned.
pub fn sweep_orphan_spyc_configs(dir: &Path, our_pid: u32) -> usize {
    // `move` captures `our_pid` (Copy) by value → the closure is itself `Copy`,
    // so it can be handed to both removers by value (no borrow).
    let is_dead_orphan = move |sock: Option<&str>| {
        sock.and_then(pid_from_sock_path)
            .is_some_and(|pid| pid != our_pid && !crate::sysinfo::pid_alive(pid))
    };
    let mut cleaned = 0;
    if matches!(
        remove_spyc_from_mcp_json(dir, is_dead_orphan, true),
        ConfigCleanup::Cleaned
    ) {
        cleaned += 1;
    }
    if matches!(
        remove_spyc_from_codex_config(dir, is_dead_orphan, true),
        ConfigCleanup::Cleaned
    ) {
        cleaned += 1;
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::{TakeoverDecision, decide_takeover, live_owner_pid};
    use std::path::Path;

    // The takeover/detection helpers' deterministic branches (no live
    // socket): an entry pointing at our own socket, or at a dead one, must
    // never trigger a takeover. The live-socket TookOver/Skipped branches
    // share the same `UnixStream::connect` call and are exercised by the
    // end-to-end takeover behavior.

    #[test]
    fn decide_takeover_proceeds_on_own_socket() {
        let sock = Path::new("/run/spyc/mcp-1.sock");
        // old == our socket → no conflict, even with takeover disallowed.
        assert!(matches!(
            decide_takeover(&sock.to_string_lossy(), sock, 1, false, ""),
            TakeoverDecision::Proceed
        ));
    }

    #[test]
    fn decide_takeover_proceeds_on_dead_socket() {
        // A path that can't be connected to (no listener) is a stale
        // registration → proceed, don't try to take it over.
        let dead = "/nonexistent/spyc-mcp-does-not-exist.sock";
        let our = Path::new("/run/spyc/mcp-2.sock");
        assert!(matches!(
            decide_takeover(dead, our, 2, true, ""),
            TakeoverDecision::Proceed
        ));
    }

    #[test]
    fn live_owner_pid_none_for_own_or_dead_socket() {
        let our = Path::new("/run/spyc/mcp-3.sock");
        assert_eq!(live_owner_pid(&our.to_string_lossy(), our), None);
        assert_eq!(live_owner_pid("/nonexistent/spyc-mcp-nope.sock", our), None);
    }

    // --- teardown cleanup ---
    use super::{ConfigCleanup, cleanup_codex_config, cleanup_mcp_json, sweep_orphan_spyc_configs};

    fn our_sock() -> String {
        crate::mcp::socket_path().to_string_lossy().into_owned()
    }

    /// A `.codex/config.toml` whose spyc entry points at `sock`, written into a
    /// fresh `.codex` under `dir`. Returns the config path.
    fn write_codex_with_sock(dir: &Path, sock: &str) -> std::path::PathBuf {
        let codex = dir.join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        let cfg = codex.join("config.toml");
        std::fs::write(
            &cfg,
            format!("[mcp_servers.spyc.env]\nSPYC_MCP_SOCK = \"{sock}\"\n"),
        )
        .unwrap();
        cfg
    }

    #[test]
    fn cleanup_codex_removes_our_entry_and_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let codex = tmp.path().join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        let cfg = codex.join("config.toml");
        std::fs::write(
            &cfg,
            format!(
                "[mcp_servers.spyc]\ncommand = \"spyc\"\nargs = [\"--mcp\"]\n[mcp_servers.spyc.env]\nSPYC_MCP_SOCK = \"{}\"\n",
                our_sock()
            ),
        )
        .unwrap();
        assert!(matches!(
            cleanup_codex_config(tmp.path()),
            ConfigCleanup::Cleaned
        ));
        assert!(!cfg.exists(), "config.toml should be deleted");
        assert!(!codex.exists(), "empty .codex dir should be removed");
    }

    #[test]
    fn cleanup_codex_preserves_a_foreign_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let codex = tmp.path().join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        let cfg = codex.join("config.toml");
        // Points at a *different* socket → not ours, leave it alone.
        std::fs::write(
            &cfg,
            "[mcp_servers.spyc.env]\nSPYC_MCP_SOCK = \"/run/other/mcp-999.sock\"\n",
        )
        .unwrap();
        assert!(matches!(
            cleanup_codex_config(tmp.path()),
            ConfigCleanup::NothingToDo
        ));
        assert!(cfg.exists(), "a foreign entry must be left untouched");
    }

    // --- startup orphan sweep (dead-PID entries from killed instances) ---

    #[test]
    fn orphan_sweep_reaps_dead_pid_codex_entry_and_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // PID 999999999 is effectively never live → an orphan.
        let cfg = write_codex_with_sock(tmp.path(), "/x/mcp-999999999.sock");
        let cleaned = sweep_orphan_spyc_configs(tmp.path(), std::process::id());
        assert_eq!(cleaned, 1, "the dead-PID entry is reaped");
        assert!(!cfg.exists(), "sole spyc entry → config.toml removed");
        assert!(
            !tmp.path().join(".codex").exists(),
            "emptied .codex dir removed"
        );
    }

    #[test]
    fn orphan_sweep_preserves_other_codex_config() {
        let tmp = tempfile::tempdir().unwrap();
        let codex = tmp.path().join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        let cfg = codex.join("config.toml");
        std::fs::write(
            &cfg,
            "model = \"o3\"\n[mcp_servers.spyc.env]\nSPYC_MCP_SOCK = \"/x/mcp-999999999.sock\"\n",
        )
        .unwrap();
        let cleaned = sweep_orphan_spyc_configs(tmp.path(), std::process::id());
        assert_eq!(cleaned, 1);
        let after = std::fs::read_to_string(&cfg).expect("file kept — other config present");
        assert!(after.contains("model"), "non-spyc config preserved");
        assert!(!after.contains("spyc"), "spyc entry removed");
        assert!(codex.exists(), ".codex dir kept (config.toml still there)");
    }

    #[test]
    fn orphan_sweep_spares_live_owner_entry() {
        // The sock embeds OUR (alive) PID → not an orphan → left intact, so a
        // running instance's registration is never swept out from under it.
        let tmp = tempfile::tempdir().unwrap();
        let our = std::process::id();
        let cfg = write_codex_with_sock(tmp.path(), &format!("/x/mcp-{our}.sock"));
        let cleaned = sweep_orphan_spyc_configs(tmp.path(), our);
        assert_eq!(cleaned, 0, "a live (our) PID is not an orphan");
        assert!(cfg.exists(), "live owner's entry untouched");
    }

    #[test]
    fn cleanup_codex_preserves_other_config_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let codex = tmp.path().join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        let cfg = codex.join("config.toml");
        std::fs::write(
            &cfg,
            format!(
                "model = \"gpt-5\"\n[mcp_servers.spyc.env]\nSPYC_MCP_SOCK = \"{}\"\n",
                our_sock()
            ),
        )
        .unwrap();
        assert!(matches!(
            cleanup_codex_config(tmp.path()),
            ConfigCleanup::Cleaned
        ));
        let after = std::fs::read_to_string(&cfg).expect("file kept (other config present)");
        assert!(after.contains("model"), "user's other config preserved");
        assert!(!after.contains("spyc"), "our entry removed");
        assert!(codex.exists(), ".codex dir kept (config.toml still there)");
    }

    #[test]
    fn cleanup_mcp_json_removes_our_entry_when_sole() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".mcp.json");
        std::fs::write(
            &path,
            format!(
                "{{\"mcpServers\":{{\"spyc\":{{\"env\":{{\"SPYC_MCP_SOCK\":\"{}\"}}}}}}}}",
                our_sock()
            ),
        )
        .unwrap();
        assert!(matches!(
            cleanup_mcp_json(tmp.path()),
            ConfigCleanup::Cleaned
        ));
        assert!(!path.exists(), "sole-spyc .mcp.json should be deleted");
    }

    /// A git-TRACKED (committed) `.mcp.json` is left byte-for-byte intact:
    /// `guard_tracked` refuses to dirty/delete a config the user committed.
    /// Every other cleanup test uses a plain (non-git) tempdir, so `is_tracked`
    /// is always false and the `SkippedTracked` branch — the load-bearing safety
    /// guard — never ran; a regression dropping it would silently rewrite a
    /// committed config with no failing test.
    #[test]
    fn cleanup_skips_a_git_tracked_mcp_json() {
        let run_git = |dir: &std::path::Path, args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@x")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@x")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .status()
                .expect("spawn git")
                .success();
            assert!(ok, "git {args:?} failed");
        };
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        run_git(repo, &["init", "-q", "--initial-branch=main"]);
        let path = repo.join(".mcp.json");
        let body = format!(
            "{{\"mcpServers\":{{\"spyc\":{{\"env\":{{\"SPYC_MCP_SOCK\":\"{}\"}}}}}}}}",
            our_sock()
        );
        std::fs::write(&path, &body).unwrap();
        run_git(repo, &["add", ".mcp.json"]);
        run_git(repo, &["commit", "-q", "-m", "add mcp config"]);

        assert!(
            matches!(cleanup_mcp_json(repo), ConfigCleanup::SkippedTracked),
            "committed .mcp.json with our entry → SkippedTracked, not Cleaned"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            body,
            "the tracked config is left byte-for-byte intact"
        );
    }

    #[test]
    fn cleanup_mcp_json_preserves_other_servers() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".mcp.json");
        std::fs::write(
            &path,
            format!(
                "{{\"mcpServers\":{{\"spyc\":{{\"env\":{{\"SPYC_MCP_SOCK\":\"{}\"}}}},\"other\":{{\"command\":\"x\"}}}}}}",
                our_sock()
            ),
        )
        .unwrap();
        assert!(matches!(
            cleanup_mcp_json(tmp.path()),
            ConfigCleanup::Cleaned
        ));
        let after = std::fs::read_to_string(&path).expect("file kept (other server present)");
        assert!(after.contains("other"), "other server preserved");
        assert!(!after.contains("spyc"), "our entry removed");
    }

    #[test]
    fn cleanup_mcp_json_leaves_foreign_socket() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".mcp.json");
        std::fs::write(
            &path,
            "{\"mcpServers\":{\"spyc\":{\"env\":{\"SPYC_MCP_SOCK\":\"/run/other/mcp-1.sock\"}}}}",
        )
        .unwrap();
        assert!(matches!(
            cleanup_mcp_json(tmp.path()),
            ConfigCleanup::NothingToDo
        ));
        assert!(path.exists(), "a successor's entry must be left in place");
    }

    #[test]
    fn cleanup_is_noop_when_no_config_present() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(
            cleanup_mcp_json(tmp.path()),
            ConfigCleanup::NothingToDo
        ));
        assert!(matches!(
            cleanup_codex_config(tmp.path()),
            ConfigCleanup::NothingToDo
        ));
    }
}
