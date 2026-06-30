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

use mlua::{HookTriggers, Lua, VmState};

use crate::context::SpycContext;

use super::api;
use super::bridge::{Bridge, LuaRequest, SharedBridge};

/// VM instructions between hook checks: small enough to interrupt a tight
/// `while true do end` promptly, large enough not to tax normal scripts.
const HOOK_EVERY: u32 = 100_000;

/// Default hard wall-clock ceiling for one run — a backstop that fires even if
/// the user never intervenes via the abort flag.
const DEFAULT_CEILING: Duration = Duration::from_secs(30);

/// Work handed to the worker. `RunFile` carries a one-shot context snapshot;
/// `LuaJob` is moved singly through the job channel, so the variant-size gap
/// isn't worth boxing for.
#[allow(clippy::large_enum_variant)]
pub enum LuaJob {
    /// Run `<lua_dir>/<name>.lua` with `snapshot` as `spyc.context()`.
    RunFile {
        name: String,
        lua_dir: PathBuf,
        snapshot: SpycContext,
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

/// The worker thread body: build the interpreter once, install the kill hook,
/// then run jobs until the channel closes or `Shutdown`.
fn run<W: Fn()>(
    job_rx: &Receiver<LuaJob>,
    outcomes: &Arc<Mutex<Vec<LuaOutcome>>>,
    abort: &Arc<AtomicBool>,
    busy: &Arc<AtomicBool>,
    ceiling: Duration,
    wake: &W,
) {
    let lua = Lua::new();
    let bridge: SharedBridge = Rc::new(RefCell::new(Bridge::default()));
    let deadline: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));

    if let Err(e) = api::install(&lua, &bridge) {
        report_setup_failure(
            job_rx,
            outcomes,
            &format!("lua: API setup failed: {e}"),
            wake,
        );
        return;
    }
    install_hook(&lua, abort, &deadline);

    while let Ok(job) = job_rx.recv() {
        match job {
            LuaJob::Shutdown => break,
            LuaJob::RunFile {
                name,
                lua_dir,
                snapshot,
            } => {
                busy.store(true, Ordering::SeqCst);
                abort.store(false, Ordering::SeqCst);
                deadline.set(Some(Instant::now() + ceiling));
                let outcome = run_file(&lua, &bridge, &name, &lua_dir, snapshot);
                deadline.set(None);
                busy.store(false, Ordering::SeqCst);
                if let Ok(mut v) = outcomes.lock() {
                    v.push(outcome);
                }
                wake();
            }
        }
    }
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

fn run_file(
    lua: &Lua,
    bridge: &SharedBridge,
    name: &str,
    lua_dir: &Path,
    snapshot: SpycContext,
) -> LuaOutcome {
    {
        let mut b = bridge.borrow_mut();
        b.requests.clear();
        b.snapshot = Some(snapshot);
    }
    let path = lua_dir.join(format!("{name}.lua"));
    let code = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            return LuaOutcome {
                requests: Vec::new(),
                error: Some(format!("lua: cannot read {}: {e}", path.display())),
            };
        }
    };
    let result = lua.load(code.as_str()).set_name(name).exec();
    let requests = std::mem::take(&mut bridge.borrow_mut().requests);
    match result {
        Ok(()) => LuaOutcome {
            requests,
            error: None,
        },
        // A failed/aborted run applies nothing — drop partial requests.
        Err(e) => LuaOutcome {
            requests: Vec::new(),
            error: Some(format!("lua: {name}: {e}")),
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
        let LuaJob::RunFile { .. } = job else {
            break;
        };
        if let Ok(mut v) = outcomes.lock() {
            v.push(LuaOutcome {
                requests: Vec::new(),
                error: Some(msg.to_string()),
            });
        }
        wake();
    }
}

#[cfg(test)]
mod tests {
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
}
