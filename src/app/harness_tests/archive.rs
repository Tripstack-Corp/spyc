//! Harness tests for archive mounts: `Enter` on a container, browsing what it
//! holds, and climbing back out.
//!
//! The mount itself is driven by running the worker inline and applying its
//! outcome, which is exactly what the event loop does — just without the thread,
//! so the assertions are deterministic.

use super::*;
use crate::app::archive_ops::{ArchiveOp, ArchiveOutcome, run_archive_op};
use crate::keymap::Action;

use std::io::Write as _;
use std::path::{Path, PathBuf};

fn build_zip(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut w = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
    for (name, body) in [
        ("README.md", "# pkg\n"),
        ("src/main.rs", "fn main() {}\n"),
        ("src/deep/mod.rs", "pub fn helper() {}\n"),
    ] {
        w.start_file(name, opts).unwrap();
        w.write_all(body.as_bytes()).unwrap();
    }
    w.finish().unwrap();
}

/// Run the mount the way the loop would — worker first, then the drain that
/// applies the outcome — and hand back the effects the drain produced.
fn mount_inline(app: &mut App, archive: &Path, staging: &Path) -> Vec<Effect> {
    let outcome = run_archive_op(ArchiveOp::Mount {
        path: archive.to_path_buf(),
        staging_root: staging.to_path_buf(),
        limits: app.state.config.archive.limits(),
        max_entries: 1000,
        confirmed: false,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        then: None,
        address: None,
        depth: 0,
        reset_staging: false,
    });
    assert!(
        matches!(outcome, ArchiveOutcome::Mounted { .. }),
        "fixture should mount: {outcome:?}"
    );
    app.runtime.archive_results.lock().unwrap().push(outcome);
    let (_, fx) = app.apply_archive_outcomes();
    fx
}

/// Puts the process cwd back on drop.
///
/// `change_dir` moves the *process* cwd, and a test that points it at a tempdir
/// leaves it dangling the moment that tempdir is cleaned up — after which every
/// later test in this binary that needs a working directory fails with
/// "Could not obtain the current working directory". Restoring to the crate root
/// is unconditional because it is the one path guaranteed to still exist.
struct CwdGuard;

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(env!("CARGO_MANIFEST_DIR"));
    }
}

/// Apply the effects a handler returned, for the two kinds these tests produce.
/// The real executor spawns threads; running them inline keeps the assertions
/// deterministic.
fn apply_effects(app: &mut App, effects: Vec<Effect>) {
    for effect in effects {
        match effect {
            Effect::ChangeDir {
                path,
                focus,
                on_ok,
                err_prefix,
            } => app
                .state
                .change_dir(&path, focus.as_deref(), on_ok.as_deref(), err_prefix),
            Effect::Archive(op) => {
                let outcome = run_archive_op(op);
                app.runtime.archive_results.lock().unwrap().push(outcome);
                app.apply_archive_outcomes();
            }
            _ => {}
        }
    }
}

fn row_names(app: &App) -> Vec<String> {
    app.state
        .cur()
        .rows
        .iter()
        .map(|r| r.display.clone())
        .collect()
}

/// Run whatever the screen produced, then whatever the drain produced, until the
/// effects settle — the loop's behaviour, minus the threads.
/// The hop limit is a runaway guard, not a budget — reaching it means the chain
/// grew past what this driver models, and every assertion after the call would
/// then run against half-applied state and fail somewhere unrelated. Say so here
/// instead.
const SETTLE_HOPS: usize = 4;

fn settle(app: &mut App, effects: Vec<Effect>) {
    let mut queue = effects;
    for _ in 0..SETTLE_HOPS {
        if queue.is_empty() {
            return;
        }
        let mut next = Vec::new();
        for effect in queue {
            let Some(effect) = app.screen_archive_effect(effect) else {
                continue;
            };
            match effect {
                Effect::Archive(op) => {
                    let outcome = run_archive_op(op);
                    app.runtime.archive_results.lock().unwrap().push(outcome);
                }
                Effect::Inventory(op) => {
                    let outcome = crate::app::inventory_ops::run_inventory_op(op);
                    app.runtime.inventory_results.lock().unwrap().push(outcome);
                }
                Effect::FileOp(op) => {
                    let outcome = crate::app::file_ops::run_file_op(op);
                    app.runtime.file_results.lock().unwrap().push(outcome);
                }
                Effect::Graveyard(op) => {
                    let outcome = crate::app::graveyard_ops::run_graveyard_op(op);
                    app.runtime.graveyard_results.lock().unwrap().push(outcome);
                }
                // A mount re-issues the `ChangeDir` that was waiting for it.
                Effect::ChangeDir {
                    path,
                    focus,
                    on_ok,
                    err_prefix,
                } => app
                    .state
                    .change_dir(&path, focus.as_deref(), on_ok.as_deref(), err_prefix),
                _ => {}
            }
        }
        let (_, fx) = app.apply_archive_outcomes();
        next.extend(fx);
        app.apply_graveyard_outcomes();
        app.apply_inventory_outcomes();
        let (_, file_fx) = app.apply_file_outcomes();
        next.extend(file_fx);
        queue = next;
    }
    assert!(
        queue.is_empty(),
        "effects still pending after {SETTLE_HOPS} hops: {queue:?}. The chain got \
         longer than this driver models — raise SETTLE_HOPS deliberately rather \
         than letting the caller assert against half-applied state."
    );
}

/// Read an archive's member names back off disk.
fn member_names(archive: &Path) -> Vec<String> {
    let indexed =
        crate::archive::read::index_seekable(archive, crate::archive::ArchiveFormat::Zip, 1000)
            .unwrap();
    let mut names: Vec<String> = indexed
        .index
        .entries
        .iter()
        .filter(|e| e.locator != crate::archive::Locator::Implied)
        .map(|e| e.inner.clone())
        .collect();
    names.sort();
    names
}

/// Same three members, as a `.tar.gz` — a *streamed* mount, which extracts every
/// member as it reads because a compressed tar can't be listed any other way.
fn build_tar_gz(path: &Path) {
    let enc = flate2::write::GzEncoder::new(
        std::fs::File::create(path).unwrap(),
        flate2::Compression::default(),
    );
    let mut b = tar::Builder::new(enc);
    for (name, body) in [
        ("README.md", "# pkg\n"),
        ("src/main.rs", "fn main() {}\n"),
        ("src/deep/mod.rs", "pub fn helper() {}\n"),
    ] {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(1_000_000);
        h.set_entry_type(tar::EntryType::Regular);
        b.append_data(&mut h, name, body.as_bytes()).unwrap();
    }
    b.into_inner().unwrap().finish().unwrap();
}

/// A `.tar.gz` with `count` small members, for the staged-set bound.
fn build_tar_gz_with(path: &Path, count: usize) {
    let enc = flate2::write::GzEncoder::new(
        std::fs::File::create(path).unwrap(),
        flate2::Compression::default(),
    );
    let mut b = tar::Builder::new(enc);
    for i in 0..count {
        let body = format!("member {i}\n");
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(1_000_000);
        h.set_entry_type(tar::EntryType::Regular);
        b.append_data(&mut h, format!("file-{i}.txt"), body.as_bytes())
            .unwrap();
    }
    b.into_inner().unwrap().finish().unwrap();
}

mod mounting;
mod navigation;
mod pending;
mod reading;
mod writing;
