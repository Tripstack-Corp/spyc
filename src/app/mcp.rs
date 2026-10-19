//! MCP / context integration: snapshot the current state into the on-disk
//! context file MCP clients read, and execute writable MCP commands on the main
//! thread. Originally extracted verbatim from `app/mod.rs` (the impl-extraction
//! sweep), same child-module `impl App` pattern. `refresh_process_stats` /
//! `write_context` / `execute_mcp_command` are `pub` (called from `loop_steps`);
//! `snapshot_context` is `pub(super)` (its own module + reachable by tests).
//! All of these read/drive the FOCUSED commander via `cur()`, so the agent's
//! view + writable tools follow the column the user is working in.

use super::App;

/// A report's backstop expiry: what it asked for, within its status's cap,
/// which is also the default. Non-blocked reports fall back to output timing
/// once it passes if the agent stops reporting; blocked is latched until
/// answered or superseded by a newer report.
fn report_ttl(activity: crate::pane::AgentActivity, requested: Option<u64>) -> std::time::Duration {
    let cap = if activity == crate::pane::AgentActivity::Waiting {
        crate::mcp_cmd::MAX_WAITING_TTL_MS
    } else {
        crate::mcp_cmd::MAX_REPORT_TTL_MS
    };
    std::time::Duration::from_millis(requested.unwrap_or(cap).min(cap))
}

/// One `report_status` call, less its target.
struct StatusReport {
    status: String,
    ttl_ms: Option<u64>,
    note: Option<String>,
    session_id: Option<String>,
    hook_event: Option<crate::agent::status_hook::StatusHookEvent>,
}

impl App {
    /// Refresh `activity.proc_rss_kb` / `activity.proc_threads`. Called once
    /// per A-monitor 1 s tick. `proc_rss_threads` reads the OS directly
    /// (sysinfo for rss + libproc for the macOS thread count) — a fast
    /// syscall, not a `ps` fork-exec, so it runs inline (the off-thread
    /// machinery #227 added for the slow `ps` spawn is no longer needed).
    pub fn refresh_process_stats(&mut self) {
        if let Some((rss, threads)) = crate::sysinfo::proc_rss_threads() {
            self.view.activity.proc_rss_kb = rss;
            self.view.activity.proc_threads = threads;
        }
    }

    /// Build a context snapshot from the current state for MCP consumers.
    pub(super) fn snapshot_context(&self) -> crate::context::SpycContext {
        // The FOCUSED commander (`cur()`): with a second commander open, the
        // agent's context follows the column the user is working in — `cwd`,
        // `cursor_file`, `picks`, `filter`. Since the read-side MCP tools
        // (search_*, get_file_content) resolve relative paths against this
        // file's `cwd`, they follow focus for free — and so does `git_branch`
        // (`cur().git.info`), now that git is a per-column field. (`inventory`
        // is global.)
        let cur = self.state.cur();
        let cursor_file = cur.rows.get(cur.cursor.index).map(|r| r.display.clone());
        crate::context::SpycContext {
            root: self.state.active_project().start_dir.clone(),
            cwd: cur.listing.dir.clone(),
            cursor_file,
            picks: cur.picks.iter().cloned().collect(),
            inventory: self.state.inventory.paths().cloned().collect(),
            filter: cur.temp_filter.clone(),
            git_branch: cur.git.info.clone(),
            project_home: self.state.active_project().home.clone(),
            // Scope MCP search to the focused column's worktree root (falls back
            // to PROJECT_HOME / cwd inside `tool_root`), so `search_paths` /
            // `search_content` follow the column the user is working in — the
            // same root grep `F` / find / harpoon use.
            search_root: Some(self.state.tool_root(self.state.focused_side())),
            session_name: self.state.session_name.clone().unwrap_or_default(),
            // Identify the running instance: our PID (the process the writable
            // tools reach over the socket) + build (version + git SHA) so a
            // client can detect a stale server and name what to restart.
            pid: std::process::id(),
            version: crate::VERSION.to_string(),
            // A member's bytes aren't on disk at its path, so a reader outside
            // this process can't tell a member from a missing file without being
            // told where the mounts are.
            archive_mounts: self
                .state
                .mounts
                .iter()
                .map(|m| crate::context::ArchiveMountRef {
                    root: m.archive().to_path_buf(),
                    staging: m.staging_root.clone(),
                    source: (m.source() != m.archive()).then(|| m.source().to_path_buf()),
                })
                .collect(),
        }
    }

