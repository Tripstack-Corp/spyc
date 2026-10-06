# feature-shell-tab-agent-tracking

Topic: feature-shell-tab-agent-tracking
Status: OPEN
Ball: Claude Code (calebjacksonhoward)

---
Entry: Claude Code (calebjacksonhoward) 2026-10-06T10:25:56Z
Index: 0
Role: planner
Type: Note
Title: CHARTER — Shell-tab agent tracking (GitHub issue 544)

Spec: planner-architecture

# Charter — track known agents launched inside shell tabs

**Source of requirement:** GitHub issue Tripstack-Corp/spyc#544, "Track known agents launched inside shell tabs, then return to shell identity" — https://github.com/Tripstack-Corp/spyc/issues/544 — opened by Derek Marshall (repo owner) 2026-10-06. Originating conversation: Slack, Derek ↔ Caleb, about Codex tabs launched from `bash` showing no activity LED (Caleb: "I was referring to codex showing the LED in any context. Being able to set it in the tab config would be dandy as well.").

**Status of authority:** there is NO ratified implementation plan yet. The issue explicitly says "Defer implementation design." This thread therefore opens in a RESEARCH phase. Any implementation plan drafted here is a proposal until ratified by a recorded Decision from the human authority (Derek as repo owner; Caleb as requester). Governing protocol: Intent-Alignment Protocol v1.1, intent-alignment-canon:0 (01KY9E1522TCHESYTNNTG4FNP4) in mostlyharmless-ai/watercooler-cloud.

## Issue 544 — quoted verbatim

> Users who launch Codex or another known agent from an existing `bash`, `zsh` or other shell tab want the agent identity and activity LED in that tab. When the agent exits, the tab should return to ordinary shell identity and behaviour, ready to relaunch an agent.
>
> Today, spyc selects the agent profile from the tab's launch command. Direct agent tabs and configured agent startup tabs cover fixed launches, but a tab launched as a shell stays classified as a shell.
>
> Acceptance targets:
>
> - Shell → known agent → shell → relaunch follows the live identity and the correct working/blocked/done activity.
> - Exiting clears stale agent status and session bindings; relaunching the same or a different supported agent binds the new session.
> - Concurrent panes retain their own MCP, hook and activity attribution.
> - Shell job control, suspend/resume and direct agent launches retain their existing behaviour.
> - Hook consent, trust review and Codex daemon isolation are respected.
>
> Defer implementation design. Questions to resolve include reliable foreground-process/lifecycle detection, nested shells and wrappers, and whether an explicit agent hint in startup-tab configuration is useful. Printed terminal text alone must not establish agent identity.
>
> Workaround: launch an agent directly with `^a c codex` or configure a direct agent startup tab.
>
> This is a separate feature request from the current Codex compatibility/A3 hook work; it does not expand that slice or its local-build acceptance gate.

## Scope fences (from the issue text)
- Does NOT expand the Codex compatibility / A3 hook slice or its acceptance gate.
- Printed terminal text alone must not establish agent identity (no screen-scrape identity).

## Working loop (drift prevention + intent calibration)
1. **Research** — answer the issue's three open questions with evidence (code citations, observed process state). Posted as Note entries.
2. **Plan** — a proposed design entry, explicitly mapping each acceptance target to a mechanism.
3. **Calibrate** — anything exceeding the issue's text (new env vars into shell tabs, new config keys, new trust surfaces) is opened as a `RESERVED —` Decision with the ball on the human; nothing proceeds past it unratified.
4. **Record** — each PR gets a `CONFORMANCE — PR #N` entry at PR-open; each deviation gets its own entry when it appears; the ORIENTATION entry is refreshed at every phase boundary.

## Provenance index
- Requirement: Tripstack-Corp/spyc#544.
- Protocol: intent-alignment-canon:0 (01KY9E1522TCHESYTNNTG4FNP4).
- Related code areas: `src/agent/` (profile registry), `src/app/pane_tabs.rs`, `src/proc_cwd.rs`, `src/app/codex_pin.rs`, `src/mcp/hooks.rs`, `src/app/status_hooks.rs`; docs `docs/AGENT_ORCHESTRATION.md`, `docs/HARNESS.md`.
- Program spine: none exists for spyc; this thread is standalone. Predecessor stage: none.
- Branch at open: `feat/pane-startup-tabs-gh` @ c8ea156 (research only; no code change).

## Closure criteria
1. The three open questions in the issue each have an evidence-backed answer on this thread.
2. A design plan is ratified by recorded Decision (or the feature is explicitly declined/deferred by Decision).
3. Implementing PR(s) are merged with conformance entries, all five acceptance targets demonstrated, and a Closure entry posted referencing the merged PR(s).

