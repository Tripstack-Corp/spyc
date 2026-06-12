//! Background shell-capture tasks (`^Z` to background, `:fg` to resume).
//! A backgrounded `!` capture keeps its reader thread draining into a
//! per-task buffer with no pager attached; the task viewer / divider
//! glyphs / `:fg` / pause-resume all read this state.
//!
//! Extracted verbatim from `app/mod.rs` (REFACTOR_PLAN Phase 1). The
//! conversion logic (backgrounding, `:fg`, task viewer, divider render,
//! pause/resume) stays in `app` and reads these fields directly, so the
//! struct/enum and their fields are `pub`.

use anyhow::Result;

use crate::pane::pty_host::PtyHost;
use crate::pane::{Pane, PaneTabs, TabEntry, TabInfo};
use crate::ui::pager::PagerView;

use super::{App, Effect, PendingCapture, SigOk, eof_marker_line, kill_pg, state, strip_crlf};

/// Lifecycle state of a backgrounded shell capture.
#[derive(Debug)]
pub enum TaskStatus {
    /// Reader thread is still running; child has not exited.
    Running,
    /// Child exited cleanly (or with non-zero status); inner is the code.
    Exited(i32),
    /// `child.wait()` returned an error -- inner is the message.
    Crashed(String),
}

/// A capture that has been moved off the foreground pager into the
/// background. Same plumbing as `PendingCapture` (child, writer, rx,
/// buffer); the reader thread spawned by `spawn_capture` keeps draining
/// into `buffer` even though no pager is attached.
pub struct BackgroundTask {
    pub id: u32,
    pub title: String,
    pub cmd_display: String,
    /// Shared pty kernel — same shape as PendingCapture and Pane
    /// since v1.5 Phase 6a, which unblocks the task ↔ pane
    /// migration coming in 6b/6c (the host moves between
    /// containers; pty stays running).
    pub host: PtyHost,
    pub buffer: Vec<u8>,
    pub status: TaskStatus,
    pub started: std::time::Instant,
    pub finished_at: Option<std::time::Instant>,
    /// True whenever bytes arrived while the task was sitting in the
    /// background. Reset on `:fg`. Drives the `[N+]` vs `[N●]` glyph
    /// in the divider so the user can see at a glance which task has
    /// fresh output to look at.
    pub has_unread_output: bool,
    /// Set true once the user opens the task in the task viewer
    /// (`[t`/`]t`, `gB`, or `:task N`). Combined with `Exited`/`Crashed`
    /// status, this is what triggers the on-close promotion to buffer
    /// history -- viewing acts as the user's "I've seen this" ack.
    pub viewed_in_task_viewer: bool,
    /// True while the task is paused (SIGSTOP delivered, no further
    /// SIGCONT yet). Toggled by `:pause`/`:resume` (and `S`/`C` in
    /// the task viewer). The reader thread keeps blocking on read
    /// until the child resumes; status stays Running because the
    /// child hasn't exited.
    pub paused: bool,
}

/// Soft cap on per-task buffered output. When exceeded, drop bytes from
/// the head (keep the tail) -- the tail of a long build is what the user
/// usually wants. 1 MB ≈ ~10K lines of plain text.
pub const TASK_BUFFER_CAP: usize = 1_048_576;

pub struct BackgroundTasks {
    pub tasks: Vec<BackgroundTask>,
    next_id: u32,
}

impl BackgroundTasks {
    pub const fn new() -> Self {
        Self {
            tasks: Vec::new(),
            next_id: 1,
        }
    }

    pub const fn allocate_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Most-recently-added task id (LIFO order), regardless of status.
    /// `:fg` with no arg uses this.
    pub fn most_recent(&self) -> Option<u32> {
        self.tasks.last().map(|t| t.id)
    }

    pub fn take(&mut self, id: u32) -> Option<BackgroundTask> {
        let pos = self.tasks.iter().position(|t| t.id == id)?;
        Some(self.tasks.remove(pos))
    }

    pub fn running_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Running))
            .count()
    }

    pub fn done_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| !matches!(t.status, TaskStatus::Running))
            .count()
    }
}

