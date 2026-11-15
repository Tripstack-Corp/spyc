//! A key for a program that asked for the kitty keyboard protocol (#1176),
//! encoded by libghostty-vt's own key encoder from the terminal's modes and the
//! flags the program pushed.
//!
//! spyc hears keys as crossterm events: a logical character plus modifiers,
//! not the physical key a platform keyboard API reports. So the mapping here
//! gives the encoder what an event does carry: the physical key where the
//! character names one on a US layout (letters, digits, the unshifted
//! punctuation, the named keys), the character as text otherwise, and the
//! modifiers. A shifted symbol (`!`) keeps its own codepoint rather than a
//! guessed base key, since on another layout the guess would be wrong.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use spyc_vt_sys::ffi::{self, GhosttyKey, GhosttyKeyAction, GhosttyString};

/// The kitty keyboard flags in force on the terminal's current screen.
pub(super) fn flags(t: ffi::GhosttyTerminal) -> u8 {
    let mut v: u8 = 0;
    // SAFETY: `t` is live, and KITTY_KEYBOARD_FLAGS writes one `uint8_t`.
    let ok = unsafe {
        ffi::ghostty_terminal_get(
            t,
            ffi::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_KITTY_KEYBOARD_FLAGS,
            (&raw mut v).cast(),
        )
    };
    if ok == spyc_vt_sys::SUCCESS { v } else { 0 }
}

/// What the encoder needs to know about one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Key {
    action: GhosttyKeyAction::Type,
    code: GhosttyKey::Type,
    mods: ffi::GhosttyMods,
    consumed: ffi::GhosttyMods,
    text: String,
    unshifted: u32,
}

/// The encoder's view of `ev`, or `None` for a key it has no name for (a media
/// key), which then goes out the legacy way.
pub(super) fn key(ev: KeyEvent) -> Option<Key> {
    use GhosttyKey as G;
    let action = match ev.kind {
        KeyEventKind::Press => GhosttyKeyAction::GHOSTTY_KEY_ACTION_PRESS,
        KeyEventKind::Repeat => GhosttyKeyAction::GHOSTTY_KEY_ACTION_REPEAT,
        KeyEventKind::Release => GhosttyKeyAction::GHOSTTY_KEY_ACTION_RELEASE,
    };
    let mut mods = mods(ev.modifiers, ev.state);
    let mut consumed = 0;
    let (code, text, unshifted) = match ev.code {
        KeyCode::Char(c) => {
            let (key, base) = physical(c);
            // A legacy host sends `A` without SHIFT; the letter says it.
            if c.is_ascii_uppercase() {
                mods |= ffi::GHOSTTY_MODS_SHIFT as ffi::GhosttyMods;
            }
            // Shift that made the character is spent on it: an `A`, and any
            // character on no named key (`!`, `é`), which Shift could only
            // have reached by producing it. Not on `a` or `1`, where it is a
            // modifier the program should see.
            let produced = base != c || key == GhosttyKey::GHOSTTY_KEY_UNIDENTIFIED;
            if produced && mods & ffi::GHOSTTY_MODS_SHIFT as ffi::GhosttyMods != 0 {
                consumed |= ffi::GHOSTTY_MODS_SHIFT as ffi::GhosttyMods;
            }
            // The encoder wants the text before any Ctrl transform, and none
            // for a control character: crossterm already reports `^a` as `a`.
            let text = if c.is_control() {
                String::new()
            } else {
                c.to_string()
            };
            (key, text, u32::from(base))
        }
        KeyCode::Enter => (G::GHOSTTY_KEY_ENTER, String::new(), 0),
        KeyCode::Tab => (G::GHOSTTY_KEY_TAB, String::new(), 0),
        KeyCode::BackTab => {
            mods |= ffi::GHOSTTY_MODS_SHIFT as ffi::GhosttyMods;
            (G::GHOSTTY_KEY_TAB, String::new(), 0)
        }
        KeyCode::Backspace => (G::GHOSTTY_KEY_BACKSPACE, String::new(), 0),
        KeyCode::Esc => (G::GHOSTTY_KEY_ESCAPE, String::new(), 0),
        KeyCode::Left => (G::GHOSTTY_KEY_ARROW_LEFT, String::new(), 0),
        KeyCode::Right => (G::GHOSTTY_KEY_ARROW_RIGHT, String::new(), 0),
        KeyCode::Up => (G::GHOSTTY_KEY_ARROW_UP, String::new(), 0),
        KeyCode::Down => (G::GHOSTTY_KEY_ARROW_DOWN, String::new(), 0),
        KeyCode::Home => (G::GHOSTTY_KEY_HOME, String::new(), 0),
        KeyCode::End => (G::GHOSTTY_KEY_END, String::new(), 0),
        KeyCode::PageUp => (G::GHOSTTY_KEY_PAGE_UP, String::new(), 0),
        KeyCode::PageDown => (G::GHOSTTY_KEY_PAGE_DOWN, String::new(), 0),
        KeyCode::Insert => (G::GHOSTTY_KEY_INSERT, String::new(), 0),
        KeyCode::Delete => (G::GHOSTTY_KEY_DELETE, String::new(), 0),
        KeyCode::F(n @ 1..=25) => (
            G::GHOSTTY_KEY_F1 + GhosttyKey::Type::from(n - 1),
            String::new(),
            0,
        ),
        KeyCode::CapsLock => (G::GHOSTTY_KEY_CAPS_LOCK, String::new(), 0),
        KeyCode::ScrollLock => (G::GHOSTTY_KEY_SCROLL_LOCK, String::new(), 0),
        KeyCode::NumLock => (G::GHOSTTY_KEY_NUM_LOCK, String::new(), 0),
        KeyCode::PrintScreen => (G::GHOSTTY_KEY_PRINT_SCREEN, String::new(), 0),
        KeyCode::Pause => (G::GHOSTTY_KEY_PAUSE, String::new(), 0),
        KeyCode::Menu => (G::GHOSTTY_KEY_CONTEXT_MENU, String::new(), 0),
        KeyCode::KeypadBegin => (G::GHOSTTY_KEY_NUMPAD_BEGIN, String::new(), 0),
        _ => return None,
    };
    Some(Key {
        action,
        code,
        mods,
        consumed,
        text,
        unshifted,
    })
}

