# feature-pane-toggle-preserve-context — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: feature-pane-toggle-preserve-context
Created: 2026-05-15T07:52:15.383190+00:00

---
Entry: Claude Code (caleb) 2026-05-15T07:52:15.383190+00:00
Role: planner
Type: Plan
Title: Plan: preserve / recover claude session across pane hide-unhide

Spec: planner-architecture

## Why this thread exists

Confirmed in `caleb-initial-thoughts-and-findings` entry 4: `F10` / `^a-\` (`Action::TogglePane`, `src/app/mod.rs:4632`) is a destructive operation. It sets `pane_tabs = None`, and `Drop for PtyHost` (`src/pane/pty_host.rs:297`) SIGKILLs the claude process group. On re-toggle, `open_pane_tab` spawns a fresh `claude` — no `--resume`, no `/resume <sid>`, no carried context.

Caleb's ask: "is there a config option, or mechanism to retain context when the lower pane is hidden and reloaded? If the context is lost, plan how to persist or recover the context."

Context is in fact lost. Below is an arc of options from cheapest to most invasive.

## Goal

After `F10`/`^a-\` round-trip, the bottom pane should resume the same claude conversation the user was in (and, if multi-tab, every tab). Ideally also: a `--resume`-on-toggle preference, per-pane startup command list, and a persisted pane-height default.

## Options

### Option A — Hide-don't-kill (true "hide")

Keep the pane subprocesses alive while the pane is invisible. Change `toggle_pane` to a visibility flag rather than a tab-clear:

- Add `state.pane_hidden: bool` (or similar).
- Render path checks both `pane_tabs.is_some()` AND `!pane_hidden` to draw the pane; layout treats hidden = no pane area.
- `toggle_pane` flips the flag instead of dropping `pane_tabs`.
- PTY child keeps running, keeps emitting bytes; `drain_output` continues to fold them into the vt100 parser so on-unhide the rendered grid is up to date. (Already does this — `drain_output` always runs.)

Tradeoffs:
- ✅ Zero changes to claude lifecycle; truest "hide". No `/resume` dance, no banner scraping.
- ✅ Stays trivial to implement (one flag + render gating).
- ⚠️ The child writes to a vt100 grid sized to the most-recent pane area. When invisible, terminal resizes (`SIGWINCH` from the host) still need to forward a sensible size. Cleanest: pin the grid to the *last visible* size while hidden, then resize on unhide if host size changed. This is what `toggle_pane_zoom` already models (`src/app/mod.rs:6829-6844`).
- ⚠️ Long-hidden claude can drift (timeouts, MCP socket shifts, terminal title noise, output piling up). Most of that is harmless given vt100 has a bounded scrollback.
- ⚠️ Discoverability — current semantics ("toggle closes tabs") become "toggle hides"; `^W x` (close) and the prompt "pane closed" message need separate wording. Probably also need an explicit `^W X`-style "kill all tabs" if users want the old behavior.

Recommended as primary path. The two close-tab flows (`PaneCloseTab`, the explicit `:close` style intent) keep the destructive semantics; the visibility toggle goes non-destructive.

### Option B — Capture + reapply via the existing session resume

Wire `toggle_pane` through `save_session` / `restore_session`:

1. **Before close (in `toggle_pane`, when pane is currently open)**: call into the existing `save_session` path, but write to a sentinel location (e.g. `$XDG_STATE_HOME/spyc/transient-toggle.json`) so it doesn't pollute the user's normal session picker. Or even an in-memory `Option<Session>` field on `App` — no disk write needed since we want it to live only across the toggle.
2. **Kill the children gracefully**: call `PtyHost::shutdown(grace)` (`src/pane/pty_host.rs:255`) instead of relying on `Drop`. SIGTERM gives claude a chance to print its `Resume this session with:` banner, which `extract_claude_resume_token` then scrapes. (Currently `save_session` calls `resolve_claude_resume_target` which already prefers the JSONL scan over banner scraping, so even SIGKILL-via-Drop *might* find an sid via the JSONL path — but only if claude has flushed a new entry recently. Switching to graceful shutdown is more robust.)
3. **On re-toggle (when pane is closed)**: if the transient snapshot exists, route through `restore_session`'s tab-respawn-with-`pending_resume_send` path instead of the bare `open_pane_tab`. Clear the snapshot afterwards.

Tradeoffs:
- ✅ Reuses all the existing infrastructure (`resolve_claude_resume_target`, `command_without_resume`, `pending_resume_send`, the codex/gemini equivalents). One small new code path.
- ✅ Multi-tab works for free.
- ✅ Survives an spyc crash too if we write to disk — toggling becomes a continuation of the cross-restart story.
- ⚠️ A few-second delay on re-toggle while claude restarts and types `/resume <sid>`. User sees the banner-and-then-conversation flash.
- ⚠️ Sid capture isn't 100% reliable. Sessions started in this spyc instance with no JSONL flush yet will lose context. The codex/gemini paths have their own gotchas (e.g. gemini's `--resume <index>` requires a synchronous `--list-sessions` lookup at unhide time).
- ⚠️ Children dying takes `grace` time. With 250-500ms grace, the toggle feels slow.

### Option C — Stash entries (less expressive variant of A)

Move the `pane_tabs` value into a `stashed_pane_tabs: Option<PaneTabs>` slot on hide and back on show. Same effect as Option A but a different shape (keeps Option semantics intact for the render path).

Functionally equivalent to A; just a style choice.

### Recommendation

**Option A (hide-don't-kill) primary, Option B as a fallback** triggered only when the user explicitly closes (`PaneCloseTab` / `:close`) and *also* wants resume next time. Document `F10` / `^a-\` as "hide" and `^W x` as "close (kill)".

Optional follow-up — surface this as a config knob if users disagree:

```toml
[pane]
toggle_behavior = "hide"   # default — keeps subprocesses alive
# toggle_behavior = "close" # SIGKILL on toggle (current behavior)
# toggle_behavior = "resume" # close + capture sid + re-resume on re-toggle (Option B)
```

## Related asks from the same Caleb message

For the same `[pane]` section, two natural additions land alongside this:

1. `default_height_pct` — make `pane_height_pct` configurable; the runtime `^a +/-` adjustments and session-restore still override.
2. `default_tabs = [...]` or `[[pane.tab]]` array — list of `{ command, cwd?, label? }` tables to open at startup instead of just one. Falls through to `default_command` (current behavior) when empty. Also clarifies the relationship with `pane.default_command` (single-tab fallback).

These are independent from the toggle-context work and could ship in any order.

## File-level pointers for whoever implements

- Toggle handler: `src/app/mod.rs:4632` (`fn toggle_pane`).
- Drop chain: `src/pane/pty_host.rs:297` (`impl Drop for PtyHost`), plus graceful `PtyHost::shutdown` at line 255.
- Layout gating already handles "no pane" cleanly via `pane_tabs.is_none()` in `compute_layout` (`src/app/mod.rs:1892` onward) — adding a `pane_hidden` flag is a few-line gate at those callsites.
- Resume reapply machinery: `pending_resume_send` (`src/app/mod.rs:5750-5772`, `src/app/mod.rs:7982-7990`); session capture (`src/app/mod.rs:7629`); restore (`src/app/mod.rs:7913`).
- Tests live next to features — `src/app/state.rs:2664` has `TogglePane` apply tests; new tests would assert "pane hidden but `pane_tabs` retains a live `PtyHost`" and "re-toggle replays the same grid".

## Open questions for caleb

- Does "hide" need to also remember zoom/focus state across the round-trip? Today `toggle_pane` clears `pane_zoomed` and `pane_focus_before_zoom`; hide-don't-kill should arguably preserve them.
- Should `^a c` (`PaneCloseTab` cmd-prompt → new tab) be available while pane is hidden, auto-unhiding? Or stay destructive only when explicitly visible?
- Acceptable to add a top-level `[pane] toggle_behavior` knob, or keep this an internal default and skip the config exposure for now?

tags: #pane #toggle #pty #sessions #resume #planning

<!-- Entry-ID: 01KRN9Y7ZMCYM9TXJ3XZDJPW3M -->
