//! The session's event log on disk: `<state>/events/<SESSION>.log`, one line
//! an event, appended by one writer thread (`app::events`) so the loop never
//! waits on the disk. Told the session's name, the writer reads that file's
//! lines back for the Model and appends after them; a session renamed, or
//! restored under another name, carries what it wrote to the new name's file.
//! Past [`CAP`] bytes a file keeps its newer half.
//!
//! `spyc events [NAME] [-n N] [-f]` prints a session's log from a shell
//! ([`print_cli`]), and with `-f` follows it, across a trim's rewrite too.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

/// What the writer is told.
#[derive(Debug)]
pub enum Write {
    /// The session is called this now.
    Session(String),
    /// Append these lines to the session's file.
    Append(Vec<String>),
    /// Answer once everything told before this is written.
    Flush(std::sync::mpsc::Sender<()>),
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
            Write::Flush(done) => {
                let _ = done.send(());
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

/// How often `spyc events -f` looks for more.
const FOLLOW_POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// `spyc events [NAME] [-n N] [-f]`: the session's log, each line as
/// `:events` shows it, the last `last` of them when given; with `follow`,
/// then what comes after, until interrupted.
pub fn print_cli(name: Option<&str>, last: Option<usize>, follow: bool) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let dir = super::state_root()
        .context("no state directory: spyc keeps no event logs")?
        .join("events");
    let name = if let Some(name) = name {
        name.to_string()
    } else {
        let running: Vec<String> = crate::pane::jar::attach::running_here()
            .into_iter()
            .map(|r| r.name)
            .collect();
        choose(
            &dir,
            &running,
            &mut std::io::stdin().lock(),
            &mut std::io::stderr(),
        )?
    };
    let path = file_for(&dir, &name).with_context(|| format!("{name:?} isn't a session's name"))?;
    anyhow::ensure!(path.is_file(), "{name} has no event log");
    let colour = std::io::IsTerminal::is_terminal(&std::io::stdout());
    let mut out = std::io::stdout().lock();
    let lines = read_lines(&path);
    let skip = last.map_or(0, |n| lines.len().saturating_sub(n));
    for line in &lines[skip..] {
        writeln!(out, "{}", shown(line, colour))?;
    }
    out.flush()?;
    if !follow {
        return Ok(());
    }
    let mut seen = lines.last().cloned();
    let mut inode = inode_of(&path);
    let mut offset = std::fs::metadata(&path).map_or(0, |m| m.len());
    loop {
        std::thread::sleep(FOLLOW_POLL);
        let now = inode_of(&path);
        let fresh = if now == inode {
            appended(&path, &mut offset)
        } else {
            // A trim rewrote the file: what follows the last line printed.
            inode = now;
            let all = read_lines(&path);
            offset = std::fs::metadata(&path).map_or(0, |m| m.len());
            let from = seen
                .as_ref()
                .and_then(|s| all.iter().rposition(|l| l == s))
                .map_or(0, |at| at + 1);
            all[from..].to_vec()
        };
        for line in &fresh {
            writeln!(out, "{}", shown(line, colour))?;
        }
        if let Some(line) = fresh.last() {
            seen = Some(line.clone());
            out.flush()?;
        }
    }
}

fn inode_of(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.ino())
}

/// The whole lines appended to `path` past `offset`, which moves past them.
fn appended(path: &Path, offset: &mut u64) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(*offset)).is_err() || file.read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    let whole = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |n| n + 1);
    *offset += whole as u64;
    String::from_utf8_lossy(&bytes[..whole])
        .lines()
        .map(String::from)
        .collect()
}

/// The columns an event's place is padded to, so what happened lines up.
const PLACE_WIDTH: usize = 20;

/// A line of the file as `:events` shows it: its time from the month on,
/// where it happened, what happened, an error in red when `colour`, and the
/// ids that trace it, dim when `colour`.
pub fn shown(line: &str, colour: bool) -> String {
    let mut parts = line.splitn(3, "  ");
    let (Some(at), Some(kind), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
        return line.to_string();
    };
    let when = at.get(5..).unwrap_or(at);
    let mut fields = rest.splitn(3, '\t');
    let text = fields.next().unwrap_or_default();
    let place = fields.next().unwrap_or_default();
    let ids = fields.next().unwrap_or_default();
    let mut out = format!("{when}  ");
    if !place.is_empty() {
        let pad = PLACE_WIDTH.saturating_sub(crate::ui::display_width(place));
        out = format!("{out}{place}{}  ", " ".repeat(pad));
    }
    out = match (kind, colour) {
        ("E", true) => format!("{out}\x1b[31m{text}\x1b[0m"),
        _ => format!("{out}{text}"),
    };
    match (ids.is_empty(), colour) {
        (true, _) => out,
        (false, true) => format!("{out}  \x1b[2m{ids}\x1b[0m"),
        (false, false) => format!("{out}  {ids}"),
    }
}

