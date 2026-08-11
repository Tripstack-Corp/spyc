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

use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(test)]
thread_local! {
    /// Test-only override: when set, `copy` spawns this binary
    /// instead of resolving a platform clipboard helper. Lets unit
    /// tests inject a stub without mutating process-global env vars
    /// (the same trick `with_state_root` uses in `src/state/mod.rs`).
    static CLIPBOARD_OVERRIDE: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Test-only stand-in for the clipboard: `paste` sleeps `delay`, then returns
/// `text` instead of spawning a helper. `delay` is what lets a test watch the
/// read happen *somewhere else* — see `App::spawn_clipboard_read`.
#[cfg(test)]
#[derive(Clone)]
struct PasteStub {
    delay: Duration,
    text: String,
}

#[cfg(test)]
static PASTE_STUB: std::sync::RwLock<Option<PasteStub>> = std::sync::RwLock::new(None);

/// Serializes stub installation. Unlike `CLIPBOARD_OVERRIDE`, this one cannot be
/// a thread-local: the read it stands in for runs on a worker thread, which is
/// exactly the property under test. Process-global state needs the lock so two
/// tests can't read each other's stub; nothing else in the suite calls `paste`.
#[cfg(test)]
static PASTE_STUB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Test-only: run `body` with [`paste`] returning `text` after `delay`.
#[cfg(test)]
pub fn with_paste_stub<R>(delay: Duration, text: &str, body: impl FnOnce() -> R) -> R {
    struct Guard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);
    impl Drop for Guard {
        fn drop(&mut self) {
            *PASTE_STUB
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    }
    // The guard takes the lock BEFORE the stub is installed and holds it for the
    // whole body, so a concurrent test can neither see nor replace this stub.
    let _g = Guard(
        PASTE_STUB_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    *PASTE_STUB
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(PasteStub {
        delay,
        text: text.to_string(),
    });
    body()
}

/// Test-only: run `body` with [`paste`] returning `text` immediately.
#[cfg(test)]
pub fn with_paste_override<R>(text: &str, body: impl FnOnce() -> R) -> R {
    with_paste_stub(Duration::ZERO, text, body)
}

#[cfg(test)]
thread_local! {
    /// Test-only override for [`HELPER_REAP_BUDGET`]. Production's budget is
    /// deliberately far too short to wait out a `/bin/sh` stub's fork+exec,
    /// which is right for a yank and useless for a test that has to observe the
    /// helper's exit status or the file it wrote. Third seam in this file's
    /// existing thread-local pattern.
    static REAP_BUDGET_OVERRIDE: std::cell::RefCell<Option<std::time::Duration>> =
        const { std::cell::RefCell::new(None) };
}

/// Test-only: run `body` with the reap budget pinned to `budget`.
#[cfg(test)]
pub fn with_reap_budget<R>(budget: std::time::Duration, body: impl FnOnce() -> R) -> R {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            REAP_BUDGET_OVERRIDE.with(|c| *c.borrow_mut() = None);
        }
    }
    REAP_BUDGET_OVERRIDE.with(|c| *c.borrow_mut() = Some(budget));
    let _g = Guard;
    body()
}

/// The bounded-wait budget [`spawn_and_pipe`] actually uses — the constant in
/// production, the thread-local override under test.
#[cfg(not(test))]
const fn reap_budget() -> Duration {
    HELPER_REAP_BUDGET
}

