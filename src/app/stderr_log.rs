//! The `:stderr` log: a follower of the file [`crate::stderr_capture`] points
//! spyc's stderr at, the reading it lands in the Model for the status bar's
//! `⚠N`, and the pager that shows it. `:stderr` or a click on the `⚠N` opens
//! the pager, which is rebuilt in place while it is in view and more arrives.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use ratatui::text::Line;

use super::state::StderrSnapshot;
use super::{App, Effect, Message, Wake};
use crate::agent::tail::watch::{Refresh, Subscription};
use crate::ui::pager::PagerView;

/// Lines the log keeps; older ones drop off the top.
const MAX_LINES: usize = 2_000;
/// Bytes of one line the log keeps; the rest of a longer one is dropped.
const MAX_LINE_BYTES: usize = 4 * 1024;
/// Past this size the file is emptied once read. What it held stays in the
/// log; a write landing between the read and the emptying is lost.
const FILE_CAP: u64 = 4 * 1024 * 1024;

/// What the follower has read, as lines.
#[derive(Default)]
struct Lines {
    kept: VecDeque<String>,
    /// The line still being written: the bytes since the last newline.
    partial: Vec<u8>,
    /// Lines finished this run.
    finished: u64,
    /// Bytes read this run.
    bytes: u64,
}

impl Lines {
    fn feed(&mut self, bytes: &[u8]) {
        self.bytes += bytes.len() as u64;
        for fragment in bytes.split_inclusive(|byte| *byte == b'\n') {
            let room = MAX_LINE_BYTES.saturating_sub(self.partial.len());
            self.partial
                .extend_from_slice(&fragment[..fragment.len().min(room)]);
            if fragment.ends_with(b"\n") {
                let line = String::from_utf8_lossy(&self.partial)
                    .trim_end_matches(['\n', '\r'])
                    .to_owned();
                self.partial.clear();
                self.kept.push_back(line);
                if self.kept.len() > MAX_LINES {
                    self.kept.pop_front();
                }
                self.finished += 1;
            }
        }
    }

    fn snapshot(&self, following: bool) -> StderrSnapshot {
        let partial =
            (!self.partial.is_empty()).then(|| String::from_utf8_lossy(&self.partial).into_owned());
        StderrSnapshot {
            total: self.finished + u64::from(partial.is_some()),
            lines: self.kept.iter().cloned().chain(partial).collect(),
            bytes: self.bytes,
            following,
        }
    }
}

/// The follower's half: reads what was appended to the log since the last
/// wake, on the subscription's worker thread.
struct Follow {
    path: PathBuf,
    /// Opened on the first read, so the loop never waits on it.
    file: Option<File>,
    offset: u64,
    cap: u64,
    lines: Lines,
    pruned: bool,
    refreshes: u64,
}

impl Follow {
    fn new(path: PathBuf, cap: u64) -> Self {
        Self {
            path,
            file: None,
            offset: 0,
            cap,
            lines: Lines::default(),
            pruned: false,
            refreshes: 0,
        }
    }

    fn read_appended(&mut self) -> io::Result<()> {
        if self.file.is_none() {
            let file = File::options().read(true).write(true).open(&self.path)?;
            self.file = Some(file);
        }
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        let len = file.metadata()?.len();
        if len < self.offset {
            self.offset = 0;
        }
        file.seek(SeekFrom::Start(self.offset))?;
        // Up to the length seen now: a write after it wakes the watch again.
        let mut appended = Vec::new();
        Read::take(&mut *file, len - self.offset).read_to_end(&mut appended)?;
        self.offset += appended.len() as u64;
        self.lines.feed(&appended);
        if self.offset >= self.cap {
            file.set_len(0)?;
            self.offset = 0;
        }
        Ok(())
    }
}

impl Refresh for Follow {
    type Update = StderrSnapshot;

    fn refresh(&mut self) -> StderrSnapshot {
        if !self.pruned {
            self.pruned = true;
            crate::stderr_capture::prune();
        }
        self.refreshes += 1;
        // A failed read keeps what was read before it; the next write retries.
        let _ = self.read_appended();
        self.lines.snapshot(true)
    }

    fn watch_failed(&self) -> StderrSnapshot {
        self.lines.snapshot(false)
    }

    fn work(&self) -> (u64, u64) {
        (self.lines.bytes, self.refreshes)
    }
}

/// The pager for `reading`, its title naming the file the whole log is in.
fn stderr_view(reading: &StderrSnapshot, path: Option<&Path>) -> PagerView {
    let held = reading.lines.len() as u64;
    let mut title = match reading.total {
        1 => "stderr \u{2014} 1 line".to_string(),
        n => format!("stderr \u{2014} {n} lines"),
    };
    if reading.total > held {
        let _ = write!(title, ", the last {held} kept");
    }
    if !reading.following {
        title.push_str(", no longer following");
    }
    if let Some(path) = path {
        let _ = write!(title, " \u{2014} {}", crate::paths::display_tilde(path));
    }
    let lines = if reading.lines.is_empty() {
        vec![Line::from("nothing written to stderr this run")]
    } else {
        super::util::buffer_to_lines(reading.lines.join("\n").as_bytes())
    };
    let mut view = PagerView::new_styled(title, lines);
    view.stderr_log = Some(reading.bytes);
    view.saveable = true;
    view
}

