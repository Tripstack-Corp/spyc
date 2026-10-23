//! An agent's own chrome pinned to the bottom of its pane — input box, status
//! line, mode line — which a scan for what the agent *printed* must skip.
//!
//! Claude Code's and Antigravity's (`agy`) composer shape, as captured in a pty:
//! a full-width `─` rule, the `❯` / `>` input row(s), a second full-width `─`
//! rule, then the footer (status line, mode line, workspace path). Both rules
//! start at column 0; `agy` additionally verifies its footer and `>` prompt row
//! so two user-turn divider rules in scrollback are never mistaken for a box.

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
/// active tool-approval dialog — or all of them when neither is on screen.
pub fn agy_output_len(lines: &[String]) -> usize {
    agy_composer_start(lines)
        .or_else(|| agy_approval_start(lines))
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
                && lines.get(i + 1).is_some_and(|r| is_agy_composer_prompt(r)))
            .then_some(i)
        })
}

fn is_agy_composer_prompt(row: &str) -> bool {
    row.starts_with('>') || row.starts_with('❯')
}

fn has_agy_footer(footer: &[String]) -> bool {
    footer.iter().any(|row| {
        row.contains("? for shortcuts")
            || row.contains("sandbox:")
            || row.contains("-- INSERT --")
            || row.contains("-- NORMAL --")
            || row.contains("-- VISUAL --")
    })
}

fn agy_approval_start(lines: &[String]) -> Option<usize> {
    super::detect_rules::scan(lines, super::AGY_DETECTION_RULES)?;
    let req_idx = lines
        .iter()
        .rposition(|row| row.trim() == "Requesting permission for:")?;
    let proceed_offset = lines[req_idx + 1..]
        .iter()
        .position(|row| row.trim() == "Do you want to proceed?")?;
    let tail = &lines[req_idx + 1 + proceed_offset + 1..];
    if !tail.iter().any(|row| row.contains("1. Yes"))
        || !tail.iter().any(|row| row.contains("to cancel"))
    {
        return None;
    }
    if req_idx > 0 && lines[req_idx - 1].trim() == "Command" {
        Some(req_idx - 1)
    } else {
        Some(req_idx)
    }
}

fn is_box_rule(line: &str) -> bool {
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
}

#[cfg(test)]
mod codex_tests;
