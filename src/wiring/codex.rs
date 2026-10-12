//! codex's user scope, `$CODEX_HOME` (else `~/.codex`). The MCP entry goes into
//! `config.toml`, edited in place so the user's comments and layout survive,
//! and only the keys spyc needs are touched. The hooks go into `hooks.json`, a
//! file codex itself never rewrites, while `config.toml` holds codex's own trust
//! records. codex asks the user to review new hooks once, in `/hooks`.

use toml_edit::{Array, DocumentMut, Item, Table, TableLike};

use super::{Env, FilePart, Part, Status, file, runs_spyc};
use crate::mcp::{PANE_ENV, Removal};

fn parse(existing: Option<&str>) -> Result<DocumentMut, String> {
    match existing {
        Some(text) if !text.trim().is_empty() => text.parse::<DocumentMut>().map_err(|e| {
            let why = e.to_string();
            let why = why.lines().next().unwrap_or("").to_string();
            format!("refusing to edit — not valid TOML ({why})")
        }),
        _ => Ok(DocumentMut::new()),
    }
}

fn spyc_table(doc: &DocumentMut) -> Option<&dyn TableLike> {
    doc.get("mcp_servers")?
        .as_table_like()?
        .get("spyc")?
        .as_table_like()
}

/// The command of an MCP server named `spyc` in `text` that doesn't run spyc.
fn foreign_spyc_server(text: &str) -> Option<String> {
    let doc = text.parse::<DocumentMut>().ok()?;
    let command = spyc_table(&doc)?.get("command")?.as_str()?;
    (!runs_spyc(command)).then(|| command.to_string())
}

/// `existing` with spyc's `[mcp_servers.spyc]` running `command`. A key spyc
/// doesn't need, and one already right, is left exactly as written.
fn mcp_with_spyc(existing: Option<&str>, command: &str) -> Result<String, String> {
    let mut doc = parse(existing)?;
    let servers = doc
        .entry("mcp_servers")
        .or_insert_with(|| {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        })
        .as_table_like_mut()
        .ok_or_else(|| "refusing to edit — `mcp_servers` isn't a table".to_string())?;
    let spyc = servers
        .entry("spyc")
        .or_insert(Item::Table(Table::new()))
        .as_table_like_mut()
        .ok_or_else(|| "refusing to edit — `mcp_servers.spyc` isn't a table".to_string())?;
    if spyc.get("command").and_then(Item::as_str) != Some(command) {
        spyc.insert("command", toml_edit::value(command));
    }
    let args_right = spyc
        .get("args")
        .and_then(Item::as_array)
        .is_some_and(|args| {
            args.len() == 1 && args.get(0).and_then(|v| v.as_str()) == Some("--mcp")
        });
    if !args_right {
        spyc.insert("args", toml_edit::value(Array::from_iter(["--mcp"])));
    }
    // SPYC-TRAP(mcp-entry-names-no-socket): codex clears an MCP server's
    // environment, so the pane's own socket and id are passed through by name,
    // and a value an older spyc pinned in `env` would override them for every
    // session.
    if let Some(env) = spyc.get_mut("env").and_then(Item::as_table_like_mut) {
        for name in PANE_ENV {
            env.remove(name);
        }
        if env.is_empty() {
            spyc.remove("env");
        }
    }
    match spyc.get_mut("env_vars").and_then(Item::as_array_mut) {
        Some(names) => {
            for name in PANE_ENV {
                if !names.iter().any(|n| n.as_str() == Some(name)) {
                    names.push(name);
                }
            }
        }
        None => {
            spyc.insert("env_vars", toml_edit::value(Array::from_iter(PANE_ENV)));
        }
    }
    Ok(doc.to_string())
}

