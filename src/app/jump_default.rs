//! `J`'s default: the newest path the active pane printed, offered dimmed in
//! the empty `jump to:` prompt so a bare Enter goes there. The pane text is
//! read on the loop (`Effect::ReadPaneText`), but picking the path `stat`s
//! every candidate, and one on a dead mount — or under macOS's `/net`
//! automounter — can block for seconds. So that half runs on a worker, and
//! the prompt opens without waiting for it.

use std::path::PathBuf;
use std::sync::{Arc, PoisonError};

use super::{App, Message, Mode, PromptKind, Wake};

/// What one `J` default request found.
pub(super) struct JumpDefaultAnswer {
    /// Which request this answers (`Runtime::jump_default_seq`).
    seq: u64,
    /// The path to offer, `~`-abbreviated; `None` when the pane named none
    /// that exists.
    path: Option<String>,
}

impl App {
    /// Resolve the default off-thread from `lines`, already cut to what the
    /// agent printed. Each request supersedes any still in flight, so a slow
    /// one can't land in a later prompt.
    pub(super) fn spawn_jump_default(&mut self, lines: Vec<String>, pane_cwd: PathBuf) {
        self.runtime.jump_default_seq += 1;
        let seq = self.runtime.jump_default_seq;
        let col = self.state.cur();
        let column_dir = col.listing.dir.clone();
        let column_root = col.git_cache.current_repo_root.clone();
        let project_home = self.state.project_home.clone();
        let results = Arc::clone(&self.runtime.jump_default_results);
        let wake = self.runtime.pane_wake_tx.clone();
        std::thread::spawn(move || {
            let bases = super::navigate::path_ref_bases(
                &pane_cwd,
                &column_dir,
                column_root.as_deref(),
                project_home.as_deref(),
            );
            let path = super::navigate::find_path_ref(&lines, &bases)
                .map(|r| crate::paths::display_tilde(&r.path));
            results
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(JumpDefaultAnswer { seq, path });
            if let Some(tx) = wake {
                let _ = tx.send(Message::Wake(Wake::JumpDefault));
            }
        });
    }

    /// Hand the newest request's answer to the `J` prompt, if one is still
    /// open. Drained every pre-recv scan, so the slot never holds a stale
    /// answer for a later prompt. Returns whether the frame changed.
    pub(crate) fn apply_jump_defaults(&mut self) -> bool {
        let landed = std::mem::take(
            &mut *self
                .runtime
                .jump_default_results
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        let latest = self.runtime.jump_default_seq;
        let Some(found) = landed
            .into_iter()
            .find_map(|a| (a.seq == latest).then_some(a.path).flatten())
        else {
            return false;
        };
        match &mut self.state.mode {
            Mode::Prompting(p) if matches!(p.kind, PromptKind::Jump) => {
                p.suggestion = Some(found);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Prompt;

    fn app_in_jump_prompt() -> App {
        let mut app = App::test_app(std::env::temp_dir());
        app.state.mode = Mode::Prompting(Prompt::shell(PromptKind::Jump, "jump to: "));
        app
    }

    fn land(app: &App, seq: u64, found: Option<&str>) {
        app.runtime
            .jump_default_results
            .lock()
            .unwrap()
            .push(JumpDefaultAnswer {
                seq,
                path: found.map(String::from),
            });
    }

    fn suggestion(app: &App) -> Option<&str> {
        match &app.state.mode {
            Mode::Prompting(p) => p.suggestion.as_deref(),
            Mode::Normal => None,
        }
    }

    #[test]
    fn the_newest_answer_reaches_the_open_prompt() {
        let mut app = app_in_jump_prompt();
        app.runtime.jump_default_seq = 2;
        land(&app, 2, Some("~/src/spyc/Cargo.toml"));
        land(&app, 1, Some("/tmp/old"));
        assert!(app.apply_jump_defaults());
        assert_eq!(suggestion(&app), Some("~/src/spyc/Cargo.toml"));
    }

    /// `J`, `Esc`, `J`: the first request's answer belongs to a prompt that's
    /// gone, even when it lands after the second.
    #[test]
    fn a_superseded_answer_is_dropped() {
        let mut app = app_in_jump_prompt();
        app.runtime.jump_default_seq = 2;
        land(&app, 1, Some("/tmp/old"));
        assert!(!app.apply_jump_defaults());
        assert_eq!(suggestion(&app), None);
        assert!(app.runtime.jump_default_results.lock().unwrap().is_empty());
    }

    #[test]
    fn an_answer_after_the_prompt_closed_is_dropped() {
        let mut app = App::test_app(std::env::temp_dir());
        app.runtime.jump_default_seq = 1;
        land(&app, 1, Some("/tmp/x"));
        assert!(!app.apply_jump_defaults());
        assert!(matches!(app.state.mode, Mode::Normal));
    }

    #[test]
    fn no_path_found_leaves_the_prompt_bare() {
        let mut app = app_in_jump_prompt();
        app.runtime.jump_default_seq = 1;
        land(&app, 1, None);
        assert!(!app.apply_jump_defaults());
        assert_eq!(suggestion(&app), None);
    }
}
