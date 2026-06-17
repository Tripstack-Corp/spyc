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
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    // `wait()?` only surfaces wait-syscall failure, not a non-zero
    // exit from the helper itself. xclip/wl-copy/xsel can launch
    // cleanly and then fail (no compositor, archived display, dbus
    // unreachable…) — we need to treat those as errors so the user
    // sees the real reason instead of a phantom "yanked" flash, and
    // so the Linux cascade doesn't get stuck on a present-but-broken
    // helper. ErrorKind::Other is deliberate: callers in this module
    // only fall through to the next candidate on `NotFound`, so
    // non-zero-exit failures stop the cascade and surface immediately.
    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "{prog} exited unsuccessfully: {status}"
        )));
    }
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
}
