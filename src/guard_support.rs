//! Shared helper for the source-scanning guard tests.
//!
//! Several guards assert a policy over *production* source only — test fixtures
//! legitimately do things production must not (spawn `git`, write state
//! non-atomically, poke `state.left`). Each one therefore needs to know which
//! part of a file is production.
//!
//! They each used to answer that with `src.split("#[cfg(test)]").next()`, which
//! is wrong twice over:
//!
//! 1. A **comment merely mentioning** the attribute truncates the scan. Under
//!    that heuristic `src/app/render/mod.rs` reads as 22 production lines when
//!    it has 827 — line 23 is the prose "`#[cfg(test)]` modules below opt back
//!    out". Everything after is unscanned.
//! 2. A `#[cfg(test)]` item **in the middle** of a file truncates the rest.
//!    `src/git/worktree.rs` declares `#[cfg(test)] pub mod test_support;` at
//!    line 128 of 1,329, so 90% of it went unscanned.
//!
//! Both failures are silent, and both fail *open* — the guard passes while
//! checking almost nothing, which reads as assurance while providing none.
//!
//! [`production_half`] instead removes `#[cfg(test)]`-guarded items and keeps
//! the rest. Where it can't confidently identify an item's extent it keeps the
//! code, biasing to false positives: a spurious offender is loud and gets
//! fixed, a missed one is the failure mode this replaces.

/// Source with `#[cfg(test)]`-guarded items removed.
///
/// Handles the two shapes that actually appear: `#[cfg(test)] mod name { … }`
/// (brace-matched and dropped) and `#[cfg(test)] mod name;` (line dropped). Any
/// other guarded item keeps its body — see the module note on biasing toward
/// false positives.
///
/// Only an attribute that *starts* a line (ignoring indentation) counts, so
/// prose mentioning it in a comment is inert.
pub fn production_half(src: &str) -> String {
    let lines: Vec<&str> = src.split('\n').collect();
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("#[cfg(test)]") {
            // Find the item this attribute guards, skipping blank lines and
            // any further attributes stacked on it.
            let mut j = i + 1;
            while j < lines.len() {
                let t = lines[j].trim_start();
                if t.is_empty() || t.starts_with("#[") {
                    j += 1;
                } else {
                    break;
                }
            }
            if j < lines.len() {
                let item = lines[j].trim_start();
                let is_mod_decl = item.starts_with("mod ") || item.starts_with("pub mod ");
                if is_mod_decl && item.ends_with(';') {
                    i = j + 1; // `#[cfg(test)] mod name;`
                    continue;
                }
                if is_mod_decl && item.contains('{') {
                    // Brace-match the module body and drop the whole block.
                    let mut depth = 0i32;
                    let mut k = j;
                    loop {
                        depth += i32::try_from(lines[k].matches('{').count()).unwrap_or(0);
                        depth -= i32::try_from(lines[k].matches('}').count()).unwrap_or(0);
                        k += 1;
                        if depth <= 0 || k >= lines.len() {
                            break;
                        }
                    }
                    i = k;
                    continue;
                }
            }
            // Unrecognized guarded item — drop only the attribute line and keep
            // scanning its body. Conservative on purpose.
            i += 1;
            continue;
        }
        out.push(lines[i]);
        i += 1;
    }
    out.join("\n")
}

/// Every markdown file under `root`, as sorted repo-relative paths with `/`
/// separators, drawn from [`tracked_files`]; each guard drops whatever its own
/// policy exempts.
pub fn markdown_files(root: &std::path::Path) -> Vec<String> {
    repo_files(root, &["md"])
}

/// Every file under `root` with one of `extensions`, as [`markdown_files`]
/// lists markdown.
pub fn repo_files(root: &std::path::Path, extensions: &[&str]) -> Vec<String> {
    tracked_files(root)
        .iter()
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| extensions.contains(&x))
        })
        .filter_map(|p| p.strip_prefix(root).ok())
        .map(|p| p.display().to_string().replace('\\', "/"))
        .collect()
}

