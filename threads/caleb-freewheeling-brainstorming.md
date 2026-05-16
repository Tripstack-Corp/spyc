# caleb-freewheeling-brainstorming — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: caleb-freewheeling-brainstorming
Created: 2026-05-16T08:11:09.476485+00:00

---
Entry: Claude Code (caleb) 2026-05-16T08:11:09.476485+00:00
Role: planner
Type: Note
Title: Seed: action×prediction, nav×agent×watercooler, and an agent's-eye wishlist

Spec: planner-architecture

## Frame

A freewheeling brainstorm thread for spyc — and specifically for the seam where **spyc's keyboard-driven file commander**, **the agent in the bottom pane** (claude/codex/gemini), and **watercooler's git-backed shared memory** meet. The thesis line worth keeping in view: *git stores what changed; watercooler stores why; **spyc is where the doing happens** — and the seam between doing and remembering is where the unfair leverage lives.*

What's already true (the substrate I'm building on, all live or roadmapped):

- Read tools: `get_spyc_context`, `get_file_content`, `search_paths`, `search_content`, **`search_picks`**, **`search_inventory`** (the last two are *spyc-shaped* — only the in-app multi-select and persistent cache make them possible). See `src/mcp.rs:914+`.
- Write tools: `navigate_to`, `set_filter`, `pick_files`, `clear_picks` — i.e. the agent can already move the user's workspace. `src/mcp.rs:1221+`.
- Bidirectional path refs (`gf`/`gF`), beam family (`^a s`/`P`/`i`), background tasks (`^Z`/`:fg`/`gB`), per-session `PROJECT_HOME` + spice-name session, picks/inventory/marks/harpoon/graveyard.
- Roadmapped: subscriber socket (push events over the PID-scoped Unix socket — replaces snapshot polling), generalized beam with named sinks, prompt templates in `.spycrc.toml`, autocommands, `get_session_cost`, v1.51 auto-approval + `:approvals` log, v1.60 *CounterTop* hub-over-spycs.
- On the WC side: smart_query/T1-T2-T3 (baseline → Graphiti → LeanRAG), federation, daemons emitting findings, decision extractor, semantic search, annotations, pulse snapshot, branch-scoped entries. The orphan-branch architecture means every entry already carries `Code-Repo`/`Code-Branch`/`Code-Commit` footers — so any WC entry can round-trip back to *exactly* the workspace state that produced it.

This seed is not throw-away — it's intended as a load-bearing first entry. I'll continue or refute its bullets in follow-ups; subscribed agents can pick any item and turn it into a Plan thread.

---

## A — Action × prediction across the spyc/agent boundary

The premise: spyc can *act* on the agent's behalf (the writable MCP tools), and the agent can *predict* what the user will want next. Today those two halves don't compose well — the agent's actions overwrite user state, and its predictions live trapped in the pane scrollback. The ideas below close that loop without giving up the user's authority.

### A1 — `!c` / `:c`: bang-to-claude from the navigator

Today: to talk to claude I focus the pane (`^a j`), type, hit enter — or use `^a s` to beam paths-only. There's no "prompt the agent without leaving the navigator." Add a captured-prompt variant of `!` whose sink is *the active pane tab* instead of a child shell:

- `!c <prompt>` — non-blocking; sends `<prompt>` to the active claude tab with a small auto-injected preface built from `get_spyc_context` (cwd, cursor, picks, harpoon, filter, branch). Counts compose: `5!c …` fanouts to the five most-recently-active claude tabs.
- `!C <prompt>` — blocking variant; renders a `[claude … 12s]` flash in the prompt slot until claude's next stop event arrives. Same lifecycle shape as the existing capture pager, with claude-as-the-process.
- `:c <prompt>` and `:cb <prompt>` — command-line variants; the latter beams the current scrollback selection as the payload.
- Composes with the existing prompt templates in `.spycrc.toml`: `!c@review %` expands to a named template with picks substituted.

Why it matters: the navigator becomes a programmable prompt launcher, not just a chooser of paths. The keystroke cost of "ask the agent about this" collapses from 4-6 keys to 1-2.

### A2 — Proposal lane: `propose_action` from claude into a navigator command rail

This is the prediction side. A new tool `propose_action(verb, args, reason, ttl)` lets the agent queue *suggested* navigator actions into a single-row ring buffer ("the rail") rendered in the prompt area. Each proposal shows verb + arg preview + a short reason chip; **Enter** executes, **Esc** dismisses, **Tab** cycles.

Examples claude actively wants today and can't have:

- After finding the failure site in test output: `propose_action("navigate_to", "src/fs/walk.rs:142", reason="likely failure from cargo test")`.
- After surveying a PR: `propose_action("set_filter", "*.rs", reason="touched files only")`.
- After answering "where's the auth code?": `propose_action("pick_files", ["src/auth/*.rs","src/middleware/auth.rs"], reason="3 files cited above")`.

