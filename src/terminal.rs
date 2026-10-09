//! The host terminal: entering and leaving the TUI, handing the tty to a
//! foreground child and back, the mouse-reporting modes, and the signal
//! handlers that restore all of it when spyc is killed.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    event::{
        DisableBracketedPaste, EnableBracketedPaste, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode, supports_keyboard_enhancement,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::term_title;

pub type Tui = Terminal<CrosstermBackend<io::Stdout>>;

/// Hide the mouse pointer while the TUI is active. Uses the "pointer
/// mode" extension supported by xterm, iTerm2, Kitty, WezTerm, and
/// most modern terminals. Terminals that don't recognize it silently
/// ignore the sequence.
struct HideMousePointer;
struct ShowMousePointer;

impl crossterm::Command for HideMousePointer {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        // XTSMPOINTER: set pointer mode to 0 (hide when typing).
        // Widely supported; ignored by terminals that don't know it.
        f.write_str("\x1b[>1p")
    }
    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        Ok(())
    }
}

impl crossterm::Command for ShowMousePointer {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[>0p")
    }
    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        Ok(())
    }
}

/// DEC private mode 1007: translate scroll-wheel into arrow keys while in the
/// alternate screen. This prevents the terminal from scrolling its main
/// scrollback buffer without capturing mouse clicks/drags (text selection
/// still works normally).
struct EnableAlternateScroll;
struct DisableAlternateScroll;

impl crossterm::Command for EnableAlternateScroll {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1007h")
    }
    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        Ok(())
    }
}

impl crossterm::Command for DisableAlternateScroll {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1007l")
    }
    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        Ok(())
    }
}

/// DEC private modes 1000 (button press/release), 1002 (button-event tracking)
/// and 1006 (SGR extended coordinates) — real mouse reporting, so spyc gets
/// `ScrollUp`/`ScrollDown`, button and drag events *with coordinates* instead of
/// 1007's coordinate-free arrow keys.
///
/// **1002 yes, 1003 emphatically no**, and the difference is the whole reason
/// drags are affordable. 1002 reports motion only while a button is HELD; 1003
/// (any-event) reports every pointer move. `run.rs` marks a redraw for every
/// `Message::Input` and `coalesce_pending` surfaces one per loop iteration, so
/// 1003 turns idle pointer movement into a redraw-per-motion storm — straight
/// through the 0-dps-at-idle invariant. Under 1002 an untouched mouse generates
/// nothing at all, and the redraws during a drag are the ones a drag needs.
/// `crossterm::event::EnableMouseCapture` emits 1003, which is why spyc doesn't
/// use it (guarded by `production_code_never_uses_crossterms_mouse_capture`).
///
/// 1006h keeps coordinates correct past column 223. Mutually exclusive with
/// [`EnableAlternateScroll`]: a terminal honouring both could deliver one tick
/// twice (once as arrows, once as a mouse event), so the two are always toggled
/// as a pair.
struct EnableWheelReporting;
struct DisableWheelReporting;

impl crossterm::Command for EnableWheelReporting {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1000h\x1b[?1002h\x1b[?1006h")
    }
    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        Ok(())
    }
}

impl crossterm::Command for DisableWheelReporting {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        // Reverse order of the enable, so a terminal that tracks a mode stack
        // unwinds cleanly.
        //
        // Clears 1003, which spyc never enables: a foreground child (vim, htop)
        // sets its own any-motion reporting, and one that dies without resetting
        // — SIGKILL, a crash — hands the tty back with it still on. Clearing only
        // what spyc turned on would leave motion reporting live, which both
        // defeats the 1007 exclusivity `resume_tui` re-establishes and leaks
        // motion events into the user's shell after spyc exits.
        f.write_str("\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l")
    }
    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Whether the TERMINAL is currently in real mouse reporting — as distinct from
/// whether the user asked for it (`[mouse] capture` + the `:mouse` override).
///
/// A process-global rather than a field on `ViewState`, because two of the three
/// things that change this state run outside `App` and cannot reach its fields:
/// [`restore_terminal`] and the **panic hook**. The hook is not exit-only —
/// `pane::parser_worker` deliberately `catch_unwind`s a parser panic on
/// untrusted child output (vt100 0.16.2 has four such classes reachable at
/// spyc's geometry clamp; `Cargo.toml`'s `[profile.release]` lists them) and
/// spyc keeps running — so
/// with the flag on `ViewState` that path disabled reporting at the terminal
/// while `App` still believed it was on. `settle_mouse_mode` then saw no
/// divergence and never re-enabled: the mouse was dead for the rest of the
/// session, and `:mouse` reported "on" because want matched the stale actual.
static MOUSE_CAPTURE_ON: AtomicBool = AtomicBool::new(false);

