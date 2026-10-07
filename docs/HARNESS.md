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
| `[pane] codex_daemon` | `false` (default) | spyc starts codex panes with `--no-daemon`. See "codex's shared daemon" below. |
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
so it is never in spyc's terminal scrollback. codex's history isn't either — its
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
| otherwise | terminal capture |

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

Codex scrollback accepts both legacy message/function-call events and current
`item_completed` user/agent, command and MCP items, plus custom-tool calls.
Structured content contributes text only; wire user-message instructions and
reasoning records are not conversation prose. Records with the same item/call
identity and kind appear once within a session; identical text with different
identities still appears each time. Calls and results remain separate entries,
and `t` hides both. Distinct wrapper and nested-tool identities are not guessed
to be duplicates.

**A forked codex thread's rollout holds only its own turns.** Its
`session_meta.history_base` names the parent thread and the byte offset in the
parent's rollout where the fork branched, instead of copying what came before.
So `^a v` on a fork reads the parent's rollout up to that offset first, and
further back for a fork of a fork (`src/state/codex_history.rs`). Without that
walk a branch would show none of the history it was forked to keep.

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

### Forking a tab (`^a F`)

| agent | fork |
|---|---|
| **claude** | a new tab running `claude --resume <id> --fork-session`. That's the `--resume` flag restore avoids, because the typed `/resume` has no fork form; on claude 2.1.284 it mounts cleanly with a long conversation behind it. The branch writes nothing to disk until its first prompt, and its first hook report carries the branch's own session id, not the parent's. |
| **codex** | a new tab running `codex fork <uuid>`. The branch writes its rollout straight away, with a new id and a start time at the fork, so spyc pins it like any fresh codex tab. |
| **agy**, **zot** | none. Each can resume a conversation but not branch it, and two tabs on one conversation would be two clients of one session, not a fork. `^a F` says so and opens nothing. |

A branch starts in the directory its parent was started in, where each agent
looks its session up, whatever directory the pane has wandered to since. A
tab running anything else has no conversation, so its fork is a copy of its
command, opened at the tab's live cwd.

### The codex quirk that confuses everyone

**Activity diagnostics distinguish setup from execution.** `:activity dump`
shows the last received semantic report, hook-file marker presence, whether
hooks existed at launch, and known config changes that require a restart.
An MCP connection or hook file alone does not prove hooks ran. After first
consenting to spyc's hooks, restart Codex and review `/hooks` and project trust.
New/changed hooks need review; spyc does not authorize them for you. Codex and
Claude reports survive redraws and quiet tool waits until TTL expiry or a newer
report; Codex cancellation reports `idle` through `Interrupt`. Without a live
report, timing-only `idle` means silence, not confirmed completion.

The dump includes up to eight reported hook-event summaries per pane. Event,
tool and available turn/call ids help diagnose permission transitions without
copying command arguments or conversation content. Metadata is unverified and
does not change the configured status. Older reporter binaries still report
status but omit event metadata: for a local test build, put its directory first
on `PATH` before launching spyc so hooks invoking `spyc` use that build too.
The pane's interactive shell can reorder `PATH` during startup. If reports
arrive without metadata, check which `spyc` that shell resolves; launching the
test with `SHELL=/bin/sh` can preserve the test directory's precedence without
editing shell configuration. This temporarily changes the pane launch shell,
not the user's permanent shell settings.
`--status-trace` records sanitized summaries, not raw hook payloads.

**Restore and fork retain Codex's launch settings**, including model, profile,
sandbox, approval policy, working directory, repeated config overrides and
extra writable directories. General options on either side of `resume` or
`fork` are preserved, with their quoting and shell expansions intact. Only the
old subcommand, selector and picker switches are removed; the captured session
id supplies the new selector. An option value such as `resume` is not mistaken
for a subcommand, and a fork's parent id is never pinned as its own conversation.

Automatic reconstruction supports a bare or path-qualified `codex` executable
and recognized interactive options. Shell operators, substitutions, wrappers,
unknown options, repeated singleton options, initial prompts and initial image
inputs require a manual reopen rather than a guessed launch or replayed input.
A refused fork opens nothing. Restore preflights every tab: an unsupported
Codex command refuses the entire restore before replacing existing tabs or
session identity, and names the affected tab and reason. The saved command is
retained verbatim when it cannot be normalized.

**A resumed codex session appends to its original rollout file and leaves
`session_meta` frozen at the original creation time.** So a rollout created a
month ago can be the live one, and its recorded start time tells you nothing
about which session is current — only the file's mtime does.

That is why identifying "which rollout belongs to this pane" is genuinely hard,
and why two codex panes in the same directory is the case that breaks it. See
**#230**; the fix ranks a *fresh* pane by start-time proximity (its rollout
necessarily starts when the pane does) while keeping mtime primary for a
resume-without-id, because those are opposite tells.

### Codex question-tool recovery

