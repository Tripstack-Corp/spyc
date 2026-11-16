//! What each tab's jar knows about the tab (R18): that it holds a pane, the
//! tab's id and label, its agent, and its project. A spyc that finds a jar no
//! session describes, after a crash, says what it is from these; `spyc
//! --jars` lists jars by them. Each tab's jar is told only what changed, on
//! the loop's pass before it waits, so a rename reaches it however made.

use std::collections::HashMap;

use super::App;
use crate::state::sessions::AgentKind;

impl App {
    /// Tell each tab's jar what it holds, where that changed.
    pub(crate) fn settle_jar_info(&mut self) {
        let projects: HashMap<_, _> = self
            .state
            .open_projects()
            .map(|p| (p.id, (p.label(), p.anchor().display().to_string())))
            .collect();
        for (project, tabs) in self.runtime.every_tab_set_mut() {
            let Some((name, home)) = projects.get(&project) else {
                continue;
            };
            for entry in tabs.tabs() {
                let label = crate::pane::tabs::strip_exit_suffix(&entry.info.label);
                entry.pane.host.tell_jar(&[
                    ("is", "pane"),
                    ("pane_id", &entry.info.id),
                    ("label", &label),
                    ("agent", agent_name(entry.info.agent().kind())),
                    ("project", name),
                    ("project_home", home),
                ]);
            }
        }
    }
}

/// An agent's name as a saved session spells it.
const fn agent_name(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Claude => "claude",
        AgentKind::Codex => "codex",
        AgentKind::Agy => "agy",
        AgentKind::Zot => "zot",
        AgentKind::Other => "other",
    }
}
