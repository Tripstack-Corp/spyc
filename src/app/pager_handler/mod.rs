//! Pager key dispatch: `handle_pager_key`, the vi-style key router for
//! the in-app pager overlay (search results, git show/diff/blame, help,
//! task viewer, find/grep pickers, captured `!` output…).
//!
//! Extracted from `app/mod.rs` (REFACTOR_PLAN Phase 2). Like `render`,
//! this is an `impl App` method in a child module, so it reads App's
//! private state directly via the descendant-module rule — no field is
//! made `pub`. It's `pub` because the key-routing path in `app` calls
//! it. The many `self.*` helpers it delegates to (clear_pager,
//! restore_session, start_capture, task pause/resume…) stay in `app`.

use std::path::Path;

use crossterm::event::KeyEvent;

use crate::pane::Pane;
use crate::shell;
use crate::ui::pager;

use super::file_ops::{FileOp, PagerDest};
use super::{App, Effect, EntryKind, PagerView, state};

/// `&mut` selector for the focused-region pager — the macro companion to
/// [`App::active_pager_ref`]. Inlined (not a method) so the borrow stays
/// field-level: a method returning `&mut PagerView` would borrow all of `*self`
/// and collide with handlers that touch sibling fields (history pick, config)
/// while holding the pager. Yields `Option<&mut PagerView>` — use with
/// `?` or `let Some(view) = active_pager_mut!(self) else { … }`.
macro_rules! active_pager_mut {
    ($self:ident) => {
        // One decision ([`App::active_pager_slot`]) shared with `active_pager_ref`;
        // this only maps the slot to its field (`&mut`). The slot pick is a
        // separate `&self` call returning a `Copy` enum, so the borrow here stays
        // field-level — a `fn(&mut self) -> &mut PagerView` would borrow all of
        // `*self` and collide with handlers touching sibling fields.
        match $self.active_pager_slot() {
            $crate::app::pager_handler::PagerSlot::Modal
            | $crate::app::pager_handler::PagerSlot::Top => $self.view.pager.as_mut(),
            $crate::app::pager_handler::PagerSlot::Scrollback => $self.view.scroll_pager.as_mut(),
            $crate::app::pager_handler::PagerSlot::Right => $self
                .view
                .pager_right
                .as_mut()
                .or($self.view.right_pager.as_mut()),
        }
    };
}

mod file_view;
mod image;
mod modes;
mod motion;
mod pickers;

pub(super) use file_view::build_pager_view;

/// Which pager slot currently owns input. Decided once by
/// [`App::active_pager_slot`]; [`App::active_pager_ref`] and the
/// `active_pager_mut!` macro each just map it to the backing field, so the
/// (ref vs mut) pair can't drift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagerSlot {
    /// A full-frame modal — grep / git-view / help / `;cmd` output → `view.pager`.
    Modal,
    /// The bottom-pane `^a v` scrollback → `view.scroll_pager`.
    Scrollback,
    /// The focused right vsplit column → its `D` pager (`view.pager_right`),
    /// else the live preview (`view.right_pager`).
    Right,
    /// The shared top / left pager → `view.pager`.
    Top,
}

