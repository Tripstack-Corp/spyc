use std::fmt::Write;
use std::path::{Path, PathBuf};

use super::state::{PagerRequest, Side};
use super::{App, Effect, Message, PaneInput, PaneTarget, pager_handler};
use crate::fs::listing::Listing;
use crate::state::inventory::Inventory;
use crate::ui::pager::PagerView;

/// Where an opened file's pager view should be installed — carried from the
/// open site, through the (possibly off-thread) build, to the install. `Copy`
/// so it rides a [`FileOp::OpenSpecialFile`] out to the worker and back without
/// ceremony.
#[derive(Debug, Clone, Copy)]
pub enum PagerDest {
    /// Centered overlay (Enter / `gF`). `scroll`, when set, overrides the
    /// landing line (`gF`'s referenced line, 0-indexed); `None` keeps the
    /// builder's default position.
    Overlay { scroll: Option<usize> },
    /// Top-pane mount (`D`): the bottom pane stays visible. Installed with
    /// `no_history` (a fresh open, not a navigation the user might revisit).
    TopPane,
}

#[derive(Debug)]
pub enum FileOp {
    Copy {
        paths: Vec<PathBuf>,
        dest: PathBuf,
    },
    Move {
        paths: Vec<PathBuf>,
        dest: PathBuf,
    },
    /// Per-file copy/move: each `(src, dest)` pair is handled independently.
    /// Produced when a copy/move destination references the source name via
    /// `%` (expanded per source to that file's basename), so a multi-pick
    /// `%.bak` batch-renames each selected file to its own target. A single
    /// pair stays on the plain `Copy`/`Move` op above (keeps the "to <dest>"
    /// flash); this variant carries the fan-out.
    RenameEach {
        pairs: Vec<(PathBuf, PathBuf)>,
        is_move: bool,
    },
    PipeContent {
        use_inventory: bool,
        inventory_ids: Vec<String>,
        paths: Vec<PathBuf>,
    },
    /// Restore a git-deleted file back into the worktree from the index/HEAD
    /// (in-process gix). `rela_path` is repo-relative; the op writes the blob
    /// and refuses to clobber an existing file.
    GitRestore {
        repo_root: PathBuf,
        rela_path: String,
    },
    /// Build the `L` long-listing table (one `symlink_metadata` + owner/group
    /// resolution per path) off the input thread — the pure `apply` dispatcher
    /// just hands over the selected paths. `title` is precomputed (it only
    /// needs the listing dir, which `apply` has).
    LongList {
        paths: Vec<PathBuf>,
        title: String,
    },
    /// Classify the file type of each path (`symlink_metadata` + a 512-byte
    /// magic read per path) off the input thread.
    FileType {
        paths: Vec<PathBuf>,
    },
    /// Read + render a **non-regular** file (char/block device, etc.) into a
    /// `PagerView` on the worker. The read this performs (`looks_like_text` +
    /// `read_truncated`/`read_hex_window`) is byte-capped, but on an exotic
    /// blocking-read device (`/dev/input/*`) it can block forever — so it must
    /// not run on the input thread. A *readable* device (`/dev/zero`,
    /// `/dev/urandom`) samples and lands a pager; a truly-blocking one parks
    /// this worker, never the UI. Regular files never reach here (the open
    /// sites build them inline — no thread/flicker for the common case). The
    /// builder inputs are captured on the main thread (the tty-size query
    /// behind `wrap`, the theme); `dest` rides back out to the install.
    OpenSpecialFile {
        path: PathBuf,
        theme: crate::ui::theme::Theme,
        open_as_rendered: bool,
        wrap: Option<u16>,
        dest: PagerDest,
    },
    /// Read a directory listing off the input thread for the WATCHER-driven
    /// refresh — a 50k-entry walk + sort would otherwise block the event loop
    /// on every debounced fs-event burst. `side`/`gen` are the staleness tag:
    /// the result is applied only if column `side` is still focused at the same
    /// `list_generation` (no chdir / sync refresh / focus switch since the spawn).
    RefreshListing {
        side: Side,
        dir: PathBuf,
        generation: u64,
    },
}