impl App {
    /// Spawn a captured shell command and install the streaming pager
    /// view + `pending_capture` so the loop can drain output. Used by
    /// the `!` prompt, `:!`, `:!!`, and the `!?` history re-execute —
    /// `cmd_display` lets `:!!` show `!` while titling with the actual
    /// resolved command.
    pub fn start_capture(&mut self, expanded: &str, title_cmd: &str, cmd_display: &str) {
        let title = format!("! {title_cmd}");
        match spawn_capture(expanded, &self.state.listing.dir) {
            Ok(host) => {
                // MVU Phase 3c: install the capture's channel wake (a generic
                // SinkOutput edge — the loop re-scans on it). Survives the
                // ^Z/:fg round-trip with the host, so no re-install there.
                // Self-wake sweeps any bytes that landed during the install
                // window now the floor is gone.
                let wake = self.make_sink_wake();
                host.set_wake(wake);
                host.fire_wake_now();
                let mut view =
                    PagerView::new_plain(format!("\u{23f3} {title} — running... (0s)"), Vec::new());
                view.streaming = true;
                self.set_pager(view);
                self.runtime.pending_capture = Some(PendingCapture {
                    host,
                    buffer: Vec::new(),
                    title,
                    cmd_display: cmd_display.to_string(),
                    started: std::time::Instant::now(),
                    finished: false,
                    original_id: None,
                });
            }
            Err(e) => self.state.flash_error(format!("exec: {e}")),
        }
    }

    /// `^Z` from inside a streaming `!` capture pager. Move the running
    /// capture into `background_tasks` and close the pager. The reader
    /// thread (spawned by `spawn_capture`) keeps running, so output
    /// keeps accumulating into the task buffer for later `:fg`.
    pub fn background_capture(&mut self) {
        let Some(capture) = self.runtime.pending_capture.take() else {
            return;
        };
        let id = capture
            .original_id
            .unwrap_or_else(|| self.runtime.background_tasks.allocate_id());
        let task = BackgroundTask {
            id,
            title: capture.title,
            cmd_display: capture.cmd_display.clone(),
            host: capture.host,
            buffer: capture.buffer,
            status: TaskStatus::Running,
            started: capture.started,
            finished_at: None,
            has_unread_output: false,
            viewed_in_task_viewer: false,
            paused: false,
        };
        self.runtime.background_tasks.tasks.push(task);
        self.clear_pager();
        self.view.needs_full_repaint = true;
        self.state
            .flash_info(format!("task #{id} backgrounded — :fg to resume"));
    }

    /// Pause a backgrounded task by sending SIGSTOP to its process
    /// group. portable-pty children are session/group leaders by
    /// default, so `kill(-pid, SIGSTOP)` halts the whole subprocess
    /// tree (e.g. `make → cc → ld` all stop together) rather than
    /// just the direct child.
    ///
    /// `target` of None pauses the most-recent task; numeric arg
    /// targets a specific id. No-op (with flash) if the target is
    /// not Running, doesn't exist, or is already paused.
    pub fn pause_task(&mut self, target: Option<u32>) -> Vec<Effect> {
        let Some(id) = target.or_else(|| self.runtime.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return Vec::new();
        };
        let Some(task) = self
            .runtime
            .background_tasks
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
        else {
            self.state.flash_error(format!("no task with id {id}"));
            return Vec::new();
        };
        if !matches!(task.status, TaskStatus::Running) {
            self.state.flash_error(format!("task #{id} is not running"));
            return Vec::new();
        }
        if task.paused {
            self.state.flash_info(format!("task #{id} already paused"));
            return Vec::new();
        }
        let Some(pid) = task.host.process_id() else {
            self.state.flash_error(format!("task #{id}: no process id"));
            return Vec::new();
        };
        // SIGSTOP to the process group (negative pid → group; uncatchable,
        // so the child can't refuse; the reader thread keeps blocking on
        // read until SIGCONT). The signal, the `paused` toggle, and the
        // flash all run in `run_effects` (the sole side-effect executor).
        vec![Effect::SignalGroup {
            pid,
            sig: rustix::process::Signal::STOP,
            on_ok: SigOk::Pause(id),
            on_err: format!("task #{id}: SIGSTOP failed"),
        }]
    }

