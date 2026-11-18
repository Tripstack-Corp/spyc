//! Shelling out: editor, pager, `%` substitution.
//!
//! Running a child process from a TUI requires tearing the terminal state
//! down so the child can own the tty, then restoring our state when it
//! exits. The actual teardown helpers live in `main.rs` because they touch
//! the `Tui` value directly; this module supplies the policy (which binary,
//! which args, whether a file is viewable).

pub mod assignments;
pub mod expand;

use assignments::program_word;

pub use expand::{PanePath, expand_percent, pane_path, pane_path_payload, shell_quote};

use std::io::Read;
use std::path::Path;

/// $EDITOR, fall back to $VISUAL, fall back to `vi`.
pub fn resolve_editor() -> Vec<String> {
    #[cfg(test)]
    if let Some(editor) = EDITOR_OVERRIDE.with(|e| e.borrow().clone()) {
        return split_command(&editor);
    }
    let raw = crate::envset::var("VISUAL")
        .filter(|s| !s.is_empty())
        .or_else(|| crate::envset::var("EDITOR").filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "vi".to_string());
    split_command(&raw)
}

#[cfg(test)]
thread_local! {
    static EDITOR_OVERRIDE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Test-only: run `body` with [`resolve_editor`] answering `editor`, so a test
/// that opens an editor never starts the developer's own. Unwound when `body`
/// returns *or panics*.
#[cfg(test)]
pub fn with_editor<R>(editor: &str, body: impl FnOnce() -> R) -> R {
    EDITOR_OVERRIDE.with(|e| *e.borrow_mut() = Some(editor.to_string()));
    let _guard =
        std::mem::DropGuard::new((), |()| EDITOR_OVERRIDE.with(|e| *e.borrow_mut() = None));
    body()
}

/// $PAGER, fall back to `less`. Used by spyc's `p` binding to hand
/// the file off to a real pager (full TTY takeover via suspend_tui),
/// which is the right tool for huge files / interactive search /
/// line-folding-on-demand. Spyc's in-app pager remains the default
/// for normal viewing; `p` is the escape hatch.
pub fn resolve_pager() -> Vec<String> {
    #[cfg(test)]
    if let Some(pager) = PAGER_OVERRIDE.with(|p| p.borrow().clone()) {
        return split_command(&pager);
    }
    let raw = crate::envset::var("PAGER")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "less".to_string());
    split_command(&raw)
}

