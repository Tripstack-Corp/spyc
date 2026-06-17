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
            // The overlay occupies the spyc unit above the divider
            // (`top_unit`) — not `status.y + Σheights`, which lands
            // off-screen and panics with `status_position = "bottom"`.
            let overlay_area = layout.top_unit;
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

            // Divider + bottom region still render normally. The bottom region
            // is the `^a v` scrollback (`view.scroll_pager`) if open, else the
            // live pane — so a top overlay and a bottom scrollback coexist (the
            // shared helper is what makes `^a v` work while an editor is open).
            if let Some(divider_rect) = layout.divider {
                self.render_pane_status_line(frame, divider_rect);
            }
            if let Some(rect) = layout.pane {
                // Suppress the pty cursor while the overlay awaits dismissal so
                // the "[process exited — press any key]" frame shows no stray
                // cursor (the overlay's own cursor is suppressed the same way).
                self.render_bottom_region(frame, rect, self.view.overlay_awaiting_dismiss);
            }
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
            // Same spyc-unit region as the `;cmd` overlay above (see the
            // `top_unit` note); anchoring at `status.y` panics under
            // `status_position = "bottom"`.
            let top_area = layout.top_unit;
            let underlying = if in_help {
                self.view.pager_help_stash.as_ref()
            } else {
                self.view.pager.as_ref()
            };
            if let Some(view) = underlying {
                crate::ui::pager::render(frame, top_area, view, &self.view.theme);
            }
            // Divider + bottom region render normally below. The bottom region
            // is the `^a v` scrollback (`view.scroll_pager`) if open, else the
            // live pane — so a `D` top pager and a bottom scrollback coexist.
            if let Some(divider_rect) = layout.divider {
                self.render_pane_status_line(frame, divider_rect);
            }
            if let Some(rect) = layout.pane {
                self.render_bottom_region(frame, rect, false);
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

        // v1.5 Phase 3: the bottom region is the `^a v` scrollback pager
        // (`view.scroll_pager`, a `LowerPane` snapshot) when one is open — it
        // replaces the pty widget while the pty runs off-screen — else the live
        // pane. Drawn by the shared `render_bottom_region` helper so it works
        // identically whether the top region is the file list or a `D` pager.
        // The scrollback lives in its own slot, independent of the help
        // overlay's `view.pager` stash.
        let bottom_is_pager = self.view.scroll_pager.is_some();
        if let Some(rect) = layout.pane {
            self.render_bottom_region(frame, rect, false);
            // output_dirty cleared in `prepare_panes`.
            // Quick Select labels paint *over* the live pane widget so the user
            // keeps the output as context. Skipped when the scrollback owns it.
            if self.view.quick_select.is_some() && !bottom_is_pager {
                self.render_quick_select_overlay(frame, rect);
            }
        }
        // Cursor placement for the bottom-pane branch is folded into the
        // `with_screen` block above (single lock window for grid + cursor).

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

    /// Draw the bottom region into `rect`: the `^a v` scrollback pager
    /// (`view.scroll_pager`) when one is open, else the live pty pane. Shared
    /// by all three top-region branches (file list, `D` TopPane pager, and the
    /// `;cmd`/`$EDITOR` overlay) so a top surface and a bottom scrollback
    /// coexist — routing it through here is what keeps `^a v` working under an
    /// open overlay. Cursor placement folds into the pane's `with_screen` lock
    /// (grid + cursor from one snapshot — no off-by-one tear during fast
    /// input). `suppress_cursor` hides the pty cursor even when the pane is
    /// focused — the overlay branch passes `overlay_awaiting_dismiss` so the
    /// "[process exited — press any key]" frame shows no stray cursor.
    fn render_bottom_region(
        &self,
        frame: &mut Frame,
        rect: ratatui::layout::Rect,
        suppress_cursor: bool,
    ) {
        if let Some(view) = self.view.scroll_pager.as_ref() {
            // The scrollback snapshot owns this rect; the pty runs off-screen.
            // The first-frame scroll snap (pending) is settled in `prepare_panes`.
            crate::ui::pager::render(frame, rect, view, &self.view.theme);
        } else if let Some(tabs) = self.runtime.pane_tabs.as_ref() {
            let focused = self.state.pane_focused();
            tabs.active().with_screen(|screen| {
                frame.render_widget(PaneWidget { screen, focused }, rect);
                if focused && !suppress_cursor {
                    place_pty_cursor_from_screen(frame, screen, rect);
                }
            });
        }
    }

    /// Draw the full-screen mermaid image overlay (the pager `i` key) on top of
    /// everything, with a dismiss-hint footer. No-op when no diagram is up. The
    /// `Protocol` was built off-thread at terminal size by `mermaid_ops`; this
    /// is a pure blit (graphics terminals only). Pure `&self`.
    pub(super) fn render_mermaid_overlay(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let Some(iv) = self.view.image_view.as_ref() else {
            return;
        };
        let protocol = &iv.protocol;
        use ratatui::layout::Rect;
        use ratatui::style::{Modifier, Style};
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Clear, Paragraph};
        frame.render_widget(Clear, area);
        // Center the diagram in the region above the one-row dismiss hint.
        // ratatui-image anchors at the rect's top-left, so we place a rect of
        // the protocol's own cell size at the centered offset. `allow_clipping`
        // guards an off-by-one from Fit's cell rounding.
        let avail = Rect {
            height: area.height.saturating_sub(1),
            ..area
        };
        let psize = protocol.size();
        let iw = psize.width.min(avail.width);
        let ih = psize.height.min(avail.height);
        let img_area = Rect {
            x: avail.x + avail.width.saturating_sub(iw) / 2,
            y: avail.y + avail.height.saturating_sub(ih) / 2,
            width: iw,
            height: ih,
        };
        frame.render_widget(
            ratatui_image::Image::new(protocol).allow_clipping(true),
            img_area,
        );
        let hint_area = Rect {
            y: area.y + area.height.saturating_sub(1),
            height: 1,
            ..area
        };
        let style = Style::default()
            .fg(self.view.theme.prompt_prefix)
            .add_modifier(Modifier::BOLD);
        // Footer: transient verb feedback if set, else the key hints. `Y`
        // (copy source) only when this is a mermaid diagram.
        let footer = iv.flash.clone().unwrap_or_else(|| {
            let yank = if iv.source.is_some() { " \u{00b7} Y copy source" } else { "" };
            format!(" mermaid diagram \u{2014} s save \u{00b7} o open \u{00b7} b base64{yank} \u{00b7} q/Esc dismiss")
        });
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(footer, style))),
            hint_area,
        );
    }
}
