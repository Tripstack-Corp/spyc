# Codex hook metadata replay fixture

These are normalized metadata received through the real reporter during native
Codex CLI 0.160.1 tests on 2026-10-07, host build eff0bce1. They are not raw hook
payloads: command arguments, question text and responses were never traced.

Question start and completion came from the same native Plan-mode
`request_user_input` call, in the dedicated `spyc-codex-a3b5-final-question`
session (10:40:08 and 10:40:15 UTC activity dumps). Permission metadata came
from `spyc-codex-a3b5-final-mixed` (10:38:52 UTC); that event has no call id.
Session, turn and call identifiers preserve their observed shape and equality.

`codex-hook-payload-agent.py` combines this observed metadata with synthetic
256 KiB content placed before it to reproduce the former 8 KiB stdin cutoff.
Its executable is deliberately named `codex` for profile routing but is a
controlled PTY child, not the native CLI. The replay runs the actual reporter
binary and host, including dispatch, pending-question correlation and activity
rendering. It does not establish native hook execution, hook trust or any new
Codex payload schema. The wrong-call completion is a deliberate negative case.
