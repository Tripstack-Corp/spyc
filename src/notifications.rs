//! Desktop notifications + terminal bell for agent status changes.
//!
//! The P3-1 "which agent needs me" ping (`docs/AGENT_AWARENESS_PLAN.md`): when an
//! agent pane transitions to `Blocked` / `Done`, `App::settle_agent_activity`
//! emits a `Notify` effect and `run_effects` routes it here.
//!
//! `send` is **fire-and-forget on a detached thread** — a desktop notification is
//! a system / D-Bus round trip that must never block the single event loop (the
//! no-blocking-IO-on-the-loop invariant). `ring_bell` writes a bare BEL to stdout
//! (cheap, inline).
//!
//! Two desktop-notification mechanisms, chosen per `[notify].desktop_via`:
//! - `send` — the OS notifier (notify-rust): macOS `mac-notification-sys`, Linux
//!   `zbus` (pure-Rust D-Bus), Windows WinRT. Lands on the machine spyc *runs*
//!   on, so over SSH it's the remote host, not the client.
//! - `notify_osc9` — an OSC 9 escape to the host terminal, which the emulator
//!   turns into a native notification. Over SSH this pops on the **client**
//!   (where your eyes are) and needs no dependency — but only terminals that
//!   support OSC 9 (iTerm2, kitty, WezTerm, …) show it.
//!
//! The `auto` default routes to OSC 9 over SSH and the OS notifier locally.

use std::io::{self, Write};

/// Fire a desktop notification titled `summary` with detail `body`, off-thread
/// and best-effort: an unsupported / failed backend is silently ignored (the tab
/// dot and any bell still convey the state). Never blocks the caller — the
/// `.show()` D-Bus/system round trip runs on a detached thread that exits as soon
/// as it returns.
pub fn send(summary: String, body: String) {
    std::thread::spawn(move || {
        let _ = notify_rust::Notification::new()
            .appname("spyc")
            .summary(&summary)
            .body(&body)
            .show();
    });
}

/// Ring the terminal bell (BEL). Cheap enough to run inline on the loop; a bare
/// `\x07` reaches the host terminal (tmux forwards it per the user's bell
/// settings). A failed stdout write is ignored — not worth aborting the loop.
pub fn ring_bell() {
    let mut out = io::stdout();
    let _ = out.write_all(b"\x07");
    let _ = out.flush();
}

/// Emit an OSC 9 desktop-notification escape to the host terminal. Unlike
/// [`send`], this reaches the terminal *emulator*, so over SSH the notification
/// pops on the **client** machine. `message` is control-char-sanitized before it
/// goes into the sequence — an embedded `\x1b`/`\x07` (the OSC terminator) could
/// otherwise close it early and inject arbitrary escapes — and wrapped in tmux's
/// DCS passthrough when inside tmux, mirroring `term_title`. Terminals without
/// OSC 9 support silently ignore it; a failed stdout write is ignored.
pub fn notify_osc9(message: &str) {
    let wrapped = osc9_sequence(message, std::env::var_os("TMUX").is_some());
    let mut out = io::stdout();
    let _ = out.write_all(wrapped.as_bytes());
    let _ = out.flush();
}

/// Build the OSC 9 notification byte sequence for `message`. Pure — the caller
/// passes `in_tmux` so tests exercise both modes without touching the global
/// `TMUX` env var (the `term_title::wrap` template). `message` is
/// control-char-sanitized first: an embedded `\x1b`/`\x07` (the OSC terminator)
/// could otherwise close the sequence early and inject arbitrary escapes. Inside
/// tmux the whole thing is DCS-wrapped with its inner ESCs doubled.
fn osc9_sequence(message: &str, in_tmux: bool) -> String {
    let clean: String = message.chars().filter(|c| !c.is_control()).collect();
    let seq = format!("\x1b]9;{clean}\x07");
    if in_tmux {
        format!("\x1bPtmux;{}\x1b\\", seq.replace('\x1b', "\x1b\x1b"))
    } else {
        seq
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc9_wraps_message_outside_tmux() {
        assert_eq!(osc9_sequence("build done", false), "\x1b]9;build done\x07");
    }

    #[test]
    fn osc9_strips_control_chars_so_an_escape_cant_break_out() {
        // A tab name crafted to close the OSC early and inject a second escape
        // (an OSC 52 clipboard write). The sanitizer must drop every control
        // byte, leaving exactly one leading ESC and one terminating BEL.
        let hostile = "tab\x07\x1b]52;c;ZXZpbA==\x07";
        let seq = osc9_sequence(hostile, false);
        assert_eq!(seq, "\x1b]9;tab]52;c;ZXZpbA==\x07");
        assert_eq!(seq.matches('\x1b').count(), 1, "only the opening ESC");
        assert_eq!(seq.matches('\x07').count(), 1, "only the terminating BEL");
    }

    #[test]
    fn osc9_doubles_esc_and_wraps_in_dcs_inside_tmux() {
        assert_eq!(
            osc9_sequence("hi", true),
            "\x1bPtmux;\x1b\x1b]9;hi\x07\x1b\\"
        );
    }
}
