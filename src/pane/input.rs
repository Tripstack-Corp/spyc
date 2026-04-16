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
            if alt {
                // Alt+Enter → newline (used by Claude CLI for multi-line input).
                out.push(b'\n');
            } else {
                out.push(b'\r');
            }
        }
        K::Tab => out.push(b'\t'),
        K::BackTab => out.extend_from_slice(b"\x1b[Z"),
        K::Backspace => out.push(0x7f),
        K::Esc => out.push(0x1b),
        K::Up => out.extend_from_slice(b"\x1b[A"),
        K::Down => out.extend_from_slice(b"\x1b[B"),
        K::Right => out.extend_from_slice(b"\x1b[C"),
        K::Left => out.extend_from_slice(b"\x1b[D"),
        K::Home => out.extend_from_slice(b"\x1b[H"),
        K::End => out.extend_from_slice(b"\x1b[F"),
        K::PageUp => out.extend_from_slice(b"\x1b[5~"),
        K::PageDown => out.extend_from_slice(b"\x1b[6~"),
        K::Delete => out.extend_from_slice(b"\x1b[3~"),
        K::Insert => out.extend_from_slice(b"\x1b[2~"),
        K::F(n) => match n {
            1 => out.extend_from_slice(b"\x1bOP"),
            2 => out.extend_from_slice(b"\x1bOQ"),
            3 => out.extend_from_slice(b"\x1bOR"),
            4 => out.extend_from_slice(b"\x1bOS"),
            5 => out.extend_from_slice(b"\x1b[15~"),
            6 => out.extend_from_slice(b"\x1b[17~"),
            7 => out.extend_from_slice(b"\x1b[18~"),
            8 => out.extend_from_slice(b"\x1b[19~"),
            9 => out.extend_from_slice(b"\x1b[20~"),
            10 => out.extend_from_slice(b"\x1b[21~"),
            11 => out.extend_from_slice(b"\x1b[23~"),
            12 => out.extend_from_slice(b"\x1b[24~"),
            _ => {}
        },
        _ => {}
    }
    out
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
}
