//! The unified "background worker streams styled content into a pager"
//! abstraction. Three features hand-rolled the same skeleton — `:grep`
//! ([`super::grep_session`], streaming append), git-view diff/show/blame
//! ([`super::git_view_session`], one-shot + a retained model for the `|`
//! layout toggle), and the agent-transcript reads ([`crate::state`]'s
//! `*_transcript` modules) — differing only in the *producer* (the worker
//! body) and the *apply-to-pager* step. This module is the shared core they
//! collapse onto, making "off-thread read/parse → streaming pager" the
//! default architecture (see ARCHITECTURE.md).
//!
//! A stream is two halves:
//! - **Producer** (on a worker thread): reads/parses/renders off the UI thread
//!   and pushes payloads through a [`crate::fs::WakingSender`], waking the loop
//!   with a payloadless [`super::Message::PagerStreamOutput`]. Spawned by
//!   [`App::spawn_pager_stream`].
//! - **Drain** (this module, on the main thread): [`App::drain_pager_stream`]
//!   id-gates the live [`crate::ui::pager::PagerView`] against the active
//!   stream and lets the stream apply whatever the worker produced, via the
//!   [`PagerStream`] trait. The payload type is **erased** inside each impl
//!   (the impl owns its `Receiver<T>`), so the trait is object-safe and lives
//!   behind `Box<dyn PagerStream>` in `Runtime`.
//!
//! All three producers — `:grep`, git-view diff/show/blame, and the agent
//! transcript reads — now ride this seam through the single `stream_id` /
//! `Message::PagerStreamOutput`.

use super::{App, state};
use crate::ui::pager::{self, PagerView};
use crate::ui::theme::Theme;

/// How a [`PagerStream::drain`] should be handled by the main loop.
pub enum DrainOutcome {
    /// Nothing arrived this tick (or the stream is already complete and only
    /// retained to back a command). No redraw.
    Idle,
    /// The stream mutated the pager (appended lines / replaced content / a
    /// title-progress refresh). Mark the frame dirty.
    Changed,
    /// A one-shot stream rendered its result. The loop keeps the stream alive
    /// iff [`PagerStream::retain_after_finish`] (git-view retains its model for
    /// the `|` toggle; streaming grep / one-shot transcript do not).
    Finished,
    /// Terminal: close the stream's pager (revealing the listing underneath, not
    /// a prior overlay) and flash this as an info message (a transcript that
    /// resolved to nothing). git-view reports "no changes" before mounting (see
    /// `drain_pending_git_view`), so it never reaches here.
    CloseInfo(String),
}

/// Render inputs handed to [`PagerStream::drain`]. **Owned** (not borrowed) so
/// the drain dispatcher can hold a `&mut PagerView` at the same time — the
/// theme and the pager both live under `self.view`, so a borrowed `&Theme`
/// would alias the `&mut`. `Theme` is a cheap `Clone` (colors only).
///
/// The fields are read by the git-view renderer; the streaming grep /
/// one-shot transcript streams ignore `ctx`.
pub struct RenderCtx {
    /// The active theme (clone of `self.view.theme`).
    pub theme: Theme,
    /// Whether the backing pager is full-width — affects diff body width.
    pub full_width: bool,
}

/// An in-pager command routed to a retained stream (via
/// [`App::dispatch_pager_command`]). git-view's `|` flips its diff layout.
pub enum PagerStreamCmd {
    /// Flip diff/show unified ⇄ side-by-side (git-view `|`).
    ToggleLayout,
}

/// A background producer streaming styled content into a [`PagerView`].
///
/// Object-safe (no generic methods, no `Self`-returning methods): stored as
/// `Box<dyn PagerStream>` in `Runtime`. Each impl owns its `Receiver<T>` and
/// keeps `T` private.
pub trait PagerStream {
    /// The stream's id, matched against the live pager's `stream_id` so a wake
    /// for a replaced / closed pager self-discards.
    fn id(&self) -> u32;

