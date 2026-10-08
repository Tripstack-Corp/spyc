//! Codex's persisted hook identity includes source/event/group/handler indices.
//! Removing an owned reporter before a user handler changes the latter's trust
//! lookup even when its command hash is identical. Refuse those edits rather
//! than moving trust records or silently disabling the user's hooks.

use std::path::Path;

use super::{CODEX_STATUS_HOOKS, codex_group_is_ours};

pub(super) fn toml_owned(handler: &toml::Value) -> bool {
    handler
        .get("command")
        .and_then(toml::Value::as_str)
        .is_some_and(|command| command.contains("--report-status"))
}

fn json_owned(handler: &serde_json::Value) -> bool {
    handler
        .get("command")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|command| command.contains("--report-status"))
}

fn toml_user_positions(groups: &[toml::Value]) -> Vec<(usize, usize)> {
    groups
        .iter()
        .enumerate()
        .flat_map(|(group_index, group)| {
            group
                .get("hooks")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(move |(handler_index, handler)| {
                    (!toml_owned(handler)).then_some((group_index, handler_index))
                })
        })
        .collect()
}

fn json_user_positions(groups: &[serde_json::Value]) -> Vec<(usize, usize)> {
    groups
        .iter()
        .enumerate()
        .flat_map(|(group_index, group)| {
            group
                .get("hooks")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(move |(handler_index, handler)| {
                    (!json_owned(handler)).then_some((group_index, handler_index))
                })
        })
        .collect()
}

/// Apply pruning only when every user handler retains both position indices.
/// Compare the actual proposed edit, including emptied groups, before applying.
pub(super) fn prune_toml_groups(groups: &mut Vec<toml::Value>) -> bool {
    let before = toml_user_positions(groups);
    let mut pruned = groups.clone();
    pruned.retain_mut(|group| {
        if !codex_group_is_ours(group) {
            return true;
        }
        let Some(handlers) = group.get_mut("hooks").and_then(toml::Value::as_array_mut) else {
            return true;
        };
        handlers.retain(|handler| !toml_owned(handler));
        !handlers.is_empty()
    });
    if toml_user_positions(&pruned) != before {
        return false;
    }
    *groups = pruned;
    true
}

pub(super) fn prune_json_groups(groups: &mut Vec<serde_json::Value>) -> Result<bool, ()> {
    let before = json_user_positions(groups);
    let mut pruned = groups.clone();
    let mut changed = false;
    pruned.retain_mut(|group| {
        if !group
            .get("hooks")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|handlers| handlers.iter().any(json_owned))
        {
            return true;
        }
        let Some(handlers) = group
            .get_mut("hooks")
            .and_then(serde_json::Value::as_array_mut)
        else {
            return true;
        };
        handlers.retain(|handler| !json_owned(handler));
        changed = true;
        !handlers.is_empty()
    });
    if json_user_positions(&pruned) != before {
        return Err(());
    }
    *groups = pruned;
    Ok(changed)
}

/// Used by cleanup preflight and diagnostics for the inline source as well as
/// legacy JSON. Invalid files keep their existing refusal/cleanup behaviour.
pub(super) fn inline_prune_refused(dir: &Path) -> bool {
    let Some(root) = std::fs::read_to_string(dir.join(".codex/config.toml"))
        .ok()
        .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
    else {
        return false;
    };
    CODEX_STATUS_HOOKS.iter().any(|(event, _)| {
        root.get("hooks")
            .and_then(|hooks| hooks.get(*event))
            .and_then(toml::Value::as_array)
            .is_some_and(|groups| !prune_toml_groups(&mut groups.clone()))
    })
}
