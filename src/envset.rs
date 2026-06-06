//! Runtime environment overrides set via `:s` (setenv).
//!
//! These used to be applied with `unsafe { std::env::set_var }`, but
//! mutating the process environment is unsound once spyc has worker
//! threads (the per-pane vt100 parsers, the git-status worker — which
//! runs gix in-process): another thread calling `getenv`
//! concurrently with `setenv` is undefined behavior.
//!
//! Instead we keep the overrides in a thread-safe map and layer them
//! over the real environment ourselves:
//! - [`var`] returns an override if present, else the real process env
//!   — spyc's own reads of user-facing vars (`$EDITOR`, `$PAGER`,
//!   `$SHELL`, `$SPYC_PANE_CMD`, `$VAR` path expansion) go through it.
//! - [`overrides`] snapshots the map so [`crate::pane::pty_host`] can
//!   merge it into every spawned child's environment — panes and `!`
//!   captures see the vars just as they did when we mutated `environ`.

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

static OVERRIDES: LazyLock<RwLock<HashMap<String, String>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Record a runtime override. Replaces any previous value for `name`.
pub fn set(name: &str, value: &str) {
    OVERRIDES
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(name.to_string(), value.to_string());
}

/// Resolve `name`: a `:s` override if one is set, otherwise the real
/// process environment. Use this in place of `std::env::var(name).ok()`
/// for any variable a user might reasonably set at runtime.
pub fn var(name: &str) -> Option<String> {
    if let Some(v) = OVERRIDES
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(name)
    {
        return Some(v.clone());
    }
    std::env::var(name).ok()
}

/// Snapshot of every override, for merging into a child process's
/// environment at spawn time.
pub fn overrides() -> Vec<(String, String)> {
    OVERRIDES
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_then_var_returns_override() {
        set("SPYC_ENVSET_TEST_KEY", "hello");
        assert_eq!(var("SPYC_ENVSET_TEST_KEY").as_deref(), Some("hello"));
        assert!(
            overrides()
                .iter()
                .any(|(k, v)| k == "SPYC_ENVSET_TEST_KEY" && v == "hello")
        );
    }

    #[test]
    fn var_falls_back_to_process_env() {
        // A var we never override resolves from the real environment.
        // HOME is set in any sane test environment.
        assert_eq!(var("HOME"), std::env::var("HOME").ok());
        // An unset, never-overridden var is None.
        assert!(var("SPYC_ENVSET_DEFINITELY_UNSET_XYZ").is_none());
    }
}