/// Read the terminal's actual mouse-reporting state. The `settle_mouse_mode`
/// reconcile compares this against what the user asked for.
pub fn mouse_capture_is_on() -> bool {
    MOUSE_CAPTURE_ON.load(Ordering::Relaxed)
}

/// Set the terminal-state flag WITHOUT touching the terminal — tests only.
///
/// Lets a unit test stand in for the executor / panic hook / `suspend_tui`, none
/// of which a test can drive (they need a real `Tui`).
#[cfg(test)]
pub fn set_mouse_capture_for_test(on: bool) {
    MOUSE_CAPTURE_ON.store(on, Ordering::Relaxed);
}

/// Serialize the tests that drive [`MOUSE_CAPTURE_ON`].
///
/// It's process-global and `cargo test` runs tests on parallel threads, so
/// without this they clobber each other's setup — an intermittent failure that
/// depends on scheduling, which is the worst kind to chase (see the CI-only gix
/// index-lock flake for the same lesson). Recovers from a poisoned lock so one
/// failing test doesn't cascade into every other test in the group.
#[cfg(test)]
pub fn mouse_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The escape sequence [`set_mouse_capture`] emits for `capture`.
///
/// Extracted from the `execute!` so the exclusivity invariant is unit-testable:
/// `set_mouse_capture` takes `&mut Tui`, which no test can construct, so before
/// this the one rule the whole feature rests on — 1007 and 1000 are never both
/// enabled — had no test at all.
fn mouse_mode_seq(capture: bool) -> String {
    let mut s = String::new();
    if capture {
        let _ = crossterm::Command::write_ansi(&DisableAlternateScroll, &mut s);
        let _ = crossterm::Command::write_ansi(&EnableWheelReporting, &mut s);
    } else {
        let _ = crossterm::Command::write_ansi(&DisableWheelReporting, &mut s);
        let _ = crossterm::Command::write_ansi(&EnableAlternateScroll, &mut s);
    }
    s
}

/// Everything [`restore_terminal`] writes, precomputed at startup so the
/// SIGTERM/SIGHUP handler can restore the terminal with one `write`.
///
/// SPYC-TRAP(signal-teardown-precomputed): built here, never inside the
/// handler. A signal handler may only call async-signal-safe functions, and
/// building this calls into crossterm's formatting and reads `$TMUX` — neither
/// is safe there. Nothing but `write` + `tcsetattr` + `_exit` may run in the
/// handler itself.
static RESTORE_SEQ: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The pre-raw-mode terminal settings, captured before `enable_raw_mode`.
///
/// crossterm keeps its own copy but doesn't expose it, and `disable_raw_mode`
/// takes an internal lock — not usable from a handler.
static ORIGINAL_TERMIOS: std::sync::OnceLock<libc::termios> = std::sync::OnceLock::new();

/// The escape sequence that undoes [`setup_terminal`].
///
/// Extracted from `restore_terminal`'s `execute!` for the same reason
/// [`mouse_mode_seq`] was: `restore_terminal` takes `&mut Tui`, which no test
/// can construct, so the byte-level contract had no test. Order mirrors
/// `restore_terminal` exactly.
fn terminal_restore_seq() -> String {
    use crossterm::Command as _;
    let mut s = String::new();
    let _ = PopKeyboardEnhancementFlags.write_ansi(&mut s);
    let _ = LeaveAlternateScreen.write_ansi(&mut s);
    let _ = DisableBracketedPaste.write_ansi(&mut s);
    let _ = DisableAlternateScroll.write_ansi(&mut s);
    // Unconditional, matching the panic hook: cheap when capture was never on,
    // and a leaked `?1000h` spams the user's next shell with mouse reports.
    let _ = DisableWheelReporting.write_ansi(&mut s);
    let _ = ShowMousePointer.write_ansi(&mut s);
    let _ = crossterm::cursor::Show.write_ansi(&mut s);
    s.push_str(&term_title::pop_sequence());
    s
}