    /// Write the context file (best-effort, errors are silently ignored).
    /// Skips the disk write when the serialized JSON is unchanged.
    pub fn write_context(&mut self) {
        let ctx = self.snapshot_context();
        // Skip the disk write when state is unchanged. Compare the snapshot
        // struct directly rather than its serialized JSON: equal structs
        // serialize to equal JSON (the snapshot has no nondeterministic
        // fields), so this is the same dedup decision without serializing a
        // second time purely to compare — `write_context_file` does the one
        // and only serialization, on the write path.
        if self.view.project().last_context.as_ref() == Some(&ctx) {
            return;
        }
        // MVU Phase 3d: only advance the dedup cache when the write actually
        // landed. If the write fails, `last_context` stays behind disk, so a
        // later identical-state mutation still writes instead of dedup-skipping
        // into a stale file. (The 500ms cap used to mask this by re-running the
        // debounced writer; it's gone now.)
        if crate::context::write_context_file(&self.view.project().context_path, &ctx).is_ok() {
            // The sidecar is discovery's record of the roots, every open
            // project's (#99), so it moves when one does (a `spyc -r` restoring
            // a project elsewhere, #523).
            let moved = self
                .view
                .project()
                .last_context
                .as_ref()
                .is_none_or(|c| c.root != ctx.root);
            if moved {
                self.record_open_roots();
            }
            self.view.project_mut().last_context = Some(ctx);
        }
    }

    /// Rewrite discovery's record of this spyc's roots, every open project's
    /// start dir (#99), while the MCP server runs.
    pub(super) fn record_open_roots(&self) {
        if !self.view.mcp_running {
            return;
        }
        let roots: Vec<&std::path::Path> = self
            .state
            .open_projects()
            .map(|p| p.start_dir.as_path())
            .collect();
        crate::mcp::record_roots(&roots);
    }

    /// Resolve an MCP worktree-path argument and guard it: trim/require it,
    /// resolve a relative path against the focused column's dir (create_worktree
    /// hands back an absolute path, but be lenient), then refuse if a column is
    /// currently open inside it — removing/cleaning it would strand that column
    /// on a deleted dir (mirrors git refusing to touch the current worktree).
    /// `Err` is a ready-to-send reason. Shared by RemoveWorktree + CleanWorktree.
    pub(crate) fn resolve_worktree_arg(&self, path: &str) -> Result<std::path::PathBuf, String> {
        let path = path.trim();
        if path.is_empty() {
            return Err("missing required parameter: path".into());
        }
        let raw = std::path::PathBuf::from(path);
        let target = if raw.is_relative() {
            // Not the raw column dir: inside an archive mount that is a path
            // *within the container*, so a relative worktree path would resolve
            // to somewhere that cannot exist.
            self.state.worktree_anchor().join(&raw)
        } else {
            raw
        };
        // A column sitting inside the target is NOT refused: the removal
        // proceeds, and `reset_orphaned_columns_to_home` (run in
        // `after_worktree_mutation`) snaps that column back to PROJECT_HOME with
        // a flash — preferred over stranding the user with a refusal.
        Ok(target)
    }

    /// `open_worktree`: open (or re-target) column `b` at `path`, answering
    /// with where it opened or why it didn't.
    fn mcp_open_worktree(&mut self, path: &str) -> crate::mcp_cmd::McpResponse {
        use crate::mcp_cmd::McpResponse;
        let path = path.trim();
        if path.is_empty() {
            return McpResponse::Error {
                message: "missing required parameter: path".into(),
            };
        }
        let raw = std::path::PathBuf::from(path);
        let target = if raw.is_relative() {
            self.state.worktree_anchor().join(&raw)
        } else {
            raw
        };
        if !target.is_dir() {
            return McpResponse::Error {
                message: format!("not a directory: {}", target.display()),
            };
        }
        // Open (or re-target) column `b` at the worktree. `cur()` now
        // resolves to `b`, so a follow-up navigate_to/search/pick lands
        // there while `a` stays put. Background open: the user keeps
        // typing to the pane below — we don't steal keyboard focus.
        if let Err(e) = self.open_second_commander_at_background(&target) {
            let message = format!("open {}: {e:#}", target.display());
            self.state.flash_error(format!("[mcp] {message}"));
            return McpResponse::Error { message };
        }
        let opened = self.state.cur().listing.dir.clone();
        self.write_context();
        McpResponse::Ok {
            message: format!("opened column b at {}", opened.display()),
        }
    }

