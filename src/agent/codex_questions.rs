//! Recognize Codex's unanswered async-question views at the viewport bottom.

/// Default-keymap collapsed summaries and complete question-editor footers.
/// A count or a quoted footer alone is insufficient. Clipped/customized forms
/// fail closed; the caller supplies only the current, geometry-bounded viewport.
pub(super) fn matches(text: &str) -> bool {
    let rows: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .collect();
    if rows.last() == Some(&"⇧← to answer") {
        return rows.len() >= 3
            && rows[..rows.len() - 2].contains(&"• Queued follow-up inputs")
            && summary(rows[rows.len() - 2]);
    }
    let mut suffix = String::new();
    for start in (0..rows.len()).rev().take(5) {
        suffix = if suffix.is_empty() {
            rows[start].to_owned()
        } else {
            format!("{} {suffix}", rows[start])
        };
        let normalized = suffix.split_whitespace().collect::<Vec<_>>().join(" ");
        if start > 0 && footer(&normalized) {
            return true;
        }
    }
    false
}

fn summary(text: &str) -> bool {
    let (count, countdown) = text
        .split_once(" · ")
        .map_or((text, None), |(a, b)| (a, Some(b)));
    if countdown.is_some_and(|value| {
        value
            .strip_suffix('s')
            .and_then(positive)
            .is_none_or(|seconds| seconds > 20)
    }) {
        return false;
    }
    let Some(count) = count.strip_prefix("? ") else {
        return false;
    };
    let Some((number, noun)) = count.split_once(' ') else {
        return false;
    };
    positive(number).is_some_and(|n| noun == if n == 1 { "question" } else { "questions" })
}

fn positive(text: &str) -> Option<usize> {
    (!text.starts_with('0') && !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn footer(text: &str) -> bool {
    let Some(mut tail) = text.strip_prefix("enter submit ⌃] skip ") else {
        return false;
    };
    if let Some(options) = tail.strip_prefix("option ") {
        let Some((position, rest)) = options.split_once(' ') else {
            return false;
        };
        let Some((current, total)) = position.split_once('/') else {
            return false;
        };
        if positive(current)
            .zip(positive(total))
            .is_none_or(|(n, total)| n > total)
        {
            return false;
        }
        tail = rest;
    }
    ["⇧→ main prompt", "⇧→ prev question"]
        .iter()
        .any(|navigation| {
            tail.strip_prefix(navigation).is_some_and(|remaining| {
                matches!(remaining, "" | " ⇧← next question" | " ⇧← queued messages")
            })
        })
}

#[cfg(test)]
mod tests {
    use crate::agent::{detect_rules, profile_for};
    use crate::pane::AgentActivity;
    use crate::state::sessions::AgentKind;

    fn scan(text: &str) -> Option<(AgentActivity, Option<&'static str>)> {
        detect_rules::scan(
            &text.lines().map(String::from).collect::<Vec<_>>(),
            profile_for(AgentKind::Codex).detection_rules(),
        )
    }

    #[test]
    fn codex_questions_recorded_expanded_and_collapsed_views_need_attention() {
        for text in [
            include_str!("../../tests/fixtures/codex-questions/expanded-options.txt"),
            include_str!("../../tests/fixtures/codex-questions/expanded-freeform.txt"),
            include_str!("../../tests/fixtures/codex-questions/expanded-wrapped.txt"),
            include_str!("../../tests/fixtures/codex-questions/collapsed.txt"),
        ] {
            assert_eq!(
                scan(text),
                Some((
                    AgentActivity::Blocked,
                    Some("awaiting async question answer")
                )),
                "{text}"
            );
        }
    }

    #[test]
    fn codex_questions_quoted_clipped_and_customized_footers_are_not_current_waits() {
        let expanded = include_str!("../../tests/fixtures/codex-questions/expanded-options.txt");
        let collapsed = include_str!("../../tests/fixtures/codex-questions/collapsed.txt");
        for text in [
            format!("{expanded}\n› Ask Codex to do anything\n100% context left | Vim: Insert"),
            format!("{collapsed}\n› An unfinished prompt\n? for shortcuts"),
            expanded.replace("⌃] skip", ""),
            expanded.replace("enter submit", "enter submi…"),
            expanded.replace("⇧→ prev question", "shift-right back"),
            expanded.replace("enter submit", "Quoted: enter submit"),
            collapsed.replace("? 2 questions", "? 0 questions"),
            collapsed.replace("? 2 questions", "? 2 question"),
            collapsed.replace("? 2 questions", "Questions: ? 2 questions"),
            collapsed.replace("⇧← to answer", "⇧← to answe…"),
            collapsed.replace("⇧← to answer", "Other footer: ⇧← to answer"),
            "enter submit   ⌃] skip   ⇧→ main prompt".into(),
            include_str!("../../tests/fixtures/codex-questions/expanded-clipped.txt").into(),
        ] {
            assert_eq!(scan(&text), None, "{text}");
        }
    }

    #[test]
    fn codex_questions_collapsed_counts_and_native_countdown_have_a_complete_shape() {
        for count in [1, 2, 64] {
            let noun = if count == 1 { "question" } else { "questions" };
            for countdown in ["", " · 20s", " · 1s"] {
                let text = format!(
                    "• Queued follow-up inputs\n  ? {count} {noun}{countdown}\n    ⇧← to answer\n"
                );
                assert_eq!(
                    scan(&text),
                    Some((
                        AgentActivity::Blocked,
                        Some("awaiting async question answer")
                    ))
                );
            }
        }
        for summary in [
            "? 00 questions",
            "? -1 questions",
            "? 2 questions · 0s",
            "? 2 questions · soon",
            "? 2 questions unrelated",
            "? 99999999999999999999 questions",
        ] {
            assert_eq!(
                scan(&format!(
                    "• Queued follow-up inputs\n{summary}\n⇧← to answer"
                )),
                None,
                "{summary}"
            );
        }
    }

    #[test]
    fn codex_questions_option_scroll_hints_follow_the_native_footer_order() {
        for footer in [
            "enter submit   ⌃] skip   option 2/4   ⇧→ main prompt",
            "enter submit   ⌃] skip\noption 2/4   ⇧→ prev question\n⇧← next question",
            "enter submit   ⌃] skip   ⇧→ main prompt   ⇧← queued messages",
        ] {
            assert_eq!(
                scan(&format!("Which colour?\n› 2. Blue\n{footer}")),
                Some((
                    AgentActivity::Blocked,
                    Some("awaiting async question answer")
                ))
            );
        }
        for footer in [
            "⌃] skip enter submit ⇧→ main prompt",
            "enter submit ⌃] skip option 0/4 ⇧→ main prompt",
            "enter submit ⌃] skip option 5/4 ⇧→ main prompt",
            "enter submit ⌃] skip ⇧→ main prompt extra",
        ] {
            assert_eq!(scan(&format!("Which colour?\n{footer}")), None, "{footer}");
        }
    }

    #[test]
    fn codex_questions_other_agents_do_not_inherit_the_fallback() {
        let lines = include_str!("../../tests/fixtures/codex-questions/expanded-options.txt")
            .lines()
            .map(String::from)
            .collect::<Vec<_>>();
        for kind in [AgentKind::Claude, AgentKind::Agy, AgentKind::Other] {
            assert_eq!(
                detect_rules::scan(&lines, profile_for(kind).detection_rules()),
                None
            );
        }
    }
}
