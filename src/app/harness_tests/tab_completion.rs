use super::*;

use crossterm::event::KeyCode;

// ── Tab completion in the prompts (#174) ──
// Each test presses Tab through `handle_key`, so it covers the router and both
// prompt editors (the simple one and the vi one) as well as the completer.
// Relative paths are written into the column's directory, never the process
// cwd: unit tests don't chdir.

/// An `App` browsing a fresh directory holding `entries` (a trailing `/` makes
/// a directory; a `/` inside a name makes its parents).
fn browsing(entries: &[&str], test: impl FnOnce(&mut App, &std::path::Path)) {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let dir = tmp.path().join("w");
        std::fs::create_dir(&dir).unwrap();
        for entry in entries {
            if let Some(sub) = entry.strip_suffix('/') {
                std::fs::create_dir_all(dir.join(sub)).unwrap();
            } else {
                let path = dir.join(entry);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, "").unwrap();
            }
        }
        let mut app = App::test_app(dir.clone());
        app.state.left.listing = crate::fs::Listing::read(&dir).unwrap();
        app.state.rebuild_rows();
        test(&mut app, &dir);
    });
}

fn open(app: &mut App, kind: PromptKind, vi: bool, typed: &str) {
    let mut prompt = if vi {
        Prompt::shell(kind, "> ")
    } else {
        Prompt::simple(kind, "> ")
    };
    prompt.buffer = typed.to_string();
    if let Some(editor) = prompt.editor.as_mut() {
        editor.set_content(typed);
    }
    app.view.tab_state = None;
    app.state.mode = Mode::Prompting(prompt);
}

/// The prompt's text after a Tab. A vi prompt's editor must agree with it, or
/// the next keystroke edits a line the user can't see.
fn press_tab(app: &mut App) -> String {
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()))
        .unwrap();
    let Mode::Prompting(prompt) = &app.state.mode else {
        panic!("Tab closed the prompt");
    };
    if let Some(editor) = &prompt.editor {
        assert_eq!(editor.text(), prompt.buffer, "editor and buffer disagree");
    }
    prompt.buffer.clone()
}

fn listed(app: &App) -> Vec<String> {
    app.state
        .left
        .rows
        .iter()
        .map(|r| r.display.clone())
        .collect()
}

#[test]
fn a_unique_prefix_completes_and_a_directory_gets_its_slash() {
    browsing(&["README.md", "src/", "Cargo.toml"], |app, _| {
        open(app, PromptKind::CopyTo, true, "REA");
        assert_eq!(press_tab(app), "README.md");
        open(app, PromptKind::CopyTo, true, "sr");
        assert_eq!(press_tab(app), "src/", "so the next Tab goes inside it");
    });
}

#[test]
fn a_shared_prefix_advances_as_far_as_the_names_agree() {
    browsing(&["cargo.toml", "cargo.lock", "other"], |app, _| {
        open(app, PromptKind::CopyTo, true, "ca");
        assert_eq!(press_tab(app), "cargo.");
        assert_eq!(app.flash_text(), Some("2 matches"));
    });
}

/// With nothing more in common, Tab shows the choices and narrows the listing
/// to them; each further Tab fills in the next one, wrapping at the end.
#[test]
fn ambiguous_names_are_listed_then_cycled() {
    browsing(&["ca", "cb", "d"], |app, _| {
        open(app, PromptKind::MoveTo, true, "c");
        assert_eq!(press_tab(app), "c", "no progress to make");
        assert_eq!(app.flash_text(), Some("ca  cb  — Tab to cycle"));
        assert_eq!(listed(app), ["ca", "cb"], "the listing narrows to them");
        assert_eq!(press_tab(app), "ca");
        assert_eq!(app.flash_text(), Some("ca — 1/2"));
        assert_eq!(press_tab(app), "cb");
        assert_eq!(app.flash_text(), Some("cb — 2/2"));
        assert_eq!(press_tab(app), "ca", "cycling wraps");
    });
}