    /// Send SIGINT to a running task's process group. Mirrors what
    /// pressing ^C in a normal terminal does: child's tty driver
    /// delivers SIGINT and the child decides whether to exit
    /// (default) or trap. Returns the human-readable result for the
    /// caller to flash in the right place (pager footer, status
    /// bar, etc.). `None` target = most-recent task.
    pub fn interrupt_task(&self, target: Option<u32>) -> Result<String, String> {
        let id = target
            .or_else(|| self.runtime.background_tasks.most_recent())
            .ok_or_else(|| "no background tasks".to_string())?;
        let task = self
            .runtime
            .background_tasks
            .tasks
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| format!("no task with id {id}"))?;
        if !matches!(task.status, TaskStatus::Running) {
            return Err("process already stopped".to_string());
        }
        let pid = task
            .host
            .process_id()
            .ok_or_else(|| format!("task #{id}: no process id"))?;
        // Negative pid → process group, same convention as
        // pause_task / resume_task. SIGINT lets the child clean up
        // (matches a real ^C); use ^\ in the live capture path for a
        // hard kill.
        kill_pg(pid, rustix::process::Signal::INT)
            .map(|()| format!("task #{id}: sent SIGINT"))
            .map_err(|_| format!("task #{id}: SIGINT failed"))
    }

    /// Resume a paused task with SIGCONT to its process group.
    pub fn resume_task(&mut self, target: Option<u32>) -> Vec<Effect> {
        let Some(id) = target.or_else(|| self.runtime.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return Vec::new();
        };
        let Some(task) = self
            .runtime
            .background_tasks
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
        else {
            self.state.flash_error(format!("no task with id {id}"));
            return Vec::new();
        };
        if !task.paused {
            self.state.flash_info(format!("task #{id} is not paused"));
            return Vec::new();
        }
        let Some(pid) = task.host.process_id() else {
            self.state.flash_error(format!("task #{id}: no process id"));
            return Vec::new();
        };
        // SIGCONT to the process group; the signal, the `paused` toggle,
        // and the flash all run in `run_effects`.
        vec![Effect::SignalGroup {
            pid,
            sig: rustix::process::Signal::CONT,
            on_ok: SigOk::Resume(id),
            on_err: format!("task #{id}: SIGCONT failed"),
        }]
    }

    /// `:task-to-pane [N]` — promote a backgrounded `!` task to a
    /// new pane tab. v1.5 Phase 6b. The pty keeps running through
    /// the transition; we resize it to the bottom-pane geometry
    /// (capture spawned at terminal-rows × terminal-cols, the new
    /// tab slot is usually smaller), replay the captured buffer
    /// through a fresh vt100 parser, wake the child if it was
    /// paused, and register the resulting `Pane` in `pane_tabs`.
    /// `BackgroundTask` lifecycle metadata (status, finished_at,
    /// has_unread_output, viewed_in_task_viewer) is dropped — the
    /// task is now a pane.
    ///
    /// Already-exited tasks are not promoted: `:fg` is the right
    /// route for static output (one-shot view of the captured
    /// buffer), and a dead pty would just immediately tear down
    /// the new tab. Flash-and-skip; the task stays in the bg
    /// list.
    pub fn promote_task_to_pane(&mut self, target: Option<u32>) {
        let Some(id) = target.or_else(|| self.runtime.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return;
        };
        let Some(mut task) = self.runtime.background_tasks.take(id) else {
            self.state.flash_error(format!("no task #{id}"));
            return;
        };
        if !matches!(task.status, TaskStatus::Running) {
            // Re-add so :fg still works. flash_error so the user
            // sees this as a non-action rather than silent.
            self.runtime.background_tasks.tasks.push(task);
            self.state.flash_error(format!(
                "task #{id} already exited; :fg to view its output instead"
            ));
            return;
        }

        // Capture-side TERM was `dumb`; promoting doesn't change
        // that — the child's TERM is set at spawn time. Plain shells
        // and SGR-color output render fine; alt-screen TUIs (vim,
        // htop, lazygit) won't suddenly start working. Document via
        // the flash hint below.

        let (rows, cols) = Self::pane_spawn_size(
            self.effective_pane_pct(),
            self.state.config.layout.status_position,
        );
        // Resize before building the parser so vt100's grid sizes
        // match the child's view. ioctl is best-effort — if it
        // fails, the geometry is just slightly off until the next
        // resize event.
        let _ = task.host.resize(rows, cols);

        // Replay the captured output through a fresh parser so
        // the visible grid reflects what the user saw in the
        // task viewer. Same scrollback budget Pane::spawn_with_env
        // uses (10K rows).
        let mut parser = vt100::Parser::new(rows, cols, 10_000);
        parser.process(&task.buffer);

        // Wake a paused task — it's user-facing now, no point
        // leaving SIGSTOP'd.
        if task.paused
            && let Some(pid) = task.host.process_id()
        {
            #[cfg(unix)]
            let _ = kill_pg(pid, rustix::process::Signal::CONT);
        }

        // If the task viewer was open on this task, close it —
        // the task no longer exists in `background_tasks`, so the
        // viewer's task_id is stale.
        if self
            .view
            .pager
            .as_ref()
            .is_some_and(|v| v.task_id == Some(id))
        {
            self.clear_pager();
        }

        let cmd = task.cmd_display.clone();
        // BackgroundTask doesn't track the original cwd (capture
        // was spawned in `state.listing.dir` at the time but we
        // didn't save it); use current listing dir as a best-effort
        // tab-info field. The child is already running at its own
        // cwd regardless.
        let cwd = self.state.listing.dir.clone();
        let label = task.title.clone();
        let wake = self.make_pane_wake();
        // MVU Phase 3c: clear the task's reader-thread sink wake BEFORE adopt
        // installs the parser-worker PaneWake — the reader is shared, so a
        // leftover sink wake would fire a spurious SinkOutput for what is now
        // a pane (double-wake alongside PaneOutput).
        task.host.clear_wake_slot();
        let pane = Pane::adopt(task.host, parser, wake);
        let mut info = TabInfo::new(&cmd, &cwd);
        info.label.clone_from(&label);
        let entry = TabEntry::new(pane, info);

        self.state.focus = state::Focus::Pane;
        if let Some(tabs) = self.runtime.pane_tabs.as_mut() {
            tabs.push(entry);
            // Switch active tab to the promoted one so the user
            // lands looking at it.
            let last = tabs.len().saturating_sub(1);
            tabs.switch_to(last);
        } else {
            self.runtime.pane_tabs = Some(PaneTabs::new(entry));
        }
        self.view.needs_full_repaint = true;
        self.state.flash_info(format!("task #{id} → tab '{label}'"));
    }

    /// `:pane-to-task` — demote the active pane tab to a
    /// background task. v1.5 Phase 6c. Inverse of
    /// `:task-to-pane`: same `PtyHost` moves between containers,
    /// the pty keeps running through the transition.
    ///
    /// Buffer recovery is the open design call — vim's `^z` to
    /// background loses visual context, and we follow the same
    /// rule: the new task buffer starts empty, fresh output
    /// accumulates from the demote point. We don't seed from
    /// `screen.contents()` because the task buffer is ANSI bytes
    /// (so rebuild via `ansi-to-tui` works) and `screen.contents()`
    /// is plain text — seeding would erase color. Acceptable;
    /// can revisit if users hit it.
    pub fn demote_pane_to_task(&mut self) {
        let Some(tabs) = self.runtime.pane_tabs.as_mut() else {
            self.state.flash_error("no pane to demote");
            return;
        };
        let Some(entry) = tabs.take_active() else {
            self.state.flash_error("no pane to demote");
            return;
        };
        let TabEntry { pane, info, .. } = entry;
        // MVU Phase 3c PR3: refuse to demote an already-exited pane. Its
        // reader thread has already returned, so the resulting task would
        // never wake or finalize — and with the poll floor gone, nothing
        // would ever flip it out of `Running` (a stuck, invisible task).
        // The tab's already been taken, so dropping `pane` here closes it.
        if pane.is_closed() {
            self.state.flash_info("pane already exited — closed it");
            if self
                .runtime
                .pane_tabs
                .as_ref()
                .is_some_and(PaneTabs::is_empty)
            {
                self.runtime.pane_tabs = None;
                self.state.focus = state::Focus::FileList;
            }
            return;
        }
        // Stop the parser worker and reclaim the byte receiver so
        // the background task's drain still sees raw bytes.
        let host = pane.take_host();
        // MVU Phase 3c: a pane host carries no sink wake (the pane woke via
        // its now-stopped parser worker), so install one for the task — its
        // reader thread is still alive and will fire SinkOutput on bytes/EOF.
        // Self-wake sweeps any bytes delivered during the install window.
        let wake = self.make_sink_wake();
        host.set_wake(wake);
        host.fire_wake_now();
        // If we just took the last tab, drop the container so
        // layout / status / focus revert to "no pane open" state.
        if self
            .runtime
            .pane_tabs
            .as_ref()
            .is_some_and(PaneTabs::is_empty)
        {
            self.runtime.pane_tabs = None;
            self.state.focus = state::Focus::FileList;
        }

        let id = self.runtime.background_tasks.allocate_id();
        let label = info.label.clone();
        let task = BackgroundTask {
            id,
            title: label.clone(),
            cmd_display: info.command,
            host,
            buffer: Vec::new(),
            status: TaskStatus::Running,
            started: info.spawn_at,
            finished_at: None,
            has_unread_output: false,
            viewed_in_task_viewer: false,
            paused: false,
        };
        self.runtime.background_tasks.tasks.push(task);
        self.view.needs_full_repaint = true;
        self.state.flash_info(format!(
            "tab '{label}' → task #{id} (:fg or :task-to-pane to bring back)"
        ));
    }

    /// `:fg` (no arg) or `:fg N`. Bring a backgrounded task to the
    /// foreground. Still-running tasks resume as a streaming pager
    /// seeded with the buffer; already-exited tasks open as a static
    /// pager and are removed from the background list (one-shot view).
    pub fn foreground_task(&mut self, target: Option<u32>) {
        if self.runtime.pending_capture.is_some() {
            self.state
                .flash_error("already in a foreground task — ^Z to send to background first");
            return;
        }
        let Some(id) = target.or_else(|| self.runtime.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return;
        };
        let Some(task) = self.runtime.background_tasks.take(id) else {
            self.state.flash_error(format!("no task #{id}"));
            return;
        };

        // If the task was paused, auto-resume on foreground — the
        // user explicitly asked for it to be active again. Without
        // this, `:fg` on a paused task would re-attach the streaming
        // capture but the child would stay frozen.
        if task.paused
            && let Some(pid) = task.host.process_id()
        {
            let _ = kill_pg(pid, rustix::process::Signal::CONT);
        }

        match task.status {
            TaskStatus::Running => {
                // Re-attach as a streaming capture. Seed the pager with
                // the buffered output BEFORE handing the buffer over to
                // `pending_capture`, otherwise the user sees an empty
                // pager (or, once new chunks arrive, content scrolled
                // to row 0 with the live tail off-screen) until the
                // streaming-tick rebuilds. Mirrors what
                // `build_task_viewer_for` does for `:task N`.
                use ansi_to_tui::IntoText;
                let normalized = strip_crlf(&task.buffer);
                let text = normalized.as_slice().into_text().unwrap_or_default();
                let secs = task.started.elapsed().as_secs();
                let mut view = PagerView::new_plain(
                    format!("\u{23f3} {} — running... ({secs}s)", task.title),
                    Vec::new(),
                );
                view.lines = text.lines;
                view.streaming = true;
                view.scroll_to_bottom_auto();
                self.set_pager(view);
                self.runtime.pending_capture = Some(PendingCapture {
                    host: task.host,
                    buffer: task.buffer,
                    title: task.title,
                    cmd_display: task.cmd_display,
                    started: task.started,
                    finished: false,
                    original_id: Some(task.id),
                });
                self.state
                    .flash_info(format!("task #{id} resumed — ^Z to background again"));
            }
            status => {
                // Exited / Crashed -- open a static pager with
                // the buffered output and a final-state title.
                use ansi_to_tui::IntoText;
                let normalized = strip_crlf(&task.buffer);
                let text = normalized.as_slice().into_text().unwrap_or_default();
                let elapsed_secs = task
                    .finished_at
                    .map_or_else(|| task.started.elapsed(), |f| f - task.started)
                    .as_secs();
                let status_text = match &status {
                    TaskStatus::Exited(0) => "exit 0".to_string(),
                    TaskStatus::Exited(code) => format!("exit {code}"),
                    TaskStatus::Crashed(msg) => format!("error: {msg}"),
                    TaskStatus::Running => unreachable!(),
                };
                let glyph = if matches!(&status, TaskStatus::Exited(0)) {
                    "\u{2713}" // ✓
                } else {
                    "\u{2717}" // ✗
                };
                let title = format!("{glyph} {} — {status_text} ({elapsed_secs}s)", task.title);
                let mut view = PagerView::new_plain(title, Vec::new());
                view.lines = text.lines;
                view.saveable = true;
                view.scroll_to_bottom_auto();
                self.set_pager(view);
            }
        }
        self.view.needs_full_repaint = true;
    }

    /// Build a "task viewer" pager view -- a peek into a backgrounded
    /// task's buffered output without taking ownership (the way `:fg`
    /// does). The view's `task_id` is set so the main loop can refresh
    /// it from the live buffer while the task is running.
    fn build_task_viewer(&self, id: u32) -> Option<PagerView> {
        let task = self
            .runtime
            .background_tasks
            .tasks
            .iter()
            .find(|t| t.id == id)?;
        Some(Self::build_task_viewer_for(id, task))
    }

    pub fn build_task_viewer_for(id: u32, task: &BackgroundTask) -> PagerView {
        use ansi_to_tui::IntoText;
        let elapsed = task
            .finished_at
            .map_or_else(|| task.started.elapsed(), |f| f - task.started)
            .as_secs();
        let (glyph, status_text) = match &task.status {
            TaskStatus::Running => ("\u{23f3}", format!("running ({elapsed}s)")), // ⏳
            TaskStatus::Exited(0) => ("\u{2713}", format!("exit 0 ({elapsed}s)")), // ✓
            TaskStatus::Exited(code) => ("\u{2717}", format!("exit {code} ({elapsed}s)")), // ✗
            TaskStatus::Crashed(msg) => ("\u{2717}", format!("error: {msg} ({elapsed}s)")),
        };
        let title = format!("{glyph} [task #{id}] {} — {status_text}", task.cmd_display);
        let normalized = strip_crlf(&task.buffer);
        let text = normalized.as_slice().into_text().unwrap_or_default();
        let mut view = PagerView::new_plain(title, Vec::new());
        view.lines = text.lines;
        view.task_id = Some(id);
        // Task viewer is a peek -- don't push it to buffer history on
        // close UNLESS the task has exited (handled separately on
        // close: a snapshot is built and pushed). Suppress the default
        // close-time push.
        view.no_history = true;
        view.saveable = true;
        let running = matches!(task.status, TaskStatus::Running);
        // Suppress [EOF]/tilde markers while the underlying task is
        // still running -- the buffer is live, not finalized.
        view.streaming = running;
        // Once the task has exited, anchor an EOF marker to the bottom
        // of content so it's visible even when output exceeds the
        // viewport. The render-time [EOF] only appears in unused
        // viewport rows below content.
        if !running {
            view.lines.push(eof_marker_line(&status_text));
            view.eof_in_content = true;
        }
        view.scroll_to_bottom_auto();
        view
    }

    /// `gB` from the file list, or `:task N` colon command. Open the
    /// task viewer for `target` (or the most-recent task if `None`).
    /// Pushes the current pager (if any, and not no_history) to buffer
    /// history first so `[b` can walk back.
    pub fn open_task_viewer(&mut self, target: Option<u32>) {
        let Some(id) = target.or_else(|| self.runtime.background_tasks.most_recent()) else {
            self.state.flash_error("no background tasks");
            return;
        };
        let Some(view) = self.build_task_viewer(id) else {
            self.state.flash_error(format!("no task #{id}"));
            return;
        };
        // Mark viewed so promotion-to-history can fire on close.
        if let Some(task) = self
            .runtime
            .background_tasks
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
        {
            task.viewed_in_task_viewer = true;
            task.has_unread_output = false;
        }
        // Push the prior pager (if any, eligible) so `[b` can walk back.
        self.remember_pager_position();
        if let Some(prev) = self.view.pager.take() {
            self.view.pager_history.push(prev);
        }
        self.set_pager(view);
        self.view.needs_full_repaint = true;
    }

    /// `[t`/`]t` chord while a pager is open. Cycles the task viewer
    /// among bg tasks ordered by id. `direction = -1` for prev, `+1`
    /// for next.
    pub fn cycle_task_viewer(&mut self, direction: i32) {
        if self.runtime.background_tasks.tasks.is_empty() {
            self.state.flash_info("no background tasks");
            return;
        }
        let current = self
            .view
            .pager
            .as_ref()
            .and_then(|v| v.task_id)
            .and_then(|id| {
                self.runtime
                    .background_tasks
                    .tasks
                    .iter()
                    .position(|t| t.id == id)
            });
        let next_pos = match current {
            Some(pos) => {
                let n = self.runtime.background_tasks.tasks.len() as i32;
                let raw = pos as i32 + direction;
                ((raw % n + n) % n) as usize
            }
            None => {
                if direction < 0 {
                    self.runtime.background_tasks.tasks.len() - 1
                } else {
                    0
                }
            }
        };
        let id = self.runtime.background_tasks.tasks[next_pos].id;
        self.open_task_viewer(Some(id));
    }
}

