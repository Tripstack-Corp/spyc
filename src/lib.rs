//! spyc — a vi-keyboard-driven terminal file commander, in the lineage of keyboard-driven file managers like spy.
//!
//! This is the crate's **library** root: it owns all the modules and the
//! [`run`] entry point. `src/main.rs` is a thin binary shim that calls
//! `spyc::run()`. The split exists so the crate also builds as a library,
//! which the `cargo-fuzz` targets in `fuzz/` link against (libFuzzer targets
//! are separate binaries). Runtime behaviour is unchanged.

mod agent;
mod app;
/// Archive browsing — see `docs/drafts/ARCHIVE_BROWSING_PLAN.md`.
///
/// Public, unlike the rest of the tree, so the round-trip tests in `tests/` —
/// which link the crate as a library — can drive a real archive all the way
/// through index → listing → materialize.
pub mod archive;
mod clipboard;
mod config;
mod context;
mod debug_log;
mod envset;
mod fs;
mod git;
#[cfg(test)]
mod guard_support;
mod key_trace;
mod keymap;
mod lua;
mod mcp;
mod mcp_cmd;
mod merge_driver;
mod notifications;
mod pane;
mod paths;
mod proc_cwd;
mod shell;
mod skill;
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
    /// Normalize one raw archive member name and discard the result.
    ///
    /// This is the boundary that makes zip-slip impossible — every member name
    /// in a downloaded archive is attacker-controlled — so it asserts the
    /// normalizer never panics and never returns a path that could escape.
    pub fn normalize_archive_name(raw: &str) {
        if let Ok(name) = crate::archive::index::normalize(raw) {
            assert!(
                !name.inner.starts_with('/') && !name.inner.split('/').any(|p| p == ".."),
                "normalized name must stay inside the mount: {:?}",
                name.inner
            );
        }
    }

    /// Drive a whole container through the mount path and assert nothing it
    /// writes lands outside the staging root.
    ///
    /// [`normalize_archive_name`] covers one member *name*; this covers the
    /// parsers that eat bytes — zip central directories, tar headers, and the
    /// gz/zst streams — plus the extraction those parsers feed. Names are only
    /// half the containment story: a member is also free to be a *symlink*, and
    /// a link the extractor creates redirects where a later member physically
    /// lands. That composition is invisible to any check that reasons about one
    /// name at a time, so the property asserted here is positional — where the
    /// bytes ended up — not lexical.
    ///
    /// The first input byte picks the container flavor and the rest is its
    /// content, so one corpus serves all four formats.
    pub fn archive_container(data: &[u8]) {
        use crate::archive::ArchiveFormat;

        // Small enough to keep executions fast; large enough that the entry cap
        // and the extract budget are both reachable rather than theoretical.
        const CAP: usize = 512;
        const BUDGET: u64 = 1 << 20;

        let Some((&flavor, body)) = data.split_first() else {
            return;
        };
        let name = match flavor % 4 {
            0 => "input.zip",
            1 => "input.tar",
            2 => "input.tar.gz",
            _ => "input.tar.zst",
        };
        let Some(format) = crate::archive::detect(name, body) else {
            return;
        };
        let Ok(sandbox) = tempfile::tempdir() else {
            return;
        };
        let root = sandbox.path();
        let archive = root.join(name);
        if std::fs::write(&archive, body).is_err() {
            return;
        }
        // Staging sits several levels down so a `..` escape lands back inside
        // the sandbox, where the walk below can see it. Rooting staging at the
        // sandbox top would let the interesting case climb out unobserved.
        let staging = root.join("mnt/a/b/staging");

        match format {
            ArchiveFormat::Zip | ArchiveFormat::Tar => {
                // Indexing writes nothing; materializing each member is where a
                // seekable container touches the filesystem.
                if let Ok(indexed) = crate::archive::read::index_seekable(&archive, format, CAP) {
                    for entry in &indexed.index.entries {
                        let _ = crate::archive::read::materialize(&archive, entry, &staging);
                    }
                }
            }
            _ => {
                let cancel = std::sync::atomic::AtomicBool::new(false);
                let _ = crate::archive::read::stream_mount(
                    &archive, format, &staging, BUDGET, CAP, &cancel,
                );
            }
        }

        assert_contained(root, &staging, &archive);
    }

    /// Every path under `root` must be the archive itself or sit inside
    /// `staging` — anything else was written through an escape.
    ///
    /// Walks with `symlink_metadata` so a link is judged by where it *is*, not
    /// by what it points at, and reports the link target when one escapes: a
    /// contained link aimed outside the mount is the step before a later member
    /// is written through it, so it is worth failing on in its own right.
    fn assert_contained(
        root: &std::path::Path,
        staging: &std::path::Path,
        archive: &std::path::Path,
    ) {
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(meta) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                if meta.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path == archive {
                    continue;
                }
                assert!(
                    path.starts_with(staging),
                    "member escaped the staging root: {} (staging {})",
                    path.display(),
                    staging.display()
                );
                if meta.is_symlink() {
                    let target = std::fs::read_link(&path).unwrap_or_default();
                    assert!(
                        !target.is_absolute(),
                        "staged symlink points at an absolute path: {} -> {}",
                        path.display(),
                        target.display()
                    );
                    // Resolve against the link's *canonical* parent, then fold
                    // `..` away. Comparing the unfolded join with `starts_with`
                    // would call `staging/x/..` contained — which is the exact
                    // lexical mistake this target exists to catch.
                    let base = path
                        .parent()
                        .and_then(|p| std::fs::canonicalize(p).ok())
                        .unwrap_or_else(|| staging.to_path_buf());
                    let mut resolved = base;
                    for part in target.components() {
                        match part {
                            std::path::Component::ParentDir => {
                                resolved.pop();
                            }
                            std::path::Component::CurDir => {}
                            other => resolved.push(other),
                        }
                    }
                    let canonical_staging =
                        std::fs::canonicalize(staging).unwrap_or_else(|_| staging.to_path_buf());
                    assert!(
                        resolved.starts_with(&canonical_staging),
                        "staged symlink points outside the staging root: {} -> {}",
                        path.display(),
                        target.display()
                    );
                }
            }
        }
    }

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
use std::sync::atomic::{AtomicBool, Ordering};

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
// `version = VERSION`, not clap's bare `version`: the bare form prints
// `CARGO_PKG_VERSION` alone, which on the CURRENT stream is the SAME string for
// every build of a whole minor cycle. The SHA is the only thing distinguishing
// them, and it is what RELEASE_ENGINEERING.md offers in place of the retired
// bump-every-PR rule.
#[command(
    name = "spyc",
    version = VERSION,
    about = "vi-keyboard-driven file commander"
)]
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

    /// Trace the agent status-reporter to mcp.log: each `--report-status` hook
    /// invocation + the env it actually saw (SPYC_MCP_SOCK / SPYC_PANE_ID). Bakes
    /// `--status-trace` into the status hooks spyc installs, so it logs even if
    /// Claude sanitizes the hook env. Off by default (the reporter fires every
    /// agent turn). Diagnose with `grep report-status <state-dir>/mcp.log`.
    #[arg(long)]
    status_trace: bool,

    /// Run as MCP server (stdio JSON-RPC)
    #[arg(long)]
    mcp: bool,

    /// Report this pane's agent activity to the running spyc and exit. Invoked
    /// by the Claude hooks spyc installs; reads `SPYC_MCP_SOCK` + `SPYC_PANE_ID`
    /// from the environment. One of: working | blocked | idle | done.
    #[arg(long, value_name = "STATE")]
    report_status: Option<String>,

    /// Print extended build info (sha, build time, rustc, TERM, os) and exit.
    /// Standalone, NOT a modifier for --version: clap handles --version itself
    /// and exits before this is read, so `--version --verbose` prints the plain
    /// version line.
    #[arg(long)]
    verbose: bool,

    /// Print a fully-commented default `.spycrc.toml` to stdout and exit.
    /// Pipe to a file to bootstrap your config:
    ///   spyc --print-config > ~/.spycrc.toml
    #[arg(long)]
    print_config: bool,

    /// Install spyc's agent skill and exit — the usage guide that teaches an
    /// agent spyc's worktree / search / git tools. Written to every host that
    /// supports personal skills: `~/.claude/skills/spyc/` (Claude Code) and
    /// `$CODEX_HOME/skills/spyc/` (codex, default `~/.codex/skills/`). Re-run to
    /// update; spyc also offers an update on startup when its embedded copy has
    /// moved on. Manage it in-app with `:skill`.
    #[arg(long)]
    install_skill: bool,

    /// Disable the embedded Lua engine for this session — no worker thread, and
    /// `map KEY lua` / init.lua won't run. The startup equivalent of `:lua off`.
    #[arg(long)]
    no_lua: bool,

    /// Color depth: `auto` (default — truecolor when $COLORTERM advertises it,
    /// else 256-color), `truecolor`, or `256`. Force `256` on terminals that
    /// can't parse 24-bit SGR (notably macOS's bundled GNU screen 4.00.03, which
    /// otherwise drops every color). Overrides `[layout] color_depth`.
    #[arg(long, value_name = "MODE")]
    color: Option<String>,

    /// Internal git merge driver for spyc's version-line conflicts. git invokes
    /// it via `.gitattributes` as `spyc --merge-driver %O %A %B`; not for direct
    /// use. Resolves the `Cargo.toml` / `Cargo.lock` version-bump conflicts that
    /// every concurrent PR collides on; exits non-zero on any real conflict.
    #[arg(long, num_args = 3, value_names = ["BASE", "CURRENT", "OTHER"], hide = true)]
    merge_driver: Option<Vec<String>>,
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

        // Dump to the debug log if active.
        let bt = std::backtrace::Backtrace::force_capture();
        debug_log::log(&format!("PANIC: {info}\n{bt}"));

        // Let the default handler print to stderr.
        default_hook(info);
    }));

    let cli = Cli::parse();

    // Record `--status-trace` so the hook installer bakes `--status-trace` into
    // the status-reporter commands it writes (the reporter then logs each fire).
    mcp::set_status_trace(cli.status_trace);

    // Disable the embedded Lua engine if requested (no worker thread spawned).
    lua::set_enabled(!cli.no_lua);

    if cli.print_config {
        print!("{}", config::DEFAULT_TEMPLATE);
        return Ok(());
    }

    if cli.install_skill {
        // Note where hand-edits were replaced: --install-skill overwrites
        // unconditionally, and that is otherwise silent.
        let before = skill::status_all();
        for (host, dir) in skill::install_all(false)? {
            let note = match before.iter().find(|(h, _)| *h == host).map(|(_, s)| s) {
                Some(skill::Status::Modified { .. }) => " (replaced your local edits)",
                _ => "",
            };
            println!(
                "\u{1f336}\u{fe0f} installed the spyc skill v{} for {} \u{2192} {}{}",
                skill::embedded_version(),
                host.label(),
                dir.display(),
                note
            );
        }
        return Ok(());
    }

    if cli.mcp {
        let root = std::env::current_dir()?;
        return mcp::run(root);
    }
    // Git merge-driver subprocess (invoked by git via `.gitattributes`, see
    // `merge_driver`). Exit non-zero on a real conflict so git reports it.
    if let Some(args) = cli.merge_driver.as_deref() {
        if let [base, current, other] = args {
            let clean = merge_driver::run_merge_driver(base, current, other)?;
            if !clean {
                std::process::exit(1);
            }
            return Ok(());
        }
        anyhow::bail!("--merge-driver expects 3 paths (%O %A %B)");
    }
    // Agent status hook reporter: a tiny one-shot that pings the running spyc.
    // Best-effort — never errors out (it runs inside the agent's lifecycle
    // hook, and must not block/break the agent if spyc is gone).
    if let Some(state) = cli.report_status.as_deref() {
        mcp::report_status_to_socket(state, cli.status_trace);
        return Ok(());
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

    // Install the version-line merge driver for this repo (idempotent,
    // best-effort) so concurrent-PR Cargo.toml/Cargo.lock conflicts auto-resolve
    // on rebase. No-op when not in a git repo or already configured.
    if let Ok(cwd) = std::env::current_dir() {
        let _ = merge_driver::ensure_installed(&cwd);
    }
    if let Some(p) = debug_log::init(cli.debug) {
        eprintln!("spyc: debug log → {p}");
    }
    if let Some(p) = key_trace::init(cli.key_trace) {
        eprintln!("spyc: key trace → {p}");
    }
    // Before the TUI starts: a stray ^C during a child takeover (less/editor/;)
    // must not bring spyc down with the child.
    install_signal_handlers();
    // Parse `--color` before touching the terminal so a typo errors cleanly
    // instead of after entering raw mode / the alt screen.
    let color_mode = match cli.color.as_deref() {
        Some(s) => s
            .parse::<config::ColorMode>()
            .map_err(|e| anyhow::anyhow!(e))?,
        None => config::ColorMode::default(),
    };
    let mcp_takeover_allowed = prompt_mcp_takeover_if_needed();
    let mut terminal = setup_terminal()?;
    let mut app = App::new(cli.resume, mcp_takeover_allowed, color_mode);
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
    let Some(old_pid) = mcp::detect_existing_spyc(&cwd)
        .or_else(|| mcp::detect_existing_spyc_codex(&cwd))
        .or_else(|| mcp::detect_existing_spyc_agy(&cwd))
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
/// [`EnableAlternateScroll`]: a terminal honoring both could deliver one tick
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
        // Clears 1002/1003 too, which spyc never ENABLES: a foreground child
        // (vim, htop) sets its own motion reporting, and one that dies without
        // resetting — SIGKILL, a crash — hands the tty back with 1002/1003 still
        // on. Clearing only our own pair would then leave motion reporting live,
        // which both defeats the 1007 exclusivity `resume_tui` re-establishes and
        // leaks motion events into the user's shell after spyc exits.
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
/// `pane::Pane` deliberately `catch_unwind`s a known vt100 panic (nvim leaving
/// the alternate screen is the documented trigger) and spyc keeps running — so
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
pub(crate) fn set_mouse_capture_for_test(on: bool) {
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
pub(crate) fn mouse_test_lock() -> std::sync::MutexGuard<'static, ()> {
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
fn detect_image_picker() -> Option<ratatui_image::picker::Picker> {
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

fn setup_terminal() -> Result<Tui> {
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
        // Blank the buffer we just entered. On terminals that honor the alt
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
        DisableWheelReporting,
        ShowMousePointer
    )?;
    MOUSE_CAPTURE_ON.store(false, Ordering::Relaxed);
    let _ = term_title::pop();
    terminal.show_cursor()?;
    Ok(())
}

/// Switch the terminal between 1007 alternate-scroll (wheel as arrow keys, native
/// selection intact) and real mouse reporting (`?1000h?1006h`).
///
/// The two are mutually exclusive on purpose — a terminal honoring both could
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
        DisableWheelReporting,
        EnableAlternateScroll
    )?;
    // `suspend_tui` already cleared this; the reconcile re-enables per config on
    // the next iteration if the user wants capture.
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
mod version_flag_tests {
    /// `--version` must carry the git SHA.
    ///
    /// RELEASE_ENGINEERING.md offers exact build identity via the SHA as the
    /// replacement for the retired bump-every-PR rule, and on the CURRENT
    /// stream `CARGO_PKG_VERSION` is identical for every build of a whole minor
    /// cycle — so clap's bare `version` leaves two builds months apart
    /// indistinguishable. This shipped broken once: the SHA reached `VERSION`
    /// (which MCP reports, so `get_spyc_context` showed it) while `--version`
    /// kept printing the bare package version, and the docs claimed otherwise.
    #[test]
    fn the_version_flag_carries_the_git_sha() {
        use clap::CommandFactory as _;
        let rendered = super::Cli::command()
            .get_version()
            .expect("clap knows a version")
            .to_string();
        assert_eq!(rendered, super::VERSION, "--version must print VERSION");
        assert!(
            rendered.contains(env!("CARGO_PKG_VERSION")),
            "keeps the package version: {rendered}"
        );
        assert!(
            rendered.contains(env!("SPYC_GIT_SHA")),
            "must name the build's SHA: {rendered}"
        );
        assert!(
            rendered.len() > env!("CARGO_PKG_VERSION").len(),
            "the bare package version is not enough: {rendered}"
        );
    }
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
    /// terminal honoring both delivers one wheel tick twice — once as arrow keys,
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
                } else if path.extension().is_some_and(|e| e == "rs")
                    && std::fs::read_to_string(&path)
                        .expect("read .rs")
                        .contains(banned)
                {
                    offenders.push(path);
                }
            }
        }

        let mut offenders = Vec::new();
        scan(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &banned,
            &mut offenders,
        );
        // This file legitimately names it in prose (the doc comment on
        // `EnableWheelReporting` explains why spyc doesn't use it).
        offenders.retain(|p| p.file_name().is_none_or(|n| n != "lib.rs"));
        assert!(
            offenders.is_empty(),
            "crossterm's EnableMouseCapture emits ?1003h (any-motion) — use \
             EnableWheelReporting instead. Offenders: {offenders:?}"
        );
    }
}

