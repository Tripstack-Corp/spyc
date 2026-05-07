//! A pty-hosted subprocess rendered inside a ratatui frame.
//!
//! Bytes from the child are fed into a `vt100::Parser` which maintains a
//! 2D cell grid we render directly. Input keystrokes are encoded as ANSI
//! and written to the master side of the pty.
//!
//! This is the foundation for M8: eventually the subprocess will default
//! to `claude`, and spyc will be able to pipe its selection into the
//! pane's stdin. For the spike it is intentionally generic.

pub mod input;
pub mod pathref;
pub mod quick_select;
pub mod tabs;
mod widget;

pub use tabs::{PaneTabs, TabEntry, TabInfo};
pub use widget::{PaneWidget, cell_style};

// v1.5 Phase 6a: shared pty kernel that `Pane`, `PendingCapture`
// and `BackgroundTask` all wrap. Phase 6b/6c (promotion / demotion)
// becomes a state shift on the same handles.
pub mod pty_host;

use std::path::Path;

use crate::pane::pty_host::{PtyHost, PtySpec};

/// A hosted subprocess + its terminal emulator state.
///
/// v1.5 Phase 6a: pty kernel (master/writer/child/reader thread/
/// event channel/last_size/closed/exit_status/has_pending/debug_dump)
/// lives in `host: PtyHost`. `Pane` is the vt100-parser shell on top.
/// Same struct shape `BackgroundTask` will adopt in 6a step 3, which
/// is what unlocks promotion / demotion in 6b/6c.
pub struct Pane {
    /// Shared pty kernel. Owns master/writer/child/reader-thread/
    /// event channel/closed/exit_status/last_size/debug_dump.
    pub host: PtyHost,
    /// Terminal emulator parser — keeps the cell grid we render.
    parser: vt100::Parser,
    /// Set by `drain_output()` when new bytes arrived; cleared after render.
    pub output_dirty: bool,
    /// When > 0, the visible viewport is shifted into the scrollback
    /// by this many lines. Independent of `scrolling` so entering
    /// scroll mode doesn't jump the view (we set the flag without
    /// shifting).
    scroll_offset: usize,
    /// True when the pane is in scroll mode: keys navigate the
    /// scrollback instead of being forwarded to the child.
    /// `drain_output()` still runs so no output is lost.
    scrolling: bool,
}

impl Pane {
    /// Spawn `command` in a fresh pty of `rows × cols`, with `cwd` as
    /// the working directory. `context_path` points at *App's* live
    /// context file (the one the main loop writes to) so the child's
    /// `SPYC_CONTEXT` always resolves to a real file regardless of
    /// where the pane itself spawns — App writes one canonical
    /// `<start_dir>/.spyc-context-<pid>.json`, but a pane can spawn
    /// in any subdir, and recomputing from `cwd` would point at a
    /// path nobody writes.
    pub fn spawn(
        command: &str,
        rows: u16,
        cols: u16,
        cwd: &Path,
        context_path: &Path,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_env(command, rows, cols, cwd, context_path, &[])
    }

    pub fn spawn_with_env(
        command: &str,
        rows: u16,
        cols: u16,
        cwd: &Path,
        context_path: &Path,
        extra_env: &[(&str, &str)],
    ) -> anyhow::Result<Self> {
        // Pane env: tell child processes (e.g. Claude CLI's MCP server)
        // where this spyc instance's context file lives.
        let context_str = context_path.to_string_lossy().into_owned();
        let mut env: Vec<(&str, &str)> = Vec::with_capacity(extra_env.len() + 1);
        env.push((crate::context::CONTEXT_ENV_VAR, context_str.as_str()));
        env.extend(extra_env.iter().copied());

        let host = PtyHost::spawn(PtySpec {
            command,
            rows,
            cols,
            cwd,
            env: &env,
            term: "xterm-256color",
            // Send SIGWINCH right after spawn so rc-file shells
            // (p10k / oh-my-zsh) re-query the pty size and render
            // their first prompt at the right geometry.
            nudge_winch: true,
            debug_dump: std::env::var("SPYC_PTY_DEBUG").is_ok(),
        })?;
        let parser = vt100::Parser::new(rows, cols, 10_000);
        Ok(Self::adopt(host, parser))
    }

