# spyc auto-approval & action log

**Status:** plan, not yet implemented. Tracking BACKLOG_DRAFT_NOTES.md entry:
> feature to allow spyc to approve certain Claude CLI actions automatically and keep a log

**Target release:** v1.51 (feature-shaped, not blocking v1.60 hub work).

## Thesis

When you're driving an agent (claude, codex, gemini) inside a spyc pane,
permission prompts are the friction: every Bash command, every file edit,
every MCP tool call asks before it runs. For trusted patterns (`git status`,
`cargo check`, edits to `*.md`, `Read` on anything in the project) the
prompt is pure tax — and it interrupts flow.

The user wants two things:

1. **Auto-approve** a curated set of action patterns so trusted operations
   don't pause for input.
2. **Keep a log** of what got auto-approved, when, so trust is verifiable
   after the fact (not "did claude really only touch what I expected?").

## Vocabulary

| Term         | Refers to                                                                |
|--------------|---------------------------------------------------------------------------|
| Allow rule   | A pattern in an agent's native settings file that bypasses prompting.    |
| Deny rule    | A pattern that bypasses prompting in the *opposite* direction (refuse).  |
| Trust list   | The combined effective allow set for the active project, per agent.      |
| Action log   | Chronological record of agent tool invocations, marked auto vs manual.   |
| Quick-allow  | The "remember this one" affordance after manually approving a prompt.    |

## Architectural choice (decided)

**Curate each agent's native permission system; do not intercept the pty.**

Considered and rejected: **pty interception** — spyc reads the pane output,
pattern-matches the prompt, injects the approval keystroke. Tempting
because it works across all agents with one code path, but the failure
mode is *silent wrong-approval*: any upstream change to the prompt format
breaks our matcher, and the safest fallback (don't approve) is the worst
UX (constant misses). Security features should not be built on regex
against another tool's UI.

The chosen approach:

- spyc reads and writes each agent's official settings file when the user
  edits rules — Claude's `.claude/settings.json`, Codex's
  `.codex/config.toml`, Gemini's (TBD — see Open Questions).
- The agent itself decides whether to prompt. spyc never intercepts.
- For the log, spyc reads each agent's transcript files (we already do
  this for session resume) and surfaces a unified view.

Wins on this path ride on each agent's wins. Losses are bounded to what
they support.

## Per-agent specifics

### Claude Code

**Settings location:** `.claude/settings.json` (per-project) plus
`~/.claude/settings.json` (user-level). Enterprise managed-settings exist
on top — we already detect that in `src/mcp.rs` and skip writing local
config when active. Same precedence applies here.

**Shape (per Claude Code docs):**
```jsonc
{
  "permissions": {
    "defaultMode": "ask",          // "ask" | "acceptEdits" | "bypassPermissions"
    "allow": [
      "Bash(git status:*)",
      "Bash(cargo check)",
      "Edit(**/*.md)",
      "Read"
    ],
    "deny": [
      "Bash(rm -rf:*)",
      "Bash(git push:*)"
    ]
  }
}
```

Patterns: tool name with optional argument matcher in parens. Suffix `:*`
means "any args after this prefix." Bare tool name means "all uses of
this tool."

**Transcript location:** `~/.claude/projects/<slug>/*.jsonl` — already
parsed in `src/state/sessions.rs::find_claude_sessions`. Tool-use entries
include the tool name, inputs, and (in newer claude-code versions) a
`permission_decision` field marking auto-approved vs manual.

### Codex

**Settings location:** `~/.codex/config.toml` (user-scope) and
`<project>/.codex/config.toml` (project-scope). Same precedence pattern
as Claude. spyc already writes the MCP block here (`src/mcp.rs:676`+).

**Shape (per Codex docs — needs verification before implementing):**

Codex's permission model is approval-mode based: `read-only`,
`auto-edit`, `full-auto`, plus an allow-list keyed under
`[approvals]` or similar. The exact TOML shape needs a doc fetch
before we write to it — putting it on the verification list.

**Transcript location:** `~/.codex/sessions/<uuid>.jsonl` — partially
parsed today; tool-use schema needs confirming.

### Gemini CLI

**Settings location:** **NEEDS VERIFICATION** — Gemini's permission
config story has been moving. As of last check, `~/.gemini/settings.json`
exists but the permission knobs aren't yet stable. We may need to ship
"read-only log" for Gemini in v1 and add rule editing in a follow-up.

**Transcript location:** `~/.gemini/tmp/<project>/chats/*.jsonl` —
parsed today in `find_gemini_sessions`. Each JSONL's first line is
metadata; subsequent lines are turns. Tool-call entries embedded
inside turns; schema needs walking.

### Unified abstraction

