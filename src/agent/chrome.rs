//! An agent's own chrome pinned to the bottom of its pane — input box, status
//! line, mode line — which a scan for what the agent *printed* must skip.
//!
//! Claude Code's and Antigravity's (`agy`) composer shape, as captured in a pty:
//! a full-width `─` rule, the `❯` / `>` input row(s), a second full-width `─`
//! rule, then the footer (status line, mode line, workspace path). Both rules
//! start at column 0; `agy` additionally verifies its `>` prompt row and that
//! everything below the box is its footer, so two user-turn divider rules in
//! scrollback don't pass as a box.

mod codex;
pub use codex::codex_output_len;

/// Non-blank rows a footer can fill below the input box: a multi-line status
/// line, the mode line, and an open completion menu. A bottom rule farther up
/// than this is not the box's.
const FOOTER_MAX_ROWS: usize = 20;

/// Rows a typed prompt can span between the box's two rules.
const INPUT_MAX_ROWS: usize = 40;

/// Shorter than any pane claude is usable in, longer than a table border.
const MIN_RULE_CHARS: usize = 20;

/// How many of `lines` (oldest first) come before claude's input box — the
/// index of the box's top rule — or all of them when no box is on screen,
/// as while a permission dialog stands in for it.
pub fn claude_output_len(lines: &[String]) -> usize {
    let Some(bottom) = lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, l)| !l.trim().is_empty())
        .take(FOOTER_MAX_ROWS)
        .find_map(|(i, l)| is_box_rule(l).then_some(i))
    else {
        return lines.len();
    };
    lines[..bottom]
        .iter()
        .enumerate()
        .rev()
        .take(INPUT_MAX_ROWS)
        .find_map(|(i, l)| is_box_rule(l).then_some(i))
        .unwrap_or(lines.len())
}

/// How many of `lines` (oldest first) come before agy's live composer box or
/// active approval/question dialog — or all of them when neither is on screen.
pub fn agy_output_len(lines: &[String]) -> usize {
    agy_composer_start(lines)
        .or_else(|| super::agy_approval::dialog_top(lines))
        .unwrap_or(lines.len())
}

fn agy_composer_start(lines: &[String]) -> Option<usize> {
    let bottom = lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, l)| !l.trim().is_empty())
        .take(FOOTER_MAX_ROWS)
        .find_map(|(i, l)| is_box_rule(l).then_some(i))?;
    if !has_agy_footer(&lines[bottom + 1..]) {
        return None;
    }
    let bottom_rule = lines[bottom].trim_end();
    lines[..bottom]
        .iter()
        .enumerate()
        .rev()
        .take(INPUT_MAX_ROWS)
        .find_map(|(i, l)| {
            (is_box_rule(l)
                && l.trim_end() == bottom_rule
                && is_agy_composer_body(&lines[i + 1..bottom]))
            .then_some(i)
        })
}

fn is_agy_composer_body(body: &[String]) -> bool {
    body.first().is_some_and(|r| is_agy_composer_prompt(r))
        && !body.iter().any(|r| {
            is_box_rule(r) || r.trim_start().starts_with("● ") || r.trim_start().starts_with("○ ")
        })
}

fn is_agy_composer_prompt(row: &str) -> bool {
    row.starts_with('>') || row.starts_with('❯')
}

/// The rows below the box are agy's footer: a shortcut, Vim-mode, workspace or
/// working `<key> to cancel` row. agy 1.4.7 keeps the box on screen while a turn
/// runs, so a draft typed mid-turn sits above the cancel footer. A `>` prompt
/// or a turn row below the bottom rule means that rule was a turn divider, not
/// the box.
fn has_agy_footer(footer: &[String]) -> bool {
    footer.iter().any(|row| {
        row.contains("? for shortcuts")
            || row.contains("sandbox:")
            || row.contains("-- INSERT --")
            || row.contains("-- NORMAL --")
            || row.contains("-- VISUAL --")
            || super::agy_approval::is_cancel_footer(row)
    }) && !footer.iter().any(|row| {
        let t = row.trim_start();
        t == "Requesting permission for:"
            || t.starts_with("● ")
            || t.starts_with("○ ")
            || is_agy_composer_prompt(row)
    })
}

