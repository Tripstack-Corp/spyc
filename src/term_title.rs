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

/// Wrap a raw escape sequence in tmux's DCS passthrough when running
/// inside tmux. Inner ESCs must be doubled.
fn wrap(inner: &str) -> String {
    if in_tmux() {
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
    emit(&wrap("\x1b[22;0t"))
}

/// xterm CSI 23;0t — pop the previously-pushed title.
pub fn pop() -> io::Result<()> {
    emit(&wrap("\x1b[23;0t"))
}

/// OSC 2 — set the *window* title only. We deliberately avoid OSC 0
/// (which also sets the icon name) so the macOS Dock label and the
/// iTerm2 tab icon name aren't churned on every state change.
pub fn set(title: &str) -> io::Result<()> {
    emit(&wrap(&format!("\x1b]2;{title}\x07")))
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
        let p = PathBuf::from("/Users/derek/src/spyc");
        let cwd = PathBuf::from("/Users/derek/src/spyc/src");
        let s = compose(Some(&p), Some("SAFFRON_CUMIN"), &cwd);
        assert_eq!(s, "\u{1f336}\u{fe0f}: spyc \u{b7} SAFFRON_CUMIN");
    }

    #[test]
    fn compose_without_session() {
        let p = PathBuf::from("/Users/derek/src/spyc");
        let cwd = PathBuf::from("/Users/derek/src/spyc");
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
        // TMUX is process-global; the shared env_test_lock serializes us
        // against the sibling `wrap_doubles_esc_inside_tmux` test (which
        // sets TMUX) so we don't race and read each other's value mid-call.
        let _lock = crate::state::env_test_lock();
        let prev = std::env::var_os("TMUX");
        unsafe {
            std::env::remove_var("TMUX");
        }
        let w = wrap("\x1b]2;hi\x07");
        assert_eq!(w, "\x1b]2;hi\x07");
        if let Some(p) = prev {
            unsafe {
                std::env::set_var("TMUX", p);
            }
        }
    }

    #[test]
    fn wrap_doubles_esc_inside_tmux() {
        let _lock = crate::state::env_test_lock();
        let prev = std::env::var_os("TMUX");
        unsafe {
            std::env::set_var("TMUX", "/tmp/tmux-1000/default,1234,0");
        }
        let w = wrap("\x1b]2;hi\x07");
        assert_eq!(w, "\x1bPtmux;\x1b\x1b]2;hi\x07\x1b\\");
        match prev {
            Some(p) => unsafe { std::env::set_var("TMUX", p) },
            None => unsafe { std::env::remove_var("TMUX") },
        }
    }
}
