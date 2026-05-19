# bug-q-command-skips-session-save — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: bug-q-command-skips-session-save
Created: 2026-05-19T06:38:09.122471+00:00

---
Entry: Claude Code (caleb) 2026-05-19T06:38:09.122471+00:00
Role: implementer
Type: Note
Title: :q / :quit skip save_session and confirm — diverging from Q keybinding

Spec: implementer-code

## Symptom

caleb's `$XDG_STATE_HOME/spyc/sessions/` directory is empty after years of running spyc, and `spyc -r` produces nothing visible — the session picker flashes "no saved sessions" and returns. Triggered the investigation: "When I try to launch spyc with -r or --resume, it doesn't bring up the session chooser."

## Root cause

Two quit surfaces, two code paths:

- `Action::Quit` (bound to `Q` and `^D` via the keymap) at `src/app/mod.rs:9579-9603` runs the full lifecycle:
  1. First call arms `state.quit_pending` and flashes either `"press again to quit"` or `"N running processes — press again to quit"`.
  2. Second call inside 2s calls `self.save_session()` and sets `should_quit = true`.
- `:q` / `:quit` at `src/app/state.rs:1148-1152` (pure-domain `AppState::dispatch_command`) just:
  ```rust
  if input == "q" || input == "quit" {
      self.should_quit = true;
      return CommandResult::Handled;
  }
  ```

Pure-domain dispatch has no access to `save_session()` (which lives on `App` and needs `pane_tabs`, `background_tasks`), so it short-circuits — no persistence, no running-process warning, no confirm.

## Why this is a contract bug, not a design choice

`595447c` ("Reserve q for future macro recording") explicitly advertises `:q` as a real quit binding via the reserved-key flash at `state.rs:1120`:

> `"q reserved for future macro recording — Q or :q to quit"`

The flash tells users `:q` is equivalent to `Q`. There's no commit message, doc-comment, or thread anywhere that says `:q` is intended to be a "quick exit, skip persistence" variant. History trace:

- `b362bc4` "Session management: save on quit, restore with --resume, picker UI" — wired save_session into the only quit path at the time.
- `14989f6` "UX polish: double-press quit" — added the 2-second confirm, stated reason: *"prevents accidentally killing a long cspy + claude session."* Safety wrapper around the existing save path.
- `2b28a2a` "Quit warns about running pane processes before confirming" — added the running-process warning.
- `595447c` "Reserve q for future macro recording" — removed bare-`q` quit; advertised `:q` as the equivalent.

The `:q` arm in `state.rs` predates all of those and was never updated when the lifecycle grew. Pure code-seam oversight.

A secondary tell: `:q` also bypasses the running-process warning, which has no plausible design rationale.

## Fix

Extract the `Action::Quit` body into `App::request_quit(&mut self)` and route both the keymap action and a new `:q`/`:quit` arm in `App::dispatch_command` through it. Pure-domain dispatch returns `CommandResult::NotHandled` for `q`/`quit` so the App layer (which owns the terminal and `save_session`) drives the lifecycle.

Files touched:

- `src/app/state.rs:1148-1153` — `:q`/`:quit` returns `NotHandled` with comment explaining the deferral.
- `src/app/state.rs` test block — `cmd_quit` and `cmd_quit_long` replaced by `cmd_quit_defers_to_app` and `cmd_quit_long_defers_to_app`, asserting NotHandled + `should_quit` stays false.
- `src/app/mod.rs:7745-7779` — new `App::request_quit()` helper (the old Action::Quit body verbatim).
- `src/app/mod.rs:9579` — `Action::Quit => self.request_quit(),`.
- `src/app/mod.rs:4253-4261` — new `:q`/`:quit` arm in `App::dispatch_command` calling `request_quit()`.
- `CHANGELOG.md` — Unreleased / Fixed entry.

## Validation

- `cargo fmt --check` clean.
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test` — 683 pass, 0 fail.
- Two new regression assertions guarantee pure-domain dispatch can never re-acquire `:q` self-handling without breaking tests.

## Status

Branch `fix/q-command-session-save` off `bitbucket/main`, commit `a1cba7b`. Issue to follow on `Tripstack-Corp/spyc`.

<!-- Entry-ID: 01KRZF9DMD7MDD8RBA59EGBQ19 -->
