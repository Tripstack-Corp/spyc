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
//! The interactive `W n` (worktree-create) key also runs its `gix` checkout on
//! this worker — a `WorktreeCompletion::InteractiveCreate` outcome that chdirs
//! the focused column into the new tree when it lands, instead of an MCP reply
//! (the full-tree checkout froze the input thread when it ran inline — the
//! code-review HIGH). `W d` removal stays synchronous (a plain
//! `git::worktree::remove`, no checkout). The synchronous
//! `App::execute_mcp_command` path (direct callers / tests) and this async path
//! share `plan_worktree_job` + `run_worktree_job` + `after_worktree_mutation`,
//! so the IO logic can't diverge — only the completion (`WorktreeCompletion`)
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
        /// Also open the new worktree in column `b` (create→work in one call).
        open: bool,
    },
    /// `remove_worktree`: safe-by-default teardown of `target` — archive
    /// untracked + uncommitted content to the graveyard, force-remove, delete
    /// the branch iff merged.
    Remove { target: std::path::PathBuf },
    /// `clean_worktree`: an alias of `Remove` (kept for the MCP tool name).
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
    /// `Some(path)` when `create_worktree` was asked to also open it — the
    /// main-thread reconcile opens column `b` there. Off-main here (no `App`).
    pub open_path: Option<std::path::PathBuf>,
    /// `Some(path)` on any `Create` success — the new worktree. The interactive
    /// `W n` completion chdirs the focused column here (the MCP completion
    /// ignores it and uses `open_path` / the response JSON).
    pub created_path: Option<std::path::PathBuf>,
}

/// What to do on the loop once a [`WorktreeJob`] lands: answer the MCP client,
/// or (interactive `W n`) chdir the focused column into the new worktree.
pub enum WorktreeCompletion {
    /// Reply to the MCP client after the loop re-applies listing/context.
    Mcp(Sender<McpResponse>),
    /// Interactive `W n`: chdir the focused column into the created worktree
    /// (+ flash + reconcile harpoon), instead of an MCP reply.
    InteractiveCreate,
}

/// A landed worker result plus its completion (MCP reply or interactive chdir),
/// applied once the main loop has re-coupled.
pub struct WorktreeOutcome {
    pub result: WorktreeJobResult,
    pub completion: WorktreeCompletion,
}

const fn err_result(message: String) -> WorktreeJobResult {
    WorktreeJobResult {
        response: McpResponse::Error { message },
        flash: None,
        mutated: false,
        open_path: None,
        created_path: None,
    }
}

/// Run the heavy IO for one worktree job and build its MCP reply. Pure infra
/// (no `App`): callable on the loop (`run_worktree_job_sync`) or off it
/// (`spawn_worktree_job`'s worker).
pub fn run_worktree_job(job: WorktreeJob) -> WorktreeJobResult {
    match job {
        WorktreeJob::Create {
            dir,
            branch,
            base,
            open,
        } => {
            match crate::git::worktree::add(&dir, &branch, base.as_deref()) {
                Ok(path) => {
                    let json = serde_json::json!({
                        "branch": branch,
                        "path": path.display().to_string(),
                    });
                    let opened = if open { " (opened in b)" } else { "" };
                    WorktreeJobResult {
                        response: McpResponse::Ok {
                            message: serde_json::to_string_pretty(&json).unwrap_or_default(),
                        },
                        flash: Some(format!(
                            "[mcp] created worktree {} ({branch}){opened}",
                            path.display()
                        )),
                        mutated: true,
                        // The main-thread reconcile opens `b` here (off-main can't).
                        open_path: open.then(|| path.clone()),
                        created_path: Some(path),
                    }
                }
                Err(e) => err_result(format!("worktree add: {e}")),
            }
        }
        // `clean` is folded into `remove`: both archive untracked + uncommitted
        // content to the graveyard, force-remove the tree, and delete the branch
        // iff merged (safe-by-default).
        WorktreeJob::Remove { target } | WorktreeJob::Clean { target } => {
            match crate::app::worktree_clean::safe_remove_worktree(&target) {
                Ok(report) => {
                    let message = safe_remove_message(&target, &report);
                    WorktreeJobResult {
                        flash: Some(format!("[mcp] {message}")),
                        response: McpResponse::Ok { message },
                        mutated: true,
                        open_path: None,
                        created_path: None,
                    }
                }
                Err(e) => err_result(format!("worktree remove: {e}")),
            }
        }
    }
}

