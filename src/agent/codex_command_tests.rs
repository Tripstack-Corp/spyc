use super::*;

const SESSION: &str = "019e8b21-9e7c-7553-a118-d1cdada725fd";

#[test]
fn baselines_keep_options_on_either_side_of_the_selector() {
    for (command, expected) in [
        ("codex", "codex"),
        ("codex resume OLD --model test", "codex --model test"),
        (
            "codex --model test fork OLD --sandbox read-only",
            "codex --model test --sandbox read-only",
        ),
        (
            "/opt/bin/codex resume --last --all --include-non-interactive --profile team",
            "/opt/bin/codex --profile team",
        ),
        (
            "codex fork --last --no-daemon --no-alt-screen",
            "codex --no-daemon --no-alt-screen",
        ),
        (
            r#"codex -c 'model="test model"' resume 'old name' -a never"#,
            r#"codex -c 'model="test model"' -a never"#,
        ),
        (
            r#"codex resume OLD --add-dir "$HOME/shared dir" -C ~/src"#,
            r#"codex --add-dir "$HOME/shared dir" -C ~/src"#,
        ),
        (
            r#"codex -cmodel="test" resume OLD --model=new --sandbox=read-only -pteam"#,
            r#"codex -cmodel="test" --model=new --sandbox=read-only -pteam"#,
        ),
        (
            r#"codex resume OLD -c "literal='resume'" -c 'tools=["a", "b"]'"#,
            r#"codex -c "literal='resume'" -c 'tools=["a", "b"]'"#,
        ),
        (
            "codex -c resume --model fork --profile 'resume'",
            "codex -c resume --model fork --profile 'resume'",
        ),
        ("codex resume -- OLD", "codex"),
        (
            r"codex resume OLD --add-dir shared\ dir",
            r"codex --add-dir shared\ dir",
        ),
        (
            r#"codex resume OLD -c 'note="$(id); # literal"'"#,
            r#"codex -c 'note="$(id); # literal"'"#,
        ),
    ] {
        assert_eq!(baseline(command).unwrap(), expected, "{command}");
        assert_eq!(
            baseline(expected).unwrap(),
            expected,
            "baseline is idempotent"
        );
        assert_eq!(
            restore(command, Some(SESSION)).unwrap(),
            format!("{expected} resume {SESSION}")
        );
        assert_eq!(
            fork(command, SESSION).unwrap(),
            format!("{expected} fork {SESSION}")
        );
    }
}

#[test]
fn unsupported_commands_are_refused_and_saved_verbatim() {
    for command in [
        "",
        "env CODEX_HOME=/tmp codex",
        "codex; echo hi",
        "codex | cat",
        "codex && echo hi",
        "codex > output",
        "codex # comment",
        "codex\nresume OLD",
        "codex -c 'unterminated",
        "codex --model test\\",
        "codex -c $(id)",
        "codex -c \"$(id)\"",
        "codex -c `id`",
        "codex --future-option resume",
        "codex --model",
        "codex --model --sandbox read-only",
        "codex --last",
        "codex --no-daemon=true",
        "codex exec 'do work'",
        "codex 'initial prompt'",
        "codex resume OLD 'initial prompt'",
        "codex fork OLD -i image.png",
        "codex resume -- OLD --search",
        "codex --model before resume OLD -m after",
        "codex -h",
        "codex --version",
        "codex -é",
        "codex\0 resume OLD",
    ] {
        assert!(restore(command, Some(SESSION)).is_err(), "{command:?}");
        assert!(fork(command, SESSION).is_err(), "{command:?}");
        assert_eq!(
            crate::agent::resume::command_without_codex_resume(command),
            command
        );
    }
}

#[test]
fn selectors_never_introduce_shell_syntax_or_switches() {
    let id = "a name'; echo injected; #";
    let command = restore("codex", Some(id)).unwrap();
    assert_eq!(shlex::split(&command).unwrap(), ["codex", "resume", id]);
    for invalid in ["", "--last", "id\0suffix"] {
        assert!(restore("codex", Some(invalid)).is_err());
        assert!(fork("codex", invalid).is_err());
    }
    assert_eq!(
        restore("codex --model test", None).unwrap(),
        "codex --model test resume --last"
    );
}

#[test]
fn resume_identity_ignores_option_values_and_fork_parents() {
    for command in [
        format!("codex resume --model test '{SESSION}' --unknown-future-flag"),
        format!("codex -c resume --model fork resume --profile team {SESSION}"),
        format!("codex resume -- {SESSION}"),
    ] {
        assert_eq!(
            crate::state::codex_transcript::resume_uuid_from_command(&command).as_deref(),
            Some(SESSION)
        );
        assert!(!crate::state::codex_transcript::is_resume_without_id(
            &command
        ));
    }
    for command in [
        "codex --model resume",
        "codex -c resume",
        "codex fork OLD",
        "codex --future-option resume",
        "codex 'please resume'",
    ] {
        assert_eq!(resume_selector(command), None, "{command}");
    }
    assert_eq!(
        resume_selector("codex resume --profile team --last"),
        Some(ResumeSelection { selector: None })
    );
    assert_eq!(
        resume_selector("codex resume 'named session'"),
        Some(ResumeSelection {
            selector: Some("named session".into())
        })
    );
}

#[test]
fn raw_quoting_and_expansions_survive_a_real_shell() {
    let exe = "/tmp/codex";
    let original = format!(
        r#"{exe} resume OLD -c 'model="a b"' --add-dir "$HOME/shared dir" --profile team\ name"#
    );
    let rewritten = fork(&original, SESSION).unwrap();
    let args = rewritten.strip_prefix(exe).unwrap();
    let output = std::process::Command::new("sh")
        .args(["-c", &format!("printf '%s\\n' {args}")])
        .env("HOME", "/test home")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        [
            "-c",
            "model=\"a b\"",
            "--add-dir",
            "/test home/shared dir",
            "--profile",
            "team name",
            "fork",
            SESSION
        ]
    );
}

proptest::proptest! {
    #[test]
    fn quoted_option_values_remain_one_identical_argument(value in "[a-zA-Z0-9 '\"é;#$()\\\\]{0,48}") {
        let quoted = crate::shell::shell_quote(&value);
        let command = format!("codex resume OLD --profile {quoted}");
        let rebuilt = fork(&command, SESSION).unwrap();
        let parsed = shlex::split(&rebuilt).unwrap();
        proptest::prop_assert_eq!(parsed, vec!["codex".to_string(), "--profile".to_string(), value, "fork".to_string(), SESSION.to_string()]);
    }
}
