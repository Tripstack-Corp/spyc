//! The environment `:setenv` adds: each project's own variables
//! ([`ProjectEnv`]), and the overlay spyc's own reads go through.
//!
//! Nothing here mutates the process environment: `std::env::set_var` is
//! unsound once worker threads read it. Every program a project starts gets
//! its [`ProjectEnv::pairs`] handed to the spawn, and [`var`] layers the
//! project in view's variables over the real environment for spyc's own
//! reads (`$EDITOR`, `$PAGER`, `$SHELL`, `$SPYC_PANE_CMD`, `$VAR` path
//! expansion). The loop keeps that overlay on the project in view
//! ([`follow`]).

use std::collections::{BTreeMap, BTreeSet};

/// The variables `:setenv` set in one project, and which of them `-p` saves
/// with it. A saved value is written to the session file as plain text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ProjectEnv {
    vars: BTreeMap<String, String>,
    saved: BTreeSet<String>,
}

impl ProjectEnv {
    /// The variables a restore gives back, all of them saved again.
    pub fn restored(saved: &BTreeMap<String, String>) -> Self {
        Self {
            vars: saved.clone(),
            saved: saved.keys().cloned().collect(),
        }
    }

    /// Set `name`, saved with the project when `save`; the last `:setenv`
    /// of a name decides both.
    pub fn set(&mut self, name: &str, value: &str, save: bool) {
        self.vars.insert(name.to_owned(), value.to_owned());
        if save {
            self.saved.insert(name.to_owned());
        } else {
            self.saved.remove(name);
        }
    }

    /// Take `name` out. Whether it was set.
    pub fn unset(&mut self, name: &str) -> bool {
        self.saved.remove(name);
        self.vars.remove(name).is_some()
    }

    /// Every variable, for a program's environment.
    pub fn pairs(&self) -> Vec<(String, String)> {
        self.vars
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// The variables `-p` saves with the project.
    pub fn saved(&self) -> BTreeMap<String, String> {
        self.vars
            .iter()
            .filter(|(k, _)| self.saved.contains(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn has_saved(&self) -> bool {
        !self.saved.is_empty()
    }
}

#[cfg(not(test))]
static OVERLAY: std::sync::LazyLock<std::sync::RwLock<BTreeMap<String, String>>> =
    std::sync::LazyLock::new(Default::default);

// Each test thread has its own overlay, so tests that settle it don't race.
#[cfg(test)]
thread_local! {
    static OVERLAY: std::cell::RefCell<BTreeMap<String, String>> = std::cell::RefCell::default();
}

#[cfg(not(test))]
fn read<R>(f: impl FnOnce(&BTreeMap<String, String>) -> R) -> R {
    f(&OVERLAY
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner))
}

#[cfg(test)]
fn read<R>(f: impl FnOnce(&BTreeMap<String, String>) -> R) -> R {
    OVERLAY.with(|o| f(&o.borrow()))
}

#[cfg(not(test))]
fn write(f: impl FnOnce(&mut BTreeMap<String, String>)) {
    f(&mut OVERLAY
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner));
}

#[cfg(test)]
fn write(f: impl FnOnce(&mut BTreeMap<String, String>)) {
    OVERLAY.with(|o| f(&mut o.borrow_mut()));
}

/// Make the overlay `env`'s variables, when it isn't already.
pub fn follow(env: &ProjectEnv) {
    if read(|o| *o != env.vars) {
        write(|o| o.clone_from(&env.vars));
    }
}

/// Resolve `name`: the project in view's `:setenv` value if it set one,
/// otherwise the real process environment. Use this in place of
/// `std::env::var(name).ok()` for any variable a user might set at runtime.
pub fn var(name: &str) -> Option<String> {
    read(|o| o.get(name).cloned()).or_else(|| std::env::var(name).ok())
}

/// Set one overlay value directly, as a project's `:setenv` would.
#[cfg(test)]
pub fn set(name: &str, value: &str) {
    write(|o| {
        o.insert(name.to_owned(), value.to_owned());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overlay_follows_a_project_and_falls_back_to_the_process_env() {
        let mut env = ProjectEnv::default();
        env.set("SPYC_ENVSET_TEST_KEY", "hello", false);
        follow(&env);
        assert_eq!(var("SPYC_ENVSET_TEST_KEY").as_deref(), Some("hello"));
        follow(&ProjectEnv::default());
        assert!(var("SPYC_ENVSET_TEST_KEY").is_none());
        assert_eq!(var("HOME"), std::env::var("HOME").ok());
    }

    #[test]
    fn only_saved_variables_are_saved() {
        let mut env = ProjectEnv::default();
        env.set("A", "1", true);
        env.set("B", "2", false);
        env.set("C", "3", true);
        env.set("C", "4", false);
        assert_eq!(env.saved(), BTreeMap::from([("A".into(), "1".into())]));
        assert_eq!(env.pairs().len(), 3);
        assert!(env.unset("A"));
        assert!(!env.has_saved());
        assert!(!env.unset("A"));
    }
}
