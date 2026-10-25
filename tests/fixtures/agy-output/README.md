# Agy viewport fixtures

Captured from the Antigravity CLI (`agy`) through spyc's own terminal engine
(`Pane::spawn_with_env` and `Pane::visible_lines()`), with personal account, GCP
project, and home-directory paths replaced by synthetic placeholders
(`user@example.com`, `example-project`, `/Users/example/src/spyc`). Blank
trailing rows are preserved where present so row offsets from the bottom match
the live viewport.

## Live PTY captures

- `approval-command.txt` (`agy` `1.4.5`, 132x38 viewport): live command-approval
  menu (`Command` header, `─` divider, `Requesting permission for:`, `Run this
  command?`, numbered options `1..4`, `↑/↓ Navigate` hint, and `esc to cancel`
  status footer). Rows 1–14 and the paths on rows 20 and 23 are sanitized
  placeholders preserving the original 38-row height.
- `approval-command-narrow.txt` (`agy` `1.4.7`, 70x28 pty): live command-approval
  menu in a 70-column pane where options 2 and 3 wrap onto continuation rows at
  column 0 (`start with 'gh'`, `to settings.json)`).
- `composer-empty.txt` (`agy` `1.4.7`, 100x28 pty): idle screen after a turn
  with an empty `>` composer between two 100-column `─` box rules and the
  `? for shortcuts` / `accept-edits · Gemini 4 Argon · high` footer below.
- `composer-draft.txt` (`agy` `1.4.7`, 100x28 pty): idle screen with an
  unsubmitted draft (`> check src/unsubmitted.rs`) in the composer box and
  `-- INSERT --` in the footer.
- `approval-edit.txt` (`agy` `1.4.7`, 100x28 pty): live file-edit approval menu
  (`Pending edit` header, 100-column `─` divider, diff preview,
  `shift+tab to auto-approve file edits`, `Accept this file edit?`, numbered
  options `1..2`, `↑/↓ Navigate` hint, and `esc to cancel` footer).
- `approval-question.txt` (`agy` `1.4.7`, 100x28 pty): live `ask_question` menu
  (`Question` header, 100-column `─` divider, `Question 1/1:` prompt, numbered
  choices `1..3`, `↑/↓ Navigate` hint, and `esc to cancel` footer).

## Synthetic fixtures

- `approval-file.txt` (synthetic, 100x28): models `agy` `1.4.7`'s `File`
  permission dialog (`File` header, 100-column `─` divider,
  `Requesting permission for:`, `Allow access to this file?`, numbered options
  `1..3`, `↑/↓ Navigate` hint, and `esc to cancel` footer) using the banner and
  footer geometry of the 100x28 live `1.4.7` captures.
- `working.txt` (synthetic, 100x28): models an active `agy` `1.4.7` turn with
  no composer box on screen, two past user turns separated by single 60-column
  `─` divider rules (`> > out.txt summary` and `> Update src/printed.rs`), a
  live braille spinner (`⣟  Running command...`), and the `esc to cancel`
  working footer.

## Recapturing against a newer `agy` release

Run the ignored capture harness in `src/app/harness_tests/agent_approval.rs`
(`cargo test --lib agy_capture_live_viewport_fixtures -- --ignored --nocapture`),
which:

1. Spawns `agy --prompt-interactive "<prompt>"` in a 100x28 (or 70x28 narrow)
   pty via `Pane::spawn_with_env` with `TERM=xterm-256color`.
2. Accepts the workspace trust prompt (`Do you trust the files in this folder?`)
   with `\r` if `agy` shows it for the temporary workspace before starting the
   turn.
3. Waits for each target state (idle empty composer, draft typed into composer,
   command approval, file-edit approval, `ask_question`), dumps
   `pane.visible_lines().join("\n")`, and sanitizes personal account/path
   identifiers before writing `tests/fixtures/agy-output/*.txt`.
