//! Keyboard → pty byte encoding.
//!
//! Most terminal applications expect xterm-style escape sequences for
//! special keys. We cover the common cases (arrows, Home/End, PgUp/PgDn,
//! function keys, Ctrl+letter, plain chars, Tab/Enter/Backspace/Esc);
//! unusual combinations fall through as an empty slice, which is the
//! terminal's "nothing happened" signal.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Encode `ev` for a child whose DECCKM (cursor-key) state is `app_cursor`
/// (`vt100::Screen::application_cursor` — set by the child's `ESC[?1h`).
///
/// A child in application-cursor mode waits for the SS3 arrow form (`ESC O A`)
/// and a strict one silently drops the CSI form, which presents as a pane that
/// ignores the arrow keys. Pass `false` for a pty spyc doesn't emulate: no mode
/// is tracked, and CSI is what an un-negotiated terminal sends.
pub fn encode_key(ev: KeyEvent, app_cursor: bool) -> Vec<u8> {
    use KeyCode as K;
    let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
    let alt = ev.modifiers.contains(KeyModifiers::ALT);
    let shift = ev.modifiers.contains(KeyModifiers::SHIFT);
    let zoo = ev.modifiers.contains(KeyModifiers::SUPER)
        || ev.modifiers.contains(KeyModifiers::META)
        || ev.modifiers.contains(KeyModifiers::HYPER);

    let mut out = Vec::new();

    match ev.code {
        K::Char(c) => {
            if ctrl {
                // Ctrl+A = 0x01, Ctrl+B = 0x02, ... Ctrl+Z = 0x1a.
                // Ctrl+Space = 0x00, Ctrl+Backslash = 0x1c, etc.
                match c {
                    '@' | ' ' => out.push(0x00),
                    '[' => out.push(0x1b),
                    '\\' => out.push(0x1c),
                    ']' => out.push(0x1d),
                    '^' => out.push(0x1e),
                    '_' | '?' => out.push(0x1f),
                    _ => {
                        let lower = c.to_ascii_lowercase();
                        if lower.is_ascii_lowercase() {
                            out.push((lower as u8) - b'a' + 1);
                        }
                    }
                }
            } else {
                if alt {
                    out.push(0x1b); // Alt = prefix Esc
                }
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
        K::Enter => {
            if alt || ctrl || shift || zoo {
                // Any modified Enter ⇒ newline (Claude CLI multi-line input).
                // Different terminals report Option+Enter differently --
                // some as Alt+Enter, some as Ctrl+Enter, some as
                // Shift+Enter, some only properly when the kitty
                // keyboard protocol is enabled. Fold them all so the
                // user's "I want a newline in my Claude prompt"
                // muscle memory just works regardless of host
                // terminal config.
                out.push(b'\n');
            } else {
                out.push(b'\r');
            }
        }
        K::Tab => out.push(b'\t'),
        K::BackTab => out.extend_from_slice(b"\x1b[Z"),
        K::Backspace => out.push(0x7f),
        K::Esc => out.push(0x1b),
        // Cursor + edit keys carry their Ctrl/Alt/Shift modifiers through the
        // standard xterm encoding (Ctrl+Right = word-motion, Shift+Arrow =
        // selection, etc.); unmodified, each emits its bare sequence verbatim.
        K::Up => push_cursor_key(&mut out, ev.modifiers, app_cursor, b'A'),
        K::Down => push_cursor_key(&mut out, ev.modifiers, app_cursor, b'B'),
        K::Right => push_cursor_key(&mut out, ev.modifiers, app_cursor, b'C'),
        K::Left => push_cursor_key(&mut out, ev.modifiers, app_cursor, b'D'),
        K::Home => push_cursor_key(&mut out, ev.modifiers, app_cursor, b'H'),
        K::End => push_cursor_key(&mut out, ev.modifiers, app_cursor, b'F'),
        K::PageUp => push_csi_tilde(&mut out, ev.modifiers, 5),
        K::PageDown => push_csi_tilde(&mut out, ev.modifiers, 6),
        K::Delete => push_csi_tilde(&mut out, ev.modifiers, 3),
        K::Insert => push_csi_tilde(&mut out, ev.modifiers, 2),
        K::F(n) => match n {
            1 => push_fn_key(&mut out, ev.modifiers, b'P'),
            2 => push_fn_key(&mut out, ev.modifiers, b'Q'),
            3 => push_fn_key(&mut out, ev.modifiers, b'R'),
            4 => push_fn_key(&mut out, ev.modifiers, b'S'),
            5 => push_csi_tilde(&mut out, ev.modifiers, 15),
            6 => push_csi_tilde(&mut out, ev.modifiers, 17),
            7 => push_csi_tilde(&mut out, ev.modifiers, 18),
            8 => push_csi_tilde(&mut out, ev.modifiers, 19),
            9 => push_csi_tilde(&mut out, ev.modifiers, 20),
            10 => push_csi_tilde(&mut out, ev.modifiers, 21),
            11 => push_csi_tilde(&mut out, ev.modifiers, 23),
            12 => push_csi_tilde(&mut out, ev.modifiers, 24),
            _ => {}
        },
        _ => {}
    }
    out
}

/// xterm modifier parameter for a modified special key: `1 + mask`, with bits
/// Shift=1, Alt=2, Ctrl=4 — the de-facto VT/xterm encoding every common pane
/// app (vim, less, readline, tmux) understands. `None` when no shift/alt/ctrl
/// is set, so callers emit the bare (unparameterized) sequence — byte-identical
/// to the pre-modifier behaviour. Super/Meta/Hyper are deliberately excluded:
/// terminals don't agree on a code for them, so falling back to the bare
/// sequence (today's behaviour) beats sending one apps won't recognize.
fn modifier_param(m: KeyModifiers) -> Option<u8> {
    let mask = u8::from(m.contains(KeyModifiers::SHIFT))
        + u8::from(m.contains(KeyModifiers::ALT)) * 2
        + u8::from(m.contains(KeyModifiers::CONTROL)) * 4;
    (mask != 0).then_some(1 + mask)
}

/// Push `n` (0..=99) as ASCII decimal — alloc-free; special-key params here are
/// at most two digits (F12 ⇒ 24, modifier ⇒ 8).
fn push_dec(out: &mut Vec<u8>, n: u8) {
    if n >= 10 {
        out.push(b'0' + n / 10);
    }
    out.push(b'0' + n % 10);
}

/// Cursor keys (arrows, Home, End) — the set DECCKM governs.
///
/// Unmodified: `ESC [ A` normally, `ESC O A` when the child set application
/// cursor mode. Modified: always the parameterized CSI form
/// (`ESC [ 1 ; 5 C` = Ctrl+Right) — xterm switches only the *unmodified* keys,
/// so ctrl/shift/alt-arrow navigation is mode-independent.
fn push_cursor_key(out: &mut Vec<u8>, m: KeyModifiers, app_cursor: bool, final_byte: u8) {
    if let Some(p) = modifier_param(m) {
        out.extend_from_slice(b"\x1b[1;");
        push_dec(out, p);
    } else if app_cursor {
        out.extend_from_slice(b"\x1bO");
    } else {
        out.extend_from_slice(b"\x1b[");
    }
    out.push(final_byte);
}

/// Tilde-terminated keys (`CSI <num> [;<mod>] ~` — Delete, Insert, PageUp/Down,
/// F5–F12): `ESC [ 3 ~` bare, `ESC [ 3 ; 5 ~` for Ctrl+Delete.
fn push_csi_tilde(out: &mut Vec<u8>, m: KeyModifiers, num: u8) {
    out.extend_from_slice(b"\x1b[");
    push_dec(out, num);
    if let Some(p) = modifier_param(m) {
        out.push(b';');
        push_dec(out, p);
    }
    out.push(b'~');
}

/// F1–F4: bare uses SS3 (`ESC O P`), but a modifier switches to the CSI form
/// (`ESC [ 1 ; <mod> P`) — the standard xterm distinction.
fn push_fn_key(out: &mut Vec<u8>, m: KeyModifiers, final_byte: u8) {
    if let Some(p) = modifier_param(m) {
        out.extend_from_slice(b"\x1b[1;");
        push_dec(out, p);
        out.push(final_byte);
    } else {
        out.extend_from_slice(b"\x1bO");
        out.push(final_byte);
    }
}

// ── mouse → pty byte encoding ─────────────────────────────────────────────

/// One mouse event, already reduced to what the wire format needs. Built by the
/// caller so this module never sees a `crossterm::MouseEvent` (and so the
/// coordinate translation happens where the layout is known).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseReport {
    /// Which button, in xterm's numbering: 0 left, 1 middle, 2 right,
    /// 64 wheel-up, 65 wheel-down.
    pub button: u8,
    /// True for a button release. Wheel events are always presses — a wheel
    /// "release" isn't a thing, and emitting one confuses apps that count clicks.
    pub release: bool,
    /// True for a drag: the pointer moved to a new cell while a button was held.
    ///
    /// Sets xterm's motion bit (32) in `Cb`. Only meaningful for a child that
    /// asked for motion — [`encode_mouse`] drops it otherwise, since DEC 1000
    /// reports press/release and no motion at all.
    pub motion: bool,
    /// **Pane-relative**, 0-based. The child believes it owns a grid starting at
    /// its own `0,0`; passing frame-absolute coordinates here is the bug that
    /// makes clicks land N rows off and read as the child's fault.
    pub col: u16,
    pub row: u16,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// Encode a mouse event in the protocol/encoding the CHILD asked for.
///
/// Returns empty when the child requested no mouse reporting (`Mode::None`) —
/// the caller must not send anything then, or the escape bytes land as literal
/// input at its prompt. That's the bracketed-paste bug (#170) in a new costume;
/// see `Pane::wants_mouse`.
///
/// Never relays the bytes spyc received: spyc asks the terminal for SGR (1006),
/// but the child may have requested the default or UTF-8 encoding, so the event
/// is re-encoded from the decoded form.
pub fn encode_mouse(
    ev: MouseReport,
    mode: vt100::MouseProtocolMode,
    encoding: vt100::MouseProtocolEncoding,
) -> Vec<u8> {
    use vt100::MouseProtocolMode as M;
    // Send only what the child's declared mode covers. `Press` (X10) reports
    // presses only — a release is noise it has no grammar for. And **only
    // ButtonMotion (1002) / AnyMotion (1003) report motion at all**: DEC 1000
    // is press+release, so handing a drag to a `PressRelease` child is the same
    // class of fault as forwarding to a child that asked for nothing.
    match mode {
        M::None => return Vec::new(),
        M::Press if ev.release => return Vec::new(),
        M::Press | M::PressRelease if ev.motion => return Vec::new(),
        _ => {}
    }

    // xterm's Cb: button in the low bits, modifiers above it.
    let mut cb = u32::from(ev.button);
    if ev.motion {
        // Bit 5 marks "this is motion, not a fresh press".
        cb |= 32;
    }
    if ev.shift {
        cb |= 4;
    }
    if ev.alt {
        cb |= 8;
    }
    if ev.ctrl {
        cb |= 16;
    }

    // The wire is 1-based; our input is 0-based.
    let (x, y) = (u32::from(ev.col) + 1, u32::from(ev.row) + 1);

    match encoding {
        vt100::MouseProtocolEncoding::Sgr => {
            // `ESC [ < Cb ; Cx ; Cy (M|m)` — release is the lowercase final byte,
            // which is how SGR distinguishes it without a sentinel button value.
            let final_byte = if ev.release { 'm' } else { 'M' };
            format!("\x1b[<{cb};{x};{y}{final_byte}").into_bytes()
        }
        vt100::MouseProtocolEncoding::Utf8 => {
            // Like the default encoding, but the three values go out as chars so
            // coordinates past 223 survive. A release is button 3 (the default
            // encoding has no lowercase-final trick).
            let mut out = b"\x1b[M".to_vec();
            let cb = if ev.release { 3 | (cb & !3) } else { cb };
            for v in [cb, x, y] {
                let ch = char::from_u32(v + 32).unwrap_or('\u{fffd}');
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
            out
        }
        vt100::MouseProtocolEncoding::Default => {
            // `ESC [ M Cb+32 Cx+32 Cy+32`, one byte each. Coordinates past 223
            // don't fit — xterm's own behaviour is to clamp rather than emit a
            // byte that would be read as part of the next sequence.
            let mut out = b"\x1b[M".to_vec();
            let cb = if ev.release { 3 | (cb & !3) } else { cb };
            for v in [cb, x, y] {
                out.push(u8::try_from(v + 32).unwrap_or(u8::MAX));
            }
            out
        }
    }
}

#[cfg(test)]
mod mouse_tests {
    use super::{MouseReport, encode_mouse};
    use vt100::{MouseProtocolEncoding as Enc, MouseProtocolMode as Mode};

    /// Wheel-up at pane-relative (0,0) — the origin case, which is where an
    /// off-by-one in the 0-based→1-based conversion shows up.
    const fn wheel_up() -> MouseReport {
        MouseReport {
            button: 64,
            release: false,
            motion: false,
            col: 0,
            row: 0,
            shift: false,
            alt: false,
            ctrl: false,
        }
    }

    /// A left-drag at pane-relative (4,2): the held button plus the motion bit.
    const fn left_drag() -> MouseReport {
        MouseReport {
            button: 0,
            release: false,
            motion: true,
            col: 4,
            row: 2,
            shift: false,
            alt: false,
            ctrl: false,
        }
    }

    /// **A drag only goes to a child that asked for motion.**
    ///
    /// DEC 1000 (`PressRelease`) reports press and release and *no motion at all*,
    /// so handing it a drag is the same class of fault as forwarding to a child
    /// that enabled nothing — bytes it has no grammar for. Only 1002
    /// (`ButtonMotion`) and 1003 (`AnyMotion`) report motion.
    ///
    /// This is the one rule that makes drag-forwarding safe to turn on globally:
    /// spyc now receives drags for every pane, and most children want none of them.
    #[test]
    fn a_drag_reaches_only_a_child_that_asked_for_motion() {
        for mode in [Mode::None, Mode::Press, Mode::PressRelease] {
            assert!(
                encode_mouse(left_drag(), mode, Enc::Sgr).is_empty(),
                "{mode:?} does not report motion — a drag must not be sent"
            );
        }
        for mode in [Mode::ButtonMotion, Mode::AnyMotion] {
            assert!(
                !encode_mouse(left_drag(), mode, Enc::Sgr).is_empty(),
                "{mode:?} reports motion — a drag must be sent"
            );
        }
    }

    /// A drag sets xterm's motion bit (32) on top of the held button, and keeps
    /// the press final byte — it is motion, not a release.
    #[test]
    fn drag_sets_the_motion_bit_on_the_held_button() {
        let out = encode_mouse(left_drag(), Mode::ButtonMotion, Enc::Sgr);
        // button 0 | 32 = 32; coords 1-based → (5,3); `M` because it is not a release.
        assert_eq!(
            out,
            b"\x1b[<32;5;3M".to_vec(),
            "got {:?}",
            String::from_utf8_lossy(&out)
        );

        // Middle and right carry their own button under the same bit.
        for (button, cb) in [(1u8, 33), (2u8, 34)] {
            let r = super::MouseReport {
                button,
                ..left_drag()
            };
            let out = encode_mouse(r, Mode::ButtonMotion, Enc::Sgr);
            let expected = format!("\x1b[<{cb};5;3M").into_bytes();
            assert_eq!(out, expected, "button {button} drag");
        }
    }

    /// The legacy encodings mark a release with button 3, and that must not
    /// swallow the motion bit — a drag is neither a fresh press nor a release, and
    /// conflating it with either is how a child ends up thinking a button is stuck.
    #[test]
    fn drag_survives_the_legacy_encodings_release_trick() {
        for enc in [Enc::Default, Enc::Utf8] {
            let out = encode_mouse(left_drag(), Mode::ButtonMotion, enc);
            assert!(!out.is_empty(), "{enc:?}: drag must encode");
            // `ESC [ M` then Cb+32 = 32+32 = 64 = b'@'.
            assert_eq!(&out[..3], b"\x1b[M", "{enc:?}: legacy prefix");
            assert_eq!(
                out[3], b'@',
                "{enc:?}: Cb must be 32 (motion|button 0) + 32"
            );
        }
    }

    /// The gate. A child that never enabled mouse reporting must receive NOTHING —
    /// the bytes would land as literal input at its prompt (#170's failure mode).
    #[test]
    fn mode_none_encodes_nothing_for_every_encoding() {
        for enc in [Enc::Default, Enc::Utf8, Enc::Sgr] {
            assert!(
                encode_mouse(wheel_up(), Mode::None, enc).is_empty(),
                "{enc:?} must emit nothing when the child wants no mouse"
            );
        }
    }

    /// X10 (`Press`) has no grammar for a release, so one must not be invented.
    #[test]
    fn press_only_mode_drops_releases_but_keeps_presses() {
        let release = MouseReport {
            button: 0,
            release: true,
            ..wheel_up()
        };
        assert!(encode_mouse(release, Mode::Press, Enc::Sgr).is_empty());
        assert!(!encode_mouse(wheel_up(), Mode::Press, Enc::Sgr).is_empty());
    }

    /// Coordinates are 1-based on the wire; ours are 0-based pane-relative.
    #[test]
    fn sgr_is_one_based_and_wheel_up_is_button_64() {
        let bytes = encode_mouse(wheel_up(), Mode::PressRelease, Enc::Sgr);
        assert_eq!(
            bytes,
            b"\x1b[<64;1;1M",
            "got {:?}",
            String::from_utf8_lossy(&bytes)
        );
    }

    #[test]
    fn sgr_encodes_position_and_wheel_down() {
        let ev = MouseReport {
            button: 65,
            col: 11,
            row: 4,
            ..wheel_up()
        };
        let bytes = encode_mouse(ev, Mode::PressRelease, Enc::Sgr);
        assert_eq!(bytes, b"\x1b[<65;12;5M");
    }

    /// Modifier bits ride in Cb above the button: shift 4, alt 8, ctrl 16.
    #[test]
    fn sgr_folds_modifiers_into_the_button_byte() {
        let ev = MouseReport {
            button: 0,
            shift: true,
            ctrl: true,
            ..wheel_up()
        };
        let bytes = encode_mouse(ev, Mode::PressRelease, Enc::Sgr);
        assert_eq!(bytes, b"\x1b[<20;1;1M", "0 | 4 | 16 = 20");
    }

    /// SGR marks a release with a lowercase final byte rather than a sentinel
    /// button — the one structural difference from the older encodings.
    #[test]
    fn sgr_release_uses_lowercase_final_byte() {
        let ev = MouseReport {
            button: 0,
            release: true,
            ..wheel_up()
        };
        let bytes = encode_mouse(ev, Mode::PressRelease, Enc::Sgr);
        assert_eq!(bytes, b"\x1b[<0;1;1m");
    }

    /// Default encoding: three single bytes, each offset by 32.
    #[test]
    fn default_encoding_offsets_every_field_by_32() {
        let ev = MouseReport {
            button: 64,
            col: 11,
            row: 4,
            ..wheel_up()
        };
        let bytes = encode_mouse(ev, Mode::PressRelease, Enc::Default);
        assert_eq!(bytes, vec![0x1b, b'[', b'M', 64 + 32, 12 + 32, 5 + 32]);
    }

    /// Past column 223 a single byte can't hold `x + 32`. Clamping is xterm's own
    /// behaviour; the alternative (wrapping) emits a byte the child would read as
    /// part of the *next* escape sequence.
    #[test]
    fn default_encoding_clamps_rather_than_wrapping_past_223() {
        let ev = MouseReport {
            col: 400,
            row: 400,
            ..wheel_up()
        };
        let bytes = encode_mouse(ev, Mode::PressRelease, Enc::Default);
        assert_eq!(bytes.len(), 6, "still one byte per field");
        assert_eq!(bytes[4], u8::MAX, "column clamped, not wrapped");
        assert_eq!(bytes[5], u8::MAX, "row clamped, not wrapped");
    }

    /// UTF-8 encoding exists precisely so large coordinates survive, so the same
    /// position must NOT clamp here.
    #[test]
    fn utf8_encoding_carries_coordinates_past_223_intact() {
        let ev = MouseReport {
            col: 400,
            row: 400,
            ..wheel_up()
        };
        let bytes = encode_mouse(ev, Mode::PressRelease, Enc::Utf8);
        let text = String::from_utf8(bytes).expect("utf-8 by construction");
        let chars: Vec<char> = text.chars().collect();
        assert_eq!(&chars[..3], &['\x1b', '[', 'M']);
        assert_eq!(chars[4] as u32, 401 + 32, "column survives");
        assert_eq!(chars[5] as u32, 401 + 32, "row survives");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }
    fn k_ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }
    fn k_mod(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }
    /// Encode for a child in NORMAL cursor-key mode — every child that never
    /// touches DECCKM, which is the overwhelming majority.
    fn enc(ev: KeyEvent) -> Vec<u8> {
        encode_key(ev, false)
    }
    /// The six keys DECCKM governs, with the final byte each carries.
    const CURSOR_KEYS: [(KeyCode, u8); 6] = [
        (KeyCode::Up, b'A'),
        (KeyCode::Down, b'B'),
        (KeyCode::Right, b'C'),
        (KeyCode::Left, b'D'),
        (KeyCode::Home, b'H'),
        (KeyCode::End, b'F'),
    ];
    /// A parser fed `escapes`, standing in for the child's declared state.
    fn child_mode(escapes: &[u8]) -> bool {
        let mut p = vt100::Parser::new(24, 80, 100);
        p.process(escapes);
        p.screen().application_cursor()
    }

    #[test]
    fn plain_char() {
        assert_eq!(enc(k(KeyCode::Char('a'))), b"a");
    }

    #[test]
    fn ctrl_letter() {
        assert_eq!(enc(k_ctrl(KeyCode::Char('c'))), vec![0x03]);
        assert_eq!(enc(k_ctrl(KeyCode::Char('a'))), vec![0x01]);
    }

    #[test]
    fn enter_is_cr() {
        assert_eq!(enc(k(KeyCode::Enter)), b"\r");
    }

    #[test]
    fn arrow_up() {
        assert_eq!(enc(k(KeyCode::Up)), b"\x1b[A");
    }

    /// The contract: cursor keys follow the CHILD's declared mode, and the mode
    /// comes from the child's own escape bytes (driven through a real parser
    /// here, exactly as `Pane::application_cursor` reads it).
    ///
    /// A child that set DECCKM and parses strictly drops the CSI form — arrows
    /// dead, bare control bytes (`^C`) still live, which is the asymmetry that
    /// made this look like an unresponsive pane rather than an encoding bug.
    #[test]
    fn cursor_keys_follow_the_childs_declared_mode() {
        for (code, final_byte) in CURSOR_KEYS {
            // Untouched: no child has spoken, so the normal CSI form.
            assert!(!child_mode(b""), "a fresh parser is in normal mode");
            assert_eq!(
                encode_key(k(code), child_mode(b"")),
                [b"\x1b[".as_slice(), &[final_byte]].concat(),
                "{code:?} before any DECCKM"
            );

            // `ESC[?1h` — application cursor keys ⇒ SS3.
            assert!(child_mode(b"\x1b[?1h"), "`ESC[?1h` sets DECCKM");
            assert_eq!(
                encode_key(k(code), child_mode(b"\x1b[?1h")),
                [b"\x1bO".as_slice(), &[final_byte]].concat(),
                "{code:?} in application-cursor mode"
            );

            // `ESC[?1l` — back to normal, and so is the encoding.
            assert!(!child_mode(b"\x1b[?1h\x1b[?1l"), "`ESC[?1l` resets DECCKM");
            assert_eq!(
                encode_key(k(code), child_mode(b"\x1b[?1h\x1b[?1l")),
                [b"\x1b[".as_slice(), &[final_byte]].concat(),
                "{code:?} after DECCKM reset"
            );
        }
    }

    /// Modified cursor keys keep the CSI form in BOTH modes — xterm switches only
    /// the unmodified keys, so ctrl/shift/alt-arrow navigation inside a child
    /// must be byte-identical either way.
    #[test]
    fn modified_cursor_keys_ignore_decckm() {
        let mod_sets = [
            KeyModifiers::CONTROL,
            KeyModifiers::SHIFT,
            KeyModifiers::ALT,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT,
        ];
        for (code, _) in CURSOR_KEYS {
            for mods in mod_sets {
                let normal = encode_key(k_mod(code, mods), false);
                let application = encode_key(k_mod(code, mods), true);
                assert_eq!(
                    normal, application,
                    "{code:?}+{mods:?} must not change with DECCKM"
                );
                assert!(
                    normal.starts_with(b"\x1b[1;"),
                    "{code:?}+{mods:?} stays on the parameterized CSI form: {normal:?}"
                );
            }
        }
    }

    /// Everything the encoder handles that DECCKM does NOT govern: identical
    /// bytes in both modes. The guard that this fix reaches only the six cursor
    /// keys — a child in application mode still gets today's Enter, Tab, tilde
    /// keys and function keys.
    #[test]
    fn only_cursor_keys_respond_to_decckm() {
        let untouched = [
            k(KeyCode::Char('a')),
            k_ctrl(KeyCode::Char('c')),
            k_mod(KeyCode::Char('x'), KeyModifiers::ALT),
            k(KeyCode::Enter),
            k_mod(KeyCode::Enter, KeyModifiers::ALT),
            k(KeyCode::Tab),
            k(KeyCode::BackTab),
            k(KeyCode::Backspace),
            k(KeyCode::Esc),
            k(KeyCode::PageUp),
            k(KeyCode::PageDown),
            k(KeyCode::Delete),
            k(KeyCode::Insert),
            k(KeyCode::F(1)),
            k(KeyCode::F(5)),
            k(KeyCode::F(12)),
        ];
        for ev in untouched {
            assert_eq!(
                encode_key(ev, false),
                encode_key(ev, true),
                "{:?} is not a cursor key — DECCKM must not touch it",
                ev.code
            );
        }
    }

    /// Regression table for a child that never touches DECCKM (the common case):
    /// every key the encoder handles, byte-for-byte as it was before the mode
    /// argument existed. If this table changes, somebody's pane input changed.
    #[test]
    fn normal_mode_encoding_is_unchanged() {
        let ctrl = KeyModifiers::CONTROL;
        let shift = KeyModifiers::SHIFT;
        let alt = KeyModifiers::ALT;
        let table: [(KeyEvent, &[u8]); 32] = [
            (k(KeyCode::Char('a')), b"a"),
            (k_mod(KeyCode::Char('x'), alt), b"\x1bx"),
            (k_ctrl(KeyCode::Char('c')), &[0x03]),
            (k_ctrl(KeyCode::Char(' ')), &[0x00]),
            (k_ctrl(KeyCode::Char('\\')), &[0x1c]),
            (k_ctrl(KeyCode::Char('?')), &[0x1f]),
            (k(KeyCode::Enter), b"\r"),
            (k_mod(KeyCode::Enter, shift), b"\n"),
            (k(KeyCode::Tab), b"\t"),
            (k(KeyCode::BackTab), b"\x1b[Z"),
            (k(KeyCode::Backspace), &[0x7f]),
            (k(KeyCode::Esc), &[0x1b]),
            (k(KeyCode::Up), b"\x1b[A"),
            (k(KeyCode::Down), b"\x1b[B"),
            (k(KeyCode::Right), b"\x1b[C"),
            (k(KeyCode::Left), b"\x1b[D"),
            (k(KeyCode::Home), b"\x1b[H"),
            (k(KeyCode::End), b"\x1b[F"),
            (k_mod(KeyCode::Up, ctrl), b"\x1b[1;5A"),
            (k_mod(KeyCode::Home, shift), b"\x1b[1;2H"),
            (k_mod(KeyCode::Up, KeyModifiers::SUPER), b"\x1b[A"),
            (k(KeyCode::PageUp), b"\x1b[5~"),
            (k(KeyCode::PageDown), b"\x1b[6~"),
            (k(KeyCode::Delete), b"\x1b[3~"),
            (k(KeyCode::Insert), b"\x1b[2~"),
            (k_mod(KeyCode::Delete, ctrl), b"\x1b[3;5~"),
            (k(KeyCode::F(1)), b"\x1bOP"),
            (k(KeyCode::F(4)), b"\x1bOS"),
            (k_mod(KeyCode::F(1), shift), b"\x1b[1;2P"),
            (k(KeyCode::F(5)), b"\x1b[15~"),
            (k(KeyCode::F(12)), b"\x1b[24~"),
            (k(KeyCode::F(13)), b""), // beyond F12 ⇒ nothing happened
        ];
        for (ev, expected) in table {
            assert_eq!(
                enc(ev),
                expected,
                "{:?}+{:?} normal-mode bytes",
                ev.code,
                ev.modifiers
            );
        }
    }

    /// A restored pane gets a fresh emulator, so it starts in normal mode
    /// whatever the child had declared before the save — no cursor-key state is
    /// persisted (nor could be: the mode belongs to the process, and a restored
    /// pane spawns a new one, which re-declares as it draws).
    #[test]
    fn a_restored_pane_starts_in_normal_cursor_mode() {
        assert!(!child_mode(b""), "a fresh parser reports normal mode");
        assert_eq!(encode_key(k(KeyCode::Up), child_mode(b"")), b"\x1b[A");
    }

    #[test]
    fn f1_through_f12() {
        assert_eq!(enc(k(KeyCode::F(1))), b"\x1bOP");
        assert_eq!(enc(k(KeyCode::F(5))), b"\x1b[15~");
        assert_eq!(enc(k(KeyCode::F(12))), b"\x1b[24~");
    }

    /// Unmodified edit/nav keys must stay byte-identical to the pre-modifier
    /// encoding (no regression for the common case).
    #[test]
    fn unmodified_special_keys_unchanged() {
        assert_eq!(enc(k(KeyCode::Home)), b"\x1b[H");
        assert_eq!(enc(k(KeyCode::End)), b"\x1b[F");
        assert_eq!(enc(k(KeyCode::Delete)), b"\x1b[3~");
        assert_eq!(enc(k(KeyCode::Insert)), b"\x1b[2~");
        assert_eq!(enc(k(KeyCode::PageUp)), b"\x1b[5~");
        assert_eq!(enc(k(KeyCode::PageDown)), b"\x1b[6~");
    }

    /// Ctrl+Arrow (word motion in readline/editors) → `CSI 1 ; 5 <final>`.
    #[test]
    fn ctrl_arrows_encode_word_motion() {
        let c = KeyModifiers::CONTROL;
        assert_eq!(enc(k_mod(KeyCode::Right, c)), b"\x1b[1;5C");
        assert_eq!(enc(k_mod(KeyCode::Left, c)), b"\x1b[1;5D");
        assert_eq!(enc(k_mod(KeyCode::Up, c)), b"\x1b[1;5A");
        assert_eq!(enc(k_mod(KeyCode::Down, c)), b"\x1b[1;5B");
    }

    /// Shift/Alt arrows + Home/End use the same form with their own param.
    #[test]
    fn shift_and_alt_modifiers() {
        assert_eq!(enc(k_mod(KeyCode::Up, KeyModifiers::SHIFT)), b"\x1b[1;2A");
        assert_eq!(enc(k_mod(KeyCode::Left, KeyModifiers::ALT)), b"\x1b[1;3D");
        assert_eq!(enc(k_mod(KeyCode::Home, KeyModifiers::SHIFT)), b"\x1b[1;2H");
        assert_eq!(enc(k_mod(KeyCode::End, KeyModifiers::SHIFT)), b"\x1b[1;2F");
    }

    /// Tilde keys (Delete, PageUp) carry the modifier param before the `~`.
    #[test]
    fn modified_tilde_keys() {
        assert_eq!(
            enc(k_mod(KeyCode::Delete, KeyModifiers::CONTROL)),
            b"\x1b[3;5~"
        );
        assert_eq!(
            enc(k_mod(KeyCode::PageUp, KeyModifiers::SHIFT)),
            b"\x1b[5;2~"
        );
    }

    /// F1–F4 flip from SS3 to CSI when modified; F5+ stay tilde-form.
    #[test]
    fn modified_function_keys() {
        assert_eq!(enc(k_mod(KeyCode::F(1), KeyModifiers::SHIFT)), b"\x1b[1;2P");
        assert_eq!(
            enc(k_mod(KeyCode::F(5), KeyModifiers::CONTROL)),
            b"\x1b[15;5~"
        );
    }

    /// Combined modifiers sum into one param: Ctrl+Shift = 4+1 ⇒ code 6.
    #[test]
    fn combined_modifiers_sum() {
        assert_eq!(
            enc(k_mod(
                KeyCode::Right,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            b"\x1b[1;6C"
        );
    }

    /// Super/Meta/Hyper alone aren't encodable ⇒ fall back to the bare
    /// sequence (no regression vs. today, and no sequence apps can't parse).
    #[test]
    fn super_only_falls_back_to_bare() {
        assert_eq!(enc(k_mod(KeyCode::Up, KeyModifiers::SUPER)), b"\x1b[A");
    }

    #[test]
    fn modifier_param_masks() {
        assert_eq!(modifier_param(KeyModifiers::empty()), None);
        assert_eq!(modifier_param(KeyModifiers::SHIFT), Some(2));
        assert_eq!(modifier_param(KeyModifiers::ALT), Some(3));
        assert_eq!(modifier_param(KeyModifiers::CONTROL), Some(5));
        assert_eq!(
            modifier_param(KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            Some(6)
        );
        assert_eq!(
            modifier_param(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT),
            Some(8)
        );
        assert_eq!(modifier_param(KeyModifiers::SUPER), None);
    }
}
