//! Pending-display, user-binding precedence, and function-key tests.
#![allow(clippy::wildcard_imports)]

use super::*;

#[test]
fn pending_display_idle() {
    let r = Resolver::new();
    assert!(r.pending_display().is_none());
}

#[test]
fn pending_display_count_only() {
    let mut r = Resolver::new();
    feed(&mut r, key('5'));
    assert_eq!(r.pending_display(), Some("5".to_string()));
}

#[test]
fn pending_display_g() {
    let mut r = Resolver::new();
    feed(&mut r, key('g'));
    assert_eq!(r.pending_display(), Some("g-".to_string()));
}

#[test]
fn pending_display_count_plus_g() {
    let mut r = Resolver::new();
    feed(&mut r, key('5'));
    feed(&mut r, key('g'));
    // count is 5 but g clears count context — actually let's check
    // Actually looking at the code: g doesn't go through count path,
    // it sets pending = G. The count stays.
    // Wait, actually 'g' enters PendingSeq::G but doesn't touch count.
    // So pending_display should be "5g-"
    assert_eq!(r.pending_display(), Some("5g-".to_string()));
}

#[test]
fn pending_display_mark() {
    let mut r = Resolver::new();
    feed(&mut r, key('m'));
    assert_eq!(r.pending_display(), Some("m-".to_string()));
}

#[test]
fn pending_display_jump_mark() {
    let mut r = Resolver::new();
    feed(&mut r, key('\''));
    assert_eq!(r.pending_display(), Some("'-".to_string()));
}

#[test]
fn pending_display_ctrl_a() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('a'));
    assert_eq!(r.pending_display(), Some("^a-".to_string()));
}

#[test]
fn pending_display_worktree() {
    let mut r = Resolver::new();
    feed(&mut r, key('W'));
    assert_eq!(r.pending_display(), Some("W-".to_string()));
}

// ── user keymap override ──────────────────────────────────────

#[test]
fn user_binding_wins_over_builtin() {
    let mut r = Resolver::new();
    let user = UserKeymap::from_bindings(vec![crate::keymap::user::UserBinding {
        chord: crate::keymap::user::KeyChord::Char('j'),
        action: BoundAction::UnixCmd("my-cmd".to_string()),
    }]);
    let out = r.feed(key('j'), &user);
    assert_eq!(
        out,
        ResolverOutcome::User(BoundAction::UnixCmd("my-cmd".to_string()))
    );
}

#[test]
fn user_binding_resets_pending() {
    let mut r = Resolver::new();
    feed(&mut r, key('g')); // enter pending G
    assert!(r.is_pending());
    let user = UserKeymap::from_bindings(vec![crate::keymap::user::UserBinding {
        chord: crate::keymap::user::KeyChord::Char('g'),
        action: BoundAction::Plain(Action::Noop),
    }]);
    r.feed(key('g'), &user);
    assert!(!r.is_pending());
}

// Regression: when a built-in chord prefix is pending (^a, ], y, …),
// user bindings for the *second* key must NOT preempt the chord.
// Reported as `^a-n` / `^a-p` flashing the pending indicator and
// then doing nothing because the user had `n`/`p` bound elsewhere.

#[test]
fn user_binding_for_n_does_not_preempt_ctrl_a_n() {
    let mut r = Resolver::new();
    let user = UserKeymap::from_bindings(vec![crate::keymap::user::UserBinding {
        chord: crate::keymap::user::KeyChord::Char('n'),
        action: BoundAction::UnixCmd("nope".to_string()),
    }]);
    // ^a primes pending=W; user binding for `n` must not win.
    r.feed(ctrl('a'), &user);
    let out = r.feed(key('n'), &user);
    assert_eq!(out, ResolverOutcome::Action(Action::PaneNextTab));
    assert!(!r.is_pending());
}

