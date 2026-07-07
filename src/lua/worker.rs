//! The Lua worker thread and its handle.
//!
//! One [`LuaWorker`] owns a single `mlua::Lua` pinned to a dedicated thread and
//! processes [`LuaJob`]s serially. Scripts can't wedge the UI: an instruction
//! hook aborts the running script when the abort flag is set (the App's kill
//! switch) or a hard wall-clock ceiling is hit. Failures are contained — a job
//! always yields a [`LuaOutcome`], never a panic.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mlua::{Function, HookTriggers, Lua, VmState};

use crate::context::SpycContext;

use super::api::{self, FnRegistry, SharedFnRegistry};
use super::bridge::{Bridge, LuaRequest, Registration, SharedBridge};

/// VM instructions between hook checks: small enough to interrupt a tight
/// `while true do end` promptly, large enough not to tax normal scripts.
const HOOK_EVERY: u32 = 100_000;

/// Default hard wall-clock ceiling for one run — a backstop that fires even if
/// the user never intervenes via the abort flag.
const DEFAULT_CEILING: Duration = Duration::from_secs(30);

/// Work handed to the worker. `RunFile` / `Load` / `Reload` carry a one-shot
/// context snapshot; `LuaJob` is moved singly through the job channel, so the
/// variant-size gap isn't worth boxing for.
#[allow(clippy::large_enum_variant)]
pub enum LuaJob {
    /// Run `<lua_dir>/<name>.lua` with `snapshot` as `spyc.context()`.
    RunFile {
        name: String,
        lua_dir: PathBuf,
        snapshot: SpycContext,
    },
    /// Run `init.lua`, collecting the `spyc.map` / `spyc.command` / `spyc.on`
    /// registrations it makes into the outcome (and storing each callback in
    /// the persistent [`FnRegistry`]).
    Load {
        init_path: PathBuf,
        snapshot: SpycContext,
    },
    /// Drop the interpreter + every stored callback, build a fresh one, then
    /// re-run `init.lua` — the live-reload path. Yields fresh registrations.
    Reload {
        init_path: PathBuf,
        snapshot: SpycContext,
    },
    /// Invoke a previously-registered callback by its `fn_id` (a `spyc.map`
    /// key, a `spyc.command`, or a `spyc.on` event hook), collecting its
    /// requests like `RunFile`. `event` is the callback argument: `None` for a
    /// map/command trigger (called 0-arg), or `Some(payload)` for an event
    /// hook — the payload is marshaled to a Lua table via `json_to_lua` and
    /// passed as the single `ev` argument.
    RunRegistered {
        fn_id: u64,
        snapshot: SpycContext,
        event: Option<serde_json::Value>,
    },
    /// Stop the worker thread.
    Shutdown,
}

/// The result of one run, drained by the main loop.
#[derive(Debug, Clone)]
pub struct LuaOutcome {
    /// Requests the script enqueued, in order. Empty if the script failed or
    /// was aborted — a failed run applies nothing.
    pub requests: Vec<LuaRequest>,
    /// Registrations a `Load`/`Reload` run made (`spyc.map` / `spyc.command` /
    /// `spyc.on`); empty for any other job. A failed load yields none.
    pub registrations: Vec<Registration>,
    /// Human-readable error if the script failed/aborted; `None` on success.
    pub error: Option<String>,
}

