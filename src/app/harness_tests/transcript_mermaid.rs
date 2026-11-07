//! A ` ```mermaid ` diagram in an agent's transcript scrollback (`^a v`): `i`
//! views it and `o` opens it, as in a markdown file; and what a key there
//! flashes is drawn, though the scrollback has no title bar to carry it.

use super::*;
use crate::app::mermaid_ops::MermaidMode;
use serde_json::json;
use std::path::Path;
use std::time::{Duration, Instant};

const CLAUDE_SID: &str = "c1a0de00-0000-4000-8000-00000000d1a6";
const CODEX_SID: &str = "019e8b21-9e7c-7553-a118-d1cdada7d1a6";
const DIAGRAM: &str = "sequenceDiagram\n    participant C as client\n    C->>S: request";

/// Agent prose holding the diagram between two paragraphs.
fn prose_with_diagram() -> String {
    format!("Here is the flow:\n\n```mermaid\n{DIAGRAM}\n```\n\nThat is the whole exchange.")
}

/// A tab running `cat` under `command`'s name, so it's detected as that agent
/// and spawns in a test.
fn agent_tab<'a>(
    app: &'a mut App,
    dir: &Path,
    command: &str,
) -> &'a mut crate::pane::tabs::TabInfo {
    let wake = app.make_pane_wake();
    let pane = crate::pane::Pane::spawn("cat", 24, 80, dir, &app.view.project().context_path, wake)
        .expect("spawn cat");
    let entry = crate::pane::tabs::TabEntry::new(
        pane,
        crate::pane::tabs::TabInfo::new(command, dir.to_path_buf()),
    );
    app.runtime.project_mut().pane_tabs = Some(crate::pane::tabs::PaneTabs::new(entry));
    app.runtime
        .project_mut()
        .pane_tabs
        .as_mut()
        .expect("just installed")
        .active_info_mut()
}

/// A test app in `dir` with a 100×40 screen and Claude's transcript behind `^a v`.
fn app_in(dir: &Path) -> App {
    let mut app = App::test_app(dir.to_path_buf());
    app.view.term_size = (100, 40);
    let project = app.state.active_project_mut();
    project.config.mouse.capture = false;
    project.config.pane.claude_transcript_scrollback = true;
    project.pane.pane_height_pct = 50;
    app
}

/// A Claude tab whose conversation, in `home`, is `records`.
fn claude_conversation(app: &mut App, dir: &Path, records: &[serde_json::Value]) {
    let home = dir.join("claude-home");
    let project = home
        .join("projects")
        .join(crate::state::sessions::project_slug(dir));
    std::fs::create_dir_all(&project).unwrap();
    let body = records
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(project.join(format!("{CLAUDE_SID}.jsonl")), body).unwrap();
    let info = agent_tab(app, dir, "claude");
    info.claude_home = Some(home);
    info.live_session_id = Some(CLAUDE_SID.into());
}

fn claude_user(text: &str) -> serde_json::Value {
    json!({"type": "user", "sessionId": CLAUDE_SID, "message": {"content": text}})
}

fn claude_assistant(text: &str) -> serde_json::Value {
    json!({"type": "assistant", "sessionId": CLAUDE_SID,
           "message": {"id": "msg-1", "content": [{"type": "text", "text": text}]}})
}

/// `^a v`, then the loop's drain until the transcript lands.
fn open_scrollback(app: &mut App) {
    app.apply(&Action::PaneScrollEnter).unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        app.drain_pager_stream();
        match app.view.project().scroll_pager.as_ref() {
            Some(pager) if !pager.streaming => break,
            None => panic!("no transcript mounted: {:?}", app.flash_text()),
            Some(_) => {}
        }
        assert!(Instant::now() < end, "the transcript never landed");
        std::thread::sleep(Duration::from_millis(10));
    }
    // A frame, as the loop draws one: it parks the view at the newest turn and
    // measures the viewport the pager's keys work in.
    screen(app);
}

/// One rendered frame, row by row, joined.
fn screen(app: &mut App) -> String {
    let (w, h) = app.view.term_size;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    let buf = terminal.backend().buffer();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf.cell((x, y)).map_or(" ", |c| c.symbol()))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn press(app: &mut App, code: KeyCode) -> Vec<Effect> {
    app.handle_pager_key(KeyEvent::new(code, KeyModifiers::empty()))
}

/// `:N` to the scrollback line that heads the diagram, as a reader gets there.
fn jump_to_diagram(app: &mut App) {
    let line = app
        .view
        .project()
        .scroll_pager
        .as_ref()
        .expect("a scrollback is open")
        .lines
        .iter()
        .position(|l| {
            l.spans
                .iter()
                .any(|s| s.content.contains("mermaid diagram"))
        })
        .expect("the transcript shows the diagram's header");
    press(app, KeyCode::Char(':'));
    for digit in (line + 1).to_string().chars() {
        press(app, KeyCode::Char(digit));
    }
    press(app, KeyCode::Enter);
    screen(app);
}

