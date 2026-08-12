//! Clipboard / selection plumbing: yank paths or the last pane prompt to the
//! system clipboard, put inventory items to cwd, send/pipe the selection into
//! the active pane, and the copy/move file-op runner. All entry points are
//! `pub` — called from `actions`/`key_dispatch`.

use std::path::{Path, PathBuf};

use crate::shell;

use super::{App, ClipMsg, Effect, Message, PaneInput, PaneTarget, Wake};

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

    /// `^a ↓` — send a single literal Ctrl-A (0x01) to the active pane.
    /// `^a` is spyc's pane prefix, so the child can never receive the byte
    /// through normal input; this is the tmux `send-prefix` escape hatch
    /// (Claude binds ^a, e.g. to expand notes).
    pub fn send_prefix_to_pane(&mut self) -> Vec<Effect> {
        if self.runtime.pane_tabs.is_none() {
            self.state.flash_error("no pane open (Ctrl-\\ to open one)");
            return Vec::new();
        }
        vec![Effect::SendToPane {
            target: PaneTarget::Active,
            input: PaneInput::Bytes(vec![0x01]),
            on_ok: None,
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
            Err(e) => self.state.flash_error(format!("error: {e:#}")),
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

/// Which delivery mechanisms a copy should use, given `[clipboard] via` and whether
/// spyc is running over SSH. Returns `(local_helper, osc52)`.
///
/// Pure, and the exact counterpart of `agent_status::desktop_delivery` — the same
/// question ("does this belong on the client or the host?") with the same answer
/// shape, so the two can be reasoned about together.
///
/// `Auto` is the whole point: `pbcopy`/`xclip` set the clipboard of the machine spyc
/// runs on, which over SSH is the *server* — text the user can never paste. OSC 52
/// travels back up the connection to the terminal they're actually typing at.
pub(super) const fn clipboard_delivery(
    via: crate::config::ClipboardVia,
    is_ssh: bool,
) -> (bool, bool) {
    use crate::config::ClipboardVia as V;
    match via {
        // Over SSH the local helper is worse than useless (it silently succeeds on
        // the wrong machine), so Auto swaps to the escape rather than adding it.
        V::Auto => {
            if is_ssh {
                (false, true)
            } else {
                (true, false)
            }
        }
        V::System => (true, false),
        V::Osc52 => (false, true),
        V::Both => (true, true),
    }
}

impl super::App {
    /// Deliver `text` to the clipboard per `[clipboard] via`.
    ///
    /// Succeeds if ANY enabled mechanism succeeded — with `Both`, a terminal that
    /// ignores OSC 52 shouldn't make a working local copy look like a failure. The
    /// error carries every attempt's reason, since "yank failed" with no cause is
    /// the report that wastes an afternoon.
    ///
    /// OSC 52 is attempted first under `Both`: it can fail *knowably* (payload over
    /// the size limit), and a real error beats the local helper's silent
    /// success-on-the-wrong-machine.
    ///
    /// `[clipboard].command` / `$SPYC_CLIPBOARD` is an exclusive top-priority tier,
    /// not one more mechanism to also try: when set, it's the ONLY thing run —
    /// `via`/OSC-52 are skipped entirely rather than layered underneath.
    ///
    /// **Anything that spawns a helper is dispatched, not awaited.** `xclip` and
    /// `xsel` legitimately stay alive after a successful copy — that is how they
    /// serve the X11 selection — so `try_wait` never answers and the reap poll
    /// spends its entire budget. Awaiting that put 150 ms of fully blocked event
    /// loop on *every single yank* on the platform where most non-macOS users
    /// are. So the helper runs on a worker (`spawn_clipboard_write`) and this
    /// returns as soon as the payload is on its way; a failure arrives later and
    /// `apply_clipboard_writes` flashes it, replacing the confirmation.
    ///
    /// OSC 52 stays inline — it is a write to spyc's own stdout, not a spawn.
    pub(super) fn deliver_clipboard(&self, text: &str) -> Result<(), String> {
        if let Some(cmd) =
            crate::clipboard::resolve_override(self.state.config.clipboard.command.as_deref())
        {
            let text = text.to_string();
            self.spawn_clipboard_write(move || {
                crate::clipboard::copy_via_user_command(&cmd, &text).map_err(|e| format!("{e:#}"))
            });
            return Ok(());
        }
        let (local, osc52) = clipboard_delivery(self.state.config.clipboard.via, self.view.is_ssh);
        let mut errs: Vec<String> = Vec::new();
        let mut ok = false;
        if osc52 {
            match crate::clipboard::copy_osc52(text) {
                Ok(()) => ok = true,
                Err(e) => errs.push(e),
            }
        }
        if local {
            // Whether OSC 52 already delivered it decides whether a later helper
            // failure is worth interrupting the user for: with `Both`, the text
            // IS on their clipboard, and the local helper failing is spyc's
            // problem rather than theirs.
            let already = ok;
            let text = text.to_string();
            self.spawn_clipboard_write(move || match crate::clipboard::copy(&text) {
                Err(e) if !already => Err(format!("{e:#}")),
                _ => Ok(()),
            });
            ok = true;
        }
        if ok {
            return Ok(());
        }
        Err(if errs.is_empty() {
            "no clipboard mechanism enabled ([clipboard] via)".to_string()
        } else {
            errs.join("; ")
        })
    }

    /// Run a clipboard write on a detached worker (the `graveyard_ops`
    /// template), landing its outcome on `runtime.clipboard_copy_results` and
    /// waking the loop.
    ///
    /// `pane_wake_tx` is `None` only before `run()` / in the test harness, where
    /// there is no loop to wake — the outcome still lands in the slot.
    fn spawn_clipboard_write(&self, write: impl FnOnce() -> Result<(), String> + Send + 'static) {
        let results = std::sync::Arc::clone(&self.runtime.clipboard_copy_results);
        let wake = self.runtime.pane_wake_tx.clone();
        std::thread::spawn(move || {
            let outcome = write().err();
            results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(outcome);
            if let Some(tx) = wake {
                let _ = tx.send(Message::Wake(Wake::ClipboardCopy));
            }
        });
    }

    /// Drain every landed clipboard write and flash whichever failed. Called
    /// each pre-recv scan, so the slot is always emptied whichever wake survived
    /// coalescing. Returns whether the frame is dirty.
    ///
    /// A failure arrives *after* the verb already flashed its confirmation, so it
    /// replaces one — which is the right way round: the last thing on screen is
    /// what actually happened.
    pub(crate) fn apply_clipboard_writes(&mut self) -> bool {
        let landed: Vec<Option<String>> = std::mem::take(
            &mut *self
                .runtime
                .clipboard_copy_results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        let mut dirty = false;
        for err in landed.into_iter().flatten() {
            self.state.flash_error(format!("clipboard: {err}"));
            dirty = true;
        }
        dirty
    }

    /// Kick the middle-click clipboard read onto a detached worker
    /// (`Effect::PasteFromClipboard`'s whole body).
    ///
    /// Reading the clipboard means spawning `pbpaste`/`xclip -o`/`wl-paste` and
    /// waiting for it — up to [`crate::clipboard::PASTE_READ_BUDGET`] against a
    /// helper that never answers. On the loop thread that is a total freeze: the
    /// input reader keeps reading, but nothing dispatches and no frame is drawn,
    /// with no key to break out. So this is the `graveyard_ops` template — the
    /// worker pushes its result onto `runtime.clipboard_paste_results` and wakes
    /// the loop; `apply_clipboard_pastes` does the routing in the pre-recv scan.
    ///
    /// `pane_wake_tx` is `None` only before `run()` / in the test harness, where
    /// there is no loop to wake — the result still lands in the slot.
    pub(super) fn spawn_clipboard_read(&self) {
        let results = std::sync::Arc::clone(&self.runtime.clipboard_paste_results);
        let wake = self.runtime.pane_wake_tx.clone();
        std::thread::spawn(move || {
            let read = crate::clipboard::paste();
            results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(read);
            if let Some(tx) = wake {
                let _ = tx.send(Message::Wake(Wake::ClipboardPaste));
            }
        });
    }

    /// Drain + route every landed clipboard read. Called every pre-recv scan, so
    /// the slot is ALWAYS emptied regardless of which wake survived coalescing.
    /// Returns whether the frame is dirty plus the effects the paste produced —
    /// the caller runs them, matching `apply_archive_outcomes`.
    ///
    /// Routing happens HERE rather than at kick time: `handle_paste` decides by
    /// the focus and mode of the moment, and a read that took a second lands in a
    /// different moment than it started in. Deciding late is what makes the text
    /// go where the user is now, and it costs nothing — a middle click doesn't
    /// move focus (only the left button does), so in practice "now" and "then"
    /// are the same place.
    pub(crate) fn apply_clipboard_pastes(&mut self) -> (bool, Vec<Effect>) {
        let landed: Vec<std::io::Result<String>> = std::mem::take(
            &mut *self
                .runtime
                .clipboard_paste_results
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        if landed.is_empty() {
            return (false, Vec::new());
        }
        let mut effects = Vec::new();
        for read in landed {
            match read {
                Ok(text) if text.is_empty() => self.state.flash_info("paste: clipboard is empty"),
                // Reuse the paste path rather than sending bytes: routing,
                // bracketed-paste wrapping, and prompt handling are all already
                // decided there.
                Ok(text) => effects.extend(self.handle_paste(text)),
                Err(e) => self.state.flash_error(format!("paste: {e:#}")),
            }
        }
        (true, effects)
    }
}

#[cfg(test)]
mod delivery_tests {
    use super::clipboard_delivery;
    use crate::config::ClipboardVia as V;

    /// The headline: over SSH, `Auto` must NOT use the local helper. `pbcopy` on the
    /// server succeeds and puts the text somewhere the user can never paste from,
    /// which is worse than an error.
    #[test]
    fn auto_routes_to_the_client_terminal_over_ssh() {
        assert_eq!(clipboard_delivery(V::Auto, true), (false, true));
    }

    /// Locally, `Auto` stays on the helper: OSC 52 is unverifiable (no reply) and
    /// some terminals disable it, and there's no SSH problem to trade that for.
    #[test]
    fn auto_stays_local_without_ssh() {
        assert_eq!(clipboard_delivery(V::Auto, false), (true, false));
    }

    /// The explicit modes ignore SSH entirely — that's what makes them an override.
    #[test]
    fn explicit_modes_do_not_depend_on_ssh() {
        for ssh in [false, true] {
            assert_eq!(
                clipboard_delivery(V::System, ssh),
                (true, false),
                "ssh={ssh}"
            );
            assert_eq!(
                clipboard_delivery(V::Osc52, ssh),
                (false, true),
                "ssh={ssh}"
            );
            assert_eq!(clipboard_delivery(V::Both, ssh), (true, true), "ssh={ssh}");
        }
    }
}

#[cfg(test)]
mod paste_tests {
    use super::App;
    use std::time::{Duration, Instant};

    /// How long the stubbed clipboard "takes" — long enough that a read on the
    /// loop thread is unmistakable, short enough not to drag the suite.
    const SLOW_READ: Duration = Duration::from_millis(1500);

    /// Drain until the read lands, or give up. Returns the effects it produced.
    fn wait_for_paste(app: &mut App) -> Vec<super::Effect> {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let (dirty, fx) = app.apply_clipboard_pastes();
            if dirty {
                return fx;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the clipboard read never landed");
    }

    /// The headline: a middle-click paste must not stall the loop waiting on a
    /// clipboard helper. A wedged `xclip -o` (or a hung pasteboard server) held
    /// the whole UI — no dispatch, no frame, no key out of it — for as long as
    /// it took. The kick returns immediately; the text arrives through the slot.
    #[test]
    fn a_middle_click_paste_does_not_read_the_clipboard_on_the_loop() {
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            crate::clipboard::with_paste_stub(SLOW_READ, "pasted text", || {
                let start = Instant::now();
                app.spawn_clipboard_read();
                let kick = start.elapsed();
                assert!(
                    kick < SLOW_READ / 3,
                    "the kick waited {kick:?} on the clipboard"
                );
                // Nothing has been routed yet — the read is still running.
                assert!(!app.apply_clipboard_pastes().0);
                assert!(app.flash_text().is_none());

                wait_for_paste(&mut app);
                // File-list normal mode has nowhere to put a paste, and says so
                // with the length — which is how we know the *text* arrived and
                // not just the wake.
                assert_eq!(
                    app.flash_text(),
                    Some("paste ignored (11 chars) — open `:` or `^\\` to paste")
                );
            });
        });
    }

    /// An empty clipboard is a distinct answer, not a silent no-op — and it must
    /// still come back through the slot rather than short-circuit at kick time.
    #[test]
    fn an_empty_clipboard_reports_itself() {
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            crate::clipboard::with_paste_stub(Duration::ZERO, "", || {
                app.spawn_clipboard_read();
                let fx = wait_for_paste(&mut app);
                assert!(fx.is_empty());
                assert_eq!(app.flash_text(), Some("paste: clipboard is empty"));
            });
        });
    }

    /// Two clicks, two reads: the slot is a `Vec`, so a second read can't drop
    /// the first (a middle click is cheap to repeat, and they overlap).
    #[test]
    fn overlapping_reads_both_land() {
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::state::with_state_root(tmp.path(), || {
            let mut app = App::test_app(std::path::PathBuf::from("/tmp/harness"));
            crate::clipboard::with_paste_stub(Duration::from_millis(200), "ab", || {
                app.spawn_clipboard_read();
                app.spawn_clipboard_read();
                // Count what's in the slot, not how many drains saw something:
                // one drain takes everything that landed since the last.
                let deadline = Instant::now() + Duration::from_secs(20);
                loop {
                    let landed = app
                        .runtime
                        .clipboard_paste_results
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .len();
                    if landed == 2 {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "only {landed} of 2 reads landed — one clobbered the other"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert!(app.apply_clipboard_pastes().0);
            });
        });
    }
}

#[cfg(test)]
mod copy_tests {
    use super::App;
    use std::time::{Duration, Instant};

    /// A helper that reads the payload and then keeps running, the way
    /// `xclip`/`xsel` do to serve the X11 selection until another app claims it.
    #[cfg(unix)]
    fn persisting_helper(dir: &std::path::Path) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let stub = dir.join("stub-persist.sh");
        std::fs::write(&stub, "#!/bin/sh\ncat > /dev/null\nsleep 10\n").expect("write stub");
        let mut perms = std::fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&stub, perms).unwrap();
        stub
    }

    /// **A write that fails still reaches the user, after the fact.**
    ///
    /// Dispatching instead of awaiting means the verb's "yanked" flash goes up
    /// before the outcome is known. That is only honest if a failure arrives and
    /// **replaces** it — otherwise this trades a 150 ms hitch for a silent
    /// wrong answer, which is the defect #350 closed.
    #[cfg(unix)]
    #[test]
    fn a_failed_write_flashes_its_reason_after_the_confirmation() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::state::with_state_root(tmp.path(), || {
            let stub = tmp.path().join("stub-fail.sh");
            std::fs::write(&stub, "#!/bin/sh\ncat > /dev/null\nexit 3\n").expect("write stub");
            let mut perms = std::fs::metadata(&stub).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&stub, perms).unwrap();

            let mut app = App::test_app(tmp.path().to_path_buf());
            crate::clipboard::with_reap_budget(Duration::from_secs(10), || {
                crate::clipboard::with_clipboard_override(&stub, || {
                    app.deliver_clipboard("hello")
                        .expect("dispatch reports no error of its own");
                    // The verb's own confirmation is already on screen at this
                    // point; production flashes it from `run_effects`.
                    app.state.flash_info("yanked 1 line");

                    let deadline = Instant::now() + Duration::from_secs(20);
                    while Instant::now() < deadline {
                        if app.apply_clipboard_writes() {
                            let flash = app.flash_text().unwrap_or_default().to_string();
                            assert!(
                                flash.contains("clipboard:") && flash.contains("exited"),
                                "the failure must replace the confirmation, got {flash:?}"
                            );
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    panic!("a failed clipboard write never surfaced");
                });
            });
        });
    }

    /// **A yank must not stall the loop while a helper persists.**
    ///
    /// `xclip`/`xsel` legitimately stay alive after a successful copy, so
    /// `try_wait` never answers and the reap poll runs its whole budget — 150 ms
    /// of fully blocked event loop on *every single yank*, which on Linux is the
    /// common case rather than an edge one. `HELPER_REAP_BUDGET`'s own comment
    /// says the fix is moving the write off-thread.
    ///
    /// Measured, not structural: what matters is that dispatching a yank returns
    /// promptly, wherever the waiting ends up living.
    #[cfg(unix)]
    #[test]
    fn a_yank_does_not_wait_out_a_persisting_helper_on_the_loop() {
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::state::with_state_root(tmp.path(), || {
            let stub = persisting_helper(tmp.path());
            let app = App::test_app(tmp.path().to_path_buf());

            crate::clipboard::with_reap_budget(Duration::from_millis(150), || {
                crate::clipboard::with_clipboard_override(&stub, || {
                    let start = Instant::now();
                    app.deliver_clipboard("hello")
                        .expect("the yank is accepted");
                    let spent = start.elapsed();
                    assert!(
                        spent < Duration::from_millis(50),
                        "the yank blocked the loop for {spent:?}"
                    );

                    // …and the write still really happens, reported from wherever
                    // it ran. A dispatch that returns fast by doing nothing would
                    // pass the assertion above.
                    let deadline = Instant::now() + Duration::from_secs(20);
                    loop {
                        let landed = app
                            .runtime
                            .clipboard_copy_results
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .pop();
                        if let Some(outcome) = landed {
                            assert_eq!(
                                outcome, None,
                                "a helper that read the payload and persisted is a success"
                            );
                            return;
                        }
                        assert!(
                            Instant::now() < deadline,
                            "the clipboard write never reported back"
                        );
                        std::thread::sleep(Duration::from_millis(10));
                    }
                });
            });
        });
    }
}
