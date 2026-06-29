//! Git merge driver for spyc's version-line conflicts.
//!
//! spyc bumps `version = "x.y.z"` in `Cargo.toml` (and the `[[package]] name =
//! "spyc"` entry in `Cargo.lock`) on every PR. So any two concurrent PRs are
//! guaranteed to conflict on that one line — never on real content. Resolving
//! it by hand on every merge-train lap is pure overhead.
//!
//! Wired via `.gitattributes`: `Cargo.toml merge=spyc-semver` invokes
//! `spyc --merge-driver %O %A %B`, which runs a three-way merge in-process (via
//! `diffy` — no `git` subprocess, per the `no_subprocess_git_in_production`
//! guard) and, for any residual conflict hunk whose two sides are *only*
//! `version = "..."` lines, keeps the higher semver. A conflict touching
//! anything else is left intact so it surfaces to the human — the driver never
//! auto-resolves real code.
//!
//! `Cargo.lock merge=ours` rides git's built-in `true` driver instead (keep our
//! side on conflict): diffy is more conservative than git's merge and would
//! turn cleanly-mergeable lock churn into spurious conflicts, whereas a kept
//! lock is regenerated deterministically by the next `cargo` invocation. Both
//! driver configs are installed by [`ensure_installed`].

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

/// The git-config merge-driver name (referenced by `.gitattributes`).
const DRIVER_NAME: &str = "spyc-semver";

/// Run the merge driver: three-way merge `current` (ours, git's `%A`) against
/// `other` (theirs, `%B`) over the common `base` (`%O`), resolving version-only
/// conflicts. Writes the result back to `current`. Returns `true` when the file
/// is fully resolved, `false` when a non-version conflict remains (the caller
/// exits non-zero so git reports the conflict).
pub fn run_merge_driver(base: &str, current: &str, other: &str) -> Result<bool> {
    let base_s = fs::read_to_string(base).with_context(|| format!("read base {base}"))?;
    let ours = fs::read_to_string(current).with_context(|| format!("read current {current}"))?;
    let theirs = fs::read_to_string(other).with_context(|| format!("read other {other}"))?;
    let (merged, clean) = merge_versioned(&base_s, &ours, &theirs);
    fs::write(current, merged).with_context(|| format!("write {current}"))?;
    Ok(clean)
}

/// Three-way merge with version-only conflict resolution. Returns the merged
/// text and whether it is conflict-free.
pub fn merge_versioned(base: &str, ours: &str, theirs: &str) -> (String, bool) {
    match diffy::merge(base, ours, theirs) {
        Ok(clean) => (clean, true),
        Err(conflicted) => match resolve_version_conflicts(&conflicted) {
            Some(resolved) => (resolved, true),
            None => (conflicted, false),
        },
    }
}

/// Walk a conflict-marked merge result. If *every* conflict hunk's two sides
/// consist solely of `version = "x.y.z"` lines, replace each hunk with the
/// higher-semver line and return the cleaned text. Returns `None` the moment a
/// hunk holds anything else (a real conflict we must not touch) or the markers
/// are malformed.
fn resolve_version_conflicts(text: &str) -> Option<String> {
    let trailing_newline = text.ends_with('\n');
    let mut out: Vec<String> = Vec::new();
    let mut iter = text.lines();
    while let Some(line) = iter.next() {
        if !line.starts_with("<<<<<<<") {
            out.push(line.to_string());
            continue;
        }
        // "ours" runs until the base marker (diff3 style) or the separator.
        let mut ours: Vec<&str> = Vec::new();
        'ours: loop {
            let l = iter.next()?; // EOF inside a conflict → malformed
            if l.starts_with("|||||||") {
                // diff3 base section — skip it through to the separator.
                loop {
                    if iter.next()?.starts_with("=======") {
                        break 'ours;
                    }
                }
            } else if l.starts_with("=======") {
                break 'ours;
            } else {
                ours.push(l);
            }
        }
        // "theirs" runs until the closing marker.
        let mut theirs: Vec<&str> = Vec::new();
        loop {
            let l = iter.next()?;
            if l.starts_with(">>>>>>>") {
                break;
            }
            theirs.push(l);
        }
        // Both sides must be version-only, else this is a real conflict.
        let (ov, oline) = sole_version(&ours)?;
        let (tv, tline) = sole_version(&theirs)?;
        out.push(if ov >= tv { oline } else { tline }.to_string());
    }
    let mut s = out.join("\n");
    if trailing_newline {
        s.push('\n');
    }
    Some(s)
}

