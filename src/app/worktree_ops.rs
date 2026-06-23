//! Off-thread MCP worktree mutations (`create` / `remove` / `clean_worktree`).
//!
//! These three MCP commands do heavy filesystem IO — a `gix` worktree checkout
//! (create) or a recursive copy + `remove_dir_all` (clean) — that would stall
//! the event loop if run inline, and they're reachable by an external MCP
//! client. So the production drain (`drain_mcp_pending`) splits them:
//!   1. **Validation stays on the loop** (`plan_worktree_job`): cheap, reads
//!      `App` state — the empty-branch / missing-path checks and the
//!      occupied-column guard (`resolve_worktree_arg`). A failure replies
//!      immediately.
//!   2. **The heavy IO runs on a detached worker** (`spawn_worktree_job` →
//!      `run_worktree_job`): no `App` access, just the `git`/graveyard infra.
//!   3. **The main loop re-couples** (`apply_worktree_outcomes`, pre-recv scan):
//!      it runs `refresh_listing` + `write_context` and only THEN sends the MCP
//!      reply — preserving the single-connection read-after-write the client
//!      relies on (`get_spyc_context` right after a mutation sees fresh state).
//!
//! The interactive `W n` / `W d` keyboard path stays synchronous (a deliberate
//! user action, not an external-client-reachable entry point). The synchronous
//! `App::execute_mcp_command` path (direct callers / tests) and this async path
//! share `plan_worktree_job` + `run_worktree_job` + `after_worktree_mutation`,
//! so the IO + response logic can't diverge — only the sync-vs-async wrapping
//! differs. Shape mirrors `preview_ops` / `mermaid_ops`: a landing slot
//! (`runtime.worktree_results`) drained on a `Message::WorktreeJobDone` wake.

use std::sync::mpsc::Sender;

use crate::mcp_cmd::{McpCommand, McpResponse};

use super::{App, Message};

/// A fully-resolved worktree mutation, ready for the worker (absolute paths,
/// validated). Built by [`App::plan_worktree_job`] on the main loop.
pub enum WorktreeJob {
    /// `create_worktree`: add a worktree for `branch`, discovering the repo from
    /// `dir` (the focused column's listing dir).
    Create {
        dir: std::path::PathBuf,
        branch: String,
        /// Start point for a NEW branch — the repo's default branch (POLA: not
        /// the focused column's HEAD). `None` falls back to HEAD in `add`.
        base: Option<String>,
    },
    /// `remove_worktree`: tear down the worktree at `target`.
    Remove { target: std::path::PathBuf },
    /// `clean_worktree`: archive untracked files, then remove `target`.
    Clean { target: std::path::PathBuf },
}

/// The outcome of running a [`WorktreeJob`] — the MCP reply, the (success-only)
/// status flash, and whether the filesystem was mutated (so the apply step
/// knows to refresh). No `App` reference: built off-thread.
pub struct WorktreeJobResult {
    pub response: McpResponse,
    /// Footer/status flash to surface on success (`None` on error).
    pub flash: Option<String>,
    /// Whether the worktree was actually created/removed — gates the
    /// `refresh_listing` + `write_context` re-apply.
    pub mutated: bool,
}

/// A landed worker result plus the one-shot reply channel to answer the MCP
/// client once the main loop has re-applied the listing/context update.
pub struct WorktreeOutcome {
    pub result: WorktreeJobResult,
    pub reply: Sender<McpResponse>,
}

const fn err_result(message: String) -> WorktreeJobResult {
    WorktreeJobResult {
        response: McpResponse::Error { message },
        flash: None,
        mutated: false,
    }
}

/// Run the heavy IO for one worktree job and build its MCP reply. Pure infra
/// (no `App`): callable on the loop (`run_worktree_job_sync`) or off it
/// (`spawn_worktree_job`'s worker).
pub fn run_worktree_job(job: WorktreeJob) -> WorktreeJobResult {
    match job {
        WorktreeJob::Create { dir, branch, base } => {
            match crate::git::worktree::add(&dir, &branch, base.as_deref()) {
                Ok(path) => {
                    let json = serde_json::json!({
                        "branch": branch,
                        "path": path.display().to_string(),
                    });
                    WorktreeJobResult {
                        response: McpResponse::Ok {
                            message: serde_json::to_string_pretty(&json).unwrap_or_default(),
                        },
                        flash: Some(format!(
                            "[mcp] created worktree {} ({branch})",
                            path.display()
                        )),
                        mutated: true,
                    }
                }
                Err(e) => err_result(format!("worktree add: {e}")),
            }
        }
        WorktreeJob::Remove { target } => match crate::git::worktree::remove(&target) {
            Ok(()) => WorktreeJobResult {
                response: McpResponse::Ok {
                    message: format!("removed worktree {}", target.display()),
                },
                flash: Some(format!("[mcp] removed worktree {}", target.display())),
                mutated: true,
            },
            Err(e) => err_result(format!("worktree remove: {e}")),
        },
        WorktreeJob::Clean { target } => {
            match crate::app::worktree_clean::clean_worktree(&target) {
                Ok(report) => {
                    let message = match &report.label {
                        Some(label) => format!(
                            "cleaned worktree {} — archived {} untracked entr{} to the graveyard as '{label}'",
                            target.display(),
                            report.archived,
                            if report.archived == 1 { "y" } else { "ies" },
                        ),
                        None => format!(
                            "cleaned worktree {} (no untracked files to archive)",
                            target.display()
                        ),
                    };
                    WorktreeJobResult {
                        flash: Some(format!("[mcp] {message}")),
                        response: McpResponse::Ok { message },
                        mutated: true,
                    }
                }
                Err(e) => err_result(format!("worktree clean: {e}")),
            }
        }
    }
}

