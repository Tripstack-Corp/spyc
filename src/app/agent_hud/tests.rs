//! Real pane geometry/input and application consumer lifecycle, with synthetic children.

use super::*;
use crate::agent::codex_metrics::{self, Availability, Mode, Sample, Snapshot, Source, Tokens};
use crate::app::{ForegroundExec, state};
use crate::pane::{
    AgentActivity, Pane, PaneTabs,
    engine::TerminalScreen,
    tabs::{TabEntry, TabInfo},
};
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::{CrosstermBackend, TestBackend},
};
use serde_json::json;
use std::{
    io::Write,
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

const THREAD: &str = "01a133b2-dc16-74c8-af3c-123456789abc";

fn entry(app: &mut App, cwd: &Path, home: &Path, command: &str) -> TabEntry {
    let wake = app.make_pane_wake();
    let pane = Pane::spawn(command, 24, 100, cwd, &app.view.context_path, wake).unwrap();
    let mut info = TabInfo::new("codex", cwd);
    info.codex_home = Some(home.to_owned());
    info.codex_session_id = Some(THREAD.to_owned());
    info.spawn_epoch_secs = 0;
    info.activity = AgentActivity::Idle;
    TabEntry::new(pane, info)
}

fn fixture(cwd: &Path, home: &Path) -> App {
    let mut app = App::test_app(std::env::current_dir().unwrap());
    app.view.term_size = (100, 32);
    app.state.config.mouse.capture = false;
    app.state.pane.pane_height_pct = 50;
    app.runtime.pane_tabs = Some(PaneTabs::new(entry(&mut app, cwd, home, "cat")));
    app
}

fn rollout(home: &Path, total: u64) -> std::path::PathBuf {
    let day = home.join("sessions/2026/10/13");
    std::fs::create_dir_all(&day).unwrap();
    let path = day.join(format!("rollout-2026-10-13T12-00-00-{THREAD}.jsonl"));
    let header = json!({"type":"session_meta","payload":{"id":THREAD}});
    std::fs::write(&path, format!("{header}\n")).unwrap();
    append_usage(&path, total, "2026-10-13T12:00:00Z");
    path
}

fn append_usage(path: &Path, total: u64, stamp: &str) {
    let record = json!({"type":"token_usage_record","timestamp":stamp,"payload":{"thread_id":THREAD,"session_id":THREAD,"turn_id":"turn-a","usage":{"total_tokens":total}}});
    writeln!(
        std::fs::OpenOptions::new().append(true).open(path).unwrap(),
        "{record}"
    )
    .unwrap();
}

fn terminal_and_foreground() -> (crate::Tui, ForegroundExec) {
    let terminal = Terminal::with_options(
        CrosstermBackend::new(std::io::stdout()),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 100, 32)),
        },
    )
    .unwrap();
    let fg = ForegroundExec {
        park: std::sync::Arc::default(),
        acked: std::sync::Arc::default(),
        reader_done: std::sync::Arc::default(),
        reader: std::thread::current(),
    };
    (terminal, fg)
}

fn tick(app: &mut App, ctx: &mut RunCtx) {
    let (mut terminal, fg) = terminal_and_foreground();
    app.drain_landed_work(ctx, &mut terminal, &fg);
    app.settle_before_recv(ctx, &mut terminal, &fg);
}

fn codex_request(update: &Update) -> Option<u64> {
    match update {
        Update::Codex(u) => u.snapshot.request.as_ref()?.value.total,
        Update::Claude(_) | Update::Agy(_) => None,
    }
}

fn wait_metrics(app: &mut App, ctx: &mut RunCtx, expected: u64) {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        tick(app, ctx);
        if app.active_agent_hud_update().and_then(codex_request) == Some(expected) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("HUD did not land request {expected}");
}

