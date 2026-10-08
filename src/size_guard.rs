//! Guard: how long a `.rs` file may get.
//!
//! Two limits. A file's non-test code stays under [`PRODUCTION`]'s, because
//! that is the code a reader, a reviewer and an agent's context have to hold
//! at once. The whole file, tests included, stays under [`WHOLE_FILE`]'s, the
//! ceiling rust-lang/rust's `tidy` sets: tests are flat lists of independent
//! cases, so their length costs far less.
//!
//! Until this guard only `app/mod.rs` had a limit (`mod_rs_stays_decomposed`),
//! and the documented one went unenforced long enough for dozens of files to
//! pass it. Enforcing the limits outright would first need every one of those
//! refactored, so this is a ratchet: each file over a limit when the guard
//! landed is pinned at its length then, and from there it may only shrink.
//! Every way a pin can drift from its file fails:
//!
//! - a file without a pin goes over a limit;
//! - a pinned file grows past its pin;
//! - a pinned file sits more than [`SLACK`] lines below its pin, so the pin
//!   follows it down and the freed room can't be regrown;
//! - a pinned file is back under the limit, or gone, so its entry is deleted.
//!
//! Raising a pin defeats the guard. Extract a cohesive child module instead: a
//! verbatim, behaviour-identical relocation (AGENTS.md → Conventions).

use std::path::Path;

/// How far a pinned file may sit below its pin before the pin must come down.
/// An extraction always exceeds it; a fix that deletes a few lines doesn't.
const SLACK: usize = 25;

/// Where the repo's Rust lives. `spikes/` is left out: throwaway experiments
/// outside the workspace, which nobody maintains.
const ROOTS: &[&str] = &["src", "crates", "tests", "fuzz/fuzz_targets", "build.rs"];

/// Files neither limit applies to, with the reason.
const EXEMPT: &[(&str, &str)] = &[(
    "crates/spyc-vt-sys/src/bindings.rs",
    "bindgen output, checked in so the build needs neither bindgen nor libclang",
)];

/// One limit and the files pinned over it.
struct Rule {
    /// The pin list's name, as error messages tell the reader to edit it.
    list: &'static str,
    /// What a counted line is, for error messages.
    unit: &'static str,
    limit: usize,
    /// Files over `limit` when the guard landed, sorted by path, each pinned at
    /// its length. An entry only ever moves down, then out.
    pins: &'static [(&'static str, usize)],
}

/// Non-test code: everything `guard_support::production_half` keeps, and
/// nothing of a whole-file test module.
const PRODUCTION: Rule = Rule {
    list: "PRODUCTION",
    unit: "non-test lines",
    limit: 1000,
    pins: &[
        ("src/app/archive.rs", 1494),
        ("src/app/commands.rs", 1059),
        ("src/app/effect.rs", 1156),
        ("src/app/pager_handler/mod.rs", 1068),
        ("src/app/run.rs", 1102),
        ("src/app/state/mod.rs", 1213),
        ("src/config/mod.rs", 1277),
        ("src/mcp/protocol.rs", 1272),
    ],
};

/// Every line of the file.
const WHOLE_FILE: Rule = Rule {
    list: "WHOLE_FILE",
    unit: "lines",
    limit: 3000,
    pins: &[("src/app/harness_tests/archive.rs", 3298)],
};

impl Rule {
    fn pin(&self, path: &str) -> Option<usize> {
        self.pins
            .iter()
            .find(|(pinned, _)| *pinned == path)
            .map(|&(_, pin)| pin)
    }

    /// What is wrong with `path` at `lines`, or `None` when it satisfies the
    /// ratchet.
    fn verdict(&self, path: &str, lines: usize) -> Option<String> {
        let (list, unit, limit) = (self.list, self.unit, self.limit);
        let Some(pin) = self.pin(path) else {
            return (lines > limit).then(|| {
                format!(
                    "{path}: {lines} {unit}, over the {limit}-line limit. Split out a \
                     cohesive module."
                )
            });
        };
        if lines > pin {
            Some(format!(
                "{path}: grew to {lines} {unit}, past its {list} pin of {pin}. Split out a \
                 cohesive module; don't raise the pin."
            ))
        } else if lines <= limit {
            Some(format!(
                "{path}: down to {lines} {unit}, under the {limit}-line limit. Delete its \
                 {list} pin."
            ))
        } else if pin - lines > SLACK {
            Some(format!(
                "{path}: shrank to {lines} {unit}. Lower its {list} pin from {pin} to {lines}."
            ))
        } else {
            None
        }
    }
}