impl App {
    /// Route a key to the pager overlay, then re-home any status-bar flash the
    /// handler raised into the pager itself (#166 — see
    /// [`Self::relocate_flash_into_pager`]). `view.input_went_to_pager` carries
    /// the same relocation past `run_effects`, for a message an executor emits.
    pub fn handle_pager_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        self.view.input_went_to_pager = true;
        let effects = self.dispatch_pager_key(key);
        self.relocate_flash_into_pager();
        effects
    }

    /// Move a status-bar flash raised by a pager keypress into the pager that
    /// still owns the screen: the pager is drawn over the status bar, so the
    /// message would render half-occluded. A handler whose result lands on the
    /// file list closed its pager first, leaving nothing to relocate into — so
    /// those messages stay on the status bar without needing a per-site opt-out.
    pub(crate) fn relocate_flash_into_pager(&mut self) {
        if self.active_pager_ref().is_none() {
            return;
        }
        if let Some(msg) = self.state.flash.take() {
            self.set_active_pager_flash(msg.text);
        }
    }

    fn dispatch_pager_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        // No image-overlay check here: `Modal::ImageView` takes the key before
        // routing ever reaches a pager sink. This used to intercept, which only
        // worked while the overlay was reachable *from a pager* — once it could
        // open from the file list or the gallery, a pane-focused key went to the
        // child instead and the overlay was undismissable.
        let Some(view) = active_pager_mut!(self) else {
            return Vec::new();
        };
        // Clear any one-shot flash message from the previous keypress.
        view.flash = None;

        if let Some(r) = self.handle_pager_ctrl_c(key) {
            return r;
        }

        let viewport = self.pager_viewport();

        if let Some(r) = self.handle_pager_search_typing(key, viewport) {
            return r;
        }
        if let Some(r) = self.handle_pager_jump_buf(key) {
            return r;
        }
        if let Some(r) = self.handle_pager_bracket(key, viewport) {
            return r;
        }
        if let Some(r) = self.handle_pager_jump_history(key, viewport) {
            return r;
        }
        if let Some(r) = self.handle_pager_worktree_pick(key, viewport) {
            return r;
        }
        if let Some(r) = self.handle_pager_history_editor(key, viewport) {
            return r;
        }
        if let Some(r) = self.handle_pager_session_pick(key, viewport) {
            return r;
        }
        if let Some(r) = self.handle_pager_placement(key, viewport) {
            return r;
        }
        if let Some(r) = self.handle_pager_visual(key, viewport) {
            return r;
        }
        self.handle_pager_motion(key, viewport)
    }

    /// The pager's content viewport height (body rows). Prefers the
    /// renderer's cached `last_viewport_h`; falls back to the centred-
    /// overlay heuristic only before the first frame has run.
    fn pager_viewport(&self) -> u16 {
        let Some(view) = self.active_pager_ref() else {
            return 2;
        };
        {
            let cached = view.last_viewport_h.get();
            if cached >= 2 {
                cached
            } else {
                let (_, term_h) = self.view.term_size;
                let pager_h = if view.full_width {
                    term_h
                } else {
                    (u32::from(term_h) * 92 / 100) as u16
                };
                pager_h.saturating_sub(2).max(2)
            }
        }
    }
}

/// Pager open/close/build hub: assign/clear the active pager (persisting
/// scroll position), build a `PagerView` from a file (text + markdown/JSON/
/// syntax + hex), and the `V`/`D` editor / pager-overlay spawns. Extracted
/// verbatim from `app/mod.rs` (the impl-extraction sweep). All `pub` — these
/// are called from many sites across `app` and its siblings (effect, actions,
/// session, tasks, navigate, git, …).
impl App {
    /// Persist the current pager's scroll position to disk if it's a
    /// file-backed view (`source_path` is set). Call before any
    /// assignment that drops or replaces `self.view.pager` so the user's
    /// reading position survives close + reopen. No-op for command
    /// output, help, picker UIs, etc. — those views intentionally
    /// don't carry a `source_path`.
    pub fn remember_pager_position(&mut self) {
        // The active pager (focused column's `D`, the modal, or the preview) —
        // so a right-column `D`'s scroll survives close+reopen too.
        if let Some((path, scroll)) = self
            .active_pager_ref()
            .and_then(|v| v.source_path.clone().map(|p| (p, v.scroll)))
        {
            self.view.pager_positions.record(&path, scroll as u64);
        }
    }

    /// Close the active pager, persisting its scroll position first.
    /// Drop-in replacement for the raw `pager = None` assignment
    /// everywhere the user's reading position should survive close
    /// + reopen.
    pub fn clear_pager(&mut self) {
        self.remember_pager_position();
        // Close whichever top pager the focused column owns: the right column's
        // own `D` slot, else the shared (left / modal / no-split) slot.
        if !self.state.pane_focused()
            && self.focused_side() == state::Side::Right
            && self.view.pager_right.is_some()
        {
            // `pager_right` doesn't use `overlay_column` (the right slot's column
            // is implicit) — leave it alone so an OPEN left pager stays pinned.
            self.view.pager_right = None;
        } else {
            self.view.pager = None;
            // Unpin the column only when nothing else is still anchored to it.
            // A `V`/`;` top-overlay pty can still be open in that column (a
            // modal `Overlay` pager like `?` help opened *over* the editor after
            // `^a l` moved focus to `b`), and it needs the pin to keep rendering
            // in place — clearing it would move the editor into the other column
            // (blanking this one, artifacting that one).
            if self.runtime.top_overlay.is_none() {
                self.view.overlay_column = None;
            }
        }
        // The top pager just closed — drop any stale `Pager(_)` focus this
        // frame (the loop top would catch it next tick regardless).
        self.recompute_focus();
    }

