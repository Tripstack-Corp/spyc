//! Tab completion in the prompts: filesystem paths, `:` command names, and
//! `J`'s visited directories, plus the cycle a repeated Tab steps through.
//!
//! The path decision is pure: [`split_word`] picks the word Tab completes,
//! [`locate`] says where that word is looked up, [`implied_names`] adds the
//! completions no directory lists, and [`complete`] says what Tab
//! does with the names found there. `App::tab_complete_path` reads the
//! directory and writes the result into the prompt.

use std::path::{Path, PathBuf};

use super::{App, Mode, PromptKind, TabState};

/// What Tab does with the names a word could complete to.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Completion {
    /// One name matches.
    Only(String),
    /// Several match and share more than was typed: fill in that much.
    Advance {
        common: String,
        matches: Vec<String>,
    },
    /// Several match and share nothing more: offer them.
    Choose(Vec<String>),
}

/// The text Tab leaves alone and the word it completes: a command line's last
/// word, or a path prompt's whole text.
pub(super) fn split_word(buffer: &str, last_word_only: bool) -> (&str, &str) {
    if !last_word_only {
        return ("", buffer);
    }
    buffer.split_at(buffer.rfind(' ').map_or(0, |i| i + 1))
}

/// Where a word is looked up: the directory to read and the part of the name
/// already typed. The name is the text after the word's last `/`, as typed, so
/// `.` and `..` are names to complete rather than steps to resolve. The text
/// before it is expanded (`~`, `$VARS`) and, when relative, read from the
/// column's directory: completion never depends on the process's cwd.
pub(super) fn locate(
    word: &str,
    column_dir: &Path,
    expand: impl Fn(&str) -> PathBuf,
) -> (PathBuf, String) {
    let dir_part = typed_dir(word);
    let typed = word[dir_part.len()..].to_string();
    let dir = if dir_part.is_empty() {
        column_dir.to_path_buf()
    } else {
        column_dir.join(expand(dir_part))
    };
    (dir, typed)
}

/// Completions no directory lists: `../` for a typed `..`, and `~/` for a word
/// that is just `~`.
pub(super) fn implied_names(word: &str, typed: &str) -> Vec<String> {
    if typed == ".." {
        vec!["../".to_string()]
    } else if word == "~" {
        vec!["~/".to_string()]
    } else {
        Vec::new()
    }
}

/// The directory part of a word as the user typed it, up to and including its
/// last `/`. The completed name goes after it, so `~/` stays `~/`.
pub(super) fn typed_dir(word: &str) -> &str {
    &word[..word.rfind('/').map_or(0, |i| i + 1)]
}

/// What Tab does with `names`, each starting with the `typed` part of a name.
/// `None` when nothing matches.
pub(super) fn complete(typed: &str, mut names: Vec<String>) -> Option<Completion> {
    names.sort();
    match names.len() {
        0 => None,
        1 => names.pop().map(Completion::Only),
        _ => {
            let common = common_prefix(&names);
            Some(if common.len() > typed.len() {
                Completion::Advance {
                    common,
                    matches: names,
                }
            } else {
                Completion::Choose(names)
            })
        }
    }
}

/// The entries of `dir` whose names start with `typed`, each directory's with a
/// trailing `/`. `None` when `dir` can't be read.
fn names_starting_with(dir: &Path, typed: &str) -> Option<Vec<String>> {
    let entries = std::fs::read_dir(dir).ok()?;
    Some(
        entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with(typed).then(|| {
                    let is_dir = e.file_type().is_ok_and(|ft| ft.is_dir());
                    format!("{name}{}", if is_dir { "/" } else { "" })
                })
            })
            .collect(),
    )
}

