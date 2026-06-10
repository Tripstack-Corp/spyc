//! Startup health check for the `~/.local/state/spyc/` persistence layer.
//!
//! Validates inventory, marks, sessions, and graveyard on launch.
//! Cleans up orphaned files automatically and returns warnings for
//! anything the user should know about.

use std::path::{Path, PathBuf};

/// Result of a startup health scan.
pub struct HealthReport {
    /// Human-readable warnings to flash to the user.
    pub warnings: Vec<String>,
    /// Number of orphaned files cleaned up.
    pub cleaned: usize,
}

/// Run all health checks. Safe to call before loading any state —
/// it only reads and removes orphaned files, never touches valid data.
pub fn check(state_dir: &Path) -> HealthReport {
    let mut warnings = Vec::new();
    let mut cleaned = 0usize;

    cleaned += check_paired_dir(
        &state_dir.join("inventory"),
        &mut warnings,
        "inventory",
        &["dat"],
    );
    // The graveyard payload is `<uuid>.tar.zst` (current) or `<uuid>.dat`
    // (pre-v1.41.0 legacy, migrated by the cascade). Both must be recognized
    // as the metadata json's partner — otherwise every valid entry's json is
    // mistaken for an orphan and deleted, permanently breaking undo.
    cleaned += check_paired_dir(
        &state_dir.join("graveyard"),
        &mut warnings,
        "graveyard",
        &["tar.zst", "dat"],
    );
    check_marks(&state_dir.join("marks.toml"), &mut warnings);
    check_sessions(&state_dir.join("sessions"), &mut warnings);
    check_graveyard_size(&state_dir.join("graveyard"), &mut warnings);
    check_frecency(&state_dir.join("frecency.json"), &mut warnings);

    HealthReport { warnings, cleaned }
}

/// Check a paired-file directory (inventory or graveyard). Every
/// `<stem>.json` should have a matching payload (`<stem>.<suffix>` for one
/// of `payload_suffixes`) and vice versa. Orphans on either side are
/// removed and counted.
///
/// `payload_suffixes` is matched as a full filename suffix, not via
/// `Path::extension` — the graveyard's `<uuid>.tar.zst` has extension
/// `zst` and stem `<uuid>.tar`, so naive extension/stem pairing would
/// never match it against `<uuid>.json` and would delete every live
/// entry as an "orphan".
fn check_paired_dir(
    dir: &Path,
    warnings: &mut Vec<String>,
    label: &str,
    payload_suffixes: &[&str],
) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0; // directory missing is fine — first run
    };

    let mut jsons = std::collections::HashSet::new();
    let mut payload_stems = std::collections::HashSet::new();
    let mut payload_files: Vec<(String, PathBuf)> = Vec::new();
    let mut corrupt_json = 0usize;

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Some(stem) = name.strip_suffix(".json") {
            // Validate JSON is parseable.
            if let Ok(text) = std::fs::read_to_string(&path) {
                if serde_json::from_str::<serde_json::Value>(&text).is_err() {
                    corrupt_json += 1;
                    let _ = std::fs::remove_file(&path);
                } else {
                    jsons.insert(stem.to_string());
                }
            }
        } else if let Some(stem) = payload_suffixes
            .iter()
            .find_map(|suf| name.strip_suffix(&format!(".{suf}")))
        {
            payload_stems.insert(stem.to_string());
            payload_files.push((stem.to_string(), path));
        }
        // else: ignore unexpected files
    }

    let mut removed = corrupt_json;

    // Orphaned .json (no matching payload) — already skipped by load,
    // clean them up.
    for stem in jsons.difference(&payload_stems) {
        let path = dir.join(format!("{stem}.json"));
        let _ = std::fs::remove_file(&path);
        removed += 1;
    }

    // Orphaned payloads (no matching .json) — interrupted writes, or
    // entries whose metadata was already lost. Reclaim the disk.
    for (stem, path) in &payload_files {
        if !jsons.contains(stem) {
            let _ = std::fs::remove_file(path);
            removed += 1;
        }
    }

    if corrupt_json > 0 {
        warnings.push(format!(
            "{label}: removed {corrupt_json} corrupt metadata file(s)"
        ));
    }
    if removed > corrupt_json {
        let orphans = removed - corrupt_json;
        warnings.push(format!("{label}: cleaned up {orphans} orphaned file(s)"));
    }

    removed
}