impl App {
    /// This run's stderr log, when [`crate::stderr_capture`] redirected
    /// stderr; `run` follows it.
    pub fn set_stderr_log(&mut self, path: Option<PathBuf>) {
        self.runtime.stderr_log = path;
    }

    /// Follow the log, waking the loop through `wake` as it grows.
    pub(super) fn follow_stderr_log(&mut self, wake: Sender<Message>) {
        let Some(path) = self.runtime.stderr_log.clone() else {
            return;
        };
        let follow = Follow::new(path.clone(), FILE_CAP);
        self.runtime.stderr_follow = Some(Subscription::start(path, follow, move || {
            let _ = wake.send(Message::Wake(Wake::Stderr));
        }));
    }

    /// Take the follower's newest reading and bring a `:stderr` pager in view
    /// up to it. Whether the frame changed.
    pub(super) fn apply_stderr(&mut self) -> bool {
        let before = self.state.stderr.unread();
        if let Some(reading) = self
            .runtime
            .stderr_follow
            .as_ref()
            .and_then(Subscription::take)
        {
            self.state.stderr.latest = reading;
        }
        let rebuilt = self.refresh_stderr_pager();
        rebuilt || self.state.stderr.unread() != before
    }

    /// Rebuild the `:stderr` pager in view if it shows an older reading, staying
    /// at the bottom if it was there. What it shows has been seen.
    fn refresh_stderr_pager(&mut self) -> bool {
        let latest = &self.state.stderr.latest;
        let Some(view) = self.view.project_mut().pager.as_mut() else {
            return false;
        };
        if view.stderr_log.is_none_or(|bytes| bytes == latest.bytes) {
            return false;
        }
        let height = match view.last_viewport_h.get() {
            0 => 40,
            h => h,
        };
        let at_bottom = view.scroll >= view.scroll_max(height);
        let fresh = stderr_view(latest, self.runtime.stderr_log.as_deref());
        view.lines = fresh.lines;
        view.title = fresh.title;
        view.stderr_log = fresh.stderr_log;
        view.clamp_state_to_lines();
        if at_bottom {
            view.scroll_to_bottom_auto();
        } else {
            view.clamp_scroll_auto();
        }
        self.state.stderr.mark_seen();
        true
    }

    /// `:stderr`, or a click on the `⚠N`: the log in the pager, newest last.
    pub(super) fn open_stderr_log(&mut self) -> Vec<Effect> {
        if self.runtime.stderr_log.is_none() {
            self.state.flash_info("stderr: not captured this run");
            return Vec::new();
        }
        let mut view = stderr_view(
            &self.state.stderr.latest,
            self.runtime.stderr_log.as_deref(),
        );
        view.scroll_to_bottom_auto();
        self.set_pager(view);
        self.state.stderr.mark_seen();
        Vec::new()
    }
}

