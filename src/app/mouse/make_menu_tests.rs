//! The make menu: a Makefile row's `make target …`, `Space m` and `:make` list
//! the Makefile's targets, read off the loop, and a row runs one as a `!`
//! capture. Each case goes through `handle_key`, `handle_mouse` or
//! `dispatch_command`, runs the effects as the loop does, lands the worker's
//! read through the pre-recv drain, and reads the menu as the frame draws it.

use super::menu_tests::{cursor_on, flash, menu_lines, offers, right_click};
use super::tests::{FRAME, centre, click, drawn, drawn_row, press};
use crate::app::{App, Effect, ForegroundExec, RunCtx};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};
use std::path::Path;
use std::time::{Duration, Instant};

const MAKEFILE: &str = "\
.PHONY: build test nap
build: ## Build it
\t@true
test: ## Run the tests
\t@true
nap: ## Sleep a while
\t@sleep 5
";

/// Run `body` over a fresh state root, with mouse reporting on, as a click
/// needs it.
pub(super) fn with_mouse(body: impl FnOnce(&Path)) {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        crate::set_mouse_capture_for_test(true);
        body(tmp.path());
        crate::set_mouse_capture_for_test(false);
    });
}

/// An App over `dir`, which holds `files` (`(name, text)`) and a note.
pub(super) fn app_over(dir: &Path, files: &[(&str, &str)]) -> App {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "x").unwrap();
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let mut app = App::test_app(dir.to_path_buf());
    app.view.term_size = (FRAME.width, FRAME.height);
    app.state.refresh_listing();
    app
}

pub(super) fn terminal_and_foreground() -> (crate::Tui, ForegroundExec) {
    let terminal = Terminal::with_options(
        CrosstermBackend::new(std::io::stdout()),
        TerminalOptions {
            viewport: Viewport::Fixed(FRAME),
        },
    )
    .unwrap();
    let fg = ForegroundExec {
        park: std::sync::Arc::default(),
        acked: std::sync::Arc::default(),
        reader_done: std::sync::Arc::default(),
        reader: std::thread::current(),
    };
    (terminal, fg)
}

/// Run `fx` as the loop does, then drain what the workers land until `done`,
/// for five seconds at most. Whether `done` came true.
fn land(app: &mut App, fx: Vec<Effect>, done: impl Fn(&App) -> bool) -> bool {
    let (mut terminal, fg) = terminal_and_foreground();
    app.run_effects(fx, &mut terminal, &fg);
    let mut ctx = RunCtx::for_test();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        if Instant::now() >= deadline {
            return false;
        }
        app.drain_landed_work(&mut ctx, &mut terminal, &fg);
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

fn menu_title(app: &App) -> Option<String> {
    app.view.chord_hint.as_ref().map(|h| h.title.clone())
}

fn make_menu_open(app: &App) -> bool {
    menu_title(app).is_some_and(|t| t.starts_with("make · "))
}

/// Land the make menu `fx` asks for, failing the test if it doesn't open.
pub(super) fn land_make_menu(app: &mut App, fx: Vec<Effect>) {
    assert!(
        land(app, fx, make_menu_open),
        "the make menu did not open: title {:?}, flash {:?}",
        menu_title(app),
        flash(app)
    );
}

/// `Space m` from the file list.
pub(super) fn space_m(app: &mut App) -> Vec<Effect> {
    let mut fx = key(app, ' ');
    fx.extend(key(app, 'm'));
    fx
}

pub(super) fn key(app: &mut App, c: char) -> Vec<Effect> {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
        .unwrap()
}

pub(super) fn special(app: &mut App, code: KeyCode) -> Vec<Effect> {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
        .unwrap()
}

fn text(buf: &ratatui::buffer::Buffer, y: u16, x: u16, right: u16) -> String {
    (x..right).map(|c| buf[(c, y)].symbol()).collect()
}

/// The box whose top border carries ` make · `: its rect, its title row and
/// its body lines, as drawn on a `frame`-sized screen.
pub(super) fn make_menu_drawn(app: &mut App, frame: Rect) -> (Rect, String, Vec<String>) {
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(
        frame.width,
        frame.height,
    ))
    .unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    for y in 0..frame.height {
        for x in 0..frame.width {
            if buf[(x, y)].symbol() != "┌" {
                continue;
            }
            let Some(right) = (x + 1..frame.width).find(|&c| buf[(c, y)].symbol() == "┐") else {
                continue;
            };
            let title = text(&buf, y, x, right + 1);
            if !title.contains(" make · ") {
                continue;
            }
            let bottom = (y + 1..frame.height)
                .find(|&r| buf[(x, r)].symbol() == "└")
                .expect("the menu's bottom-left corner");
            let lines = (y + 1..bottom)
                .map(|r| text(&buf, r, x + 1, right))
                .collect();
            return (Rect::new(x, y, right - x + 1, bottom - y + 1), title, lines);
        }
    }
    panic!("no make menu is drawn");
}