/// Handle to the Lua worker thread. Held by `Runtime`.
pub struct LuaWorker {
    job_tx: Sender<LuaJob>,
    outcomes: Arc<Mutex<Vec<LuaOutcome>>>,
    abort: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl LuaWorker {
    /// Spawn the worker with the default runaway ceiling. `wake` is called once
    /// per completed job so the main loop can drain outcomes.
    pub fn spawn<W>(wake: W) -> Self
    where
        W: Fn() + Send + 'static,
    {
        Self::spawn_with_ceiling(wake, DEFAULT_CEILING)
    }

    fn spawn_with_ceiling<W>(wake: W, ceiling: Duration) -> Self
    where
        W: Fn() + Send + 'static,
    {
        let (job_tx, job_rx) = mpsc::channel::<LuaJob>();
        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let abort = Arc::new(AtomicBool::new(false));
        let busy = Arc::new(AtomicBool::new(false));
        let handle = {
            let outcomes = Arc::clone(&outcomes);
            let abort = Arc::clone(&abort);
            let busy = Arc::clone(&busy);
            std::thread::Builder::new()
                .name("spyc-lua".to_string())
                .spawn(move || run(&job_rx, &outcomes, &abort, &busy, ceiling, &wake))
                .ok()
        };
        Self {
            job_tx,
            outcomes,
            abort,
            busy,
            handle,
        }
    }

    /// Queue a job. Returns an error string if the worker thread is gone.
    pub fn submit(&self, job: LuaJob) -> Result<(), &'static str> {
        self.job_tx.send(job).map_err(|_| "lua worker unavailable")
    }

    /// Whether a job is currently executing.
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    /// Request the in-flight script abort: the instruction hook unwinds the
    /// running `call()` at its next check (the App's interactive kill switch
    /// for a runaway). The worker resets `abort=false` at each job start, so
    /// this only affects the currently-executing job — a no-op when idle.
    pub fn request_abort(&self) {
        self.abort.store(true, Ordering::SeqCst);
    }

    /// Take all completed outcomes (the main loop drains these on wake).
    pub fn drain_outcomes(&self) -> Vec<LuaOutcome> {
        self.outcomes
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }
}