/// Check marks.toml — warn if it exists but can't be parsed.
fn check_marks(path: &Path, warnings: &mut Vec<String>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return; // missing is fine
    };
    if text.is_empty() {
        return;
    }
    if toml::from_str::<toml::Value>(&text).is_err() {
        warnings.push("marks.toml is corrupt — marks will be empty this session".into());
    }
}

/// Check session files — warn about corrupt ones.
fn check_sessions(dir: &Path, warnings: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return; // missing is fine
    };

    let mut corrupt = 0usize;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if serde_json::from_str::<serde_json::Value>(&text).is_err() {
            let _ = std::fs::remove_file(&path);
            corrupt += 1;
        }
    }

    if corrupt > 0 {
        warnings.push(format!(
            "sessions: removed {corrupt} corrupt session file(s)"
        ));
    }
}

/// Warn if the graveyard is consuming significant disk space.
fn check_graveyard_size(dir: &Path, warnings: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let total_bytes: u64 = entries
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();

    // Warn above 100 MB.
    if total_bytes > 100 * 1024 * 1024 {
        let mb = total_bytes / (1024 * 1024);
        warnings.push(format!(
            "graveyard is using {mb} MB — run `z` then confirm to clear"
        ));
    }
}

/// Validate the frecency database (JSON).
fn check_frecency(path: &Path, warnings: &mut Vec<String>) {
    if !path.exists() {
        return;
    }
    if let Ok(text) = std::fs::read_to_string(path)
        && serde_json::from_str::<crate::state::frecency::Frecency>(&text).is_err()
    {
        warnings.push(format!("frecency: corrupt {}", path.display()));
    }
}

