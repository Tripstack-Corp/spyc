//! Opt-in debug logging to a file.
//!
//! Enabled by `--debug` / `-d` CLI flag or `SPYC_DEBUG=1` env var.
//! Logs go to an **owner-only** file in the XDG state dir
//! (`spyc-debug-<TIMESTAMP>.log`); the resolved path is printed to stderr
//! at startup so you can `tail -f` it. (Previously `/tmp/spyc-debug-*`,
//! but `/tmp` is world-writable — another user could pre-create the file
//! to capture our output, or plant a symlink to redirect the writes.
//! `state::open_state_file_append` creates it `0600` + `O_NOFOLLOW`.)
//!
//! Usage:
//!   spyc_debug!("view_top={} grid={}x{}", vt, cols, rows);
//!
//! The `spyc_debug!` macro is gated by an atomic `ENABLED` flag so that
//! when logging is *off* (the common case) it neither formats its
//! arguments nor locks the global mutex — both were paid on per-wake /
//! per-fs-event hot paths regardless of whether logging was active.

use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

struct LogState {
    file: Option<std::fs::File>,
    path: Option<String>,
}

static LOG: Mutex<LogState> = Mutex::new(LogState {
    file: None,
    path: None,
});

/// Fast, lock-free gate for `spyc_debug!`. Set true once in `init` when a
/// log file is successfully opened; stays false otherwise. Reading it is a
/// single relaxed load — cheap enough for the hottest paths.
static ENABLED: AtomicBool = AtomicBool::new(false);

fn log_file_name() -> String {
    let ts = crate::sysinfo::epoch_secs();
    format!("spyc-debug-{ts}.log")
}

/// Whether debug logging is active. The `spyc_debug!` macro checks this
/// before formatting, so a disabled log costs one relaxed atomic load.
#[doc(hidden)]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Call once at startup.  Returns the log path if debug mode is active.
pub fn init(flag: bool) -> Option<String> {
    let on = flag
        || std::env::var("SPYC_DEBUG").is_ok_and(|v| !v.is_empty() && v != "0" && v != "false");
    if !on {
        return None;
    }
    let name = log_file_name();
    // Owner-only (0600) + O_NOFOLLOW in the XDG state dir — no shared-/tmp
    // pre-creation or symlink-redirect window. `None` if no state dir
    // resolves ($HOME/$XDG_STATE_HOME) or the open fails: debug logging
    // then stays inert rather than falling back to a world-readable path.
    let f = crate::state::open_state_file_append(&name)?;
    let path = crate::state::state_file_path(&name)?
        .to_string_lossy()
        .into_owned();
    let mut state = LOG.lock().expect("debug log mutex poisoned");
    state.file = Some(f);
    state.path = Some(path.clone());
    drop(state);
    ENABLED.store(true, Ordering::Relaxed);
    Some(path)
}

#[doc(hidden)]
pub fn log(msg: &str) {
    if let Some(f) = LOG.lock().expect("debug log mutex poisoned").file.as_mut() {
        let _ = writeln!(f, "{msg}");
    }
}

#[macro_export]
macro_rules! spyc_debug {
    ($($arg:tt)*) => {
        if $crate::debug_log::enabled() {
            $crate::debug_log::log(&format!($($arg)*))
        }
    };
}

#[cfg(test)]
mod tests {
    /// With logging disabled (no `init`), the macro must be inert: the
    /// `enabled()` gate short-circuits before any `format!`/mutex lock.
    /// No test calls `init`, so the process-global flag stays false.
    #[test]
    fn disabled_macro_is_inert() {
        assert!(!super::enabled());
        crate::spyc_debug!("never formatted {}", 1 + 1);
        assert!(!super::enabled());
    }
}
