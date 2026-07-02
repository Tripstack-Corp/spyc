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
    let clean: String = message.chars().filter(|c| !c.is_control()).collect();
    let seq = format!("\x1b]9;{clean}\x07");
    let wrapped = if std::env::var_os("TMUX").is_some() {
        // Inner ESCs doubled, per tmux DCS passthrough (see `term_title::wrap`).
        format!("\x1bPtmux;{}\x1b\\", seq.replace('\x1b', "\x1b\x1b"))
    } else {
        seq
    };
    let mut out = io::stdout();
    let _ = out.write_all(wrapped.as_bytes());
    let _ = out.flush();
}
