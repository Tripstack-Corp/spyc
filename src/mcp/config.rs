//! Managing the agents' MCP `spyc` entry (`.mcp.json`, codex `config.toml`,
//! agy `mcp_config.json`): writing it, and removing it once no spyc needs it.
use std::io::{self};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::server::pid_from_sock_path;
use super::{mcp_log, socket_path};

/// Status of MCP configuration for this directory.
#[derive(Debug)]
pub enum McpConfigStatus {
    /// The `spyc` entry is written.
    Configured,
    /// Enterprise managed-settings.json blocks spyc.
    BlockedByEnterprise,
    /// Enterprise managed-mcp.json already defines spyc — Claude
    /// resolves through the org config; we run the socket server but
    /// skip writing local `.mcp.json` (and clean up any prior write).
    ManagedByEnterprise,
}

/// An "I won't touch that file" error.
///
/// Returned instead of rewriting an agent config we can't safely splice into.
/// These files are the user's: `.mcp.json` can define other MCP servers,
/// `.codex/config.toml` holds their `model` and `approval_policy`. Replacing one
/// with a spyc-only document because a trailing comma made it unparseable is
/// silent data loss, and the caller's `Err` arm already flashes and lets the
/// pane launch — so refusing costs spyc's tools in that repo, not their config.
///
/// `InvalidData` rather than `Other`: the file exists and is readable, its
/// contents are the problem.
fn refuse(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// The variables an agent pane's env carries (`open_pane_tab_into`) that its
/// `spyc --mcp` proxy needs: which spyc launched it, and which tab it is.
const PANE_ENV: [&str; 2] = ["SPYC_MCP_SOCK", "SPYC_PANE_ID"];

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
fn enterprise_defines_spyc() -> bool {
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
pub fn ensure_mcp_json(dir: &Path) -> Result<McpConfigStatus, io::Error> {
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

    ensure_spyc_in_mcp_json(&dir.join(".mcp.json"))
}

/// Write spyc's stdio MCP entry into an `mcpServers`-shaped JSON file at `path`,
/// preserving any other servers already declared there.
///
/// Shared by claude's `<dir>/.mcp.json` and agy's `<dir>/.agents/mcp_config.json`
/// — the two formats are byte-identical, so the only difference is the path (and
/// the enterprise-policy gate, which is claude-specific and stays in
/// [`ensure_mcp_json`]).
pub(super) fn ensure_spyc_in_mcp_json(path: &Path) -> Result<McpConfigStatus, io::Error> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("spyc"));

    // SPYC-TRAP(mcp-entry-names-no-socket): no `env`. Claude and agy start the
    // proxy with the agent's own env, where the pane put its spyc's socket.
    let spyc_entry = json!({
        "command": exe.to_string_lossy(),
        "args": ["--mcp"],
    });

    // Default content, for an absent or blank file only — there's nothing to
    // lose in either case.
    let fresh = || {
        serde_json::to_string_pretty(&json!({ "mcpServers": { "spyc": spyc_entry } }))
            .expect("serializing a serde_json::Value cannot fail")
    };
    let content = match std::fs::read_to_string(path) {
        // An existing file is the user's data — it can hold other MCP servers,
        // and rewriting it from scratch drops every one. Refuse instead: the
        // caller flashes the error and the pane still launches, so the cost is
        // spyc's tools being unavailable in this repo until they fix the file.
        Ok(text) if !text.trim().is_empty() => {
            let mut parsed: Value = serde_json::from_str(&text).map_err(|e| {
                refuse(&format!(
                    "{}: refusing to overwrite — exists but isn't valid JSON ({e}); \
                     fix or delete it",
                    path.display()
                ))
            })?;
            let servers = parsed.as_object_mut().and_then(|t| {
                let entry = t.entry("mcpServers").or_insert_with(|| json!({}));
                entry.as_object_mut()
            });
            match servers {
                Some(map) => {
                    map.insert("spyc".to_string(), spyc_entry);
                    serde_json::to_string_pretty(&parsed)
                        .expect("serializing a serde_json::Value cannot fail")
                }
                None => {
                    return Err(refuse(&format!(
                        "{}: refusing to overwrite — top level or `mcpServers` is not an object",
                        path.display()
                    )));
                }
            }
        }
        Ok(_) => fresh(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => fresh(),
        // Anything else (permissions, a directory in the way) is not "no file";
        // writing over it would be a guess.
        Err(e) => {
            return Err(io::Error::new(
                e.kind(),
                format!("reading {}: {e}", path.display()),
            ));
        }
    };

    // agy's file sits one level down (`.agents/`); claude's parent is `dir`
    // itself, so this is a no-op there.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::fs::write_atomic(path, (content + "\n").as_bytes())?;
    mcp_log(&format!("wrote {} (exe={})", path.display(), exe.display()));
    Ok(McpConfigStatus::Configured)
}

/// Codex's equivalent of `ensure_mcp_json`. Writes a stdio MCP entry
/// for spyc into `<dir>/.codex/config.toml` so the codex CLI discovers
/// us automatically, the same way claude does via `.mcp.json`. Codex
/// reads both `~/.codex/config.toml` (user-scope) and
/// `<cwd>/.codex/config.toml` (project-scope); we only ever write the
/// project file to mirror claude's project-scoped behaviour and avoid
/// touching the user's main config.
///
/// Codex's TOML schema is `[mcp_servers.<name>]` with `command` and `args`
/// for a stdio server. Codex starts it with a cleared environment plus the
/// names `env_vars` lists, so that list is how the pane's socket and id reach
/// the proxy:
///
/// ```toml
/// [mcp_servers.spyc]
/// command = "spyc"
/// args = ["--mcp"]
/// env_vars = ["SPYC_MCP_SOCK", "SPYC_PANE_ID"]
/// ```
///
/// Enterprise policies are claude-specific and don't apply here.
pub fn ensure_codex_config_toml(dir: &Path) -> Result<McpConfigStatus, io::Error> {
    let path = dir.join(".codex").join("config.toml");
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("spyc"));

    // Build a fresh `[mcp_servers.spyc]` table — used both as the
    // splice target and as the whole-file fallback when the existing
    // file is malformed or has the wrong shape (top-level not a
    // table, mcp_servers not a table, etc.).
    let build_entry = || {
        let mut entry = toml::Table::new();
        entry.insert(
            "command".into(),
            toml::Value::String(exe.to_string_lossy().into_owned()),
        );
        entry.insert(
            "args".into(),
            toml::Value::Array(vec![toml::Value::String("--mcp".into())]),
        );
        // SPYC-TRAP(mcp-entry-names-no-socket): pass the pane's own socket
        // through; never pin one in `env`.
        entry.insert(
            "env_vars".into(),
            toml::Value::Array(PANE_ENV.map(|v| toml::Value::String(v.into())).to_vec()),
        );
        entry
    };
    let fresh = || {
        let mut servers = toml::Table::new();
        servers.insert("spyc".into(), toml::Value::Table(build_entry()));
        let mut root = toml::Table::new();
        root.insert("mcp_servers".into(), toml::Value::Table(servers));
        // A root holding one table and no bare values always serializes (TOML
        // requires values before tables, which a single key trivially satisfies).
        toml::to_string_pretty(&toml::Value::Table(root))
            .expect("a single-table root always serializes")
    };

    let content = match std::fs::read_to_string(&path) {
        // This file is the user's codex config — `model`, `approval_policy`,
        // their own hooks. Rewriting it from scratch because we couldn't parse
        // it (one trailing comma, a half-finished edit) destroys all of it.
        Ok(text) if !text.trim().is_empty() => {
            let mut parsed: toml::Value = toml::from_str(&text).map_err(|e| {
                refuse(&format!(
                    "{}: refusing to overwrite — exists but isn't valid TOML ({e}); \
                     fix or delete it",
                    path.display()
                ))
            })?;
            let servers_ok = parsed.as_table_mut().and_then(|t| {
                let entry = t
                    .entry("mcp_servers")
                    .or_insert_with(|| toml::Value::Table(toml::Table::new()));
                entry.as_table_mut()
            });
            match servers_ok {
                Some(map) => {
                    map.insert("spyc".to_string(), toml::Value::Table(build_entry()));
                    // Re-serializing a table we only inserted into can still
                    // fail on TOML's values-before-tables rule. Refuse rather
                    // than fall back to `fresh()` — that would be the clobber.
                    toml::to_string_pretty(&parsed).map_err(|e| {
                        refuse(&format!(
                            "{}: refusing to overwrite — merged config won't \
                             re-serialize ({e})",
                            path.display()
                        ))
                    })?
                }
                None => {
                    return Err(refuse(&format!(
                        "{}: refusing to overwrite — top level or `mcp_servers` is not a table",
                        path.display()
                    )));
                }
            }
        }
        Ok(_) => fresh(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => fresh(),
        Err(e) => {
            return Err(io::Error::new(
                e.kind(),
                format!("reading {}: {e}", path.display()),
            ));
        }
    };

    // Create the `.codex/` parent directory if missing.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::fs::write_atomic(&path, content.as_bytes())?;
    mcp_log(&format!("wrote .codex/config.toml (exe={})", exe.display()));
    Ok(McpConfigStatus::Configured)
}

/// Remove just the "spyc" entry from `<dir>/.mcp.json`. Enterprise path:
/// removes *any* spyc entry unconditionally (org config owns the name),
/// ignoring ownership and git-tracking.
fn clean_local_mcp_entry(dir: &Path) {
    let _ = remove_spyc_from_mcp_json(&dir.join(".mcp.json"), |_| true, false);
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

/// True when `sock_str` is *our* PID-scoped MCP socket. Our socket path embeds
/// our pid, so this is a sound "we wrote it" proxy for an entry pinned to it.
fn sock_is_ours(sock_str: &str) -> bool {
    // No state dir → we never had a socket, so no entry can be pinned to it.
    socket_path().is_some_and(|p| p.to_string_lossy() == sock_str)
}

/// Whether a `spyc` entry is free to remove once no live spyc claims the dir
/// (`state::dir_owners`). `sock` is the socket the entry pins, `None` for the
/// entry spyc writes now, which pins nothing. One pinned to another live spyc
/// was written by an older version still running there, which removes its own.
fn entry_is_unowned(sock: Option<&str>) -> bool {
    sock.is_none_or(|s| {
        sock_is_ours(s) || pid_from_sock_path(s).is_some_and(|pid| !crate::sysinfo::pid_alive(pid))
    })
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
    path: &Path,
    should_remove: impl Fn(Option<&str>) -> bool,
    guard_tracked: bool,
) -> ConfigCleanup {
    let Ok(text) = std::fs::read_to_string(path) else {
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
    if guard_tracked && crate::git::discovery::is_tracked(path) {
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
        let _ = std::fs::remove_file(path);
        mcp_log(&format!(
            "removed empty .mcp.json after cleaning spyc entry ({})",
            path.display()
        ));
        return ConfigCleanup::Cleaned;
    }
    if let Ok(out) = serde_json::to_string_pretty(&parsed) {
        let _ = crate::fs::write_atomic(path, (out + "\n").as_bytes());
        mcp_log(&format!(
            "cleaned spyc entry from .mcp.json (preserved other servers, {})",
            path.display()
        ));
    }
    ConfigCleanup::Cleaned
}

/// Teardown counterpart to [`ensure_mcp_json`], for the last spyc out of `dir`:
/// remove the spyc entry from `<dir>/.mcp.json`, deleting the file if it's left
/// empty. Leaves an entry an older live spyc pinned and any git-tracked file.
pub fn cleanup_mcp_json(dir: &Path) -> ConfigCleanup {
    remove_spyc_from_mcp_json(&dir.join(".mcp.json"), entry_is_unowned, true)
}

/// Teardown counterpart to [`ensure_codex_config_toml`], for the last spyc out
/// of `dir`: remove the spyc entry from `<dir>/.codex/config.toml`, preserving
/// any other codex config the user has. If that empties the file, delete it and
/// then the `.codex/` directory too (only when it's now empty — `remove_dir` is
/// a no-op otherwise, so a `.codex/` holding other files is left alone). Leaves
/// an entry an older live spyc pinned and any git-tracked file.
pub fn cleanup_codex_config(dir: &Path) -> ConfigCleanup {
    remove_spyc_from_codex_config(dir, entry_is_unowned, true)
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

/// Startup orphan sweep: reap the spyc MCP entries that instances killed
/// without running teardown left behind in `dir`. Does nothing while another
/// live spyc claims the dir (`state::dir_owners`); otherwise removes the `spyc`
/// entry from `.mcp.json`, `.codex/config.toml` and `.agents/mcp_config.json`
/// when [`entry_is_unowned`], reusing the conservative removal (preserve any
/// other config/servers, delete an emptied file / `.codex` / `.agents` dir, skip
/// a git-tracked file). Returns how many entries it cleaned.
pub fn sweep_orphan_spyc_configs(dir: &Path, our_pid: u32) -> usize {
    use crate::state::dir_owners::{Shared, claimed_by_another};
    if claimed_by_another(Shared::McpEntry, dir, our_pid) {
        return 0;
    }
    let is_dead_orphan = entry_is_unowned;
    let mut cleaned = 0;
    if matches!(
        remove_spyc_from_mcp_json(&dir.join(".mcp.json"), is_dead_orphan, true),
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
    // Agy `.agents/mcp_config.json` uses the same schema as `.mcp.json`.
    let agy_path = agy_mcp_config_path(dir);
    if matches!(
        remove_spyc_from_mcp_json(&agy_path, is_dead_orphan, true),
        ConfigCleanup::Cleaned
    ) {
        if !agy_path.exists() {
            let _ = std::fs::remove_dir(dir.join(".agents"));
        }
        cleaned += 1;
    }
    cleaned
}

/// Where agy discovers a workspace's MCP servers: a customization root in the
/// project (`<dir>/.agents/`), same schema as claude's `.mcp.json`.
fn agy_mcp_config_path(dir: &Path) -> PathBuf {
    dir.join(".agents/mcp_config.json")
}

/// Agy's counterpart to [`ensure_mcp_json`]. No enterprise-policy gate: that's a
/// claude managed-settings mechanism with no agy equivalent.
pub fn ensure_agy_mcp_config(dir: &Path) -> Result<McpConfigStatus, io::Error> {
    ensure_spyc_in_mcp_json(&agy_mcp_config_path(dir))
}

/// Teardown counterpart to [`ensure_agy_mcp_config`], for the last spyc out of
/// `dir`: drop the entry, then the `.agents/` dir if that left it empty. Leaves
/// an entry an older live spyc pinned and any git-tracked file.
pub fn cleanup_agy_mcp_config(dir: &Path) -> ConfigCleanup {
    let path = agy_mcp_config_path(dir);
    let result = remove_spyc_from_mcp_json(&path, entry_is_unowned, true);
    if matches!(result, ConfigCleanup::Cleaned) && !path.exists() {
        // Only succeeds while empty — a user's own hooks.json / skills/ keeps it.
        let _ = std::fs::remove_dir(dir.join(".agents"));
    }
    result
}
#[cfg(test)]
mod tests {
    use std::path::Path;

    // --- teardown cleanup ---
    use super::{
        ConfigCleanup, McpConfigStatus, Value, cleanup_agy_mcp_config, cleanup_codex_config,
        cleanup_mcp_json, ensure_agy_mcp_config, sweep_orphan_spyc_configs,
    };

    fn our_sock() -> String {
        crate::mcp::socket_path()
            .expect("tests run with HOME set")
            .to_string_lossy()
            .into_owned()
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
        // Pinned to another live spyc (pid 1 always runs): an older version
        // still running there, which removes its own.
        std::fs::write(
            &cfg,
            "[mcp_servers.spyc.env]\nSPYC_MCP_SOCK = \"/run/other/mcp-1.sock\"\n",
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

    // --- agy `.agents/mcp_config.json` (same schema as `.mcp.json`) ---

    fn write_agy_with_sock(dir: &Path, sock: &str) -> std::path::PathBuf {
        let agents = dir.join(".agents");
        std::fs::create_dir_all(&agents).unwrap();
        let path = agents.join("mcp_config.json");
        std::fs::write(
            &path,
            format!("{{\"mcpServers\":{{\"spyc\":{{\"env\":{{\"SPYC_MCP_SOCK\":\"{sock}\"}}}}}}}}"),
        )
        .unwrap();
        path
    }

    #[test]
    fn ensure_agy_writes_the_stdio_entry_and_creates_the_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(
            ensure_agy_mcp_config(tmp.path()),
            Ok(McpConfigStatus::Configured)
        ));
        let path = tmp.path().join(".agents/mcp_config.json");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            v.pointer("/mcpServers/spyc/args/0").and_then(Value::as_str),
            Some("--mcp"),
            "spyc registers itself as a stdio proxy"
        );
        assert!(
            v.pointer("/mcpServers/spyc/env").is_none(),
            "no socket pinned: the proxy uses the one its pane names"
        );
    }

    #[test]
    fn ensure_agy_preserves_a_foreign_server_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let agents = tmp.path().join(".agents");
        std::fs::create_dir_all(&agents).unwrap();
        let path = agents.join("mcp_config.json");
        std::fs::write(&path, "{\"mcpServers\":{\"other\":{\"command\":\"x\"}}}").unwrap();

        ensure_agy_mcp_config(tmp.path()).unwrap();
        let once = std::fs::read_to_string(&path).unwrap();
        assert!(once.contains("\"other\""), "a user's own server survives");
        assert!(once.contains("\"spyc\""));

        ensure_agy_mcp_config(tmp.path()).unwrap();
        assert_eq!(
            once,
            std::fs::read_to_string(&path).unwrap(),
            "re-running the writer changed a byte"
        );
    }

    #[test]
    fn cleanup_agy_removes_our_entry_and_the_emptied_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_agy_with_sock(tmp.path(), &our_sock());
        assert!(matches!(
            cleanup_agy_mcp_config(tmp.path()),
            ConfigCleanup::Cleaned
        ));
        assert!(!path.exists(), "sole-spyc mcp_config.json deleted");
        assert!(
            !tmp.path().join(".agents").exists(),
            "emptied .agents dir removed"
        );
    }

    #[test]
    fn cleanup_agy_leaves_a_foreign_socket_and_keeps_a_shared_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_agy_with_sock(tmp.path(), "/run/other/mcp-1.sock");
        // A sibling customization file the user owns — teardown must not take
        // `.agents/` down with it.
        std::fs::write(tmp.path().join(".agents/hooks.json"), "{}").unwrap();
        assert!(matches!(
            cleanup_agy_mcp_config(tmp.path()),
            ConfigCleanup::NothingToDo
        ));
        assert!(
            path.exists(),
            "another instance's entry is not ours to drop"
        );
        assert!(tmp.path().join(".agents").exists());
    }

    #[test]
    fn orphan_sweep_reaps_dead_pid_agy_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_agy_with_sock(tmp.path(), "/x/mcp-999999999.sock");
        assert_eq!(sweep_orphan_spyc_configs(tmp.path(), std::process::id()), 1);
        assert!(!path.exists());
        assert!(!tmp.path().join(".agents").exists());
    }

    /// The `.mcp.json` half of the sweep, which had no coverage: the shared
    /// remover takes a FILE path, and passing the containing directory instead
    /// still compiles (both are `&Path`) while silently reaping nothing.
    #[test]
    fn orphan_sweep_reaps_dead_pid_mcp_json_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".mcp.json");
        std::fs::write(
            &path,
            "{\"mcpServers\":{\"spyc\":{\"env\":{\"SPYC_MCP_SOCK\":\"/x/mcp-999999999.sock\"}}}}",
        )
        .unwrap();
        assert_eq!(
            sweep_orphan_spyc_configs(tmp.path(), std::process::id()),
            1,
            "a dead-PID .mcp.json entry must be reaped"
        );
        assert!(!path.exists(), "sole spyc entry → .mcp.json removed");
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
            let ok = crate::git::test_support::git_command(dir)
                .args(args)
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

    /// The entry spyc writes now pins nothing, and the last spyc out removes it.
    #[test]
    fn cleanup_removes_an_entry_that_pins_no_socket() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".mcp.json");
        std::fs::write(
            &path,
            "{\"mcpServers\":{\"spyc\":{\"command\":\"spyc\",\"args\":[\"--mcp\"]}}}",
        )
        .unwrap();
        assert!(matches!(
            cleanup_mcp_json(tmp.path()),
            ConfigCleanup::Cleaned
        ));
        assert!(!path.exists());
    }

    /// A killed spyc's socket-free entry is reaped at the next launch there,
    /// unless another live spyc still claims the dir.
    #[test]
    fn orphan_sweep_reaps_an_unclaimed_entry_and_spares_a_claimed_one() {
        use crate::state::dir_owners::{Shared, claim};
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(&tmp.path().join("state"), || {
            let dir = tmp.path().join("proj");
            std::fs::create_dir(&dir).unwrap();
            let path = dir.join(".mcp.json");
            let entry = "{\"mcpServers\":{\"spyc\":{\"command\":\"spyc\",\"args\":[\"--mcp\"]}}}";

            std::fs::write(&path, entry).unwrap();
            claim(Shared::McpEntry, &dir, 1);
            assert_eq!(sweep_orphan_spyc_configs(&dir, std::process::id()), 0);
            assert!(path.exists(), "a live spyc (pid 1) still relies on it");

            let _ = crate::state::dir_owners::release(Shared::McpEntry, &dir, 1);
            assert_eq!(sweep_orphan_spyc_configs(&dir, std::process::id()), 1);
            assert!(!path.exists());
        });
    }
}