/// Where `row` is drawn in the open make menu: where a click aims.
pub(super) fn make_row_at(app: &mut App, row: &str) -> (u16, u16) {
    let (menu, _, lines) = make_menu_drawn(app, FRAME);
    for (i, line) in lines.iter().enumerate() {
        if let Some(byte) = line.find(row) {
            let col = line[..byte].chars().count();
            return (
                menu.x + 1 + u16::try_from(col).unwrap(),
                menu.y + 1 + u16::try_from(i).unwrap(),
            );
        }
    }
    panic!("no row {row:?} in the make menu: {lines:#?}");
}

/// Where `text` is first drawn on the screen.
fn drawn_at(app: &mut App, needle: &str) -> (u16, u16) {
    let buf = drawn(app);
    for y in 0..FRAME.height {
        let line = text(&buf, y, 0, FRAME.width);
        if let Some(byte) = line.find(needle) {
            return (u16::try_from(line[..byte].chars().count()).unwrap(), y);
        }
    }
    panic!("{needle:?} is not drawn");
}

pub(super) fn pager_title(app: &App) -> Option<String> {
    app.view.project().pager.as_ref().map(|v| v.title.clone())
}

pub(super) fn pager_flash(app: &App) -> Option<String> {
    app.view
        .project()
        .pager
        .as_ref()
        .and_then(|v| v.flash.clone())
}

/// Drain the running capture until it has exited and been finalized, then
/// close its pager, back to the file list.
fn finish_capture(app: &mut App) {
    assert!(
        land(app, Vec::new(), |app| app.runtime.pending_capture.is_none()),
        "the capture never finished: {:?}",
        pager_title(app)
    );
    key(app, 'q');
    assert!(pager_title(app).is_none(), "the finished pager closed");
}

// ── the entry menu's row ──

/// A right-click on a `Makefile` row puts `make target …  Space m` first in its
/// menu, and a click on that row opens `make · Makefile`, once the read lands,
/// with the targets, their labels and the default goal.
#[test]
fn a_right_click_on_a_makefile_offers_its_targets_first() {
    with_mouse(|tmp| {
        let mut app = app_over(&tmp.join("work"), &[("Makefile", MAKEFILE)]);
        let row = drawn_at(&mut app, "Makefile");

        right_click(&mut app, row);

        let lines = menu_lines(&mut app, "Makefile");
        assert!(
            lines[0].contains("Space m") && lines[0].contains("make target …"),
            "the first row: {lines:#?}"
        );
        let row = drawn_row(&mut app, "Makefile", "make target …");
        let fx = click(&mut app, row);
        land_make_menu(&mut app, fx);
        let (_, title, lines) = make_menu_drawn(&mut app, FRAME);
        assert!(title.contains(" make · Makefile "), "{title}");
        assert!(
            offers(&lines, ":make build  Build it (default)"),
            "{lines:#?}"
        );
        assert!(offers(&lines, ":make test  Run the tests"), "{lines:#?}");
        assert!(offers(&lines, ":make nap  Sleep a while"), "{lines:#?}");
    });
}

/// A plain file's menu has no make row.
#[test]
fn a_plain_files_menu_has_no_make_row() {
    with_mouse(|tmp| {
        let mut app = app_over(&tmp.join("work"), &[("Makefile", MAKEFILE)]);
        cursor_on(&mut app, "notes.txt");
        press(&mut app, '.');
        let lines = menu_lines(&mut app, "notes.txt");
        assert!(offers(&lines, "toggle pick"), "premise: {lines:#?}");
        assert!(!offers(&lines, "Space m"), "{lines:#?}");
    });
}

// ── running a target ──

