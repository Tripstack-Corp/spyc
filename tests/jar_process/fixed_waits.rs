//! A guard over the jar tests: no fixed wait concludes a fact (the 3.0
//! review, B3 and F5). A clock may bound a wait for a state; it may not
//! decide that something did or didn't happen, nor time a whole operation
//! against a number of seconds: a loaded machine slows everything, and a
//! fast one hides a jar that waits too long. Three shapes are caught:
//!
//! - **sleep, then assert**: a `sleep` (`thread::sleep(`, `park_timeout(` or
//!   a bare `sleep(Duration`) followed by an assertion (`assert!`,
//!   `assert_eq!`, `assert_ne!`, their `debug_` forms, or `panic!`) anywhere
//!   later in the same block, or after a bare `{ }` block it sits in, before
//!   a loop's, an `if`'s or a closure's block closes or another sleep
//!   starts. An assertion that is itself a wait's bound
//!   (`Instant::now() < deadline`) doesn't count;
//! - **a clock verdict**: an assertion, or an `if` whose block panics, that
//!   compares an `elapsed()` time or a `Duration`, directly or through a
//!   variable the same function bound to one (`let took = t.elapsed();`,
//!   `Instant::now() - t`, `duration_since(`);
//! - **a fixed window**: a loop `while Instant::now() < w` where `w` was set
//!   to `Instant::now()` plus a literal duration, and nothing in the loop
//!   leaves it early: a stretch of time whose end decides what was seen.
//!
//! What it can't see, being a line scan: a wait or a clock inside a helper
//! the test calls; a comparison spread over lines it doesn't join (a
//! `let` broken across lines, a condition in a helper); a fixed window
//! that leaves early yet concludes from what it didn't see; `recv(d)`
//! or `recv_timeout(d)` read as "nothing came"; a duration held in a struct
//! field; a sleep followed by an assertion after a loop or an `if` block
//! that holds the sleep (a polling loop looks the same); a clock passed
//! through a second variable (`let a = t.elapsed(); let b = a;`); a wait
//! made some other way (`Condvar::wait_timeout`, `recv_timeout` as a
//! sleep, a busy loop on `Instant`). Those need reading.
//!
//! A site that can't be put on a state stays only by name, below, with why,
//! one entry per site; an entry that no longer matches fails too.

/// One file the guard reads, and the part of it that is test code.
struct Source {
    path: &'static str,
    text: String,
}

/// The test half of a file whose tests are a `#[cfg(test)] mod tests`.
fn tests_of(path: &'static str, src: &str) -> Source {
    let at = src
        .find("\n#[cfg(test)]\nmod tests {")
        .unwrap_or_else(|| panic!("{path}: no `#[cfg(test)] mod tests` to read"));
    Source {
        path,
        text: src[at..].to_string(),
    }
}

fn whole(path: &'static str, src: &str) -> Source {
    Source {
        path,
        text: src.to_string(),
    }
}