impl App {
    /// Tab-complete a filesystem path in the prompt buffer. For shell
    /// prompts, completes just the last whitespace-delimited word.
    pub fn tab_complete_path(&mut self) {
        let Mode::Prompting(ref prompt) = self.state.mode else {
            return;
        };
        let last_word_only = matches!(
            prompt.kind,
            PromptKind::ShellCmd | PromptKind::ShellCmdCaptured | PromptKind::Command
        );
        let is_jump = matches!(prompt.kind, PromptKind::Jump);
        let is_command = matches!(prompt.kind, PromptKind::Command);
        let buffer = prompt.buffer.clone();

        if self.cycle_tab_completion(&buffer) {
            return;
        }

        // `:` prompt with no whitespace yet — complete the spyc command
        // name from the command registry (`COMMAND_TABLE`) rather than
        // falling through to filesystem completion (which would try to match
        // a file starting with "pa" in cwd, almost never useful here).
        if is_command && !buffer.contains(char::is_whitespace) {
            self.tab_complete_spyc_command(&buffer);
            return;
        }

        let (before, word) = split_word(&buffer, last_word_only);
        let column_dir = self.state.cur().listing.dir.clone();
        let (dir, typed) = locate(word, &column_dir, crate::paths::expand);
        let Some(mut names) = names_starting_with(&dir, &typed) else {
            return;
        };
        names.extend(implied_names(word, &typed));
        let cycle = |matches| TabState {
            original_buf: String::new(),
            buf_prefix: before.to_string(),
            word_base: typed_dir(word).to_string(),
            matches,
            cycle_index: 0,
        };
        match complete(&typed, names) {
            // No filesystem matches — try frecency for Jump prompts.
            None if is_jump => self.frecency_complete(word, before),
            None => {}
            Some(Completion::Only(name)) => {
                self.set_prompt_text(format!("{before}{}{name}", typed_dir(word)));
                self.view.tab_state = None;
            }
            // Advanced to the common prefix, but further Tabs can still cycle.
            Some(Completion::Advance { common, matches }) => {
                self.state.flash_info(format!("{} matches", matches.len()));
                let text = format!("{before}{}{common}", typed_dir(word));
                self.set_prompt_text(text.clone());
                self.view.tab_state = Some(TabState {
                    original_buf: text,
                    ..cycle(matches)
                });
            }
            Some(Completion::Choose(matches)) => {
                if dir == column_dir {
                    // Local dir — also filter the listing.
                    self.state.cur_mut().temp_filter = Some(format!("{typed}*"));
                    self.state.rebuild_rows();
                }
                self.state.flash_info(cycle_hint(&matches));
                self.view.tab_state = Some(TabState {
                    original_buf: buffer.clone(),
                    ..cycle(matches)
                });
            }
        }
    }

    /// A repeated Tab while choices are on offer fills in the next one. `false`
    /// when there is no cycle to continue.
    fn cycle_tab_completion(&mut self, buffer: &str) -> bool {
        let Some(ts) = self.view.tab_state.as_mut() else {
            return false;
        };
        if !(ts.original_buf == buffer || ts.cycle_index > 0) || ts.matches.len() <= 1 {
            return false;
        }
        let idx = ts.cycle_index % ts.matches.len();
        let completed = format!("{}{}{}", ts.buf_prefix, ts.word_base, ts.matches[idx]);
        ts.cycle_index = idx + 1;
        let flash = format!("{} — {}/{}", ts.matches[idx], idx + 1, ts.matches.len());
        self.state.flash_info(flash);
        self.set_prompt_text(completed);
        true
    }

    /// Replace the prompt's text, keeping a vi prompt's editor in step.
    fn set_prompt_text(&mut self, text: String) {
        let Mode::Prompting(ref mut prompt) = self.state.mode else {
            return;
        };
        if let Some(editor) = prompt.editor.as_mut() {
            editor.set_content(&text);
        }
        prompt.buffer = text;
    }

    /// Tab-complete a `:` command base name from the command registry
    /// ([`crate::app::command_table::COMMAND_TABLE`]). Single match: fill the name
    /// plus a trailing space (so the user can keep typing args, or hit Enter
    /// for the no-arg form — `dispatch_command` trims). Common-prefix advance:
    /// fill the shared prefix and flash a count. Otherwise show all matches
    /// and stage cycle state for repeated Tab.
    fn tab_complete_spyc_command(&mut self, prefix: &str) {
        // Built-in (COMMAND_TABLE) names plus any `init.lua`-registered
        // `spyc.command` names — appended at the call site, not in the static
        // table (which stays guard-tested + untouched).
        let mut matches: Vec<String> = crate::app::command_table::completion_command_names()
            .filter(|c| c.starts_with(prefix))
            .map(str::to_string)
            .collect();
        for name in self.runtime.lua_registry.commands.keys() {
            if name.starts_with(prefix) {
                matches.push(name.clone());
            }
        }
        if matches.is_empty() {
            return;
        }
        matches.sort_unstable();
        matches.dedup();

        if matches.len() == 1 {
            let buffer = format!("{} ", matches[0]);
            let Mode::Prompting(ref mut prompt) = self.state.mode else {
                return;
            };
            if let Some(ed) = prompt.editor.as_mut() {
                ed.set_content(&buffer);
            }
            prompt.buffer = buffer;
            self.view.tab_state = None;
            return;
        }

        let common = common_prefix(&matches);
        if common.len() > prefix.len() {
            // Filled some chars but more matches remain — stage cycle
            // state so a follow-up Tab on the same buffer can rotate.
            self.state.flash_info(cycle_hint(&matches));
            let Mode::Prompting(ref mut prompt) = self.state.mode else {
                return;
            };
            if let Some(ed) = prompt.editor.as_mut() {
                ed.set_content(&common);
            }
            prompt.buffer = common;
            self.view.tab_state = Some(TabState {
                original_buf: prompt.buffer.clone(),
                buf_prefix: String::new(),
                word_base: String::new(),
                matches,
                cycle_index: 0,
            });
            return;
        }

        // No textual progress — leave the buffer alone, show all
        // matches, and stage cycle state. The cycle path on the next
        // Tab will compare `original_buf == buffer` (true since we
        // didn't change the buffer) and rotate.
        self.state.flash_info(cycle_hint(&matches));
        self.view.tab_state = Some(TabState {
            original_buf: prefix.to_string(),
            buf_prefix: String::new(),
            word_base: String::new(),
            matches,
            cycle_index: 0,
        });
    }