#[cfg(test)]
fn reap_budget() -> Duration {
    REAP_BUDGET_OVERRIDE
        .with(|c| *c.borrow())
        .unwrap_or(HELPER_REAP_BUDGET)
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
        // Clone out before sleeping — holding the read lock across the delay
        // would make the next test's stub install wait on this worker.
        let stub = PASTE_STUB
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(stub) = stub {
            std::thread::sleep(stub.delay);
            return Ok(stub.text);
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

/// How long a clipboard READ waits for its helper before killing it.
///
/// Generous where [`HELPER_REAP_BUDGET`] is stingy, because the two are paid in
/// different places: the write budget is spent on the event-loop thread, so a
/// whole second there would be its own bug, while the read runs on a worker
/// (`App::spawn_clipboard_read`) and the only cost of waiting is how late the
/// failure is reported. What this bounds is the *leak* — `xclip -o` blocks until
/// the selection owner transfers, and a wedged owner would otherwise pin a
/// thread and a child process for the rest of the session, one pair per middle
/// click.
const PASTE_READ_BUDGET: Duration = Duration::from_secs(5);

/// Read `pipe` to EOF on its own thread.
///
/// Both of the child's pipes must be drained *concurrently*: reading one to the
/// end while the other fills deadlocks, and a full pipe also stops the helper
/// exiting — which would read as a timeout on a perfectly healthy helper.
fn drain_pipe<R: Read + Send + 'static>(pipe: Option<R>) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    rx
}

/// Run `prog args` and return its stdout as a lossy `String`. A non-zero exit is
/// an error carrying the helper's stderr, so a broken `$DISPLAY` reads as itself
/// rather than as an empty paste.
///
/// Bounded by [`PASTE_READ_BUDGET`]: a helper that never answers is killed and
/// reported as `TimedOut` rather than waited out. `Command::output()` cannot do
/// this — it blocks until the child exits — so the wait is a `try_wait` poll
/// against a deadline, the same shape [`spawn_and_pipe`] uses for the write.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn capture(prog: &str, args: &[&str]) -> io::Result<String> {
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = drain_pipe(child.stdout.take());
    let stderr = drain_pipe(child.stderr.take());

    let budget = PASTE_READ_BUDGET;
    let deadline = Instant::now() + budget;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            // Kill and reap: the point of the budget is that neither the child
            // nor its reader threads outlive the read that started them.
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{prog} did not answer within {}s", budget.as_secs_f32()),
            ));
        }
        std::thread::sleep(HELPER_REAP_POLL_INTERVAL);
    };

    // The child is gone, so both pipes are at EOF and these land immediately —
    // bounded anyway, because a grandchild that inherited a pipe holds it open.
    let out = stdout.recv_timeout(budget).unwrap_or_default();
    if status.success() {
        return Ok(String::from_utf8_lossy(&out).into_owned());
    }
    let err = stderr.recv_timeout(budget).unwrap_or_default();
    let msg = String::from_utf8_lossy(&err).trim().to_string();
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

/// A clipboard image: PNG bytes plus its natural pixel size.
pub type ClipboardImage = (Vec<u8>, (u32, u32));

/// Read an image off the system clipboard, re-encoded as PNG.
///
/// `Ok(None)` means "the clipboard holds no image" — the overwhelmingly common
/// case when this fires on a paste keystroke, and not a failure worth telling
/// the user about.
///
/// **Runs on a worker, never the loop.** `arboard` hands back raw RGBA (33 MB
/// for a 4K screenshot), so this allocates heavily and then spends real time
/// encoding. Verified safe off the main thread on macOS: a clipboard image
/// round-trips exactly from a spawned thread, ~3 ms at 3840×2160 for the read.
///
/// Encoding uses fast compression deliberately — this is a transient preview
/// cache, and a smaller file isn't worth a second of a worker's time.
///
/// Over SSH this reads the *server's* clipboard, the same asymmetry OSC 52
/// exists to solve for the write direction. That's the right behavior here:
/// the agent running in the pane reads that same clipboard, so spyc sees
/// exactly what the agent saw.
pub fn read_image() -> Result<Option<ClipboardImage>, String> {
    let img = match arboard::Clipboard::new().and_then(|mut cb| cb.get_image()) {
        Ok(img) => img,
        // Every "nothing usable there" shape — no image, empty clipboard, an
        // unsupported flavor — is the same non-event to the caller.
        Err(arboard::Error::ContentNotAvailable | arboard::Error::ClipboardNotSupported) => {
            return Ok(None);
        }
        Err(e) => return Err(format!("clipboard: {e}")),
    };
    let (w, h) = (
        u32::try_from(img.width).map_err(|_| "clipboard image too wide".to_string())?,
        u32::try_from(img.height).map_err(|_| "clipboard image too tall".to_string())?,
    );
    let buf: image::RgbaImage = image::ImageBuffer::from_raw(w, h, img.bytes.into_owned())
        .ok_or_else(|| "clipboard image dimensions don't match its bytes".to_string())?;
    let mut out = std::io::Cursor::new(Vec::new());
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new_with_quality(
            &mut out,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::NoFilter,
        ),
        &buf,
        w,
        h,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| format!("png encode: {e}"))?;
    Ok(Some((out.into_inner(), (w, h))))
}

