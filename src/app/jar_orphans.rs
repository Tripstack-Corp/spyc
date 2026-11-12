//! Jar spike: the notice of orphaned jars at spyc's start (the linger
//! decision). Panes and tasks wait for an owner however long, so a jar no spyc
//! takes back would run on unseen; a few seconds in, once restored tabs have
//! adopted theirs, a worker lists the live jars with no owner and the notice
//! names them.

use super::{App, Wake};

/// How long after start the jars are looked at: restored tabs adopt theirs
/// first.
const SETTLE: std::time::Duration = std::time::Duration::from_secs(3);

impl App {
    /// Look for orphaned jars off the loop, once.
    pub(super) fn spawn_jar_orphans(&self) {
        if !crate::pane::jar_backend::enabled() {
            return;
        }
        let results = self
            .runtime
            .jar_orphans
            .sender(self.runtime.pane_wake_tx.clone(), Wake::JarOrphans);
        std::thread::spawn(move || {
            std::thread::sleep(SETTLE);
            let _ = results.send(crate::pane::jar_backend::orphans());
        });
    }

    /// Flash what the worker found. Returns whether the frame changed.
    pub(crate) fn apply_jar_orphans(&mut self) -> bool {
        let Some(found) = self.runtime.jar_orphans.drain().into_iter().next() else {
            return false;
        };
        if found.is_empty() {
            return false;
        }
        let shown: Vec<&str> = found.iter().take(3).map(String::as_str).collect();
        let more = found.len().saturating_sub(shown.len());
        let tail = if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        };
        self.state.flash_info(format!(
            "{} jar(s) no spyc owns: {}{tail} (spyc --jars)",
            found.len(),
            shown.join(", ")
        ));
        true
    }
}
