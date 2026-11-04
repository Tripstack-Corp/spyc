//! Makefile target discovery: what `targets` lists from a Makefile's text, and
//! what `discover` reads from disk, `include`s and all. Fixtures are written
//! to a temp dir; spyc's own `Makefile` is read where it is.

use std::path::Path;

use super::make::{MakeTarget, targets};
use super::{Taskfile, discover};

fn names(listed: &[MakeTarget]) -> Vec<&str> {
    listed.iter().map(|t| t.name.as_str()).collect()
}

fn find<'a>(listed: &'a [MakeTarget], name: &str) -> &'a MakeTarget {
    listed
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("{name} is not listed: {:?}", names(listed)))
}

fn label<'a>(listed: &'a [MakeTarget], name: &str) -> Option<&'a str> {
    find(listed, name).label.as_deref()
}

/// Write `files` (`(relative path, text)`) under `dir`, making parents.
fn write(dir: &Path, files: &[(&str, &str)]) {
    for (rel, text) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().expect("a parent")).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

fn discovered(dir: &Path, root: &Path) -> Taskfile {
    discover(dir, root).unwrap_or_else(|e| panic!("discover {}: {e:#}", dir.display()))
}

// ── spyc's own Makefile ──

/// spyc's own `Makefile`, read where it is: the `##` labels its `make help`
/// prints, `build` (its first rule) as the default, and nothing that isn't a
/// target a user can name.
#[test]
fn spycs_own_makefile_lists_its_targets_with_their_labels() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let found = discovered(repo, repo);
    assert_eq!(found.path, repo.join("Makefile"));
    assert!(found.is_default, "plain `make` reads it");
    let listed = &found.targets;
    assert_eq!(label(listed, "check"), Some("Full quality gate (CI)"));
    assert_eq!(
        label(listed, "install"),
        Some("Install to ~/.local/bin (builds release first; override with PREFIX=/usr/local)")
    );
    assert!(
        label(listed, "fuzz").is_some_and(|l| l.starts_with("Coverage-guided fuzz")),
        "{:?}",
        label(listed, "fuzz")
    );
    assert_eq!(
        label(listed, "release-macos-universal"),
        Some("macOS Universal binary (arm64 + x86_64)"),
        "a label after the prerequisites"
    );
    let defaults: Vec<&str> = listed
        .iter()
        .filter(|t| t.default)
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(defaults, ["build"], "the first rule is the default goal");
    assert!(listed.iter().all(|t| t.phony), "every target is .PHONY");
    for t in listed {
        assert!(
            !t.name.starts_with('.') && !t.name.contains('%') && !t.name.contains('$'),
            "{} is not a target a user names",
            t.name
        );
    }
    assert_eq!(
        names(listed)[..3],
        ["build", "run", "check"],
        "source order"
    );
}

// ── which targets ──

const EXCLUSIONS: &str = "\
OBJS = a.o b.o
BIN := app

all: $(BIN)

%.o: %.c
\tcc -c $<

$(BIN): $(OBJS)
\tcc -o $@ $^

${OBJS}: x.h

.hidden:
\t@true

$(OBJS): %.o: %.c
\tcc -c $<

a.o b.o: %.o: %.c
\tcc -c $<

build: CFLAGS = -O2
debug: CFLAGS = -g

build:
\t@true

clean::
\trm -f a.o

clean::
\trm -f b.o

libx.a(member.o): member.o

define TEMPLATE
fake: other
\t@echo never a rule
endef

ifeq ($(OS),Windows_NT)
windows-only:
\t@true
else
unix-only:
\t@true
endif

one \\
  two: ; @true
";

/// Only targets a user can name: no pattern rule, no unexpanded reference, no
/// `.`-led name, no static pattern, no target-specific variable, no archive
/// member. A double-colon rule's target is listed once, a `define` body names
/// no rule, both branches of a conditional are listed (nothing is evaluated),
/// and a line continuation joins two targets.
#[test]
fn only_explicit_targets_are_listed() {
    let listed = targets(EXCLUSIONS);
    assert_eq!(
        names(&listed),
        [
            "all",
            "build",
            "clean",
            "windows-only",
            "unix-only",
            "one",
            "two"
        ]
    );
}

/// `.PHONY` targets come first, in source order, then the rest.
#[test]
fn phony_targets_come_first() {
    let listed =
        targets("out.txt:\n\ttouch $@\n.PHONY: test lint\nlint:\n\t@true\ntest:\n\t@true\n");
    assert_eq!(names(&listed), ["lint", "test", "out.txt"]);
    assert!(find(&listed, "lint").phony && !find(&listed, "out.txt").phony);
}

/// The default goal is the first target of the first rule, or what
/// `.DEFAULT_GOAL` names.
#[test]
fn the_default_goal_is_marked() {
    let first = targets(".PHONY: b\na:\n\t@true\nb:\n\t@true\n");
    assert!(find(&first, "a").default && !find(&first, "b").default);
    let named = targets("a:\n\t@true\nb:\n\t@true\n.DEFAULT_GOAL := b\n");
    assert!(find(&named, "b").default && !find(&named, "a").default);
}