pub(super) fn is_box_rule(line: &str) -> bool {
    let rule = line.trim_end();
    !rule.starts_with(char::is_whitespace)
        && rule.chars().count() >= MIN_RULE_CHARS
        && rule.chars().all(|c| c == '─')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &[&str]) -> Vec<String> {
        s.iter().copied().map(String::from).collect()
    }

    fn rule() -> String {
        "─".repeat(60)
    }

    /// The captured shape, footer included: the status line's `CLAUDE.md` is
    /// what a bottom-up path scan found before the conversation's own path.
    fn claude_screen(output: &[&str]) -> Vec<String> {
        let mut v = lines(output);
        v.push(String::new());
        v.push(rule());
        v.push("❯\u{a0}".into());
        v.push(rule());
        v.push("  [Opus 5.5] │ spyc git:(main)".into());
        v.push("  1 CLAUDE.md | 1 MCPs | 11 hooks".into());
        v.push("  -- INSERT -- ⏵⏵ auto mode on".into());
        v
    }

    #[test]
    fn output_ends_at_the_input_box_top_rule() {
        let screen = claude_screen(&["⏺ Wrote src/main.rs", "  done"]);
        let n = claude_output_len(&screen);
        assert_eq!(
            &screen[..n],
            &lines(&["⏺ Wrote src/main.rs", "  done", ""])[..]
        );
    }

    #[test]
    fn blank_rows_below_the_footer_do_not_hide_the_box() {
        let mut screen = claude_screen(&["out"]);
        let with_box = claude_output_len(&screen);
        screen.extend(std::iter::repeat_n(String::new(), 30));
        assert_eq!(claude_output_len(&screen), with_box);
    }

    #[test]
    fn a_multi_line_prompt_stays_inside_the_box() {
        let mut screen = lines(&["out", "", &rule(), "❯ first line"]);
        screen.extend((0..10).map(|i| format!("  line {i}")));
        screen.push(rule());
        screen.push("  -- INSERT --".into());
        assert_eq!(claude_output_len(&screen), 2);
    }

    #[test]
    fn indented_rules_are_output_not_the_box() {
        // A markdown `---` in a reply renders indented under its `⏺`.
        let indented = format!("  {}", rule());
        let screen = lines(&["⏺ Part one", &indented, "  Part two", &indented]);
        assert_eq!(claude_output_len(&screen), screen.len());
    }

    #[test]
    fn a_lone_rule_is_not_a_box() {
        // A permission dialog draws one top rule and no input box under it.
        let screen = lines(&["out", &rule(), " Bash command", " Do you want to proceed?"]);
        assert_eq!(claude_output_len(&screen), screen.len());
    }

    #[test]
    fn a_short_rule_is_not_a_box_border() {
        let short = "─".repeat(MIN_RULE_CHARS - 1);
        let screen = lines(&["out", &short, "❯", &short, "  -- INSERT --"]);
        assert_eq!(claude_output_len(&screen), screen.len());
    }

    #[test]
    fn a_rule_far_above_the_footer_is_not_the_box() {
        let mut screen = lines(&[&rule(), "❯", &rule()]);
        screen.extend((0..FOOTER_MAX_ROWS).map(|i| format!("out {i}")));
        assert_eq!(claude_output_len(&screen), screen.len());
    }

    #[test]
    fn no_lines_is_no_output() {
        assert_eq!(claude_output_len(&[]), 0);
        assert_eq!(agy_output_len(&[]), 0);
    }

    fn agy_screen(output: &[&str], draft: &[&str]) -> Vec<String> {
        let mut v = lines(output);
        v.push(String::new());
        v.push(rule());
        if draft.is_empty() {
            v.push("> Accept-edits mode: file edits auto-approved (shift+tab to cycle)".into());
        } else {
            for (i, row) in draft.iter().enumerate() {
                if i == 0 {
                    v.push(format!("> {row}"));
                } else {
                    v.push(format!("  {row}"));
                }
            }
        }
        v.push(rule());
        v.push("? for shortcuts                                         accept-edits".into());
        v.push("                                               Gemini 3.1 Pro (High)".into());
        v.push("~/src/spyc (main)                                       sandbox: off".into());
        v
    }

    #[test]
    fn agy_output_ends_at_the_composer_top_rule() {
        let screen = agy_screen(
            &["● Edited src/main.rs", "  done"],
            &["inspect src/draft.rs"],
        );
        let n = agy_output_len(&screen);
        assert_eq!(
            &screen[..n],
            &lines(&["● Edited src/main.rs", "  done", ""])[..]
        );
    }

    #[test]
    fn agy_user_turn_dividers_without_a_composer_stay_in_output() {
        let turn_rule = "─".repeat(50);
        let screen = lines(&[
            &turn_rule,
            "first user prompt mentioning src/first.rs",
            "",
            "● Explored 1 file (src/first.rs)",
            "",
            &turn_rule,
            "second user prompt mentioning src/second.rs",
            "",
            "Thought for 4.0s",
            "Running cargo test in src/lib.rs.",
        ]);
        assert_eq!(agy_output_len(&screen), screen.len());
    }

    #[test]
    fn agy_approval_dialog_and_footer_are_excluded_from_output() {
        let screen = lines(&[
            "● Wrote src/printed.rs",
            "● run_command (Git status check)",
            "Command",
            "Requesting permission for:",
            "  git -C /Users/example/src/spyc status --short",
            "Purpose:",
            "  Git status check",
            "Do you want to proceed?",
            "❯ 1. Yes",
            "  2. Yes, and always allow (Persist to settings.json)",
            "  3. No",
            "↑/↓ Navigate · tab Amend · ctrl+g edit/expand command",
            "esc to cancel                          Gemini 3.1 Pro (High)",
            "~/src/spyc (main)                      sandbox: off",
        ]);
        let n = agy_output_len(&screen);
        assert_eq!(
            &screen[..n],
            &lines(&["● Wrote src/printed.rs", "● run_command (Git status check)",])[..]
        );
    }

    /// A prose reply carries no `●`/`○`, so an earlier turn's divider rule, its
    /// `>` prompt and the reply look like a composer body. The rows below the
    /// next divider (a `>` prompt, the spinner) are no composer footer.
    #[test]
    fn agy_prose_only_earlier_turn_is_output() {
        let rule = "─".repeat(60);
        let screen = lines(&[
            "● Explored 2 files",
            "",
            &rule,
            "> what does src/printed.rs do?",
            "  It prints a greeting; see src/printed.rs:1.",
            "",
            &rule,
            "> now explain src/other.rs",
            "",
            "⣟  Thinking... (esc to cancel, 2s)",
            "",
            "esc to cancel     accept-edits · Gemini 4 Argon · high",
            "/Users/example/src/spyc (main)     sandbox: off",
        ]);
        let n = agy_output_len(&screen);
        assert!(
            screen[..n]
                .iter()
                .any(|r| r.contains("It prints a greeting")),
            "the earlier reply was cut from the output: {:?}",
            &screen[..n]
        );
    }

    /// A draft typed while a turn runs sits in a composer whose footer is the
    /// working `esc to cancel` row, with no vim marker or `sandbox:` row. The
    /// draft is not output.
    #[test]
    fn agy_mid_turn_draft_under_the_cancel_footer_is_not_output() {
        let rule = "─".repeat(100);
        let screen = lines(&[
            "● Edited 1 file (src/printed.rs) (ctrl+o to expand)",
            "",
            "⣟  Running command... (esc to cancel, 4s)",
            "",
            &rule,
            "> check src/unsubmitted.rs",
            &rule,
            "esc to cancel                                       accept-edits · Gemini 4 Argon · high",
        ]);
        assert_eq!(agy_output_len(&screen), 4);
    }

    /// Below an answered dialog that is still drawn, the spinner is not the
    /// `↑/↓ Navigate` row's wrapped tail: nothing is live, so all of it is output.
    #[test]
    fn agy_spinner_below_an_answered_dialog_keeps_the_output() {
        let rule = "─".repeat(70);
        let screen = lines(&[
            "● Ran (gh pr list --limit 5) (ctrl+o to expand)",
            "",
            "Command",
            &rule,
            "",
            "Requesting permission for:",
            "   gh pr list --limit 5",
            "",
            "Run this command?",
            "> 1. Yes, run command",
            "  2. No, cancel",
            "",
            "  ↑/↓ Navigate · tab Amend · ctrl+g edit/expand command",
            "⣟  Running command... (esc to cancel, 4s)",
            "esc to cancel                     accept-edits · Gemini 4 Argon · high",
        ]);
        assert_eq!(agy_output_len(&screen), screen.len());
    }

    #[test]
    fn agy_quoted_approval_above_a_live_composer_remains_output() {
        let screen = agy_screen(
            &[
                "● Wrote src/printed.rs",
                "Command",
                "Requesting permission for:",
                "  git status",
                "Do you want to proceed?",
                "❯ 1. Yes",
                "esc to cancel",
            ],
            &[],
        );
        assert_eq!(agy_output_len(&screen), 8);
    }

    fn agy_fixture(name: &str) -> Vec<String> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/agy-output")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
            .lines()
            .map(String::from)
            .collect()
    }

    /// agy 1.4.7 draws its composer while a turn runs, under the working
    /// `esc to cancel` footer (after `-- INSERT --` in vim mode). The output
    /// stops at the composer's top rule and keeps the running turn.
    #[test]
    fn agy_working_fixtures_stop_at_the_composer_and_keep_the_turn() {
        for fixture in ["working.txt", "working-draft.txt"] {
            let screen = agy_fixture(fixture);
            let prompt = screen
                .iter()
                .rposition(|row| row.starts_with('>'))
                .unwrap_or_else(|| panic!("{fixture} must contain the composer's `>` row"));
            let n = agy_output_len(&screen);
            assert_eq!(
                n,
                prompt - 1,
                "{fixture} must stop at the composer's top rule"
            );
            assert!(
                screen[..n]
                    .iter()
                    .any(|row| row.contains("Running (uname -a && sleep 8)")),
                "{fixture} must keep the running turn: {:?}",
                &screen[..n]
            );
        }
    }

    #[test]
    fn agy_short_approval_after_quoted_prompt_keeps_turn_output_and_strips_dialog() {
        let turn_rule = "─".repeat(60);
        let dialog_rule = "─".repeat(80);
        let screen = lines(&[
            &turn_rule,
            "> > out.txt redirect",
            "",
            "● Edited 1 file (src/printed.rs)",
            "",
            "Command",
            &dialog_rule,
            "",
            "Requesting permission for:",
            "  git status",
            "",
            "Run this command?",
            "> 1. Yes, run command",
            "  2. No, cancel",
            "",
            "  ↑/↓ Navigate · tab Amend",
            "esc to cancel                                     accept-edits · Gemini 4 Argon · high",
        ]);
        let n = agy_output_len(&screen);
        assert_eq!(
            &screen[..n],
            &lines(&[
                &turn_rule,
                "> > out.txt redirect",
                "",
                "● Edited 1 file (src/printed.rs)",
                "",
            ])[..]
        );
    }

    #[test]
    fn agy_captured_approval_and_question_fixtures_strip_dialog_from_header_down() {
        for (fixture, header) in [
            ("approval-command.txt", "Command"),
            ("approval-command-narrow.txt", "Command"),
            ("approval-file.txt", "File"),
            ("approval-edit.txt", "Pending edit"),
            ("approval-question.txt", "Question"),
        ] {
            let screen = agy_fixture(fixture);
            let expected = screen
                .iter()
                .rposition(|row| row.trim() == header)
                .unwrap_or_else(|| panic!("{fixture} missing {header} header"));
            assert_eq!(
                agy_output_len(&screen),
                expected,
                "{fixture} must strip from the `{header}` header row downward"
            );
        }

        // Question header and prompt must sit at column 0; indented prose is output.
        let q_screen = agy_fixture("approval-question.txt");
        let indented_hdr: Vec<String> = q_screen
            .iter()
            .map(|r| {
                if r.trim_end() == "Question" {
                    format!("  {r}")
                } else {
                    r.clone()
                }
            })
            .collect();
        assert_eq!(
            agy_output_len(&indented_hdr),
            indented_hdr.len(),
            "indented Question header must remain output"
        );
        let indented_prompt: Vec<String> = q_screen
            .iter()
            .map(|r| {
                if r.starts_with("Question 1/1:") {
                    format!("  {r}")
                } else {
                    r.clone()
                }
            })
            .collect();
        assert_eq!(
            agy_output_len(&indented_prompt),
            indented_prompt.len(),
            "indented Question prompt row must remain output"
        );
    }

    #[test]
    fn agy_composer_rejects_turn_rows_or_rules_inside_body_and_footer() {
        let rule60 = "─".repeat(60);
        let rule80 = "─".repeat(80);
        for body_intruder in [
            rule80.as_str(),
            "● Edited 1 file (src/printed.rs)",
            "○ Thought for 2.0s",
        ] {
            let screen = lines(&[
                "● Explored 1 file (src/first.rs)",
                &rule80,
                "> first user prompt",
                body_intruder,
                &rule80,
                "? for shortcuts                                         accept-edits · Gemini 4 Argon · high",
            ]);
            assert_eq!(
                agy_output_len(&screen),
                screen.len(),
                "composer body containing `{body_intruder}` must not be stripped as a composer"
            );
        }

        for footer_intruder in [
            "Requesting permission for:",
            "● Edited 1 file (src/printed.rs)",
            "○ Thought for 2.0s",
            rule60.as_str(),
        ] {
            let screen = lines(&[
                "● Explored 1 file (src/first.rs)",
                &rule80,
                "> first user prompt",
                &rule80,
                footer_intruder,
                "/Users/example/src/spyc (main)                                                          sandbox: off",
            ]);
            assert_eq!(
                agy_output_len(&screen),
                screen.len(),
                "footer containing `{footer_intruder}` below bottom rule must not match a composer"
            );
        }

        let mid_turn_composer = lines(&[
            "● Edited 1 file (src/printed.rs)",
            &rule80,
            "> check src/unsubmitted.rs",
            &rule80,
            "esc to cancel · -- INSERT --                            accept-edits · Gemini 4 Argon · high",
        ]);
        assert_eq!(
            agy_output_len(&mid_turn_composer),
            1,
            "a live composer box during a turn with `esc to cancel` in its footer must still strip the draft"
        );
    }

    #[test]
    fn agy_composer_empty_and_draft_fixtures_stop_at_the_top_box_rule() {
        for (fixture, draft_token) in [
            ("composer-empty.txt", ">"),
            ("composer-draft.txt", "> check src/unsubmitted.rs"),
        ] {
            let screen = agy_fixture(fixture);
            let prompt_idx = screen
                .iter()
                .rposition(|row| row.trim_end() == draft_token)
                .unwrap_or_else(|| panic!("{fixture} must contain `{draft_token}`"));
            let n = agy_output_len(&screen);
            assert_eq!(
                n,
                prompt_idx - 1,
                "{fixture} must stop at the composer's top box rule"
            );
            assert!(
                screen[..n]
                    .iter()
                    .any(|row| row.contains("● Ran (uname -a && sleep 8)")),
                "{fixture} must retain turn output above composer"
            );
        }
    }

    #[test]
    fn agy_composer_requires_a_verified_footer_below_the_bottom_rule() {
        let rule80 = "─".repeat(80);
        let screen = lines(&[
            "● Edited 1 file (src/printed.rs)",
            &rule80,
            "> check src/unsubmitted.rs",
            &rule80,
            "  /Users/example/src/spyc",
        ]);
        assert_eq!(
            agy_output_len(&screen),
            screen.len(),
            "box rules without a recognized agy footer must remain output"
        );
    }

    #[test]
    fn agy_composer_requires_matching_top_and_bottom_rule_widths() {
        let rule60 = "─".repeat(60);
        let rule80 = "─".repeat(80);
        let screen = lines(&[
            "● Edited 1 file (src/printed.rs)",
            &rule60,
            "> check src/unsubmitted.rs",
            &rule80,
            "? for shortcuts                                         accept-edits · Gemini 4 Argon · high",
        ]);
        assert_eq!(
            agy_output_len(&screen),
            screen.len(),
            "mismatched rule widths must not be treated as a composer box"
        );
    }

    #[test]
    fn agy_composer_requires_a_prompt_glyph_on_the_first_box_row() {
        let rule80 = "─".repeat(80);
        let without_glyph = lines(&[
            "● Edited 1 file (src/printed.rs)",
            &rule80,
            "  plain table or markdown block inside rules",
            &rule80,
            "? for shortcuts                                         accept-edits · Gemini 4 Argon · high",
        ]);
        assert_eq!(
            agy_output_len(&without_glyph),
            without_glyph.len(),
            "a rule box whose first line lacks `>` or `❯` must remain output"
        );

        let with_ascii_glyph = lines(&[
            "● Edited 1 file (src/printed.rs)",
            &rule80,
            "> draft using ascii prompt glyph",
            &rule80,
            "? for shortcuts                                         accept-edits · Gemini 4 Argon · high",
        ]);
        assert_eq!(
            agy_output_len(&with_ascii_glyph),
            1,
            "`>` prompt glyph must be recognized as a composer box"
        );

        let with_heavy_glyph = lines(&[
            "● Edited 1 file (src/printed.rs)",
            &rule80,
            "❯ draft using heavy prompt glyph",
            &rule80,
            "? for shortcuts                                         accept-edits · Gemini 4 Argon · high",
        ]);
        assert_eq!(
            agy_output_len(&with_heavy_glyph),
            1,
            "`❯` prompt glyph must be recognized as a composer box"
        );
    }
}

#[cfg(test)]
mod codex_tests;
