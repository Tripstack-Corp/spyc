# Recorded Codex command execution fixture

Captured from a local Codex CLI 0.160.1 rollout on 2026-10-06. The command is
`codex --version`, invoked through code-mode `exec`; the completed execution
record supplies the actual argv without parsing the JavaScript call. In the
inspected snapshot on 2026-10-08, all 1570 completed `CommandExecution` items
used arrays, while the reader accepted only strings.

The recorded argv, output, status, timing, timestamp, ordinal and item shape
are retained. Thread/turn/item/process identifiers and the working directory
are replaced with fixture values. The version and alias warning are historical
recorded output, not a requirement for an installed CLI. No user prompts,
reasoning, credentials or hook arguments are included.

The public transcript regression and controlled `Ctrl-A v` TUI replay both
consume this fixture. Additional malformed, quoting, duplicate and hidden-tool
cases are generated as negative or compatibility variants of the recorded shape.
