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
    /// tagged with the active project, for the drain (`apply_archive_outcomes`)
    /// to apply. Tests run the op inline (`run_archive_op`) and hand its
    /// outcome here.
    pub(crate) fn file_archive_outcome(&self, outcome: super::archive_ops::ArchiveOutcome) {
        let origin = self.state.active;
        self.runtime.archive_results.land((origin, outcome));
    }

    /// File a graveyard worker's outcome as `spawn_graveyard_op` does, for the
    /// drain (`apply_graveyard_outcomes`) to apply. Tests run the op inline
    /// (`run_graveyard_op`), on the thread that holds their state root, and
    /// hand its outcome here.
    pub(crate) fn file_graveyard_outcome(&self, outcome: super::graveyard_ops::GraveyardOutcome) {
        self.runtime.graveyard_results.land(outcome);
    }

    /// File an inventory worker's outcome as `spawn_inventory_op` does, for the
    /// drain (`apply_inventory_outcomes`) to apply. Tests run the op inline
    /// (`run_inventory_op`) and hand its outcome here.
    pub(crate) fn file_inventory_outcome(&self, outcome: super::inventory_ops::InventoryOutcome) {
        self.runtime.inventory_results.land(outcome);
    }

    /// File an image worker's outcome as `spawn_image_work` does, tagged with
    /// the active project, for the drain (`apply_image_outcomes`) to apply.
    /// Tests run the worker inline and hand its outcome here.
    pub(crate) fn file_image_outcome(&self, outcome: super::image_ops::ImageOutcome) {
        let origin = self.state.active;
        self.runtime.image_results.land((origin, outcome));
    }

    /// Wire the background git worker as `App::new` does once its thread
    /// spawns, so a cache miss enqueues a walk instead of running it inline.
    pub(crate) fn wire_git_worker(&mut self) {
        self.state.git_worker_available = true;
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
    /// [`Self::ready_test_project`], with its context file where production
    /// keeps one (`process_context_path`). The active project stays active.
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
            super::view_state::ProjectView::new(
                crate::context::process_context_path(home, id),
                false,
            ),
        );
        self.ready_test_project(id, home);
        id
    }

    /// Close project `id` as `Space x` does once the user is out of it
    /// (`drop_project`). It must not be the active project; tests switch away
    /// first, as closing does.
    pub(crate) fn remove_test_project(&mut self, id: state::ProjectId) {
        self.drop_project(id);
    }

    /// Wait, at most five seconds, for `done`, landing the switcher's closed
    /// rows, read off the loop thread, as the loop does.
    pub(crate) fn land_closed_rows(&mut self, done: impl Fn(&Self) -> bool) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !done(self) {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            self.apply_closed_rows();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        true
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