// No `derive(Debug)`: `SpecialFileOpened` carries a `PagerView`, which isn't
// `Debug` (it holds styled `ratatui::text::Line`s). Same call as
// `preview_ops::PreviewOutcome`, the other off-thread carrier of a built view.
pub enum FileOutcome {
    Copied {
        count: usize,
        dest: PathBuf,
        result: Result<(), String>,
    },
    Moved {
        count: usize,
        dest: PathBuf,
        result: Result<(), String>,
    },
    /// Outcome of a [`FileOp::RenameEach`] fan-out: `verb_move` picks the
    /// flash wording, `count` is how many pairs were attempted, and `result`
    /// is the first failure (stops at the first error) or Ok.
    RenamedEach {
        count: usize,
        is_move: bool,
        result: Result<(), String>,
    },
    PipedContent {
        payload: Vec<u8>,
        count: usize,
        skipped: usize,
    },
    Restored {
        rela_path: String,
        result: Result<(), String>,
    },
    /// Computed `L` long-listing lines, ready to open in a fit-to-content pager.
    LongListReady { title: String, lines: Vec<String> },
    /// Computed file-type classification: `flash` is set for a single path
    /// (shown in the status line), otherwise `pager_lines` holds the
    /// `name: type` rows for a multi-file pager.
    FileTypeReady {
        flash: Option<String>,
        pager_lines: Vec<String>,
    },
    /// Result of a [`FileOp::OpenSpecialFile`] build. `Ok` carries the built
    /// view (boxed — `PagerView` is large) to install at `dest`; `Err` is the
    /// builder's reason string (the refusal for a tty/FIFO/socket, or a read
    /// error), flashed on the status bar.
    SpecialFileOpened {
        result: Result<Box<PagerView>, String>,
        dest: PagerDest,
    },
    /// A watcher-driven listing read ([`FileOp::RefreshListing`]). `side`/`gen`
    /// are the staleness tag carried from the spawn; `result` is the read (`Err`
    /// is logged and dropped). Applied by `apply_one_file_outcome`.
    ListingRefreshed {
        side: Side,
        generation: u64,
        result: Result<Listing, String>,
    },
}

