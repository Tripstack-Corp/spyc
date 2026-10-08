//! MCP / context integration: write the `.mcp.json` / `.codex/config.toml`
//! client config when an agent pane launches (and remove it again on exit),
//! snapshot the current state into the on-disk context file MCP clients read,
//! and execute writable MCP commands on the main thread. Originally extracted
//! verbatim from `app/mod.rs` (the impl-extraction sweep), same child-module
//! `impl App` pattern. `ensure_agent_mcp_config` / `cleanup_written_mcp_configs`
//! / `refresh_process_stats` / `write_context` / `execute_mcp_command` are
//! `pub` (called from the pane-launch path / `run_teardown` / `loop_steps`);
//! `snapshot_context` is `pub(super)` (its own module + reachable by tests).
//! All of these read/drive the FOCUSED commander via `cur()`, so the agent's
//! view + writable tools follow the column the user is working in.

use super::App;

/// Backstop expiry for a `report_status` self-report (5 min) when the agent
/// gives no `ttl_ms`. Non-blocked reports eventually fall back to output timing
/// if the agent stops reporting; blocked is latched until answered or superseded
/// by a newer report. Overridable per-report.
const DEFAULT_REPORT_TTL_MS: u64 = crate::mcp_cmd::MAX_REPORT_TTL_MS;

impl App {
    /// Write the MCP client config a *launching* agent needs to discover spyc's
    /// socket — `.mcp.json` for claude, `.codex/config.toml` for codex — into
    /// `cwd` (the pane's launch dir). Called from the pane-launch path
    /// ([`open_pane_tab_in`](Self::open_pane_tab_in)) right before the pty
    /// spawns, NOT at startup: we only create these files in directories where
    /// the user actually runs the agent, instead of writing them into every
    /// directory spyc is ever opened in. A no-op when the MCP socket isn't
    /// running or the command isn't a config-needing agent.
    pub fn ensure_agent_mcp_config(&mut self, cmd: &str, cwd: &std::path::Path) {
        if !self.view.mcp_running {
            return;
        }
        // The entry names no instance (`mcp-entry-names-no-socket`), so writing
        // it is safe whichever spyc wrote it last; what we record is that this
        // one relies on it, so a sibling's teardown leaves it in place.
        let wrote = match crate::agent::detect(cmd).kind() {
            crate::state::sessions::AgentKind::Claude => match crate::mcp::ensure_mcp_json(cwd) {
                Ok(crate::mcp::McpConfigStatus::Configured) => true,
                Ok(crate::mcp::McpConfigStatus::BlockedByEnterprise) => {
                    self.state.flash_error(
                        "MCP: blocked by enterprise policy (deniedMcpServers or allowedMcpServers)",
                    );
                    false
                }
                Ok(crate::mcp::McpConfigStatus::ManagedByEnterprise) => {
                    self.state
                        .flash_info("MCP: enterprise-managed (skipped local .mcp.json)");
                    false
                }
                Err(e) => {
                    self.state.flash_error(format!(".mcp.json: {e:#}"));
                    false
                }
            },
            crate::state::sessions::AgentKind::Agy => {
                match crate::mcp::ensure_agy_mcp_config(cwd) {
                    Ok(crate::mcp::McpConfigStatus::Configured) => true,
                    Ok(_) => false,
                    Err(e) => {
                        self.state.flash_error(format!("mcp_config.json: {e:#}"));
                        false
                    }
                }
            }
            // `[pane] codex_mcp = false`: skip registering our MCP server for
            // codex and strip any entry we wrote before, so codex has no spyc
            // tool to call — the escape hatch for codex's /review elicitation
            // hang (openai/codex#25856). Status hooks are written separately
            // (maybe_preinstall_startup_hooks), so activity dots keep working.
            crate::state::sessions::AgentKind::Codex if !self.state.config.pane.codex_mcp => {
                let _ = crate::mcp::cleanup_codex_config(cwd);
                self.state
                    .flash_info("codex MCP off ([pane] codex_mcp=false) — /review workaround");
                false
            }
            // Enterprise-flavoured statuses are claude-specific; codex shouldn't
            // return them, but if it ever does we treat them as a no-op.
            crate::state::sessions::AgentKind::Codex => {
                match crate::mcp::ensure_codex_config_toml(cwd) {
                    Ok(crate::mcp::McpConfigStatus::Configured) => true,
                    Ok(_) => false,
                    Err(e) => {
                        self.state.flash_error(format!(".codex/config.toml: {e:#}"));
                        false
                    }
                }
            }
            _ => false,
        };
        if wrote {
            crate::state::dir_owners::claim(
                crate::state::dir_owners::Shared::McpEntry,
                cwd,
                std::process::id(),
            );
            if !self.runtime.mcp_config_dirs.iter().any(|d| d == cwd) {
                self.runtime.mcp_config_dirs.push(cwd.to_path_buf());
            }
        }
    }

