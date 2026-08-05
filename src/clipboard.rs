//! Cross-platform clipboard helper for spyc's yank features.
//!
//! `copy(text)` writes to the system clipboard and `paste()` reads back from it,
//! each fanning out to a platform-appropriate helper:
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

#[cfg(test)]
thread_local! {
    /// Test-only override: when set, `paste` returns this text instead of
    /// spawning a helper. Mirrors `CLIPBOARD_OVERRIDE`, but a value rather than a
    /// binary — the read has nothing to pipe *into*, so a stub script would only
    /// be testing `capture`.
    static PASTE_OVERRIDE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// Test-only: run `body` with [`paste`] returning `text`.
#[cfg(test)]
pub fn with_paste_override<R>(text: &str, body: impl FnOnce() -> R) -> R {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            PASTE_OVERRIDE.with(|c| *c.borrow_mut() = None);
        }
    }
    PASTE_OVERRIDE.with(|c| *c.borrow_mut() = Some(text.to_string()));
    let _g = Guard;
    body()
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

/// Read the system clipboard as text — the middle-click paste source.
///
/// Shell-based for the same reason [`copy`] is (no crate dependency, mirroring
/// the in-tree fork-exec pattern), and the helper list is `copy`'s in reverse:
/// `pbpaste` on macOS; `wl-paste`/`xclip -o`/`xsel -ob` on Linux, tried in the
/// same order so a session that copies with one tool pastes with it too.
///
/// A spawn that reports `NotFound` falls through to the next candidate; any other
/// error from a helper that *did* run is returned, so the user sees the real
/// problem rather than a generic "no clipboard helper".
pub fn paste() -> io::Result<String> {
    #[cfg(test)]
    {
        if let Some(text) = PASTE_OVERRIDE.with(|c| c.borrow().clone()) {
            return Ok(text);
        }
    }
    paste_impl()
}

#[cfg(target_os = "macos")]
fn paste_impl() -> io::Result<String> {
    capture("pbpaste", &[])
}

#[cfg(target_os = "linux")]
fn paste_impl() -> io::Result<String> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let mut candidates: Vec<(&str, &[&str])> = Vec::new();
    if wayland {
        // `-n` so wl-paste doesn't append a newline the user never copied.
        candidates.push(("wl-paste", &["-n"]));
    }
    candidates.push(("xclip", &["-selection", "clipboard", "-o"]));
    candidates.push(("xsel", &["-ob"]));

    let mut last: Option<io::Error> = None;
    for (prog, args) in candidates {
        match capture(prog, args) {
            Ok(text) => return Ok(text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no clipboard helper found (install wl-clipboard, xclip, or xsel)",
        )
    }))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn paste_impl() -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "clipboard paste is not supported on this platform",
    ))
}

/// Run `prog args` and return its stdout as a lossy `String`. A non-zero exit is
/// an error carrying the helper's stderr, so a broken `$DISPLAY` reads as itself
/// rather than as an empty paste.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn capture(prog: &str, args: &[&str]) -> io::Result<String> {
    let out = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(io::Error::other(if msg.is_empty() {
        format!("{prog} failed")
    } else {
        format!("{prog}: {msg}")
    }))
}

/// Copy a PNG image to the system clipboard (the image-pager `y` verb).
///
/// Text (below) stays shell-based, but there's no portable shell helper for
/// image clipboard, so this uses `arboard`. macOS works cleanly; on Linux the
/// image is only held while spyc runs (X11/Wayland clipboards don't persist
/// after the owning process exits without a clipboard manager).
pub fn copy_image(png: &[u8]) -> Result<(), String> {
    let img = image::load_from_memory(png)
        .map_err(|e| format!("decode: {e}"))?
        .to_rgba8();
    let (width, height) = (img.width() as usize, img.height() as usize);
    let data = arboard::ImageData {
        width,
        height,
        bytes: std::borrow::Cow::Owned(img.into_raw()),
    };
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.set_image(data))
        .map_err(|e| format!("clipboard: {e}"))
}

