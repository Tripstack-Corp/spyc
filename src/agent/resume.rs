//! Pure resume-flag parsing for the agent profiles.
//!
//! These helpers strip a CLI's resume/continue flags from a saved command
//! line (so session restore can re-derive a clean baseline) and parse
//! `gemini --list-sessions` output. They're pure string functions with no
//! `App`/state dependency — they live here, next to the `AgentProfile` impls
//! that call them, rather than in `crate::app` (MVU Stage 4: the agent layer
//! shouldn't reach back up into `app` for its own behavior).

/// Strip `--resume <token>` from a command line. Used to derive a
/// fresh-session fallback when an automatic resume fails — we want to
/// preserve any other flags the user had on their original `claude`
/// invocation but drop the resume itself so the fallback doesn't fail
/// for the same reason.
pub fn command_without_resume(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(parts.len());
    let mut skip_next = false;
    for p in parts {
        if skip_next {
            skip_next = false;
            continue;
        }
        if p == "--resume" {
            skip_next = true;
            continue;
        }
        out.push(p);
    }
    let stripped = out.join(" ");
    if stripped.is_empty() {
        "claude".to_string()
    } else {
        stripped
    }
}

/// Strip codex's `resume [...args]` subcommand and any of its flags
/// from a command line, leaving the bare `codex` invocation. Used at
/// session-save time so a saved tab restores cleanly even if the
/// user had explicitly typed `codex resume <UUID>`. Mirrors
/// `command_without_resume` for claude. The id we'll resume to is
/// stored separately in `agent_session_id`.
pub fn command_without_codex_resume(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(parts.len());
    let mut hit_resume = false;
    for p in parts {
        if !hit_resume && p == "resume" {
            // Drop "resume" and everything after it — typically a UUID
            // and/or `--last`/`--all`/`--include-non-interactive` flags
            // that only make sense with `resume`.
            hit_resume = true;
            continue;
        }
        if hit_resume {
            continue;
        }
        out.push(p);
    }
    let stripped = out.join(" ");
    if stripped.is_empty() {
        "codex".to_string()
    } else {
        stripped
    }
}

/// Parse `gemini --list-sessions` stdout for the line whose
/// bracketed UUID matches `uuid`, returning the leading `<n>.`
/// index. The expected format is:
///
/// ```text
/// Available sessions for this project (2):
///   1. let's do a code review of this app (1 day ago) [76422c62-...-d149]
///   2. Analyze project for bugs and provide recommendations. (...) [4a7cd126-...-7544]
/// ```
///
/// Pure helper so the parser has unit tests; the IO side
/// (`gemini_resume_index_for`) just spawns the process and feeds
/// stdout in.
pub fn parse_gemini_list_sessions_for_uuid(text: &str, uuid: &str) -> Option<u32> {
    for line in text.lines() {
        let trimmed = line.trim_start();
        let Some((idx_str, rest)) = trimmed.split_once('.') else {
            continue;
        };
        let Ok(idx) = idx_str.trim().parse::<u32>() else {
            continue;
        };
        let Some(open) = rest.rfind('[') else {
            continue;
        };
        let Some(close) = rest.rfind(']') else {
            continue;
        };
        if open >= close {
            continue;
        }
        if rest[open + 1..close].eq_ignore_ascii_case(uuid) {
            return Some(idx);
        }
    }
    None
}

/// Strip Gemini's `--resume <id>` (or `-r <id>`) and `--session-id
/// <UUID>` flags from a command line, leaving a clean baseline that
/// session restore can re-decorate. The resume index is unstable
/// across runs (it's just a position in `--list-sessions` output) so
/// we always recompute it at restore time from the saved UUID; baking
/// the old index into the saved command would silently resume the
/// wrong conversation.
pub fn command_without_gemini_resume(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(parts.len());
    let mut skip_next = false;
    for p in parts {
        if skip_next {
            skip_next = false;
            continue;
        }
        if p == "--resume" || p == "-r" || p == "--session-id" {
            skip_next = true;
            continue;
        }
        if let Some(_value) = p.strip_prefix("--resume=") {
            continue;
        }
        if let Some(_value) = p.strip_prefix("--session-id=") {
            continue;
        }
        out.push(p);
    }
    let stripped = out.join(" ");
    if stripped.is_empty() {
        "gemini".to_string()
    } else {
        stripped
    }
}

/// Strip Antigravity's `--conversation <UUID>`, `-c <UUID>`, and `--continue` flags from a command line.
pub fn command_without_agy_resume(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(parts.len());
    let mut skip_next = false;
    for p in parts {
        if skip_next {
            skip_next = false;
            continue;
        }
        if p == "--conversation" || p == "-c" {
            skip_next = true;
            continue;
        }
        if p == "--continue" {
            continue;
        }
        if let Some(_value) = p.strip_prefix("--conversation=") {
            continue;
        }
        if let Some(_value) = p.strip_prefix("-c=") {
            continue;
        }
        out.push(p);
    }
    let stripped = out.join(" ");
    if stripped.is_empty() {
        "agy".to_string()
    } else {
        stripped
    }
}