    /// Teardown removes shared MCP entries, then releases separate per-agent
    /// hook leases. Only successful hook installation grants cleanup authority;
    /// borrowed reporters and other agents' hooks remain in place. Tracked
    /// files are preserved with a warning after the terminal is restored.
    pub fn cleanup_written_mcp_configs(&mut self) {
        use crate::state::dir_owners::{Shared, release};
        let me = std::process::id();
        for dir in std::mem::take(&mut self.runtime.mcp_config_dirs) {
            let mut tracked: Vec<std::path::PathBuf> = Vec::new();
            // Try each shape per dir; one an agent never used is a no-op.
            if release(Shared::McpEntry, &dir, me) {
                tracked.extend(
                    [
                        matches!(
                            crate::mcp::cleanup_mcp_json(&dir),
                            crate::mcp::ConfigCleanup::SkippedTracked
                        )
                        .then(|| dir.join(".mcp.json")),
                        matches!(
                            crate::mcp::cleanup_agy_mcp_config(&dir),
                            crate::mcp::ConfigCleanup::SkippedTracked
                        )
                        .then(|| dir.join(".agents").join("mcp_config.json")),
                        matches!(
                            crate::mcp::cleanup_codex_config(&dir),
                            crate::mcp::ConfigCleanup::SkippedTracked
                        )
                        .then(|| dir.join(".codex").join("config.toml")),
                    ]
                    .into_iter()
                    .flatten(),
                );
            }
            for tracked_path in tracked {
                eprintln!(
                    "spyc: left git-tracked MCP config in place: {} (remove the spyc entry by hand if unwanted)",
                    tracked_path.display()
                );
            }
        }
        for tracked_path in self.cleanup_written_status_hooks() {
            eprintln!(
                "spyc: left git-tracked status hooks in place: {}",
                tracked_path.display()
            );
        }
    }

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
            root: self.state.start_dir.clone(),
            cwd: cur.listing.dir.clone(),
            cursor_file,
            picks: cur.picks.iter().cloned().collect(),
            inventory: self.state.inventory.paths().cloned().collect(),
            filter: cur.temp_filter.clone(),
            git_branch: cur.git.info.clone(),
            project_home: self.state.project_home.clone(),
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

