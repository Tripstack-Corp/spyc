//! Following a live transcript: the reader reads only what was appended, only
//! from the bound session's file, and the watch wakes it only when that file
//! changes.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::json;

use super::stream::Reader;
use super::watch::Subscription;
use super::{Availability, Binding};

const SESSION: &str = "c1a0de00-0000-4000-8000-000000000001";
const OTHER: &str = "c1a0de00-0000-4000-8000-000000000002";

/// A Claude home with one empty transcript for [`SESSION`].
fn claude_home() -> (tempfile::TempDir, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let project = home.path().join("projects/-tmp-project");
    std::fs::create_dir_all(&project).unwrap();
    let transcript = project.join(format!("{SESSION}.jsonl"));
    std::fs::write(&transcript, "").unwrap();
    (home, transcript)
}

fn binding(home: &Path) -> Binding {
    Binding {
        pane_id: "pane-a".to_owned(),
        session_id: SESSION.to_owned(),
        claude_home: home.to_owned(),
        generation: 1,
    }
}

fn message(id: &str, second: u32, output: u64) -> String {
    let record = json!({"type": "assistant", "sessionId": SESSION,
        "timestamp": format!("2026-10-13T12:00:{second:02}Z"),
        "message": {"id": id, "model": "claude-opus-5-5", "content": [],
                    "usage": {"input_tokens": 1, "cache_creation_input_tokens": 10,
                              "cache_read_input_tokens": 100, "output_tokens": output}}});
    format!("{record}\n")
}

fn append(path: &Path, text: &str) {
    std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

/// The second refresh reads the appended record, plus a bounded anchor check,
/// not the transcript again.
#[test]
fn a_transcript_is_followed_by_what_it_appends() {
    let (home, path) = claude_home();
    append(&path, &message("msg-1", 1, 5));
    let mut reader = Reader::new(binding(home.path()), path.clone());
    let first = reader.refresh();
    assert_eq!(first.availability, Availability::Available);
    assert_eq!(first.snapshot.messages, 1);
    assert!(!first.tail_only);

    let before = reader.bytes_read;
    let next = message("msg-2", 2, 7);
    append(&path, &next);
    let second = reader.refresh();
    assert_eq!(second.snapshot.messages, 2);
    assert_eq!(second.snapshot.totals.output, 12);
    let read = reader.bytes_read - before;
    assert!(read < next.len() as u64 + 256, "read {read} bytes");

    let before = reader.bytes_read;
    assert_eq!(reader.refresh(), second, "nothing new, nothing changes");
    assert!(reader.bytes_read - before <= 128);
}

#[test]
fn an_unfinished_line_waits_for_its_newline() {
    let (home, path) = claude_home();
    let line = message("msg-1", 1, 5);
    let (head, tail) = line.split_at(line.len() / 2);
    append(&path, head);
    let mut reader = Reader::new(binding(home.path()), path.clone());
    assert_eq!(reader.refresh().snapshot.messages, 0);
    append(&path, tail);
    assert_eq!(reader.refresh().snapshot.messages, 1);
}

/// A rewritten transcript is a different one: what was folded from the old
/// contents is dropped, never added to.
#[test]
fn a_replaced_or_truncated_transcript_starts_over() {
    let (home, path) = claude_home();
    append(&path, &message("msg-1", 1, 5));
    append(&path, &message("msg-2", 2, 5));
    let mut reader = Reader::new(binding(home.path()), path.clone());
    assert_eq!(reader.refresh().snapshot.messages, 2);

    std::fs::write(&path, message("msg-3", 3, 40)).unwrap();
    let truncated = reader.refresh().snapshot;
    assert_eq!((truncated.messages, truncated.totals.output), (1, 40));

    let replacement = path.with_extension("tmp");
    std::fs::write(&replacement, message("msg-4", 4, 9)).unwrap();
    std::fs::rename(&replacement, &path).unwrap();
    let replaced = reader.refresh().snapshot;
    assert_eq!((replaced.messages, replaced.totals.output), (1, 9));

    std::fs::remove_file(&path).unwrap();
    let missing = reader.refresh();
    assert_eq!(missing.availability, Availability::Missing);
    assert_eq!(missing.snapshot.messages, 0);
}

/// Only `<claude_home>/projects/…/<session>.jsonl` is read: the file's name is
/// the session it holds, since a transcript has no header to check.
#[test]
fn only_the_bound_sessions_file_under_its_claude_home_is_read() {
    let (home, path) = claude_home();
    let refresh = |binding: Binding, path: &Path| {
        Reader::new(binding, path.to_owned()).refresh().availability
    };

    let other = path.with_file_name(format!("{OTHER}.jsonl"));
    std::fs::write(&other, message("msg-1", 1, 5)).unwrap();
    assert_eq!(
        refresh(binding(home.path()), &other),
        Availability::IdentityMismatch,
        "another session's file, though its records might name ours"
    );

    let outside = home.path().join(format!("elsewhere/{SESSION}.jsonl"));
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, message("msg-1", 1, 5)).unwrap();
    assert_eq!(
        refresh(binding(home.path()), &outside),
        Availability::IdentityMismatch
    );

    let mut relative = binding(home.path());
    relative.claude_home = PathBuf::from(".claude");
    assert_eq!(refresh(relative, &path), Availability::IdentityMismatch);
    assert_eq!(
        refresh(binding(home.path()), Path::new("projects/x.jsonl")),
        Availability::IdentityMismatch
    );

    let (other_home, _) = claude_home();
    assert_eq!(
        refresh(binding(other_home.path()), &path),
        Availability::IdentityMismatch,
        "another Claude home's transcript"
    );
}

