//! A debug build's crash at a named point of a handover (the 3.0 review, G2).
//! `SPYC_HANDOVER_FAULT=<point>` makes spyc end itself with `SIGKILL` the
//! first time it reaches `<point>`, and `<point>:<n>` the `n`th time, as a
//! crash or a `kill -9` would: no destructor, no save, no word to a jar. It
//! says which point on its standard error first, so a test can tell the
//! point from any other death. A release build reads no variable and its
//! points do nothing, as `SPYC_JAR_RESUME_FAULT` is for the jar's resume.
//!
//! A point follows each durable write and each release of a jar in the four
//! handovers, `^d`, `:upgrade`, `:detach project` and `:attach project`,
//! and in `:detach tab` and `:attach tab`, which share their code. The
//! writes are the session's files (its manifest, its jar's token, the jar
//! itself started); a release ends this spyc's connection to a jar. They were
//! found with
//! `rg -n 'save_session_durably\(|autosave_session\(|remove_session\(|tokens::save\(|tokens::forget\(|forget_token\(|sever_jar\(|session::start\(' src/app/detach.rs src/app/project_move.rs src/pane/jar/session.rs`,
//! and the guard below keeps every such call followed by a point:
//!
//! - `^d` in a plain spyc, and `:upgrade` there: `detach.saved` (the session
//!   saved as no running spyc's), `session.token-saved` (the session jar's
//!   token written, the jar not yet started), `detach.jar-started` (the
//!   session's jar runs, its spyc waiting for this one's jars),
//!   `detach.released` (a jar let go, once per tab and task), and
//!   `detach.kept` (the session saved again as this spyc's when its jar
//!   didn't start).
//! - `:upgrade` in spyc's own jar: `upgrade.saved`, `upgrade.released`.
//! - `:detach project`: `move.saved` (the new session on disk),
//!   `move.released` (once per tab), `move.resaved` (this session saved
//!   without the project), `session.token-saved`, `move.jar-started`.
//!   `:detach tab`: `move-tab.saved`, `move-tab.released`,
//!   `move-tab.resaved`, then the same start.
//! - `:attach project` and `:attach tab`, in the spyc that gives its session
//!   up (`spyc/release`): `release.saved`, `release.token-forgotten`,
//!   `release.released` (once per tab and task); in the spyc that takes it:
//!   `take.saved` and `take.removed` (the giver's file gone), or
//!   `take-tabs.saved` and `take-tabs.removed`.
//!
//! What isn't a point: the context files, staging and capture spill a
//! release removes (none says where a program runs); the MCP socket
//! `mcp::cleanup_socket` removes and the event log flushed and closed before
//! a handover (neither says where a program runs or whose it is); a stale
//! socket or name file `session::start` removes (a jar that has ended);
//! `:name`, which is no handover; and a jar taken back, which writes nothing
//! a crash could lose. `tests/handover_faults.rs` kills spyc at each point and checks that
//! no jar is lost, no program starts twice, and the session comes back.

/// A point in a handover where a debug build may be made to crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Point {
    DetachSaved,
    SessionTokenSaved,
    DetachJarStarted,
    DetachReleased,
    DetachKept,
    UpgradeSaved,
    UpgradeReleased,
    MoveSaved,
    MoveReleased,
    MoveResaved,
    MoveJarStarted,
    MoveTabSaved,
    MoveTabReleased,
    MoveTabResaved,
    ReleaseSaved,
    ReleaseTokenForgotten,
    ReleaseReleased,
    TakeSaved,
    TakeRemoved,
    TakeTabsSaved,
    TakeTabsRemoved,
}

impl Point {
    /// The name `SPYC_HANDOVER_FAULT` gives it.
    #[cfg(any(debug_assertions, test))]
    pub const fn name(self) -> &'static str {
        match self {
            Self::DetachSaved => "detach.saved",
            Self::SessionTokenSaved => "session.token-saved",
            Self::DetachJarStarted => "detach.jar-started",
            Self::DetachReleased => "detach.released",
            Self::DetachKept => "detach.kept",
            Self::UpgradeSaved => "upgrade.saved",
            Self::UpgradeReleased => "upgrade.released",
            Self::MoveSaved => "move.saved",
            Self::MoveReleased => "move.released",
            Self::MoveResaved => "move.resaved",
            Self::MoveJarStarted => "move.jar-started",
            Self::MoveTabSaved => "move-tab.saved",
            Self::MoveTabReleased => "move-tab.released",
            Self::MoveTabResaved => "move-tab.resaved",
            Self::ReleaseSaved => "release.saved",
            Self::ReleaseTokenForgotten => "release.token-forgotten",
            Self::ReleaseReleased => "release.released",
            Self::TakeSaved => "take.saved",
            Self::TakeRemoved => "take.removed",
            Self::TakeTabsSaved => "take-tabs.saved",
            Self::TakeTabsRemoved => "take-tabs.removed",
        }
    }
}