/// No-op handler for SIGINT / SIGQUIT. Replaces the default
/// "terminate-the-process" disposition so spyc can survive a stray
/// `^C` (or `^\`) that arrives while raw mode is off and the kernel
/// is generating signals from tty input.
// Intentionally empty -- we want SIGINT/SIGQUIT to be a no-op
// for spyc, NOT inherited as SIG_IGN by children. Can't be const
// since extern "C" fn pointers don't work with const-fn.
#[allow(clippy::missing_const_for_fn)]
extern "C" fn signal_noop(_: libc::c_int) {}

/// Restore the terminal, then die, for signals that mean "terminate": SIGTERM
/// (`pkill spyc`, a service manager, an OOM-adjacent kill) and SIGHUP (the
/// terminal closed, or a logout).
///
/// Without this the default disposition kills spyc with no cleanup, handing the
/// shell back on the alt screen, in raw mode, and — since `[mouse] capture`
/// defaults on — with `?1000h` still armed, so every pointer move and click
/// emits escape garbage into the shell for the rest of the session.
///
/// Async-signal-safe by construction: one `write` of a string built at startup,
/// one `tcsetattr`, then `_exit`. No allocation, no locks, no stdio (`exit`
/// would run atexit handlers and flush buffers — neither is safe here).
extern "C" fn signal_terminate(sig: libc::c_int) {
    // SAFETY: the only calls here are `write`, `tcsetattr` and `_exit`, all
    // async-signal-safe per POSIX. Both statics are read-only by now; an
    // uninitialized one (signal before `setup_terminal`) just skips its step.
    unsafe {
        if let Some(seq) = RESTORE_SEQ.get() {
            // Best-effort: a partial or failed write can't be retried safely,
            // and we're about to exit regardless.
            libc::write(
                libc::STDOUT_FILENO,
                seq.as_ptr().cast::<libc::c_void>(),
                seq.len(),
            );
        }
        if let Some(termios) = ORIGINAL_TERMIOS.get() {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, termios);
        }
        // 128 + signum is the shell's own convention for death by signal.
        libc::_exit(128 + sig);
    }
}

/// Install no-op handlers for SIGINT and SIGQUIT so spyc never dies
/// from a Ctrl+C / Ctrl+\ that wasn't intended for it, plus SIG_IGN
/// for SIGTTOU so the post-child `tcsetpgrp` restore succeeds.
///
/// **The bug this fixes:** spyc runs in raw mode, where the kernel's
/// tty signal generation (`ISIG`) is disabled and `^C` arrives as a
/// regular key event. But `p` → `$PAGER`, `v` → `$EDITOR`, and `;`
/// foreground commands all call `suspend_tui` first, which restores
/// canonical mode + `ISIG`. Now a `^C` from the tty driver is sent
/// as `SIGINT` to the *foreground process group* of the controlling
/// terminal — which is spyc's process group, since the child
/// inherited it. Both spyc and the child receive the signal:
///   - The child (less, vim) installs its own `SIGINT` handler at
///     startup and treats it as "interrupt current operation" (less
///     stops counting lines, vim cancels current input).
///   - spyc, with the default disposition, *terminates*. The tty
///     session leader exits, the kernel sends `SIGHUP` to remaining
///     foreground processes, less + sh die too. From the user's
///     perspective: "spyc died on ^C in less."
///
/// Fix: install a custom no-op handler for SIGINT (and SIGQUIT for
/// the same reason). spyc receives the signal, ignores it. Per
/// POSIX `execve(2)` semantics, custom handlers are reset to
/// `SIG_DFL` in the child, so the child receives the signal with
/// normal disposition and handles it correctly. (Pure `SIG_IGN`
/// would inherit across exec, breaking the child's signal handling.)
///
/// SIGTTOU is raised on a process not in the FG process group when
/// it calls `tcsetpgrp()`. We use `tcsetpgrp` to hand tty foreground
/// to/from children for `p` / `v` / `;` takeovers — the *restore*
/// call after the child exits comes from a process that's no longer
/// the FG group. POSIX `tcsetpgrp(3)` succeeds in that situation
/// only if SIGTTOU is **blocked or ignored**. A custom Rust handler
/// (signal-hook style) does NOT satisfy this: the kernel still
/// delivers SIGTTOU, the syscall returns `EINTR`, and the FG group
/// stays pointed at the dead child's group — leaving spyc unable to
/// read stdin without first being SIGTTIN'd. So we use raw `SIG_IGN`
/// here, accepting that SIGTTOU's ignore disposition inherits across
/// exec. No well-behaved child process in the foreground triggers
/// SIGTTOU anyway (it's a background-write signal), so the inherit
/// is harmless.
pub fn install_signal_handlers() {
    // The whole block is one well-isolated unsafe at startup. Signal
    // handler installation is not exposed safely through `rustix` /
    // `signal-hook` for our exact need (SIG_IGN inheritance for
    // SIGTTOU) — see the function-level doc above.
    unsafe {
        // libc::signal returns the previous handler; we don't care
        // about it. SIG_ERR ⇒ failure, but on a sane Unix this
        // doesn't fail for a regular handler install.
        let h = signal_noop as *const () as libc::sighandler_t;
        libc::signal(libc::SIGINT, h);
        libc::signal(libc::SIGQUIT, h);
        libc::signal(libc::SIGTTOU, libc::SIG_IGN);
        // SIGTERM / SIGHUP mean "terminate" and must not skip the terminal
        // restore — see `signal_terminate`.
        let t = signal_terminate as *const () as libc::sighandler_t;
        libc::signal(libc::SIGTERM, t);
        libc::signal(libc::SIGHUP, t);
    }
}

