//! spyc — a vi-keyboard-driven file commander inspired by SideFX's spy.

mod app;
mod config;
mod context;
mod debug_log;
mod fs;
mod keymap;
mod mcp;
mod mcp_cmd;
mod pane;
mod paths;
mod shell;
mod state;
mod sysinfo;
mod ui;

use std::io;

use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::app::App;

fn main() -> Result<()> {
    // Restore the terminal on panic so the user's shell isn't left in raw
    // mode / alt screen. This runs before the default handler which prints
    // the panic message to stderr.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Best-effort terminal restore — ignore errors.
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableBracketedPaste,
            ShowMousePointer
        );

        // Dump to the debug log if active.
        let bt = std::backtrace::Backtrace::force_capture();
        debug_log::log(&format!("PANIC: {info}\n{bt}"));

        // Let the default handler print to stderr.
        default_hook(info);
    }));

    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "spyc {} — vi-keyboard-driven file commander\n\n\
             Usage: spyc [OPTIONS]\n\n\
             Options:\n  \
               -r, --resume   Open pane with `claude --resume`\n  \
               -d, --debug    Write debug log to /tmp/spyc-debug-<ts>.log\n  \
               -h, --help     Show this help\n  \
               -v, --version  Show version (add --verbose for build info)\n  \
               --mcp          Run as MCP server (stdio JSON-RPC)",
            env!("CARGO_PKG_VERSION"),
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--mcp") {
        let root = std::env::current_dir()?;
        return mcp::run(root);
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        let verbose = args.iter().any(|a| a == "--verbose");
        println!("\u{1f336}\u{fe0f} spyc {}", env!("CARGO_PKG_VERSION"));
        if verbose {
            println!("  git:     {}", env!("SPYC_GIT_SHA"));
            println!("  built:   {}", env!("SPYC_BUILD_TIME"));
            println!("  rustc:   {}", env!("SPYC_RUSTC_VERSION"));
            println!("  TERM:    {}", std::env::var("TERM").unwrap_or_default());
            println!(
                "  COLOR:   {}",
                std::env::var("COLORTERM").unwrap_or_default()
            );
            println!("  os:      {} {}", std::env::consts::OS, std::env::consts::ARCH);
        }
        return Ok(());
    }

    let resume = args.iter().any(|a| a == "--resume" || a == "-r");
    let debug = args.iter().any(|a| a == "--debug" || a == "-d");
    if let Some(p) = debug_log::init(debug) {
        eprintln!("spyc: debug log → {p}");
    }
    let mut terminal = setup_terminal()?;
    let mut app = App::new(resume);
    let result = app.run(&mut terminal);
    restore_terminal(&mut terminal)?;
    if let Some(summary) = &app.exit_summary {
        println!("\u{1f336}\u{fe0f} {summary}");
    }
    result
}

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

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        HideMousePointer
    )?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        ShowMousePointer
    )?;
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
        EnableBracketedPaste
    )?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    Ok(())
}
