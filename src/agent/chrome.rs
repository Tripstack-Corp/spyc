//! An agent's own chrome pinned to the bottom of its pane — input box, status
//! line, mode line — which a scan for what the agent *printed* must skip.
//!
//! Claude Code's shape, as captured from v2.1 in a pty: a full-width `─` rule,
//! the `❯` input row(s), a second full-width `─` rule, then the footer (status
//! line, `-- INSERT --` mode line). Both rules start at column 0; a rule claude
//! prints in its output is indented under the reply's `⏺`, which is what keeps
//! a markdown `---` from being read as the box.

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
    }
}