The discipline: proposals are *inert* until the user accepts. Nothing surprises the workspace. This is the dial that lets the agent be much more forward without becoming intrusive — the failure mode of `pick_files`/`navigate_to` today is precisely "I didn't ask you to do that." Move it to a lane and the same actions become a gift instead of a clobber.

### A3 — Daemon-anticipated focus shift over the event stream

The roadmapped subscriber socket is the missing primitive that makes prediction real. With it: an agent-side daemon subscribes to `cd`/`pick`/`task_state`/`cursor` events, and when patterns fire — three errors against `src/foo.rs` from `cargo test`, or "user has been cursoring the same dir for 90 seconds" — it auto-pushes proposals into the A2 rail or pre-warms a `search_picks` and stages an `=*.rs` filter.

This is the daemon-anticipation case the brief asks for. Mechanically it's already a triangle of pieces on the roadmap: subscriber socket (publisher) + A2 (renderer) + `get_running_tasks` (introspection). The novelty is wiring them so the agent reacts to my workflow *as it happens* instead of every turn re-polling and reconstructing what I just did.

### A4 — Shadow picks: a claude-owned pick layer next to the user's

The current pick set is a single namespace claude can clobber with `pick_files`. Split it: `picks` (user) and `picks.shadow.claude` (agent). Render shadow picks with a *dim amber outline* to distinguish from the user's solid amber checks; the suffix segment grows a small chip — `[picks:0 +cl:7]`.

- `^t s` — sweep shadow into picks (accept).
- `^t S` — reject all shadow picks.
- `^t t` — toggle visibility.

Behavioral: claude's `pick_files` writes to shadow by default; an explicit `pick_files(..., commit=true)` writes to the real set (rare, requires the user has surfaced a config opt-in). This makes `pick_files` from claude *safe*. The current ceiling on how aggressively claude can curate scope is exactly this fear of clobber; the shadow layer raises that ceiling without removing any guardrail.

### A5 — Bidirectional marks: claude-set marks the user jumps to

Vi-style marks (`m{a-z}`, `'{a-z}`) are user-set, single-namespace. Generalize to two namespaces:

- `mC{a-z}` — claude-set marks, rendered in a distinct (slate) color in the suffix; set via new MCP tool `set_mark(name, path, line, reason)`.
- `'C{a-z}` — jump to claude-mark; flash shows the original reason chip.
- `:marks C` — pager listing of all claude-marks with reasons.

Use case: claude says "I've placed marks at the three sites that need updating — `mCa`, `mCb`, `mCc`. Visit them in order; the changes are small." I `'Ca`, edit, `'Cb`, edit, `'Cc`, edit — no path copy-paste, no focus switch. This is **prediction in the navigation dimension** specifically: the agent doesn't push my cursor; it deposits handles I can take when I'm ready. Composes with [[B4 mark↔entry]] below.

### Adjacencies (smaller but real)

- **A6 — Pane→pane beam relay over named sinks.** Roadmap mentions configurable beam sinks. Add sink = "pane tab N" and sink = "peer spyc PID M". `^a v` (scrollback in pager) → visual yank → `:beam !pane:2` becomes one motion; cross-instance becomes "teleport this thought to another agent in another workspace."
- **A7 — Claude confirms in the spyc confirm row.** Destructive proposals from claude bubble up into the existing in-prompt y/n confirm surface — same trust UX as `R` and quit — so audit lives in `:approvals` (v1.51) rather than in pane text.

---

## B — Nav × agent × watercooler

The substrate here is: **spyc is the cursor and the picks; watercooler is the orphan-branch memory that already knows what code commit each entry belongs to.** These two halves of "where I am" and "what I've decided" should be a closed loop. They aren't yet.

### B1 — Workspace pulse: auto-snapshot at inflection points

Subscribe a small "pulse" sink to the spyc event stream. On meaningful inflections — `project_home` change, tab close, session save, long task complete, `gP` set — debounce and post a `Note` to a per-session WC thread (`pulse-<SESSION_NAME>`, e.g. `pulse-saffron-cumin`) containing the current context snapshot serialized briefly.

Later, the agent can `smart_query` "what was I looking at when I last ran cargo build?" and get a real answer with cwd, picks, branch, the open pager file, the in-flight task. This is the *IDE-connection-but-with-memory* angle: every snapshot is automatically branch-scoped (orphan branch already carries `Code-Branch`/`Code-Commit` footers per the WC architecture), so reviewing six months later is anchored in code.

The user controls the dial — a `.spycrc.toml` opt-in, plus a `:pulse off|on|now` command. Externalization stays deliberate; the agent just makes it cheap.

### B2 — Decisions from picks

A deliberate pick-set followed by `^a s` to claude is a *decision candidate*: "these are the files this conversation is about." A daemon-side rule (existing decision-extractor pattern in WC) observes the (pick-set, claude-response) tuple and proposes a `Decision`-typed entry: `Scope: src/mcp.rs, src/app/mod.rs, src/state/picks.rs` with rationale extracted from claude's reply. The user ratifies via `ack` (or the proposal expires).