    /// Frecency fallback for the J prompt: when filesystem completion finds
    /// no matches, search the frecency database for directories matching
    /// the typed fragment.
    fn frecency_complete(&mut self, word: &str, buf_prefix: &str) {
        let hits = self.state.frecency.search(word);
        if hits.is_empty() {
            return;
        }

        // Convert to display strings with trailing slash.
        let names: Vec<String> = hits
            .iter()
            .map(|p| format!("{}/", p.to_string_lossy()))
            .collect();

        if names.len() == 1 {
            // Single match — fill it in directly.
            let completed = format!("{buf_prefix}{}", names[0]);
            let Mode::Prompting(ref mut prompt) = self.state.mode else {
                return;
            };
            prompt.buffer = completed;
            if let Some(ed) = prompt.editor.as_mut() {
                ed.set_content(&prompt.buffer);
            }
            self.view.tab_state = None;
        } else {
            // Multiple frecency matches — fill best, set up cycling.
            let completed = format!("{buf_prefix}{}", names[0]);
            self.state
                .flash_info(format!("{} — 1/{} frecency", names[0], names.len()));
            let Mode::Prompting(ref mut prompt) = self.state.mode else {
                return;
            };
            let original = prompt.buffer.clone();
            prompt.buffer = completed;
            if let Some(ed) = prompt.editor.as_mut() {
                ed.set_content(&prompt.buffer);
            }
            self.view.tab_state = Some(TabState {
                original_buf: original,
                buf_prefix: buf_prefix.to_string(),
                word_base: String::new(),
                matches: names,
                cycle_index: 1, // already showing first match
            });
        }
    }

    /// Clear a Tab-completion preview: the cycle state (`tab_state`) **and**
    /// its paired preview filter (`temp_filter`), atomically.
    ///
    /// Only the Tab preview is paired with `tab_state`, so a user-set
    /// `=`/`:limit` filter (which has no `tab_state`) is left untouched —
    /// without this gate, cancelling any unrelated prompt wiped the active
    /// limit filter.
    ///
    /// Called wherever a Tab preview is abandoned: when a prompt is cancelled
    /// or dispatched, **and** on the first non-Tab key after a preview. Keeping
    /// the pair clear atomic is the invariant `cancel_prompt`/`dispatch_prompt`
    /// rely on: if `tab_state` were nulled alone, the preview filter would
    /// outlive the cycle and leak into the listing behind the prompt.
    pub(crate) fn clear_tab_preview(&mut self) {
        if self.view.tab_state.is_some() && self.state.cur().temp_filter.is_some() {
            self.state.cur_mut().temp_filter = None;
            self.state.rebuild_rows();
        }
        self.view.tab_state = None;
    }
}

/// Build the multi-match Tab-completion flash: candidates joined by two
/// spaces, truncated to the first 12 with a `(+N more)` tail, suffixed
/// with the cycle hint. Shared by the file / command / frecency completion
/// paths, which each reach the "no further shared prefix" branch identically.
fn cycle_hint(matches: &[String]) -> String {
    const MAX_SHOWN: usize = 12;
    let shown = if matches.len() > MAX_SHOWN {
        format!(
            "{}  (+{} more)",
            matches[..MAX_SHOWN]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("  "),
            matches.len() - MAX_SHOWN
        )
    } else {
        matches
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("  ")
    };
    format!("{shown}  — Tab to cycle")
}