fn paint(app: &mut App, width: u16, height: u16) -> (crate::app::FrameLayout, Vec<String>) {
    app.view.term_size = (width, height);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let layout = app.frame_layout(Rect::new(0, 0, width, height));
    let rows = layout.agent_hud.map_or_else(Vec::new, |rect| {
        (rect.y..rect.bottom())
            .map(|y| {
                (rect.x..rect.right())
                    .map(|x| terminal.backend().buffer().cell((x, y)).unwrap().symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    });
    (layout, rows)
}

fn wait_screen(app: &mut App, text: &str) {
    let end = Instant::now() + Duration::from_secs(10);
    while !app
        .runtime
        .pane_tabs
        .as_ref()
        .unwrap()
        .active()
        .with_screen(|s| s.contents().contains(text))
    {
        app.drain_pane_output();
        assert!(Instant::now() < end, "child never painted {text}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn agent_hud_real_loop_rebinds_same_cwd_homes_and_stops_without_a_consumer() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let a = tmp.path().join("home-a");
        let b = tmp.path().join("home-b");
        rollout(&a, 11);
        let path_b = rollout(&b, 99);
        let mut app = fixture(tmp.path(), &a);
        let mut ctx = RunCtx::for_test();
        tick(&mut app, &mut ctx);
        assert!(
            app.runtime.agent_hud.is_none(),
            "default off must not create a collector"
        );
        app.cmd_agent_hud("compact");
        tick(&mut app, &mut ctx);
        assert!(
            app.runtime.agent_hud.is_some(),
            "visible HUD must start collection through the executor"
        );
        wait_metrics(&mut app, &mut ctx, 11);
        let old = app.state.pane.agent_hud.update.clone().unwrap();
        let second = entry(&mut app, tmp.path(), &b, "cat");
        app.runtime.pane_tabs.as_mut().unwrap().push(second);
        assert!(
            app.active_agent_hud_update().is_none(),
            "tab switch must not paint the prior home"
        );
        wait_metrics(&mut app, &mut ctx, 99);
        let now = Timestamp::now();
        assert!(!app.land_agent_hud_update(old, (0, 0), now));
        assert_eq!(
            app.active_agent_hud_update().and_then(codex_request),
            Some(99)
        );
        append_usage(&path_b, 77, "2026-10-13T12:00:01Z");
        wait_metrics(&mut app, &mut ctx, 77);
        for disabled in 0..4 {
            match disabled {
                0 => app.state.pane.pane_hidden = true,
                1 => app.state.pane.zoom = state::ZoomTarget::TopList,
                2 => app.view.term_size = (100, 12),
                _ => {
                    app.runtime
                        .pane_tabs
                        .as_mut()
                        .unwrap()
                        .active_info_mut()
                        .agent_hint = Some(AgentKind::Agy);
                }
            }
            tick(&mut app, &mut ctx);
            assert!(app.runtime.agent_hud.is_none() && app.state.pane.agent_hud.update.is_none());
            app.state.pane.pane_hidden = false;
            app.state.pane.zoom = state::ZoomTarget::None;
            app.view.term_size = (100, 32);
            app.runtime
                .pane_tabs
                .as_mut()
                .unwrap()
                .active_info_mut()
                .agent_hint = None;
            wait_metrics(&mut app, &mut ctx, 77);
        }
        let protocol = crate::app::image_ops::raster_to_protocol(
            image::DynamicImage::new_rgba8(1, 1),
            &ratatui_image::picker::Picker::halfblocks(),
            1,
            1,
        )
        .unwrap();
        app.view.image_view = Some(crate::app::ImageView {
            protocol,
            encoded: Vec::new(),
            ext: "png",
            dims: (1, 1),
            origin: crate::app::image_ops::ImageOrigin::Pending { seq: 0 },
            dark: false,
            flash: None,
        });
        tick(&mut app, &mut ctx);
        assert!(app.runtime.agent_hud.is_none() && app.agent_hud_target().is_none());
        app.view.image_view = None;
        wait_metrics(&mut app, &mut ctx, 77);
        let binding = app.state.pane.agent_hud.binding.clone().unwrap();
        app.cmd_agent_hud("off");
        tick(&mut app, &mut ctx);
        assert!(app.runtime.agent_hud.is_none());
        // A delayed start effect cannot resurrect a cancelled incarnation.
        let (mut terminal, fg) = terminal_and_foreground();
        app.run_effects(vec![Effect::AgentHud(Some(binding))], &mut terminal, &fg);
        assert!(app.runtime.agent_hud.is_none());
        app.cmd_agent_hud("full");
        wait_metrics(&mut app, &mut ctx, 77);
        let closing = entry(&mut app, tmp.path(), &b, "true");
        app.runtime.pane_tabs.as_mut().unwrap().push(closing);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !app.runtime.pane_tabs.as_ref().unwrap().active().is_closed() {
            app.drain_pane_output();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        tick(&mut app, &mut ctx);
        assert!(
            app.runtime.agent_hud.is_none() && app.agent_hud_target().is_none(),
            "closed panes cannot retain a HUD consumer"
        );
        app.runtime.pane_tabs = None;
        tick(&mut app, &mut ctx);
        assert!(app.runtime.agent_hud.is_none() && app.state.pane.agent_hud.binding.is_none());
    });
}

#[test]
fn agent_hud_real_frame_resizes_child_and_preserves_chrome_across_modes() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = fixture(tmp.path(), tmp.path());
        app.state.config.pane.agent_hud = AgentHudMode::Compact;
        assert_eq!(paint(&mut app, 100, 32).1.len(), 2);
        app.state.config.pane.agent_hud = AgentHudMode::Full;
        assert_eq!(paint(&mut app, 100, 32).1.len(), 4);
        for status in [
            crate::config::StatusPosition::Top,
            crate::config::StatusPosition::Bottom,
        ] {
            app.state.config.layout.status_position = status;
            for zoom in [
                state::ZoomTarget::None,
                state::ZoomTarget::BottomPane,
                state::ZoomTarget::TopList,
            ] {
                app.state.pane.zoom = zoom;
                for split in [
                    None,
                    Some(state::VSplit {
                        width_pct: 50,
                        mode: state::VsplitMode::FullHeight,
                        focus: state::Side::Left,
                    }),
                    Some(state::VSplit {
                        width_pct: 50,
                        mode: state::VsplitMode::TopOnly,
                        focus: state::Side::Left,
                    }),
                ] {
                    app.state.vsplit = split;
                    for (width, height) in [(100, 32), (40, 24), (20, 16), (100, 12)] {
                        app.state.config.pane.agent_hud = AgentHudMode::Off;
                        let (off, _) = paint(&mut app, width, height);
                        for mode in [AgentHudMode::Compact, AgentHudMode::Full] {
                            app.state.config.pane.agent_hud = mode;
                            let (on, rows) = paint(&mut app, width, height);
                            assert_eq!(
                                (
                                    on.status,
                                    on.list,
                                    on.prompt,
                                    on.divider,
                                    on.right,
                                    on.vdivider
                                ),
                                (
                                    off.status,
                                    off.list,
                                    off.prompt,
                                    off.divider,
                                    off.right,
                                    off.vdivider
                                )
                            );
                            if let Some(strip) = on.agent_hud {
                                let child = on.pane.unwrap();
                                assert!(child.height >= 6);
                                assert_eq!(child.bottom(), strip.y);
                                assert_eq!(child.height + strip.height, off.pane.unwrap().height);
                                assert_eq!(
                                    app.runtime
                                        .pane_tabs
                                        .as_ref()
                                        .unwrap()
                                        .active()
                                        .with_screen(TerminalScreen::size),
                                    (child.height, child.width)
                                );
                                assert!(rows[0].contains("56789abc") || width < 22);
                                if strip.width >= 40 {
                                    assert!(rows[1].starts_with("│ context"));
                                    assert!(rows[1].ends_with("awaiting metrics"));
                                }
                            } else {
                                assert_eq!(on.pane, off.pane);
                            }
                        }
                    }
                }
            }
        }
        app.state.pane.zoom = state::ZoomTarget::BottomPane;
        app.state.vsplit = None;
        app.view.theme.mono = true;
        app.state.config.pane.agent_hud = AgentHudMode::Full;
        let mut pager =
            crate::ui::pager::PagerView::new_plain("scroll", vec!["native history".to_owned()]);
        pager.mount = crate::ui::pager::Mount::LowerPane;
        app.view.scroll_pager = Some(pager);
        let (layout, rows) = paint(&mut app, 100, 32);
        assert_eq!(rows.len(), 4);
        assert!(rows[3].starts_with("│ mode") && rows[3].contains("hooks off"));
        assert!(rows[2].starts_with("│ recent") && rows[2].ends_with("--"));
        assert!(
            app.agent_hud_lines(layout.agent_hud.unwrap())
                .iter()
                .flat_map(|l| &l.spans)
                .all(|s| s.style.fg == Some(ratatui::style::Color::Reset))
        );
        app.view.pager = Some(crate::ui::pager::PagerView::new_plain(
            "top",
            vec!["top overlay".to_owned()],
        ));
        assert_eq!(
            paint(&mut app, 100, 32).1.len(),
            4,
            "top pager must retain the strip below"
        );
    });
}

#[test]
fn agent_hud_only_changed_visible_fields_request_a_repaint() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = fixture(tmp.path(), tmp.path());
        app.state.config.pane.agent_hud = AgentHudMode::Compact;
        let mut ctx = RunCtx::for_test();
        app.settle_agent_hud(Instant::now(), &mut ctx);
        let Some(Binding::Codex(binding)) = app.state.pane.agent_hud.binding.clone() else {
            panic!("a Codex tab binds a Codex consumer");
        };
        let at = "2026-10-13T12:00:00Z".parse().unwrap();
        let mut update = codex_metrics::Update {
            binding,
            availability: Availability::Available,
            tail_only: false,
            snapshot: Snapshot::default(),
        };
        assert!(app.land_agent_hud_update(Update::Codex(Box::new(update.clone())), (1, 1), at));
        update.snapshot.model_context_window = Some(Sample {
            at,
            source: Source::LegacyTokenCount,
            value: 900_000,
        });
        assert!(!app.land_agent_hud_update(Update::Codex(Box::new(update.clone())), (2, 2), at));
        update.snapshot.mode = Some(Sample {
            at,
            source: Source::TurnContext,
            value: Mode {
                approval: Some("on-request".into()),
                sandbox: Some("workspace-write".into()),
                ..Mode::default()
            },
        });
        // Compact shows the model, which this report doesn't name, and not the mode.
        assert!(!app.land_agent_hud_update(Update::Codex(Box::new(update.clone())), (3, 3), at));
        update.snapshot.mode.as_mut().unwrap().value.approval = Some("never".into());
        assert!(!app.land_agent_hud_update(Update::Codex(Box::new(update.clone())), (4, 4), at));
        update.snapshot.request = Some(Sample {
            at,
            source: Source::UsageRecord,
            value: Tokens {
                total: Some(0),
                ..Tokens::default()
            },
        });
        assert!(
            app.land_agent_hud_update(Update::Codex(Box::new(update.clone())), (5, 5), at),
            "reported zero differs from unknown"
        );
        app.state.config.pane.agent_hud = AgentHudMode::Full;
        update.snapshot.mode.as_mut().unwrap().value.approval = Some("on-request".into());
        assert!(
            app.land_agent_hud_update(Update::Codex(Box::new(update)), (6, 6), at),
            "full mode exposes approval"
        );
        app.cmd_agent_hud("info");
        assert!(app.view.pager.as_ref().unwrap().saveable);
        assert_eq!(
            app.runtime
                .pane_tabs
                .as_ref()
                .unwrap()
                .active_info()
                .activity,
            AgentActivity::Idle
        );
    });
}