#[cfg(test)]
thread_local! {
    static PAGER_OVERRIDE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Test-only: run `body` with [`resolve_pager`] answering `pager`, as
/// [`with_editor`] does for the editor.
#[cfg(test)]
pub fn with_pager<R>(pager: &str, body: impl FnOnce() -> R) -> R {
    PAGER_OVERRIDE.with(|p| *p.borrow_mut() = Some(pager.to_string()));
    let _guard = std::mem::DropGuard::new((), |()| PAGER_OVERRIDE.with(|p| *p.borrow_mut() = None));
    body()
}

/// Split an `$EDITOR`-style value into `[program, arg, arg, ...]` on
/// whitespace. This is what git does. People who need shell features set
/// `EDITOR` to a wrapper script.
fn split_command(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(ToString::to_string).collect()
}

/// How to spawn a pty command: `(shell_path, [args...])`, ready to feed to a
/// process spawner. Pane tabs, `:!cmd` captures and `;cmd` foreground panes
/// all come through here.
///
/// The command runs under the user's `$SHELL` with `-i` so rc files fire and
/// their aliases / functions / rc-set PATH work, matching a regular terminal
/// tab; without it `$- == ""` and the shell skips interactive startup.
///
/// SPYC-TRAP(pane-shell-rc-double-source): an `exec_replace` pane whose
/// command is itself an rc-sourcing shell must NOT get the wrapper's `-i`.
pub fn pane_invocation(command: &str, exec_replace: bool) -> (String, Vec<String>) {
    pane_invocation_for(pane_shell().as_deref(), command, exec_replace)
}

/// `$SHELL`, except under unit tests, which get `/bin/sh` so no rc runs: a
/// developer's rc is outside the test's control, and one that starts
/// background helpers (powerlevel10k's gitstatus) orphans them when a pane
/// execs or is killed mid-startup.
fn pane_shell() -> Option<String> {
    if cfg!(test) {
        #[cfg(test)]
        return TEST_SHELL
            .with(|shell| shell.borrow().clone())
            .or_else(|| Some("/bin/sh".to_string()));
    }
    crate::envset::var("SHELL")
}

/// Pure half of `pane_invocation`, taking the SHELL value as an argument.
fn pane_invocation_for(
    shell: Option<&str>,
    command: &str,
    exec_replace: bool,
) -> (String, Vec<String>) {
    // `exec` so the rc-sourcing shell replaces itself with the command and no
    // job-control wrapper survives to fight `^z`. Empty command → bare shell.
    let exec = exec_replace && !command.trim().is_empty();
    let cmd = if exec {
        exec_command(command)
    } else {
        command.to_string()
    };
    // When the target sources its own rc, the wrapper's would be the second
    // pass under one pid — see the trap rationale on `pane_invocation`.
    let interactive_rc = !(exec && command_is_interactive_shell(command));
    user_shell_invocation_for(shell, &cmd, interactive_rc)
}

/// `command` with `exec` before its program, after any assignments it opens
/// with: `exec FOO=1 claude` would have the shell look for a program named
/// `FOO=1`.
fn exec_command(command: &str) -> String {
    match assignments::split_assignments(command) {
        ("", _) => format!("exec {command}"),
        (_, "") => command.to_string(),
        (assignments, program) => format!("{assignments} exec {program}"),
    }
}

/// True when a command is itself a shell that sources an interactive startup
/// file — `zsh`, `/bin/bash`, `fish -l`, `FOO=1 zsh`. Only the program word
/// counts, so a `sudo`- or `env`-wrapped command doesn't match.
fn command_is_interactive_shell(command: &str) -> bool {
    program_word(command)
        .and_then(|w| Path::new(w).file_name())
        .and_then(|n| n.to_str())
        .is_some_and(sources_interactive_rc)
}

/// Shell basenames that source a startup file when interactive — so `-i`
/// earns its keep, and so a nested one would source it twice. POSIX `sh` and
/// `dash` are absent: they read no rc file in `-i` mode.
fn sources_interactive_rc(basename: &str) -> bool {
    matches!(basename, "zsh" | "bash" | "fish" | "ksh" | "ksh93" | "mksh")
}

/// True when a pane command is a shell — a child that runs its own job
/// control, so `^z` is *its* key (background the foreground job) and must
/// reach the pty rather than becoming spyc's managed suspend.
///
/// Deliberately broader than [`sources_interactive_rc`]: `sh`, `dash` and the
/// csh family read no interactive rc but still background jobs. An empty
/// command spawns the bare `$SHELL`, so it counts too — and "unsure" should
/// fall on the side of forwarding, never of swallowing a key.
pub fn command_is_shell(command: &str) -> bool {
    let Some(word) = program_word(command) else {
        return true; // empty ⇒ bare $SHELL
    };
    Path::new(word)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|basename| {
            sources_interactive_rc(basename)
                || matches!(basename, "sh" | "dash" | "csh" | "tcsh" | "ash" | "yash")
        })
}

