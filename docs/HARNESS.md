# Running agents in spyc — the harness guide

spyc hosts several agents in its pty pane, and each behaves differently in ways
you otherwise learn by getting bitten. This is the "what will surprise you"
document: recommended settings, what screen mode each agent uses and what that
costs you, where conversation history actually lives, and how session recovery
differs per agent.

Scope, so you know where to look for what:

- **This file** — per-agent quirks and the settings worth changing.
- [`AGENT_ORCHESTRATION.md`](AGENT_ORCHESTRATION.md) — how activity dots,
  notifications, session persistence and the merge/scope registry work.
- [`../CONFIGURATION.md`](../CONFIGURATION.md) — every setting, with syntax.
- [`KEYBINDINGS.md`](KEYBINDINGS.md) — the full keymap.

---

## 1. Harness settings worth getting right

### Your agent's model can be pinned outside the agent

This one costs whole sessions, so it goes first. Claude Code reads
**managed settings**, which can pin a model your `/model` selection cannot
override for long:

```
/Library/Application Support/ClaudeCode/managed-settings.json    # macOS
```

Relevant keys: `model`, `availableModels`, `enforceAvailableModels`.

The failure mode is quiet. `/model` appears to work — it switches the *current*
session — but the pin reasserts itself on restart, so a long task silently
continues on a different model than you think, and nothing announces the
change except one line at the moment you switch. If a session feels
uncharacteristically weak, check the pin before concluding anything about the
model.

That file is **root-owned** (`root:admin`), so changing it needs elevation. It's
the right place to change it, though: re-selecting via `/model` after every
restart is the thing that doesn't stick.

Verify what's actually in effect:

```sh
sudo -v && python3 -c "import json;print(json.load(open('/Library/Application Support/ClaudeCode/managed-settings.json')).get('model'))"
```

### spyc settings for agent work

| Setting | Suggested | Why |
|---|---|---|
| `[mouse] capture` | `true` (default) | The wheel scrolls whatever the pointer is over, including panes whose child ignores mouse reports. Costs your terminal's native click-drag select — hold **Shift** (most terminals) or **Option/Fn** (iTerm2), or `:mouse off` for the session. |
| status hooks | on | Per-agent hooks let the agent *report* `working`/`blocked`/`done`, which is what makes the tab dots trustworthy instead of guesses from output timing. `:hooks` shows state. |
| `[notify]` | `desktop = true` | The "which agent needs me" ping. `Blocked` fires every enabled channel; the routine `Done` only fires channels that opt in via `*_done`. Details in [`AGENT_ORCHESTRATION.md`](AGENT_ORCHESTRATION.md). |
| `[clipboard] command` | set it on WSL | No X display by default under WSL2 — point it at `clip.exe`. Alternatively `[clipboard] via = "osc52"`. |

---

## 2. Screen modes, per agent

**This is the biggest source of surprise**, because a child's screen mode changes
what scrollback, text selection *and* the wheel do — all three at once. The mouse
columns below were captured from each agent's actual init escape sequences in a
pty, not inferred.

| agent | screen | mouse reporting | wheel behavior in spyc |
|---|---|---|---|
| **claude** — `/tui fullscreen` | alternate screen (`?1049h`) | `?1000h ?1002h ?1003h ?1006h` | Forwarded verbatim — claude scrolls and selects natively. A real mouse report carries coordinates a synthesized key can't, so forwarding always wins. |
| **claude** — `/tui default` | inline, **no** scroll region | **none** | Nothing to forward and no verified scroll key, so a sustained wheel-up opens **spyc's own** captured history (`^a v`'s pager). Inline claude's completed output reaches the main buffer, so unlike codex there genuinely is a capture to show. |
| **codex** | inline, with a DECSTBM scroll region | **none** | codex discards mouse events outright, so spyc synthesizes arrows to drive its own `^T` transcript overlay. |
| **agy** | configurable; default `native terminal (inline)` | none (inline) / `ButtonMotion` (altscreen) | Inline: spyc sends **Shift+Arrow**, agy's own scroll affordance. Altscreen: agy requests motion reporting, so it scrolls natively. |
| **zot** | — | — | No transcript or scroll integration yet; treated as a plain child. |

agy's own settings UI frames the trade-off well, and it applies generally:
**altscreen** means no flicker but you must hold Shift/Option for native
selection; **inline** preserves terminal behaviour but may truncate long
conversations.

claude's own mode is `/tui`, persisted as `"tui": "default" | "fullscreen"` in
`~/.claude/settings.json` — `default` (inline) is what you get out of the box, and
an invalid value there makes claude skip the whole settings file. spyc doesn't
read that key: it branches on what the pty actually did (`?1049h` observed, and
whether the child asked for mouse reporting), which stays right when you flip the
mode mid-session.

### The consequence people hit

An **alternate-screen** agent's history never enters the terminal's main buffer,
so it is never in spyc's vt100 scrollback. codex's history isn't either — its
scroll region keeps completed turns off the main buffer, so **zero** rows reach
spyc's emulator. In both cases "just scroll up" cannot work, no matter what spyc
does. Section 3 is what does work.

