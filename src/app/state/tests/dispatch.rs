#![allow(clippy::wildcard_imports)]
use super::*;

#[test]
fn cmd_empty_is_handled() {
    let mut s = test_state();
    assert!(matches!(s.dispatch_command(""), CommandResult::Handled));
    assert!(matches!(s.dispatch_command("   "), CommandResult::Handled));
}

#[test]
fn pattern_pick_bad_glob_flashes_error() {
    let mut s = state_with_rows(&["foo.rs", "bar.txt"]);
    // `[` is an unterminated character class — an invalid glob.
    let r = s.dispatch_prompt(&PromptKind::PatternPick, "[");
    assert!(matches!(r, PromptResult::Handled));
    assert!(
        s.flash.as_ref().is_some_and(|f| f.text.contains("pattern")),
        "expected a bad-pattern flash, got {:?}",
        s.flash
    );
}

#[test]
fn pattern_pick_good_glob_selects_and_no_error() {
    let mut s = state_with_rows(&["foo.rs", "bar.txt", "baz.rs"]);
    let r = s.dispatch_prompt(&PromptKind::PatternPick, "*.rs");
    assert!(matches!(r, PromptResult::Handled));
    // A valid pattern doesn't flash an error.
    assert!(s.flash.is_none(), "unexpected flash: {:?}", s.flash);
}

#[test]
fn cmd_quit_defers_to_app() {
    // :q / :quit are App-layer commands now — they need save_session
    // and the double-tap confirm, neither of which the pure-domain
    // layer can see. Pure-domain must return the typed Quit variant
    // (forcing the App-side match to handle it) and must NOT flip
    // should_quit on its own.
    let mut s = test_state();
    assert!(matches!(s.dispatch_command("q"), CommandResult::Quit));
    assert!(!s.should_quit);
}

#[test]
fn cmd_quit_long_defers_to_app() {
    let mut s = test_state();
    assert!(matches!(s.dispatch_command("quit"), CommandResult::Quit));
    assert!(!s.should_quit);
}

#[test]
fn cmd_limit_set_and_clear() {
    let mut s = state_with_rows(&["foo.rs", "bar.txt", "baz.rs"]);
    s.dispatch_command("limit *.rs");
    assert_eq!(s.left.temp_filter.as_deref(), Some("*.rs"));
    assert!(s.flash.as_ref().unwrap().text.contains("limit:"));

    s.dispatch_command("limit");
    assert!(s.left.temp_filter.is_none());
    assert!(s.flash.as_ref().unwrap().text.contains("cleared"));
}

#[test]
fn cmd_limit_picks_only() {
    let mut s = test_state();
    s.dispatch_command("limit !");
    assert_eq!(s.left.temp_filter.as_deref(), Some("!"));
}

#[test]
fn cmd_sort_query() {
    let mut s = test_state();
    s.dispatch_command("sort");
    assert!(s.flash.as_ref().unwrap().text.contains("name"));
}

#[test]
fn cmd_sort_set() {
    let mut s = test_state();
    s.dispatch_command("sort size");
    assert_eq!(s.left.sort_order, SortMode::Size);
    assert!(s.flash.as_ref().unwrap().text.contains("size"));
}

#[test]
fn cmd_sort_invalid() {
    let mut s = test_state();
    s.dispatch_command("sort bogus");
    assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
}

#[test]
fn cmd_marks_empty() {
    let mut s = test_state();
    let result = s.dispatch_command("marks");
    assert!(matches!(result, CommandResult::Handled));
    assert!(s.flash.as_ref().unwrap().text.contains("no marks"));
}

#[test]
fn cmd_marks_with_entries() {
    let mut s = test_state();
    s.marks.set(
        'a',
        Mark {
            dir: PathBuf::from("/tmp"),
            focus: None,
        },
    );
    let result = s.dispatch_command("marks");
    match result {
        CommandResult::OpenPager { title, lines } => {
            assert_eq!(title, "marks");
            assert_eq!(lines.len(), 1);
            assert!(lines[0].contains("/tmp"));
        }
        _ => panic!("expected OpenPager"),
    }
}

#[test]
fn cmd_set_sort() {
    let mut s = test_state();
    s.dispatch_command("set sort=mtime");
    assert_eq!(s.left.sort_order, SortMode::Mtime);
}

