//! The agents' MCP `spyc` entry: the JSON codecs user-scope wiring
//! (`crate::wiring`) writes it with, and claude's enterprise checks.
use std::path::Path;

use serde_json::{Value, json};

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
