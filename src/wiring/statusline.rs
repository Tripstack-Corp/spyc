//! claude's `statusLine`, wrapped so its input reaches spyc. Claude Code hands
//! the plan's rate limits and the context window only to the statusline
//! command, so spyc runs first, forwards them to its own pane, then runs the
//! command the user had, on the same input; its output is unchanged.
//!
//! The wrapper carries the user's command inside it rather than in a file of
//! spyc's: removing spyc restores it exactly. A `settings.json` synced to a
//! machine without spyc still runs it, through the `command -v` guard, and one
//! whose spyc predates `--statusline` runs it through the `||` fallback: such a
//! spyc fails before it reads its input, and `spyc --statusline` exits 0 once it
//! has run the command, so the fallback never runs it twice.

use serde_json::{Value, json};

use crate::mcp::Removal;

/// What spyc's own wrapper runs: `--statusline`, the original as one argument.
const FLAG: &str = "--statusline";

// SPYC-TRAP(statusline-wrapper-format): `unwrap` must keep recognizing every
// format `wrap` ever wrote. A wrapper it can't recognize reads as the user's
// command, so the next install wraps it again, one layer per launch.

/// The `statusLine.command` running `spyc` (a name on `$PATH` or a path) before
/// `original`, the command the user had, if any.
pub(super) fn wrap(original: Option<&str>, spyc: &str) -> String {
    let exe = quote(spyc);
    let guard = format!("command -v {exe} >/dev/null 2>&1");
    match original {
        Some(original) => format!(
            "o={}; {guard} && {exe} {FLAG} -- \"$o\" || exec sh -c \"$o\"",
            quote(original)
        ),
        None => format!("{guard} && exec {exe} {FLAG}"),
    }
}

/// What one of spyc's wrappers wraps.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Wrapped {
    /// There was no statusline; spyc's is the only one.
    Nothing,
    /// The user's own command.
    Command(String),
}

impl Wrapped {
    fn command(&self) -> Option<&str> {
        match self {
            Self::Nothing => None,
            Self::Command(command) => Some(command),
        }
    }
}

/// What spyc's wrapper wraps, or `None` when `command` isn't one.
pub(super) fn unwrap(command: &str) -> Option<Wrapped> {
    let words = shlex::split(command)?;
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    match words.as_slice() {
        [
            "command",
            "-v",
            exe,
            ">/dev/null",
            "2>&1",
            "&&",
            "exec",
            again,
            FLAG,
        ] if exe == again => Some(Wrapped::Nothing),
        [
            assignment,
            "command",
            "-v",
            exe,
            ">/dev/null",
            "2>&1",
            "&&",
            again,
            FLAG,
            "--",
            "$o",
            "||",
            "exec",
            "sh",
            "-c",
            "$o",
        ] if exe == again => {
            let original = assignment.strip_prefix("o=")?.strip_suffix(';')?;
            // A command that only looks like ours must reproduce exactly.
            (wrap(Some(original), exe) == command).then(|| Wrapped::Command(original.to_owned()))
        }
        _ => None,
    }
}

/// `text`, a claude `settings.json`, with its statusline wrapped. A statusline
/// spyc can't wrap (not a `command`) is left as it is; one already wrapped is
/// rewrapped around the same command, so this is idempotent.
pub(super) fn settings_with(text: &str, spyc: &str) -> Option<String> {
    let mut root: Value = serde_json::from_str(text).ok()?;
    let obj = root.as_object_mut()?;
    match obj.get_mut("statusLine") {
        None => {
            obj.insert(
                "statusLine".into(),
                json!({ "type": "command", "command": wrap(None, spyc) }),
            );
        }
        Some(line) => {
            // A NUL can't be quoted for a shell, so such a command is left alone.
            if let Some(command) = wrappable(line).filter(|c| !c.contains('\0')) {
                let original = unwrap(&command).unwrap_or(Wrapped::Command(command));
                line["command"] = Value::String(wrap(original.command(), spyc));
            }
        }
    }
    serde_json::to_string_pretty(&root)
        .ok()
        .map(|out| out + "\n")
}

/// `text` without spyc's wrapper: the user's command back in its place, or no
/// `statusLine` at all when spyc's wrapped nothing. `None` when `text` isn't a
/// JSON object.
pub(super) fn settings_without(text: &str) -> Option<Removal> {
    let mut root: Value = serde_json::from_str(text).ok()?;
    let obj = root.as_object_mut()?;
    let Some(original) = obj
        .get("statusLine")
        .and_then(wrappable)
        .and_then(|command| unwrap(&command))
    else {
        return Some(Removal::NothingOurs);
    };
    match original {
        Wrapped::Command(original) => obj["statusLine"]["command"] = Value::String(original),
        Wrapped::Nothing => {
            obj.shift_remove("statusLine");
        }
    }
    let empty = obj.is_empty();
    let text = serde_json::to_string_pretty(&root).ok()? + "\n";
    Some(Removal::Rest { text, empty })
}

/// A statusline's command, when it is one spyc can wrap.
fn wrappable(line: &Value) -> Option<String> {
    let typed = line.get("type").is_none_or(|t| t == "command");
    typed
        .then(|| line.get("command")?.as_str().map(str::to_owned))
        .flatten()
}

