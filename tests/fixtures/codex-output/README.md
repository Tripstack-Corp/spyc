# Codex output-boundary fixtures

These text grids are extracted from OpenAI Codex's renderer snapshots at
`rust-v0.162.1`, matching the installed CLI. JSON-quoted rows are decoded;
snapshot metadata and leading blank separators are removed. Text and remaining
spacing and row order are preserved.
They are upstream renderer captures, not successful local native-CLI captures.
Synthetic printed paths and draft variations in tests are labelled separately.

Sources:
- `draft.txt`: `codex_tui__bottom_pane__chat_composer__tests__footer_mode_hidden_while_typing.snap`
- `vim.txt`: `codex_tui__bottom_pane__chat_composer__tests__vim_fresh_draft.snap`
- `statusline.txt`: `codex_tui__chatwidget__tests__status_line_model_with_reasoning_context_remaining_footer.snap`
- `shortcuts.txt`: `codex_tui__bottom_pane__chat_composer__tests__footer_mode_shortcut_overlay.snap`
- `approval-exec.txt`: `codex_tui__chatwidget__tests__approval_modal_exec.snap`
- `approval-edit.txt`: `codex_tui__chatwidget__tests__approval_modal_patch.snap`
- `approval-mcp.txt`: `codex_tui__bottom_pane__mcp_server_elicitation__tests__mcp_server_elicitation_approval_form_with_param_summary.snap`

All sources are under the tagged
[TUI source tree](https://github.com/openai/codex/tree/rust-v0.162.1/codex-rs/tui/src).
The command/network narrow fixtures referenced by tests retain their separate
provenance in `../codex-approvals.md`.
