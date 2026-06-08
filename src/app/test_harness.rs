//! Shared test harness: `App::test_app` / `seed_rows` / `flash_text`, used
//! by unit tests across the app child modules. Relocated from mod.rs (800-LoC
//! campaign).

use super::*;

impl App {
    /// Test-only `App` constructor for workflow-harness tests
    /// (`docs/TEST_IMPROVEMENT_PLAN.md` Phase 1). Builds a deterministic
    /// `App` with **no** terminal, **no** MCP socket server, **no**
    /// git-status worker thread, and **no** real-env cwd — unlike
    /// `App::new`. Drive it with `apply(&Action)` / `handle_key(KeyEvent)`
    /// and assert on `self.state.*`, `self.runtime.pane_tabs`, `self.view.pager`, etc.
    ///
    /// Wrap callers in `crate::state::with_state_root(tmp, || …)` so the
    /// history / pager-position / inventory state dir is an isolated temp.
    pub(crate) fn test_app(cwd: std::path::PathBuf) -> Self {
        // No MCP server / git worker is spawned. The harness never drives
        // `run()`'s drain loop, and `apply` / `handle_key` don't read these
        // receivers, so both `mcp_cmd_rx` and `git_result_rx` are `None`
        // (Phase 3a/3d: `run()` is the only `.take()` site).
        let context_path = crate::context::context_path(&cwd);
        let mut app = Self {
            state: state::AppState::test_default(cwd),
            view: ViewState::new(Theme::default(), context_path, false, false),
            exit_summary: None,
            runtime: Runtime {
                git_result_rx: None,
                git_worker_tx: None,
                mcp_cmd_rx: None,
                pane_wake_tx: None,
                next_sink_id: 0,
                pane_tabs: None,
                top_overlay: None,
                pending_capture: None,
                background_tasks: BackgroundTasks::new(),
                find_picker: None,
                pager_stream: None,
                next_stream_id: 1,
                stashed_pager_streams: std::collections::HashMap::new(),
                agent_status_pending: std::sync::Arc::new(std::sync::Mutex::new(None)),
                agent_status_refreshing: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                    false,
                )),
            },
        };
        app.state.rebuild_rows();
        app
    }

    /// Seed the listing with fake file rows (no real fs), mirroring the
    /// `state_with_rows` pattern; clamps the cursor into range.
    pub(crate) fn seed_rows(&mut self, names: &[&str]) {
        let dir = self.state.listing.dir.clone();
        self.state.rows = names
            .iter()
            .map(|n| RowData {
                path: dir.join(n),
                display: (*n).to_string(),
                kind: EntryKind::File,
            })
            .collect();
        self.state.cursor.clamp(self.state.rows.len());
    }

    /// Flash message text, if any — compact assertion helper.
    pub(crate) fn flash_text(&self) -> Option<&str> {
        self.state.flash.as_ref().map(|f| f.text.as_str())
    }
}
