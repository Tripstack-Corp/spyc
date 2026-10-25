# agy viewport fixtures

Screens of the Antigravity CLI (`agy`) as spyc's terminal engine holds them
(`Pane::visible_lines()`): one row per line, trailing blank rows kept, so a
row's distance from the bottom matches the live viewport.

| Fixture | Source | `agy` | Pane (cols × rows) | Screen |
|---|---|---|---|---|
| `approval-command.txt` | live | 1.4.7 | 100 × 28 | `Command` approval for `uname -a && sleep 8` |
| `approval-command-narrow.txt` | live | 1.4.7 | 70 × 28 | `Command` approval whose options wrap to column 0 |
| `approval-edit.txt` | live | 1.4.7 | 100 × 28 | `Pending edit` approval, default mode |
| `approval-question.txt` | live | 1.4.7 | 100 × 28 | `ask_question` with three choices and a write-in |
| `composer-empty.txt` | live | 1.4.7 | 100 × 28 | idle, empty composer, after a turn |
| `composer-draft.txt` | live | 1.4.7 | 100 × 28 | idle, an unsent draft in the composer |
| `working.txt` | live | 1.4.7 | 100 × 28 | a running command, empty composer below it |
| `working-draft.txt` | live | 1.4.7 | 100 × 28 | a running command, a draft typed mid-turn |
| `approval-file.txt` | synthetic | 1.4.7 layout | 100 × 28 | `File` approval |

## Live screens

`agy_live_capture_viewport_fixtures` in `src/app/harness_tests/agy_live.rs`
captured them on 2026-10-24. It starts `agy` in a spyc pane, drives it through
each screen, and writes each one sanitized. Recapture with:

```sh
SPYC_AGY_CAPTURE_DIR=/tmp/agy-capture \
  cargo test --lib agy_live_capture -- --ignored --nocapture --test-threads=1
```

Then read the screens and copy them here. The capture ran with agy's vim
editor mode on, so the draft footers show `-- INSERT --`. Its user allows
`sleep` without asking, so the command screens use `uname`, which agy asks
about.

## Synthetic screens

`approval-file.txt` is hand-written. With `allowNonWorkspaceAccess` set, agy
reads outside the workspace without asking, so the capture never sees a `File`
dialog. It follows `approval-command.txt`'s layout with the `File` header,
`Allow access to this file?` and three options.
