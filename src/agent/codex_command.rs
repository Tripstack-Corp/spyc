//! Token-aware reconstruction of interactive Codex commands. Raw shell words
//! preserve quoting and expansions; unsupported syntax is never reinterpreted.

use anyhow::{Result, bail};

#[derive(Debug)]
struct Word<'a> {
    raw: &'a str,
    value: String,
}

#[derive(Debug)]
struct Invocation {
    base: String,
    operation: Option<String>,
    selector: Option<String>,
}

fn words(command: &str) -> Result<Vec<Word<'_>>> {
    let mut start = None;
    let mut quote = None;
    let mut escaped = false;
    let mut spans = Vec::new();
    let mut chars = command.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        if matches!(character, '\0' | '\n' | '\r') {
            bail!("multiline commands and NUL bytes cannot be reconstructed");
        }
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('\'') {
            if character == '\'' {
                quote = None;
            }
            continue;
        }
        if character == '\\' {
            start.get_or_insert(index);
            escaped = true;
            continue;
        }
        if character == '`'
            || (character == '$' && chars.peek().is_some_and(|(_, next)| *next == '('))
        {
            bail!("shell command substitutions cannot be reconstructed");
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            }
        } else if matches!(character, '\'' | '"') {
            start.get_or_insert(index);
            quote = Some(character);
        } else if character.is_ascii_whitespace() {
            if let Some(begin) = start.take() {
                spans.push(&command[begin..index]);
            }
        } else {
            if matches!(character, ';' | '|' | '&' | '<' | '>' | '(' | ')' | '#') {
                bail!("shell operators and comments cannot be reconstructed");
            }
            start.get_or_insert(index);
        }
    }
    if escaped || quote.is_some() {
        bail!("incomplete shell quoting cannot be reconstructed");
    }
    if let Some(begin) = start {
        spans.push(&command[begin..]);
    }
    spans
        .into_iter()
        .map(|raw| {
            let parsed = shlex::split(raw).ok_or_else(|| anyhow::anyhow!("invalid shell word"))?;
            let [value] = parsed.as_slice() else {
                bail!("ambiguous shell word");
            };
            Ok(Word {
                raw,
                value: value.clone(),
            })
        })
        .collect()
}

fn takes_value(option: &str) -> Option<bool> {
    match option {
        "-c"
        | "--config"
        | "--enable"
        | "--disable"
        | "--remote"
        | "--remote-auth-token-env"
        | "-i"
        | "--image"
        | "-m"
        | "--model"
        | "--local-provider"
        | "-p"
        | "--profile"
        | "-s"
        | "--sandbox"
        | "-C"
        | "--cd"
        | "--add-dir"
        | "-a"
        | "--ask-for-approval" => Some(true),
        "--strict-config"
        | "--oss"
        | "--approve-for-me"
        | "--full-auto"
        | "--dangerously-bypass-approvals-and-sandbox"
        | "--yolo"
        | "--dangerously-bypass-hook-trust"
        | "--worktree"
        | "--search"
        | "--no-alt-screen"
        | "--no-daemon" => Some(false),
        _ => None,
    }
}

fn option_parts(value: &str) -> (&str, bool) {
    if value.starts_with("--") {
        value
            .split_once('=')
            .map_or((value, false), |(name, _)| (name, true))
    } else if value.len() > 2 && value.starts_with('-') {
        value.get(..2).map_or((value, false), |name| (name, true))
    } else {
        (value, false)
    }
}

