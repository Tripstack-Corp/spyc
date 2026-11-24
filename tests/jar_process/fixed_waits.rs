//! A guard over the jar tests: no fixed wait concludes a fact (the 3.0
//! review, B3 and F5). A clock may bound a wait for a state; it may not
//! decide that something did or didn't happen, nor time a whole operation
//! against a number of seconds, since a loaded machine slows everything and
//! a fast one hides a jar that waits too long. Two shapes are caught:
//!
//! - a `thread::sleep` whose next statement is an assertion: the sleep
//!   stands in for a state the test didn't wait for;
//! - an assertion that compares an `elapsed()` time or a `Duration`:
//!   the clock is the verdict. A bounded wait is written as
//!   `Instant::now() < deadline` instead, its deadline moved on by
//!   progress where the wait is for progress.
//!
//! A site that can't be put on a state stays only by name, below, with why;
//! an entry that no longer matches fails too, so the list can't go stale.

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
        whole("tests/jar_process/contain.rs", include_str!("contain.rs")),
        whole("tests/jar_process/lock.rs", include_str!("lock.rs")),
        whole(
            "tests/jar_process/ownership.rs",
            include_str!("ownership.rs"),
        ),
        whole("tests/jar_process/timers.rs", include_str!("timers.rs")),
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
        tests_of(
            "crates/spyc-jar/src/client.rs",
            include_str!("../../crates/spyc-jar/src/client.rs"),
        ),
    ]
}

/// The sites that keep a fixed wait, by file, test and shape, and why no
/// state can replace it.
const KEPT: &[(&str, &str, Shape, &str)] = &[
    (
        "tests/jar_process.rs",
        "a_finite_linger_ends_the_program_once_its_owner_has_gone",
        Shape::SleepThenAssert,
        "past the 300 ms linger it sets, to show an owned jar doesn't end: the jar reports no state for a linger it isn't running",
    ),
    (
        "tests/jar_process.rs",
        "without_a_linger_the_program_waits_for_an_owner",
        Shape::SleepThenAssert,
        "an ownerless jar with no linger running on: an absence, which load can't turn red",
    ),
    (
        "tests/jar_process.rs",
        "a_jar_asked_for_a_wire_version_it_doesnt_speak_exits_75",
        Shape::SleepThenAssert,
        "the jar has exited; a program it had started would be nobody's child, and nothing reports it",
    ),
    (
        "tests/jar_process.rs",
        "a_failed_exec_leaves_a_jar_that_survives_the_next_refusal",
        Shape::SleepThenAssert,
        "a death after the refusal, from the thread still writing the blob to a validator that has gone: nothing reports that thread's end",
    ),
    (
        "tests/jar_adopt.rs",
        "spyc_r_after_a_kill_takes_the_running_program_back",
        Shape::SleepThenAssert,
        "past the 2 s a restore waits before typing /resume: spyc reports no decision not to type",
    ),
    (
        "tests/jar_attach.rs",
        "a_terminal_slow_to_read_keeps_its_session",
        Shape::SleepThenAssert,
        "a terminal that reads nothing for longer than the attach client's ping: the clock is the scenario",
    ),
    (
        "src/pane/jar/tests.rs",
        "a_jar_that_wont_run_fails_the_tab_and_runs_nothing",
        Shape::SleepThenAssert,
        "neither binary can run a program, so only a launch outside a jar could make the mark: an absence",
    ),
    (
        "src/pane/jar/tests.rs",
        "replies_wait_until_the_replayed_tail_is_parsed",
        Shape::SleepThenAssert,
        "either side of the reply gate's own 500 ms clock (review C7): the clock is what is tested",
    ),
    (
        "src/pane/jar/tests.rs",
        "a_restored_tab_whose_jar_wont_take_it_is_detached_and_starts_nothing",
        Shape::SleepThenAssert,
        "no second start: a detached tab has no program, and a start elsewhere would use the same socket path",
    ),
    (
        "src/pane/jar/tests.rs",
        "a_tab_whose_jar_is_slow_to_start_opens_with_its_program_run_once",
        Shape::SleepThenAssert,
        "no second start's line in the program's file: an absence",
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    SleepThenAssert,
    ClockVerdict,
}

/// The name of the function line `at` is in: the nearest `fn` above it.
fn function_at(lines: &[&str], at: usize) -> String {
    lines[..=at]
        .iter()
        .rev()
        .find_map(|l| {
            let t = l.trim_start();
            let t = t.strip_prefix("pub ").unwrap_or(t);
            let t = t.strip_prefix("pub(super) ").unwrap_or(t);
            let t = t.strip_prefix("pub(crate) ").unwrap_or(t);
            let name = t.strip_prefix("fn ")?;
            Some(name.split(['(', '<']).next()?.to_string())
        })
        .unwrap_or_default()
}

fn code(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && !t.starts_with("//")
}

/// Every site of either shape in `src`: (function, shape, line number).
fn sites(src: &Source) -> (Vec<(String, Shape, usize)>, usize) {
    let lines: Vec<&str> = src.text.lines().collect();
    let mut found = Vec::new();
    let mut sleeps = 0;
    for (i, line) in lines.iter().enumerate() {
        if !code(line) {
            continue;
        }
        let t = line.trim();
        if t.contains("thread::sleep(") {
            sleeps += 1;
            if let Some(next) = lines[i + 1..].iter().find(|l| code(l))
                && next.trim().starts_with("assert")
            {
                found.push((function_at(&lines, i), Shape::SleepThenAssert, i + 1));
            }
        }
        if t.starts_with("assert!(") || t.starts_with("assert_eq!(") || t.starts_with("assert_ne!(")
        {
            let mut call = String::new();
            for l in &lines[i..lines.len().min(i + 15)] {
                call.push_str(l.trim());
                call.push(' ');
                if l.trim_end().ends_with(");") {
                    break;
                }
            }
            if call.contains(".elapsed()")
                || call.contains("< Duration::")
                || call.contains("<= Duration::")
                || call.contains("> Duration::")
            {
                found.push((function_at(&lines, i), Shape::ClockVerdict, i + 1));
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
        for (function, shape, line) in found {
            match KEPT
                .iter()
                .position(|(p, f, s, _)| *p == src.path && *f == function && *s == shape)
            {
                Some(k) => kept_seen[k] = true,
                None => offenders.push(format!("{}:{line} in {function}: {shape:?}", src.path)),
            }
        }
    }
    // Fails closed: the files hold dozens of polling sleeps, so a guard that
    // counts none is reading the wrong thing.
    assert!(
        sleeps >= 40,
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
        .map(|((p, f, s, _), _)| format!("{p} {f} {s:?}"))
        .collect();
    assert!(
        stale.is_empty(),
        "kept sites that no longer match: {stale:?}"
    );
}