#[test]
fn agent_hud_collector_has_no_idle_reads_and_cancels_pending_startup() {
    let tmp = tempfile::tempdir().unwrap();
    let path = rollout(tmp.path(), 5);
    let binding = Binding::Codex(codex_metrics::Binding {
        pane_id: "a".into(),
        thread_id: THREAD.into(),
        codex_home: tmp.path().to_owned(),
        generation: 1,
    });
    let (tx, rx) = mpsc::channel();
    let mut collector = collector::Collector::start(binding.clone(), Some(tx));
    let end = Instant::now() + Duration::from_secs(10);
    while collector.take().is_none_or(|u| codex_request(&u).is_none()) {
        assert!(Instant::now() < end);
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
    }
    while rx.recv_timeout(Duration::from_millis(50)).is_ok() {}
    let work = collector.work();
    assert!(rx.recv_timeout(Duration::from_millis(250)).is_err());
    assert_eq!(collector.work(), work);
    drop(collector);
    append_usage(&path, 9, "2026-10-13T12:00:01Z");
    // A callback already in flight may finish; the detached worker must exit.
    while rx.recv_timeout(Duration::from_secs(5)).is_ok() {}
    assert!(matches!(
        rx.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    let (tx, rx) = mpsc::channel();
    let pending = collector::Collector::start(binding, Some(tx));
    let start = Instant::now();
    drop(pending);
    assert!(start.elapsed() < Duration::from_millis(100));
    while rx.recv_timeout(Duration::from_secs(5)).is_ok() {}
    assert!(matches!(
        rx.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn agent_hud_typing_and_paste_reach_the_child_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        for mode in [AgentHudMode::Off, AgentHudMode::Compact, AgentHudMode::Full] {
            let got = tmp.path().join(mode.as_str());
            let script = tmp.path().join(format!("{}.sh", mode.as_str()));
            std::fs::write(&script,format!("stty -echo -icanon -icrnl -inlcr -igncr min 1 time 0\nprintf 'ready'\ndd bs=1 count=7 of={} 2>/dev/null\nexec sleep 30\n",got.display())).unwrap();
            let command = format!("/bin/sh {}", script.display());
            let mut app = fixture(tmp.path(), tmp.path());
            app.runtime.pane_tabs = Some(PaneTabs::new(entry(
                &mut app,
                tmp.path(),
                tmp.path(),
                &command,
            )));
            app.state.config.pane.agent_hud = mode;
            app.state.focus = state::Focus::Pane;
            app.snapshot_pane_routing();
            paint(&mut app, 100, 32);
            wait_screen(&mut app, "ready");
            let mut effects = app
                .handle_key(crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char('j'),
                    crossterm::event::KeyModifiers::NONE,
                ))
                .unwrap();
            effects.extend(app.handle_paste("hello".into()));
            effects.extend(
                app.handle_key(crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Enter,
                    crossterm::event::KeyModifiers::NONE,
                ))
                .unwrap(),
            );
            let (mut terminal, fg) = terminal_and_foreground();
            app.run_effects(effects, &mut terminal, &fg);
            let end = Instant::now() + Duration::from_secs(10);
            while std::fs::read(&got).ok().is_none_or(|b| b.len() < 7) {
                assert!(Instant::now() < end);
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(std::fs::read(got).unwrap(), b"jhello\r");
        }
    });
}

#[test]
fn agent_hud_mouse_rows_never_reach_child_and_geometry_changes_cancel_owned_drags() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let _lock = crate::mouse_test_lock();
    struct Capture;
    impl Drop for Capture {
        fn drop(&mut self) {
            crate::set_mouse_capture_for_test(false);
        }
    }
    crate::set_mouse_capture_for_test(true);
    let _capture = Capture;
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let script = tmp.path().join("mouse.sh");
        std::fs::write(&script, "stty -echo -icanon min 1 time 0\nprintf '\\033[?1002h\\033[?1006h\\033[Hdrag this text'\nexec sleep 30\n").unwrap();
        let mut app = fixture(tmp.path(), tmp.path());
        let child = entry(
            &mut app,
            tmp.path(),
            tmp.path(),
            &format!("/bin/sh {}", script.display()),
        );
        app.runtime.pane_tabs = Some(PaneTabs::new(child));
        wait_screen(&mut app, "drag this text");
        let pane = paint(&mut app, 100, 32).0.pane.unwrap();
        app.snapshot_pane_routing();
        let event = |kind, x, y, modifiers| MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers,
        };
        app.handle_mouse(event(
            MouseEventKind::Down(MouseButton::Left),
            pane.x,
            pane.y,
            KeyModifiers::CONTROL,
        ));
        assert!(app.view.pane_selection_snapshot.is_some());
        // Config reload and command changes share the same geometry and cancellation.
        app.state.config.pane.agent_hud = AgentHudMode::Compact;
        let hud = paint(&mut app, 100, 32).0.agent_hud.unwrap();
        assert!(app.view.pane_selection_snapshot.is_none());
        for kind in [
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            let effects = app.handle_mouse(event(kind, pane.x + 8, pane.y, KeyModifiers::NONE));
            assert!(!effects.iter().any(|e| matches!(
                e,
                Effect::SendToPane { .. } | Effect::CopyToClipboard { .. }
            )));
        }
        for y in hud.y..hud.bottom() {
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Drag(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Left),
                MouseEventKind::ScrollUp,
            ] {
                let effects = app.handle_mouse(event(kind, hud.x + 3, y, KeyModifiers::NONE));
                assert!(
                    !effects
                        .iter()
                        .any(|e| matches!(e, Effect::SendToPane { .. })),
                    "HUD row {y} leaked to child"
                );
            }
        }
        app.view.chrome_selection = None;
        let child = app.frame_layout(Rect::new(0, 0, 100, 32)).pane.unwrap();
        let effects = app.handle_mouse(event(
            MouseEventKind::Down(MouseButton::Left),
            child.x + 3,
            child.y,
            KeyModifiers::NONE,
        ));
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::SendToPane { .. })),
            "ordinary child clicks still route to child"
        );
    });
}

