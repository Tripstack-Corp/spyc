//! The agents' MCP `spyc` entry: the JSON codecs user-scope wiring
//! (`crate::wiring`) writes it with, claude's enterprise checks, and removing
//! the per-directory entry older spycs wrote (`.mcp.json`, codex `config.toml`,
//! agy `mcp_config.json`).
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::server::pid_from_sock_path;
use super::{mcp_log, socket_path};

/// The variables an agent pane's env carries (`open_pane_tab_into`) that its
/// `spyc --mcp` proxy needs: which spyc launched it, and which tab it is.
pub const PANE_ENV: [&str; 2] = ["SPYC_MCP_SOCK", "SPYC_PANE_ID"];

/// Well-known paths for Claude Code enterprise managed settings.
pub const MANAGED_SETTINGS_PATHS: &[&str] = &[
    // macOS system-wide
    "/Library/Application Support/ClaudeCode/managed-settings.json",
    // Linux / WSL system-wide
    "/etc/claude-code/managed-settings.json",
];

/// Well-known paths for Claude Code enterprise-deployed MCP definitions. One
/// that exists has exclusive control of claude's MCP servers.
pub const MANAGED_MCP_PATHS: &[&str] = &[
    "/Library/Application Support/ClaudeCode/managed-mcp.json",
    "/etc/claude-code/managed-mcp.json",
];

