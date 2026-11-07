//! Claude Code's [`AgentProfile`]: it resumes by typing `/resume` into a fresh
//! claude, and finds its conversations under the tab's Claude home.

use std::collections::HashSet;
use std::path::Path;

use super::{
    AgentProfile, ExitSummaryMode, ForkMode, RestorePlan, ResumeAction, StatusHookSupport,
    TranscriptImageSpec, TranscriptSpec, chrome, closest_short_id, resume,
};
use crate::pane::Pane;
use crate::state::sessions::AgentKind;

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
        claude_home: Option<&Path>,
    ) -> (Option<String>, Option<String>) {
        claude_home.map_or((None, None), |home| {
            resume::resolve_claude_resume_target(pane, cwd, spawn_epoch_secs, claimed, home)
        })
    }
    /// Claude's pin is taken as-is. The hook names the conversation this tab
    /// runs from claude's startup, but the transcript appears only with the
    /// first message: requiring it here sent an unwritten pin to the resolver,
    /// which saved another tab's conversation (#584). Restore checks for the
    /// transcript instead, before typing `/resume`.
    fn validate_live_session_id(
        &self,
        _cwd: &Path,
        id: &str,
        claude_home: Option<&Path>,
    ) -> Option<(String, Option<String>)> {
        Some((
            id.to_string(),
            claude_home.and_then(|home| crate::state::sessions::find_claude_session_name(home, id)),
        ))
    }
    fn command_without_resume(&self, cmd: &str) -> String {
        resume::command_without_resume(cmd)
    }
    fn reconstruct_restore(
        &self,
        cmd: &str,
        sid: Option<&str>,
        cwd: &Path,
        claude_home: Option<&Path>,
    ) -> RestorePlan {
        // Claude always spawns fresh; the `/resume <sid>` stdin dance is
        // armed by the event loop when a session id is present.
        RestorePlan {
            command: resume::command_without_resume(cmd),
            resume: match sid {
                // Saved before its first message, so there is nothing to
                // resume. A name can't be checked; claude resolves it.
                Some(s)
                    if crate::state::sessions::is_uuid(s)
                        && !claude_home.is_some_and(|home| {
                            crate::state::sessions::claude_jsonl_exists(home, cwd, s)
                        }) =>
                {
                    ResumeAction::None
                }
                Some(s) => ResumeAction::ClaudeStdin {
                    session_id: s.to_string(),
                },
                None => ResumeAction::None,
            },
        }
    }
    fn fork_mode(&self) -> ForkMode {
        ForkMode::Branch(|cmd, sid| Ok(resume::claude_fork_command(cmd, sid)))
    }
    fn resolve_short_id(
        &self,
        cwd: &Path,
        spawn_epoch_secs: u64,
        claude_home: Option<&Path>,
    ) -> Option<String> {
        closest_short_id(
            crate::state::sessions::find_claude_sessions(claude_home?, cwd),
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
            render: crate::state::claude_transcript::render_transcript_doc,
            config_key: Some("claude_transcript_scrollback"),
            default_enabled: false,
            miss_message: None,
        })
    }
    fn transcript_images(&self) -> Option<TranscriptImageSpec> {
        Some(TranscriptImageSpec {
            resolve: crate::state::claude_transcript::resolve_active_jsonl,
            format: crate::state::transcript_images::Format::Claude,
        })
    }
    /// Claude Code pastes an image with `^v`, not the terminal's own paste
    /// (`Cmd+V`/`Ctrl+Shift+V` go through the terminal, which can only carry
    /// text). Same key on macOS and Linux.
    fn image_paste_keys(&self) -> &'static [crossterm::event::KeyEvent] {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        const KEYS: &[KeyEvent] = &[KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL)];
        KEYS
    }
    fn status_hooks(&self) -> Option<StatusHookSupport> {
        Some(StatusHookSupport { live_reload: true })
    }
    fn output_len(&self, lines: &[String]) -> usize {
        chrome::claude_output_len(lines)
    }
}