/// The variable that names the point.
#[cfg(debug_assertions)]
const VAR: &str = "SPYC_HANDOVER_FAULT";

/// The point `SPYC_HANDOVER_FAULT` names, and which time it is reached
/// that ends spyc (from 1).
#[cfg(debug_assertions)]
fn asked(value: Option<&str>) -> Option<(String, usize)> {
    let value = value?.trim();
    match value.rsplit_once(':') {
        Some((name, n)) => Some((name.to_string(), n.parse().ok().filter(|&n| n > 0)?)),
        None if value.is_empty() => None,
        None => Some((value.to_string(), 1)),
    }
}

/// `point` is reached: in a debug build, end spyc here with `SIGKILL` when
/// `SPYC_HANDOVER_FAULT` names it, the time it names.
#[cfg(debug_assertions)]
pub fn reached(point: Point) {
    use rustix::process::{Signal, getpid, kill_process};
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static ASKED: OnceLock<Option<(String, usize)>> = OnceLock::new();
    static SEEN: AtomicUsize = AtomicUsize::new(0);
    let Some((name, nth)) = ASKED.get_or_init(|| asked(std::env::var(VAR).ok().as_deref())) else {
        return;
    };
    if point.name() != name || SEEN.fetch_add(1, Ordering::SeqCst) + 1 != *nth {
        return;
    }
    eprintln!("spyc: handover fault at {name}:{nth}, as {VAR} asked");
    // SIGKILL can't be caught or blocked: it is delivered before the call
    // returns, and nothing after it runs.
    let _ = kill_process(getpid(), Signal::KILL);
    loop {
        std::thread::park();
    }
}

