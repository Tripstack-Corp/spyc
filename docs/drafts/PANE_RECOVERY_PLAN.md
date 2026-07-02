# spyc pane state recovery

**Status:** plan, not yet implemented. Sourced from external-contributor
analysis (Caleb Howard, 2026-05-15) — kept here so the design doesn't
evaporate.

**Target release:** v1.52 (feature-shaped, not blocking v1.60 hub work).

**Distinct from** [`feature-pane-toggle-preserve-context`](https://...) —
that's the in-session hide/unhide round-trip (`F10` / `^a-\`), which has
a different ceiling and a simpler fix (hide-don't-destroy; the pty stays
alive). This doc is the broader story: spyc quit→relaunch, system
reboot, crash recovery, *"I forgot which tabs I had open last week."*

## Thesis

The "recovery" word collapses three very different problems. Pretending
they're one problem leads to either over-promising (we cannot revive an
arbitrary `bash` shell) or under-delivering (we *could* show users what
they were looking at, even if the process is gone).

Recovery splits along **what kind of program is in the pane**. Each kind
admits a different ceiling on what we can restore, and the right answer
is to handle each tier explicitly.

## What exists today

Cross-restart resume already lives in `src/state/sessions.rs`, keyed off
`AgentKind`:

```rust
pub enum AgentKind {
    Claude,    // /resume <sid>\r over stdin; sid scraped from JSONL or exit banner
    Codex,     // spawn `codex resume <UUID>` directly; UUID from exit banner
    Gemini,    // --resume <index>; index found via `gemini --list-sessions` lookup
    Other,     // no resume — respawn fresh
}
```

That's: tier-1 agents resume their conversations; everything else gets
the bare command respawned. Cross-restart only — hide/unhide doesn't go
through this path today.

## The tiers and their ceilings

### Tier 1 — Known agent CLIs (Claude, Codex, Gemini)

These have first-class session-resume protocols and spyc already
implements them. Wins from here are incremental:

- **Add to `AgentKind`** when new vendor CLIs ship resume protocols
  (Aider has `--restore-chat-history`; Cursor and Continue.dev have
  their own formats). Few lines each in `resolve_*_resume_target` plus a
  respawn branch in `restore_session`.
- **Tighten the Claude sid-capture path.** Today, sid capture depends on
  either the exit banner (only printed on clean quit) or the JSONL scan
  (only fires after Claude has flushed an entry — flaky for very-new
  sessions). A more reliable signal: spyc is already the MCP host;
  listen for Claude's "session created" notification on the MCP socket
  and pull the session id from protocol traffic. Eliminates the
  banner/JSONL race entirely.
- **Document `F11`** as the per-tab recover key (it already exists —
  `Action::ResumePane` opens claude's own resume picker for the cwd).

### Tier 2 — Stateful processes with their own persistence

vim with session files, lazygit's git state, less with its marks. The
process keeps state in its own files; recovery means **respawning the
same command** and letting the program rediscover its own state. The
current `Other` behavior — save command+cwd, respawn on restore —
already does the right thing for most of these.

Wins:

- **Document the contract**: "non-agent tabs are respawned bare; if your
  tool persists its own state, you'll see it after restore."
- **Optional: per-command resume recipes** for tools that need a flag
  flip. A `[[pane.recipe]]` table in `.spycrc.toml`:
  ```toml
  [[pane.recipe]]
  match = "vim"
  resume_command = "vim -S Session.vim"
  condition = "{{cwd}}/Session.vim exists"   # optional
  ```
  **Premature for v1.** Sketch only — most stateful tools already
  rediscover their state from the bare respawn.

### Tier 3 — Pure ephemeral processes (bash, fish, dev servers, top/htop)

Their state is in-memory and **cannot survive process death** in any
honest sense:

- Shell history goes to disk per shell; env vars / aliases / functions /
  background jobs / pwd-walks are gone.
- Dev servers' watchers, in-flight requests, sockets — gone.
- `top` / `htop` aren't supposed to persist anything.

There is **no general OS mechanism** to revive these (CRIU exists,
fragile + Linux-only + not suitable for interactive tty programs).
Three honest paths:

#### Option 3A — Cosmetic snapshot (recommended for v1.52 Phase 0)

Capture the pane's **rendered grid** (vt100 cell buffer + recent
scrollback) at close. On restore: respawn the command, render the
snapshot as a faded backdrop until the new process produces output —
purely visual continuity, no claim that the process is alive.

- ✅ Solves "I lost my place in the output."
- ✅ Tiny implementation — `Pane::snapshot() -> SnapshotBlob`
  (serialize the existing vt100 grid) + a render-side overlay.
- ⚠️ The text in the snapshot is not interactive — quick-select on it
  is a fresh-process operation. Probably fine.

