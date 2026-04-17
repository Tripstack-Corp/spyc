//! spyc — a vi-keyboard-driven file commander inspired by SideFX's spy.

mod app;
mod config;
mod debug_log;
mod fs;
mod keymap;
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
        disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::App;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "spyc {} — vi-keyboard-driven file commander\n\n\
             Usage: spyc [OPTIONS]\n\n\
             Options:\n  \
               -r, --resume   Open pane with `claude --resume`\n  \
               -d, --debug    Write debug log to /tmp/spyc-debug-<ts>.log\n  \
               -h, --help     Show this help\n  \
               -v, --version  Show version",
            env!("CARGO_PKG_VERSION"),
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("spyc {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let resume = args.iter().any(|a| a == "--resume" || a == "-r");
    let debug = args.iter().any(|a| a == "--debug" || a == "-d");
    if let Some(p) = debug_log::init(debug) {
        eprintln!("spyc: debug log → {p}");
    }
    let mut terminal = setup_terminal()?;
    let result = App::new(resume)?.run(&mut terminal);
    restore_terminal(&mut terminal)?;
    result
}

pub type Tui = Terminal<CrosstermBackend<io::Stdout>>;

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste
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
        DisableBracketedPaste
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
