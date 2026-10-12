//! claude's user scope. Its hooks live in `settings.json` under its config dir
//! (`$CLAUDE_CONFIG_DIR`, else `~/.claude`), which claude reloads live. Its MCP
//! servers live in `.claude.json`, claude's own constantly rewritten state file,
//! so spyc reads that file but registers the entry through `claude mcp`. An
//! organization's `managed-mcp.json` takes exclusive control of claude's MCP
//! servers, and then provides spyc's entry or forbids it.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{Env, FilePart, Part, Status, file, json_object, runs_spyc};

/// How long a `claude mcp` command may take before spyc gives up on it.
const CLI_DEADLINE: Duration = Duration::from_secs(20);

fn config_dir(env: &Env) -> PathBuf {
    env.claude_config_dir
        .clone()
        .unwrap_or_else(|| env.home.join(".claude"))
}

fn claude_json(env: &Env) -> PathBuf {
    env.claude_config_dir.as_ref().map_or_else(
        || env.home.join(".claude.json"),
        |dir| dir.join(".claude.json"),
    )
}

/// Why an organization's settings stop spyc's hooks: claude then runs only the
/// hooks its managed settings declare.
fn hooks_blocked(env: &Env) -> Option<String> {
    let managed = env
        .managed_settings
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())?;
    (managed["allowManagedHooksOnly"] == true)
        .then(|| "your organization's managed settings allow only its own hooks".to_string())
}

/// Run `f` over the hooks part.
fn with_hooks<R>(env: &Env, f: impl FnOnce(&FilePart) -> R) -> R {
    let with = |existing: Option<&str>| {
        crate::mcp::merged_status_hooks_json(existing, &env.spyc)
            .ok_or_else(|| "refusing to overwrite — not a JSON object".to_string())
    };
    f(&FilePart {
        path: config_dir(env).join("settings.json"),
        with: &with,
        without: &|text| json_object(crate::mcp::claude_hooks_without_spyc(text)),
        blocked: hooks_blocked(env),
    })
}

/// The MCP entry `claude mcp add-json` registers.
fn entry_json(env: &Env) -> String {
    serde_json::json!({ "type": "stdio", "command": env.spyc, "args": ["--mcp"] }).to_string()
}

fn mcp_status(env: &Env) -> Part {
    if env.managed_mcp.iter().any(|path| path.exists()) {
        return if crate::mcp::enterprise_defines_spyc_in(&env.managed_mcp) {
            Part::Managed
        } else {
            Part::Blocked(
                "your organization's managed-mcp.json controls claude's MCP servers and doesn't \
                 define spyc"
                    .to_string(),
            )
        };
    }
    if crate::mcp::enterprise_allows_spyc_in(&env.managed_settings) == Some(false) {
        return Part::Blocked(
            "your organization's managed settings don't allow an MCP server named spyc".to_string(),
        );
    }
    let path = claude_json(env);
    let text = match file::read(&path) {
        Ok(Some(text)) => text,
        Ok(None) => return Part::Absent,
        Err(why) => return Part::Refused(why),
    };
    let Ok(root) = serde_json::from_str::<Value>(&text) else {
        return Part::Refused(format!("{} isn't valid JSON", path.display()));
    };
    let Some(entry) = root.pointer("/mcpServers/spyc") else {
        return Part::Absent;
    };
    let command = entry["command"].as_str().unwrap_or_default();
    if !runs_spyc(command) {
        return Part::Blocked(format!("an MCP server named spyc already runs {command}"));
    }
    if command == env.spyc && entry["args"] == serde_json::json!(["--mcp"]) {
        Part::Current
    } else {
        Part::Outdated
    }
}

/// Run `claude <args>`, failing with the first line it printed to stderr.
fn run_cli(env: &Env, args: &[&str]) -> Result<(), String> {
    let Some(cli) = &env.claude_cli else {
        return Err(format!(
            "the claude CLI isn't on PATH; register spyc with: claude mcp add-json -s user spyc '{}'",
            entry_json(env)
        ));
    };
    let mut command = Command::new(cli);
    command
        .args(args)
        .env("HOME", &env.home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(dir) = &env.claude_config_dir {
        command.env("CLAUDE_CONFIG_DIR", dir);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("running {}: {e}", cli.display()))?;
    let deadline = Instant::now() + CLI_DEADLINE;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("`claude {}` didn't finish", args.join(" ")));
            }
            Err(e) => return Err(format!("waiting for claude: {e}")),
        }
    };
    if status.success() {
        return Ok(());
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = std::io::Read::read_to_string(&mut pipe, &mut stderr);
    }
    let said = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    Err(format!(
        "`claude {}` failed ({status}): {said}",
        args.join(" ")
    ))
}

fn mcp_install(env: &Env) -> Part {
    let entry = entry_json(env);
    let add = ["mcp", "add-json", "-s", "user", "spyc", entry.as_str()];
    let ran = match mcp_status(env) {
        Part::Absent => run_cli(env, &add),
        Part::Outdated => {
            run_cli(env, &["mcp", "remove", "-s", "user", "spyc"]).and_then(|()| run_cli(env, &add))
        }
        settled => return settled,
    };
    match ran {
        Ok(()) => mcp_status(env),
        Err(why) => Part::Refused(why),
    }
}

fn mcp_remove(env: &Env) -> Part {
    match mcp_status(env) {
        Part::Current | Part::Outdated => {
            match run_cli(env, &["mcp", "remove", "-s", "user", "spyc"]) {
                Ok(()) => mcp_status(env),
                Err(why) => Part::Refused(why),
            }
        }
        settled => settled,
    }
}

pub(super) fn status(env: &Env) -> Status {
    with_hooks(env, |hooks| Status {
        hooks: hooks.status(),
        hooks_file: hooks.path.clone(),
        mcp: mcp_status(env),
        mcp_file: claude_json(env),
        notes: Vec::new(),
    })
}

pub(super) fn install(env: &Env) -> Status {
    with_hooks(env, |hooks| Status {
        hooks: hooks.install(),
        hooks_file: hooks.path.clone(),
        mcp: mcp_install(env),
        mcp_file: claude_json(env),
        notes: Vec::new(),
    })
}

pub(super) fn remove(env: &Env) -> Status {
    with_hooks(env, |hooks| Status {
        hooks: hooks.remove(),
        hooks_file: hooks.path.clone(),
        mcp: mcp_remove(env),
        mcp_file: claude_json(env),
        notes: Vec::new(),
    })
}