/// A whole-file test module, reached through `#[cfg(test)] mod name;` and so
/// carrying no in-file marker for `production_half` to find. The repo's naming
/// convention, as `git::no_subprocess_git_in_production` reads it too.
fn is_test_module(path: &str) -> bool {
    let p = Path::new(path);
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let under_tests = p.parent().is_some_and(|dir| {
        dir.components()
            .filter_map(|c| c.as_os_str().to_str())
            .any(|c| c == "tests" || c.ends_with("_tests"))
    });
    name == "tests.rs" || name == "test_support.rs" || name.ends_with("_tests.rs") || under_tests
}

/// Every `.rs` file under [`ROOTS`]: `(repo-relative path, lines, non-test lines)`.
fn rust_files(repo: &Path) -> Vec<(String, usize, usize)> {
    let mut found = Vec::new();
    for root in ROOTS {
        collect(repo, &repo.join(root), &mut found);
    }
    found
}

fn collect(repo: &Path, path: &Path, found: &mut Vec<(String, usize, usize)>) {
    if path.is_dir() {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name == "target" || name.starts_with('.') {
            return;
        }
        let entries =
            std::fs::read_dir(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        for entry in entries {
            collect(repo, &entry.expect("directory entry").path(), found);
        }
    } else if path.extension().is_some_and(|ext| ext == "rs") {
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let rel = path.strip_prefix(repo).expect("walked from the repo root");
        let rel = rel.to_string_lossy().replace('\\', "/");
        let production = if is_test_module(&rel) {
            0
        } else {
            crate::guard_support::production_half(&src).lines().count()
        };
        found.push((rel, src.lines().count(), production));
    }
}

#[test]
fn no_rust_file_grows_past_its_limits() {
    let files = rust_files(Path::new(env!("CARGO_MANIFEST_DIR")));
    // A walk that finds nothing passes everything.
    assert!(
        files.len() > 100 && files.iter().any(|(path, ..)| path == "src/lib.rs"),
        "the walk found {} files; it is looking in the wrong place",
        files.len()
    );
    let mut problems: Vec<String> = Vec::new();
    for (path, lines, production) in &files {
        if EXEMPT.iter().any(|(exempt, _)| exempt == path) {
            continue;
        }
        problems.extend(PRODUCTION.verdict(path, *production));
        problems.extend(WHOLE_FILE.verdict(path, *lines));
    }
    let found = |name: &str| files.iter().any(|(path, ..)| path == name);
    let listed = PRODUCTION
        .pins
        .iter()
        .chain(WHOLE_FILE.pins)
        .map(|(path, _)| *path);
    for path in listed.chain(EXEMPT.iter().map(|(path, _)| *path)) {
        if !found(path) {
            problems.push(format!(
                "{path}: listed in src/size_guard.rs but not found. Delete it."
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "file-size ratchet (src/size_guard.rs):\n{}",
        problems.join("\n")
    );
}

#[test]
fn the_ratchet_turns_one_way() {
    const RULE: Rule = Rule {
        list: "TEST",
        unit: "lines",
        limit: 800,
        pins: &[("pinned.rs", 1000)],
    };
    // Unpinned: the limit itself passes, one line more fails.
    assert_eq!(RULE.verdict("free.rs", 800), None);
    assert!(RULE.verdict("free.rs", 801).is_some());
    // Pinned: at the pin, or up to SLACK below it, passes.
    assert_eq!(RULE.verdict("pinned.rs", 1000), None);
    assert_eq!(RULE.verdict("pinned.rs", 1000 - SLACK), None);
    // Growth fails, even by one line.
    let grew = RULE.verdict("pinned.rs", 1001).expect("growth fails");
    assert!(grew.contains("past its TEST pin"), "{grew}");
    // A shrink past SLACK fails until the pin follows it down.
    let shrank = RULE
        .verdict("pinned.rs", 1000 - SLACK - 1)
        .expect("a stale pin fails");
    assert!(shrank.contains("Lower its TEST pin"), "{shrank}");
    // Back under the limit, the entry goes.
    let paid = RULE
        .verdict("pinned.rs", 800)
        .expect("a paid-off pin fails");
    assert!(paid.contains("Delete its TEST pin"), "{paid}");
}

#[test]
fn whole_file_test_modules_are_recognized_by_name() {
    for test in [
        "src/ui/diff_render/tests.rs",
        "src/app/mod_tests.rs",
        "src/app/state/tests/mod.rs",
        "src/app/harness_tests/archive.rs",
        "src/git/test_support.rs",
        "tests/filesystem.rs",
    ] {
        assert!(is_test_module(test), "{test} is a test module");
    }
    for production in ["src/app/effect.rs", "src/lib.rs", "src/testing_ground.rs"] {
        assert!(!is_test_module(production), "{production} is production");
    }
}

#[test]
fn pins_are_sorted_with_one_entry_per_file() {
    for rule in [&PRODUCTION, &WHOLE_FILE] {
        assert!(
            rule.pins.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "keep {} sorted by path, one entry per file",
            rule.list
        );
    }
}
