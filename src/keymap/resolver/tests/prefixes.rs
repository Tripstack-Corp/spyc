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

// ── Ctrl-s vertical-split chord ───────────────────────────────

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

/// `^s |` is the split's own open/close key (`^a |` is the alias), and `^s f`
/// flips its height — the two halves the old single cycling key conflated.
#[test]
fn ctrl_s_hosts_the_split_toggles() {
    let mut r = Resolver::new();
    feed(&mut r, ctrl('s'));
    assert_eq!(
        feed(&mut r, key('|')),
        ResolverOutcome::Action(Action::VsplitToggle)
    );
    feed(&mut r, ctrl('s'));
    assert_eq!(
        feed(&mut r, key('f')),
        ResolverOutcome::Action(Action::VsplitToggleHeight)
    );
    // The long-standing `^a |` still opens/closes.
    feed(&mut r, ctrl('a'));
    assert_eq!(
        feed(&mut r, key('|')),
        ResolverOutcome::Action(Action::VsplitToggle)
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
/// (or vice-versa), this fails. A label naming several keys (`"a h"`, `"\\ C"`)
/// has each of them fed, so an alias can't ride along unverified; what stays
/// unverified is a label with no single key to feed — a range (`"1-9"`, `"a-z"`),
/// a non-char key (`"↓"`), or a word key (`"Space"`).
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
            // A label may name SEVERAL keys for one action (`"\\ C"` for the
            // pane toggle, `"a h"` for vsplit-focus-left), and every one of them
            // is a promise the popup makes. Each single-byte ASCII token is
            // fed, so an alias can't drift unverified just by sharing a row —
            // which is what happened when `"\\"` grew a `C` and silently left
            // the set this guard checks. A range (`"1-9"`), a word key
            // (`"Space"`) or a non-ASCII key (`"↓"`) is one token with no
            // `Char` to feed, and is skipped as before.
            for tok in keys.split_whitespace() {
                if tok.len() != 1 {
                    continue;
                }
                let Some(ch) = tok.chars().next() else {
                    continue;
                };
                let mut r2 = Resolver::new();
                feed(&mut r2, *entry);
                let got = feed(&mut r2, key(ch));
                match &row {
                    ChordEntry::Act(_, action) => assert_eq!(
                        got,
                        ResolverOutcome::Action(action.clone()),
                        "{name}{tok} should resolve to the action the popup advertises"
                    ),
                    ChordEntry::Sub(_, _) => assert_eq!(
                        got,
                        ResolverOutcome::Pending,
                        "{name}{tok} should open the submenu the popup advertises"
                    ),
                }
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

/// The about page sits next to `Space ?` help — both informational pages on the
/// leader, reachable from pane focus as `^a Space a`.
#[test]
fn space_a_opens_about() {
    let mut r = Resolver::new();
    feed(&mut r, key(' '));
    assert_eq!(
        feed(&mut r, key('a')),
        ResolverOutcome::Action(Action::About)
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
    // `gh` was dropped in favour of the leader (`Space p`); `gw` stays.
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

// ── the chord tree, walked rather than listed ─────────────────────────────

/// Keys a chord state might bind: printable ASCII plus the arrows `^a ↓` needs.
///
/// Deliberately no ctrl-modified keys — inside a pending chord those re-enter a
/// prefix rather than completing one, so feeding them would exercise a different
/// transition than the popup describes.
fn candidate_keys() -> Vec<crossterm::event::KeyEvent> {
    let mut out: Vec<_> = (0x20u8..=0x7e).map(|b| key(b as char)).collect();
    out.extend([KeyCode::Up, KeyCode::Down, KeyCode::Left, KeyCode::Right].map(special));
    out
}

/// The single key a `ChordEntry` label stands for, when it names exactly one.
fn hint_key(label: &str) -> Option<crossterm::event::KeyEvent> {
    match label {
        "Space" => Some(key(' ')),
        "↓" => Some(special(KeyCode::Down)),
        _ => {
            let mut cs = label.chars();
            let c = cs.next()?;
            cs.next().is_none().then(|| key(c))
        }
    }
}

/// Every chord state whose popup shows something, found by walking the tree.
///
/// Depth 1 is any single key that leaves the resolver pending with a non-empty
/// `continuations()`; deeper states come from following each `Sub` entry. Walked
/// rather than listed because the tier guard below used to hardcode `Space w` as
/// the only submenu it descended into — complete the day it was written, and
/// silently incomplete for any submenu added afterwards.
fn chord_states() -> Vec<(String, Vec<crossterm::event::KeyEvent>)> {
    let mut out: Vec<(String, Vec<crossterm::event::KeyEvent>)> = Vec::new();
    let mut queue: Vec<(String, Vec<crossterm::event::KeyEvent>)> = Vec::new();

    for ev in candidate_keys() {
        let mut r = Resolver::new();
        if feed(&mut r, ev) != ResolverOutcome::Pending || r.continuations().is_empty() {
            continue;
        }
        let label = match ev.code {
            KeyCode::Char(c) => c.to_string(),
            other => format!("{other:?}"),
        };
        queue.push((label, vec![ev]));
    }
    // `^a` and `^s` are ctrl-prefixed, so the depth-1 sweep can't reach them.
    for (label, ev) in [("^a", ctrl('w')), ("^s", ctrl('s'))] {
        let mut r = Resolver::new();
        if feed(&mut r, ev) == ResolverOutcome::Pending && !r.continuations().is_empty() {
            queue.push((label.to_string(), vec![ev]));
        }
    }

    while let Some((label, entry)) = queue.pop() {
        let mut r = Resolver::new();
        for ev in &entry {
            feed(&mut r, *ev);
        }
        for c in r.continuations() {
            if let ChordEntry::Sub(k, _) = c
                && let Some(ev) = hint_key(k)
            {
                let mut deeper = entry.clone();
                deeper.push(ev);
                // `^a Space` loops back to the leader; following it forever
                // would not terminate.
                if !out.iter().any(|(_, e)| *e == deeper)
                    && !queue.iter().any(|(_, e)| *e == deeper)
                {
                    queue.push((format!("{label} {k}"), deeper));
                }
            }
        }
        out.push((label, entry));
    }
    out
}

/// A chord that BINDS an action must ADVERTISE it.
///
/// FEATURES.md sells the which-key popup as "the discovery surface for the dense
/// keymap", which makes a bound-but-unlisted key a shipped feature the user
/// cannot find. `^a g` — the image gallery — was exactly that: bound in the
/// resolver, absent from the `PendingSeq::W` continuations, reachable only by
/// reading the source.
///
/// Compared by action **variant**, so an alias needs no row of its own (`^a G`
/// for `^a g`, `^a C` for `^a \`), and by discriminant rather than value so the
/// `1-9` row's `PaneTabByIndex(0)` label covers `PaneTabByIndex(4)`.
#[test]
fn every_action_a_chord_binds_is_offered_in_its_hints() {
    use std::mem::{Discriminant, discriminant};

    let states = chord_states();
    // The tree, named — not a floor. `>= 8` against sixteen real states let the
    // walk lose half of them and still pass, which is the one failure a
    // completeness check exists to prevent: the guard below would go on
    // reporting "every bound action is advertised" about a shrinking share of
    // the keymap, and say nothing about the rest. Adding a chord prefix or a
    // submenu is *supposed* to fail this line — that is the prompt to confirm
    // the walk reached it.
    let mut found: Vec<&str> = states.iter().map(|(l, _)| l.as_str()).collect();
    found.sort_unstable();
    assert_eq!(
        found,
        [
            " ",   // leader
            "  w", // leader → worktree submenu
            "'",
            "H",
            "W",
            "Z",
            "[",
            "]",
            "^a",         // pane prefix
            "^a Space",   // pane → leader
            "^a Space w", // pane → leader → worktree
            "^s",         // vertical split
            "d",
            "g",
            "m",
            "y",
        ],
        "the chord-state walk no longer reaches the tree it used to"
    );

    for (label, entry) in states {
        let mut r = Resolver::new();
        for ev in &entry {
            feed(&mut r, *ev);
        }
        let hinted: Vec<Discriminant<Action>> = r
            .continuations()
            .into_iter()
            .filter_map(|e| match e {
                ChordEntry::Act(_, a) => Some(discriminant(&a)),
                ChordEntry::Sub(..) => None,
            })
            .collect();

        for cand in candidate_keys() {
            let mut r = Resolver::new();
            for ev in &entry {
                feed(&mut r, *ev);
            }
            if let ResolverOutcome::Action(a) = feed(&mut r, cand)
                && !hinted.contains(&discriminant(&a))
            {
                panic!(
                    "`{label}` + {cand:?} fires {a:?}, which the which-key popup never \
                     offers — add a ChordEntry for it, or unbind the key. The popup is \
                     the documented discovery surface for this keymap."
                );
            }
        }
    }
}

/// The documented binding taxonomy (DESIGN.md): the leader namespace carries
/// only Global/Meta actions, and the pane prefixes only Pane/Meta. This
/// guards against drift — e.g. a pane op accidentally added to the leader, or
/// a frame/global op landing on `^a`. Drives it through the resolver's own
/// `continuations()` so it tracks the real bindings.
///
/// Namespaces come from [`chord_states`], so every submenu under them is
/// checked rather than the one that was hardcoded. `^s` is in scope as a pane
/// prefix: AGENTS.md places the vertical split in the PANE tier, and all four of
/// its actions are `Tier::Pane`.
#[test]
fn leader_and_pane_namespaces_respect_tiers() {
    use crate::keymap::Tier;

    let states = chord_states();
    let actions_under = |root: crossterm::event::KeyEvent| -> Vec<(String, Action)> {
        states
            .iter()
            .filter(|(_, entry)| entry.first() == Some(&root))
            .flat_map(|(label, entry)| {
                let mut r = Resolver::new();
                for ev in entry {
                    feed(&mut r, *ev);
                }
                r.continuations()
                    .into_iter()
                    .filter_map(|e| match e {
                        ChordEntry::Act(_, a) => Some((label.clone(), a)),
                        ChordEntry::Sub(..) => None,
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    };

    // Leader (`Space`) and everything below it → Global/Meta only.
    let leader = actions_under(key(' '));
    assert!(
        leader.len() > 5,
        "the leader subtree looks empty: {leader:?}"
    );
    for (label, a) in leader {
        assert!(
            matches!(a.tier(), Tier::Global | Tier::Meta),
            "`{label}` action {a:?} is {:?}; the leader namespace is Global/Meta only",
            a.tier()
        );
    }

    // Pane prefixes (`^a`, `^s`) → Pane/Meta only. `^a Space` is the leader
    // reached from the pane, so its contents are checked as the leader's above.
    for root in [ctrl('w'), ctrl('s')] {
        let under = actions_under(root);
        assert!(!under.is_empty(), "{root:?} subtree looks empty");
        for (label, a) in under {
            if label.contains("Space") {
                continue;
            }
            assert!(
                matches!(a.tier(), Tier::Pane | Tier::Meta),
                "`{label}` action {a:?} is {:?}; the pane namespace is Pane/Meta only",
                a.tier()
            );
        }
    }
}