/// Builds the `$SHELL -i -c <cmd>` invocation, taking the SHELL value as an
/// argument. Tests call this directly so they don't need to mutate the
/// process-global env var.
fn user_shell_invocation_for(
    shell: Option<&str>,
    cmd: &str,
    interactive_rc: bool,
) -> (String, Vec<String>) {
    let shell = shell
        .filter(|s| !s.is_empty())
        .map_or_else(|| "/bin/sh".to_string(), ToString::to_string);
    let basename = Path::new(&shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sh");
    let interactive = interactive_rc && sources_interactive_rc(basename);
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

    /// A pane a unit test spawns must not run the developer's interactive
    /// shell. Its rc is outside the test's control, and powerlevel10k's
    /// gitstatus left an orphaned helper behind for many of the spawns.
    #[test]
    fn a_test_pane_runs_sh_without_an_rc() {
        let (sh, args) = pane_invocation("cat", true);
        assert_eq!(sh, "/bin/sh");
        assert_eq!(args, vec!["-c", "exec cat"]);
    }

    #[test]
    fn user_shell_zsh_gets_interactive_flag() {
        let (sh, args) = user_shell_invocation_for(Some("/bin/zsh"), "echo hi", true);
        assert_eq!(sh, "/bin/zsh");
        assert_eq!(args, vec!["-i", "-c", "echo hi"]);
    }

    #[test]
    fn user_shell_bash_gets_interactive_flag() {
        let (sh, args) = user_shell_invocation_for(Some("/usr/local/bin/bash"), "ls", true);
        assert_eq!(sh, "/usr/local/bin/bash");
        assert_eq!(args, vec!["-i", "-c", "ls"]);
    }

    #[test]
    fn user_shell_posix_sh_skips_interactive() {
        let (sh, args) = user_shell_invocation_for(Some("/bin/sh"), "ls", true);
        assert_eq!(sh, "/bin/sh");
        assert_eq!(args, vec!["-c", "ls"]);
    }

    #[test]
    fn user_shell_dash_skips_interactive() {
        let (_, args) = user_shell_invocation_for(Some("/bin/dash"), "ls", true);
        assert_eq!(args, vec!["-c", "ls"]);
    }

    #[test]
    fn user_shell_unset_falls_back_to_sh() {
        let (sh, args) = user_shell_invocation_for(None, "ls", true);
        assert_eq!(sh, "/bin/sh");
        assert_eq!(args, vec!["-c", "ls"]);
    }

    #[test]
    fn user_shell_empty_falls_back_to_sh() {
        let (sh, _) = user_shell_invocation_for(Some(""), "ls", true);
        assert_eq!(sh, "/bin/sh");
    }

    #[test]
    fn user_shell_rc_minimal_drops_interactive_flag() {
        let (_, args) = user_shell_invocation_for(Some("/bin/zsh"), "exec zsh", false);
        assert_eq!(args, vec!["-c", "exec zsh"]);
    }

    // An agent keeps the interactive wrapper: it needs the rc-file PATH to be
    // found at all, and it sources no startup file of its own.
    #[test]
    fn pane_agent_command_execs_under_interactive_shell() {
        let (sh, args) = pane_invocation_for(Some("/bin/zsh"), "claude", true);
        assert_eq!(sh, "/bin/zsh");
        assert_eq!(args, vec!["-i", "-c", "exec claude"]);
    }

    // The regression this guards: `^a c` + `zsh` used to source .zshrc twice
    // under one pid (exec preserves it), colliding p10k/gitstatus lock files.
    #[test]
    fn pane_shell_command_drops_the_wrappers_rc_pass() {
        let (_, args) = pane_invocation_for(Some("/bin/zsh"), "zsh", true);
        assert_eq!(args, vec!["-c", "exec zsh"]);
    }

    #[test]
    fn pane_shell_command_matches_on_basename_and_first_word() {
        for cmd in ["/bin/bash", "fish -l", "/opt/homebrew/bin/zsh"] {
            let (_, args) = pane_invocation_for(Some("/bin/zsh"), cmd, true);
            assert!(!args.contains(&"-i".to_string()), "{cmd} kept the -i pass");
        }
    }

    // `sh` and `dash` read no rc file, so the wrapper's pass is the only one
    // there is — dropping it would strip the user's rc PATH for nothing.
    #[test]
    fn pane_non_rc_shell_command_keeps_the_wrapper_pass() {
        for cmd in ["sh", "/bin/dash"] {
            let (_, args) = pane_invocation_for(Some("/bin/zsh"), cmd, true);
            let want = vec!["-i".to_string(), "-c".to_string(), format!("exec {cmd}")];
            assert_eq!(args, want, "{cmd}");
        }
    }

    // Conservative: only the program word is inspected, so anything wrapping a
    // shell still goes through the normal interactive path.
    #[test]
    fn pane_wrapped_shell_command_is_not_treated_as_a_shell() {
        for cmd in ["sudo zsh", "env FOO=1 zsh", "zshx", "myzsh"] {
            let (_, args) = pane_invocation_for(Some("/bin/zsh"), cmd, true);
            let want = vec!["-i".to_string(), "-c".to_string(), format!("exec {cmd}")];
            assert_eq!(args, want, "{cmd}");
        }
    }

    // `exec FOO=1 claude` has the shell look for a program named `FOO=1`: the
    // assignments come first, then `exec` and the program they're for.
    #[test]
    fn pane_env_prefixed_command_execs_its_program() {
        let (_, args) = pane_invocation_for(Some("/bin/zsh"), "FOO=1 BAR='a b' claude --x", true);
        assert_eq!(args, vec!["-i", "-c", "FOO=1 BAR='a b' exec claude --x"]);
    }

    /// The requirement, run by a real shell: the program starts and sees the
    /// variables.
    #[test]
    fn an_env_prefixed_pane_command_runs_with_its_variables() {
        let command = r#"SPYC_A=1 SPYC_B="two words" sh -c 'echo "$SPYC_A:$SPYC_B"'"#;
        let (sh, args) = pane_invocation_for(Some("/bin/sh"), command, true);
        let out = std::process::Command::new(sh).args(&args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), "1:two words\n");
    }

    // An env-prefixed shell is still a shell that sources its own rc.
    #[test]
    fn pane_env_prefixed_shell_command_drops_the_wrappers_rc_pass() {
        let (_, args) = pane_invocation_for(Some("/bin/zsh"), "FOO=1 zsh", true);
        assert_eq!(args, vec!["-c", "FOO=1 exec zsh"]);
    }

    // Non-exec_replace panes (captures, background tasks) keep the wrapper —
    // it owns job control there, so there is nothing to exec away.
    #[test]
    fn pane_without_exec_replace_is_left_alone() {
        let (_, args) = pane_invocation_for(Some("/bin/zsh"), "zsh", false);
        assert_eq!(args, vec!["-i", "-c", "zsh"]);
    }

    #[test]
    fn pane_empty_command_spawns_a_bare_shell() {
        let (_, args) = pane_invocation_for(Some("/bin/zsh"), "", true);
        assert_eq!(args, vec!["-i", "-c", ""]);
    }

    #[test]
    fn shells_own_their_job_control() {
        // Includes the rc-less ones: `sh`/`dash` background jobs all the same,
        // so `^z` is theirs even though they take no `-i`.
        for cmd in [
            "zsh",
            "/bin/bash",
            "fish -l",
            "sh",
            "dash",
            "tcsh",
            "FOO=1 zsh",
        ] {
            assert!(command_is_shell(cmd), "{cmd}");
        }
        // Empty ⇒ bare $SHELL. "Unsure" must forward, never swallow.
        assert!(command_is_shell(""));
        assert!(command_is_shell("   "));
    }

    #[test]
    fn non_shell_pane_commands_leave_ctrl_z_to_spyc() {
        // The reported case: a raw-mode TUI runs with ISIG off, so a forwarded
        // ^z is a byte it discards — spyc's managed suspend has to take it.
        for cmd in ["claude", "rmatrix", "htop", "vim src/lib.rs", "npm run dev"] {
            assert!(!command_is_shell(cmd), "{cmd}");
        }
        // Same program-word-only conservatism as `pane_invocation`: a wrapped
        // shell isn't one (the wrapper is the child that sees the key).
        for cmd in ["sudo zsh", "env FOO=1 zsh", "zshx", "myzsh", "FOO=1 claude"] {
            assert!(!command_is_shell(cmd), "{cmd}");
        }
    }
}

#[cfg(test)]
thread_local! {
    static TEST_SHELL: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Controlled rc fixture; every other test still uses the rc-free system shell.
#[cfg(test)]
pub fn with_test_shell<T>(shell: &str, run: impl FnOnce() -> T) -> T {
    let previous = TEST_SHELL.with(|value| value.replace(Some(shell.to_string())));
    let _reset = std::mem::DropGuard::new(previous, |previous| {
        TEST_SHELL.with(|shell| *shell.borrow_mut() = previous);
    });
    run()
}