/// Which session's log, when none was named: the one session `running`,
/// else a choice of every session with a log, newest first.
fn choose(
    dir: &Path,
    running: &[String],
    input: &mut impl std::io::BufRead,
    output: &mut impl std::io::Write,
) -> anyhow::Result<String> {
    if let [one] = running {
        return Ok(one.clone());
    }
    let logs = logs_newest_first(dir);
    match logs.as_slice() {
        [] => anyhow::bail!("no session has an event log yet"),
        [one] => return Ok(one.clone()),
        _ => {}
    }
    for (i, name) in logs.iter().enumerate() {
        let here = if running.contains(name) {
            "  (running)"
        } else {
            ""
        };
        writeln!(output, "{:>3}  {name}{here}", i + 1)?;
    }
    write!(output, "Show which? ")?;
    output.flush()?;
    let mut answer = String::new();
    input.read_line(&mut answer)?;
    let answer = answer.trim();
    answer
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| logs.get(i))
        .or_else(|| logs.iter().find(|n| *n == answer))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{answer:?} is none of them"))
}

/// The sessions with a log in `dir`, the one written last first.
fn logs_newest_first(dir: &Path) -> Vec<String> {
    let mut logs: Vec<(std::time::SystemTime, String)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path
                .file_name()?
                .to_str()?
                .strip_suffix(".log")?
                .to_string();
            let written = entry.metadata().ok()?.modified().ok()?;
            Some((written, name))
        })
        .collect();
    logs.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    logs.into_iter().map(|(_, name)| name).collect()
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

    /// A flush is answered once what was told before it is on disk, so a
    /// handover's line is in the file before the next spyc writes to it.
    #[test]
    fn a_flush_is_answered_once_what_came_before_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let events = dir.path().join("events");
        let (tx, thread) = spawn(events.clone(), |_| {});
        tx.send(Write::Session("SAGE".into())).unwrap();
        tx.send(Write::Append(vec!["detached".into()])).unwrap();
        let (done, written) = channel();
        tx.send(Write::Flush(done)).unwrap();
        written.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(read_lines(&events.join("SAGE.log")), ["detached"]);
        drop(tx);
        thread.unwrap().join().unwrap();
    }

    #[test]
    fn a_line_shows_as_events_shows_it() {
        let error = "2026-11-20 06:30:31  E  jar upgrade stopped";
        assert_eq!(shown(error, false), "11-20 06:30:31  jar upgrade stopped");
        assert_eq!(
            shown(error, true),
            "11-20 06:30:31  \x1b[31mjar upgrade stopped\x1b[0m"
        );
        assert_eq!(
            shown("2026-11-20 06:30:31  I  two  spaces", true),
            "11-20 06:30:31  two  spaces"
        );
        assert_eq!(shown("not a line of ours", true), "not a line of ours");
    }

    /// A tab's event shows where it happened, in a column, and the ids that
    /// trace it after what happened.
    #[test]
    fn a_line_shows_its_place_and_ids() {
        let line =
            "2026-11-25 05:54:18  I  claude needs you: Bash\tweb tab 2\tpane=t-9 project=p-1";
        assert_eq!(
            shown(line, false),
            "11-25 05:54:18  web tab 2             claude needs you: Bash  pane=t-9 project=p-1"
        );
        assert_eq!(
            shown(line, true),
            "11-25 05:54:18  web tab 2             claude needs you: Bash  \x1b[2mpane=t-9 project=p-1\x1b[0m"
        );
        assert_eq!(
            shown("2026-11-25 05:54:18  I  saved\tweb\t", false),
            "11-25 05:54:18  web                   saved"
        );
    }

    #[test]
    fn following_reads_only_whole_lines_appended() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("S.log");
        std::fs::write(&path, "one\n").unwrap();
        let mut offset = 4;
        std::fs::write(&path, "one\ntwo\nthr").unwrap();
        assert_eq!(appended(&path, &mut offset), ["two"]);
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
        assert_eq!(appended(&path, &mut offset), ["three"]);
        assert_eq!(appended(&path, &mut offset), Vec::<String>::new());
    }

    #[test]
    fn a_choice_lists_the_logs_newest_first_and_takes_a_number_or_a_name() {
        let dir = tempfile::tempdir().unwrap();
        for (name, age) in [("OLD", 100), ("NEW", 0)] {
            let path = dir.path().join(format!("{name}.log"));
            std::fs::write(&path, "x\n").unwrap();
            let at = std::time::SystemTime::now() - std::time::Duration::from_secs(age);
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(at)
                .unwrap();
        }
        assert_eq!(logs_newest_first(dir.path()), ["NEW", "OLD"]);
        let mut shown = Vec::new();
        let picked = choose(dir.path(), &[], &mut &b"2\n"[..], &mut shown).unwrap();
        assert_eq!(picked, "OLD");
        assert!(String::from_utf8(shown).unwrap().contains("  1  NEW"));
        assert_eq!(
            choose(dir.path(), &[], &mut &b"NEW\n"[..], &mut Vec::new()).unwrap(),
            "NEW"
        );
        assert!(choose(dir.path(), &[], &mut &b"9\n"[..], &mut Vec::new()).is_err());
        assert_eq!(
            choose(dir.path(), &["OLD".into()], &mut &b""[..], &mut Vec::new()).unwrap(),
            "OLD",
            "the one session running, without asking"
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