/// The largest payload spyc will hand to OSC 52.
///
/// xterm's own limit is ~74 994 bytes of base64 and several terminals inherit it;
/// tmux caps at 1 MB only with `set-clipboard on`. A payload over the limit is
/// silently *truncated* by some terminals and dropped entirely by others, and a
/// half-pasted selection is worse than a clear failure — so past this we fall back
/// to the local helper rather than gamble.
const OSC52_MAX_BASE64: usize = 74_994;

/// Ask the TERMINAL to set the clipboard, via OSC 52.
///
/// This is the half that works over SSH: the escape travels back up the same
/// connection the UI is drawn on, so the text lands on the clipboard of the machine
/// the user is actually typing at. `pbcopy`/`xclip` set the clipboard of whatever
/// host spyc runs on, which over SSH is the *server* — text the user can never
/// paste.
///
/// `Err` when the payload is too large to send safely; the caller falls back.
/// Success here means "the sequence was written", not "the terminal honored it":
/// OSC 52 is write-only with no reply, and support varies (kitty/WezTerm/iTerm2/
/// Ghostty/Alacritty yes; tmux needs `set -g set-clipboard on`; some terminals gate
/// it deliberately, since a remote host writing your clipboard is a real risk).
/// That unverifiability is exactly why `Auto` still prefers the local helper when
/// there's no SSH session to justify the trade.
pub fn copy_osc52(text: &str) -> Result<(), String> {
    let seq = osc52_sequence(text, std::env::var_os("TMUX").is_some())?;
    let mut out = io::stdout();
    out.write_all(seq.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|e| format!("osc52: {e}"))
}

/// Build the OSC 52 byte sequence for `text`. Pure — `in_tmux` is passed so tests
/// exercise both modes without touching the process-global `TMUX`, the
/// `term_title::wrap` / `notifications::osc9_sequence` template.
///
/// No sanitizing step is needed, unlike OSC 9's message: the payload is base64, so
/// it cannot contain an `\x1b` or `\x07` to close the sequence early and inject
/// escapes. Encoding *is* the escaping — which is why the raw text is never written
/// through.
fn osc52_sequence(text: &str, in_tmux: bool) -> Result<String, String> {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    if encoded.len() > OSC52_MAX_BASE64 {
        return Err(format!(
            "selection too large for OSC 52 ({} bytes base64, limit {OSC52_MAX_BASE64})",
            encoded.len()
        ));
    }
    // `c` = the CLIPBOARD selection (not the X11 primary). BEL-terminated, which
    // more terminals accept than ST.
    let inner = format!("\x1b]52;c;{encoded}\x07");
    Ok(if in_tmux {
        // tmux eats escapes aimed at itself; DCS passthrough forwards them to the
        // outer terminal, with inner ESCs doubled.
        format!("\x1bPtmux;{}\x1b\\", inner.replace('\x1b', "\x1b\x1b"))
    } else {
        inner
    })
}

/// Resolve a user clipboard-command override: `$SPYC_CLIPBOARD` if set and
/// non-empty, else `config_command` if non-empty, else `None`. Env wins over
/// config, matching how other spyc envs layer over static config.
///
/// `copy()` calls this with `config_command: None` — it's a leaf module (see
/// AGENTS.md's "dependency direction one-way": `app` depends on this module,
/// never the reverse), so it can honor `$SPYC_CLIPBOARD` but not
/// `[clipboard].command`. `deliver_clipboard` (which does have config access)
/// passes the config value through, so both sources are checked exactly once
/// from there.
pub fn resolve_override(config_command: Option<&str>) -> Option<String> {
    crate::envset::var("SPYC_CLIPBOARD")
        .filter(|s| !s.is_empty())
        .or_else(|| {
            config_command
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
        })
}

