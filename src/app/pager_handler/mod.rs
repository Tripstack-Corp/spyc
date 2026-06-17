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

use crossterm::event::{KeyCode, KeyEvent};

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
        if $self.state.pane_focused() && $self.view.scroll_pager.is_some() {
            $self.view.scroll_pager.as_mut()
        } else {
            $self.view.pager.as_mut()
        }
    };
}

mod modes;
mod motion;
mod pickers;

impl App {
    /// Route a key to the pager overlay. Also uses vi-like motion so the
    /// pager feels native to the rest of the UI. Delegates each input
    /// context to a sub-handler (returning `Some` when it consumes the key,
    /// `None` to fall through); the final motion handler always consumes.
    pub fn handle_pager_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        // The full-screen mermaid image overlay sits on top of the pager and is
        // modal: intercept before any pager handler. q/Esc/i/o dismiss it; every
        // other key is swallowed so nothing scrolls underneath it.
        if self.view.mermaid_image.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q' | 'i' | 'o')) {
                self.view.mermaid_image = None;
                self.view.needs_full_repaint = true;
            }
            return Vec::new();
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
        if let Some(r) = self.handle_pager_worktree_pick(key) {
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
        if let Some(view) = self.view.pager.as_ref()
            && let Some(path) = view.source_path.clone()
        {
            let scroll = view.scroll;
            self.view.pager_positions.record(&path, scroll);
        }
    }

    /// Close the active pager, persisting its scroll position first.
    /// Drop-in replacement for the raw `pager = None` assignment
    /// everywhere the user's reading position should survive close
    /// + reopen.
    pub fn clear_pager(&mut self) {
        self.remember_pager_position();
        self.view.pager = None;
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
    pub(super) const fn active_pager_ref(&self) -> Option<&PagerView> {
        if self.state.pane_focused() && self.view.scroll_pager.is_some() {
            self.view.scroll_pager.as_ref()
        } else {
            self.view.pager.as_ref()
        }
    }

    pub fn edit_in_pane(&mut self) {
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
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
        let (rows, cols) =
            Self::top_overlay_size(self.effective_pane_pct(), self.runtime.pane_tabs.is_some());
        let cwd = self.state.listing.dir.clone();
        let wake = self.make_pane_wake();
        match Pane::spawn(&cmd, rows, cols, &cwd, &self.view.context_path, wake) {
            Ok(p) => {
                self.runtime.top_overlay = Some(p);
                self.state.focus = state::Focus::Overlay;
            }
            Err(e) => self.state.flash_error(format!("spawn: {e}")),
        }
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
        let Some(row) = self.state.rows.get(self.state.cursor.index) else {
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
        let Some(mut view) = self.build_pager_view_for_file(&path) else {
            return;
        };
        view.mount = crate::ui::pager::Mount::TopPane;
        // Don't push to buffer history: this is a fresh open, not a
        // page the user navigated away from and might want to revisit
        // via `[b` / `]b`.
        view.no_history = true;
        self.set_pager(view);
        self.state.focus = state::Focus::Pager(pager::Mount::TopPane);
        self.view.needs_full_repaint = true;
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
    pub fn build_pager_view_for_file(&mut self, path: &Path) -> Option<PagerView> {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
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
                    self.state.flash_error(format!("read: {e}"));
                    return None;
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
                let (term_w, _) = crossterm::terminal::size().unwrap_or((80, 24));
                let body_w = crate::ui::pager::centered_body_width(term_w) as usize;
                let source_line_count = content.lines().count().max(1);
                let gutter_w = (source_line_count.saturating_mul(4)).max(1).ilog10() as usize + 2;
                let pager_w = body_w.saturating_sub(2 + gutter_w);
                let doc =
                    crate::ui::markdown::render_doc(&content, &self.view.theme, Some(pager_w));
                let crate::ui::markdown::MarkdownDoc {
                    lines: rendered,
                    mermaid_blocks,
                } = doc;
                if self.state.config.markdown.open_as_rendered {
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
                        .fg(self.view.theme.pick)
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
            // Restore the scroll position from the previous visit (if
            // any). Clamp to `lines.len() - 1` so a saved row that's
            // now past the end (file shrank) lands at the new last
            // line rather than blanking the viewport.
            if let Some(saved) = self.view.pager_positions.get(path) {
                let last = view.lines.len().saturating_sub(1);
                view.scroll = saved.min(u16::try_from(last).unwrap_or(u16::MAX));
            }
            Some(view)
        } else {
            // Binary file: hex dump via pretty-hex (styled in the ui layer).
            match crate::ui::hex::hex_dump_lines(path, &self.view.theme) {
                Ok(lines) => {
                    let mut view = PagerView::new_plain(format!("{name} [hex]"), Vec::new());
                    view.lines = lines;
                    Some(view)
                }
                Err(e) => {
                    self.state.flash_error(format!("hex: {e}"));
                    None
                }
            }
        }
    }

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
        let (rows, cols) =
            Self::top_overlay_size(self.effective_pane_pct(), self.runtime.pane_tabs.is_some());
        let cwd = self.state.listing.dir.clone();
        let wake = self.make_pane_wake();
        match Pane::spawn(&cmd, rows, cols, &cwd, &self.view.context_path, wake) {
            Ok(p) => {
                self.runtime.top_overlay = Some(p);
                self.state.focus = state::Focus::Overlay;
            }
            Err(e) => self.state.flash_error(format!("spawn: {e}")),
        }
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
