//! The session's event log, in the Model: every flash, and each event of the
//! session's life spyc notes. An event is noted here as it happens; the loop
//! stamps it with the time on its pass before it waits, keeps it, and has it
//! written to the session's file (`app::events`). The last [`KEPT`] stay
//! here for `:events`.

use std::collections::VecDeque;

/// Events the Model keeps; older ones drop off the front.
pub const KEPT: usize = 5_000;

/// What an event is: news, or something that went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Info,
    Error,
}

/// One event, stamped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Local time, `YYYY-MM-DD HH:MM:SS`.
    pub at: String,
    pub kind: EventKind,
    /// One line: a newline in what was noted reads as ` ⏎ `.
    pub text: String,
}

impl Event {
    /// The event as a line of the session's file.
    pub fn to_line(&self) -> String {
        let kind = match self.kind {
            EventKind::Info => 'I',
            EventKind::Error => 'E',
        };
        format!("{}  {kind}  {}", self.at, self.text)
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
        Some(Self {
            at,
            kind,
            text: parts.next().unwrap_or_default().to_string(),
        })
    }
}

#[derive(Debug, Default)]
pub struct EventLog {
    /// Noted since the loop's last pass, not yet stamped.
    pending: Vec<(EventKind, String)>,
    kept: VecDeque<Event>,
    /// Bumped whenever `kept` changes, so a pager showing it knows it's old.
    pub generation: u64,
}

impl EventLog {
    /// A log whose first event is `text`: a spyc's start, noted before
    /// anything it does.
    pub fn starting(text: &str) -> Self {
        let mut log = Self::default();
        log.note(EventKind::Info, text);
        log
    }

    /// Note an event. The same text noted again straight after is one event.
    pub fn note(&mut self, kind: EventKind, text: &str) {
        let text = text.trim().replace('\n', " \u{23ce} ");
        let last = self
            .pending
            .last()
            .map(|(k, t)| (*k, t.as_str()))
            .or_else(|| self.kept.back().map(|e| (e.kind, e.text.as_str())));
        if text.is_empty() || last == Some((kind, text.as_str())) {
            return;
        }
        self.pending.push((kind, text));
    }

    /// What was noted since the last pass, for the loop to stamp.
    pub fn take_pending(&mut self) -> Vec<(EventKind, String)> {
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
        }
    }

    #[test]
    fn a_noted_event_waits_for_its_stamp_and_a_repeat_is_one() {
        let mut log = EventLog::default();
        log.note(EventKind::Info, "saved\nfile");
        log.note(EventKind::Info, "saved\nfile");
        log.note(EventKind::Error, "saved\nfile");
        log.note(EventKind::Info, "  ");
        let pending = log.take_pending();
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
        log.note(EventKind::Info, "event 1");
        assert_eq!(log.take_pending(), []);
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
        let event = Event {
            at: "2026-11-19 10:00:01".into(),
            kind: EventKind::Error,
            text: "jar upgrade stopped:  two  spaces".into(),
        };
        assert_eq!(Event::from_line(&event.to_line()), Some(event));
        assert_eq!(Event::from_line("garbage"), None);
    }
}
