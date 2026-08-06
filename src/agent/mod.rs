//! Agent profile registry.
//!
//! Each AI coding agent spyc can host in the pane (claude/codex/agy/zot)
//! is described by an [`AgentProfile`] impl. The registry plus
//! [`detect`] / [`profile_for`] replace what used to be ~10
//! `match AgentKind` dispatch sites scattered across `app`, `state`,
//! and `config`. Adding an agent is a new impl + one `REGISTRY` entry —
//! no match-arm edits.
//!
//! [`AgentKind`] (in `state::sessions`) stays the *persistence* tag
//! serialized into saved sessions; profiles carry *behavior*. The two
//! meet at [`profile_for`] (kind → profile, for restored tabs) and
//! [`detect`] (command → profile, for live panes).

pub mod detect_rules;
pub mod resume;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ratatui::text::Line;

use crate::pane::Pane;
use crate::state::sessions::{AgentKind, SessionCandidate};
use crate::ui::theme::Theme;
use detect_rules::DetectionRule;

/// How a restored pane re-establishes its conversation.
pub enum ResumeAction {
    /// Resume is baked into the spawned command (codex/agy/zot)
    /// or there's nothing to resume (Other).
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
    /// No summary line (Other).
    None,
    /// List human-readable session names (claude).
    Names,
    /// Count tabs that captured a session id (codex / agy).
    Count,
}

/// Everything a resolver needs to find the transcript file belonging to a
/// *specific* live pane. Passed to [`TranscriptSpec::resolve`]. `command` lets
/// a resolver use an explicit session id baked into the spawn command (codex's
/// `resume <uuid>`) for an exact match; resolvers that don't need it ignore it.
#[derive(Clone, Copy)]
pub struct TranscriptQuery<'a> {
    /// The pane's working directory.
    pub cwd: &'a Path,
    /// When the pane's subprocess was spawned (epoch seconds).
    pub spawn_epoch_secs: u64,
    /// The command the pane was spawned with (e.g. `codex resume <uuid>`).
    pub command: &'a str,
    /// The session id pinned to this pane at spawn (codex — Option B), if
    /// resolved. The strongest signal: an exact match on the session's rollout.
    /// `None` until pinned (or for agents that don't pin).
    pub session_id: Option<&'a str>,
}

/// Describes an agent's on-disk transcript view for `^a v`.
pub struct TranscriptSpec {
    /// Locate the transcript file for the pane (see [`TranscriptQuery`]).
    pub resolve: fn(TranscriptQuery) -> Option<PathBuf>,
    /// Render that file into pager lines. `width` is the pager body-width
    /// hint (cells) so agent prose reflows to the scrollback pane width
    /// when rendered as Markdown; `None` falls back to the default.
    /// `show_tool_calls` keeps the agent's tool-use / tool-result lines
    /// (`t` toggles them in the scrollback) — `false` renders prose only.
    pub render: fn(&Path, &Theme, Option<usize>, bool) -> Vec<Line<'static>>,
    /// Config key gating the view; `None` = always-on (codex).
    pub config_key: Option<&'static str>,
    /// Default when the config key is unset.
    pub default_enabled: bool,
    /// Flash text when no transcript is found. Either way `^a v` flashes and
    /// closes back to the pane (`pane_scroll`'s `TranscriptStream`): `Some(msg)`
    /// uses `msg`, `None` uses a generic "`<agent>`: no transcript found for this
    /// session". There is no fall-through to vt100 terminal capture — that only
    /// runs when the agent has no transcript spec at all, or the view is disabled.
    pub miss_message: Option<&'static str>,
}

