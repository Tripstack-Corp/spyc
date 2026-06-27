//! g / m / quote prefix-chord tests.
#![allow(clippy::wildcard_imports)]

use super::*;

#[test]
fn g_enters_pending() {
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, key('g')), ResolverOutcome::Pending);
    assert!(r.is_pending());
}

#[test]
fn gg_goes_to_first() {
    let mut r = Resolver::new();
    feed(&mut r, key('g'));
    assert_eq!(
        feed(&mut r, key('g')),
        ResolverOutcome::Action(Action::GotoFirst)
    );
    assert!(!r.is_pending());
}

#[test]
fn gd_is_git_diff() {
    let mut r = Resolver::new();
    feed(&mut r, key('g'));
    assert_eq!(
        feed(&mut r, key('d')),
        ResolverOutcome::Action(Action::GitDiff)
    );
}

#[test]
fn gb_is_git_blame() {
    let mut r = Resolver::new();
    feed(&mut r, key('g'));
    assert_eq!(
        feed(&mut r, key('b')),
        ResolverOutcome::Action(Action::GitBlame)
    );
}

#[test]
fn g_cap_d_is_git_diff_cached() {
    let mut r = Resolver::new();
    feed(&mut r, key('g'));
    assert_eq!(
        feed(&mut r, key('D')),
        ResolverOutcome::Action(Action::GitDiffCached)
    );
}

#[test]
fn gu_is_git_diff_unstaged() {
    let mut r = Resolver::new();
    feed(&mut r, key('g'));
    assert_eq!(
        feed(&mut r, key('u')),
        ResolverOutcome::Action(Action::GitDiffUnstaged)
    );
}

#[test]
fn gf_is_goto_file() {
    let mut r = Resolver::new();
    feed(&mut r, key('g'));
    assert_eq!(
        feed(&mut r, key('f')),
        ResolverOutcome::Action(Action::GotoFile)
    );
}

#[test]
fn g_cap_f_is_goto_file_line() {
    let mut r = Resolver::new();
    feed(&mut r, key('g'));
    assert_eq!(
        feed(&mut r, key('F')),
        ResolverOutcome::Action(Action::GotoFileLine)
    );
}

#[test]
fn g_followed_by_unknown_is_ignored() {
    let mut r = Resolver::new();
    feed(&mut r, key('g'));
    assert_eq!(feed(&mut r, key('x')), ResolverOutcome::Ignored);
    assert!(!r.is_pending());
}

#[test]
fn cap_g_goes_to_last() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, key('G')),
        ResolverOutcome::Action(Action::GotoLast)
    );
}

// ── marks ─────────────────────────────────────────────────────

#[test]
fn m_enters_mark_pending() {
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, key('m')), ResolverOutcome::Pending);
    assert!(r.is_pending());
}

#[test]
fn m_a_sets_mark() {
    let mut r = Resolver::new();
    feed(&mut r, key('m'));
    assert_eq!(
        feed(&mut r, key('a')),
        ResolverOutcome::Action(Action::SetMark('a'))
    );
}

#[test]
fn m_z_sets_mark() {
    let mut r = Resolver::new();
    feed(&mut r, key('m'));
    assert_eq!(
        feed(&mut r, key('z')),
        ResolverOutcome::Action(Action::SetMark('z'))
    );
}

#[test]
fn m_nonletter_is_ignored() {
    let mut r = Resolver::new();
    feed(&mut r, key('m'));
    assert_eq!(feed(&mut r, key('1')), ResolverOutcome::Ignored);
}

#[test]
fn quote_a_jumps_to_mark() {
    let mut r = Resolver::new();
    feed(&mut r, key('\''));
    assert_eq!(
        feed(&mut r, key('a')),
        ResolverOutcome::Action(Action::JumpMark('a'))
    );
}

#[test]
fn quote_quote_jumps_prev_dir() {
    let mut r = Resolver::new();
    feed(&mut r, key('\''));
    assert_eq!(
        feed(&mut r, key('\'')),
        ResolverOutcome::Action(Action::JumpPrevDir)
    );
}

#[test]
fn quote_nonletter_is_ignored() {
    let mut r = Resolver::new();
    feed(&mut r, key('\''));
    assert_eq!(feed(&mut r, key('3')), ResolverOutcome::Ignored);
}

// ── Ctrl-W pane commands ──────────────────────────────────────

// ── Ctrl-s second-commander chord ─────────────────────────────

#[test]
fn ctrl_s_enters_pending() {
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, ctrl('s')), ResolverOutcome::Pending);
    assert!(r.is_pending());
}

#[test]
fn ctrl_s_n_opens_second_commander() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('s'));
    assert_eq!(
        feed(&mut r, key('n')),
        ResolverOutcome::Action(Action::OpenSecondCommander)
    );
}

#[test]
fn ctrl_s_x_closes_second_commander() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('s'));
    assert_eq!(
        feed(&mut r, key('x')),
        ResolverOutcome::Action(Action::CloseSecondCommander)
    );
}

#[test]
fn ctrl_s_unknown_key_is_ignored() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('s'));
    assert_eq!(feed(&mut r, key('q')), ResolverOutcome::Ignored);
    assert!(!r.is_pending(), "the chord resets after an unknown key");
}

// ── which-key continuations (the chord-hint popup's data) ──────

/// The which-key popup is only trustworthy if every key it advertises for a
/// chord actually fires the action it claims. This drives that end-to-end
/// through the public API: arm each chord by its entry keystroke, then feed
/// each single-byte continuation key on a fresh resolver and compare against
/// `continuations()`'s listed action. If a `feed` arm is re-bound without
/// updating `continuations()` (or vice-versa), this fails. Multi-byte display
/// strings — ranges (`"1-9"`), sets (`"a h"`), and non-char keys (`"↓"`) —
/// are listed for the popup but not feed-verified here.
#[test]
fn chord_continuations_resolve_to_their_actions() {
    let prefixes: &[(KeyEvent, &str)] = &[
        (key('g'), "g"),
        (ctrl('w'), "^a"),
        (ctrl('s'), "^s"),
        (key('W'), "W"),
        (key('H'), "H"),
        (key('y'), "y"),
        (key('m'), "m"),
        (key('\''), "'"),
        (key('['), "["),
        (key(']'), "]"),
        (key('d'), "d"),
        (key('Z'), "Z"),
    ];
    for (entry, name) in prefixes {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, *entry),
            ResolverOutcome::Pending,
            "{name} should arm a chord"
        );
        let rows = r.continuations();
        assert!(
            !rows.is_empty(),
            "{name} chord has no continuations for the popup"
        );
        for (keys, action) in rows {
            // Only single-byte ASCII keys correspond to a `Char` we can feed.
            if keys.len() != 1 {
                continue;
            }
            let Some(ch) = keys.chars().next() else {
                continue;
            };
            let mut r2 = Resolver::new();
            feed(&mut r2, *entry);
            assert_eq!(
                feed(&mut r2, key(ch)),
                ResolverOutcome::Action(action),
                "{name}{keys} should resolve to the action the popup advertises"
            );
        }
    }
}