/// A click on a target row starts a live `!` capture of it, titled
/// `! make <target>`, and names `:make <target>`; the open capture's menu then
/// offers to background it.
#[test]
fn a_click_on_a_target_runs_it_as_a_capture() {
    with_mouse(|tmp| {
        let mut app = app_over(&tmp.join("work"), &[("Makefile", MAKEFILE)]);
        let fx = space_m(&mut app);
        land_make_menu(&mut app, fx);

        let row = make_row_at(&mut app, ":make nap");
        click(&mut app, row);

        assert!(app.runtime.pending_capture.is_some(), "a capture runs");
        let title = pager_title(&app).unwrap_or_default();
        assert!(title.contains("! make nap"), "{title}");
        let hint = pager_flash(&app).unwrap_or_default();
        assert!(
            hint.contains(":make nap"),
            "the hint names the command: {hint:?}"
        );
        drawn(&mut app);
        right_click(&mut app, centre(FRAME));
        let buf = drawn(&mut app);
        let screen: String = (0..FRAME.height)
            .map(|y| text(&buf, y, 0, FRAME.width))
            .collect();
        assert!(
            screen.contains("background the capture"),
            "the capture's menu backgrounds it"
        );
    });
}

/// `Enter` on a row highlighted with the arrows runs it too.
#[test]
fn arrows_and_enter_run_a_target() {
    with_mouse(|tmp| {
        let mut app = app_over(&tmp.join("work"), &[("Makefile", MAKEFILE)]);
        let fx = space_m(&mut app);
        land_make_menu(&mut app, fx);

        special(&mut app, KeyCode::Down);
        special(&mut app, KeyCode::Down);
        special(&mut app, KeyCode::Enter);

        let title = pager_title(&app).unwrap_or_default();
        assert!(title.contains("! make test"), "the second row: {title}");
    });
}

// ── `Space m` and `:make` ──

/// `Space m` opens the menu for the focused column's Makefile; where there is
/// none, it says so.
#[test]
fn space_m_opens_the_menu_or_says_there_is_no_makefile() {
    with_mouse(|tmp| {
        let mut app = app_over(&tmp.join("work"), &[("Makefile", MAKEFILE)]);
        let fx = space_m(&mut app);
        land_make_menu(&mut app, fx);

        let bare = tmp.join("bare");
        let mut app = app_over(&bare, &[]);
        let fx = space_m(&mut app);
        let said = |app: &App| flash(app).is_some_and(|f| f.contains("no Makefile"));
        assert!(land(&mut app, fx, said), "flash {:?}", flash(&app));
        assert_eq!(
            flash(&app).as_deref(),
            Some(format!("no Makefile in {}", bare.display()).as_str())
        );
        assert!(app.view.chord_hint.is_none(), "and no menu opens");
    });
}

/// `:make build` runs `build`; a bare `:make` opens the menu.
#[test]
fn make_runs_a_target_and_bare_make_opens_the_menu() {
    with_mouse(|tmp| {
        let mut app = app_over(&tmp.join("work"), &[("Makefile", MAKEFILE)]);
        app.dispatch_command("make build");
        let title = pager_title(&app).unwrap_or_default();
        assert!(title.contains("! make build"), "{title}");
        finish_capture(&mut app);

        let mut app = app_over(&tmp.join("again"), &[("Makefile", MAKEFILE)]);
        let fx = app.dispatch_command("make");
        land_make_menu(&mut app, fx);
    });
}

/// A target a shell would misread runs as one quoted word: a name holding `;`
/// or a backtick, listed from the Makefile or typed after `:make`, runs make
/// and nothing else.
#[test]
fn a_hostile_target_runs_as_one_quoted_word() {
    with_mouse(|tmp| {
        let dir = tmp.join("work");
        let hostile = "\
x;touch>pwned-semi: ## Semicolon
\t@true
y`touch>pwned-tick`: ## Backtick
\t@true
";
        let mut app = app_over(&dir, &[("Makefile", hostile)]);
        for (row, word) in [
            ("Semicolon", "'x;touch>pwned-semi'"),
            ("Backtick", "'y`touch>pwned-tick`'"),
        ] {
            let fx = space_m(&mut app);
            land_make_menu(&mut app, fx);
            let at = make_row_at(&mut app, row);
            click(&mut app, at);
            let title = pager_title(&app).unwrap_or_default();
            assert!(title.contains(&format!("! make {word}")), "{title}");
            finish_capture(&mut app);
        }
        app.dispatch_command("make z;touch>pwned-typed");
        let title = pager_title(&app).unwrap_or_default();
        assert!(title.contains("! make 'z;touch>pwned-typed'"), "{title}");
        finish_capture(&mut app);

        for marker in ["pwned-semi", "pwned-tick", "pwned-typed"] {
            assert!(!dir.join(marker).exists(), "{marker}: the shell ran more");
        }
    });
}

