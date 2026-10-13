//! The Model pane cluster, including transient HUD and attachment state.

use super::{PaneSnapshot, PendingImage, ZoomTarget, agent_hud};

/// `AppState`'s loose pane fields.
#[derive(Default)]
pub struct PaneLayout {
    pub pane_prompt_buf: String,
    pub last_pane_prompt: Option<String>,
    /// Images pasted into each tab but not yet sent, keyed by tab id (unlike
    /// `pane_prompt_buf`, which tracks only the focused pane — an image pasted
    /// into tab 2 must not surface in tab 1's gallery). Cleared for a tab when
    /// its prompt is submitted, at which point the agent's transcript becomes
    /// the record instead.
    pub pending_images: std::collections::HashMap<String, Vec<PendingImage>>,
    /// Accepted submission invalidates clipboard reads still running for that
    /// draft. A late result must not become an unsent image in the next prompt.
    pub image_capture_epochs: std::collections::HashMap<String, u64>,
    /// MVU Phase 5: active-pane routing flags, refreshed at loop-top (see
    /// [`PaneSnapshot`]). Read by `route_snapshot` so routing reads the
    /// Model, not the live pane host.
    pub pane_snapshot: PaneSnapshot,
    /// Bounded telemetry for the one visible consumer, never persisted.
    pub agent_hud: agent_hud::AgentHudState,
    pub pane_height_pct: u16,
    /// Tmux-style "zoom" of the *active* region (see [`ZoomTarget`]).
    /// `BottomPane` fills the middle (list → 0 rows); `TopList` fills the
    /// frame (pane collapses off-screen). The user's preferred
    /// `pane_height_pct` is preserved untouched so un-zoom restores exactly
    /// the prior split. Zoom always targets whichever region had focus at
    /// zoom-on, so focus is unchanged across the toggle.
    pub zoom: ZoomTarget,
    /// `F10` / `^a-\` (TogglePane) toggles this. When true, the pane
    /// row is hidden — render skips the pane area, layout treats it
    /// as if no pane existed — but `pane_tabs` and every child pty
    /// stay alive. Re-toggle flips it back; the rendered grid picks
    /// up wherever the running processes left off. Previous behaviour
    /// was destructive (`pane_tabs = None`, SIGKILL via `Drop for
    /// PtyHost`), which the user experienced as "I lost my claude
    /// conversation every time I needed the whole screen for a
    /// second."  Explicit kill of a tab still goes through `^a-x`
    /// (`PaneCloseTab`).
    pub pane_hidden: bool,
    /// In a vertical split, the left side's vertical position when focus last
    /// left it for the right column — `true` = the bottom pane, `false` = the
    /// file list. `^a h` (focus left) restores it, so returning from the right
    /// doesn't always snap to the top list.
    pub vsplit_left_was_pane: bool,
}