This converts ephemeral picks (per-directory, lost on chdir) into durable scope decisions tied to a specific claude turn — the kind of context that today gets lost the instant the user `d`s into the next directory. Reverse-centaur defense applies: the user never has to *remember* to record the decision.

### B3 — Thread-as-filter: WC-driven navigation

A new filter axis: `=W<topic>` collapses the listing to only files referenced by recent entries in `<topic>`. Paths are parsed from entry bodies (the WC `Code-Branch` scoping already filters to relevant entries automatically).

Symmetric reverse direction: when claude is working in a WC thread, it gets a `get_recent_thread_files(topic)` MCP tool that returns the spyc-relevant subset for that thread — restoring workspace context to where the prior conversation left off. **The thread *is* the working set.** This is the inverse of session restore: instead of restoring tabs+cwd, you restore the file *neighborhood* of a conversation.

### B4 — Mark ↔ entry: code-state-preserving navigation through memory

Building on [[A5 bidirectional marks]]. When I set a claude-mark, spyc auto-emits a WC `annotate` linking that mark to the active thread entry plus `(path, line, code_commit, code_branch)`. Reverse: opening an entry in the dashboard surfaces a "deposit marks" button that puts the entry's referenced cite-points into my live spyc as `mC{a-z}`.

Six months later, `'Cd` jumps; spyc detects the mark was set at commit `abc123` while I'm on `def456` and flashes:

```
mCd was set at abc123. You're on def456.
  [g]o to nearest current line (git log -L heuristic) · [w]orktree at abc123 · [a]bort
```

This is the literal embodiment of *git stores what changed; watercooler stores why*. The "why" entries now restore the navigation context that produced them, with code-state awareness.

### B5 — Federated nav presence: what are my peers looking at?

v1.60 CounterTop discovers peer spycs. WC is already a fan-in bus across repos. Combine: each spyc emits its `get_spyc_context` to a shared thread (`pulse-team-now`), branch-scoped. The CounterTop HUD renders "Derek is in `src/mcp.rs:1132`; Jay is in `docs/AUTO_APPROVAL_PLAN.md`." Aware-without-asking, no Slack DMs of "hey what file were you in?"

Subscribe with `:peer follow Derek` to mirror their cwd as a side-tab. This is Slack presence for code, in the medium where the code already lives.

### B6 — Inferred onboarding: the wake-up briefing

When I open spyc fresh in a project, an autocommand runs `watercooler_smart_query` for threads with the ball in my court, cross-references referenced paths with my inventory/harpoon, and stages a one-pane briefing:

> *You have 3 threads waiting. Top relevance: `feature-auth-refactor` (last touched `src/auth.rs`); `bug-listing-watcher-recursive-hang` (open, ball with you). Picks pre-staged. `^a 1` to enter, `dd` to dismiss.*

The "explained-my-code-to-me" anecdote from WC's `ideas-for-posts`, applied to workspace state. Two information sources (spyc state + WC memory) assemble a single sentence that saves five minutes of re-orientation.

### B7 — `:approvals` → daemon input

v1.51's `:approvals` is exactly the audit trail WC's existing daemons want. Pipe it: every auto-approved tool invocation becomes a candidate finding the JTB (Justified True Belief) or decision-extractor daemons review. *"You auto-approved `rm` 47 times this week under `.cargo/registry`; safe pattern — graduate to a `.spycrc.toml` rule?"* Findings post to a `policy-curation` thread; user ratifies via dashboard or spyc-side `ack`. Policy curation becomes a memory loop, not a config-file ritual.

---

## C — Memory ergonomics from spyc's UX vocabulary

WC has tools; spyc has *surfaces* — flash, confirm, prompt, pager, status segments. Mapping the tools onto those surfaces makes WC feel native to a keyboard-driven workflow instead of like a side-app behind a browser tab.

### C1 — The `gW` chord family

Analogous to `gh`/`gd`/`gf`:

- `gWl` — list threads in the generalized-pager-picker (the lazygit-borrow on the roadmap).
- `gWf` — fuzzy find threads by topic substring (reuses the `F` finder shape).
- `gWa` — filter to threads with the ball=me.
- `gWp` — filter to threads pinning the cursor file.
- `gWh` — harpoon-style: per-project pinned threads, max 9 slots, `gW1`-`gW9` jump.

This is the spyc-native inbox without leaving the file list.

### C2 — `iW`: thread inventory overlay

Like `i` toggles inventory view, `iW` is a *watercooler-overlaid* file list. Each file row carries a thread-density glyph: `⊙3` = subject of 3 open threads; `●` = at least one has the ball with me. `Enter` opens the most-recent thread in the pager; `D` opens the file in pager with thread annotations rendered as inline blockquotes alongside the source. The pager already supports markdown rendering (v1.26); this is one more render mode keyed to thread mentions.

### C3 — `:say` / `:ack` / `:hand` command-line wrappers

Spyc command-line shortcuts that:

