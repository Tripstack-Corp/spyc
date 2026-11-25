//! The session's event log, in the Model: every flash, and each event of the
//! session's life spyc notes. An event is noted here as it happens, with its
//! [`Origin`]: the project it happened in and, when a tab did it, that tab.
//! The loop stamps it with the time on its pass before it waits, keeps it, and
//! has it written to the session's file (`app::events`). The last [`KEPT`]
//! stay here for `:events`.

use std::collections::VecDeque;

/// Events the Model keeps; older ones drop off the front.
pub const KEPT: usize = 5_000;

/// What an event is: news, or something that went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Info,
    Error,
}

/// Where an event happened: the project, and the tab when one did it, with
/// the ids that trace it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Origin {
    /// The project's label.
    pub project: Option<String>,
    pub project_id: Option<String>,
    /// The tab's 1-based number in its project, while it has one.
    pub tab: Option<usize>,
    pub pane_id: Option<String>,
    /// The MCP connection that asked, as `:activity dump` numbers it.
    pub conn: Option<u64>,
}

impl Origin {
    /// `web tab 2`, `web`, or empty when it names neither.
    pub fn place(&self) -> String {
        match (&self.project, self.tab) {
            (Some(project), Some(tab)) => format!("{project} tab {tab}"),
            (Some(project), None) => project.clone(),
            (None, Some(tab)) => format!("tab {tab}"),
            (None, None) => String::new(),
        }
    }

    /// `pane=… project=… conn=…`, those it has.
    pub fn ids(&self) -> String {
        let mut ids = Vec::new();
        if let Some(pane) = &self.pane_id {
            ids.push(format!("pane={pane}"));
        }
        if let Some(project) = &self.project_id {
            ids.push(format!("project={project}"));
        }
        if let Some(conn) = self.conn {
            ids.push(format!("conn={conn}"));
        }
        ids.join(" ")
    }

    /// Whether a flash from it leads with its place: a tab's does, since the
    /// status row says nothing else about where it came from.
    pub fn leads_flash(&self) -> bool {
        (self.tab.is_some() || self.pane_id.is_some()) && !self.place().is_empty()
    }

    /// What the status row shows for `text` ([`Self::leads_flash`]).
    pub fn flashed(&self, text: String) -> String {
        if self.leads_flash() {
            return format!("{}: {text}", self.place());
        }
        text
    }
}

/// One event, stamped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Local time, `YYYY-MM-DD HH:MM:SS`; empty until the loop stamps it.
    pub at: String,
    pub kind: EventKind,
    /// One line: a newline in what was noted reads as ` ⏎ `, a tab as a
    /// space.
    pub text: String,
    /// [`Origin::place`], empty for none.
    pub place: String,
    /// [`Origin::ids`], empty for none.
    pub ids: String,
}

impl Event {
    /// The event as a line of the session's file: its place and ids, when it
    /// has either, follow the text after a tab each, so a spyc that predates
    /// them reads the line as it always did.
    pub fn to_line(&self) -> String {
        let kind = match self.kind {
            EventKind::Info => 'I',
            EventKind::Error => 'E',
        };
        let mut line = format!("{}  {kind}  {}", self.at, self.text);
        if !self.place.is_empty() || !self.ids.is_empty() {
            line = format!("{line}\t{}\t{}", self.place, self.ids);
        }
        line
    }

    /// An event from a line of the session's file; `None` for one it can't
    /// read.
    pub fn from_line(line: &str) -> Option<Self> {
        let mut parts = line.splitn(3, "  ");
        let at = parts.next()?.to_string();
        let kind = match parts.next()? {
            "I" => EventKind::Info,
            "E" => EventKind::Error,
            _ => return None,
        };
        let mut rest = parts.next().unwrap_or_default().splitn(3, '\t');
        let mut next = || rest.next().unwrap_or_default().to_string();
        Some(Self {
            at,
            kind,
            text: next(),
            place: next(),
            ids: next(),
        })
    }
}

#[derive(Debug, Default)]
pub struct EventLog {
    /// Noted since the loop's last pass, not yet stamped.
    pending: Vec<Event>,
    kept: VecDeque<Event>,
    /// Bumped whenever `kept` changes, so a pager showing it knows it's old.
    pub generation: u64,
    /// The tab whose doing is noted now, set while `App::noting_from` runs
    /// it; with none, an event is the active project's.
    pub source: Option<Origin>,
}

impl EventLog {
    /// A log whose first event is `text`: a spyc's start, noted before
    /// anything it does.
    pub fn starting(text: &str) -> Self {
        let mut log = Self::default();
        log.note(EventKind::Info, text, &Origin::default());
        log
    }

    /// Note an event from `origin`. The same text from the same place noted
    /// again straight after is one event.
    pub fn note(&mut self, kind: EventKind, text: &str, origin: &Origin) {
        let text = text.trim().replace('\n', " \u{23ce} ").replace('\t', " ");
        let place = origin.place();
        let last = self.pending.last().or_else(|| self.kept.back());
        if text.is_empty()
            || last.is_some_and(|e| e.kind == kind && e.text == text && e.place == place)
        {
            return;
        }
        self.pending.push(Event {
            at: String::new(),
            kind,
            text,
            place,
            ids: origin.ids(),
        });
    }