#[test]
fn a_path_ending_in_a_slash_completes_inside_that_directory() {
    browsing(&["sub/file.txt"], |app, dir| {
        let typed = format!("{}/sub/", dir.display());
        open(app, PromptKind::CopyTo, true, &typed);
        assert_eq!(press_tab(app), format!("{typed}file.txt"));
    });
}

#[test]
fn an_absolute_path_completes_inside_its_own_directory() {
    browsing(&["sub/file.txt", "sub/other"], |app, dir| {
        let typed = format!("{}/sub/fi", dir.display());
        open(app, PromptKind::CopyTo, true, &typed);
        assert_eq!(press_tab(app), format!("{}/sub/file.txt", dir.display()));
    });
}

#[test]
fn a_dot_prefix_completes_dotfiles() {
    browsing(&[".gitignore", "gitx"], |app, _| {
        open(app, PromptKind::CopyTo, true, ".gi");
        assert_eq!(press_tab(app), ".gitignore");
    });
}

#[test]
fn no_match_leaves_the_prompt_alone() {
    browsing(&["README.md"], |app, _| {
        open(app, PromptKind::CopyTo, true, "zzz");
        assert_eq!(press_tab(app), "zzz");
        assert_eq!(app.flash_text(), None);
        assert_eq!(listed(app).len(), 1, "and the listing alone");
    });
}

/// A shell or `:` prompt completes its last word and leaves the command
/// before it as typed.
#[test]
fn a_command_line_completes_only_its_last_word() {
    browsing(&["README.md"], |app, _| {
        for kind in [
            PromptKind::ShellCmd,
            PromptKind::ShellCmdCaptured,
            PromptKind::Command,
        ] {
            open(app, kind, true, "cat REA");
            assert_eq!(press_tab(app), "cat README.md");
        }
    });
}

/// Before its first space a `:` prompt is naming a command, so Tab completes
/// command names, not files.
#[test]
fn a_command_prompt_completes_command_names_before_its_first_space() {
    browsing(&["graveyard-notes.txt"], |app, _| {
        open(app, PromptKind::Command, true, "graveya");
        assert_eq!(press_tab(app), "graveyard ", "ready for its arguments");
    });
}

/// Command names that share more than was typed advance to it; past that, Tab
/// offers them and cycles.
#[test]
fn ambiguous_command_names_advance_then_cycle() {
    browsing(&[], |app, _| {
        open(app, PromptKind::Command, true, "why");
        assert_eq!(press_tab(app), "why-", "as far as the names agree");
        assert_eq!(
            app.flash_text(),
            Some("why-git  why-status  — Tab to cycle")
        );
        assert_eq!(press_tab(app), "why-git");
        assert_eq!(press_tab(app), "why-status");

        open(app, PromptKind::Command, true, "gr");
        assert_eq!(press_tab(app), "gr", "no progress to make");
        assert_eq!(app.flash_text(), Some("graveyard  grep  — Tab to cycle"));
        assert_eq!(press_tab(app), "graveyard");
    });
}

/// `J` falls back to the directories it has visited when nothing on disk
/// matches what was typed.
#[test]
fn a_jump_with_no_path_match_completes_a_visited_directory() {
    browsing(&[], |app, dir| {
        let far = dir.join("elsewhere/projx");
        app.state.frecency.record(&far);
        open(app, PromptKind::Jump, true, "projx");
        assert_eq!(press_tab(app), format!("{}/", far.display()));
    });
}

#[test]
fn several_visited_matches_fill_the_most_visited_then_cycle() {
    browsing(&[], |app, dir| {
        let often = dir.join("elsewhere/projx-often");
        let once = dir.join("elsewhere/projx-once");
        app.state.frecency.record(&often);
        app.state.frecency.record(&often);
        app.state.frecency.record(&once);
        open(app, PromptKind::Jump, true, "projx");
        let first = format!("{}/", often.display());
        assert_eq!(press_tab(app), first);
        assert_eq!(app.flash_text(), Some(&*format!("{first} — 1/2 frecency")));
        assert_eq!(press_tab(app), format!("{}/", once.display()));
    });
}