    /// Execute a writable MCP command from Claude. Runs on the main
    /// thread with full access to `AppState`. Returns a response that
    /// the MCP server thread forwards to Claude.
    pub fn execute_mcp_command(
        &mut self,
        cmd: crate::mcp_cmd::McpCommand,
    ) -> crate::mcp_cmd::McpResponse {
        use crate::mcp_cmd::{McpCommand, McpResponse};
        if cmd.is_notice() {
            return self.execute_notice(cmd);
        }
        // Heavy worktree ops (create/remove/clean) go through the shared planner
        // + job. The production drain off-threads them (`spawn_worktree_job`); a
        // direct caller / test runs them synchronously here. Both share
        // `run_worktree_job` + `after_worktree_mutation`, so the sync and async
        // paths can't diverge.
        if let Some(planned) = self.plan_worktree_job(&cmd) {
            return match planned {
                Ok(job) => self.run_worktree_job_sync(job),
                Err(resp) => resp,
            };
        }
        match cmd {
            McpCommand::NavigateTo { path } => {
                match self.state.jump_to(&path) {
                    Ok(()) => {
                        self.state.flash_info(format!(
                            "[mcp] navigated to {}",
                            self.state.cur().listing.dir.display()
                        ));
                        // Write synchronously: an MCP client commonly
                        // calls get_spyc_context right after a mutation,
                        // and that reads the on-disk context file. The
                        // debounced gate can lag seconds behind under a
                        // typing burst, so the follow-up read would see
                        // stale state. (Non-MCP edits stay debounced.)
                        self.write_context();
                        let ctx = self.snapshot_context();
                        let json = serde_json::to_string_pretty(&ctx).unwrap_or_default();
                        McpResponse::Ok { message: json }
                    }
                    Err(e) => McpResponse::Error {
                        message: format!("navigate failed: {e}"),
                    },
                }
            }
            McpCommand::SetFilter { pattern } => {
                match pattern {
                    Some(ref p) if p.is_empty() => self.state.cur_mut().temp_filter = None,
                    Some(p) => self.state.cur_mut().temp_filter = Some(p),
                    None => self.state.cur_mut().temp_filter = None,
                }
                self.state.rebuild_rows();
                let count = self.state.cur().rows.len();
                let label = self
                    .state
                    .cur()
                    .temp_filter
                    .as_deref()
                    .unwrap_or("(cleared)");
                self.state.flash_info(format!("[mcp] filter: {label}"));
                self.write_context();
                McpResponse::Ok {
                    message: format!("filter applied, {count} items visible"),
                }
            }
            McpCommand::PickFiles { patterns } => {
                // Collect matches first (immutable borrow of the focused
                // commander's entries), then insert — `cur()`/`cur_mut()` borrow
                // all of `state`, so iterating entries while inserting picks in
                // the same loop would alias. Targets the FOCUSED column.
                let mut errors = Vec::new();
                let mut to_pick: Vec<std::path::PathBuf> = Vec::new();
                for pat_str in &patterns {
                    match glob::Pattern::new(pat_str) {
                        Ok(pat) => {
                            for e in &self.state.cur().listing.entries {
                                if pat.matches(&e.name) {
                                    to_pick.push(e.path.clone());
                                }
                            }
                        }
                        Err(e) => errors.push(format!("{pat_str}: {e}")),
                    }
                }
                if !errors.is_empty() {
                    return McpResponse::Error {
                        message: format!("invalid patterns: {}", errors.join(", ")),
                    };
                }
                let total = to_pick.len();
                for path in &to_pick {
                    self.state.cur_mut().picks.insert(path);
                }
                let next_gen = self.state.cur().list_generation.wrapping_add(1);
                self.state.cur_mut().list_generation = next_gen;
                self.state
                    .flash_info(format!("[mcp] picked {total} file(s)"));
                self.write_context();
                McpResponse::Ok {
                    message: format!(
                        "picked {total} file(s), {} total",
                        self.state.cur().picks.len()
                    ),
                }
            }
            McpCommand::ClearPicks => {
                let count = self.state.cur().picks.len();
                self.state.cur_mut().picks.clear();
                let next_gen = self.state.cur().list_generation.wrapping_add(1);
                self.state.cur_mut().list_generation = next_gen;
                self.state.flash_info("[mcp] picks cleared");
                self.write_context();
                McpResponse::Ok {
                    message: format!("cleared {count} pick(s)"),
                }
            }
            McpCommand::CreateWorktree { .. }
            | McpCommand::RemoveWorktree { .. }
            | McpCommand::CleanWorktree { .. } => {
                // Routed above via `plan_worktree_job` (sync) or by the drain
                // (off-thread) — never reached through this match.
                unreachable!("worktree create/remove/clean go through plan_worktree_job")
            }
            McpCommand::OpenWorktree { path } => self.mcp_open_worktree(&path),
            McpCommand::ReportStatus {
                pane_id,
                pane,
                status,
                ttl_ms,
                note,
                session_id,
                hook_event,
            } => self.apply_status_report(
                pane_id.as_deref(),
                pane,
                StatusReport {
                    status,
                    ttl_ms,
                    note,
                    session_id,
                    hook_event,
                },
            ),
            McpCommand::RegisterScope {
                pane_id,
                pane,
                paths,
                intent,
                pr,
                note,
            } => {
                use crate::state::scope_registry::{ScopeClaim, ScopeIntent, conflicts};
                let Some(parsed_intent) = ScopeIntent::parse(&intent) else {
                    return McpResponse::Error {
                        message: format!("intent must be 'editing' or 'merging', got '{intent}'"),
                    };
                };
                if paths.is_empty() {
                    return McpResponse::Error {
                        message: "register_scope needs at least one path".into(),
                    };
                }
                // Resolve the claiming tab → its stable owner key + label (same
                // targeting as `report_status`: pane_id → pane → focused).
                let Some(tabs) = self.runtime.project().pane_tabs.as_ref() else {
                    return McpResponse::Error {
                        message: "no pane open".into(),
                    };
                };
                let ids: Vec<&str> = tabs.tabs().iter().map(|t| t.info.id.as_str()).collect();
                let idx = match resolve_report_target(
                    pane_id.as_deref(),
                    pane,
                    &ids,
                    tabs.active_index(),
                ) {
                    Ok(i) => i,
                    Err(message) => return McpResponse::Error { message },
                };
                let info = &tabs.tabs()[idx].info;
                let owner = info.claim_owner.clone();
                let owner_label = info.label.clone();
                // Opaque monotonic-ish id (one past the current max). Reuse after a
                // full release is harmless — waiters key on paths, not id.
                let claim_id = self
                    .state
                    .scope_registry
                    .iter()
                    .map(|c| c.id)
                    .max()
                    .unwrap_or(0)
                    + 1;
                let claim = ScopeClaim {
                    id: claim_id,
                    owner: owner.clone(),
                    owner_label,
                    paths,
                    intent: parsed_intent,
                    pr,
                    note,
                    claimed_at_secs: crate::sysinfo::epoch_secs(),
                };
                // Surface any blocking overlap now, so the agent can decide to
                // `wait_for_scope_clear` without a separate `list_scopes` round-trip.
                let blocking: Vec<_> = conflicts(&self.state.scope_registry, &owner, &claim.paths)
                    .iter()
                    .map(|c| {
                        serde_json::json!({ "owner": c.owner_label, "pr": c.pr, "paths": c.paths })
                    })
                    .collect();
                self.state.scope_registry.push(claim);
                McpResponse::Ok {
                    message: serde_json::json!({
                        "claim_id": claim_id,
                        "conflicting_merges": blocking,
                    })
                    .to_string(),
                }
            }
            McpCommand::ListScopes => {
                let json = serde_json::to_string(&self.state.scope_registry)
                    .unwrap_or_else(|_| "[]".to_string());
                McpResponse::Ok { message: json }
            }
            McpCommand::ReleaseScope { id } => {
                let before = self.state.scope_registry.len();
                self.state.scope_registry.retain(|c| c.id != id);
                let dropped = before - self.state.scope_registry.len();
                McpResponse::Ok {
                    message: format!("released {dropped} claim(s) with id {id}"),
                }
            }
            // `wait_for_scope_clear` is intercepted + parked in
            // `drain_mcp_pending` (it can't reply inline), so it never reaches
            // here — answer defensively rather than panic if that ever changes.
            McpCommand::WaitForScopeClear { .. } => McpResponse::Error {
                message: "wait_for_scope_clear is handled by the loop's park path".into(),
            },
            c @ (McpCommand::Bind { .. } | McpCommand::CallerContext { .. }) => {
                self.answer_binding(c)
            }
            // Notices, which `execute_notice` took before this match.
            McpCommand::Statusline { .. }
            | McpCommand::Disconnected { .. }
            | McpCommand::ConnectionInitialized { .. }
            | McpCommand::ConnectionClosed { .. }
            | McpCommand::ToolCalled { .. } => McpResponse::Ok {
                message: "ok".into(),
            },
            McpCommand::MalformedSocketMessage { detail } => {
                // The socket server couldn't frame/parse a message and dropped
                // it. Surface it — a silent drop hid the bare-newline
                // report-status framing bug for days — and tally it under
                // `malformed` in the `A`-overlay / `:activity dump` mcp counts.
                *self
                    .view
                    .activity
                    .mcp_tool_calls
                    .entry("malformed".to_string())
                    .or_insert(0) += 1;
                self.state.flash_error(format!(
                    "\u{26a0} MCP: dropped a malformed socket message ({detail}) — see mcp.log"
                ));
                McpResponse::Ok {
                    message: "noted".into(),
                }
            }
        }
    }

