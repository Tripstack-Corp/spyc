//! Which project an MCP call acts on (#99, `docs/drafts/PROJECTS_PLAN.md` §2).
//!
//! A connection is bound to a project at its `initialize`: the project of the
//! tab its proxy named, else the one whose root holds its cwd, else the active
//! one. Each request carries that project, and the drain serves it inside
//! [`App::in_project`], so every handle (`cur()`, `runtime.project()`,
//! `view.project()`) resolves to the caller's project and the handlers never
//! ask which project they are in. An agent in a project the user isn't looking
//! at reads and moves its own, never the user's view.

use std::path::Path;
use std::sync::mpsc::Sender;

use serde_json::json;

use super::App;
use super::state::ProjectId;
use crate::mcp_cmd::{McpCommand, McpRequest, McpResponse};

impl App {
    /// Run `f` with `id` as the project every handle resolves to, then put the
    /// user's back. For a project other than the user's, `state.in_background`
    /// is set while `f` runs, and the flash the user was reading is kept: the
    /// MCP reply carries what happened. Callers resolve an open project first.
    pub(super) fn in_project<R>(&mut self, id: ProjectId, f: impl FnOnce(&mut Self) -> R) -> R {
        let users = self.state.active;
        if id == users {
            return f(self);
        }
        assert!(
            self.state.has_project(id),
            "in_project is handed an open project: its callers resolve one first"
        );
        let flash = self.state.flash.take();
        self.set_active(id);
        self.state.in_background = true;
        let out = f(self);
        self.state.in_background = false;
        self.set_active(users);
        self.state.flash = flash;
        out
    }

    /// The three records of which project is active, moved together.
    const fn set_active(&mut self, id: ProjectId) {
        self.state.active = id;
        self.runtime.active = id;
        self.view.active = id;
    }

    /// Serve one MCP request in the project it acts on ([`Self::mcp_target`]).
    /// A request from a project that has since closed gets an error and is
    /// never served in the user's.
    pub(super) fn route_mcp_request(&mut self, req: McpRequest) {
        let McpRequest {
            command,
            reply,
            project,
        } = req;
        match self.mcp_target(&command, project) {
            Some(target) => self.in_project(target, |app| app.serve_mcp(command, reply)),
            None => {
                let _ = reply.send(McpResponse::Error {
                    message: "the project this agent belongs to has closed".into(),
                });
            }
        }
    }

    /// The project `command` acts on, from a connection bound to `caller`:
    /// - a driving tool acts on the caller's project, else the active one, and
    ///   on none once the caller's has closed;
    /// - a report about a tab acts on the project holding that tab, else the
    ///   caller's, else the active one, whose focused tab is the last resort
    ///   (a tab number, too, counts in that project);
    /// - anything else is global, and is served from the active project.
    fn mcp_target(&self, command: &McpCommand, caller: Option<ProjectId>) -> Option<ProjectId> {
        let active = self.state.active;
        let open = caller.filter(|id| self.state.has_project(*id));
        let holding = |pane_id: &str| self.runtime.project_of_pane(pane_id);
        match command {
            McpCommand::NavigateTo { .. }
            | McpCommand::SetFilter { .. }
            | McpCommand::PickFiles { .. }
            | McpCommand::ClearPicks
            | McpCommand::OpenWorktree { .. }
            | McpCommand::CreateWorktree { .. }
            | McpCommand::RemoveWorktree { .. }
            | McpCommand::CleanWorktree { .. } => caller.map_or(Some(active), |_| open),
            McpCommand::ReportStatus { pane_id, .. }
            | McpCommand::RegisterScope { pane_id, .. }
            | McpCommand::WaitForScopeClear { pane_id, .. } => Some(
                pane_id
                    .as_deref()
                    .and_then(holding)
                    .or(open)
                    .unwrap_or(active),
            ),
            McpCommand::Statusline { pane_id, .. } => Some(holding(pane_id).unwrap_or(active)),
            _ => Some(active),
        }
    }

