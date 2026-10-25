//! Recognize Antigravity (`agy`) live tool-approval and question dialogs at the
//! bottom of the pane viewport.
//!
//! Shared between [`super::detect_rules::Matcher::AgyApproval`] (status scrape
//! fallback) and [`super::chrome::agy_output_len`] (output navigation boundary)
//! so both enforce the same contiguous dialog chrome and return the dialog's
//! top header row.

use super::chrome::is_box_rule;

const SCAN_NON_EMPTY_ROWS: usize = 30;
const MAX_TARGET_ROWS: usize = 12;
const MAX_EDIT_DIFF_ROWS: usize = 20;

/// Whether `text` (the bottom non-empty lines of a pane) ends with a live `agy`
/// tool-approval dialog.
pub(super) fn matches(text: &str) -> bool {
    let rows: Vec<&str> = text.lines().filter(|row| !row.trim().is_empty()).collect();
    approval_top(&rows).is_some()
}

/// Index in `lines` of the top header row (`Command`, `File`, `Pending edit`,
/// `Question`, etc.) of a live `agy` approval or question dialog at the bottom
/// of the viewport.
pub(super) fn dialog_top(lines: &[String]) -> Option<usize> {
    let mut indexed: Vec<(usize, &str)> = lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, row)| !row.trim().is_empty())
        .take(SCAN_NON_EMPTY_ROWS)
        .map(|(i, row)| (i, row.as_str()))
        .collect();
    indexed.reverse();
    let rows: Vec<&str> = indexed.iter().map(|&(_, row)| row).collect();
    let top = approval_top(&rows).or_else(|| question_top(&rows))?;
    Some(indexed[top].0)
}

/// Index in `rows` (non-empty lines) of the header word starting a live `agy`
/// tool-approval dialog.
fn approval_top(rows: &[&str]) -> Option<usize> {
    let cancel_idx = cancel_footer_index(rows)?;
    let nav_idx = navigate_row_index(rows, cancel_idx)?;
    let q_idx = (0..nav_idx)
        .rev()
        .find(|&i| is_col0(rows[i]) && is_approval_question(rows[i].trim()))?;
    if !has_ordered_options(&rows[q_idx + 1..nav_idx], true) {
        return None;
    }
    permission_header_top(rows, q_idx).or_else(|| pending_edit_header_top(rows, q_idx))
}

/// Index in `rows` (non-empty lines) of the `Question` header starting a live
/// `agy` `ask_question` dialog.
fn question_top(rows: &[&str]) -> Option<usize> {
    let cancel_idx = cancel_footer_index(rows)?;
    let nav_idx = navigate_row_index(rows, cancel_idx)?;
    let hdr_idx = (0..nav_idx)
        .rev()
        .find(|&i| is_col0(rows[i]) && rows[i].trim() == "Question")?;
    if hdr_idx + 3 >= nav_idx || !is_box_rule(rows[hdr_idx + 1]) {
        return None;
    }
    let prompt_idx = hdr_idx + 2;
    if !is_col0(rows[prompt_idx])
        || (!rows[prompt_idx].starts_with("Question ")
            && !rows[prompt_idx].trim_end().ends_with('?'))
    {
        return None;
    }
    has_ordered_options(&rows[prompt_idx + 1..nav_idx], false).then_some(hdr_idx)
}

fn permission_header_top(rows: &[&str], q_idx: usize) -> Option<usize> {
    let req_idx = (0..q_idx)
        .rev()
        .find(|&i| is_col0(rows[i]) && rows[i].trim() == "Requesting permission for:")?;
    let target = &rows[req_idx + 1..q_idx];
    if target.is_empty()
        || target.len() > MAX_TARGET_ROWS
        || target.iter().any(|&r| is_turn_or_rule_row(r))
    {
        return None;
    }
    // agy 1.4.7 draws a rule under the header; the 1.1.11 sample has none.
    let above = req_idx.checked_sub(1)?;
    let header = if is_box_rule(rows[above]) {
        above.checked_sub(1)?
    } else {
        above
    };
    is_permission_header(rows[header]).then_some(header)
}

