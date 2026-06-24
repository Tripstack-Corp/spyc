//! Clipboard / selection plumbing: yank paths or the last pane prompt to the
//! system clipboard, put inventory items to cwd, send/pipe the selection into
//! the active pane, and the copy/move file-op runner. All entry points are
//! `pub` — called from `actions`/`key_dispatch`.

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
        let dest = self.state.cur().listing.dir.clone();
        let ids: Vec<String> = if self.state.inventory.picks.is_empty() {
            self.state.inventory.items().map(|i| i.id.clone()).collect()
        } else {
            self.state.inventory.picks.iter().cloned().collect()
        };
        if ids.is_empty() {
            self.state.flash_error("inventory is empty");
            return Vec::new();
        }
        vec![Effect::Inventory(super::inventory_ops::InventoryOp::Put {
            dest_dir: dest,
            ids,
        })]
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
        let (inventory_ids, paths) = if use_inventory {
            let ids = self.state.inventory.selected_ids();
            if ids.is_empty() {
                self.state.flash_error("inventory is empty");
                return Vec::new();
            }
            (ids, Vec::new())
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
            (Vec::new(), paths)
        };

        vec![Effect::FileOp(super::file_ops::FileOp::PipeContent {
            use_inventory,
            inventory_ids,
            paths,
        })]
    }

    /// Resolve `raw_dest` and run a copy-like or move-like operation across
    /// the current selection. Flash a success / error message afterwards
    /// and refresh the listing so results are visible immediately.
    ///
    /// `%` in the destination refers to each source file's own basename (a
    /// literal percent is `%%`), spy-style: `M %.o` on `Makefile` renames it
    /// to `Makefile.o`, and a multi-pick `%.bak` batch-renames every selected
    /// file to its own `<name>.bak`. Without a `%` the destination is a single
    /// target (a directory to move into, or a rename when one file is selected)
    /// exactly as before.
    pub fn run_selection_to(&mut self, raw_dest: &str, is_move: bool) -> Vec<Effect> {
        let dest_trim = raw_dest.trim();
        if dest_trim.is_empty() {
            return Vec::new();
        }
        let paths: Vec<PathBuf> = self
            .state
            .selection_paths()
            .into_iter()
            .map(std::path::Path::to_path_buf)
            .collect();
        if paths.is_empty() {
            self.state.flash_error("nothing selected");
            return Vec::new();
        }
        let base_dir = self.state.cur().listing.dir.clone();

        // Per-file `%` expansion: each source resolves the destination against
        // its OWN name. Single source reuses the plain Copy/Move op (keeping
        // its "to <dest>" flash); multiple sources fan out via RenameEach.
        if dest_references_name(dest_trim) {
            let pairs: Vec<(PathBuf, PathBuf)> = paths
                .into_iter()
                .map(|src| {
                    let name = src
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    let dest = resolve_dest(&expand_dest_name(dest_trim, &name), &base_dir);
                    (src, dest)
                })
                .collect();
            if pairs.len() == 1 {
                let (src, dest) = pairs.into_iter().next().expect("len == 1");
                let paths = vec![src];
                return vec![Effect::FileOp(if is_move {
                    super::file_ops::FileOp::Move { paths, dest }
                } else {
                    super::file_ops::FileOp::Copy { paths, dest }
                })];
            }
            return vec![Effect::FileOp(super::file_ops::FileOp::RenameEach {
                pairs,
                is_move,
            })];
        }

        let dest = resolve_dest(dest_trim, &base_dir);
        if is_move {
            vec![Effect::FileOp(super::file_ops::FileOp::Move {
                paths,
                dest,
            })]
        } else {
            vec![Effect::FileOp(super::file_ops::FileOp::Copy {
                paths,
                dest,
            })]
        }
    }

    /// Set the flash message based on the result of a mutating operation.
    pub fn run_and_flash(&mut self, result: std::io::Result<()>, success_msg: String) {
        match result {
            Ok(()) => self.state.flash_info(success_msg),
            Err(e) => self.state.flash_error(format!("error: {e}")),
        }
    }
}

/// True when `template` contains a `%` that references the source name — an
/// unescaped `%` (a literal percent is written `%%`). Drives whether a
/// copy/move destination is expanded per source file.
fn dest_references_name(template: &str) -> bool {
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            if chars.peek() == Some(&'%') {
                chars.next(); // `%%` is a literal percent, not a name reference
            } else {
                return true;
            }
        }
    }
    false
}

