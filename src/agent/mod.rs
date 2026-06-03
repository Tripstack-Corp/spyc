//! Agent profile registry.
//!
//! Each AI coding agent spyc can host in the pane (claude/codex/gemini/
//! agy) is described by an [`AgentProfile`] impl. The registry plus
//! [`detect`] / [`profile_for`] replace what used to be ~10
//! `match AgentKind` dispatch sites scattered across `app`, `state`,
//! and `config`. Adding an agent is a new impl + one `REGISTRY` entry —
//! no match-arm edits.
//!
//! [`AgentKind`] (in `state::sessions`) stays the *persistence* tag
//! serialized into saved sessions; profiles carry *behavior*. The two
//! meet at [`profile_for`] (kind → profile, for restored tabs) and
//! [`detect`] (command → profile, for live panes).

pub mod resume;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ratatui::text::Line;

use crate::pane::Pane;
use crate::state::sessions::{AgentKind, SessionCandidate};
use crate::ui::theme::Theme;

/// How a restored pane re-establishes its conversation.
pub enum ResumeAction {
    /// Resume is baked into the spawned command (codex/gemini/agy) or
    /// there's nothing to resume (Other).
    None,
    /// Claude: spawn fresh, then type `/resume <sid>` into stdin once
    /// the banner settles (the `--resume` CLI flag has a mount-crash
    /// regression). The event loop arms `pending_resume_send`.
    ClaudeStdin { session_id: String },
}

/// Reconstructed restore command for a saved tab.
pub struct RestorePlan {
    pub command: String,
    pub resume: ResumeAction,
}

/// How an agent contributes to the on-quit exit-summary line.
pub enum ExitSummaryMode {
    /// No summary line (gemini / Other).
    None,
    /// List human-readable session names (claude).
    Names,
    /// Count tabs that captured a session id (codex / agy).
    Count,
}

/// Describes an agent's on-disk transcript view for `^a v`.
pub struct TranscriptSpec {
    /// Locate the transcript file for the pane's (cwd, spawn time).
    pub resolve: fn(&Path, u64) -> Option<PathBuf>,
    /// Render that file into pager lines.
    pub render: fn(&Path, &Theme) -> Vec<Line<'static>>,
    /// Config key gating the view; `None` = always-on (codex).
    pub config_key: Option<&'static str>,
    /// Default when the config key is unset.
    pub default_enabled: bool,
    /// When no transcript is found: `Some(msg)` flashes `msg` and stops
    /// (codex — its history isn't in the terminal grid); `None` falls
    /// through to vt100 terminal capture (claude / agy).
    pub miss_message: Option<&'static str>,
}

/// Per-agent behavior. Default methods express "this agent doesn't do
/// X" — an agent without a capability simply doesn't override.
pub trait AgentProfile: Sync {
    fn kind(&self) -> AgentKind;
    fn name(&self) -> &'static str;
    fn binary(&self) -> &'static str;

    /// True if `cmd`'s first token is this agent's binary, bare or
    /// path-qualified (`agy`, `/usr/local/bin/agy`). Equivalent to the
    /// old `is_<agent>_command`.
    fn matches_command(&self, cmd: &str) -> bool {
        let first = cmd.split_whitespace().next().unwrap_or("");
        first.rsplit('/').next() == Some(self.binary())
    }

    /// SAVE: resolve `(session_id, session_name)` to persist. Default:
    /// nothing to resume.
    fn resolve_resume_target(
        &self,
        _pane: &Pane,
        _cwd: &Path,
        _spawn_epoch_secs: u64,
        _claimed: &HashSet<String>,
    ) -> (Option<String>, Option<String>) {
        (None, None)
    }

    /// SAVE: strip resume flag(s) so the saved baseline restores
    /// cleanly. Default: identity.
    fn command_without_resume(&self, cmd: &str) -> String {
        cmd.to_string()
    }

    /// RESTORE: reconstruct the spawn command and how to resume.
    /// Default: spawn the saved command verbatim, nothing to resume.
    fn reconstruct_restore(&self, cmd: &str, _sid: Option<&str>, _cwd: &Path) -> RestorePlan {
        RestorePlan {
            command: cmd.to_string(),
            resume: ResumeAction::None,
        }
    }

    /// Status-bar short id for the active pane. Default: none.
    fn resolve_short_id(&self, _cwd: &Path, _spawn_epoch_secs: u64) -> Option<String> {
        None
    }

