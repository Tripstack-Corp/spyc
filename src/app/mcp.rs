//! MCP / context integration: write the `.mcp.json` (+ codex) config on
//! startup, snapshot the current state into the on-disk context file MCP
//! clients read, and execute writable MCP commands on the main thread.
//! Extracted verbatim from `app/mod.rs` (the impl-extraction sweep), same
//! child-module `impl App` pattern. `ensure_mcp_config` / `refresh_process_stats`
//! / `write_context` / `execute_mcp_command` are `pub` (called from `new` /
//! `loop_steps`); `snapshot_context` is internal to this module.

use super::App;

impl App {
    /// Write `.mcp.json` with stdio transport on startup.
    /// If enterprise policy blocks spyc, flash an error instead.
    pub fn ensure_mcp_config(&mut self, takeover_allowed: bool) {
        match crate::mcp::ensure_mcp_json(&self.state.listing.dir, takeover_allowed) {
            Ok(crate::mcp::McpConfigStatus::Configured) => {}
            Ok(crate::mcp::McpConfigStatus::TookOver { old_pid }) => {
                self.state
                    .flash_info(format!("MCP: took over from PID {old_pid}"));
            }
            Ok(crate::mcp::McpConfigStatus::SkippedTakeover { old_pid }) => {
                self.state.flash_info(format!(
                    "MCP: kept PID {old_pid} as owner (Claude here will talk to it)"
                ));
            }
            Ok(crate::mcp::McpConfigStatus::BlockedByEnterprise) => {
                self.state.flash_error(
                    "MCP: blocked by enterprise policy (deniedMcpServers or allowedMcpServers)",
                );
            }
            Ok(crate::mcp::McpConfigStatus::ManagedByEnterprise) => {
                self.state
                    .flash_info("MCP: enterprise-managed (skipped local .mcp.json)");
            }
            Err(e) => self.state.flash_error(format!(".mcp.json: {e}")),
        }

        // Codex equivalent: write `.codex/config.toml` so the codex CLI
        // discovers spyc's MCP server the same way claude does. Both
        // agents share the same socket; the writer just registers a
        // stdio entry that re-execs `spyc --mcp` to proxy. Failures
        // here flash but don't gate startup — codex isn't required.
        // Enterprise-flavored statuses are claude-specific; codex
        // shouldn't return them, but if it ever does we treat them as
        // a no-op.
        match crate::mcp::ensure_codex_config_toml(&self.state.listing.dir, takeover_allowed) {
            Ok(crate::mcp::McpConfigStatus::TookOver { old_pid }) => {
                self.state
                    .flash_info(format!("codex MCP: took over from PID {old_pid}"));
            }
            Ok(crate::mcp::McpConfigStatus::SkippedTakeover { old_pid }) => {
                self.state.flash_info(format!(
                    "codex MCP: kept PID {old_pid} as owner (codex here will talk to it)"
                ));
            }
            Ok(_) => {}
            Err(e) => self.state.flash_error(format!(".codex/config.toml: {e}")),
        }
    }

    /// Refresh `activity_proc_rss_kb` / `activity_proc_threads`. Called once
    /// per A-monitor 1 s tick. `proc_rss_threads` reads the OS directly
    /// (sysinfo for rss + libproc for the macOS thread count) — a fast
    /// syscall, not a `ps` fork-exec, so it runs inline (the off-thread
    /// machinery #227 added for the slow `ps` spawn is no longer needed).
    pub fn refresh_process_stats(&mut self) {
        if let Some((rss, threads)) = crate::sysinfo::proc_rss_threads() {
            self.view.activity_proc_rss_kb = rss;
            self.view.activity_proc_threads = threads;
        }
    }

    /// Build a context snapshot from the current state for MCP consumers.
    fn snapshot_context(&self) -> crate::context::SpycContext {
        let cursor_file = self
            .state
            .rows
            .get(self.state.cursor.index)
            .map(|r| r.display.clone());
        crate::context::SpycContext {
            cwd: self.state.listing.dir.clone(),
            cursor_file,
            picks: self.state.picks.iter().cloned().collect(),
            inventory: self.state.inventory.paths().cloned().collect(),
            filter: self.state.temp_filter.clone(),
            git_branch: self.state.git.info.clone(),
            project_home: self.state.project_home.clone(),
            session_name: self.state.session_name.clone().unwrap_or_default(),
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

    /// Execute a writable MCP command from Claude. Runs on the main
    /// thread with full access to `AppState`. Returns a response that
    /// the MCP server thread forwards to Claude.
    pub fn execute_mcp_command(
        &mut self,
        cmd: crate::mcp_cmd::McpCommand,
    ) -> crate::mcp_cmd::McpResponse {
        use crate::mcp_cmd::{McpCommand, McpResponse};
        match cmd {
            McpCommand::NavigateTo { path } => {
                match self.state.jump_to(&path) {
                    Ok(()) => {
                        self.state.flash_info(format!(
                            "[mcp] navigated to {}",
                            self.state.listing.dir.display()
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
                    Some(ref p) if p.is_empty() => self.state.temp_filter = None,
                    Some(p) => self.state.temp_filter = Some(p),
                    None => self.state.temp_filter = None,
                }
                self.state.rebuild_rows();
                let count = self.state.rows.len();
                let label = self.state.temp_filter.as_deref().unwrap_or("(cleared)");
                self.state.flash_info(format!("[mcp] filter: {label}"));
                self.write_context();
                McpResponse::Ok {
                    message: format!("filter applied, {count} items visible"),
                }
            }
            McpCommand::PickFiles { patterns } => {
                let mut total = 0usize;
                let mut errors = Vec::new();
                for pat_str in &patterns {
                    match glob::Pattern::new(pat_str) {
                        Ok(pat) => {
                            for e in &self.state.listing.entries {
                                if pat.matches(&e.name) {
                                    self.state.picks.insert(&e.path);
                                    total += 1;
                                }
                            }
                        }
                        Err(e) => errors.push(format!("{pat_str}: {e}")),
                    }
                }
                self.state.list_generation = self.state.list_generation.wrapping_add(1);
                if !errors.is_empty() {
                    return McpResponse::Error {
                        message: format!("invalid patterns: {}", errors.join(", ")),
                    };
                }
                self.state
                    .flash_info(format!("[mcp] picked {total} file(s)"));
                self.write_context();
                McpResponse::Ok {
                    message: format!("picked {total} file(s), {} total", self.state.picks.len()),
                }
            }
            McpCommand::ClearPicks => {
                let count = self.state.picks.len();
                self.state.picks.clear();
                self.state.list_generation = self.state.list_generation.wrapping_add(1);
                self.state.flash_info("[mcp] picks cleared");
                self.write_context();
                McpResponse::Ok {
                    message: format!("cleared {count} pick(s)"),
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
        }
    }
}