Text selection follows the same fork, and needs nothing extra: a child that asked
for mouse reporting draws its **own** selection (fullscreen claude), and a child
that didn't gets **spyc's** drag-select over the pane grid, copied to the
clipboard on release. Only the wheel needed the third answer above, because it
has somewhere else to go — spyc's capture — when neither side owns it.

---

## 3. Scrollback: where history actually lives

`^a v` opens pane scrollback, and it picks its source rather than assuming one:

| condition | source |
|---|---|
| agent has an on-disk transcript, and it's enabled **or** the agent is alt-screen | the **transcript file** |
| alt-screen with no transcript | dead end — spyc says so rather than showing you an empty pager |
| otherwise | vt100 terminal capture |

That's `decide_scroll_source` in `src/app/pane_scroll.rs` — a pure function, if
you want to read the exact ladder.

So for codex and agy — and for claude in `/tui fullscreen` — `^a v` reads the
agent's **own transcript file**, not the screen. That's strictly better than
capture: real text, no grid or repaint artifacts, and searchable.

**Inline claude has both**, and that's the one case where the choice is real. Its
capture is genuine (its output reaches the main buffer) and holds what the
transcript never will, like whatever the shell printed before the agent started;
the transcript holds real text where the grid has repaint artifacts. So the
config gate decides which one `^a v` opens first
(`[pane] claude_transcript_scrollback`, default off = the capture) and **`T`
swaps** — no config edit, no relaunch. The flip sticks for that tab, so `r`
reloads what you picked, and is dropped when the view closes.

The gate only applies while a capture *exists*: a pane that hasn't scrolled
anything off yet is in the same position as an alt-screen one, so it engages the
transcript too rather than showing you an empty pager.

Inside either view:

- `T` — swap the source: terminal capture ⇄ agent transcript (says which one is
  missing if the pane only has one)
- `t` — toggle the agent's tool-call / tool-result lines (transcript scrollback
  only; a long tool-heavy session is much easier to read with them off)
- `l` — toggle line numbers
- `/` — search, `r` — reload
- scrolling **down past the end** leaves scroll mode and snaps back to the live
  pane — the same as `Esc`, so a flick out the bottom returns you to the agent
  instead of parking you at `[EOF]`

Transcript sources per agent live under `src/state/` (`claude_transcript.rs`,
`codex_transcript.rs`, `agy_transcript.rs`). zot has none yet.

---

## 4. Session recovery, per agent

`spyc -r` restores tabs, agent conversations and the vsplit. **How** each agent
resumes differs, and the differences leak:

| agent | resume mechanism |
|---|---|
| **claude** | spyc spawns fresh, then **types `/resume <id>` into stdin** once the banner settles. The `--resume` CLI flag has a mount-crash regression, so the stdin route is deliberate, not a workaround for convenience. |
| **codex** | baked into the spawn command — `codex resume <uuid>`, or `resume --last`. The uuid comes from the tab's *pinned* rollout claim, not from codex's exit banner, so a tab that was still running at quit resumes exactly too. |
| **agy** | baked into the spawn command — `--conversation <uuid>` when spyc has a pinned session id for the tab, falling back to `--continue` (the most recent for this cwd) when it does not. |
| **zot** | `--continue`. spyc doesn't capture a specific session path yet, so restore always continues the most recent. |

### The codex quirk that confuses everyone

**A resumed codex session appends to its original rollout file and leaves
`session_meta` frozen at the original creation time.** So a rollout created a
month ago can be the live one, and its recorded start time tells you nothing
about which session is current — only the file's mtime does.

That is why identifying "which rollout belongs to this pane" is genuinely hard,
and why two codex panes in the same directory is the case that breaks it. See
**#230**; the fix ranks a *fresh* pane by start-time proximity (its rollout
necessarily starts when the pane does) while keeping mtime primary for a
resume-without-id, because those are opposite tells.

### The autosave window

Beyond the save on quit, spyc keeps a debounced crash-sufficient autosave —
**2 seconds** after any tab/cwd/vsplit/geometry change
(`AUTOSAVE_DEBOUNCE`, `src/app/session.rs`). A `SIGKILL` therefore loses at most
that window, not everything since launch. It's armed only while dirty, so an
idle spyc still does no work.

---

## 5. Multiple spyc instances

Instances coexist — the MCP server uses a **PID-scoped** Unix socket
(`~/.local/state/spyc/mcp-<pid>.sock`), and if another instance already owns an
agent's config entry you get a takeover prompt rather than a silent fight.

The thing that genuinely breaks is **concurrent agents of the same kind in the
same directory**, because that's what makes session resolution ambiguous — same
cwd, same agent, and the on-disk signals stop distinguishing them (see #230).
If you're comparing two codex sessions side by side, expect that to be the
sharp edge, and prefer distinct worktrees.

---

## 6. Quick reference

| Want | Do |
|---|---|
| See agent history that isn't on screen | `^a v` (reads the transcript, not the screen) |
| Hide tool-call noise in that view | `t` |
| Find out why a dot isn't red | `:why-status` (active tab), `:activity dump` (all panes) |
| Reclaim native text selection | hold Shift (or Option/Fn on iTerm2), or `:mouse off` |
| Check which build you're on | `:about`, `:version`, or `gV` |
| See who's editing what, across agents | `:agent list` / `:agent registry` |
