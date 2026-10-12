//! Agents launched inside shell tabs (#544). A shell tab carries the agent env
//! (`SPYC_MCP_SOCK` + `SPYC_PANE_ID`), so an agent typed into it can reach spyc
//! and name the tab. And because spyc learns which agent runs only after it has
//! started, while agents read their MCP config (and codex and agy their hooks)
//! at startup, a shell tab in a project that has consented to status hooks gets
//! every supported agent's config written up front.

use super::{App, state};

impl App {
    /// Write, for each agent spyc supports, what it reads at startup: its MCP
    /// entry and its status hooks, each recorded for cleanup on exit exactly as
    /// an agent tab's are. Only where that agent's hook root has consented; a
    /// shell tab anywhere else writes nothing. Quiet, because opening a shell
    /// is not launching an agent.
    pub(super) fn provision_shell_tab(&mut self, cwd: &std::path::Path) {
        if !self.view.mcp_running {
            return;
        }
        for profile in crate::agent::REGISTRY {
            let Some(support) = profile.status_hooks() else {
                continue;
            };
            let dir = (support.config_dir)(cwd);
            let root = state::find_repo_root(&dir).unwrap_or_else(|| dir.clone());
            if crate::state::hook_consent::consent_for(&root) != Some(true) {
                continue;
            }
            self.write_mcp_config(profile.kind(), cwd, false);
            self.install_status_hooks(cwd, profile.kind());
        }
    }
}
