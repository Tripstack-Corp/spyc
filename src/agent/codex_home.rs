//! One pane-scoped Codex home for rollout discovery and its consumers.

use std::path::{Path, PathBuf};

/// Resolve the home from the environment captured immediately before the agent execs.
/// Relative overrides are relative to its launch cwd. Empty or non-UTF-8
/// `CODEX_HOME` uses the default, matching Codex's home resolver.
/// Canonicalizing existing paths freezes symlink aliases before later lookups.
pub fn from_launch(env: &crate::pane::pty_host::LaunchEnvironment, cwd: &Path) -> Option<PathBuf> {
    let home = env
        .codex_home
        .as_deref()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env.home
                .as_ref()
                .map(|home| PathBuf::from(home).join(".codex"))
        })?;
    let home = if home.is_absolute() {
        home
    } else {
        cwd.join(home)
    };
    Some(std::fs::canonicalize(&home).unwrap_or(home))
}

/// The session directory belongs to the captured home, even when unavailable.
pub fn sessions(home: &Path) -> PathBuf {
    home.join("sessions")
}

/// Fork ancestry stays in the session tree containing the child rollout.
pub fn sessions_for_rollout(rollout: &Path) -> Option<&Path> {
    rollout.ancestors().nth(4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::pty_host::LaunchEnvironment;

    #[test]
    fn default_empty_and_custom_homes_use_the_launch_environment() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = std::fs::canonicalize(tmp.path()).unwrap();
        let mut env = LaunchEnvironment {
            home: Some(cwd.clone().into_os_string()),
            codex_home: None,
        };
        assert_eq!(from_launch(&env, &cwd), Some(cwd.join(".codex")));
        env.codex_home = Some("".into());
        assert_eq!(from_launch(&env, &cwd), Some(cwd.join(".codex")));
        env.codex_home = Some("custom".into());
        assert_eq!(from_launch(&env, &cwd), Some(cwd.join("custom")));
        env.home = None;
        env.codex_home = None;
        assert_eq!(from_launch(&env, &cwd), None);
    }

    #[test]
    fn an_unavailable_custom_home_never_uses_the_default() {
        let tmp = tempfile::tempdir().unwrap();
        let env = LaunchEnvironment {
            home: Some(tmp.path().into()),
            codex_home: Some(tmp.path().join("missing").into_os_string()),
        };
        assert_eq!(
            from_launch(&env, tmp.path()),
            Some(tmp.path().join("missing"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_homes_share_one_identity_and_stay_frozen() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("first");
        let second = tmp.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let alias = tmp.path().join("alias");
        std::os::unix::fs::symlink(&first, &alias).unwrap();
        let env = LaunchEnvironment {
            home: None,
            codex_home: Some(alias.clone().into_os_string()),
        };
        let frozen = from_launch(&env, tmp.path()).unwrap();
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&second, &alias).unwrap();
        assert_eq!(frozen, first.canonicalize().unwrap());
        assert_ne!(Some(frozen), from_launch(&env, tmp.path()));
    }
}