/// Run a user-supplied clipboard command verbatim and pipe `text` to its
/// stdin. Whitespace-split into argv — no shell features, same contract as
/// `$EDITOR`/`$PAGER` resolution in `src/shell/mod.rs` (wrap it in a script if
/// you need pipes/redirection/etc).
pub fn copy_via_user_command(cmd: &str, text: &str) -> io::Result<()> {
    let mut parts = cmd.split_whitespace();
    let Some(prog) = parts.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "clipboard command is empty",
        ));
    };
    let args: Vec<&str> = parts.collect();
    spawn_and_pipe(prog, &args, text)
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
    if let Some(cmd) = resolve_override(None) {
        return copy_via_user_command(&cmd, text);
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

/// How long spawn_and_pipe will block waiting for a helper to exit before
/// treating "still running" as success and detaching a reaper thread.
/// xclip/xsel legitimately persist after a successful copy to keep serving
/// the X11 selection until another app claims it — that is NOT a hang, and
/// `Child::wait()` would block the caller (the event-loop thread, since
/// Effect::CopyToClipboard runs inline in run_effects) for as long as the
/// selection goes unclaimed, sometimes indefinitely. A genuine launch
/// failure (bad $DISPLAY, missing libX11) exits within this window.
///
/// Deliberately short, because this budget is paid on the event-loop thread
/// **every** time an X11 helper persists — i.e. on every single yank under
/// `xclip`/`xsel`, which is the common case on that platform, not an edge one.
/// A whole-second stall there would be a worse bug than the one this bounding
/// exists to fix.
///
/// The cost of being short: a helper that launches, then fails *slower* than
/// this, is reported as success (the user sees "yanked" and gets nothing on
/// the clipboard). Accepted, because the fast-failure case this does catch —
/// `xclip` with a bad `$DISPLAY` exits in single-digit ms — is the one that
/// actually happens, and a broken clipboard announces itself at the next
/// paste. The real fix for both halves is moving the write off-thread
/// entirely (AGENTS.md's `graveyard_ops` template), where no budget is needed.
const HELPER_REAP_BUDGET: Duration = Duration::from_millis(150);

/// Poll granularity for the bounded wait below — small enough that the
/// common case (helper exits almost immediately) doesn't add perceptible
/// latency to a yank.
const HELPER_REAP_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
    // handle without reaping it, leaking a zombie (the bug this guards).
    // Capture the result and reap below instead.
    let write_result = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(text.as_bytes()),
        None => Ok(()),
    };

    // Poll with `try_wait()` rather than block on `wait()` — see
    // HELPER_REAP_BUDGET. A genuine launch failure exits promptly; a helper
    // still serving the selection past the budget is not our problem to wait
    // out.
    let deadline = Instant::now() + reap_budget();
    let mut exited = None;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait()? {
            exited = Some(status);
            break;
        }
        std::thread::sleep(HELPER_REAP_POLL_INTERVAL);
    }

    let Some(status) = exited else {
        // Still running past the budget: treat as success and detach a
        // reaper so it's still cleaned up (no zombie) whenever it does
        // eventually exit. Nobody is waiting on that outcome, so no
        // Message/feedback is needed.
        write_result?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        return Ok(());
    };

    // A non-zero exit means xclip/wl-copy/xsel launched cleanly and then
    // failed (no compositor, archived display, dbus unreachable…) — treat it
    // as an error so the user sees the real reason instead of a phantom
    // "yanked" flash, and so the Linux cascade doesn't get stuck on a
    // present-but-broken helper. The exit status is the more informative
    // signal, so it takes precedence over a stdin-write error (e.g. an EPIPE
    // from a helper that bailed before reading). ErrorKind::Other is
    // deliberate: callers only fall through on `NotFound`, so a non-zero exit
    // stops the cascade and surfaces immediately.
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

    /// Generous budget for tests that must observe the helper's exit status or
    /// the file it wrote. Production's is deliberately too short for that (see
    /// `HELPER_REAP_BUDGET`), so those tests say so explicitly rather than
    /// racing a stub's fork+exec under a loaded parallel run.
    const TEST_REAP_BUDGET: Duration = Duration::from_secs(10);

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
        assert!(
            PASTE_STUB
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none()
        );
    }

    /// A clipboard helper that never answers must be given up on, not waited out.
    ///
    /// `xclip -o` blocks until the selection owner transfers; a wedged owner, a
    /// stalled compositor, or a hung macOS pasteboard server means it never does.
    /// The read has no business outliving the user's patience regardless of which
    /// thread it runs on.
    #[cfg(unix)]
    #[test]
    fn a_wedged_paste_helper_gives_up_instead_of_waiting_forever() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let stub = tmp.path().join("stub-wedged.sh");
        // Never writes, never exits — the shape of a helper waiting on a
        // selection transfer that will not come.
        fs::write(&stub, "#!/bin/sh\nsleep 30\n").expect("write stub");
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();

        let start = Instant::now();
        let err = retry_text_busy(|| capture(&stub.display().to_string(), &[]))
            .expect_err("a helper that never answers must surface an error");
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the read waited {:?} on a wedged helper",
            start.elapsed()
        );
        assert_eq!(err.kind(), io::ErrorKind::TimedOut, "got {err:?}");
    }

    /// Run `f`, retrying only while the OS reports `ETXTBSY` ("Text file busy").
    ///
    /// These tests write a stub script and execute it microseconds later. If another
    /// test thread forks in that window — the suite runs in parallel and plenty of
    /// tests spawn processes — the child inherits the still-open writable fd and
    /// holds a dup across its own exec, so ours is refused
    /// (rust-lang/rust#74253). Nothing under test is racy: the *fixture* is, and the
    /// window is microseconds, so a bounded retry beats serializing the suite.
    /// Every other outcome, success or failure, is returned untouched — the tests
    /// that assert a specific error still see it on the first attempt.
    #[cfg(unix)]
    fn retry_text_busy<T>(mut f: impl FnMut() -> io::Result<T>) -> io::Result<T> {
        let busy = rustix::io::Errno::TXTBSY.raw_os_error();
        for _ in 0..50 {
            match f() {
                Err(e) if e.raw_os_error() == Some(busy) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                other => return other,
            }
        }
        f()
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

        // Long budget: this test reads the file the stub writes, so it needs
        // spawn_and_pipe to have actually waited for the child.
        with_reap_budget(TEST_REAP_BUDGET, || {
            retry_text_busy(|| with_clipboard_override(&stub, || copy("hello world\n")))
                .expect("copy via stub should succeed");
        });

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

        let err = with_reap_budget(TEST_REAP_BUDGET, || {
            retry_text_busy(|| with_clipboard_override(&stub, || copy("ignored")))
                .expect_err("non-zero exit should surface as error")
        });
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
        let err = retry_text_busy(|| with_clipboard_override(&stub, || copy(&big)))
            .expect_err("a helper that ignores a large stdin should surface a write error");
        assert_eq!(err.kind(), io::ErrorKind::BrokenPipe, "got {err:?}");
    }

    /// `src/envset.rs` has no thread-local test seam (its overrides are a
    /// process-global `RwLock`, by design — they're meant to layer like the
    /// real environment). All three precedence cases live in ONE test and run
    /// in a fixed order, so this is the only place in the suite that ever
    /// touches `SPYC_CLIPBOARD` and there's no cross-test ordering hazard from
    /// the lack of an "unset" primitive.
    #[test]
    fn resolve_override_prefers_env_over_config_over_none() {
        assert_eq!(resolve_override(None), None, "nothing set");
        assert_eq!(
            resolve_override(Some("cmd-from-config")),
            Some("cmd-from-config".to_string()),
            "config used when env absent"
        );
        assert_eq!(
            resolve_override(Some("")),
            None,
            "an empty config value doesn't count as set"
        );

        crate::envset::set("SPYC_CLIPBOARD", "cmd-from-env");
        assert_eq!(
            resolve_override(Some("cmd-from-config")),
            Some("cmd-from-env".to_string()),
            "env wins over config"
        );
        assert_eq!(
            resolve_override(None),
            Some("cmd-from-env".to_string()),
            "env alone is enough"
        );

        // An empty env override doesn't shadow a real config value — same
        // "filtered non-empty" rule applies to both sources.
        crate::envset::set("SPYC_CLIPBOARD", "");
        assert_eq!(
            resolve_override(Some("cmd-from-config")),
            Some("cmd-from-config".to_string()),
            "empty env falls through to config"
        );
    }

    #[cfg(unix)]
    #[test]
    fn copy_via_user_command_splits_on_whitespace_and_args_reach_the_child() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let stub = tmp.path().join("stub-args.sh");
        let sidecar = tmp.path().join("captured.txt");
        // Echo argv (not stdin) so the test can see the split actually
        // happened, then drain stdin so the caller's write doesn't EPIPE.
        fs::write(
            &stub,
            format!(
                "#!/bin/sh\necho \"$1|$2\" > {}\ncat > /dev/null\n",
                sidecar.display()
            ),
        )
        .expect("write stub");
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();

        let cmd = format!("{} first second", stub.display());
        with_reap_budget(TEST_REAP_BUDGET, || {
            retry_text_busy(|| copy_via_user_command(&cmd, "hello")).expect("stub should succeed");
        });

        let captured = fs::read_to_string(&sidecar).expect("read sidecar");
        assert_eq!(captured.trim(), "first|second");
    }

    #[cfg(unix)]
    #[test]
    fn copy_via_user_command_pipes_text_to_stdin() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let stub = tmp.path().join("stub-stdin.sh");
        let sidecar = tmp.path().join("captured.txt");
        fs::write(&stub, format!("#!/bin/sh\ncat > {}\n", sidecar.display())).expect("write stub");
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();

        with_reap_budget(TEST_REAP_BUDGET, || {
            retry_text_busy(|| copy_via_user_command(&stub.display().to_string(), "hello world\n"))
                .expect("stub should succeed");
        });

        let captured = fs::read_to_string(&sidecar).expect("read sidecar");
        assert_eq!(captured, "hello world\n");
    }

    #[cfg(unix)]
    #[test]
    fn copy_via_user_command_propagates_non_zero_exit() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let stub = tmp.path().join("stub-user-fail.sh");
        fs::write(&stub, "#!/bin/sh\ncat > /dev/null\nexit 1\n").expect("write stub");
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();

        let err = with_reap_budget(TEST_REAP_BUDGET, || {
            retry_text_busy(|| copy_via_user_command(&stub.display().to_string(), "ignored"))
                .expect_err("non-zero exit should surface as error")
        });
        assert!(
            err.to_string().contains("exited unsuccessfully"),
            "got: {err}"
        );
    }

    /// Pins "detach, don't wait" as the actual executed behavior, not just an
    /// assertion of intent: a helper that keeps running well past
    /// HELPER_REAP_BUDGET (mirrors xclip/xsel serving the X11 selection after
    /// a successful copy) must not make `spawn_and_pipe` block for anywhere
    /// close to its full lifetime.
    #[cfg(unix)]
    #[test]
    fn spawn_and_pipe_detaches_a_persisting_child_instead_of_blocking() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let stub = tmp.path().join("stub-persist.sh");
        // Read stdin first so the write doesn't EPIPE, THEN keep running well
        // past the reap budget — the leftover process is intentionally
        // abandoned; the test asserts we didn't wait on it, not that it's
        // gone by the time the test returns.
        fs::write(&stub, "#!/bin/sh\ncat > /dev/null\nsleep 10\n").expect("write stub");
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();

        // Bounded well above HELPER_REAP_BUDGET + fork/exec + poll-interval
        // slop, but far below the stub's 10s sleep — proves we detached rather
        // than waited, without being sensitive to scheduling jitter under a
        // loaded/parallel test run.
        let start = Instant::now();
        with_reap_budget(Duration::from_millis(100), || {
            retry_text_busy(|| with_clipboard_override(&stub, || copy("hello")))
                .expect("a persisting-but-successful helper must read as Ok, not an error");
        });
        assert!(
            start.elapsed() < Duration::from_secs(4),
            "spawn_and_pipe blocked for {:?}, should have detached at ~{:?}",
            start.elapsed(),
            HELPER_REAP_BUDGET
        );
    }
}