Codex's `request_user_input` tool has narrowly matched `PreToolUse` and
`PostToolUse` reporters. A start reports `blocked`; successful completion
restores `working` only when pane, session, turn and call match a pending
question. Other questions or uncorrelated blocked reports retain attention.
Enter alone does not clear an identified question, and unrelated or late
completions are recorded as unapplied with a reason in `:activity dump`.

Correlation is bounded to eight pending questions per pane, with overflow
remaining blocked until a lifecycle or explicit agent report. It is reset for
replaced panes and never stores question arguments or answers. The hook wire
values require a capable host and metadata-bearing reporter; older components
cannot silently apply an uncorrelated completion. Restart Codex and review both
new hooks in `/hooks`; spyc never approves them or edits trust records.

This instruments the question tool's attempt/completion, not authenticated
proof that a dialogue opened. Invalid/cancelled calls may have no post event;
`Stop`, `Interrupt` or an explicit newer report retires the wait. Correlated semantic
permission completion cannot be correlated because `PermissionRequest` has
no call id. It runs before hook decisions, automatic review or a human dialogue,
as shown by the exact-version
[permission event](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/hooks/src/events/permission_request.rs)
and [review path](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/core/src/tools/approvals.rs).
Metadata-bearing permission reports are recorded as observational
(`not applied: PermissionRequest precedes review; human wait unverified`). They
cannot latch blocked or interfere with a later question. The existing hook
command remains unchanged, preserving its trust hash; the host and reporter
on `PATH` must both support lifecycle metadata.

Command approvals use a narrow fallback: a complete default approval dialogue
at the bottom of the current viewport, after 250 ms of quiet, temporarily
overrides a non-blocked report. `:why-status` and `:activity dump` identify
`scrape-fallback` as the source. The silent-work report remains stored and
resumes after the dialogue closes. Clipped/customized dialogues and other
approval types are not inferred. This is verified UI detection, not semantic
permission completion. Explicit agent blocks retain their ordinary input
recovery; identified questions require matching completion instead of Enter.
Real-CLI native question and quiet-after-answer checks passed through the
automated TUI harness with already trusted hooks. Fresh hook trust onboarding
remains a separate acceptance case.