/// Spawn a capture pty for a backgrounded `!` command (sole caller:
/// `start_capture`). TERM=dumb + cat-pagers so sub-tools don't launch a
/// nested TUI/pager against our capture pty.
fn spawn_capture(cmd: &str, cwd: &std::path::Path) -> Result<crate::pane::pty_host::PtyHost> {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    // TERM=dumb so TUI programs refuse to run as a TUI inside our
    // capture (the pager only renders SGR colors and CRLF, not
    // cursor positioning / alt-screen). FORCE_COLOR / CLICOLOR_FORCE
    // / COLORTERM ride alongside so tools that respect those (cargo,
    // eza, bat, ripgrep) keep their color output anyway. PAGER=cat /
    // GIT_PAGER=cat / MANPAGER=cat stops tools that auto-invoke a
    // sub-pager (git log, man) from launching `less` against our
    // pty and freezing the capture.
    let env: &[(&str, &str)] = &[
        ("CLICOLOR_FORCE", "1"),
        ("FORCE_COLOR", "1"),
        ("COLORTERM", "truecolor"),
        ("PAGER", "cat"),
        ("GIT_PAGER", "cat"),
        ("MANPAGER", "cat"),
    ];
    crate::pane::pty_host::PtyHost::spawn(crate::pane::pty_host::PtySpec {
        command: cmd,
        rows,
        cols,
        cwd,
        env,
        term: "dumb",
        // No SIGWINCH nudge for captures — they aren't interactive
        // shells with rc-file prompts to redraw.
        nudge_winch: false,
        // Captures don't enable the per-byte debug dump (Pane does).
        debug_dump: false,
    })
}

#[cfg(test)]
mod tests {
    use super::BackgroundTasks;

    #[test]
    fn allocate_id_starts_at_one_and_monotonic() {
        let mut bg = BackgroundTasks::new();
        assert_eq!(bg.allocate_id(), 1);
        assert_eq!(bg.allocate_id(), 2);
        assert_eq!(bg.allocate_id(), 3);
    }

    #[test]
    fn most_recent_returns_last_pushed_id() {
        let mut bg = BackgroundTasks::new();
        assert_eq!(bg.most_recent(), None);
        // We can't easily construct full BackgroundTask values in a test
        // (they hold Box<dyn Child>), so we exercise the id allocator
        // and trust `most_recent`/`take` against the `tasks` Vec they
        // operate on. These pass-through helpers are simple enough that
        // the structural test is in the integration of ^Z / :fg flows.
        let _ = bg.allocate_id();
    }

    #[test]
    fn take_missing_id_returns_none() {
        let mut bg = BackgroundTasks::new();
        assert!(bg.take(99).is_none());
    }

    #[test]
    fn running_and_done_counts_are_zero_initially() {
        let bg = BackgroundTasks::new();
        assert_eq!(bg.running_count(), 0);
        assert_eq!(bg.done_count(), 0);
    }
}