/// How spyc installs an agent's activity-status lifecycle hooks (the ones that
/// call `spyc --report-status <state>` so the tab dot tracks the agent's turn).
/// Returned by [`AgentProfile::status_hooks`] for agents spyc can auto-wire
/// (claude/codex/agy); `None` for the rest. The two `fn` pointers are the
/// format-specific writer/cleaner in [`crate::mcp`] — JSON `settings.json` for
/// claude, TOML `config.toml` for codex, JSON `.agents/hooks.json` for agy.
pub struct StatusHookSupport {
    /// Write/refresh our hooks into the project dir; returns whether our hooks
    /// are present in a file we own (so teardown tracks the dir for cleanup).
    pub ensure: fn(&Path) -> bool,
    /// Remove only our hooks from the project dir.
    pub cleanup: fn(&Path) -> crate::mcp::ConfigCleanup,
    /// The config file the consent popup names, relative to the project root
    /// (e.g. `.claude/settings.json`).
    pub config_label: &'static str,
    /// True if the agent re-reads its hook config live (claude reloads
    /// `.claude/settings.json` each turn). False = config is read once at
    /// startup (codex), so the hooks must be written BEFORE the pane spawns and
    /// a post-launch enable only takes effect on the agent's next launch.
    pub live_reload: bool,
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

    /// SAVE: confirm the session id pinned to this tab
    /// ([`crate::pane::tabs::TabInfo::live_session_id`] — from a status-hook
    /// payload or an injected `/resume`) still names a real conversation, so save
    /// can prefer it over the spawn-proximity resolver. `Some((id, name))` when it
    /// checks out; `None` when it's stale or the agent has no history to check it
    /// against.
    fn validate_live_session_id(&self, _cwd: &Path, _id: &str) -> Option<(String, Option<String>)> {
        None
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

    /// Transcript scrollback spec, if any. Default: none.
    fn transcript(&self) -> Option<TranscriptSpec> {
        None
    }

    /// Activity-status lifecycle hooks, if spyc can auto-install them for this
    /// agent (claude/codex/agy). Default: none — the dot then rides P0 output
    /// timing only (no semantic working/blocked/done self-report via hooks).
    fn status_hooks(&self) -> Option<StatusHookSupport> {
        None
    }

    /// P1-2 scrape fallback: priority-ordered pane-text detection rules for an
    /// agent that can't (or doesn't yet) self-report — consulted only while no
    /// live semantic report is authoritative for the tab (`report_status`
    /// always wins; see `app::agent_status::effective_activity`). Default:
    /// empty — no fallback beyond P0 output timing, which is correct for any
    /// agent whose prompt text isn't verified here (guessing at UI text spyc
    /// hasn't observed would be worse than no fallback).
    fn detection_rules(&self) -> &'static [DetectionRule] {
        &[]
    }

    /// Which keypresses scroll this agent's own view, for an agent that does
    /// **not** speak mouse.
    ///
    /// Consulted only when [`crate::pane::Pane::wants_mouse`] is false: an agent
    /// that requested mouse reporting gets the wheel forwarded verbatim (claude),
    /// which is always better than synthesizing keys. `None` means spyc has no
    /// verified scroll key for this agent and the wheel does nothing over its
    /// pane — deliberately, because the wrong key is worse than no key here. Up
    /// in a composer usually recalls prompt history, which is the exact bug DEC
    /// 1007's wheel-to-arrows translation caused and that `[mouse] capture` was
    /// turned on to fix.
    ///
    /// Only fill this in for a binding **observed** to scroll, not one that looks
    /// plausible from a keymap file — several of these agents bind the same key to
    /// history recall in their input box.
    fn wheel_scroll(&self) -> Option<WheelScroll> {
        None
    }

