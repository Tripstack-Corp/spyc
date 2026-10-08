# Codex approval fixture provenance

Fixtures describe Codex CLI `0.160.1`. Keep captured UI and upstream snapshots
separate from invented payloads. Modal text is extracted without spyc chrome,
colour escapes or private prompt details.

- `codex-approval-exec.txt`: native command approval captured during A3b5.
- `codex-approval-edit.txt`: native file-edit approval captured during A3b6.
- `codex-approval-exec-narrow.txt`: native command approval launched at 40 columns
  on 2026-10-08; includes the CLI's actual header, option and footer wrapping.
- `codex-approval-mcp.txt`: native single-field MCP tool approval at 200 columns
  on 2026-10-08, using a disposable fixed-marker tool and per-launch prompt policy.
- `codex-approval-mcp-narrow.txt`: the same MCP modal after resizing to 40 columns.
  Its server heading is clipped; the required field counter, choices and footer
  remain visible.
- `codex-approval-mcp-session.txt`: extracted from the upstream
  [session-persistence UI snapshot](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/tui/src/bottom_pane/snapshots/codex_tui__bottom_pane__mcp_server_elicitation__tests__mcp_server_elicitation_approval_form_with_session_persist.snap).
  Session/always choices were not selected in native captures.
- `codex-approval-network.txt`: extracted from the upstream
  [network approval UI snapshot](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/tui/src/bottom_pane/snapshots/codex_tui__bottom_pane__approval_overlay__tests__network_exec_prompt.snap).
  This is an upstream fixture, not a successful live network-dialogue capture.

Native captures and step logs are retained under `/private/tmp/spyc-a3b6-native-*`.
Startup update and hook-review prompts were skipped with Esc in disposable UI
sessions. No hook trust or persistent approval was granted. These captures
establish UI shape; semantic hook delivery needs a separate trusted-hook test.