fn quote(word: &str) -> String {
    shlex::try_quote(word).map_or_else(
        // A NUL can't reach a shell at all; quoting it would change the command.
        |_| "''".to_owned(),
        std::borrow::Cow::into_owned,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// claude-hud's own `statusLine.command`, quotes within quotes.
    const HUD: &str = r#"bash -c 'cols=$(stty size </dev/tty 2>/dev/null | awk '"'"'{print $2}'"'"'); export COLUMNS=$(( ${cols:-120} > 4 ? ${cols:-120} - 4 : 1 )); exec "/opt/homebrew/bin/node" "${plugin_dir}dist/index.js"'"#;

    #[test]
    fn a_wrapped_command_comes_back_exactly() {
        for original in [
            HUD,
            "echo 'a b'",
            "printf \"%s\\n\" \"$HOME\"",
            "x;\ny",
            "o=1;",
        ] {
            for spyc in ["spyc", "/Users/me/.local/bin/spyc", "/path with space/spyc"] {
                let wrapped = wrap(Some(original), spyc);
                assert_eq!(
                    unwrap(&wrapped),
                    Some(Wrapped::Command(original.to_owned())),
                    "{wrapped}"
                );
            }
        }
        assert_eq!(unwrap(&wrap(None, "spyc")), Some(Wrapped::Nothing));
        for foreign in [
            HUD,
            "spyc --statusline",
            "",
            "o=x; echo",
            // Its words, but not as spyc writes them: not spyc's to take back.
            r#"o="x"; command -v spyc >/dev/null 2>&1 && exec spyc --statusline -- "$o" || exec sh -c "$o""#,
        ] {
            assert_eq!(unwrap(foreign), None, "{foreign:?} isn't spyc's");
        }
    }

    #[test]
    fn settings_gain_the_wrapper_once_and_lose_it_exactly() {
        let before = serde_json::to_string_pretty(&json!({
            "model": "opus",
            "statusLine": { "type": "command", "command": HUD, "padding": 0 },
        }))
        .unwrap()
            + "\n";
        let with = settings_with(&before, "spyc").unwrap();
        assert_eq!(settings_with(&with, "spyc").unwrap(), with, "idempotent");
        let moved = settings_with(&with, "/new/spyc").unwrap();
        let parsed: Value = serde_json::from_str(&moved).unwrap();
        assert_eq!(
            unwrap(parsed["statusLine"]["command"].as_str().unwrap()),
            Some(Wrapped::Command(HUD.to_owned())),
            "a new spyc path wraps the same command, never the old wrapper"
        );
        assert_eq!(parsed["statusLine"]["padding"], 0, "other fields stay");
        let Some(Removal::Rest { text, empty }) = settings_without(&with) else {
            panic!("spyc's wrapper is spyc's to remove");
        };
        assert_eq!(text, before);
        assert!(!empty);
        assert_eq!(settings_without(&before), Some(Removal::NothingOurs));
    }

    #[test]
    fn with_no_statusline_spyc_adds_one_and_removing_it_leaves_none() {
        let with = settings_with("{}", "spyc").unwrap();
        let Some(Removal::Rest { text, empty }) = settings_without(&with) else {
            panic!("spyc added it");
        };
        assert_eq!(text.trim(), "{}");
        assert!(empty);
        let other = r#"{"statusLine": {"type": "static", "text": "hi"}}"#;
        let kept: Value = serde_json::from_str(&settings_with(other, "spyc").unwrap()).unwrap();
        assert_eq!(
            kept["statusLine"]["text"], "hi",
            "a kind spyc can't wrap is left"
        );
        assert_eq!(settings_with("[]", "spyc"), None);
    }

    /// The wrapper as a shell runs it: with spyc, spyc gets the original as one
    /// argument and the input; without spyc, the original runs on the input.
    #[test]
    fn the_wrapper_runs_the_original_with_or_without_spyc() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake-spyc");
        std::fs::write(
            &fake,
            "#!/bin/sh\nprintf 'spyc[%s][%s][%s]' \"$1\" \"$2\" \"$3\"\nexec sh -c \"$3\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let original = "printf 'out:'; cat";
        let run = |command: &str| {
            let mut child = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(command)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(b"{\"x\":1}").unwrap();
            String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
        };
        let present = run(&wrap(Some(original), fake.to_str().unwrap()));
        assert_eq!(
            present,
            format!("spyc[--statusline][--][{original}]out:{{\"x\":1}}")
        );
        let absent = run(&wrap(Some(original), "/no/such/spyc"));
        assert_eq!(absent, "out:{\"x\":1}");
        // A spyc from before `--statusline` fails before reading its input.
        let old = dir.path().join("old-spyc");
        std::fs::write(&old, "#!/bin/sh\necho 'unexpected argument' >&2\nexit 2\n").unwrap();
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            run(&wrap(Some(original), old.to_str().unwrap())),
            "out:{\"x\":1}"
        );
        assert_eq!(run(&wrap(None, "/no/such/spyc")), "");
    }
}