    /// Screen-scrape marker confirming this agent's OWN scrollback view is
    /// currently open — checked against [`crate::pane::Pane::visible_lines`]
    /// before deciding to auto-open it or escalate to [`Self::fast_wheel_scroll`].
    /// `None` for an agent with no such view (agy's Shift+Arrow scrolls the live
    /// content directly; there's nothing to open).
    ///
    /// A plain substring, not the `Region`/`Matcher` machinery in
    /// `detect_rules` — that module's rules are specifically about
    /// self-report-fallback semantics (`AgentActivity`), a different question
    /// ("is this agent blocked?") from this one ("is this VIEW open?").
    fn transcript_open_marker(&self) -> Option<&'static str> {
        None
    }

    /// The key that OPENS this agent's own scrollback view — sent once, only
    /// while [`Self::transcript_open_marker`] confirms it's closed. `None` when
    /// there's nothing for spyc to toggle. Not necessarily the same key that
    /// CLOSES it, though today it is (codex's `^T` is a genuine toggle).
    fn transcript_toggle_key(
        &self,
    ) -> Option<(crossterm::event::KeyCode, crossterm::event::KeyModifiers)> {
        None
    }

    /// The keys that PAGE (rather than line-scroll) this agent's own view —
    /// substituted for [`Self::wheel_scroll`]'s keys once a sustained same-
    /// direction wheel gesture has run long enough to justify a bigger jump.
    /// `None` when unverified: an agent's own line-scroll and page keys can
    /// differ in which contexts they're safe (see `CodexProfile`'s doc), so
    /// this is a distinct, separately-verified opt-in rather than assumed from
    /// `wheel_scroll` existing.
    fn fast_wheel_scroll(&self) -> Option<WheelScroll> {
        None
    }

    /// Whether this agent's own scrollback view, if open, is confirmed at (or
    /// past) its bottom — checked before closing it on a further "scroll down"
    /// tick that has nowhere left to go. `visible_lines` is the pane's CURRENT
    /// viewport (same source `transcript_open_marker` is checked against).
    ///
    /// Default `false` — never confirmed, so spyc never closes speculatively.
    /// Each agent's "nothing more to scroll" indicator (if it has one at all)
    /// is specific enough to need its own verified reading rather than a
    /// shared "look for 100%" heuristic that might not mean the same thing
    /// elsewhere.
    fn transcript_at_bottom(&self, visible_lines: &[String]) -> bool {
        let _ = visible_lines;
        false
    }

    /// The key that CLOSES this agent's own scrollback view — distinct from
    /// [`Self::transcript_toggle_key`] (which also OPENS it): closing via a
    /// dedicated key rather than the toggle means a stale "still open" read
    /// during the close settle window sends the *same* safe key again, rather
    /// than the toggle risking a reopen. `None` when there's nothing to close.
    fn transcript_close_key(
        &self,
    ) -> Option<(crossterm::event::KeyCode, crossterm::event::KeyModifiers)> {
        None
    }
}