#[test]
fn user_binding_for_p_does_not_preempt_ctrl_a_p() {
    let mut r = Resolver::new();
    let user = UserKeymap::from_bindings(vec![crate::keymap::user::UserBinding {
        chord: crate::keymap::user::KeyChord::Char('p'),
        action: BoundAction::UnixCmd("nope".to_string()),
    }]);
    r.feed(ctrl('a'), &user);
    let out = r.feed(key('p'), &user);
    assert_eq!(out, ResolverOutcome::Action(Action::PanePrevTab));
}

#[test]
fn ctrl_a_ctrl_a_is_last_tab() {
    let mut r = Resolver::new();
    let user = empty_keymap();
    // ^a primes pending=W; a second ^a is "last window".
    r.feed(ctrl('a'), &user);
    let out = r.feed(ctrl('a'), &user);
    assert_eq!(out, ResolverOutcome::Action(Action::PaneLastTab));
    assert!(!r.is_pending());
}

#[test]
fn ctrl_a_plain_a_is_focus_down() {
    let mut r = Resolver::new();
    let user = empty_keymap();
    // Plain `a` (no ctrl) after the prefix stays focus-down — only
    // the ctrl-modified second key is last-tab.
    r.feed(ctrl('a'), &user);
    let out = r.feed(key('a'), &user);
    assert_eq!(out, ResolverOutcome::Action(Action::PaneFocusDown));
}

#[test]
fn user_binding_for_g_does_not_preempt_bracket_g() {
    let mut r = Resolver::new();
    let user = UserKeymap::from_bindings(vec![crate::keymap::user::UserBinding {
        chord: crate::keymap::user::KeyChord::Char('g'),
        action: BoundAction::UnixCmd("nope".to_string()),
    }]);
    r.feed(key(']'), &user);
    let out = r.feed(key('g'), &user);
    assert_eq!(out, ResolverOutcome::Action(Action::JumpNextGitChange));
}

#[test]
fn user_binding_for_y_second_key_does_not_preempt_yank_chord() {
    let mut r = Resolver::new();
    let user = UserKeymap::from_bindings(vec![crate::keymap::user::UserBinding {
        chord: crate::keymap::user::KeyChord::Char('p'),
        action: BoundAction::UnixCmd("nope".to_string()),
    }]);
    r.feed(key('y'), &user);
    let out = r.feed(key('p'), &user);
    assert_eq!(out, ResolverOutcome::Action(Action::YankPrompt));
}

#[test]
fn user_binding_for_digit_does_not_preempt_harpoon_chord() {
    let mut r = Resolver::new();
    let user = UserKeymap::from_bindings(vec![crate::keymap::user::UserBinding {
        chord: crate::keymap::user::KeyChord::Char('1'),
        action: BoundAction::UnixCmd("nope".to_string()),
    }]);
    r.feed(key('H'), &user);
    let out = r.feed(key('1'), &user);
    assert_eq!(out, ResolverOutcome::Action(Action::HarpoonJump(1)));
}

#[test]
fn user_binding_for_letter_does_not_preempt_mark_chord() {
    let mut r = Resolver::new();
    let user = UserKeymap::from_bindings(vec![crate::keymap::user::UserBinding {
        chord: crate::keymap::user::KeyChord::Char('a'),
        action: BoundAction::UnixCmd("nope".to_string()),
    }]);
    r.feed(key('m'), &user);
    let out = r.feed(key('a'), &user);
    assert_eq!(out, ResolverOutcome::Action(Action::SetMark('a')));
}

#[test]
fn user_binding_for_letter_does_not_preempt_worktree_chord() {
    let mut r = Resolver::new();
    let user = UserKeymap::from_bindings(vec![crate::keymap::user::UserBinding {
        chord: crate::keymap::user::KeyChord::Char('l'),
        action: BoundAction::UnixCmd("nope".to_string()),
    }]);
    r.feed(key('W'), &user);
    let out = r.feed(key('l'), &user);
    assert_eq!(out, ResolverOutcome::Action(Action::WorktreeList));
}