    /// Where the tab with `pane_id` runs, for an MCP connection bound to it:
    /// the tab's live cwd and that cwd's worktree root and branch, not the
    /// column the user is browsing. `None` when no live tab has the id.
    fn pane_context(&self, pane_id: &str) -> Option<serde_json::Value> {
        let tabs = self.runtime.pane_tabs.as_ref()?;
        let (index, entry) = tabs
            .tabs()
            .iter()
            .enumerate()
            .find(|(_, t)| t.info.id == pane_id)?;
        let cwd = entry.live_cwd();
        let worktree_root = super::state::find_repo_root(&cwd);
        let git_branch = worktree_root
            .as_deref()
            .and_then(crate::git::discovery::head_branch);
        Some(serde_json::json!({
            "id": pane_id,
            "tab": index + 1,
            "label": entry.info.label,
            "cwd": cwd,
            "worktree_root": worktree_root,
            "git_branch": git_branch,
        }))
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
        if self.view.last_context.as_ref() == Some(&ctx) {
            return;
        }
        // MVU Phase 3d: only advance the dedup cache when the write actually
        // landed. If the write fails, `last_context` stays behind disk, so a
        // later identical-state mutation still writes instead of dedup-skipping
        // into a stale file. (The 500ms cap used to mask this by re-running the
        // debounced writer; it's gone now.)
        if crate::context::write_context_file(&self.view.context_path, &ctx).is_ok() {
            // The sidecar is discovery's record of the root, so it moves when
            // the root does (a `spyc -r` restoring a project elsewhere, #523).
            let moved = self
                .view
                .last_context
                .as_ref()
                .is_none_or(|c| c.root != ctx.root);
            if moved && self.view.mcp_running {
                crate::mcp::record_root(&ctx.root);
            }
            self.view.last_context = Some(ctx);
        }
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

    /// Execute a writable MCP command from Claude. Runs on the main
    /// thread with full access to `AppState`. Returns a response that
    /// the MCP server thread forwards to Claude.
    pub fn execute_mcp_command(
        &mut self,
        cmd: crate::mcp_cmd::McpCommand,
    ) -> crate::mcp_cmd::McpResponse {
        use crate::mcp_cmd::{McpCommand, McpResponse};
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
            McpCommand::OpenWorktree { path } => {
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
                self.open_second_commander_at_background(&target);
                let opened = self.state.cur().listing.dir.clone();
                self.write_context();
                McpResponse::Ok {
                    message: format!("opened column b at {}", opened.display()),
                }
            }
            McpCommand::ReportStatus {
                pane_id,
                pane,
                status,
                ttl_ms,
                session_id,
                hook_event,
            } => {
                use crate::pane::{AgentActivity, ReportedStatus};
                let activity = match status.as_str() {
                    "working" | crate::agent::codex_recovery::QUESTION_END => {
                        AgentActivity::Working
                    }
                    "blocked" | crate::agent::codex_recovery::QUESTION_START => {
                        AgentActivity::Blocked
                    }
                    "idle" => AgentActivity::Idle,
                    "done" => AgentActivity::Done,
                    other => {
                        return McpResponse::Error {
                            message: format!("unknown status: {other}"),
                        };
                    }
                };
                let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
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
                let now = std::time::Instant::now();
                let ttl = std::time::Duration::from_millis(
                    ttl_ms
                        .unwrap_or(DEFAULT_REPORT_TTL_MS)
                        .min(crate::mcp_cmd::MAX_REPORT_TTL_MS),
                );
                let entry = &mut tabs.tabs_mut()[idx];
                if entry.pane.is_closed() {
                    return McpResponse::Error {
                        message: "pane has exited; activity report rejected".into(),
                    };
                }
                let kind = crate::agent::detect(&entry.info.command).kind();
                let question_signal = matches!(
                    status.as_str(),
                    crate::agent::codex_recovery::QUESTION_START
                        | crate::agent::codex_recovery::QUESTION_END
                );
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
                } else if question_signal {
                    Err("Codex question hook requires a Codex pane")
                } else {
                    Ok(activity)
                };
                let received = ReportedStatus {
                    status: activity,
                    at: now,
                    expiry: now + ttl,
                };
                entry.info.last_reported = Some(received);
                entry.info.last_report_ignored = decision.err();
                if decision.is_ok() {
                    entry.info.reported = Some(received);
                }
                if let Some(event) = hook_event {
                    let history = &mut entry.info.recent_hook_events;
                    if history.len() >= crate::agent::status_hook::HISTORY_LIMIT {
                        history.pop_front();
                    }
                    history.push_back((event, activity, now));
                }
                // Apply immediately so this frame reflects it; `settle_agent_activity`
                // maintains it (and falls back to timing once it expires).
                if decision.is_ok() {
                    entry.info.activity = activity;
                }
                // P1-3: a live session id piggybacked on the hook report supersedes
                // the spawn-proximity resolver at save time — route it to the id
                // field for this tab's agent. claude reports `session_id` and agy
                // `conversationId` (both land in `live_session_id`); codex has its
                // own spawn-ordered rollout claim.
                if let Some(sid) = session_id.filter(|_| decision.is_ok()) {
                    match kind {
                        crate::state::sessions::AgentKind::Claude
                        | crate::state::sessions::AgentKind::Agy => {
                            entry.info.live_session_id = Some(sid);
                        }
                        crate::state::sessions::AgentKind::Codex => {
                            entry.info.codex_session_id = Some(sid);
                        }
                        _ => {}
                    }
                }
                let label = entry.info.label.clone();
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
                let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
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
            McpCommand::PaneContext { pane_id } => match self.pane_context(&pane_id) {
                Some(pane) => McpResponse::Ok {
                    message: pane.to_string(),
                },
                None => McpResponse::Error {
                    message: format!("no pane with id {pane_id} (closed?)"),
                },
            },
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
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
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

#[cfg(test)]
mod tests {
    use super::resolve_report_target;
    use crate::app::App;

    /// The regression: a second spyc quitting used to delete the status hooks
    /// out from under the first one's live panes. Teardown removes them only
    /// once it is the last live owner of the dir.
    /// The agents' MCP entry is shared the same way: writing it claims the
    /// dir, so a sibling can't remove it from under our agents, our exit leaves
    /// it for a live sibling, and the last one out removes it.
    #[test]
    fn teardown_leaves_the_mcp_entry_a_live_sibling_still_needs() {
        use crate::state::dir_owners::{Shared, claim, release};
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        crate::state::with_state_root(&dir.join("state"), || {
            let mut app = App::test_app(dir.clone());
            app.view.mcp_running = true;
            app.ensure_agent_mcp_config("agy", &dir);
            let entry = dir.join(".agents").join("mcp_config.json");
            assert!(entry.exists(), "launching an agent writes it");

            claim(Shared::McpEntry, &dir, 1);
            assert!(
                !release(Shared::McpEntry, &dir, 1),
                "our claim keeps a sibling's exit from removing it"
            );
            claim(Shared::McpEntry, &dir, 1);
            app.cleanup_written_mcp_configs();
            assert!(entry.exists(), "a live sibling's agents still need it");

            assert!(release(Shared::McpEntry, &dir, 1));
            app.runtime.mcp_config_dirs.push(dir.clone());
            app.cleanup_written_mcp_configs();
            assert!(!entry.exists(), "the last one out removes it");
        });
    }

    #[test]
    fn teardown_leaves_the_hooks_a_live_sibling_still_needs() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        crate::state::with_state_root(&dir.join("state"), || {
            let mut app = App::test_app(dir.clone());
            crate::state::hook_consent::set_consent(&dir, true);
            app.install_status_hooks(&dir, crate::state::sessions::AgentKind::Claude);
            let settings = dir.join(".claude").join("settings.json");
            assert!(settings.exists(), "install must write them");

            // A sibling spyc claims the same dir (pid 1 is always live).
            crate::state::dir_owners::claim(crate::state::dir_owners::Shared::StatusHooks, &dir, 1);
            app.cleanup_written_mcp_configs();
            assert!(
                settings.exists(),
                "a live sibling's hooks must survive our exit"
            );

            // Sibling gone: the next instance out is free to clean up.
            assert!(crate::state::dir_owners::release(
                crate::state::dir_owners::Shared::StatusHooks,
                &dir,
                1
            ));
            app.install_status_hooks(&dir, crate::state::sessions::AgentKind::Claude);
            app.cleanup_written_mcp_configs();
            assert!(!settings.exists(), "the last one out must clean up");
        });
    }

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