/// `:stderr`.
pub(super) fn cmd_stderr(app: &mut App, _args: &str) -> Vec<Effect> {
    app.open_stderr_log()
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::time::{Duration, Instant};

    use super::*;

    fn text(lines: &Lines) -> Vec<String> {
        lines.snapshot(true).lines.to_vec()
    }

    #[test]
    fn a_line_still_being_written_shows_and_counts_once() {
        let mut lines = Lines::default();
        lines.feed(b"first\r\nsec");
        assert_eq!(text(&lines), ["first", "sec"]);
        assert_eq!(lines.snapshot(true).total, 2);
        lines.feed(b"ond\nthi");
        assert_eq!(text(&lines), ["first", "second", "thi"]);
        assert_eq!(lines.snapshot(true).total, 3);
        assert_eq!(lines.snapshot(true).bytes, 17);
    }

    #[test]
    fn the_oldest_lines_drop_off_and_the_count_keeps_going() {
        let mut lines = Lines::default();
        for i in 0..MAX_LINES + 5 {
            lines.feed(format!("line {i}\n").as_bytes());
        }
        let reading = lines.snapshot(true);
        assert_eq!(reading.lines.len(), MAX_LINES);
        assert_eq!(reading.lines[0], "line 5");
        assert_eq!(reading.total, (MAX_LINES + 5) as u64);
    }

    #[test]
    fn a_runaway_line_is_cut_and_the_next_starts_whole() {
        let mut lines = Lines::default();
        lines.feed(&vec![b'x'; MAX_LINE_BYTES * 3]);
        lines.feed(b"\nnext\n");
        let kept = text(&lines);
        assert_eq!(kept[0].len(), MAX_LINE_BYTES);
        assert_eq!(kept[1], "next");
    }

    fn append(path: &Path, bytes: &str) {
        let mut file = File::options().append(true).open(path).unwrap();
        file.write_all(bytes.as_bytes()).unwrap();
    }

    #[test]
    fn the_follower_reads_only_what_was_appended_and_empties_a_full_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("1.log");
        std::fs::write(&path, "").unwrap();
        let mut follow = Follow::new(path.clone(), 16);
        append(&path, "one\n");
        assert_eq!(follow.refresh().lines.to_vec(), ["one"]);
        append(&path, "two\n");
        let reading = follow.refresh();
        assert_eq!(reading.lines.to_vec(), ["one", "two"]);
        assert_eq!(reading.bytes, 8);

        append(&path, "past the cap\n");
        assert_eq!(follow.refresh().total, 3);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0, "emptied");
        append(&path, "four\n");
        assert_eq!(
            follow.refresh().lines.to_vec(),
            ["one", "two", "past the cap", "four"],
            "what was read survives the emptying"
        );
    }

    /// Wake by wake, as the loop does, until `done` holds.
    fn drain_until(
        app: &mut App,
        wakes: &std::sync::mpsc::Receiver<Message>,
        done: impl Fn(&App) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(app) {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                wakes.recv_timeout(left).is_ok(),
                "no wake for the stderr log"
            );
            app.apply_stderr();
        }
    }

    /// One real frame, which also records the chrome rows a click is tested
    /// against; the status bar's cells.
    fn status_cells(app: &mut App) -> Vec<String> {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| app.render(f)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..100)
            .map(|x| buf.cell((x, 0)).map_or(" ", |c| c.symbol()).to_string())
            .collect()
    }

    fn status_row(app: &mut App) -> String {
        status_cells(app).concat()
    }

    fn pager_text(app: &App) -> Option<String> {
        app.view.project().pager.as_ref().map(|view| {
            view.lines
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    /// The whole path the feature promises, through the real watch: output
    /// raises `⚠N`, `:stderr` shows it and clears the mark, more output lands
    /// in the open pager without raising it, and output after closing raises
    /// it again.
    #[test]
    fn what_stderr_receives_raises_the_notice_and_the_pager_shows_it() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.view.term_size = (100, 24);
            let path = tmp.path().join("4242.log");
            std::fs::write(&path, "").unwrap();
            app.set_stderr_log(Some(path.clone()));
            let (tx, wakes) = std::sync::mpsc::channel();
            app.follow_stderr_log(tx);
            assert!(
                !status_row(&mut app).contains('\u{26a0}'),
                "nothing to say yet"
            );

            append(&path, "MallocStackLogging: boom\n");
            drain_until(&mut app, &wakes, |app| app.state.stderr.unread() == 1);
            assert!(
                status_row(&mut app).contains("\u{26a0}1"),
                "{}",
                status_row(&mut app)
            );

            app.dispatch_command("stderr");
            let shown = pager_text(&app).expect("the :stderr pager");
            assert!(shown.contains("MallocStackLogging: boom"), "{shown}");
            assert_eq!(app.state.stderr.unread(), 0);
            assert!(!status_row(&mut app).contains('\u{26a0}'), "seen");

            append(&path, "again\n");
            drain_until(&mut app, &wakes, |app| {
                pager_text(app).is_some_and(|text| text.contains("again"))
            });
            assert_eq!(app.state.stderr.unread(), 0, "it arrived in view");

            app.view.project_mut().pager = None;
            append(&path, "third\n");
            drain_until(&mut app, &wakes, |app| app.state.stderr.unread() == 1);
        });
    }

    /// A click on the drawn `⚠N` opens the log; one beside it, on the same
    /// row, doesn't.
    #[test]
    fn a_click_on_the_notice_opens_the_log() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let _lock = crate::mouse_test_lock();
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.view.term_size = (100, 24);
            app.set_stderr_log(Some(tmp.path().join("4242.log")));
            app.state.stderr.latest = StderrSnapshot {
                lines: vec!["boom".to_string()].into(),
                total: 1,
                bytes: 5,
                following: true,
            };
            let cells = status_cells(&mut app);
            let at = cells
                .iter()
                .position(|c| c == "\u{26a0}")
                .expect("the notice is drawn");
            let click = |app: &mut App, column: usize| {
                let column = u16::try_from(column).unwrap();
                for kind in [
                    MouseEventKind::Down(MouseButton::Left),
                    MouseEventKind::Up(MouseButton::Left),
                ] {
                    app.handle_mouse(MouseEvent {
                        kind,
                        column,
                        row: 0,
                        modifiers: KeyModifiers::NONE,
                    });
                }
            };
            crate::set_mouse_capture_for_test(true);
            click(&mut app, at + 3);
            let beside = app.view.project().pager.is_some();
            click(&mut app, at + 1);
            let on = app
                .view
                .project()
                .pager
                .as_ref()
                .is_some_and(|view| view.stderr_log.is_some());
            crate::set_mouse_capture_for_test(false);
            assert!(!beside, "a click beside the notice opened a pager");
            assert!(on, "a click on the notice didn't open the log");
            assert_eq!(app.state.stderr.unread(), 0);
        });
    }

    #[test]
    fn stderr_says_so_when_nothing_is_captured() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(tmp.path().to_path_buf());
            app.dispatch_command("stderr");
            assert!(app.view.project().pager.is_none());
            assert_eq!(
                app.state.flash.as_ref().map(|f| f.text.as_str()),
                Some("stderr: not captured this run")
            );
        });
    }
}
