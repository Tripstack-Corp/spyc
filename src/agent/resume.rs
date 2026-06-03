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

// ── resume-target resolution ──────────────────────────────────────
// These do fs / subprocess work (not pure like the strippers above) but
// take no `App` state — they read a pane's scrollback, walk the agent's
// session dirs, or shell out, all via args. They lived as associated fns on
// `App` purely by inertia; they belong with the profiles that call them.

/// Resolve the `claude --resume <token>` target to use on session save.
///
/// Multi-pane safety: when several Claude tabs share a cwd, we
/// can't blindly use "most-recent JSONL for this cwd" — they'd
/// all save the same ID and collapse onto a single conversation
/// at restore. The caller threads `pane_spawn_epoch_secs` and a
/// `claimed` set; the resolver picks a unique session record per
/// pane by matching `startedAt` to the pane's spawn time.
///
/// Strategy, in order:
/// 1. Read the exit-banner token from pane scrollback. If it's a
///    UUID, verify a JSONL exists for it under
///    `~/.claude/projects/<slug>/`. Claude sometimes prints the
///    banner with a session ID it never persisted (e.g. user
///    `/clear`'d or `/resume`'d before exit), so an unconditional
///    trust leads to "No conversation found …" on restore. The
///    banner is unambiguously this pane, so it bypasses `claimed`.
/// 2. Walk `~/.claude/sessions/` records matching the cwd, skip
///    any already in `claimed`, pick the one whose `startedAt` is
///    closest to this pane's spawn time, verify JSONL on disk.
/// 3. Last-ditch: most-recently-modified JSONL in the project
///    slug, but only if it isn't already in `claimed`. Without
///    the claimed-check this is what was producing the bug.
pub fn resolve_claude_resume_target(
    pane: &crate::pane::Pane,
    cwd: &std::path::Path,
    pane_spawn_epoch_secs: u64,
    claimed: &std::collections::HashSet<String>,
) -> (Option<String>, Option<String>) {
    use crate::state::sessions as s;

    let resolved: (Option<String>, Option<String>) = (|| {
        let banner_lines = pane.recent_lines(200);
        if let Some(tok) = s::extract_claude_resume_token(&banner_lines) {
            if s::is_uuid(&tok) {
                if s::claude_jsonl_exists(cwd, &tok) {
                    let name = s::find_claude_session_name_public(&tok);
                    return (Some(tok), name);
                }
                // Banner UUID has no JSONL — fall through.
            } else {
                // Named sessions: claude resolves names itself, trust it.
                return (Some(tok.clone()), Some(tok));
            }
        }

        // Step 2: pick the per-pane match by spawn-time proximity.
        // Filter to JSONL-on-disk first so the picker only sees
        // resumable candidates.
        let candidates: Vec<_> = s::find_claude_sessions(cwd)
            .into_iter()
            .filter(|c| s::claude_jsonl_exists(cwd, &c.session_id))
            .collect();
        if let Some(c) =
            s::pick_closest_unclaimed_session(candidates, pane_spawn_epoch_secs, claimed)
        {
            return (Some(c.session_id), c.name);
        }

        // Step 3: final fallback. Most-recent JSONL — but only if
        // unclaimed; otherwise leave this pane unresumable rather
        // than collapse it onto another pane's conversation.
        if let Some(id) = s::most_recent_jsonl_for_cwd(cwd)
            && !claimed.contains(&id)
        {
            let name = s::find_claude_session_name_public(&id);
            return (Some(id), name);
        }
        (None, None)
    })();

    if let (Some(id), _) = &resolved
        && s::is_uuid(id)
        && !s::claude_jsonl_exists(cwd, id)
    {
        crate::spyc_debug!(
            "resolve_claude_resume_target: dropping ghost id {} (no JSONL under {})",
            id,
            cwd.display()
        );
        return (None, None);
    }
    resolved
}

/// Resolve the Gemini resume target to save for a pane.
///
/// Gemini's CLI doesn't print an exit banner with a resume token,
/// so we pull the candidate set from
/// `~/.gemini/tmp/<project>/chats/*.jsonl` (each file's first line
/// is JSON metadata with `sessionId` and `startTime`) and pick the
/// unclaimed record whose start time is closest to this pane's
/// `spawn_epoch_secs`. Multi-pane safety: the `claimed` set
/// prevents two panes in the same project from collapsing onto
/// one conversation. Returns the UUID; Gemini doesn't expose a
/// human-readable session name from the CLI, so the second slot
/// is always `None`.
pub fn resolve_gemini_resume_target(
    cwd: &std::path::Path,
    pane_spawn_epoch_secs: u64,
    claimed: &std::collections::HashSet<String>,
) -> (Option<String>, Option<String>) {
    use crate::state::sessions as s;
    let candidates = s::find_gemini_sessions(cwd);
    s::pick_closest_unclaimed_session(candidates, pane_spawn_epoch_secs, claimed)
        .map_or((None, None), |c| (Some(c.session_id), None))
}

/// At restore time, translate a saved Gemini session UUID into
/// the index `gemini --resume <N>` consumes. Runs `gemini
/// --list-sessions` synchronously in `cwd` and delegates parsing
/// to `parse_gemini_list_sessions_for_uuid`. Returns `None` when
/// the binary errors, the UUID isn't in the listing, or the
/// output format drifts. Failure is recoverable: the caller falls
/// back to spawning `gemini` bare and lets the user pick.
pub fn gemini_resume_index_for(cwd: &std::path::Path, uuid: &str) -> Option<u32> {
    let out = std::process::Command::new("gemini")
        .arg("--list-sessions")
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&out.stdout).ok()?;
    parse_gemini_list_sessions_for_uuid(text, uuid)
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
