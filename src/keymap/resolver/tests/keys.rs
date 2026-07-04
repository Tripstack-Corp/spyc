//! Single-key actions, motions, harpoon/inventory/file-ops, esc.
#![allow(clippy::wildcard_imports)]

use super::*;

#[test]
fn ctrl_d_quits() {
    // `^d` maps to quit; `request_quit` keeps its own two-tap confirm, so
    // quitting stays `^d^d` with a warning. It never closes the second
    // commander (that's `^s x`) — so quitting with `b` open preserves it for
    // session restore.
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, ctrl('d')),
        ResolverOutcome::Action(Action::Quit)
    );
}

#[test]
fn ctrl_l_redraws() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, ctrl('l')),
        ResolverOutcome::Action(Action::Redraw)
    );
}

#[test]
fn ctrl_b_page_up() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, ctrl('b')),
        ResolverOutcome::Action(Action::PageUp)
    );
}

#[test]
fn ctrl_f_page_down() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, ctrl('f')),
        ResolverOutcome::Action(Action::PageDown)
    );
}

#[test]
fn ctrl_t_toggles_all_picks() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, ctrl('t')),
        ResolverOutcome::Action(Action::PickToggleAll)
    );
}

#[test]
fn ctrl_x_is_unbound_after_chmod_demotion() {
    // `^X` (chmod +x) was demoted to `:chmod` to slim the default map; the
    // key is now free (re-bind via `map ^X command chmod`).
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, ctrl('x')), ResolverOutcome::Ignored);
}

#[test]
fn demoted_standalone_keys_are_unbound() {
    // A (:activity), s (:set), f (:filetype) lost their default key in the
    // keymap slim — each is now free, reached via its : command and
    // re-bindable with `map KEY command <name>`.
    for c in ['A', 's', 'f'] {
        let mut r = Resolver::new();
        assert_eq!(
            feed(&mut r, key(c)),
            ResolverOutcome::Ignored,
            "`{c}` should be unbound after the demotion"
        );
    }
}

#[test]
fn capital_l_long_lists() {
    // `L` keeps its default binding — long listing is table stakes for a file
    // commander, so it earns a key (the `:longlist` command remains too).
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, key('L')),
        ResolverOutcome::Action(Action::LongList)
    );
}

#[test]
fn ctrl_r_reloads_config() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, ctrl('r')),
        ResolverOutcome::Action(Action::ReloadConfig)
    );
}

#[test]
fn ctrl_resets_pending_state() {
    let mut r = Resolver::new();
    feed(&mut r, key('g'));
    assert!(r.is_pending());
    assert_eq!(
        feed(&mut r, ctrl('d')),
        ResolverOutcome::Action(Action::Quit)
    );
    assert!(!r.is_pending());
}

// ── simple single-key actions ─────────────────────────────────

#[test]
fn basic_motions_default_count_1() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, key('j')),
        ResolverOutcome::Action(Action::Down(1))
    );
    assert_eq!(
        feed(&mut r, key('k')),
        ResolverOutcome::Action(Action::Up(1))
    );
    assert_eq!(
        feed(&mut r, key('h')),
        ResolverOutcome::Action(Action::Left(1))
    );
    assert_eq!(
        feed(&mut r, key('l')),
        ResolverOutcome::Action(Action::Right(1))
    );
}

#[test]
fn arrow_keys_work() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, special(KeyCode::Down)),
        ResolverOutcome::Action(Action::Down(1))
    );
    assert_eq!(
        feed(&mut r, special(KeyCode::Up)),
        ResolverOutcome::Action(Action::Up(1))
    );
    assert_eq!(
        feed(&mut r, special(KeyCode::Left)),
        ResolverOutcome::Action(Action::Left(1))
    );
    assert_eq!(
        feed(&mut r, special(KeyCode::Right)),
        ResolverOutcome::Action(Action::Right(1))
    );
}

#[test]
fn shell_prompts() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, key('!')),
        ResolverOutcome::Action(Action::ShellCapturedPrompt)
    );
    assert_eq!(
        feed(&mut r, key(';')),
        ResolverOutcome::Action(Action::ShellForegroundPrompt)
    );
    assert_eq!(
        feed(&mut r, key('$')),
        ResolverOutcome::Action(Action::StartShell)
    );
}

#[test]
fn search_keys() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, key('/')),
        ResolverOutcome::Action(Action::SearchPrompt)
    );
    assert_eq!(
        feed(&mut r, key('n')),
        ResolverOutcome::Action(Action::SearchNext)
    );
    assert_eq!(
        feed(&mut r, key('N')),
        ResolverOutcome::Action(Action::SearchPrev)
    );
}