impl Drop for LuaWorker {
    fn drop(&mut self) {
        // Abort any in-flight script so a runaway can't block the join, then
        // ask the thread to stop and wait for it.
        self.abort.store(true, Ordering::SeqCst);
        let _ = self.job_tx.send(LuaJob::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// The worker thread body: build the interpreter, install the kill hook, then
/// run jobs until the channel closes or `Shutdown`. A `Reload` job rebuilds the
/// interpreter in place (dropping every stored callback), so `lua` is `mut`.
fn run<W: Fn()>(
    job_rx: &Receiver<LuaJob>,
    outcomes: &Arc<Mutex<Vec<LuaOutcome>>>,
    abort: &Arc<AtomicBool>,
    busy: &Arc<AtomicBool>,
    ceiling: Duration,
    wake: &W,
) {
    let bridge: SharedBridge = Rc::new(RefCell::new(Bridge::default()));
    let fnreg: SharedFnRegistry = Rc::new(RefCell::new(FnRegistry::default()));
    let deadline: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));

    let mut lua = match new_interpreter(&bridge, &fnreg, abort, &deadline) {
        Ok(lua) => lua,
        Err(e) => {
            report_setup_failure(
                job_rx,
                outcomes,
                &format!("lua: API setup failed: {e}"),
                wake,
            );
            return;
        }
    };

    // Run one job under the busy flag + runaway deadline, push its outcome, and
    // wake the loop. Shared by every run/reload arm so the bookkeeping is
    // identical across them.
    let run_job = |outcome_fn: &mut dyn FnMut() -> LuaOutcome| {
        busy.store(true, Ordering::SeqCst);
        abort.store(false, Ordering::SeqCst);
        deadline.set(Some(Instant::now() + ceiling));
        let outcome = outcome_fn();
        deadline.set(None);
        busy.store(false, Ordering::SeqCst);
        if let Ok(mut v) = outcomes.lock() {
            v.push(outcome);
        }
        wake();
    };

    while let Ok(job) = job_rx.recv() {
        match job {
            LuaJob::Shutdown => break,
            LuaJob::RunFile {
                name,
                lua_dir,
                snapshot,
            } => run_job(&mut || run_file(&lua, &bridge, &name, &lua_dir, snapshot.clone())),
            LuaJob::Load {
                init_path,
                snapshot,
            } => run_job(&mut || run_load(&lua, &bridge, &init_path, snapshot.clone())),
            LuaJob::RunRegistered {
                fn_id,
                snapshot,
                event,
            } => {
                run_job(&mut || {
                    run_registered(
                        &lua,
                        &bridge,
                        &fnreg,
                        fn_id,
                        snapshot.clone(),
                        event.clone(),
                    )
                });
            }
            LuaJob::Reload {
                init_path,
                snapshot,
            } => {
                // A fresh interpreter frees every old `RegistryKey`; clearing
                // the registry keeps ids aligned with the new keys.
                fnreg.borrow_mut().clear();
                match new_interpreter(&bridge, &fnreg, abort, &deadline) {
                    Ok(fresh) => {
                        lua = fresh;
                        run_job(&mut || run_load(&lua, &bridge, &init_path, snapshot.clone()));
                    }
                    Err(e) => run_job(&mut || LuaOutcome {
                        requests: Vec::new(),
                        registrations: Vec::new(),
                        error: Some(format!("lua: reload failed: {e}")),
                    }),
                }
            }
        }
    }
}

/// Build a fresh interpreter with the `spyc` API installed and the runaway hook
/// armed. Used at startup and on every `Reload`.
fn new_interpreter(
    bridge: &SharedBridge,
    fnreg: &SharedFnRegistry,
    abort: &Arc<AtomicBool>,
    deadline: &Rc<Cell<Option<Instant>>>,
) -> mlua::Result<Lua> {
    let lua = Lua::new();
    api::install(&lua, bridge, fnreg)?;
    install_hook(&lua, abort, deadline)?;
    Ok(lua)
}

fn install_hook(
    lua: &Lua,
    abort: &Arc<AtomicBool>,
    deadline: &Rc<Cell<Option<Instant>>>,
) -> mlua::Result<()> {
    let abort = Arc::clone(abort);
    let deadline = Rc::clone(deadline);
    // mlua 0.12: `set_hook` is fallible (returns `Result`); propagate to the
    // `new_interpreter` caller, which is already `mlua::Result`.
    lua.set_hook(
        HookTriggers::default().every_nth_instruction(HOOK_EVERY),
        move |_lua, _debug| {
            if abort.load(Ordering::Relaxed) {
                return Err(mlua::Error::runtime("aborted"));
            }
            if deadline.get().is_some_and(|d| Instant::now() >= d) {
                return Err(mlua::Error::runtime("exceeded time limit"));
            }
            Ok(VmState::Continue)
        },
    )
}

/// Reset the bridge for a new run: drop the prior run's requests/registrations
/// and install the snapshot the script will read via `spyc.context()`.
fn reset_bridge(bridge: &SharedBridge, snapshot: SpycContext) {
    let mut b = bridge.borrow_mut();
    b.requests.clear();
    b.registrations.clear();
    b.snapshot = Some(snapshot);
}

fn run_file(
    lua: &Lua,
    bridge: &SharedBridge,
    name: &str,
    lua_dir: &Path,
    snapshot: SpycContext,
) -> LuaOutcome {
    reset_bridge(bridge, snapshot);
    let path = lua_dir.join(format!("{name}.lua"));
    let code = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            return LuaOutcome {
                requests: Vec::new(),
                registrations: Vec::new(),
                error: Some(format!("lua: cannot read {}: {e}", path.display())),
            };
        }
    };
    let result = lua.load(code.as_str()).set_name(name).exec();
    let requests = std::mem::take(&mut bridge.borrow_mut().requests);
    match result {
        Ok(()) => LuaOutcome {
            requests,
            registrations: Vec::new(),
            error: None,
        },
        // A failed/aborted run applies nothing — drop partial requests.
        Err(e) => LuaOutcome {
            requests: Vec::new(),
            registrations: Vec::new(),
            error: Some(format!("lua: {name}: {e}")),
        },
    }
}