#[test]
fn agent_hud_cached_branch_must_belong_to_the_panes_repository() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = fixture(tmp.path(), tmp.path());
        app.state.config.pane.agent_hud = AgentHudMode::Compact;
        let column = app.state.cur_mut();
        column.listing.dir = tmp.path().to_owned();
        column.git_cache.current_repo_root = Some(tmp.path().to_owned());
        column.git_cache.head_branch_cache = Some((
            "/unrelated".into(),
            std::time::SystemTime::UNIX_EPOCH,
            "foreign-branch".into(),
        ));
        let rect = app
            .frame_layout(Rect::new(0, 0, 100, 32))
            .agent_hud
            .unwrap();
        assert!(
            !app.agent_hud_lines(rect)[0]
                .to_string()
                .contains("foreign-branch")
        );
        app.state.cur_mut().git_cache.head_branch_cache = Some((
            tmp.path().to_owned(),
            std::time::SystemTime::UNIX_EPOCH,
            "local-branch".into(),
        ));
        assert!(
            app.agent_hud_lines(rect)[0]
                .to_string()
                .contains("local-branch")
        );
    });
}

#[test]
fn agent_hud_waiting_attention_preserves_the_semantic_state_without_requesting_help() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = fixture(tmp.path(), tmp.path());
        app.state.config.pane.agent_hud = AgentHudMode::Compact;
        app.runtime
            .pane_tabs
            .as_mut()
            .unwrap()
            .active_info_mut()
            .activity = AgentActivity::Waiting;
        let rect = app
            .frame_layout(Rect::new(0, 0, 100, 32))
            .agent_hud
            .unwrap();
        let lines = app.agent_hud_lines(rect);
        let theme = &app.view.theme;
        assert!(lines[0].to_string().starts_with("○ waiting"));
        // The tab's own dot and colour: calm, never the needs-you red.
        let word = lines[0]
            .spans
            .iter()
            .find(|s| s.content == "waiting")
            .unwrap();
        assert_eq!(word.style.fg, Some(theme.dir));
        assert_ne!(word.style.fg, Some(theme.diff_del_fg));
        assert!(!lines[0].to_string().contains("needs you"));
    });
}

const SESSION: &str = "c1a0de00-0000-4000-8000-000000000001";

fn claude_entry(app: &mut App, cwd: &Path, session: Option<&str>) -> TabEntry {
    let wake = app.make_pane_wake();
    let pane = Pane::spawn("cat", 24, 100, cwd, &app.view.context_path, wake).unwrap();
    let mut info = TabInfo::new("claude", cwd);
    info.live_session_id = session.map(str::to_owned);
    info.spawn_epoch_secs = 0;
    info.activity = AgentActivity::Idle;
    TabEntry::new(pane, info)
}

