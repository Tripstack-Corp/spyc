//! Ctrl-w pane/tab and W worktree chord tests.
#![allow(clippy::wildcard_imports)]

use super::*;

#[test]
fn ctrl_w_enters_pane_pending() {
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, ctrl('w')), ResolverOutcome::Pending);
    assert!(r.is_pending());
}

#[test]
fn ctrl_w_j_focuses_down() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('j')),
        ResolverOutcome::Action(Action::PaneFocusDown)
    );
}

#[test]
fn ctrl_w_k_focuses_up() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('k')),
        ResolverOutcome::Action(Action::PaneFocusUp)
    );
}

#[test]
fn ctrl_w_plus_grows_pane() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('+')),
        ResolverOutcome::Action(Action::PaneGrow)
    );
}

#[test]
fn ctrl_w_minus_shrinks_pane() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('-')),
        ResolverOutcome::Action(Action::PaneShrink)
    );
}

#[test]
fn ctrl_w_n_next_tab() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('n')),
        ResolverOutcome::Action(Action::PaneNextTab)
    );
}

#[test]
fn ctrl_a_c_new_tab() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('a'));
    assert_eq!(
        feed(&mut r, key('c')),
        ResolverOutcome::Action(Action::PaneNewTab)
    );
}

#[test]
fn ctrl_w_x_close_tab() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('x')),
        ResolverOutcome::Action(Action::PaneCloseTab)
    );
}

#[test]
fn ctrl_w_digit_switches_tab() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('3')),
        ResolverOutcome::Action(Action::PaneTabByIndex(3))
    );
}

#[test]
fn ctrl_w_bracket_navigates_tabs() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key(']')),
        ResolverOutcome::Action(Action::PaneNextTab)
    );

    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('[')),
        ResolverOutcome::Action(Action::PanePrevTab)
    );
}

#[test]
fn ctrl_w_s_sends_selection() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('s')),
        ResolverOutcome::Action(Action::PaneSendSelection)
    );
}

#[test]
fn ctrl_a_down_sends_literal_prefix() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('a'));
    assert_eq!(
        feed(&mut r, special(KeyCode::Down)),
        ResolverOutcome::Action(Action::PaneSendPrefix)
    );
    // The chord must reset so a following Down is a plain motion again.
    assert!(!r.is_pending());
}

#[test]
fn ctrl_w_v_enters_scroll() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('v')),
        ResolverOutcome::Action(Action::PaneScrollEnter)
    );
}

#[test]
fn ctrl_w_backslash_toggles_pane() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('\\')),
        ResolverOutcome::Action(Action::TogglePane)
    );
}

#[test]
fn ctrl_w_r_renames_tab() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('r')),
        ResolverOutcome::Action(Action::PaneRenameTab)
    );
}

#[test]
fn ctrl_w_p_prev_tab() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('p')),
        ResolverOutcome::Action(Action::PanePrevTab)
    );
}

#[test]
fn ctrl_a_shift_p_pipes_content() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('a'));
    assert_eq!(
        feed(&mut r, key('P')),
        ResolverOutcome::Action(Action::PanePipeContent)
    );
}

#[test]
fn ctrl_w_i_pipes_inventory() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('i')),
        ResolverOutcome::Action(Action::PanePipeInventory)
    );
}

#[test]
fn ctrl_w_z_zooms_pane() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, key('z')),
        ResolverOutcome::Action(Action::TogglePaneZoom)
    );
}

#[test]
fn ctrl_w_unknown_is_ignored() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(feed(&mut r, key('q')), ResolverOutcome::Ignored);
}

// ── Ctrl held through the second key (rapid chord) ────────────
// Regression (key-trace confirmed): firing `^a n` fast leaves Ctrl down,
// so the second key arrives as `^n` (Char('n') + CONTROL), not bare `n`.
// The pane chord must still resolve — the generic Ctrl block used to eat
// these as an unknown control code and reset the pending `^a-` chord, so
// the tab switch was silently lost. screen treats `^a ^n` == `^a n`.

#[test]
fn ctrl_a_then_ctrl_n_still_next_tab() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('a'));
    assert_eq!(
        feed(&mut r, ctrl('n')),
        ResolverOutcome::Action(Action::PaneNextTab)
    );
}

#[test]
fn ctrl_a_then_ctrl_p_still_prev_tab() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('a'));
    assert_eq!(
        feed(&mut r, ctrl('p')),
        ResolverOutcome::Action(Action::PanePrevTab)
    );
}

#[test]
fn ctrl_w_then_ctrl_n_still_next_tab() {
    // The `^w` alias prefix behaves the same.
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(
        feed(&mut r, ctrl('n')),
        ResolverOutcome::Action(Action::PaneNextTab)
    );
}

#[test]
fn ctrl_a_then_ctrl_c_still_new_tab() {
    // `^a ^c` == `^a c` (screen-style new window), Ctrl held or not.
    let mut r = Resolver::new();
    feed(&mut r, ctrl('a'));
    assert_eq!(
        feed(&mut r, ctrl('c')),
        ResolverOutcome::Action(Action::PaneNewTab)
    );
}

#[test]
fn ctrl_a_then_ctrl_a_is_last_tab_not_focus_down() {
    // `^a ^a` must stay PaneLastTab (intercepted before the pane block),
    // NOT fall into the `'a'` focus-down arm now that the block matches
    // code-only and runs ahead of the generic Ctrl block.
    let mut r = Resolver::new();
    feed(&mut r, ctrl('a'));
    assert_eq!(
        feed(&mut r, ctrl('a')),
        ResolverOutcome::Action(Action::PaneLastTab)
    );
}

// ── W (worktree) prefix ───────────────────────────────────────

#[test]
fn cap_w_enters_worktree_pending() {
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, key('W')), ResolverOutcome::Pending);
    assert!(r.is_pending());
}

#[test]
fn w_l_lists_worktrees() {
    let mut r = Resolver::new();
    feed(&mut r, key('W'));
    assert_eq!(
        feed(&mut r, key('l')),
        ResolverOutcome::Action(Action::WorktreeList)
    );
}

#[test]
fn w_n_creates_worktree() {
    let mut r = Resolver::new();
    feed(&mut r, key('W'));
    assert_eq!(
        feed(&mut r, key('n')),
        ResolverOutcome::Action(Action::WorktreeNew)
    );
}

#[test]
fn w_d_deletes_worktree() {
    let mut r = Resolver::new();
    feed(&mut r, key('W'));
    assert_eq!(
        feed(&mut r, key('d')),
        ResolverOutcome::Action(Action::WorktreeDelete)
    );
}

#[test]
fn w_unknown_is_ignored() {
    let mut r = Resolver::new();
    feed(&mut r, key('W'));
    assert_eq!(feed(&mut r, key('z')), ResolverOutcome::Ignored);
}

// ── control codes ─────────────────────────────────────────────