<!-- Entry-ID: 01M48BZ7TDCKKA5QTKG9N7YDC8 -->

---
Entry: Claude Code (calebjacksonhoward) 2026-10-06T10:26:11Z
Index: 1
Role: planner
Type: Note
Title: Conformance ledger — seed

Spec: planner-architecture

# Conformance ledger — seed

Entries below this one whose title begins "CONFORMANCE —" form this stage's conformance ledger (Intent-Alignment Protocol v1.1, Part 1 practice 1, intent-alignment-canon:0 (01KY9E1522TCHESYTNNTG4FNP4)). One per PR (or tightly coupled PR set), posted at PR-open, citing the ratified plan entry + section it implements, the items covered, and its deviations — each deviation having its own entry, born when it appears. "None" is a claim. Until a plan is ratified on this thread, no CONFORMANCE entry can be valid, because there is nothing to conform to.

<!-- Entry-ID: 01M48BZQAKE5D9A9YVH6K4T7YH -->

---
Entry: Claude Code (calebjacksonhoward) 2026-10-06T10:26:33Z
Index: 2
Role: planner
Type: Note
Title: ORIENTATION — research phase open

Spec: planner-architecture

# ORIENTATION — research phase open (2026-10-06)

- **Ratified:** nothing for this feature. Requirement source is Tripstack-Corp/spyc#544, quoted in feature-shell-tab-agent-tracking:0 (01M48BZ7TDCKKA5QTKG9N7YDC8), "CHARTER — Shell-tab agent tracking". The issue defers implementation design.
- **Landed:** nothing.
- **In flight:** research into the issue's three open questions (foreground-process/lifecycle detection; nested shells and wrappers; explicit agent hint in startup-tab config) — findings to follow as the next entry.
- **Reserved:** none opened yet. Expected candidates once research lands: whether shell tabs get per-tab MCP/hook attribution env, and whether a startup-tab agent hint key is added.

<!-- Entry-ID: 01M48C0CMNR07TGPV5B6QJD6PJ -->

---
Entry: Claude Code (calebjacksonhoward) 2026-10-06T10:30:02Z
Index: 3
Role: planner
Type: Note
Title: RESEARCH — issue 544 open questions: findings and design op…

Spec: planner-architecture

# Research findings — issue 544's open questions

Charter: feature-shell-tab-agent-tracking:0 (01M48BZ7TDCKKA5QTKG9N7YDC8), "CHARTER — Shell-tab agent tracking (GitHub issue 544)". Everything under **Observed** was checked in this session, either against code or against live process state. Anything under **Proposed** is a design option only; none of it is ratified.

**Code baseline.** Local `feat/pane-startup-tabs-gh` @ c8ea156 is 51 commits behind `origin/main` @ e1b714a. Where main differs, citations are written `origin/main:`. Any implementation must build on main.

**Live specimen.** The main spyc instance (pid 132802) has two `bash` tabs, `[8]+ bash` and `[9]+ bash`. One of them is running Codex, which is the case Caleb reported in Slack.

## A. Observed facts

### A1. Shell tabs cannot attribute anything today
- Fresh-spawn env injection is gated on `is_agent` (`origin/main:src/app/pane_tabs.rs:185-189`):
  ```
  (true, Some(sock)) => SPYC_MCP_SOCK + SPYC_PANE_ID
  (false, _)         => nothing
  ```
- Live `/proc/<pid>/environ` confirms this:
  - The direct `claude` tab 132844 has `SPYC_MCP_SOCK`, `SPYC_PANE_ID` and `SPYC_CONTEXT`.
  - The `bash` tab 132930 has **only** `SPYC_CONTEXT`.
  - The Codex processes launched inside that bash tab inherit only `SPYC_CONTEXT`.
- The hook reporter `report_status_to_socket` (`src/mcp/mod.rs:250-345`) returns silently when the socket env is empty. So a shell-launched agent's hooks report nothing.
- Even if a report did arrive, `effective_activity` returns `Unknown` for a non-agent tab (`src/app/agent_status.rs:422-424`), and the tab bar draws no dot for `Unknown` (`src/app/render/chrome.rs:121`).
- **The missing activity LED is therefore three gaps, not one:** identity, attribution and display.

