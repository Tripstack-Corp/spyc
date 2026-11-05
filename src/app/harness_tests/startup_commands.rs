//! `spyc -c` commands take effect in the order given, as if typed one after
//! another: each sees what the ones before it left, though they all run in one
//! pass of the loop.
//!
//! Every test starts in [`tree`] and moves only its column: nothing here moves
//! the process cwd.

use super::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// `root/other.txt` and `root/sub/only.txt`, each holding `needle`.
fn tree() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("sub")).unwrap();
    std::fs::write(tmp.path().join("other.txt"), "needle\n").unwrap();
    std::fs::write(tmp.path().join("sub/only.txt"), "needle\n").unwrap();
    tmp
}

/// An app whose column lists `dir`, with `commands` queued as `spyc -c`
/// queues them.
fn app_with(dir: &Path, commands: &[&str]) -> App {
    let mut app = App::test_app(dir.to_path_buf());
    app.state.refresh_listing();
    app.queue_startup_commands(commands.iter().map(ToString::to_string).collect());
    app
}

/// Run `effects` through the loop's executor.
fn run(app: &mut App, effects: Vec<Effect>) {
    use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect};
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(std::io::stdout()),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 100, 32)),
        },
    )
    .unwrap();
    let fg = crate::app::ForegroundExec {
        park: std::sync::Arc::default(),
        acked: std::sync::Arc::default(),
        reader_done: std::sync::Arc::default(),
        reader: std::thread::current(),
    };
    app.run_effects(effects, &mut terminal, &fg);
}

/// The loop's `spyc -c` step, as `settle_before_recv` drives it.
fn settle_startup(app: &mut App) {
    while let Some(fx) = app.settle_startup_commands() {
        run(app, fx);
    }
}

/// Wait for a file `pick` chooses to appear under `root`, and return it. A
/// bound for a broken spawn, not a measure of a working one.
fn first_file(root: &Path, pick: impl Fn(&Path) -> bool) -> PathBuf {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let found = [root.to_path_buf(), root.join("sub")]
            .iter()
            .filter_map(|dir| std::fs::read_dir(dir).ok())
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| pick(p));
        if let Some(path) = found {
            return path;
        }
        assert!(Instant::now() < deadline, "no command ran");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The file named `here`, wherever a command made it.
fn here(p: &Path) -> bool {
    p.file_name().is_some_and(|n| n == "here")
}

/// `spyc -c 'cd sub' -c '!touch here'`: the capture runs where the `cd` went.
#[test]
#[ignore = "red: the capture starts before the `cd` lands"]
fn a_capture_runs_where_an_earlier_cd_went() {
    let state = tempfile::tempdir().unwrap();
    let tree = tree();
    crate::state::with_state_root(state.path(), || {
        let mut app = app_with(tree.path(), &["cd sub", "!touch here"]);

        settle_startup(&mut app);

        assert_eq!(
            first_file(tree.path(), here),
            tree.path().join("sub/here"),
            "{:?}",
            app.flash_text()
        );
    });
}

/// `spyc -c 'cd sub' -c ';touch here'`: the command runs where the `cd` went.
#[test]
#[ignore = "red: the command starts before the `cd` lands"]
fn a_foreground_command_runs_where_an_earlier_cd_went() {
    let state = tempfile::tempdir().unwrap();
    let tree = tree();
    crate::state::with_state_root(state.path(), || {
        let mut app = app_with(tree.path(), &["cd sub", ";touch here"]);

        settle_startup(&mut app);

        assert_eq!(
            first_file(tree.path(), here),
            tree.path().join("sub/here"),
            "{:?}",
            app.flash_text()
        );
    });
}

/// `spyc -c 'cd sub' -c '!touch %.seen'`: `%` names the cursor row of the
/// directory the `cd` went to.
#[test]
#[ignore = "red: `%` is expanded before the `cd` lands"]
fn percent_names_the_row_an_earlier_cd_left_the_cursor_on() {
    let state = tempfile::tempdir().unwrap();
    let tree = tree();
    crate::state::with_state_root(state.path(), || {
        let mut app = app_with(tree.path(), &["cd sub", "!touch %.seen"]);

        settle_startup(&mut app);

        let seen = first_file(tree.path(), |p| p.extension().is_some_and(|e| e == "seen"));
        assert_eq!(
            seen,
            tree.path().join("sub/only.txt.seen"),
            "{:?}",
            app.flash_text()
        );
    });
}