    /// Session-picker label. Default: `name:short`.
    fn picker_label(&self, short_id: &str, _session_name: Option<&str>) -> String {
        format!("{}:{short_id}", self.name())
    }

    /// On-quit exit-summary contribution. Default: none.
    fn exit_summary_mode(&self) -> ExitSummaryMode {
        ExitSummaryMode::None
    }

    /// Transcript scrollback spec, if any. Default: none (gemini).
    fn transcript(&self) -> Option<TranscriptSpec> {
        None
    }
}

/// Shared helper: pick the session whose start time is closest to the
/// pane's spawn time and return its short id. Mirrors the old
/// `resolve_active_session_short_id` body.
fn closest_short_id<T: SessionCandidate>(
    candidates: Vec<T>,
    spawn_epoch_secs: u64,
) -> Option<String> {
    candidates
        .into_iter()
        .min_by_key(|c| c.started_at_secs().abs_diff(spawn_epoch_secs))
        .map(|c| crate::state::sessions::short_id(c.session_id()))
}

// ── Profiles ──────────────────────────────────────────────────────────

pub struct ClaudeProfile;
impl AgentProfile for ClaudeProfile {
    fn kind(&self) -> AgentKind {
        AgentKind::Claude
    }
    fn name(&self) -> &'static str {
        "claude"
    }
    fn binary(&self) -> &'static str {
        "claude"
    }
    fn resolve_resume_target(
        &self,
        pane: &Pane,
        cwd: &Path,
        spawn_epoch_secs: u64,
        claimed: &HashSet<String>,
    ) -> (Option<String>, Option<String>) {
        resume::resolve_claude_resume_target(pane, cwd, spawn_epoch_secs, claimed)
    }
    fn command_without_resume(&self, cmd: &str) -> String {
        resume::command_without_resume(cmd)
    }
    fn reconstruct_restore(&self, cmd: &str, sid: Option<&str>, _cwd: &Path) -> RestorePlan {
        // Claude always spawns fresh; the `/resume <sid>` stdin dance is
        // armed by the event loop when a session id is present.
        RestorePlan {
            command: resume::command_without_resume(cmd),
            resume: match sid {
                Some(s) => ResumeAction::ClaudeStdin {
                    session_id: s.to_string(),
                },
                None => ResumeAction::None,
            },
        }
    }
    fn resolve_short_id(&self, cwd: &Path, spawn_epoch_secs: u64) -> Option<String> {
        closest_short_id(
            crate::state::sessions::find_claude_sessions(cwd),
            spawn_epoch_secs,
        )
    }
    fn picker_label(&self, short_id: &str, session_name: Option<&str>) -> String {
        match session_name {
            Some(name) => format!("claude:{name} ({short_id})"),
            None => format!("claude:{short_id}"),
        }
    }
    fn exit_summary_mode(&self) -> ExitSummaryMode {
        ExitSummaryMode::Names
    }
    fn transcript(&self) -> Option<TranscriptSpec> {
        Some(TranscriptSpec {
            resolve: crate::state::claude_transcript::resolve_active_jsonl,
            render: crate::state::claude_transcript::render_transcript,
            config_key: Some("claude_transcript_scrollback"),
            default_enabled: false,
            miss_message: None,
        })
    }
}

pub struct CodexProfile;
impl AgentProfile for CodexProfile {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }
    fn name(&self) -> &'static str {
        "codex"
    }
    fn binary(&self) -> &'static str {
        "codex"
    }
    fn resolve_resume_target(
        &self,
        pane: &Pane,
        _cwd: &Path,
        _spawn_epoch_secs: u64,
        claimed: &HashSet<String>,
    ) -> (Option<String>, Option<String>) {
        let lines = pane.recent_lines(200);
        let id = crate::state::sessions::extract_codex_resume_token(&lines)
            .filter(|tok| !claimed.contains(tok));
        (id, None)
    }
    fn command_without_resume(&self, cmd: &str) -> String {
        resume::command_without_codex_resume(cmd)
    }
    fn reconstruct_restore(&self, cmd: &str, sid: Option<&str>, _cwd: &Path) -> RestorePlan {
        let base = resume::command_without_codex_resume(cmd);
        let command = match sid {
            Some(s) => format!("{base} resume {s}"),
            None => format!("{base} resume --last"),
        };
        RestorePlan {
            command,
            resume: ResumeAction::None,
        }
    }
    fn exit_summary_mode(&self) -> ExitSummaryMode {
        ExitSummaryMode::Count
    }
    fn transcript(&self) -> Option<TranscriptSpec> {
        Some(TranscriptSpec {
            resolve: crate::state::codex_transcript::resolve_active_rollout,
            render: crate::state::codex_transcript::render_transcript,
            config_key: None,
            default_enabled: true,
            miss_message: Some("codex: no transcript on disk yet for this session"),
        })
    }
}

