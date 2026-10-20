//! #99 step 6c: every open project's attention, where the user looks first: a
//! chip for each project in the status bar, a blocked count in the terminal
//! title, and a glyph on each switcher row.
//!
//! Four projects in one `App`, opened in the order `x`, `y`, `z`, `v`, with the
//! user in `y`. Each case gives the others agents and reports, then draws the
//! real frame.

use std::path::PathBuf;
use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use super::project_tabs::{agent_tab_in, report_at};
use super::projects_open::leader;
use super::*;
use crate::pane::AgentActivity;

const SESSION: &str = "SAFFRON_CUMIN";
const BLOCKED_RED: Color = Color::Rgb(0xF7, 0x76, 0x8E);

struct Four {
    app: App,
    ids: [state::ProjectId; 4],
    homes: [PathBuf; 4],
}

impl Four {
    /// Give project `i` an agent tab, and return its tab index.
    fn agent(&mut self, i: usize) -> usize {
        agent_tab_in(&mut self.app, self.ids[i], &self.homes[i], "claude");
        self.app.as_project(self.ids[i], |app| {
            app.runtime
                .project()
                .pane_tabs
                .as_ref()
                .map_or(0, |tabs| tabs.tabs().len() - 1)
        })
    }

    /// Project `i`'s new agent reports `status`, and the walk sees it.
    fn reports(&mut self, i: usize, status: AgentActivity) {
        let tab = self.agent(i);
        report_at(&mut self.app, self.ids[i], tab, status, Instant::now());
    }
}

/// `xproj`, `yproj`, `zproj` and `vproj` open in that order, the user in `y`,
/// the session named.
fn with_four(test: impl FnOnce(&mut Four)) {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let [state_dir, rc_home] = ["state", "home"].map(|d| base.join(d));
    let homes = ["xproj", "yproj", "zproj", "vproj"].map(|d| base.join(d));
    for d in [&state_dir, &rc_home].into_iter().chain(&homes) {
        std::fs::create_dir_all(d).unwrap();
    }
    crate::state::with_state_root(&state_dir, || {
        crate::config::with_home_root(&rc_home, || {
            let mut app = App::test_app(homes[0].clone());
            let x = app.state.active;
            app.ready_test_project(x, &homes[0]);
            let ids = [
                x,
                app.add_test_project(&homes[1]),
                app.add_test_project(&homes[2]),
                app.add_test_project(&homes[3]),
            ];
            app.make_active(ids[1]);
            app.state.session_name = Some(SESSION.to_string());
            test(&mut Four { app, ids, homes });
        });
    });
}

/// The frame drawn `w` columns wide.
fn draw(app: &mut App, w: u16) -> Buffer {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, 30)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    terminal.backend().buffer().clone()
}

/// Row `y` of the frame, as text.
fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width)
        .map(|x| buf.cell((x, y)).map_or(" ", |c| c.symbol()))
        .collect()
}

/// The status bar's text: the top row, where the bar is by default.
fn bar(app: &mut App, w: u16) -> String {
    row(&draw(app, w), 0)
}

/// The first row of `buf` holding every one of `needles`.
fn row_with(buf: &Buffer, needles: &[&str]) -> Option<u16> {
    (0..buf.area.height).find(|&y| {
        let text = row(buf, y);
        needles.iter().all(|n| text.contains(n))
    })
}

/// The cell one space after `word` on row `y`: a chip's or a switcher row's
/// glyph, or a blank.
fn after<'b>(buf: &'b Buffer, y: u16, word: &str) -> Option<&'b ratatui::buffer::Cell> {
    let w = buf.area.width;
    let len = u16::try_from(word.chars().count()).unwrap();
    (0..w.saturating_sub(len + 1)).find_map(|x| {
        let here: String = (x..x + len)
            .map(|cx| buf.cell((cx, y)).map_or(" ", |c| c.symbol()))
            .collect();
        let gap = buf.cell((x + len, y)).map(ratatui::buffer::Cell::symbol);
        (here == word && gap == Some(" "))
            .then(|| buf.cell((x + len + 1, y)))
            .flatten()
    })
}