#[test]
fn cmd_set_unknown_key() {
    let mut s = test_state();
    s.dispatch_command("set foo=bar");
    assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
}

#[test]
fn cmd_shell_not_handled() {
    let mut s = test_state();
    assert!(matches!(
        s.dispatch_command("!ls"),
        CommandResult::NotHandled
    ));
    assert!(matches!(
        s.dispatch_command(";htop"),
        CommandResult::NotHandled
    ));
    assert!(matches!(
        s.dispatch_command("bprev"),
        CommandResult::NotHandled
    ));
    assert!(matches!(
        s.dispatch_command("bnext"),
        CommandResult::NotHandled
    ));
}

#[test]
fn cmd_unknown() {
    let mut s = test_state();
    s.dispatch_command("foobar");
    let flash = s.flash.as_ref().unwrap();
    assert!(matches!(flash.kind, FlashKind::Error));
    assert!(flash.text.contains("foobar"));
}

// ── dispatch_prompt ───────────────────────────────────────────

#[test]
fn prompt_search_saves_last_search() {
    let mut s = test_state();
    let result = s.dispatch_prompt(&PromptKind::Search { saved_cursor: 0 }, "foo");
    assert!(matches!(result, PromptResult::Handled));
    assert_eq!(s.last_search.as_deref(), Some("foo"));
}

#[test]
fn prompt_search_empty_does_not_save() {
    let mut s = test_state();
    s.last_search = Some("old".to_string());
    s.dispatch_prompt(&PromptKind::Search { saved_cursor: 0 }, "");
    assert_eq!(s.last_search.as_deref(), Some("old"));
}

#[test]
fn prompt_limit_sets_filter() {
    let mut s = test_state();
    s.dispatch_prompt(&PromptKind::Limit, "*.rs");
    assert_eq!(s.left.temp_filter.as_deref(), Some("*.rs"));
}

#[test]
fn prompt_limit_empty_clears() {
    let mut s = test_state();
    s.left.temp_filter = Some("old".to_string());
    s.dispatch_prompt(&PromptKind::Limit, "");
    assert!(s.left.temp_filter.is_none());
}

#[test]
fn prompt_set_env() {
    let mut s = test_state();
    s.dispatch_prompt(&PromptKind::SetEnv, "TEST_SPYC_VAR=hello");
    assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Info));
    // Verify the override was recorded (in the envset store, not the
    // process env — `:s` no longer mutates `environ`).
    assert_eq!(
        crate::envset::var("TEST_SPYC_VAR").as_deref(),
        Some("hello")
    );
}

#[test]
fn prompt_set_env_bad_format() {
    let mut s = test_state();
    s.dispatch_prompt(&PromptKind::SetEnv, "no_equals_sign");
    assert!(matches!(s.flash.as_ref().unwrap().kind, FlashKind::Error));
}

#[test]
fn prompt_pattern_pick() {
    let mut s = test_state();
    // Add some listing entries for the pattern to match against
    s.left.listing = Listing::empty(PathBuf::from("/tmp/test"));
    use crate::fs::entry::{Entry, EntryKind};
    s.left.listing.entries = vec![
        Entry {
            path: PathBuf::from("/tmp/test/foo.rs"),
            name: "foo.rs".to_string(),
            kind: EntryKind::File,
            size: 0,
            mtime: std::time::SystemTime::UNIX_EPOCH,
        },
        Entry {
            path: PathBuf::from("/tmp/test/bar.txt"),
            name: "bar.txt".to_string(),
            kind: EntryKind::File,
            size: 0,
            mtime: std::time::SystemTime::UNIX_EPOCH,
        },
    ];
    s.dispatch_prompt(&PromptKind::PatternPick, "*.rs");
    assert!(s.left.picks.contains(Path::new("/tmp/test/foo.rs")));
    assert!(!s.left.picks.contains(Path::new("/tmp/test/bar.txt")));
}

#[test]
fn prompt_pane_new_tab_cmd_stashes() {
    let mut s = test_state();
    s.dispatch_prompt(&PromptKind::PaneNewTabCmd, "bash");
    assert_eq!(s.pending_new_tab_cmd.as_deref(), Some("bash"));
    assert!(matches!(s.mode, Mode::Prompting(_)));
}