### A2. Identity is decided once, from the launch command, and never stored
- `agent::detect(cmd)` checks only the basename of the first token (`src/agent/mod.rs:156-159, 837-871`).
- `TabInfo` has no kind field (`src/pane/tabs.rs:99-257`). Every consumer calls `detect(&info.command)` again:
  - `pane_tabs.rs:171`
  - `agent_status.rs:580`
  - `mcp.rs:517`
  - `session.rs:111`
  - `codex_pin.rs:144,215`
  - `commands.rs:205,593,739`
- `info.command` is never reassigned, so the identity is fixed for the tab's lifetime. A live identity would need either a mutable kind on `TabInfo` or a change at each of those call sites.

### A3. Foreground-process facility: a pgid only, read on demand
- `Pane::foreground_pgrp()` is tcgetpgrp via portable_pty (`src/pane/mod.rs:353-358`, `pty_host.rs:439-448`).
- Its only callers are `^z` suspend and capture `^C`.
- Nothing reads comm, exe or cmdline anywhere.
- The live-cwd poll reads `/proc/<child pid>/cwd`: the direct child, not the foreground group. It runs only for the active tab, from render `prepare_panes`, with a 1s TTL. There is no scheduler deadline for it.

### A4. Wrappers are the norm, not the edge case
The live Codex specimen in bash tab 132930 (session 132930, terminal foreground pgid 302278):
- pid 302278 is the foreground-group **leader**:
  - comm = `node`
  - exe = `…/node`
  - argv = `node …/bin/codex`
- pid 302289 is in the **same pgrp**:
  - comm = `codex`
  - exe = `…/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/bin/codex`

`which codex` resolves to an npm `codex.js` script. A check that looks only at the leader's comm or exe would see `node` and miss Codex.

By contrast, `claude` launched from bash (session 132935) is a native binary: comm `claude`, exe `~/.local/share/claude/versions/2.1.285`. The exe basename is the version number, not "claude", so **exe basename is also not a reliable match key**.

