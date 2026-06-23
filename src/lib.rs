//! spyc — a vi-keyboard-driven file commander inspired by SideFX's spy.
//!
//! This is the crate's **library** root: it owns all the modules and the
//! [`run`] entry point. `src/main.rs` is a thin binary shim that calls
//! `spyc::run()`. The split exists so the crate also builds as a library,
//! which the `cargo-fuzz` targets in `fuzz/` link against (libFuzzer targets
//! are separate binaries). Runtime behaviour is unchanged.

mod agent;
mod app;
mod clipboard;
mod config;
mod context;
mod debug_log;
mod envset;
mod fs;
mod git;
mod key_trace;
mod keymap;
mod mcp;
mod mcp_cmd;
mod pane;
mod paths;
mod proc_cwd;
mod shell;
mod state;
mod sysinfo;
mod term_title;
mod ui;

/// Human-readable build identity, e.g. `1.59.0 (25abd0a)`.
///
/// The crate version plus the short git SHA baked in at build time
/// (`build.rs`). The SHA changes every commit, so this is the signal that tells
/// an MCP client whether the running spyc predates a tool it expects —
/// surfaced over MCP via the `initialize` `serverInfo` and `get_spyc_context`.
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("SPYC_GIT_SHA"), ")");

/// Public entry points for the `cargo-fuzz` targets in `fuzz/`.
///
/// The crate is otherwise all-private modules. Each wrapper takes raw input
/// and discards the result (no internal types leak into the public API), so a
/// fuzz target asserts only the "never panics" property.
pub mod fuzz {
    /// Parse one keymap-DSL line and discard the result. See
    /// `fuzz/fuzz_targets/dsl_parse.rs`.
    pub fn parse_keymap_line(line: &str) {
        let _ = crate::config::dsl::parse(line);
    }

    /// Render arbitrary markdown to styled lines and discard the result — the
    /// fuzz target asserts the renderer never panics on adversarial markdown
    /// (it ingests untrusted file content via the pager).
    pub fn render_markdown(source: &str) {
        let _ = crate::ui::markdown::render(source, &crate::ui::theme::Theme::default(), Some(80));
    }

    /// Syntax-highlight arbitrary content (as if it were a Rust file) and
    /// discard the result — asserts the highlighter never panics.
    pub fn highlight(content: &str) {
        let _ = crate::ui::syntax::highlight_to_lines("fuzz.rs", content);
    }

    /// Word-wrap arbitrary text at `width`, asserting the wrap invariant.
    ///
    /// Every returned byte range must land on char boundaries and be
    /// sliceable — a mid-codepoint range would panic the pager's actual
    /// slicing, which is the bug class this catches.
    pub fn word_wrap(text: &str, width: usize) {
        for (start, end) in crate::ui::wrap::word_wrap_ranges(text, width) {
            assert!(
                start <= end && end <= text.len(),
                "wrap range out of bounds: ({start},{end}) len {}",
                text.len()
            );
            assert!(
                text.is_char_boundary(start) && text.is_char_boundary(end),
                "wrap range splits a codepoint: ({start},{end}) in {text:?}"
            );
            let _ = &text[start..end]; // must not panic
        }
    }

    /// Expand `~` / `$VAR` / `${VAR}` in an arbitrary path string and discard
    /// the result — asserts the path expander never panics on adversarial
    /// variable syntax.
    pub fn expand_path(input: &str) {
        let _ = crate::paths::expand(input);
    }

    /// Expand a `%`-template (the `unix CMD` substitution) against a couple of
    /// fixed target paths and discard the result — asserts the template parser
    /// never panics on arbitrary `%`/escape syntax.
    pub fn expand_percent(template: &str) {
        let _ = crate::shell::expand_percent(
            template,
            &[
                std::path::Path::new("/tmp/a.rs"),
                std::path::Path::new("/tmp/b c.txt"),
            ],
        );
    }
}

use std::io;

use anyhow::Result;
use clap::Parser;
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

use crate::app::App;

/// spyc — vi-keyboard-driven file commander
#[derive(Parser)]
#[command(name = "spyc", version, about = "vi-keyboard-driven file commander")]
struct Cli {
    /// Open the saved-session restore picker on startup
    #[arg(short, long)]
    resume: bool,

    /// Write debug log to an owner-only spyc-debug-<ts>.log in the state dir
    #[arg(short, long)]
    debug: bool,