    /// Tear down a `^a-v` scrollback pager: snap the pty back to
    /// live, clear the pager, force a repaint, and flash the
    /// status change. Mirrors the Esc/q close path so chord-driven
    /// and focus-switch escapes land in the same final state. No-op
    /// when no pane_scroll pager is open — so the scrollback Esc/`q`
    /// close path (`pager_handler::motion`) can call it unconditionally.
    pub fn close_pane_scroll_pager(&mut self) {
        if self.view.scroll_pager.is_none() {
            return;
        }
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            tabs.active_mut().exit_scroll_mode();
        }
        // The scrollback lives in its own region slot; the top/overlay pager
        // (if any) stays put.
        self.view.scroll_pager = None;
        // A `T` flip lives exactly as long as the view it was made in, so the
        // next `^a v` starts from the automatic source choice again.
        self.view.scroll_source_override = None;
        self.view.needs_full_repaint = true;
        self.recompute_focus();
        self.state.flash_info("scroll: off");
    }

    /// Assign a new pager view, persisting the outgoing view's
    /// scroll position first. Drop-in replacement for the bare
    /// `pager = Some(view)` pattern at open / replace sites
    /// — covers the case where the user has one file open, opens
    /// another, then later wants to come back to the first one.
    pub fn set_pager(&mut self, view: PagerView) {
        self.remember_pager_position();
        self.view.pager = Some(view);
    }

    /// Build a [`PagerView`] from a [`state::PagerRequest`] and install it.
    /// The single place that turns the pure-side "open a pager with these
    /// lines" request into the view — shared by the `Update::OpenPager`
    /// bridge (`actions.rs`) and the off-thread listing/file-type outcomes
    /// (`file_ops.rs`), so the `columns` / `fit_to_content` handling can't
    /// drift between them.
    pub(crate) fn open_pager_request(&mut self, req: state::PagerRequest) {
        let mut view = PagerView::new_plain(req.title, req.lines);
        view.columns = req.columns;
        if req.fit_to_content {
            view.fit_to_content = true;
            // Line-number gutter is noise for short summaries.
            view.show_line_numbers = false;
        }
        self.set_pager(view);
    }

    /// Spawn `cmd` as a top-overlay editor/`$PAGER` PTY (overlay geometry,
    /// focused listing dir as cwd) and install it into the focused column's
    /// slot, then focus it. The right column (`b`) uses its own
    /// `top_overlay_right` slot so it coexists with a `V`/`D` in `a`; both
    /// auto-dismiss on exit. Shared by `V` (`edit_in_pane`), `D` on a huge file,
    /// and the in-pager `v` editor handoff.
    pub(super) fn spawn_top_overlay(&mut self, cmd: &str) {
        let (rows, cols) =
            Self::top_overlay_size(self.effective_pane_pct(), self.runtime.pane_tabs.is_some());
        let cwd = self.state.cur().listing.dir.clone();
        let wake = self.make_pane_wake();
        match Pane::spawn(cmd, rows, cols, &cwd, &self.view.context_path, wake) {
            Ok(p) => self.install_overlay_pty(p),
            Err(e) => self.state.flash_error(format!("spawn: {e:#}")),
        }
    }

    pub(super) fn install_overlay_pty(&mut self, p: Pane) {
        if self.overlay_targets_right() {
            self.runtime.top_overlay_right = Some(p);
        } else {
            self.runtime.top_overlay = Some(p);
            // Interactive overlay (editor / pager): on exit, return to spyc
            // immediately rather than holding a "press any key" frame.
            self.view.overlay_auto_dismiss = true;
            // This is the LEFT / single slot, so it scopes to the left column
            // when split (`None` when no split). The right column uses its own
            // slot above and never sets this.
            self.view.overlay_column = self.state.vsplit.map(|_| state::Side::Left);
        }
        self.state.focus = state::Focus::Overlay;
    }

    /// Install a `D` `TopPane` pager into the focused column's slot, then focus
    /// it. Right column (`b`) → its own `pager_right` slot (coexists with a
    /// `V`/`D` in `a`); left / single / no-split → the shared `pager` slot with
    /// the column pin. Mirrors [`Self::install_overlay_pty`] for the in-process
    /// pager case.
    pub(super) fn install_top_pager(&mut self, view: PagerView) {
        if self.overlay_targets_right() {
            self.remember_pager_position();
            self.view.pager_right = Some(view);
        } else {
            self.set_pager(view);
            // LEFT / single slot → scopes to the left column when split (`None`
            // with no split). The right column uses its own `pager_right` slot.
            self.view.overlay_column = self.state.vsplit.map(|_| state::Side::Left);
        }
        self.state.focus = state::Focus::Pager(pager::Mount::TopPane);
        self.view.needs_full_repaint = true;
    }

    /// The pager the key handlers act on (read-only). The bottom pane-scrollback
    /// (`view.scroll_pager`) and the top/overlay pager (`view.pager`) live in
    /// separate region slots so a `D` top pager and a `^a v` bottom scrollback
    /// coexist; this picks the one that currently owns input. Keys reach a
    /// pager handler only when its region is focused (see `route_key`), so:
    /// pane focused + a scrollback open → the scrollback; otherwise the
    /// top/overlay pager.
    ///
    /// The `&mut` companion is the [`active_pager_mut!`] macro, not a method:
    /// a `fn(&mut self) -> &mut PagerView` borrows all of `*self` for the
    /// return's lifetime, which collides with handlers that also touch sibling
    /// fields (`pending_history_pick`, `state.config`) while
    /// holding the pager. The macro inlines the slot pick so the borrow stays
    /// field-level (`view.pager` / `view.scroll_pager` only).
    pub(super) fn active_pager_ref(&self) -> Option<&PagerView> {
        match self.active_pager_slot() {
            PagerSlot::Modal | PagerSlot::Top => self.view.pager.as_ref(),
            PagerSlot::Scrollback => self.view.scroll_pager.as_ref(),
            PagerSlot::Right => self
                .view
                .pager_right
                .as_ref()
                .or(self.view.right_pager.as_ref()),
        }
    }

    /// Set the flash line on whichever pager currently owns input — how the
    /// `CopyToPagerClipboard` / `SavePagerOutput` executors land a yank/save
    /// confirmation in the pager title (where the user is looking). Mirrors
    /// `active_pager_mut!` without the macro, which the `effect` module can't see.
    pub(crate) fn set_active_pager_flash(&mut self, msg: String) {
        let view = match self.active_pager_slot() {
            PagerSlot::Modal | PagerSlot::Top => self.view.pager.as_mut(),
            PagerSlot::Scrollback => self.view.scroll_pager.as_mut(),
            PagerSlot::Right => self
                .view
                .pager_right
                .as_mut()
                .or(self.view.right_pager.as_mut()),
        };
        if let Some(view) = view {
            view.flash = Some(msg);
        }
    }

    /// The pager in `slot`, mutably — a direct field lookup, with no reference to
    /// which region has focus.
    ///
    /// That independence is the point. Resolving a *pointer*-derived gesture
    /// through the focus-derived [`Self::active_pager_slot`] ladder made the result
    /// depend on `focus_region` having already succeeded, which it declines to do
    /// while `^a z`-zoomed — so a click could land on one pager and mutate another,
    /// or silently nothing.
    pub(super) fn pager_in_slot_mut(&mut self, slot: PagerSlot) -> Option<&mut PagerView> {
        match slot {
            PagerSlot::Modal | PagerSlot::Top => self.view.pager.as_mut(),
            PagerSlot::Scrollback => self.view.scroll_pager.as_mut(),
            PagerSlot::Right => self
                .view
                .pager_right
                .as_mut()
                .or(self.view.right_pager.as_mut()),
        }
    }

    /// Which pager, if any, is painted under `(col, row)`.
    ///
    /// **This is what gives an open pager priority over the layout region beneath
    /// it.** `region_at` answers from `compute_layout` geometry, but a pager is
    /// drawn *over* that geometry: an `Overlay` renders into `frame.area()` and a
    /// `^a v` scrollback renders into the pane's own rect. Since `region_at` tests
    /// `layout.pane` first, both used to resolve as `Region::Pane` — so the wheel
    /// scrolled the agent *behind* a full-screen pager, and a click was forwarded
    /// to it (which is what made claude start its own selection under the pager).
    ///
    /// Answered from each pager's `last_content_area`, the rect the renderer
    /// actually drew it into — the same source of truth `PagerView::hit_test` uses,
    /// so "which pager did I click" and "which character did I click" can't
    /// disagree. Content rect, not the whole box: beside a centred overlay the
    /// user genuinely sees the list, and clicking there should reach the list.
    ///
    /// Checked in paint order (topmost first). A slot holding a pager that is not
    /// currently drawn would carry a stale rect; in practice a pager is `take`n out
    /// of its slot when it stops being drawn (closed, stashed on tab switch, split
    /// closed), so an occupied slot is a drawn slot.
    pub(super) fn pager_slot_at(&self, col: u16, row: u16) -> Option<PagerSlot> {
        let covers = |v: &PagerView| {
            let a = v.last_content_area.get();
            a.width > 0
                && a.height > 0
                && col >= a.x
                && row >= a.y
                && col < a.x.saturating_add(a.width)
                && row < a.y.saturating_add(a.height)
        };
        // A full-frame modal paints last, so it wins wherever it covers.
        if let Some(v) = self.view.pager.as_ref()
            && matches!(v.mount, crate::ui::pager::Mount::Overlay)
            && covers(v)
        {
            return Some(PagerSlot::Modal);
        }
        if let Some(v) = self.view.scroll_pager.as_ref().filter(|v| covers(v)) {
            let _ = v;
            return Some(PagerSlot::Scrollback);
        }
        if let Some(v) = self
            .view
            .pager_right
            .as_ref()
            .or(self.view.right_pager.as_ref())
            .filter(|v| covers(v))
        {
            let _ = v;
            return Some(PagerSlot::Right);
        }
        if let Some(v) = self.view.pager.as_ref().filter(|v| covers(v)) {
            let _ = v;
            return Some(PagerSlot::Top);
        }
        None
    }

    /// Scroll the pager in `slot` by `delta` lines. Targets the slot the pointer is
    /// over, not the focused one, so the wheel scrolls what you are looking at.
    pub(super) fn scroll_pager_in_slot(&mut self, slot: PagerSlot, delta: i32) {
        let viewport = self.pager_viewport();
        // A wheel-down that can't move in a `^a v` scrollback exits it, the same as
        // the keyboard's `j` — see `exit_scrollback_past_the_end`. Decided against
        // THIS slot rather than the focused pager: the wheel follows the pointer, so
        // the view under it is the one that gets to close.
        let exit_scrollback = self.pager_in_slot_mut(slot).is_some_and(|view| {
            // Prefer the height the renderer recorded for THIS pager; `pager_viewport`
            // reports the focused one, which is a different box when the pointer is
            // over the other column.
            let h = view.last_viewport_h.get();
            let h = if h >= 2 { h } else { viewport };
            let is_scrollback = view.pane_scroll;
            let moved = motion::scrolled_down(view, delta, h);
            is_scrollback && delta > 0 && !moved
        });
        if exit_scrollback {
            self.close_pane_scroll_pager();
        }
    }

    /// Decide which pager slot owns input — the single source of truth shared by
    /// [`Self::active_pager_ref`] and the `active_pager_mut!` macro. A flat
    /// priority ladder: a full-frame modal first, then the focused region (right
    /// column / bottom-pane scrollback), else the shared top pager.
    pub(super) fn active_pager_slot(&self) -> PagerSlot {
        if self.modal_pager_open() {
            PagerSlot::Modal
        } else if !self.state.pane_focused() && self.focused_side() == state::Side::Right {
            PagerSlot::Right
        } else if self.state.pane_focused() && self.view.scroll_pager.is_some() {
            PagerSlot::Scrollback
        } else {
            PagerSlot::Top
        }
    }

    /// Take (remove + return) the pager that currently owns input — the same
    /// slot [`Self::active_pager_ref`] reads. Used by the `q`/Esc close path to
    /// move the focused pager into history without disturbing the OTHER column's
    /// pager (a hardcoded `view.pager.take()` would evict `a`'s pager while
    /// closing `b`'s — both columns went dark).
    pub(super) fn take_active_pager(&mut self) -> Option<PagerView> {
        match self.active_pager_slot() {
            PagerSlot::Modal | PagerSlot::Top => self.view.pager.take(),
            PagerSlot::Scrollback => self.view.scroll_pager.take(),
            PagerSlot::Right => self
                .view
                .pager_right
                .take()
                .or_else(|| self.view.right_pager.take()),
        }
    }

    /// A full-frame modal pager (grep / git-view / help / `;cmd` output) is open
    /// in the shared slot — it owns input regardless of which column is focused.
    fn modal_pager_open(&self) -> bool {
        matches!(
            self.view.pager.as_ref().map(|v| v.mount),
            Some(crate::ui::pager::Mount::Overlay)
        )
    }

    pub fn edit_in_pane(&mut self) -> Vec<Effect> {
        // Edit the FOCUSED column's cursor file. From the right commander the
        // editor opens INSIDE `b` (its own `top_overlay_right` slot), so it
        // coexists with a `V`/`D` already open in `a` instead of evicting it.
        let Some(row) = self.state.cur().rows.get(self.state.cur().cursor.index) else {
            return Vec::new();
        };
        let path = row.path.clone();
        if row.kind == EntryKind::Dir
            || (row.kind == EntryKind::Symlink && crate::fs::target_is_dir(&path))
        {
            self.state.flash_error("V: cannot edit a directory");
            return Vec::new();
        }
        // A member has no bytes on disk yet, so it is extracted first and the
        // editor opens on the extracted copy.
        if self.state.mounts.holds_member(&path) {
            return self.open_member(
                &path,
                crate::app::archive_ops::MaterializeThen::Edit { in_pane: true },
            );
        }
        let argv = shell::resolve_editor();
        if argv.is_empty() {
            self.state.flash_error("no $VISUAL or $EDITOR set");
            return Vec::new();
        }
        let cmd = format!(
            "{} {}",
            argv.join(" "),
            shell::shell_quote(&path.display().to_string()),
        );
        self.spawn_top_overlay(&cmd);
        Vec::new()
    }

    /// `D` — open the cursor file in spyc's in-app pager mounted in
    /// the top-pane slot, so the bottom pane (claude / zsh / etc.)
    /// stays visible. Mirror of `edit_in_pane` for the read path.
    /// Common workflow: `D` on a doc, `^a-j` into claude, work,
    /// `^a-k` back to scroll.
    ///
    /// v1.5 Phase 5 swapped the implementation from
    /// "spawn `\$PAGER` as a pty top overlay" to "use the in-app
    /// pager." The pager is more capable on every axis we care about
    /// (search, jump, syntax highlighting, range yank, markdown
    /// render, hex dump for binaries), and uses the existing
    /// `Mount::TopPane` rail laid in Phase 1.
    ///
    /// **Huge-file fallback:** files past `MAX_PAGER_BYTES` are
    /// still handed to `\$PAGER` as a top overlay because `less`
    /// streams from disk while the in-app pager loads the (already
    /// truncated) buffer into memory. Streaming wins for multi-GB
    /// logs.
    pub fn display_in_pane(&mut self) -> Vec<Effect> {
        // Page the FOCUSED column's cursor file. From the right commander the
        // pager opens INSIDE `b` (its own `pager_right` slot), coexisting with a
        // `V`/`D` already open in `a` instead of evicting it.
        let Some(row) = self.state.cur().rows.get(self.state.cur().cursor.index) else {
            return Vec::new();
        };
        let path = row.path.clone();
        if row.kind == EntryKind::Dir
            || (row.kind == EntryKind::Symlink && crate::fs::target_is_dir(&path))
        {
            self.state.flash_error("D: cannot page a directory");
            return Vec::new();
        }
        let file_size = std::fs::metadata(&path).map_or(0, |m| m.len());
        if file_size > crate::fs::ops::MAX_PAGER_BYTES {
            // Huge file: $PAGER's stream-from-disk wins over our
            // in-memory pager. Fall back to the pre-v1.5 behaviour
            // (spawn $PAGER as a top overlay). A char device reports
            // length 0, so it never lands here — it routes to the
            // device-safe off-thread open below.
            self.spawn_pager_overlay_for_path(&path);
            return Vec::new();
        }
        // With a vertical split open, `D`'s pager renders inside the focused
        // column (the carve scopes `top_unit` to it), so wrap to that column's
        // width — not the full terminal, or the markdown overflows the narrow
        // column. (`None` = full width, used when there's no split.)
        let wrap = self.state.vsplit.and_then(|v| {
            let (term_w, _) = self.view.term_size;
            super::vsplit::vsplit_column_widths(term_w, v.width_pct).map(
                |(left_w, right_w)| match v.focus {
                    state::Side::Left => left_w,
                    state::Side::Right => right_w,
                },
            )
        });
        // Regular file → built + mounted inline (no thread/flicker); a special
        // file → read off-thread (returned as the `OpenSpecialFile` effect) so a
        // blocking device can't freeze the input thread.
        self.plan_pager_open(&path, wrap, PagerDest::TopPane)
            .map(Effect::FileOp)
            .into_iter()
            .collect()
    }

    /// Load a one-shot preview of the file under the cursor into the right
    /// column of a vertical split (`view.right_pager`, `Mount::RightPane`).
    /// Mirrors `display_in_pane`'s dir/size checks; a directory or oversized
    /// file leaves the right region blank (the split still opens). PR5 makes
    /// this re-render automatically when the file changes on disk.
    /// The path under the cursor if it's previewable in the right split — a
    /// readable file, **not** a directory (huge files page truncated). `None`
    /// for a directory (or no row); the caller flashes the warning.
    pub(super) fn previewable_cursor_path(&self) -> Option<std::path::PathBuf> {
        let col = self.state.cur();
        let row = col.rows.get(col.cursor.index)?;
        let path = row.path.clone();
        let is_dir = row.kind == EntryKind::Dir
            || (row.kind == EntryKind::Symlink && crate::fs::target_is_dir(&path));
        (!is_dir).then_some(path)
    }

    /// Load `path` into the right-split preview slot (`Mount::RightPane`),
    /// wrapping markdown to the right column's width (not the full terminal,
    /// or long lines overflow the narrow column). Caller has already checked
    /// `previewable_cursor_path`. Returns `true` iff the file loaded and the
    /// slot was (re)assigned; on a read/render failure `build_pager_view_for_file`
    /// flashes the error, this leaves any existing preview untouched, and it
    /// returns `false` — so callers can avoid announcing a file they didn't
    /// actually show, or restoring a split that never gained content.
    pub(super) fn load_right_preview(&mut self, path: &std::path::Path) -> bool {
        let wrap = self.right_preview_body_width();
        if let Some(mut view) = self.build_pager_view_for_file(path, Some(wrap)) {
            view.mount = crate::ui::pager::Mount::RightPane;
            view.no_history = true;
            self.view.right_pager = Some(view);
            true
        } else {
            false
        }
    }

    /// Build a `PagerView` from a file on disk. Handles text (with
    /// markdown rendering / syntax highlighting / truncation banner
    /// for big files) and binary (hex dump). Flashes a read error
    /// and returns `None` on failure. The returned view has
    /// `mount = Overlay` (the default); callers override for
    /// `TopPane` / `LowerPane` mounts. Extracted from the old
    /// inline body of `ActivateIntent::Display` so both `Enter` /
    /// `d` (overlay) and `D` (top pane) share the same loading
    /// path.
    ///
    /// The heavy load+render is the pure free fn [`build_pager_view`] (no
    /// `&mut self`), so the live-reload worker (`preview_ops`) can run it
    /// off-thread; this method adds the `&mut self` glue — flash on error,
    /// and the per-file scroll-position restore — that only makes sense on
    /// the main thread.
    pub fn build_pager_view_for_file(
        &mut self,
        path: &Path,
        wrap_width: Option<u16>,
    ) -> Option<PagerView> {
        match build_pager_view(
            path,
            &self.view.theme,
            self.state.config.markdown.open_as_rendered,
            wrap_width,
        ) {
            Ok(mut view) => {
                view.tab_width = self.state.config.pager.tab_width;
                // Restore the scroll position from the previous visit (if any).
                if let Some(saved) = self.view.pager_positions.get(path) {
                    let last = view.lines.len().saturating_sub(1);
                    view.scroll = usize::try_from(saved).unwrap_or(usize::MAX).min(last);
                    // Then clamp to the document END for the viewport, not just
                    // the last line — a saved row near the bottom (e.g. from a
                    // taller/wider column) would otherwise sit at the viewport
                    // TOP with everything below EOF blank. Uses the last rendered
                    // height (or a 40-row guess on the first frame). Keeps EOF
                    // pinned to the bottom so the last page fills the view.
                    view.clamp_scroll_auto();
                }
                Some(view)
            }
            Err(e) => {
                self.state.flash_error(e);
                None
            }
        }
    }

    /// An `Effect::OpenImage` for `path` when it's an image spyc can decode.
    ///
    /// Detection is by magic bytes, not extension: a screenshot saved without a
    /// suffix is still an image, and a `.png` holding text is not. Callers put
    /// this ahead of their normal open so an image lands in the full-screen
    /// overlay rather than a hex dump.
    pub(crate) fn plan_image_open(&self, path: &Path) -> Option<Effect> {
        if !crate::fs::ops::is_decodable_image(path) {
            return None;
        }
        let (cols, rows) = self.view.term_size;
        Some(Effect::OpenImage(crate::app::image_ops::ImageOpenOp {
            path: path.to_path_buf(),
            cols,
            // Leave the footer row to the overlay's verb hints.
            rows: rows.saturating_sub(1),
        }))
    }

    /// Plan opening `path` in a pager at `dest`, off-loading the read of a
    /// *non-regular* file (char/block device, …) onto the file-op worker so a
    /// blocking read can't freeze the input thread. Returns:
    ///
    /// * `None` — `path` is a regular file (the common case): it was built and
    ///   installed **inline**, here, synchronously (no thread spawn, no
    ///   one-frame "computing" flicker, an immediate read error flashes). A stat
    ///   failure also counts as "regular" so a missing/typo'd path flashes its
    ///   real error now rather than after a worker round-trip.
    /// * `Some(op)` — `path` is non-regular: the caller runs `op` (a pure-side
    ///   caller wraps it in `Effect::FileOp`; the executor-layer `gF` caller
    ///   passes it to [`App::spawn_file_op`]). The worker reads it (sampling a
    ///   readable device, parking on a blocking one) and the drain installs the
    ///   result at `dest`.
    ///
    /// `fs::metadata` follows symlinks (a symlink to a device is non-regular),
    /// matching `build_pager_view`'s special-file guard.
    pub(crate) fn plan_pager_open(
        &mut self,
        path: &Path,
        wrap: Option<u16>,
        dest: PagerDest,
    ) -> Option<FileOp> {
        let is_regular = std::fs::metadata(path).map_or(true, |m| m.is_file());
        if is_regular {
            if let Some(view) = self.build_pager_view_for_file(path, wrap) {
                self.install_pager_at_dest(view, dest);
            }
            return None;
        }
        Some(FileOp::OpenSpecialFile {
            path: path.to_path_buf(),
            theme: self.view.theme.clone(),
            open_as_rendered: self.state.config.markdown.open_as_rendered,
            wrap,
            dest,
        })
    }

    /// Install an already-built `view` at `dest`. Shared by the inline regular
    /// path ([`Self::plan_pager_open`]) and the off-thread special-file drain
    /// ([`Self::apply_file_outcomes`]), so the mount + scroll handling can't
    /// drift between them.
    pub(crate) fn install_pager_at_dest(&mut self, mut view: PagerView, dest: PagerDest) {
        match dest {
            PagerDest::Overlay { scroll } => {
                // gF lands at its referenced line; Enter keeps the builder's
                // position (a regular file's restored scroll, or the top).
                if let Some(s) = scroll {
                    view.scroll = s;
                }
                self.set_pager(view);
            }
            PagerDest::TopPane => {
                view.mount = crate::ui::pager::Mount::TopPane;
                // A fresh open, not a page navigated away from → keep it out of
                // the `[b`/`]b` buffer history.
                view.no_history = true;
                self.install_top_pager(view);
            }
        }
    }
}

impl App {
    /// Pre-v1.5 `D` behaviour: spawn `\$PAGER` as a top overlay pty.
    /// Now used only as the huge-file fallback path from
    /// `display_in_pane` — files past `MAX_PAGER_BYTES` benefit from
    /// `less`'s stream-from-disk over our in-memory pager.
    pub fn spawn_pager_overlay_for_path(&mut self, path: &Path) {
        let argv = shell::resolve_pager();
        if argv.is_empty() {
            self.state.flash_error("no $PAGER set");
            return;
        }
        let cmd = format!(
            "{} {}",
            argv.join(" "),
            shell::shell_quote(&path.display().to_string()),
        );
        self.spawn_top_overlay(&cmd);
    }
}
