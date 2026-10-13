use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::agent::codex_metrics::tests::{append, binding, header, rollout, usage};

fn start(home: &Path, path: &Path) -> (Subscription, mpsc::Receiver<()>) {
    let (tx, rx) = mpsc::channel();
    let subscription = Subscription::start(binding(home.to_owned()), path.to_owned(), move || {
        let _ = tx.send(());
    });
    (subscription, rx)
}

#[test]
fn idle_collection_does_no_reads_or_wakes_and_drop_cancels_the_worker() {
    let (home, path) = rollout();
    append(&path, &usage(10, 1));
    let (subscription, rx) = start(home.path(), &path);
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        subscription
            .take()
            .unwrap()
            .snapshot
            .request
            .unwrap()
            .value
            .total,
        Some(10)
    );
    let before = subscription.work();
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

#[test]
fn real_append_and_replacement_watches_deliver_only_the_selected_rollout() {
    let (home, path) = rollout();
    let (subscription, rx) = start(home.path(), &path);
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    subscription.take();
    append(&path, &usage(30, 1));
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        subscription
            .take()
            .unwrap()
            .snapshot
            .request
            .unwrap()
            .value
            .total,
        Some(30)
    );
    let other = path.with_extension("sibling");
    std::fs::write(&other, usage(900, 2).to_string()).unwrap();
    assert_eq!(
        rx.recv_timeout(Duration::from_millis(200)),
        Err(mpsc::RecvTimeoutError::Timeout)
    );
    let replacement = path.with_extension("replacement");
    std::fs::write(&replacement, header() + &(usage(40, 3).to_string() + "\n")).unwrap();
    std::fs::rename(replacement, &path).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        subscription
            .take()
            .unwrap()
            .snapshot
            .request
            .unwrap()
            .value
            .total,
        Some(40)
    );
}

#[test]
fn unread_bursts_replace_the_single_mailbox_slot_without_repeated_wakes() {
    let latest = Mailbox::default();
    let count = AtomicU64::new(0);
    let (home, path) = rollout();
    let mut reader = Reader::new(binding(home.path().to_owned()), path.clone());
    for second in 1..50 {
        append(&path, &usage(u64::from(second), second));
        publish(&latest, reader.refresh(), &|| {
            count.fetch_add(1, Ordering::Relaxed);
        });
    }
    assert_eq!(count.load(Ordering::Relaxed), 1);
    let snapshot = latest.latest.lock().unwrap().take().unwrap().snapshot;
    assert_eq!(snapshot.request.unwrap().value.total, Some(49));
}

#[test]
fn read_access_and_other_files_do_not_trigger_collection() {
    use notify::event::{AccessKind, AccessMode, ModifyKind};
    let rollout = Path::new("/home/sessions/day/rollout.jsonl");
    // A write's close follows its own `Modify`: counting it read each append twice.
    for kind in [
        AccessKind::Open(AccessMode::Read),
        AccessKind::Read,
        AccessKind::Close(AccessMode::Read),
        AccessKind::Close(AccessMode::Write),
        AccessKind::Open(AccessMode::Write),
    ] {
        assert!(!relevant(
            EventKind::Access(kind),
            &[rollout.to_owned()],
            rollout
        ));
    }
    assert!(!relevant(
        EventKind::Modify(ModifyKind::Any),
        &[rollout.with_file_name("other.jsonl")],
        rollout
    ));
    assert!(relevant(
        EventKind::Modify(ModifyKind::Any),
        &[rollout.to_owned()],
        rollout
    ));
}

#[test]
fn aliased_parent_paths_receive_canonical_backend_events() {
    let (home, path) = rollout();
    let alias = home.path().join("alias");
    std::os::unix::fs::symlink(path.parent().unwrap(), &alias).unwrap();
    let aliased = alias.join(path.file_name().unwrap());
    let (subscription, rx) = start(home.path(), &aliased);
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    subscription.take();
    append(&path, &usage(55, 1));
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        subscription
            .take()
            .unwrap()
            .snapshot
            .request
            .unwrap()
            .value
            .total,
        Some(55)
    );
}

#[test]
fn two_live_panes_and_custom_homes_keep_their_own_snapshots() {
    let (home_a, path_a) = rollout();
    let (home_b, path_b) = rollout();
    append(&path_a, &usage(10, 1));
    append(&path_b, &usage(50, 1));
    let (a, rx_a) = start(home_a.path(), &path_a);
    let (b, rx_b) = start(home_b.path(), &path_b);
    rx_a.recv_timeout(Duration::from_secs(5)).unwrap();
    rx_b.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        a.take().unwrap().snapshot.request.unwrap().value.total,
        Some(10)
    );
    assert_eq!(
        b.take().unwrap().snapshot.request.unwrap().value.total,
        Some(50)
    );
    append(&path_a, &usage(20, 2));
    rx_a.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        a.take().unwrap().snapshot.request.unwrap().value.total,
        Some(20)
    );
    assert_eq!(
        rx_b.recv_timeout(Duration::from_millis(200)),
        Err(mpsc::RecvTimeoutError::Timeout)
    );
}
