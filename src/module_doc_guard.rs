//! Guard: every module in `src/` opens with a `//!` doc saying what it is for.
//!
//! AGENTS.md's module index gives each module one line because the module's own
//! doc carries the rest, so a module without one has only that line to explain
//! it. The modules without one when this guard landed are listed in
//! [`UNDOCUMENTED`], and the list only shrinks.

use std::path::Path;

/// Modules with no `//!` doc when this guard landed. Writing one means deleting
/// its entry; adding one defeats the guard.
const UNDOCUMENTED: &[&str] = &[
    "src/agent/agy_metrics/reducer.rs",
    "src/agent/claude_metrics/reducer.rs",
    "src/agent/codex_metrics/reducer.rs",
    "src/agent/status_hook.rs",
    "src/fs/entry.rs",
    "src/fs/listing.rs",
    "src/keymap/action.rs",
    "src/keymap/resolver/mod.rs",
    "src/mcp/hooks/codex.rs",
    "src/state/cursor.rs",
    "src/state/ignore.rs",
    "src/state/picks.rs",
    "src/ui/prompt.rs",
    "src/ui/status.rs",
];

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
    let mut problems = Vec::new();
    for (rel, src) in &modules {
        match (has_module_doc(src), UNDOCUMENTED.contains(&rel.as_str())) {
            (false, false) => problems.push(format!(
                "{rel}: no `//!` module doc. Say what the module is for, in a line or two."
            )),
            (true, true) => problems.push(format!(
                "{rel}: has a module doc now. Delete its UNDOCUMENTED entry."
            )),
            _ => {}
        }
    }
    for listed in UNDOCUMENTED {
        if !modules.iter().any(|(rel, _)| rel == listed) {
            problems.push(format!(
                "{listed}: listed in UNDOCUMENTED but not found. Delete it."
            ));
        }
    }
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

#[test]
fn undocumented_is_sorted_with_one_entry_per_file() {
    assert!(
        UNDOCUMENTED.windows(2).all(|pair| pair[0] < pair[1]),
        "keep UNDOCUMENTED sorted, one entry per file"
    );
}