#[cfg(test)]
mod osc52_tests {
    use super::{OSC52_MAX_BASE64, osc52_sequence};

    #[test]
    fn plain_sequence_is_osc52_clipboard_base64_bel() {
        let seq = osc52_sequence("hi", false).expect("small payload");
        // "hi" -> aGk=
        assert_eq!(seq, "\x1b]52;c;aGk=\x07");
    }

    /// Inside tmux the sequence must be DCS-wrapped with inner ESCs doubled, or tmux
    /// consumes it instead of forwarding to the outer terminal.
    #[test]
    fn tmux_wraps_in_dcs_passthrough_with_doubled_escapes() {
        let seq = osc52_sequence("hi", true).expect("small payload");
        assert!(seq.starts_with("\x1bPtmux;"), "got {seq:?}");
        assert!(seq.ends_with("\x1b\\"), "got {seq:?}");
        assert!(
            seq.contains("\x1b\x1b]52;c;aGk="),
            "inner ESC must be doubled: {seq:?}"
        );
    }

    /// **Encoding is the escaping.** Text containing the OSC terminators — the
    /// injection vector that forces `notifications::osc9_sequence` to strip control
    /// chars — cannot break out here, because everything between `52;c;` and the
    /// terminator is base64 and its alphabet excludes ESC and BEL.
    #[test]
    fn control_chars_in_the_text_cannot_terminate_the_sequence() {
        let hostile = "\x1b]52;c;evil\x07 and \x1b\\ more";
        let seq = osc52_sequence(hostile, false).expect("small payload");
        let body = seq
            .strip_prefix("\x1b]52;c;")
            .and_then(|r| r.strip_suffix('\x07'))
            .expect("well-formed wrapper");
        assert!(
            body.chars()
                .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c)),
            "payload must be pure base64, got {body:?}"
        );
        // Exactly one terminator: the one we wrote.
        assert_eq!(seq.matches('\x07').count(), 1);
    }

    /// Round-trips, so the terminal receives what the user selected.
    #[test]
    fn payload_decodes_back_to_the_original_text() {
        use base64::Engine as _;
        let text = "multi\nline\tselection — ünïcode 中";
        let seq = osc52_sequence(text, false).expect("small payload");
        let body = seq
            .strip_prefix("\x1b]52;c;")
            .and_then(|r| r.strip_suffix('\x07'))
            .expect("well-formed");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(body)
            .expect("valid base64");
        assert_eq!(String::from_utf8(decoded).expect("utf8"), text);
    }

    /// Over-limit payloads must ERROR rather than be sent: several terminals
    /// silently truncate an oversized OSC 52, and half a selection on the clipboard
    /// is worse than a reported failure (the caller falls back to the local helper).
    #[test]
    fn an_oversized_payload_is_refused_not_truncated() {
        // 3 bytes -> 4 base64 chars, so this comfortably exceeds the cap.
        let big = "x".repeat(OSC52_MAX_BASE64);
        let err = osc52_sequence(&big, false).expect_err("must refuse");
        assert!(err.contains("too large"), "got {err:?}");
    }

    /// And a payload just under the cap still goes out — the guard must not be so
    /// conservative that ordinary selections start failing.
    #[test]
    fn a_payload_just_under_the_cap_is_sent() {
        // 3 raw bytes encode to exactly 4 base64 chars, no padding growth.
        let raw = (OSC52_MAX_BASE64 / 4) * 3;
        let text = "y".repeat(raw);
        assert!(osc52_sequence(&text, false).is_ok());
    }
}
