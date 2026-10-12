//! spyc's status hooks in codex's user-scope `hooks.json`. Codex keys its trust
//! in a hook on the file, the event and the hook's indices there, so a handler
//! spyc owns is rewritten where it stands, a missing one is appended, and no
//! handler, the user's or spyc's, ever changes position.

use serde_json::{Value, json};

use super::{CODEX_STATUS_HOOKS, identity, reporter_command};
use crate::mcp::Removal;

fn group(event: &str, command: &str) -> Value {
    let handler = json!({ "type": "command", "command": command });
    if matches!(event, "PreToolUse" | "PostToolUse") {
        json!({ "matcher": "^request_user_input$", "hooks": [handler] })
    } else {
        json!({ "hooks": [handler] })
    }
}

fn owned_handler<'a>(groups: &'a mut [Value], exe: &str) -> Option<&'a mut Value> {
    groups
        .iter_mut()
        .filter_map(|g| g.get_mut("hooks").and_then(Value::as_array_mut))
        .flatten()
        .find(|h| identity::json_owned(h, Some(exe)))
}

/// `existing`, a codex `hooks.json` (`None` when absent), with spyc's status
/// hooks running `exe`. `Err` says why the file is refused.
pub fn codex_hooks_json_with_spyc(existing: Option<&str>, exe: &str) -> Result<String, String> {
    let mut root = super::super::merge_root_json(existing)
        .ok_or_else(|| "refusing to edit — not a JSON object".to_string())?;
    let obj = root
        .as_object_mut()
        .expect("merge_root_json returns an object");
    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| "refusing to edit — `hooks` isn't an object".to_string())?;
    for (event, state) in CODEX_STATUS_HOOKS {
        let command = reporter_command(exe, state);
        let groups = hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| format!("refusing to edit — `hooks.{event}` isn't a list"))?;
        match owned_handler(groups, exe) {
            Some(handler) if handler["command"] == command.as_str() => {}
            Some(handler) => handler["command"] = json!(command),
            None => groups.push(group(event, &command)),
        }
    }
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    Ok(text + "\n")
}

/// `text`, a codex `hooks.json`, without spyc's status hooks. Refused when that
/// would move one of the user's hooks, which codex would then ask them to trust
/// again.
pub fn codex_hooks_json_without_spyc(text: &str, exe: &str) -> Result<Removal, String> {
    let mut root: Value =
        serde_json::from_str(text).map_err(|_| "refusing to edit — not valid JSON".to_string())?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "refusing to edit — not a JSON object".to_string())?;
    let Some(hooks) = obj.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(Removal::NothingOurs);
    };
    let mut changed = false;
    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        changed |= identity::prune_json_groups(groups, Some(exe)).map_err(|()| {
            "removing spyc's hooks would move one of yours, which codex would ask you to \
             trust again; take spyc's out by hand"
                .to_string()
        })?;
    }
    if !changed {
        return Ok(Removal::NothingOurs);
    }
    hooks.retain(|_, groups| !groups.as_array().is_some_and(Vec::is_empty));
    if hooks.is_empty() {
        obj.shift_remove("hooks");
    }
    let empty = obj.is_empty();
    let text = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    Ok(Removal::Rest {
        text: text + "\n",
        empty,
    })
}
