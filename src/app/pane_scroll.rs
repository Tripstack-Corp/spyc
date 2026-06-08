//! Pane scrollback / transcript pager (`^a-v`): snapshot the active pane's
//! vt100 scrollback (or an agent's on-disk transcript) into a lower-pane
//! pager, the vi-style scroll-mode key handler, and the tab-switch stash/
//! restore pair that keeps a scrollback pager attached to its tab. Extracted
//! verbatim from `app/mod.rs` (the impl-extraction sweep). The open / stash /
//! restore / key-handler entry points are `pub` (called from `actions` /
//! `key_dispatch`); `mount_scroll_pager` is internal.

use ratatui::text::Line;

use super::pager_stream::{DrainOutcome, PagerStream, PagerStreamMount, RenderCtx};
use super::{App, Effect, PaneTextKind, PaneTextSink, state};
use crate::ui::pager::PagerView;

/// Inputs to the pure `^a v` scrollback-source decision.
#[derive(Clone, Copy)]
struct ScrollSnapshot {
    /// The agent running in the pane has a `TranscriptSpec`.
    has_transcript: bool,
    /// The transcript is enabled by config (`config_key` / `default_enabled`).
    transcript_enabled: bool,
    /// The pane is on the alternate screen (a full-screen TUI / agent).
    is_alt_screen: bool,
}

/// Where `^a v` sources pane scrollback from.
#[derive(Debug, PartialEq, Eq)]
enum ScrollSource {
    /// Read the agent's structured on-disk transcript.
    Transcript,
    /// Snapshot the pane's vt100 scrollback buffer.
    Vt100,
    /// A non-agent alt-screen app — nothing to show; flash a hint.
    AltScreenDeadEnd,
}

/// Pure routing for `^a v` (the `route.rs` / `focus.rs` template). On the alt
/// screen an agent with a transcript ALWAYS uses it — there's no usable vt100
/// capture to fall back to (this is what makes claude's full-screen mode work),
/// so the config gate is bypassed. Inline, the transcript stays config-gated; a
/// non-agent alt-screen app dead-ends; everything else snapshots vt100.
const fn decide_scroll_source(s: ScrollSnapshot) -> ScrollSource {
    if s.has_transcript && (s.transcript_enabled || s.is_alt_screen) {
        ScrollSource::Transcript
    } else if s.is_alt_screen {
        ScrollSource::AltScreenDeadEnd
    } else {
        ScrollSource::Vt100
    }
}