/// Detect the terminal graphics protocol + font cell size for inline mermaid
/// rendering. `from_query_stdio` probes via Kitty/Sixel capability *queries*;
/// iTerm2 answers none of them and so falls back to `Halfblocks` (which renders
/// nothing useful for a diagram). iTerm2 has its own inline-image protocol, so
/// when the env identifies iTerm2 we force it. Returns `None` only if the query
/// errored outright (→ mermaid `i` reports "no image protocol").
pub fn detect_image_picker() -> Option<ratatui_image::picker::Picker> {
    use ratatui_image::picker::{Picker, ProtocolType};
    let mut picker = Picker::from_query_stdio().ok()?;
    // SPYC-TRAP(iterm-osc1337): do not drop the iTerm2 override below — iTerm2
    // answers the Kitty probe but only its native OSC 1337 actually paints, so
    // images silently fail to render on iTerm2 without it.
    // iTerm2 (3.5+) also implements the Kitty graphics protocol, so the probe
    // detects Kitty — but iTerm2's Kitty emulation doesn't paint reliably here,
    // while its native inline-image protocol (OSC 1337) does. And without a
    // graphics response it falls back to Halfblocks. Either way, prefer the
    // native iTerm2 protocol whenever the env identifies iTerm2 (the detected
    // font size from the successful query is kept).
    let is_iterm = std::env::var("TERM_PROGRAM").is_ok_and(|t| t.contains("iTerm"))
        || std::env::var("LC_TERMINAL").is_ok_and(|t| t.contains("iTerm"));
    if is_iterm && picker.protocol_type() != ProtocolType::Iterm2 {
        picker.set_protocol_type(ProtocolType::Iterm2);
    }
    Some(picker)
}

pub fn setup_terminal() -> Result<Tui> {
    // Stash what the signal teardown needs BEFORE the terminal is touched:
    // the pre-raw termios, and the restore string (built here because the
    // handler may not build it — see `signal_terminate`).
    let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: `tcgetattr` fills the struct; we only read it on success.
    if unsafe { libc::tcgetattr(libc::STDIN_FILENO, termios.as_mut_ptr()) } == 0 {
        let _ = ORIGINAL_TERMIOS.set(unsafe { termios.assume_init() });
    }
    let _ = RESTORE_SEQ.set(terminal_restore_seq());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        // Blank the buffer we just entered. On terminals that honour the alt
        // screen this is redundant (the alt buffer starts empty), but GNU
        // screen with `altscreen off` (macOS's bundled 4.00.03 default) ignores
        // `?1049h` and leaves us on the main buffer with the shell's content
        // still there. ratatui's first draw diffs against an all-blank previous
        // buffer, so it never emits cells for regions spyc keeps blank — the old
        // shell text bleeds through below the file list. `\x1b[2J` wipes it once;
        // no cursor read, so the SSH trap (see `force_full_repaint`) doesn't apply.
        Clear(ClearType::All),
        MoveTo(0, 0),
        EnableBracketedPaste,
        EnableAlternateScroll,
        HideMousePointer
    )?;
    // Kitty keyboard protocol: ask the terminal to send unambiguous
    // modifier info on every key. The big practical win is
    // Option+Enter on macOS -- without this, terminals like Ghostty,
    // Kitty, WezTerm, foot, and modern iTerm2 either fold it into
    // Alt+Enter or send it as ESC+Enter ambiguously. With
    // DISAMBIGUATE_ESCAPE_CODES, we get an unambiguous Alt+Enter
    // KeyEvent every time, and `pane::input::encode_key` folds it
    // to a `\n` newline (multi-line input in Claude). Best-effort:
    // terminals that don't support the protocol (Terminal.app, older
    // Alacritty) simply don't reply to the request -- no harm done.
    if supports_keyboard_enhancement().unwrap_or(false) {
        let _ = execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }
    // Save the current window title so we can restore it on quit.
    // Best-effort: terminals that don't implement xterm CSI 22;0t just
    // ignore it.
    let _ = term_title::push();
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    // Pop the kitty keyboard enhancement flag (best-effort -- if
    // we never pushed it because the terminal didn't support it,
    // the pop is a no-op). Terminals that *do* support it leave
    // the flag set if we don't pop, which would affect any other
    // TUI started in the same shell session.
    let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        DisableAlternateScroll,
        DisableWheelReporting,
        ShowMousePointer
    )?;
    MOUSE_CAPTURE_ON.store(false, Ordering::Relaxed);
    let _ = term_title::pop();
    terminal.show_cursor()?;
    Ok(())
}

