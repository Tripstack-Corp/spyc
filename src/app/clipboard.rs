//! Clipboard / selection plumbing: yank paths or the last pane prompt to the
//! system clipboard, put inventory items to cwd, send/pipe the selection into
//! the active pane, and the copy/move file-op runner. All entry points are
//! `pub` — called from `actions`/`key_dispatch`.

use std::fmt::Write;
use std::path::{Path, PathBuf};

use crate::shell;

use super::{App, ClipMsg, Effect, PaneInput, PaneTarget};

impl App {
    /// yf — yank the cursor file's absolute path to the system
    /// clipboard. When picks are active, yanks all of them
    /// newline-separated. Always absolute paths so the receiving
    /// shell resolves them correctly regardless of where the user
    /// pastes them. The user's recurring real-world ask was a clean
    /// way to grab a path for one-off shell commands like `git
    /// restore <path>` without opening a pane.
    pub fn yank_paths_to_clipboard(&mut self) -> Vec<Effect> {
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            self.state.flash_error("no path to yank");
            return Vec::new();
        }
        let text: String = paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let ok = if paths.len() == 1 {
            ClipMsg::SinglePath
        } else {
            ClipMsg::MultiPath { count: paths.len() }
        };
        vec![Effect::CopyToClipboard { text, ok }]
    }

    /// yP — yank the last prompt the user typed into the pane.
    pub fn yank_last_prompt_to_clipboard(&mut self) -> Vec<Effect> {
        let Some(text) = self.state.pane.last_pane_prompt.as_ref() else {
            self.state.flash_error("no prompt to yank");
            return Vec::new();
        };
        vec![Effect::CopyToClipboard {
            text: text.clone(),
            ok: ClipMsg::Prompt,
        }]
    }

    /// Put inventory items to the current working directory.
    /// Picked items only if any picks exist, else all.
    /// Items are removed from inventory after successful put.
    pub fn put_inventory_to_cwd(&mut self) -> Vec<Effect> {
        let dest = self.state.left.listing.dir.clone();
        let item_count = if self.state.inventory.picks.is_empty() {
            self.state.inventory.len()
        } else {
            self.state.inventory.picks.len()
        };
        if item_count == 0 {
            self.state.flash_error("inventory is empty");
            return Vec::new();
        }
        // Puts are unconditional today; large puts (>10 items) could
        // grow a confirmation prompt if the unguarded put ever bites.
        let (count, _, err) = self.state.inventory.put_to(&dest);
        self.state.rebuild_rows();
        if count > 0 {
            self.state.refresh_listing();
            self.state
                .flash_info(format!("put {count} file(s) to {}", dest.display()));
        }
        if let Some(e) = err {
            self.state.flash_error(e);
        }
        Vec::new()
    }

    /// ^W s — write the current selection as shell-quoted paths to the
    /// pane's stdin. A trailing space is appended so the user can keep
    /// typing without concatenating against the last path. No newline
    /// — let the user decide when to submit.
    pub fn send_selection_to_pane(&mut self) -> Vec<Effect> {
        if self.runtime.pane_tabs.is_none() {
            self.state.flash_error("no pane open (Ctrl-\\ to open one)");
            return Vec::new();
        }
        // Build the payload before grabbing the pane mut-borrow, so we
        // can still call self.flash_* below without overlapping borrows.
        // Clone project_home up front so the immutable borrow doesn't
        // overlap with the selection_paths borrow below.
        let project_home = self.state.project_home.clone();
        let (payload, count) = {
            let paths = self.state.selection_paths();
            if paths.is_empty() {
                self.state.flash_error("nothing selected");
                return Vec::new();
            }
            let count = paths.len();
            let mut out = String::new();
            for (i, p) in paths.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                // Anchor paths on PROJECT_HOME so what lands in the
                // pane matches what an agent / shell session running
                // inside that project would type. Outside-project
                // paths stay absolute rather than walking up with
                // `../../..`, which is rarely what the user wants.
                let display = project_home
                    .as_deref()
                    .and_then(|home| p.strip_prefix(home).ok())
                    .map_or_else(
                        || p.to_path_buf(),
                        |rel| {
                            if rel.as_os_str().is_empty() {
                                // path == project_home itself.
                                std::path::PathBuf::from(".")
                            } else {
                                rel.to_path_buf()
                            }
                        },
                    );
                out.push_str(&shell::shell_quote(&display.to_string_lossy()));
            }
            out.push(' ');
            (out, count)
        };
        vec![Effect::SendToPane {
            target: PaneTarget::Active,
            input: PaneInput::Bytes(payload.into_bytes()),
            on_ok: Some(format!("sent {count} path(s) to pane")),
            err_prefix: Some("send failed"),
        }]
    }

    /// ^W p / ^W i — read file contents of selection (or inventory) and
    /// send them to the active pane tab as bracketed paste. Each file is
    /// wrapped with a header so the recipient (e.g. Claude) knows what
    /// it's looking at.
    pub fn pipe_content_to_pane(&mut self, use_inventory: bool) -> Vec<Effect> {
        if self.runtime.pane_tabs.is_none() {
            self.state.flash_error("no pane open");
            return Vec::new();
        }
        // Build payload: read from cache for inventory, from disk for selection.
        let mut payload = String::new();
        let mut count = 0usize;
        let mut skipped = 0usize;

        if use_inventory {
            let ids = self.state.inventory.selected_ids();
            if ids.is_empty() {
                self.state.flash_error("inventory is empty");
                return Vec::new();
            }
            for id in &ids {
                if let Some(item) = self.state.inventory.items().find(|i| &i.id == id) {
                    if let Some(bytes) = self.state.inventory.read_content(id) {
                        if let Ok(text) = String::from_utf8(bytes) {
                            if !payload.is_empty() {
                                payload.push('\n');
                            }
                            let _ =
                                write!(payload, "[file: {}]\n{}", item.orig_path.display(), text);
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
            let paths: Vec<PathBuf> = self
                .state
                .selection_paths()
                .into_iter()
                .map(Path::to_path_buf)
                .collect();
            if paths.is_empty() {
                self.state.flash_error("nothing selected");
                return Vec::new();
            }
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

        if count == 0 {
            self.state
                .flash_error("no readable text files in selection");
            return Vec::new();
        }
        // Send as bracketed paste so it arrives as a single block.
        let mut buf = Vec::with_capacity(payload.len() + 12);
        buf.extend_from_slice(b"\x1b[200~");
        buf.extend_from_slice(payload.as_bytes());
        buf.extend_from_slice(b"\x1b[201~");
        let msg = if skipped > 0 {
            format!("piped {count} file(s), skipped {skipped} binary/unreadable")
        } else {
            format!("piped {count} file(s) to pane")
        };
        vec![Effect::SendToPane {
            target: PaneTarget::Active,
            input: PaneInput::Bytes(buf),
            on_ok: Some(msg),
            err_prefix: Some("pipe failed"),
        }]
    }

    /// Resolve `raw_dest` and run a copy-like or move-like operation across
    /// the current selection. Flash a success / error message afterwards
    /// and refresh the listing so results are visible immediately.
    pub fn run_selection_to(
        &mut self,
        raw_dest: &str,
        op: fn(&[&Path], &Path) -> std::io::Result<()>,
        verb: &str,
    ) {
        let dest_trim = raw_dest.trim();
        if dest_trim.is_empty() {
            return;
        }
        let paths = self.state.selection_paths();
        if paths.is_empty() {
            self.state.flash_error("nothing selected");
            return;
        }
        let count = paths.len();
        let expanded = crate::paths::expand(dest_trim);
        let dest = if expanded.is_absolute() {
            expanded
        } else {
            self.state.left.listing.dir.join(&expanded)
        };
        self.run_and_flash(
            op(&paths, &dest),
            format!("{verb} {count} item(s) to {}", dest.display()),
        );
        // Picks point at paths that may no longer exist after a move.
        self.state.left.picks.clear();
        self.state.refresh_listing();
    }

    /// Set the flash message based on the result of a mutating operation.
    pub fn run_and_flash(&mut self, result: std::io::Result<()>, success_msg: String) {
        match result {
            Ok(()) => self.state.flash_info(success_msg),
            Err(e) => self.state.flash_error(format!("error: {e}")),
        }
    }
}