impl App {
    /// Stash the active scrollback pager (if any) onto the
    /// currently-active tab's slot. Tab-switch handlers call this
    /// **before** flipping the active-tab pointer; the companion
    /// `restore_active_tab_scrollback_pager` runs **after** the flip
    /// to surface the destination tab's stashed pager if it has one.
    /// Together: scroll back on tab 1, `^a-n`, the pager visually
    /// disappears (replaced by tab 2's live pty); `^a-p` back to
    /// tab 1, the pager comes back at the same scroll / search /
    /// selection state.
    ///
    /// Only acts on scrollback pagers (`pane_scroll == true`).
    /// Content-bound pagers (Overlay file viewer, TopPane Markdown,
    /// etc.) are App-level and persist across tab switches.
    pub fn stash_scrollback_pager_to_active_tab(&mut self) {
        // The bottom scrollback lives in its own region slot now; the
        // top-region `view.pager` (a `D` doc, etc.) is App-level and persists
        // across tab switches untouched.
        let Some(view) = self.view.scroll_pager.take() else {
            return;
        };
        // If the stashed pager is backed by an in-flight stream, park the
        // stream by id so `drain_pager_stream` doesn't drop it while its pager
        // is off-screen (it id-gates against the *live* pager). Re-installed on
        // restore. (The stream lives here, not on the `pane::TabEntry`, per the
        // one-way `app → pane` rule.)
        if let Some(id) = view.stream_id
            && self
                .runtime
                .pager_stream
                .as_ref()
                .is_some_and(|s| s.id() == id)
            && let Some(stream) = self.runtime.pager_stream.take()
        {
            self.runtime.stashed_pager_streams.insert(id, stream);
        }
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            tabs.active_entry_mut().stashed_scrollback_pager = Some(view);
        }
    }

    /// Restore the active tab's stashed scrollback pager into
    /// `self.view.scroll_pager` if one is stashed AND none is currently
    /// displayed. See `stash_scrollback_pager_to_active_tab` for the outgoing
    /// half of the pair. (The top-region `view.pager` is independent — a `D`
    /// doc stays visible across tab switches.)
    pub fn restore_active_tab_scrollback_pager(&mut self) {
        if self.view.scroll_pager.is_some() {
            return;
        }
        let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
            return;
        };
        let Some(view) = tabs.active_entry_mut().stashed_scrollback_pager.take() else {
            return;
        };
        // Pull the parked stream (if any) back into the live slot by id so it
        // resumes draining (`tabs` borrow ends at the `take` above).
        if let Some(id) = view.stream_id
            && let Some(stream) = self.runtime.stashed_pager_streams.remove(&id)
        {
            self.runtime.pager_stream = Some(stream);
        }
        self.view.scroll_pager = Some(view);
    }

    pub fn open_pane_scroll_pager(&mut self) {
        let Some(tabs) = self.runtime.pane_tabs.as_ref() else {
            return;
        };
        let active_info = tabs.active_info();
        let label = active_info.label.clone();
        let command = active_info.command.clone();
        let cwd = active_info.cwd.clone();
        let spawn = active_info.spawn_epoch_secs;
        let is_alt_screen = tabs.active().is_alternate_screen();

        // Agent-aware scrollback. An agent's `AgentProfile` may carry a
        // `TranscriptSpec` — its structured on-disk transcript, the source of
        // truth: codex/agy confine history to a scroll region vt100 can't
        // capture, and claude's full-screen mode renders on the *alternate
        // screen*, so vt100 captures nothing either. `decide_scroll_source` is
        // the pure routing: on the alt screen an agent with a transcript ALWAYS
        // uses it (bypassing the config gate) — there's no usable vt100 capture
        // to fall back to; inline, the transcript stays config-gated, and a
        // non-agent alt-screen app gets the dead-end hint. The engaged read +
        // render runs off-thread (the worker below).
        let profile = crate::agent::detect(&command);
        let spec = profile.transcript();
        let transcript_enabled = spec.as_ref().is_some_and(|s| match s.config_key {
            None => s.default_enabled,
            Some(key) => self
                .state
                .config
                .pane
                .transcript_enabled(key, s.default_enabled),
        });

        match decide_scroll_source(ScrollSnapshot {
            has_transcript: spec.is_some(),
            transcript_enabled,
            is_alt_screen,
        }) {
            ScrollSource::Transcript => {
                let spec = spec.expect("has_transcript implies a spec");
                // Off-thread: resolve + read + parse + render the transcript on
                // a worker — a 4MB tail-read + per-line JSON parse, far too
                // heavy for the keypress path (and `resolve` itself scans +
                // parses every session-index file). An empty / not-found result
                // flashes the miss and closes back to the pane. (The old
                // synchronous vt100-on-miss fallthrough is retired: vestigial
                // for inline agents, absent for a full-screen alt-screen agent.)
                let resolve = spec.resolve;
                let render = spec.render;
                let theme = self.view.theme.clone();
                let miss = spec.miss_message.map_or_else(
                    || format!("{}: no transcript found for this session", profile.name()),
                    str::to_string,
                );
                let title = format!(" {label} (transcript)");
                self.spawn_pager_stream(
                    PagerStreamMount::LowerPane { title },
                    move |tx| {
                        let lines = match resolve(&cwd, spawn) {
                            Some(path) => render(&path, &theme),
                            None => Vec::new(),
                        };
                        let _ = tx.send(lines);
                    },
                    |id, rx| Box::new(TranscriptStream { id, rx, miss }),
                );
                return;
            }
            ScrollSource::AltScreenDeadEnd => {
                // A non-agent alt-screen app (vim, less, htop, …) does virtual
                // scrolling inside a fixed grid — old content lives in app
                // memory, not the terminal — so spyc has nothing to show.
                self.state
                    .flash_info("scroll: alt-screen app — use its own scrollback / history keys");
                return;
            }
            ScrollSource::Vt100 => {}
        }

        // vt100 terminal-scrollback path: not an alt-screen app and no engaged
        // transcript, so snapshot the pane's own scrollback buffer.
        let tabs = self
            .runtime
            .pane_tabs
            .as_mut()
            .expect("pane_tabs presence checked above");
        let active = tabs.active_mut();
        // Drain pending bytes before snapshotting. Bytes that hit
        // the OS pipe between the last render tick and this keypress
        // may still be in flight on the reader/parser threads; a few
        // short yields let them flush so the snapshot includes the
        // most-recent paint.
        for _ in 0..3 {
            active.drain_output();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        active.drain_output();
        // Empty scrollback ⇒ a fresh process, or an app that keeps
        // its own history (scroll region / virtual scroll). Flash a
        // hint; still open the pager so search/yank of the visible
        // screen works.
        let scrollback_rows = active.with_screen_mut(crate::ui::scrollback::scrollback_len);
        let lines = active.with_screen_mut(crate::ui::scrollback::lines_from_scrollback);
        if scrollback_rows == 0 {
            self.state
                .flash_info("no scrollback captured — this app keeps its own history");
        }
        self.mount_scroll_pager(format!(" {label} (history)"), lines);
    }

    /// Mount a lower-pane scroll/transcript pager from pre-built
    /// lines. Shared by the vt100-scrollback path and the codex
    /// on-disk transcript path. Enters the active pane's scroll mode
    /// (divider cues + key routing flip to the pager) and parks the
    /// view at the bottom on first render.
    fn mount_scroll_pager(&mut self, title: String, lines: Vec<ratatui::text::Line<'static>>) {
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            tabs.active_mut().enter_scroll_mode();
        }
        let mut view = crate::ui::pager::PagerView::new_styled(title, lines);
        view.mount = crate::ui::pager::Mount::LowerPane;
        view.pane_scroll = true;
        // Gutter off so existing content doesn't jump horizontally
        // when the pager opens. Toggle with `l`.
        view.show_line_numbers = false;
        view.no_history = true;
        // Wrap long lines (compiler errors, diffs, transcript turns)
        // — no horizontal scroll, so truncation would hide content.
        view.wrap = true;
        // Park at the bottom on first render via the deferred flag;
        // the LowerPane render branch knows the real viewport height
        // and scrolls there, avoiding a one-frame jump.
        view.pending_scroll_to_bottom.set(true);
        // The bottom-region scrollback slot — coexists with a top-region
        // `view.pager` (`D`) rather than evicting it.
        self.view.scroll_pager = Some(view);
        self.state.focus = state::Focus::Pane;
        self.view.needs_full_repaint = true;
        self.state
            .flash_info("scroll: on (/, n/N, :N, V, y, Esc exit)");
    }

    pub fn handle_pane_scroll_key(&mut self, key: crossterm::event::KeyEvent) -> Vec<Effect> {
        use crossterm::event::{KeyCode, KeyModifiers};
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Handle pending `g` prefix: gg = scroll top, gf/gF = goto file.
        if self.view.scroll_pending_g {
            self.view.scroll_pending_g = false;
            return match key.code {
                KeyCode::Char('g') => {
                    self.runtime
                        .pane_tabs
                        .as_mut()
                        .expect("pane_tabs presence checked by key routing")
                        .active_mut()
                        .scroll_to_top();
                    Vec::new()
                }
                // gf/gF while scrolling a pane — same path as the file-list
                // action: emit a `ReadPaneText`/`GotoFile` effect so the
                // pickable read + navigation run in `run_effects` (PR 5b).
                KeyCode::Char('f') => vec![Effect::ReadPaneText {
                    kind: PaneTextKind::Pickable(200),
                    then: PaneTextSink::GotoFile {
                        open_at_line: false,
                    },
                }],
                KeyCode::Char('F') => vec![Effect::ReadPaneText {
                    kind: PaneTextKind::Pickable(200),
                    then: PaneTextSink::GotoFile { open_at_line: true },
                }],
                _ => Vec::new(), // Unknown g-sequence, ignore
            };
        }

        // `r`: reload a transcript scroll pager — re-resolve + re-render the
        // on-disk transcript off-thread (full-screen agents keep appending, so
        // a snapshot goes stale). Only meaningful for a stream-backed pager;
        // the vt100 snapshot path falls through to the normal key handling.
        if matches!(key.code, KeyCode::Char('r'))
            && !ctrl
            && self
                .view
                .pager
                .as_ref()
                .is_some_and(|p| p.stream_id.is_some())
        {
            self.open_pane_scroll_pager();
            return Vec::new();
        }

        let pane = self
            .runtime
            .pane_tabs
            .as_mut()
            .expect("pane_tabs presence checked by key routing")
            .active_mut();
        match key.code {
            KeyCode::Char('k') | KeyCode::Up => pane.scroll_up(1),
            KeyCode::Char('j') | KeyCode::Down => pane.scroll_down_or_exit(1),
            KeyCode::PageUp | KeyCode::Char('b') if ctrl => pane.scroll_up(20),
            KeyCode::Char('u') if ctrl => pane.scroll_up(10),
            KeyCode::PageDown | KeyCode::Char('f') if ctrl => pane.scroll_down_or_exit(20),
            KeyCode::Char('d') if ctrl => pane.scroll_down_or_exit(10),
            KeyCode::Char('g') => {
                self.view.scroll_pending_g = true;
            }
            KeyCode::Char('G') => pane.scroll_to_bottom(),
            KeyCode::Char('s') => match pane.save_to_file() {
                Ok(path) => {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    self.state.flash_info(format!("saved: {name}"));
                }
                Err(e) => self.state.flash_info(format!("save error: {e}")),
            },
            KeyCode::Esc | KeyCode::Char('q') => {
                pane.exit_scroll_mode();
                self.state.flash_info("scroll: off");
            }
            _ => {}
        }
        Vec::new()
    }
}

