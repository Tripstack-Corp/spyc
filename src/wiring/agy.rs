//! agy's user scope, `~/.gemini/`, which only `$HOME` relocates: its hooks in
//! `~/.gemini/config/hooks.json` as the `spyc-status` set (which agy collapses
//! by name across scopes), its MCP servers in `~/.gemini/config/mcp_config.json`,
//! and its `statusLine` wrapper in `~/.gemini/antigravity-cli/settings.json`.

use super::{Env, FilePart, Part, Status, file, foreign_spyc_server, json_object, statusline};

fn dir(env: &Env) -> std::path::PathBuf {
    env.home.join(".gemini/config")
}

fn settings_path(env: &Env) -> std::path::PathBuf {
    env.home.join(".gemini/antigravity-cli/settings.json")
}

/// The hooks file, the MCP file, and the CLI settings file holding `statusLine`.
pub(super) fn files(env: &Env) -> [std::path::PathBuf; 3] {
    [
        dir(env).join("hooks.json"),
        dir(env).join("mcp_config.json"),
        settings_path(env),
    ]
}

/// Run `f` over agy's three parts: hooks, MCP, and `statusLine`.
fn with_parts<R>(env: &Env, f: impl FnOnce(&FilePart, &FilePart, &FilePart) -> R) -> R {
    let dir = dir(env);
    let hooks_with = |existing: Option<&str>| {
        crate::mcp::merged_agy_status_hooks_json(existing, &env.spyc)
            .ok_or_else(|| "refusing to overwrite — not a JSON object".to_string())
    };
    let hooks = FilePart {
        path: dir.join("hooks.json"),
        with: &hooks_with,
        without: &|text| json_object(crate::mcp::agy_hooks_without_spyc(text)),
        blocked: None,
    };
    let mcp_path = dir.join("mcp_config.json");
    let blocked = file::read(&mcp_path)
        .ok()
        .flatten()
        .and_then(|text| foreign_spyc_server(&text))
        .map(|command| format!("an MCP server named spyc already runs {command}"));
    let mcp_with = |existing: Option<&str>| crate::mcp::mcp_json_with_spyc(existing, &env.spyc);
    let mcp = FilePart {
        path: mcp_path,
        with: &mcp_with,
        without: &|text| json_object(crate::mcp::mcp_json_without_spyc(text)),
        blocked,
    };
    let sl_with = |existing: Option<&str>| {
        statusline::settings_with(existing.unwrap_or("{}"), &env.spyc)
            .ok_or_else(|| "refusing to overwrite — not a JSON object".to_string())
    };
    let sl = FilePart {
        path: settings_path(env),
        with: &sl_with,
        without: &|text| json_object(statusline::settings_without(text)),
        blocked: None,
    };
    f(&hooks, &mcp, &sl)
}

fn report(
    hooks: &FilePart,
    hooks_part: Part,
    mcp: &FilePart,
    mcp_part: Part,
    sl: &FilePart,
    sl_part: Part,
) -> Status {
    let mut notes = Vec::new();
    match sl_part {
        Part::Refused(why) => notes.push(format!(
            "statusLine in {} left alone: {why}",
            crate::paths::display_tilde(&sl.path)
        )),
        Part::Blocked(why) => notes.push(format!(
            "statusLine in {} blocked: {why}",
            crate::paths::display_tilde(&sl.path)
        )),
        _ => {}
    }
    Status {
        hooks: hooks_part,
        hooks_file: hooks.path.clone(),
        mcp: mcp_part,
        mcp_file: mcp.path.clone(),
        notes,
    }
}

pub(super) fn status(env: &Env) -> Status {
    with_parts(env, |hooks, mcp, sl| {
        report(hooks, hooks.status(), mcp, mcp.status(), sl, sl.status())
    })
}

pub(super) fn install(env: &Env) -> Status {
    with_parts(env, |hooks, mcp, sl| {
        report(hooks, hooks.install(), mcp, mcp.install(), sl, sl.install())
    })
}

pub(super) fn remove(env: &Env) -> Status {
    with_parts(env, |hooks, mcp, sl| {
        report(hooks, hooks.remove(), mcp, mcp.remove(), sl, sl.remove())
    })
}
