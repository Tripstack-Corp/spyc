//! Set, push, and pop the host terminal's window title.
//!
//! Title is `🌶️: <project> · <session>` (session omitted when there's
//! no `SESSION_NAME`). `<project>` is the basename of `PROJECT_HOME`
//! when set, otherwise the basename of the current cwd.
//!
//! When `$TMUX` is set, every emitted sequence is wrapped in tmux's
//! DCS passthrough so it reaches the outer terminal (e.g. iTerm2).
//! For the wrapped sequences to actually take effect on the outer
//! title, the user needs `set -g set-titles on` in tmux. Absent that,
//! we still emit but the wrap is a no-op outwardly.

use std::io::{self, Write};
use std::path::Path;

fn in_tmux() -> bool {
    std::env::var_os("TMUX").is_some()
}

/// Wrap a raw escape sequence in tmux's DCS passthrough when
/// `in_tmux` is true. Inner ESCs must be doubled in the wrapped form.
/// The caller decides the tmux-ness so tests can exercise both modes
/// without touching the process-global `TMUX` env var.
fn wrap(inner: &str, in_tmux: bool) -> String {
    if in_tmux {
        let doubled = inner.replace('\x1b', "\x1b\x1b");
        format!("\x1bPtmux;{doubled}\x1b\\")
    } else {
        inner.to_string()
    }
}

fn emit(bytes: &str) -> io::Result<()> {
    let mut out = io::stdout();
    out.write_all(bytes.as_bytes())?;
    out.flush()
}

/// xterm CSI 22;0t — push current title onto the terminal's title
/// stack. Supported by iTerm2, xterm, kitty, alacritty, wezterm.
pub fn push() -> io::Result<()> {
    emit(&wrap("\x1b[22;0t", in_tmux()))
}

/// xterm CSI 23;0t — pop the previously-pushed title.
pub fn pop() -> io::Result<()> {
    emit(&wrap("\x1b[23;0t", in_tmux()))
}

/// OSC 2 — set the *window* title only. We deliberately avoid OSC 0
/// (which also sets the icon name) so the macOS Dock label and the
/// iTerm2 tab icon name aren't churned on every state change.
pub fn set(title: &str) -> io::Result<()> {
    emit(&wrap(
        &format!("\x1b]2;{}\x07", sanitize_title(title)),
        in_tmux(),
    ))
}

/// Strip control characters from a title before it goes into an OSC
/// sequence. The title is derived from directory / session names, which an
/// attacker controls; an embedded `\x07` (the OSC terminator), `\x1b`, or a
/// newline could otherwise close the OSC early and inject arbitrary terminal
/// escapes (title-report → fake input, clipboard writes via OSC 52, etc.).
/// `char::is_control` covers C0/C1 + DEL; the emoji logo and `·` separator
/// are not control chars, so legitimate titles are unchanged.
fn sanitize_title(title: &str) -> String {
    title.chars().filter(|c| !c.is_control()).collect()
}

/// Compose the title from project + session info.
pub fn compose(project_home: Option<&Path>, session_name: Option<&str>, cwd: &Path) -> String {
    let logo = "\u{1f336}\u{fe0f}";
    let project = project_home
        .and_then(|p| p.file_name())
        .or_else(|| cwd.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("spyc");
    match session_name {
        Some(s) if !s.is_empty() => format!("{logo}: {project} \u{b7} {s}"),
        _ => format!("{logo}: {project}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn compose_with_project_and_session() {
        let p = PathBuf::from("/Users/x/src/spyc");
        let cwd = PathBuf::from("/Users/x/src/spyc/src");
        let s = compose(Some(&p), Some("SAFFRON_CUMIN"), &cwd);
        assert_eq!(s, "\u{1f336}\u{fe0f}: spyc \u{b7} SAFFRON_CUMIN");
    }

    #[test]
    fn compose_without_session() {
        let p = PathBuf::from("/Users/x/src/spyc");
        let cwd = PathBuf::from("/Users/x/src/spyc");
        let s = compose(Some(&p), None, &cwd);
        assert_eq!(s, "\u{1f336}\u{fe0f}: spyc");
    }

    #[test]
    fn compose_falls_back_to_cwd_basename() {
        let cwd = PathBuf::from("/tmp/scratch");
        let s = compose(None, Some("SAGE_CLOVE"), &cwd);
        assert_eq!(s, "\u{1f336}\u{fe0f}: scratch \u{b7} SAGE_CLOVE");
    }

    #[test]
    fn compose_empty_session_treated_as_none() {
        let p = PathBuf::from("/x/proj");
        let s = compose(Some(&p), Some(""), &p);
        assert_eq!(s, "\u{1f336}\u{fe0f}: proj");
    }

    #[test]
    fn wrap_is_identity_outside_tmux() {
        let w = wrap("\x1b]2;hi\x07", false);
        assert_eq!(w, "\x1b]2;hi\x07");
    }

    #[test]
    fn wrap_doubles_esc_inside_tmux() {
        let w = wrap("\x1b]2;hi\x07", true);
        assert_eq!(w, "\x1bPtmux;\x1b\x1b]2;hi\x07\x1b\\");
    }

    #[test]
    fn sanitize_title_strips_control_chars_keeps_normal() {
        // A directory named to inject an OSC terminator + a second escape.
        let hostile = "proj\x07\x1b]52;c;ZXZpbA==\x07";
        let safe = sanitize_title(hostile);
        assert!(!safe.contains('\x07') && !safe.contains('\x1b'));
        assert_eq!(safe, "proj]52;c;ZXZpbA==");
        // The real composed title (emoji + middle dot) is untouched.
        assert_eq!(
            sanitize_title("\u{1f336}\u{fe0f}: spyc \u{b7} SAGE"),
            "\u{1f336}\u{fe0f}: spyc \u{b7} SAGE"
        );
    }
}