#[cfg(not(debug_assertions))]
#[inline]
pub const fn reached(_: Point) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every point, as the enum lists them.
    const ALL: [Point; 21] = [
        Point::DetachSaved,
        Point::SessionTokenSaved,
        Point::DetachJarStarted,
        Point::DetachReleased,
        Point::DetachKept,
        Point::UpgradeSaved,
        Point::UpgradeReleased,
        Point::MoveSaved,
        Point::MoveReleased,
        Point::MoveResaved,
        Point::MoveJarStarted,
        Point::MoveTabSaved,
        Point::MoveTabReleased,
        Point::MoveTabResaved,
        Point::ReleaseSaved,
        Point::ReleaseTokenForgotten,
        Point::ReleaseReleased,
        Point::TakeSaved,
        Point::TakeRemoved,
        Point::TakeTabsSaved,
        Point::TakeTabsRemoved,
    ];

    /// The calls that write what a later spyc reads to find or take a
    /// session (its manifest, its jar's token, its jar), or let a jar go.
    const WRITES: &[&str] = &[
        "save_session_durably(",
        "save_session(",
        "autosave_session(",
        "remove_session(",
        "tokens::save(",
        "tokens::forget(",
        "forget_token(",
        "sever_jar(",
        "session::start(",
    ];

    /// A write with no point after it, kept by its line, and why.
    const KEPT: &[(&str, &str, &str)] = &[(
        "src/pane/jar/session.rs",
        "super::tokens::forget(place, &socket);",
        "the session's jar didn't come up (it failed to spawn, exited, or wasn't \
         live in time and was killed), so the handover stays where it is and \
         the token goes with that jar",
    )];

    /// The handovers' code: two files whole, and the session jar's start.
    fn sources() -> Vec<(&'static str, String)> {
        let session = crate::guard_support::production_half(include_str!("pane/jar/session.rs"));
        let start = session
            .find("fn start_in(")
            .expect("session.rs: no `start_in` to read");
        let end = session[start..]
            .find("\n}\n")
            .map_or(session.len(), |e| start + e + 3);
        vec![
            (
                "src/app/detach.rs",
                crate::guard_support::production_half(include_str!("app/detach.rs")),
            ),
            (
                "src/app/project_move.rs",
                crate::guard_support::production_half(include_str!("app/project_move.rs")),
            ),
            ("src/pane/jar/session.rs", session[start..end].to_string()),
        ]
    }

    /// The name a line that starts a function gives it.
    fn starts_fn(line: &str) -> Option<&str> {
        let mut t = line.trim_start();
        for prefix in ["pub(super) ", "pub(crate) ", "pub ", "const "] {
            t = t.strip_prefix(prefix).unwrap_or(t);
        }
        let rest = t.strip_prefix("fn ")?;
        rest.split(['(', '<']).next()
    }

    /// Each write, by file, line and text, with no point after it before the
    /// next write or the end of its function; and how many writes were read.
    fn unmarked(file: &str, src: &str) -> (Vec<String>, usize) {
        /// A write still waiting for its point, unless it is kept.
        fn flag(file: &str, pending: &mut Option<(usize, String)>, found: &mut Vec<String>) {
            if let Some((n, text)) = pending.take()
                && !KEPT.iter().any(|(f, t, _)| *f == file && *t == text)
            {
                found.push(format!("{file}:{n}: {text}"));
            }
        }
        let mut found = Vec::new();
        let mut writes = 0;
        let mut pending: Option<(usize, String)> = None;
        for (i, line) in src.lines().enumerate() {
            let t = line.trim();
            if t.starts_with("//") {
                continue;
            }
            if starts_fn(line).is_some() {
                flag(file, &mut pending, &mut found);
                continue;
            }
            if t.contains("handover_fault::reached(") {
                pending = None;
                continue;
            }
            if WRITES.iter().any(|w| t.contains(w)) {
                writes += 1;
                flag(file, &mut pending, &mut found);
                pending = Some((i + 1, t.to_string()));
            }
        }
        flag(file, &mut pending, &mut found);
        (found, writes)
    }

    /// G2: every write and release in a handover is followed by a named
    /// point, so a test can crash spyc just after it. Fails closed: it must
    /// read the writes it knows are there, and every kept site must still
    /// be one.
    #[test]
    fn every_handover_write_and_release_is_followed_by_a_fault_point() {
        let mut offenders = Vec::new();
        let mut writes = 0;
        for (file, src) in sources() {
            let (found, n) = unmarked(file, &src);
            assert!(
                n > 0,
                "{file}: no write read; the guard reads the wrong text"
            );
            writes += n;
            offenders.extend(found);
        }
        assert!(
            writes >= 15,
            "only {writes} writes read: the guard reads the wrong text"
        );
        assert!(
            offenders.is_empty(),
            "a handover writes or lets a jar go with no fault point after it \
             (`handover_fault::reached`):\n{}",
            offenders.join("\n")
        );
        for (file, text, why) in KEPT {
            let src = sources()
                .into_iter()
                .find(|(f, _)| f == file)
                .map_or_default(|(_, s)| s);
            assert!(
                src.lines().any(|l| l.trim() == *text),
                "a kept site no longer matches: {file}: {text} ({why})"
            );
        }
    }

    /// G2: every point is reached somewhere in the handovers' code, and the
    /// tests crash spyc at each one: a point nothing reaches, or no test
    /// kills spyc at, proves nothing.
    #[test]
    fn every_fault_point_is_placed_and_has_a_case() {
        let enum_body = include_str!("handover_fault.rs");
        let at = enum_body.find("pub enum Point {").expect("the enum");
        let variants = enum_body[at..]
            .lines()
            .skip(1)
            .take_while(|l| l.trim() != "}")
            .count();
        assert_eq!(variants, ALL.len(), "`ALL` doesn't list every point");
        let code: String = sources().into_iter().map(|(_, s)| s).collect();
        let cases = include_str!("../tests/handover_faults.rs");
        assert!(
            cases.contains("#[test]"),
            "no case read in tests/handover_faults.rs"
        );
        for point in ALL {
            let variant = format!("Point::{point:?}");
            assert!(
                code.contains(&variant),
                "{variant} is reached nowhere in the handovers"
            );
            let (whole, nth) = (
                format!("\"{}\"", point.name()),
                format!("\"{}:", point.name()),
            );
            assert!(
                cases.contains(&whole) || cases.contains(&nth),
                "no case in tests/handover_faults.rs kills spyc at {whole}"
            );
        }
    }

    #[test]
    fn the_variable_names_a_point_and_which_time() {
        assert_eq!(
            asked(Some("detach.saved")),
            Some(("detach.saved".into(), 1))
        );
        assert_eq!(
            asked(Some("move.released:2")),
            Some(("move.released".into(), 2))
        );
        assert_eq!(asked(Some("move.released:0")), None);
        assert_eq!(asked(Some("")), None);
        assert_eq!(asked(None), None);
    }
}
