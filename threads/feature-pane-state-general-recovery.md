# feature-pane-state-general-recovery — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: feature-pane-state-general-recovery
Created: 2026-05-15T08:02:56.640793+00:00

---
Entry: Claude Code (caleb) 2026-05-15T08:02:56.640793+00:00
Role: planner
Type: Plan
Title: Plan: general pane-state persistence / recovery (claude, codex, gemini — and everything else)

Spec: planner-architecture

## Why this thread exists

Caleb's question: "`claude --recover` maybe for the claude case — but what about the general state?"

This thread separates the recovery problem along the dimension that actually matters: **what kind of program is running in the pane**. Each kind admits a different ceiling on how much state can be revived, and the right answer is to handle each tier explicitly rather than promise something we can't deliver for arbitrary processes.

Distinct from the toggle thread (`feature-pane-toggle-preserve-context`), which is specifically about the *hide-unhide round-trip*. This thread is the broader story: spyc quit/relaunch, system reboot, crash recovery, "I forgot which tabs I had open last week".

## What exists today (recap from `caleb-initial-thoughts-and-findings` #4)

Spyc already has tier-aware resume in `src/state/sessions.rs`. `AgentKind` enum (`src/state/sessions.rs:19`):

```rust
pub enum AgentKind {
    Claude,    // /resume <sid>\r over stdin; sid scraped from JSONL or exit banner
    Codex,     // spawn `codex resume <UUID>` directly; UUID from exit banner
    Gemini,    // --resume <index>; index found via `gemini --list-sessions` lookup
    Other,     // no resume — respawn fresh
}
```

Cross-restart only. Hide-unhide doesn't go through this path (see the toggle thread for that fix). Tier-3 here is **`Other`** — anything that isn't a known agent CLI gets the bare command respawned with no continuity.

The user's `claude --recover` framing: claude itself has no `--recover` flag — recovery is via `/resume <sid>` or `--resume`. So "what's the equivalent for the general case" is the right framing.

## The tiers and their ceilings

### Tier 1 — Known agent CLIs (Claude, Codex, Gemini)

These have first-class session-resume protocols. Spyc already implements them. Wins from here are incremental:

