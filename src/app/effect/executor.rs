//! `run_effects`, the **sole** side-effect executor: every [`Effect`] a
//! handler returns is carried out here, and nowhere else touches the OS.
//! An `impl App` block in a descendant of `app`, so it reaches App's
//! private state and helpers the way `actions` / `key_dispatch` do.

use crate::Tui;
use crate::ui::pager::PagerView;

use super::{Effect, PaneTextKind, PaneTextSink};
use crate::app::{
    App, ForegroundExec, Message, PagerReturn, Wake, archive_ops, graveyard_ops, image_ops,
    inventory_ops, kill_pg, mermaid_ops, worktree_ops,
};

impl App {
    /// Execute a tick's worth of effects, in emission order. The **sole**
    /// side-effect executor for the run loop (MVU Phase 4).
    ///
    /// Borrow split: A-class effects (later slices) need `&mut self`;
    /// `ForegroundExec` needs `terminal` AND the loop-local
    /// `foreground_exec` (which owns the park/ack `Arc`s and is *not*
    /// reachable through `&mut self`), so `fg` is passed in. The three
    /// borrows are disjoint — `ForegroundExec::run` takes `&self`.
    pub(in crate::app) fn run_effects(
        &mut self,
        effects: Vec<Effect>,
        terminal: &mut Tui,
        fg: &ForegroundExec,
    ) {
        // `ForegroundExec` tears the TUI down, so it must be the sole or
        // last effect in a tick (the wider `Vec<Effect>` newly permits a
        // violation the single-`PostAction` return could not).
        debug_assert!(
            effects.iter().enumerate().all(|(i, e)| !matches!(
                e,
                Effect::ForegroundExec { .. }
            ) || i + 1 == effects.len()),
            "ForegroundExec must be the sole or last effect emitted in a tick"
        );

        for effect in effects {
            // Screen for archive members before executing: an op naming one is
            // held back until its bytes exist, and one that would write into a
            // container is refused. `run_effects` is the only route to the OS, so
            // this is the only place that has to know.
            let Some(effect) = self.screen_archive_effect(effect) else {
                continue;
            };
            match effect {
                // A-class: copy + flash synchronously, same tick. Never
                // `?`-propagate a clipboard error — a failed copy flashes
                // and the loop survives (unlike the former inline sites,
                // which were not in `?` scope, this arm must not abort the
                // run loop on a transient backend failure).
                Effect::CopyToClipboard { text, ok } => match self.deliver_clipboard(&text) {
                    Ok(()) => self.state.flash_info(ok.success(&text)),
                    // `{e:#}` even though `deliver_clipboard` returns a String: it is
                    // a no-op for Display, and the guard that requires it here is
                    // worth more than the one saved character. `deliver_clipboard`
                    // has already rendered each backend's own chain with `{e:#}`.
                    Err(e) => self.state.flash_error(format!("yank failed: {e:#}")),
                },
                // A-class: copy + flash the ACTIVE PAGER's title (not the status
                // bar), so a yank inside a pager confirms where the user is
                // looking — the former inline `view.flash` behaviour, now after a
                // copy that runs in the executor.
                Effect::CopyToPagerClipboard { text, ok_msg } => {
                    let msg = match self.deliver_clipboard(&text) {
                        Ok(()) => ok_msg,
                        Err(e) => format!("yank failed: {e}"),
                    };
                    self.set_active_pager_flash(msg);
                }
                // A-class: write the pager body to a timestamped file in the
                // process cwd, then flash the path/error in the active pager's
                // title (mirrors the former inline `save_to_file` + `view.flash`).
                Effect::SavePagerOutput { content } => {
                    let stamp = crate::sysinfo::format_now().replace([' ', ':'], "_");
                    let stamp = stamp.trim_end_matches("_UTC");
                    let filename = format!("spyc_output_{stamp}.txt");
                    let msg = match std::env::current_dir() {
                        Ok(dir) => {
                            let path = dir.join(&filename);
                            match std::fs::write(&path, &content) {
                                Ok(()) => format!("saved: {}", path.display()),
                                Err(e) => format!("save failed: {e}"),
                            }
                        }
                        Err(e) => format!("save failed: {e}"),
                    };
                    self.set_active_pager_flash(msg);
                }
                // Read the active pane's text from the live host (bounded,
                // yank-/gf-gated), then route per `then`. The no-pane guard +
                // the pane read moved here from the former yank / gf handlers.
                // The read (lines + the pane's cwd, used by GotoFile) happens
                // behind one borrow that ends before we flash / copy / navigate
                // — byte-identical flash strings, same `Event::Key` tick.
                Effect::ReadPaneText { kind, then } => {
                    let Some((mut lines, pane_cwd, agent)) =
                        self.runtime.pane_tabs.as_mut().map(|tabs| {
                            let lines = match kind {
                                PaneTextKind::Visible => tabs.active_mut().visible_lines(),
                                PaneTextKind::Scrollback(n) => tabs.active_mut().recent_lines(n),
                                PaneTextKind::Pickable(n) => tabs.active_mut().pickable_text(n),
                            };
                            let info = tabs.active_info();
                            (lines, info.cwd.clone(), crate::agent::detect(&info.command))
                        })
                    else {
                        if !matches!(then, PaneTextSink::JumpDefault) {
                            self.state.flash_error("no pane open");
                        }
                        continue;
                    };
                    match then {
                        // A-class: join + copy, byte-identical to the former
                        // inline yank sites (`yp`/`ya`).
                        PaneTextSink::Clipboard { ok } => {
                            let text = lines
                                .iter()
                                .map(|l| l.trim_end())
                                .collect::<Vec<_>>()
                                .join("\n");
                            if text.trim().is_empty() {
                                // Empty-flash string is per-kind, byte-identical
                                // to the former inline yank sites for the two
                                // kinds yank actually uses (Visible / Scrollback).
                                // Pickable only ever pairs with `GotoFile`, never
                                // Clipboard — its arm here is for exhaustiveness.
                                self.state.flash_error(match kind {
                                    PaneTextKind::Visible | PaneTextKind::Pickable(_) => {
                                        "pane is empty"
                                    }
                                    PaneTextKind::Scrollback(_) => "pane scrollback is empty",
                                });
                            } else {
                                match self.deliver_clipboard(&text) {
                                    Ok(()) => self.state.flash_info(ok.success(&text)),
                                    Err(e) => {
                                        self.state.flash_error(format!("yank failed: {e:#}"));
                                    }
                                }
                            }
                        }
                        // C-class: gf/gF. Resolve a path reference from the
                        // pickable lines and navigate to it (synchronous, same
                        // tick); gF also opens it in the pager at the line.
                        PaneTextSink::GotoFile { open_at_line } => {
                            lines.truncate(agent.output_len(&lines));
                            self.goto_file_navigate(lines, pane_cwd, open_at_line);
                        }
                        PaneTextSink::JumpDefault => {
                            lines.truncate(agent.output_len(&lines));
                            self.spawn_jump_default(lines, pane_cwd);
                        }
                    }
                }
                // A-class: queue input for the target pane (same tick).
                // Resolve the target; if it's gone, skip silently (matches
                // the former `if let Some(…)` guards). Errors flash their cause
                // without aborting the loop; rejected input changes no prompt state.
                Effect::SendToPane {
                    target,
                    input,
                    on_ok,
                    err_prefix,
                } => {
                    self.execute_pane_input(target, input, on_ok, err_prefix);
                }
                // A-class: queue input for the running capture child
                // (raw — captures rarely enable bracketed paste). A vanished
                // `pending_capture` skips silently, matching the former inline
                // `if let Some(capture)` write in the key/paste handlers.
                Effect::SendToCapture { bytes } => {
                    if let Some(capture) = self.runtime.pending_capture.as_mut()
                        && let Err(e) = capture.host.write_all(&bytes)
                    {
                        self.state.flash_error(format!("capture input: {e:#}"));
                    }
                }
                // A-class: the only side effect of a terminal-title update;
                // compose + dedup already happened loop-side.
                Effect::SetTerminalTitle { title } => {
                    let _ = crate::term_title::set(&title);
                }
                // Middle-click paste. The read spawns a helper and waits on it, so
                // it runs on a detached worker — never here; `spawn_clipboard_read`
                // documents why. `apply_clipboard_pastes` hands the text to
                // `handle_paste` when it lands.
                Effect::PasteFromClipboard => self.spawn_clipboard_read(),
                Effect::SetProcessCwd { dir } => {
                    // A failure (the dir vanished) leaves the settle seeing
                    // divergence, so it retries once the column moves on.
                    if let Err(e) = std::env::set_current_dir(&dir) {
                        crate::spyc_debug!("process cwd → {}: {e}", dir.display());
                    }
                }
                Effect::SetMouseMode { capture } => {
                    // Mutually exclusive with 1007 alternate-scroll: a terminal
                    // honouring both could deliver one wheel tick twice.
                    //
                    // `set_mouse_capture` records the new terminal state itself, and
                    // only on success — so a failed write leaves the reconcile
                    // seeing divergence and retrying, rather than going quiet on a
                    // state the terminal never reached.
                    if let Err(e) = crate::set_mouse_capture(terminal, capture) {
                        self.state
                            .flash_error(format!("mouse: could not set reporting: {e:#}"));
                    }
                }
                Effect::Notify { system, osc9, bell } => {
                    if let Some((summary, body)) = system {
                        crate::notifications::send(summary, body);
                    }
                    if let Some(msg) = osc9 {
                        crate::notifications::notify_osc9(&msg);
                    }
                    if bell {
                        crate::notifications::ring_bell();
                    }
                }
                // C-class: the chdir de-IO fork (MVU Phase 5). The blocking
                // listing read runs here in the executor — never in the pure
                // `apply()` transition that emitted us — then focus + flash via
                // the shared `change_dir`. Synchronous: it completes before the
                // next render, so the first post-action frame shows the new dir
                // (mark-jump / `..` / `gs` / `gp` / `gh` / prev-dir).
                Effect::ChangeDir {
                    path,
                    focus,
                    on_ok,
                    err_prefix,
                } => {
                    self.state
                        .change_dir(&path, focus.as_deref(), on_ok.as_deref(), err_prefix);
                    // The chdir may have moved the focused column into a
                    // different worktree → re-key its harpoon. This effect runs
                    // in the executor AFTER `apply`'s reconcile, so without this
                    // the swap would lag a frame (synchronous chdirs inside
                    // `apply_inner` are already covered by that reconcile).
                    self.reconcile_harpoon();
                }
                // A-class: signal the group, then (on success) toggle the
                // task's paused flag — re-found by id, same tick — and
                // flash. Like the clipboard arm this never `?`-propagates;
                // a failed signal flashes `on_err` and the loop survives.
                #[cfg(unix)]
                Effect::SignalGroup {
                    pid,
                    sig,
                    on_ok,
                    on_err,
                } => match kill_pg(pid, sig) {
                    Ok(()) => {
                        // Re-find the task by id (same tick, so still
                        // present) and toggle its paused flag, then flash.
                        if let Some(t) = self
                            .runtime
                            .background_tasks
                            .tasks
                            .iter_mut()
                            .find(|t| t.id == on_ok.task_id())
                        {
                            t.paused = on_ok.paused();
                        }
                        self.state.flash_info(on_ok.message());
                    }
                    Err(_) => self.state.flash_error(on_err),
                },
                // Pane `^z` toggle: SIGTSTP (suspend) / SIGCONT (resume) to the
                // pane's process group. The `suspended` flip + flash already ran
                // in the producer; only signal here (a failed kill is rare — we
                // just read a live pid — so flash and let the loop survive).
                #[cfg(unix)]
                Effect::SignalPane { pgrp, resume } => {
                    // SIGSTOP (not SIGTSTP): uncatchable, so Claude can't run
                    // its self-suspend handler — which on macOS ends in the
                    // false-exit. The agent just freezes; reader keeps blocking.
                    let sig = if resume {
                        rustix::process::Signal::CONT
                    } else {
                        rustix::process::Signal::STOP
                    };
                    if kill_pg(pgrp, sig).is_err() {
                        self.state.flash_error("pane: signal failed");
                    }
                }
                // Capture `^C`/`^\`: a real signal to the running command's
                // foreground group. A stale group (child already gone) fails
                // silently — the capture is finalizing on its own EOF anyway.
                #[cfg(unix)]
                Effect::SignalCapture { pgrp, sig } => {
                    let _ = kill_pg(pgrp, sig);
                }
                Effect::ForegroundExec {
                    program,
                    args,
                    pause_after,
                } => {
                    // Capture the result instead of `?`-propagating it: a
                    // failed spawn (missing/misspelled $EDITOR/$SHELL) used to
                    // bubble out of run_effects → App::run and exit spyc,
                    // dropping every pane PtyHost (SIGKILL on the agent
                    // children) without saving the session. `run` already
                    // restored the TUI on the spawn-error path, so here we just
                    // flash it (below) and carry on.
                    let fg_result = fg.run(terminal, &program, &args, pause_after);
                    // `suspend_tui` handed mouse reporting to the child; record
                    // that so the loop-bottom `settle_mouse_mode` re-enables it
                    // if the user wants it. `resume_tui` deliberately doesn't do
                    // this itself — it has no config access, and the settle is
                    // the single place that decides.
                    // --- after-work (moved verbatim from the run loop's
                    // former `if let PostAction::Spawn` call site) ---
                    // Runs regardless of the spawn result: the pager
                    // round-trip still needs unwinding (the temp file holds the
                    // pre-edit content when the editor never launched) and the
                    // listing refresh is harmless.
                    // Child may have clobbered our title; force a
                    // re-emit on next draw.
                    self.view.last_term_title = None;
                    // The listing may have changed (mv, rm, chmod, etc).
                    self.state.refresh_listing();
                    // If we were editing a pager buffer, restore it.
                    if let Some(ret) = self.view.pending_pager_return.take() {
                        match ret {
                            PagerReturn::TempFile {
                                path,
                                title,
                                scroll,
                                mount,
                                pane_scroll,
                            } => {
                                match std::fs::read_to_string(&path) {
                                    Ok(content) => {
                                        let lines: Vec<String> =
                                            content.lines().map(String::from).collect();
                                        let mut view = PagerView::new_plain(title, lines);
                                        view.scroll = scroll;
                                        view.saveable = true;
                                        view.mount = mount;
                                        view.pane_scroll = pane_scroll;
                                        // A scrollback-sourced edit returns to
                                        // its own bottom slot, not the top
                                        // `view.pager` (`set_pager`).
                                        if pane_scroll {
                                            self.restore_scroll_pager_view(view);
                                        } else {
                                            self.set_pager(view);
                                        }
                                        let _ = std::fs::remove_file(&path);
                                    }
                                    Err(e) => {
                                        // Reading the edited buffer back failed.
                                        // Do NOT delete the temp file — it holds
                                        // the user's edits; deleting it (the old
                                        // behaviour) silently discarded them. Tell
                                        // the user where to recover them.
                                        self.state.flash_error(format!(
                                            "couldn't read back edits ({e:#}); preserved at {}",
                                            path.display()
                                        ));
                                    }
                                }
                            }
                            PagerReturn::SourceFile {
                                path,
                                scroll,
                                mount,
                                pane_scroll,
                            } => {
                                // Reuse `build_pager_view_for_file` so a
                                // markdown file edited via `v` re-renders
                                // on return. Reported by JRob: open a .md
                                // (rendered), `v` to edit, quit $EDITOR —
                                // file came back as plain text with no
                                // `m`-toggle (the inline rebuild here used
                                // `PagerView::new_plain` and skipped the
                                // markdown / alt_lines branch entirely).
                                if let Some(mut view) = self.build_pager_view_for_file(&path, None)
                                {
                                    // Override the position restored from
                                    // the per-file cache with the scroll
                                    // we explicitly stashed before
                                    // launching $EDITOR — it's the more
                                    // recent intent for this round-trip.
                                    view.scroll = scroll;
                                    view.mount = mount;
                                    view.pane_scroll = pane_scroll;
                                    if pane_scroll {
                                        self.restore_scroll_pager_view(view);
                                    } else {
                                        self.set_pager(view);
                                    }
                                }
                            }
                        }
                    }
                    // Flash a failed spawn last (so it's the message left on
                    // screen) — never fatal. The TUI and pager were already
                    // restored above.
                    if let Err(e) = fg_result {
                        self.state.flash_error(format!("{program}: {e:#}"));
                    }
                }
                // Tier 5: run the tar+zstd / trash IO on a detached worker —
                // never the event loop. The worker pushes its outcome onto the
                // shared slot and wakes the loop; `apply_graveyard_outcomes`
                // (pre-recv scan) does the flash + refresh. `pane_wake_tx` is
                // `None` only before `run()` / in the test harness, where there
                // is no loop to wake — the outcome still lands in the slot.
                Effect::Graveyard(op) => {
                    let results = std::sync::Arc::clone(&self.runtime.graveyard_results);
                    let wake = self.runtime.pane_wake_tx.clone();
                    std::thread::spawn(move || {
                        let outcome = graveyard_ops::run_graveyard_op(op);
                        results.lock().unwrap().push(outcome);
                        if let Some(tx) = wake {
                            let _ = tx.send(Message::Wake(Wake::Graveyard));
                        }
                    });
                }
                // Archive mount / materialize / staging cleanup — same detached
                // shape as Graveyard. A streamed mount can run for seconds, so
                // it also gets the shared cancel flag (`Esc` sets it).
                Effect::Archive(op) => {
                    let results = std::sync::Arc::clone(&self.runtime.archive_results);
                    let wake = self.runtime.pane_wake_tx.clone();
                    std::thread::spawn(move || {
                        let outcome = archive_ops::run_archive_op(op);
                        results.lock().unwrap().push(outcome);
                        if let Some(tx) = wake {
                            let _ = tx.send(Message::Wake(Wake::Archive));
                        }
                    });
                }
                // Render the mermaid diagram + open it externally on a detached
                // worker (same shape as Graveyard); the outcome lands in
                // `image_results` and `apply_image_outcomes` installs/flashes it.
                Effect::RenderMermaid(op) => {
                    let results = std::sync::Arc::clone(&self.runtime.image_results);
                    let wake = self.runtime.pane_wake_tx.clone();
                    // The picker (graphics-protocol capability, detected once at
                    // startup) lives in Runtime; inject it so the View mode can
                    // build a Protocol off-thread. `None` ⇒ no graphics protocol.
                    let picker = self.runtime.picker.clone();
                    std::thread::spawn(move || {
                        let outcome = mermaid_ops::render_mermaid_op(op, picker);
                        results.lock().unwrap().push(outcome);
                        if let Some(tx) = wake {
                            let _ = tx.send(Message::Wake(Wake::Image));
                        }
                    });
                }
                // Build an overlay from bytes already in hand.
                Effect::ShowImageBytes(op) => {
                    let results = std::sync::Arc::clone(&self.runtime.image_results);
                    let wake = self.runtime.pane_wake_tx.clone();
                    let picker = self.runtime.picker.clone();
                    std::thread::spawn(move || {
                        let outcome = image_ops::show_image_bytes(op, picker);
                        results.lock().unwrap().push(outcome);
                        if let Some(tx) = wake {
                            let _ = tx.send(Message::Wake(Wake::Image));
                        }
                    });
                }
                // Grab the clipboard image off-thread as the user pastes.
                Effect::CaptureClipboardImage(op) => {
                    let results = std::sync::Arc::clone(&self.runtime.image_results);
                    let wake = self.runtime.pane_wake_tx.clone();
                    std::thread::spawn(move || {
                        let outcome = image_ops::capture_clipboard_image(op);
                        results.lock().unwrap().push(outcome);
                        if let Some(tx) = wake {
                            let _ = tx.send(Message::Wake(Wake::Image));
                        }
                    });
                }
                // Index an agent transcript's images off-thread; same slot and
                // wake as every other image producer.
                Effect::IndexTranscriptImages(op) => {
                    let results = std::sync::Arc::clone(&self.runtime.image_results);
                    let wake = self.runtime.pane_wake_tx.clone();
                    std::thread::spawn(move || {
                        let outcome = image_ops::index_transcript(op);
                        results.lock().unwrap().push(outcome);
                        if let Some(tx) = wake {
                            let _ = tx.send(Message::Wake(Wake::Image));
                        }
                    });
                }
                // Re-read + decode one indexed transcript image off-thread.
                Effect::OpenTranscriptImage(op) => {
                    let results = std::sync::Arc::clone(&self.runtime.image_results);
                    let wake = self.runtime.pane_wake_tx.clone();
                    let picker = self.runtime.picker.clone();
                    std::thread::spawn(move || {
                        let outcome = image_ops::open_transcript_image(op, picker);
                        results.lock().unwrap().push(outcome);
                        if let Some(tx) = wake {
                            let _ = tx.send(Message::Wake(Wake::Image));
                        }
                    });
                }
                // Read + decode an image file off-thread; same slot and wake as
                // the mermaid render above.
                Effect::OpenImage(op) => {
                    let results = std::sync::Arc::clone(&self.runtime.image_results);
                    let wake = self.runtime.pane_wake_tx.clone();
                    let picker = self.runtime.picker.clone();
                    std::thread::spawn(move || {
                        let outcome = image_ops::open_image_file(op, picker);
                        results.lock().unwrap().push(outcome);
                        if let Some(tx) = wake {
                            let _ = tx.send(Message::Wake(Wake::Image));
                        }
                    });
                }
                // The single spawn site lives in `file_ops` (shared with the gF
                // executor open); this arm just hands it the op.
                Effect::FileOp(op) => self.spawn_file_op(op),
                // Interactive `W n`: hand the create job to the shared worktree
                // worker with an interactive completion (chdir into the new tree
                // when it lands, not an MCP reply).
                Effect::WorktreeCreateInteractive { dir, branch, base } => self.spawn_worktree_job(
                    worktree_ops::WorktreeJob::Create {
                        dir,
                        branch,
                        base,
                        open: false,
                    },
                    worktree_ops::WorktreeCompletion::InteractiveCreate,
                ),
                Effect::Inventory(op) => {
                    let results = std::sync::Arc::clone(&self.runtime.inventory_results);
                    let wake = self.runtime.pane_wake_tx.clone();
                    std::thread::spawn(move || {
                        let outcome = inventory_ops::run_inventory_op(op);
                        results.lock().unwrap().push(outcome);
                        if let Some(tx) = wake {
                            let _ = tx.send(Message::Wake(Wake::Inventory));
                        }
                    });
                }
            }
        }
    }
}
