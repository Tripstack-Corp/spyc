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
        // restore. Scroll pagers always use `scroll_stream` (never
        // `pager_stream`, which is for overlay grep / git-view). (The stream
        // lives here, not on the `pane::TabEntry`, per the one-way `app → pane`
        // rule.)
        if let Some(id) = view.stream_id
            && self
                .runtime
                .scroll_stream
                .as_ref()
                .is_some_and(|s| s.id() == id)
            && let Some(stream) = self.runtime.scroll_stream.take()
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
        // resumes draining (`tabs` borrow ends at the `take` above). Scroll
        // pagers always use `scroll_stream`.
        if let Some(id) = view.stream_id
            && let Some(stream) = self.runtime.stashed_pager_streams.remove(&id)
        {
            self.runtime.scroll_stream = Some(stream);
        }
        self.view.scroll_pager = Some(view);
    }

    /// Stream ids still owned by a living tab's stashed scrollback pager — the
    /// ids `prune_orphaned_pager_streams` must keep.
    fn live_stashed_stream_ids(&self) -> std::collections::HashSet<u32> {
        self.runtime
            .pane_tabs
            .as_ref()
            .map(|tabs| {
                tabs.tabs()
                    .iter()
                    .filter_map(|e| {
                        e.stashed_scrollback_pager
                            .as_ref()
                            .and_then(|v| v.stream_id)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Drop parked pager streams whose owning tab no longer exists. A scrollback
    /// pager's backing stream is parked in `runtime.stashed_pager_streams` (keyed
    /// by id) while its tab is off-screen — see
    /// [`stash_scrollback_pager_to_active_tab`](Self::stash_scrollback_pager_to_active_tab) —
    /// and [`restore_active_tab_scrollback_pager`](Self::restore_active_tab_scrollback_pager)
    /// is the *only* other drain. So a tab dropped while stashed — closed
    /// (`^W x`), restarted (`^a R`), demoted (`:pane-to-task`), or replaced
    /// (claude crash-recover) — would leak its stream forever. The `pane` layer
    /// can't reach `runtime` (the one-way `app → pane` rule), so the owning side
    /// reclaims here, *after* the tab is gone. Idempotent and path-independent: a
    /// call from any removal site sweeps every now-orphaned stream, so a future
    /// removal path is covered as long as it (or a later call) reaches here.
    pub fn prune_orphaned_pager_streams(&mut self) {
        if self.runtime.stashed_pager_streams.is_empty() {
            return; // common case: nothing parked — skip the tab walk
        }
        let live = self.live_stashed_stream_ids();
        retain_live_streams(&mut self.runtime.stashed_pager_streams, &live);
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
        // The session uuid pinned to this pane — the codex rollout claim, or the
        // claude session id lifted from its status hook. The resolver's strongest
        // signal, and the only reliable one when two agent tabs share a cwd (the
        // spawn-time proximity fallback collapses them onto one transcript).
        let session_id = active_info.pinned_session_id().map(str::to_string);
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
                // Markdown-render width hint: the scrollback pager fills the
                // full-width lower pane with the gutter off, so the body is the
                // terminal width minus the block's two border columns. Computed
                // on the main thread (the worker has no terminal handle) and
                // captured into the producer; `r` re-runs this after a resize.
                let (term_w, _) = self.view.term_size;
                let width = Some(usize::from(term_w.saturating_sub(2)));
                // `t` toggles whether the agent's tool-use / tool-result lines
                // are rendered; the worker re-runs `render` with the current
                // value, so the toggle takes effect on the next mount / reload.
                let show_tool_calls = self.view.transcript_show_tool_calls;
                self.spawn_pager_stream(
                    PagerStreamMount::LowerPane { title },
                    move |tx| {
                        let query = crate::agent::TranscriptQuery {
                            cwd: &cwd,
                            spawn_epoch_secs: spawn,
                            command: &command,
                            session_id: session_id.as_deref(),
                        };
                        let lines = match resolve(query) {
                            Some(path) => render(&path, &theme, width, show_tool_calls),
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
        // Empty scrollback ⇒ a fresh/short process, or an inline agent whose
        // structured transcript is toggled off (an agent with its transcript
        // engaged or on the alt screen took the Transcript branch above).
        // There's nothing above the visible screen to scroll to, so DON'T enter
        // scroll mode — mounting a one-screen pager just traps the user in a
        // view they have to Esc out of (reported on a fresh zsh). Flash the
        // reason and stay live: the visible screen is already on screen, and
        // `yp` yanks it without scroll mode. For an agent we point at its
        // transcript (spyc parses its log); for a plain process we state the
        // fact — never the old "this app keeps its own history", which was false
        // for a shell and backwards for an agent.
        let scrollback_rows = active.with_screen_mut(crate::ui::scrollback::scrollback_len);
        if scrollback_rows == 0 {
            let hint = if spec.is_some() {
                format!(
                    "no terminal scrollback — {} keeps its history in a transcript (toggle it on)",
                    profile.name()
                )
            } else {
                "no terminal scrollback captured".to_string()
            };
            self.state.flash_info(hint);
            return;
        }
        let lines = active.with_screen_mut(crate::ui::scrollback::lines_from_scrollback);
        self.mount_scroll_pager(format!(" {label} (history)"), lines);
    }

    /// Mount a lower-pane scroll/transcript pager from pre-built
    /// lines. Shared by the vt100-scrollback path and the codex
    /// on-disk transcript path. Enters the active pane's scroll mode
    /// (divider cues + key routing flip to the pager) and parks the
    /// view at the bottom on first render.
    fn mount_scroll_pager(&mut self, title: String, lines: Vec<ratatui::text::Line<'static>>) {
        self.install_lower_pane_scroll_view(title, lines, None);
    }

    /// Re-install an already-built `PagerView` into the bottom scrollback slot
    /// (`view.scroll_pager`), preserving its scroll/content, and focus the pane.
    /// Used by the in-pager `v` editor round-trip: a scrollback pager edited in
    /// $EDITOR must return to its OWN slot, not the top `view.pager` that
    /// `set_pager` writes — otherwise the original orphans in `scroll_pager`
    /// while the edited copy lands in the top slot and `q`/Esc can't reach it.
    /// The pane stays in scroll mode across the round-trip (the launch clears
    /// only the slot, not the mode), so this just reseats the view.
    pub(crate) fn restore_scroll_pager_view(&mut self, view: crate::ui::pager::PagerView) {
        self.view.scroll_pager = Some(view);
        self.state.focus = state::Focus::Pane;
        self.view.needs_full_repaint = true;
    }

    /// Build + install a bottom-pane scrollback `PagerView` with the shared
    /// LowerPane flags (gutter off so content doesn't jump horizontally, wrap
    /// on so long lines aren't clipped, excluded from buffer history, parked at
    /// the bottom on first render), enter the pty's scroll mode, focus the pane,
    /// and flash. Shared by the static `mount_scroll_pager` and the streaming
    /// `pager_stream::mount_stream_pager` LowerPane arm (`stream_id` set → the
    /// view is a live stream). The two used to set this flag block independently.
    /// `t` in an agent-transcript scrollback: flip whether the agent's
    /// tool-use / tool-result lines render, then re-spawn so the change
    /// shows (the worker re-runs `render` with the new flag). A no-op with
    /// a hint on a plain vt100 scrollback — there are no tool calls there.
    pub(crate) fn toggle_scrollback_tool_calls(&mut self) {
        let has_transcript = self.runtime.pane_tabs.as_ref().is_some_and(|tabs| {
            crate::agent::detect(&tabs.active_info().command)
                .transcript()
                .is_some()
        });
        if !has_transcript {
            self.state
                .flash_info("tool calls: only in an agent transcript view");
            return;
        }
        self.view.transcript_show_tool_calls = !self.view.transcript_show_tool_calls;
        self.open_pane_scroll_pager();
        let shown = if self.view.transcript_show_tool_calls {
            "shown"
        } else {
            "hidden"
        };
        self.state.flash_info(format!("tool calls: {shown}"));
    }

    pub(crate) fn install_lower_pane_scroll_view(
        &mut self,
        title: String,
        lines: Vec<ratatui::text::Line<'static>>,
        stream_id: Option<u32>,
    ) {
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            tabs.active_mut().enter_scroll_mode();
        }
        let mut view = crate::ui::pager::PagerView::new_styled(title, lines);
        view.mount = crate::ui::pager::Mount::LowerPane;
        view.pane_scroll = true;
        view.tab_width = self.state.config.pager.tab_width;
        // Line numbers on by default: a gutter the live pane never has,
        // so it reads at a glance as "scrolled back, not live" (and `:N`
        // / `V` line targeting is legible). `l` toggles it off.
        view.show_line_numbers = true;
        view.no_history = true;
        view.wrap = true;
        view.pending_scroll_to_bottom.set(true);
        if let Some(id) = stream_id {
            view.stream_id = Some(id);
            view.streaming = true;
        }
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

        // NB: `r` (reload a transcript scrollback) is handled in
        // `handle_pager_motion` (motion.rs), which owns the stream-backed
        // `scroll_pager`. This handler only runs for the raw vt100 scroll mode
        // (`InputSink::PaneScroll`), where a stream pager never lives in
        // `view.pager` (those are Modal / Scrollback → routed to PagerKey), so
        // the `view.pager.stream_id` check here was always false (dead). `r`
        // falls through to the no-op below.

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
            KeyCode::Char('s') => {
                let result = pane.save_to_file();
                self.state.flash_saved_file(result);
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                pane.exit_scroll_mode();
                self.state.flash_info("scroll: off");
            }
            _ => {}
        }
        Vec::new()
    }
}

/// Evict parked streams whose id isn't in `live` (their owning tab is gone).
/// Split from the `pane_tabs` walk in
/// [`App::prune_orphaned_pager_streams`] so the keep/drop policy is unit-testable
/// without spawning a pane.
fn retain_live_streams(
    parked: &mut std::collections::HashMap<u32, Box<dyn PagerStream>>,
    live: &std::collections::HashSet<u32>,
) {
    parked.retain(|id, _| live.contains(id));
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
    use std::collections::{HashMap, HashSet};

    use super::{
        App, DrainOutcome, PagerStream, PagerView, RenderCtx, ScrollSnapshot, ScrollSource,
        decide_scroll_source, retain_live_streams,
    };

    /// Inert [`PagerStream`] for the leak tests — only its id matters; it's
    /// never actually drained here.
    struct FakeStream(u32);
    impl PagerStream for FakeStream {
        fn id(&self) -> u32 {
            self.0
        }
        fn drain(&mut self, _view: &mut PagerView, _ctx: &RenderCtx) -> DrainOutcome {
            DrainOutcome::Idle
        }
    }

    fn parked(ids: &[u32]) -> HashMap<u32, Box<dyn PagerStream>> {
        ids.iter()
            .map(|&id| (id, Box::new(FakeStream(id)) as Box<dyn PagerStream>))
            .collect()
    }

    /// The keep/drop policy: only streams whose id a living tab still references
    /// survive; an id with no living tab (closed / demoted / replaced) is evicted.
    #[test]
    fn retain_live_streams_drops_only_unreferenced() {
        let mut map = parked(&[7, 8]);
        let live: HashSet<u32> = std::iter::once(8).collect();
        retain_live_streams(&mut map, &live);
        assert!(!map.contains_key(&7), "orphaned stream 7 should be dropped");
        assert!(map.contains_key(&8), "referenced stream 8 must survive");
        assert_eq!(map.len(), 1);
    }

    /// With no tabs left (last tab closed / only tab demoted), every parked
    /// stream is orphaned — the integration that closes the leak.
    #[test]
    fn prune_empties_parked_streams_when_no_tabs() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.runtime.stashed_pager_streams = parked(&[1, 2]);
            assert!(app.runtime.pane_tabs.is_none());
            app.prune_orphaned_pager_streams();
            assert!(
                app.runtime.stashed_pager_streams.is_empty(),
                "no tabs ⇒ all parked streams orphaned"
            );
        });
    }

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