/// The panic hook's half of [`restore_terminal`]: it has no `Tui` to write
/// through, so it writes to stdout.
pub fn restore_on_panic() {
    // Best-effort terminal restore — ignore errors. Mirror
    // `restore_terminal` so a crash doesn't leave the shell in raw mode,
    // on the alt screen, or — the two that were easy to miss here — with
    // the kitty keyboard-enhancement flag still pushed or alternate-scroll
    // still on. Both of those silently corrupt the next TUI / scroll-wheel
    // behaviour in the *same shell session*, long after the panic.
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    let _ = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        DisableAlternateScroll,
        // Unconditional: cheap, harmless when capture was never on, and a
        // leaked `?1000h` silently breaks click-drag selection in the user's
        // shell for the rest of the session.
        DisableWheelReporting,
        ShowMousePointer,
        crossterm::cursor::Show,
    );
    // This hook is NOT exit-only (`pane::Pane` catches a known vt100 panic
    // and spyc keeps running), so the flag has to follow the terminal or the
    // reconcile will believe capture is still on and never restore it.
    MOUSE_CAPTURE_ON.store(false, Ordering::Relaxed);
    let _ = term_title::pop();
}

/// Switch the terminal between 1007 alternate-scroll (wheel as arrow keys, native
/// selection intact) and real mouse reporting (`?1000h?1006h`).
///
/// The two are mutually exclusive on purpose — a terminal honouring both could
/// deliver one wheel tick twice, once as arrows and once as a mouse event — so
/// this always emits the disable of one alongside the enable of the other. Called
/// only from the `Effect::SetMouseMode` executor, which in turn is emitted only by
/// the reconcile in `App::settle_mouse_mode`.
pub fn set_mouse_capture(terminal: &mut Tui, capture: bool) -> Result<()> {
    use std::io::Write as _;
    // One write of the paired sequence (see `mouse_mode_seq`) so the disable and
    // the enable cannot be separated by a failure between two `execute!` calls.
    let seq = mouse_mode_seq(capture);
    terminal.backend_mut().write_all(seq.as_bytes())?;
    terminal.backend_mut().flush()?;
    // Record only on success: a failed write leaves the terminal in its previous
    // mode, and claiming otherwise is what makes the reconcile go quiet on a
    // state it never reached.
    MOUSE_CAPTURE_ON.store(capture, Ordering::Relaxed);
    Ok(())
}

/// Release the tty so a child process (editor, pager, shell) can own it,
/// without exposing the user's shell scrollback in the interim.
///
/// Key detail: we **stay in the alternate screen**. If we call
/// `LeaveAlternateScreen`, the terminal flips back to the main buffer for
/// the split second between our call and the child's own `smcup`, which
/// causes the "flash of old shell content" glitch. Instead, we blank our
/// alt screen and let the child's `smcup` reuse or stack on top of it.
pub fn suspend_tui(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        Clear(ClearType::All),
        MoveTo(0, 0),
        DisableBracketedPaste,
        DisableAlternateScroll,
        // Hand the mouse to the child: `less`/`vim` set up their own reporting,
        // and leaving ours on would have both of us reading the same events.
        DisableWheelReporting,
    )?;
    // The child owns the mouse now. Clearing this is what makes
    // `settle_mouse_mode` reclaim it on the next iteration after we resume.
    MOUSE_CAPTURE_ON.store(false, Ordering::Relaxed);
    terminal.show_cursor()?;
    Ok(())
}

