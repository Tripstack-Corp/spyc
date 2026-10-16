mod identity;
mod reporter;
mod user;

pub use user::{codex_hooks_json_with_spyc, codex_hooks_json_without_spyc};

use super::reporter_command;

// ── Codex status hooks ────────────────────────────────────────────────
//
// Codex reads its hooks from `$CODEX_HOME/hooks.json` (`user.rs`).
// `UserPromptSubmit` → working, `Stop` → done, `Interrupt` → idle.
// `PermissionRequest` retains its wire command for hook-trust stability; a
// metadata-capable host records it observationally because it precedes both
// automatic and human review. Codex reads its hooks once at startup (no live
// reload), so a first-launch `yes` only takes effect on codex's next launch.

/// Lifecycle events are unfiltered; tool hooks match only `request_user_input`.
/// Generated reporter invocations identify owned handlers for cleanup.
const CODEX_STATUS_HOOKS: [(&str, &str); 6] = [
    ("UserPromptSubmit", "working"),
    ("PermissionRequest", "blocked"),
    ("Stop", "done"),
    ("Interrupt", "idle"),
    ("PreToolUse", crate::agent::codex_recovery::QUESTION_START),
    ("PostToolUse", crate::agent::codex_recovery::QUESTION_END),
];
