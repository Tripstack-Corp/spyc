//! Pane input execution and tracking of input accepted by the writer queue.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::effect::clear_blocked_for_input;
use super::{App, Effect, Mode, PaneInput, PaneTarget, PromptKind};

impl App {
    /// The `SendToPane` executor; child input always passes through `PtyHost`.
    pub(super) fn execute_pane_input(
        &mut self,
        target: PaneTarget,
        mut input: PaneInput,
        on_ok: Option<String>,
        err_prefix: Option<&'static str>,
    ) {
        let result = match target {
            PaneTarget::Active => self.runtime.pane_tabs.as_mut().map(|tabs| {
                if let PaneInput::ConfirmedPipe { tab_id, .. } = &input
                    && *tab_id != tabs.active_info().id
                {
                    return Err(anyhow::anyhow!("pipe recipient changed; nothing was sent"));
                }
                let result = input.send_to(tabs.active_mut());
                if result.is_ok() {
                    let info = tabs.active_info_mut();
                    let recovery = self.state.codex_recovery.get_mut(&info.id);
                    clear_blocked_for_input(info, &input, recovery);
                }
                result
            }),
            PaneTarget::Overlay => {
                let slot = if self.focused_side() == super::state::Side::Right
                    && self.runtime.top_overlay_right.is_some()
                {
                    self.runtime.top_overlay_right.as_mut()
                } else {
                    self.runtime.top_overlay.as_mut()
                };
                slot.map(|overlay| input.send_to(overlay))
            }
        };
        match result {
            Some(Ok(())) => {
                if matches!(target, PaneTarget::Active) {
                    if self.view.show_activity {
                        self.view.pane_send_at = Some(std::time::Instant::now());
                    }
                    self.track_accepted_pane_input(&input);
                }
                if let Some(message) = on_ok {
                    self.state.flash_info(message);
                }
            }
            Some(Err(error)) => {
                self.state
                    .flash_error(format!("{}: {error:#}", err_prefix.unwrap_or("pane input")));
            }
            None => {}
        }
    }

    fn track_accepted_pane_input(&mut self, input: &PaneInput) {
        match input {
            PaneInput::Paste { text, .. } => self.state.pane.pane_prompt_buf.push_str(text),
            PaneInput::Key(key) => match key.code {
                KeyCode::Enter => {
                    let trimmed = super::util::strip_ansi_escapes(&self.state.pane.pane_prompt_buf);
                    if !trimmed.is_empty() {
                        self.state.pane.last_pane_prompt = Some(trimmed);
                    }
                    self.state.pane.pane_prompt_buf.clear();
                    self.clear_pending_images_for_active_tab();
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.state.pane.pane_prompt_buf.clear();
                }
                KeyCode::Backspace => {
                    self.state.pane.pane_prompt_buf.pop();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.state.pane.pane_prompt_buf.push(c);
                }
                _ => {}
            },
            _ => {}
        }
    }