pub fn run_file_op(op: FileOp) -> FileOutcome {
    match op {
        FileOp::Copy { paths, dest } => {
            let count = paths.len();
            let paths_ref: Vec<&std::path::Path> = paths.iter().map(PathBuf::as_path).collect();
            let result =
                crate::fs::ops::copy_selection_to(&paths_ref, &dest).map_err(|e| e.to_string());
            FileOutcome::Copied {
                count,
                dest,
                result,
            }
        }
        FileOp::Move { paths, dest } => {
            let count = paths.len();
            let paths_ref: Vec<&std::path::Path> = paths.iter().map(PathBuf::as_path).collect();
            let result =
                crate::fs::ops::move_selection_to(&paths_ref, &dest).map_err(|e| e.to_string());
            FileOutcome::Moved {
                count,
                dest,
                result,
            }
        }
        FileOp::RenameEach { pairs, is_move } => {
            let count = pairs.len();
            let mut result = Ok(());
            for (src, dest) in &pairs {
                // `dispatch_selection` with a single source + non-existent dest
                // renames; with an existing dir it moves into it — either is a
                // sensible outcome for a per-file target.
                let one = [src.as_path()];
                let r = if is_move {
                    crate::fs::ops::move_selection_to(&one, dest)
                } else {
                    crate::fs::ops::copy_selection_to(&one, dest)
                };
                if let Err(e) = r {
                    result = Err(format!("{}: {e}", src.display()));
                    break;
                }
            }
            FileOutcome::RenamedEach {
                count,
                is_move,
                result,
            }
        }
        FileOp::GitRestore {
            repo_root,
            rela_path,
        } => {
            let result = crate::git::restore::restore_to_worktree(&repo_root, &rela_path).map(drop);
            FileOutcome::Restored { rela_path, result }
        }
        FileOp::LongList { paths, title } => {
            let refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
            let lines = crate::fs::long_listing::format_long_listing(&refs);
            FileOutcome::LongListReady { title, lines }
        }
        FileOp::FileType { paths } => {
            // Single path → a status flash; multiple → a pager table. Mirrors
            // the synchronous behavior the pure `apply` arm used to produce.
            if let [only] = paths.as_slice() {
                let label = crate::fs::ops::file_type_label(only);
                FileOutcome::FileTypeReady {
                    flash: Some(format!("{}: {label}", file_name_of(only))),
                    pager_lines: Vec::new(),
                }
            } else {
                let pager_lines = paths
                    .iter()
                    .map(|p| {
                        format!(
                            "{}: {}",
                            file_name_of(p),
                            crate::fs::ops::file_type_label(p)
                        )
                    })
                    .collect();
                FileOutcome::FileTypeReady {
                    flash: None,
                    pager_lines,
                }
            }
        }
        FileOp::OpenSpecialFile {
            path,
            theme,
            open_as_rendered,
            wrap,
            dest,
        } => {
            // The blocking read lives here, off the input thread. Reuse the
            // exact pure builder the inline path uses, so a device's pager
            // (sample, hex-dump, line numbers) is byte-identical to a regular
            // file's. Scroll-position restore is the `&mut self` wrapper's job
            // and is skipped here — a device has no meaningful saved offset.
            let result = pager_handler::build_pager_view(&path, &theme, open_as_rendered, wrap)
                .map(Box::new);
            FileOutcome::SpecialFileOpened { result, dest }
        }
        FileOp::RefreshListing {
            side,
            dir,
            generation,
        } => {
            // The heavy 50k-entry walk + sort, off the input thread.
            let result = Listing::read(&dir).map_err(|e| e.to_string());
            FileOutcome::ListingRefreshed {
                side,
                generation,
                result,
            }
        }
        FileOp::PipeContent {
            use_inventory,
            inventory_ids,
            paths,
        } => {
            let mut payload = String::new();
            let mut count = 0usize;
            let mut skipped = 0usize;

            if use_inventory {
                let inv = Inventory::load();
                for id in &inventory_ids {
                    if let Some(item) = inv.items().find(|i| &i.id == id) {
                        if let Some(bytes) = inv.read_content(id) {
                            if let Ok(text) = String::from_utf8(bytes) {
                                if !payload.is_empty() {
                                    payload.push('\n');
                                }
                                let _ = write!(
                                    payload,
                                    "[file: {}]\n{}",
                                    item.orig_path.display(),
                                    text
                                );
                                count += 1;
                            } else {
                                skipped += 1;
                            }
                        } else {
                            skipped += 1;
                        }
                    }
                }
            } else {
                for path in &paths {
                    let Ok(contents) = std::fs::read_to_string(path) else {
                        skipped += 1;
                        continue;
                    };
                    if !payload.is_empty() {
                        payload.push('\n');
                    }
                    let _ = write!(payload, "[file: {}]\n{}", path.display(), contents);
                    count += 1;
                }
            }

            let mut buf = Vec::new();
            if count > 0 {
                // Send as bracketed paste so it arrives as a single block.
                buf.reserve(payload.len() + 12);
                buf.extend_from_slice(b"\x1b[200~");
                buf.extend_from_slice(payload.as_bytes());
                buf.extend_from_slice(b"\x1b[201~");
            }
            FileOutcome::PipedContent {
                payload: buf,
                count,
                skipped,
            }
        }
    }
}

