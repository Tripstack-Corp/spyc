# spyc → agent path handoff

**Status:** exploration, decision pending. Captured from a 2026-05-27
design session so the reasoning doesn't evaporate. No code written yet.

## The problem

`^a s` (`send_selection_to_pane`, `src/app/mod.rs:7515`) writes the
current selection to the focused pane's stdin so an agent (claude /
codex) or shell can act on it. Today it anchors each path on
**`PROJECT_HOME`** (`src/app/mod.rs:7542`): under-project paths are made
relative to the project root, everything else stays absolute.

That breaks when the agent's working directory isn't the project root.
Observed: spyc pasted `book-org/client-api-20-contract/docs`, the agent
ran `cd book-org/... && …` from `~/src/tripstack_platform`, and got
`no such file or directory` — because the relative path was anchored on
spyc's project root, not the agent's actual cwd.

What we actually want from a handoff reference:

1. **Terse** in the agent's view (not a leaked `/Users/x/…`).
2. **Resolves deterministically** to the correct absolute file,
   regardless of the agent's cwd.
3. **Universal** — works for claude, codex, *and* a plain shell/REPL.

## The core constraint (why this is awkward)

There are only four channels between spyc and the agent, each with a
hard tradeoff:

- **PTY (terminal bytes)** — the only consumer that interprets arbitrary
  text here is *the model*. A terse token sent this way resolves only if
  the model chooses to expand it. Not deterministic.
- **MCP side-channel** — structured, deterministic *data* (spyc owns the
  context file), but the agent only reads it when the model decides to
  call the tool. The *trigger* is model-dependent.
- **Shared filesystem** — a real path resolves deterministically for any
  tool, but a real path can't be both terse and complete.
- **Agent-side hook** — can expand a token *before* the model sees it
  (deterministic), but currently claude-only and requires setup.

**Consequence: "terse in the agent's view" + "deterministic resolution"
is impossible through the PTY alone.** It requires an expander on the
agent side, which is either the model (voluntary) or the buffer owner
(a hook). This is channel topology, not a missing trick.

## Options explored

### A. Native path, tiered on the pane's live cwd  — RECOMMENDED DEFAULT

Change `^a s` to anchor on the pane's **live cwd** instead of
`PROJECT_HOME`:

- file under the agent's live cwd → **relative** (terse, the common case)
- otherwise → **absolute** (safe; not `../../..`)

spyc already tracks the live cwd: `TabEntry::live_cwd`
(`src/pane/tabs.rs:162`), polled via `proc_cwd::cwd_for_pid`
(`src/proc_cwd.rs`, `readlink /proc/<pid>/cwd` on Linux, `lsof` on macOS,
1 Hz TTL cache). Deterministic, universal, zero setup, no model
cooperation. **This is the actual bug fix and should ship regardless.**

Note: don't `~`-collapse the absolute tier — claude's `Read` tool wants
real absolute paths and won't reliably expand `~`. The in-tree relative
path already gives the terseness win.

### B. Consumer-aware `^a s`: MCP imperative for agent panes

spyc already detects the pane's agent kind (`detect_agent_kind`,
`src/app/mod.rs:5966`) and already publishes the selection over MCP:
`snapshot_context` (`src/app/mod.rs:1133`) writes `cwd` + `cursor_file` +
`picks` to the context file on every change; the `get_spyc_context` tool
and `spyc://context` resource (`src/mcp.rs`, `CONTEXT_URI` at `:31`)
expose it, resolving picks to absolute paths against cwd.

So for a claude/codex pane, `^a s` could paste a short **imperative**
instead of paths:

> Read the files from my spyc selection — call `get_spyc_context` for the
> absolute paths.

Terse display, deterministic *data* (spyc owns the context file), and
high reliability because it's an explicit prompt directing a named tool
— not a passive token in scrollback. Works for claude and codex; it
sidesteps codex's weak `instructions` handling (see D) because the
directive is in the prompt.

Keep behavior predictable: `^a s` = consumer-aware, plus a sibling
(`^a S`) that always sends literal native paths (option A) for when you
want the path itself (e.g. dropping it into a code comment).

### C. `UserPromptSubmit` hook — the clean claude version of "expand on Enter"

The exact dream flow — terse `spyc://` token visible in the input box,
expanded to a full path on Enter, deterministically — belongs at the
layer that *owns the input buffer*: the agent. claude exposes this via
the `UserPromptSubmit` hook: claude hands the submitted prompt to the
hook, the hook calls back to spyc (reads the context file / MCP socket),
expands `spyc://` refs, and returns the rewritten prompt.

Deterministic and terse-in-box. Cost: **claude-only**, one-time hook
install. Codex has no submit-hook equivalent. Worth prototyping so we can
feel the flow; not worth making the default.

### D. `spyc://` token in paste + MCP `instructions`  — WEAK, not recommended

