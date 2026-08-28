//! The **ViewState** half of the MVU split: render ephemerals and caches,
//! plus the seven types that exist only as its field types — the pager-return
//! slot, the harpoon menu, the image gallery and full-screen view, the chord
//! hint, the visual bell, and the tab-rename scratch.
//!
//! They travel with the fields they describe. Extracted from `app/mod.rs` for
//! the same reason as [`super::runtime`].
//!
//! Fields are `pub(super)`: from here that is the reachability they had as
//! items of `app`, which is what render, the handlers and the loop steps read
//! them through.

use std::path::PathBuf;

use crossterm::event::KeyCode;

use super::{AgentStatusCache, PagerHistory, activity, image_ops, mouse, pane_scroll};
use crate::ui::line_edit::LineEditor;
use crate::ui::list_view::Row;
use crate::ui::pager::PagerView;
use crate::ui::theme::Theme;

/// State for returning to the pager after `v` (edit) exits.
pub(super) enum PagerReturn {
    /// Buffer content: reload from this temp file, then delete it.
    TempFile {
        path: PathBuf,
        title: String,
        scroll: usize,
        mount: crate::ui::pager::Mount,
        pane_scroll: bool,
    },
    /// On-disk file: reopen from the original path.
    SourceFile {
        path: PathBuf,
        scroll: usize,
        mount: crate::ui::pager::Mount,
        pane_scroll: bool,
    },
}

/// State for the harpoon menu overlay (`Hh` / `gh`). Shows the
/// project's harpoon slots and lets the user reorder, delete, or
/// jump while the overlay is open. Keys are intercepted before
/// normal dispatch when `Some`.
pub(super) struct HarpoonMenu {
    /// Cursor row inside the menu (0-based, indexes the *active*
    /// non-empty slots). Clamped to `slots.len() - 1` after each
    /// mutation so deletes never leave it dangling.
    pub(super) cursor: usize,
    /// vim-style `dd` arming: `d` arms, second `d` deletes; any
    /// other key clears it. Avoids accidental deletion from a
    /// single-key slip.
    pub(super) delete_armed: bool,
}

/// The `^a g` image-gallery popup.
///
/// Holds the *received* half (indexed off the agent's transcript) inline, but
/// reads the unsent half from `state.pane.pending_images` by `tab_id` rather
/// than copying it — those are megabytes each, and the popup is a glance.
pub struct ImageGallery {
    /// Which pane tab this gallery belongs to. Pins a late-arriving index so a
    /// tab switch mid-load can't show one agent's images under another's name.
    pub(super) tab_id: String,
    pub(super) received: Vec<crate::state::transcript_images::TranscriptImage>,
    /// Where `received` came from, and where an image's bytes are re-read from.
    /// `None` when there's no readable transcript and only unsent images show.
    pub(super) transcript_path: Option<std::path::PathBuf>,
    pub(super) cursor: usize,
    /// The transcript index is still being built off-thread.
    pub(super) loading: bool,
}

/// A rendered image shown full-screen. Holds the ready-to-blit protocol plus
/// the encoded bytes, so the overlay verbs (`s` save, `y` copy, `b` base64)
/// work without re-rendering, and the [`ImageOrigin`](image_ops::ImageOrigin)
/// that tells them *what* they're acting on. See
/// `docs/archive/MERMAID_PAGER_PLAN.md`.
pub struct ImageView {
    pub(super) protocol: ratatui_image::protocol::Protocol,
    /// The image as it arrived — a mermaid render's PNG, or an image file's own
    /// bytes verbatim (which may be JPEG/GIF/WebP, hence not `png`).
    pub(super) encoded: Vec<u8>,
    /// Extension matching `encoded`'s real format, for `s`.
    pub(super) ext: &'static str,
    /// Natural pixel size, for the footer.
    pub(super) dims: (u32, u32),
    pub(super) origin: image_ops::ImageOrigin,
    /// Whether the current render uses the dark theme — tracked so `c` toggles it.
    pub(super) dark: bool,
    /// Transient verb feedback (e.g. "saved: …"), shown in the overlay footer —
    /// the normal flash area is hidden behind the full-screen image.
    pub(super) flash: Option<String>,
}