#[test]
fn quit_keys() {
    let mut r = Resolver::new();
    // Lowercase q is reserved for future macro recording; only Q quits.
    assert_eq!(
        feed(&mut r, key('q')),
        ResolverOutcome::Action(Action::MacroRecordReserved)
    );
    assert_eq!(
        feed(&mut r, key('Q')),
        ResolverOutcome::Action(Action::Quit)
    );
}

#[test]
fn navigation_keys() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, key('u')),
        ResolverOutcome::Action(Action::Climb)
    );
    assert_eq!(
        feed(&mut r, key('-')),
        ResolverOutcome::Action(Action::Climb)
    );
    // `H` is the harpoon chord prefix (was `Home` alias; freed
    // for `H1`..`H9`, `Ha`, `Hx`, `Hh`). `~` and the Home key
    // remain the bindings for jumping to `$HOME`.
    assert_eq!(feed(&mut r, key('H')), ResolverOutcome::Pending);
    // `Hh` opens the harpoon menu (recovers from the pending state).
    assert_eq!(
        feed(&mut r, key('h')),
        ResolverOutcome::Action(Action::HarpoonOpenMenu)
    );
    assert_eq!(
        feed(&mut r, key('~')),
        ResolverOutcome::Action(Action::Home)
    );
}

#[test]
fn harpoon_chord_jumps_to_slot() {
    let mut r = Resolver::new();
    feed(&mut r, key('H'));
    assert_eq!(
        feed(&mut r, key('3')),
        ResolverOutcome::Action(Action::HarpoonJump(3))
    );
}

#[test]
fn harpoon_chord_append_remove() {
    let mut r = Resolver::new();
    feed(&mut r, key('H'));
    assert_eq!(
        feed(&mut r, key('a')),
        ResolverOutcome::Action(Action::HarpoonAppend)
    );
    feed(&mut r, key('H'));
    assert_eq!(
        feed(&mut r, key('x')),
        ResolverOutcome::Action(Action::HarpoonRemove)
    );
}

#[test]
fn inventory_keys() {
    let mut r = Resolver::new();
    assert_eq!(feed(&mut r, key('y')), ResolverOutcome::Pending);
    assert_eq!(
        feed(&mut r, key('y')),
        ResolverOutcome::Action(Action::Take)
    );
    assert_eq!(feed(&mut r, key('y')), ResolverOutcome::Pending);
    assert_eq!(
        feed(&mut r, key('p')),
        ResolverOutcome::Action(Action::YankPrompt)
    );
    assert_eq!(
        feed(&mut r, key('Y')),
        ResolverOutcome::Action(Action::Untake)
    );
    assert_eq!(
        feed(&mut r, key('p')),
        ResolverOutcome::Action(Action::Drop)
    );
    assert_eq!(
        feed(&mut r, key('i')),
        ResolverOutcome::Action(Action::ToggleInventoryView)
    );
    assert_eq!(
        feed(&mut r, key('z')),
        ResolverOutcome::Action(Action::EmptyInventory)
    );
}

#[test]
fn file_operation_keys() {
    let mut r = Resolver::new();
    assert_eq!(
        feed(&mut r, key('c')),
        ResolverOutcome::Action(Action::CopyPrompt)
    );
    assert_eq!(
        feed(&mut r, key('R')),
        ResolverOutcome::Action(Action::RemovePrompt(None))
    );
    assert_eq!(
        feed(&mut r, key('M')),
        ResolverOutcome::Action(Action::MovePrompt)
    );
    assert_eq!(
        feed(&mut r, key('+')),
        ResolverOutcome::Action(Action::MakeDirPrompt)
    );
}

// ── Esc resets ────────────────────────────────────────────────

#[test]
fn esc_resets_count_and_pending() {
    let mut r = Resolver::new();
    feed(&mut r, key('5'));
    feed(&mut r, key('g'));
    assert!(r.is_pending());
    // Esc hits the catch-all `_ =>` arm which resets and returns Ignored
    // (g-pending intercepts first, so Esc after g is "unknown in g-seq")
    let out = feed(&mut r, special(KeyCode::Esc));
    assert_eq!(out, ResolverOutcome::Ignored);
    // State is fully reset — next key is fresh
    assert!(!r.is_pending());
    assert_eq!(
        feed(&mut r, key('j')),
        ResolverOutcome::Action(Action::Down(1))
    );
}

#[test]
fn esc_from_normal_resets_count() {
    let mut r = Resolver::new();
    feed(&mut r, key('5'));
    // Esc from normal (non-pending) state returns Pending but resets count
    let out = feed(&mut r, special(KeyCode::Esc));
    assert_eq!(out, ResolverOutcome::Pending);
    assert!(!r.is_pending());
    assert_eq!(
        feed(&mut r, key('j')),
        ResolverOutcome::Action(Action::Down(1))
    );
}

// ── pending display ───────────────────────────────────────────
