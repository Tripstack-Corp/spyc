//! Shared test harness: `App::test_app` / `seed_rows` / `flash_text`, used
//! by unit tests across the app child modules. Relocated from mod.rs (800-LoC
//! campaign).

use super::*;

impl App {
    /// Test-only `App` constructor for workflow-harness tests
    /// (`docs/archive/TESTING_STRATEGY.md` Phase 1). Builds a deterministic
    /// `App` with **no** terminal, **no** MCP socket server, **no**
    /// git-status worker thread, and **no** real-env cwd — unlike
    /// `App::new`. Drive it with `apply(&Action)` / `handle_key(KeyEvent)`
    /// and assert on `self.state.*`, `self.runtime.project().pane_tabs`,
    /// `self.view.project().pager`, etc.
    ///
    /// Wrap callers in `crate::state::with_state_root(tmp, || …)` so the
    /// history / pager-position / inventory state dir is an isolated temp.
    pub(crate) fn test_app(cwd: std::path::PathBuf) -> Self {
        // No MCP server / git worker is spawned. The harness never drives
        // `run()`'s drain loop, and `apply` / `handle_key` don't read these
        // receivers, so both `mcp_cmd_rx` and `git_result_rx` are `None`
        // (Phase 3a/3d: `run()` is the only `.take()` site).
        let context_path = crate::context::context_path(&cwd);
        let state = state::AppState::test_default(cwd);
        let active = state.active;
        let mut app = Self {
            state,
            view: ViewState::new(
                Theme::default(),
                context_path,
                false,
                false,
                crate::ui::color_depth::ColorDepth::TrueColor,
                active,
            ),
            exit_summary: None,
            // `next_stream_id` starts at 1 here (0 in `App::new`) — a
            // pre-existing distinction from before this constructor existed;
            // preserved via the functional-update override rather than
            // guessed away.
            runtime: Runtime {
                next_stream_id: 1,
                ..Runtime::new(None, None, None, active)
            },
        };
        app.state.rebuild_rows();
        app
    }

    /// Seed the listing with fake file rows (no real fs), mirroring the
    /// `state_with_rows` pattern; clamps the cursor into range.
    pub(crate) fn seed_rows(&mut self, names: &[&str]) {
        let dir = self.state.active_project().left.listing.dir.clone();
        let p = self.state.active_project_mut();
        p.left.rows = names
            .iter()
            .map(|n| RowData {
                path: dir.join(n),
                display: (*n).to_string(),
                kind: EntryKind::File,
                deleted: false,
            })
            .collect();
        let len = p.left.rows.len();
        p.left.cursor.clamp(len);
    }

    /// Flash message text, if any — compact assertion helper.
    pub(crate) fn flash_text(&self) -> Option<&str> {
        self.state.flash.as_ref().map(|f| f.text.as_str())
    }
}