    /// Trace every key event + dispatch decision to
    /// /tmp/spyc-key-trace-<ts>.log. Useful for diagnosing
    /// "input doesn't work when done too quickly" reports.
    /// Equivalent to setting SPYC_KEY_TRACE=1.
    #[arg(long)]
    key_trace: bool,

    /// Run as MCP server (stdio JSON-RPC)
    #[arg(long)]
    mcp: bool,

    /// Show extended build info with --version
    #[arg(long)]
    verbose: bool,

    /// Print a fully-commented default `.spycrc.toml` to stdout and exit.
    /// Pipe to a file to bootstrap your config:
    ///   spyc --print-config > ~/.spycrc.toml
    #[arg(long)]
    print_config: bool,
}

/// Binary entry point. `src/main.rs` is a thin shim that just calls this;
/// all the real startup logic lives here so the crate can also be a library.
pub fn run() -> Result<()> {
    // Restore the terminal on panic so the user's shell isn't left in raw
    // mode / alt screen. This runs before the default handler which prints
    // the panic message to stderr.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Best-effort terminal restore — ignore errors. Mirror
        // `restore_terminal` so a crash doesn't leave the shell in raw mode,
        // on the alt screen, or — the two that were easy to miss here — with
        // the kitty keyboard-enhancement flag still pushed or alternate-scroll
        // still on. Both of those silently corrupt the next TUI / scroll-wheel
        // behavior in the *same shell session*, long after the panic.
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableBracketedPaste,
            DisableAlternateScroll,
            ShowMousePointer,
            crossterm::cursor::Show,
        );
        let _ = term_title::pop();

        // Dump to the debug log if active.
        let bt = std::backtrace::Backtrace::force_capture();
        debug_log::log(&format!("PANIC: {info}\n{bt}"));

        // Let the default handler print to stderr.
        default_hook(info);
    }));

    let cli = Cli::parse();

    if cli.print_config {
        print!("{}", config::DEFAULT_TEMPLATE);
        return Ok(());
    }

    if cli.mcp {
        let root = std::env::current_dir()?;
        return mcp::run(root);
    }
    if cli.verbose {
        println!("\u{1f336}\u{fe0f} spyc {}", env!("CARGO_PKG_VERSION"));
        println!("  git:     {}", env!("SPYC_GIT_SHA"));
        println!("  built:   {}", env!("SPYC_BUILD_TIME"));
        println!("  rustc:   {}", env!("SPYC_RUSTC_VERSION"));
        println!("  TERM:    {}", std::env::var("TERM").unwrap_or_default());
        println!(
            "  COLOR:   {}",
            std::env::var("COLORTERM").unwrap_or_default()
        );
        println!(
            "  os:      {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        return Ok(());
    }

    if let Some(p) = debug_log::init(cli.debug) {
        eprintln!("spyc: debug log → {p}");
    }
    if let Some(p) = key_trace::init(cli.key_trace) {
        eprintln!("spyc: key trace → {p}");
    }
    // Install signal handlers BEFORE the TUI starts so a stray
    // ^C during a suspended-mode takeover (`p` → less, `v` →
    // editor, `;` → top pane) doesn't bring spyc down with the
    // child. See `install_signal_handlers` for the full reasoning.
    install_signal_handlers();
    let mcp_takeover_allowed = prompt_mcp_takeover_if_needed();
    let mut terminal = setup_terminal()?;
    let mut app = App::new(cli.resume, mcp_takeover_allowed);
    // Detect the terminal's graphics protocol (Kitty/iTerm2/Sixel/halfblocks +
    // font cell size) for inline diagram rendering — ONCE, here, before the
    // input reader spawns, because `from_query_stdio` reads stdin/cursor
    // responses (the #444 no-live-cursor-read rule). Best-effort.
    app.set_picker(detect_image_picker());
    let result = app.run(&mut terminal);
    mcp::cleanup_socket();
    // Restore the terminal BEFORE teardown so `run_teardown`'s "waiting for …"
    // lines land on the normal screen instead of behind the alt-screen. A
    // restore error is deferred so teardown still runs unconditionally (the
    // PR8b guarantee that pane children are always SIGTERM-graced on exit).
    let restore = restore_terminal(&mut terminal);
    app.run_teardown();
    if let Some(summary) = &app.exit_summary {
        println!("\u{1f336}\u{fe0f} {summary}");
    }
    restore?;
    result
}

pub type Tui = Terminal<CrosstermBackend<io::Stdout>>;