/// A path's basename for display, falling back to the full path string.
fn file_name_of(p: &Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

impl App {
    pub(crate) fn apply_file_outcomes(&mut self) -> (bool, Vec<Effect>) {
        let landed: Vec<FileOutcome> =
            std::mem::take(&mut *self.runtime.file_results.lock().unwrap());
        if landed.is_empty() {
            return (false, Vec::new());
        }
        let mut effects = Vec::new();
        for outcome in landed {
            self.apply_one_file_outcome(outcome, &mut effects);
        }
        (true, effects)
    }

    fn apply_one_file_outcome(&mut self, outcome: FileOutcome, effects: &mut Vec<Effect>) {
        match outcome {
            FileOutcome::Copied {
                count,
                dest,
                result,
            } => {
                match result {
                    Ok(()) => self
                        .state
                        .flash_info(format!("copied {count} item(s) to {}", dest.display())),
                    Err(e) => self.state.flash_error(format!("error: {e}")),
                }
                self.state.cur_mut().picks.clear();
                self.state.refresh_listing();
            }
            FileOutcome::Moved {
                count,
                dest,
                result,
            } => {
                match result {
                    Ok(()) => self
                        .state
                        .flash_info(format!("moved {count} item(s) to {}", dest.display())),
                    Err(e) => self.state.flash_error(format!("error: {e}")),
                }
                self.state.cur_mut().picks.clear();
                self.state.refresh_listing();
            }
            FileOutcome::RenamedEach {
                count,
                is_move,
                result,
            } => {
                let verb = if is_move { "renamed" } else { "copied" };
                match result {
                    Ok(()) => self.state.flash_info(format!("{verb} {count} item(s)")),
                    Err(e) => self.state.flash_error(format!("error: {e}")),
                }
                self.state.cur_mut().picks.clear();
                self.state.refresh_listing();
            }
            FileOutcome::Restored { rela_path, result } => {
                match result {
                    Ok(()) => self.state.flash_info(format!("restored {rela_path}")),
                    Err(e) => self.state.flash_error(format!("restore failed: {e}")),
                }
                // The file is back on disk (so its ghost row clears) and the
                // git status changes — refresh both.
                self.state.refresh_listing();
            }
            FileOutcome::PipedContent {
                payload,
                count,
                skipped,
            } => {
                if count == 0 {
                    self.state
                        .flash_error("no readable text files in selection");
                    return;
                }
                let msg = if skipped > 0 {
                    format!("piped {count} file(s), skipped {skipped} binary/unreadable")
                } else {
                    format!("piped {count} file(s) to pane")
                };
                effects.push(Effect::SendToPane {
                    target: PaneTarget::Active,
                    input: PaneInput::Bytes(payload),
                    on_ok: Some(msg),
                    err_prefix: Some("pipe failed"),
                });
            }
            FileOutcome::LongListReady { title, lines } => {
                self.open_pager_request(PagerRequest {
                    title,
                    lines,
                    columns: 1,
                    fit_to_content: true,
                });
            }
            FileOutcome::FileTypeReady { flash, pager_lines } => {
                if let Some(msg) = flash {
                    self.state.flash_info(msg);
                } else {
                    self.open_pager_request(PagerRequest {
                        title: "file types".to_string(),
                        lines: pager_lines,
                        columns: 1,
                        fit_to_content: false,
                    });
                }
            }
            FileOutcome::SpecialFileOpened { result, dest } => match result {
                Ok(view) => self.install_pager_at_dest(*view, dest),
                Err(e) => self.state.flash_error(e),
            },
            FileOutcome::ListingRefreshed {
                side,
                generation,
                result,
            } => {
                self.runtime.listing_refresh_inflight = false;
                if let Ok(listing) = result {
                    // Apply only if nothing changed this column's view since the
                    // read was spawned — a chdir / sync `refresh_listing` / focus
                    // switch all bump `list_generation` (or move focus), and the
                    // fresh read would otherwise clobber that newer state.
                    if self.state.focused_side() == side
                        && self.state.col(side).list_generation == generation
                    {
                        self.state.apply_refreshed_listing(listing);
                        // The fs-driven refresh may have shifted cursor_file /
                        // git_branch — mirror the old inline `context_dirty`.
                        self.view.context_dirty = true;
                    }
                }
                // A refresh requested while this read was in flight → re-spawn
                // so the latest on-disk state is eventually shown.
                if std::mem::take(&mut self.runtime.listing_refresh_dirty) {
                    self.spawn_listing_refresh();
                }
            }
        }
    }

    /// Kick the watcher-driven listing refresh off the input thread. Does the
    /// cheap self-heal inline (≤2 `is_dir` stats), then spawns a worker for the
    /// heavy `Listing::read`. Single-in-flight: a request arriving while a read
    /// is running just marks `dirty`, and the result handler re-spawns — so a
    /// burst of fs events can't pile up overlapping reads. The result is
    /// staleness-tagged with the focused column's `list_generation`.
    pub(crate) fn spawn_listing_refresh(&mut self) {
        self.state.reset_orphaned_columns_to_home();
        if self.runtime.listing_refresh_inflight {
            self.runtime.listing_refresh_dirty = true;
            return;
        }
        let side = self.state.focused_side();
        let dir = self.state.col(side).listing.dir.clone();
        let generation = self.state.col(side).list_generation;
        self.runtime.listing_refresh_inflight = true;
        self.spawn_file_op(FileOp::RefreshListing {
            side,
            dir,
            generation,
        });
    }

    /// Spawn the detached file-op worker: run `op`, push its outcome onto
    /// `runtime.file_results`, and wake the loop. The single spawn site shared
    /// by the [`Effect::FileOp`] executor arm and the executor-layer `gF` open
    /// (`goto_file_navigate`), so the slot/wake wiring can't drift. `wake` is
    /// `None` only before `run()` / in the test harness, where the outcome
    /// still lands in the slot for a manual drain.
    pub(crate) fn spawn_file_op(&self, op: FileOp) {
        let results = std::sync::Arc::clone(&self.runtime.file_results);
        let wake = self.runtime.pane_wake_tx.clone();
        std::thread::spawn(move || {
            let outcome = run_file_op(op);
            results.lock().unwrap().push(outcome);
            if let Some(tx) = wake {
                let _ = tx.send(Message::FileOpDone);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::state::Side;
    use super::{App, Effect, FileOp, FileOutcome, PagerDest, run_file_op};
    use crate::ui::pager::PagerView;
    use crate::ui::theme::Theme;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn write_file(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn copy_op_copies_files_into_dest_dir() {
        let work = tempdir().unwrap();
        let a = write_file(work.path(), "a.txt", "alpha");
        let dest = work.path().join("dest");
        std::fs::create_dir(&dest).unwrap();
        let out = run_file_op(FileOp::Copy {
            paths: vec![a.clone()],
            dest: dest.clone(),
        });
        let FileOutcome::Copied { count, result, .. } = out else {
            panic!("expected Copied");
        };
        assert_eq!(count, 1);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            std::fs::read_to_string(dest.join("a.txt")).unwrap(),
            "alpha"
        );
        assert!(a.exists(), "copy leaves the source in place");
    }

    #[test]
    fn move_op_relocates_files() {
        let work = tempdir().unwrap();
        let a = write_file(work.path(), "a.txt", "alpha");
        let dest = work.path().join("dest");
        std::fs::create_dir(&dest).unwrap();
        let out = run_file_op(FileOp::Move {
            paths: vec![a.clone()],
            dest: dest.clone(),
        });
        let FileOutcome::Moved { count, result, .. } = out else {
            panic!("expected Moved");
        };
        assert_eq!(count, 1);
        assert!(result.is_ok(), "{result:?}");
        assert!(dest.join("a.txt").exists());
        assert!(!a.exists(), "move removes the source");
    }

    #[test]
    fn rename_each_renames_every_pair_to_its_own_dest() {
        let work = tempdir().unwrap();
        let a = write_file(work.path(), "a.txt", "alpha");
        let b = write_file(work.path(), "b.txt", "beta");
        let out = run_file_op(FileOp::RenameEach {
            pairs: vec![
                (a.clone(), work.path().join("a.txt.bak")),
                (b.clone(), work.path().join("b.txt.bak")),
            ],
            is_move: true,
        });
        let FileOutcome::RenamedEach {
            count,
            is_move,
            result,
        } = out
        else {
            panic!("expected RenamedEach");
        };
        assert_eq!(count, 2);
        assert!(is_move);
        assert!(result.is_ok(), "{result:?}");
        assert!(work.path().join("a.txt.bak").exists());
        assert!(work.path().join("b.txt.bak").exists());
        assert!(!a.exists() && !b.exists(), "move removes the sources");
    }

    #[test]
    fn pipe_content_from_selection_wraps_bracketed_paste() {
        let work = tempdir().unwrap();
        let a = write_file(work.path(), "a.txt", "hello");
        let missing = work.path().join("nope.txt");
        let out = run_file_op(FileOp::PipeContent {
            use_inventory: false,
            inventory_ids: Vec::new(),
            paths: vec![a, missing],
        });
        let FileOutcome::PipedContent {
            payload,
            count,
            skipped,
        } = out
        else {
            panic!("expected PipedContent");
        };
        assert_eq!(count, 1);
        assert_eq!(skipped, 1, "the missing file is skipped");
        assert!(payload.starts_with(b"\x1b[200~"));
        assert!(payload.ends_with(b"\x1b[201~"));
        assert!(String::from_utf8_lossy(&payload).contains("[file: "));
    }

    #[test]
    fn pipe_content_with_nothing_readable_yields_empty_payload() {
        let work = tempdir().unwrap();
        let missing = work.path().join("nope.txt");
        let out = run_file_op(FileOp::PipeContent {
            use_inventory: false,
            inventory_ids: Vec::new(),
            paths: vec![missing],
        });
        let FileOutcome::PipedContent {
            payload,
            count,
            skipped,
        } = out
        else {
            panic!("expected PipedContent");
        };
        assert_eq!(count, 0);
        assert_eq!(skipped, 1);
        assert!(
            payload.is_empty(),
            "no bracketed-paste wrapper when nothing is readable"
        );
    }

    #[test]
    fn apply_piped_content_drains_slot_and_returns_send_effect() {
        let work = tempdir().unwrap();
        let mut app = App::test_app(work.path().to_path_buf());
        // Empty slot → nothing applied, no effects.
        let (drew, fx) = app.apply_file_outcomes();
        assert!(!drew);
        assert!(fx.is_empty());
        // A landed PipedContent outcome is drained and surfaced as a SendToPane
        // effect for the run loop to execute (no executor bypass).
        app.runtime
            .file_results
            .lock()
            .unwrap()
            .push(FileOutcome::PipedContent {
                payload: b"\x1b[200~hi\x1b[201~".to_vec(),
                count: 1,
                skipped: 0,
            });
        let (drew, fx) = app.apply_file_outcomes();
        assert!(drew);
        assert_eq!(fx.len(), 1);
        assert!(matches!(fx[0], Effect::SendToPane { .. }));
    }

    #[test]
    fn long_list_op_builds_table_off_thread() {
        let work = tempdir().unwrap();
        let a = write_file(work.path(), "hello.txt", "hi");
        let out = run_file_op(FileOp::LongList {
            paths: vec![a],
            title: "long listing — x".to_string(),
        });
        let FileOutcome::LongListReady { title, lines } = out else {
            panic!("expected LongListReady");
        };
        assert_eq!(title, "long listing — x");
        assert!(lines[0].contains("MODE"), "header row: {lines:?}");
        assert!(
            lines.iter().any(|l| l.contains("hello.txt")),
            "filename row: {lines:?}"
        );
    }

    #[test]
    fn refresh_listing_op_reads_dir_off_thread() {
        let work = tempdir().unwrap();
        write_file(work.path(), "alpha.txt", "a");
        write_file(work.path(), "beta.txt", "b");
        let out = run_file_op(FileOp::RefreshListing {
            side: Side::Left,
            dir: work.path().to_path_buf(),
            generation: 7,
        });
        let FileOutcome::ListingRefreshed {
            side,
            generation,
            result,
        } = out
        else {
            panic!("expected ListingRefreshed");
        };
        assert_eq!(side, Side::Left);
        assert_eq!(generation, 7, "staleness tag rides through the worker");
        let listing = result.expect("read ok");
        assert!(listing.entries.iter().any(|e| e.name == "alpha.txt"));
        assert!(listing.entries.iter().any(|e| e.name == "beta.txt"));
    }

    #[test]
    fn fresh_listing_refresh_applies_to_focused_column() {
        let work = tempdir().unwrap();
        let mut app = App::test_app(work.path().to_path_buf());
        let side = app.state.focused_side();
        let generation = app.state.col(side).list_generation;
        let listing = crate::fs::listing::Listing::read(work.path()).unwrap();
        app.runtime.listing_refresh_inflight = true;
        app.runtime
            .file_results
            .lock()
            .unwrap()
            .push(FileOutcome::ListingRefreshed {
                side,
                generation,
                result: Ok(listing),
            });
        let (drew, _fx) = app.apply_file_outcomes();
        assert!(drew, "applying a refreshed listing redraws");
        assert!(!app.runtime.listing_refresh_inflight, "in-flight cleared");
        // apply ran → rebuild_rows bumped the generation.
        assert_ne!(app.state.col(side).list_generation, generation);
    }

    #[test]
    fn stale_listing_refresh_is_discarded() {
        let work = tempdir().unwrap();
        let mut app = App::test_app(work.path().to_path_buf());
        let side = app.state.focused_side();
        let generation = app.state.col(side).list_generation;
        let listing = crate::fs::listing::Listing::read(work.path()).unwrap();
        app.runtime.listing_refresh_inflight = true;
        // Tag with a generation that no longer matches (a chdir / sync refresh
        // happened since the read was spawned) → the read must be dropped.
        app.runtime
            .file_results
            .lock()
            .unwrap()
            .push(FileOutcome::ListingRefreshed {
                side,
                generation: generation.wrapping_sub(1),
                result: Ok(listing),
            });
        app.apply_file_outcomes();
        assert!(!app.runtime.listing_refresh_inflight, "in-flight cleared");
        // Discarded → apply_refreshed_listing never ran, so no generation bump.
        assert_eq!(app.state.col(side).list_generation, generation);
    }

    #[test]
    fn file_type_op_single_yields_flash_not_pager() {
        let work = tempdir().unwrap();
        let a = write_file(work.path(), "a.txt", "plain text");
        let out = run_file_op(FileOp::FileType { paths: vec![a] });
        let FileOutcome::FileTypeReady { flash, pager_lines } = out else {
            panic!("expected FileTypeReady");
        };
        let flash = flash.expect("single path flashes");
        assert!(flash.starts_with("a.txt: "), "got {flash}");
        assert!(pager_lines.is_empty(), "single path has no pager lines");
    }

    #[test]
    fn file_type_op_multi_yields_pager_lines() {
        let work = tempdir().unwrap();
        let a = write_file(work.path(), "a.txt", "x");
        let b = write_file(work.path(), "b.txt", "y");
        let out = run_file_op(FileOp::FileType { paths: vec![a, b] });
        let FileOutcome::FileTypeReady { flash, pager_lines } = out else {
            panic!("expected FileTypeReady");
        };
        assert!(flash.is_none(), "multi path opens a pager, no flash");
        assert_eq!(pager_lines.len(), 2);
        assert!(pager_lines[0].starts_with("a.txt: "));
        assert!(pager_lines[1].starts_with("b.txt: "));
    }

    #[test]
    fn long_list_outcome_drains_into_a_pager() {
        let work = tempdir().unwrap();
        let mut app = App::test_app(work.path().to_path_buf());
        app.runtime
            .file_results
            .lock()
            .unwrap()
            .push(FileOutcome::LongListReady {
                title: "long listing — x".to_string(),
                lines: vec!["INODE  MODE".to_string(), "1  -rw-".to_string()],
            });
        let (drew, fx) = app.apply_file_outcomes();
        assert!(drew);
        assert!(fx.is_empty(), "opening a pager emits no follow-on effect");
        let pager = app.view.pager.as_ref().expect("pager opened from outcome");
        assert_eq!(pager.title, "long listing — x");
        assert!(pager.fit_to_content, "L pager is fit-to-content");
    }

    #[test]
    fn open_special_file_op_builds_a_view_off_thread() {
        // The worker reuses the regular open path's builder, so a *readable*
        // special file (here a plain file standing in for `/dev/zero`) yields a
        // pager view titled by the file name — the sample the user wants.
        let work = tempdir().unwrap();
        let a = write_file(work.path(), "sample.bin", "zeros\n");
        let out = run_file_op(FileOp::OpenSpecialFile {
            path: a,
            theme: Theme::default(),
            open_as_rendered: false,
            wrap: None,
            dest: PagerDest::Overlay { scroll: Some(0) },
        });
        let FileOutcome::SpecialFileOpened { result, dest } = out else {
            panic!("expected SpecialFileOpened");
        };
        assert!(matches!(dest, PagerDest::Overlay { scroll: Some(0) }));
        let view = result.expect("a readable file builds a view");
        assert_eq!(view.title, "sample.bin");
    }

    /// The worker carries the build *failure* back as `Err` (instead of
    /// blocking or panicking): a socket is refused by `build_pager_view`'s
    /// special-file guard. A unix socket is a portable non-regular stand-in.
    #[cfg(unix)]
    #[test]
    fn open_special_file_op_refuses_a_socket() {
        let work = tempdir().unwrap();
        let sock = work.path().join("s.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let out = run_file_op(FileOp::OpenSpecialFile {
            path: sock,
            theme: Theme::default(),
            open_as_rendered: false,
            wrap: None,
            dest: PagerDest::Overlay { scroll: None },
        });
        let FileOutcome::SpecialFileOpened { result, .. } = out else {
            panic!("expected SpecialFileOpened");
        };
        let err = result.err().expect("a socket is refused, not paged");
        assert!(err.contains("not a readable file"), "got {err}");
    }

    #[test]
    fn special_file_outcome_installs_overlay_pager_at_scroll() {
        let work = tempdir().unwrap();
        let mut app = App::test_app(work.path().to_path_buf());
        let view = PagerView::new_plain(
            "dev".to_string(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        app.runtime
            .file_results
            .lock()
            .unwrap()
            .push(FileOutcome::SpecialFileOpened {
                result: Ok(Box::new(view)),
                dest: PagerDest::Overlay { scroll: Some(2) },
            });
        let (drew, fx) = app.apply_file_outcomes();
        assert!(drew);
        assert!(
            fx.is_empty(),
            "installing a pager emits no follow-on effect"
        );
        let pager = app.view.pager.as_ref().expect("overlay pager installed");
        assert_eq!(
            pager.scroll, 2,
            "the dest's scroll override (gF line) applied"
        );
    }

    #[test]
    fn special_file_outcome_installs_top_pane_pager() {
        let work = tempdir().unwrap();
        let mut app = App::test_app(work.path().to_path_buf());
        let view = PagerView::new_plain("dev".to_string(), vec!["x".to_string()]);
        app.runtime
            .file_results
            .lock()
            .unwrap()
            .push(FileOutcome::SpecialFileOpened {
                result: Ok(Box::new(view)),
                dest: PagerDest::TopPane,
            });
        let (drew, _) = app.apply_file_outcomes();
        assert!(drew);
        let pager = app.view.pager.as_ref().expect("top-pane pager installed");
        assert!(matches!(pager.mount, crate::ui::pager::Mount::TopPane));
        assert!(pager.no_history, "D opens are not pushed to buffer history");
    }

    #[test]
    fn special_file_outcome_flashes_on_build_error() {
        let work = tempdir().unwrap();
        let mut app = App::test_app(work.path().to_path_buf());
        app.runtime
            .file_results
            .lock()
            .unwrap()
            .push(FileOutcome::SpecialFileOpened {
                result: Err("s.sock: not a readable file".to_string()),
                dest: PagerDest::Overlay { scroll: None },
            });
        let (drew, fx) = app.apply_file_outcomes();
        assert!(drew);
        assert!(fx.is_empty());
        assert!(app.view.pager.is_none(), "a refused open leaves no pager");
        assert!(
            app.state
                .flash
                .as_ref()
                .is_some_and(|f| f.text.contains("not a readable file")),
            "the refusal is flashed, got {:?}",
            app.state.flash
        );
    }
}
