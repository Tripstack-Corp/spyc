//! The variable assignments a shell command line opens with
//! (`FOO=1 BAR="a b" claude`): where they end, so a pane's `exec` goes after
//! them and the program is read past them.

/// `command` as its leading assignments and the rest, each trimmed:
/// `("FOO=1 BAR='a b'", "claude --x")`. A command with no assignments, or one
/// this scanner can't split safely (an open quote, an operator inside an
/// assignment word, a newline), is all rest: `("", command)`.
pub fn split_assignments(command: &str) -> (&str, &str) {
    let whole = ("", command.trim());
    let mut at = 0;
    loop {
        let start = at + (command[at..].len() - command[at..].trim_start().len());
        if start == command.len() {
            return (command[..at].trim(), "");
        }
        let Some(end) = word_end(command, start) else {
            return whole;
        };
        if !is_assignment(&command[start..end]) {
            return (command[..at].trim(), command[start..].trim());
        }
        at = end;
    }
}

/// `NAME=…` with a POSIX name: unquoted, so the shell takes it as an
/// assignment rather than a word.
fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    name.chars().next().is_some_and(|c| !c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Where the shell word starting at `start` ends: the next unquoted blank
/// outside `$(…)` and `${…}`. `None` when it can't tell (an unclosed quote
/// or expansion) or the word carries an unquoted operator or newline, which
/// would make the line more than one simple command.
fn word_end(s: &str, start: usize) -> Option<usize> {
    let mut chars = s[start..].char_indices().peekable();
    let mut depth = 0usize;
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next()?;
            }
            '\'' => {
                chars.find(|&(_, c)| c == '\'')?;
            }
            '"' | '`' => loop {
                match chars.next()?.1 {
                    '\\' => {
                        chars.next()?;
                    }
                    q if q == c => break,
                    _ => {}
                }
            },
            '$' if matches!(chars.peek(), Some((_, '(' | '{'))) => {
                chars.next();
                depth += 1;
            }
            '(' | '{' if depth > 0 => depth += 1,
            ')' | '}' if depth > 0 => depth -= 1,
            '\n' | ';' | '&' | '|' | '<' | '>' | '(' | ')' if depth == 0 => return None,
            c if c.is_whitespace() && depth == 0 => return Some(start + i),
            _ => {}
        }
    }
    (depth == 0).then_some(s.len())
}

#[cfg(test)]
mod tests {
    use super::split_assignments;

    #[test]
    fn leading_assignments_split_from_the_program() {
        assert_eq!(split_assignments("FOO=1 claude"), ("FOO=1", "claude"));
        assert_eq!(
            split_assignments("  A=1   B='x y' C=\"p q\" D=a\\ b  prog --flag  "),
            ("A=1   B='x y' C=\"p q\" D=a\\ b", "prog --flag")
        );
        assert_eq!(
            split_assignments("P=$(echo a b) Q=`echo c d` R=${X:-e f} prog"),
            ("P=$(echo a b) Q=`echo c d` R=${X:-e f}", "prog")
        );
    }

    #[test]
    fn a_command_without_assignments_is_all_program() {
        for cmd in [
            "claude",
            "env FOO=1 claude",
            "a/b=c prog",
            "1A=2 prog",
            "\"A\"=1 prog",
            "",
        ] {
            assert_eq!(split_assignments(cmd), ("", cmd.trim()), "{cmd}");
        }
    }

    #[test]
    fn only_assignments_leave_no_program() {
        assert_eq!(split_assignments("FOO=1 BAR=2 "), ("FOO=1 BAR=2", ""));
    }

    /// A line the scanner can't read as one simple command keeps its shape.
    #[test]
    fn an_unsplittable_line_is_left_whole() {
        for cmd in [
            "FOO='open claude",
            "FOO=$(open claude",
            "FOO=1;claude",
            "FOO=1&&claude",
            "FOO=1>x claude",
            "FOO=1\nclaude",
            "claude;ls",
        ] {
            assert_eq!(split_assignments(cmd), ("", cmd), "{cmd}");
        }
    }

    /// Only the opening words count: an operator after the program is the
    /// program's business.
    #[test]
    fn the_rest_is_kept_verbatim() {
        assert_eq!(
            split_assignments("FOO=1 claude && echo 'done; x'"),
            ("FOO=1", "claude && echo 'done; x'")
        );
    }
}
