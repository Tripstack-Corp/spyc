//! `:setenv`: given `NAME=VALUE` it sets the variable, and bare it asks.

use super::*;

fn app() -> App {
    App::test_app(std::env::temp_dir())
}

#[test]
fn setenv_with_an_assignment_sets_it_without_asking() {
    let mut app = app();
    let fx = app.dispatch_command("setenv SPYC_SETENV_ARG_TEST=1");
    assert!(fx.is_empty(), "{fx:?}");
    assert!(
        !matches!(app.state.mode, Mode::Prompting(_)),
        "a prompt opened for a value already given"
    );
    assert_eq!(
        crate::envset::var("SPYC_SETENV_ARG_TEST").as_deref(),
        Some("1")
    );
    assert_eq!(app.flash_text(), Some("setenv SPYC_SETENV_ARG_TEST=1"));
}

/// The value is everything after the first `=`, spaces and `=` included.
#[test]
fn setenv_keeps_the_whole_value() {
    let mut app = app();
    app.dispatch_command("setenv SPYC_SETENV_VALUE_TEST=a b=c");
    assert_eq!(
        crate::envset::var("SPYC_SETENV_VALUE_TEST").as_deref(),
        Some("a b=c")
    );
}

#[test]
fn bare_setenv_asks_for_the_assignment() {
    let mut app = app();
    app.dispatch_command("setenv");
    assert!(
        matches!(
            app.state.mode,
            Mode::Prompting(Prompt {
                kind: PromptKind::SetEnv,
                ..
            })
        ),
        "no setenv prompt"
    );
}

#[test]
fn setenv_without_an_equals_sign_says_so() {
    let mut app = app();
    app.dispatch_command("setenv SPYC_SETENV_NO_VALUE_TEST");
    assert!(!matches!(app.state.mode, Mode::Prompting(_)));
    assert_eq!(app.flash_text(), Some("setenv: expected NAME=VALUE"));
    assert!(crate::envset::var("SPYC_SETENV_NO_VALUE_TEST").is_none());
}