/// The terminal title as the next frame would set it.
fn title(app: &mut App) -> String {
    app.view.last_term_title = None;
    match app.term_title_effect() {
        Some(Effect::SetTerminalTitle { title }) => title,
        other => panic!("no title: {other:?}"),
    }
}

/// The control: with one project open, the bar and the title read as they
/// always have, the home and the session named and no glyph, whatever its
/// agents are doing.
#[test]
fn with_one_project_the_bar_and_title_read_as_before() {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let (state_dir, home) = (base.join("state"), base.join("xproj"));
    for d in [&state_dir, &home] {
        std::fs::create_dir_all(d).unwrap();
    }
    crate::state::with_state_root(&state_dir, || {
        let mut app = App::test_app(home.clone());
        let x = app.state.active;
        app.ready_test_project(x, &home);
        app.state.session_name = Some(SESSION.to_string());
        agent_tab_in(&mut app, x, &home, "claude");
        agent_tab_in(&mut app, x, &home, "claude");
        report_at(&mut app, x, 1, AgentActivity::Blocked, Instant::now());
        let text = bar(&mut app, 120);
        assert!(text.contains(" xproj "), "{text}");
        assert!(text.contains(SESSION), "{text}");
        assert!(!text.contains("xproj ■"), "{text}");
        assert_eq!(
            title(&mut app),
            format!("\u{1f336}\u{fe0f}: xproj \u{b7} {SESSION}")
        );
    });
}

/// With several open, each project's chip is in the bar in switcher order,
/// with its tab dots' glyph: red `■` blocked, teal `■` a done the user hasn't
/// seen, hollow `○` waiting, and none for a project with nothing to say. The
/// one in view is bold and the others dim; mono reads the same, uncoloured.
#[test]
fn several_projects_show_in_the_bar_each_with_its_glyph() {
    with_four(|t| {
        t.reports(0, AgentActivity::Blocked);
        t.reports(2, AgentActivity::Done);
        t.reports(3, AgentActivity::Waiting);
        let chips = "xproj ■ · yproj · zproj ■ · vproj ○";
        let buf = draw(&mut t.app, 160);
        assert!(row(&buf, 0).contains(chips), "{}", row(&buf, 0));
        let theme = crate::ui::theme::Theme::default();
        let fg = |word| after(&buf, 0, word).map(|c| (c.symbol().to_string(), c.fg));
        assert_eq!(fg("xproj"), Some(("■".into(), BLOCKED_RED)));
        assert_eq!(fg("zproj"), Some(("■".into(), theme.take)));
        assert_eq!(fg("vproj"), Some(("○".into(), theme.dir)));
        let style = |word: &str| {
            let at = row(&buf, 0).find(word).expect("the chip is drawn");
            let x = u16::try_from(row(&buf, 0)[..at].chars().count()).unwrap();
            buf.cell((x, 0)).expect("a cell").modifier
        };
        assert!(style("yproj").contains(Modifier::BOLD), "the active chip");
        assert!(
            style("xproj").contains(Modifier::DIM),
            "another project's chip"
        );

        t.app.view.theme.mono = true;
        let mono = bar(&mut t.app, 160);
        assert!(mono.contains(chips), "{mono}");
    });
}

/// The session name leaves the bar while several projects are open, giving
/// the chips its room; the title still names it.
#[test]
fn the_session_name_leaves_the_bar_while_several_are_open() {
    with_four(|t| {
        for mono in [false, true] {
            t.app.view.theme.mono = mono;
            let text = bar(&mut t.app, 160);
            assert!(!text.contains(SESSION), "mono={mono}: {text}");
            assert!(text.contains("xproj · yproj"), "mono={mono}: {text}");
        }
        assert!(title(&mut t.app).ends_with(SESSION));
    });
}