`request_user_input_async` is a separate question path. Its immediate completion
acknowledges posting a question; it does not establish that the user answered.
These native question hooks do not instrument that path. The
[`0.160.1` async handler](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/core/src/tools/handlers/request_user_input_async.rs)
posts an async message and immediately returns `accepted`; the
[TUI reply parser](https://github.com/openai/codex/blob/rust-v0.160.1/codex-rs/tui/src/async_question_reply.rs)
handles the answer as a later user message. A blocked latch tied to the tool's
completion would misrepresent an agent that continues working. Async-answer
lifecycle coverage remains open in A3.

For a native-hook test, restart Codex after any restart-needed diagnostic and
review the new hooks in `/hooks`. Enter `/plan` as an actual CLI slash command
before asking for the diagnostic question. Confirm that `:activity dump` records
`PreToolUse` for `request_user_input` while waiting, then a matching `PostToolUse`
after answering. An async question call does not exercise this transition.

### Automate Codex attention smoke tests

`scripts/codex-tui-smoke.py` drives the installed Microsoft `tui-test` CLI with
its Ghostty backend. It creates a dedicated named session, launches the supplied
spyc binary, opens a real Codex pane and captures the UI and `:activity dump`.
The driver stays alive while its daemon runs because tool runners can reap
detached descendants when their launching command exits. It closes only its
own session. The output directory must be new for each run.

```sh
python3 scripts/codex-tui-smoke.py --binary /absolute/path/to/spyc --output /tmp/spyc-codex-question-run
python3 scripts/codex-tui-smoke.py --binary /absolute/path/to/spyc --output /tmp/spyc-codex-approval-run --scenario approval
python3 scripts/codex-tui-smoke.py --binary /absolute/path/to/spyc --output /tmp/spyc-codex-mixed-run --scenario mixed
python3 scripts/codex-tui-smoke.py --binary /absolute/path/to/spyc --output /tmp/spyc-codex-auto-run --scenario auto
python3 scripts/codex-tui-smoke.py --binary /absolute/path/to/spyc --output /tmp/spyc-codex-parallel-run --scenario parallel
```

The question scenario selects Plan mode and asserts blocked while the native
question is open, a single matching start/completion pair, authoritative working
after the answer and during `sleep 30`, then a single Stop reporting done.
Final text and the Stop reporter are asynchronous, so it waits for both. Checks
read the current viewport rather than stale scrollback. The binary hash, step
log, captures, extracted dumps, recording and result are saved beside each run.

The approval scenario uses a test-only CLI invocation with user-reviewed
approvals and a read-only sandbox. It approves only the displayed `sleep 30`
command once, verifies the visible-dialogue fallback and retained working
report after Enter, then done. This does not claim correlated semantic
permission completion. The mixed scenario
first approves the displayed `sleep 1` once, then answers a native question in
the same turn. It checks matching completion, authoritative working during
`sleep 30` and done. It catches stale approval state poisoning question recovery.
The driver waits for diagnostic text to render before submitting it, preventing
Codex's paste-burst handling from leaving a long prompt in the composer.
The auto scenario requests the same harmless sleep under automatic review,
presses no approval key, and checks that the permission observation cannot
latch blocked. The parallel scenario opens two Codex panes in one worktree,
holds both native questions open, answers the second first, then the first.
It checks distinct pane/session/turn/call identities, matching completions,
quiet working and done without recovering the unanswered pane.
None of the scenarios approves hook trust or changes trust records. Hooks
must already be trusted; fresh onboarding is a separate manual acceptance case. These real-model smoke
tests run on demand rather than in CI.

`scripts/codex-hook-readiness-smoke.py --output /tmp/spyc-codex-fresh-run`
creates a temporary project and empty `CODEX_HOME`, then reads `hooks/list`
without starting a turn or approving trust. It verifies that project hook
declarations remain unavailable and Codex explains the untrusted-project
gate. This negative discovery case does not establish trusted hook execution;
the full onboarding flow remains a separate acceptance case.

### Duplicate Codex hook sources

Codex loads `.codex/hooks.json` alongside inline hooks in `.codex/config.toml`.
If both contain spyc reporters, each matching event can report twice. At
installation, spyc migrates its `--report-status` handlers out of the JSON file
and keeps its reporters in TOML. Other handlers, matcher fields and trust records
are preserved. An emptied legacy file is removed; user-only files are not
rewritten. `:hooks off` and last-owner cleanup also remove legacy reporters.

Legacy files containing tracked reporters, or malformed/unreadable files,
block migration; `:activity dump`
names the source and repair action. Use `:hooks on` to migrate a valid untracked
file, then restart Codex and review `/hooks`. A running CLI may retain its old
hook definitions, so removing a duplicate file does not establish single
delivery before restart. A JSON-only change also sets the restart diagnostic.
Migration does not approve hooks or infer that a pending question or permission
request was answered.

### Codex hooks in linked worktrees

Codex takes project hook definitions from the corresponding root-checkout
directory, while ordinary config and the MCP entry stay worktree-local. spyc
therefore installs and refcounts those hooks at the root-checkout source, using
that project's saved consent. `:activity dump` names the actual source for a
linked worktree; a reporter marker in the worktree's ignored file is insufficient.
After a hook change, restart Codex and review `/hooks` from the new session.
Native `request_user_input` has exact start/completion hooks; the async question
tool's immediate acknowledgement is not an answer signal.

### codex's shared daemon

**Recent codex (seen on 0.158.0) runs sessions on one shared background server,
`codex app-server --managed-daemon`, started by the first codex you launch.**
That server spawns every session's MCP servers and hooks, and gives them its
own environment, not the environment of the codex in the tab. spyc's status
hooks and MCP proxy find their spyc and their tab through `SPYC_MCP_SOCK` and
`SPYC_PANE_ID` in the pane's env, so on the shared server they all get the
values of whichever pane happened to start it. Reports go to a spyc that has
since exited, or light the wrong tab's dot.

So spyc starts codex panes with `--no-daemon` (added after the program, the
tab keeps what you typed). `[pane] codex_daemon = true` turns that off, at the
cost of the dots and MCP attribution. A codex started outside spyc is
unaffected either way.

### The autosave window

Beyond the save on quit, spyc keeps a debounced crash-sufficient autosave —
**2 seconds** after any tab/cwd/vsplit/geometry change
(`AUTOSAVE_DEBOUNCE`, `src/app/session.rs`). A `SIGKILL` therefore loses at most
that window, not everything since launch. It's armed only while dirty, so an
idle spyc still does no work.

---

## 5. Multiple spyc instances

Instances coexist — the MCP server uses a **PID-scoped** Unix socket
(`~/.local/state/spyc/mcp-<pid>.sock`), and an agent's config entry names no
instance, so every agent reaches the spyc that launched it.

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

## Child input backpressure

A stopped or non-reading child must leave spyc navigation, tab controls and
pagers responsive. Child input is queued in order off the UI thread. The input
queue is bounded to 512 waiting batches and 8 MiB including the in-flight write.
If it fills, spyc reports that the new input was not sent; retry after the child
resumes reading. Accepted input can remain queued while the child is stopped.
Closing its tab retains the normal process-group shutdown.

`scripts/pane-input-smoke.py` uses a dedicated `tui-test` session and a disposable
raw-mode child that does not read stdin. A synthetic paste fills the PTY while
spyc's own activity pager must still open. It also checks capture input and a
pane producing continuous output. A failure can capture a macOS stack sample
before killing the owned test child. It never drives the user's current pane.