/// A link to another session's transcript names that one once resolved, and
/// neither a directory nor a FIFO is read, nor waited on for a writer.
#[test]
fn links_directories_and_fifos_are_refused() {
    let (home, path) = claude_home();
    let project = path.parent().unwrap().to_owned();
    std::fs::remove_file(&path).unwrap();
    let other = project.join(format!("{OTHER}.jsonl"));
    std::fs::write(&other, message("msg-1", 1, 5)).unwrap();
    std::os::unix::fs::symlink(&other, &path).unwrap();
    assert_eq!(
        Reader::new(binding(home.path()), path.clone())
            .refresh()
            .availability,
        Availability::IdentityMismatch
    );

    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(
        Reader::new(binding(home.path()), path.clone())
            .refresh()
            .availability,
        Availability::Unreadable
    );

    std::fs::remove_dir(&path).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        Reader::new(binding(home.path()), path)
            .refresh()
            .availability,
        Availability::Unreadable
    );
}

/// The watch wakes the consumer when the transcript grows, does no work and
/// sends no wake at idle, and its worker ends when the subscription drops.
#[test]
fn the_watch_wakes_on_append_and_is_silent_at_idle() {
    let (home, path) = claude_home();
    append(&path, &message("msg-1", 1, 5));
    let (tx, rx) = mpsc::channel();
    let subscription = Subscription::start(binding(home.path()), path.clone(), move || {
        let _ = tx.send(());
    });
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let first = subscription.take().unwrap();
    assert_eq!(first.snapshot.messages, 1);
    assert_eq!(
        first.for_binding(Some(&binding(home.path()))),
        Some(&first.snapshot)
    );
    let mut rebound = binding(home.path());
    rebound.generation = 2;
    assert_eq!(first.for_binding(Some(&rebound)), None);
    assert_eq!(first.for_binding(None), None);

    append(&path, &message("msg-2", 2, 5));
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(subscription.take().unwrap().snapshot.messages, 2);

    // Idle starts once the append's own events are spent: a backend may report
    // one write more than once, and that is not work at idle.
    let mut before = subscription.work();
    let settle = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let now = subscription.work();
        if now == before {
            break;
        }
        assert!(
            std::time::Instant::now() < settle,
            "the watch never went idle"
        );
        before = now;
    }
    assert_eq!(
        rx.recv_timeout(Duration::from_millis(200)),
        Err(mpsc::RecvTimeoutError::Timeout)
    );
    assert_eq!(subscription.work(), before);
    drop(subscription);
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
}