    /// A notice ([`crate::mcp_cmd::McpCommand::is_notice`]): it only records
    /// something.
    fn execute_notice(&mut self, cmd: crate::mcp_cmd::McpCommand) -> crate::mcp_cmd::McpResponse {
        use crate::mcp_cmd::{McpCommand, McpResponse};
        match cmd {
            McpCommand::Statusline {
                pane_id,
                statusline,
            } => {
                self.land_statusline(&pane_id, statusline);
                McpResponse::Ok {
                    message: "acknowledged".into(),
                }
            }
            // Keep serving: agents this spyc launches still reach it through
            // their pane's env, and the next one it launches rewrites the entry
            // to pin nothing again.
            McpCommand::Disconnected { new_pid } => {
                self.state.flash_info(format!(
                    "MCP: an older spyc (PID {new_pid}) pinned this dir's agent config to \
                     itself; agents started outside spyc will reach it"
                ));
                McpResponse::Ok {
                    message: "acknowledged".into(),
                }
            }
            McpCommand::ConnectionInitialized { conn, pane_id } => {
                self.view.activity.mcp_connections.insert(
                    conn,
                    super::activity::McpConnection {
                        pane_id,
                        since: crate::sysinfo::format_now(),
                        calls: 0,
                    },
                );
                McpResponse::Ok {
                    message: "ok".into(),
                }
            }
            McpCommand::ConnectionClosed { conn } => {
                self.view.activity.mcp_connections.remove(&conn);
                McpResponse::Ok {
                    message: "ok".into(),
                }
            }
            McpCommand::ToolCalled { name, conn } => {
                if let Some(c) = conn.and_then(|c| self.view.activity.mcp_connections.get_mut(&c)) {
                    c.calls += 1;
                }
                // Telemetry only: bump the cumulative per-tool tally (for the `A`
                // overlay) + the 1 Hz aggregate `mcp:N/s` rate. Sent for every
                // tools/call (reads included), so this is the SOLE `mcp_reqs`
                // bump — the writable commands no longer count themselves. No
                // context write; the reply is discarded.
                *self.view.activity.mcp_tool_calls.entry(name).or_insert(0) += 1;
                self.view.activity.live.mcp_reqs =
                    self.view.activity.live.mcp_reqs.saturating_add(1);
                McpResponse::Ok {
                    message: "ok".into(),
                }
            }
            _ => McpResponse::Error {
                message: "not a notice".into(),
            },
        }
    }

