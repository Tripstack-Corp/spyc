//! Guard: every module in `src/` opens with a `//!` doc saying what it is for.
//!
//! AGENTS.md's module index gives each module one line because the module's own
//! doc carries the rest, so a module without one has only that line to explain
//! it.

use std::path::Path;

/// Whether `src` opens with a module doc, past any inner attributes.
fn has_module_doc(src: &str) -> bool {
    src.lines()
        .map(str::trim_start)
        .find(|l| !l.is_empty() && !l.starts_with("#!["))
        .is_some_and(|l| l.starts_with("//!"))
}

#[test]
fn every_module_opens_with_a_module_doc() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let modules: Vec<(String, String)> = crate::size_guard::rust_sources(repo)
        .into_iter()
        .filter(|(rel, _)| rel.starts_with("src/") && !crate::size_guard::is_test_module(rel))
        .collect();
    // A walk that finds nothing passes everything.
    assert!(
        modules.len() > 200,
        "found {} modules; the walk is looking in the wrong place",
        modules.len()
    );
    let mut problems: Vec<String> = modules
        .iter()
        .filter(|(_, src)| !has_module_doc(src))
        .map(|(rel, _)| {
            format!("{rel}: no `//!` module doc. Say what the module is for, in a line or two.")
        })
        .collect();
    problems.sort();
    assert!(
        problems.is_empty(),
        "module docs (src/module_doc_guard.rs):\n{}",
        problems.join("\n")
    );
}

#[test]
fn a_module_doc_is_the_first_thing_past_inner_attributes() {
    assert!(has_module_doc("//! What this is.\nuse std::fs;\n"));
    assert!(has_module_doc(
        "#![allow(dead_code)]\n\n//! What this is.\n"
    ));
    assert!(!has_module_doc("/// An item's doc.\nfn f() {}\n"));
    assert!(!has_module_doc("// A plain comment.\n//! Too late.\n"));
    assert!(!has_module_doc("use std::fs;\n"));
    assert!(!has_module_doc(""));
}
