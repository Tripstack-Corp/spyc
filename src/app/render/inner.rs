//! The main draw pass (`render_inner`): paints the composed frame for every
//! branch — `;cmd`/`$EDITOR` top-overlay, a `TopPane`/`LowerPane`-mounted
//! pager, and the default status-bar + file-list + prompt + bottom-pane
//! layout — plus the centered `Overlay` pager and the harpoon menu on top.
//! Split from `app/render.rs` verbatim; an `impl App` child module reading
//! App's private state via the descendant-module rule.

use ratatui::Frame;

use crate::pane::PaneWidget;
use crate::ui::list_view::ListView;
use crate::ui::pager;
use crate::ui::prompt::PromptLine;
use crate::ui::status::StatusBar;

use crate::app::{
    App, FlashKind, FrameLayout, Mode, View, path_basename_display, place_pty_cursor_from_screen,
};

impl App {
    pub(super) fn render_inner(&self, frame: &mut Frame, layout: FrameLayout) {
        // `layout` and the list rows/grid are settled by `prepare_frame`
        // before this draw pass; this method only paints.

        // If a top-overlay pty is active (`;top`, `;vim`, etc.), it
        // replaces the entire spyc area. Status, list, and prompt are
        // hidden; only the overlay + divider + bottom pane render.
        if let Some(overlay) = self.runtime.top_overlay.as_ref() {
            // The overlay occupies status + list + prompt area.
            let overlay_area = ratatui::layout::Rect {
                x: layout.status.x,
                y: layout.status.y,
                width: layout.status.width,
                height: layout.status.height + layout.list.height + layout.prompt.height,
            };
            // Overlay resize/drain + the dismissal flag are settled in
            // `prepare_panes` before this draw (MVU Stage 2).
            // Visual focus tracks `state.pane_focused`: false ⇒
            // overlay focused (cursor block, full color); true ⇒
            // bottom pane focused (overlay dims to half-lightness via
            // PaneWidget's DIM modifier). User toggles with ^a-j/k.
            let overlay_focused = !self.state.pane_focused();
            let want_overlay_cursor = overlay_focused && !self.view.overlay_awaiting_dismiss;
            overlay.with_screen(|screen| {
                frame.render_widget(
                    PaneWidget {
                        screen,
                        focused: overlay_focused,
                    },
                    overlay_area,
                );
                if want_overlay_cursor {
                    place_pty_cursor_from_screen(frame, screen, overlay_area);
                }
            });
            // Show a dismiss prompt when the subprocess has exited.
            if self.view.overlay_awaiting_dismiss && overlay_area.height > 0 {
                use ratatui::{
                    style::{Modifier, Style},
                    text::{Line, Span},
                    widgets::Paragraph,
                };
                let prompt_y = overlay_area.y + overlay_area.height.saturating_sub(1);
                let prompt_rect = ratatui::layout::Rect {
                    x: overlay_area.x,
                    y: prompt_y,
                    width: overlay_area.width,
                    height: 1,
                };
                let style = Style::default()
                    .fg(self.view.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD);
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "[process exited — press any key to continue]",
                        style,
                    ))),
                    prompt_rect,
                );
            }

            // Divider + bottom pane still render normally.
            if let Some(divider_rect) = layout.divider {
                self.render_pane_status_line(frame, divider_rect);
            }
            let bottom_pane_rect: Option<ratatui::layout::Rect> =
                if let (Some(tabs), Some(rect)) = (self.runtime.pane_tabs.as_ref(), layout.pane) {
                    // Pane resize/drain settled in `prepare_panes`.
                    let focused = self.state.pane_focused();
                    // Single lock window: render the pane AND place
                    // the OS cursor under the same screen snapshot,
                    // so a worker-thread parse landing between the
                    // two can't produce a cursor that's ahead of the
                    // rendered grid (off-by-one tearing in claude
                    // backspace was the symptom).
                    let want_cursor = focused && !self.view.overlay_awaiting_dismiss;
                    tabs.active().with_screen(|screen| {
                        frame.render_widget(PaneWidget { screen, focused }, rect);
                        if want_cursor {
                            place_pty_cursor_from_screen(frame, screen, rect);
                        }
                    });
                    Some(rect)
                } else {
                    None
                };
            // Cursor placement is now folded into the overlay and
            // bottom-pane with_screen blocks above, so the rendered
            // grid and the cursor share a single lock acquisition.
            // (Pre-v1.50.84 they were two separate calls; the worker
            // thread could parse a chunk between them, leaving the
            // cursor ahead of the rendered grid — visible as
            // off-by-one tearing during fast input.)
            let _ = bottom_pane_rect;
            return;
        }

        // v1.5 Phase 5: a `TopPane`-mounted pager (today: only `D`
        // opening a file in-pager) replaces the status + list +
        // prompt area, just like the `;cmd` top overlay does, so the
        // bottom pane (claude / zsh) keeps running visibly below.
        // The pager's own title bar provides the visual identity;
        // status / prompt rows aren't drawn behind it.
        // While the pager-help overlay is open, the underlying
        // pager (stashed for restore on dismiss) still owns its
        // slot — peek into the stash so the user doesn't see the
        // pane "jump" back to live-pty / file-list rendering for
        // the lifetime of the help overlay.
        let in_help = self
            .view
            .pager
            .as_ref()
            .is_some_and(|v| v.title == crate::ui::pager::PAGER_HELP_TITLE);
        let top_pager = if in_help {
            self.view.pager_help_stash.as_ref()
        } else {
            self.view.pager.as_ref()
        }
        .is_some_and(|v| matches!(v.mount, crate::ui::pager::Mount::TopPane));
        if top_pager {
            let top_area = ratatui::layout::Rect {
                x: layout.status.x,
                y: layout.status.y,
                width: layout.status.width,
                height: layout.status.height + layout.list.height + layout.prompt.height,
            };
            let underlying = if in_help {
                self.view.pager_help_stash.as_ref()
            } else {
                self.view.pager.as_ref()
            };
            if let Some(view) = underlying {
                crate::ui::pager::render(frame, top_area, view, &self.view.theme);
            }
            // Divider + bottom pane render normally below.
            if let Some(divider_rect) = layout.divider {
                self.render_pane_status_line(frame, divider_rect);
            }
            if let (Some(tabs), Some(rect)) = (self.runtime.pane_tabs.as_ref(), layout.pane) {
                // Pane resize/drain + output_dirty clear settled in
                // `prepare_panes`.
                let focused = self.state.pane_focused();
                tabs.active().with_screen(|screen| {
                    frame.render_widget(PaneWidget { screen, focused }, rect);
                    if focused {
                        place_pty_cursor_from_screen(frame, screen, rect);
                    }
                });
            }
            // The TopPane branch returns early — if the pager-help
            // overlay is up over a TopPane pager, render it here on
            // top of the just-drawn slot before returning. The
            // standard branch's centered-overlay tail (further down)
            // never runs in this path.
            if in_help && let Some(help) = self.view.pager.as_ref() {
                crate::ui::pager::render(frame, frame.area(), help, &self.view.theme);
            }
            return;
        }

        let (path, suffix) = self.header_parts();
        let project_label = self
            .state
            .project_home
            .as_deref()
            .map(path_basename_display);
        let agent_info = self.active_agent_status();
        StatusBar {
            project_home: project_label.as_deref(),
            session_name: self.state.session_name.as_deref(),
            path: &path,
            suffix: &suffix,
            git_info: self.state.git.info.as_deref(),
            agent_info: agent_info.as_deref(),
            theme: &self.view.theme,
        }
        .render(frame, layout.status);

        // The rows cache + view_top↔grid stabilization were settled in
        // `prepare_frame` (MVU Stage 2); read the results for the draw.
        let rows = &self.view.cached_rows;
        let list_focused = !self.state.pane_focused();

        frame.render_widget(
            ListView {
                rows,
                cursor: self.state.cursor.index,
                view_top: self.state.cursor.view_top,
                empty_marker: self.state.view == View::Dir,
                focused: list_focused,
                theme: &self.view.theme,
            },
            layout.list,
        );

        // v1.5 Phase 3: a `LowerPane`-mounted pager (today: only
        // pane scrollback view, opened via `^a-v`) replaces the
        // pty widget in the bottom slot — the pty keeps running
        // off-screen but the user is reading a frozen snapshot
        // through the pager. The standard centered-overlay pager
        // path further down is skipped for `LowerPane`-mounted
        // views (rect dispatch happens here instead).
        // Same `underlying` logic as the TopPane branch above:
        // while the help overlay is open, peek into the stash so
        // the LowerPane scrollback view stays drawn underneath
        // instead of flickering back to the live pty.
        let bottom_is_pager = if in_help {
            self.view.pager_help_stash.as_ref()
        } else {
            self.view.pager.as_ref()
        }
        .is_some_and(|v| matches!(v.mount, crate::ui::pager::Mount::LowerPane));
        let bottom_pane_rect: Option<ratatui::layout::Rect> =
            if let (Some(tabs), Some(rect)) = (self.runtime.pane_tabs.as_ref(), layout.pane) {
                // Pane resize/drain + output_dirty clear settled in
                // `prepare_panes`.
                if bottom_is_pager {
                    // Skip the pty widget — the pager owns this rect now.
                    // The LowerPane first-frame scroll snap (the pending
                    // case) is settled in `prepare_panes`, before this draw.
                    let underlying = if in_help {
                        self.view.pager_help_stash.as_ref()
                    } else {
                        self.view.pager.as_ref()
                    };
                    if let Some(view) = underlying {
                        crate::ui::pager::render(frame, rect, view, &self.view.theme);
                    }
                } else {
                    let focused = self.state.pane_focused();
                    // Fold cursor placement into the same lock
                    // acquisition as the pane render — otherwise
                    // the worker thread can advance the screen
                    // between the two and we paint the grid from
                    // one frame and the cursor from the next
                    // (visible as off-by-one tearing during fast
                    // input).
                    tabs.active().with_screen(|screen| {
                        frame.render_widget(PaneWidget { screen, focused }, rect);
                        if focused {
                            place_pty_cursor_from_screen(frame, screen, rect);
                        }
                    });
                }
                // output_dirty cleared in `prepare_panes`.
                // Quick Select labels paint *over* the pane widget so
                // the user keeps the live output as context. Render
                // here, after the pane, before the divider.
                if self.view.quick_select.is_some() && !bottom_is_pager {
                    self.render_quick_select_overlay(frame, rect);
                }
                Some(rect)
            } else {
                None
            };
        // Cursor placement for the bottom-pane branch is folded into
        // the `with_screen` block above (single lock window for grid
        // + cursor). `bottom_pane_rect` is still computed so other
        // branches that need the geometry can read it.
        let _ = bottom_pane_rect;

        if let Some(divider_rect) = layout.divider {
            self.render_pane_status_line(frame, divider_rect);
        }

        if let Mode::Prompting(p) = &self.state.mode {
            PromptLine {
                prefix: &p.prefix,
                buffer: &p.buffer,
                theme: &self.view.theme,
                cursor_pos: p.editor.as_ref().map(|e| e.cursor),
                vi_mode: p.editor.as_ref().map(|e| e.mode),
            }
            .render(frame, layout.prompt);
        } else if let Some(flash) = &self.state.flash {
            use ratatui::{
                style::{Modifier, Style},
                text::{Line, Span},
                widgets::Paragraph,
            };
            let color = match flash.kind {
                FlashKind::Info => self.view.theme.take,
                FlashKind::Error => self.view.theme.cursor_bg,
            };
            let line = Line::from(Span::styled(
                flash.text.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            frame.render_widget(Paragraph::new(line), layout.prompt);
        } else if let Some(pending) = self.state.resolver.pending_display() {
            use ratatui::{
                style::{Modifier, Style},
                text::{Line, Span},
                widgets::Paragraph,
            };
            let line = Line::from(Span::styled(
                pending,
                Style::default()
                    .fg(self.view.theme.prompt_prefix)
                    .add_modifier(Modifier::BOLD),
            ));
            frame.render_widget(Paragraph::new(line), layout.prompt);
        }

        // Pager comes after list but before help (help always wins).
        // `LowerPane` and `TopPane` mounts already rendered into
        // their slots above; only `Overlay` mount hits this centered
        // render path.
        if let Some(view) = &self.view.pager
            && matches!(view.mount, crate::ui::pager::Mount::Overlay)
        {
            pager::render(frame, frame.area(), view, &self.view.theme);
        }

        // Harpoon menu overlay — modal, drawn on top of everything
        // except the activity monitor.
        if self.view.harpoon_menu.is_some() {
            self.render_harpoon_menu(frame);
        }
    }
}