/// A done on the tab in view is seen as it lands, so the project in view's
/// chip has nothing to say; one on its other tab is unseen, and teal.
#[test]
fn a_done_in_view_is_seen_and_one_out_of_view_is_not() {
    with_four(|t| {
        t.agent(1);
        t.agent(1);
        let y = t.ids[1];
        t.app.as_project(y, |app| {
            let tabs = app.runtime.project_mut().pane_tabs.as_mut();
            tabs.expect("two tabs").switch_to(0);
        });
        let now = Instant::now();
        report_at(&mut t.app, y, 0, AgentActivity::Done, now);
        let buf = draw(&mut t.app, 160);
        assert_eq!(
            after(&buf, 0, "yproj").map(|c| c.symbol().to_string()),
            Some("\u{b7}".into()),
            "{}",
            row(&buf, 0)
        );
        report_at(&mut t.app, y, 1, AgentActivity::Done, now);
        let buf = draw(&mut t.app, 160);
        let theme = crate::ui::theme::Theme::default();
        let glyph = after(&buf, 0, "yproj").map(|c| (c.symbol().to_string(), c.fg));
        assert_eq!(glyph, Some(("■".into(), theme.take)), "{}", row(&buf, 0));
    });
}

/// A done that landed out of view is seen as soon as its tab is: the chip of
/// the project switched to has nothing to say from the first frame, before
/// the next walk clears it.
#[test]
fn an_unseen_done_is_seen_from_the_first_frame_in_view() {
    with_four(|t| {
        t.reports(0, AgentActivity::Done);
        let glyph = |app: &mut App| {
            let buf = draw(app, 160);
            let at = after(&buf, 0, "xproj").map(|c| c.symbol().to_string());
            (at, row(&buf, 0))
        };
        assert_eq!(glyph(&mut t.app).0, Some("■".into()), "unseen from y");
        t.app.make_active(t.ids[0]);
        let (at, text) = glyph(&mut t.app);
        assert_eq!(at, Some("\u{b7}".into()), "seen in view: {text}");
    });
}

/// The four of the narrowing cases: `x` blocked, `z` working, `v` with nothing
/// to say.
fn narrowing(t: &mut Four) {
    t.reports(0, AgentActivity::Blocked);
    t.reports(2, AgentActivity::Working);
    t.agent(3);
}

/// The chips get a third of the bar. Where they all don't fit, the projects
/// with nothing to say go first, counted as `+N`.
#[test]
fn a_narrow_bar_drops_the_quiet_projects_first() {
    with_four(|t| {
        narrowing(t);
        let wide = bar(&mut t.app, 120);
        assert!(wide.contains("xproj ■ · yproj · zproj ● · vproj"), "{wide}");
        let text = bar(&mut t.app, 100);
        assert!(text.contains("xproj ■ · yproj · zproj ● +1"), "{text}");
        assert!(!text.contains("vproj"), "{text}");
    });
}

/// Narrower, the others collapse to where the user is among them and their
/// most urgent glyph: project 2 of 4, and something elsewhere is blocked.
#[test]
fn a_narrower_bar_collapses_the_others() {
    with_four(|t| {
        narrowing(t);
        let buf = draw(&mut t.app, 60);
        let text = row(&buf, 0);
        assert!(text.contains("yproj 2/4 ■"), "{text}");
        assert!(!text.contains("xproj"), "{text}");
        let glyph = after(&buf, 0, "2/4").map(|c| c.fg);
        assert_eq!(glyph, Some(BLOCKED_RED), "{text}");
    });
}

/// Narrowest, the bar keeps the project in view's label and a red `■` while
/// another project is blocked: that is what the chips are for, so it is never
/// dropped, however narrow the bar.
#[test]
fn the_narrowest_bar_still_shows_a_project_is_blocked() {
    with_four(|t| {
        narrowing(t);
        for w in [36, 24] {
            let buf = draw(&mut t.app, w);
            let text = row(&buf, 0);
            assert!(text.contains("yproj ■"), "{w}: {text}");
            assert!(!text.contains("2/4"), "{w}: {text}");
            let glyph = after(&buf, 0, "yproj").map(|c| c.fg);
            assert_eq!(glyph, Some(BLOCKED_RED), "{w}: {text}");
        }
    });
}

