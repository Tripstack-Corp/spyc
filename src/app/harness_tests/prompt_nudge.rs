//! A question that waits for its answer pulses the screen's border when it
//! is given anything else: a key it re-asks on, or a click that answers
//! nothing. Without it a prompt in the one-line bar reads as spyc being stuck.

use super::clicks::{click, drawn_at, listing};
use super::*;

fn asking(app: &mut App, kind: PromptKind) {
    app.state.mode = Mode::Prompting(Prompt::simple(kind, "a question".to_string()));
}

fn skill_update() -> PromptKind {
    PromptKind::SkillUpdate {
        fingerprint: "fp".to_string(),
        overwrites_edits: false,
    }
}

fn still_asking(app: &App) -> bool {
    matches!(app.state.mode, Mode::Prompting(_))
}

fn pulsing(app: &App) -> bool {
    app.view.visual_bell.is_some()
}

#[test]
#[ignore = "red: a stray key only re-asks"]
fn a_stray_key_at_the_skill_update_pulses_the_border() {
    listing(|app, _| {
        asking(app, skill_update());

        app.handle_key(key('x')).unwrap();

        assert!(still_asking(app), "the question stays");
        assert!(pulsing(app), "and the border pulses");
    });
}

#[test]
#[ignore = "red: a stray key only re-asks"]
fn a_stray_key_at_the_startup_tabs_consent_pulses_the_border() {
    listing(|app, work| {
        asking(
            app,
            PromptKind::ProjectTabsConsent {
                source: work.join(".spycrc.toml"),
                tabs: Vec::new(),
            },
        );

        app.handle_key(key('x')).unwrap();

        assert!(still_asking(app), "the question stays");
        assert!(pulsing(app), "and the border pulses");
    });
}

/// A click on a file row while the agent wiring pop-up asks answers nothing:
/// the pop-up stays and the border pulses.
#[test]
#[ignore = "red: a click beside a question is swallowed"]
fn a_click_away_from_a_question_pulses_the_border() {
    listing(|app, _| {
        let docs = drawn_at(app, "docs");
        asking(
            app,
            PromptKind::HookConsent {
                agent: crate::state::sessions::AgentKind::Claude,
            },
        );

        click(app, docs);

        assert!(still_asking(app), "the question stays");
        assert!(pulsing(app), "and the border pulses");
    });
}

/// The answer itself closes the question without a pulse.
#[test]
fn answering_does_not_pulse() {
    listing(|app, _| {
        asking(app, skill_update());

        app.handle_key(key('n')).unwrap();

        assert!(!still_asking(app));
        assert!(!pulsing(app));
    });
}

/// A typed prompt takes any key as text: no pulse.
#[test]
fn typing_into_a_line_prompt_does_not_pulse() {
    listing(|app, _| {
        app.handle_key(key(':')).unwrap();

        app.handle_key(key('x')).unwrap();

        assert!(still_asking(app));
        assert!(!pulsing(app));
    });
}
