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
/// chord actually does what it claims. This drives that end-to-end through the
/// public API: arm each chord by its entry keystroke, then feed each single-byte
/// continuation key on a fresh resolver and compare against `continuations()` —
/// an `Act` entry must resolve to its action, a `Sub` entry must open a submenu
/// (`Pending`). If a `feed` arm is re-bound without updating `continuations()`
/// (or vice-versa), this fails. Multi-byte display strings — ranges (`"1-9"`),
/// sets (`"a h"`), non-char keys (`"↓"`), and word keys (`"Space"`) — are listed
/// for the popup but not feed-verified here.
#[test]
fn chord_continuations_resolve_to_their_actions() {
    let prefixes: &[(KeyEvent, &str)] = &[
        (key(' '), "leader"),
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
        for row in rows {
            let keys = match &row {
                ChordEntry::Act(k, _) | ChordEntry::Sub(k, _) => *k,
            };
            // Only single-byte ASCII keys correspond to a `Char` we can feed.
            if keys.len() != 1 {
                continue;
            }
            let Some(ch) = keys.chars().next() else {
                continue;
            };
            let mut r2 = Resolver::new();
            feed(&mut r2, *entry);
            let got = feed(&mut r2, key(ch));
            match row {
                ChordEntry::Act(_, action) => assert_eq!(
                    got,
                    ResolverOutcome::Action(action),
                    "{name}{keys} should resolve to the action the popup advertises"
                ),
                ChordEntry::Sub(_, _) => assert_eq!(
                    got,
                    ResolverOutcome::Pending,
                    "{name}{keys} should open the submenu the popup advertises"
                ),
            }
        }
    }
}

// ── leader (Space / ^a Space) ─────────────────────────────────

#[test]
fn space_enters_leader() {
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, key(' ')), ResolverOutcome::Pending);
    assert!(r.is_pending());
}

#[test]
fn space_p_jumps_project_home() {
    let mut r = Resolver::new();
    feed(&mut r, key(' '));
    assert_eq!(
        feed(&mut r, key('p')),
        ResolverOutcome::Action(Action::JumpProjectHome)
    );
}

#[test]
fn space_w_opens_worktree_submenu_then_new() {
    let mut r = Resolver::new();
    feed(&mut r, key(' '));
    assert_eq!(feed(&mut r, key('w')), ResolverOutcome::Pending);
    assert_eq!(
        feed(&mut r, key('n')),
        ResolverOutcome::Action(Action::WorktreeNew)
    );
}

#[test]
fn ctrl_a_space_enters_leader_from_pane_prefix() {
    // The pane-focus path into the global menu: `^a` then Space.
    let mut r = Resolver::new();
    feed(&mut r, ctrl('w'));
    assert_eq!(feed(&mut r, key(' ')), ResolverOutcome::Pending);
    assert_eq!(
        feed(&mut r, key('p')),
        ResolverOutcome::Action(Action::JumpProjectHome)
    );
}

#[test]
fn gh_no_longer_jumps_project_home() {
    // `gh` was dropped in favor of the leader (`Space p`); `gw` stays.
    let mut r = Resolver::new();
    feed(&mut r, key('g'));
    assert_eq!(feed(&mut r, key('h')), ResolverOutcome::Ignored);
}

#[test]
fn demoted_g_chord_keys_are_unbound() {
    // gy (:graveyard), gU (:whoami), gs (:sort) demoted to `:`-only in the
    // keymap slim; gw / gP / gd … stay on the g chord.
    for c in ['y', 'U', 's'] {
        let mut r = Resolver::new();
        feed(&mut r, key('g'));
        assert_eq!(
            feed(&mut r, key(c)),
            ResolverOutcome::Ignored,
            "`g{c}` should be unbound after the demotion"
        );
    }
}

/// The documented binding taxonomy (DESIGN.md): the leader namespace carries
/// only Global/Meta actions, and the `^a` pane prefix only Pane/Meta. This
/// guards against drift — e.g. a pane op accidentally added to the leader, or
/// a frame/global op landing on `^a`. Drives it through the resolver's own
/// `continuations()` so it tracks the real bindings.
#[test]
fn leader_and_pane_namespaces_respect_tiers() {
    use crate::keymap::Tier;

    // The `Act` actions reachable after arming the keys in `entry`.
    fn actions_after(entry: &[crossterm::event::KeyEvent]) -> Vec<Action> {
        let mut r = Resolver::new();
        for ev in entry {
            feed(&mut r, *ev);
        }
        r.continuations()
            .into_iter()
            .filter_map(|e| match e {
                ChordEntry::Act(_, a) => Some(a),
                ChordEntry::Sub(..) => None,
            })
            .collect()
    }

    // Leader (`Space`) and its worktree submenu (`Space w`) → Global/Meta only.
    let mut leader = actions_after(&[key(' ')]);
    leader.extend(actions_after(&[key(' '), key('w')]));
    for a in leader {
        assert!(
            matches!(a.tier(), Tier::Global | Tier::Meta),
            "leader action {a:?} is {:?}; the leader namespace is Global/Meta only",
            a.tier()
        );
    }

    // Pane prefix (`^a`) → Pane/Meta only.
    for a in actions_after(&[ctrl('w')]) {
        assert!(
            matches!(a.tier(), Tier::Pane | Tier::Meta),
            "^a action {a:?} is {:?}; the pane namespace is Pane/Meta only",
            a.tier()
        );
    }
}