/// `:make fmt lint` builds both targets, as `make fmt lint` does.
#[test]
fn make_with_two_words_builds_both_targets() {
    with_mouse(|tmp| {
        let dir = tmp.join("work");
        let text = "fmt:\n\t@touch fmt.done\nlint:\n\t@touch lint.done\n";
        let mut app = app_over(&dir, &[("Makefile", text)]);

        app.dispatch_command("make fmt lint");
        let title = pager_title(&app).unwrap_or_default();
        finish_capture(&mut app);

        assert!(title.contains("! make fmt lint"), "{title}");
        assert!(dir.join("fmt.done").exists(), "fmt ran");
        assert!(dir.join("lint.done").exists(), "lint ran");
    });
}

/// `:make check V=1` passes the variable to make, as `make check V=1` does.
#[test]
fn make_passes_a_variable() {
    with_mouse(|tmp| {
        let dir = tmp.join("work");
        let text = "check:\n\t@echo \"$(V)\" > v.out\n";
        let mut app = app_over(&dir, &[("Makefile", text)]);

        app.dispatch_command("make check V=1");
        finish_capture(&mut app);

        let out = std::fs::read_to_string(dir.join("v.out")).unwrap_or_default();
        assert_eq!(out.trim(), "1", "the recipe saw V");
    });
}

/// Several typed words a shell would misread each run as one quoted word: a
/// `;`, a backtick and a `$(` run make and nothing else.
#[test]
fn typed_words_a_shell_would_misread_run_nothing_else() {
    with_mouse(|tmp| {
        let dir = tmp.join("work");
        let mut app = app_over(&dir, &[("Makefile", "ok:\n\t@true\n")]);

        app.dispatch_command("make ok x;touch>pwned-semi `touch pwned-tick` $(touch pwned-sub)");
        finish_capture(&mut app);

        for marker in ["pwned-semi", "pwned-tick", "pwned-sub"] {
            assert!(!dir.join(marker).exists(), "{marker}: the shell ran more");
        }
    });
}

/// A read that lands once the user has opened another menu is dropped: the
/// menu they opened stays.
#[test]
fn a_read_that_lands_after_another_menu_opened_is_dropped() {
    with_mouse(|tmp| {
        let mut app = app_over(&tmp.join("work"), &[("Makefile", MAKEFILE)]);
        cursor_on(&mut app, "notes.txt");
        let fx = space_m(&mut app);
        press(&mut app, '.');
        let entry_menu = |app: &App| menu_title(app).is_some_and(|t| t.starts_with("notes.txt"));
        assert!(entry_menu(&app), "premise: {:?}", menu_title(&app));

        let replaced = land(&mut app, fx, |app| !entry_menu(app));

        assert!(!replaced, "the read replaced it: {:?}", menu_title(&app));
    });
}

// ── where it applies ──

