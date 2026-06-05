//! Git status queries (subprocess backend).
//!
//! Relocated verbatim from `sysinfo.rs`. The pure parser
//! (`parse_porcelain_statuses`) stays in `sysinfo.rs` for now; it moves
//! here when the gix backend lands (PR 4) and produces the structured
//! status map directly.

/// Spawn `git status --porcelain` and return the raw stdout. Split
/// out so callers (e.g. the chdir path) can cache the raw text across
/// navigations within the same repo — the index walk is the expensive
/// part of the spawn and produces identical output for every dir under
/// one repo root.
///
/// Returns `None` if the spawn fails or git exits non-zero.
///
/// Always requests untracked files (`-unormal`). We used to switch to
/// `-uno` on "huge" trees, but the huge-tree flag counts *on-disk*
/// subdirs (dominated by gitignored build dirs like `target/`), while
/// git's untracked scan skips gitignored dirs entirely — so `-unormal`
/// is ~as cheap as `-uno` for the repos that tripped the heuristic,
/// and `-uno` was silently hiding the `?` untracked markers. The cost
/// of `-unormal` on a genuinely large *non-ignored* tree is absorbed
/// by the background git worker (off the UI thread) and the 10 s
/// huge-tree poll throttle.
pub fn porcelain_raw(dir: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain", "-unormal"])
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}