fn mcp_without_spyc(text: &str) -> Result<Removal, String> {
    let mut doc = parse(Some(text))?;
    let ours = spyc_table(&doc)
        .and_then(|spyc| spyc.get("command"))
        .and_then(Item::as_str)
        .is_some_and(runs_spyc);
    if !ours {
        return Ok(Removal::NothingOurs);
    }
    let servers = doc
        .get_mut("mcp_servers")
        .and_then(Item::as_table_like_mut)
        .expect("spyc_table found it");
    servers.remove("spyc");
    if servers.is_empty() {
        doc.remove("mcp_servers");
    }
    let empty = doc.is_empty();
    Ok(Removal::Rest {
        text: doc.to_string(),
        empty,
    })
}

/// Whether `config.toml` declares hooks of its own. codex then warns about two
/// representations in one layer; its `[hooks.state]` trust records don't count.
fn declares_inline_hooks(text: &str) -> bool {
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return false;
    };
    doc.get("hooks")
        .and_then(Item::as_table_like)
        .is_some_and(|hooks| {
            hooks
                .iter()
                .any(|(event, groups)| event != "state" && !groups.is_none())
        })
}

/// The hooks file and the MCP file.
pub(super) fn files(env: &Env) -> [std::path::PathBuf; 2] {
    [
        env.codex_home.join("hooks.json"),
        env.codex_home.join("config.toml"),
    ]
}

/// Run `f` over codex's two parts.
fn with_parts<R>(env: &Env, f: impl FnOnce(&FilePart, &FilePart) -> R) -> R {
    let hooks_with =
        |existing: Option<&str>| crate::mcp::codex_hooks_json_with_spyc(existing, &env.spyc);
    let hooks_without = |text: &str| crate::mcp::codex_hooks_json_without_spyc(text, &env.spyc);
    let hooks = FilePart {
        path: env.codex_home.join("hooks.json"),
        with: &hooks_with,
        without: &hooks_without,
        blocked: None,
    };
    let config = env.codex_home.join("config.toml");
    let blocked = file::read(&config)
        .ok()
        .flatten()
        .and_then(|text| foreign_spyc_server(&text))
        .map(|command| format!("an MCP server named spyc already runs {command}"));
    let mcp_with = |existing: Option<&str>| mcp_with_spyc(existing, &env.spyc);
    let mcp = FilePart {
        path: config,
        with: &mcp_with,
        without: &mcp_without_spyc,
        blocked,
    };
    f(&hooks, &mcp)
}

fn notes(env: &Env, hooks: &Part) -> Vec<String> {
    let mut notes = Vec::new();
    let config = env.codex_home.join("config.toml");
    if file::read(&config)
        .ok()
        .flatten()
        .is_some_and(|text| declares_inline_hooks(&text))
    {
        notes.push(format!(
            "{} declares hooks too; codex suggests keeping one representation",
            crate::paths::display_tilde(&config)
        ));
    }
    if *hooks == Part::Current {
        notes
            .push("codex asks you to review spyc's hooks once: approve them in /hooks".to_string());
    }
    notes
}

fn report(env: &Env, hooks: &FilePart, hooks_part: Part, mcp: &FilePart, mcp_part: Part) -> Status {
    Status {
        notes: notes(env, &hooks_part),
        hooks: hooks_part,
        hooks_file: hooks.path.clone(),
        mcp: mcp_part,
        mcp_file: mcp.path.clone(),
    }
}

pub(super) fn status(env: &Env) -> Status {
    with_parts(env, |hooks, mcp| {
        let mcp_part = match mcp.status() {
            Part::Absent if !env.codex_mcp => Part::Off,
            part => part,
        };
        report(env, hooks, hooks.status(), mcp, mcp_part)
    })
}

pub(super) fn install(env: &Env) -> Status {
    with_parts(env, |hooks, mcp| {
        let mcp_part = if env.codex_mcp {
            mcp.install()
        } else {
            match mcp.remove() {
                Part::Absent => Part::Off,
                part => part,
            }
        };
        report(env, hooks, hooks.install(), mcp, mcp_part)
    })
}

pub(super) fn remove(env: &Env) -> Status {
    with_parts(env, |hooks, mcp| {
        report(env, hooks, hooks.remove(), mcp, mcp.remove())
    })
}