/// Write `text` to the system clipboard.
pub fn copy(text: &str) -> io::Result<()> {
    #[cfg(test)]
    {
        if let Some(p) = CLIPBOARD_OVERRIDE.with(|c| c.borrow().clone()) {
            // Route the override through `/bin/sh <script>` rather
            // than execve'ing the script directly. Direct exec of a
            // just-written file intermittently trips
            // `Text file busy (os error 26)` on Linux even after
            // `fs::write` has returned — the kernel can still hold a
            // writer reference long enough to race the next exec.
            // sh opens the file for reading, so the busy-text race
            // goes away.
            let path = p.to_string_lossy().into_owned();
            return spawn_and_pipe("/bin/sh", &[path.as_str()], text);
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

    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        && let Some(r) = try_one("wl-copy", &[])
    {
        return r;
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
    // Null out stdout/stderr: spyc runs in raw-mode alternate-screen, so
    // anything a helper prints (xclip usage text, a wl-copy warning) would
    // scribble over the TUI. We surface failures via the exit status below,
    // so we don't need the helper's own stderr.
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Write the payload, then drop stdin so the helper sees EOF — but do NOT
    // early-return on a write error: a bare `?` here would drop the child
    // handle without `wait()`, leaking a zombie (the bug this guards). Capture
    // the result and reap the child first.
    let write_result = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(text.as_bytes()),
        None => Ok(()),
    };
    // `wait()` reaps the child (no zombie) and only surfaces wait-syscall
    // failure, not a non-zero exit. xclip/wl-copy/xsel can launch cleanly and
    // then fail (no compositor, archived display, dbus unreachable…) — treat a
    // non-zero exit as an error so the user sees the real reason instead of a
    // phantom "yanked" flash, and so the Linux cascade doesn't get stuck on a
    // present-but-broken helper. The exit status is the more informative
    // signal, so it takes precedence over a stdin-write error (e.g. an EPIPE
    // from a helper that bailed before reading). ErrorKind::Other is
    // deliberate: callers only fall through on `NotFound`, so a non-zero exit
    // stops the cascade and surfaces immediately.
    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "{prog} exited unsuccessfully: {status}"
        )));
    }
    // Helper succeeded but our write didn't complete → the clipboard wasn't
    // set; report it rather than flash a false "yanked".
    write_result?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn paste_via_override_returns_the_injected_text() {
        with_paste_override("hello \u{1f336}", || {
            assert_eq!(paste().expect("override never fails"), "hello \u{1f336}");
        });
    }

    /// The override unwinds even on panic (RAII), so one test can't leak clipboard
    /// state into the next — the reason `with_clipboard_override` is shaped this
    /// way too.
    #[test]
    fn paste_override_unwinds_after_the_body() {
        with_paste_override("x", || {});
        // Outside the override the real helper runs; we only assert the override
        // is gone, not what the host clipboard holds (that's the user's).
        PASTE_OVERRIDE.with(|c| assert!(c.borrow().is_none()));
    }

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

    #[cfg(unix)]
    #[test]
    fn copy_via_override_propagates_non_zero_exit() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let stub = tmp.path().join("stub-fail.sh");
        // Drain stdin so spyc's `write_all` doesn't fail with EPIPE
        // before the helper exits — we want to exercise the
        // *exit-status* path, not the stdin-broken-pipe path.
        fs::write(&stub, "#!/bin/sh\ncat > /dev/null\nexit 1\n").expect("write stub");
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();

        let err = with_clipboard_override(&stub, || copy("ignored"))
            .expect_err("non-zero exit should surface as error");
        // Crucially NOT NotFound — the Linux cascade falls through
        // only on NotFound, so a present-but-failing helper must
        // produce a different ErrorKind to halt the cascade.
        assert_ne!(err.kind(), io::ErrorKind::NotFound);
        assert!(
            err.to_string().contains("exited unsuccessfully"),
            "error message should mention non-zero exit, got: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn copy_reaps_child_and_errors_when_helper_ignores_large_stdin() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let stub = tmp.path().join("stub-ignore.sh");
        // Helper exits 0 WITHOUT reading stdin. A payload larger than the pipe
        // buffer then overflows, and our `write_all` hits EPIPE once the reader
        // is gone. The fix must still reap the child (no zombie), must not hang,
        // and must surface the write failure rather than flash a false "yanked".
        fs::write(&stub, "#!/bin/sh\nexit 0\n").expect("write stub");
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();

        let big = "x".repeat(512 * 1024); // >> any pipe buffer
        let err = with_clipboard_override(&stub, || copy(&big))
            .expect_err("a helper that ignores a large stdin should surface a write error");
        assert_eq!(err.kind(), io::ErrorKind::BrokenPipe, "got {err:?}");
    }
}