/// Run `init.lua`, returning the registrations it made. Each `spyc.map` /
/// `spyc.command` / `spyc.on` call stored its `Function` in the [`FnRegistry`]
/// and pushed a [`Registration`] onto the bridge during the run.
fn run_load(
    lua: &Lua,
    bridge: &SharedBridge,
    init_path: &Path,
    snapshot: SpycContext,
) -> LuaOutcome {
    reset_bridge(bridge, snapshot);
    let code = match std::fs::read_to_string(init_path) {
        Ok(c) => c,
        Err(e) => {
            return LuaOutcome {
                requests: Vec::new(),
                registrations: Vec::new(),
                error: Some(format!("lua: cannot read {}: {e}", init_path.display())),
            };
        }
    };
    let result = lua.load(code.as_str()).set_name("init").exec();
    let registrations = std::mem::take(&mut bridge.borrow_mut().registrations);
    match result {
        Ok(()) => LuaOutcome {
            requests: Vec::new(),
            registrations,
            error: None,
        },
        // A failed load registers nothing — drop partial registrations.
        Err(e) => LuaOutcome {
            requests: Vec::new(),
            registrations: Vec::new(),
            error: Some(format!("lua: init.lua: {e}")),
        },
    }
}

/// Invoke a previously-registered callback by its `fn_id`, collecting the
/// requests it enqueues (a `spyc.map` key, a `spyc.command`, or a `spyc.on`
/// event hook firing). `event` is the callback argument: `None` calls it 0-arg
/// (map/command); `Some(payload)` marshals the payload to a Lua table and
/// passes it as the single `ev` argument (an event hook).
fn run_registered(
    lua: &Lua,
    bridge: &SharedBridge,
    fnreg: &SharedFnRegistry,
    fn_id: u64,
    snapshot: SpycContext,
    event: Option<serde_json::Value>,
) -> LuaOutcome {
    reset_bridge(bridge, snapshot);
    // Resolve the stored callback to an owned `Function` before calling — the
    // registry borrow must not be held across the call (a callback could
    // register more functions, re-borrowing it).
    let func: mlua::Result<Function> = {
        let reg = fnreg.borrow();
        match reg.get(fn_id) {
            Some(key) => lua.registry_value::<Function>(key),
            None => Err(mlua::Error::runtime(format!(
                "no registered function #{fn_id}"
            ))),
        }
    };
    let result = func.and_then(|f| match &event {
        // An event hook: pass the marshaled payload table as `ev`.
        Some(payload) => api::json_to_lua(lua, payload).and_then(|arg| f.call::<()>(arg)),
        // A map/command trigger: 0-arg.
        None => f.call::<()>(()),
    });
    let requests = std::mem::take(&mut bridge.borrow_mut().requests);
    match result {
        Ok(()) => LuaOutcome {
            requests,
            registrations: Vec::new(),
            error: None,
        },
        Err(e) => LuaOutcome {
            requests: Vec::new(),
            registrations: Vec::new(),
            error: Some(format!("lua: registered fn #{fn_id}: {e}")),
        },
    }
}

/// On interpreter setup failure, answer every queued job with the error so
/// callers aren't left waiting (extreme edge — `Lua::new` + API install
/// effectively never fail).
fn report_setup_failure<W: Fn()>(
    job_rx: &Receiver<LuaJob>,
    outcomes: &Arc<Mutex<Vec<LuaOutcome>>>,
    msg: &str,
    wake: &W,
) {
    while let Ok(job) = job_rx.recv() {
        if matches!(job, LuaJob::Shutdown) {
            break;
        }
        if let Ok(mut v) = outcomes.lock() {
            v.push(LuaOutcome {
                requests: Vec::new(),
                registrations: Vec::new(),
                error: Some(msg.to_string()),
            });
        }
        wake();
    }
}

#[cfg(test)]
mod tests {
    use super::super::bridge::RegKind;
    use super::*;
    use crate::git::test_support::run_git;

    fn dummy_snapshot() -> SpycContext {
        SpycContext {
            cwd: PathBuf::from("/tmp"),
            cursor_file: None,
            picks: Vec::new(),
            inventory: Vec::new(),
            filter: None,
            git_branch: None,
            project_home: None,
            search_root: None,
            session_name: String::new(),
            pid: 0,
            version: String::new(),
        }
    }

    /// A snapshot rooted at `dir` (both `cwd` and `search_root`), so the live
    /// reads scope to a real temp dir / git repo the test just built.
    fn snapshot_at(dir: &Path) -> SpycContext {
        SpycContext {
            cwd: dir.to_path_buf(),
            search_root: Some(dir.to_path_buf()),
            ..dummy_snapshot()
        }
    }

