//! Opt-in per-key dispatch trace.
//!
//! Enabled by `--key-trace` CLI flag or `SPYC_KEY_TRACE=1` env var.
//! Trace lines go to `/tmp/spyc-key-trace-<TIMESTAMP>.log`, separate from
//! the general debug log so a key-trace session doesn't get drowned in
//! pane drain / repaint noise.
//!
//! The intent is diagnostic-only: when a user reports an input bug
//! ("typed ^a-j too fast and the focus didn't switch"), they can flip
//! the flag, reproduce, and ship the log. Each line is a single key
//! event annotated with the elapsed time since spyc start, the
//! key+modifiers, and where the dispatch sent it.

use std::io::Write;
use std::sync::Mutex;

struct TraceState {
    file: std::fs::File,
    start: std::time::Instant,
    /// Timestamp of the most recent RX event. TX log lines are
    /// annotated with the delta since this so an external reader
    /// can spot input → output latency without correlating two
    /// columns by eye. `None` until the first `note_rx_event` call.
    last_rx_at: Option<std::time::Instant>,
}

static TRACE: Mutex<Option<TraceState>> = Mutex::new(None);

/// Call once at startup. Returns the trace-log path when active.
pub fn init(flag: bool) -> Option<String> {
    let enabled = flag
        || std::env::var("SPYC_KEY_TRACE").is_ok_and(|v| !v.is_empty() && v != "0" && v != "false");
    if !enabled {
        return None;
    }
    let ts = crate::sysinfo::epoch_secs();
    // Owner-only (0600) in the XDG state dir, not the old world-readable,
    // symlink-followable `/tmp/spyc-key-trace-*.log` — the trace records
    // every keystroke (incl. pane input), so on a shared machine /tmp let
    // another user read it or redirect it via a planted symlink.
    let name = format!("spyc-key-trace-{ts}.log");
    let file = crate::state::open_state_file_append(&name)?;
    let display_path =
        crate::state::state_file_path(&name).map_or(name, |p| p.display().to_string());
    {
        let mut slot = TRACE.lock().expect("key-trace mutex poisoned");
        *slot = Some(TraceState {
            file,
            start: std::time::Instant::now(),
            last_rx_at: None,
        });
    }
    Some(display_path)
}

/// Returns true when the trace is armed. Callers can guard expensive
/// formatting behind this.
pub fn is_enabled() -> bool {
    let slot = TRACE.lock().unwrap();
    slot.is_some()
}

/// Log one trace line, prefixed with elapsed-since-start in milliseconds.
/// No-op when the trace isn't armed.
pub fn log(msg: &str) {
    let mut slot = TRACE.lock().unwrap();
    if let Some(state) = slot.as_mut() {
        let elapsed_ms = state.start.elapsed().as_millis();
        let _ = writeln!(state.file, "[{elapsed_ms:>8}ms] {msg}");
    }
}

/// Stamp the most recent RX timestamp. Called immediately after the
/// RX line is logged so `log_tx` can compute "elapsed since the last
/// input event arrived". Cheap; no-op when the trace isn't armed.
pub fn note_rx_event() {
    let mut slot = TRACE.lock().unwrap();
    if let Some(state) = slot.as_mut() {
        state.last_rx_at = Some(std::time::Instant::now());
    }
}

/// Log a TX (bytes-written-to-pty) event. Prepends the standard
/// `[Nms]` global timestamp and appends `[+Nms since RX]` so the
/// reader can see input → forward latency at a glance. The "since
/// RX" suffix is dropped on the very first TX before any RX (e.g.
/// startup banner forwarding from the resume-pane path).
pub fn log_tx(msg: &str) {
    let mut slot = TRACE.lock().unwrap();
    if let Some(state) = slot.as_mut() {
        let now = std::time::Instant::now();
        let elapsed_ms = (now - state.start).as_millis();
        let suffix = state
            .last_rx_at
            .map(|t| format!(" [+{}ms since RX]", (now - t).as_millis()))
            .unwrap_or_default();
        let _ = writeln!(state.file, "[{elapsed_ms:>8}ms] TX {msg}{suffix}");
    }
}