/// Resolve the state directory path, consistent with other state modules.
pub fn state_dir() -> Option<PathBuf> {
    crate::state::state_root()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_dir_is_healthy() {
        let tmp = tempfile::tempdir().unwrap();
        let report = check(tmp.path());
        assert!(report.warnings.is_empty());
        assert_eq!(report.cleaned, 0);
    }

    #[test]
    fn orphaned_dat_cleaned_up() {
        let tmp = tempfile::tempdir().unwrap();
        let inv = tmp.path().join("inventory");
        std::fs::create_dir_all(&inv).unwrap();
        // .dat without .json
        std::fs::write(inv.join("abc123.dat"), b"orphan").unwrap();
        let report = check(tmp.path());
        assert_eq!(report.cleaned, 1);
        assert!(!inv.join("abc123.dat").exists());
    }

    #[test]
    fn orphaned_json_cleaned_up() {
        let tmp = tempfile::tempdir().unwrap();
        let inv = tmp.path().join("inventory");
        std::fs::create_dir_all(&inv).unwrap();
        // .json without .dat
        std::fs::write(inv.join("abc123.json"), r#"{"id":"abc123"}"#).unwrap();
        let report = check(tmp.path());
        assert_eq!(report.cleaned, 1);
        assert!(!inv.join("abc123.json").exists());
    }

    #[test]
    fn corrupt_json_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let inv = tmp.path().join("inventory");
        std::fs::create_dir_all(&inv).unwrap();
        std::fs::write(inv.join("bad.json"), "not json {{{").unwrap();
        std::fs::write(inv.join("bad.dat"), b"data").unwrap();
        let report = check(tmp.path());
        assert!(report.cleaned > 0);
        assert!(report.warnings.iter().any(|w| w.contains("corrupt")));
    }

    #[test]
    fn valid_pair_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let inv = tmp.path().join("inventory");
        std::fs::create_dir_all(&inv).unwrap();
        std::fs::write(inv.join("good.json"), r#"{"id":"good"}"#).unwrap();
        std::fs::write(inv.join("good.dat"), b"content").unwrap();
        let report = check(tmp.path());
        assert_eq!(report.cleaned, 0);
        assert!(inv.join("good.json").exists());
        assert!(inv.join("good.dat").exists());
    }

    #[test]
    fn graveyard_tar_zst_pair_is_untouched() {
        // The post-v1.41.0 schema: <uuid>.json + <uuid>.tar.zst. The old
        // check paired json↔dat only, so it deleted every live json here.
        let tmp = tempfile::tempdir().unwrap();
        let grave = tmp.path().join("graveyard");
        std::fs::create_dir_all(&grave).unwrap();
        std::fs::write(grave.join("uuid1.json"), r#"{"id":"uuid1"}"#).unwrap();
        std::fs::write(grave.join("uuid1.tar.zst"), b"\x28\xb5\x2f\xfd").unwrap();
        let report = check(tmp.path());
        assert_eq!(report.cleaned, 0, "valid graveyard pair must survive");
        assert!(grave.join("uuid1.json").exists());
        assert!(grave.join("uuid1.tar.zst").exists());
    }

    #[test]
    fn graveyard_legacy_dat_pair_is_untouched() {
        // Pre-v1.41.0 entries (<uuid>.json + <uuid>.dat) are still valid —
        // the cascade migrates them; the health check must not delete them.
        let tmp = tempfile::tempdir().unwrap();
        let grave = tmp.path().join("graveyard");
        std::fs::create_dir_all(&grave).unwrap();
        std::fs::write(grave.join("old1.json"), r#"{"id":"old1"}"#).unwrap();
        std::fs::write(grave.join("old1.dat"), b"legacy").unwrap();
        let report = check(tmp.path());
        assert_eq!(report.cleaned, 0);
        assert!(grave.join("old1.json").exists());
        assert!(grave.join("old1.dat").exists());
    }

    #[test]
    fn graveyard_orphaned_tar_zst_removed() {
        // A tarball with no metadata json (interrupted write, or stranded by
        // the old bug) is unreachable from the viewer — reclaim the disk.
        let tmp = tempfile::tempdir().unwrap();
        let grave = tmp.path().join("graveyard");
        std::fs::create_dir_all(&grave).unwrap();
        std::fs::write(grave.join("lonely.tar.zst"), b"\x28\xb5\x2f\xfd").unwrap();
        let report = check(tmp.path());
        assert_eq!(report.cleaned, 1);
        assert!(!grave.join("lonely.tar.zst").exists());
    }

    #[test]
    fn graveyard_orphaned_json_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let grave = tmp.path().join("graveyard");
        std::fs::create_dir_all(&grave).unwrap();
        std::fs::write(grave.join("meta.json"), r#"{"id":"meta"}"#).unwrap();
        let report = check(tmp.path());
        assert_eq!(report.cleaned, 1);
        assert!(!grave.join("meta.json").exists());
    }

    #[test]
    fn corrupt_marks_warns() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("marks.toml"), "not = [valid toml").unwrap();
        let report = check(tmp.path());
        assert!(report.warnings.iter().any(|w| w.contains("marks.toml")));
    }

    #[test]
    fn corrupt_session_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let sess = tmp.path().join("sessions");
        std::fs::create_dir_all(&sess).unwrap();
        std::fs::write(sess.join("123.json"), "broken{{{").unwrap();
        let report = check(tmp.path());
        assert!(report.warnings.iter().any(|w| w.contains("session")));
        assert!(!sess.join("123.json").exists());
    }

    #[test]
    fn corrupt_frecency_warns() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("frecency.json"), "not valid json{{{").unwrap();
        let report = check(tmp.path());
        assert!(report.warnings.iter().any(|w| w.contains("frecency")));
    }

    #[test]
    fn valid_frecency_no_warning() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("frecency.json"), r#"{"entries":{}}"#).unwrap();
        let report = check(tmp.path());
        assert!(!report.warnings.iter().any(|w| w.contains("frecency")));
    }
}