/// Every file the repo at `root` holds, as sorted absolute paths: the index's
/// entries that exist on disk, so a staged new file counts and an untracked one
/// doesn't. An untracked file belongs to a tool or to the user (an agent's
/// memory directory, an editor's workspace file), and checking it fails a guard
/// on one machine that passes on every clean clone. Outside a git checkout (a
/// source tarball) it is every file a walk finds, build output and VCS
/// metadata skipped, so a guard there still checks everything.
pub fn tracked_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let index = gix::open(root)
        .ok()
        .and_then(|repo| repo.index_or_empty().ok());
    let mut out: Vec<std::path::PathBuf> = if let Some(index) = index {
        index
            .entries()
            .iter()
            .map(|e| root.join(gix::path::from_bstr(e.path(&index))))
            .filter(|p| p.is_file())
            .collect()
    } else {
        let mut found = Vec::new();
        walk_all(root, &mut found);
        found
    };
    out.sort();
    out.dedup();
    out
}

fn walk_all(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".cache"];
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            if !SKIP_DIRS.contains(&name) {
                walk_all(&p, out);
            }
        } else {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{production_half, repo_files, tracked_files};
    use crate::git::test_support::run_git;

    #[test]
    fn a_comment_mentioning_the_attribute_does_not_truncate() {
        // The exact shape that blinded the old heuristic (render/mod.rs:23).
        let src = concat!(
            "// `#[cfg(test)]",
            "` modules below opt back out.\n",
            "fn production() { OFFENDER }\n",
        );
        assert!(
            production_half(src).contains("OFFENDER"),
            "prose mentioning the attribute must not end the scan"
        );
    }

    #[test]
    fn a_midfile_test_mod_does_not_truncate_the_rest() {
        // The shape in git/worktree.rs: a cfg(test) mod declaration at line 128
        // of 1,329, with production continuing after it.
        let src = concat!(
            "fn before() {}\n",
            "#[cfg(test)]\n",
            "pub mod test_support;\n",
            "fn after() { OFFENDER }\n",
        );
        let prod = production_half(src);
        assert!(
            prod.contains("OFFENDER"),
            "production after a cfg(test) mod must still be scanned"
        );
        assert!(
            !prod.contains("test_support"),
            "the guarded declaration itself must be dropped"
        );
    }

    #[test]
    fn a_test_module_body_is_dropped() {
        let src = concat!(
            "fn production() {}\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    fn fixture() { FIXTURE_ONLY }\n",
            "}\n",
        );
        let prod = production_half(src);
        assert!(prod.contains("production"));
        assert!(
            !prod.contains("FIXTURE_ONLY"),
            "test bodies must not be scanned — fixtures may do what production may not"
        );
    }

    #[test]
    fn nested_braces_in_a_test_module_are_matched() {
        let src = concat!(
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    fn a() { if x { y(); } }\n",
            "}\n",
            "fn after() { OFFENDER }\n",
        );
        let prod = production_half(src);
        assert!(
            prod.contains("OFFENDER"),
            "brace matching must find the real end of the module"
        );
    }

    /// A guard reads what the repo holds: a committed file and a staged new
    /// one, never an untracked file a tool left in the checkout.
    #[test]
    fn a_guard_reads_tracked_and_staged_files_but_not_untracked_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        run_git(root, &["init", "-q", "--initial-branch=main"]);
        std::fs::write(root.join("committed.md"), "x\n").unwrap();
        run_git(root, &["add", "committed.md"]);
        run_git(
            root,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "c",
            ],
        );
        std::fs::create_dir(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/staged.md"), "x\n").unwrap();
        run_git(root, &["add", "docs/staged.md"]);
        std::fs::create_dir(root.join(".serena")).unwrap();
        std::fs::write(root.join(".serena/notes.md"), "x\n").unwrap();
        std::fs::write(root.join("untracked.md"), "x\n").unwrap();

        assert_eq!(
            repo_files(root, &["md"]),
            ["committed.md", "docs/staged.md"]
        );
    }

    /// Outside a git checkout there is no index to go by, so every file
    /// counts: the guard checks everything rather than nothing.
    #[test]
    fn outside_a_checkout_every_file_is_read() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("target/built.md"), "x\n").unwrap();
        std::fs::write(root.join("a.md"), "x\n").unwrap();
        std::fs::write(root.join("b.rs"), "x\n").unwrap();

        assert_eq!(tracked_files(root), [root.join("a.md"), root.join("b.rs")]);
    }
}