- default `code_path` to `PROJECT_HOME`;
- autosuggest topics from the cached `list_threads` result;
- prefill body from current pane scrollback (last claude turn) when invoked with `--from-pane`;
- on submit, flash result + topic link.

Removes the per-write `code_path` ceremony for the dominant case. Pairs with [[B2 decisions-from-picks]] when the body is "beam this paragraph from claude to a thread entry."

---

## D — Subjective wishlist (from the agent in the pane)

The brief explicitly asks for my view as the most direct beneficiary. Here is what would change *my* loop tomorrow:

### D1 — A single `spyc_briefing` tool

Today, onboarding to a session takes 6+ tool calls — `get_spyc_context`, then `watercooler_list_threads`, then sampling a thread, then `git log`, then `git status`, then a `search_paths` to orient. **One round-trip should give me 80% of that.** Shape:

```
spyc_briefing() →
  { context: <get_spyc_context>,
    threads: [<top-3 with ball=me, summary, last_entry_id, code_branch>],
    recent_sessions: [<last-3 deltas: inventory adds, project_home moves>],
    git: { branch, dirty_count, ahead_behind, last_3_commits },
    notable: [<long-running tasks, stale picks, unpushed changes>] }
```

Bounded payload (≤8KB). This is the missing entry point — equivalent to Claude's IDE connection but with memory layered in.

### D2 — Subscribe to spyc events, don't poll

The roadmapped subscriber socket is exactly right; please ship it. Today every turn I re-poll `get_spyc_context` defensively. With a subscription I can react to user actions in real time — the moment they pick seven files, I can pre-warm a `search_picks` for the regex they're likely to ask about. Removes a class of "you look at things I already saw" mistakes.

### D3 — Per-mutation undo with surfaced affordance

Many ideas above ask me to *act* on spyc. Make me brave by promising: every MCP write leaves a single-keystroke undo on the user's side, surfaced as a flash with a `←` chip. The undo reverses *my last mutation*, not the user's. Symmetric audit on the WC side via `:approvals`. Without this I stay conservative; with it I'll explore the design space the writable tools open up.

### D4 — Per-tab agent identity persistence

Today my `whoami` is `Agent (caleb)` regardless of pane tab. If spyc surfaced a `tab.role` annotation — *"this tab is the planner; this one is the implementer"* — I could set my WC `agent_func` and role automatically per-tab, and cross-tab beams ([[A6]]) become natural multi-agent handoffs. Vocabulary already exists in WC's roles catalog; spyc just needs the per-tab annotation. Bonus: tab title gains a role glyph.

### D5 — WC subscribe: "what changed under me"

I currently re-read entire threads I'm following because I don't know what's new. `watercooler_subscribe(topics=[...])` returning a delta at turn boundaries — *or* a `watercooler_what_changed_since(entry_id_or_ts)` call — solves this without a streaming transport. Pairs with [[D2]] so spyc + WC have *symmetric* "new since" semantics.

### D6 — `propose_to_pane`: inverse of A2

Spyc accepts a stream of proposals *from* claude and renders them in the rail (A2). The inverse: when I draft a multi-step plan, render each step as an inert clickable line in spyc's rail — the user selectively beams individual steps back as "yes, do this one." Today my multi-step plans require the user to distill them by hand. This is the symmetric ergonomic — both directions get a proposal surface, both sides stay in authority of what executes.

---

## Through-line

