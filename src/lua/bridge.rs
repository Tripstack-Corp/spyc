//! The data a Lua script produces and the worker-thread-local bridge state.
//!
//! A script never touches spyc's `App` — its `spyc.*` functions enqueue
//! [`LuaRequest`] values into the [`Bridge`], which the worker hands back after
//! the script returns. The App layer (`src/app/lua.rs`) translates each request
//! into the existing effect/action vocabulary. Keeping requests as plain data
//! is what lets the interpreter live off the main thread without sharing any
//! `App` state.

use std::cell::RefCell;
use std::rc::Rc;

use crate::context::SpycContext;

/// Cap on requests a single script run may enqueue. The requests are applied on
/// the MAIN thread (`handle_lua_done`), outside the worker's instruction-hook
/// kill switch, so an unbounded `for … do spyc.notify() end` would flood the
/// loop. Hitting the cap aborts the script with an error.
pub const MAX_REQUESTS: usize = 10_000;

/// One request a script makes back to spyc. Plain data — no OS handles, no
/// `App` types — so it crosses the worker→loop boundary freely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LuaRequest {
    /// Run a built-in action by its `.spycrc` DSL verb (e.g. `down`, `git_blame`).
    Action { name: String, count: Option<u32> },
    /// Run a `:` command line, sans the leading colon (e.g. `grep foo`).
    Command(String),
    /// Navigate the focused column to a path.
    Navigate(String),
    /// Pick files matching glob patterns (additive to the current picks).
    Pick(Vec<String>),
    /// Clear all picks in the focused column.
    ClearPicks,
    /// Set the limit filter; `None` (or empty) clears it.
    Filter(Option<String>),
    /// Self-report agent status: `working` | `blocked` | `done` | `idle`.
    ReportStatus(String),
    /// Flash an informational message on the status line.
    Notify(String),
    /// Flash a warning on the status line.
    Warn(String),
}

/// Worker-thread-local state shared (via `Rc<RefCell<…>>`) between the `spyc.*`
/// API closures and the per-run driver: the read-only context snapshot the
/// script sees, and the queue of requests it builds. Reset before every run.
#[derive(Default)]
pub struct Bridge {
    pub snapshot: Option<SpycContext>,
    pub requests: Vec<LuaRequest>,
}

pub type SharedBridge = Rc<RefCell<Bridge>>;