/// Re-acquire the tty after the child has exited.
///
/// `EnterAlternateScreen` is idempotent on most terminals; sending it
/// here means that if the child's `rmcup` did drop us to the main screen
/// we bounce right back before anything is visible.
pub fn resume_tui(terminal: &mut Tui) -> Result<()> {
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        // Wipe the child's leftover output. Without a working alt screen (old
        // GNU screen) the editor/pager we just returned from is still on the
        // main buffer; the `force_full_repaint` below only repaints spyc's
        // non-blank cells, so clear the rest here. See `setup_terminal`.
        Clear(ClearType::All),
        MoveTo(0, 0),
        EnableBracketedPaste,
        // Clear any reporting the child left behind BEFORE turning 1007 back on.
        // A child killed without resetting (SIGKILLed vim, a crashed TUI) hands
        // the tty back with its own 1000/1002/1003 still live; enabling 1007 on
        // top of that is the both-modes-on state the pairing exists to prevent,
        // and the terminal may then deliver one wheel tick twice.
        //
        // Resuming therefore comes back with mouse reporting OFF, deliberately:
        // `settle_mouse_mode` re-enables it on the next loop iteration if
        // `[mouse] capture` says so, which keeps one path deciding it.
        DisableWheelReporting,
        EnableAlternateScroll
    )?;
    terminal.hide_cursor()?;
    force_full_repaint(terminal)?;
    Ok(())
}

// SPYC-TRAP(cursor-read-ssh): do not "simplify" this back to
// `Terminal::clear()` — its `ESC[6n` cursor round-trip silently hangs/crashes
// the session, but only over SSH, so it passes every local test.
/// Clear the whole screen and force a full repaint on the next draw.
///
/// Avoids ratatui's `Terminal::clear()`, which does a `get_cursor_position()`
/// (`ESC[6n`) round-trip: over SSH the reply can exceed crossterm's timeout and
/// races the just-unparked input reader, failing with "cursor position could
/// not be read" and tearing the session down. `Terminal::resize()` to the
/// current size clears and forces a full repaint without reading the cursor.
pub fn force_full_repaint(terminal: &mut Tui) -> Result<()> {
    let area = ratatui::layout::Rect::from(terminal.size()?);
    terminal.resize(area)?;
    Ok(())
}

#[cfg(test)]
mod mouse_reporting_tests {
    use super::{DisableWheelReporting, EnableWheelReporting};
    use crossterm::Command;

    fn ansi(cmd: &impl Command) -> String {
        let mut out = String::new();
        cmd.write_ansi(&mut out).expect("write to String");
        out
    }

    /// The bytes matter and are invisible in manual testing on a still pointer:
    /// `?1003h` (any-event motion) would wake the loop and mark a redraw on every
    /// pointer move, straight through the 0-dps-at-idle invariant — and you'd only
    /// notice by waving the mouse while watching the `A` overlay. `crossterm`'s own
    /// `EnableMouseCapture` emits it, which is exactly why spyc doesn't use that.
    ///
    /// **1002 is requested and 1003 is not**, which is not a fine distinction: 1002
    /// reports motion only while a button is held, so an untouched mouse generates
    /// nothing and the invariant is untouched. Asserting only "no motion modes"
    /// would have made drag support impossible to add without deleting the guard
    /// that matters.
    #[test]
    fn enable_asks_for_buttons_drags_and_sgr_but_never_any_event_motion() {
        let seq = ansi(&EnableWheelReporting);
        assert!(seq.contains("\x1b[?1000h"), "button reporting: {seq:?}");
        assert!(
            seq.contains("\x1b[?1002h"),
            "button-event tracking, for drags: {seq:?}"
        );
        assert!(seq.contains("\x1b[?1006h"), "SGR coordinates: {seq:?}");
        assert!(
            !seq.contains("1003"),
            "must never request ANY-EVENT motion (?1003h) — that is the idle \
             redraw storm: {seq:?}"
        );
    }

    /// A leaked `?1000h` silently breaks click-drag selection in the user's shell
    /// for the rest of the session, so the disable has to undo everything the
    /// enable asked for — this is the pair the panic hook and `restore_terminal`
    /// rely on.
    #[test]
    fn disable_undoes_every_mode_the_enable_set() {
        let disable = ansi(&DisableWheelReporting);
        for mode in ["1000", "1006"] {
            assert!(
                disable.contains(&format!("\x1b[?{mode}l")),
                "?{mode}l missing from {disable:?}"
            );
        }
        // Unwound in reverse, so a terminal tracking a mode stack pops cleanly.
        let sgr = disable.find("1006").expect("1006");
        let btn = disable.find("1000").expect("1000");
        assert!(sgr < btn, "expected 1006l before 1000l: {disable:?}");
    }