fn pending_edit_header_top(rows: &[&str], q_idx: usize) -> Option<usize> {
    if rows[q_idx].trim() != "Accept this file edit?" || q_idx < 3 {
        return None;
    }
    if !is_auto_approve_edits_row(rows[q_idx - 1]) {
        return None;
    }
    let rule_idx = (1..q_idx - 1).rev().find(|&i| is_box_rule(rows[i]))?;
    let body = &rows[rule_idx + 1..q_idx - 1];
    if body.is_empty()
        || body.len() > MAX_EDIT_DIFF_ROWS
        || body.iter().any(|&r| is_turn_row(r))
        || !is_edit_header(rows[rule_idx - 1])
    {
        return None;
    }
    Some(rule_idx - 1)
}

fn cancel_footer_index(rows: &[&str]) -> Option<usize> {
    let last = rows.len().checked_sub(1)?;
    if is_cancel_footer(rows[last]) {
        return Some(last);
    }
    if rows.len() >= 2
        && is_cancel_footer(rows[last - 1])
        && (is_workspace_footer(rows[last]) || is_wrapped_cancel_tail(rows[last]))
    {
        return Some(last - 1);
    }
    None
}

fn navigate_row_index(rows: &[&str], cancel_idx: usize) -> Option<usize> {
    if cancel_idx >= 1 && is_navigate_row(rows[cancel_idx - 1]) {
        return Some(cancel_idx - 1);
    }
    if cancel_idx >= 2
        && is_navigate_row(rows[cancel_idx - 2])
        && is_option_continuation(rows[cancel_idx - 1])
    {
        return Some(cancel_idx - 2);
    }
    None
}

fn is_navigate_row(row: &str) -> bool {
    row.trim_start().starts_with("↑/↓ Navigate")
}

/// Agy formats its cancel hint from `"%s to cancel"`, so `<key token> to cancel`
/// matches both the default `esc` key and any user-rebound cancel key while
/// rejecting ordinary prose words such as `"how to cancel"`.
pub(super) fn is_cancel_footer(row: &str) -> bool {
    let trimmed = row.trim_start();
    let Some((token, _)) = trimmed.split_once(" to cancel") else {
        return false;
    };
    is_key_token(token)
}

/// Agy formats its file-edit auto-approve hint from `"to auto-approve file edits"`
/// preceded by the bound key token (default `shift+tab`).
fn is_auto_approve_edits_row(row: &str) -> bool {
    let Some((token, rest)) = row.trim().split_once(" to auto-approve file edits") else {
        return false;
    };
    rest.is_empty() && is_key_token(token)
}

fn is_key_token(token: &str) -> bool {
    let inner = token
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(token);
    if inner.is_empty() || token.len() > 20 {
        return false;
    }
    let mut parts = inner.split(['+', '-', '^']).filter(|p| !p.is_empty());
    let Some(first) = parts.next() else {
        return false;
    };
    std::iter::once(first).chain(parts).all(is_key_part)
}

fn is_key_part(part: &str) -> bool {
    // A letter, a digit, or a punctuation key such as `ctrl+]`'s.
    if part.len() == 1 {
        return true;
    }
    if let Some(digits) = part.strip_prefix('f').or_else(|| part.strip_prefix('F'))
        && let Ok(n) = digits.parse::<u8>()
        && (1..=24).contains(&n)
    {
        return true;
    }
    matches!(
        part.to_ascii_lowercase().as_str(),
        "esc"
            | "escape"
            | "enter"
            | "return"
            | "tab"
            | "space"
            | "spc"
            | "backspace"
            | "bs"
            | "del"
            | "delete"
            | "ins"
            | "insert"
            | "up"
            | "down"
            | "left"
            | "right"
            | "pgup"
            | "pgdn"
            | "pageup"
            | "pagedown"
            | "home"
            | "end"
            | "ctrl"
            | "control"
            | "alt"
            | "opt"
            | "option"
            | "shift"
            | "meta"
            | "cmd"
            | "super"
    )
}

