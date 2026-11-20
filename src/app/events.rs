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

use super::state::{Event, EventKind, EventLog};
use super::{App, Effect, Wake};
use crate::state::event_log::Write;
use crate::ui::pager::PagerView;

/// The writer, and the session name it was last told.
pub(super) struct EventWriter {
    tx: Sender<Write>,
    told: Option<String>,
}

impl App {
    /// Start the writer, once `run` can wake the loop, and note the start.
    pub(super) fn start_event_log(&mut self) {
        let pid = std::process::id();
        self.state.note_event(&format!(
            "spyc {} ({}) started, pid {pid}",
            crate::VERSION,
            env!("SPYC_GIT_SHA")
        ));
        if let Some(flash) = self.state.flash.clone()
            && matches!(flash.kind, super::FlashKind::Error)
        {
            self.state.events.note(EventKind::Error, &flash.text);
        }
        let Some(root) = crate::state::state_root() else {
            return;
        };
        let landed = self
            .runtime
            .events_earlier
            .sender_waking(self.loop_waker(Wake::PaneStart));
        let tx = crate::state::event_log::spawn(root.join("events"), move |lines| {
            let _ = landed.send(lines);
        });
        self.runtime.event_writer = Some(EventWriter { tx, told: None });
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
            .map(|(kind, text)| Event {
                at: at.clone(),
                kind,
                text,
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

    /// `:events`: the log in the pager, newest last.
    pub(super) fn open_events(&mut self) -> Vec<Effect> {
        let mut view = events_view(&self.state.events, self.state.session_display());
        view.scroll_to_bottom_auto();
        self.set_pager(view);
        Vec::new()
    }
}

/// `:events` / `:log`.
pub(super) fn cmd_events(app: &mut App, _args: &str) -> Vec<Effect> {
    app.open_events()
}

/// The pager for `log`: one line an event, its time then what happened, an
/// error in red.
fn events_view(log: &EventLog, session: &str) -> PagerView {
    let mut title = format!("events \u{2014} {session}");
    let _ = match log.len() {
        1 => write!(title, " \u{2014} 1 event"),
        n => write!(title, " \u{2014} {n} events"),
    };
    let mut text = String::new();
    for event in log.iter() {
        let when = event.at.get(5..).unwrap_or(&event.at);
        let _ = match event.kind {
            EventKind::Info => writeln!(text, "{when}  {}", event.text),
            EventKind::Error => writeln!(text, "{when}  \x1b[31m{}\x1b[0m", event.text),
        };
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