    /// The disable also clears 1002/1003, which spyc never *enables*.
    ///
    /// A foreground child (vim, htop) sets its own motion reporting; one killed
    /// without resetting hands the tty back with those still live. Clearing only
    /// our own pair would leave motion reporting on — which breaks the 1007
    /// exclusivity `resume_tui` re-establishes, and leaks motion events into the
    /// user's shell after spyc exits.
    #[test]
    fn disable_also_clears_a_childs_leftover_motion_reporting() {
        let disable = ansi(&DisableWheelReporting);
        for mode in ["1002", "1003"] {
            assert!(
                disable.contains(&format!("\x1b[?{mode}l")),
                "?{mode}l missing — a child's leaked motion mode would survive: {disable:?}"
            );
        }
    }

    /// **The invariant the whole feature rests on**, in both directions: DEC 1007
    /// (wheel-as-arrows) and DEC 1000 (real reporting) are never both enabled. A
    /// terminal honouring both delivers one wheel tick twice — once as arrow keys,
    /// once as a mouse event.
    ///
    /// Untestable before `mouse_mode_seq` was split out of `set_mouse_capture`,
    /// which takes `&mut Tui` — a type no unit test can construct. So the one rule
    /// that matters most had no coverage at all.
    #[test]
    fn the_two_modes_are_never_both_enabled() {
        // Enabling capture: 1007 off, 1000/1006 on.
        let on = super::mouse_mode_seq(true);
        assert!(on.contains("\x1b[?1007l"), "must disable 1007: {on:?}");
        assert!(on.contains("\x1b[?1000h"), "must enable 1000: {on:?}");
        assert!(!on.contains("\x1b[?1007h"), "must not enable 1007: {on:?}");

        // Disabling capture: the mirror.
        let off = super::mouse_mode_seq(false);
        assert!(off.contains("\x1b[?1000l"), "must disable 1000: {off:?}");
        assert!(off.contains("\x1b[?1007h"), "must enable 1007: {off:?}");
        assert!(
            !off.contains("\x1b[?1000h"),
            "must not enable 1000: {off:?}"
        );

        // Ordering: in each direction the disable precedes the enable, so there is
        // no instant with both modes live even for a terminal applying them
        // sequentially.
        assert!(
            on.find("1007l") < on.find("1000h"),
            "disable 1007 before enabling 1000: {on:?}"
        );
        assert!(
            off.find("1000l") < off.find("1007h"),
            "disable 1000 before enabling 1007: {off:?}"
        );
    }

    /// Neither direction may request ANY-EVENT motion — the `?1003h` redraw storm
    /// is invisible on a still pointer, so only a byte assertion catches it.
    ///
    /// `?1002h` (motion only while a button is held) is expected when enabling and
    /// absent when disabling.
    #[test]
    fn neither_direction_requests_any_event_motion() {
        for capture in [true, false] {
            let seq = super::mouse_mode_seq(capture);
            assert!(
                !seq.contains("1003h"),
                "capture={capture} must not request ?1003h: {seq:?}"
            );
        }
        assert!(
            super::mouse_mode_seq(true).contains("1002h"),
            "enabling capture must ask for drags"
        );
        assert!(
            !super::mouse_mode_seq(false).contains("1002h"),
            "disabling capture must not enable anything"
        );
    }

    /// The SIGTERM/SIGHUP restore must undo every mode `setup_terminal` set.
    ///
    /// This is the byte-level contract of a path no test can drive end-to-end
    /// (it ends in `_exit`), and the leak it prevents is invisible until you're
    /// back in your shell with the mouse spewing escapes.
    #[test]
    fn signal_restore_undoes_every_mode_setup_enabled() {
        let seq = super::terminal_restore_seq();
        for (mode, why) in [
            ("\x1b[?1049l", "leave the alt screen"),
            ("\x1b[?2004l", "disable bracketed paste"),
            ("\x1b[?1007l", "disable alternate scroll"),
            ("\x1b[?1000l", "disable mouse reporting"),
            ("\x1b[?1002l", "disable drag reporting"),
            ("\x1b[?1006l", "disable SGR coordinates"),
            ("\x1b[?25h", "show the cursor"),
            ("\x1b[>0p", "show the mouse pointer"),
            ("\x1b[23;0t", "pop the window title"),
        ] {
            assert!(
                seq.contains(mode),
                "restore must {why} ({mode:?} missing): {seq:?}"
            );
        }
    }