    /// P2 `wait_for_scope_clear`: answer immediately if the caller's queried
    /// scope is already clear, else **park** the reply sender until
    /// `settle_scope_waiters` fires it on a clear / timeout. Owns its reply
    /// (park-or-answer) rather than returning an `McpResponse` inline. Resolves
    /// the caller's owner key from the same pane targeting as `report_status`, so
    /// the caller's *own* claims never block it.
    pub(crate) fn handle_scope_wait(
        &mut self,
        pane_id: Option<String>,
        pane: Option<usize>,
        paths: Vec<String>,
        timeout_ms: u64,
        reply: std::sync::mpsc::Sender<crate::mcp_cmd::McpResponse>,
    ) {
        use crate::mcp_cmd::McpResponse;
        if paths.is_empty() {
            let _ = reply.send(McpResponse::Error {
                message: "wait_for_scope_clear needs at least one path".into(),
            });
            return;
        }
        let Some(tabs) = self.runtime.project().pane_tabs.as_ref() else {
            let _ = reply.send(McpResponse::Error {
                message: "no pane open".into(),
            });
            return;
        };
        let ids: Vec<&str> = tabs.tabs().iter().map(|t| t.info.id.as_str()).collect();
        let owner = match resolve_report_target(pane_id.as_deref(), pane, &ids, tabs.active_index())
        {
            Ok(idx) => tabs.tabs()[idx].info.claim_owner.clone(),
            Err(message) => {
                let _ = reply.send(McpResponse::Error { message });
                return;
            }
        };
        // Fast path: already clear ⇒ answer now, don't park.
        if crate::state::scope_registry::conflicts(&self.state.scope_registry, &owner, &paths)
            .is_empty()
        {
            let _ = reply.send(McpResponse::Ok {
                message: serde_json::json!({ "outcome": "cleared", "conflicts": [] }).to_string(),
            });
            return;
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        self.runtime.scope_waiters.push(super::PendingScopeWait {
            owner,
            paths,
            deadline,
            reply,
        });
    }

    /// P2 `wait_for_scope_clear` settle (PRE-recv): resolve each parked waiter —
    /// `cleared` once its scope has no blocking `Merging` conflict, `timed_out`
    /// once its deadline passes — and re-arm `Deadline::ScopeWait` at the
    /// earliest remaining deadline (disarmed when none remain, so idle stays 0
    /// dps). Runs after `drain_mcp_pending` so a `release_scope` in the same tick
    /// unblocks its waiters at once.
    pub(crate) fn settle_scope_waiters(
        &mut self,
        now: std::time::Instant,
        ctx: &mut super::RunCtx,
    ) {
        use crate::state::scope_registry::{WaitOutcome, conflicts, wait_outcome};
        if self.runtime.scope_waiters.is_empty() {
            ctx.scheduler.disarm(super::Deadline::ScopeWait);
            return;
        }
        // Take the waiters out so we can read `scope_registry` immutably while
        // deciding each — `retain`'s closure would otherwise alias `self`.
        let waiters = std::mem::take(&mut self.runtime.scope_waiters);
        let mut still_waiting = Vec::new();
        let mut earliest: Option<std::time::Instant> = None;
        for w in waiters {
            let blocking = conflicts(&self.state.scope_registry, &w.owner, &w.paths);
            match wait_outcome(!blocking.is_empty(), now >= w.deadline) {
                Some(WaitOutcome::Cleared) => {
                    let _ = w.reply.send(crate::mcp_cmd::McpResponse::Ok {
                        message: serde_json::json!({ "outcome": "cleared", "conflicts": [] })
                            .to_string(),
                    });
                }
                Some(WaitOutcome::TimedOut) => {
                    let conflicts_json: Vec<_> = blocking
                        .iter()
                        .map(|c| {
                            serde_json::json!({ "owner": c.owner_label, "pr": c.pr, "paths": c.paths })
                        })
                        .collect();
                    let _ = w.reply.send(crate::mcp_cmd::McpResponse::Ok {
                        message: serde_json::json!({
                            "outcome": "timed_out",
                            "conflicts": conflicts_json,
                        })
                        .to_string(),
                    });
                }
                None => {
                    earliest = Some(earliest.map_or(w.deadline, |m| m.min(w.deadline)));
                    still_waiting.push(w);
                }
            }
        }
        self.runtime.scope_waiters = still_waiting;
        match earliest {
            Some(when) => ctx.scheduler.arm(super::Deadline::ScopeWait, when),
            None => ctx.scheduler.disarm(super::Deadline::ScopeWait),
        }
    }
}

/// Resolve a `report_status` target tab index from its optional pane id (uuid)
/// / 1-based `pane` index / focused fallback — the testable core of the
/// `ReportStatus` arm. Priority: `pane_id` (the stable `SPYC_PANE_ID` the hook
/// sends — survives reorder) → `pane` → `active`. A `pane_id` matching no live
/// tab is an **error**, not a silent fall-through: a stale report from a closed
/// pane must never clobber whatever tab happens to be focused. `ids` are the
/// tabs' ids in order; `active` is the focused index.
fn resolve_report_target(
    pane_id: Option<&str>,
    pane: Option<usize>,
    ids: &[&str],
    active: usize,
) -> Result<usize, String> {
    if let Some(pid) = pane_id {
        return ids
            .iter()
            .position(|id| *id == pid)
            .ok_or_else(|| format!("no pane with id {pid} (closed?)"));
    }
    match pane {
        Some(n) if (1..=ids.len()).contains(&n) => Ok(n - 1),
        Some(n) => Err(format!("no pane {n} (have {})", ids.len())),
        None => Ok(active),
    }
}

impl App {
    /// `report_status`: record the report on its tab and apply it to the dot,
    /// unless the Codex question model or a live `waiting` holds it back, or
    /// it is the agent's own `waiting` sent before its turn has ended.
    fn apply_status_report(
        &mut self,
        pane_id: Option<&str>,
        pane: Option<usize>,
        report: StatusReport,
    ) -> crate::mcp_cmd::McpResponse {
        use crate::agent::waiting;
        use crate::mcp_cmd::McpResponse;
        use crate::pane::ReportedStatus;
        let StatusReport {
            status,
            ttl_ms,
            note,
            session_id,
            hook_event,
        } = report;
        let Some(activity) = report_activity(&status) else {
            return McpResponse::Error {
                message: format!("unknown status: {status}"),
            };
        };
        let Some(tabs) = self.runtime.project_mut().pane_tabs.as_mut() else {
            return McpResponse::Error {
                message: "no pane open".into(),
            };
        };
        let ids: Vec<&str> = tabs.tabs().iter().map(|t| t.info.id.as_str()).collect();
        let idx = match resolve_report_target(pane_id, pane, &ids, tabs.active_index()) {
            Ok(i) => i,
            Err(message) => return McpResponse::Error { message },
        };
        let now = std::time::Instant::now();
        let entry = &mut tabs.tabs_mut()[idx];
        if entry.pane.is_closed() {
            return McpResponse::Error {
                message: "pane has exited; activity report rejected".into(),
            };
        }
        let kind = entry.info.agent().kind();
        let decision = if kind == crate::state::sessions::AgentKind::Codex {
            self.state
                .codex_recovery
                .entry(entry.info.id.clone())
                .or_default()
                .report(
                    &status,
                    activity,
                    session_id.as_deref(),
                    hook_event.as_ref(),
                )
        } else {
            report_decision(kind, &status, activity)
        };
        // A direct agent call carries neither; spyc's status hook always sends
        // at least one.
        let origin = if hook_event.is_some() || session_id.is_some() {
            waiting::Origin::Hook(hook_event.as_ref())
        } else {
            waiting::Origin::Agent
        };
        let promoted = follow_turn(&mut entry.info, activity, origin);
        if decision.is_ok() && waiting::held_for_turn_end(activity, origin, entry.info.turn_open) {
            return hold_waiting(&mut entry.info, idx, ttl_ms, note, now);
        }
        let verdict = waiting::meet(entry.info.reported.as_ref(), activity, origin, now);
        let decision = decision.and_then(|a| verdict.ignored_because().map_or(Ok(a), Err));
        if verdict == waiting::Verdict::AbsorbTurnEnd
            && let Some(live) = entry.info.reported.as_mut()
        {
            live.turn_end_absorbed = true;
        }
        let mut received = ReportedStatus::new(activity, now, now + report_ttl(activity, ttl_ms));
        received.note = waiting::note_for(
            entry.info.reported.as_ref(),
            activity,
            note,
            hook_event.as_ref(),
            now,
        );
        entry.info.last_reported = Some(received.clone());
        entry.info.last_report_ignored = decision.err();
        if decision.is_ok() {
            entry.info.reported = Some(received);
        }
        if let Some(event) = hook_event {
            record_hook_event(&mut entry.info, event, activity, now);
        }
        // Apply immediately so this frame reflects it; `settle_agent_activity`
        // maintains it (and falls back to timing once it expires).
        if decision.is_ok() {
            entry.info.activity = activity;
        } else if promoted && let Some(live) = &entry.info.reported {
            entry.info.activity = live.status;
        }
        if let Some(sid) = session_id.filter(|_| decision.is_ok()) {
            pin_live_session(&mut entry.info, kind, sid);
        }
        let pane_id = entry.info.id.clone();
        let label = entry.info.label.clone();
        self.state.agent_hud_status_reported(&pane_id);
        McpResponse::Ok {
            message: match decision {
                Ok(_) => format!("status '{status}' set for pane {} ({label})", idx + 1),
                Err(reason) => format!(
                    "status '{status}' recorded for pane {} ({label}); not applied: {reason}",
                    idx + 1
                ),
            },
        }
    }
}

/// Keep a hook event for the diagnostics, oldest dropped past the limit.
fn record_hook_event(
    info: &mut crate::pane::tabs::TabInfo,
    event: crate::agent::status_hook::StatusHookEvent,
    activity: crate::pane::AgentActivity,
    now: std::time::Instant,
) {
    let history = &mut info.recent_hook_events;
    if history.len() >= crate::agent::status_hook::HISTORY_LIMIT {
        history.pop_front();
    }
    history.push_back((event, activity, now));
}

/// Why the agent's own `waiting` waits: it was sent before its turn ended.
const HELD_FOR_TURN_END: &str = "held until this turn ends";

/// Follow the agent's turn through a hook report. The turn's end makes a
/// `waiting` the agent sent during it the live report, which then meets that
/// end like a report sent there; any newer report from the agent replaces it.
/// Returns whether a held report took over.
fn follow_turn(
    info: &mut crate::pane::tabs::TabInfo,
    activity: crate::pane::AgentActivity,
    origin: crate::agent::waiting::Origin<'_>,
) -> bool {
    let crate::agent::waiting::Origin::Hook(event) = origin else {
        info.held_waiting = None;
        return false;
    };
    let was_open = info.turn_open;
    info.turn_open = crate::agent::waiting::turn_open_after(was_open, activity, event);
    if was_open
        && !info.turn_open
        && let Some(held) = info.held_waiting.take()
    {
        info.reported = Some(held);
        return true;
    }
    false
}

/// Record the agent's own `waiting`, sent while its turn is open, to take
/// effect when the turn ends; the dot keeps showing the turn until then.
fn hold_waiting(
    info: &mut crate::pane::tabs::TabInfo,
    idx: usize,
    ttl_ms: Option<u64>,
    note: Option<String>,
    now: std::time::Instant,
) -> crate::mcp_cmd::McpResponse {
    let activity = crate::pane::AgentActivity::Waiting;
    let mut held =
        crate::pane::ReportedStatus::new(activity, now, now + report_ttl(activity, ttl_ms));
    held.note = note;
    info.last_reported = Some(held.clone());
    info.last_report_ignored = Some(HELD_FOR_TURN_END);
    info.held_waiting = Some(held);
    crate::mcp_cmd::McpResponse::Ok {
        message: format!(
            "status 'waiting' recorded for pane {} ({}); not applied: {HELD_FOR_TURN_END}",
            idx + 1,
            info.label
        ),
    }
}

/// The state a `report_status` name sets, Codex's correlated question
/// signals included.
fn report_activity(status: &str) -> Option<crate::pane::AgentActivity> {
    use crate::agent::codex_recovery::{QUESTION_END, QUESTION_START};
    use crate::pane::AgentActivity;
    match status {
        QUESTION_END => Some(AgentActivity::Working),
        QUESTION_START => Some(AgentActivity::Blocked),
        other => AgentActivity::reportable(other),
    }
}

/// P1-3: a live session id piggybacked on the hook report supersedes the
/// spawn-proximity resolver at save time — route it to the id field for this
/// tab's agent. claude reports `session_id` and agy `conversationId` (both
/// land in `live_session_id`); codex has its own spawn-ordered rollout claim.
fn pin_live_session(
    info: &mut crate::pane::tabs::TabInfo,
    kind: crate::state::sessions::AgentKind,
    sid: String,
) {
    use crate::state::sessions::AgentKind;
    match kind {
        AgentKind::Claude | AgentKind::Agy => info.live_session_id = Some(sid),
        AgentKind::Codex => info.codex_session_id = Some(sid),
        _ => {}
    }
}

/// Whether a report to a non-Codex tab moves its dot (Codex's go through its
/// question recovery). A question signal needs a Codex pane. A tab with no
/// agent keeps the report as evidence only: a shell tab's agents can reach the
/// socket, but until the tab knows which one runs, a report is not a status.
fn report_decision(
    kind: crate::state::sessions::AgentKind,
    status: &str,
    activity: crate::pane::AgentActivity,
) -> Result<crate::pane::AgentActivity, &'static str> {
    use crate::agent::codex_recovery::{QUESTION_END, QUESTION_START};
    if matches!(status, QUESTION_START | QUESTION_END) {
        Err("Codex question hook requires a Codex pane")
    } else if kind == crate::state::sessions::AgentKind::Other {
        Err("no agent is running in this tab")
    } else {
        Ok(activity)
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_report_target;

    #[test]
    fn report_target_prefers_pane_id_then_index_then_focused() {
        let ids = ["aaa", "bbb", "ccc"];
        // pane_id wins and resolves to its position, regardless of `pane`.
        assert_eq!(resolve_report_target(Some("bbb"), Some(1), &ids, 0), Ok(1));
        // A pane_id for a closed/unknown pane is an error (no silent fallback).
        assert!(resolve_report_target(Some("zzz"), None, &ids, 0).is_err());
        // No pane_id: 1-based `pane` index.
        assert_eq!(resolve_report_target(None, Some(3), &ids, 0), Ok(2));
        // Out-of-range index errors.
        assert!(resolve_report_target(None, Some(9), &ids, 0).is_err());
        assert!(resolve_report_target(None, Some(0), &ids, 0).is_err());
        // Neither → the focused tab.
        assert_eq!(resolve_report_target(None, None, &ids, 2), Ok(2));
    }
}
