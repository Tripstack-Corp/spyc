//! `:events` (and `:log`): the session's event log. Every flash, and each
//! event of the session's life spyc notes, is noted in the Model
//! (`state::events`), stamped with the time on the loop's pass before it
//! waits, and appended to the session's file,
//! `<state>/events/<SESSION>.log`, by a writer thread
//! (`crate::state::event_log`). A crash, a handover or an upgrade loses none of
//! it: the next spyc on the session reads the file's lines back. The pager
//! shows the log newest last, and keeps up with it while it is in view.

use std::fmt::Write as _;
use std::sync::mpsc::Sender;

use super::state::{Event, EventKind, EventLog, Origin};
use super::{App, Effect, Wake};
use crate::state::event_log::Write;
use crate::ui::pager::PagerView;

/// How long a flush waits for the writer: an append to a local file.
const FLUSH_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// The writer, the session name it was last told, and its thread, which a
/// spyc on its way out waits for so nothing noted is lost.
pub(super) struct EventWriter {
    tx: Sender<Write>,
    told: Option<String>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl App {
    /// Start the writer, once `run` can wake the loop. A start error spyc
    /// set before the loop ran is noted too.
    pub(super) fn start_event_log(&mut self) {
        if let Some(flash) = self.state.flash.clone()
            && matches!(flash.kind, super::FlashKind::Error)
        {
            self.state.note_error(&flash.text);
        }
        let Some(root) = crate::state::state_root() else {
            return;
        };
        let landed = self
            .runtime
            .events_earlier
            .sender_waking(self.loop_waker(Wake::PaneStart));
        let (tx, thread) = crate::state::event_log::spawn(root.join("events"), move |lines| {
            let _ = landed.send(lines);
        });
        self.runtime.event_writer = Some(EventWriter {
            tx,
            told: None,
            thread,
        });
    }

    /// On the way out (quit, handover, upgrade): write what was noted since
    /// the last pass, and wait for the writer to finish. Once a run.
    pub(crate) fn close_event_log(&mut self) {
        self.settle_events();
        if let Some(EventWriter { tx, thread, .. }) = self.runtime.event_writer.take() {
            drop(tx);
            if let Some(thread) = thread {
                let _ = thread.join();
            }
        }
    }

    /// Write what was noted so far, and wait for the writer to have written
    /// it: another spyc about to write to the same file (a handover's) then
    /// writes after it. The wait is bounded; the writer answers in order.
    pub(crate) fn flush_events(&mut self) {
        self.settle_events();
        let Some(writer) = &self.runtime.event_writer else {
            return;
        };
        let (done, written) = std::sync::mpsc::channel();
        if writer.tx.send(Write::Flush(done)).is_ok() {
            let _ = written.recv_timeout(FLUSH_WAIT);
        }
    }

    /// Stamp what was noted since the last pass, keep it, and have it
    /// written. Whether the frame changed.
    pub(crate) fn settle_events(&mut self) -> bool {
        let pending = self.state.events.take_pending();
        if let Some(writer) = &mut self.runtime.event_writer
            && let Some(name) = self.state.session_name.as_deref()
            && writer.told.as_deref() != Some(name)
        {
            let _ = writer.tx.send(Write::Session(name.to_string()));
            writer.told = Some(name.to_string());
        }
        if pending.is_empty() {
            return false;
        }
        let at = jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S").to_string();
        let stamped: Vec<Event> = pending
            .into_iter()
            .map(|event| Event {
                at: at.clone(),
                ..event
            })
            .collect();
        if let Some(writer) = &self.runtime.event_writer
            && writer.told.is_some()
        {
            let _ = writer
                .tx
                .send(Write::Append(stamped.iter().map(Event::to_line).collect()));
        }
        self.state.events.keep(stamped);
        self.refresh_events_pager()
    }

    /// The session's events from before this spyc, read back from its file.
    /// Whether the frame changed.
    pub(crate) fn apply_earlier_events(&mut self) -> bool {
        let landed = self.runtime.events_earlier.drain();
        if landed.is_empty() {
            return false;
        }
        for lines in landed {
            let events = lines.iter().filter_map(|l| Event::from_line(l)).collect();
            self.state.events.earlier(events);
        }
        self.refresh_events_pager()
    }

    /// Rebuild an `:events` pager in view that shows an older log, staying at
    /// the bottom if it was there.
    fn refresh_events_pager(&mut self) -> bool {
        let session = self.state.session_display().to_string();
        let log = &self.state.events;
        let Some(view) = self.view.project_mut().pager.as_mut() else {
            return false;
        };
        if view
            .events_log
            .is_none_or(|generation| generation == log.generation)
        {
            return false;
        }
        let height = match view.last_viewport_h.get() {
            0 => 40,
            h => h,
        };
        let at_bottom = view.scroll >= view.scroll_max(height);
        let fresh = events_view(log, &session);
        view.lines = fresh.lines;
        view.title = fresh.title;
        view.events_log = fresh.events_log;
        view.clamp_state_to_lines();
        if at_bottom {
            view.scroll_to_bottom_auto();
        } else {
            view.clamp_scroll_auto();
        }
        true
    }

    /// Where the tab `pane_id` is: its project and number, and its ids. A tab
    /// that's gone keeps its pane id.
    pub(super) fn tab_origin(&self, pane_id: &str) -> Origin {
        let project = self
            .runtime
            .project_of_pane(pane_id)
            .and_then(|id| self.state.project(id));
        Origin {
            project: project.map(super::state::Project::label),
            project_id: project.map(|p| p.id.to_string()),
            tab: self.runtime.tab_of_pane(pane_id).map(|(number, _)| number),
            pane_id: Some(pane_id.to_string()),
            conn: None,
        }
    }

    /// Who an MCP request came from: the tab `pane_id`, else the active
    /// project, with the connection `conn`; `None` when it names neither.
    pub(super) fn asker_origin(&self, pane_id: Option<&str>, conn: Option<u64>) -> Option<Origin> {
        if pane_id.is_none() && conn.is_none() {
            return None;
        }
        let mut origin = pane_id.map_or_else(|| self.state.origin(), |id| self.tab_origin(id));
        origin.conn = conn;
        Some(origin)
    }

    /// Run `f` with what it notes and flashes coming from `origin`, when
    /// there is one: a tab's doing, which the event log and the status row
    /// then say is that tab's.
    pub(super) fn noting_from<R>(
        &mut self,
        origin: Option<Origin>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let Some(origin) = origin else {
            return f(self);
        };
        let outer = self.state.events.source.replace(origin);
        let out = f(self);
        self.state.events.source = outer;
        out
    }

    /// What the session's event log calls the tab `pane_id` names: its label,
    /// or what it is when it names none.
    pub(super) fn tab_label_of(&self, pane_id: Option<&str>) -> String {
        let Some(id) = pane_id else {
            return "an agent outside any tab".into();
        };
        self.runtime
            .every_tab_set()
            .flat_map(|(_, tabs)| tabs.tabs().iter())
            .find(|e| e.info.id == id)
            .map_or_else(
                || "a tab that's gone".into(),
                |e| crate::pane::tabs::strip_exit_suffix(&e.info.label),
            )
    }

    /// `:events`: the log in the pager, newest last.
    pub(super) fn open_events(&mut self) -> Vec<Effect> {
        let mut view = events_view(&self.state.events, self.state.session_display());
        view.scroll_to_bottom_auto();
        self.set_pager(view);
        Vec::new()
    }
}

/// The kind an agent's turn is noted as, when it is: one that needs the user,
/// finished, or failed.
pub(super) const fn noted_turn(new: crate::pane::AgentActivity) -> Option<EventKind> {
    use crate::pane::AgentActivity::{Blocked, Done, Error};
    match new {
        Blocked | Done => Some(EventKind::Info),
        Error => Some(EventKind::Error),
        _ => None,
    }
}

/// The note a spyc's start is: the first event of its log.
pub(super) fn started_note() -> String {
    format!(
        "spyc {} started, pid {}",
        crate::VERSION,
        std::process::id()
    )
}

/// `:events` / `:log`.
pub(super) fn cmd_events(app: &mut App, _args: &str) -> Vec<Effect> {
    app.open_events()
}

/// The pager for `log`: one line an event, as `spyc events` prints it
/// (`crate::state::event_log::shown`).
fn events_view(log: &EventLog, session: &str) -> PagerView {
    let mut title = format!("events \u{2014} {session}");
    let _ = match log.len() {
        1 => write!(title, " \u{2014} 1 event"),
        n => write!(title, " \u{2014} {n} events"),
    };
    let mut text = String::new();
    for event in log.iter() {
        let _ = writeln!(
            text,
            "{}",
            crate::state::event_log::shown(&event.to_line(), true)
        );
    }
    if log.is_empty() {
        text.push_str("no events yet this session\n");
    }
    let mut view = PagerView::new_styled(title, super::util::buffer_to_lines(text.as_bytes()));
    view.events_log = Some(log.generation);
    view.saveable = true;
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(app: &App) -> String {
        let view = app.view.project().pager.as_ref().expect("a pager");
        view.lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A flash gone from the status row stays in `:events`, and one that comes
    /// while the log is open joins it there.
    #[test]
    fn a_flash_stays_in_the_log_and_an_open_log_keeps_up() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::test_app(tmp.path().to_path_buf());
        app.state.flash_error("detach: spyc's jar didn't answer");
        app.state.flash_info("saved: notes.md");
        app.state.flash = None;
        app.settle_events();
        app.dispatch_command("events");
        let text = shown(&app);
        assert!(text.contains("detach: spyc's jar didn't answer"), "{text}");
        assert!(text.contains("saved: notes.md"), "{text}");

        app.state.flash_info("a later flash");
        assert!(app.settle_events(), "the open log is redrawn");
        assert!(shown(&app).contains("a later flash"));
        app.dispatch_command("log");
        assert!(shown(&app).contains("a later flash"), ":log is :events");
    }

    fn noted(app: &App) -> Vec<String> {
        app.state.events.iter().map(|e| e.text.clone()).collect()
    }

    /// `:q` is noted, and a hint about where focus went isn't.
    #[test]
    fn a_quit_is_noted_and_a_focus_hint_is_not() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::test_app(tmp.path().to_path_buf());
        app.state.flash_hint("focus: claude");
        app.dispatch_command("q");
        app.settle_events();
        let noted = noted(&app);
        assert!(noted.iter().any(|e| e.starts_with("quit:")), "{noted:?}");
        assert!(!noted.iter().any(|e| e.contains("focus:")), "{noted:?}");
    }