/// A transcript under a project the pane's cwd doesn't name: Claude Code files
/// a session by the directory it started in.
fn transcript(home: &Path) -> std::path::PathBuf {
    let project = home.join("projects/-somewhere-else");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join(format!("{SESSION}.jsonl"));
    std::fs::write(&path, "").unwrap();
    path
}

fn append_message(path: &Path, id: &str, cache_read: u64) {
    let record = json!({"type": "assistant", "sessionId": SESSION,
        "timestamp": "2026-10-13T12:00:00Z", "effort": "xhigh",
        "message": {"id": id, "model": "claude-opus-5-5", "content": [],
                    "usage": {"input_tokens": 1, "cache_creation_input_tokens": 10,
                              "cache_read_input_tokens": cache_read, "output_tokens": 5}}});
    writeln!(
        std::fs::OpenOptions::new().append(true).open(path).unwrap(),
        "{record}"
    )
    .unwrap();
}

fn wait_hud_line(app: &mut App, ctx: &mut RunCtx, row: usize, text: &str) -> Vec<String> {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        tick(app, ctx);
        let rect = app
            .frame_layout(Rect::new(0, 0, 100, 32))
            .agent_hud
            .unwrap();
        let lines: Vec<String> = app
            .agent_hud_lines(rect)
            .iter()
            .map(ToString::to_string)
            .collect();
        if lines.get(row).is_some_and(|line| line.contains(text)) {
            return lines;
        }
        assert!(
            Instant::now() < end,
            "HUD row {row} never showed {text:?}: {lines:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn agent_hud_follows_a_claude_tabs_reported_session_in_its_home() {
    // The real loop settles the process-global mouse mode.
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let home = tmp.path().join("claude-home");
        let path = transcript(&home);
        append_message(&path, "msg-1", 100);
        let mut app = App::test_app(std::env::current_dir().unwrap());
        app.view.term_size = (100, 32);
        app.state.config.mouse.capture = false;
        app.state.pane.pane_height_pct = 50;
        app.state.pane.agent_hud.claude_home = Some(home);
        let unreported = claude_entry(&mut app, tmp.path(), None);
        app.runtime.pane_tabs = Some(PaneTabs::new(unreported));
        app.cmd_agent_hud("full");
        let mut ctx = RunCtx::for_test();
        let lines = wait_hud_line(&mut app, &mut ctx, 1, "awaiting metrics");
        assert!(
            app.runtime.agent_hud.is_none(),
            "no session reported yet, so nothing to read: {lines:?}"
        );

        let reported = claude_entry(&mut app, tmp.path(), Some(SESSION));
        app.runtime.pane_tabs.as_mut().unwrap().push(reported);
        let lines = wait_hud_line(&mut app, &mut ctx, 1, " 111 ");
        assert!(lines[0].contains("opus-5-5 xhigh"), "{lines:?}");
        assert!(
            lines[1].contains("out 5 ") && lines[1].contains("hit 90%"),
            "{lines:?}"
        );
        append_message(&path, "msg-2", 2_000);
        wait_hud_line(&mut app, &mut ctx, 1, " 2.0k ");

        let info = app.runtime.pane_tabs.as_mut().unwrap().active_info_mut();
        info.activity = AgentActivity::Waiting;
        let mut report = crate::pane::tabs::ReportedStatus::new(
            AgentActivity::Waiting,
            Instant::now(),
            Instant::now() + Duration::from_secs(60),
        );
        report.note = Some("CI on #641".into());
        info.reported = Some(report);
        let lines = wait_hud_line(&mut app, &mut ctx, 0, "CI on #641");
        assert!(lines[0].starts_with("○ waiting"), "{lines:?}");
        assert!(
            lines[0].ends_with("CI on #641"),
            "the note trails the row: {lines:?}"
        );

        app.cmd_agent_hud("info");
        let info: Vec<String> = app
            .view
            .pager
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .map(ToString::to_string)
            .collect();
        for expected in [
            format!("reported session: {SESSION}"),
            "context (latest request's input and cache): 2011 tokens".to_owned(),
            "totals over 2 distinct messages".to_owned(),
        ] {
            assert!(
                info.iter().any(|line| line.contains(&expected)),
                "{expected:?} missing from {info:?}"
            );
        }

        // Another config directory names other files: the consumer starts over.
        let binding = app.state.pane.agent_hud.binding.clone();
        app.state.pane.agent_hud.claude_home = Some(tmp.path().join("other-home"));
        let lines = wait_hud_line(&mut app, &mut ctx, 1, "transcript missing");
        assert_ne!(app.state.pane.agent_hud.binding, binding, "{lines:?}");
    });
}

const AGY_SESSION: &str = "a6900000-0000-4000-8000-000000000001";

fn agy_entry(app: &mut App, cwd: &Path, session: Option<&str>) -> TabEntry {
    let wake = app.make_pane_wake();
    let pane = Pane::spawn("cat", 24, 100, cwd, &app.view.context_path, wake).unwrap();
    let mut info = TabInfo::new("agy", cwd);
    info.live_session_id = session.map(str::to_owned);
    info.spawn_epoch_secs = 0;
    info.activity = AgentActivity::Idle;
    TabEntry::new(pane, info)
}

