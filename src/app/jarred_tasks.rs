//! The `!` captures and background tasks whose programs run in jars, as a
//! session saves them and a restore takes them back (S2c). A capture's jar
//! keeps its output until a spyc reads it, so after a crash `spyc -r`
//! reopens each as a background task, its jar taken back on a worker
//! (`app::capture_starts`) and its output replayed from the oldest byte the
//! jar holds. Its program is never started again: a jar that's gone ends the
//! task saying so. A restore of a session another spyc still runs takes
//! none: they're that spyc's.

use std::hash::Hash;

use crate::state::sessions::SavedTask;

use super::App;
use super::tasks::{BackgroundTask, TaskStatus};

impl App {
    /// The captures and tasks a save records: each whose program runs in a
    /// jar and hasn't ended.
    pub(super) fn saved_tasks(&self) -> Vec<SavedTask> {
        let capture = self
            .runtime
            .pending_capture
            .iter()
            .filter(|c| !c.finished)
            .filter_map(|c| saved(&c.title, &c.cmd_display, &c.host));
        let tasks = self
            .runtime
            .background_tasks
            .tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Running))
            .filter_map(|t| saved(&t.title, &t.cmd_display, &t.host));
        capture.chain(tasks).collect()
    }

    /// [`Self::saved_tasks`]'s part of the session's fingerprint.
    pub(super) fn hash_saved_tasks(&self, h: &mut std::collections::hash_map::DefaultHasher) {
        self.saved_tasks().hash(h);
    }

    /// Take back each saved task's jar, as a background task on a stand-in
    /// until its jar's host lands.
    pub(in crate::app) fn restore_tasks(&mut self, tasks: &[SavedTask]) {
        for saved in tasks {
            let (host, jar_start) = match self.retake_task_host(&saved.jar_socket) {
                Ok(started) => started,
                Err(e) => {
                    self.state
                        .flash_error(format!("restore: task {}: {e:#}", saved.title));
                    continue;
                }
            };
            let id = self.runtime.background_tasks.allocate_id();
            self.runtime.background_tasks.tasks.push(BackgroundTask {
                id,
                title: saved.title.clone(),
                cmd_display: saved.cmd_display.clone(),
                host,
                buffer: Vec::new(),
                status: TaskStatus::Running,
                started: std::time::Instant::now(),
                finished_at: None,
                has_unread_output: false,
                viewed_in_task_viewer: false,
                paused: false,
                jar_start: Some(jar_start),
            });
        }
    }
}

/// A capture or task as a save records it, when its program runs in a jar.
fn saved(
    title: &str,
    cmd_display: &str,
    host: &crate::pane::pty_host::PtyHost,
) -> Option<SavedTask> {
    host.jar_socket().map(|socket| SavedTask {
        title: title.to_string(),
        cmd_display: cmd_display.to_string(),
        jar_socket: socket.to_path_buf(),
    })
}
