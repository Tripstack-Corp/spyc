# Codex async-question fixtures

These small text fixtures are the rendered body of snapshots from the upstream
`rust-v0.162.1` tag. Their prompts are upstream test data, not conversation
captures. Snapshot metadata headings are omitted; `collapsed.txt` keeps the
`ONLY QUESTIONS` section. No real account, home, session or answer is recorded.

| Fixture | Upstream snapshot |
|---|---|
| `expanded-options.txt` | [last question with queue](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/tui/src/chatwidget/tests/snapshots/codex_tui__chatwidget__tests__questions_tests__last_question_with_queue.snap) |
| `expanded-freeform.txt` | [single question while working](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/tui/src/chatwidget/tests/snapshots/codex_tui__chatwidget__tests__questions_tests__single_question_working_spacing.snap) |
| `collapsed.txt` | [questions with status and queue](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/tui/src/chatwidget/tests/snapshots/codex_tui__chatwidget__tests__questions_tests__questions_with_status_and_queue.snap) |
| `expanded-wrapped.txt` | [wrapped other option](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/tui/src/bottom_pane/async_questions/snapshots/codex_tui__bottom_pane__async_questions__tests__question_wrapped_other.snap) |
| `expanded-clipped.txt` | [clipped narrow view](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/tui/src/bottom_pane/async_questions/snapshots/codex_tui__bottom_pane__async_questions__tests__question_narrow_other.snap) |

[Summary rendering](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/tui/src/bottom_pane/questions.rs)
supplies the positive count, optional countdown and answer hint.
[Editor rendering](https://github.com/openai/codex/blob/rust-v0.162.1/codex-rs/tui/src/bottom_pane/async_questions/render.rs)
supplies the ordered submit/skip/option/navigation footer.

Countdown, invalid-count, quoted-history and customized-key variants in the Rust
tests are explicitly synthetic derivatives. The scanner accepts only the current
complete default-keymap view. A cancelled or cleared native view is not proof of
an answered question, and no tool-post completion is used as answer evidence.