### A5. Daemon isolation is the deepest blocker for shell-launched Codex
- The shell-launched Codex TUI (pid 302289) has an established unix-socket connection to `/tmp/codex-daemon-1000/…`, the shared app-server daemon.
- That daemon's process tree sits under the user systemd/session manager, outside every spyc.
- Its MCP servers (uv/python children) run there.
- Hooks and MCP for that session are spawned by the daemon with **the daemon's** env, not the tab's. Main already fixes this for direct Codex tabs by appending `--no-daemon` (`origin/main:src/agent/mod.rs:504-513`, #510), with opt-out `[pane] codex_daemon`. **A command the user types into a shell bypasses that rewrite.** `codex --no-daemon` exists on the CLI (0.159.2 `--help`).
- **Stale context leak.** The live daemon (pid 1636772) carries `SPYC_CONTEXT=/home/caleb/.spyc-context-132802.json`, inherited from whichever spyc tab first started it. Every process the daemon spawns points at that one spyc, regardless of which spyc's tab is driving it.
  - This is a concurrent-pane attribution hazard in its own right (acceptance target 3).
  - It probably exists today for any daemon-mode Codex, so it is worth a separate issue.

### A6. Session bindings are never cleared on exit
- `live_session_id` and `codex_session_id` are only ever set:
  - `src/app/mcp.rs:516-526`
  - `src/app/codex_pin.rs:236`
  - `session.rs:520`
- Exit only appends ` [exited N]` to the label (`src/pane/tabs.rs:560-574`).
- Acceptance target 2 ("exiting clears stale agent status and session bindings") needs new clearing logic in every design.
- `codex_pin` claims a rollout by **tab spawn time**: start ≥ spawn−5s, within a `PIN_WINDOW` of 30s (`codex_pin.rs:25`). For an agent launched later inside a shell, the anchor has to be the detected agent-start time instead.

### A7. Startup-tab config has no hint field and rejects unknown keys
- `[[pane.tab]]` takes `command` / `cwd` / `label`, and `FilePaneTab` is `#[serde(deny_unknown_fields)]` (`src/config/mod.rs:263, 327`).
- An `agent = …` key would be a parse error today.

## B. Answers to the three open questions (proposed)

### Q1. Reliable foreground-process and lifecycle detection
**Recommended:** the foreground process group read through the OS, never the screen.
- **Identity.**
  - Read tpgid (the existing `foreground_pgrp()`), then enumerate the members of that pgrp.
    - Linux: scan `/proc/*/stat` for pgrp == tpgid.
    - macOS: libproc `proc_listpgrppids`, in line with the `ROADMAP.md:304` crate preference.
  - Match each member's **argv[0] basename and comm** against `AgentProfile::binary()`. This is the same rule as today's `matches_command`, applied to live processes. A4 shows that leader-only checks and exe basenames both miss real cases.
- **Lifecycle.**
  - The agent has started when tpgid moves off the shell's own pgid and a member matches.
  - The agent has exited when tpgid returns to the shell's pgid, or the matched pid is gone.
  - Under job control, `^z` inside the shell gives a stopped agent pgrp with tpgid back on the shell. That reads as "agent suspended, tab is the shell", not as an exit.
- **Cadence.** This must respect the 0-dps-at-idle invariant, so no free-running poll. Trigger a check:
  - on Enter forwarded to a shell tab;
  - on the first output burst after a quiet period;
  - on a single settle deadline after either of those.
  - Exit is caught by the same triggers, because an exiting TUI repaints the shell prompt.
- **Not used:** printed text, as the issue requires. No identity from screen scraping.

### Q2. Nested shells and wrappers
- **npm/node shim, `npx`, `uv run`, `bash -c 'codex'`.** The agent binary is in the foreground pgrp, as the leader or a member, and pgrp-member scanning covers it (A4).
- **Nested interactive shell** (`bash` → `zsh` → `codex`). A job-control shell puts its foreground job in a fresh pgrp and sets tpgid, so the outermost tcgetpgrp still lands on the agent's group.
- **Out of reach, and should stay "shell":**
  - `ssh host codex`: the agent is remote. A local member named `ssh` matches nothing.
  - tmux, screen or zellij inside a pane: the server is not in the pty's foreground group.
  - An agent launched via `exec` replacing the shell: the tab's child itself becomes the agent. That is detectable by the same scan, but the "return to shell" leg never happens; the tab simply exits.
- **Attribution is the hard part, not detection** (A1). An agent's env is fixed at exec from the shell's env, which is fixed at tab spawn. Two routes:
  1. **Inject `SPYC_PANE_ID` + `SPYC_MCP_SOCK` into shell tabs at spawn.**
     - Simple, and consistent with main's `spyc/paneId` binding (`origin/main:src/mcp/protocol.rs:80`).
     - It widens the socket-path exposure from agent processes to every command run in a shell tab.
     - The socket is per-user and pid-scoped, but this is still a trust-surface change. → **reserved**
  2. **Peer-pid attribution.**
     - spyc reads `SO_PEERCRED` (Linux) / `LOCAL_PEERPID` (macOS) on the MCP socket and walks the reporter's ppid chain to a tab's child pid. No env change at all.
     - The cost is per-OS code.
     - It cannot see through the Codex daemon, because the reporter's ancestor there is the daemon, not the tab.
- **Shell-launched Codex specifically needs `--no-daemon`, or its hooks never carry tab identity under either route** (A5). Options:
  - document `codex --no-daemon` as the required invocation;
  - a codex config-level opt-out, if Codex has one (**not verified**);
  - a shell alias that spyc offers to install.
  - Each of these is a user-environment change. → **reserved**

### Q3. Is an explicit agent hint in startup-tab config useful?
- **Yes, for direct tabs whose command is a wrapper `detect()` cannot see through.** Examples: `npx @openai/codex`, `./scripts/codex-dev.sh`, `env FOO=1 codex`. An `agent = "codex"` key on `[[pane.tab]]` would force the profile at spawn. This answers Caleb's Slack ask ("Being able to set it in the tab config would be dandy as well").
- **For shell tabs a hint is not needed** if Q1 detection lands. A per-tab `track_agents = true|false` toggle might be wanted instead, as the opt-in or opt-out for the env injection.
- Either way it is a new config key on a `deny_unknown_fields` struct. → **reserved**

## C. Reserved items surfaced
Each of these exceeds the issue's text and needs a human yes before design proceeds:
1. Inject `SPYC_PANE_ID` / `SPYC_MCP_SOCK` into shell tabs (trust surface), or use peer-pid attribution instead.
2. How shell-launched Codex gets `--no-daemon` (a user-environment change).
3. A new `[[pane.tab]]` key: an `agent` hint and/or a `track_agents` toggle.
4. Whether the stale `SPYC_CONTEXT` leak into the Codex daemon (A5) is filed as its own bug, outside this feature.

## D. Not yet verified
- macOS pgrp enumeration: not tested on this Linux box.
- Codex config-level daemon opt-out.
- Whether zsh/fish job control sets tpgid the same way bash does. This is expected from POSIX job control but was not observed.
- Behaviour of `claude` under `--resume` launched from bash.

<!-- Entry-ID: 01M48C6R5N7KPAW74NCSGYZV74 -->