#[cfg(test)]
mod fuzz_target_registration_tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    fn repo() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn on_disk() -> BTreeSet<String> {
        std::fs::read_dir(repo().join("fuzz/fuzz_targets"))
            .expect("read fuzz_targets")
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                (p.extension()? == "rs").then(|| p.file_stem()?.to_str().map(String::from))?
            })
            .collect()
    }

    /// A target absent from `fuzz/Cargo.toml` builds nowhere; one absent from the
    /// weekly matrix builds but never runs.
    ///
    /// Both failures are silent — `archive_name` sat outside the matrix from the
    /// day it was added, so the one target covering attacker-controlled archive
    /// names had never executed in CI. Three lists, one source of truth.
    #[test]
    fn every_fuzz_target_is_registered_and_scheduled() {
        let targets = on_disk();
        assert!(!targets.is_empty(), "no fuzz targets found");

        let manifest =
            std::fs::read_to_string(repo().join("fuzz/Cargo.toml")).expect("read fuzz/Cargo.toml");
        let workflow = std::fs::read_to_string(repo().join(".github/workflows/fuzz.yml"))
            .expect("read fuzz.yml");

        for target in &targets {
            assert!(
                manifest.contains(&format!("name = \"{target}\"")),
                "fuzz target {target} has no [[bin]] in fuzz/Cargo.toml"
            );
            assert!(
                workflow.contains(&format!("- {target}")),
                "fuzz target {target} is missing from the fuzz.yml matrix — it \
                 would build but never run"
            );
        }
    }
}
