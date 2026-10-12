//! `spyc --install-agents` and `spyc --remove-agents`.

use anyhow::{Result, bail};

use super::{Env, KINDS, Part, Status, install, remove};
use crate::state::sessions::AgentKind;
use crate::state::wiring_consent;

const fn name(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Claude => "claude",
        AgentKind::Codex => "codex",
        AgentKind::Agy => "agy",
        AgentKind::Zot => "zot",
        AgentKind::Other => "other",
    }
}

/// The agents a `--install-agents=LIST` names; `all` is every one spyc wires.
fn kinds(list: &str) -> Result<Vec<AgentKind>> {
    if list == "all" {
        return Ok(KINDS.to_vec());
    }
    list.split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(|n| {
            KINDS
                .iter()
                .copied()
                .find(|k| name(*k) == n)
                .ok_or_else(|| {
                    anyhow::anyhow!("spyc can't wire `{n}` into user scope (it wires: claude, agy)")
                })
        })
        .collect()
}

fn describe(part: &Part, file: &std::path::Path, installing: bool) -> String {
    let file = crate::paths::display_tilde(file);
    match part {
        Part::Current => format!("installed in {file}"),
        Part::Absent if installing => format!("not installed in {file}"),
        Part::Absent => format!("not in {file}"),
        Part::Outdated => format!("an older copy is in {file}"),
        Part::Managed => "provided by your organization".to_string(),
        Part::Blocked(why) => format!("blocked: {why}"),
        Part::Refused(why) => format!("left alone: {why}"),
    }
}

fn report(kind: AgentKind, st: &Status, installing: bool) {
    println!(
        "\u{1f336}\u{fe0f} {}: hooks {}; MCP {}",
        name(kind),
        describe(&st.hooks, &st.hooks_file, installing),
        describe(&st.mcp, &st.mcp_file, installing),
    );
}

/// Install (or remove) the wiring of the agents `list` names, report each, and
/// record the answer so spyc stops (or resumes) its per-directory writes.
pub fn run_cli(installing: bool, list: &str) -> Result<()> {
    let kinds = kinds(list)?;
    let Some(env) = Env::current() else {
        bail!("$HOME isn't set, so there is no user configuration to wire");
    };
    let mut refused = 0;
    for kind in kinds {
        let st = if installing {
            install(kind, &env)
        } else {
            remove(kind, &env)
        }
        .expect("KINDS are wired here");
        report(kind, &st, installing);
        refused += [&st.hooks, &st.mcp]
            .iter()
            .filter(|p| matches!(p, Part::Refused(_)))
            .count();
        if installing && st.hooks.in_place() {
            wiring_consent::set(kind, Some(true));
        } else if !installing && st.hooks == Part::Absent {
            wiring_consent::set(kind, Some(false));
        }
    }
    if refused > 0 {
        bail!("{refused} part(s) were left alone; see above");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::kinds;
    use crate::state::sessions::AgentKind;

    #[test]
    fn a_list_names_agents_and_all_means_every_wired_one() {
        assert_eq!(kinds("all").unwrap(), [AgentKind::Claude, AgentKind::Agy]);
        assert_eq!(kinds("agy").unwrap(), [AgentKind::Agy]);
        assert_eq!(
            kinds("agy, claude").unwrap(),
            [AgentKind::Agy, AgentKind::Claude]
        );
        assert!(kinds("codex").is_err());
        assert!(kinds("bogus").is_err());
    }
}