    pub(super) fn handle_pipe_confirm_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let Mode::Prompting(prompt) = std::mem::replace(&mut self.state.mode, Mode::Normal) else {
            return Vec::new();
        };
        let PromptKind::PipeConfirm {
            payload,
            tab_id,
            on_ok,
        } = prompt.kind
        else {
            return Vec::new();
        };
        if !matches!(key.code, KeyCode::Char('y' | 'Y'))
            || !key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
        {
            self.state.flash_info("pipe cancelled; nothing was sent");
            return Vec::new();
        }
        vec![Effect::SendToPane {
            target: PaneTarget::Active,
            input: PaneInput::ConfirmedPipe {
                bytes: payload,
                tab_id,
            },
            on_ok: Some(on_ok),
            err_prefix: Some("pipe failed"),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::file_ops::FileOutcome;
    use std::time::{Duration, Instant};

    fn test_pane(dir: &std::path::Path) -> App {
        let mut app = App::test_app(dir.to_path_buf());
        assert!(app.open_pane_tab_in("cat", dir));
        app
    }

    fn apply_pipe(app: &mut App, payload: Vec<u8>) -> Vec<Effect> {
        app.runtime
            .file_results
            .lock()
            .unwrap()
            .push(FileOutcome::PipedContent {
                payload,
                count: 1,
                skipped: 0,
            });
        app.apply_file_outcomes().1
    }

    fn execute(app: &mut App, effects: Vec<Effect>) {
        for effect in effects {
            let Effect::SendToPane {
                target,
                input,
                on_ok,
                err_prefix,
            } = effect
            else {
                panic!("expected input effect");
            };
            app.execute_pane_input(target, input, on_ok, err_prefix);
        }
    }

    #[test]
    fn large_file_pipes_require_confirmation_and_cancel_sends_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = test_pane(tmp.path());
            for key in [KeyCode::Enter, KeyCode::Esc, KeyCode::Char('n')] {
                assert!(
                    apply_pipe(
                        &mut app,
                        vec![b'x'; crate::pane::pty_host::MAX_INPUT_BYTES + 1]
                    )
                    .is_empty()
                );
                assert!(
                    matches!(&app.state.mode, Mode::Prompting(p) if matches!(p.kind, PromptKind::PipeConfirm { .. }) && p.prefix.contains("MiB"))
                );
                let effects = app
                    .handle_key(KeyEvent::new(key, KeyModifiers::NONE))
                    .unwrap();
                assert!(effects.is_empty());
                assert_eq!(app.flash_text(), Some("pipe cancelled; nothing was sent"));
            }
        });
    }

    #[test]
    fn confirmed_pipe_refuses_a_changed_recipient() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = test_pane(tmp.path());
            assert!(
                apply_pipe(
                    &mut app,
                    vec![b'x'; crate::pane::pty_host::MAX_INPUT_BYTES + 1]
                )
                .is_empty()
            );
            let effects = app
                .handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE))
                .unwrap();
            // Another MCP caller can change the active recipient despite a modal prompt.
            assert!(app.open_pane_tab_in("cat", tmp.path()));
            execute(&mut app, effects);
            assert!(app.flash_text().unwrap().contains("recipient changed"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn a_non_reading_real_pty_keeps_input_responsive_and_rejection_keeps_attention() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = crate::app::App::test_app(tmp.path().to_path_buf());
            assert!(app.open_pane_tab_in(
                "/bin/sh -c 'stty raw -echo; printf NON_READING_READY; exec sleep 30'",
                tmp.path()
            ));
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let pane = app.runtime.pane_tabs.as_mut().unwrap().active_mut();
                pane.drain_output();
                if pane
                    .visible_lines()
                    .iter()
                    .any(|line| line.contains("NON_READING_READY"))
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "test child never entered raw mode"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            let pid = app
                .runtime
                .pane_tabs
                .as_ref()
                .unwrap()
                .active()
                .process_id()
                .unwrap();
            let (cancel, watchdog) = std::sync::mpsc::channel::<()>();
            let killer = std::thread::spawn(move || {
                // A synchronous-write regression must fail within a deadline,
                // not hang CI. This PID belongs only to this disposable child.
                if watchdog.recv_timeout(Duration::from_secs(3)).is_err() {
                    let _ = rustix::process::kill_process(
                        rustix::process::Pid::from_raw(pid as i32).unwrap(),
                        rustix::process::Signal::KILL,
                    );
                }
            });
            let at = Instant::now();
            app.execute_pane_input(
                PaneTarget::Active,
                PaneInput::Bytes(vec![b'x'; crate::pane::pty_host::MAX_INPUT_BYTES]),
                None,
                None,
            );
            let elapsed = at.elapsed();
            assert!(
                elapsed < Duration::from_millis(500),
                "PTY input waited for child: {elapsed:?}"
            );
            let now = Instant::now();
            {
                let info = app.runtime.pane_tabs.as_mut().unwrap().active_info_mut();
                info.command = "codex".into();
                info.reported = Some(crate::pane::ReportedStatus {
                    status: crate::pane::AgentActivity::Blocked,
                    at: now,
                    expiry: now + Duration::from_secs(60),
                });
            }
            app.runtime
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut()
                .reported
                .as_mut()
                .unwrap()
                .status = crate::pane::AgentActivity::Working;
            app.execute_pane_input(
                PaneTarget::Active,
                PaneInput::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
                None,
                None,
            );
            assert_eq!(
                app.runtime
                    .pane_tabs
                    .as_ref()
                    .unwrap()
                    .active_info()
                    .reported
                    .unwrap()
                    .status,
                crate::pane::AgentActivity::Working,
                "rejected interrupt input cannot retire a working report"
            );
            assert!(app.flash_text().unwrap().contains("queue full"));
            app.runtime
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut()
                .reported
                .as_mut()
                .unwrap()
                .status = crate::pane::AgentActivity::Blocked;
            app.state.pane.pane_prompt_buf = "accepted".into();
            app.set_pane_focus(true);
            let paste = app.handle_paste("rejected paste".into());
            execute(&mut app, paste);
            assert_eq!(app.state.pane.pane_prompt_buf, "accepted");
            assert!(app.flash_text().unwrap().contains("queue full"));
            let enter = app
                .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .unwrap();
            execute(&mut app, enter);
            assert_eq!(
                app.runtime
                    .pane_tabs
                    .as_ref()
                    .unwrap()
                    .active_info()
                    .reported
                    .unwrap()
                    .status,
                crate::pane::AgentActivity::Blocked
            );
            assert_eq!(app.state.pane.pane_prompt_buf, "accepted");
            assert!(app.state.pane.last_pane_prompt.is_none());
            let _ = cancel.send(());
            killer.join().unwrap();
            assert!(
                elapsed < Duration::from_millis(500),
                "PTY input waited for child: {elapsed:?}"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn a_confirmed_file_over_the_limit_arrives_complete_with_its_paste_envelope() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let source = tmp.path().join("large.txt");
            let received = tmp.path().join("received.txt");
            std::fs::write(
                &source,
                vec![b'x'; crate::pane::pty_host::MAX_INPUT_BYTES + 1],
            )
            .unwrap();
            let script = tmp.path().join("reader.sh");
            std::fs::write(
                &script,
                format!(
                    "stty raw -echo\nprintf READER_READY\nexec cat > '{}'\n",
                    received.display()
                ),
            )
            .unwrap();
            let mut app = App::test_app(tmp.path().to_path_buf());
            assert!(app.open_pane_tab_in(&format!("/bin/sh '{}'", script.display()), tmp.path()));
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let pane = app.runtime.pane_tabs.as_mut().unwrap().active_mut();
                pane.drain_output();
                if pane
                    .visible_lines()
                    .iter()
                    .any(|line| line.contains("READER_READY"))
                {
                    break;
                }
                assert!(Instant::now() < deadline, "test reader never started");
                std::thread::sleep(Duration::from_millis(10));
            }
            let outcome =
                crate::app::file_ops::run_file_op(crate::app::file_ops::FileOp::PipeContent {
                    use_inventory: false,
                    inventory_ids: vec![],
                    paths: vec![source],
                });
            let FileOutcome::PipedContent { payload, .. } = &outcome else {
                panic!("pipe outcome");
            };
            let expected = payload.clone();
            app.runtime.file_results.lock().unwrap().push(outcome);
            assert!(app.apply_file_outcomes().1.is_empty());
            assert!(!received.exists() || std::fs::metadata(&received).unwrap().len() == 0);
            let effects = app
                .handle_key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::SHIFT))
                .unwrap();
            execute(&mut app, effects);
            assert_eq!(app.flash_text(), Some("piped 1 file(s) to pane"));
            let deadline = Instant::now() + Duration::from_secs(10);
            while std::fs::metadata(&received).map_or(0, |m| m.len()) < expected.len() as u64 {
                assert!(
                    Instant::now() < deadline,
                    "large pipe did not arrive completely"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(std::fs::read(received).unwrap(), expected);
            assert!(expected.starts_with(b"\x1b[200~[file: "));
            assert!(expected.ends_with(b"\x1b[201~"));
        });
    }

    #[test]
    fn accepted_input_commits_prompt_replay_only_after_delivery() {
        let tmp = tempfile::tempdir().unwrap();
        crate::state::with_state_root(tmp.path(), || {
            let mut app = test_pane(tmp.path());
            app.set_pane_focus(true);
            let paste = app.handle_paste("hello".into());
            assert!(app.state.pane.pane_prompt_buf.is_empty());
            execute(&mut app, paste);
            assert_eq!(app.state.pane.pane_prompt_buf, "hello");
            let enter = app
                .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .unwrap();
            assert_eq!(app.state.pane.pane_prompt_buf, "hello");
            execute(&mut app, enter);
            assert!(app.state.pane.pane_prompt_buf.is_empty());
            assert_eq!(app.state.pane.last_pane_prompt.as_deref(), Some("hello"));
        });
    }

    #[test]
    fn send_effect_uses_the_tested_input_executor() {
        // Pin the production seam exercised by the real-PTY regression.
        let source = crate::guard_support::production_half(include_str!("effect/executor.rs"));
        assert!(source.contains("self.execute_pane_input(target, input, on_ok, err_prefix)"));
    }
}