```rust
// src/state/auto_approval/mod.rs
pub trait AgentApprovalProvider {
    /// Path to the per-project settings file for this agent.
    fn project_settings_path(&self, project_root: &Path) -> Option<PathBuf>;

    /// Path to the user-scope settings file.
    fn user_settings_path(&self) -> Option<PathBuf>;

    /// Parse current allow/deny rules.
    fn read_rules(&self, settings_path: &Path) -> Result<RuleSet>;

    /// Walk transcripts under `project_root` (or all projects, if None)
    /// and emit `ActionEntry`s in time order.
    fn read_actions<'a>(
        &'a self,
        project_root: Option<&Path>,
        since: Option<SystemTime>,
    ) -> Box<dyn Iterator<Item = ActionEntry> + 'a>;
}
```

One impl per `AgentKind`. The dispatch is keyed by `SavedTab.agent_kind`,
which we already track.

## UI surfaces

### `:approvals` — action log pager

A new pager view, opened by `:approvals` (or a binding TBD). Layout
mirrors the task viewer:

```
                                    Action log — last 24h ▾    [filter: all]
───────────────────────────────────────────────────────────────────────────
  09:14:22  claude   AUTO   Read  README.md                                
  09:14:24  claude   AUTO   Bash  git status                               
  09:14:31  claude   MANUAL Edit  src/app/mod.rs        (diff: +12 -3)     
  09:14:48  codex    AUTO   Bash  cargo check                              
  09:14:52  claude   MANUAL Bash  cargo test --lib                         
  ...                                                                     
```

- **Columns:** timestamp, agent, AUTO/MANUAL, tool, target (concise).
- **Sort:** newest first (matches our other viewers).
- **Filters:** `f a` agent (claude/codex/gemini/all), `f t` tool,
  `f m` mode (auto / manual / both), `f /` substring on target.
- **Detail view:** Enter on a row opens a pager with the full
  tool-call JSON (inputs, outputs, decision metadata).
- **Quick-deny:** `D` on a row that was auto-approved adds a `deny`
  rule for that exact pattern and rewrites the settings file
  (with a confirmation flash).
- **Quick-allow:** `A` on a row that was manually approved offers
  the inferred pattern (`Bash(cargo test:*)` from `Bash(cargo test --lib)`)
  and adds it on confirm.

This is a `PagerView` with a custom row renderer. Streaming refresh on
new transcript entries (tail mode), like `:grep` already does.

### Editing rules — `:approvals edit` / `:perm`

```
:perm                  # opens the active project's claude settings
:perm user             # opens ~/.claude/settings.json
:perm codex            # opens .codex/config.toml in active project
:perm codex user       # opens ~/.codex/config.toml
:perm gemini ...       # symmetric, pending Gemini's settings shape
```

Behavior:
- Resolves the file path (creating with a template if it doesn't exist).
- Tears down the TUI, calls `$EDITOR <path>`, restores on exit.
  (Same pattern as `V` for editing; reuses `suspend_tui`.)
- After the editor exits, runs a parse/validate pass on the file and
  flashes a one-line summary: `"+2 allow, +1 deny"` or
  `"settings: invalid JSON — see <path>:<line>"`.

We deliberately don't build a custom rule-editor widget. JSON / TOML
in vim with a known schema is fine, and the maintenance cost of a
bespoke widget is much higher than the typing tax of editing the file.

### Quick-allow from a pane — `^a-A`

When the user is staring at a permission prompt in a claude/codex/gemini
pane and types `^a-A` instead of `1`/`y`:

1. Scrape the most-recent prompt from the pane's scrollback (look for
   the prompt sentinel — each agent has one, e.g. claude's "Do you want
   to allow…" line).
2. Parse out the tool + target.
3. Show a one-line spyc prompt: `add allow Bash(git push) — y/n?` with
   the proposed pattern editable inline (vi line editor).
4. On confirm, append to the active project's settings file (creating
   if missing), flash `"added allow Bash(git push)"`, and let the user
   keep pressing `1` to approve the current prompt manually.

The rule applies starting from the *next* tool call — we don't try to
race-approve the in-flight prompt. Simpler, safer, no race.

### Status-bar surface

A small `auto:N` segment in the status bar showing the count of
auto-approvals in the current spyc session. Click-equivalent: `:approvals`
to drill in. Resets at quit. Optional — keep behind a config knob if
the noise is too much:

```toml
[approvals]
status_counter = true
```

## Configuration

New `[approvals]` section in `.spycrc.toml`:

```toml
[approvals]
# Default agent for :perm with no argument. Active pane's agent
# overrides this; falls back here when no pane is open.
default_agent = "claude"      # "claude" | "codex" | "gemini"

# Show the auto-approval counter in the status bar.
status_counter = true

# Look-back window for the default :approvals view. Older entries
# are still browseable via :approvals --all.
log_window_hours = 24
```

(Mirrors the per-section shape we already use — `[yank]`, `[markdown]`,
`[pane]`, `[layout]`.)

## Compatibility

- **Older agents** (versions that pre-date permissions or use a different
  schema): spyc detects on read, flashes once per session, falls back to
  read-only mode (log surface still works; rule editing disabled).
- **Enterprise managed-settings**: already detected in `src/mcp.rs`. If
  Claude's managed-settings disables local-config, we honor that and
  refuse `:perm` writes with a clear error.