    /// The single request a live-read test produces: the script computed a read
    /// and encoded the result into `spyc.notify(...)`, so asserting on the
    /// notify text proves the read ran end to end through a real worker.
    fn notify_text(outcome: &LuaOutcome) -> &str {
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        match outcome.requests.as_slice() {
            [LuaRequest::Notify(msg)] => msg,
            other => panic!("expected one Notify request, got {other:?}"),
        }
    }

    #[test]
    fn runs_a_script_and_collects_requests_in_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("t.lua"),
            "spyc.notify('hi'); spyc.navigate('/etc'); spyc.action('down', 3)",
        )
        .unwrap();

        let (tx, rx) = mpsc::channel::<()>();
        let worker = LuaWorker::spawn(move || {
            let _ = tx.send(());
        });
        worker
            .submit(LuaJob::RunFile {
                name: "t".to_string(),
                lua_dir: dir.path().to_path_buf(),
                snapshot: dummy_snapshot(),
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(5))
            .expect("job completed");

        let outcomes = worker.drain_outcomes();
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].error.is_none(), "{:?}", outcomes[0].error);
        assert_eq!(
            outcomes[0].requests,
            vec![
                LuaRequest::Notify("hi".to_string()),
                LuaRequest::Navigate("/etc".to_string()),
                LuaRequest::Action {
                    name: "down".to_string(),
                    count: Some(3),
                },
            ]
        );
    }

    #[test]
    fn context_reads_reflect_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("c.lua"),
            "if spyc.context().cwd == '/tmp' then spyc.notify('ok') end",
        )
        .unwrap();

        let (tx, rx) = mpsc::channel::<()>();
        let worker = LuaWorker::spawn(move || {
            let _ = tx.send(());
        });
        worker
            .submit(LuaJob::RunFile {
                name: "c".to_string(),
                lua_dir: dir.path().to_path_buf(),
                snapshot: dummy_snapshot(),
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(5))
            .expect("job completed");

        let outcomes = worker.drain_outcomes();
        assert_eq!(
            outcomes[0].requests,
            vec![LuaRequest::Notify("ok".to_string())]
        );
    }

    #[test]
    fn aborts_a_runaway_script_via_the_ceiling() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("loop.lua"), "while true do end").unwrap();

        let (tx, rx) = mpsc::channel::<()>();
        // Short ceiling so the hook trips quickly instead of waiting 30s.
        let worker = LuaWorker::spawn_with_ceiling(
            move || {
                let _ = tx.send(());
            },
            Duration::from_millis(200),
        );
        worker
            .submit(LuaJob::RunFile {
                name: "loop".to_string(),
                lua_dir: dir.path().to_path_buf(),
                snapshot: dummy_snapshot(),
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(5))
            .expect("runaway aborted, not hung");

        let outcomes = worker.drain_outcomes();
        assert_eq!(outcomes.len(), 1);
        assert!(
            outcomes[0].error.is_some(),
            "a runaway script must report an error"
        );
        assert!(
            outcomes[0].requests.is_empty(),
            "aborted run applies nothing"
        );
    }

    #[test]
    fn missing_script_reports_error_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel::<()>();
        let worker = LuaWorker::spawn(move || {
            let _ = tx.send(());
        });
        worker
            .submit(LuaJob::RunFile {
                name: "nope".to_string(),
                lua_dir: dir.path().to_path_buf(),
                snapshot: dummy_snapshot(),
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(5))
            .expect("job completed");

        let outcomes = worker.drain_outcomes();
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].error.is_some());
    }

    #[test]
    fn syntax_error_is_contained() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.lua"), "this is not lua )(").unwrap();

        let (tx, rx) = mpsc::channel::<()>();
        let worker = LuaWorker::spawn(move || {
            let _ = tx.send(());
        });
        worker
            .submit(LuaJob::RunFile {
                name: "bad".to_string(),
                lua_dir: dir.path().to_path_buf(),
                snapshot: dummy_snapshot(),
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(5))
            .expect("job completed");

        let outcomes = worker.drain_outcomes();
        assert!(outcomes[0].error.is_some());
        assert!(outcomes[0].requests.is_empty());
    }

    /// Spawn a worker, submit `job`, and drain the single outcome it produces.
    /// Shared by the Load/RunRegistered round-trip tests below.
    fn run_one(job: LuaJob) -> LuaOutcome {
        let (tx, rx) = mpsc::channel::<()>();
        let worker = LuaWorker::spawn(move || {
            let _ = tx.send(());
        });
        worker.submit(job).unwrap();
        rx.recv_timeout(Duration::from_secs(5))
            .expect("job completed");
        let mut outcomes = worker.drain_outcomes();
        assert_eq!(outcomes.len(), 1);
        outcomes.pop().expect("one outcome")
    }

    /// The Tier-B round-trip: `init.lua` registers a `spyc.map` callback (a),
    /// and a subsequent `RunRegistered` for that callback's `fn_id` fires it,
    /// producing its requests (b). This is the contract the whole init.lua
    /// platform rests on, so both halves are asserted end to end.
    #[test]
    fn load_registers_a_map_and_run_registered_fires_it() {
        let dir = tempfile::tempdir().unwrap();
        let init = dir.path().join("init.lua");
        std::fs::write(&init, "spyc.map('z', function() spyc.notify('hit') end)").unwrap();

        // (a) The load collects exactly one Map registration.
        let loaded = run_one(LuaJob::Load {
            init_path: init.clone(),
            snapshot: dummy_snapshot(),
        });
        assert!(loaded.error.is_none(), "{:?}", loaded.error);
        assert!(
            loaded.requests.is_empty(),
            "registration alone enqueues no requests"
        );
        assert_eq!(loaded.registrations.len(), 1);
        let reg = &loaded.registrations[0];
        assert_eq!(reg.kind, RegKind::Map("z".to_string()));

        // (b) Firing that callback by its id runs the body — note that the
        // FnRegistry persists across runs on the SAME worker, so we keep the
        // worker alive rather than using `run_one` for both halves.
        let (tx, rx) = mpsc::channel::<()>();
        let worker = LuaWorker::spawn(move || {
            let _ = tx.send(());
        });
        worker
            .submit(LuaJob::Load {
                init_path: init,
                snapshot: dummy_snapshot(),
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).expect("load done");
        let fn_id = worker.drain_outcomes()[0].registrations[0].fn_id;

        worker
            .submit(LuaJob::RunRegistered {
                fn_id,
                snapshot: dummy_snapshot(),
                event: None,
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).expect("fired");
        let fired = worker.drain_outcomes();
        assert_eq!(fired.len(), 1);
        assert!(fired[0].error.is_none(), "{:?}", fired[0].error);
        assert_eq!(
            fired[0].requests,
            vec![LuaRequest::Notify("hit".to_string())]
        );
    }

    /// `spyc.command` / `spyc.on` register with their own kinds — the App
    /// stores each and (for `spyc.on`) fires it on the wired events.
    #[test]
    fn load_records_command_and_event_registrations() {
        let dir = tempfile::tempdir().unwrap();
        let init = dir.path().join("init.lua");
        std::fs::write(
            &init,
            "spyc.command('blame', function() spyc.action('left') end)\n\
             spyc.on('startup', function() end)",
        )
        .unwrap();

        let loaded = run_one(LuaJob::Load {
            init_path: init,
            snapshot: dummy_snapshot(),
        });
        assert!(loaded.error.is_none(), "{:?}", loaded.error);
        let kinds: Vec<&RegKind> = loaded.registrations.iter().map(|r| &r.kind).collect();
        assert_eq!(
            kinds,
            vec![
                &RegKind::Command("blame".to_string()),
                &RegKind::Event("startup".to_string()),
            ]
        );
    }

    /// Calling an unregistered id reports an error rather than panicking.
    #[test]
    fn run_registered_unknown_id_errors() {
        let outcome = run_one(LuaJob::RunRegistered {
            fn_id: 999,
            snapshot: dummy_snapshot(),
            event: None,
        });
        assert!(outcome.error.is_some());
        assert!(outcome.requests.is_empty());
    }

    /// A `spyc.on` event hook receives the event payload as its argument: load
    /// registers an `agent_status` handler that reads `ev.state`, fire it via
    /// `RunRegistered` with an event payload, and assert the callback saw the
    /// table (it echoes `ev.state` back through `spyc.notify`). Proves the
    /// `event: Some(..)` arm marshals the payload and calls the callback 1-arg.
    #[test]
    fn run_registered_passes_the_event_payload() {
        let dir = tempfile::tempdir().unwrap();
        let init = dir.path().join("init.lua");
        std::fs::write(
            &init,
            "spyc.on('agent_status', function(ev) spyc.notify('state=' .. ev.state) end)",
        )
        .unwrap();

        let (tx, rx) = mpsc::channel::<()>();
        let worker = LuaWorker::spawn(move || {
            let _ = tx.send(());
        });
        worker
            .submit(LuaJob::Load {
                init_path: init,
                snapshot: dummy_snapshot(),
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).expect("load done");
        let loaded = worker.drain_outcomes();
        let fn_id = loaded[0].registrations[0].fn_id;
        assert_eq!(
            loaded[0].registrations[0].kind,
            RegKind::Event("agent_status".to_string())
        );

        worker
            .submit(LuaJob::RunRegistered {
                fn_id,
                snapshot: dummy_snapshot(),
                event: Some(serde_json::json!({ "pane": 2, "state": "blocked" })),
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).expect("fired");
        let fired = worker.drain_outcomes();
        assert_eq!(fired.len(), 1);
        assert!(fired[0].error.is_none(), "{:?}", fired[0].error);
        assert_eq!(
            fired[0].requests,
            vec![LuaRequest::Notify("state=blocked".to_string())],
            "the callback must receive the event payload as `ev`"
        );
    }

    /// The must-have live-read proof: `spyc.read` returns a written file's
    /// contents through a real worker run, resolving the relative path against
    /// the snapshot's cwd.
    #[test]
    fn spyc_read_returns_file_contents() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hello.txt"), "world\n").unwrap();
        std::fs::write(
            dir.path().join("t.lua"),
            "spyc.notify(spyc.read('hello.txt'))",
        )
        .unwrap();

        let outcome = run_one(LuaJob::RunFile {
            name: "t".to_string(),
            lua_dir: dir.path().to_path_buf(),
            snapshot: snapshot_at(dir.path()),
        });
        assert_eq!(notify_text(&outcome), "world\n");
    }

    /// `spyc.read` of a missing file raises, so the run reports an error and
    /// applies nothing (the raise-on-failure convention).
    #[test]
    fn spyc_read_missing_file_raises() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("t.lua"), "spyc.read('nope.txt')").unwrap();

        let outcome = run_one(LuaJob::RunFile {
            name: "t".to_string(),
            lua_dir: dir.path().to_path_buf(),
            snapshot: snapshot_at(dir.path()),
        });
        assert!(outcome.error.is_some(), "missing file must raise");
        assert!(outcome.requests.is_empty(), "a raised read applies nothing");
    }

    /// `spyc.search_paths` finds a file by fuzzy query (gitignore-aware walk),
    /// returning a sequence table the script can index.
    #[test]
    fn spyc_search_paths_finds_a_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join("needle.rs"), "x\n").unwrap();
        std::fs::write(
            dir.path().join("t.lua"),
            "local hits = spyc.search_paths('needle')\n\
             spyc.notify(hits[1] or 'none')",
        )
        .unwrap();

        let outcome = run_one(LuaJob::RunFile {
            name: "t".to_string(),
            lua_dir: dir.path().to_path_buf(),
            snapshot: snapshot_at(dir.path()),
        });
        assert_eq!(notify_text(&outcome), "needle.rs");
    }

    /// `spyc.search_content` returns `{file, line, text}` rows the script reads.
    #[test]
    fn spyc_search_content_returns_matches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join("a.txt"), "one\nNEEDLE here\nthree\n").unwrap();
        // The script itself contains the needle (as its search argument), so the
        // search matches BOTH a.txt and t.lua and the result order is unspecified.
        // Pick the a.txt row by file rather than trusting hits[1], so the assertion
        // is deterministic regardless of directory-walk order.
        std::fs::write(
            dir.path().join("t.lua"),
            "local hits = spyc.search_content('NEEDLE')\n\
             local m\n\
             for _, h in ipairs(hits) do if h.file == 'a.txt' then m = h end end\n\
             spyc.notify(m and (m.file .. ':' .. m.line .. ':' .. m.text) or 'a.txt not found')",
        )
        .unwrap();

        let outcome = run_one(LuaJob::RunFile {
            name: "t".to_string(),
            lua_dir: dir.path().to_path_buf(),
            snapshot: snapshot_at(dir.path()),
        });
        assert_eq!(notify_text(&outcome), "a.txt:2:NEEDLE here");
    }

    /// `spyc.search_content` with an invalid regex raises (aborting the run).
    #[test]
    fn spyc_search_content_bad_regex_raises() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join("t.lua"),
            "spyc.search_content('[unterminated')",
        )
        .unwrap();

        let outcome = run_one(LuaJob::RunFile {
            name: "t".to_string(),
            lua_dir: dir.path().to_path_buf(),
            snapshot: snapshot_at(dir.path()),
        });
        assert!(outcome.error.is_some(), "a bad regex must raise");
    }

    /// `spyc.git_status` returns a table of `{path, staged, unstaged,
    /// untracked}` rows reflecting the worktree.
    #[test]
    fn spyc_git_status_reflects_the_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = std::fs::canonicalize(dir.path()).unwrap();
        run_git(&repo, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-q", "-m", "first"]);
        // One untracked file → git_status has a single untracked entry. The
        // script lives OUTSIDE the repo so it doesn't itself show as untracked.
        std::fs::write(repo.join("new.txt"), "x\n").unwrap();
        let scripts = tempfile::tempdir().unwrap();
        std::fs::write(
            scripts.path().join("t.lua"),
            "local st = spyc.git_status()\n\
             spyc.notify(#st .. ':' .. st[1].path .. ':' .. tostring(st[1].untracked))",
        )
        .unwrap();

        let outcome = run_one(LuaJob::RunFile {
            name: "t".to_string(),
            lua_dir: scripts.path().to_path_buf(),
            snapshot: snapshot_at(&repo),
        });
        assert_eq!(notify_text(&outcome), "1:new.txt:true");
    }

    /// `spyc.git_log` returns commit tables with a `limit` opt; the newest
    /// commit's subject comes back.
    #[test]
    fn spyc_git_log_returns_commits() {
        let dir = tempfile::tempdir().unwrap();
        let repo = std::fs::canonicalize(dir.path()).unwrap();
        run_git(&repo, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-q", "-m", "first"]);
        std::fs::write(repo.join("b.txt"), "b\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-q", "-m", "second"]);
        std::fs::write(
            repo.join("t.lua"),
            "local log = spyc.git_log({ limit = 1 })\n\
             spyc.notify(#log .. ':' .. log[1].subject)",
        )
        .unwrap();

        let outcome = run_one(LuaJob::RunFile {
            name: "t".to_string(),
            lua_dir: repo.clone(),
            snapshot: snapshot_at(&repo),
        });
        assert_eq!(notify_text(&outcome), "1:second");
    }

    /// `spyc.worktrees` returns a table of worktree entries (the main worktree
    /// at least), each with a `branch` field.
    #[test]
    fn spyc_worktrees_lists_the_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = std::fs::canonicalize(dir.path()).unwrap();
        run_git(&repo, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-q", "-m", "first"]);
        std::fs::write(
            repo.join("t.lua"),
            "local wts = spyc.worktrees()\n\
             spyc.notify(#wts .. ':' .. wts[1].branch)",
        )
        .unwrap();

        let outcome = run_one(LuaJob::RunFile {
            name: "t".to_string(),
            lua_dir: repo.clone(),
            snapshot: snapshot_at(&repo),
        });
        assert_eq!(notify_text(&outcome), "1:main");
    }
}