fn agy_transcript(home: &Path) -> std::path::PathBuf {
    let dir = home.join(format!(
        ".gemini/antigravity-cli/brain/{AGY_SESSION}/.system_generated/logs"
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("transcript.jsonl");
    std::fs::write(&path, "").unwrap();
    path
}

fn append_agy_model_change(path: &Path, model: &str) {
    let record = json!({
        "type": "USER_INPUT",
        "created_at": "2026-10-13T12:00:00Z",
        "content": format!("<USER_SETTINGS_CHANGE>\nThe user changed setting `Model Selection` from prev to {model}. No need to respond to this message directly.</USER_SETTINGS_CHANGE>")
    });
    writeln!(
        std::fs::OpenOptions::new().append(true).open(path).unwrap(),
        "{record}"
    )
    .unwrap();
}

fn append_agy_message(path: &Path, step: u64, cache_read: u64) {
    let record = json!({
        "type": "PLANNER_RESPONSE",
        "step_index": step,
        "created_at": "2026-10-13T12:00:00Z",
        "input_tokens": 11,
        "cache_read_tokens": cache_read,
        "output_tokens": 5,
        "tool_calls": []
    });
    writeln!(
        std::fs::OpenOptions::new().append(true).open(path).unwrap(),
        "{record}"
    )
    .unwrap();
}

#[test]
fn agent_hud_follows_an_agy_tabs_reported_session_in_its_home() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let home = tmp.path().join("agy-home");
        let path = agy_transcript(&home);
        append_agy_model_change(&path, "gemini-2.5-pro");
        append_agy_message(&path, 1, 100);

        let mut app = App::test_app(std::env::current_dir().unwrap());
        app.view.term_size = (100, 32);
        app.state.config.mouse.capture = false;
        app.state.pane.pane_height_pct = 50;
        app.state.pane.agent_hud.agy_home = Some(home);
        let unreported = agy_entry(&mut app, tmp.path(), None);
        app.runtime.pane_tabs = Some(PaneTabs::new(unreported));
        app.cmd_agent_hud("full");
        let mut ctx = RunCtx::for_test();
        let lines = wait_hud_line(&mut app, &mut ctx, 1, "awaiting metrics");
        assert!(
            app.runtime.agent_hud.is_none(),
            "no session reported yet, so nothing to read: {lines:?}"
        );

        let reported = agy_entry(&mut app, tmp.path(), Some(AGY_SESSION));
        app.runtime.pane_tabs.as_mut().unwrap().push(reported);
        let lines = wait_hud_line(&mut app, &mut ctx, 1, " 111 ");
        assert!(lines[0].contains("gemini-2.5-pro"), "{lines:?}");
        assert!(
            lines[1].contains("out 5 ") && lines[1].contains("hit 90%"),
            "{lines:?}"
        );
        append_agy_message(&path, 2, 2_000);
        wait_hud_line(&mut app, &mut ctx, 1, " 2.0k ");

        let info = app.runtime.pane_tabs.as_mut().unwrap().active_info_mut();
        info.activity = AgentActivity::Waiting;
        let mut report = crate::pane::tabs::ReportedStatus::new(
            AgentActivity::Waiting,
            Instant::now(),
            Instant::now() + Duration::from_secs(60),
        );
        report.note = Some("indexing codebase".into());
        info.reported = Some(report);
        let lines = wait_hud_line(&mut app, &mut ctx, 0, "indexing codebase");
        assert!(lines[0].starts_with("○ waiting"), "{lines:?}");
        assert!(
            lines[0].ends_with("indexing codebase"),
            "the note trails the row: {lines:?}"
        );

        app.cmd_agent_hud("info");
        let info: Vec<String> = app
            .view
            .pager
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .map(ToString::to_string)
            .collect();
        for expected in [
            format!("reported session: {AGY_SESSION}"),
            "context (latest request's input and cache read): 2011 tokens".to_owned(),
            "totals over 2 distinct messages".to_owned(),
        ] {
            assert!(
                info.iter().any(|line| line.contains(&expected)),
                "{expected:?} missing from {info:?}"
            );
        }

        // Another config directory names other files: the consumer starts over.
        let binding = app.state.pane.agent_hud.binding.clone();
        app.state.pane.agent_hud.agy_home = Some(tmp.path().join("other-home"));
        let lines = wait_hud_line(&mut app, &mut ctx, 1, "transcript missing");
        assert_ne!(app.state.pane.agent_hud.binding, binding, "{lines:?}");
    });
}

/// Saving the rc for any other reason keeps what `:agent hud` chose; changing
/// `agent_hud` in it is the newer word.
#[test]
fn a_config_reload_keeps_the_hud_choice_unless_it_changes_agent_hud() {
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(tmp.path().to_path_buf());
        app.cmd_agent_hud("full");
        let mut unrelated = app.state.config.clone();
        unrelated.pane.preview_pasted_images = !unrelated.pane.preview_pasted_images;
        app.adopt_config(unrelated);
        assert_eq!(app.agent_hud_preference(), AgentHudMode::Full);
        let mut edited = app.state.config.clone();
        edited.pane.agent_hud = AgentHudMode::Compact;
        app.adopt_config(edited);
        assert_eq!(app.agent_hud_preference(), AgentHudMode::Compact);
        assert!(app.state.pane.agent_hud.choice.is_none());
    });
}

/// A double-click on the strip opens its dump; one click opens nothing, and
/// neither copies the row or reaches the child.
#[test]
fn agent_hud_double_click_opens_the_dump_and_a_single_click_does_not() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let _lock = crate::mouse_test_lock();
    struct Capture;
    impl Drop for Capture {
        fn drop(&mut self) {
            crate::set_mouse_capture_for_test(false);
        }
    }
    crate::set_mouse_capture_for_test(true);
    let _capture = Capture;
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = fixture(tmp.path(), tmp.path());
        app.state.config.mouse.capture = true;
        app.cmd_agent_hud("full");
        let hud = paint(&mut app, 100, 32).0.agent_hud.unwrap();
        app.snapshot_pane_routing();
        let press = |app: &mut App, kind| {
            app.handle_mouse(MouseEvent {
                kind,
                column: hud.x + 3,
                row: hud.y + 1,
                modifiers: KeyModifiers::NONE,
            })
        };
        let mut effects = press(&mut app, MouseEventKind::Down(MouseButton::Left));
        effects.extend(press(&mut app, MouseEventKind::Up(MouseButton::Left)));
        assert!(app.view.pager.is_none(), "one click opens nothing");
        effects.extend(press(&mut app, MouseEventKind::Down(MouseButton::Left)));
        effects.extend(press(&mut app, MouseEventKind::Up(MouseButton::Left)));
        let pager = app
            .view
            .pager
            .as_ref()
            .expect("a double-click opens the dump");
        assert!(pager.lines[0].to_string().starts_with("agent HUD: full"));
        assert!(
            !effects.iter().any(|e| matches!(
                e,
                Effect::SendToPane { .. } | Effect::CopyToClipboard { .. }
            )),
            "{effects:?}"
        );
        app.view.pager = None;
        paint(&mut app, 100, 32);
        let mut drag = press(&mut app, MouseEventKind::Down(MouseButton::Left));
        drag.extend(app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: hud.x + 9,
            row: hud.y + 1,
            modifiers: KeyModifiers::NONE,
        }));
        drag.extend(app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: hud.x + 9,
            row: hud.y + 1,
            modifiers: KeyModifiers::NONE,
        }));
        assert!(
            drag.iter()
                .any(|e| matches!(e, Effect::CopyToClipboard { .. })),
            "a drag across the strip copies its text: {drag:?}"
        );
        assert!(!drag.iter().any(|e| matches!(e, Effect::SendToPane { .. })));
    });
}