/// If `lines` (a conflict side) contains only blank lines and `version = "..."`
/// lines — at least one — return the highest semver found and the line that
/// carries it (formatting preserved). Otherwise `None`.
fn sole_version<'a>(lines: &[&'a str]) -> Option<((u64, u64, u64), &'a str)> {
    let mut best: Option<((u64, u64, u64), &'a str)> = None;
    for &l in lines {
        if l.trim().is_empty() {
            continue;
        }
        let v = parse_version_line(l)?; // any non-version line → bail
        if best.is_none_or(|(bv, _)| v > bv) {
            best = Some((v, l));
        }
    }
    best
}

/// Parse a `version = "MAJOR.MINOR.PATCH"` line into its semver triple. Returns
/// `None` for anything that isn't exactly that shape (extra components, ranges,
/// pre-release suffixes), so the resolver treats it as a real conflict.
fn parse_version_line(line: &str) -> Option<(u64, u64, u64)> {
    let rest = line.trim().strip_prefix("version")?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let inner = rest.strip_prefix('"')?.strip_suffix('"')?;
    let mut parts = inner.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None; // x.y.z.w — not our shape
    }
    Some((major, minor, patch))
}

/// Ensure spyc's merge drivers are configured in the repo's shared git config
/// so `.gitattributes` actually takes effect. Idempotent and best-effort:
/// returns whether it wrote. Locates the common git dir via gix (shared across
/// linked worktrees, so one install covers every worktree) and appends the
/// `[merge "spyc-semver"]` (version resolver) + `[merge "ours"]` (keep-ours, for
/// `Cargo.lock`) sections if absent — no `git` subprocess (gix discovery + a
/// plain-fs append).
pub fn ensure_installed(start_dir: &Path) -> Result<bool> {
    let repo = gix::discover(start_dir)?;
    let common = fs::canonicalize(repo.common_dir()).unwrap_or_else(|_| repo.common_dir().into());
    let config_path = common.join("config");
    let existing = fs::read_to_string(&config_path).unwrap_or_default();
    if existing.contains(&format!("[merge \"{DRIVER_NAME}\"]")) {
        return Ok(false);
    }
    // `spyc-semver`: the driver resolves by exit code (0 = resolved); git passes
    // the ancestor / current / other temp paths as %O / %A / %B. `spyc` is found
    // on PATH; if it isn't, git treats the failed driver as a conflict (safe).
    // `ours`: git's built-in no-op `true` driver keeps %A — used by Cargo.lock.
    let section = format!(
        "\n[merge \"{DRIVER_NAME}\"]\n\
         \tname = resolve spyc version-line conflicts by higher semver\n\
         \tdriver = spyc --merge-driver %O %A %B\n\
         [merge \"ours\"]\n\
         \tname = keep our version (cargo regenerates the lock)\n\
         \tdriver = true\n"
    );
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&section);
    fs::write(&config_path, content).with_context(|| format!("write {}", config_path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_version_lines() {
        assert_eq!(parse_version_line("version = \"1.77.0\""), Some((1, 77, 0)));
        assert_eq!(
            parse_version_line("  version = \"0.2.13\""),
            Some((0, 2, 13))
        );
        assert_eq!(parse_version_line("version=\"10.0.5\""), Some((10, 0, 5)));
    }

    #[test]
    fn rejects_non_version_or_malformed_lines() {
        assert_eq!(parse_version_line("name = \"spyc\""), None);
        assert_eq!(parse_version_line("version = \"1.2\""), None); // too few
        assert_eq!(parse_version_line("version = \"1.2.3.4\""), None); // too many
        assert_eq!(parse_version_line("edition = \"2024\""), None);
        assert_eq!(parse_version_line("version = \"1.2.0-rc1\""), None);
    }

    /// A clean three-way merge (changes on non-adjacent lines) passes straight
    /// through.
    #[test]
    fn clean_merge_passes_through() {
        let base = "a\nb\nc\nd\ne\n";
        let ours = "a\nB\nc\nd\ne\n"; // changed line 2
        let theirs = "a\nb\nc\nd\nE\n"; // changed line 5
        let (merged, clean) = merge_versioned(base, ours, theirs);
        assert!(clean);
        assert_eq!(merged, "a\nB\nc\nd\nE\n");
    }

    /// diffy is more conservative than git: two sides editing *adjacent* but
    /// distinct lines collide. The driver leaves that as a real conflict (it's
    /// not version-only) rather than guessing — documented, and why Cargo.lock
    /// uses `merge=ours` instead of this driver.
    #[test]
    fn adjacent_distinct_edits_are_left_as_conflict() {
        let base = "a\nb\nc\n";
        let ours = "a\nB\nc\n";
        let theirs = "a\nb\nC\n";
        let (_merged, clean) = merge_versioned(base, ours, theirs);
        assert!(!clean);
    }

    /// The headline case: both sides bumped the version differently. The higher
    /// semver wins and the file comes out conflict-free.
    #[test]
    fn version_only_conflict_resolves_to_higher_semver() {
        let base = "[package]\nname = \"spyc\"\nversion = \"1.74.0\"\nedition = \"2024\"\n";
        let ours = "[package]\nname = \"spyc\"\nversion = \"1.74.11\"\nedition = \"2024\"\n";
        let theirs = "[package]\nname = \"spyc\"\nversion = \"1.77.0\"\nedition = \"2024\"\n";
        let (merged, clean) = merge_versioned(base, ours, theirs);
        assert!(clean, "version-only conflict should auto-resolve");
        assert!(merged.contains("version = \"1.77.0\""));
        assert!(!merged.contains("1.74.11"));
        assert!(!merged.contains("<<<<<<<"));
    }

    /// Lower-on-theirs is symmetric — ours wins when it's higher.
    #[test]
    fn version_resolution_picks_higher_regardless_of_side() {
        let base = "version = \"1.0.0\"\n";
        let ours = "version = \"2.5.0\"\n";
        let theirs = "version = \"2.1.0\"\n";
        let (merged, clean) = merge_versioned(base, ours, theirs);
        assert!(clean);
        assert_eq!(merged, "version = \"2.5.0\"\n");
    }

    /// A real content conflict (alongside the version bump) is left intact, so
    /// it reaches the human — the driver must never paper over real edits.
    #[test]
    fn real_content_conflict_is_left_unresolved() {
        let base = "version = \"1.0.0\"\nfoo = 1\n";
        let ours = "version = \"1.1.0\"\nfoo = 2\n";
        let theirs = "version = \"1.2.0\"\nfoo = 3\n";
        let (merged, clean) = merge_versioned(base, ours, theirs);
        assert!(!clean, "a real conflict must not be auto-resolved");
        assert!(merged.contains("<<<<<<<"));
    }

    /// Cargo.lock shape: main added a dependency entry AND bumped spyc. The new
    /// entry merges cleanly; only the version line conflicts → resolved.
    #[test]
    fn cargo_lock_shape_merges_deps_and_resolves_version() {
        let base = "[[package]]\nname = \"spyc\"\nversion = \"1.74.0\"\n";
        let ours = "[[package]]\nname = \"newdep\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"spyc\"\nversion = \"1.76.0\"\n";
        let theirs = "[[package]]\nname = \"spyc\"\nversion = \"1.77.0\"\n";
        let (merged, clean) = merge_versioned(base, ours, theirs);
        assert!(clean);
        assert!(merged.contains("name = \"newdep\""), "added dep kept");
        assert!(
            merged.contains("version = \"1.77.0\""),
            "higher spyc version kept"
        );
        assert!(!merged.contains("<<<<<<<"));
    }

    /// `ensure_installed` discovers the repo, writes both driver sections, and
    /// is idempotent. Uses a real `git init` fixture (test code may spawn git;
    /// the no-subprocess guard scans only the production portion of the file).
    #[test]
    fn ensure_installed_writes_once_and_is_idempotent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(tmp.path())
            .status()
            .expect("git init")
            .success();
        assert!(ok, "git init failed");

        assert!(
            ensure_installed(tmp.path()).expect("install"),
            "first call writes"
        );
        let cfg = fs::read_to_string(tmp.path().join(".git").join("config")).expect("read config");
        assert!(cfg.contains("[merge \"spyc-semver\"]"));
        assert!(cfg.contains("driver = spyc --merge-driver %O %A %B"));
        assert!(cfg.contains("[merge \"ours\"]"));

        assert!(
            !ensure_installed(tmp.path()).expect("install again"),
            "second call must be a no-op"
        );
    }
}