#[cfg(test)]
mod gemini_helpers_tests {
    use super::{command_without_gemini_resume, parse_gemini_list_sessions_for_uuid};

    // ── command_without_gemini_resume ─────────────────────────────

    #[test]
    fn strips_long_resume_with_value() {
        assert_eq!(command_without_gemini_resume("gemini --resume 5"), "gemini");
    }

    #[test]
    fn strips_short_resume_with_value() {
        assert_eq!(command_without_gemini_resume("gemini -r latest"), "gemini");
    }

    #[test]
    fn strips_resume_with_equals_form() {
        assert_eq!(command_without_gemini_resume("gemini --resume=3"), "gemini");
    }

    #[test]
    fn strips_session_id_flag() {
        assert_eq!(
            command_without_gemini_resume(
                "gemini --session-id 11111111-1111-1111-1111-111111111111"
            ),
            "gemini"
        );
    }

    #[test]
    fn preserves_unrelated_flags() {
        assert_eq!(
            command_without_gemini_resume("gemini -y --model flash --resume 2"),
            "gemini -y --model flash"
        );
    }

    #[test]
    fn empty_input_falls_back_to_gemini() {
        assert_eq!(command_without_gemini_resume(""), "gemini");
    }

    // ── parse_gemini_list_sessions_for_uuid ────────────────────────

    #[test]
    fn parses_real_world_listing() {
        let stdout = "Available sessions for this project (2):
  1. let's do a code review of this app (1 day ago) [76422c62-ea2f-4334-8e3d-45fba862d149]
  2. Analyze project for bugs and provide recommendations. (1 day ago) [4a7cd126-f849-47c2-8035-80a07c807544]
The 'metricReader' option is deprecated. Please use 'metricReaders' instead.
";
        assert_eq!(
            parse_gemini_list_sessions_for_uuid(stdout, "76422c62-ea2f-4334-8e3d-45fba862d149"),
            Some(1)
        );
        assert_eq!(
            parse_gemini_list_sessions_for_uuid(stdout, "4a7cd126-f849-47c2-8035-80a07c807544"),
            Some(2)
        );
    }

    #[test]
    fn returns_none_for_unknown_uuid() {
        let stdout = "  1. only session [11111111-1111-1111-1111-111111111111]\n";
        assert!(
            parse_gemini_list_sessions_for_uuid(stdout, "22222222-2222-2222-2222-222222222222")
                .is_none()
        );
    }

    #[test]
    fn matches_uuid_case_insensitively() {
        // Defensive: gemini emits lowercase but match either way.
        let stdout = "  3. example [76422C62-EA2F-4334-8E3D-45FBA862D149]\n";
        assert_eq!(
            parse_gemini_list_sessions_for_uuid(stdout, "76422c62-ea2f-4334-8e3d-45fba862d149"),
            Some(3)
        );
    }

    #[test]
    fn skips_lines_without_brackets_or_index() {
        // The header / trailing deprecation warning must not derail
        // the per-line parse.
        let stdout = "Available sessions for this project (1):\n  1. example (1 day ago) [11111111-1111-1111-1111-111111111111]\nThe 'metricReader' option is deprecated.\n";
        assert_eq!(
            parse_gemini_list_sessions_for_uuid(stdout, "11111111-1111-1111-1111-111111111111"),
            Some(1)
        );
    }

    #[test]
    fn rejects_malformed_index() {
        // If the leading token can't parse as a number we just skip
        // the line, never returning the wrong index.
        let stdout = "  X. malformed [11111111-1111-1111-1111-111111111111]\n";
        assert!(
            parse_gemini_list_sessions_for_uuid(stdout, "11111111-1111-1111-1111-111111111111")
                .is_none()
        );
    }
}

#[cfg(test)]
mod agy_helpers_tests {
    use super::command_without_agy_resume;

    #[test]
    fn strips_conversation_with_value() {
        assert_eq!(
            command_without_agy_resume("agy --conversation 11111111-1111-1111-1111-111111111111"),
            "agy"
        );
    }

    #[test]
    fn strips_c_with_value() {
        assert_eq!(
            command_without_agy_resume("agy -c 11111111-1111-1111-1111-111111111111"),
            "agy"
        );
    }

    #[test]
    fn strips_conversation_equals_value() {
        assert_eq!(
            command_without_agy_resume("agy --conversation=11111111-1111-1111-1111-111111111111"),
            "agy"
        );
    }

    #[test]
    fn strips_c_equals_value() {
        assert_eq!(
            command_without_agy_resume("agy -c=11111111-1111-1111-1111-111111111111"),
            "agy"
        );
    }

    #[test]
    fn strips_continue_flag() {
        assert_eq!(command_without_agy_resume("agy --continue"), "agy");
    }

    #[test]
    fn preserves_unrelated_flags() {
        assert_eq!(
            command_without_agy_resume("agy --print \"hello\" --continue"),
            "agy --print \"hello\""
        );
    }

    #[test]
    fn empty_input_falls_back_to_agy() {
        assert_eq!(command_without_agy_resume(""), "agy");
    }
}
