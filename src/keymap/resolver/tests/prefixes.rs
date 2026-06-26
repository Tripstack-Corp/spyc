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