- **Add to `AgentKind`** when new vendor CLIs ship resume (Aider has `--restore-chat-history`; Cursor's CLI is more complex; Continue.dev has its own format). Each is a few lines in `resolve_*_resume_target` + a respawn branch in `restore_session`.
- **Tighten the Claude path**: today, sid capture depends on either the exit banner (only printed on clean quit) or the JSONL scan (only works if claude has flushed an entry — flaky for very-new sessions). A more reliable signal would be to listen for claude's "session created" notification on the MCP socket, since spyc is already the MCP host. The session id arrives in that protocol traffic. This eliminates the banner/JSONL race entirely.
- **Document `--recover` as `F11`**: today `F11`/`-r` opens claude's own resume picker. If users say `--recover`, they probably want either F11 or the post-restart picker.

### Tier 2 — Stateful processes with their own persistence (vim, less, lazygit, journalctl -f)

The process keeps state in its own files (vim sessions, less marks, lazygit's stash view). Recovery means **respawning the same command** and letting the program rediscover its own state. Spyc's current `Other` behavior — save the command + cwd, respawn on restore — actually does the right thing for many of these.

Wins:

- **Document the contract**: "non-agent tabs are respawned bare; if your tool persists its own state (vim with a session file, lazygit's git state), you'll see it after restore."
- **Optional: per-command resume recipes** for tools that need a flag flip. A `[[pane.recipe]]` table:
  ```toml
  [[pane.recipe]]
  match = "vim"               # exact command (or regex with `regex = true`)
  resume_command = "vim -S Session.vim"   # used on restore_session if Session.vim exists in cwd
  ```
  Trade simplicity for the small fraction of users who care.

### Tier 3 — Pure ephemeral processes (bash, fish, dev servers, top, htop)

Their state is in-memory and cannot survive process death:
- Shell history goes to disk (each shell handles it); env vars / aliases / functions / bg jobs / pwd-walks are gone.
- Dev servers' watchers, in-flight requests, sockets — gone.
- `top` / `htop` aren't supposed to persist anything.

There is **no general mechanism** to revive these. Three honest options:

#### Option 3A — Don't try; do cosmetic snapshot only

Capture the pane's **rendered grid** (vt100 cell buffer + recent scrollback) at close. On restore, respawn the command and overlay the snapshot as a faded "history" backdrop until the new process has produced output. This is purely visual continuity.

- ✅ Solves "I lost my place in the output". No process-state lies.
- ✅ Tiny implementation: `Pane::snapshot() -> SnapshotBlob` (serialize the existing vt100 grid) + restore-side overlay.
- ⚠️ The text in the snapshot is no longer interactive — selecting it through quick-select is a fresh-process operation.

#### Option 3B — tmux/screen wrapping

Spawn each pane inside a hidden tmux/screen session (one tmux session per tab, named by tab uuid). When spyc closes the pane, tmux keeps the session alive in the background. When spyc respawns, attach to the same tmux session — the program inside (bash, vim, dev server) has been running the whole time.

- ✅ The most powerful answer. Real continuity for *anything*.
- ✅ Survives spyc crashes (tmux outlives spyc).
- ⚠️ Hard dependency on `tmux` (or `screen`) being installed. Could be opt-in (`pane.use_tmux = true` in spycrc).
- ⚠️ Two layers of pty between user and program — tmux adds latency, eats some keystrokes for its own commands (^B by default; collides with claude bindings), and changes how SIGWINCH propagates. Workable, but a real engineering cost. The wezterm/zellij precedent shows it's done routinely; the bug surface is non-trivial.
- ⚠️ The tmux session needs cleanup on user intent ("forget this pane forever"). Naming scheme: `spyc-<repo-hash>-<tab-uuid>`. Cleanup on explicit close.

#### Option 3C — Process detach (disown / setsid)

Detach the child so it survives spyc death. Doesn't work cleanly: spyc owns the pty; on spyc death the pty closes; bash on the other end gets SIGHUP. Workarounds (re-parent stdin/stdout to /dev/null) defeat the purpose (you can't reattach output). Functionally collapses into Option 3B without tmux's framing.

Rule it out.

### Tier 4 — Spyc's own UI state

Distinct from process state — the things spyc itself owns. Most are already persisted (cwd via `Session.cwd`, project_home, pane_height_pct, harpoon, marks, frecency, history, graveyard). Gaps:

- **Pane scrollback contents** — today vt100 grid + scrollback is per-process and dies with the process. Option 3A above would cover this even without process recovery.
- **Quick-select state** — ephemeral; not persisted, probably fine.
- **Current pager/grep view in the top region** — not persisted across spyc restart. Probably orthogonal to "pane state" but worth flagging.

## Recommendation

Three deliverables, in order of cost / value:

1. **Polish Tier 1.** Add MCP-side session-id capture for Claude (eliminate banner/JSONL race). Add aliases for any other agent CLI when their resume protocol stabilizes. Document `F11` as "the recover key for claude".

2. **Ship Tier 3A — PTY-grid snapshot.** Cheap, gives users the cosmetic continuity they probably want most of the time when they say "remember where I was". Roundtrip the vt100 grid through `Session.tabs[i].grid_snapshot: Option<Vec<u8>>` (or an external blob file when grids get large).

3. **Defer Tier 3B (tmux wrapping) behind opt-in.** `pane.use_tmux = true`. Worth the engineering cost only if users specifically ask. Scope: one round of design to map our PaneTabs onto tmux's session model and decide the binding collision strategy (probably remap our `^a` prefix when tmux is present, or set tmux's prefix to something exotic).

Don't try to invent a general process-state serializer. That problem is unsolved at the OS level (CRIU exists but is fragile + Linux-only + not suitable for interactive tty programs).

## Companion: per-command resume recipes (Tier 2)

If we add the recipes table, it lives next to `[pane]`:

```toml
[[pane.recipe]]
match = "vim"
resume_command = "vim -S {{cwd}}/Session.vim"
condition = "{{cwd}}/Session.vim exists"   # optional

[[pane.recipe]]
match_regex = "^node .*"
resume_command = "{{original_command}}"     # default — bare respawn
capture_token = "ready on port (\\d+)"      # regex on scrollback; sets {{token}}
```

This generalizes the AgentKind dispatch into user-extensible territory. Premature for v1; sketch only.

## File pointers

- AgentKind + resume scaffolding: `src/state/sessions.rs:19/297-498` and the per-kind paths.
- Resume reapply: `src/app/mod.rs:5750-5772` (pending_resume_send timer).
- Save / restore: `src/app/mod.rs:7629/7913`.
- vt100 grid (snapshot source): `src/pane/mod.rs:36-53`, `Pane::parser`.
- MCP socket (Tier-1 polish hook): `src/mcp.rs`, `src/mcp_cmd.rs`.

## Open questions

- Is `claude --recover` something Caleb saw published, or shorthand for the resume flow? If real, what flag — happy to integrate.
- Acceptable scope cap: do we want any process-detach experiment, or hold the line at "non-agents respawn bare + cosmetic snapshot"?
- For the recipes table — does this earn its keep, or is it just `default_command` with extra steps?

Companion threads: `feature-pane-toggle-preserve-context` (in-session hide-unhide), `feature-pane-multi-split-config` (layout).

tags: #pane #sessions #recovery #persistence #planning

<!-- Entry-ID: 01KRNAHTBJ4GJSD4WERS4P5STG -->
