//! Opt-in debug logging to a file.
//!
//! Enabled by `--debug` / `-d` CLI flag or `SPYC_DEBUG=1` env var.
//! Logs go to `/tmp/spyc-debug-<TIMESTAMP>.log`.  The path is printed to
//! stderr at startup so you can `tail -f` it.
//!
//! Usage:
//!   spyc_debug!("view_top={} grid={}x{}", vt, cols, rows);

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;

struct LogState {
    file: Option<std::fs::File>,
    path: Option<String>,
}

static LOG: Mutex<LogState> = Mutex::new(LogState {
    file: None,
    path: None,
});

fn make_path() -> String {
    let ts = crate::sysinfo::epoch_secs();
    format!("/tmp/spyc-debug-{ts}.log")
}

/// Call once at startup.  Returns the log path if debug mode is active.
pub fn init(flag: bool) -> Option<String> {
    let enabled = flag
        || std::env::var("SPYC_DEBUG").is_ok_and(|v| !v.is_empty() && v != "0" && v != "false");
    if !enabled {
        return None;
    }
    let path = make_path();
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
        let mut state = LOG.lock().unwrap();
        state.file = Some(f);
        state.path = Some(path.clone());
        drop(state);
        Some(path)
    } else {
        None
    }
}

#[doc(hidden)]
pub fn log(msg: &str) {
    if let Some(f) = LOG.lock().unwrap().file.as_mut() {
        let _ = writeln!(f, "{msg}");
    }
}

#[macro_export]
macro_rules! spyc_debug {
    ($($arg:tt)*) => {
        $crate::debug_log::log(&format!($($arg)*))
    };
}