/// Every jar test file, read at build time: one that moves breaks the build
/// rather than leave the guard reading nothing.
fn sources() -> Vec<Source> {
    vec![
        whole("tests/jar_process.rs", include_str!("../jar_process.rs")),
        whole("tests/jar_process/answers.rs", include_str!("answers.rs")),
        whole("tests/jar_process/contain.rs", include_str!("contain.rs")),
        whole("tests/jar_process/lock.rs", include_str!("lock.rs")),
        whole(
            "tests/jar_process/ownership.rs",
            include_str!("ownership.rs"),
        ),
        whole("tests/jar_process/sigpipe.rs", include_str!("sigpipe.rs")),
        whole("tests/jar_process/timers.rs", include_str!("timers.rs")),
        whole("tests/jar_process/threads.rs", include_str!("threads.rs")),
        whole("tests/jar_process/upgrade.rs", include_str!("upgrade.rs")),
        whole("tests/jar_adopt.rs", include_str!("../jar_adopt.rs")),
        whole("tests/jar_attach.rs", include_str!("../jar_attach.rs")),
        whole("tests/jar_differ.rs", include_str!("../jar_differ.rs")),
        whole("tests/jar_compat.rs", include_str!("../jar_compat.rs")),
        whole("tests/session_jar.rs", include_str!("../session_jar.rs")),
        whole(
            "crates/spyc-jar/tests/wire_v1.rs",
            include_str!("../../crates/spyc-jar/tests/wire_v1.rs"),
        ),
        whole(
            "src/pane/jar/tests.rs",
            include_str!("../../src/pane/jar/tests.rs"),
        ),
        whole(
            "src/pane/jar/test_dir.rs",
            include_str!("../../src/pane/jar/test_dir.rs"),
        ),
        whole(
            "src/app/harness_tests/jar_start.rs",
            include_str!("../../src/app/harness_tests/jar_start.rs"),
        ),
        whole(
            "src/app/harness_tests/jar_captures.rs",
            include_str!("../../src/app/harness_tests/jar_captures.rs"),
        ),
        whole(
            "src/app/harness_tests/jar_orphans.rs",
            include_str!("../../src/app/harness_tests/jar_orphans.rs"),
        ),
        whole(
            "src/app/harness_tests/never_twice.rs",
            include_str!("../../src/app/harness_tests/never_twice.rs"),
        ),
        whole(
            "tests/jar_interrupted.rs",
            include_str!("../jar_interrupted.rs"),
        ),
        whole("tests/continued.rs", include_str!("../continued.rs")),
        tests_of(
            "crates/spyc-jar/src/client.rs",
            include_str!("../../crates/spyc-jar/src/client.rs"),
        ),
        tests_of(
            "crates/spyc-jar/src/cli.rs",
            include_str!("../../crates/spyc-jar/src/cli.rs"),
        ),
        tests_of(
            "crates/spyc-jar/src/handover.rs",
            include_str!("../../crates/spyc-jar/src/handover.rs"),
        ),
        tests_of(
            "crates/spyc-jar/src/proto.rs",
            include_str!("../../crates/spyc-jar/src/proto.rs"),
        ),
        tests_of(
            "crates/spyc-jar/src/store.rs",
            include_str!("../../crates/spyc-jar/src/store.rs"),
        ),
        tests_of(
            "crates/spyc-jar/src/validation.rs",
            include_str!("../../crates/spyc-jar/src/validation.rs"),
        ),
        tests_of(
            "src/pane/jar/attach.rs",
            include_str!("../../src/pane/jar/attach.rs"),
        ),
        tests_of(
            "src/pane/jar/repaint.rs",
            include_str!("../../src/pane/jar/repaint.rs"),
        ),
        tests_of(
            "src/pane/jar/session.rs",
            include_str!("../../src/pane/jar/session.rs"),
        ),
        tests_of(
            "src/pane/jar/shutdown.rs",
            include_str!("../../src/pane/jar/shutdown.rs"),
        ),
        whole(
            "src/pane/jar/start_tests.rs",
            include_str!("../../src/pane/jar/start_tests.rs"),
        ),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    SleepThenAssert,
    ClockVerdict,
    FixedWindow,
}

/// A site kept on purpose: its file, its function, its shape, the text of
/// the line that starts it, which of the function's lines with that text it
/// is (from 1), and why no state can replace it.
struct Kept {
    path: &'static str,
    function: &'static str,
    shape: Shape,
    line: &'static str,
    nth: usize,
    why: &'static str,
}

const fn kept(
    path: &'static str,
    function: &'static str,
    shape: Shape,
    line: &'static str,
    nth: usize,
    why: &'static str,
) -> Kept {
    Kept {
        path,
        function,
        shape,
        line,
        nth,
        why,
    }
}

const PAST_LINGER: &str = "past the jar's own linger, to show an owned jar doesn't end: the jar reports no linger it isn't running";
const ABSENCE: &str = "an absence nothing reports: no state says a thing won't happen, and load can only make the check easier, not red";

/// The sites that keep a fixed wait, each by name, and why.
const KEPT: &[Kept] = &[
    kept(
        "tests/jar_process.rs",
        "a_finite_linger_ends_the_program_once_its_owner_has_gone",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_millis(500));",
        1,
        PAST_LINGER,
    ),
    kept(
        "tests/jar_process.rs",
        "without_a_linger_the_program_waits_for_an_owner",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_secs(2));",
        1,
        ABSENCE,
    ),
    kept(
        "tests/jar_process.rs",
        "a_jar_asked_for_a_wire_version_it_doesnt_speak_exits_75",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_millis(200));",
        1,
        "the jar has exited; a program it had started would be nobody's child, and nothing reports it",
    ),
    kept(
        "tests/jar_process.rs",
        "a_failed_exec_leaves_a_jar_that_survives_the_next_refusal",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_millis(300));",
        1,
        "a death after the refusal, from the thread still writing the blob to a validator that has gone: nothing reports that thread's end",
    ),
    kept(
        "tests/jar_process.rs",
        "tokenless_connections_cannot_exhaust_the_jars_descriptors",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_secs(3));",
        1,
        "a CPU sample: the span is what is measured, and the jar's own CPU time doesn't grow with load",
    ),
    kept(
        "tests/jar_process.rs",
        "a_pane_that_does_not_linger_survives_a_handover",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_millis(500));",
        1,
        "past the linger of 0 the scenario sets: a successor that arrives late is the case",
    ),
    kept(
        "tests/jar_process/ownership.rs",
        "a_jar_holding_its_program_for_a_first_owner_waits_idle_past_the_grace",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_secs(13));",
        1,
        "a CPU sample past the jar's 10 s first-owner grace: the span is the case",
    ),
    kept(
        "tests/jar_process/ownership.rs",
        "an_owner_paused_mid_frame_inside_its_deadline_keeps_its_lease",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_secs(7));",
        1,
        "a pause longer than the frame deadline and inside the owner's: the clock is the case",
    ),
    kept(
        "tests/jar_process/ownership.rs",
        "a_jar_sits_idle_in_every_idle_state",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_secs(12));",
        1,
        "a CPU sample: the span is what is measured",
    ),
    kept(
        "tests/jar_adopt.rs",
        "spyc_r_after_a_kill_takes_the_running_program_back",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_secs(5));",
        1,
        "past the 2 s a restore waits before typing /resume: spyc reports no decision not to type",
    ),
    kept(
        "tests/jar_adopt.rs",
        "spyc_attach_from_a_newer_install_offers_the_upgrade",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_secs(2));",
        1,
        "a start time is whole seconds: the install must come a second after it",
    ),
    kept(
        "tests/jar_attach.rs",
        "a_terminal_slow_to_read_keeps_its_session",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_secs(6));",
        1,
        "a terminal that reads nothing for longer than the attach client's ping: the clock is the case",
    ),
    kept(
        "src/pane/jar/tests.rs",
        "a_jar_that_wont_run_fails_the_tab_and_runs_nothing",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_millis(300));",
        1,
        "neither binary can run a program, so only a launch outside a jar could make the mark: an absence",
    ),
    kept(
        "src/pane/jar/tests.rs",
        "a_snapshot_follows_a_second_of_quiet_and_never_a_held_frame",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_millis(1100));",
        1,
        "past the snapshotter's second of quiet: its clock is what is tested",
    ),
    kept(
        "src/pane/jar/tests.rs",
        "a_snapshot_follows_a_second_of_quiet_and_never_a_held_frame",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_millis(1100));",
        2,
        "past the snapshotter's second of quiet: its clock is what is tested",
    ),
    kept(
        "src/pane/jar/tests.rs",
        "replies_wait_until_the_replayed_tail_is_parsed",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_millis(600));",
        1,
        "either side of the reply gate's own 500 ms clock (review C7): the clock is what is tested",
    ),
    kept(
        "src/pane/jar/tests.rs",
        "replies_wait_until_the_replayed_tail_is_parsed",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_millis(600));",
        2,
        "either side of the reply gate's own 500 ms clock (review C7): the clock is what is tested",
    ),
    kept(
        "src/pane/jar/tests.rs",
        "a_restored_tab_whose_jar_wont_take_it_is_detached_and_starts_nothing",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_millis(300));",
        1,
        "no second start: a detached tab has no program, and a start elsewhere would use the same socket path",
    ),
    kept(
        "src/pane/jar/tests.rs",
        "a_tab_whose_jar_is_slow_to_start_opens_with_its_program_run_once",
        Shape::SleepThenAssert,
        "std::thread::sleep(Duration::from_secs(2));",
        1,
        ABSENCE,
    ),
];