/// Expand `%` in a copy/move destination to `name` (a source file's basename);
/// `%%` is a literal percent. Mirrors [`crate::shell::expand_percent`]'s escape
/// rule but substitutes a single bare name with no shell quoting, so the result
/// is a plain path — `M %.o` on `Makefile` yields `Makefile.o`.
fn expand_dest_name(template: &str, name: &str) -> String {
    let mut out = String::with_capacity(template.len() + name.len());
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            if chars.peek() == Some(&'%') {
                chars.next();
                out.push('%');
            } else {
                out.push_str(name);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Resolve a (possibly `~`/env-bearing) destination string against `base_dir`:
/// tilde/env expansion first, then anchor a relative result on the listing dir.
fn resolve_dest(dest: &str, base_dir: &Path) -> PathBuf {
    let expanded = crate::paths::expand(dest);
    if expanded.is_absolute() {
        expanded
    } else {
        base_dir.join(expanded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dest_references_name_detects_unescaped_percent() {
        assert!(dest_references_name("%.o"));
        assert!(dest_references_name("backup/%"));
        assert!(dest_references_name("%%-%")); // literal then a real ref
        assert!(!dest_references_name("plain.txt"));
        assert!(!dest_references_name("100%%done")); // only an escaped literal
        assert!(!dest_references_name(""));
    }

    #[test]
    fn expand_dest_name_substitutes_basename() {
        assert_eq!(expand_dest_name("%.o", "Makefile"), "Makefile.o");
        assert_eq!(expand_dest_name("%.o", "SECURITY.md"), "SECURITY.md.o");
        // Multiple refs all expand.
        assert_eq!(expand_dest_name("%/%", "a"), "a/a");
        // `%%` stays a literal percent; a bare `%` is the name.
        assert_eq!(expand_dest_name("%%-%", "x"), "%-x");
        // No ref → unchanged.
        assert_eq!(expand_dest_name("plain.txt", "x"), "plain.txt");
    }

    /// The user's exact case: `M %.o` on the cursor file renames it to
    /// `<name>.o`, in the listing dir — reusing the plain Move op for one file.
    #[test]
    fn run_selection_to_expands_percent_for_cursor_file() {
        use super::super::file_ops::FileOp;
        let mut app = App::test_app(std::env::temp_dir());
        app.state.left.listing.dir = PathBuf::from("/projects/demo");
        app.seed_rows(&["Makefile", "README.md"]); // cursor at 0 = Makefile
        let fx = app.run_selection_to("%.o", true);
        match fx.as_slice() {
            [Effect::FileOp(FileOp::Move { paths, dest })] => {
                assert_eq!(paths, &[PathBuf::from("/projects/demo/Makefile")]);
                assert_eq!(dest, &PathBuf::from("/projects/demo/Makefile.o"));
            }
            _ => panic!("expected a single Move to Makefile.o"),
        }
    }

    /// Multi-pick `%` fans out per file: `%.bak` renames every picked file to
    /// its OWN `<name>.bak`, via the RenameEach op.
    #[test]
    fn run_selection_to_batch_renames_picks_via_percent() {
        use super::super::View;
        use super::super::file_ops::FileOp;
        let mut app = App::test_app(std::env::temp_dir());
        app.state.left.listing.dir = PathBuf::from("/projects/demo");
        app.seed_rows(&["a.txt", "b.txt"]);
        app.state.left.view = View::Dir;
        app.state
            .left
            .picks
            .insert(Path::new("/projects/demo/a.txt"));
        app.state
            .left
            .picks
            .insert(Path::new("/projects/demo/b.txt"));
        let fx = app.run_selection_to("%.bak", false);
        match fx.as_slice() {
            [Effect::FileOp(FileOp::RenameEach { pairs, is_move })] => {
                assert!(!is_move, "copy, not move");
                assert_eq!(pairs.len(), 2);
                for (src, dest) in pairs {
                    let want = PathBuf::from(format!("{}.bak", src.display()));
                    assert_eq!(dest, &want, "each picked file → its own .bak");
                }
            }
            _ => panic!("expected RenameEach for a multi-pick %"),
        }
    }
}