/// The keys that scroll a non-mouse agent's own view one line, sent by spyc in
/// place of a wheel event it cannot forward. See
/// [`AgentProfile::wheel_scroll`] / [`AgentProfile::fast_wheel_scroll`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WheelScroll {
    pub up: (crossterm::event::KeyCode, crossterm::event::KeyModifiers),
    pub down: (crossterm::event::KeyCode, crossterm::event::KeyModifiers),
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
    fn validate_live_session_id(&self, cwd: &Path, id: &str) -> Option<(String, Option<String>)> {
        if crate::state::sessions::claude_jsonl_exists(cwd, id) {
            Some((
                id.to_string(),
                crate::state::sessions::find_claude_session_name(id),
            ))
        } else {
            None
        }
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
    fn status_hooks(&self) -> Option<StatusHookSupport> {
        Some(StatusHookSupport {
            ensure: crate::mcp::ensure_claude_status_hooks,
            cleanup: crate::mcp::cleanup_claude_status_hooks,
            config_label: ".claude/settings.json",
            live_reload: true,
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

    /// Plain `Up` / `Down`, which scroll codex's `^T` transcript overlay — the only
    /// scrollable view it has. Its main chat view doesn't scroll at all, and its
    /// vt100 scrollback is empty (it confines the transcript to a DECSTBM scroll
    /// region), so this is the only surface either side can move.
    ///
    /// Safe outside the overlay, which is why it's plain arrows and not PageUp:
    /// codex binds `plain(Up)` to `editor.move_up` — move the cursor within the
    /// draft — so on an empty or single-line composer it's a no-op. History recall
    /// is on `Ctrl+R` / `Alt+Up`, so this can't reproduce the wheel-recalls-history
    /// bug that DEC 1007 caused for claude. Worst case, with a multi-line draft in
    /// progress, the draft cursor moves; nothing is submitted or lost.
    fn wheel_scroll(&self) -> Option<WheelScroll> {
        use crossterm::event::{KeyCode, KeyModifiers as M};
        Some(WheelScroll {
            up: (KeyCode::Up, M::NONE),
            down: (KeyCode::Down, M::NONE),
        })
    }

    /// Confirmed by a live pty capture: codex's `^T` transcript renders a
    /// tiled banner reading `T R A N S C R I P T` across its top row (a
    /// letter-spaced watermark, not a plain title), which does not appear
    /// anywhere in the composer/chat view. Checked as a plain substring — the
    /// banner repeats across the full row width, so even a narrow terminal
    /// shows an occurrence.
    fn transcript_open_marker(&self) -> Option<&'static str> {
        Some("T R A N S C R I P T")
    }

    /// `^T`: bound to BOTH `global.open_transcript` and, inside the pager,
    /// `pager.close_transcript` — a genuine same-key toggle, confirmed in
    /// `codex-rs/tui/src/keymap.rs`.
    fn transcript_toggle_key(
        &self,
    ) -> Option<(crossterm::event::KeyCode, crossterm::event::KeyModifiers)> {
        use crossterm::event::{KeyCode, KeyModifiers as M};
        Some((KeyCode::Char('t'), M::CONTROL))
    }

    /// codex's own footer, visible while the transcript is open, spells out
    /// `pgup/pgdn to page` — these are the agent's OWN documented fast-scroll
    /// keys for this exact view (`pager.page_up` / `pager.page_down` in
    /// `codex-rs/tui/src/keymap.rs`), not a guess at what might work faster.
    ///
    /// Confirmed harmless outside the transcript too: a live probe typed a
    /// two-line draft, sent PageUp/PageDown four times, and the draft was
    /// still sitting there untouched — codex's `list` keymap namespace also
    /// binds PageUp/PageDown (for slash-command / picker menus), but that
    /// namespace isn't active in the plain composer, and `transcript_open_marker`
    /// gates this to the transcript specifically regardless.
    fn fast_wheel_scroll(&self) -> Option<WheelScroll> {
        use crossterm::event::{KeyCode, KeyModifiers as M};
        Some(WheelScroll {
            up: (KeyCode::PageUp, M::NONE),
            down: (KeyCode::PageDown, M::NONE),
        })
    }

    /// Confirmed against `codex-rs/tui/src/pager_overlay.rs`: `percent` is
    /// computed directly from `scroll_offset` clamped to `max_scroll`
    /// (`total_len - viewport_height`), so 100% means the view is genuinely AT
    /// its bottom, not merely close to it — a further scroll-down tick has
    /// nowhere left to go. The percentage is rendered as ` NNN% ` (both
    /// spaces literal) right-aligned on the dash-filled separator row directly
    /// above the footer hints.
    ///
    /// Anchored to that row specifically — the LAST visible line containing
    /// `─` — rather than a blind whole-screen substring search: bare "100%"
    /// could appear as ordinary transcript prose (a coding assistant's reply
    /// mentioning test coverage, say), but " 100% " embedded in a row that's
    /// otherwise almost entirely dashes is not a coincidence.
    ///
    /// Also correctly reads "unknown" (via no line matching): codex hides this
    /// indicator entirely while more history remains unloaded
    /// (`scroll_percentage_visible = !state.has_unloaded_history()`), which is
    /// exactly the case where "100%" would be misleading — there's more below
    /// what's loaded. No matching row falls through to `false` (never close),
    /// which is the same safe default this method's whole design commits to.
    fn transcript_at_bottom(&self, visible_lines: &[String]) -> bool {
        visible_lines
            .iter()
            .rev()
            .find(|l| l.contains('─'))
            .is_some_and(|l| l.contains(" 100% "))
    }

    /// `q`: codex's OWN footer hint reads "q to quit" — the dedicated close
    /// action (`pager.close`), distinct from the `^T` toggle used to open. See
    /// `AgentProfile::transcript_close_key`'s doc for why closing via a
    /// dedicated key rather than re-sending the toggle matters here.
    fn transcript_close_key(
        &self,
    ) -> Option<(crossterm::event::KeyCode, crossterm::event::KeyModifiers)> {
        use crossterm::event::{KeyCode, KeyModifiers as M};
        Some((KeyCode::Char('q'), M::NONE))
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
    fn status_hooks(&self) -> Option<StatusHookSupport> {
        // Codex's event hooks live in `.codex/config.toml` (the same file as the
        // MCP entry) and are read once at startup → `live_reload: false`, so the
        // app-layer install runs pre-spawn for an already-consented repo.
        Some(StatusHookSupport {
            ensure: crate::mcp::ensure_codex_status_hooks,
            cleanup: crate::mcp::cleanup_codex_status_hooks,
            config_label: ".codex/config.toml",
            live_reload: false,
        })
    }
}

/// P1-2: agy DOES have `status_hooks()`, but its event vocabulary is
/// `PreToolUse` / `PostToolUse` / `PreInvocation` / `PostInvocation` / `Stop`
/// (agy's own `agy-customizations/docs/hooks.md`) — none of which fires when
/// agy asks the *user* to approve a tool call. `PreToolUse` is the nearest
/// thing and is the wrong instrument: it runs *instead of* the prompt and must
/// answer with an `allow`/`deny`/`ask` decision, so hanging a status report on
/// it would make spyc arbitrate agy's permissions. Hence `blocked` — the one
/// state that actually earns the user's attention — comes from this screen
/// scrape instead.
///
/// All three phrases are required (see [`detect_rules::Matcher::All`]): an
/// agent discussing permissions prints any one of them readily, and a false
/// red "needs me" square is worse than no fallback.
static AGY_DETECTION_RULES: &[DetectionRule] = &[DetectionRule {
    region: detect_rules::Region::BottomNonEmptyLines(15),
    matcher: detect_rules::Matcher::All(&[
        "Requesting permission for:",
        "Do you want to proceed?",
        "esc to cancel",
    ]),
    state: crate::pane::AgentActivity::Blocked,
    visible_blocker: Some("awaiting tool-execution approval"),
}];

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
        cwd: &Path,
        spawn_epoch_secs: u64,
        claimed: &HashSet<String>,
    ) -> (Option<String>, Option<String>) {
        let lines = pane.recent_lines(200);
        if let Some(id) = crate::state::sessions::extract_agy_resume_token(&lines)
            .filter(|tok| !claimed.contains(tok))
        {
            return (Some(id), None);
        }

        // Fallback: pick the per-pane match by spawn-time proximity
        let candidates = crate::state::sessions::find_agy_sessions(cwd);
        if let Some(c) = crate::state::sessions::pick_closest_unclaimed_session(
            candidates,
            spawn_epoch_secs,
            claimed,
        ) {
            return (Some(c.session_id), None);
        }

        (None, None)
    }
    fn validate_live_session_id(&self, cwd: &Path, id: &str) -> Option<(String, Option<String>)> {
        if crate::state::sessions::agy_jsonl_exists(cwd, id) {
            Some((id.to_string(), None))
        } else {
            None
        }
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
    fn status_hooks(&self) -> Option<StatusHookSupport> {
        // A named hook-set in `.agents/hooks.json`, read at startup → written
        // pre-spawn. Covers `working` + `done` only; `blocked` has no event to
        // hang on and comes from AGY_DETECTION_RULES instead.
        Some(StatusHookSupport {
            ensure: crate::mcp::ensure_agy_status_hooks,
            cleanup: crate::mcp::cleanup_agy_status_hooks,
            config_label: ".agents/hooks.json",
            live_reload: false,
        })
    }
    fn detection_rules(&self) -> &'static [DetectionRule] {
        AGY_DETECTION_RULES
    }

    /// Shift+Up / Shift+Down, verified by hand in agy's default `native terminal
    /// (inline)` rendering mode. agy discards mouse reports, so the wheel has
    /// nothing to forward to and these are the only scroll affordance it offers.
    fn wheel_scroll(&self) -> Option<WheelScroll> {
        use crossterm::event::{KeyCode, KeyModifiers as M};
        Some(WheelScroll {
            up: (KeyCode::Up, M::SHIFT),
            down: (KeyCode::Down, M::SHIFT),
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
    // detection_rules: default `&[]` (P1-2 scrape fallback) — no verified
    // prompt/UI text for zot to build a rule from; add one once observed.
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
static AGY: AgyProfile = AgyProfile;
static ZOT: ZotProfile = ZotProfile;
static OTHER: OtherProfile = OtherProfile;

/// All real agents, in detection-precedence order. Binaries don't
/// overlap, so order is not load-bearing — but keep it stable.
pub static REGISTRY: &[&dyn AgentProfile] = &[&CLAUDE, &CODEX, &AGY, &ZOT];

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

    /// Which agents claim a wheel scroll key, and what it encodes to on the wire.
    ///
    /// Asserting the BYTES, not just the `KeyCode`: the whole failure mode here is
    /// silent — a binding that encodes to something the agent ignores looks
    /// identical to no binding at all from the outside, which is how this bug
    /// reached a user in the first place.
    #[test]
    fn wheel_scroll_is_claimed_only_where_it_was_verified() {
        // agy: ignores mouse reports, scrolls on Shift+Arrow in its default
        // inline rendering mode. `\e[1;2A` / `\e[1;2B` are xterm Shift+Up/Down.
        let agy = detect("agy")
            .wheel_scroll()
            .expect("agy scrolls on Shift+Arrow");
        let enc = |(code, mods)| {
            crate::pane::input::encode_key(crossterm::event::KeyEvent::new(code, mods))
        };
        assert_eq!(enc(agy.up), b"\x1b[1;2A");
        assert_eq!(enc(agy.down), b"\x1b[1;2B");

        // claude requests mouse reporting (?1000h/?1002h/?1003h + SGR), so the
        // wheel is forwarded verbatim and a synthesized key would be strictly
        // worse — it cannot carry coordinates.
        assert!(detect("claude").wheel_scroll().is_none());

        // codex discards mouse events outright (`map_crossterm_event` maps only
        // Key/Resize/Paste/Focus), so forwarding can never work — but plain arrows
        // scroll its `^T` transcript overlay (`tui.keymap.pager.scroll_up`), and
        // outside that overlay `plain(Up)` is `editor.move_up` (draft cursor), NOT
        // history recall. `\e[A` / `\e[B` are bare xterm Up/Down.
        let codex = detect("codex")
            .wheel_scroll()
            .expect("codex scrolls on arrows");
        assert_eq!(enc(codex.up), b"\x1b[A");
        assert_eq!(enc(codex.down), b"\x1b[B");

        // A plain process is not an agent: its scrollback belongs to spyc.
        assert!(detect("bash -lc 'make'").wheel_scroll().is_none());
    }

    /// codex's toggleable `^T` view — the marker, the toggle, and the fast key —
    /// measured against a live pty capture, not assumed.
    #[test]
    fn codex_transcript_view_is_fully_specified() {
        let enc = |(code, mods)| {
            crate::pane::input::encode_key(crossterm::event::KeyEvent::new(code, mods))
        };
        let codex = detect("codex");

        // The tiled banner a live capture showed across row 0 of the open `^T`
        // view. Doesn't appear anywhere in the composer/chat view.
        assert_eq!(codex.transcript_open_marker(), Some("T R A N S C R I P T"));

        // ^T: bound to BOTH global.open_transcript and pager.close_transcript in
        // codex's own keymap.rs — a genuine same-key toggle.
        let (code, mods) = codex.transcript_toggle_key().expect("codex has a toggle");
        assert_eq!(enc((code, mods)), b"\x14"); // Ctrl+T

        // codex's own footer, visible only while `^T` is open, spells out
        // "pgup/pgdn to page" — its own documented fast-scroll keys for this view.
        let fast = codex.fast_wheel_scroll().expect("codex has a fast scroll");
        assert_eq!(enc(fast.up), b"\x1b[5~"); // PageUp
        assert_eq!(enc(fast.down), b"\x1b[6~"); // PageDown

        // agy has no dedicated toggleable view — its Shift+Arrow scrolls the live
        // content directly, with nothing to open or page through.
        let agy = detect("agy");
        assert!(agy.transcript_open_marker().is_none());
        assert!(agy.transcript_toggle_key().is_none());
        assert!(agy.fast_wheel_scroll().is_none());
        assert!(agy.transcript_close_key().is_none());
        assert!(!agy.transcript_at_bottom(&["100%".to_string()]));
    }

    /// `transcript_at_bottom`: anchored to the dash-filled separator row, not a
    /// blind whole-screen search — measured against `pager_overlay.rs`'s
    /// literal `format!(" {percent}% ")`.
    #[test]
    fn codex_at_bottom_is_anchored_to_the_separator_row() {
        let codex = detect("codex");

        let open_at_bottom = vec![
            "some reply text".to_string(),
            "─────────────────────────────────────────── 100% ─".to_string(),
            " ↑/↓ to scroll   pgup/pgdn to page   home/end to jump".to_string(),
            " q to quit   esc to edit prev".to_string(),
        ];
        assert!(codex.transcript_at_bottom(&open_at_bottom));

        let open_not_at_bottom = vec![
            "some reply text".to_string(),
            "─────────────────────────────────────────────── 42% ─".to_string(),
            " ↑/↓ to scroll   pgup/pgdn to page   home/end to jump".to_string(),
            " q to quit   esc to edit prev".to_string(),
        ];
        assert!(!codex.transcript_at_bottom(&open_not_at_bottom));

        // codex hides the indicator entirely while more history is unloaded —
        // no matching row must read as "unknown", never as "at bottom".
        let indicator_hidden = vec![
            "some reply text".to_string(),
            "───────────────────────────────────────────────────".to_string(),
            " ↑/↓ to scroll   pgup/pgdn to page   home/end to jump".to_string(),
        ];
        assert!(!codex.transcript_at_bottom(&indicator_hidden));

        // Ordinary transcript prose that happens to mention a percentage must
        // NOT be mistaken for the indicator — it isn't on a dash-filled row.
        let prose_mentions_percent = vec![
            "test coverage is now 100% across the module".to_string(),
            "─────────────────────────────────────────────── 42% ─".to_string(),
        ];
        assert!(!codex.transcript_at_bottom(&prose_mentions_percent));

        assert_eq!(
            codex.transcript_close_key().unwrap().0,
            crossterm::event::KeyCode::Char('q')
        );
    }

    #[test]
    fn detects_known_agents_and_other() {
        assert_eq!(detect("claude").kind(), AgentKind::Claude);
        assert_eq!(
            detect("/usr/local/bin/codex resume").kind(),
            AgentKind::Codex
        );
        assert_eq!(detect("agy --continue").kind(), AgentKind::Agy);
        assert_eq!(detect("zot").kind(), AgentKind::Zot);
        assert_eq!(detect("/opt/bin/zot -c").kind(), AgentKind::Zot);
        assert_eq!(detect("bash -lc 'make'").kind(), AgentKind::Other);
        assert_eq!(detect("").kind(), AgentKind::Other);
    }

    /// claude / codex / agy auto-install status hooks, each naming its own
    /// config file; only claude live-reloads (codex/agy read config at startup).
    #[test]
    fn status_hook_support_per_agent() {
        let claude = ClaudeProfile
            .status_hooks()
            .expect("claude has status hooks");
        assert_eq!(claude.config_label, ".claude/settings.json");
        assert!(claude.live_reload, "claude reloads settings.json live");

        let codex = CodexProfile.status_hooks().expect("codex has status hooks");
        assert_eq!(codex.config_label, ".codex/config.toml");
        assert!(!codex.live_reload, "codex reads config once at startup");

        let agy = AgyProfile.status_hooks().expect("agy has status hooks");
        assert_eq!(agy.config_label, ".agents/hooks.json");
        assert!(!agy.live_reload, "agy reads config at startup");

        assert!(ZotProfile.status_hooks().is_none());
        assert!(OtherProfile.status_hooks().is_none());
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

    /// Other (bash/vim/make): the saved command runs verbatim and any
    /// stray session id is ignored — no resume, no panic.
    #[test]
    fn other_restore_runs_verbatim_ignoring_sid() {
        let cwd = Path::new("/tmp");
        let plan = OtherProfile.reconstruct_restore("bash -lc 'make'", Some("ignored"), cwd);
        assert_eq!(plan.command, "bash -lc 'make'");
        assert!(matches!(plan.resume, ResumeAction::None));
    }

    // ── kind → profile dispatch (restore-time) ────────────────────────
    // `detect` (command → profile) is covered above; these pin the OTHER
    // direction, `profile_for` (persisted AgentKind → behavior), which the
    // restore loop uses to choose each tab's resume strategy.

    /// Every registered agent's kind round-trips through `profile_for`:
    /// the persisted tag resolves back to the profile that owns it.
    /// Catches a REGISTRY entry whose `kind()` is wrong, or a missing one.
    #[test]
    fn profile_for_round_trips_every_registered_kind() {
        for &p in REGISTRY {
            let back = profile_for(p.kind());
            assert_eq!(back.kind(), p.kind());
            assert_eq!(
                back.binary(),
                p.binary(),
                "kind {:?} routed to the wrong profile",
                p.kind()
            );
        }
    }

    /// All five real kinds resolve to a matching profile, and the
    /// un-registered `Other` falls back to the no-op profile (identity
    /// restore, no panic).
    #[test]
    fn profile_for_resolves_all_kinds_including_other() {
        for k in [
            AgentKind::Claude,
            AgentKind::Codex,
            AgentKind::Agy,
            AgentKind::Zot,
        ] {
            assert_eq!(profile_for(k).kind(), k);
        }
        assert_eq!(profile_for(AgentKind::Other).kind(), AgentKind::Other);
        let plan =
            profile_for(AgentKind::Other).reconstruct_restore("vim", Some("x"), Path::new("/tmp"));
        assert_eq!(plan.command, "vim");
        assert!(matches!(plan.resume, ResumeAction::None));
    }

    /// Back-compat end-to-end (no PTY): a pre-1.41.6 Claude tab — saved
    /// with `claude_session_id` and no `agent_kind` (so `agent_kind ==
    /// Other`) — must still route to the Claude resume path. This is the
    /// `effective_kind → profile_for → reconstruct_restore` chain the
    /// restore loop runs for each tab.
    #[test]
    fn legacy_claude_tab_resumes_via_effective_kind() {
        let tab = crate::state::sessions::SavedTab {
            command: "claude".into(),
            label: "claude".into(),
            cwd: "/tmp".into(),
            agent_kind: AgentKind::Other, // legacy save: field absent → Other
            agent_session_id: Some("sid-legacy".into()),
            agent_session_name: Some("OLD".into()),
            claim_owner: String::new(),
        };
        // `effective_kind` upgrades the legacy Other → Claude.
        assert_eq!(tab.effective_kind(), AgentKind::Claude);
        let plan = profile_for(tab.effective_kind()).reconstruct_restore(
            &tab.command,
            tab.agent_session_id.as_deref(),
            Path::new("/tmp"),
        );
        assert_eq!(plan.command, "claude");
        assert!(matches!(
            plan.resume,
            ResumeAction::ClaudeStdin { session_id } if session_id == "sid-legacy"
        ));
    }

    /// A legacy tab with no session id stays a fresh, verbatim spawn
    /// (`effective_kind == Other` → no resume) — never a panic.
    #[test]
    fn legacy_tab_without_sid_is_a_fresh_spawn() {
        let tab = crate::state::sessions::SavedTab {
            command: "claude".into(),
            label: "claude".into(),
            cwd: "/tmp".into(),
            agent_kind: AgentKind::Other,
            agent_session_id: None,
            agent_session_name: None,
            claim_owner: String::new(),
        };
        assert_eq!(tab.effective_kind(), AgentKind::Other);
        let plan = profile_for(tab.effective_kind()).reconstruct_restore(
            &tab.command,
            None,
            Path::new("/tmp"),
        );
        assert_eq!(plan.command, "claude");
        assert!(matches!(plan.resume, ResumeAction::None));
    }
}