fn parse(command: &str, strict: bool) -> Result<Invocation> {
    let tokens = words(command)?;
    let Some(executable) = tokens.first() else {
        bail!("empty Codex command");
    };
    if executable.value.rsplit('/').next() != Some("codex") || executable.raw != executable.value {
        bail!("expected a bare or path-qualified codex executable");
    }
    let mut retained = vec![executable.raw];
    let mut operation = None;
    let mut selector = None;
    let mut options = std::collections::HashSet::new();
    let mut index = 1;
    while let Some(word) = tokens.get(index) {
        let value = word.value.as_str();
        if operation.is_none() && matches!(value, "resume" | "fork") {
            operation = Some(value.to_string());
        } else if matches!(value, "--last" | "--all" | "--include-non-interactive") {
            if operation.is_none() {
                bail!("selector switch {value} requires resume or fork");
            }
        } else if value.starts_with('-') && value != "--" {
            let (name, attached) = option_parts(value);
            let Some(needs_value) = takes_value(name) else {
                bail!("unsupported Codex option {name}; reopen this tab manually");
            };
            let canonical = match name {
                "-m" => "--model",
                "-p" => "--profile",
                "-s" => "--sandbox",
                "-C" => "--cd",
                "-a" => "--ask-for-approval",
                "--yolo" => "--dangerously-bypass-approvals-and-sandbox",
                other => other,
            };
            if strict
                && !matches!(
                    name,
                    "-c" | "--config" | "--enable" | "--disable" | "--add-dir"
                )
                && !options.insert(canonical)
            {
                bail!(
                    "repeated option {canonical} cannot be flattened safely; reopen this tab manually"
                );
            }
            if attached && !needs_value {
                bail!("unexpected value for {name}");
            }
            if strict && matches!(name, "-i" | "--image") {
                bail!(
                    "initial image inputs cannot be replayed automatically; reopen this tab manually"
                );
            }
            retained.push(word.raw);
            if needs_value && !attached {
                index += 1;
                let Some(argument) = tokens.get(index) else {
                    bail!("missing value for {name}");
                };
                if argument.value.starts_with('-') {
                    bail!("ambiguous value for {name}; use {name}=VALUE");
                }
                retained.push(argument.raw);
            }
        } else {
            if operation.is_none() {
                bail!(
                    "initial prompts and non-interactive subcommands cannot be replayed automatically; reopen this tab manually"
                );
            }
            if selector.is_some() {
                bail!("initial prompts cannot be replayed automatically; reopen this tab manually");
            }
            let argument = if value == "--" {
                index += 1;
                if strict && tokens.len() > index + 1 {
                    bail!(
                        "initial prompts after -- cannot be replayed automatically; reopen this tab manually"
                    );
                }
                tokens
                    .get(index)
                    .ok_or_else(|| anyhow::anyhow!("missing selector after --"))?
            } else {
                word
            };
            selector = Some(argument.value.clone());
            if !strict {
                break;
            }
        }
        index += 1;
    }
    Ok(Invocation {
        base: retained.join(" "),
        operation,
        selector,
    })
}

/// A saved baseline retains every general option, including its raw quoting.
pub fn baseline(command: &str) -> Result<String> {
    Ok(parse(command, true)?.base)
}

fn rebuild(command: &str, operation: &str, session_id: Option<&str>) -> Result<String> {
    let base = baseline(command)?;
    let selector = match session_id {
        Some(id) if !id.is_empty() && !id.starts_with('-') && !id.contains('\0') => {
            crate::shell::shell_quote(id)
        }
        Some(_) => bail!("invalid conversation selector"),
        None => "--last".to_string(),
    };
    Ok(format!("{base} {operation} {selector}"))
}

/// Resume the captured conversation, or use the last-session fallback.
pub fn restore(command: &str, session_id: Option<&str>) -> Result<String> {
    rebuild(command, "resume", session_id)
}

/// Fork the active conversation without changing its launch settings.
pub fn fork(command: &str, session_id: &str) -> Result<String> {
    rebuild(command, "fork", Some(session_id))
}

/// A resume operation, with an optional explicit conversation selector.
#[derive(Debug, PartialEq, Eq)]
pub struct ResumeSelection {
    pub selector: Option<String>,
}

/// Fork's selector names the parent, never the new pane's conversation.
pub fn resume_selector(command: &str) -> Option<ResumeSelection> {
    let parsed = parse(command, false).ok()?;
    (parsed.operation.as_deref() == Some("resume")).then_some(ResumeSelection {
        selector: parsed.selector,
    })
}

#[cfg(test)]
#[path = "codex_command_tests.rs"]
mod tests;
