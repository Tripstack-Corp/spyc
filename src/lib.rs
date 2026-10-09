//! spyc — a vi-keyboard-driven terminal file commander, in the lineage of keyboard-driven file managers like spy.
//!
//! This is the crate's **library** root: it owns all the modules and the
//! [`run`] entry point. `src/main.rs` is a thin binary shim that calls
//! `spyc::run()`. The split exists so the crate also builds as a library,
//! which the `cargo-fuzz` targets in `fuzz/` link against (libFuzzer targets
//! are separate binaries). Runtime behaviour is unchanged.

mod agent;
mod app;
/// Archive browsing — see `docs/archive/ARCHIVE_BROWSING_PLAN.md`.
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
pub mod fuzz;
mod fuzz_support;
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
mod terminal;
mod ui;

/// Human-readable build identity, e.g. `1.59.0 (25abd0a)`.
///
/// The crate version plus the short git SHA baked in at build time
/// (`build.rs`). The SHA changes every commit, so this is the signal that tells
/// an MCP client whether the running spyc predates a tool it expects —
/// surfaced over MCP via the `initialize` `serverInfo` and `get_spyc_context`.
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("SPYC_GIT_SHA"), ")");

use anyhow::Result;
use clap::Parser;

use crate::app::App;
pub use crate::terminal::{
    Tui, force_full_repaint, mouse_capture_is_on, resume_tui, set_mouse_capture, suspend_tui,
};
use crate::terminal::{
    detect_image_picker, install_signal_handlers, restore_terminal, setup_terminal,
};
#[cfg(test)]
pub(crate) use crate::terminal::{mouse_test_lock, set_mouse_capture_for_test};

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

    /// Run a `:` command once spyc has started: after `init.lua` loads, a `-r`
    /// session is picked, and any startup question is answered. Repeatable,
    /// run in order; the leading `:` is optional:
    ///   spyc -c "sort mtime" -c "limit *.rs"
    #[arg(short = 'c', long = "cmd", value_name = "CMD")]
    cmd: Vec<String>,

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
    /// invocation, bounded event metadata and routing env (SPYC_MCP_SOCK /
    /// SPYC_PANE_ID), never hook arguments, prompts or responses. Bakes
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

    /// Colour depth: `auto` (default — truecolor when $COLORTERM advertises it,
    /// else 256-colour), `truecolor`, or `256`. Force `256` on terminals that
    /// can't parse 24-bit SGR (notably macOS's bundled GNU screen 4.00.03, which
    /// otherwise drops every colour). Overrides `[layout] color_depth`.
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
        terminal::restore_on_panic();

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
    let mut terminal = setup_terminal()?;
    let mut app = App::new(cli.resume, color_mode);
    app.queue_startup_commands(cli.cmd);
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
mod fn_size_guard;

#[cfg(test)]
mod size_guard;

#[cfg(test)]
mod style_guard;

#[cfg(test)]
mod pages_artifact_guards {
    use std::path::{Path, PathBuf};

    fn repo() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn workflows() -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(repo().join(".github/workflows"))
            .expect("read .github/workflows")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "yml" || e == "yaml"))
            .collect();
        found.sort();
        found
    }

    /// Line range of the workflow step containing `uses_idx`, as `[start, end)`.
    ///
    /// A step is its `- ` bullet plus every line indented past it, so the span
    /// ends at the next bullet at the same indent or at any dedent.
    fn step_span(lines: &[&str], uses_idx: usize) -> (usize, usize) {
        let indent_of = |l: &str| l.len() - l.trim_start().len();
        let mut start = uses_idx;
        while start > 0 && !lines[start].trim_start().starts_with("- ") {
            start -= 1;
        }
        let indent = indent_of(lines[start]);
        let mut end = start + 1;
        while end < lines.len() {
            let line = lines[end];
            if !line.trim().is_empty() && indent_of(line) <= indent {
                break;
            }
            end += 1;
        }
        (start, end)
    }

    /// `actions/upload-pages-artifact` v4 began excluding dotfiles from the
    /// tarball (`--exclude=.[^/]*`), so the archive's `.nojekyll` reaches Pages
    /// only while the step opts back in with `include-hidden-files: true`.
    ///
    /// No linter can see this: the workflow stays valid YAML, every action
    /// input is legal, and `apt.yml` never runs on a PR — it fires on release.
    /// The v3 -> v5 bump (#444) passed a fully green check set while silently
    /// dropping the file, and Pages has no rollback.
    #[test]
    fn every_pages_upload_keeps_hidden_files() {
        let mut checked = 0;
        for path in workflows() {
            let src = std::fs::read_to_string(&path).expect("read workflow");
            let lines: Vec<&str> = src.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if !line.contains("uses:") || !line.contains("actions/upload-pages-artifact") {
                    continue;
                }
                checked += 1;
                let (start, end) = step_span(&lines, i);
                let value = lines[start..end]
                    .iter()
                    .find_map(|l| l.trim().strip_prefix("include-hidden-files:"))
                    .map(|v| v.trim().trim_matches(|c| c == '"' || c == '\''));
                assert_eq!(
                    value,
                    Some("true"),
                    "{}: the upload-pages-artifact step must set \
                     `include-hidden-files: true` (found {value:?}). Without it the \
                     action drops every dotfile from the artifact, which silently \
                     removes the archive's .nojekyll from the deployed site.",
                    path.display(),
                );
            }
        }
        // A rename of the action would otherwise leave this guard passing while
        // inspecting nothing — the fail-open shape src/guard_support.rs exists
        // to document.
        assert!(
            checked > 0,
            "no upload-pages-artifact step found in any workflow — this guard \
             is inspecting nothing; retarget it or delete it"
        );
    }

    /// The other half of the pair above: the flag is only load-bearing while
    /// something in the archive actually starts with a dot. If that write goes
    /// away this fails, so the flag's continued value gets a human decision
    /// rather than quietly becoming cargo cult.
    #[test]
    fn the_apt_archive_still_writes_the_dotfile_the_flag_protects() {
        let apt = std::fs::read_to_string(repo().join(".github/workflows/apt.yml"))
            .expect("read apt.yml");
        assert!(
            apt.lines().any(|l| l.trim() == "touch .nojekyll"),
            "apt.yml no longer writes .nojekyll — decide whether \
             `include-hidden-files: true` on the upload step is still needed, \
             then update or drop this guard and its pair"
        );
    }
}