    /// Drain whatever the worker has produced into `view`. `ctx` carries an
    /// owned theme + width for renderers that need them (git-view); simple
    /// append-streams ignore it.
    fn drain(&mut self, view: &mut PagerView, ctx: &RenderCtx) -> DrainOutcome;

    /// Keep the stream alive after it reports [`DrainOutcome::Finished`]?
    /// Git-view retains its model to back the `|` layout toggle; streaming grep
    /// and one-shot transcript reads do not. Default: drop on finish.
    fn retain_after_finish(&self) -> bool {
        false
    }

    /// Handle an in-pager command (re-rendering `view` from a retained model
    /// without re-touching the worker). Returns true if handled. Default: not
    /// handled (streaming grep / one-shot transcript have no commands).
    fn on_pager_command(
        &mut self,
        _cmd: PagerStreamCmd,
        _view: &mut PagerView,
        _ctx: &RenderCtx,
    ) -> bool {
        false
    }
}

/// How [`App::spawn_pager_stream`] mounts the initial (empty / "computing…")
/// pager that the worker then fills.
pub enum PagerStreamMount {
    /// Centered overlay (grep, git-view): pushes the prior pager onto history
    /// for `:bprev`. `line_count_hint` locks the gutter width while streaming.
    Overlay {
        /// Initial title (e.g. `"grep — … — scanning…"`).
        title: String,
        /// Gutter-width hint so the line-number column doesn't widen as
        /// results stream in (grep passes `MAX_MATCHES`).
        line_count_hint: Option<usize>,
    },
    /// Lower-pane scroll pager (agent-transcript scrollback): enters the active
    /// pane's scroll mode, parks at the bottom, gutter off, wrap on. Mirrors
    /// the vt100 `mount_scroll_pager` setup.
    LowerPane {
        /// Initial title (e.g. `" claude (transcript)"`).
        title: String,
    },
}

impl App {
    /// Spawn a worker that runs `produce` off-thread (pushing payloads through a
    /// [`crate::fs::WakingSender`]), mount an empty pager tagged with a fresh
    /// `stream_id`, and install the boxed session built by `build`. The shared
    /// open path for grep / git-view / transcript — the per-feature parts are
    /// just `produce` (the worker body) and `build` (the `PagerStream` impl).
    pub(crate) fn spawn_pager_stream<T, P, B>(
        &mut self,
        mount: PagerStreamMount,
        produce: P,
        build: B,
    ) where
        T: Send + 'static,
        P: FnOnce(crate::fs::WakingSender<T>) + Send + 'static,
        B: FnOnce(u32, std::sync::mpsc::Receiver<T>) -> Box<dyn PagerStream>,
    {
        let id = self.runtime.next_stream_id;
        self.runtime.next_stream_id = self.runtime.next_stream_id.wrapping_add(1);
        let (tx, rx) = std::sync::mpsc::channel::<T>();
        // Wake the loop on the worker's send (via WakingSender) and once more
        // after it returns — the final wake drives the drain that observes the
        // rx disconnect, with no poll floor (mirrors grep/git-view).
        let wake = self.make_pager_stream_wake();
        let final_wake = std::sync::Arc::clone(&wake);
        let tx = crate::fs::WakingSender::new(tx, wake);
        std::thread::spawn(move || {
            produce(tx);
            final_wake();
        });
        self.mount_stream_pager(mount, id);
        self.runtime.pager_stream = Some(build(id, rx));
    }

