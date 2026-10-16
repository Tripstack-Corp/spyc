//! Codex's persisted hook identity includes source/event/group/handler indices.
//! Removing an owned reporter before a user handler changes the latter's trust
//! lookup even when its command hash is identical. Refuse those edits rather
//! than moving trust records or silently disabling the user's hooks.

use super::reporter;

pub(super) fn json_owned(handler: &serde_json::Value, exe: Option<&str>) -> bool {
    if handler
        .get("type")
        .is_some_and(|kind| kind.as_str() != Some("command"))
    {
        return false;
    }
    handler
        .get("command")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|command| reporter::is_owned(command, exe))
}

fn json_user_positions(groups: &[serde_json::Value], exe: Option<&str>) -> Vec<(usize, usize)> {
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
                    (!json_owned(handler, exe)).then_some((group_index, handler_index))
                })
        })
        .collect()
}

/// Apply pruning only when every user handler retains both position indices.
/// Compare the actual proposed edit, including emptied groups, before applying.
pub(super) fn prune_json_groups(
    groups: &mut Vec<serde_json::Value>,
    exe: Option<&str>,
) -> Result<bool, ()> {
    let before = json_user_positions(groups, exe);
    let mut pruned = groups.clone();
    let mut changed = false;
    pruned.retain_mut(|group| {
        if !group
            .get("hooks")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|handlers| handlers.iter().any(|handler| json_owned(handler, exe)))
        {
            return true;
        }
        let Some(handlers) = group
            .get_mut("hooks")
            .and_then(serde_json::Value::as_array_mut)
        else {
            return true;
        };
        handlers.retain(|handler| !json_owned(handler, exe));
        changed = true;
        !handlers.is_empty()
    });
    if json_user_positions(&pruned, exe) != before {
        return Err(());
    }
    *groups = pruned;
    Ok(changed)
}