pub struct GeminiProfile;
impl AgentProfile for GeminiProfile {
    fn kind(&self) -> AgentKind {
        AgentKind::Gemini
    }
    fn name(&self) -> &'static str {
        "gemini"
    }
    fn binary(&self) -> &'static str {
        "gemini"
    }
    fn resolve_resume_target(
        &self,
        _pane: &Pane,
        cwd: &Path,
        spawn_epoch_secs: u64,
        claimed: &HashSet<String>,
    ) -> (Option<String>, Option<String>) {
        resume::resolve_gemini_resume_target(cwd, spawn_epoch_secs, claimed)
    }
    fn command_without_resume(&self, cmd: &str) -> String {
        resume::command_without_gemini_resume(cmd)
    }
    fn reconstruct_restore(&self, cmd: &str, sid: Option<&str>, cwd: &Path) -> RestorePlan {
        let base = resume::command_without_gemini_resume(cmd);
        // Gemini's `--resume` consumes an *index* into `--list-sessions`,
        // not a UUID; recompute it synchronously. Fall back to bare on
        // lookup failure (binary missing, session pruned, format drift).
        let command = match sid {
            Some(uuid) => match resume::gemini_resume_index_for(cwd, uuid) {
                Some(idx) => format!("{base} --resume {idx}"),
                None => base,
            },
            None => base,
        };
        RestorePlan {
            command,
            resume: ResumeAction::None,
        }
    }
    fn resolve_short_id(&self, cwd: &Path, spawn_epoch_secs: u64) -> Option<String> {
        closest_short_id(
            crate::state::sessions::find_gemini_sessions(cwd),
            spawn_epoch_secs,
        )
    }
    // exit_summary_mode: None (gemini is omitted from the summary).
    // transcript: None (gemini has no transcript renderer).
}

pub struct AgyProfile;
impl AgentProfile for AgyProfile {
    fn kind(&self) -> AgentKind {
        AgentKind::Agy
    }
    fn name(&self) -> &'static str {
        "agy"
    }
    fn binary(&self) -> &'static str {
        "agy"
    }
    fn resolve_resume_target(
        &self,
        pane: &Pane,
        _cwd: &Path,
        _spawn_epoch_secs: u64,
        claimed: &HashSet<String>,
    ) -> (Option<String>, Option<String>) {
        let lines = pane.recent_lines(200);
        let id = crate::state::sessions::extract_agy_resume_token(&lines)
            .filter(|tok| !claimed.contains(tok));
        (id, None)
    }
    fn command_without_resume(&self, cmd: &str) -> String {
        resume::command_without_agy_resume(cmd)
    }
    fn reconstruct_restore(&self, cmd: &str, sid: Option<&str>, _cwd: &Path) -> RestorePlan {
        let base = resume::command_without_agy_resume(cmd);
        let command = match sid {
            Some(s) => format!("{base} --conversation {s}"),
            None => format!("{base} --continue"),
        };
        RestorePlan {
            command,
            resume: ResumeAction::None,
        }
    }
    fn resolve_short_id(&self, cwd: &Path, spawn_epoch_secs: u64) -> Option<String> {
        closest_short_id(
            crate::state::sessions::find_agy_sessions(cwd),
            spawn_epoch_secs,
        )
    }
    fn exit_summary_mode(&self) -> ExitSummaryMode {
        ExitSummaryMode::Count
    }
    fn transcript(&self) -> Option<TranscriptSpec> {
        Some(TranscriptSpec {
            resolve: crate::state::agy_transcript::resolve_active_jsonl,
            render: crate::state::agy_transcript::render_transcript,
            config_key: Some("agy_transcript_scrollback"),
            default_enabled: true,
            miss_message: None,
        })
    }
}