/// Inside a mounted archive a Makefile is a member, not a file make can read:
/// `Space m` and `:make` are refused, and the member's menu has no make row.
#[test]
fn the_make_menu_is_refused_inside_a_mounted_archive() {
    use crate::app::archive_ops::{ArchiveOp, ArchiveOutcome, run_archive_op};
    use std::io::Write as _;
    with_mouse(|tmp| {
        // Canonical, as `chdir` makes a column's directory.
        let canonical = std::fs::canonicalize(tmp).unwrap();
        let tmp = canonical.as_path();
        let archive = tmp.join("pkg.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
        for (name, body) in [("Makefile", MAKEFILE), ("README.md", "# pkg\n")] {
            w.start_file(name, opts).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap();
        let mut app = App::test_app(tmp.to_path_buf());
        app.view.term_size = (FRAME.width, FRAME.height);
        let outcome = run_archive_op(ArchiveOp::Mount {
            path: archive.clone(),
            staging_root: tmp.join("staging"),
            limits: app.state.active_project().config.archive.limits(),
            max_entries: 1000,
            confirmed: false,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            then: None,
            address: None,
            depth: 0,
            reset_staging: false,
            hold: None,
            watch: None,
        });
        assert!(
            matches!(outcome, ArchiveOutcome::Mounted { .. }),
            "{outcome:?}"
        );
        app.file_archive_outcome(outcome);
        app.apply_archive_outcomes();
        assert_eq!(app.state.cur().listing.dir, archive, "premise: mounted");
        cursor_on(&mut app, "Makefile");

        let fx = space_m(&mut app);
        assert!(
            fx.iter().all(|e| matches!(e, Effect::SaveLearned(_))),
            "nothing is read, only the key counted: {fx:?}"
        );
        assert!(
            flash(&app).is_some_and(|f| f.starts_with("archive: ")),
            "{:?}",
            flash(&app)
        );
        let fx = app.dispatch_command("make build");
        assert!(fx.is_empty() && app.runtime.pending_capture.is_none());
        assert!(
            flash(&app).is_some_and(|f| f.starts_with("archive: ")),
            "{:?}",
            flash(&app)
        );
        press(&mut app, '.');
        let lines = menu_lines(&mut app, "Makefile");
        assert!(offers(&lines, "toggle pick"), "premise: {lines:#?}");
        assert!(!offers(&lines, "Space m"), "{lines:#?}");
    });
}

/// Make a FIFO named `Makefile` in `dir`.
#[cfg(unix)]
fn fifo_makefile(dir: &Path) {
    assert!(
        std::process::Command::new("mkfifo")
            .arg(dir.join("Makefile"))
            .status()
            .unwrap()
            .success()
    );
}

/// A FIFO named `Makefile` is no Makefile, so its menu has no make row.
#[cfg(unix)]
#[test]
fn a_fifo_named_makefile_has_no_make_row() {
    with_mouse(|tmp| {
        let dir = tmp.join("work");
        let mut app = app_over(&dir, &[]);
        fifo_makefile(&dir);
        app.state.refresh_listing();
        cursor_on(&mut app, "Makefile");

        press(&mut app, '.');

        let lines = menu_lines(&mut app, "Makefile");
        assert!(offers(&lines, "toggle pick"), "premise: {lines:#?}");
        assert!(!offers(&lines, "Space m"), "{lines:#?}");
    });
}

/// `Space m` on a FIFO named `Makefile` reads the directory, which holds no
/// Makefile, rather than the FIFO.
#[cfg(unix)]
#[test]
fn space_m_on_a_fifo_named_makefile_reads_the_directory() {
    with_mouse(|tmp| {
        let dir = tmp.join("work");
        let mut app = app_over(&dir, &[]);
        fifo_makefile(&dir);
        app.state.refresh_listing();
        cursor_on(&mut app, "Makefile");

        let fx = space_m(&mut app);

        let said = |app: &App| flash(app).is_some_and(|f| f.contains("Makefile"));
        assert!(land(&mut app, fx, said), "flash {:?}", flash(&app));
        assert_eq!(
            flash(&app).as_deref(),
            Some(format!("no Makefile in {}", dir.display()).as_str())
        );
        assert!(app.view.chord_hint.is_none(), "and no menu opens");
    });
}

/// A Makefile of pattern rules alone names no target: the menu says so in a
/// row that runs nothing.
#[test]
fn a_makefile_without_explicit_targets_says_so() {
    with_mouse(|tmp| {
        let patterns = "%.o: %.c\n\tcc -c $<\n";
        let mut app = app_over(&tmp.join("work"), &[("Makefile", patterns)]);
        let fx = space_m(&mut app);
        land_make_menu(&mut app, fx);
        let (_, _, lines) = make_menu_drawn(&mut app, FRAME);
        assert!(offers(&lines, "make: no explicit targets"), "{lines:#?}");

        let at = make_row_at(&mut app, "make: no explicit targets");
        let fx = click(&mut app, at);

        assert!(fx.is_empty(), "{fx:?}");
        assert!(app.runtime.pending_capture.is_none(), "nothing runs");
        assert!(pager_title(&app).is_none(), "no pager opens");
    });
}

/// Beside a `GNUmakefile`, make would read that, so the `Makefile` the menu
/// listed runs with `-f`.
#[test]
fn a_makefile_make_would_not_pick_runs_with_f() {
    with_mouse(|tmp| {
        let mut app = app_over(
            &tmp.join("work"),
            &[
                ("GNUmakefile", "gnu:\n\t@true\n"),
                ("Makefile", "plain: ## Plain\n\t@true\n"),
            ],
        );
        let row = drawn_at(&mut app, "Makefile");
        right_click(&mut app, row);
        let row = drawn_row(&mut app, "Makefile", "make target …");
        let fx = click(&mut app, row);
        land_make_menu(&mut app, fx);
        let (_, title, lines) = make_menu_drawn(&mut app, FRAME);
        assert!(title.contains(" make · Makefile "), "{title}");
        assert!(!offers(&lines, ":make gnu"), "{lines:#?}");

        let at = make_row_at(&mut app, ":make plain");
        click(&mut app, at);

        let title = pager_title(&app).unwrap_or_default();
        assert!(title.contains("! make -f Makefile plain"), "{title}");
    });
}

// ── a long list ──

/// Forty targets don't fit an 80×24 frame: the menu keeps the rows it can
/// draw, every one of them clickable, and ends in a row naming how many more
/// `:make <target>` reaches. The arrows wrap onto that last drawn row.
#[test]
fn a_long_target_list_ends_in_a_row_naming_the_rest() {
    use std::fmt::Write as _;
    with_mouse(|tmp| {
        let mut text = String::new();
        for i in 1..=40 {
            writeln!(text, "target-{i:02}: ## Label number {i:02}\n\t@true").unwrap();
        }
        let mut app = app_over(&tmp.join("work"), &[("Makefile", &text)]);
        let small = Rect::new(0, 0, 80, 24);
        app.view.term_size = (small.width, small.height);
        let fx = space_m(&mut app);
        land_make_menu(&mut app, fx);

        let (menu, _, lines) = make_menu_drawn(&mut app, small);
        assert!(menu.bottom() <= small.bottom(), "the box fits the frame");
        let shown = lines.iter().filter(|l| l.contains(":make target-")).count();
        let (count, _) = lines
            .last()
            .and_then(|l| l.trim().strip_prefix("… "))
            .and_then(|l| l.split_once(" more: :make <target>"))
            .unwrap_or_else(|| panic!("no closing row: {lines:#?}"));
        let more: usize = count.trim().parse().expect("a count");
        assert_eq!(shown + more, 40, "{lines:#?}");
        let entries = app.view.chord_hint.as_ref().map_or(0, |h| h.entries.len());
        assert_eq!(entries, shown + 1, "every entry is drawn");

        special(&mut app, KeyCode::Up);
        special(&mut app, KeyCode::Enter);
        assert!(
            app.runtime.pending_capture.is_none(),
            "the closing row runs nothing"
        );
    });
}

// ── the guard ──

/// Every make menu row's key is `:make <its target>`, and typing that runs
/// the target the row does: the menu teaches a command that works. Walks a
/// Makefile of assorted names, each row fired and each command typed.
#[test]
fn every_make_rows_key_runs_its_target() {
    with_mouse(|tmp| {
        let dir = tmp.join("work");
        let text = "\
.PHONY: all lint
all: ## Everything
\t@true
lint:
\t@true
release-x86_64: ## A release
\t@true
out.txt:
\ttouch $@
a;b: ## A semicolon
\t@true
";
        let mut app = app_over(&dir, &[("Makefile", text)]);
        let listed = crate::taskfile::discover(&dir, &dir).expect("the fixture reads");
        assert!(listed.targets.len() >= 5, "premise: {listed:?}");

        let mut checked = 0;
        for (i, target) in listed.targets.iter().enumerate() {
            let fx = space_m(&mut app);
            land_make_menu(&mut app, fx);
            let key = format!(":make {}", target.name);
            let row_keys = app
                .view
                .chord_hint
                .as_ref()
                .map(|h| h.rows.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>())
                .unwrap_or_default();
            assert_eq!(row_keys.get(i), Some(&key), "row {i}: {row_keys:?}");

            app.fire_chord_row(i);
            let fired = pager_title(&app).unwrap_or_default();
            finish_capture(&mut app);
            app.dispatch_command(&key[1..]);
            let typed = pager_title(&app).unwrap_or_default();
            finish_capture(&mut app);

            let word = crate::shell::shell_quote(&target.name);
            assert!(fired.contains(&format!("! make {word}")), "{key}: {fired}");
            assert!(typed.contains(&format!("! make {word}")), "{key}: {typed}");
            checked += 1;
        }
        assert_eq!(checked, listed.targets.len());
        assert!(checked >= 5, "only {checked} rows checked");
    });
}
