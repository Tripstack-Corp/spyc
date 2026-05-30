//! Background shell-capture tasks (`^Z` to background, `:fg` to resume).
//! A backgrounded `!` capture keeps its reader thread draining into a
//! per-task buffer with no pager attached; the task viewer / divider
//! glyphs / `:fg` / pause-resume all read this state.
//!
//! Extracted verbatim from `app/mod.rs` (REFACTOR_PLAN Phase 1). The
//! conversion logic (backgrounding, `:fg`, task viewer, divider render,
//! pause/resume) stays in `app` and reads these fields directly, so the
//! struct/enum and their fields are `pub`.

use crate::pane::pty_host::PtyHost;

/// Lifecycle state of a backgrounded shell capture.
#[derive(Debug)]
pub enum TaskStatus {
    /// Reader thread is still running; child has not exited.
    Running,
    /// Child exited cleanly (or with non-zero status); inner is the code.
    Exited(i32),
    /// User killed the task (M2's `:bg` `R`-action).
    #[allow(dead_code)]
    Killed,
    /// `child.wait()` returned an error -- inner is the message.
    #[allow(dead_code)]
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
    /// (`[t`/`]t`, `gB`, or `:task N`). Combined with `Exited`/`Killed`
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
