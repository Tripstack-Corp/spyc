//! Embedded Lua scripting engine (docs/LUA_SCRIPTING_PLAN.md).
//!
//! `mlua::Lua` runs on ONE dedicated worker thread ([`LuaWorker`]); the main
//! loop talks to it over channels. A script never mutates spyc directly — its
//! `spyc.*` API enqueues [`LuaRequest`]s, which the App layer translates into
//! the existing effect/action vocabulary after the script returns. The
//! interpreter handle stays thread-local to the worker, so spyc never needs
//! mlua's `send` feature.
//!
//! Runaway scripts can't wedge the UI: the worker installs an instruction-count
//! hook that aborts on a shared abort flag (the App's kill switch) or a hard
//! wall-clock ceiling. Failures are contained — a run yields an error string,
//! never a panic.

mod api;
mod bridge;
mod worker;

pub use bridge::{LuaRequest, RegKind, Registration};
pub use worker::{LuaJob, LuaOutcome, LuaWorker};

use std::sync::atomic::{AtomicBool, Ordering};

/// Process-wide enable flag. On by default; `--no-lua` clears it at startup and
/// `:lua on` / `:lua off` toggle it at runtime. Read by `App::ensure_lua_worker`
/// before spawning the interpreter, so a disabled engine costs nothing.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Enable or disable the Lua engine process-wide.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Whether the Lua engine is currently enabled.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}
