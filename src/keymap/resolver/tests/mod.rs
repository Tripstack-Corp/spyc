//! Tests for the key-resolution state machine, split by theme.
//! Shared helpers live here; themed submodules pull them via `use super::*`.

#![allow(clippy::wildcard_imports)]

use super::*;

mod bindings;
mod counts;
mod keys;
mod panes;
mod prefixes;

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn special(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn empty_keymap() -> UserKeymap {
    UserKeymap::default()
}

fn feed(r: &mut Resolver, ev: KeyEvent) -> ResolverOutcome {
    r.feed(ev, &empty_keymap())
}