/// A tab shows its own context share only for its own session, and the plan's
/// windows from the newest report any tab sent, since the plan is the account's.
#[test]
fn statusline_reports_reach_the_strip_and_the_dump() {
    use crate::agent::claude_statusline::{ContextWindow, RateWindow, Statusline};
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::env::current_dir().unwrap());
        app.view.term_size = (120, 32);
        app.state.config.mouse.capture = false;
        app.state.pane.pane_height_pct = 50;
        app.state.pane.agent_hud.claude_home = Some(tmp.path().join("claude-home"));
        let older = claude_entry(&mut app, tmp.path(), None);
        let older_id = older.info.id.clone();
        app.runtime.pane_tabs = Some(PaneTabs::new(older));
        let mine = claude_entry(&mut app, tmp.path(), Some(SESSION));
        let mine_id = mine.info.id.clone();
        app.runtime.pane_tabs.as_mut().unwrap().push(mine);
        app.cmd_agent_hud("full");
        let window = |used_percent| RateWindow {
            used_percent,
            resets_at: Some(1_791_900_000),
        };
        app.land_statusline(
            &mine_id,
            Statusline {
                session_id: Some(SESSION.into()),
                context: Some(ContextWindow {
                    size: 1_000_000,
                    used_percent: Some(34),
                }),
                five_hour: Some(window(10)),
                seven_day: Some(window(20)),
            },
        );
        app.land_statusline(
            &older_id,
            Statusline {
                five_hour: Some(window(23)),
                seven_day: Some(window(61)),
                ..Statusline::default()
            },
        );
        // Explicit times: two landings can share a clock tick.
        for (id, second) in [(&mine_id, 0), (&older_id, 1)] {
            let tabs = app.runtime.pane_tabs.as_mut().unwrap();
            let entry = tabs
                .tabs_mut()
                .iter_mut()
                .find(|e| &e.info.id == id)
                .unwrap();
            entry.info.claude_statusline.as_mut().unwrap().1 =
                Timestamp::from_second(1_791_892_800 + second).unwrap();
        }
        app.land_statusline("no-such-pane", Statusline::default());
        let codex = entry(&mut app, tmp.path(), tmp.path(), "cat");
        let codex_id = codex.info.id.clone();
        app.runtime.pane_tabs.as_mut().unwrap().push(codex);
        app.land_statusline(&codex_id, Statusline::default());
        let tabs = app.runtime.pane_tabs.as_mut().unwrap();
        assert!(
            tabs.active_info().claude_statusline.is_none(),
            "only a Claude Code tab keeps a report"
        );
        tabs.switch_to(1);
        let rect = app
            .frame_layout(Rect::new(0, 0, 120, 32))
            .agent_hud
            .unwrap();
        let usage = app.agent_hud_lines(rect)[1].to_string();
        assert!(usage.contains("34%"), "its own session's share: {usage}");
        assert!(
            usage.contains("5h 23%") && usage.contains("7d 61%"),
            "the newest plan: {usage}"
        );
        let info = app.runtime.pane_tabs.as_mut().unwrap().active_info_mut();
        info.live_session_id = Some("c1a0de00-0000-4000-8000-000000000002".into());
        let usage = app.agent_hud_lines(rect)[1].to_string();
        assert!(
            !usage.contains("34%"),
            "another session's share is not this one's: {usage}"
        );
        app.cmd_agent_hud("info");
        let dump: Vec<String> = app
            .view
            .pager
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .map(ToString::to_string)
            .collect();
        for expected in [
            "statusline (spyc --statusline): reported at",
            "context window: 1000000 tokens; 34% used",
            "five-hour: 23% used; resets at",
            "seven-day: 61% used",
        ] {
            assert!(
                dump.iter().any(|l| l.contains(expected)),
                "{expected:?} missing from {dump:?}"
            );
        }
    });
}

#[test]
fn agent_hud_retries_missing_agy_transcript_on_status_report() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let home = tmp.path().join("agy-home");
        let mut app = App::test_app(std::env::current_dir().unwrap());
        app.view.term_size = (100, 32);
        app.state.config.mouse.capture = false;
        app.state.pane.pane_height_pct = 50;
        app.state.pane.agent_hud.agy_home = Some(home.clone());
        let entry = agy_entry(&mut app, tmp.path(), Some(AGY_SESSION));
        app.runtime.pane_tabs = Some(PaneTabs::new(entry));
        app.cmd_agent_hud("full");
        let mut ctx = RunCtx::for_test();

        let lines = wait_hud_line(&mut app, &mut ctx, 1, "transcript missing");
        assert_eq!(
            app.active_agent_hud_update().map(Update::availability),
            Some(Availability::Missing),
            "{lines:?}"
        );

        let path = agy_transcript(&home);
        append_agy_model_change(&path, "gemini-2.5-pro");
        append_agy_message(&path, 1, 100);

        let resp = app.execute_mcp_command(crate::mcp_cmd::McpCommand::ReportStatus {
            pane_id: None,
            pane: None,
            status: "working".into(),
            ttl_ms: None,
            note: None,
            session_id: Some(AGY_SESSION.into()),
            hook_event: None,
        });
        assert!(matches!(resp, crate::mcp_cmd::McpResponse::Ok { .. }));

        let lines = wait_hud_line(&mut app, &mut ctx, 1, " 111 ");
        assert!(lines[0].contains("gemini-2.5-pro"), "{lines:?}");
        assert_eq!(
            app.active_agent_hud_update().map(Update::availability),
            Some(Availability::Available)
        );
    });
}