#[test]
fn prompt_pane_new_tab_cmd_empty_is_noop() {
    let mut s = test_state();
    s.dispatch_prompt(&PromptKind::PaneNewTabCmd, "");
    assert!(s.pending_new_tab_cmd.is_none());
}

#[test]
fn prompt_shell_cmd_not_handled() {
    let mut s = test_state();
    assert!(matches!(
        s.dispatch_prompt(&PromptKind::ShellCmd, "ls"),
        PromptResult::NotHandled
    ));
    assert!(matches!(
        s.dispatch_prompt(&PromptKind::ShellCmdCaptured, "ls"),
        PromptResult::NotHandled
    ));
    assert!(matches!(
        s.dispatch_prompt(&PromptKind::CopyTo, "/tmp"),
        PromptResult::NotHandled
    ));
}

#[test]
fn prompt_remove_confirm_handled() {
    let mut s = test_state();
    assert!(matches!(
        s.dispatch_prompt(&PromptKind::RemoveConfirm, "n"),
        PromptResult::Handled
    ));
}

// ── apply() action dispatch ───────────────────────────────────

#[test]
fn command_table_dispatches_without_unknown() {
    use crate::app::command_table::{COMMAND_TABLE, CmdLayer};
    // The registry is the single source of truth: every registered
    // command must dispatch (no silent "unknown command" flash), AND its
    // declared layer must match how `AppState::dispatch_command` routes
    // the bare name — `Pure` resolves here (not NotHandled), `App` passes
    // through (NotHandled). This is the guard that turns the old
    // four-list-desync footgun (e.g. forgetting the NotHandled allowlist,
    // which bit `:undo`) into a test failure instead of a runtime flash.
    for spec in COMMAND_TABLE {
        let mut s = state_with_rows(&[]);
        s.flash = None;
        let result = s.dispatch_command(spec.name);

        if let Some(ref f) = s.flash {
            assert!(
                !f.text.starts_with("unknown command:"),
                "COMMAND_TABLE registers `{}` but dispatch_command reports \
                     it as unknown — add its arm (or drop the entry)",
                spec.name,
            );
        }

        match spec.layer() {
            CmdLayer::App => assert!(
                matches!(result, CommandResult::NotHandled),
                "`{}` is registered as CmdLayer::App but AppState did not \
                     return NotHandled — fix the layer or the arm",
                spec.name,
            ),
            CmdLayer::Pure => assert!(
                !matches!(result, CommandResult::NotHandled),
                "`{}` is registered as CmdLayer::Pure but AppState returned \
                     NotHandled — fix the layer or add the pure arm",
                spec.name,
            ),
        }
    }
}

// ── :cd → Effect::ChangeDir emission (MVU Phase 5 / PR9) ───────
// `:cd` no longer chdirs inline; it emits a deferred `Effect::ChangeDir`
// via the new `CommandResult::Post` carrier (run by `run_effects`),
// matching the `apply()` Action arms from PR7. Pure (no chdir IO here).

#[test]
fn cd_command_emits_change_dir() {
    let mut s = state_with_rows(&[]);
    match s.dispatch_command("cd /tmp/somewhere") {
        CommandResult::Post(fx) => match fx.as_slice() {
            [
                Effect::ChangeDir {
                    path,
                    focus,
                    on_ok,
                    err_prefix,
                },
            ] => {
                // Path is whatever `expand` produces (compare against it
                // directly so the test is robust to expansion rules).
                assert_eq!(path, &crate::paths::expand("/tmp/somewhere"));
                assert!(focus.is_none());
                assert!(on_ok.is_none());
                assert_eq!(*err_prefix, "cd");
            }
            other => panic!("expected one ChangeDir, got {other:?}"),
        },
        other => panic!("expected Post, got {other:?}"),
    }
    // `:cd` with no arg → $HOME (or "/" when unset); still a ChangeDir.
    // (A trailing-space `:cd ` is trimmed to `cd` by dispatch_command, so
    // it takes this same no-arg path.)
    assert!(matches!(
        s.dispatch_command("cd"),
        CommandResult::Post(ref fx)
            if matches!(fx.as_slice(), [Effect::ChangeDir { err_prefix: "cd", .. }])
    ));
}
