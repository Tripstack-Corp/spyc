//! A live Codex composer needs both its input shape and a verified footer.
//! Unknown or clipped forms retain all text rather than hiding a reply.

use crate::agent::{codex_approval, detect_rules};

pub fn codex_output_len(lines: &[String]) -> usize {
    if lines
        .iter()
        .take(3)
        .any(|row| row.trim_start().starts_with("T R A N S C R I P T"))
    {
        return lines.len();
    }
    composer_start(lines)
        .or_else(|| approval_start(lines))
        .unwrap_or(lines.len())
}

fn composer_start(lines: &[String]) -> Option<usize> {
    let last = lines.iter().rposition(|row| !row.trim().is_empty())?;
    let mut footer = last;
    while footer > last.saturating_sub(2) && !lines[footer - 1].trim().is_empty() {
        footer -= 1;
    }
    let footer_text = normalized(&lines[footer..=last]);
    if !known_footer(&footer_text) {
        return None;
    }
    let start = lines[..footer]
        .iter()
        .rposition(|row| composer_prompt(row))?;
    if (start > 0 && !lines[start - 1].trim().is_empty())
        || !lines[start + 1..footer]
            .iter()
            .any(|row| row.trim().is_empty())
        || lines[start + 1..footer]
            .iter()
            .any(|row| !row.trim().is_empty() && !row.starts_with("  "))
    {
        return None;
    }
    if footer_text == "? / esc close"
        && let Some(heading) = lines[..start]
            .iter()
            .rposition(|row| row.trim() == "Keyboard shortcuts")
        && lines[heading..start]
            .iter()
            .any(|row| row.contains("/keymap customize"))
        && lines[heading..start]
            .iter()
            .any(|row| row.contains("Compose") && row.contains("Session"))
    {
        return Some(heading);
    }
    Some(start)
}

fn composer_prompt(row: &str) -> bool {
    ["›", "»", "!"].iter().any(|glyph| {
        row.strip_prefix(glyph)
            .is_some_and(|tail| tail.is_empty() || tail.starts_with(' '))
    })
}

fn known_footer(text: &str) -> bool {
    let words: Vec<_> = text.split_whitespace().collect();
    words.windows(3).any(|parts| {
        (percentage(parts[0]) && parts[1..] == ["context", "left"])
            || (parts[0] == "Context" && percentage(parts[1]) && parts[2] == "left")
    }) || words
        .windows(2)
        .any(|parts| parts[0] == "Vim:" && matches!(parts[1], "Insert" | "Normal" | "Replace"))
        || text.starts_with("? for shortcuts")
        || text == "? / esc close"
        || text.starts_with("esc to interrupt")
        || text.starts_with("tab to queue message")
}

fn percentage(text: &str) -> bool {
    text.strip_suffix('%')
        .and_then(|value| value.parse::<u8>().ok())
        .is_some_and(|value| value <= 100)
}

fn approval_start(lines: &[String]) -> Option<usize> {
    for rule in codex_approval::RULES {
        if detect_rules::scan(lines, std::slice::from_ref(rule)).is_none() {
            continue;
        }
        if let detect_rules::Matcher::AllAtBottom { needles, .. } = rule.matcher {
            let header = needles.first()?;
            return (0..lines.len()).rev().find(|&start| {
                let limit = lines.len().min(start + header.split_whitespace().count());
                (start + 1..=limit).any(|end| normalized(&lines[start..end]).starts_with(header))
            });
        }
    }
    None
}

fn normalized(lines: &[String]) -> String {
    lines
        .iter()
        .flat_map(|row| row.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ")
}
