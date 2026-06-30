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
    /// key or a `spyc.command`), collecting its requests like `RunFile`.
    RunRegistered { fn_id: u64, snapshot: SpycContext },
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
            LuaJob::RunRegistered { fn_id, snapshot } => {
                run_job(&mut || run_registered(&lua, &bridge, &fnreg, fn_id, snapshot.clone()));
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
    install_hook(&lua, abort, deadline);
    Ok(lua)
}

fn install_hook(lua: &Lua, abort: &Arc<AtomicBool>, deadline: &Rc<Cell<Option<Instant>>>) {
    let abort = Arc::clone(abort);
    let deadline = Rc::clone(deadline);
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
    );
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
/// requests it enqueues (a `spyc.map` key or a `spyc.command` firing).
fn run_registered(
    lua: &Lua,
    bridge: &SharedBridge,
    fnreg: &SharedFnRegistry,
    fn_id: u64,
    snapshot: SpycContext,
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
    let result = func.and_then(|f| f.call::<()>(()));
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

    /// `spyc.command` / `spyc.on` register with their own kinds; the inert
    /// `spyc.on` still records a registration (the App stores but never
    /// dispatches it).
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
        });
        assert!(outcome.error.is_some());
        assert!(outcome.requests.is_empty());
    }
}