/// Tab completes a path in every prompt that takes one, whichever editor it
/// has, and in no prompt that takes other text.
#[test]
fn tab_completes_paths_exactly_where_a_path_is_typed() {
    browsing(&["unique.txt"], |app, _| {
        for vi in [false, true] {
            for (name, kind, typed, completed) in [
                ("Jump", PromptKind::Jump, "uniq", "unique.txt"),
                ("CopyTo", PromptKind::CopyTo, "uniq", "unique.txt"),
                ("MoveTo", PromptKind::MoveTo, "uniq", "unique.txt"),
                ("MakeDir", PromptKind::MakeDir, "uniq", "unique.txt"),
                ("NewFile", PromptKind::NewFile, "uniq", "unique.txt"),
                (
                    "PaneNewTabCwd",
                    PromptKind::PaneNewTabCwd,
                    "uniq",
                    "unique.txt",
                ),
                ("ShellCmd", PromptKind::ShellCmd, "uniq", "unique.txt"),
                (
                    "ShellCmdCaptured",
                    PromptKind::ShellCmdCaptured,
                    "uniq",
                    "unique.txt",
                ),
                ("Command", PromptKind::Command, "e uniq", "e unique.txt"),
            ] {
                open(app, kind, vi, typed);
                assert_eq!(press_tab(app), completed, "{name} (vi: {vi})");
            }
            for (name, kind) in [
                ("PatternPick", PromptKind::PatternPick),
                ("Search", PromptKind::Search { saved_cursor: 0 }),
                ("Limit", PromptKind::Limit),
                ("SetEnv", PromptKind::SetEnv),
                ("PaneNewTabCmd", PromptKind::PaneNewTabCmd),
                ("PaneRenameTab", PromptKind::PaneRenameTab),
                ("WorktreeNewBranch", PromptKind::WorktreeNewBranch),
            ] {
                open(app, kind, vi, "uniq");
                assert_eq!(press_tab(app), "uniq", "{name} (vi: {vi})");
            }
        }
    });
}

// Found while writing the tests above. Each states the behaviour a user
// expects; the fix un-ignores them.

#[test]
#[ignore = "bug: `.` has no file name, so Tab offers every entry in the directory"]
fn a_bare_dot_completes_dotfiles() {
    browsing(&[".env", "README.md"], |app, _| {
        open(app, PromptKind::CopyTo, true, ".");
        assert_eq!(press_tab(app), ".env");
    });
}

#[test]
#[ignore = "bug: `..` has no file name, so Tab offers every entry in the directory"]
fn a_bare_dot_dot_completes_to_the_parent() {
    browsing(&["README.md"], |app, _| {
        open(app, PromptKind::CopyTo, true, "..");
        assert_eq!(press_tab(app), "../");
    });
}

#[test]
#[ignore = "bug: `src/.` reads as the name `src`, completing to `src/src/`"]
fn a_dot_after_a_directory_completes_its_dotfiles() {
    browsing(&["src/.env", "src/main.rs"], |app, _| {
        open(app, PromptKind::CopyTo, true, "src/.");
        assert_eq!(press_tab(app), "src/.env");
    });
}

#[test]
#[ignore = "bug: `~` expands to $HOME and completes as a bare relative name"]
fn a_bare_tilde_completes_to_home() {
    browsing(&[], |app, _| {
        open(app, PromptKind::CopyTo, true, "~");
        assert_eq!(press_tab(app), "~/");
    });
}

#[test]
#[ignore = "bug: a relative directory is read from the process's cwd, not the column's"]
fn a_relative_path_completes_inside_the_columns_directory() {
    browsing(&["sub/file.txt"], |app, _| {
        open(app, PromptKind::CopyTo, true, "sub/fi");
        assert_eq!(press_tab(app), "sub/file.txt");
    });
}