/// The diagram a key asked to render: its source, and whether it's the
/// in-terminal view (`i`) rather than the OS viewer (`o`).
fn mermaid_render(effects: &[Effect]) -> Option<(&str, bool)> {
    effects.iter().find_map(|e| match e {
        Effect::RenderMermaid(op) => Some((
            op.source.as_str(),
            matches!(op.mode, MermaidMode::View { .. }),
        )),
        _ => None,
    })
}

/// `i` views the diagram on screen and `o` opens it, each with its source.
fn assert_i_and_o_render_the_diagram(app: &mut App) {
    let viewed = press(app, KeyCode::Char('i'));
    assert_eq!(
        mermaid_render(&viewed),
        Some((DIAGRAM, true)),
        "`i` views the diagram; the scrollback says {:?}",
        app.view
            .project()
            .scroll_pager
            .as_ref()
            .and_then(|v| v.flash.clone())
    );
    let opened = press(app, KeyCode::Char('o'));
    assert_eq!(
        mermaid_render(&opened),
        Some((DIAGRAM, false)),
        "`o` opens it"
    );
}

/// The reported case: Claude draws a diagram, and `i` / `o` on it in `^a v` do
/// what the header above it promises.
#[test]
#[ignore = "red: a transcript scrollback drops its mermaid blocks"]
fn i_and_o_render_a_diagram_in_a_claude_transcript_scrollback() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = app_in(&dir);
        claude_conversation(
            &mut app,
            &dir,
            &[
                claude_user("draw the request flow"),
                claude_assistant(&prose_with_diagram()),
            ],
        );
        open_scrollback(&mut app);
        jump_to_diagram(&mut app);
        assert_i_and_o_render_the_diagram(&mut app);
    });
}

/// A long conversation keeps only its newest lines; a diagram among them is
/// still the one `i` and `o` find once the older lines are dropped.
#[test]
#[ignore = "red: a transcript scrollback drops its mermaid blocks"]
fn a_diagram_survives_a_long_claude_transcript_dropping_its_oldest_lines() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = app_in(&dir);
        let lines = |n: usize| {
            (0..n)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        // More than twice the kept lines before the last record, so the view
        // drops its oldest while the conversation is read and again at its end.
        claude_conversation(
            &mut app,
            &dir,
            &[
                claude_user(&lines(crate::state::MAX_TRANSCRIPT_LINES * 3 / 2)),
                claude_assistant(&prose_with_diagram()),
                claude_user(&lines(crate::state::MAX_TRANSCRIPT_LINES / 2)),
                claude_user("thanks"),
            ],
        );
        open_scrollback(&mut app);
        let kept = app
            .view
            .project()
            .scroll_pager
            .as_ref()
            .unwrap()
            .lines
            .len();
        assert!(
            kept <= crate::state::MAX_TRANSCRIPT_LINES,
            "the view kept {kept} lines, so it dropped none"
        );
        jump_to_diagram(&mut app);
        assert_i_and_o_render_the_diagram(&mut app);
    });
}

/// Codex's rollout, the same.
#[test]
#[ignore = "red: a transcript scrollback drops its mermaid blocks"]
fn i_and_o_render_a_diagram_in_a_codex_transcript_scrollback() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = app_in(&dir);
        let home = dir.join("codex-home");
        let day = home.join("sessions/2026/11/07");
        std::fs::create_dir_all(&day).unwrap();
        let records = [
            json!({"type": "session_meta", "payload": {"id": CODEX_SID, "cwd": dir,
                   "timestamp": jiff::Timestamp::now().to_string()}}),
            json!({"type": "event_msg", "payload": {"type": "user_message",
                   "message": "draw the request flow"}}),
            json!({"type": "event_msg", "payload": {"type": "agent_message",
                   "message": prose_with_diagram()}}),
        ];
        let body = records
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(day.join(format!("rollout-now-{CODEX_SID}.jsonl")), body).unwrap();
        let info = agent_tab(&mut app, &dir, "codex");
        info.codex_home = Some(home);
        info.codex_session_id = Some(CODEX_SID.into());

        open_scrollback(&mut app);
        jump_to_diagram(&mut app);
        assert_i_and_o_render_the_diagram(&mut app);
    });
}

/// The scrollback is borderless, so it has no title bar for a flash; what its
/// keys flash still reaches the screen. Here, `o` with no diagram in view.
#[test]
#[ignore = "red: a borderless pager draws no flash"]
fn a_flash_from_a_scrollback_key_is_on_screen() {
    let _lock = crate::mouse_test_lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::state::with_state_root(&tmp.path().join("state"), || {
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let mut app = app_in(&dir);
        claude_conversation(
            &mut app,
            &dir,
            &[
                claude_user("no diagrams here"),
                claude_assistant("Just prose."),
            ],
        );
        open_scrollback(&mut app);
        let effects = press(&mut app, KeyCode::Char('o'));
        assert_eq!(mermaid_render(&effects), None, "nothing to open");
        let shown = screen(&mut app);
        assert!(
            shown.contains("no mermaid diagram in view"),
            "the reply to `o` is drawn:\n{shown}"
        );
    });
}