    /// Build a `Pane` around an already-spawned `PtyHost` and a
    /// vt100 parser. v1.5 Phase 6 promotion path: a backgrounded
    /// task hands over its `PtyHost`, we wrap it in a fresh parser
    /// (or one pre-fed with the task's captured output), and the
    /// pty keeps running through the transition.
    pub const fn adopt(host: PtyHost, parser: vt100::Parser) -> Self {
        Self {
            host,
            parser,
            output_dirty: false,
            scroll_offset: 0,
            scrolling: false,
        }
    }

    /// Drain any pending output from the child into the parser. Call
    /// Quick check whether the reader thread has posted data since
    /// the last drain. Uses a relaxed atomic — no locking, no
    /// syscall. Delegates to the shared `PtyHost`.
    pub fn has_pending_output(&self) -> bool {
        self.host.has_pending_output()
    }

    /// Drain any pending output from the child into the parser. Call
    /// each render tick before drawing. A `Closed` event marks the
    /// subprocess as finished so the caller can tear the pane down.
    /// Returns `true` if any bytes were processed.
    pub fn drain_output(&mut self) -> bool {
        // Pull bytes through the pty host into a local Vec so the
        // borrow on `self.host` doesn't overlap with the
        // `process_bytes_safe(&mut self, ...)` call below — vt100
        // reasoning is per-Pane, not per-host, so the parser stays
        // outside the kernel.
        let mut chunks: Vec<Vec<u8>> = Vec::new();
        let result = self.host.drain(|bytes| chunks.push(bytes.to_vec()));
        for chunk in &chunks {
            self.process_bytes_safe(chunk);
        }
        if result.had_bytes {
            self.output_dirty = true;
        }
        result.had_bytes
    }

    /// True once the subprocess has exited (reader thread saw EOF).
    pub const fn is_closed(&self) -> bool {
        self.host.closed
    }

    /// Exit status captured from the reaped child, if available.
    /// Replaces the pre-v1.5 `pub exit_status` field.
    pub const fn exit_status(&self) -> Option<&portable_pty::ExitStatus> {
        self.host.exit_status.as_ref()
    }

    /// Retry harvesting the exit status if `drain_output` missed it
    /// (the reader thread closes before `try_wait` can reap).
    /// `tabs.rs::label_exited_panes` calls this once per tick.
    pub fn try_harvest_exit_status(&mut self) {
        if self.host.exit_status.is_none() {
            if let Ok(Some(status)) = self.host.child.try_wait() {
                self.host.exit_status = Some(status);
            }
        }
    }