/// Strip zot's resume flags so a saved baseline restores cleanly:
/// `-c`/`--continue` and `-r`/`--resume` (no-arg) plus `--session
/// <path>` / `--session=<path>` (a specific session file). Restore
/// re-decorates with `--continue`.
fn command_without_zot_resume(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(parts.len());
    let mut skip_next = false;
    for p in parts {
        if skip_next {
            skip_next = false;
            continue;
        }
        match p {
            "-c" | "--continue" | "-r" | "--resume" => {}
            "--session" => skip_next = true,
            _ if p.starts_with("--session=") => {}
            _ => out.push(p),
        }
    }
    let stripped = out.join(" ");
    if stripped.is_empty() {
        "zot".to_string()
    } else {
        stripped
    }
}

pub struct ZotProfile;
impl AgentProfile for ZotProfile {
    fn kind(&self) -> AgentKind {
        AgentKind::Zot
    }
    fn name(&self) -> &'static str {
        "zot"
    }
    fn binary(&self) -> &'static str {
        "zot"
    }
    fn command_without_resume(&self, cmd: &str) -> String {
        command_without_zot_resume(cmd)
    }
    fn reconstruct_restore(&self, cmd: &str, _sid: Option<&str>, _cwd: &Path) -> RestorePlan {
        // zot sessions are files under `$ZOT_HOME/sessions/<cwd-hash>/`;
        // `--continue` resumes the most recent one for this cwd (zot's
        // own resume-latest). We don't capture a specific session path
        // at save time yet, so restore always continues-most-recent —
        // same shape as codex `resume --last` / agy `--continue`.
        RestorePlan {
            command: format!("{} --continue", command_without_zot_resume(cmd)),
            resume: ResumeAction::None,
        }
    }
    // No transcript / short-id / save-target yet: zot's session-file
    // layout (`<cwd-hash>` scheme) and JSONL schema need a real session
    // on disk to implement faithfully. Follow-up: add `zot_transcript`
    // + flip `transcript()` to `Some`, and capture the active session
    // path for `--session`-based specific resume.
}

/// The no-op profile for `bash`/`vim`/anything unrecognized. Not in
/// `REGISTRY`; it's the `detect` / `profile_for` fallback, reproducing
/// `AgentKind::Other` (no resume, identity strip, no transcript).
pub struct OtherProfile;
impl AgentProfile for OtherProfile {
    fn kind(&self) -> AgentKind {
        AgentKind::Other
    }
    fn name(&self) -> &'static str {
        ""
    }
    fn binary(&self) -> &'static str {
        ""
    }
    fn matches_command(&self, _cmd: &str) -> bool {
        false
    }
}

// ── Registry ──────────────────────────────────────────────────────────

static CLAUDE: ClaudeProfile = ClaudeProfile;
static CODEX: CodexProfile = CodexProfile;
static GEMINI: GeminiProfile = GeminiProfile;
static AGY: AgyProfile = AgyProfile;
static ZOT: ZotProfile = ZotProfile;
static OTHER: OtherProfile = OtherProfile;

/// All real agents, in detection-precedence order. Binaries don't
/// overlap, so order is not load-bearing — but keep it stable.
pub static REGISTRY: &[&dyn AgentProfile] = &[&CLAUDE, &CODEX, &GEMINI, &AGY, &ZOT];

/// Profile for a persisted [`AgentKind`] (restored tabs, exit summary,
/// picker). Returns the no-op [`OtherProfile`] for `Other`.
pub fn profile_for(kind: AgentKind) -> &'static dyn AgentProfile {
    REGISTRY
        .iter()
        .copied()
        .find(|p| p.kind() == kind)
        .unwrap_or(&OTHER)
}