/// The which-key chord-hint popup's render data — a chord prefix's title and
/// its continuation rows (`keys → label`). Built in `settle_chord_hint` from
/// `Resolver::continuations` once the hint delay elapses while a chord is still
/// pending; read (never mutated) by `render_chord_hint`.
pub struct ChordHint {
    /// The armed prefix, e.g. `"^a"` or `"g"`.
    pub(super) title: String,
    /// `(keys, label)` per continuation, e.g. `("z", "zoom pane (toggle fullscreen)")`.
    pub(super) rows: Vec<(&'static str, &'static str)>,
}

/// A brief spice-heat border-pulse flash — the P3-1 visual bell. `start` times
/// the ~half-second decay; `frame` is the animation step advanced in
/// `settle_visual_bell` (a `&mut` settle — the pure draw can't read the clock)
/// so the perimeter pulse can sweep its pepper→ember→orange→spark gradient.
/// `None` on `view.visual_bell` means no flash is active.
#[derive(Clone, Copy, Debug)]
pub struct VisualBell {
    pub(super) start: std::time::Instant,
    pub(super) frame: u64,
}

/// MVU end-state: the **ViewState** cluster — render ephemerals + derived
/// caches + UI-layer state. Pure of OS handles (those live in [`Runtime`]) and
/// of domain state (that lives in `AppState`). Owned by `App` as a disjoint
/// field; handlers reach it via `self.view.…`.
#[allow(clippy::struct_excessive_bools)]
pub struct ViewState {
    /// The top-region / centered pager: `Mount::Overlay` (grep, git-view,
    /// help, command output, file viewers) or `Mount::TopPane` (`D`). Drives
    /// the file-list area or the centered overlay.
    pub(super) pager: Option<PagerView>,
    /// The bottom-region pane-scrollback pager (`^a v`): always
    /// `Mount::LowerPane` + `pane_scroll`. Held in a *separate* slot so it
    /// coexists with a top-region [`Self::pager`] (read a `D` doc up top while
    /// scrolling claude's history below) — they occupy different screen regions
    /// and must not evict each other. The focused-region pager is selected by
    /// [`App::active_pager_mut`]; render draws both independently.
    pub(super) scroll_pager: Option<PagerView>,
    /// The RIGHT column's top-region (`D`) pager in a vertical split: a
    /// `Mount::TopPane` `PagerView`, the dual-overlay twin of [`Self::pager`]
    /// for column `b`. Its own slot so a `D` in `b` coexists with a `D`/`V` in
    /// `a` instead of evicting it. Rendered into `layout.right` by
    /// `render_right_split`. `None` outside a split or when `b` has no `D` open.
    /// (The full-frame modals — grep / git-view / help / `;cmd` output — stay in
    /// the single [`Self::pager`] slot; only the column-scoped `D` mirrors here.)
    pub(super) pager_right: Option<PagerView>,
    /// The right-column pager of a vertical split (the live-reloading
    /// preview). Its **own** slot, like [`Self::scroll_pager`], so it coexists
    /// with the top and bottom region pagers — render draws it into
    /// `layout.right`. `None` until the vsplit keys (PR4) open a split; it is
    /// re-read + re-rendered off-thread when its `source_path` changes (PR5).
    pub(super) right_pager: Option<PagerView>,
    /// A preview an **agent** displaced: `create_worktree(open:true)` /
    /// `open_worktree` put a commander in the right column while the user was
    /// reading there. The right column hosts one thing, so the preview has to
    /// yield — but the user didn't ask for that, so it's set aside with the
    /// split shape it had and comes back when `b` closes. `None` when the user
    /// opened `b` themselves (`^s n` IS the ask) or after a restore.
    pub(super) displaced_preview: Option<(super::state::VSplit, PagerView)>,
    /// Whether to fade the inactive split column / list (the focus dim). On by
    /// default; toggled by `^a d` for users who prefer both columns bright.
    pub(super) dim_inactive: bool,
    /// Set when a preview-file change arrived while an off-thread reload was
    /// already in flight (`kick_preview_reload`); the reload's drain re-kicks
    /// once so the FINAL save is the one rendered. Main-thread-only — set in the
    /// fs ingest, read+cleared in `apply_preview_reloads` — so a plain `bool`.
    pub(super) preview_dirty: bool,
    /// Full-screen image overlay (the pager `i` key): a rendered diagram/image
    /// blitted over everything until dismissed (q/Esc), with its own verbs
    /// (`s`/`Y`/`o`/…). `None` when nothing is being viewed. Set by
    /// `apply_image_outcomes`. Graphics terminals only. See
    /// `docs/archive/MERMAID_PAGER_PLAN.md`.
    pub(super) image_view: Option<ImageView>,
    /// The `^a g` gallery popup, when open.
    pub(super) image_gallery: Option<ImageGallery>,
    pub(super) pager_history: PagerHistory,
    pub(super) pager_pending_bracket: Option<char>,
    /// True after a bare `z` in the pager — the vim fold prefix, awaiting
    /// `a` / `R` / `M`. Its own flag rather than a second use of
    /// `pager_pending_bracket`, so a `z` can never be answered by a `]`.
    pub(super) pager_pending_z: bool,
    pub(super) pager_was_open: bool,
    /// This turn's input went to a pager, so a status-bar flash raised while
    /// running its effects belongs in the pager instead (#166). Consumed at
    /// loop bottom by `relocate_flash_into_pager`.
    pub(super) input_went_to_pager: bool,
    /// Stash for the pager that was active when `?` opened the
    /// pager-help overlay. Restored verbatim on Esc/q dismissal so
    /// the user lands back in the same view. Separate from
    /// `pager_history` because the latter silently drops
    /// `no_history=true` views — going through history would lose them.
    pub(super) pager_help_stash: Option<PagerView>,
    /// Stash for the real scrollback (`scroll_pager`) while its dedicated help
    /// (`H` in `^a v`) is shown in that same bottom slot. `H` toggles the help
    /// between scrollback- and pager-keys variants; `Esc`/`q` restores this.
    /// Kept apart from `pager_help_stash` (the top-overlay help) so the two
    /// regions' help flows never clobber each other.
    pub(super) scroll_pager_help_stash: Option<PagerView>,
    /// Per-file scroll memory for the pager (loaded once at startup;
    /// see [`super::state::pager_positions`]).
    pub(super) pager_positions: crate::state::pager_positions::PagerPositions,
    /// Color/style overrides.
    pub(super) theme: Theme,
    /// One-shot full buffer clear/redraw request.
    pub(super) needs_full_repaint: bool,
    /// Cached `build_rows()` output; invalidated by `list_generation`.
    pub(super) cached_rows: Vec<Row>,
    pub(super) cached_rows_gen: u64,
    /// Grid stabilization cache key: (list_gen, view_top, cursor, width, height).
    pub(super) cached_grid_key: (u64, usize, usize, u16, u16),
    /// The same row cache + grid key for the **right** column's second
    /// commander (`state.right`), settled independently. Unused (empty / `MAX`
    /// gen) while no second commander is open.
    pub(super) right_cached_rows: Vec<Row>,
    pub(super) right_cached_rows_gen: u64,
    pub(super) right_cached_grid_key: (u64, usize, usize, u16, u16),
    /// Last terminal-window title emitted (OSC 2 dedup). `None` forces
    /// a re-emit on next draw.
    pub(super) last_term_title: Option<String>,
    // --- D3: the remaining UI-layer ephemerals + activity counters ---
    /// Active harpoon menu overlay (interactive: reorder, delete, jump).
    // Module-private (type `HarpoonMenu` is module-private).
    pub(super) harpoon_menu: Option<HarpoonMenu>,
    /// The which-key chord-hint popup, once the hint delay has elapsed while a
    /// chord is still armed. `None` when no popup is showing (set in
    /// `settle_chord_hint`, cleared the moment the chord resolves/cancels).
    pub(super) chord_hint: Option<ChordHint>,
    /// When the chord-hint popup is due to appear (set on a pending chord in
    /// `handle_key`, consumed by `settle_chord_hint`). `None` when no chord is
    /// arming a popup. Distinct from `chord_hint` so the timer and the shown
    /// popup are tracked independently.
    pub(super) chord_hint_due: Option<std::time::Instant>,
    /// Active Quick Select picker (`^a u`).
    pub(super) quick_select: Option<crate::pane::quick_select::QuickSelect>,
    /// `dd` arming for the graveyard view (first `d` arms, second deletes).
    pub(super) graveyard_pending_d: bool,
    /// `gg` arming for the graveyard view (jump to top).
    pub(super) graveyard_pending_g: bool,
    pub(super) overlay_awaiting_dismiss: bool,
    /// When the current top overlay's child exits, return to spyc **immediately**
    /// instead of holding the "[process exited — press any key]" frame. Set for
    /// interactive overlays — the `V` editor, the `D` huge-file `$PAGER`, the
    /// in-pager editor — where there's no command output to linger on (you `:q`
    /// and want straight back). Left `false` for `;cmd` / `:`-spawned commands,
    /// whose output the await-dismiss preserves (so `;ls` doesn't flash + vanish).
    pub(super) overlay_auto_dismiss: bool,
    /// Which vsplit column the current `V`/`D` overlay/TopPane-pager lives in
    /// (`None` when no split, or no overlay). The overlay is pinned to the
    /// column it opened from: `top_unit` scopes to it and it stays there even
    /// when `^a l`/`^a h` moves keyboard focus to the other column. Set at open,
    /// cleared at teardown.
    pub(super) overlay_column: Option<super::state::Side>,
    /// TTL cache for the active pane's status-line session short-id.
    // Module-private (type `AgentStatusCache` is module-private); the
    // `app::*` descendant modules still reach it via `self.view.…`.
    pub(super) agent_status_cache: Option<AgentStatusCache>,
    pub(super) pending_history_pick: Option<LineEditor>,
    /// Snapshot of jump-history entries for the `J`-prompt popup.
    pub(super) pending_jump_history: Option<Vec<String>>,
    pub(super) history_pending_g: bool,
    /// Pending `g` in pane scroll mode (`gg`/`gf`/`gF`).
    pub(super) scroll_pending_g: bool,
    // Module-private (type `PagerReturn` is module-private).
    pub(super) pending_pager_return: Option<PagerReturn>,
    /// Path to the `.spyc-context.json` file (written each loop for MCP).
    pub(super) context_path: PathBuf,
    /// Last context snapshot written to disk — skip the write when the new
    /// snapshot compares equal (avoids serializing just to diff).
    pub(super) last_context: Option<crate::context::SpycContext>,
    /// `.spyc-context.json` is stale and should be rewritten (debounced +
    /// typing-burst-guarded).
    pub(super) context_dirty: bool,
    /// Whether the MCP socket server is running.
    pub(super) mcp_running: bool,
    /// Whether this instance may take over the MCP socket from another spyc
    /// when it writes a client config. Captured once at startup (the
    /// `App::new` arg) and read at agent-launch time, when we actually write
    /// `.mcp.json` / `.codex/config.toml`.
    pub(super) mcp_takeover_allowed: bool,
    /// When a focus-switch chord just completed: (when, completing key) —
    /// the next dispatch drops a Press/Repeat of that key within ~60 ms.
    pub(super) focus_chord_completed: Option<(std::time::Instant, KeyCode)>,
    /// Activity monitor (`A`): the overlay visibility toggle. The counters
    /// themselves live in [`activity::ActivityMonitor`] (`self.view.activity`).
    pub(super) show_activity: bool,
    /// Activity-monitor counters: live/snapshot double-buffer + peaks + proc
    /// stats. See [`activity::ActivityMonitor`].
    pub(super) activity: activity::ActivityMonitor,
    /// Forward timestamp for the keystroke→echo latency peak, measured against
    /// the next active-pane output.
    pub(super) pane_send_at: Option<std::time::Instant>,
    /// `App::run` process start (activity-monitor uptime).
    pub(super) started_at: std::time::Instant,
    /// Agent-activity (P0) "spicy pulse" animation frame, advanced in
    /// `settle_agent_activity` (a `&mut` settle point — render is pure and
    /// can't read the clock) while ≥1 agent tab is Working. The pure draw maps
    /// it to a warm heat color for the per-tab dot.
    pub(super) agent_anim_frame: u64,
    /// Active spice-heat border-pulse flash (P3-1 visual bell), or `None`.
    /// Started by `settle_agent_activity` on a Blocked/Done transition when
    /// `[notify].visual` is set (on by default); advanced + cleared by
    /// `settle_visual_bell`. The pure draw reads it to paint the perimeter ring —
    /// armed only on a transition, so idle panes never touch it.
    pub(super) visual_bell: Option<VisualBell>,
    /// Process-lifetime constants for the activity HUD, snapshotted ONCE at
    /// construction so the pure `&self` render pass never reads the OS / env
    /// per frame (the render-purity contract): the pid (for `sample`/lldb),
    /// the terminal's `$TERM`, and its truecolor capability. None of these
    /// change after startup.
    pub(super) hud_pid: u32,
    pub(super) hud_term: String,
    pub(super) hud_truecolor: bool,
    /// Resolved color depth for this session (CLI `--color` > `[layout]
    /// color_depth` > `$COLORTERM` auto-detect). When not `TrueColor`, the
    /// per-frame render downgrades every `Color::Rgb` to the nearest 256-color
    /// index so terminals that can't parse 24-bit SGR (old GNU screen) still get
    /// colors. `hud_truecolor` is derived from this — they never disagree.
    pub(super) color_depth: crate::ui::color_depth::ColorDepth,
    /// Whether spyc is running over SSH (`$SSH_CONNECTION`/`$SSH_TTY`/`$SSH_CLIENT`),
    /// snapshotted once at construction (doesn't change mid-session). Drives the
    /// P3-1 `desktop_via = "auto"` routing — OSC-9 (client-side) over SSH, the OS
    /// notifier locally — so the pure `notification_for_transition` never reads env.
    pub(super) is_ssh: bool,
    /// Tab-completion / cycle state.
    // Module-private (type `TabState` is module-private).
    pub(super) tab_state: Option<TabState>,
    /// Scroll throttle: timestamp + direction of last processed arrow key.
    pub(super) scroll_last: Option<(std::time::Instant, KeyCode)>,
    /// A forwarded mouse press is awaiting its release.
    ///
    /// Set when a button press is delivered to the pane's child, cleared when the
    /// matching release is. Keyed on "did the press go to the child" rather than on
    /// where the pointer is now, which is what makes the pairing exact: a press
    /// that moved the file-list focus must not produce a release for the child, and
    /// a press the child DID receive must get its release even if the pointer left
    /// the pane first. Children track button state — claude fires its click on the
    /// release — so both an unpaired and a missing release misbehave.
    pub(super) mouse_press_forwarded: bool,
    /// A left press landed on a pager and a spyc-side text selection owns the
    /// drag until the button comes up.
    ///
    /// The exact counterpart of `mouse_press_forwarded`, and mutually exclusive
    /// with it: a gesture belongs to whoever received its press. Without this, a
    /// drag begun on a pager would fall through to the child-forwarding path and
    /// start typing mouse reports into the agent mid-selection.
    ///
    /// Which surface owns the in-flight drag.
    ///
    /// Holds the target so the drag keeps addressing the surface it STARTED in even
    /// if the pointer wanders elsewhere — a selection that retargeted mid-drag would
    /// extend against the wrong buffer's indices. For a pager that's the slot, not
    /// the mount, because a mount can't tell `view.pager` from `view.right_pager`.
    pub(super) mouse_selection: Option<mouse::MouseDragTarget>,
    /// A file-list row selection, kept after the drag ends so the highlight
    /// persists and a follow-up yank can still find it (same contract as the
    /// pager's charwise selection).
    pub(super) list_selection: Option<mouse::ListSelection>,
    /// A spyc-side charwise selection over the pane's visible grid, for a child
    /// that ignores mouse reports — `(anchor, focus)` in SCREEN coordinates,
    /// unordered so a backwards drag keeps its direction.
    ///
    /// Screen coordinates, so it is only meaningful for the frame it was made in:
    /// [`App::drain_pane_output`] clears it when the child paints, because the grid
    /// scrolls out from under a selection anchored this way. That is fine for the
    /// case it exists to serve — reading a static transcript overlay — and is the
    /// simple half of the tradeoff recorded in
    /// `docs/archive/mouse_selection_plan.md`.
    pub(super) pane_selection: Option<((u16, u16), (u16, u16))>,
    /// A charwise selection over one of the single-line chrome surfaces (the status
    /// bar, the pane divider/tab line) — the row and an unordered column pair.
    pub(super) chrome_selection: Option<mouse::ChromeSelection>,
    /// The last press on a chrome row: which row, and when. The whole state a
    /// double-click needs — see [`mouse::is_double_click`]. Not reset on release,
    /// because the second press of the pair arrives after one.
    pub(super) last_chrome_click: Option<(u16, std::time::Instant)>,
    /// What each chrome row actually rendered this frame, for a mouse copy.
    ///
    /// `RefCell` because the draw pass is `&self` and this is the renderer recording
    /// what it drew — the same role (and the same reason) as `PagerView`'s
    /// `last_content_area` / `last_body_w` `Cell`s. It holds the DRAWN line, not the
    /// semantic segments: a selection maps screen COLUMNS back to characters, and
    /// only the drawn line has the width-driven truncation the user is looking at.
    pub(super) chrome_rows: std::cell::RefCell<Vec<mouse::ChromeRow>>,
    /// A sustained same-direction wheel-scroll gesture over an agent's own view
    /// (today: codex's `^T`), tracked so a long one can escalate to a page-sized
    /// step and so a short one can't open the view by accident — see
    /// `App::send_agent_view_scroll_keys`. `None` outside a gesture.
    pub(super) pane_scroll_streak: Option<mouse::PaneScrollStreak>,
    /// Set right after spyc sends an agent's `transcript_toggle_key` or
    /// `transcript_close_key`, so a fast follow-up wheel tick — arriving before the
    /// child has redrawn — doesn't act on the STALE screen and send the same key
    /// again. Retired once a scrape confirms the send landed, which is
    /// direction-specific (`mouse::pending_view_confirmed`: marker present for an
    /// `Open`, absent for a `Close`), or after `TOGGLE_SETTLE` if it never does
    /// (self-healing rather than stuck refusing to retry).
    pub(super) pane_view_sent: Option<(std::time::Instant, mouse::PendingViewIntent)>,
    /// Whether an agent-transcript scrollback (`^a v`) renders the agent's
    /// tool-use / tool-result lines. `t` toggles it; the transcript is
    /// re-rendered with the new value. Session-scoped (persists across
    /// re-opens), defaults to shown.
    pub(super) transcript_show_tool_calls: bool,
    /// The scrollback source the user flipped to with `T`, and the pane tab id it
    /// was flipped on — ids are stable and never reused, so another tab's `^a v`
    /// can't inherit this choice. Outlives a re-open (`r` reloads the flipped
    /// source) and is dropped when the scrollback closes, which is the whole life
    /// of the view the flip was made in. `None` = whatever
    /// `decide_scroll_source` picks.
    pub(super) scroll_source_override: Option<(String, pane_scroll::ScrollSourcePick)>,
    /// Cached terminal dimensions (columns, rows). Read once at startup via
    /// `crossterm::terminal::size()` and refreshed on every `Event::Resize` in
    /// `handle_resize`. Handlers read this instead of calling `terminal::size()`
    /// inline, which avoids the repeated syscall and keeps them OS-call-free.
    pub(super) term_size: (u16, u16),
}