/// A strip showing plan resets wakes spyc for the next reset, and a countdown
/// also each time its label ticks over; a hidden strip asks for nothing.
#[test]
fn plan_resets_wake_the_strip_only_when_its_cells_change() {
    use crate::agent::claude_statusline::{RateWindow, Statusline};
    use crate::config::AgentHudResets;
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::env::current_dir().unwrap());
        app.view.term_size = (120, 32);
        app.state.config.mouse.capture = false;
        app.state.pane.pane_height_pct = 50;
        app.state.pane.agent_hud.claude_home = Some(tmp.path().join("claude-home"));
        let tab = claude_entry(&mut app, tmp.path(), Some(SESSION));
        let id = tab.info.id.clone();
        app.runtime.pane_tabs = Some(PaneTabs::new(tab));
        app.cmd_agent_hud("compact");
        let now = Timestamp::now().as_second();
        let five = now + 3 * 3600 + 125;
        let window = |used_percent, at| RateWindow {
            used_percent,
            resets_at: Some(at),
        };
        app.land_statusline(
            &id,
            Statusline {
                five_hour: Some(window(23, five)),
                // Half past an hour, so its own hourly tick is ~30 minutes off.
                seven_day: Some(window(61, now + 3 * 86_400 + 1800)),
                ..Statusline::default()
            },
        );
        // Seconds until the strip's own wake, if it armed one.
        let wake_in = |app: &mut App| {
            let mut ctx = RunCtx::for_test();
            let start = Instant::now();
            app.settle_agent_hud(start, &mut ctx);
            ctx.scheduler
                .next()
                .map(|at| at.duration_since(start).as_secs())
        };
        let clock = wake_in(&mut app).expect("a reset to wake for");
        assert!(
            (3 * 3600 + 120..=3 * 3600 + 125).contains(&clock),
            "the five-hour reset: {clock}s"
        );
        app.state.config.pane.agent_hud_resets = AgentHudResets::Countdown;
        let countdown = wake_in(&mut app).expect("a tick to wake for");
        assert!((1..=60).contains(&countdown), "the next tick: {countdown}s");
        app.cmd_agent_hud("off");
        assert_eq!(wake_in(&mut app), None, "no strip, no wake");
    });
}

/// A reset passing between two settles is what changes the strip, so it alone
/// must repaint it: the five-hour share stops being claimed.
#[test]
fn a_plan_reset_passing_repaints_the_strip() {
    use crate::agent::claude_statusline::{RateWindow, Statusline};
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let mut app = App::test_app(std::env::current_dir().unwrap());
        app.view.term_size = (120, 32);
        app.state.config.mouse.capture = false;
        app.state.pane.pane_height_pct = 50;
        app.state.pane.agent_hud.claude_home = Some(tmp.path().join("claude-home"));
        let tab = claude_entry(&mut app, tmp.path(), Some(SESSION));
        let id = tab.info.id.clone();
        app.runtime.pane_tabs = Some(PaneTabs::new(tab));
        app.cmd_agent_hud("compact");
        let now = Timestamp::now().as_second();
        app.land_statusline(
            &id,
            Statusline {
                five_hour: Some(RateWindow {
                    used_percent: 23,
                    resets_at: Some(now - 10),
                }),
                ..Statusline::default()
            },
        );
        let mut ctx = RunCtx::for_test();
        app.settle_agent_hud(Instant::now(), &mut ctx);
        // As if the last settle came before the reset.
        app.state.pane.agent_hud.as_of = Some(Timestamp::from_second(now - 20).unwrap());
        let rect = app
            .frame_layout(Rect::new(0, 0, 120, 32))
            .agent_hud
            .unwrap();
        assert!(app.agent_hud_lines(rect)[1].to_string().contains("5h 23%"));
        let mut ctx = RunCtx::for_test();
        app.settle_agent_hud(Instant::now(), &mut ctx);
        assert!(ctx.draw.is_dirty(), "the reset repaints");
        let usage = app.agent_hud_lines(rect)[1].to_string();
        assert!(usage.contains("5h --"), "{usage}");
    });
}

/// Claude Code reports its session at startup but writes the transcript only
/// with the first prompt, so the first lookup can miss a file that then exists.
#[test]
fn agent_hud_retries_missing_claude_transcript_on_status_report() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(tmp.path(), || {
        let home = tmp.path().join("claude-home");
        std::fs::create_dir_all(home.join("projects")).unwrap();
        let mut app = App::test_app(std::env::current_dir().unwrap());
        app.view.term_size = (100, 32);
        app.state.config.mouse.capture = false;
        app.state.pane.pane_height_pct = 50;
        app.state.pane.agent_hud.claude_home = Some(home.clone());
        let entry = claude_entry(&mut app, tmp.path(), Some(SESSION));
        app.runtime.pane_tabs = Some(PaneTabs::new(entry));
        app.cmd_agent_hud("full");
        let mut ctx = RunCtx::for_test();

        let lines = wait_hud_line(&mut app, &mut ctx, 1, "transcript missing");
        assert_eq!(
            app.active_agent_hud_update().map(Update::availability),
            Some(Availability::Missing),
            "{lines:?}"
        );
        app.cmd_agent_hud("info");
        let dump = app.view.pager.take().unwrap().lines;
        assert!(
            dump.iter()
                .any(|l| l.to_string().contains("next status report")),
            "the dump says a report retries it"
        );

        let path = transcript(&home);
        append_message(&path, "msg-1", 100);

        let resp = app.execute_mcp_command(crate::mcp_cmd::McpCommand::ReportStatus {
            pane_id: None,
            pane: None,
            status: "working".into(),
            ttl_ms: None,
            note: None,
            session_id: Some(SESSION.into()),
            hook_event: None,
        });
        assert!(matches!(resp, crate::mcp_cmd::McpResponse::Ok { .. }));

        let lines = wait_hud_line(&mut app, &mut ctx, 1, " 111 ");
        assert!(lines[0].contains("opus-5-5 xhigh"), "{lines:?}");
        assert_eq!(
            app.active_agent_hud_update().map(Update::availability),
            Some(Availability::Available)
        );
    });
}