    /// Feed bytes to the vt100 parser, recovering from any panic.
    ///
    /// We're pinned at vt100 0.15.2; upstream is at 0.16.2 (worth
    /// trying as a separate upgrade — the bug may already be fixed
    /// there). 0.15 has a known panic-on-`unwrap` path on some valid
    /// escape sequences — e.g. nvim's exit-from-alt-screen byte
    /// stream after a session in zsh has produced a particular
    /// scroll/cursor state hits `vt100/screen.rs:934.unwrap()` on
    /// `drawing_cell(pos)`. When that fires the entire spyc process
    /// aborts, taking down all panes. Wrapping `parser.process` in
    /// `catch_unwind` and replacing the parser with a fresh instance
    /// keeps spyc alive — the user loses this pane's screen state
    /// at the moment of recovery (which mirrors what happens when
    /// the child redraws anyway). Useful safety net regardless of
    /// vt100 version: any third-party parser can hit edge cases.
    fn process_bytes_safe(&mut self, bytes: &[u8]) {
        // Capture the grid size before the parse; even reading
        // `parser.screen()` after a panic isn't safe, so we use the
        // cached `last_size` from the host (maintained for resize
        // coalescing).
        let (rows, cols) = self.host.last_size;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.parser.process(bytes);
        }));
        if result.is_err() {
            crate::spyc_debug!(
                "vt100 parser panicked on {} bytes; replacing parser to recover",
                bytes.len()
            );
            // Fresh parser at the same dimensions + scrollback. The
            // child's next render will repopulate the grid; in the
            // meantime the screen looks blank, which is preferable
            // to spyc dying.
            self.parser = vt100::Parser::new(rows, cols, 10_000);
        }
    }

    /// Tell the pty about a new size. We also resize the emulator so the
    /// cell grid matches — without this, the child keeps drawing at the
    /// old dimensions.
    pub fn resize(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        if (rows, cols) == self.host.last_size {
            return Ok(());
        }
        self.host.resize(rows, cols)?;
        self.parser.screen_mut().set_size(rows, cols);
        Ok(())
    }

    /// Forward a crossterm key to the child as ANSI bytes.
    pub fn send_key(&mut self, key: crossterm::event::KeyEvent) -> anyhow::Result<()> {
        let bytes = input::encode_key(key);
        if !bytes.is_empty() {
            self.host.write_all(&bytes)?;
        }
        Ok(())
    }

    /// PID of the spawned child, if available. `None` after the child
    /// has been reaped or if portable_pty couldn't surface it.
    pub fn process_id(&self) -> Option<u32> {
        self.host.process_id()
    }

    /// Best-effort kill of just the immediate child (no signal-group
    /// escalation). Used by tab-restart, which then re-spawns.
    pub fn try_kill(&mut self) {
        let _ = self.host.child.kill();
    }

    /// Orderly shutdown of the child and its descendants. Sends
    /// `SIGTERM` to the child's process group (negative PID — reaches
    /// every grandchild, which is the actual user-reported scenario:
    /// `npm run dev` → node → esbuild → workers all need to die when
    /// the tab closes), waits up to `grace` for voluntary exit, then
    /// `SIGKILL`s the group if it's still alive. Reaps the child so
    /// no zombies are left behind.
    ///
    /// portable-pty calls `setsid` for spawned children on Unix, so
    /// the child's PID is also its process-group leader — sending to
    /// `-pid` reaches the whole tree. Already-exited children
    /// short-circuit (the reader thread set `closed`).
    pub fn shutdown(&mut self, grace: std::time::Duration) {
        // Delegated to the host — same SIGTERM-then-SIGKILL escalation
        // as the pre-v1.5 inline implementation, just owned by the
        // shared kernel now.
        self.host.shutdown(grace);
    }

    /// Write arbitrary bytes to the child (e.g. paste, or send-selection).
    #[allow(dead_code)] // wired into the S-key handler in the next step
    pub fn send_bytes(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.host.write_all(bytes)
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// Mutable access to the underlying vt100 screen. Needed by
    /// the v1.5 scrollback adapter, which walks the scrollback
    /// buffer by mutating `scrollback_offset` and restoring it
    /// before returning. Don't use from the render loop — the
    /// adapter is the right entry point.
    pub fn parser_screen_mut(&mut self) -> &mut vt100::Screen {
        self.parser.screen_mut()
    }

    /// True when the child has switched to the xterm alternate screen
    /// (`\e[?1049h` or `\e[?47h`). Full-screen TUIs (codex, claude
    /// post-startup, vim, htop, lazygit) live there. Content drawn in
    /// alt-screen never enters main-screen scrollback, so spyc's
    /// `^a v` scroll-back has nothing to show in that mode — callers
    /// can flash a hint pointing the user at the app's own history
    /// viewer instead.
    pub fn is_alternate_screen(&self) -> bool {
        self.parser.screen().alternate_screen()
    }

    /// Return visible screen content as individual lines (plain text,
    /// no ANSI escapes). When the pane is in scroll mode, this is
    /// exactly the viewport the user is looking at — *not* the live
    /// tail. Use `pickable_text` for picker/scanner code that should
    /// follow the user's eye.
    pub fn visible_lines(&self) -> Vec<String> {
        let screen = self.parser.screen();
        screen.contents().lines().map(String::from).collect()
    }

    /// Text that interactive pickers (`gf`/`gF`, `^a u`) should scan:
    /// what the user is currently looking at. While scrolling, that
    /// is the exact visible viewport at the user's scroll position;
    /// while live, we widen to the last `recent_n` lines so
    /// paths/URLs that just rolled past the bottom are still
    /// findable. Without this distinction, scanning from a fixed
    /// slice means a user who scrolled up to find a URL would have
    /// it ignored — the picker would read a different region than
    /// their eyes.
    pub fn pickable_text(&mut self, recent_n: usize) -> Vec<String> {
        if self.is_scrolling() {
            self.visible_lines()
        } else {
            self.recent_lines(recent_n)
        }
    }

    /// Return the most recent `max_lines` of output (scrollback + visible
    /// screen). Used by `gf`/`gF` so path references that scrolled past
    /// the viewport are still found.
    pub fn recent_lines(&mut self, max_lines: usize) -> Vec<String> {
        let prev = self.scroll_offset;
        let max_sb = self.max_scrollback();
        self.parser.screen_mut().set_scrollback(max_sb);
        let all: Vec<String> = self
            .parser
            .screen()
            .contents()
            .lines()
            .map(String::from)
            .collect();
        self.parser.screen_mut().set_scrollback(prev);
        // Return only the last `max_lines` lines.
        if all.len() > max_lines {
            all[all.len() - max_lines..].to_vec()
        } else {
            all
        }
    }

    // ---- Scroll mode ------------------------------------------------

    /// True when the user is browsing scrollback history.
    pub const fn is_scrolling(&self) -> bool {
        self.scrolling
    }

    /// Enter scroll mode without shifting the view. The mode flag
    /// drives the divider re-color and tab uppercase that signal
    /// "you've left live view" — shifting the viewport would just
    /// add a jarring jump.
    pub const fn enter_scroll_mode(&mut self) {
        self.scrolling = true;
    }

    /// Exit scroll mode and snap back to live.
    pub fn exit_scroll_mode(&mut self) {
        self.scrolling = false;
        self.scroll_offset = 0;
        self.apply_scroll();
    }

    /// Scroll up (further into history) by `n` lines.
    pub fn scroll_up(&mut self, n: usize) {
        let max = self.max_scrollback();
        self.scroll_offset = self.scroll_offset.saturating_add(n).min(max);
        self.apply_scroll();
    }

    /// Scroll down (toward live) by `n` lines. If we're at the live
    /// position, this is a no-op while still in scroll mode (use
    /// `exit_scroll_mode` to leave).
    pub fn scroll_down_or_exit(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
        self.apply_scroll();
    }

    /// Jump to the oldest line in scrollback.
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = self.max_scrollback().max(1);
        self.apply_scroll();
    }

    /// Jump back to live view (stays in scroll mode; user must
    /// explicitly `exit_scroll_mode` to leave).
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.apply_scroll();
    }

    /// Save full scrollback + screen contents to a timestamped file.
    pub fn save_to_file(&mut self) -> std::io::Result<std::path::PathBuf> {
        // Temporarily set scrollback to max so contents() captures everything.
        let prev = self.scroll_offset;
        let max = self.max_scrollback();
        self.parser.screen_mut().set_scrollback(max);
        let text = self.parser.screen().contents();
        // Restore previous view.
        self.parser.screen_mut().set_scrollback(prev);

        let now = crate::sysinfo::format_now().replace([' ', ':'], "_");
        let stamp = now.trim_end_matches("_UTC");
        let filename = format!("spyc_pane_{stamp}.txt");
        let path = std::env::current_dir()?.join(&filename);
        std::fs::write(&path, text.trim_end().to_string() + "\n")?;
        Ok(path)
    }

    #[allow(clippy::unused_self)]
    const fn max_scrollback(&self) -> usize {
        // vt100 stores scrollback rows in an internal VecDeque.
        // screen().scrollback() returns the *current* offset, not
        // the maximum. We can probe by temporarily setting a huge
        // offset — set_scrollback clamps to the actual buffer length.
        // But that mutates, so we use a simpler heuristic: the
        // contents() method with full scrollback gives us everything.
        // For navigation we need the max offset. The internal buffer
        // length isn't directly exposed, so we binary-search or just
        // set a large value and read back what it clamped to.
        //
        // Actually, set_scrollback(usize::MAX) clamps internally,
        // but we'd need &mut. Since we already have &self in some
        // callers, let's store it. For now, use a reasonable upper
        // bound and accept the clamp.
        10_000 // matches our scrollback_len
    }

    fn apply_scroll(&mut self) {
        self.parser.screen_mut().set_scrollback(self.scroll_offset);
    }
}

// Drop and reader_loop moved into `pty_host.rs` in Phase 6a — Pane
// no longer owns the pty kernel directly, so the safety-net SIGKILL
// and the byte-pump thread live with the kernel. PtyHost::Drop
// fires when a Pane is dropped (PtyHost is the only field that
// owns the child).