/// A [`PagerStream`] backing an agent-transcript scrollback view. The worker
/// resolves + reads + renders the on-disk transcript off-thread and sends the
/// rendered lines once; this drain installs them (or, on an empty / not-found
/// result, flashes `miss` and closes back to the pane). One-shot — not retained.
struct TranscriptStream {
    id: u32,
    rx: std::sync::mpsc::Receiver<Vec<Line<'static>>>,
    /// Flashed when the transcript resolves to nothing (no session on disk yet,
    /// or a parse that yielded no renderable turns).
    miss: String,
}

impl PagerStream for TranscriptStream {
    fn id(&self) -> u32 {
        self.id
    }

    fn drain(&mut self, view: &mut PagerView, _ctx: &RenderCtx) -> DrainOutcome {
        match self.rx.try_recv() {
            Ok(lines) if !lines.is_empty() => {
                view.lines = lines;
                view.streaming = false;
                // Re-arm park-at-bottom: the mount-time flag already fired on
                // the empty pager (content arrives async), so without this the
                // view stays at the top instead of the newest turn.
                view.pending_scroll_to_bottom.set(true);
                DrainOutcome::Finished
            }
            // Resolved to nothing, or the worker died before sending: flash the
            // miss and close. (The pager was already mounted in scroll mode;
            // `close_stream_pager` exits it.)
            Ok(_) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                DrainOutcome::CloseInfo(self.miss.clone())
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => DrainOutcome::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ScrollSnapshot, ScrollSource, decide_scroll_source};

    fn snap(has_transcript: bool, transcript_enabled: bool, is_alt_screen: bool) -> ScrollSnapshot {
        ScrollSnapshot {
            has_transcript,
            transcript_enabled,
            is_alt_screen,
        }
    }

    /// The headline: a full-screen (alt-screen) agent with a transcript engages
    /// it REGARDLESS of the config gate — there's no vt100 to fall back to.
    #[test]
    fn alt_screen_agent_engages_transcript_even_when_config_disabled() {
        assert_eq!(
            decide_scroll_source(snap(true, false, true)),
            ScrollSource::Transcript
        );
        assert_eq!(
            decide_scroll_source(snap(true, true, true)),
            ScrollSource::Transcript
        );
    }

    /// Inline (not alt-screen): the transcript stays config-gated.
    #[test]
    fn inline_transcript_is_config_gated() {
        assert_eq!(
            decide_scroll_source(snap(true, true, false)),
            ScrollSource::Transcript
        );
        // Disabled + inline → verbatim vt100 capture (today's behavior).
        assert_eq!(
            decide_scroll_source(snap(true, false, false)),
            ScrollSource::Vt100
        );
    }

    /// A non-agent app (no transcript spec) on the alt screen dead-ends;
    /// otherwise vt100.
    #[test]
    fn non_agent_routes_by_alt_screen() {
        assert_eq!(
            decide_scroll_source(snap(false, false, true)),
            ScrollSource::AltScreenDeadEnd
        );
        assert_eq!(
            decide_scroll_source(snap(false, false, false)),
            ScrollSource::Vt100
        );
    }
}