    /// One request, in whichever project [`Self::in_project`] made active.
    /// `wait_for_scope_clear` blocks: it doesn't reply inline — the loop parks
    /// its reply sender (or answers at once if already clear) and
    /// `settle_scope_waiters` fires it later. Heavy worktree ops
    /// (create/remove/clean) run off the loop: validate synchronously (cheap,
    /// reads App state incl. the occupied guard), then hand the gix/copy IO to
    /// a worker that replies once the main loop has re-applied refresh+context.
    /// Everything else is served synchronously, replying inline.
    fn serve_mcp(&mut self, command: McpCommand, reply: Sender<McpResponse>) {
        if let McpCommand::WaitForScopeClear {
            pane_id,
            pane,
            paths,
            timeout_ms,
        } = command
        {
            self.handle_scope_wait(pane_id, pane, paths, timeout_ms, reply);
        } else if let Some(planned) = self.plan_worktree_job(&command) {
            match planned {
                Ok(job) => {
                    self.spawn_worktree_job(
                        job,
                        super::worktree_ops::WorktreeCompletion::Mcp(reply),
                    );
                }
                Err(resp) => {
                    let _ = reply.send(resp);
                }
            }
        } else {
            let resp = self.execute_mcp_command(command);
            let _ = reply.send(resp);
        }
    }

    /// `Bind` and `CallerContext`: what a connection's thread asks about the
    /// agent on the other end.
    pub(super) fn answer_binding(&mut self, command: McpCommand) -> McpResponse {
        match command {
            McpCommand::Bind { pane_id, cwd } => {
                self.bind_connection(pane_id.as_deref(), cwd.as_deref())
            }
            McpCommand::CallerContext { pane_id, project } => {
                self.caller_context(pane_id.as_deref(), project)
            }
            _ => unreachable!("answer_binding is handed only Bind and CallerContext"),
        }
    }

    fn bind_connection(&mut self, pane_id: Option<&str>, cwd: Option<&Path>) -> McpResponse {
        let pane_project = pane_id.and_then(|id| self.runtime.project_of_pane(id));
        let project = pane_project
            .or_else(|| cwd.and_then(|cwd| self.project_holding(cwd)))
            .unwrap_or(self.state.active);
        // The connection's reads answer from this file, so write it now: a
        // project the user isn't in has one, and a current one.
        let context_file = self.in_project(project, |app| {
            app.write_context();
            app.view.project().context_path.clone()
        });
        McpResponse::Ok {
            message: json!({
                "pane": pane_project.is_some(),
                "project": project.to_string(),
                "context_file": context_file,
            })
            .to_string(),
        }
    }

    /// The open project whose root holds `cwd`, the nearest when roots nest. A
    /// root is a project's `start_dir`: the path its context file names and the
    /// sidecar lists, so binding agrees with discovery.
    fn project_holding(&self, cwd: &Path) -> Option<ProjectId> {
        let cwd = std::fs::canonicalize(cwd).ok()?;
        self.state
            .open_projects()
            .filter_map(|p| Some((std::fs::canonicalize(&p.start_dir).ok()?, p.id)))
            .filter(|(root, _)| cwd.starts_with(root))
            .max_by_key(|(root, _)| root.components().count())
            .map(|(_, id)| id)
    }

    fn caller_context(&self, pane_id: Option<&str>, project: ProjectId) -> McpResponse {
        let Some(p) = self.state.project(project) else {
            return McpResponse::Error {
                message: "the project this connection belongs to has closed".into(),
            };
        };
        let mut about = serde_json::Map::new();
        about.insert(
            "project".into(),
            json!({
                "id": project.to_string(),
                "label": p.label(),
                "home": p.home,
                "active": project == self.state.active,
            }),
        );
        if let Some(pane) = pane_id.and_then(|id| self.pane_context(id)) {
            about.insert("pane".into(), pane);
        }
        McpResponse::Ok {
            message: serde_json::Value::Object(about).to_string(),
        }
    }

    /// Where the tab with `pane_id` runs, in whichever project holds it: the
    /// tab's live cwd and that cwd's worktree root and branch, not the column
    /// the user is browsing. `None` when no live tab has the id.
    fn pane_context(&self, pane_id: &str) -> Option<serde_json::Value> {
        let (tab, entry) = self.runtime.tab_of_pane(pane_id)?;
        let cwd = entry.live_cwd();
        let worktree_root = super::state::find_repo_root(&cwd);
        let git_branch = worktree_root
            .as_deref()
            .and_then(crate::git::discovery::head_branch);
        Some(json!({
            "id": pane_id,
            "tab": tab,
            "label": entry.info.label,
            "cwd": cwd,
            "worktree_root": worktree_root,
            "git_branch": git_branch,
        }))
    }
}
