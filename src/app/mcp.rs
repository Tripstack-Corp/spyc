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
/// gives no `ttl_ms`: long enough that a genuinely-blocked agent's dot persists
/// until answered, short enough that a crashed agent's stale `working`/`blocked`
/// eventually falls back to output timing. Overridable per-report.
const DEFAULT_REPORT_TTL_MS: u64 = 300_000;

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
        let takeover = self.view.mcp_takeover_allowed;
        // True only when *we* wrote our own entry (Configured / TookOver) — then
        // we record the dir so teardown removes our (now-dead-socket) entry. A
        // skipped takeover, enterprise block, or managed env leaves nothing of
        // ours to clean, so those don't record.
        let wrote_ours = match crate::agent::detect(cmd).kind() {
            crate::state::sessions::AgentKind::Claude => {
                match crate::mcp::ensure_mcp_json(cwd, takeover) {
                    Ok(crate::mcp::McpConfigStatus::Configured) => true,
                    Ok(crate::mcp::McpConfigStatus::TookOver { old_pid }) => {
                        self.state
                            .flash_info(format!("MCP: took over from PID {old_pid}"));
                        true
                    }
                    Ok(crate::mcp::McpConfigStatus::SkippedTakeover { old_pid }) => {
                        self.state.flash_info(format!(
                            "MCP: kept PID {old_pid} as owner (Claude here will talk to it)"
                        ));
                        false
                    }
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
                        self.state.flash_error(format!(".mcp.json: {e}"));
                        false
                    }
                }
            }
            // Codex equivalent: both agents share the same socket; the writer
            // just registers a stdio entry that re-execs `spyc --mcp` to proxy.
            // Enterprise-flavored statuses are claude-specific; codex shouldn't
            // return them, but if it ever does we treat them as a no-op.
            crate::state::sessions::AgentKind::Codex => {
                match crate::mcp::ensure_codex_config_toml(cwd, takeover) {
                    Ok(crate::mcp::McpConfigStatus::TookOver { old_pid }) => {
                        self.state
                            .flash_info(format!("codex MCP: took over from PID {old_pid}"));
                        true
                    }
                    Ok(crate::mcp::McpConfigStatus::SkippedTakeover { old_pid }) => {
                        self.state.flash_info(format!(
                            "codex MCP: kept PID {old_pid} as owner (codex here will talk to it)"
                        ));
                        false
                    }
                    Ok(crate::mcp::McpConfigStatus::Configured) => true,
                    Ok(_) => false,
                    Err(e) => {
                        self.state.flash_error(format!(".codex/config.toml: {e}"));
                        false
                    }
                }
            }
            _ => false,
        };
        if wrote_ours && !self.runtime.mcp_config_dirs.iter().any(|d| d == cwd) {
            self.runtime.mcp_config_dirs.push(cwd.to_path_buf());
        }
    }

    /// Teardown: remove the MCP client config entries *we* wrote (a `.mcp.json`
    /// / `.codex/config.toml` pointing at our now-dead socket) from every dir we
    /// launched an agent in, deleting a file/`.codex` dir left empty. A
    /// git-tracked config is left in place with a stderr warning — we never
    /// dirty or delete something the user committed. Best-effort; called from
    /// `run_teardown` after the terminal is restored, so warnings are visible.
    pub fn cleanup_written_mcp_configs(&mut self) {
        for dir in std::mem::take(&mut self.runtime.mcp_config_dirs) {
            // Try each shape per dir; each is a no-op unless it finds an entry
            // that's *ours*, so attempting the wrong one is harmless. The Claude
            // status hooks (`.claude/settings.json`) ride the same dir set.
            for tracked_path in [
                matches!(
                    crate::mcp::cleanup_mcp_json(&dir),
                    crate::mcp::ConfigCleanup::SkippedTracked
                )
                .then(|| dir.join(".mcp.json")),
                matches!(
                    crate::mcp::cleanup_codex_config(&dir),
                    crate::mcp::ConfigCleanup::SkippedTracked
                )
                .then(|| dir.join(".codex").join("config.toml")),
                matches!(
                    crate::mcp::cleanup_claude_status_hooks(&dir),
                    crate::mcp::ConfigCleanup::SkippedTracked
                )
                .then(|| dir.join(".claude").join("settings.json")),
            ]
            .into_iter()
            .flatten()
            {
                eprintln!(
                    "spyc: left git-tracked MCP config in place: {} (remove the spyc entry by hand if unwanted)",
                    tracked_path.display()
                );
            }
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
        if self.view.last_context.as_ref() == Some(&ctx) {
            return;
        }
        // MVU Phase 3d: only advance the dedup cache when the write actually
        // landed. If the write fails, `last_context` stays behind disk, so a
        // later identical-state mutation still writes instead of dedup-skipping
        // into a stale file. (The 500ms cap used to mask this by re-running the
        // debounced writer; it's gone now.)
        if crate::context::write_context_file(&self.view.context_path, &ctx).is_ok() {
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
            self.state.cur().listing.dir.join(&raw)
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
                    self.state.cur().listing.dir.join(&raw)
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
                // there while `a` stays put.
                self.open_second_commander_at(&target);
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
            } => {
                use crate::pane::{AgentActivity, ReportedStatus};
                let activity = match status.as_str() {
                    "working" => AgentActivity::Working,
                    "blocked" => AgentActivity::Blocked,
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
                let ttl = std::time::Duration::from_millis(ttl_ms.unwrap_or(DEFAULT_REPORT_TTL_MS));
                let entry = &mut tabs.tabs_mut()[idx];
                entry.info.reported = Some(ReportedStatus {
                    status: activity,
                    at: now,
                    expiry: now + ttl,
                });
                // Apply immediately so this frame reflects it; `settle_agent_activity`
                // maintains it (and falls back to timing once it expires).
                entry.info.activity = activity;
                let label = entry.info.label.clone();
                McpResponse::Ok {
                    message: format!("status '{status}' set for pane {} ({label})", idx + 1),
                }
            }
            McpCommand::Disconnected { new_pid } => {
                self.view.mcp_running = false;
                self.state.flash_error(format!(
                    "MCP taken over by spyc PID {new_pid} — Claude is connected to that instance"
                ));
                McpResponse::Ok {
                    message: "acknowledged".into(),
                }
            }
            McpCommand::ToolCalled { name } => {
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