/// Profile for a live command line (detection). Returns the no-op
/// [`OtherProfile`] when nothing matches.
pub fn detect(cmd: &str) -> &'static dyn AgentProfile {
    REGISTRY
        .iter()
        .copied()
        .find(|p| p.matches_command(cmd))
        .unwrap_or(&OTHER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_agents_and_other() {
        assert_eq!(detect("claude").kind(), AgentKind::Claude);
        assert_eq!(
            detect("/usr/local/bin/codex resume").kind(),
            AgentKind::Codex
        );
        assert_eq!(detect("gemini mcp").kind(), AgentKind::Gemini);
        assert_eq!(detect("agy --continue").kind(), AgentKind::Agy);
        assert_eq!(detect("zot").kind(), AgentKind::Zot);
        assert_eq!(detect("/opt/bin/zot -c").kind(), AgentKind::Zot);
        assert_eq!(detect("bash -lc 'make'").kind(), AgentKind::Other);
        assert_eq!(detect("").kind(), AgentKind::Other);
    }

    #[test]
    fn zot_strips_resume_flags() {
        assert_eq!(command_without_zot_resume("zot -c"), "zot");
        assert_eq!(command_without_zot_resume("zot --continue"), "zot");
        assert_eq!(command_without_zot_resume("zot -r"), "zot");
        assert_eq!(command_without_zot_resume("zot --resume"), "zot");
        assert_eq!(
            command_without_zot_resume("zot --session /tmp/a/s.jsonl"),
            "zot"
        );
        assert_eq!(
            command_without_zot_resume("zot --session=/tmp/s.jsonl"),
            "zot"
        );
        assert_eq!(command_without_zot_resume(""), "zot");
    }

    #[test]
    fn zot_strip_preserves_unrelated_flags() {
        assert_eq!(
            command_without_zot_resume("zot --model gpt-5 --continue"),
            "zot --model gpt-5"
        );
    }

    #[test]
    fn zot_restore_continues_most_recent() {
        let plan =
            ZotProfile.reconstruct_restore("zot --session /tmp/x.jsonl", None, Path::new("/tmp"));
        assert_eq!(plan.command, "zot --continue");
        assert!(matches!(plan.resume, ResumeAction::None));
    }

    // ── reconstruct_restore per agent (session restore) ───────────────

    /// Claude spawns fresh (strips any baked `--resume`) and arms the
    /// `/resume <sid>` stdin dance when a session id is present.
    #[test]
    fn claude_restore_strips_resume_and_arms_stdin() {
        let cwd = Path::new("/tmp");
        let with_sid =
            ClaudeProfile.reconstruct_restore("claude --resume old-sid", Some("new-sid"), cwd);
        assert_eq!(with_sid.command, "claude");
        assert!(matches!(
            with_sid.resume,
            ResumeAction::ClaudeStdin { session_id } if session_id == "new-sid"
        ));

        let fresh = ClaudeProfile.reconstruct_restore("claude", None, cwd);
        assert_eq!(fresh.command, "claude");
        assert!(matches!(fresh.resume, ResumeAction::None));
    }

    /// Codex bakes resume into the command: `resume <UUID>` with an id,
    /// `resume --last` without one.
    #[test]
    fn codex_restore_bakes_resume_into_command() {
        let cwd = Path::new("/tmp");
        let with_sid = CodexProfile.reconstruct_restore("codex", Some("UUID-123"), cwd);
        assert_eq!(with_sid.command, "codex resume UUID-123");
        assert!(matches!(with_sid.resume, ResumeAction::None));

        // A stale baked `resume <old>` is stripped before re-baking.
        let none = CodexProfile.reconstruct_restore("codex resume old-uuid", None, cwd);
        assert_eq!(none.command, "codex resume --last");
    }

    /// Agy: `--conversation <sid>` with an id, `--continue` without.
    #[test]
    fn agy_restore_bakes_conversation_or_continues() {
        let cwd = Path::new("/tmp");
        assert_eq!(
            AgyProfile
                .reconstruct_restore("agy", Some("SID"), cwd)
                .command,
            "agy --conversation SID"
        );
        assert_eq!(
            AgyProfile.reconstruct_restore("agy", None, cwd).command,
            "agy --continue"
        );
    }

    /// Gemini with no recorded id restores the bare command (the
    /// `--resume <index>` lookup needs a live `gemini --list-sessions`,
    /// so it's exercised only when an id is present — kept out of unit
    /// tests to avoid spawning the CLI).
    #[test]
    fn gemini_restore_without_id_is_bare() {
        let plan = GeminiProfile.reconstruct_restore("gemini", None, Path::new("/tmp"));
        assert_eq!(plan.command, "gemini");
        assert!(matches!(plan.resume, ResumeAction::None));
    }

    /// Other (bash/vim/make): the saved command runs verbatim and any
    /// stray session id is ignored — no resume, no panic.
    #[test]
    fn other_restore_runs_verbatim_ignoring_sid() {
        let cwd = Path::new("/tmp");
        let plan = OtherProfile.reconstruct_restore("bash -lc 'make'", Some("ignored"), cwd);
        assert_eq!(plan.command, "bash -lc 'make'");
        assert!(matches!(plan.resume, ResumeAction::None));
    }
}