/// Longest common prefix of a slice of strings (byte-safe for UTF-8).
fn common_prefix(strings: &[String]) -> String {
    let Some(first) = strings.first() else {
        return String::new();
    };
    let mut byte_len = first.len();
    for s in &strings[1..] {
        byte_len = byte_len.min(s.len());
        for ((i, a), b) in first.char_indices().zip(s.chars()) {
            if a != b {
                byte_len = byte_len.min(i);
                break;
            }
        }
    }
    first[..byte_len].to_string()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Completion, complete, cycle_hint, implied_names, locate, split_word, typed_dir};

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    /// A command line completes its last word; a path prompt's text is one
    /// path, spaces and all.
    #[test]
    fn only_a_command_line_splits_at_spaces() {
        assert_eq!(split_word("cp a.txt sub/fi", true), ("cp a.txt ", "sub/fi"));
        assert_eq!(split_word("cp a.txt ", true), ("cp a.txt ", ""));
        assert_eq!(split_word("my dir/fi", false), ("", "my dir/fi"));
    }

    /// A pretend `paths::expand`, so `~` means the same thing on every machine.
    fn expand(text: &str) -> PathBuf {
        PathBuf::from(text.replacen('~', "/home/u", 1))
    }

    /// The name being completed is the text after the last `/`, as typed; the
    /// directory before it is expanded and, when relative, read from the
    /// column.
    #[test]
    fn a_word_is_looked_up_where_its_directory_part_says() {
        let column = Path::new("/column");
        for (word, dir, typed) in [
            ("", "/column", ""),
            ("fo", "/column", "fo"),
            (".", "/column", "."),
            ("..", "/column", ".."),
            ("sub/fi", "/column/sub", "fi"),
            ("src/.", "/column/src", "."),
            ("src/..", "/column/src", ".."),
            ("../x", "/column/..", "x"),
            ("/abs/fi", "/abs", "fi"),
            ("~/fo", "/home/u", "fo"),
            ("sub/", "/column/sub", ""),
        ] {
            let (found_dir, found_typed) = locate(word, column, expand);
            assert_eq!(
                (found_dir.as_path(), found_typed.as_str()),
                (Path::new(dir), typed),
                "{word:?}"
            );
        }
    }

    /// `~/fo` completes after the `~/` the user typed, not after its expansion.
    #[test]
    fn a_completion_keeps_the_directory_as_typed() {
        assert_eq!(typed_dir("~/fo"), "~/");
        assert_eq!(typed_dir("src/."), "src/");
        assert_eq!(typed_dir("fo"), "");
    }

    #[test]
    fn dot_dot_and_a_bare_tilde_complete_to_directories_no_listing_shows() {
        assert_eq!(implied_names("..", ".."), ["../"]);
        assert_eq!(implied_names("src/..", ".."), ["../"]);
        assert_eq!(implied_names("~", "~"), ["~/"]);
        assert!(implied_names("~/", "").is_empty());
        assert!(implied_names(".", ".").is_empty(), "`.` completes dotfiles");
    }

    #[test]
    fn tab_fills_one_match_advances_shared_text_and_offers_the_rest() {
        assert_eq!(complete("x", Vec::new()), None);
        assert_eq!(
            complete("on", v(&["only"])),
            Some(Completion::Only("only".into()))
        );
        assert_eq!(
            complete("ca", v(&["cargo.toml", "cargo.lock"])),
            Some(Completion::Advance {
                common: "cargo.".into(),
                matches: v(&["cargo.lock", "cargo.toml"]),
            })
        );
        assert_eq!(
            complete("c", v(&["cb", "ca"])),
            Some(Completion::Choose(v(&["ca", "cb"]))),
            "offered in order"
        );
    }

    /// Shared text is measured in characters, so names that differ inside a
    /// multi-byte character never advance to half of one.
    #[test]
    fn shared_text_stops_at_a_character_boundary() {
        assert_eq!(
            complete("", v(&["éa", "éb"])),
            Some(Completion::Advance {
                common: "é".into(),
                matches: v(&["éa", "éb"]),
            })
        );
        assert_eq!(
            complete("", v(&["é", "è"])),
            Some(Completion::Choose(v(&["è", "é"])))
        );
    }

    #[test]
    fn joins_with_two_spaces_and_cycle_suffix() {
        assert_eq!(cycle_hint(&v(&["a", "b"])), "a  b  — Tab to cycle");
    }

    #[test]
    fn shows_all_at_the_twelve_match_boundary() {
        let m = v(&[
            "m1", "m2", "m3", "m4", "m5", "m6", "m7", "m8", "m9", "m10", "m11", "m12",
        ]);
        let hint = cycle_hint(&m);
        assert!(
            !hint.contains("more"),
            "12 matches must not truncate: {hint}"
        );
        assert!(hint.ends_with("  — Tab to cycle"));
    }

    #[test]
    fn truncates_past_twelve_with_more_count() {
        let m: Vec<String> = (0..15).map(|i| format!("m{i}")).collect();
        let hint = cycle_hint(&m);
        assert!(hint.contains("(+3 more)"), "expected +3 more: {hint}");
        assert!(hint.starts_with("m0  m1  "));
        assert!(!hint.contains("m12"), "13th+ match must be hidden: {hint}");
    }
}