/// `spyc -c 'cd sub' -c 'grep needle'`: the pager searches from where the
/// `cd` went.
#[test]
#[ignore = "red: the search starts before the `cd` lands"]
fn a_grep_searches_from_where_an_earlier_cd_went() {
    let state = tempfile::tempdir().unwrap();
    let tree = tree();
    crate::state::with_state_root(state.path(), || {
        let mut app = app_with(tree.path(), &["cd sub", "grep needle"]);

        settle_startup(&mut app);

        let deadline = Instant::now() + Duration::from_secs(30);
        let pager = loop {
            app.drain_pager_stream();
            let pager = app.view.project().pager.as_ref().expect("a pager opened");
            if !pager.streaming {
                break pager;
            }
            assert!(Instant::now() < deadline, "the search never finished");
            std::thread::sleep(Duration::from_millis(20));
        };
        let lines: Vec<String> = pager.lines.iter().map(ToString::to_string).collect();
        assert!(
            lines.iter().any(|l| l.contains("only.txt")),
            "{}: {lines:?}",
            pager.title
        );
        assert!(
            !lines.iter().any(|l| l.contains("other.txt")),
            "the search started above the `cd`: {lines:?}"
        );
    });
}

/// `spyc -c 'there' -c '!touch here'`, `there` an `init.lua` command that
/// navigates into `sub`: the capture waits for the Lua run to land.
#[test]
#[ignore = "red: the capture starts before the Lua run lands"]
fn a_command_after_a_lua_command_waits_for_it_to_land() {
    use crate::lua::LuaWorker;

    let state = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let tree = tree();
    std::fs::write(
        config.path().join("init.lua"),
        format!(
            "spyc.command('there', function() spyc.navigate([[{}]]) end)",
            tree.path().join("sub").display()
        ),
    )
    .unwrap();
    crate::state::with_state_root(state.path(), || {
        let mut app = app_with(tree.path(), &["there", "!touch here"]);
        let (tx, rx) = std::sync::mpsc::channel::<Message>();
        app.runtime.pane_wake_tx = Some(tx.clone());
        app.runtime.lua = Some(LuaWorker::spawn(move || {
            let _ = tx.send(Message::Wake(Wake::Lua));
        }));
        let land_lua = |app: &mut App| {
            rx.recv_timeout(Duration::from_secs(10))
                .expect("a Lua run landed");
            let (_, fx) = app.handle_lua_done();
            run(app, fx);
        };
        crate::state::with_config_root(config.path(), || app.load_init_lua());
        land_lua(&mut app);

        settle_startup(&mut app);
        land_lua(&mut app);
        settle_startup(&mut app);

        assert_eq!(
            first_file(tree.path(), here),
            tree.path().join("sub/here"),
            "{:?}",
            app.flash_text()
        );
    });
}

/// `spyc -c 'setenv' -c 'sort mtime'`: a prompt the first command opens holds
/// the second until it's answered, as a typed `:sort mtime` would have gone
/// into it.
#[test]
#[ignore = "red: `sort` runs under the prompt `setenv` opened"]
fn a_prompt_an_earlier_command_opens_holds_the_rest() {
    let state = tempfile::tempdir().unwrap();
    let tree = tree();
    crate::state::with_state_root(state.path(), || {
        let mut app = app_with(tree.path(), &["setenv", "sort mtime"]);

        settle_startup(&mut app);
        assert!(matches!(app.state.mode, Mode::Prompting(_)), "setenv asks");
        assert_eq!(
            app.state.cur().sort_order,
            crate::fs::listing::SortMode::Name,
            "`sort` ran under the prompt"
        );

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()))
            .unwrap();
        settle_startup(&mut app);
        assert_eq!(
            app.state.cur().sort_order,
            crate::fs::listing::SortMode::Mtime,
            "`sort` ran once the prompt closed"
        );
    });
}
