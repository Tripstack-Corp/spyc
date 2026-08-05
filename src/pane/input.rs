//! Keyboard → pty byte encoding.
//!
//! Most terminal applications expect xterm-style escape sequences for
//! special keys. We cover the common cases (arrows, Home/End, PgUp/PgDn,
//! function keys, Ctrl+letter, plain chars, Tab/Enter/Backspace/Esc);
//! unusual combinations fall through as an empty slice, which is the
//! terminal's "nothing happened" signal.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn encode_key(ev: KeyEvent) -> Vec<u8> {
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
        K::Up => push_csi_final(&mut out, ev.modifiers, b'A'),
        K::Down => push_csi_final(&mut out, ev.modifiers, b'B'),
        K::Right => push_csi_final(&mut out, ev.modifiers, b'C'),
        K::Left => push_csi_final(&mut out, ev.modifiers, b'D'),
        K::Home => push_csi_final(&mut out, ev.modifiers, b'H'),
        K::End => push_csi_final(&mut out, ev.modifiers, b'F'),
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
/// to the pre-modifier behavior. Super/Meta/Hyper are deliberately excluded:
/// terminals don't agree on a code for them, so falling back to the bare
/// sequence (today's behavior) beats sending one apps won't recognize.
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

/// Cursor/edit keys on the `CSI [1;<mod>] <final>` form (arrows, Home, End):
/// bare when unmodified (`ESC [ A`), parameterized when modified
/// (`ESC [ 1 ; 5 C` = Ctrl+Right).
fn push_csi_final(out: &mut Vec<u8>, m: KeyModifiers, final_byte: u8) {
    out.extend_from_slice(b"\x1b[");
    if let Some(p) = modifier_param(m) {
        out.extend_from_slice(b"1;");
        push_dec(out, p);
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
    // `Press` (X10) reports presses only — a release would be noise it has no
    // grammar for. Every other active mode reports both.
    match mode {
        M::None => return Vec::new(),
        M::Press if ev.release => return Vec::new(),
        _ => {}
    }

    // xterm's Cb: button in the low bits, modifiers above it.
    let mut cb = u32::from(ev.button);
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
            col: 0,
            row: 0,
            shift: false,
            alt: false,
            ctrl: false,
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

    #[test]
    fn plain_char() {
        assert_eq!(encode_key(k(KeyCode::Char('a'))), b"a");
    }

    #[test]
    fn ctrl_letter() {
        assert_eq!(encode_key(k_ctrl(KeyCode::Char('c'))), vec![0x03]);
        assert_eq!(encode_key(k_ctrl(KeyCode::Char('a'))), vec![0x01]);
    }

    #[test]
    fn enter_is_cr() {
        assert_eq!(encode_key(k(KeyCode::Enter)), b"\r");
    }

    #[test]
    fn arrow_up() {
        assert_eq!(encode_key(k(KeyCode::Up)), b"\x1b[A");
    }

    #[test]
    fn f1_through_f12() {
        assert_eq!(encode_key(k(KeyCode::F(1))), b"\x1bOP");
        assert_eq!(encode_key(k(KeyCode::F(5))), b"\x1b[15~");
        assert_eq!(encode_key(k(KeyCode::F(12))), b"\x1b[24~");
    }

    /// Unmodified edit/nav keys must stay byte-identical to the pre-modifier
    /// encoding (no regression for the common case).
    #[test]
    fn unmodified_special_keys_unchanged() {
        assert_eq!(encode_key(k(KeyCode::Home)), b"\x1b[H");
        assert_eq!(encode_key(k(KeyCode::End)), b"\x1b[F");
        assert_eq!(encode_key(k(KeyCode::Delete)), b"\x1b[3~");
        assert_eq!(encode_key(k(KeyCode::Insert)), b"\x1b[2~");
        assert_eq!(encode_key(k(KeyCode::PageUp)), b"\x1b[5~");
        assert_eq!(encode_key(k(KeyCode::PageDown)), b"\x1b[6~");
    }

    /// Ctrl+Arrow (word motion in readline/editors) → `CSI 1 ; 5 <final>`.
    #[test]
    fn ctrl_arrows_encode_word_motion() {
        let c = KeyModifiers::CONTROL;
        assert_eq!(encode_key(k_mod(KeyCode::Right, c)), b"\x1b[1;5C");
        assert_eq!(encode_key(k_mod(KeyCode::Left, c)), b"\x1b[1;5D");
        assert_eq!(encode_key(k_mod(KeyCode::Up, c)), b"\x1b[1;5A");
        assert_eq!(encode_key(k_mod(KeyCode::Down, c)), b"\x1b[1;5B");
    }

    /// Shift/Alt arrows + Home/End use the same form with their own param.
    #[test]
    fn shift_and_alt_modifiers() {
        assert_eq!(
            encode_key(k_mod(KeyCode::Up, KeyModifiers::SHIFT)),
            b"\x1b[1;2A"
        );
        assert_eq!(
            encode_key(k_mod(KeyCode::Left, KeyModifiers::ALT)),
            b"\x1b[1;3D"
        );
        assert_eq!(
            encode_key(k_mod(KeyCode::Home, KeyModifiers::SHIFT)),
            b"\x1b[1;2H"
        );
        assert_eq!(
            encode_key(k_mod(KeyCode::End, KeyModifiers::SHIFT)),
            b"\x1b[1;2F"
        );
    }

    /// Tilde keys (Delete, PageUp) carry the modifier param before the `~`.
    #[test]
    fn modified_tilde_keys() {
        assert_eq!(
            encode_key(k_mod(KeyCode::Delete, KeyModifiers::CONTROL)),
            b"\x1b[3;5~"
        );
        assert_eq!(
            encode_key(k_mod(KeyCode::PageUp, KeyModifiers::SHIFT)),
            b"\x1b[5;2~"
        );
    }

    /// F1–F4 flip from SS3 to CSI when modified; F5+ stay tilde-form.
    #[test]
    fn modified_function_keys() {
        assert_eq!(
            encode_key(k_mod(KeyCode::F(1), KeyModifiers::SHIFT)),
            b"\x1b[1;2P"
        );
        assert_eq!(
            encode_key(k_mod(KeyCode::F(5), KeyModifiers::CONTROL)),
            b"\x1b[15;5~"
        );
    }

    /// Combined modifiers sum into one param: Ctrl+Shift = 4+1 ⇒ code 6.
    #[test]
    fn combined_modifiers_sum() {
        assert_eq!(
            encode_key(k_mod(
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
        assert_eq!(
            encode_key(k_mod(KeyCode::Up, KeyModifiers::SUPER)),
            b"\x1b[A"
        );
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