/// If another live spyc owns MCP for the current directory, ask the
/// user whether to take it over. Default Y on empty input. Returns
/// `false` to mean "leave the existing instance alone."
///
/// Non-tty stdin (CI, piped input) keeps the historical auto-takeover
/// behavior — there's no one to prompt.
fn prompt_mcp_takeover_if_needed() -> bool {
    use std::io::{BufRead, IsTerminal, Write};

    // Under enterprise control we don't write `.mcp.json` at all, so
    // there's nothing to take over and the prompt would just confuse.
    if mcp::enterprise_defines_spyc() {
        return true;
    }
    let Ok(cwd) = std::env::current_dir() else {
        return true;
    };
    // Either claude's `.mcp.json` or codex's `.codex/config.toml`
    // can hold a stale-by-PID spyc entry; check both so the takeover
    // prompt fires regardless of which agent the prior instance had
    // configured.
    let Some(old_pid) =
        mcp::detect_existing_spyc(&cwd).or_else(|| mcp::detect_existing_spyc_codex(&cwd))
    else {
        return true;
    };
    if !io::stdin().is_terminal() {
        return true;
    }

    let mut stderr = io::stderr();
    let _ = write!(
        stderr,
        "\u{1f336}\u{fe0f} spyc: PID {old_pid} already owns MCP here. Take over? [Y/n] "
    );
    let _ = stderr.flush();

    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return true;
    }
    let trimmed = line.trim();
    !matches!(trimmed, "n" | "N" | "no" | "No" | "NO")
}

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

/// No-op handler for SIGINT / SIGQUIT. Replaces the default
/// "terminate-the-process" disposition so spyc can survive a stray
/// `^C` (or `^\`) that arrives while raw mode is off and the kernel
/// is generating signals from tty input.
// Intentionally empty -- we want SIGINT/SIGQUIT to be a no-op
// for spyc, NOT inherited as SIG_IGN by children. Can't be const
// since extern "C" fn pointers don't work with const-fn.
#[allow(clippy::missing_const_for_fn)]
extern "C" fn signal_noop(_: libc::c_int) {}

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
fn install_signal_handlers() {
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
    }
}

/// Detect the terminal graphics protocol + font cell size for inline mermaid
/// rendering. `from_query_stdio` probes via Kitty/Sixel capability *queries*;
/// iTerm2 answers none of them and so falls back to `Halfblocks` (which renders
/// nothing useful for a diagram). iTerm2 has its own inline-image protocol, so
/// when the env identifies iTerm2 we force it. Returns `None` only if the query
/// errored outright (→ mermaid `i` reports "no image protocol").
fn detect_image_picker() -> Option<ratatui_image::picker::Picker> {
    use ratatui_image::picker::{Picker, ProtocolType};
    let mut picker = Picker::from_query_stdio().ok()?;
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

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
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

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
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
        ShowMousePointer
    )?;
    let _ = term_title::pop();
    terminal.show_cursor()?;
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
    )?;
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
        EnableBracketedPaste,
        EnableAlternateScroll
    )?;
    terminal.hide_cursor()?;
    force_full_repaint(terminal)?;
    Ok(())
}

/// Clear the whole screen and force a full repaint on the next draw.
///
/// Deliberately avoids ratatui 0.30's `Terminal::clear()`, which snapshots
/// the cursor via a `get_cursor_position()` (`ESC[6n`) round-trip and
/// restores it afterward. That round-trip is fine on a fast local terminal
/// but is a latent crash
/// over SSH: the reply can exceed crossterm's ~2 s timeout, *and* the
/// just-unparked input-reader thread races to read the same reply off stdin —
/// either way `position()` fails with "cursor position could not be read",
/// which propagated out of `resume_tui` / the `pending_clear` draw and tore
/// the whole session down (e.g. closing a foreground pager, or any navigation
/// that set `needs_full_repaint`, over an SSH link).
///
/// `Terminal::resize()` to the current size has the same on-screen effect
/// (clears `All` + resets the back buffer so the next frame is a full
/// repaint) but takes the no-cursor-read branch on a fullscreen viewport.
/// The next `draw()` positions the cursor from the frame, so the snapshot
/// `clear()` did was pointless for us anyway.
pub fn force_full_repaint(terminal: &mut Tui) -> Result<()> {
    let area = ratatui::layout::Rect::from(terminal.size()?);
    terminal.resize(area)?;
    Ok(())
}