- **No agent installed**: surface available for whichever subset *is*
  installed. The unified log iterator just skips missing agents.

## Phases

Six phases, smallest-first so we can ship value early.

### Phase 0 — Read-only log for Claude
- `AgentApprovalProvider` trait + Claude impl.
- Walk `~/.claude/projects/<slug>/*.jsonl`, emit `ActionEntry`s.
- `:approvals` pager view (no filters yet, no edit hooks).
- Single agent, single project. ~2 days.

### Phase 1 — Multi-agent log
- Codex + Gemini providers (read-only; verify schemas).
- Filter chips (agent / tool / mode / substring).
- Cross-project view (`:approvals --all`).
- ~2 days.

### Phase 2 — `:perm` editor
- File-path resolver for each agent + each scope (project / user).
- Tear-down → `$EDITOR` → tear-up.
- Post-edit validation pass with one-line summary flash.
- ~1 day.

### Phase 3 — Quick-allow from pane
- Pane scrollback scrape for the most recent prompt.
- Per-agent prompt sentinels (parsed, not regex'd against UI).
- Editable proposed pattern + append-on-confirm.
- ~2-3 days (the trickiest piece).

### Phase 4 — Quick-deny / quick-allow from `:approvals`
- `D` / `A` chords on rows in the log view.
- Pattern inference from concrete action.
- ~1 day.

### Phase 5 — Status-bar counter
- New segment between git and agent badges in `src/ui/status.rs`.
- Resets at quit; lifetime count optional behind config.
- ~half a day.

## Open questions

1. **Codex's permission schema** — exact TOML shape needs a doc fetch
   before Phase 2 can write to it safely. Worth a half-hour of reading
   their docs / source before committing to the writer.

2. **Gemini's permission schema** — same. Likely the slowest piece;
   we can ship read-only log for Gemini in Phase 1 and skip Phase 2
   for it if the surface isn't stable.

3. **Auto-detection vs explicit `:perm` arg** — when the user types
   `:perm` with the active pane being claude, we open claude's file.
   What if they're in a non-agent pane? Fall back to `default_agent`
   from config, or refuse and require explicit arg?

4. **"Allow rule too broad" guard** — should we warn when adding a rule
   that obviously casts a wide net (`Bash(*)` or `Bash(rm:*)`)? Felt
   important during planning but might be paternalistic. Default off,
   opt-in via `[approvals] warn_on_broad = true`?

5. **Live reload** — if the user `:perm`s and changes the settings,
   does claude pick up the change without restart? Per claude-code
   docs, settings reload between turns. Codex: TBD. Gemini: TBD. If
   any of them require a restart, `:perm` should offer to `^a-R` the
   pane afterward.

6. **Rule history / undo** — when a quick-allow adds a rule, can the
   user undo it? Simplest answer: the action log records the rule
   add as its own row (different `tool` value: `Rule(allow:added)`),
   and `D` on that row removes it. Reuses the log surface; no new
   undo stack.

7. **Cross-project rules** — sometimes you want `Bash(cargo test:*)`
   allowed *everywhere*, not per project. `:perm user` covers it
   (user-level settings), but the discovery flow when adding a rule
   from a pane isn't obvious — quick-allow defaults to project scope;
   should we surface a "user-scope?" toggle in the confirmation
   prompt?

## Non-goals

- **Pty-level interception** — explicitly rejected (see Architectural
  choice).
- **Live editing of in-flight prompts** — we add rules for *next* time;
  the current prompt always requires the user's manual approval.
- **Auto-approval based on directory or filename only** — we use each
  agent's native pattern syntax verbatim. If they don't support
  `Edit(src/**.rs)`, we don't fake it.
- **Cross-agent rule unification** — each agent owns its own rule list.
  We don't try to sync a single "trust list" across all three; the
  schemas don't line up and the synchronization edge cases multiply.

## Risks

- **Settings format drift in any of the three agents** — same risk as
  the v1.60 hub plan's "older spyc peer" matrix. Mitigation: parse
  defensively, validate after write, fall back to read-only when we
  can't parse.
- **Log volume on chatty agents** — a busy claude session can do 50+
  tool calls in an hour. The 24h-window default and the filter chips
  exist for this; if it's still too much, paginate.
- **User confusion: "spyc auto-approved this!"** — easy to think spyc
  is doing the approval when it's claude. The `:approvals` view labels
  each row with the agent name and the rule source. The status-bar
  counter is per-spyc-session so the user sees "this session" not
  "claude's history."

## Out of scope (deferred to a follow-up)

- A **rule recommender** that watches your manual-approval pattern and
  suggests rules ("you've approved `cargo check` 12 times in 30 minutes
  — add allow rule?"). Plausibly useful, definitely a different shape;
  punt to a separate plan once Phase 0–4 are stable.
- A **CounterTop hub integration** showing aggregate approval activity
  across every running spyc. Natural pairing with v1.60; punt to the
  hub plan once both pieces exist.
