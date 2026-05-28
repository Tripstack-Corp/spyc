//! Shelling out: editor, pager, `%` substitution.
//!
//! Running a child process from a TUI requires tearing the terminal state
//! down so the child can own the tty, then restoring our state when it
//! exits. The actual teardown helpers live in `main.rs` because they touch
//! the `Tui` value directly; this module supplies the policy (which binary,
//! which args, whether a file is viewable).

pub mod expand;

pub use expand::{expand_percent, shell_quote};

use std::io::Read;
use std::path::Path;

/// $EDITOR, fall back to $VISUAL, fall back to `vi`.
pub fn resolve_editor() -> Vec<String> {
    let raw = crate::envset::var("VISUAL")
        .filter(|s| !s.is_empty())
        .or_else(|| crate::envset::var("EDITOR").filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "vi".to_string());
    split_command(&raw)
}

/// $PAGER, fall back to `less`. Used by spyc's `p` binding to hand
/// the file off to a real pager (full TTY takeover via suspend_tui),
/// which is the right tool for huge files / interactive search /
/// line-folding-on-demand. Spyc's in-app pager remains the default
/// for normal viewing; `p` is the escape hatch.
pub fn resolve_pager() -> Vec<String> {
    let raw = crate::envset::var("PAGER")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "less".to_string());
    split_command(&raw)
}

/// Split an `$EDITOR`-style value into `[program, arg, arg, ...]` on
/// whitespace. This is what git does. People who need shell features set
/// `EDITOR` to a wrapper script.
fn split_command(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(ToString::to_string).collect()
}

/// Resolve the user's preferred interactive shell for running
/// commands that need alias / function / rc-file PATH resolution.
/// Returns `(shell_path, [args...])` ready to feed to a process
/// spawner.
///
/// `:!cmd` (capture) and `;cmd` (foreground pane) both go through
/// this. Without `-i`, aliases defined in `.zshrc` / `.bashrc`
/// aren't loaded — `$- == ""` and the shell skips interactive
/// startup. With `-i`, rc files fire and the user's aliases /
/// functions / rc-set PATH all work, matching what they see in a
/// regular terminal tab.
///
/// POSIX `sh` and `dash` don't read rc files in `-i` mode anyway
/// (and dash warns about it), so we only set `-i` for shells that
/// actually source a startup file interactively.
pub fn user_shell_invocation(cmd: &str) -> (String, Vec<String>) {
    let shell = crate::envset::var("SHELL");
    user_shell_invocation_for(shell.as_deref(), cmd)
}

/// Pure version of `user_shell_invocation` that takes the SHELL value
/// as an argument. Tests call this directly so they don't need to
/// mutate the process-global env var.
fn user_shell_invocation_for(shell: Option<&str>, cmd: &str) -> (String, Vec<String>) {
    let shell = shell
        .filter(|s| !s.is_empty())
        .map_or_else(|| "/bin/sh".to_string(), ToString::to_string);
    let basename = Path::new(&shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sh");
    let interactive = matches!(basename, "zsh" | "bash" | "fish" | "ksh" | "ksh93" | "mksh");
    let mut args = Vec::with_capacity(3);
    if interactive {
        args.push("-i".to_string());
    }
    args.push("-c".to_string());
    args.push(cmd.to_string());
    (shell, args)
}

/// Heuristic text/binary detection: look for a NUL byte in the first 8 KiB.
/// Matches what `grep` and `file` effectively do.
pub fn looks_like_text(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 8192];
    let Ok(read) = f.read(&mut buf) else {
        return false;
    };
    !buf[..read].contains(&0u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_shell_zsh_gets_interactive_flag() {
        let (sh, args) = user_shell_invocation_for(Some("/bin/zsh"), "echo hi");
        assert_eq!(sh, "/bin/zsh");
        assert_eq!(args, vec!["-i", "-c", "echo hi"]);
    }

    #[test]
    fn user_shell_bash_gets_interactive_flag() {
        let (sh, args) = user_shell_invocation_for(Some("/usr/local/bin/bash"), "ls");
        assert_eq!(sh, "/usr/local/bin/bash");
        assert_eq!(args, vec!["-i", "-c", "ls"]);
    }

    #[test]
    fn user_shell_posix_sh_skips_interactive() {
        let (sh, args) = user_shell_invocation_for(Some("/bin/sh"), "ls");
        assert_eq!(sh, "/bin/sh");
        assert_eq!(args, vec!["-c", "ls"]);
    }

    #[test]
    fn user_shell_dash_skips_interactive() {
        let (_, args) = user_shell_invocation_for(Some("/bin/dash"), "ls");
        assert_eq!(args, vec!["-c", "ls"]);
    }

    #[test]
    fn user_shell_unset_falls_back_to_sh() {
        let (sh, args) = user_shell_invocation_for(None, "ls");
        assert_eq!(sh, "/bin/sh");
        assert_eq!(args, vec!["-c", "ls"]);
    }

    #[test]
    fn user_shell_empty_falls_back_to_sh() {
        let (sh, _) = user_shell_invocation_for(Some(""), "ls");
        assert_eq!(sh, "/bin/sh");
    }
}
