//! The session's event log on disk: `<state>/events/<SESSION>.log`, one line
//! an event, appended by one writer thread (`app::events`) so the loop never
//! waits on the disk. Told the session's name, the writer reads that file's
//! lines back for the Model and appends after them; a session renamed, or
//! restored under another name, carries what it wrote to the new name's file.
//! Past [`CAP`] bytes a file keeps its newer half.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

/// What the writer is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// The session is called this now.
    Session(String),
    /// Append these lines to the session's file.
    Append(Vec<String>),
}

/// The most a session's file holds before it keeps its newer half.
pub const CAP: u64 = 1 << 20;

/// Start the writer for files in `dir`; `earlier` hears each session's lines
/// from before, once it's named. The thread ends once every sender has gone
/// and it has written what they sent; `None` when it couldn't start.
pub fn spawn(
    dir: PathBuf,
    earlier: impl Fn(Vec<String>) + Send + 'static,
) -> (Sender<Write>, Option<std::thread::JoinHandle<()>>) {
    let (tx, rx) = channel();
    let thread = std::thread::Builder::new()
        .name("event-log".into())
        .spawn(move || run(&dir, &rx, &earlier))
        .ok();
    (tx, thread)
}

fn run(dir: &Path, rx: &Receiver<Write>, earlier: &dyn Fn(Vec<String>)) {
    let mut current: Option<PathBuf> = None;
    for told in rx {
        match told {
            Write::Session(name) => {
                let Some(to) = file_for(dir, &name) else {
                    current = None;
                    continue;
                };
                if current.as_ref() == Some(&to) {
                    continue;
                }
                let before = read_lines(&to);
                if let Some(from) = current.take() {
                    let _ = carry(dir, &from, &to);
                }
                current = Some(to);
                earlier(before);
            }
            Write::Append(lines) => {
                if let Some(path) = &current {
                    let _ = append(dir, path, &lines);
                }
            }
        }
    }
}

/// The file for session `name`; `None` for a name that isn't one.
pub fn file_for(dir: &Path, name: &str) -> Option<PathBuf> {
    let ok = !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    ok.then(|| dir.join(format!("{name}.log")))
}

/// The lines of `path`, none when it can't be read.
pub fn read_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path).map_or_else(
        |_| Vec::new(),
        |text| text.lines().map(String::from).collect(),
    )
}

/// Append `lines` to `path`, made owner-only, then keep its newer half
/// once it has grown past [`CAP`].
pub fn append(dir: &Path, path: &Path, lines: &[String]) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let mut text = lines.join("\n");
    text.push('\n');
    file.write_all(text.as_bytes())?;
    if file.metadata()?.len() > CAP {
        keep_newer_half(path)?;
    }
    Ok(())
}

/// Rewrite `path` as its last half, from a line's start.
fn keep_newer_half(path: &Path) -> std::io::Result<()> {
    let text = std::fs::read(path)?;
    let half = text.len() - text.len() / 2;
    let start = text[half..]
        .iter()
        .position(|b| *b == b'\n')
        .map_or(text.len(), |n| half + n + 1);
    crate::fs::write_atomic(path, &text[start..])
}

/// Move `from`'s lines to the end of `to`.
fn carry(dir: &Path, from: &Path, to: &Path) -> std::io::Result<()> {
    let lines = read_lines(from);
    if !lines.is_empty() {
        append(dir, to, &lines)?;
    }
    std::fs::remove_file(from)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_session_reads_its_lines_back_then_appends_after_them() {
        let dir = tempfile::tempdir().unwrap();
        let events = dir.path().join("events");
        append(&events, &events.join("SAGE.log"), &["old".into()]).unwrap();
        let (heard_tx, heard) = channel();
        let (tx, thread) = spawn(events.clone(), move |lines| {
            let _ = heard_tx.send(lines);
        });
        tx.send(Write::Session("SAGE".into())).unwrap();
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(10)).unwrap(),
            ["old"]
        );
        tx.send(Write::Append(vec!["new".into()])).unwrap();

        // Renamed: what it wrote goes to the new name's file, after that
        // file's own lines, which it reads back.
        append(&events, &events.join("CLOVE.log"), &["clove's".into()]).unwrap();
        tx.send(Write::Session("CLOVE".into())).unwrap();
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(10)).unwrap(),
            ["clove's"]
        );
        drop(tx);
        thread.unwrap().join().unwrap();
        assert!(!events.join("SAGE.log").exists());
        assert_eq!(
            read_lines(&events.join("CLOVE.log")),
            ["clove's", "old", "new"]
        );
    }

    #[test]
    fn a_name_that_isnt_one_has_no_file() {
        let dir = Path::new("/state/events");
        assert_eq!(
            file_for(dir, "CORIANDER_CLOVE"),
            Some(dir.join("CORIANDER_CLOVE.log"))
        );
        for bad in ["", "../x", "a/b", ".hidden", "sp ace"] {
            assert_eq!(file_for(dir, bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_file_past_the_cap_keeps_its_newer_half_from_a_line_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("S.log");
        let line = "x".repeat(99);
        let lines: Vec<String> = (0..(CAP / 100 + 10))
            .map(|i| format!("{i:09}{line}"))
            .collect();
        append(dir.path(), &path, &lines).unwrap();
        let len = std::fs::metadata(&path).unwrap().len();
        assert!(len <= CAP, "{len}");
        let kept = read_lines(&path);
        assert_eq!(kept.last(), lines.last(), "the newest stay");
        assert!(kept.iter().all(|l| l.len() == 108), "every line whole");
    }
}