    /// An agent's MCP connection is noted as it comes and goes.
    #[test]
    fn an_mcp_connection_is_noted_as_it_comes_and_goes() {
        use crate::mcp_cmd::McpCommand;
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::test_app(tmp.path().to_path_buf());
        app.execute_mcp_command(McpCommand::ConnectionInitialized {
            conn: 7,
            pane_id: None,
        });
        app.execute_mcp_command(McpCommand::ConnectionClosed { conn: 7 });
        app.settle_events();
        let noted = noted(&app);
        assert!(
            noted.contains(&"MCP: an agent outside any tab connected".to_string())
                && noted.contains(&"MCP: an agent outside any tab disconnected".to_string()),
            "{noted:?}"
        );
    }

    /// The session's earlier events, read back from its file, come first.
    #[test]
    fn earlier_events_come_first() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::test_app(tmp.path().to_path_buf());
        app.state.flash_info("this spyc's");
        app.settle_events();
        app.runtime
            .events_earlier
            .land(vec!["2026-11-18 09:00:00  E  the last spyc's".to_string()]);
        app.apply_earlier_events();
        let texts: Vec<_> = app.state.events.iter().map(|e| e.text.clone()).collect();
        assert_eq!(texts, ["the last spyc's", "this spyc's"]);
    }
}