/// The title counts the blocked tabs in the other projects, for a user
/// looking at another window: `x`'s two, not `y`'s own, which its tab bar
/// shows.
#[test]
fn the_title_counts_blocked_tabs_in_the_other_projects() {
    with_four(|t| {
        assert_eq!(
            title(&mut t.app),
            format!("\u{1f336}\u{fe0f}: yproj \u{b7} {SESSION}"),
            "nothing is blocked"
        );
        t.reports(0, AgentActivity::Blocked);
        t.reports(0, AgentActivity::Blocked);
        t.reports(1, AgentActivity::Blocked);
        t.reports(2, AgentActivity::Done);
        assert_eq!(
            title(&mut t.app),
            format!("\u{1f336}\u{fe0f}: yproj (\u{25a0}2) \u{b7} {SESSION}")
        );
    });
}

/// `Space j` gives each project's row its glyph, after its number, in its
/// colour.
#[test]
fn the_switcher_shows_each_projects_glyph() {
    with_four(|t| {
        t.reports(0, AgentActivity::Blocked);
        leader(&mut t.app, 'j');
        let buf = draw(&mut t.app, 120);
        let x_row = row_with(&buf, &["[1]", "xproj"]).expect("x's row");
        let y_row = row_with(&buf, &["[2]", "yproj"]).expect("y's row");
        let glyph = |y, n| after(&buf, y, n).map(|c| (c.symbol().to_string(), c.fg));
        assert_eq!(
            glyph(x_row, "[1]"),
            Some(("■".into(), BLOCKED_RED)),
            "{}",
            row(&buf, x_row)
        );
        assert_eq!(
            glyph(y_row, "[2]").map(|(s, _)| s),
            Some(" ".into()),
            "{}",
            row(&buf, y_row)
        );
    });
}

/// With one project open, its `Space j` row reads as it always has: no glyph
/// column, whatever its agents are doing.
#[test]
fn with_one_project_the_switcher_row_reads_as_before() {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let (state_dir, home) = (base.join("state"), base.join("xproj"));
    for d in [&state_dir, &home] {
        std::fs::create_dir_all(d).unwrap();
    }
    crate::state::with_state_root(&state_dir, || {
        let mut app = App::test_app(home.clone());
        let x = app.state.active;
        app.ready_test_project(x, &home);
        agent_tab_in(&mut app, x, &home, "claude");
        agent_tab_in(&mut app, x, &home, "claude");
        report_at(&mut app, x, 1, AgentActivity::Blocked, Instant::now());
        leader(&mut app, 'j');
        let buf = draw(&mut app, 120);
        let y = row_with(&buf, &["[1]", "xproj"]).expect("x's row");
        assert!(row(&buf, y).contains("  [1]  xproj  "), "{}", row(&buf, y));
    });
}

/// A `^z`-suspended agent's tab shows 💤, so it counts for nothing, though it
/// was blocked: no glyph on its project's chip and no count in the title.
#[test]
fn a_suspended_agent_is_not_counted() {
    with_four(|t| {
        t.reports(0, AgentActivity::Blocked);
        t.app.as_project(t.ids[0], |app| {
            let tabs = app.runtime.project_mut().pane_tabs.as_mut();
            tabs.expect("a tab").tabs_mut()[0].info.suspended = true;
        });
        let title = title(&mut t.app);
        assert!(!title.contains('\u{25a0}'), "{title}");
        let buf = draw(&mut t.app, 160);
        let glyph = after(&buf, 0, "xproj").map(|c| c.symbol().to_string());
        assert_eq!(glyph, Some("\u{b7}".into()), "{}", row(&buf, 0));
    });
}