#### Option 3B — tmux / screen wrapping (deferred behind opt-in)

Spawn each pane inside a hidden tmux session (one tmux session per tab,
named by tab UUID). spyc closing the pane leaves the tmux session
alive; respawn attaches to the same session — the program inside has
been running the whole time. The wezterm/zellij precedent shows it's
done routinely.

- ✅ The most powerful answer; real continuity for *anything*.
- ✅ Survives spyc crashes (tmux outlives spyc).
- ⚠️ Hard dependency on tmux. Opt-in via `[pane] use_tmux = true`.
- ⚠️ Two layers of pty: latency, keystrokes eaten by tmux for its own
  commands (default `^B` collides with claude bindings — would need to
  remap our `^a` prefix or set tmux's prefix to something exotic),
  SIGWINCH propagation changes.
- ⚠️ Cleanup story: tmux sessions named `spyc-<repo-hash>-<tab-uuid>`;
  removed on explicit pane close.

#### Option 3C — Process detach (disown / setsid) — rejected

Detach the child so it survives spyc death. Doesn't work cleanly: spyc
owns the pty; on spyc death the pty closes; the child gets SIGHUP.
Workarounds (re-parent stdin/stdout to /dev/null) defeat the purpose
(can't reattach to output later). Functionally collapses into 3B
without tmux's framing. Skip.

### Tier 4 — Spyc's own UI state

Distinct from process state — the things spyc itself owns. Most are
already persisted (cwd, project_home, pane_height_pct, harpoon, marks,
frecency, history, graveyard). Gaps:

- **Pane scrollback contents** — today the vt100 grid + scrollback is
  per-process and dies with the process. Option 3A above covers this
  cosmetically even without process recovery.
- **Quick-select state** — ephemeral; not persisted; almost certainly
  fine to leave alone.
- **Current pager / grep view in the top region** — not persisted
  across spyc restart. Orthogonal to "pane state" but worth flagging.

## Phases

**Phase 0 — Tier 3A cosmetic snapshot.** Highest value per line of
code. `Pane::snapshot()` serializes the current vt100 cell grid +
recent scrollback into a `SnapshotBlob`. Session save attaches one
blob per `SavedTab`. Restore renders the blob as a faded backdrop
until the new process produces output, then naturally replaces it.
Survives quit → `spyc -r` cleanly. ~1-2 days.

**Phase 1 — Tier 1 polish: MCP-side Claude sid capture.** Listen for
the "session created" notification on the MCP socket; record the sid
in `TabInfo` directly. Removes dependency on the exit banner + JSONL
scan. ~1 day.

**Phase 2 — Optional Tier 3B: `[pane] use_tmux` flag.** Opt-in wrapper
that spawns each pane inside a per-tab tmux session. Includes the
binding-collision strategy (remap spyc's `^a` prefix or change tmux's
prefix to e.g. `^_`) and a cleanup story. Gated on real user demand.
~1 week.

**Phase 3 (out of scope unless requested): per-command resume
recipes.** `[[pane.recipe]]` table. The bare-respawn path already
covers most stateful tools.

## Recommendation

Ship Phase 0 first. Phase 1 only if the banner/JSONL race actually
bites in production (no reports yet). Phase 2 only if a user
specifically asks for "I want my bash session to survive a spyc
restart." Don't try to invent a general process-state serializer —
that problem is unsolved at the OS layer.

## Open questions

1. **Snapshot size cap.** A vt100 grid for a tall window with 10K
   scrollback is on the order of hundreds of KB. Worth bounding the
   per-tab snapshot size (e.g. only the visible viewport + last 200
   scrollback rows) to keep sessions JSON manageable.
2. **Snapshot fidelity.** Cell-by-cell with colors? Or plain-text rows?
   Plain text is simpler but loses syntax highlighting in the backdrop.
   Probably cell-by-cell, since vt100 already has the data.
3. **When to drop the backdrop.** First byte from the new process?
   First non-empty paint? Configurable timeout?
4. **Interaction with `F11` (Action::ResumePane).** F11 already does
   per-tab resume for claude. Phase 0 snapshot complements it; Phase 1
   sid-capture would make F11 more reliable for fresh sessions.

## Provenance

Plan structure and tier analysis written by Caleb Howard
(2026-05-15) — captured here near-verbatim with light editing for our
voice and a "phases" section added. File pointers verified against
`main` at the time of writing:

- `AgentKind` + resume scaffolding: `src/state/sessions.rs`
- Resume reapply (pending_resume_send timer): `src/app/mod.rs`
  around the pane-startup path
- Save / restore entry points: `App::save_session` /
  `App::restore_session` in `src/app/mod.rs`
- vt100 grid (snapshot source): `src/pane/mod.rs`
- MCP socket (Phase 1 hook): `src/mcp.rs`