/// Build the human/agent-facing message for a safe-remove outcome: what was
/// archived, and whether the branch was deleted (merged) or kept (unmerged).
fn safe_remove_message(
    target: &std::path::Path,
    report: &crate::app::worktree_clean::SafeRemoveReport,
) -> String {
    let mut parts = vec![format!("removed worktree {}", target.display())];
    if let Some(label) = &report.label {
        parts.push(format!(
            "archived {} uncommitted/untracked entr{} to the graveyard as '{label}'",
            report.archived,
            if report.archived == 1 { "y" } else { "ies" }
        ));
    }
    match (
        &report.branch,
        report.branch_deleted,
        report.kept_unmerged_ahead,
    ) {
        (Some(b), true, _) => parts.push(format!("deleted merged branch '{b}'")),
        (Some(b), false, Some(ahead)) => parts.push(format!(
            "kept branch '{b}' ({ahead} commit{} not in base)",
            if ahead == 1 { "" } else { "s" }
        )),
        (Some(b), false, _) => parts.push(format!("kept branch '{b}'")),
        (None, _, _) => parts.push("detached HEAD (no branch)".to_string()),
    }
    parts.join("; ")
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
            McpCommand::CreateWorktree { branch, base, open } => {
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
                        // Caller's explicit `base` override, else PROJECT_HOME's
                        // default branch (POLA: not whatever the focused column
                        // is on).
                        base: base.clone().or_else(|| {
                            self.state
                                .project_home
                                .as_deref()
                                .and_then(crate::git::branch::default_base)
                        }),
                        open: *open,
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
            open_path,
            created_path: _,
        } = run_worktree_job(job);
        if mutated {
            self.after_worktree_mutation(flash, open_path);
        }
        response
    }

    /// Hand a validated worktree job to a detached worker. The worker does the
    /// heavy IO, lands the result + the MCP reply channel in
    /// `runtime.worktree_results`, and wakes the loop; `apply_worktree_outcomes`
    /// re-applies + answers the client.
    pub(crate) fn spawn_worktree_job(&self, job: WorktreeJob, completion: WorktreeCompletion) {
        let results = std::sync::Arc::clone(&self.runtime.worktree_results);
        let wake = self.runtime.pane_wake_tx.clone();
        std::thread::spawn(move || {
            let result = run_worktree_job(job);
            results
                .lock()
                .unwrap()
                .push(WorktreeOutcome { result, completion });
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
        for WorktreeOutcome { result, completion } in outcomes {
            let WorktreeJobResult {
                response,
                flash,
                mutated,
                open_path,
                created_path,
            } = result;
            match completion {
                WorktreeCompletion::Mcp(reply) => {
                    if mutated {
                        self.after_worktree_mutation(flash, open_path);
                    }
                    // The client may have timed out (5 s) and dropped its
                    // receiver — a failed send is fine; the refresh/context
                    // update already happened.
                    let _ = reply.send(response);
                }
                WorktreeCompletion::InteractiveCreate => {
                    // Mirror the former synchronous `W n` completion, now that the
                    // checkout ran off-thread: chdir the focused column into the
                    // new tree (`chdir` refreshes its listing), or surface the
                    // add error. Harpoon reconciles either way (as the inline
                    // handler did).
                    match created_path {
                        Some(path) => {
                            self.state
                                .flash_info(format!("created worktree: {}", path.display()));
                            if let Err(e) = self.state.chdir(&path) {
                                self.state.flash_error(format!("chdir: {e}"));
                            }
                        }
                        None => {
                            if let McpResponse::Error { message } = response {
                                self.state.flash_error(message);
                            }
                        }
                    }
                    self.reconcile_harpoon();
                }
            }
        }
        true
    }

    /// Shared post-mutation step: optionally open the freshly-created worktree
    /// in column `b` (`create_worktree open=true`), surface the status flash,
    /// refresh the focused listing (the worktree lives in a sibling dir, so
    /// usually a no-op), and rewrite the context file synchronously (the client
    /// commonly reads it right after).
    fn after_worktree_mutation(
        &mut self,
        flash: Option<String>,
        open_path: Option<std::path::PathBuf>,
    ) {
        // Open `b` FIRST so the refresh + context capture the now-active column.
        // Background open: `b` becomes `cur()` (so the refresh/context see it and
        // the agent's follow-ups target it) without stealing the pane the user is
        // typing into.
        if let Some(path) = open_path {
            self.open_second_commander_at_background(&path);
        }
        if let Some(message) = flash {
            self.state.flash_info(message);
        }
        // A removal can delete the dir a column (A/B) was sitting in — snap any
        // such column back to PROJECT_HOME (with its own flash) before the
        // listing refresh, so we never refresh against a deleted cwd.
        self.state.reset_orphaned_columns_to_home();
        self.state.refresh_listing();
        self.write_context();
    }
}