/// A Makefile with only pattern rules lists nothing, and that is not an error.
#[test]
fn a_makefile_of_pattern_rules_lists_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[("Makefile", "%.o: %.c\n\tcc -c $<\n%: %.o\n\tcc -o $@ $<\n")],
    );
    assert!(discovered(tmp.path(), tmp.path()).targets.is_empty());
}

// ── labels ──

/// The trailing `##` on the rule's line wins over a comment above it; a
/// comment above is taken when there is none; a comment parted from the rule
/// by a blank line is not.
#[test]
fn labels_come_from_the_rules_line_then_the_comment_above() {
    let listed = targets(
        "# Above build.\nbuild: deps ## Build it all\n\t@true\n\n\
         # Run the tests.\ntest:\n\t@true\n\n\
         # Not about lone.\n\nlone:\n\t@true\n\n\
         deps: # a plain comment\n\t@true\n",
    );
    assert_eq!(label(&listed, "build"), Some("Build it all"));
    assert_eq!(label(&listed, "test"), Some("Run the tests."));
    assert_eq!(label(&listed, "lone"), None);
    assert_eq!(
        label(&listed, "deps"),
        None,
        "only `##` labels a rule's line"
    );
}

// ── includes ──

/// A literal `include` inside the root is followed; one that climbs out of it,
/// by `..` or by an absolute path, is not; a missing `-include` is silent.
#[test]
fn includes_are_followed_inside_the_root_only() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let outside = tmp.path().join("outside.mk");
    write(tmp.path(), &[("outside.mk", "escaped:\n\t@true\n")]);
    write(
        &root,
        &[
            ("sub/inner.mk", "inner: ## From the include\n\t@true\n"),
            (
                "Makefile",
                &format!(
                    "top:\n\t@true\ninclude sub/inner.mk\ninclude ../outside.mk\n\
                     include {}\n-include missing.mk\n",
                    outside.display()
                ),
            ),
        ],
    );

    let listed = discovered(&root, &root).targets;

    assert_eq!(names(&listed), ["top", "inner"]);
    assert_eq!(label(&listed, "inner"), Some("From the include"));
}

/// Exit criterion 5: discovery runs nothing. A `$(shell …)` in a variable, a
/// target name, an include path and a recipe leaves no file behind, and the
/// plain target beside them is still listed.
#[test]
fn discovery_never_evaluates_the_makefile() {
    let tmp = tempfile::tempdir().unwrap();
    let mark = |what: &str| tmp.path().join(format!("pwned-{what}"));
    let text = format!(
        "STAMP := $(shell touch {var})\n\
         $(shell touch {target}):\n\t@true\n\
         include $(shell touch {include})\n\
         safe:\n\ttouch {recipe}\n\t$(shell touch {recipe})\n",
        var = mark("var").display(),
        target = mark("target").display(),
        include = mark("include").display(),
        recipe = mark("recipe").display(),
    );
    let dir = tmp.path().join("work");
    write(&dir, &[("Makefile", &text)]);

    let listed = discovered(&dir, &dir).targets;

    assert_eq!(names(&listed), ["safe"]);
    for what in ["var", "target", "include", "recipe"] {
        assert!(!mark(what).exists(), "{what}: discovery ran a command");
    }
}

// ── which Makefile ──

/// make reads `GNUmakefile` before `Makefile`, so a directory holding both is
/// read through the former, and the latter, named, is not make's pick.
#[test]
fn a_directory_is_read_through_the_file_make_picks() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            ("GNUmakefile", "gnu:\n\t@true\n"),
            ("Makefile", "plain:\n\t@true\n"),
        ],
    );

    let picked = discovered(tmp.path(), tmp.path());
    assert_eq!(picked.path, tmp.path().join("GNUmakefile"));
    assert!(picked.is_default);
    assert_eq!(names(&picked.targets), ["gnu"]);

    let named = discovered(&tmp.path().join("Makefile"), tmp.path());
    assert_eq!(named.path, tmp.path().join("Makefile"));
    assert!(!named.is_default, "make would read GNUmakefile");
    assert_eq!(names(&named.targets), ["plain"]);
}

/// A directory without a Makefile is an error that says so.
#[test]
fn a_directory_without_a_makefile_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let err = discover(tmp.path(), tmp.path()).expect_err("no Makefile");
    assert_eq!(
        format!("{err:#}"),
        format!("no Makefile in {}", tmp.path().display())
    );
}

/// A FIFO named `Makefile` is no Makefile, and opening it to read would wait
/// for a writer for ever, holding the worker that asked. `discover` refuses
/// it at once, naming it.
#[cfg(unix)]
#[test]
fn a_fifo_named_makefile_is_refused_without_blocking() {
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let fifo = tmp.path().join("Makefile");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let (path, root) = (fifo.clone(), tmp.path().to_path_buf());
    std::thread::spawn(move || {
        let _ = tx.send(discover(&path, &root).map_err(|e| format!("{e:#}")));
    });

    let answer = rx.recv_timeout(std::time::Duration::from_secs(2));
    // A reader blocked in `open` is released by a writer; with none blocked,
    // the non-blocking open fails and nothing waits.
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&fifo);

    let found = answer.expect("discover returned, rather than waiting on the FIFO");
    assert_eq!(
        found.expect_err("a FIFO is refused"),
        format!("{} is not a regular file", fifo.display())
    );
}