/// The physical key `c` names on a US layout, and the unshifted character on
/// it: `a` for `A`, `1` for `1`. A character with no such key (a shifted
/// symbol, any non-ASCII) is its own base on no named key.
fn physical(c: char) -> (GhosttyKey::Type, char) {
    use GhosttyKey as G;
    let lower = c.to_ascii_lowercase();
    let key = match lower {
        'a'..='z' => G::GHOSTTY_KEY_A + GhosttyKey::Type::from(lower as u8 - b'a'),
        '0'..='9' => G::GHOSTTY_KEY_DIGIT_0 + GhosttyKey::Type::from(lower as u8 - b'0'),
        ' ' => G::GHOSTTY_KEY_SPACE,
        '`' => G::GHOSTTY_KEY_BACKQUOTE,
        '\\' => G::GHOSTTY_KEY_BACKSLASH,
        '[' => G::GHOSTTY_KEY_BRACKET_LEFT,
        ']' => G::GHOSTTY_KEY_BRACKET_RIGHT,
        ',' => G::GHOSTTY_KEY_COMMA,
        '=' => G::GHOSTTY_KEY_EQUAL,
        '-' => G::GHOSTTY_KEY_MINUS,
        '.' => G::GHOSTTY_KEY_PERIOD,
        '\'' => G::GHOSTTY_KEY_QUOTE,
        ';' => G::GHOSTTY_KEY_SEMICOLON,
        '/' => G::GHOSTTY_KEY_SLASH,
        _ => return (G::GHOSTTY_KEY_UNIDENTIFIED, c),
    };
    (key, lower)
}

fn mods(m: KeyModifiers, state: KeyEventState) -> ffi::GhosttyMods {
    let mut out: u32 = 0;
    for (from, to) in [
        (KeyModifiers::SHIFT, ffi::GHOSTTY_MODS_SHIFT),
        (KeyModifiers::CONTROL, ffi::GHOSTTY_MODS_CTRL),
        (KeyModifiers::ALT, ffi::GHOSTTY_MODS_ALT),
        (KeyModifiers::META, ffi::GHOSTTY_MODS_ALT),
        (KeyModifiers::SUPER, ffi::GHOSTTY_MODS_SUPER),
    ] {
        if m.contains(from) {
            out |= to;
        }
    }
    if state.contains(KeyEventState::CAPS_LOCK) {
        out |= ffi::GHOSTTY_MODS_CAPS_LOCK;
    }
    if state.contains(KeyEventState::NUM_LOCK) {
        out |= ffi::GHOSTTY_MODS_NUM_LOCK;
    }
    out as ffi::GhosttyMods
}

/// Encode `key` with the modes and kitty flags in force on `t`.
pub(super) fn encode(t: ffi::GhosttyTerminal, key: &Key) -> Vec<u8> {
    let mut enc: ffi::GhosttyKeyEncoder = std::ptr::null_mut();
    let mut ev: ffi::GhosttyKeyEvent = std::ptr::null_mut();
    // SAFETY: a null allocator is libghostty-vt's default; each handle is
    // written on success and freed below exactly once.
    let made = unsafe {
        ffi::ghostty_key_encoder_new(std::ptr::null(), &raw mut enc) == spyc_vt_sys::SUCCESS
            && ffi::ghostty_key_event_new(std::ptr::null(), &raw mut ev) == spyc_vt_sys::SUCCESS
    };
    let mut out = Vec::new();
    if made {
        // SAFETY: `enc`, `ev` and `t` are live. The event borrows `key.text`,
        // which outlives every call that reads it, the encode included.
        unsafe {
            ffi::ghostty_key_encoder_set_from_terminal(enc, t);
            ffi::ghostty_key_event_set_action(ev, key.action);
            ffi::ghostty_key_event_set_key(ev, key.code);
            ffi::ghostty_key_event_set_mods(ev, key.mods);
            ffi::ghostty_key_event_set_consumed_mods(ev, key.consumed);
            ffi::ghostty_key_event_set_utf8(
                ev,
                GhosttyString {
                    ptr: key.text.as_ptr(),
                    len: key.text.len(),
                },
            );
            ffi::ghostty_key_event_set_unshifted_codepoint(ev, key.unshifted);
            let mut need = 0usize;
            let r =
                ffi::ghostty_key_encoder_encode(enc, ev, std::ptr::null_mut(), 0, &raw mut need);
            if r == ffi::GhosttyResult::GHOSTTY_OUT_OF_SPACE && need > 0 {
                out.resize(need, 0);
                let mut wrote = 0usize;
                let r = ffi::ghostty_key_encoder_encode(
                    enc,
                    ev,
                    out.as_mut_ptr(),
                    out.len(),
                    &raw mut wrote,
                );
                out.truncate(if r == spyc_vt_sys::SUCCESS { wrote } else { 0 });
            }
        }
    }
    // SAFETY: each handle is non-null only if its `new` succeeded, and is
    // freed here once and never used again.
    unsafe {
        if !ev.is_null() {
            ffi::ghostty_key_event_free(ev);
        }
        if !enc.is_null() {
            ffi::ghostty_key_encoder_free(enc);
        }
    }
    out
}