fn is_workspace_footer(row: &str) -> bool {
    let trimmed = row.trim();
    !trimmed.starts_with(['>', '❯', '+', '-', '@', '●', '○']) && trimmed.contains("sandbox")
}

fn is_wrapped_cancel_tail(row: &str) -> bool {
    let trimmed = row.trim();
    !trimmed.starts_with(['>', '❯', '+', '-', '@', '●', '○', '─'])
        && !is_cancel_footer(row)
        && trimmed.contains('·')
}

fn is_col0(row: &str) -> bool {
    !row.is_empty() && !row.starts_with(char::is_whitespace)
}

fn is_turn_or_rule_row(row: &str) -> bool {
    is_box_rule(row) || is_turn_row(row)
}

fn is_turn_row(row: &str) -> bool {
    let t = row.trim_start();
    t.starts_with("● ") || t.starts_with("○ ")
}

/// agy's spinner row (`⣽  Running command...`) starts with a braille glyph.
pub(super) fn is_spinner_row(row: &str) -> bool {
    row.trim_start()
        .chars()
        .next()
        .is_some_and(|c| ('\u{2800}'..='\u{28ff}').contains(&c))
}

fn is_permission_header(row: &str) -> bool {
    is_col0(row)
        && matches!(
            row.trim(),
            "Command" | "File" | "Directory" | "URL" | "MCP Tool" | "Permission" | "Edit"
        )
}

fn is_edit_header(row: &str) -> bool {
    is_col0(row) && matches!(row.trim(), "Pending edit" | "Edit" | "File")
}

fn is_approval_question(trimmed: &str) -> bool {
    matches!(
        trimmed,
        "Run this command?"
            | "Allow access to this file?"
            | "Allow access to this URL?"
            | "Allow calling this tool?"
            | "Allow creation of this file?"
            | "Accept this file edit?"
            | "Grant requested permission?"
            | "Allow sandbox bypass for command execution?"
            | "Allow administrator elevation?"
            | "Do you want to proceed?"
    )
}

fn has_ordered_options(rows: &[&str], require_yes_no: bool) -> bool {
    let mut expected_num = 1usize;
    let mut first_is_yes = false;
    let mut last_is_no = false;
    for &row in rows {
        if let Some((num, rest)) = parse_option(row) {
            if num != expected_num {
                return false;
            }
            if num == 1 {
                first_is_yes = rest.starts_with("Yes");
            }
            last_is_no = rest.starts_with("No");
            expected_num += 1;
        } else if expected_num == 1 || !is_option_continuation(row) {
            return false;
        }
    }
    expected_num >= 3 && (!require_yes_no || (first_is_yes && last_is_no))
}

fn parse_option(row: &str) -> Option<(usize, &str)> {
    let trimmed = row.trim_start();
    let after_cursor = trimmed
        .strip_prefix('>')
        .or_else(|| trimmed.strip_prefix('❯'))
        .map_or(trimmed, str::trim_start);
    let (digits, rest) = after_cursor.split_once(". ")?;
    let num = digits.parse::<usize>().ok()?;
    Some((num, rest.trim_start()))
}

fn is_option_continuation(row: &str) -> bool {
    let trimmed = row.trim();
    !trimmed.is_empty()
        && parse_option(row).is_none()
        && !trimmed.starts_with(['●', '○', '─'])
        && !is_spinner_row(row)
        && !trimmed.starts_with("↑/↓")
        && !is_cancel_footer(row)
        && !is_approval_question(trimmed)
        && trimmed != "Requesting permission for:"
}