    /// Mount the initial empty pager for a freshly-spawned stream, tagged with
    /// `id` so [`App::drain_pager_stream`] id-gates the worker's output.
    pub(crate) fn mount_stream_pager(&mut self, mount: PagerStreamMount, id: u32) {
        match mount {
            PagerStreamMount::Overlay {
                title,
                line_count_hint,
            } => {
                let mut view = pager::PagerView::new_plain(title, Vec::<String>::new());
                view.streaming = true;
                view.line_count_hint = line_count_hint;
                view.stream_id = Some(id);
                view.saveable = true;
                // Push any open pager onto the back stack so `:bprev` returns
                // to it; save its scroll first (mirrors the old grep/git-view).
                self.remember_pager_position();
                if let Some(prev) = self.view.pager.take() {
                    self.view.pager_history.push(prev);
                }
                self.set_pager(view);
                self.view.needs_full_repaint = true;
            }
            PagerStreamMount::LowerPane { title } => {
                if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                    tabs.active_mut().enter_scroll_mode();
                }
                let mut view = pager::PagerView::new_styled(title, Vec::new());
                view.mount = pager::Mount::LowerPane;
                view.pane_scroll = true;
                view.stream_id = Some(id);
                view.streaming = true;
                // Gutter off so existing content doesn't jump horizontally;
                // wrap on so long transcript turns aren't clipped.
                view.show_line_numbers = false;
                view.no_history = true;
                view.wrap = true;
                // Park at the bottom on first render (deferred — the LowerPane
                // branch knows the real viewport height).
                view.pending_scroll_to_bottom.set(true);
                // The bottom-region scrollback slot — coexists with a
                // top-region `view.pager` (`D`) rather than evicting it.
                self.view.scroll_pager = Some(view);
                self.state.focus = state::Focus::Pane;
                self.view.needs_full_repaint = true;
                self.state
                    .flash_info("scroll: on (/, n/N, :N, V, y, Esc exit)");
            }
        }
    }

    /// Drain the active pager stream into its backing pager. Called every tick
    /// from the run loop (a no-op when no stream is active). Returns true when
    /// something changed so the caller can request a redraw.
    ///
    /// id-gate: if the live pager's `stream_id` no longer matches (closed /
    /// replaced / stashed), the stream is dropped and its worker exits on its
    /// next send.
    pub(crate) fn drain_pager_stream(&mut self) -> bool {
        let Some(stream) = self.runtime.pager_stream.as_ref() else {
            return false;
        };
        let id = stream.id();
        // The stream's pager may live in either region slot: the top/overlay
        // `view.pager` (grep, git-view) or the bottom `view.scroll_pager`
        // (transcript). Drop the stream if neither holds it (closed / replaced
        // / stashed).
        let Some(in_scroll) = self.stream_slot(id) else {
            self.runtime.pager_stream = None;
            return false;
        };
        // Owned RenderCtx (clone the theme) so the `&mut` pager borrow below
        // doesn't alias `&self.view.theme` — both live under `self.view`.
        let full_width = if in_scroll {
            self.view
                .scroll_pager
                .as_ref()
                .is_some_and(|p| p.full_width)
        } else {
            self.view.pager.as_ref().is_some_and(|p| p.full_width)
        };
        let ctx = RenderCtx {
            theme: self.view.theme.clone(),
            full_width,
        };
        // Field-level `&mut` (one pager slot) + the disjoint `runtime` borrow
        // coexist; both end when `drain` returns the owned outcome.
        let view = if in_scroll {
            self.view.scroll_pager.as_mut()
        } else {
            self.view.pager.as_mut()
        }
        .expect("matching slot checked above");
        let stream = self
            .runtime
            .pager_stream
            .as_mut()
            .expect("stream presence checked above");
        match stream.drain(view, &ctx) {
            DrainOutcome::Idle => false,
            DrainOutcome::Changed => true,
            DrainOutcome::Finished => {
                let retain = self
                    .runtime
                    .pager_stream
                    .as_ref()
                    .is_some_and(|s| s.retain_after_finish());
                if !retain {
                    self.runtime.pager_stream = None;
                }
                true
            }
            DrainOutcome::CloseInfo(msg) => {
                self.close_stream_pager(in_scroll);
                self.state.flash_info(msg);
                self.runtime.pager_stream = None;
                true
            }
        }
    }

    /// Which region slot holds the pager backing stream `id`: `Some(true)` =
    /// the bottom `view.scroll_pager`, `Some(false)` = the top/overlay
    /// `view.pager`, `None` = neither (stale / closed). Pure read (returns a
    /// `Copy` bool), so callers can hold disjoint `&mut` borrows afterward.
    fn stream_slot(&self, id: u32) -> Option<bool> {
        if self
            .view
            .scroll_pager
            .as_ref()
            .is_some_and(|p| p.stream_id == Some(id))
        {
            Some(true)
        } else if self
            .view
            .pager
            .as_ref()
            .is_some_and(|p| p.stream_id == Some(id))
        {
            Some(false)
        } else {
            None
        }
    }

    /// Route an in-pager command (e.g. git-view's `|` layout toggle) to the
    /// active stream if it backs a live pager (either region slot). Returns
    /// whether it was handled (false → the key falls through as a no-op, e.g.
    /// `|` on a non-git-view / blame / transcript pager). Re-renders from the
    /// retained model — no worker round-trip.
    pub(crate) fn dispatch_pager_command(&mut self, cmd: PagerStreamCmd) -> bool {
        let Some(stream) = self.runtime.pager_stream.as_ref() else {
            return false;
        };
        let id = stream.id();
        let Some(in_scroll) = self.stream_slot(id) else {
            return false;
        };
        let full_width = if in_scroll {
            self.view
                .scroll_pager
                .as_ref()
                .is_some_and(|p| p.full_width)
        } else {
            self.view.pager.as_ref().is_some_and(|p| p.full_width)
        };
        let ctx = RenderCtx {
            theme: self.view.theme.clone(),
            full_width,
        };
        let view = if in_scroll {
            self.view.scroll_pager.as_mut()
        } else {
            self.view.pager.as_mut()
        }
        .expect("matching slot checked above");
        let stream = self
            .runtime
            .pager_stream
            .as_mut()
            .expect("stream presence checked above");
        stream.on_pager_command(cmd, view, &ctx)
    }

    /// Close the stream-backed pager being torn down. `in_scroll` selects the
    /// region slot: the bottom transcript scrollback (`view.scroll_pager`) is
    /// cleared and scroll mode exited (it's `no_history` — nothing to pop); a
    /// top/overlay stream pager (git-view / grep) is cleared so the dir listing
    /// it overlaid is revealed.
    ///
    /// It must NOT pop `pager_history` back to the prior pager: combined with
    /// the push-at-mount in [`Self::mount_stream_pager`], that *resurrected the
    /// previous overlay* on an empty/failed result — e.g. `gd src/` (changes)
    /// then `gd tests/` (no changes) re-showed `src/`'s diff instead of "no
    /// changes". Revealing the base listing keeps `gd <clean path>` honest; the
    /// prior diff was already pushed to `pager_history`, so `:bprev` still
    /// reaches it for deliberate buffer navigation.
    fn close_stream_pager(&mut self, in_scroll: bool) {
        if in_scroll {
            self.view.scroll_pager = None;
            if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
                tabs.active_mut().exit_scroll_mode();
            }
        } else if self
            .view
            .pager
            .as_ref()
            .is_some_and(|p| p.stream_id.is_some())
        {
            self.view.pager = None;
        }
        self.view.needs_full_repaint = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::pager::PagerView;

    /// A `PagerStream` that returns one scripted outcome per `drain` and (for
    /// `Changed`/`Finished`) appends a marker line so the test can see it ran.
    struct FakeStream {
        id: u32,
        outcome: Option<DrainOutcome>,
        retain: bool,
    }
    impl PagerStream for FakeStream {
        fn id(&self) -> u32 {
            self.id
        }
        fn drain(&mut self, view: &mut PagerView, _ctx: &RenderCtx) -> DrainOutcome {
            let outcome = self.outcome.take().unwrap_or(DrainOutcome::Idle);
            if matches!(outcome, DrainOutcome::Changed | DrainOutcome::Finished) {
                view.lines.push(ratatui::text::Line::from("fake"));
            }
            outcome
        }
        fn retain_after_finish(&self) -> bool {
            self.retain
        }
    }

    fn pager_with_stream_id(id: u32) -> PagerView {
        let mut v = PagerView::new_plain("test", Vec::<String>::new());
        v.stream_id = Some(id);
        v
    }

    fn with_app(f: impl FnOnce(&mut App)) {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            f(&mut app);
        });
    }

    #[test]
    fn no_stream_is_noop() {
        with_app(|app| {
            assert!(!app.drain_pager_stream());
        });
    }

    #[test]
    fn id_mismatch_drops_stream() {
        with_app(|app| {
            app.view.pager = Some(pager_with_stream_id(7));
            app.runtime.pager_stream = Some(Box::new(FakeStream {
                id: 99, // does not match the pager
                outcome: Some(DrainOutcome::Changed),
                retain: false,
            }));
            assert!(!app.drain_pager_stream());
            assert!(app.runtime.pager_stream.is_none());
        });
    }

    #[test]
    fn changed_appends_and_redraws_without_dropping() {
        with_app(|app| {
            app.view.pager = Some(pager_with_stream_id(1));
            app.runtime.pager_stream = Some(Box::new(FakeStream {
                id: 1,
                outcome: Some(DrainOutcome::Changed),
                retain: false,
            }));
            assert!(app.drain_pager_stream());
            assert!(app.runtime.pager_stream.is_some());
            assert_eq!(app.view.pager.as_ref().unwrap().lines.len(), 1);
        });
    }

    #[test]
    fn finished_drops_unless_retained() {
        with_app(|app| {
            app.view.pager = Some(pager_with_stream_id(1));
            app.runtime.pager_stream = Some(Box::new(FakeStream {
                id: 1,
                outcome: Some(DrainOutcome::Finished),
                retain: false,
            }));
            assert!(app.drain_pager_stream());
            assert!(app.runtime.pager_stream.is_none(), "non-retained drops");
        });
        with_app(|app| {
            app.view.pager = Some(pager_with_stream_id(2));
            app.runtime.pager_stream = Some(Box::new(FakeStream {
                id: 2,
                outcome: Some(DrainOutcome::Finished),
                retain: true,
            }));
            assert!(app.drain_pager_stream());
            assert!(app.runtime.pager_stream.is_some(), "retained survives");
        });
    }

    #[test]
    fn close_info_clears_pager_and_drops_stream() {
        with_app(|app| {
            app.view.pager = Some(pager_with_stream_id(1));
            app.runtime.pager_stream = Some(Box::new(FakeStream {
                id: 1,
                outcome: Some(DrainOutcome::CloseInfo("no changes".into())),
                retain: false,
            }));
            assert!(app.drain_pager_stream());
            // The empty result reveals the base listing; stream drops.
            assert!(app.view.pager.is_none());
            assert!(app.runtime.pager_stream.is_none());
        });
    }

    /// Regression: `gd` on a path with no changes must reveal the dir listing,
    /// NOT resurrect whatever diff was open before. The second `gd`'s mount
    /// pushes the prior diff onto `pager_history`; an empty result used to
    /// `pop_back()` it straight back into view (so `gd src/` then `gd tests/`
    /// showed `src/`'s changes). It must clear to the base view instead.
    #[test]
    fn close_info_does_not_resurrect_prior_diff_from_history() {
        with_app(|app| {
            // Prior diff sitting in the buffer history (as mount_stream_pager
            // would have pushed it when the second git-view mounted).
            let mut prior = pager_with_stream_id(1);
            prior
                .lines
                .push(ratatui::text::Line::from("PRIOR DIFF CONTENT"));
            app.view.pager_history.push(prior);
            // The freshly-mounted (second) git-view stream, about to report
            // "no changes".
            app.view.pager = Some(pager_with_stream_id(2));
            app.runtime.pager_stream = Some(Box::new(FakeStream {
                id: 2,
                outcome: Some(DrainOutcome::CloseInfo("no changes".into())),
                retain: false,
            }));
            assert!(app.drain_pager_stream());
            assert!(
                app.view.pager.is_none(),
                "empty git-view must reveal the listing, not pop the prior diff"
            );
            assert!(app.runtime.pager_stream.is_none());
            // The prior diff is still reachable via `:bprev` (left in history).
            assert_eq!(app.view.pager_history.back_len(), 1);
        });
    }
}