impl ViewState {
    /// Build the initial ViewState. `theme`/`context_path` are the only
    /// caller-specific values; `context_dirty` (write-context-on-startup) and
    /// `mcp_running` differ between the live app (`true` / actual) and the test
    /// harness (`false` / `false`). Everything else starts empty.
    pub(super) fn new(
        theme: Theme,
        context_path: PathBuf,
        context_dirty: bool,
        mcp_running: bool,
        color_depth: crate::ui::color_depth::ColorDepth,
    ) -> Self {
        use crate::ui::color_depth::ColorDepth;
        Self {
            pager: None,
            scroll_pager: None,
            pager_right: None,
            right_pager: None,
            displaced_preview: None,
            dim_inactive: true,
            preview_dirty: false,
            image_view: None,
            image_gallery: None,
            pager_history: PagerHistory::new(),
            pager_pending_bracket: None,
            pager_pending_z: false,
            pager_was_open: false,
            input_went_to_pager: false,
            pager_help_stash: None,
            scroll_pager_help_stash: None,
            scroll_source_override: None,
            pager_positions: crate::state::pager_positions::PagerPositions::load(),
            theme,
            needs_full_repaint: false,
            cached_rows: Vec::new(),
            cached_rows_gen: u64::MAX, // force first build
            cached_grid_key: (u64::MAX, 0, 0, 0, 0),
            right_cached_rows: Vec::new(),
            right_cached_rows_gen: u64::MAX, // force first build
            right_cached_grid_key: (u64::MAX, 0, 0, 0, 0),
            last_term_title: None,
            harpoon_menu: None,
            chord_hint: None,
            chord_hint_due: None,
            quick_select: None,
            graveyard_pending_d: false,
            graveyard_pending_g: false,
            overlay_awaiting_dismiss: false,
            overlay_auto_dismiss: false,
            overlay_column: None,
            agent_status_cache: None,
            pending_history_pick: None,
            pending_jump_history: None,
            history_pending_g: false,
            scroll_pending_g: false,
            pending_pager_return: None,
            context_path,
            last_context: None,
            context_dirty,
            mcp_running,
            // Set from the `App::new` arg in bootstrap; the test harness never
            // writes client configs, so the default is fine there.
            mcp_takeover_allowed: false,
            focus_chord_completed: None,
            show_activity: false,
            activity: activity::ActivityMonitor::new(std::time::Instant::now()),
            pane_send_at: None,
            started_at: std::time::Instant::now(),
            agent_anim_frame: 0,
            visual_bell: None,
            hud_pid: std::process::id(),
            hud_term: std::env::var("TERM").unwrap_or_else(|_| "?".to_string()),
            // Derived from the resolved depth, not a second COLORTERM read, so the
            // HUD's "truecolor" line and the gradient path always match what we
            // actually emit (forcing `--color 256` also disables the RGB gradient).
            hud_truecolor: color_depth == ColorDepth::TrueColor,
            color_depth,
            is_ssh: std::env::var_os("SSH_CONNECTION").is_some()
                || std::env::var_os("SSH_TTY").is_some()
                || std::env::var_os("SSH_CLIENT").is_some(),
            // `setup_terminal` starts us in 1007 alternate-scroll, so the
            // terminal is NOT in real mouse reporting yet. `settle_mouse_mode`
            // turns it on at the first loop bottom if config asks for it.
            tab_state: None,
            scroll_last: None,
            mouse_press_forwarded: false,
            mouse_selection: None,
            list_selection: None,
            pane_selection: None,
            chrome_selection: None,
            chrome_rows: std::cell::RefCell::new(Vec::new()),
            last_chrome_click: None,
            pane_scroll_streak: None,
            pane_view_sent: None,
            transcript_show_tool_calls: true,
            term_size: crossterm::terminal::size().unwrap_or((80, 24)),
        }
    }
}

/// State for Tab-completion cycling. Tracks the original buffer, the
/// computed completions, and which one is currently filled in.
pub(super) struct TabState {
    /// Buffer content when the first Tab was pressed.
    pub(super) original_buf: String,
    /// Shell command prefix (e.g., "ls " for `!ls ~/Do<tab>`), empty for J prompt.
    pub(super) buf_prefix: String,
    /// Path prefix up to the last `/` in the typed word (e.g., "~/").
    pub(super) word_base: String,
    /// Matched file/dir names (e.g. `Documents/`, `Downloads/`).
    pub(super) matches: Vec<String>,
    /// 0 = list was just shown (first Tab). 1+ = cycling through matches.
    pub(super) cycle_index: usize,
}