#[test]
fn g_chord_remains_user_overridable() {
    // Counter-test: `g` is the deliberate exception. A user binding
    // for the second char of a g-chord still wins.
    let mut r = Resolver::new();
    let user = UserKeymap::from_bindings(vec![crate::keymap::user::UserBinding {
        chord: crate::keymap::user::KeyChord::Char('d'),
        action: BoundAction::UnixCmd("custom-d".to_string()),
    }]);
    r.feed(key('g'), &user);
    let out = r.feed(key('d'), &user);
    assert_eq!(
        out,
        ResolverOutcome::User(BoundAction::UnixCmd("custom-d".to_string()))
    );
}

// ── special keys ──────────────────────────────────────────────

#[test]
fn f1_is_help() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, special(KeyCode::F(1))),
        ResolverOutcome::Action(Action::Help)
    );
}

#[test]
fn f9_resumes_pane() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, special(KeyCode::F(9))),
        ResolverOutcome::Action(Action::ResumePane)
    );
}

#[test]
fn f10_toggles_pane() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, special(KeyCode::F(10))),
        ResolverOutcome::Action(Action::TogglePane)
    );
}

#[test]
fn page_up_down() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, special(KeyCode::PageUp)),
        ResolverOutcome::Action(Action::PageUp)
    );
    assert_eq!(
        feed(&mut r, special(KeyCode::PageDown)),
        ResolverOutcome::Action(Action::PageDown)
    );
}

#[test]
fn home_key() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, special(KeyCode::Home)),
        ResolverOutcome::Action(Action::Home)
    );
}

#[test]
fn enter_is_enter_or_display() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, special(KeyCode::Enter)),
        ResolverOutcome::Action(Action::EnterOrDisplay)
    );
}

#[test]
fn unknown_key_is_ignored() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, special(KeyCode::F(20))),
        ResolverOutcome::Ignored
    );
}

// ── property tests ────────────────────────────────────────────
//
// Count machinery invariants. Bounded to 1-4 leading-non-zero
// digits so values stay well below u32::MAX (the underlying
// multiply isn't checked; that's a separate concern).

proptest::proptest! {
    /// Feeding N digits (first non-zero) followed by a motion key
    /// produces the motion action with count == the parsed integer.
    #[test]
    fn count_digits_compose_to_parsed_integer(
        first in 1u32..=9,
        rest in proptest::collection::vec(0u32..=9, 0..=3),
    ) {
        let mut digits = String::new();
        digits.push(char::from_digit(first, 10).unwrap());
        let mut value: u32 = first;
        for d in rest {
            digits.push(char::from_digit(d, 10).unwrap());
            value = value * 10 + d;
        }
        let mut r = Resolver::new();
        for c in digits.chars() {
            feed(&mut r, key(c));
        }
        let out = feed(&mut r, key('j'));
        proptest::prop_assert_eq!(out, ResolverOutcome::Action(Action::Down(value as usize)));
        // And the count is consumed: a follow-up motion is count-1.
        let next = feed(&mut r, key('j'));
        proptest::prop_assert_eq!(next, ResolverOutcome::Action(Action::Down(1)));
    }

    /// Bare `0` is ignored and leaves no pending state, regardless
    /// of how many leading zeros the user types.
    #[test]
    fn leading_zeros_are_ignored(zeros in 1usize..=5) {
        let mut r = Resolver::new();
        for _ in 0..zeros {
            let out = feed(&mut r, key('0'));
            proptest::prop_assert_eq!(out, ResolverOutcome::Ignored);
        }
        proptest::prop_assert!(!r.is_pending());
        // A motion right after must still default to count 1.
        proptest::prop_assert_eq!(
            feed(&mut r, key('j')),
            ResolverOutcome::Action(Action::Down(1))
        );
    }
}
