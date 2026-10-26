//! A real `agy` driven through spyc's own pane: the screens in
//! `tests/fixtures/agy-output/`, and the output gaps behind agy's `working`
//! quiet window (`docs/HARNESS.md`). Each test needs the local `agy`, signed in,
//! so each is `#[ignore]`d and run by hand:
//!
//! ```sh
//! SPYC_AGY_CAPTURE_DIR=/tmp/agy-capture \
//!   cargo test --lib agy_live -- --ignored --nocapture --test-threads=1
//! ```
//!
//! agy runs with `AGY_CLI_HIDE_ACCOUNT_INFO`, and screens are written
//! sanitized besides: the workspace becomes `/Users/example/src/spyc`, the
//! temporary directory `/tmp`, `$HOME` `/Users/example`, an email address
//! `user@example.com`, a GCP project `example-project` and `$USER` `example`.
//! Read them before committing.
//!
//! Both work with agy's vim editor mode, which opens the composer in NORMAL
//! mode and makes Enter in INSERT mode a newline.
//!
//! No dialog is answered with an `always` option, which writes a rule into
//! agy's `settings.json`.

use super::*;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The draft typed into the composer, idle and mid-turn.
const DRAFT: &str = "check src/unsubmitted.rs";

/// Where a run writes its screens and timeline: `$SPYC_AGY_CAPTURE_DIR`.
fn capture_dir() -> PathBuf {
    let dir = std::env::var_os("SPYC_AGY_CAPTURE_DIR")
        .map(PathBuf::from)
        .expect("set SPYC_AGY_CAPTURE_DIR to the directory screens are written to");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A real `agy` in a spyc pane, and when its output arrived.
struct LiveAgy {
    pane: crate::pane::Pane,
    root: PathBuf,
    dir: PathBuf,
    started: Instant,
    output_at: Vec<Duration>,
    timeline: String,
    last_tail: String,
    vim: bool,
    _app: App,
}

impl LiveAgy {
    fn spawn(rows: u16, cols: u16, hooks: Option<&str>) -> Self {
        // Before agy starts, so a run without the variable never launches it.
        let dir = capture_dir();
        // One fixed workspace, so agy's trusted-workspace list gains one entry
        // however many times this runs.
        let root = std::env::temp_dir().join("spyc-agy-live-workspace");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::write(root.join("src/printed.rs"), "pub fn printed() {}\n").unwrap();
        if let Some(hooks) = hooks {
            std::fs::create_dir_all(root.join(".agents")).unwrap();
            std::fs::write(root.join(".agents/hooks.json"), hooks).unwrap();
        }
        let mut app = App::test_app(root.clone());
        let context_path = app.view.project().context_path.clone();
        let wake = app.make_pane_wake();
        let pane = crate::pane::Pane::spawn_with_env(
            "agy",
            rows,
            cols,
            &root,
            &context_path,
            &[
                ("TERM", "xterm-256color"),
                ("AGY_CLI_HIDE_ACCOUNT_INFO", "1"),
            ],
            wake,
        )
        .unwrap();
        Self {
            pane,
            root,
            dir,
            started: Instant::now(),
            output_at: Vec::new(),
            timeline: String::new(),
            last_tail: String::new(),
            vim: false,
            _app: app,
        }
    }

    /// Drains the pane, noting when output arrived and logging each change to
    /// the screen's bottom rows.
    fn pump(&mut self) {
        if !self.pane.drain_output() {
            return;
        }
        let at = self.started.elapsed();
        self.output_at.push(at);
        let screen = self.pane.visible_lines();
        let mut tail: Vec<&str> = screen
            .iter()
            .rev()
            .filter(|r| !r.trim().is_empty())
            .take(4)
            .map(String::as_str)
            .collect();
        tail.reverse();
        let tail = tail.join(" | ");
        if tail != self.last_tail {
            let _ = writeln!(
                self.timeline,
                "{:8.2}s  {}",
                at.as_secs_f64(),
                self.clean(&tail)
            );
            self.last_tail = tail;
        }
    }

    fn screen(&self) -> String {
        self.pane.visible_lines().join("\n")
    }

    /// Pumps until `ready` holds for the screen; `None` after `timeout`.
    fn wait_for(&mut self, timeout: Duration, ready: impl Fn(&str) -> bool) -> Option<String> {
        let deadline = Instant::now() + timeout;
        loop {
            self.pump();
            let screen = self.screen();
            if ready(&screen) {
                return Some(screen);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// [`Self::wait_for`], logging `what` and the screen when it times out.
    fn expect(&mut self, what: &str, timeout: Duration, ready: impl Fn(&str) -> bool) -> bool {
        if self.wait_for(timeout, ready).is_some() {
            return true;
        }
        let screen = self.clean(&self.screen());
        let _ = writeln!(self.timeline, "TIMED OUT waiting for {what}:\n{screen}\n");
        println!("timed out waiting for {what}:\n{screen}");
        false
    }

    /// Pumps for `span`, whatever the screen does.
    fn watch(&mut self, span: Duration) {
        let until = Instant::now() + span;
        while Instant::now() < until {
            self.pump();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.pane.send_bytes(bytes).unwrap();
    }

    /// Puts the composer in a state where typed text is text: in vim mode an
    /// `i` enters INSERT mode, and anywhere else it is typed, so it is erased.
    fn start_typing(&mut self) {
        self.send(b"i");
        self.watch(Duration::from_millis(400));
        let screen = self.screen();
        self.vim = screen.contains("-- INSERT --");
        let typed_i = screen
            .lines()
            .any(|r| r.starts_with('>') && r.trim_end().ends_with('i'));
        if typed_i {
            self.send(&[0x7f]);
            self.watch(Duration::from_millis(300));
        }
    }

    /// Types `prompt` and submits it, leaving vim's INSERT mode first.
    fn submit(&mut self, prompt: &str) {
        self.start_typing();
        self.send(prompt.as_bytes());
        let tail: String = prompt
            .chars()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        self.expect("the typed prompt", Duration::from_secs(5), |s| {
            s.contains(&tail)
        });
        if self.vim {
            self.send(b"\x1b");
            self.watch(Duration::from_millis(300));
        }
        self.send(b"\r");
    }

    /// Waits for the submitted turn to start and then to end.
    fn finish_turn(&mut self, what: &str, timeout: Duration) {
        self.wait_for(Duration::from_secs(15), is_working);
        self.expect(what, timeout, |s| !is_working(s));
    }

    fn type_draft(&mut self) -> bool {
        self.start_typing();
        self.send(DRAFT.as_bytes());
        self.expect("the typed draft", Duration::from_secs(5), |s| {
            s.contains(DRAFT)
        })
    }

    /// Erases the draft, and leaves vim's INSERT mode as the composer was found.
    fn erase_draft(&mut self) {
        self.send(&vec![0x7f; DRAFT.len()]);
        self.watch(Duration::from_millis(500));
        if self.vim {
            self.send(b"\x1b");
            self.watch(Duration::from_millis(500));
        }
    }

    /// Answers the dialog on screen without granting or remembering anything:
    /// moves the cursor to its one-time `No` option and selects it, or, with
    /// none, as on a question, skips it with Esc. The `No, and always deny`
    /// options below it write a deny rule into agy's `settings.json`.
    fn decline(&mut self) {
        let Some((at, no)) = one_time_no(&self.screen()) else {
            self.send(b"\x1b");
            return;
        };
        let key: &[u8] = if no > at { b"\x1b[B" } else { b"\x1b[A" };
        for _ in 0..at.abs_diff(no) {
            self.send(key);
            self.watch(Duration::from_millis(200));
        }
        // Never confirm a choice that would grant or persist something.
        if one_time_no(&self.screen()).is_some_and(|(at, no)| at == no) {
            self.send(b"\r");
        } else {
            self.send(b"\x1b");
        }
    }

    /// Sanitizes `text` for a fixture.
    fn clean(&self, text: &str) -> String {
        let email = regex::Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
            .expect("a valid email pattern");
        let mut out = text.replace(&self.root.display().to_string(), "/Users/example/src/spyc");
        // A narrow pane cuts the workspace path short of the whole root.
        let tmp = std::env::temp_dir();
        for dir in [tmp.canonicalize().unwrap_or_else(|_| tmp.clone()), tmp] {
            let dir = dir.display().to_string();
            out = out.replace(dir.trim_end_matches('/'), "/tmp");
        }
        if let Ok(home) = std::env::var("HOME")
            && !home.is_empty()
        {
            out = out.replace(&home, "/Users/example");
        }
        out = email.replace_all(&out, "user@example.com").into_owned();
        let project = regex::Regex::new(r"(GCP Project: )\S+").expect("a valid project pattern");
        out = project
            .replace_all(&out, "${1}example-project")
            .into_owned();
        if let Ok(user) = std::env::var("USER")
            && !user.is_empty()
        {
            out = out.replace(&user, "example");
        }
        out
    }

    /// Writes the screen, sanitized, as `<name>.txt`.
    fn snapshot(&mut self, name: &str) {
        let text = self.clean(&self.screen());
        std::fs::write(self.dir.join(format!("{name}.txt")), text + "\n").unwrap();
        let _ = writeln!(
            self.timeline,
            "{:8.2}s  SNAPSHOT {name}",
            self.started.elapsed().as_secs_f64()
        );
    }

    /// The longest stretch without output in `from..to`, and where it began.
    fn longest_gap(&self, from: Duration, to: Duration) -> (Duration, Duration) {
        let mut prev = from;
        let mut best = (Duration::ZERO, from);
        for &at in self.output_at.iter().filter(|&&at| at > from && at <= to) {
            if at.saturating_sub(prev) > best.0 {
                best = (at.saturating_sub(prev), prev);
            }
            prev = at;
        }
        if to.saturating_sub(prev) > best.0 {
            best = (to.saturating_sub(prev), prev);
        }
        best
    }

    fn save_timeline(&self, name: &str) {
        std::fs::write(self.dir.join(format!("{name}.log")), &self.timeline).unwrap();
    }
}

/// In the dialog below the screen's last rule: the selected option's index
/// and the first `No` option's that neither remembers nor persists the choice.
fn one_time_no(screen: &str) -> Option<(usize, usize)> {
    let rows: Vec<&str> = screen.lines().collect();
    let rule = rows.iter().rposition(|r| r.starts_with('─'))?;
    let options: Vec<(bool, &str)> = rows[rule + 1..]
        .iter()
        .filter_map(|row| {
            let t = row.trim_start();
            let (selected, t) = t
                .strip_prefix('>')
                .map_or((false, t), |rest| (true, rest.trim_start()));
            let (num, text) = t.split_once(". ")?;
            num.parse::<usize>().ok()?;
            Some((selected, text))
        })
        .collect();
    let at = options.iter().position(|&(selected, _)| selected)?;
    let no = options.iter().position(|&(_, text)| {
        text.starts_with("No") && !text.contains("always") && !text.contains("Persist")
    })?;
    Some((at, no))
}

fn is_trust_prompt(screen: &str) -> bool {
    screen.contains("trust")
}

/// agy's footer shows its cancel hint while a turn runs, after vim's mode
/// marker when there is one, and never once the turn has ended.
fn is_working(screen: &str) -> bool {
    last_row(screen).is_some_and(|r| r.contains("esc to cancel"))
}

fn is_approval(screen: &str) -> bool {
    screen.contains("Navigate") && screen.contains("Requesting permission for:")
}

/// agy's `ask_question` dialog, under its `Question` header.
fn is_question(screen: &str) -> bool {
    screen.contains("Navigate") && screen.lines().any(|r| r.trim() == "Question")
}

/// Any agy dialog: its `↑/↓ Navigate` row sits just above the footer.
fn is_dialog(screen: &str) -> bool {
    screen
        .lines()
        .rev()
        .filter(|r| !r.trim().is_empty())
        .take(3)
        .any(|r| r.trim_start().starts_with("↑/↓ Navigate"))
}

/// The mode word in agy's footer, before ` · <model>`.
fn footer_mode(screen: &str) -> String {
    last_row(screen)
        .and_then(|r| r.split(" · ").next())
        .and_then(|r| r.split_whitespace().last())
        .unwrap_or_default()
        .to_owned()
}

fn last_row(screen: &str) -> Option<&str> {
    screen.lines().rev().find(|r| !r.trim().is_empty())
}

/// Starts agy and answers the workspace trust prompt if it asks.
fn start(rows: u16, cols: u16) -> LiveAgy {
    start_with_hooks(rows, cols, None)
}

/// [`start`], with `hooks` as the workspace's `.agents/hooks.json`.
fn start_with_hooks(rows: u16, cols: u16, hooks: Option<&str>) -> LiveAgy {
    let mut agy = LiveAgy::spawn(rows, cols, hooks);
    agy.expect("agy's first screen", Duration::from_secs(60), |s| {
        is_trust_prompt(s) || s.contains("accept-edits") || s.contains("for shortcuts")
    });
    agy.watch(Duration::from_secs(2));
    if is_trust_prompt(&agy.screen()) {
        agy.snapshot("trust-prompt");
        agy.send(b"\r");
        agy.watch(Duration::from_secs(3));
    }
    agy.snapshot("start");
    agy
}

/// The screens `tests/fixtures/agy-output/` holds, live.
#[test]
#[ignore = "manual: drives the local agy, signed in"]
fn agy_live_capture_viewport_fixtures() {
    let mut agy = start(28, 100);

    agy.submit(
        "Use run_command to run `uname -a && sleep 8` and wait for it, then reply with only: done",
    );
    if agy.expect("the command approval", Duration::from_secs(90), is_approval) {
        agy.watch(Duration::from_millis(500));
        agy.snapshot("approval-command");
        agy.send(b"\r");
    }
    if agy.expect("the running command", Duration::from_secs(30), is_working) {
        agy.watch(Duration::from_secs(1));
        agy.snapshot("working");
        if agy.type_draft() {
            agy.watch(Duration::from_millis(500));
            agy.snapshot("working-draft");
            agy.erase_draft();
        }
    }
    agy.expect("the turn's end", Duration::from_secs(120), |s| {
        !is_working(s)
    });
    agy.watch(Duration::from_secs(2));
    agy.snapshot("composer-empty");
    if agy.type_draft() {
        agy.watch(Duration::from_millis(500));
        agy.snapshot("composer-draft");
        agy.erase_draft();
    }

    agy.pane.resize(28, 70).unwrap();
    agy.watch(Duration::from_secs(1));
    agy.submit("Use run_command to run `uname -m` and wait for it, then reply with only: listed");
    if agy.expect(
        "the narrow command approval",
        Duration::from_secs(90),
        is_approval,
    ) {
        agy.watch(Duration::from_millis(500));
        agy.snapshot("approval-command-narrow");
        agy.send(b"\x1b");
    }
    agy.finish_turn("the narrow turn's end", Duration::from_secs(60));
    agy.submit(
        "Use the ask_question tool to ask me exactly: Which of these colours should the example heading use as its accent colour? Offer red, green and blue.",
    );
    if agy.expect("the narrow question", Duration::from_secs(90), is_question) {
        agy.watch(Duration::from_millis(500));
        agy.snapshot("approval-question-narrow");
        agy.send(b"\x1b");
    }
    agy.finish_turn("the narrow question turn's end", Duration::from_secs(60));
    agy.pane.resize(28, 100).unwrap();
    agy.watch(Duration::from_secs(1));

    agy.submit(
        "Use the ask_question tool to ask me which colour I prefer, offering red, green and blue.",
    );
    if agy.expect("the question", Duration::from_secs(90), is_question) {
        agy.watch(Duration::from_millis(500));
        agy.snapshot("approval-question");
        agy.send(b"\x1b");
    }
    agy.finish_turn("the question turn's end", Duration::from_secs(60));

    // agy asks before an edit only outside accept-edits mode; shift+tab cycles
    // the mode, and the cycle ends back at accept-edits.
    let mut modes = Vec::new();
    for _ in 0..4 {
        agy.send(b"\x1b[Z");
        agy.watch(Duration::from_secs(1));
        let mode = footer_mode(&agy.screen());
        let _ = writeln!(agy.timeline, "MODE {mode}");
        if mode == "accept-edits" {
            break;
        }
        modes.push(mode);
    }
    println!("modes after accept-edits: {modes:?}");
    if let Some(steps) = modes
        .iter()
        .position(|m| !matches!(m.as_str(), "auto" | "plan"))
    {
        for _ in 0..=steps {
            agy.send(b"\x1b[Z");
            agy.watch(Duration::from_secs(1));
        }
        agy.submit("Use a file edit tool to change src/printed.rs so that printed returns 1u8.");
        if agy.expect("the edit approval", Duration::from_secs(90), |s| {
            s.contains("Accept this file edit?") && s.contains("Navigate")
        }) {
            agy.watch(Duration::from_millis(500));
            agy.snapshot("approval-edit");
            // Esc leaves this dialog up: choose `2. No, reject this change`.
            agy.send(b"\x1b[B");
            agy.watch(Duration::from_millis(300));
            agy.send(b"\r");
        }
        agy.finish_turn("the edit turn's end", Duration::from_secs(60));
        while footer_mode(&agy.screen()) != "accept-edits" {
            agy.send(b"\x1b[Z");
            agy.watch(Duration::from_secs(1));
        }
    }

    agy.save_timeline("capture-timeline");
    drop(agy);
    capture_forced_dialogs();
}

/// A workspace `PreToolUse` hook that makes agy ask before every MCP tool call
/// and file read, which its user may allow already: spyc's tools by a rule,
/// reads outside the workspace by `allowNonWorkspaceAccess`.
const FORCE_ASK_HOOKS: &str = r#"{"spyc-harness-ask": {"PreToolUse": [{"matcher": "call_mcp_tool|read_file", "hooks": [{"type": "command", "command": "cat >/dev/null; echo '{\"decision\":\"force_ask\"}'"}]}]}}"#;

/// The MCP tool and file access dialogs, in a second agy whose workspace holds
/// [`FORCE_ASK_HOOKS`]. Each is declined.
fn capture_forced_dialogs() {
    let mut agy = start_with_hooks(28, 100, Some(FORCE_ASK_HOOKS));
    agy.submit(
        "Without reading any files, call the spyc MCP tool list_scopes, then reply with only: called",
    );
    if agy.expect("the MCP tool approval", Duration::from_secs(90), |s| {
        is_dialog(s) && s.contains("Allow calling this tool?")
    }) {
        agy.watch(Duration::from_millis(500));
        agy.snapshot("approval-mcp");
        agy.decline();
    }
    agy.finish_turn("the MCP turn's end", Duration::from_secs(60));
    agy.submit("Read src/printed.rs and reply with only its first word.");
    if agy.expect("the file access approval", Duration::from_secs(90), |s| {
        is_dialog(s) && s.contains("Allow access to this file?")
    }) {
        agy.watch(Duration::from_millis(500));
        agy.snapshot("approval-file-access");
        agy.decline();
    }
    agy.finish_turn("the file access turn's end", Duration::from_secs(60));
    agy.save_timeline("capture-forced-timeline");
}

/// One probe: a prompt, and the longest output gap from the moment agy is
/// answered (or the prompt submitted) to the end of its turn.
fn probe(agy: &mut LiveAgy, name: &str, prompt: &str, results: &mut String) {
    agy.submit(prompt);
    let mut from = agy.started.elapsed();
    let deadline = Instant::now() + Duration::from_secs(240);
    agy.wait_for(Duration::from_secs(20), is_working);
    while Instant::now() < deadline {
        agy.pump();
        let screen = agy.screen();
        if is_approval(&screen) {
            agy.watch(Duration::from_millis(300));
            agy.send(b"\r");
            from = agy.started.elapsed();
            agy.watch(Duration::from_secs(1));
            continue;
        }
        if !is_working(&screen) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let to = agy.started.elapsed();
    let (gap, at) = agy.longest_gap(from, to);
    let _ = writeln!(
        results,
        "{name}: turn {:.1}s, longest output gap {:.2}s starting {:.1}s in",
        to.saturating_sub(from).as_secs_f64(),
        gap.as_secs_f64(),
        at.saturating_sub(from).as_secs_f64()
    );
    agy.watch(Duration::from_secs(2));
}

/// How long agy goes without output while it works: the evidence behind
/// `AgyProfile::working_quiet_window`. Writes `probe-results.txt`.
#[test]
#[ignore = "manual: drives the local agy, signed in"]
fn agy_live_output_gaps_while_working() {
    let mut agy = start(28, 100);
    let mut results = String::new();
    probe(
        &mut agy,
        "sleep 40, agy choosing its own wait",
        "Use run_command to run `sleep 40` in the foreground and wait for it to finish, then reply with only: slept",
        &mut results,
    );
    probe(
        &mut agy,
        "sleep 40, backgrounded after 2 s",
        "Use run_command to run `sleep 40` with WaitMsBeforeAsync set to 2000, then wait until that command has finished and reply with only: slept",
        &mut results,
    );
    probe(
        &mut agy,
        "long think",
        "Before answering, reason carefully and at length: how many primes are there below 10000, and what is the largest? Reply with only the two numbers.",
        &mut results,
    );
    println!("{results}");
    std::fs::write(agy.dir.join("probe-results.txt"), &results).unwrap();
    agy.save_timeline("probe-timeline");
}
