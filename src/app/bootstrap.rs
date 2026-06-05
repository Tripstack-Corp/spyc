//! `App::new` — the constructor/bootstrap: reads config + args, builds the
//! initial `Listing` / `Runtime` / `ViewState`, wires the resolver and session
//! restore, and arms MCP. Moved verbatim from `app/mod.rs` (800-LoC campaign).

use std::path::PathBuf;

use crate::config::Config;
use crate::fs::Listing;
use crate::keymap::{Resolver, UserKeymap};
use crate::spyc_debug;
use crate::state::{Cursor, Harpoon, History, IgnoreMasks, Inventory, Marks, Picks};
use crate::ui::theme::Theme;

use super::{
    App, BackgroundTasks, FlashKind, FlashMessage, Mode, Runtime, View, ViewState, state,
    user_host_string,
};

impl App {
    pub fn new(resume: bool, mcp_takeover_allowed: bool) -> Self {
        let (cwd, start_error) = if let Ok(d) = std::env::current_dir() {
            (d, None)
        } else {
            // cwd not accessible — fall back to $HOME.
            let home = std::env::var("HOME").map_or_else(|_| PathBuf::from("/tmp"), PathBuf::from);
            let _ = std::env::set_current_dir(&home);
            (
                home,
                Some("cwd not accessible, started in $HOME".to_string()),
            )
        };
        let (listing, start_error) = match Listing::read(&cwd) {
            Ok(l) => (l, start_error),
            Err(e) => (
                Listing::empty(cwd.clone()),
                Some(start_error.unwrap_or_default() + &format!("{e}")),
            ),
        };
        // Defer the initial git-status read to the background worker
        // (kicked off after AppState is built, below). Previously
        // these two `git status` spawns blocked the first paint by
        // 200-500 ms on a ~110k-file repo. Cache-miss handling in
        // the chdir / event-loop path will populate `git_info` and
        // `git_files` once the worker reports back.
        let git_info: Option<String> = None;
        let git_files: std::collections::HashMap<String, crate::ui::list_view::GitFileStatus> =
            std::collections::HashMap::new();
        let (config, load_note) = match Config::load_default(&cwd) {
            Ok(c) => {
                let note = if c.sources.is_empty() {
                    None
                } else {
                    Some(format!("loaded {} config file(s)", c.sources.len()))
                };
                (c, note)
            }
            Err(e) => (Config::default(), Some(format!("config error: {e}"))),
        };
        let user_keymap = UserKeymap::from_bindings(config.bindings.clone());
        let theme = Theme::default().with_overrides(&config.colors);

        // Always anchor PROJECT_HOME on the launch dir. Previously
        // this was gated on `cwd.join(".git").exists()`, which meant
        // launching spyc one level above the actual repo (e.g. from
        // `~/src/workspace` containing a Java monorepo at
        // `~/src/workspace/inner-repo`) left `project_home` None —
        // and downstream code (session save, harpoon, MCP context)
        // had no project anchor at all. Honoring the launch dir
        // gives every spyc invocation a project anchor; users who
        // want a different anchor can override with `:project <path>`
        // or `gP`. Cleared with `:project clear`.
        let project_home = Some(cwd.clone());
        let session_name = Some(crate::state::session_names::generate());

        // Load the harpoon list for the active project (if any). When
        // `PROJECT_HOME` is unset, harpoon stays `None` and all H-prefix
        // bindings flash a hint. Loaded once at startup; reloaded on
        // chdir into a different `PROJECT_HOME`.
        let harpoon = project_home.as_ref().map(|p| Harpoon::load(p));

        // Run health check before loading state — cleans up orphaned
        // files so Inventory::load() et al. see a consistent directory.
        let health_warnings = if let Some(sd) = crate::state::health::state_dir() {
            let report = crate::state::health::check(&sd);
            if report.cleaned > 0 {
                spyc_debug!("health check: cleaned {} orphaned file(s)", report.cleaned);
            }
            report.warnings
        } else {
            Vec::new()
        };

        let app_state = state::AppState {
            listing,
            picks: Picks::new(),
            inventory: Inventory::load(),
            marks: Marks::load(),
            masks: {
                let mut m = IgnoreMasks::default();
                m.apply_config(&config.ignore_masks);
                m
            },
            temp_filter: None,
            sort_order: crate::fs::listing::SortMode::Name,
            sort_reversed: false,
            view: View::Dir,
            cursor: Cursor::new(),
            resolver: Resolver::new(),
            user_keymap,
            config,
            mode: Mode::Normal,
            project_home,
            session_name,
            frecency: crate::state::Frecency::load(),
            focus: state::Focus::FileList,
            // spyc (top) = 30%, pane (bottom) = 70%. Resize with `^W +/-`.
            pane_height_pct: 70,
            pane_zoomed: false,
            pane_focus_before_zoom: None,
            pane_hidden: false,
            harpoon_filter_set: harpoon
                .as_ref()
                .map(|h| h.ancestor_set().clone())
                .unwrap_or_default(),
            // MVU Phase 5: domain fields relocated from `App`. Note
            // `harpoon` is moved (consumed) AFTER `harpoon_filter_set`
            // borrowed it just above.
            harpoon,
            pane_prompt_buf: String::new(),
            last_pane_prompt: None,
            pane_snapshot: state::PaneSnapshot::default(),
            pending_delete_preview: None,
            // Populated on the first successful `refresh_git_state`
            // call. See `AppState::git_poll_cache` doc for why this
            // starts None.
            git_poll_cache: None,
            // The very first chdir of App::run will set both based
            // on the actual tree size. Bootstrap defaults are fine —
            // the small-tree cadence is conservative until proven
            // huge.
            is_huge_tree: false,
            huge_tree_anchor: None,
            huge_tree_decisions: std::collections::HashMap::new(),
            current_repo_root: None,
            current_gitdir: None,
            git_status_raw_cache: None,
            git_worker_available: false,
            pending_git_requests: Vec::new(),
            git_generation: 0,
            last_git_invalidation: None,
            last_git_request_at: None,
            graveyard: Vec::new(),
            pending_new_tab_cmd: None,
            last_captured_cmd: None,
            pending_worktrees: None,
            pending_sessions: None,
            start_dir: cwd,
            prev_dir: None,
            last_search: None,
            quit_pending: None,
            history: History::load(),
            pane_history: History::load_file("pane_history"),
            pane_cwd_history: History::load_file("pane_cwd_history"),
            jump_history: History::load_file("jump_history"),
            command_history: History::load_file("command_history"),
            flash: start_error.map(|text| FlashMessage {
                text,
                kind: FlashKind::Error,
            }),
            user_host: user_host_string(),
            git: state::GitState {
                info: git_info,
                files: git_files,
            },
            should_quit: false,
            rows: Vec::new(),
            grid_dims: crate::ui::list_view::GridDims {
                cols: 1,
                rows_per_col: 1,
            },
            list_generation: 0,
        };
        let context_path = crate::context::context_path(&app_state.start_dir);
        // Command channel for writable MCP actions (Claude → main loop).
        let (mcp_cmd_tx, mcp_cmd_rx) = std::sync::mpsc::channel();
        // Start the MCP Unix socket server so `spyc --mcp` (spawned by
        // Claude Code) can proxy to us for full read/write MCP access.
        let mcp_running = crate::mcp::start_socket_server(context_path.clone(), mcp_cmd_tx)
            .map_or_else(
                |e| {
                    spyc_debug!("MCP socket server failed to start: {e}");
                    false
                },
                |()| true,
            );
        // Background git-status worker. Owns the spawn of
        // `git status --porcelain` on cache miss so the chdir UI
        // returns immediately. Lives for the lifetime of the
        // process; the OS reaps the thread on exit. Both channel ends
        // live on the Runtime (`runtime.git_worker_tx` sender,
        // `runtime.git_result_rx` receiver); the Model holds no channel —
        // it records desired requests in `state.pending_git_requests`,
        // which the run loop flushes to the worker via `flush_git_requests`.
        // See `state::GitWorkerRequest` / `state::GitWorkerResult`.
        let (git_req_tx, git_req_rx) = std::sync::mpsc::channel::<state::GitWorkerRequest>();
        let (git_res_tx, git_res_rx) = std::sync::mpsc::channel::<state::GitWorkerResult>();
        std::thread::spawn(move || {
            while let Ok(req) = git_req_rx.recv() {
                // Stat the cache-key mtimes BEFORE reading status. An
                // index write racing this read then lands in the *next*
                // poll's diff: an older key paired with newer status is
                // safe (forces one redundant refresh), whereas the
                // reverse order — newer key, older status — would make
                // the 1 Hz poll short-circuit on a stale snapshot
                // forever, hiding staged/working changes until an
                // unrelated later write moved the mtime.
                let (index_mtime, head_mtime) = crate::git::discovery::gitdir(&req.repo_root)
                    .map_or((None, None), |gd| {
                        let i = std::fs::metadata(gd.join("index"))
                            .and_then(|m| m.modified())
                            .ok();
                        let h = std::fs::metadata(gd.join("HEAD"))
                            .and_then(|m| m.modified())
                            .ok();
                        (i, h)
                    });
                let raw = crate::git::status::porcelain_raw(&req.canonical);
                let _ = git_res_tx.send(state::GitWorkerResult {
                    generation: req.generation,
                    repo_root: req.repo_root,
                    raw,
                    index_mtime,
                    head_mtime,
                });
            }
        });
        let mut app_state = app_state;
        app_state.git_worker_available = true;
        let mut app = Self {
            state: app_state,
            // Write context once on startup so claude sees initial state
            // (context_dirty: true).
            view: ViewState::new(theme, context_path, true, mcp_running),
            exit_summary: None,
            runtime: Runtime {
                git_result_rx: Some(git_res_rx),
                git_worker_tx: Some(git_req_tx),
                mcp_cmd_rx: Some(mcp_cmd_rx),
                pane_wake_tx: None,
                next_sink_id: 0,
                pane_tabs: None,
                top_overlay: None,
                pending_capture: None,
                background_tasks: BackgroundTasks::new(),
                find_picker: None,
                grep_session: None,
                next_grep_id: 0,
                agent_status_pending: std::sync::Arc::new(std::sync::Mutex::new(None)),
                agent_status_refreshing: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                    false,
                )),
            },
        };
        app.state.rebuild_rows();
        // Evaluate huge-tree status at startup so the first 1 Hz poll
        // / first event-driven refresh uses the right cadence and
        // `git status` flag. Without this, spyc launched directly
        // in a 110k-file project root would run small-tree cadence
        // until the user navigated somewhere.
        let initial_cwd = app.state.listing.dir.clone();
        app.state.update_huge_tree(&initial_cwd);
        // Now that the worker is wired and we know is_huge_tree,
        // kick off the first git read in the background. The branch
        // string is computed sync from `.git/HEAD` so it's available
        // on the first paint; only the per-file markers and dirty
        // flag wait for the worker.
        app.state.git.info = app.state.compute_git_info_fast();
        let _ = app.state.git_file_statuses_cached(&initial_cwd);
        // The bootstrap cache-miss queued a request into the Model's
        // outbox (git_worker_available is now true); flush it onto the
        // worker channel so the first per-file markers land as early as
        // they did when the send was inline.
        app.flush_git_requests();
        if let Some(msg) = load_note {
            app.state.flash_info(msg);
        }
        // Surface any health check warnings so the user knows state
        // was repaired. Overrides the config load note if both exist.
        if !health_warnings.is_empty() {
            app.state.flash_error(health_warnings.join("; "));
        }
        // Graveyard cascade: if total exceeds the cap, push the
        // oldest entries to the system trash (FIFO) until under
        // the cap. Best-effort and silent on failure (the user
        // would see a flash from any visible-error path; failures
        // here are uncommon disk/permissions issues that don't
        // need to interrupt startup).
        let cap = crate::state::graveyard::GRAVEYARD_CAP_BYTES;
        if crate::state::graveyard::Graveyard::load().total_bytes() > cap {
            let (trashed, _errors) = crate::state::graveyard::Graveyard::cascade_until_under(cap);
            if trashed > 0 {
                app.state.flash_info(format!(
                    "graveyard: {trashed} item(s) moved to system trash (cap reached)"
                ));
            }
        }

        if resume {
            app.show_session_picker();
        }
        // Write .mcp.json so Claude Code spawns `spyc --mcp` (stdio),
        // which proxies to our Unix socket.
        if app.view.mcp_running {
            app.ensure_mcp_config(mcp_takeover_allowed);
        }
        app
    }
}