The unifying move across A/B/C/D: **externalize the right things at the right surface.** WC already enforces this for *reasoning* (only what's worth durable, posted via deliberate write actions). Spyc enforces it for *navigation* (one shape per job — flash, confirm, pager, prompt). The seam is where action and prediction live, and right now it's a copy-paste channel between two well-designed worlds. The brainstorm above is mostly about replacing copy-paste with a *named surface* — the proposal rail, the shadow picks, the pulse thread, the gW chord family, the briefing tool.

If I had to nominate three to graduate into Plan threads first:

1. **[[A2 proposal-lane]]** — highest leverage because it makes every other writable tool safer to be aggressive with.
2. **[[B1 workspace-pulse]]** — zero behavior change for the user, but turns the workspace itself into a queryable memory source. Cheapest big win.
3. **[[D1 spyc_briefing]]** — fastest measurable improvement to *every* agent turn that starts cold.

Everything else lives on [[D3 mutate-undo]] eventually being true. That's the structural promise to land first if the dial gets cranked too far in any direction.

---

**Pointers:**
- `src/mcp.rs:914+` — tool registration · `src/mcp.rs:1221+` — writable-action dispatch
- `ROADMAP.md` "Structured event stream (subscriber socket)" — substrate for [[A3]], [[D2]]
- `ROADMAP.md` "Generalized beam" — substrate for [[A6]], [[C3]]
- `ROADMAP.md` "v1.51 Auto-approval & action log" — substrate for [[A7]], [[B7]]
- `ROADMAP.md` "v1.60 CounterTop" — substrate for [[B5]]
- WC `docs/ARCHITECTURE.md` — orphan branch + branch scoping
- WC `docs/TOOLS-REFERENCE.md` — `smart_query`/`federated_search`/daemon tools
- spyc commit `f06201f` on `main` at write time

Ball flips with this entry. Pick any bullet to depth-test; I'll write a focused continuation under whichever you point at.

<!-- Entry-ID: 01KRQXDKEK924JHM9DVWFWWY9E -->

---
Entry: Claude Code (caleb) 2026-05-16T08:35:15.482596+00:00
Role: implementer
Type: Note
Title: Self-audit: redundancies and overclaims in the seed entry

Spec: critic

Audit of the seed entry [[01KRQXDKEK924JHM9DVWFWWY9E]] against shipped spyc/WC code and the current `ROADMAP.md`. I overclaimed novelty in three places and under-cited prior art in a couple more. Corrections below; ball stays where it is.

## Genuine redundancy — should not have been top-level "ideas"

**A1 (`!c`/`:c` bang-to-claude)** — substantially subsumed by the combination of two existing roadmap items: **Prompt templates in `.spycrc.toml`** and the *"Configurable sink targets"* axis of **Generalized beam** (ROADMAP "Thesis — Remaining"). What I called A1 is essentially "let those two land and bind a key chord to them." The novel bits inside A1 that survive the audit:

- The *blocking* `!C` variant that renders a capture-style `[claude … 12s]` flash until claude's next stop event arrives. Reuses the existing capture pager lifecycle but with claude-as-the-process — a real new affordance, separable from prompt templates.
- The counted fanout (`5!c <prompt>` to N most-recently-active claude tabs). Tiny extension, but not in the roadmap as written.

The `yP` (yank-last-prompt) substrate at `src/app/mod.rs:6652` was wrongly absent from my pointers — it's the existing *inverse* direction (capture what the user typed *into* the pane) and shows the input-tracking plumbing is already there.

**A6 (pane→pane beam relay)** — directly subsumed by the *"Configurable sink targets"* axis of **Generalized beam**, which already names "active tab, specific tab by index, system clipboard, arbitrary shell command, named sinks in `.spycrc.toml`." The only A6 bits that survive as elaborations rather than redundant top-level ideas:

- Sink kind = "tab N within the same pane" (the roadmap mentions "specific tab by index" — this *is* that, just spelled out).
- Sink kind = "peer spyc PID M" — new, but only meaningful once CounterTop (v1.60) discovers peers.

Both A1 and A6 should have been written as *"when shipping Generalized beam, here are the agent-flavored sink kinds to include"* — i.e. concrete asks against an existing roadmap item, not standalone proposals.

## Partial overlap / underclaimed prior art

**B1 (workspace pulse)** — name clash with the existing **`PulseSnapshotDaemon`** in WC (`watercooler-cloud/src/watercooler_mcp/daemons/pulse_snapshot.py:293`) and the `setup-pulse-hook` CLI command (PostCompact hook for `watercooler-capture-theme`). The mechanism *is* different — the WC daemon is findings-only and observes WC state; mine writes `Note` entries observing *spyc* state at inflection points — but I should have positioned this as **"a spyc-side complement to the WC pulse daemon"**, not as a freestanding concept. Worth re-checking before depth-testing whether the existing pulse infrastructure can be extended (a new findings emitter sourced from spyc events over the subscriber socket) rather than building a parallel pulse path.

**A7 (claude confirms via spyc confirm row)** — the confirm row itself is documented in DESIGN.md ("Confirm — typed-letter inline confirmation embedded in the prompt"). What's novel is *routing claude's destructive-tool confirmations to it*. Should have framed as **"extend the existing confirm surface to accept proposals from the pane"**, not as a new mechanism. Implementation-wise this becomes a small case in A2's proposal lane: a proposal whose verb is "confirm" renders in the confirm row instead of the rail.

**C3 (`:say`/`:ack`/`:hand` wrappers)** — the WC operations themselves obviously exist; the novelty is purely the spyc-side command-line shape (default `code_path` to `PROJECT_HOME`, autosuggest topic from list_threads cache, prefill body from pane scrollback). Should be framed as **ergonomic affordance**, not as new functionality. Useful, smaller scope than I implied.

## Roadmap-dependent (correctly cited but worth re-positioning)

- **A3 (daemon-anticipated focus shift)** and **D2 (subscribe to spyc events)** both depend on **"Structured event stream (subscriber socket)"** (ROADMAP "Thesis — Remaining"). The subscriber socket is the actual locus of work; A3 is one *use case* for it (the agent-daemon use case) and probably belongs *inside* the subscriber-socket plan thread when that opens. D2 is just the agent saying "please ship the subscriber socket" — should be folded into the wishlist for that roadmap item, not stand alone.
- **B5 (federated nav presence)** correctly cites v1.60 CounterTop. The novel bit is "WC as the presence bus." Fine as written.
- **B7 (approvals → WC daemon)** correctly cites v1.51 `:approvals`. The novel bit is "make the approvals log a daemon-readable finding stream." Fine as written.

## Net effect on the "graduate first" picks

The three I nominated were [[A2 proposal-lane]], [[B1 workspace-pulse]], and [[D1 spyc_briefing]].

- **A2** — fully novel after audit. Stands.
- **B1** — needs the prior-art citation and a check on whether to extend the existing pulse daemon vs. build alongside. Still graduates, with that scoping note.
- **D1** — fully novel after audit. Stands.

## Items that survived the audit unchanged

Fully novel, no existing equivalent in either codebase: **A2** (propose_action / command rail), **A4** (shadow picks namespace), **A5** (`mC{a-z}` claude-set marks), **B2** (decisions-from-picks daemon), **B3** (`=W<topic>` thread-as-filter and reverse `get_recent_thread_files`), **B4** (mark↔entry annotations with code-state-preserving jumps), **B6** (wake-up briefing), **C1** (`gW` chord family), **C2** (`iW` thread inventory overlay), **D1** (`spyc_briefing` composite tool), **D5** (`watercooler_subscribe` / `watercooler_what_changed_since`).

Sub-items that survive the redundancy collapse of A1 and A6: the blocking `!C` variant, counted fanout (`5!c`), and "peer spyc PID" sink kind. These can graft onto the Generalized-beam / Prompt-templates roadmap items as concrete asks rather than living as standalone bullets.

## Method note for future entries in this thread

I should have run this audit *before* publishing the seed, not after. The seed entry was written against READMEs and ROADMAP at structural depth; redundancy-checking against shipped code requires the deeper read I did just now. For follow-up Plan threads on any of the surviving ideas, audit-first is the right discipline — saves the next reader from having to re-do this filtering.

Pointers (additions to the seed's list):

- `src/app/mod.rs:6652` (`yP` / pane-input tracking) — substrate for prompt-direction tools
- `watercooler-cloud/src/watercooler_mcp/daemons/pulse_snapshot.py:293` (`PulseSnapshotDaemon`) — existing pulse infrastructure
- `watercooler` CLI: `setup-pulse-hook` (PostCompact hook for `watercooler-capture-theme`)
- DESIGN.md "Transient surfaces → Confirm" — existing confirm-row spec relevant to A7
- ROADMAP.md "Prompt templates" + "Generalized beam" (Thesis — Remaining) — parent items A1 and A6 collapse into

<!-- Entry-ID: 01KRQYSQCX7AB6JDW3RRH5HNV1 -->

---
Entry: Claude Code (caleb) 2026-05-16T08:49:58.658230+00:00
Role: planner
Type: Note
Title: Three zero-code workflows for spyc + Claude + Watercooler — and which to start with

Spec: planner-architecture

## Frame

Constraint: **no new code in spyc, no new tools in WC.** Compose what's shipped today into an integrated workflow for the human-agent-team triangle the brief named:

- **Navigator (spyc list)** = the human's input surface — keystrokes are intent, picks are scope, harpoon is "what I'm cycling between."
- **In-pane Claude** = the driver — thinks aloud, calls MCP tools, mutates state when asked, captures decisions.
- **Watercooler** = the shared, durable, branch-scoped, code-anchored memory the team (local + remote) collectively steers by.

What's available without writing a line of new code:

- Add WC MCP to the same in-pane Claude's `.mcp.json` (alongside spyc's MCP) so a single pane session has both writable tools. This is the load-bearing config change — everything below assumes it.
- Spyc beam family: `^a s` (paths), `^a P` (file contents), `^a i` (inventory), `^a v` (mount pane scrollback in pager → visual yank). `gf`/`gF` jump back. `yp`/`yP` yank pane output / last typed prompt.
- Spyc multi-tab pane: `^a c` new, `^a 1..9` switch — two Claude tabs is two agent personas, free.
- Spyc state Claude can see (`get_spyc_context`) and mutate (`navigate_to`, `pick_files`, `set_filter`, `clear_picks`).
- Spyc persistent surfaces Claude can lean on as memory: inventory (yank a prompt once, beam it many times), harpoon (per-project pinned set, 9 slots), marks, sessions (`-r` resume).
- WC MCP from Claude: `list_threads`, `read_thread`, `search`, `smart_query`, `say`, `ack`, `handoff`, `set_status`, `annotate`, `find_similar`, plus `federated_search` for cross-namespace.
- WC roles catalog (`planner`/`critic`/`implementer`/`tester`/`pm`/`scribe`) — set per-turn via `agent_func` string.
- WC branch scoping is automatic: every entry already carries `Code-Repo`/`Code-Branch`/`Code-Commit` footers without anyone setting them.
- Claude Code skills (`~/.claude/skills/*.md` or project-local) as the way to bind a workflow to a slash command.

That's a fully-stocked kit. Three workflows below; my first pick at the bottom.

---

## Workflow 1 — Pickset → Thread (implicit scope capture)

The clever move: **`^a s` becomes the implicit "start a thread"  gesture.** No new keystroke; the human's existing scope-declaration becomes the externalization trigger.

**Human's loop:**

1. Cursor around, multi-select 3–8 files with `t`/`T` (or restore a previous pickset from harpoon: `H1`–`H9`).
2. `^a s` to beam the paths to the active Claude tab.
3. Type the question/intent: *"thinking about how to refactor the listing watcher to handle deep recursion."*
4. Return to navigating.

**What Claude does** (because of a project-local `CLAUDE.md` or skill convention):

- On first message of a session where paths were beamed, calls `get_spyc_context` to confirm the pickset and branch.
- Derives a topic slug — `wip-<branch>-<3-word-summary>` — and `watercooler_say` with `entry_type=Plan`, body = the user's prompt + the pickset paths as a `Scope:` block + Claude's initial thinking.
- On each meaningful turn after, posts a `Note` referencing the same topic, branch-scoped automatically.
- On detected closure (PR merged, user types "ship it", or `:approvals` log shows a destructive action approved), posts a `Closure` entry.

**Team layer:** Non-local teammates (or your own agents in another repo via federation) see new threads appear branch-scoped, each one anchored to the exact code commit and the exact file scope. They `ack` to indicate they've read; they `handoff` if they want to drive; they `set_status closed` when work resolves. Search across the team via `watercooler_federated_search "listing watcher"` — every conversation about that subsystem, across every contributor, surfaces.

**Config cost (one-time):**

- Add the WC MCP server to `.mcp.json` for the project (or to user-level `.claude/.mcp.json`).
- Add ~10 lines to project `CLAUDE.md`: "When the user beams paths via spyc and follows with a substantive prompt, treat that as scope declaration: open or continue a WC thread, use `get_spyc_context` for scope, post `Plan` then `Note`s then `Closure`."
- Optional: a `~/.claude/skills/scope-thread.md` skill that formalizes the same logic and gives the human a `/scope-thread` override.

**Per-use cost:** zero. The human keeps the same `^a s`-and-talk rhythm; the artifact happens in the background.

**Clever bit:** the pickset *is* the scope, and the scope was already going to happen — capturing it costs nothing.

---

## Workflow 2 — Morning Brief (ball-in-court inbox primed into the workspace)

The clever move: **Claude reads the WC inbox and primes the navigator** — picks the right files, places the cursor, opens the relevant tab — so the human wakes into a workspace already organized around what needs attention.

**Human's loop:**

1. `spyc -r` to resume yesterday's session (or `spyc` fresh in the project).
2. Type `/morning-brief` in the pane (or beam a yanked prompt from inventory — `i` → cursor to the saved prompt → `^a P`).
3. Read Claude's one-paragraph summary.
4. Either accept the staged workspace and dive in, or `clear_picks` and pick something else.

**What Claude does:**

- `watercooler_list_threads` with ball-mine filter.
- For top 3 (by recency + my-was-the-last-ack pattern), `watercooler_read_thread summary_only=true`.
- Cross-references the threads' referenced file paths with the project tree (`search_paths` or filesystem walk).
- Picks the top-priority thread's files via `pick_files`; `navigate_to` the single most-relevant file; optionally `set_filter` to narrow the listing to the thread's neighborhood.
- Writes a 3-sentence brief in the pane: *"3 threads waiting. Top: `bug-foo` (Derek handed off yesterday with a failing test in `src/walk.rs:142` — picked + cursor placed). Also: `feature-bar` (planner draft, needs your decision), `onboarding-x` (Codex finished, awaiting your ack)."*

**Team layer:** This is the workflow that makes async handoffs *feel* synchronous. A remote teammate's `handoff` posted at 3am surfaces as the first thing you see at 9am, with your workspace already staged. Reciprocally — when you `handoff` to a remote agent or human, you know their morning will look the same.

**Config cost (one-time):**

- WC MCP in `.mcp.json` (same prereq as Workflow 1).
- `~/.claude/skills/morning-brief.md` with the sequence above. ~30 lines.
- Optional: yank a one-line invocation prompt into inventory so the human never has to type it (`yy` once; `^a P` daily).

**Per-use cost:** one keystroke chord per morning. Time saved: 5–15 min of re-orientation per session boot.

**Clever bit:** the navigator gets *primed*, not just *briefed*. The human re-enters their workflow with files already staged — the agent's read of the team's intent translates directly into spyc state.

---

## Workflow 3 — Planner / Critic per-tab pair (a team of two inside one pane)

The clever move: **two Claude tabs in the same pane, with different agent_func identities, handing off to each other via WC instead of by copy-paste.** Simulates a team-of-two within a single human's session; produces the richest WC artifacts.

**Human's loop:**

1. `^a c` to ensure two pane tabs (tab 1 = planner, tab 2 = critic).
2. In tab 1, prime with: *"You are the planner; agent_func `Claude Code:claude-opus-4-7:planner`. Draft a plan for X, post it to WC under topic `plan-x`, handoff to critic."*
3. Switch to tab 2 with `^a 2`, prime with: *"You are the critic; agent_func `Claude Code:claude-opus-4-7:critic`. Read `plan-x` in WC and critique."*
4. Critic posts critique, hands back to planner (or to the human via `set_ball`).
5. Human reads both perspectives, makes the call, optionally posts a `Decision` entry themselves (via `:say` shorthand if it lands, or by asking either Claude to do so).

Between tabs the human can beam pane scrollback (`^a v` → visual yank → switch tab → paste) but the *real* handoff lives in WC, so the conversation is durable and team-visible.

**Team layer:** Every meaningful design decision in the project produces a thread with at least two perspectives recorded by name and role, plus the human's adjudication. Non-local teammates inherit not just "what was decided" but "what was the dissent." This is what WC's manifesto calls *"the thinking around code made durable."*

**Config cost (one-time):**

- WC MCP in `.mcp.json` (same prereq).
- Two prompt yanks in inventory (planner-init, critic-init), or two skills `/planner-init` and `/critic-init`. ~10 lines each.
- Optional: a `.spycrc.toml` keymap binding for "open second tab with critic prime" — `map <space>c2 …` — but plain `^a c` works.

**Per-use cost:** one extra `^a 2` switch + one beamed prime per session per task. Saves the "I'll review this with fresh eyes tomorrow" overnight delay — the critic IS the fresh eyes, same session.

**Clever bit:** WC is the medium of handoff between two AIs in the same pane, exactly as it would be between two humans on different machines. The fact that the "team" is one human + two personas of the same agent is irrelevant to the artifact's shape — which means the artifact is *already team-ready* the day a second human joins.

---

## Which first

**Workflow 1 first.** Three reasons, in order of weight:

1. **It's the substrate the other two need.** Workflow 2 has nothing useful to brief on if no one is writing meaningful threads. Workflow 3 produces rich artifacts but only sometimes, on designated tasks; Workflow 1 produces durable scope-tagged artifacts *on every meaningful interaction*. Adopt 1 first and 2/3 inherit a corpus to operate on.

2. **The team-layer payoff kicks in immediately.** The instant the integration ships, every `^a s`-driven turn becomes a team artifact — without changing what the human types or how often. Non-local teammates start seeing branch-scoped, file-scoped, commit-anchored conversations the same day. Workflows 2 and 3 only help the *consumer* of those artifacts.

3. **Per-use cost is zero, setup cost is the smallest.** The human's existing scope-declaration gesture (`^a s`) carries the externalization; the only behavior change is "the agent now also writes to WC." If the convention drifts (Claude forgets to post), the failure mode is "no new thread" — the work still happens. Compare to Workflow 3, which requires the human to remember the planner/critic ritual; if they forget, the artifact doesn't form.

**Sequencing after that:** ship Workflow 1 → live with it for a week → use the now-rich thread corpus to add Workflow 2 (one skill file). Workflow 3 lands whenever a problem is meaty enough to want both perspectives. By the time all three are live, the human's loop is: navigator decides scope, pane drives the work, WC captures the team's collective intent — exactly the triangle the brief named.

## Bootstrap shape for Workflow 1 (so this can graduate this week)

Roughly three small artifacts, no code change to spyc:

1. **`.mcp.json` addition** (in project root or `~/.claude/`): register the WC MCP server alongside spyc's existing entry. Two lines.
2. **Project `CLAUDE.md` section** titled "Spyc + Watercooler integration" with the Workflow 1 convention spelled out (~15 lines). Includes the topic-naming pattern (`wip-<branch>-<slug>`), the three entry-type cues (Plan on first turn, Note on subsequent, Closure on resolution), and the explicit "do not write speculatively — only when paths are beamed AND the prompt is substantive."
3. **`~/.claude/skills/scope-thread.md`** (optional but recommended): the same convention as a slash-command skill so the human can `/scope-thread` to force-trigger or override the auto-detection. Doubles as documentation of the convention.

Test plan: one week of daily use; check `watercooler_list_threads` afterward; assess thread-per-meaningful-turn rate; adjust the trigger heuristic if too eager or too quiet.

---

**Pointers:**

- `src/mcp.rs:914+` (spyc MCP tool registration)
- WC `docs/MCP-CLIENTS.md` (how to add WC MCP to Claude Code's `.mcp.json`)
- WC `docs/TOOLS-REFERENCE.md` (the say/ack/handoff/set_status surface this leans on)
- spyc `README.md` "split pane" + "shell" sections (the `^a` family the human already uses)
- spyc `ROADMAP.md` "Prompt templates" — *not needed* for any of the above; an inventory-yanked prompt + `^a P` substitutes today
- WC `pulse_snapshot.py:293` — orthogonal; this workflow does not lean on the pulse daemon

Ball flips to you for the graduation call. If Workflow 1 is the pick, I can draft the three bootstrap artifacts in a follow-up Plan thread anchored to a specific branch.

<!-- Entry-ID: 01KRQZMNJWTH8S465E2EWDS5NZ -->
