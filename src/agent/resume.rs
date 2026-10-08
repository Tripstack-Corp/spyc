//! Pure resume-flag parsing for the agent profiles.
//!
//! These helpers strip a CLI's resume/continue flags from a saved command
//! line (so session restore can re-derive a clean baseline) and parse
//! `agy --list-sessions` output. They're pure string functions with no
//! `App`/state dependency — they live here, next to the `AgentProfile` impls
//! that call them, rather than in `crate::app` (MVU Stage 4: the agent layer
//! shouldn't reach back up into `app` for its own behaviour).

/// Strip claude's resume/continue flags from a command line. Used to derive a
/// fresh-session fallback when an automatic resume fails — we want to preserve
/// any other flags the user had on their original `claude` invocation but drop
/// the resume itself so the fallback doesn't fail for the same reason.
///
/// Handles all of claude's forms: `--resume`/`-r` (optional `[sessionId]`),
/// `--continue`/`-c` (no argument), and the `--fork-session` that turns either
/// into a branch — a saved fork resumes its own conversation. For
/// `--resume`/`-r` the following token is dropped **only** when it's an id, not
/// another flag — `claude --resume --verbose` must keep `--verbose` rather than
/// eating it.
pub fn command_without_resume(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(parts.len());
    let mut i = 0;
    while i < parts.len() {
        match parts[i] {
            "--resume" | "-r" => {
                // Drop an optional session-id argument too, but not a following
                // flag (a bare `--resume`/`-r` takes no id).
                if parts.get(i + 1).is_some_and(|n| !n.starts_with('-')) {
                    i += 1;
                }
            }
            "--continue" | "-c" | "--fork-session" => {} // no argument to drop
            other => out.push(other),
        }
        i += 1;
    }
    let stripped = out.join(" ");
    if stripped.is_empty() {
        "claude".to_string()
    } else {
        stripped
    }
}

/// Strip Codex's old conversation selector, preserving general options and
/// quoting. Unsupported commands remain verbatim for restore diagnostics.
pub fn command_without_codex_resume(cmd: &str) -> String {
    super::codex_command::baseline(cmd).unwrap_or_else(|_| cmd.to_string())
}

/// `^a F` for claude: a new session that starts from `sid`'s history. Goes
/// through the CLI flag, unlike restore's typed `/resume`, because the slash
/// command has no fork form.
pub fn claude_fork_command(cmd: &str, sid: &str) -> String {
    format!(
        "{} --resume {sid} --fork-session",
        command_without_resume(cmd)
    )
}

/// `^a F` for codex: `codex fork <uuid>` starts a new thread from `sid`'s.
pub fn codex_fork_command(cmd: &str, sid: &str) -> anyhow::Result<String> {
    super::codex_command::fork(cmd, sid)
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

/// Slack for the second-granularity clocks a pane's spawn time and claude's
/// own timestamps are compared on.
const PANE_CLOCK_SKEW_SECS: u64 = 5;

/// How long after its pane spawns claude can still be starting: the shell's
/// rc and claude's own startup, on a loaded machine.
const CLAUDE_STARTUP_SECS: u64 = 60;

/// Whether a claude process that started at `started` (epoch secs) can be the
/// one a pane spawned at `spawn` is running.
const fn started_with_pane(started: u64, spawn: u64) -> bool {
    started + PANE_CLOCK_SKEW_SECS >= spawn && started <= spawn + CLAUDE_STARTUP_SECS
}

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
/// 2. Walk `~/.claude/sessions/` records matching the cwd and started
///    around this pane's spawn, skip any already in `claimed`, and take
///    the one whose `startedAt` is closest: this pane's own claude. Its
///    conversation is the answer, or none if claude hasn't written it yet.
/// 3. Last-ditch, when no record matched: the most-recently-modified JSONL
///    in the project slug, if it was written since this pane started, isn't
///    already in `claimed`, and isn't a running claude's conversation.
///
/// Steps 2 and 3 used to consider only conversations already on disk, and
/// any age of one. Claude writes a conversation only with its first
/// message, so a pane saved before that took another pane's (#584).
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
                    let name = s::find_claude_session_name(&tok);
                    return (Some(tok), name);
                }
                // Banner UUID has no JSONL — fall through.
            } else if !tok.chars().any(char::is_control) {
                // Named sessions: claude resolves names itself, trust it —
                // but `tok` came from untrusted pane scrollback, so reject
                // control characters first. A spoofed banner with a newline
                // or ESC in the "token" would otherwise be typed verbatim as
                // `/resume <tok>` into the pane, injecting a second command
                // line or a terminal escape.
                return (Some(tok.clone()), Some(tok));
            }
        }

        // Step 2: this pane's own claude process, by spawn-time proximity.
        let records = s::find_claude_sessions(cwd);
        let running: std::collections::HashSet<String> =
            records.iter().map(|c| c.session_id.clone()).collect();
        let candidates: Vec<_> = records
            .into_iter()
            .filter(|c| started_with_pane(c.started_at_secs, pane_spawn_epoch_secs))
            .collect();
        if let Some(own) =
            s::pick_closest_unclaimed_session(candidates, pane_spawn_epoch_secs, claimed)
        {
            return if s::claude_jsonl_exists(cwd, &own.session_id) {
                (Some(own.session_id), own.name)
            } else {
                (None, None)
            };
        }

        // Step 3: final fallback. Unclaimed, so this pane never collapses
        // onto another pane's conversation; written since it started; and not
        // a running claude's, which step 2 would have matched were it ours.
        let since = pane_spawn_epoch_secs.saturating_sub(PANE_CLOCK_SKEW_SECS);
        if let Some(id) = s::most_recent_jsonl_for_cwd_since(cwd, since)
            && !claimed.contains(&id)
            && !running.contains(&id)
        {
            let name = s::find_claude_session_name(&id);
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

#[cfg(test)]
mod claude_resume_tests {
    use super::command_without_resume;

    #[test]
    fn strips_resume_and_session_id() {
        assert_eq!(command_without_resume("claude --resume abc123"), "claude");
        assert_eq!(command_without_resume("claude -r abc123"), "claude");
    }

    #[test]
    fn strips_continue_flags() {
        assert_eq!(command_without_resume("claude --continue"), "claude");
        assert_eq!(command_without_resume("claude -c"), "claude");
    }

    #[test]
    fn bare_resume_does_not_eat_following_flag() {
        // Regression: a bare `--resume`/`-r` (no id) must keep a trailing flag.
        assert_eq!(
            command_without_resume("claude --resume --verbose"),
            "claude --verbose"
        );
        assert_eq!(
            command_without_resume("claude -r --model opus"),
            "claude --model opus"
        );
    }

    #[test]
    fn preserves_unrelated_flags() {
        assert_eq!(
            command_without_resume("claude --model opus --resume abc"),
            "claude --model opus"
        );
    }

    #[test]
    fn bare_resume_at_end_drops_just_the_flag() {
        assert_eq!(command_without_resume("claude --resume"), "claude");
        assert_eq!(command_without_resume("claude -r"), "claude");
    }

    #[test]
    fn empty_input_falls_back_to_claude() {
        assert_eq!(command_without_resume(""), "claude");
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