impl App {
    /// Classify + validate an MCP command as a heavy worktree job. `None` for
    /// any non-worktree command (the caller falls back to `execute_mcp_command`
    /// / `OpenWorktree`, which stay synchronous). `Some(Ok(job))` once validated
    /// (branch present / path resolved / occupied-column guard passed),
    /// `Some(Err(resp))` on a validation failure that should reply immediately.
    /// Pure read of `App` state — safe to run on the loop before off-threading.
    pub(crate) fn plan_worktree_job(
        &self,
        cmd: &McpCommand,
    ) -> Option<Result<WorktreeJob, McpResponse>> {
        Some(match cmd {
            McpCommand::CreateWorktree { branch } => {
                let branch = branch.trim();
                if branch.is_empty() {
                    Err(McpResponse::Error {
                        message: "missing required parameter: branch".into(),
                    })
                } else {
                    // Anchor on the FOCUSED column's repo (consistent with `W n`):
                    // `worktree::add` discovers the enclosing repo from any dir
                    // inside it.
                    Ok(WorktreeJob::Create {
                        dir: self.state.cur().listing.dir.clone(),
                        branch: branch.to_string(),
                        // POLA: base a new worktree off PROJECT_HOME's default
                        // branch, not whatever the focused column is on.
                        base: self
                            .state
                            .project_home
                            .as_deref()
                            .and_then(crate::git::branch::default_base),
                    })
                }
            }
            McpCommand::RemoveWorktree { path } => match self.resolve_worktree_arg(path) {
                Ok(target) => Ok(WorktreeJob::Remove { target }),
                Err(message) => Err(McpResponse::Error { message }),
            },
            McpCommand::CleanWorktree { path } => match self.resolve_worktree_arg(path) {
                Ok(target) => Ok(WorktreeJob::Clean { target }),
                Err(message) => Err(McpResponse::Error { message }),
            },
            _ => return None,
        })
    }

    /// Run a validated worktree job synchronously on the loop (the direct /
    /// test path). The production drain uses `spawn_worktree_job` instead.
    pub(crate) fn run_worktree_job_sync(&mut self, job: WorktreeJob) -> McpResponse {
        let WorktreeJobResult {
            response,
            flash,
            mutated,
        } = run_worktree_job(job);
        if mutated {
            self.after_worktree_mutation(flash);
        }
        response
    }

    /// Hand a validated worktree job to a detached worker. The worker does the
    /// heavy IO, lands the result + the MCP reply channel in
    /// `runtime.worktree_results`, and wakes the loop; `apply_worktree_outcomes`
    /// re-applies + answers the client.
    pub(crate) fn spawn_worktree_job(&self, job: WorktreeJob, reply: Sender<McpResponse>) {
        let results = std::sync::Arc::clone(&self.runtime.worktree_results);
        let wake = self.runtime.pane_wake_tx.clone();
        std::thread::spawn(move || {
            let result = run_worktree_job(job);
            results
                .lock()
                .unwrap()
                .push(WorktreeOutcome { result, reply });
            // Wake AFTER the outcome is stored, so the pre-recv scan sees it.
            if let Some(tx) = wake {
                let _ = tx.send(Message::WorktreeJobDone);
            }
        });
    }

    /// Pre-recv drain: for each landed worker outcome, re-apply the
    /// listing/context update on the loop (success only) and THEN reply to the
    /// MCP client — preserving read-after-write. Returns whether a redraw is
    /// needed.
    pub(crate) fn apply_worktree_outcomes(&mut self) -> bool {
        let outcomes = std::mem::take(&mut *self.runtime.worktree_results.lock().unwrap());
        if outcomes.is_empty() {
            return false;
        }
        for WorktreeOutcome { result, reply } in outcomes {
            let WorktreeJobResult {
                response,
                flash,
                mutated,
            } = result;
            if mutated {
                self.after_worktree_mutation(flash);
            }
            // The client may have timed out (5 s) and dropped its receiver — a
            // failed send is fine; the refresh/context update already happened.
            let _ = reply.send(response);
        }
        true
    }

    /// Shared post-mutation step: surface the status flash, refresh the focused
    /// listing (the worktree lives in a sibling dir, so usually a no-op), and
    /// rewrite the context file synchronously (the client commonly reads it
    /// right after).
    fn after_worktree_mutation(&mut self, flash: Option<String>) {
        if let Some(message) = flash {
            self.state.flash_info(message);
        }
        self.state.refresh_listing();
        self.write_context();
    }
}
