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

    /// Run `f` inside project `id`, as an MCP call from its agent runs, then
    /// put the active project back: how a test sets up and inspects a project
    /// the keyboard isn't in (#99).
    pub(crate) fn as_project<R>(
        &mut self,
        id: state::ProjectId,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.in_project(id, f)
    }

    /// Make project `id` the one the user sees and drives, as a switch does.
    /// Unlike [`Self::as_project`] it stays, and nothing is hidden from the
    /// user: no `in_background`, and the flash is theirs.
    pub(crate) fn make_active(&mut self, id: state::ProjectId) {
        assert!(
            self.state.has_project(id),
            "make_active is handed an open project"
        );
        self.state.active = id;
        self.runtime.active = id;
        self.view.active = id;
    }

    /// File an archive worker's outcome as the `Effect::Archive` worker does,
    /// for the drain (`apply_archive_outcomes`) to apply. Tests run the op
    /// inline (`run_archive_op`) and hand its outcome here.
    pub(crate) fn file_archive_outcome(&self, outcome: super::archive_ops::ArchiveOutcome) {
        self.runtime.archive_results.lock().unwrap().push(outcome);
    }

    /// Ready project `id` for MCP: `home` is its home, its column lists `home`
    /// for real, and its context file is written.
    pub(crate) fn ready_test_project(&mut self, id: state::ProjectId, home: &std::path::Path) {
        self.as_project(id, |app| {
            let listing = crate::fs::listing::Listing::read(home).expect("home lists");
            let project = app.state.active_project_mut();
            project.home = Some(home.to_path_buf());
            project.left.listing = listing;
            app.state.rebuild_rows();
            app.write_context();
        });
    }

    /// A second project homed at `home`, beside the active one, readied by
    /// [`Self::ready_test_project`] with its context file in `home`. The
    /// active project stays active.
    pub(crate) fn add_test_project(&mut self, home: &std::path::Path) -> state::ProjectId {
        let project = state::AppState::test_default(home.to_path_buf())
            .projects
            .remove(0);
        let id = project.id;
        self.state.projects.push(project);
        self.runtime
            .projects
            .insert(id, super::runtime::ProjectRuntime::default());
        self.view.projects.insert(
            id,
            super::view_state::ProjectView::new(crate::context::context_path(home), false),
        );
        self.ready_test_project(id, home);
        id
    }

    /// Open a `cat` tab in project `id`, as that project's keyboard would, and
    /// return the tab's pane id.
    pub(crate) fn open_tab_in_project(
        &mut self,
        id: state::ProjectId,
        cwd: &std::path::Path,
    ) -> String {
        self.as_project(id, |app| {
            assert!(app.open_pane_tab_in("cat", cwd), "the tab opens");
            let tabs = app.runtime.project().pane_tabs.as_ref().expect("a tab");
            tabs.tabs().last().expect("a tab").info.id.clone()
        })
    }
}