Encode the convention in the MCP `initialize` `instructions` field
(spyc currently sends none — `handle_initialize`, `src/mcp.rs:903`) so
the model knows to resolve `spyc://` tokens via `get_spyc_context`.

- **claude** surfaces `instructions` as a standing system-prompt block
  (reasonable odds it's followed).
- **codex** folds `instructions` into the *tool namespace description*
  (`rmcp_client.rs:371-375`), surfaced mainly via `tool_search`
  (test: `tool_search_uses_non_app_mcp_server_instructions_as_namespace_description`).
  Buried metadata, not standing guidance — unlikely to be acted on
  proactively.

Either way it's model-voluntary: the model has to *notice* a token in
noisy scrollback and spend a tool call. Option B (explicit imperative)
dominates it.

### E. Intercept `<ENTER>`, rewrite the input ourselves  — INFEASIBLE

The appealing idea: spyc intercepts Enter, expands `spyc://` refs in the
agent's input line to full paths, then lets Enter through.

Blocked because **spyc keeps no model of the agent's input buffer.** When
the pane is focused, non-meta keys are blind pass-through to the PTY
(`KeyDestination::BottomPane`, `src/app/route.rs:129`). To rewrite the
buffer on Enter, spyc would need its contents *and* cursor position:

- keystroke replay is lossy — backspace/`^w`/arrow-edits, history recall
  (up-arrow injects text spyc never saw), paste, autocomplete/`@`-mentions
  — reconstructing the buffer means reimplementing each agent's line
  editor;
- screen-scraping the input box from the vt100 grid is agent- and
  version-specific and rots fast (cf. the codex DECSTBM scroll-region
  saga — same coupling trap);
- even knowing the text, replacing a token in place requires sending
  precise backspaces/cursor-moves into the agent's editor, which needs
  authoritative cursor state spyc doesn't have.

The *instinct* (spyc expands refs itself, deterministically) is right —
it just has to happen where spyc owns the bytes as a unit, which is
option A/B (injection time) or option F (bracketed paste), not mid-edit
in a foreign buffer. The buffer-owning version is option C (claude hook).

### F. Bracketed-paste interception  — feasible add-on

When the user *pastes* into a pane, the payload arrives at spyc as one
delimited burst (paste-start … paste-end). spyc could expand `spyc://`
refs in that burst before forwarding. Universal, deterministic, and it
handles refs spyc didn't inject (e.g. pasted from notes) — but only
pasted, not typed, text. Small and self-contained; nice-to-have.

### G. spyc-owned compose line  — feasible, niche

A spyc prompt where you type a message with `spyc://` / `%` refs; on
spyc's own Enter (spyc owns *this* buffer) it expands and sends the full
text + newline to the pane. Universal and deterministic — cost is you
compose in spyc and lose the agent's native input box while doing so.

### Rejected outright

- **Symlink farm** under `~/.cache/spyc/refs/` pasted as absolute symlink
  paths — universal/terse/deterministic, but leaks a *fake* location: the
  agent reasons as if the file lives in the cache dir, breaking
  git/relative-import context, with basename collisions and cleanup
  burden.
- **OSC 8 hyperlinks** (terse visible text, hidden `file://` target) — the
  model reads the visible text, not the hidden URI; it's a display
  feature, not an input semantic. (Could still be nice for spyc's *own*
  pager rendering.)

## Recommendation (for the future decision)

1. **Ship option A** (live-cwd-anchored native path) — it's the actual
   bug fix, universal and deterministic.
2. **Add option B** (consumer-aware `^a s` → MCP imperative for
   claude/codex; `^a S` for literal paths) — small, the detection and
   context-publishing infra already exist.
3. **Prototype option C** (`UserPromptSubmit` hook) to feel the
   terse-token-on-Enter flow on claude; keep it opt-in.
4. Options F/G are optional polish; D and E are documented as dead ends
   so we don't relitigate them.

## Key code references

- `^a s` impl: `send_selection_to_pane`, `src/app/mod.rs:7515`
  (PROJECT_HOME anchoring at `:7542`); help text `src/ui/help.rs:224`.
- live cwd: `TabEntry::live_cwd`, `src/pane/tabs.rs:162`;
  `proc_cwd::cwd_for_pid`, `src/proc_cwd.rs`.
- agent detection: `detect_agent_kind`, `src/app/mod.rs:5966`.
- MCP context publish: `snapshot_context` / `write_context`,
  `src/app/mod.rs:1133` / `:1147`.
- MCP server: `src/mcp.rs` — `handle_initialize` (`:903`, no
  `instructions` field today), `CONTEXT_URI` (`:31`), `resources/list`
  (`:921`), `get_spyc_context` tool (`~:972`), `read_picks_from_context`
  (`:1317`).
- input routing (blind PTY pass-through): `src/app/route.rs:129`.
- codex `instructions` handling: `~/src/codex` —
  `codex-rs/codex-mcp/src/rmcp_client.rs:541` (capture), `:371-375`
  (namespace-description fallback); surfaced via `tool_search`.