fn code(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && !t.starts_with("//")
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The line that starts the function line `at` is in, and its name.
fn function_at(lines: &[&str], at: usize) -> (usize, String) {
    for i in (0..=at).rev() {
        let t = lines[i].trim_start();
        let t = ["pub(crate) ", "pub(super) ", "pub "]
            .iter()
            .fold(t, |t, p| t.strip_prefix(p).unwrap_or(t));
        if let Some(name) = t.strip_prefix("fn ")
            && let Some(name) = name.split(['(', '<']).next()
        {
            return (i, name.to_string());
        }
    }
    (0, String::new())
}

fn is_sleep(t: &str) -> bool {
    t.contains("thread::sleep(")
        || t.contains("park_timeout(")
        || t.starts_with("sleep(Duration")
        || t.contains(" sleep(Duration")
}

fn is_assertion(t: &str) -> bool {
    [
        "assert!(",
        "assert_eq!(",
        "assert_ne!(",
        "debug_assert!(",
        "debug_assert_eq!(",
        "debug_assert_ne!(",
        "panic!(",
    ]
    .iter()
    .any(|m| t.starts_with(m))
}

/// The statement starting at line `at`, joined to one line.
fn statement(lines: &[&str], at: usize) -> String {
    let mut s = String::new();
    for l in &lines[at..lines.len().min(at + 15)] {
        s.push_str(l.trim());
        s.push(' ');
        let e = l.trim_end();
        if e.ends_with(';') || e.ends_with('{') || e.ends_with('}') {
            break;
        }
    }
    s
}

/// A wait's own bound, which is how a bounded wait fails, not a verdict.
fn is_wait_bound(s: &str) -> bool {
    ["Instant::now() <", "Instant::now() >"]
        .iter()
        .any(|p| s.contains(p))
}

const COMPARE: [&str; 4] = [" < ", " <= ", " > ", " >= "];

/// Whether `s` compares `what` with something: `what < x`, `x > what`.
fn compares(s: &str, what: &str) -> bool {
    let mut from = 0;
    while let Some(at) = s[from..].find(what) {
        let start = from + at;
        let end = start + what.len();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !word(s[..start].chars().next_back()) && !word(s[end..].chars().next()) {
            let after = &s[end..];
            let before = &s[..start];
            if COMPARE
                .iter()
                .any(|c| after.starts_with(c) || before.ends_with(c))
            {
                return true;
            }
        }
        from = end;
    }
    false
}

/// Every site of the three shapes in `src`: (function, shape, line text,
/// line number), and how many sleeps it read.
fn sites(src: &Source) -> (Vec<(String, Shape, String, usize)>, usize) {
    let lines: Vec<&str> = src.text.lines().collect();
    let mut found = Vec::new();
    let mut sleeps = 0;
    for (i, line) in lines.iter().enumerate() {
        if !code(line) {
            continue;
        }
        let t = line.trim();
        let (start, function) = function_at(&lines, i);
        if is_sleep(t) {
            sleeps += 1;
            let mut depth = indent(line);
            for (j, next) in lines.iter().enumerate().skip(i + 1) {
                if !code(next) {
                    continue;
                }
                if indent(next) < depth {
                    // Out of a bare `{ }` block, the scan goes on in the
                    // block around it; out of a loop, an `if` or a closure,
                    // it stops.
                    let closing = indent(next);
                    let opener = lines[start..i]
                        .iter()
                        .rev()
                        .find(|l| code(l) && indent(l) == closing && l.trim_end().ends_with('{'));
                    if next.trim() == "}" && opener.is_some_and(|o| o.trim() == "{") {
                        depth = closing;
                        continue;
                    }
                    break;
                }
                if is_sleep(next.trim()) {
                    break;
                }
                if is_assertion(next.trim()) {
                    if !is_wait_bound(&statement(&lines, j)) {
                        found.push((
                            function.clone(),
                            Shape::SleepThenAssert,
                            t.to_string(),
                            i + 1,
                        ));
                    }
                    break;
                }
            }
        }
        let verdict = is_assertion(t)
            || (t.starts_with("if ")
                && (t.contains("panic!(")
                    || (t.ends_with('{')
                        && lines[i + 1..lines.len().min(i + 4)]
                            .iter()
                            .any(|l| is_assertion(l.trim())))));
        if verdict {
            let s = statement(&lines, i);
            // Variables this function bound to a clock reading or a duration.
            let clocks: Vec<String> = lines[start..i]
                .iter()
                .filter_map(|l| {
                    let l = l.trim();
                    let rest = l.strip_prefix("let ")?;
                    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
                    let (name, value) = rest.split_once(" = ")?;
                    let clock = (value.contains(".elapsed()")
                        || value.contains("Duration::")
                        || value.contains("Instant::now() -")
                        || value.contains("duration_since("))
                        && !value.contains("Instant::now() +");
                    clock.then(|| {
                        name.trim_end_matches(|c: char| c == ':' || c.is_whitespace())
                            .split(':')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string()
                    })
                })
                .collect();
            if compares(&s, ".elapsed()")
                || s.contains("< Duration::")
                || s.contains("<= Duration::")
                || s.contains("> Duration::")
                || s.contains(">= Duration::")
                || clocks.iter().any(|v| !v.is_empty() && compares(&s, v))
            {
                found.push((function.clone(), Shape::ClockVerdict, t.to_string(), i + 1));
            }
        }
        if let Some(rest) = t.strip_prefix("while Instant::now() < ")
            && let Some(w) = rest.strip_suffix(" {")
        {
            let set = lines[start..i].iter().any(|l| {
                let l = l.trim();
                l.contains(&format!("{w} = Instant::now() + Duration::from_"))
            });
            let depth = indent(line);
            let leaves = lines[i + 1..]
                .iter()
                .take_while(|l| !code(l) || indent(l) > depth)
                .any(|l| {
                    let l = l.trim();
                    l.starts_with("break")
                        || l.starts_with("return")
                        || l.contains("=> break")
                        || l.contains("=> return")
                });
            if set && !leaves {
                found.push((function, Shape::FixedWindow, t.to_string(), i + 1));
            }
        }
    }
    (found, sleeps)
}

#[test]
fn no_fixed_wait_concludes_a_jar_test() {
    let sources = sources();
    let mut offenders = Vec::new();
    let mut kept_seen = vec![false; KEPT.len()];
    let mut sleeps = 0;
    for src in &sources {
        assert!(
            src.text.contains("#[test]"),
            "{}: no test read; the guard would check nothing",
            src.path
        );
        let (found, n) = sites(src);
        sleeps += n;
        for (i, (function, shape, text, line)) in found.iter().enumerate() {
            let (function, shape, text) = (function.as_str(), *shape, text.as_str());
            let count = found[..=i]
                .iter()
                .filter(|(f, s, t, _)| f == function && *s == shape && t == text)
                .count();
            match KEPT.iter().position(|k| {
                k.path == src.path
                    && k.function == function
                    && k.shape == shape
                    && k.line == text
                    && k.nth == count
            }) {
                Some(k) => kept_seen[k] = true,
                None => offenders.push(format!(
                    "{}:{line} in {function}: {shape:?}: {text}",
                    src.path
                )),
            }
        }
    }
    // Fails closed: the files hold dozens of polling sleeps, so a guard that
    // counts few is reading the wrong thing.
    assert!(
        sleeps >= 60,
        "only {sleeps} sleeps read: the guard reads the wrong text"
    );
    assert!(
        offenders.is_empty(),
        "a fixed wait decides these tests; wait for the state instead, or bound the wait with \
         `Instant::now() < deadline`:\n{}",
        offenders.join("\n")
    );
    let stale: Vec<_> = KEPT
        .iter()
        .zip(&kept_seen)
        .filter(|(_, seen)| !**seen)
        .map(|(k, _)| {
            format!(
                "{} {} {:?} {:?} #{}: {}",
                k.path, k.function, k.shape, k.line, k.nth, k.why
            )
        })
        .collect();
    assert!(
        stale.is_empty(),
        "kept sites that no longer match: {stale:?}"
    );
}
