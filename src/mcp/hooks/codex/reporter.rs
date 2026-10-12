//! Conservative command ownership for Codex hook edits.

use std::path::Path;

use super::{CODEX_STATUS_HOOKS, reporter_command};

/// Recognize generated commands and bare legacy invocations. `exe` is the
/// caller's resolved reporter, allowing a renamed current executable without
/// interpreting unrelated programs' flags as ownership.
pub(super) fn is_owned(command: &str, exe: Option<&str>) -> bool {
    let command = command.trim();
    let Some(words) = shlex::split(command) else {
        return false;
    };
    let Some(program) = words.first() else {
        return false;
    };
    if Path::new(program)
        .file_name()
        .is_none_or(|name| name != "spyc")
        && exe != Some(program.as_str())
    {
        return false;
    }
    // SPYC-TRAP(codex-hook-ownership-is-a-command): a quoted flag grants no permission to prune.
    CODEX_STATUS_HOOKS.iter().any(|(_, state)| {
        command == reporter_command(program, state)
            // Older spycs' forms: no fail-soft tail, or a baked `--status-trace`.
            || ["", " --status-trace"].into_iter().any(|trace| {
                let bare = format!(
                    "{} --report-status {state}{trace}",
                    crate::shell::shell_quote(program)
                );
                command == bare || command == format!("{bare} 2>/dev/null || true")
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_every_generated_status_with_quoting_trace_and_legacy_forms() {
        for exe in [
            "spyc",
            "/opt/bin/spyc",
            "/Users/My User's/bin/spyc",
            "/opt/custom/reporter",
        ] {
            for (_, state) in CODEX_STATUS_HOOKS {
                let command = reporter_command(exe, state);
                assert!(is_owned(&command, Some(exe)), "{command}");
                if Path::new(exe).file_name().unwrap() == "spyc" {
                    assert!(is_owned(&command, None), "moved reporter: {command}");
                }
                for trace in ["", " --status-trace"] {
                    let bare = format!(
                        "{} --report-status {state}{trace}",
                        crate::shell::shell_quote(exe)
                    );
                    assert!(is_owned(&bare, Some(exe)), "{bare}");
                    let tailed = format!("{bare} 2>/dev/null || true");
                    assert!(is_owned(&tailed, Some(exe)), "{tailed}");
                }
            }
        }
    }

    #[test]
    fn preserves_flag_mentions_shell_actions_unknown_states_and_other_programs() {
        for command in [
            "printf '%s' '--report-status'",
            "echo 'spyc --report-status done'",
            "other-tool --report-status done",
            "/opt/other-tool --report-status done",
            "spyc --report-status done; printf user-hook",
            "spyc --report-status done && true",
            "spyc --report-status done-custom",
            "spyc --report-status done --custom",
            "env spyc --report-status done",
            "$(echo spyc) --report-status done",
            "spyc-wrapper --report-status done",
            "spyc --report-status",
            "'spyc --report-status done",
            "spyc --report-status done 2>/dev/null || true; printf user-hook",
        ] {
            assert!(!is_owned(command, Some("spyc")), "{command}");
        }
        assert!(!is_owned("/opt/custom/reporter --report-status done", None));
    }
}