    /// What was noted since the last pass, unstamped, for the loop to stamp.
    pub fn take_pending(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }

    /// Keep stamped events, newest last.
    pub fn keep(&mut self, events: impl IntoIterator<Item = Event>) {
        self.kept.extend(events);
        while self.kept.len() > KEPT {
            self.kept.pop_front();
        }
        self.generation += 1;
    }

    /// Put events from before this spyc in front: the session's file, read
    /// back.
    pub fn earlier(&mut self, events: Vec<Event>) {
        let room = KEPT.saturating_sub(self.kept.len());
        let skip = events.len().saturating_sub(room);
        for event in events.into_iter().skip(skip).rev() {
            self.kept.push_front(event);
        }
        self.generation += 1;
    }

    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.kept.iter()
    }

    pub fn len(&self) -> usize {
        self.kept.len()
    }

    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(n: u32) -> Event {
        Event {
            at: format!("2026-11-19 10:00:{n:02}"),
            kind: EventKind::Info,
            text: format!("event {n}"),
            place: String::new(),
            ids: String::new(),
        }
    }

    fn tab_of(project: &str, tab: usize) -> Origin {
        Origin {
            project: Some(project.into()),
            project_id: Some("p-1".into()),
            tab: Some(tab),
            pane_id: Some("t-9".into()),
            conn: Some(7),
        }
    }

    #[test]
    fn a_noted_event_waits_for_its_stamp_and_a_repeat_is_one() {
        let none = Origin::default();
        let mut log = EventLog::default();
        log.note(EventKind::Info, "saved\nfile", &none);
        log.note(EventKind::Info, "saved\nfile", &none);
        log.note(EventKind::Error, "saved\nfile", &none);
        log.note(EventKind::Info, "  ", &none);
        let pending: Vec<_> = log
            .take_pending()
            .into_iter()
            .map(|e| (e.kind, e.text))
            .collect();
        assert_eq!(
            pending,
            [
                (EventKind::Info, "saved \u{23ce} file".to_string()),
                (EventKind::Error, "saved \u{23ce} file".to_string())
            ]
        );
        assert_eq!(log.take_pending(), []);
        assert!(log.is_empty(), "nothing is kept until it's stamped");
    }

    #[test]
    fn a_repeat_of_the_last_kept_event_is_one_too() {
        let mut log = EventLog::default();
        log.keep([at(1)]);
        log.note(EventKind::Info, "event 1", &Origin::default());
        assert_eq!(log.take_pending(), []);
    }

    /// The same words from two tabs are two events: each says where.
    #[test]
    fn the_same_text_from_another_tab_is_another_event() {
        let mut log = EventLog::default();
        log.note(EventKind::Info, "claude finished", &tab_of("web", 1));
        log.note(EventKind::Info, "claude finished", &tab_of("web", 2));
        log.note(EventKind::Info, "claude finished", &tab_of("web", 2));
        let places: Vec<_> = log.take_pending().into_iter().map(|e| e.place).collect();
        assert_eq!(places, ["web tab 1", "web tab 2"]);
    }

    /// A tab's event names its project and tab, and carries the ids that trace
    /// it; one with only a project names that.
    #[test]
    fn an_origin_names_its_place_and_ids() {
        let origin = tab_of("web", 2);
        assert_eq!(origin.place(), "web tab 2");
        assert_eq!(origin.ids(), "pane=t-9 project=p-1 conn=7");
        assert_eq!(
            origin.flashed("claude finished".into()),
            "web tab 2: claude finished"
        );
        let project = Origin {
            project: Some("web".into()),
            ..Origin::default()
        };
        assert_eq!(project.place(), "web");
        assert_eq!(
            project.flashed("saved: notes.md".into()),
            "saved: notes.md",
            "the user's own doing needs no place on the status row"
        );
        assert_eq!(Origin::default().place(), "");
    }

    #[test]
    fn the_oldest_drop_off_and_earlier_ones_go_in_front() {
        let mut log = EventLog::default();
        log.keep((0..KEPT as u32 + 3).map(at));
        assert_eq!(log.len(), KEPT);
        assert_eq!(log.iter().next().unwrap().text, "event 3");

        let mut log = EventLog::default();
        log.keep([at(5)]);
        let before = log.generation;
        log.earlier(vec![at(1), at(2)]);
        let texts: Vec<_> = log.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["event 1", "event 2", "event 5"]);
        assert!(log.generation > before);
    }

    #[test]
    fn an_event_reads_back_from_its_line() {
        let mut event = Event {
            at: "2026-11-19 10:00:01".into(),
            kind: EventKind::Error,
            text: "jar upgrade stopped:  two  spaces".into(),
            place: String::new(),
            ids: String::new(),
        };
        assert_eq!(
            event.to_line(),
            "2026-11-19 10:00:01  E  jar upgrade stopped:  two  spaces",
            "an event with no origin writes the line an older spyc wrote"
        );
        assert_eq!(Event::from_line(&event.to_line()), Some(event.clone()));
        event.place = "web tab 2".into();
        event.ids = "pane=t-9 project=p-1".into();
        assert_eq!(Event::from_line(&event.to_line()), Some(event));
        assert_eq!(Event::from_line("garbage"), None);
    }
}