/// Whether the first parseable `managed-settings.json` among `paths` allows an
/// MCP server named spyc: `Some(false)` when its denylist names spyc or its
/// allowlist leaves it out, `None` when no file or no restriction applies.
pub fn enterprise_allows_spyc_in(paths: &[impl AsRef<Path>]) -> Option<bool> {
    for path in paths {
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

/// Whether a `managed-mcp.json` among `paths` defines a server named spyc.
pub fn enterprise_defines_spyc_in(paths: &[impl AsRef<Path>]) -> bool {
    for path in paths {
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

/// What taking spyc's part out of a config file's text leaves behind.
#[derive(Debug, PartialEq, Eq)]
pub enum Removal {
    /// Nothing of spyc's was there.
    NothingOurs,
    /// The rest of the file; `empty` when no other content remains.
    Rest { text: String, empty: bool },
}

/// `existing`, an `mcpServers`-shaped JSON file (`None` when absent), with
/// spyc's stdio entry running `command`; any other server is kept. `Err` says
/// why the file is refused: an existing file is the user's data, and replacing
/// one that won't parse drops every server it declares.
pub fn mcp_json_with_spyc(existing: Option<&str>, command: &str) -> Result<String, String> {
    // SPYC-TRAP(mcp-entry-names-no-socket): no `env`. Claude and agy start the
    // proxy with the agent's own env, where the pane put its spyc's socket.
    let spyc_entry = json!({
        "command": command,
        "args": ["--mcp"],
    });
    let mut parsed = match existing {
        Some(text) if !text.trim().is_empty() => {
            serde_json::from_str::<Value>(text).map_err(|e| {
                format!(
                    "refusing to overwrite — exists but isn't valid JSON ({e}); fix or delete it"
                )
            })?
        }
        _ => json!({}),
    };
    let servers = parsed.as_object_mut().and_then(|t| {
        let entry = t.entry("mcpServers").or_insert_with(|| json!({}));
        entry.as_object_mut()
    });
    let Some(map) = servers else {
        return Err(
            "refusing to overwrite — top level or `mcpServers` is not an object".to_string(),
        );
    };
    map.insert("spyc".to_string(), spyc_entry);
    Ok(
        serde_json::to_string_pretty(&parsed).expect("serializing a serde_json::Value cannot fail")
            + "\n",
    )
}

/// `text` without its `spyc` server, or `None` when `text` isn't a JSON object.
/// `empty` means nothing but an empty `mcpServers` would remain.
pub fn mcp_json_without_spyc(text: &str) -> Option<Removal> {
    let mut parsed = serde_json::from_str::<Value>(text).ok()?;
    let root = parsed.as_object_mut()?;
    let Some(servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return Some(Removal::NothingOurs);
    };
    if servers.shift_remove("spyc").is_none() {
        return Some(Removal::NothingOurs);
    }
    let empty = servers.is_empty() && root.len() == 1;
    let text = serde_json::to_string_pretty(&parsed).ok()? + "\n";
    Some(Removal::Rest { text, empty })
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
    let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
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
    let Some(Removal::Rest { text: rest, empty }) = mcp_json_without_spyc(&text) else {
        return ConfigCleanup::NothingToDo;
    };
    if empty {
        let _ = std::fs::remove_file(path);
        mcp_log(&format!(
            "removed empty .mcp.json after cleaning spyc entry ({})",
            path.display()
        ));
        return ConfigCleanup::Cleaned;
    }
    let _ = crate::fs::write_atomic(path, rest.as_bytes());
    mcp_log(&format!(
        "cleaned spyc entry from .mcp.json (preserved other servers, {})",
        path.display()
    ));
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

/// Where agy discovers a workspace's MCP servers: a customization root in the
/// project (`<dir>/.agents/`), same schema as claude's `.mcp.json`.
fn agy_mcp_config_path(dir: &Path) -> PathBuf {
    dir.join(".agents/mcp_config.json")
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
/// What an older spyc wrote into `<dir>/.codex/config.toml`: tests build the
/// leftovers the per-directory cleanups take out with it.
#[cfg(test)]
pub fn ensure_codex_config_toml(dir: &Path) -> std::io::Result<()> {
    let path = dir.join(".codex").join("config.toml");
    let mut root: toml::Table = match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(std::io::Error::other)?,
        Err(_) => toml::Table::new(),
    };
    let servers = root
        .entry("mcp_servers")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let mut spyc = toml::Table::new();
    spyc.insert("command".into(), "spyc".into());
    spyc.insert("args".into(), toml::Value::Array(vec!["--mcp".into()]));
    spyc.insert(
        "env_vars".into(),
        toml::Value::Array(PANE_ENV.map(toml::Value::from).to_vec()),
    );
    servers
        .as_table_mut()
        .expect("a test file's mcp_servers is a table")
        .insert("spyc".into(), toml::Value::Table(spyc));
    std::fs::create_dir_all(path.parent().expect("has a parent"))?;
    let text = toml::to_string_pretty(&root).map_err(std::io::Error::other)?;
    crate::fs::write_atomic(&path, text.as_bytes())
}

/// What an older spyc wrote into `<dir>/.agents/mcp_config.json`.
#[cfg(test)]
pub fn ensure_agy_mcp_config(dir: &Path) -> std::io::Result<()> {
    let path = agy_mcp_config_path(dir);
    let existing = std::fs::read_to_string(&path).ok();
    let text = mcp_json_with_spyc(existing.as_deref(), "spyc").map_err(std::io::Error::other)?;
    std::fs::create_dir_all(path.parent().expect("has a parent"))?;
    crate::fs::write_atomic(&path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    // --- teardown cleanup ---
    use super::{ConfigCleanup, cleanup_agy_mcp_config, cleanup_codex_config, cleanup_mcp_json};

    /// The socket `cleanup_*` treats as this process's own. `socket_path` lives
    /// under the state root, so call it inside [`with_our_state_root`].
    fn our_sock() -> String {
        crate::mcp::socket_path()
            .expect("the test pinned a state root")
            .to_string_lossy()
            .into_owned()
    }

    fn with_our_state_root(tmp: &tempfile::TempDir, body: impl FnOnce()) {
        crate::state::with_state_root(&tmp.path().join("state"), body);
    }

    #[test]
    fn cleanup_codex_removes_our_entry_and_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        with_our_state_root(&tmp, || {
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
        });
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
    fn cleanup_codex_preserves_other_config_keys() {
        let tmp = tempfile::tempdir().unwrap();
        with_our_state_root(&tmp, || {
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
        });
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
    fn cleanup_agy_removes_our_entry_and_the_emptied_dir() {
        let tmp = tempfile::tempdir().unwrap();
        with_our_state_root(&tmp, || {
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
        });
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
    fn cleanup_mcp_json_removes_our_entry_when_sole() {
        let tmp = tempfile::tempdir().unwrap();
        with_our_state_root(&tmp, || {
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
        });
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
        with_our_state_root(&tmp, || {
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
        });
    }

    #[test]
    fn cleanup_mcp_json_preserves_other_servers() {
        let tmp = tempfile::tempdir().unwrap();
        with_our_state_root(&tmp, || {
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
        });
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
}
