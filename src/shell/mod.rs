//! Shelling out: editor, pager, `%` substitution.
//!
//! Running a child process from a TUI requires tearing the terminal state
//! down so the child can own the tty, then restoring our state when it
//! exits. The actual teardown helpers live in `main.rs` because they touch
//! the `Tui` value directly; this module supplies the policy (which binary,
//! which args, whether a file is viewable).

pub mod expand;

pub use expand::{expand_percent, shell_quote};

use std::io::Read;
use std::path::Path;

/// $EDITOR, fall back to $VISUAL, fall back to `vi`.
pub fn resolve_editor() -> Vec<String> {
    let raw = std::env::var("VISUAL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("EDITOR").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "vi".to_string());
    split_command(&raw)
}

/// $PAGER, fall back to `less`. Used by spyc's `p` binding to hand
/// the file off to a real pager (full TTY takeover via suspend_tui),
/// which is the right tool for huge files / interactive search /
/// line-folding-on-demand. Spyc's in-app pager remains the default
/// for normal viewing; `p` is the escape hatch.
pub fn resolve_pager() -> Vec<String> {
    let raw = std::env::var("PAGER")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "less".to_string());
    split_command(&raw)
}

/// Split an `$EDITOR`-style value into `[program, arg, arg, ...]` on
/// whitespace. This is what git does. People who need shell features set
/// `EDITOR` to a wrapper script.
fn split_command(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(ToString::to_string).collect()
}

/// Heuristic text/binary detection: look for a NUL byte in the first 8 KiB.
/// Matches what `grep` and `file` effectively do.
pub fn looks_like_text(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 8192];
    let Ok(read) = f.read(&mut buf) else {
        return false;
    };
    !buf[..read].contains(&0u8)
}
