//! Cross-platform clipboard helper for spyc's yank features.
//!
//! Exposes one public function: `copy(text)` writes `text` to the
//! system clipboard, fanning out to a platform-appropriate helper:
//!
//! - macOS → `pbcopy` (built-in).
//! - Linux → `wl-copy` if `$WAYLAND_DISPLAY` is set, then
//!   `xclip -selection clipboard`, then `xsel -ib`. Returns a clear
//!   `NotFound` error mentioning the installable helpers when none
//!   succeed.
//! - Other platforms → `Unsupported`.
//!
//! No external crate dependency — mirrors spyc's in-tree fork-exec
//! pattern (see `src/sysinfo.rs` for the same `cfg(target_os)` shape).

use std::io::{self, Write};
use std::process::{Command, Stdio};

#[cfg(test)]
thread_local! {
    /// Test-only override: when set, `copy` spawns this binary
    /// instead of resolving a platform clipboard helper. Lets unit
    /// tests inject a stub without mutating process-global env vars
    /// (the same trick `with_state_root` uses in `src/state/mod.rs`).
    static CLIPBOARD_OVERRIDE: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Test-only: run `body` with the clipboard helper pinned to `bin`.
/// The override is unwound when `body` returns *or panics* (RAII).
#[cfg(test)]
pub fn with_clipboard_override<R>(bin: &std::path::Path, body: impl FnOnce() -> R) -> R {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            CLIPBOARD_OVERRIDE.with(|c| *c.borrow_mut() = None);
        }
    }
    CLIPBOARD_OVERRIDE.with(|c| *c.borrow_mut() = Some(bin.to_path_buf()));
    let _g = Guard;
    body()
}

/// Write `text` to the system clipboard.
pub fn copy(text: &str) -> io::Result<()> {
    #[cfg(test)]
    {
        if let Some(p) = CLIPBOARD_OVERRIDE.with(|c| c.borrow().clone()) {
            return spawn_and_pipe(p.to_string_lossy().as_ref(), &[], text);
        }
    }
    copy_impl(text)
}

#[cfg(target_os = "macos")]
fn copy_impl(text: &str) -> io::Result<()> {
    spawn_and_pipe("pbcopy", &[], text)
}

#[cfg(target_os = "linux")]
fn copy_impl(text: &str) -> io::Result<()> {
    // ENOENT (helper not installed) → fall through to the next
    // candidate. Any other error from a helper that *did* run is
    // returned immediately so the user sees the real problem instead
    // of a generic "no clipboard helper available".
    let try_one = |prog: &str, args: &[&str]| -> Option<io::Result<()>> {
        match spawn_and_pipe(prog, args, text) {
            Ok(()) => Some(Ok(())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => Some(Err(e)),
        }
    };

    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if let Some(r) = try_one("wl-copy", &[]) {
            return r;
        }
    }
    if std::env::var_os("DISPLAY").is_some() {
        if let Some(r) = try_one("xclip", &["-selection", "clipboard"]) {
            return r;
        }
        if let Some(r) = try_one("xsel", &["-ib"]) {
            return r;
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "no clipboard helper available — install xclip, xsel, or wl-copy",
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn copy_impl(_text: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "clipboard not supported on this platform",
    ))
}

fn spawn_and_pipe(prog: &str, args: &[&str], text: &str) -> io::Result<()> {
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    child.wait()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(unix)]
    #[test]
    fn copy_via_override_writes_to_stub() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let stub = tmp.path().join("stub-clip.sh");
        let sidecar = tmp.path().join("captured.txt");
        fs::write(&stub, format!("#!/bin/sh\ncat > {}\n", sidecar.display())).expect("write stub");
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();

        with_clipboard_override(&stub, || copy("hello world\n"))
            .expect("copy via stub should succeed");

        let captured = fs::read_to_string(&sidecar).expect("read sidecar");
        assert_eq!(captured, "hello world\n");
    }

    #[test]
    fn spawn_and_pipe_returns_not_found_for_missing_binary() {
        let err = spawn_and_pipe("this-binary-does-not-exist-spyc-test", &[], "ignored")
            .expect_err("missing binary should error");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
