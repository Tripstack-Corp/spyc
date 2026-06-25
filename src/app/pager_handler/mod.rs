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

mod image;
mod modes;
mod motion;
mod pickers;

/// Which pager slot currently owns input. Decided once by
/// [`App::active_pager_slot`]; [`App::active_pager_ref`] and the
/// `active_pager_mut!` macro each just map it to the backing field, so the
/// (ref vs mut) pair can't drift.
#[derive(Clone, Copy)]
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
    /// Route a key to the pager overlay. Also uses vi-like motion so the
    /// pager feels native to the rest of the UI. Delegates each input
    /// context to a sub-handler (returning `Some` when it consumes the key,
    /// `None` to fall through); the final motion handler always consumes.
    pub fn handle_pager_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        // The full-screen image overlay sits on top of the pager and is modal:
        // intercept before any pager handler and route to its own verbs.
        if self.view.image_view.is_some() {
            return self.handle_image_view_key(key);
        }
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
        if let Some(r) = self.handle_pager_bracket(key) {
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
    /// renderer's cached `last_viewport_h`; falls back to the centered-
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
                let (_, term_h) = crossterm::terminal::size().unwrap_or((80, 24));
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
            // The left/single slot just closed — unpin its column before the
            // focus recompute so it resolves to the file list, not a stale pager.
            self.view.overlay_column = None;
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

    /// Install a freshly-spawned editor / `$PAGER` overlay PTY into the focused
    /// column's slot, then focus it. The right column (`b`) gets its own
    /// `top_overlay_right` slot — coexisting with a `V`/`D` in `a` and always
    /// auto-dismissing on exit (`prepare_panes`). The left / single / no-split
    /// case keeps the existing `top_overlay` slot with the auto-dismiss flag and
    /// the column pin. Shared by `V` (`edit_in_pane`), `D`-on-a-huge-file
    /// (`spawn_pager_overlay_for_path`), and the in-pager `v` editor handoff.
    /// Spawn `cmd` as a top-overlay PTY: the `top_overlay_size` geometry, the
    /// focused listing dir as cwd, the standard pane wake. Installs it into the
    /// focused column's overlay slot on success, or flashes the spawn error.
    /// Shared by the `V`/`D`-huge-file editor overlays and the in-pager `v`
    /// `TopPane` editor handoff (the block was copy-pasted at three sites).
    pub(super) fn spawn_top_overlay(&mut self, cmd: &str) {
        let (rows, cols) =
            Self::top_overlay_size(self.effective_pane_pct(), self.runtime.pane_tabs.is_some());
        let cwd = self.state.cur().listing.dir.clone();
        let wake = self.make_pane_wake();
        match Pane::spawn(cmd, rows, cols, &cwd, &self.view.context_path, wake) {
            Ok(p) => self.install_overlay_pty(p),
            Err(e) => self.state.flash_error(format!("spawn: {e}")),
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

    pub fn edit_in_pane(&mut self) {
        // Edit the FOCUSED column's cursor file. From the right commander the
        // editor opens INSIDE `b` (its own `top_overlay_right` slot), so it
        // coexists with a `V`/`D` already open in `a` instead of evicting it.
        let Some(row) = self.state.cur().rows.get(self.state.cur().cursor.index) else {
            return;
        };
        let path = row.path.clone();
        if row.kind == EntryKind::Dir
            || (row.kind == EntryKind::Symlink && crate::fs::target_is_dir(&path))
        {
            self.state.flash_error("V: cannot edit a directory");
            return;
        }
        let argv = shell::resolve_editor();
        if argv.is_empty() {
            self.state.flash_error("no $VISUAL or $EDITOR set");
            return;
        }
        let cmd = format!(
            "{} {}",
            argv.join(" "),
            shell::shell_quote(&path.display().to_string()),
        );
        self.spawn_top_overlay(&cmd);
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
    pub fn display_in_pane(&mut self) {
        // Page the FOCUSED column's cursor file. From the right commander the
        // pager opens INSIDE `b` (its own `pager_right` slot), coexisting with a
        // `V`/`D` already open in `a` instead of evicting it.
        let Some(row) = self.state.cur().rows.get(self.state.cur().cursor.index) else {
            return;
        };
        let path = row.path.clone();
        if row.kind == EntryKind::Dir
            || (row.kind == EntryKind::Symlink && crate::fs::target_is_dir(&path))
        {
            self.state.flash_error("D: cannot page a directory");
            return;
        }
        let file_size = std::fs::metadata(&path).map_or(0, |m| m.len());
        if file_size > crate::fs::ops::MAX_PAGER_BYTES {
            // Huge file: $PAGER's stream-from-disk wins over our
            // in-memory pager. Fall back to the pre-v1.5 behavior
            // (spawn $PAGER as a top overlay).
            self.spawn_pager_overlay_for_path(&path);
            return;
        }
        // With a vertical split open, `D`'s pager renders inside the focused
        // column (the carve scopes `top_unit` to it), so wrap to that column's
        // width — not the full terminal, or the markdown overflows the narrow
        // column. (`None` = full width, used when there's no split.)
        let wrap = self.state.vsplit.and_then(|v| {
            let (term_w, _) = crossterm::terminal::size().unwrap_or((80, 24));
            super::vsplit::vsplit_column_widths(term_w, v.width_pct).map(
                |(left_w, right_w)| match v.focus {
                    state::Side::Left => left_w,
                    state::Side::Right => right_w,
                },
            )
        });
        let Some(mut view) = self.build_pager_view_for_file(&path, wrap) else {
            return;
        };
        view.mount = crate::ui::pager::Mount::TopPane;
        // Don't push to buffer history: this is a fresh open, not a
        // page the user navigated away from and might want to revisit
        // via `[b` / `]b`.
        view.no_history = true;
        self.install_top_pager(view);
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
        let row = self.state.left.rows.get(self.state.left.cursor.index)?;
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
}

/// Pure load+render half of [`App::build_pager_view_for_file`]: read `path`
/// off disk and build a `PagerView` (markdown render / syntax highlight /
/// big-file truncation banner / binary hex-dump), wrapping markdown to
/// `wrap_width` (or the centered-overlay body width when `None`). No
/// `&mut self` — `theme` and `open_as_rendered` are passed in — so the
/// live-reload worker can call it on a detached thread. Returns the error
/// string (instead of flashing) on a read/decode failure; the caller decides
/// where it surfaces. Does NOT restore the per-file scroll position (the
/// `&mut self` wrapper does that, and the reload path preserves its own).
pub(super) fn build_pager_view(
    path: &Path,
    theme: &crate::ui::theme::Theme,
    open_as_rendered: bool,
    wrap_width: Option<u16>,
) -> Result<PagerView, String> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    // Refuse non-regular files BEFORE any read. A char device (`/dev/zero`,
    // `/dev/stderr` → the tty), FIFO, or socket has no finite contents:
    // `looks_like_text` below opens + reads the path, which *blocks* on a tty
    // (the reported "Enter on /dev/stderr locks up") or streams unbounded. The
    // `MAX_PAGER_BYTES` size cap can't save us either — their `metadata().len()`
    // is 0. `fs::metadata` follows symlinks, so `/dev/stderr` resolves to the
    // device. (A stat failure falls through to the existing read-error path.)
    if std::fs::metadata(path).is_ok_and(|m| !m.is_file()) {
        return Err(format!("{name}: not a regular file"));
    }
    if shell::looks_like_text(path) {
        let file_size = std::fs::metadata(path).map_or(0, |m| m.len());
        // Big files used to OOM us: read_to_string + syntect every
        // token = file size × ~50 in pager state. Cap at
        // MAX_PAGER_BYTES; past that, load just MAX_PAGER_LINES of
        // plain text and tell the user how to hand off to $PAGER
        // for the full thing.
        let load_result = if file_size > crate::fs::ops::MAX_PAGER_BYTES {
            crate::fs::ops::read_truncated(path, crate::fs::ops::MAX_PAGER_LINES)
        } else {
            std::fs::read_to_string(path).map(|c| {
                let n = c.lines().count();
                (c, n, false)
            })
        };
        let (content, _line_count, truncated) = match load_result {
            Ok(t) => t,
            Err(e) => {
                return Err(format!("read: {e}"));
            }
        };
        let content = expand_tabs(&content);
        let is_md = crate::ui::markdown::is_markdown_path(path);
        // JSON pretty-print: try parse + canonical re-emit. On a
        // successful parse with output differing from the raw
        // bytes, `lines` holds the pretty version and `alt_lines`
        // holds the raw (`m` toggles). Re-uses the alt-view
        // machinery currently named for markdown (`alt_lines`,
        // `markdown_rendered`) — the name stays markdown-specific even
        // though JSON pretty-print also rides it.
        let json_pretty: Option<String> = if !truncated && crate::ui::json::is_json_path(path) {
            crate::ui::json::pretty_print(&content)
        } else {
            None
        };
        // Source-side lines: syntect-highlighted if available AND
        // we loaded the whole file (highlighting a partial file
        // would still mostly work but blows memory, and the
        // savings is the whole point of truncation).
        let source_lines: Vec<ratatui::text::Line<'static>> = if truncated {
            content
                .lines()
                .map(|l| ratatui::text::Line::from(l.to_string()))
                .collect()
        } else {
            crate::ui::syntax::highlight_to_lines(&name, &content).unwrap_or_else(|| {
                content
                    .lines()
                    .map(|l| ratatui::text::Line::from(l.to_string()))
                    .collect()
            })
        };
        let mut view = if let Some(pretty) = json_pretty {
            // Pretty differs from raw: build a styled view of the
            // pretty bytes, stash the (already-highlighted) raw
            // lines as alt for the `m` toggle.
            let pretty_lines: Vec<ratatui::text::Line<'static>> =
                crate::ui::syntax::highlight_to_lines(&name, &pretty).unwrap_or_else(|| {
                    pretty
                        .lines()
                        .map(|l| ratatui::text::Line::from(l.to_string()))
                        .collect()
                });
            let mut v = PagerView::new_styled(name.clone(), pretty_lines);
            if pretty != content {
                v.alt_lines = Some(source_lines);
                // `markdown_rendered = true` semantically means
                // "the processed/alt-form is in `lines`". Same
                // interpretation for JSON: pretty is "rendered".
                v.markdown_rendered = true;
            }
            v
        } else if is_md && !truncated {
            // Pre-compute both views; `m` toggles. Yank/save
            // always hit the source via `source_text()`. Which
            // view shows first is configurable via
            // `[markdown] open_as_rendered`. Skipped for
            // truncated files since markdown rendering of half a
            // doc looks weird (broken refs, half-closed code
            // fences).
            // Hint the markdown renderer at the actual pager body
            // width so wide tables expand instead of wrapping into
            // the 80-col prose budget. Centered overlay pager
            // claims 90% of the terminal minus block borders;
            // matches the `pager_inner_area` math.
            //
            // Subtract the projected line-number gutter so a wide
            // table doesn't overflow the right edge of the
            // viewport. The gutter is `ilog10(lines) + 2` cells
            // wide (see `pager::render`); we don't yet know the
            // RENDERED line count (it can exceed the source's
            // because of soft-break-as-hard-break + table
            // expansion), so use 4× the source count as a
            // conservative estimate, which buys ~1 digit of
            // safety on the gutter.
            // A caller-supplied `wrap_width` (the right-split column) wins;
            // otherwise wrap to the centered overlay body width. The
            // terminal-size query is lazy so the off-thread reload path
            // (which always supplies a width) never touches the tty.
            let body_w = wrap_width.unwrap_or_else(|| {
                let (term_w, _) = crossterm::terminal::size().unwrap_or((80, 24));
                crate::ui::pager::centered_body_width(term_w)
            }) as usize;
            let source_line_count = content.lines().count().max(1);
            let gutter_w = (source_line_count.saturating_mul(4)).max(1).ilog10() as usize + 2;
            let pager_w = body_w.saturating_sub(2 + gutter_w);
            let doc = crate::ui::markdown::render_doc(&content, theme, Some(pager_w));
            let crate::ui::markdown::MarkdownDoc {
                lines: rendered,
                mermaid_blocks,
            } = doc;
            if open_as_rendered {
                let mut v = PagerView::new_styled(name, rendered);
                v.alt_lines = Some(source_lines);
                v.markdown_rendered = true;
                v.mermaid_blocks = mermaid_blocks;
                v
            } else {
                // Source first: `lines` holds source, `alt_lines`
                // holds the rendered view, `markdown_rendered`
                // is false. `m` swap is symmetric. The mermaid block
                // ranges index the rendered view, so `visible_mermaid_block`
                // ignores them until the user toggles to rendered.
                let mut v = PagerView::new_styled(name, source_lines);
                v.alt_lines = Some(rendered);
                v.markdown_rendered = false;
                v.mermaid_blocks = mermaid_blocks;
                v
            }
        } else {
            let display_name = if truncated {
                format!(
                    "{name} \u{26a0} truncated · {} MB",
                    file_size / (1024 * 1024)
                )
            } else {
                name
            };
            let mut v = PagerView::new_styled(display_name, source_lines);
            if truncated {
                // Append a banner row pointing at the escape
                // hatch so the user knows the cap fired and what
                // to do.
                let warn_style = ratatui::style::Style::default()
                    .fg(theme.pick)
                    .add_modifier(ratatui::style::Modifier::BOLD);
                v.lines.push(ratatui::text::Line::from(""));
                v.lines
                    .push(ratatui::text::Line::from(ratatui::text::Span::styled(
                        format!(
                            "[truncated at {} lines · {} MB total · press p to open in $PAGER]",
                            crate::fs::ops::MAX_PAGER_LINES,
                            file_size / (1024 * 1024)
                        ),
                        warn_style,
                    )));
                // Also flash an immediate hint — the banner is at
                // the bottom and the user might not scroll there
                // before wondering what happened to their file.
                v.flash = Some(format!(
                    "truncated at {} lines · press p for full file in $PAGER",
                    crate::fs::ops::MAX_PAGER_LINES
                ));
            }
            v
        };
        view.source_path = Some(path.to_path_buf());
        Ok(view)
    } else {
        // Binary file: hex dump via pretty-hex (styled in the ui layer).
        match crate::ui::hex::hex_dump_lines(path, theme) {
            Ok(lines) => {
                let mut view = PagerView::new_plain(format!("{name} [hex]"), Vec::new());
                view.lines = lines;
                Ok(view)
            }
            Err(e) => Err(format!("hex: {e}")),
        }
    }
}

impl App {
    /// Pre-v1.5 `D` behavior: spawn `\$PAGER` as a top overlay pty.
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

/// Expand tab characters to spaces at 8-column tab stops (the pager renders
/// with a fixed tab width). Sole caller is `build_pager_view_for_file`.
fn expand_tabs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for ch in s.chars() {
        if ch == '\t' {
            let spaces = 8 - (col % 8);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else if ch == '\n' {
            out.push(ch);
            col = 0;
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}
