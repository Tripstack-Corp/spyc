use std::fmt::Write;
use std::path::PathBuf;

use super::{App, Effect, PaneInput, PaneTarget};
use crate::state::inventory::Inventory;

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
}

#[derive(Debug)]
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{App, Effect, FileOp, FileOutcome, run_file_op};
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
}
