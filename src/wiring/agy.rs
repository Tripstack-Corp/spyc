//! agy's user scope, `~/.gemini/config/`, which only `$HOME` relocates: its
//! hooks in `hooks.json` as the `spyc-status` set, which agy collapses by name
//! across scopes, and its MCP servers in `mcp_config.json`.

use super::{Env, FilePart, Part, Status, file, foreign_spyc_server, json_object};

/// Run `f` over agy's two parts.
fn with_parts<R>(env: &Env, f: impl FnOnce(&FilePart, &FilePart) -> R) -> R {
    let dir = env.home.join(".gemini/config");
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
    f(&hooks, &mcp)
}

fn report(hooks: &FilePart, hooks_part: Part, mcp: &FilePart, mcp_part: Part) -> Status {
    Status {
        hooks: hooks_part,
        hooks_file: hooks.path.clone(),
        mcp: mcp_part,
        mcp_file: mcp.path.clone(),
        notes: Vec::new(),
    }
}

pub(super) fn status(env: &Env) -> Status {
    with_parts(env, |hooks, mcp| {
        report(hooks, hooks.status(), mcp, mcp.status())
    })
}

pub(super) fn install(env: &Env) -> Status {
    with_parts(env, |hooks, mcp| {
        report(hooks, hooks.install(), mcp, mcp.install())
    })
}

pub(super) fn remove(env: &Env) -> Status {
    with_parts(env, |hooks, mcp| {
        report(hooks, hooks.remove(), mcp, mcp.remove())
    })
}
