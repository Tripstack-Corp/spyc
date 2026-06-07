//! Count-prefix accumulation and dd/zz chord tests.
#![allow(clippy::wildcard_imports)]

use super::*;

// ── count accumulation ────────────────────────────────────────

#[test]
fn bare_dd_emits_remove_prompt_without_count() {
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, key('d')), ResolverOutcome::Pending);
    assert_eq!(
        feed(&mut r, key('d')),
        ResolverOutcome::Action(Action::RemovePrompt(None))
    );
}

#[test]
fn count_dd_emits_remove_prompt_with_count() {
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, key('4')), ResolverOutcome::Pending);
    assert_eq!(feed(&mut r, key('d')), ResolverOutcome::Pending);
    assert_eq!(
        feed(&mut r, key('d')),
        ResolverOutcome::Action(Action::RemovePrompt(Some(4)))
    );
}

#[test]
fn dd_chord_cancels_on_non_d_followup() {
    let mut r = Resolver::new();
    feed(&mut r, key('d'));
    // `dx` is not a known chord; should drop without firing
    // RemovePrompt. Subsequent `d` is a fresh chord start.
    assert_eq!(feed(&mut r, key('x')), ResolverOutcome::Ignored);
    assert_eq!(feed(&mut r, key('d')), ResolverOutcome::Pending);
}

#[test]
fn zz_emits_quit() {
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, key('Z')), ResolverOutcome::Pending);
    assert_eq!(
        feed(&mut r, key('Z')),
        ResolverOutcome::Action(Action::Quit)
    );
}

#[test]
fn enter_alone_still_opens_after_d_split() {
    // Splitting `d` off from EnterOrDisplay shouldn't break Enter.
    let mut r = Resolver::new();
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(
        feed(&mut r, enter),
        ResolverOutcome::Action(Action::EnterOrDisplay)
    );
}

#[test]
fn bare_zero_is_ignored() {
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, key('0')), ResolverOutcome::Ignored);
}

#[test]
fn single_digit_starts_count() {
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, key('3')), ResolverOutcome::Pending);
}

#[test]
fn multi_digit_count() {
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, key('1')), ResolverOutcome::Pending);
    assert_eq!(feed(&mut r, key('2')), ResolverOutcome::Pending);
    // 12j should move down 12
    assert_eq!(
        feed(&mut r, key('j')),
        ResolverOutcome::Action(Action::Down(12))
    );
}

#[test]
fn count_with_trailing_zero() {
    let mut r = Resolver::new();
    feed(&mut r, key('1'));
    feed(&mut r, key('0'));
    assert_eq!(
        feed(&mut r, key('k')),
        ResolverOutcome::Action(Action::Up(10))
    );
}

#[test]
fn count_resets_after_action() {
    let mut r = Resolver::new();
    feed(&mut r, key('5'));
    feed(&mut r, key('j'));
    // Next motion without count should default to 1
    assert_eq!(
        feed(&mut r, key('j')),
        ResolverOutcome::Action(Action::Down(1))
    );
}

#[test]
fn count_applies_to_all_motions() {
    let mut r = Resolver::new();
    feed(&mut r, key('3'));
    assert_eq!(
        feed(&mut r, key('h')),
        ResolverOutcome::Action(Action::Left(3))
    );

    feed(&mut r, key('7'));
    assert_eq!(
        feed(&mut r, key('l')),
        ResolverOutcome::Action(Action::Right(7))
    );
}

#[test]
fn count_resets_on_non_motion_key() {
    let mut r = Resolver::new();
    feed(&mut r, key('5'));
    // 't' is toggle pick — doesn't use count, resets
    assert_eq!(
        feed(&mut r, key('t')),
        ResolverOutcome::Action(Action::TogglePick)
    );
    // Count should be gone
    assert_eq!(
        feed(&mut r, key('j')),
        ResolverOutcome::Action(Action::Down(1))
    );
}

#[test]
fn large_count() {
    let mut r = Resolver::new();
    for c in "999".chars() {
        feed(&mut r, key(c));
    }
    assert_eq!(
        feed(&mut r, key('j')),
        ResolverOutcome::Action(Action::Down(999))
    );
}

// ── gg sequence ───────────────────────────────────────────────