    /// The restore must not *enable* anything. `restore_terminal` deliberately
    /// omits `EnableAlternateScroll` (unlike `mouse_mode_seq(false)`, which is
    /// re-arming a live session) — spyc is exiting, so leaving 1007 on would
    /// hand the shell a wheel that still emits arrow keys.
    #[test]
    fn signal_restore_enables_nothing() {
        let seq = super::terminal_restore_seq();
        for enable in [
            "1000h", "1002h", "1003h", "1006h", "1007h", "1049h", "2004h",
        ] {
            assert!(
                !seq.contains(enable),
                "restore must not enable ?{enable}: {seq:?}"
            );
        }
    }

    /// True when `banned` appears in `src` outside a comment.
    ///
    /// The whole-file exemption this replaces existed so `lib.rs` could *name*
    /// the constant in prose — at the cost of blinding the guard to the only file
    /// holding `setup_terminal` / `restore_terminal` / `resume_tui`, the three
    /// places that could reintroduce the storm. Ignoring comments keeps the prose
    /// and the coverage.
    fn code_mentions(src: &str, banned: &str) -> bool {
        src.lines()
            .map(|line| {
                if line.trim_start().starts_with("//") {
                    return "";
                }
                line.split_once("//").map_or(line, |(code, _)| code)
            })
            .any(|code| code.contains(banned))
    }

    /// Source-scan guard: nothing in `src/` may use crossterm's own
    /// `EnableMouseCapture`.
    ///
    /// The byte tests above only cover the structs spyc defines. `EnableMouseCapture`
    /// emits `?1000h ?1002h ?1003h ?1015h ?1006h` — the motion modes included — so
    /// one convenient-looking call anywhere (`setup_terminal`, `resume_tui`, a future
    /// feature) reintroduces the redraw storm while every existing test still passes.
    /// It reads as the obvious API to reach for, which is exactly why this is a
    /// guard and not a comment.
    #[test]
    fn production_code_never_uses_crossterms_mouse_capture() {
        use std::path::{Path, PathBuf};

        // Assembled so this test's own source doesn't trip the scan.
        let banned = ["Enable", "MouseCapture"].concat();

        fn scan(dir: &Path, banned: &str, offenders: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("read src dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    scan(&path, banned, offenders);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let src = std::fs::read_to_string(&path).expect("read .rs");
                    if code_mentions(&src, banned) {
                        offenders.push(path);
                    }
                }
            }
        }

        let mut offenders = Vec::new();
        scan(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &banned,
            &mut offenders,
        );
        // Built from `banned` rather than spelled out: this line is code, and the
        // scan is now honest enough to flag its own failure message.
        assert!(
            offenders.is_empty(),
            "crossterm's {banned} emits ?1003h (any-motion) — use \
             EnableWheelReporting instead. Offenders: {offenders:?}"
        );
    }

    /// The guard above has to survive its own exemption being needed.
    ///
    /// It previously dropped every file named `lib.rs`, so the prose reference in
    /// `EnableWheelReporting`'s doc comment could coexist with it — at the cost of
    /// blinding it to `setup_terminal`, `restore_terminal` and `resume_tui`, which
    /// lived in that file then and live in this one now. Injecting the call there
    /// and watching the guard stay green is how that was found, so the injection
    /// is the test.
    #[test]
    fn the_capture_guard_sees_code_in_the_file_that_sets_the_terminal_up() {
        let banned = ["Enable", "MouseCapture"].concat();
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/terminal.rs"),
        )
        .expect("read terminal.rs");

        // As it stands: named in prose, so a naive substring scan flags it...
        assert!(
            src.contains(&banned),
            "this file is supposed to mention it in prose"
        );
        // ...but the scan the guard actually uses does not.
        assert!(
            !code_mentions(&src, &banned),
            "a comment must not read as a violation"
        );
        // And a real call in this file does.
        let injected = format!("    execute!(out, {banned})?;\n");
        assert!(
            code_mentions(&injected, &banned),
            "an actual call must be caught wherever it lives"
        );
        // Including one with a trailing comment, the shape a scan splitting on
        // `//` could drop.
        assert!(code_mentions(
            &format!("    execute!(out, {banned})?; // needed for X\n"),
            &banned
        ));
    }
}
