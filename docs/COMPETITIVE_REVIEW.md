# Competitive review & go-to-market — AI coding-agent managers (+ the file-manager lane)

Competitive analysis of the AI coding-agent orchestration tools spyc shares a
category with, plus a go-to-market plan — grounded in **verified user voices**,
not vendor marketing. Every quote below was fetched from its source and checked
for verbatim text and correct attribution. spyc straddles two lanes — the
agent-manager category (§1, §1a–§1c) and the TUI **file-commander** lane, whose
incumbent Yazi is covered in §1d (folded in 2026-07-01 from the standalone Yazi
review; the raw original is archived at `docs/archive/YAZI_COMPETITIVE_REVIEW.md`)
and whose minimalist counterweight lf is covered in §1e. §1f records a third
thing — the **session layer** beneath both lanes (Superlogical, zmx), added
2026-09-04 on a stated-facts basis rather than the mine-and-verify standard.

> **Method.** Two deep-research passes. The first (v1) drew only on vendor
> primary sources (GitHub READMEs, changelogs, marketing pages) and produced a
> conclusion that turned out to be wrong (see the correction below). This
> document is the second pass (v2): a targeted mine of real user sentiment —
> Hacker News comment threads, GitHub issues, Reddit, X — with an adversarial
> verification step that **fetched each source to confirm the quote and its
> attribution**. 78 evidence items were mined → **71 verified real, 6 killed as
> misattributed** (real quote, wrong author), 1 partial. Star/version counts are
> mid-June 2026 and drift; this space moves weekly.
>
> **Third pass (2026-08-12)** added §1e (lf) on the same standard: repo counts,
> releases, contributors and per-author commit history from the GitHub API; the
> feature surface read from lf's own `doc.md`/`CHANGELOG.md` at master rather than
> a summary; and **every quote re-fetched from HN's Firebase item API** for author
> and timestamp, because a search-engine snippet is not attribution. Two stale
> spyc-side rows in §1d were corrected in the same pass (mouse support, Yazi's
> star count).

---

## Correction to the first pass

> ~~"The TUI-native agent-orchestration space is almost empty — a blue ocean."~~

**It isn't.** The terminal lane already has **two** incumbents — **Claude Squad**
(7.9k★, Go, the self-described "most popular claude code multiplexer") and, as of
2026, **herdr** (~7.4k★, **Rust**, "agent multiplexer that lives in your
terminal") — plus agent-deck, crmux, Codirigent, AgentWire and others. Users
track **8+ tools by name** and are visibly fatigued by the flood. spyc's opening
is real, but it is **differentiation in a crowd**, not first-mover in a vacuum.

herdr matters more than the rest because it is spyc's **Rust-TUI twin**: same
language, same cross-platform lane, and it has *already shipped* the two things
this review flagged as spyc's biggest openings — a live agent-status sidebar
(unmet need #2) and, architecturally, session persistence via a detach/reattach
daemon (unmet need #4). It is fresh, sharp proof that **"a cross-platform Rust
TUI" cannot itself be spyc's differentiator.**

The differentiator therefore has to be **what spyc is that herdr and Claude Squad
are not** — and the deep dive in §1a confirms herdr has *none* of these: a real
vi-driven **file manager** you already live in (navigation, git-status gutter,
marks, harpoon, picks), an **in-process diff/review loop** (herdr's own marketing
concedes worktree-diff-review to others), and the **MCP bridge** that grounds the
agent in your view. The wedge is **file-manager + review + MCP**, not "terminal,
cross-platform, Rust" — herdr owns that ground too.

---

## 1. The landscape

Two lanes. The **GUI/macOS lane** is busy and well-funded but structurally
locked to one OS and paying for it in stability. The **terminal/cross-platform
lane** is where spyc lives — and it already has occupants.

| Tool | Lane | What it is · signal |
|------|------|---------------------|
| **Claude Squad** | TUI · cross-plat | 7.9k★ · Go · tmux+worktrees, AGPL. The terminal incumbent; "works like you'd expect," but dinged for clunky UX and no uninstall script. |
| **herdr** | TUI · cross-plat | ~7.4k★ · **Rust** · AGPL+commercial, single maintainer. The closest twin — live agent-status sidebar (blocked/working/done/idle), detach/reattach daemon + live binary handoff, a Unix-socket orchestration API with **wait/subscribe** verbs, 15+ agents. But **no file manager, no diff/review, no MCP** — and its two headline features (hook-free status detection, persistence) are exactly where its bug reports cluster. See §1a. |
| **cmux** | GUI · macOS | Swift/libghostty. Vertical tabs + notifications are loved; plagued by an 80 GB OOM leak, session loss, crash windows, and an agent-wrapper trust crisis. |
| **supacode** | GUI · macOS 26 | Worktree command-center; beta, single-maintainer (~1,574 commits by one person), "Other" license, macOS-26-only floor, ~1k Homebrew installs/qtr. |
| **Conductor** | GUI · macOS | Cloud/GitHub-OAuth model triggered a trust backlash ("keys for the kingdom"); "silicon only." |
| **Superset** | GUI · macOS | "Run 10 parallel agents"; surfaced the review-bottleneck and DB-isolation debates. |
| **agent-deck** | TUI · cross-plat | 338★ · Go · MIT. Feature-maximalist: MCP socket pooling (claims 85–90% memory reduction), status-transition notifications, cost dashboard, "conductor" agents. |
| **psmux** | TUI · **Windows** | Rust · "the native Windows tmux" (ConPTY, reads `.tmux.conf`, 83 tmux commands). A **multiplexer**, not a manager — its one agent feature renders Claude Code teammate agents into panes. No file manager, MCP, git, or context-sharing. Different lane (Windows) + different layer. See §1b. |
| **claude-code-ide.el** | editor plugin | 1.6k★ · 109 forks · Emacs. The closest **architectural** twin outside the TUI-manager space: Claude Code runs in a hosted terminal pane (vterm/eat/ghostty) behind a **bidirectional MCP bridge** that pushes live editor context (file, selection, diagnostics, project) to Claude automatically. Different host (a 40-year-old editor, not a file commander) and different center of gravity — the buffer/editor is the noun, dired is a visited mode, not the driving UI. See §1c. |
| **Yazi** | TUI · cross-plat · **file manager** | ~41.3k★ (2026-08-12) · Rust · the gold-standard TUI file commander (image preview, plugins + package manager, async scheduler). A *different lane* — no agent concept, no MCP; it sets the bar for the file-manager half of spyc's identity, not the agent half. See §1d. |
| **lf** | TUI · cross-plat · **file manager** | 9.5k★ · Go · MIT · ranger-inspired minimalist, now **community-maintained** (founder's last commit 2024-03-31). Ships the lane's only external **read+write** control surface (`lf -remote send`/`query`) — yet zero agent framing, and its README *Non-Features* (no tabs, no builtin pager/editor, no builtin file ops) are the precise inverse of spyc's integration bet. See §1e. |
| **Superlogical** | session layer | Launched 2026-07-29 · Hashimoto, Pearkes, Monk, Simpson · Notable Capital + Amplify Partners. Server-side terminal multiplexer on MIT libghostty — "the multiplexer for all work." **Waitlist only, nothing shipped** as of 2026-09-04. A *layer beneath* spyc, not a manager. See §1f. |
| **zmx** | session layer | Zig · the unbundled form of tmux persistence: daemon per session over a unix socket, passthrough to the PTY, libghostty-vt as a shadow state machine for rehydrating a client on reattach. Deliberately **no windows, tabs or splits**. See §1f. |
| **long tail** | mixed | Chorus · Vibetunnel · VibeKanban · Mux · Happy · AutoClaude · Codirigent · Centurion · AgentWire · Baton … |
| **spyc** | TUI · cross-plat | Rust/ratatui · in-process gix worktrees · MCP bridge · vi-keyboard **file+process+agent manager** (not just a multiplexer). |

The crowdedness is itself a documented user complaint:

> "I've been following this space and a lot of good apps: Conductor / Chorus /
> Vibetunnel / VibeKanban / Mux / Happy / AutoClaude / ClaudeSquad. All of these
> allow you to work on multiple terminals at once. Some support work trees and
> others don't. Some work on your phone and others are desktop only."
> — theturtletalks, [HN](https://news.ycombinator.com/item?id=46424501)

> "with so many open source agent managers cropping up … What's the top 5 (or
> any N) that come to mind: A) GUI based B) terminal based C) web based? Like,
> not just personal projects but something with a bit of a community around it?"
> — KronisLV, [HN](https://news.ycombinator.com/item?id=47602407)

> "hey, co-creator of Claude Squad – imo the most popular+used of these 'claude
> code multiplexers' and it's also open source and free :) works like you'd
> expect" — moofeez, [HN](https://news.ycombinator.com/item?id=44630194)

**Takeaway:** mindshare is now a moat. spyc needs a sharp one-line wedge or it
disappears into this list.

---

## 1a. herdr — the closest twin (deep dive, June 2026)

[herdr](https://github.com/ogulcancelik/herdr) — *"agent multiplexer that lives
in your terminal."* Rust TUI, ~7.4k★ / 457 forks, v0.7.1 (2026-06-24, ~935
commits), single maintainer, **dual AGPL-3.0 + commercial**. A **client/server
daemon**: PTYs live in a headless background server, the TUI is a thin client.
Headline features: a live **agent-status sidebar** (🔴 blocked / 🟡 working / 🔵
done / 🟢 idle), **detach/reattach** + experimental **live binary handoff**
(processes survive an upgrade), a **Unix-socket orchestration API**, mouse-native
panes, 18 themes, 15+ agents, remote SSH attach. No file manager, no diff/review,
no MCP.

### What users actually say (verified)

Discussion footprint is **thin and GitHub-star-driven, not community-driven** —
herdr's own Show HN scored **5 points, 0 comments**
([48247248](https://news.ycombinator.com/item?id=48247248)); most "reviews" are
SEO/AI blog spam. The genuine voices, fetched and verified:

- **The agent-status sidebar is the real hook.** "The one thing that [tmux] never
  did well was tell me which agent needed me. In tmux I cycle through windows …
  the only way to find out is to go and look at each." — Matt Coles,
  [coles.codes](https://coles.codes/posts/herding-agents-with-herdr/). "Herdr
  gives you a shared screen: who's blocked, who finished, who's still working."
  — max_tokens, [maxtokens.ai](https://maxtokens.ai/posts/herdr-agent-multiplexer/).
- **Detach/reattach over SSH + single Rust binary (no Electron)** is the #2 loved
  trait. "The part that wins me over on a flaky ssh connection is detach and
  reattach … the most natural fit I've come across." — Matt Coles (ibid).
- **It beat cmux on cross-platform + remote** — the same wedge spyc claims:
  > "a week ago I was using cmux but its osx only and doen't work on remote
  > terminals. then I switched to herdr" — cultofmetatron,
  > [HN](https://news.ycombinator.com/item?id=48220211)
- **Young-project caution is universal** — both independent reviewers keep tmux
  as a fallback. "It's brand new, v0.1.x … you're betting on something young, so
  I've kept my tmux config around as a fallback." — Matt Coles (ibid).
- **No sandboxing is a noted gap;** one user is openly tempted to a non-terminal
  tool over it. "i'm currently using herd[r] … along with Nono for sandboxing …
  I do think about something that works outside the terminal may be nicer." —
  jpeeler, [HN](https://news.ycombinator.com/item?id=48424303).

### The decisive signal: its two headline features are its two biggest bug clusters

herdr's issue tracker (surveyed via `gh`, 2026-06-26) tells the strategic story.
Of the last ~120 closed issues, **46+ are agent-detection failures** — because
detection is **screen-scraping** the agent's terminal UI (OSC titles + Braille
spinner glyphs + body text). Every agent-side cosmetic change or non-standard
launcher silently breaks it:

- Claude moved its spinner into the body → status stuck
  ([#671](https://github.com/ogulcancelik/herdr/issues/671),
  [#634](https://github.com/ogulcancelik/herdr/issues/634),
  [#673](https://github.com/ogulcancelik/herdr/issues/673)).
- nix/Happy-wrapped Claude is invisible to process-name matching
  ([#803](https://github.com/ogulcancelik/herdr/issues/803),
  [#773](https://github.com/ogulcancelik/herdr/issues/773)).
- On **Linux, live agent status was a no-op entirely** until v0.7.0
  ([#656](https://github.com/ogulcancelik/herdr/issues/656),
  [#655](https://github.com/ogulcancelik/herdr/issues/655)) — detection is
  macOS-first.

Because herdr **owns a full terminal emulator**, it also inherits every VT/PTY
bug a multiplexer has — and the worst one is a clean spyc win: `less` / `man` /
git-pager are *"unusable inside panes"* because OSC 2/7 leak as literal text
([#816](https://github.com/ogulcancelik/herdr/issues/816)). And the daemon
architecture bites in macOS-specific ways — the live-handoff server loses
responsible-process attribution, breaking the 1Password CLI forever
([#808](https://github.com/ogulcancelik/herdr/issues/808)). Notably, herdr users
are *requesting* the `<repo>.worktrees/<branch>` sibling layout
([#261](https://github.com/ogulcancelik/herdr/issues/261)) that spyc already does
natively, and the most-discussed open feature is a re-think of the agent panel to
lead with *agent + task*, not folder
([#222](https://github.com/ogulcancelik/herdr/issues/222), 10 comments).

### Strategic read

**What herdr validates (good news):** the cross-platform Rust-TUI bet, the
terminal-native trust pitch, and — independently — the *"agents drive the
manager"* pattern (its socket API is the same idea as spyc's MCP bridge). The
demand for "which agent needs me" is now proven, not hypothetical.

**Where herdr beats spyc (be honest):**
- **Persistent detach/reattach of *running* processes** (daemon + live handoff).
  spyc is single-process; `-r` restores tabs + agent conversations, not live
  PTYs. This is a structural advantage we cannot claim parity on — and **should
  not chase**: a daemon fights spyc's single-process MVU/sync core (see §6).
- **A live, multi-pane agent-status panel shipped.** spyc's is still on the
  backlog. This is a shared gap where herdr is ahead in *ambition*.

**Where spyc wins (the sharpened moat):** all three confirmed absent from herdr —
- **File manager.** herdr is panes/tabs/workspaces; spyc *is* a vi file manager
  (git gutter, marks, harpoon, picks, frecency). Clearest unique identity.
- **In-process diff/review loop.** herdr's own marketing concedes it ("Conductor,
  Emdash, Superset … review diffs; Herdr orchestrates live terminals and agent
  state"). Unmet need #3 is now **uncontested** in the TUI lane.
- **MCP.** herdr's socket is proprietary and drives *its own* panes; spyc speaks
  the protocol agents already discover natively, and grounds the agent in *your
  view* (cwd/cursor/picks/git) — a different, richer axis.

**The one insight that turns herdr's weakness into spyc's feature:** herdr's
status detection is fragile *because it screen-scrapes*. spyc already owns an MCP
server and already lazily writes per-agent config (`ensure_agent_mcp_config`).
Building agent-status on the **MCP/hook channel** — a cooperative agent
*self-reporting* working/blocked, plus a tunable scrape *fallback* — dodges the
entire #671/#803 fragility class. That is how spyc wins a feature herdr struggles
to keep working. The implementation plan is **`docs/archive/AGENT_AWARENESS_PLAN.md`**.

---

## 1b. psmux — a multiplexer, not a manager (does not move the thesis)

[psmux](https://github.com/psmux/psmux) markets itself as **"the native Windows
tmux. Born in PowerShell, made in Rust."** It is a faithful tmux clone for
Windows (ConPTY, reads `.tmux.conf`, 83 tmux commands, themes, session
persistence) — for people who want tmux without WSL. Its one agent-adjacent
feature: when **Claude Code** spawns teammate agents inside a psmux session,
they render in **separate panes** instead of in-process. No file manager, no
MCP, no git awareness, no context-sharing, no agent status/notifications.

**It does not alter spyc's thesis** — it's a different *layer* on a different
*platform*, missing exactly what spyc *is*:

| | psmux | spyc |
|---|---|---|
| Core | terminal multiplexer (host many shells) | keyboard-driven **file commander** |
| Agent value | *hosts* agent panes (paning) | agent **sees your context** via MCP (cursor / picks / git / worktrees) |
| Platform | Windows-native (anti-WSL) | macOS / Linux (WSL-only on Windows) |
| Moat | tmux fidelity | *the file commander is the noun the agent operates on* |

The only overlap — **hosting an agent in a pane** — is the one thing spyc
explicitly does *not* try to win. spyc's embedded pane is a dogfooding vehicle,
not the product; multiplexing is commoditized (tmux, wezterm, psmux, and Claude
Code's own `teammateMode`). spyc's bet is orthogonal: not "run more agent
panes," but "the one agent you're pairing with operates on your live working
tree without you describing it." A psmux user could still want that bridge; you
could even run spyc's agent pane *inside* psmux. Complementary layers.

**What it signals (reinforcement, not a pivot):**
- **"Agents in panes" is now a trend** (psmux + Claude Code teammate-mode). That
  *validates* spyc's direction and confirms the multiplexer layer is being
  commoditized — so the guardrail holds: **don't over-invest in pane/multiplexer
  features; the differentiator is the MCP context bridge + agent-awareness.**
  (Same conclusion the charter reached parking P2 orchestration to "primitives
  only, no auto-spawn.")
- **Windows is a real gap** psmux fills natively while spyc stays WSL-only —
  reinforces, doesn't threaten, spyc's platform positioning.

**Minor compatibility note:** Claude Code's teammate feature spawns teammates
into the *host multiplexer* (tmux/iterm2). Running multi-agent Claude Code
*inside spyc's pane* (which isn't tmux) falls back to in-process — teammates
won't get their own panes. Not a thesis issue, but a "known interaction" if spyc
is ever marketed for multi-agent work: spyc is single-pair-agent-focused by
design; the teammate grid is the multiplexer's job (grid multiplexer is
explicitly out of scope per the charter).

---

## 1c. claude-code-ide.el — the Emacs convergence (validates the shape, not a threat)

The recurring joke when showing spyc around: *"aren't you just recreating
Emacs?"* Worth taking seriously for a minute, because the Emacs ecosystem has in
fact converged on something architecturally close —
**[claude-code-ide.el](https://github.com/manzaltu/claude-code-ide.el)** (1.6k★,
109 forks, active). It:

- Runs Claude Code in a **hosted terminal pane** inside Emacs (vterm / eat /
  ghostty backend) — the same "agent lives in a pane" shape as spyc.
- Bridges via **MCP, bidirectionally** — and critically, **pushes live context
  to Claude automatically**, without the user re-describing state: current
  file, selection, buffer diagnostics (Flycheck/Flymake), project info. That is
  spyc's entire thesis, restated with Emacs as the host instead of a file
  commander.
- Exposes Emacs-native tools back to Claude (`xref-find-references`,
  tree-sitter info, `imenu` symbols, project metadata) and lets Claude execute
  Elisp directly via an `executeCode` tool.

A second, differently-shaped project — **emacs-claude-agent** — skips the
terminal pane entirely: a chat-buffer agent with a tool registry that includes
an `emacs-dired` tool (file ops, pattern search) alongside git/shell/project
tools. Structurally that's "an agent that can drive dired," closer to a
tool-calling assistant than a file-commander-with-a-guest-agent. The more
common Emacs pattern (`aider.el`, `gptel`, `ai-code-interface.el`) is simpler
still: mark files in dired, manually add them to the agent's context — a
push-button action, not a live bridge.

**Read on this: validation, not a scoop.** Two independent lineages — a
40-year-old Lisp editor and a from-scratch Rust TUI — arrived at the same three
primitives for agent integration: **(1)** host the agent in a pane, **(2)** a
context bridge that pushes state to the agent automatically, **(3)** let the
agent act back on the host. That convergence is evidence the shape is right,
not evidence spyc is redundant.

**Where it actually diverges — the substance of the rebuttal:** in every Emacs
variant, **the editor/buffer is the noun and the agent bolts onto it**; dired is
a mode you visit, not the persistent driving interface, and vi-style modal
navigation is a separate package (`evil-mode`) layered on top of decades of
editor-first defaults. spyc inverts the centre of gravity —
*"the file commander is the noun the agent operates on"* — the file commander
is primary, an editor is optional-to-absent, and vi-modal navigation is the
whole interaction model from the start, not a retrofit. Recreating spyc in
Emacs means layering `evil-mode` + `claude-code-ide.el` + a from-scratch
dired-as-primary-UI + a custom activity/notification system on top of an
editor whose entire ideology is "everything is a buffer" — which is to say,
you'd end up writing spyc, in Emacs Lisp, fighting the host's bones the whole
way. It's the 30-year-old joke — *"Emacs: a great operating system, lacking
only a decent editor"* — taken seriously and built on purpose, in Rust,
without the Lisp tax.

---

## 1d. Yazi — the file-manager lane (adjacent axis, not agent-competitive)

The sections above cover the *agent-manager* lane. spyc also lives in a second
lane — the **TUI file commander** — and its closest neighbour there is
**[Yazi](https://github.com/sxyazi/yazi)** (~41.3k★ as of 2026-08-12, Rust; latest
stable **still v26.5.6 / 2026-05-05** — re-verified 2026-08-12, so no significant
release since the 2026-05-28 pass despite the star growth — this
fold tracks *spyc-side* changes: Lua scripting, mermaid rendering, and
agent-awareness all shipped since). `ROADMAP.md` already cites Yazi as the
gold-standard "reputable, install-and-rely-on TUI tool" we benchmark launch
hygiene against, and carries four Yazi-inspired entries (bulk rename, cwd
export, visual-mode range pick, structured event stream). Yazi is **not in the
agent-manager game** — no MCP, no agent concept — so it doesn't move the wedge;
it sets the bar for the *file-manager half* of spyc's identity. spyc's one-line
distinction: a two-pane file commander whose distinguishing feature is a local
MCP socket the bottom-pane agent (Claude Code / Codex / Gemini) calls into —
**the file commander is the noun the agent operates on. Yazi is not in this
game.**

### Yazi's headline recent move: PR #4005 — drag and drop

Merged 2026-05-28 via **kitty's OSC 72 DnD protocol**: drag files out to other
apps, drop files in from external apps; kitty 0.47.0+ only (the only terminal
implementing OSC 72 today); exposes Lua-facing `rt.tty` queue/flush APIs + DnD
event bindings, ships a preset `dnd.lua` plugin (~50 files across
`yazi-tty`/`yazi-term`/`yazi-shim`/`yazi-scheduler`/`yazi-plugin`/`yazi-binding`/`yazi-actor`).

**Implication for spyc:** `ROADMAP.md`'s "drag and drop" entry predates the
OSC 72 spec and is stale in two ways — OSC 52 is *clipboard*, not DnD (it was a
placeholder); OSC 72 is the actual protocol but **kitty-only**, and spyc's macOS
users are predominantly iTerm2/Ghostty/Terminal.app, so the kitty-gated payoff is
small. Defer native DnD until ≥1 more terminal ships OSC 72; the path-paste
fallback is an independent, cheap win that can ship on its own.

### Feature-by-feature standing

Columns: **Yazi** = ships it; **spyc** = ships it / on roadmap / out of scope.
Roadmap pointers cite `ROADMAP.md` by file only (line numbers go stale fast).

**Core capabilities**

| Capability                             | Yazi    | spyc                                   |
|----------------------------------------|---------|----------------------------------------|
| Async I/O, multithreaded scheduler     | Yes     | Heavy work is off-thread (git-status walk, file ops, previews/git-view, fs-watch, pager streams, watcher-triggered listing refresh); the on-navigation `Listing::read` stays synchronous — background loading for 100K+ dirs is roadmapped, 50k-entry cap today |
| Image preview (kitty/iTerm2/sixel)     | Yes     | No *general* image preview, but **mermaid diagrams render as terminal-graphics images** in the pager (v1.58.11) |
| Code preview / syntax highlighting     | Yes     | Yes (tree-sitter, v1.50.61) |
| Markdown rendered preview              | No (?)  | Yes (v1.26.0); fenced `mermaid` blocks render as diagrams (v1.58.11) |
| Multi-tab                              | Yes     | Yes                                    |
| Trash bin                              | Yes (single tier) | Yes — two-tier: in-app **graveyard** (compressed, undo-able) cascading to system trash (`src/state/graveyard.rs`) |
| Archive extraction                     | Yes     | Not implemented                        |
| Bulk rename via `$EDITOR`              | Yes     | Roadmap                                |
| Visual-mode range select               | Yes     | Roadmap                                |
| Cwd export on quit                     | Yes (`y` wrapper) | Roadmap (Foundations queue)  |
| Drag and drop (OSC 72)                 | Yes (PR #4005) | Roadmap, stale — see above    |
| `fzf`/`fd`/`ripgrep`/`zoxide` integration | Yes  | `F` finder, `:grep`, frecency `J` (homegrown) |
| Mouse support                          | Yes     | Yes — **default on** (`[mouse] capture`): wheel scrolls whatever is under the pointer, left/middle/right buttons, drag-select in four surfaces, `:mouse on\|off\|auto` *(row corrected 2026-08-12 — the previous "explicit non-goal" predated the mouse campaign)* |

**Extensibility**

| Capability                             | Yazi    | spyc                                   |
|----------------------------------------|---------|----------------------------------------|
| Lua scripting / plugins                | Yes — plugins + package manager (BETA) | **Lua scripting shipped** (`map KEY lua`, `init.lua` platform, `spyc.on` events, full-`Action` `spyc.action`; off-thread worker, `src/lua/`, v1.84–1.89). No plugin *package manager* (deliberate). |
| Theme system / "Flavors"               | Yes (BETA) | Themes in `.spycrc.toml` + nerd-font/mono toggle; no installable-theme package system |
| Custom previewers / fetchers           | Yes (Lua) | Not exposed (spyc's Lua drives nav/actions/reads, not previewers) |
| Keymap customization                   | Yes (`keymap.toml`) | Yes (`.spycrc.toml`)         |
| Package manager for plugins/themes     | Yes     | No — Lua *scripting*, not a plugin/theme package ecosystem |

**Automation / external integration**

| Capability                             | Yazi    | spyc                                   |
|----------------------------------------|---------|----------------------------------------|
| Event publish (`ya pub`)               | Yes     | **Explicit non-goal** — wider attack surface, off thesis |
| Event subscribe (`--local-events`/`--remote-events`) | Yes | **Partial** — `spyc.on(startup\|dir_changed\|project_changed\|agent_status)` Lua event hooks (in-process, v1.89); a subscriber socket on the MCP UDS stays roadmap |
| Cross-instance state distribution      | Yes ("Data Distribution Service") | Per-PID MCP socket; instances coexist but don't share state |
| MCP server (agent-callable)            | **No**  | **Yes — core thesis** |
| Virtual filesystem for remote files    | Yes     | Out of scope                           |

**Distribution / hygiene** (all spyc entries are the 2.0 "Launch plan")

| Capability                             | Yazi    | spyc                                   |
|----------------------------------------|---------|----------------------------------------|
| Pre-built binaries on tagged release   | Yes     | Roadmap for 2.0  |
| Homebrew tap                           | Yes     | Roadmap for 2.0  |
| Signed artifacts                       | Partial | Minisign roadmap |
| Docs site                              | Yes (yazi-rs.github.io) | Single-file `*.md` — deferred |
| Migration page from peer tools         | Yes (per-tool keymap tables) | Roadmap |
| GitHub presence                        | Yes (~39.9k★) | Private `Tripstack-Corp/spyc` (dev + CI on GitHub); goes public at the 2.0 launch |

### Where Yazi clearly leads

1. **Image preview.** Yazi treats it as a headline feature with multiple
   protocols; spyc has none (mermaid aside). A real gap for image-heavy users —
   acknowledge it plainly in the eventual migration page.
2. **Plugin ecosystem.** Yazi's Lua plugins + package manager mean third parties
   ship previewers, fetchers, themes. spyc's non-goal stance is a deliberate
   trade — frame the *value of not having* a plugin API (single-binary
   stability, no plugin-API churn).
3. **Mass.** ~39.9k★ vs. our pre-launch numbers. Not a feature, but it changes
   the "is this safe to install?" calculus — the 2.0 launch-hygiene pass is the
   response.
4. **Drag and drop (as of today).** kitty-only and Lua-gated, but they're first
   in this corner of the design space.

### Where spyc clearly leads

1. **MCP bridge.** Yazi's automation story is `ya pub` / event streams into
   custom shell scripts; spyc speaks MCP — the protocol Claude Code / Codex
   already speak. No glue code on the user's side.
2. **Two-pane agent pairing as a first-class layout** (Yazi approximates it via
   plugins; spyc ships it as the default UX).
3. **Picks / inventory as a structured selection model** the agent reads via
   `get_spyc_context` — selection becomes a data structure the agent acts on,
   not just highlighted rows.
4. **Session save/restore for both Claude and Codex** — resume a spyc and the
   agent panes resume too. Out of Yazi's scope by design.
5. **Git-aware listings + frecency `J`** as built-ins (Yazi gets them via
   plugins / external tools).
6. **Two-tier delete (graveyard → system trash)** — compressed, undo-able from
   inside spyc (`p`/`P`), FIFO-cascading to trash at a cap. "I deleted the wrong
   thing" is one keystroke from recovery with no Finder context switch.
7. **Live agent-status awareness** — per-pane working/blocked/done dot from an
   MCP/hook self-report channel + desktop notifications on the transition. Yazi
   has no agent concept at all. Charter `docs/archive/AGENT_AWARENESS_PLAN.md`.

### Where we deliberately differ

- **Plugin *package ecosystem*.** spyc shipped Lua *scripting* — but not a
  plugin *marketplace* like Yazi's. The extensibility surface is in-process
  scripting + the MCP bridge, not a third-party plugin API to version.
- ~~**Mouse beyond current** — non-goal; keyboard-first by thesis.~~ **No longer a
  non-goal** (corrected 2026-08-12): real mouse reporting shipped and is *default
  on* — wheel, buttons, drag-select, `:mouse` controls. Keyboard-first remains the
  thesis (every mouse action has a keyboard path), but "mouse is a non-goal" is a
  stale claim. `ROADMAP.md`'s non-goal entry and `DESIGN.md`'s "mouse is a
  courtesy" line were corrected in the same pass — the surviving non-goal is a
  mouse-**first** UI, not mouse support.
- **Event publishing (`ya pub` equivalent)** — non-goal; the consumer ecosystem
  we care about is agent-flavoured, not a generic automation bus, and accepting
  arbitrary publishes from anywhere on the box isn't worth the attack surface.
- **Virtual filesystem for remote files** — out of scope; ssh + local mount is
  the supported workflow.
- **Localization** — English-only by stated non-goal.

### Recommendations (Yazi-specific)

1. **Update `ROADMAP.md` (drag and drop):** replace OSC 52 with OSC 72, cite
   Yazi PR #4005, note it's kitty-only today and defer until ≥1 more terminal
   adopts OSC 72; keep the path-paste fallback as an independent cheap win.
2. **Add an "image preview" row to the migration page** with honest framing:
   "Yazi has it; spyc doesn't; if you live in image-heavy directories, Yazi may
   suit you better." Hiding the gap burns trust faster than naming it.
3. **Lead the migration page's differentiator paragraph with MCP/Claude
   pairing**, then picks-as-data-structure, then session save/restore — the
   three things Yazi cannot match without changing what it is.
4. **Extensibility stays scripting + MCP, not a plugin marketplace.**
5. **Re-run this fold** when Yazi cuts its next significant release or a second
   terminal adopts OSC 72 (whichever comes first). The four Yazi-inspired
   `ROADMAP.md` entries stay as the per-feature design notes; this section is
   the synoptic view.

---

## 1e. lf — the delegation-lane minimalist (and the lane's only external control surface)

[lf](https://github.com/gokcehan/lf) ("list files") — a Go terminal file manager
with, in its own words, *"a heavy inspiration from `ranger`"*. MIT, **9,459★** /
374 forks, created 2016-08-13, **1,574 commits**, latest release **r42
(2026-07-31)**, 57 open issues / 25 open PRs. Counts verified 2026-08-12 via the
GitHub API; the feature surface below is read from lf's own `doc.md` (2,309 lines)
and `CHANGELOG.md` at master.

lf is emphatically **not** an agent tool — a grep for `mcp|llm|AI|claude|agent`
across `doc.md` + `CHANGELOG.md` returns **zero hits**. It earns a section anyway
for two reasons the Yazi fold doesn't cover: it is the only tool in the
file-manager lane that already ships an external **read+write control surface**,
and its explicit **Non-Features** list is the precise philosophical inverse of
spyc — which makes it the cleanest available argument for *why* spyc integrates
instead of delegating.

### The governance story: it survived its founder going quiet

The most transferable fact about lf has nothing to do with features.
**gokcehan's last commit was 2024-03-31** — zero commits in the past 12 months —
while the repo took **333 commits since 2025-08-01** and shipped r42 twelve days
ago. Day-to-day maintenance sits with collaborators (joelim-work 306 commits,
CatsDeservePets 110, valoq 49), releases are cut by `github-actions[bot]`, and
issues get collaborator answers within days.

That is a useful correction to how this doc has been reading bus-factor risk. §1
dings supacode and herdr for being single-maintainer projects; lf shows the risk
isn't *"one maintainer"* — it's *"no succession path"*. lf still carries its
founder's name, but an in-repo `CONTRIBUTING.md`, commit rights handed to two
active contributors, and CI-cut releases were enough to keep it shipping through a
two-year founder absence. **spyc is a single-developer project with none of those
three.** Cheap insurance, and it belongs in launch hygiene rather than the someday
pile.

*(Coincidence worth noting so nobody "corrects" it later: lf's 1,574 commits is the
same figure as supacode's in §1. Both verified independently; lf's via the API's
`Link: rel="last"` page count.)*

### `lf -remote` — the closest analogue to spyc's MCP bridge in this lane

lf is **server/client**: clients connect to a server on startup (`--single` opts
out), and any process can drive a running instance.

| Surface | What it does |
|---|---|
| `lf -remote 'send [id] <command>'` | run any lf command in one client or all — **write** |
| `lf -remote "query $id <type>"` | dump `maps`, `nmaps`, `vmaps`, `cmaps`, `cmds`, `jumps`, `history`, `files` — **read** |
| `lf -remote 'list' \| 'quit' \| 'quit!'` | enumerate clients; shut the server down |
| `-print-selection` · `-print-last-dir` · `-last-dir-path` · `-command` | one-shot use as a file picker / cwd exporter for a wrapper script |

That is a bidirectional surface, and Yazi has no equivalent (`ya pub` is an event
bus, not a state read). **An agent could drive lf today** — a thin MCP shim over
`lf -remote` is roughly a weekend's work, and it is the cheapest path anyone has
into spyc's lane. Better to name it plainly here than to discover it later.

Why MCP still wins, specifically:

- **Discovery.** MCP tools are self-describing; Claude Code and Codex enumerate
  them from `initialize` with zero user setup. `-remote` is a shell-string
  vocabulary the *user* must teach the agent, per project, forever.
- **Types.** `query` returns pager-shaped text meant for a human —
  `cmd maps $lf -remote "query $id maps" | $PAGER` is the documented usage. spyc
  returns structured results the agent consumes without a scraper.
- **The wrong nouns.** `query` exposes keymaps, command definitions, the jump
  list, history and the file list. It cannot answer *where is the cursor*, *what
  is picked*, *what is the git state*, *which worktree*, *what is `PROJECT_HOME`* —
  the questions `get_spyc_context` exists to answer. lf's surface is built to
  **synchronize lf instances**, not to ground an outside reader in a human's view.
- **No thesis.** Zero agent/MCP/LLM mentions anywhere in its docs. The primitive
  exists; the intent doesn't.

**Moat read:** the socket was never the moat — the *integrated* surface behind it
is (git gutter, worktree lifecycle, review loop, picks semantics, agent status). An
MCP-shimmed lf would give an agent a cursor it can move and a file list it can
read. It would not give it a review loop.

### Non-Features: delegation vs integration — the actual strategic core

lf's README lists three **Non-Features**, verbatim:

> "Tabs or windows (better handled by window manager or terminal multiplexer)
> - Builtin pager/editor (better handled by your pager/editor of choice)
> - Builtin commands for file operations (better handled by the underlying shell tools)"

All three are things spyc deliberately **has**: pane tabs plus a second commander,
an in-app pager (ANSI / hex / syntax / markdown / mermaid / diff), and built-in
file ops with a graveyard. Read as a design disagreement, lf is right on classical
Unix grounds and spyc looks like scope creep. Read against the thesis, it inverts —
and this is the sharpest way to state spyc's position anywhere in this document:

> **Every delegation boundary is state that leaves the file manager's model, and a
> context bridge can only expose what the model knows.** Hand the pager to
> `$PAGER` and no bridge can tell an agent what the user is reading. Hand file ops
> to `cp` and there is no operation to report, undo, or scope-claim. Hand tabs to
> tmux and "which agent needs you" belongs to a process spyc doesn't own.

spyc's integration is therefore not feature-maximalism — it is the **precondition**
for the MCP bridge. lf's Non-Features are precisely the seams where an agent bridge
goes blind. It also pays off defensively: because spyc reviews in its own pager, it
structurally avoids herdr's *"`less`/`man` unusable inside panes"* bug class
([#816](https://github.com/ogulcancelik/herdr/issues/816), §1a).

**One precision note, since lf's README overstates it:** the third non-feature has
drifted from lf's own reference. `doc.md` documents a built-in copy/move — a custom
`paste` command is *"called when it is defined instead of the built-in
implementation"* — so lf's real position is "built-in ops, overridable by shell",
not "no built-in ops". `delete` is genuinely unbound by default, and deliberately:
*"By default, lf does not assign `delete` command to a key to protect new users."*
No trash tier either way.

### Feature standing (only where they meaningfully differ)

| Capability | lf | spyc |
|---|---|---|
| Inline image preview | **Yes — sixel, on by default** (the opt-in `sixel` option was *removed* as no longer required, CHANGELOG #2109); previewer-produced, cached in memory | No inline preview; full-screen **view** of PNG/JPEG/GIF/WebP by magic bytes, plus mermaid diagrams rendered as images |
| Multi-column layout | `ratios` (default `1:2:3`) — ranger-style parent/current/preview | vsplit preview + optional second commander (`^s`) — a different shape, not a port |
| Tabs | **Non-goal** (delegated to the multiplexer) | Pane tabs |
| In-app pager | **Non-goal** (delegated to `$PAGER`) | Yes — and load-bearing, see above |
| Archive browsing | **No — [#170](https://github.com/gokcehan/lf/issues/170) open since 2019-05-22** | Shipped — a mount is an *index, not a directory* (`src/archive/`) |
| Trash / undo | None; `delete` unbound by default | Two-tier graveyard → system trash, `p`/`P` restore, `:undo` |
| Bulk rename | Via an external tool (users reach for `edir`) | Roadmap |
| Cwd export on quit | **Yes** — `-print-last-dir` / `-last-dir-path` + `lfcd` wrappers | Roadmap (post-2.0 papercut queue) |
| Selection → outside consumer | `-print-selection` → a script's stdout | Picks → an **agent**, via `get_spyc_context` |
| External control surface | `-remote send`/`query`/`list` — untyped text | MCP — typed, discoverable, agent-native |
| Hook / event surface | **9 hooks**: `on-init`, `on-cd`, `pre-cd`, `on-load`, `on-select`, `on-redraw`, `on-focus-gained`, `on-focus-lost`, `on-quit` | 4 Lua events: `spyc.on(startup\|dir_changed\|project_changed\|agent_status)` |
| Mouse | `mouse` bool, **default false** | **Default on**; wheel + buttons + drag-select |
| Config model | `lfrc` — a shell-command DSL (`cmd`, `map`, and `$`/`&`/`%`/`!` command types) | `.spycrc.toml` + Lua scripting |
| Windows | **Native** | WSL only |
| Agent / MCP | **None** | Core thesis |

### Where lf leads

1. **Windows-native**, which spyc is not — and a documented reason people pick it:
   > "Unfortunately nnn and Ranger don't work on Windows … edit: Looks like lf
   > works on Windows" — rahen,
   > [HN](https://news.ycombinator.com/item?id=32436331)
2. **Hook surface breadth** — nine events to spyc's four. `on-quit`, `on-select`
   and focus-gained/lost have no spyc equivalent. Cheap to steal; see below.
3. **Sixel previews on by default**, where spyc has no inline preview at all.
4. **Startup and footprint**, consistently the user framing:
   > "It's a lot faster in all aspects, has mostly the same features and is pretty
   > much a standalone binary." — thworp,
   > [HN](https://news.ycombinator.com/item?id=40298792)
   >
   > "Then learned about lf, a ranger clone written in go, which is much faster.
   > That is what I use now." — 29athrowaway,
   > [HN](https://news.ycombinator.com/item?id=25127212)
5. **Proven succession** (above) — something spyc has not demonstrated.

### Where spyc leads

Archive browsing (lf's own seven-year-old open request), the in-process review
loop, the graveyard, worktree lifecycle + git-status gutter, agent-status
awareness, and the MCP thesis. Against lf the delta is *wider* than against Yazi:
lf declines by design most of what spyc integrates, so the comparison is a
disagreement about scope rather than a feature gap either side is racing to close.

### Mindshare: lf is being displaced by Yazi in the very threads that recommend it

Worth tracking, because it runs on the same lever §5 identifies. lf's discovery is
organic recommendation inside adjacent shell-workflow threads — which is exactly
where the lead for this section came from ("A shell exclamation mark is not for
yelling. Be lazy", 2026-08-06):

> "If you really want to be lazy, use a terminal file manager (I prefer lf) it
> helps avoiding the cd && ls dance. That way, your history only contains commands
> you actually would want to rerun." — RMPR,
> [HN](https://news.ycombinator.com/item?id=49268693)

But in threads where lf gets named, Yazi is increasingly the reply:

> "> Also lf. … Check out yazi, its the nicest TUI file manager I have used" —
> ac29, [HN](https://news.ycombinator.com/item?id=43419584)
>
> "my favorite is yazi since it has great preview capabilities out of the box and
> **requires zero config**, but other alternatives are nnn, ranger, walk and lf."
> — yuvadam, [HN](https://news.ycombinator.com/item?id=45272749)

**"Requires zero config" is the knock, and it is the actionable one.** lf's ethos —
*"Extendable and configurable with shell commands"* — means a new user wires up a
previewer before the tool feels good, and it visibly costs lf recommendations to a
peer that works on first run. spyc's shipped defaults + chord-hint discovery are
the right instinct; **protect first-run quality as a launch requirement, not a
polish item.** The counter-voice is real and also verified — minimalism is a
feature to some users:

> "This looks neat, but has a lot going on. I really like how minimalist yet
> extensible lf is and just use edir to rename files in bulk." — monopoliessuck,
> on the superfile thread,
> [HN](https://news.ycombinator.com/item?id=40323990)

### The rest of the lane, in one line

So the "what about X?" question doesn't stay open. Star counts verified 2026-08-12:
**superfile** 22.5k★ (Go), **nnn** 21.8k★ (C), **ranger** 17.3k★ (Python — the
ancestor), **broot** 12.9k★ (Rust, tree-oriented), **lf** 9.5k★, **joshuto** 3.7k★
(Rust), **walk** 3.6k★ (Go, last pushed 2026-01-06). Yazi (41.3k★) leads the lane
on polish; lf leads it on external control; **none of them ship an agent bridge.**
The file-manager lane is crowded and mature, which is exactly why spyc's wedge is
the agent axis (§4) and not file-manager feature parity.

### Recommendations (lf-specific)

1. **Steal the hook vocabulary.** `on-quit`, `on-select`, `on-focus-gained` /
   `on-focus-lost` are cheap additions to `spyc.on`, and lf has already proven the
   naming. Focus events pair naturally with agent-status ("the user looked away"
   is a meaningful input to notification gating).
2. **Promote cwd-export out of the papercut bucket.** Both peers ship it (Yazi's
   `y` wrapper, lf's `-print-last-dir`) and it is the single most-cited lf
   integration in user voices:
   > "I have it integrated into zsh so the current directory is whatever dir I was
   > in when exiting lf." — nvllsvm,
   > [HN](https://news.ycombinator.com/item?id=41797442)
3. **Do not build a `-remote`-style generic socket.** Consistent with the `ya pub`
   non-goal in §1d: MCP is the same surface with discovery and types, and a second
   untyped control plane is attack surface for no thesis gain.
4. **Add lf to the migration page beside Yazi.** The honest pitch to an lf user:
   *keep* vi keys and a single static binary; *gain* worktrees, review, graveyard,
   archive browsing and the agent bridge; *give up* Windows-native and inline sixel
   previews.
5. **Fix the bus factor.** lf is the working model — CONTRIBUTING in-repo, a second
   committer, CI-cut releases. Fold into launch hygiene.
6. **Re-run this section** when lf cuts r43, or the moment anyone publishes an MCP
   shim over `lf -remote`. That shim is the entry vector into spyc's lane, and it
   would show up on GitHub well before it shows up in an HN thread.

---

## 1f. The session layer — Superlogical and zmx (2026-09-04)

A second wave forming under the manager category: not agent managers at all,
but the **session layer** those managers sit on. Recorded here because it is
the closest thing to a structural challenge spyc has, and because it sharpens
the thesis by contrast rather than by overlap.

> **Provenance.** Unlike §1a–§1e, this section was not produced by the mine-and-
> verify pass described in the Method note. It records the facts as supplied in
> the strategy brief that prompted it — launch date, people, backers, licence
> basis, shipping status, and stated architecture. Nothing here is a fetched
> quote, and no claim is made about either project's unshipped internals. Apply
> the document's normal standard when either ships something to read.

**[Superlogical](https://superlogical.com)** launched **2026-07-29** — Mitchell
Hashimoto's new company, founded with Jack Pearkes, Alasdair Monk and Hector
Simpson, backed by Notable Capital and Amplify Partners. The stated aim is a
**server-side terminal multiplexer** built on MIT-licensed **libghostty** — "the
multiplexer for all work." As of 2026-09-04 it is **waitlist only; nothing has
shipped**, so there is no surface to evaluate and none is guessed at here.

**[zmx](https://github.com/neurosnap/zmx)** (Zig) shipped the unbundled form of
what tmux's persistence actually is: a **daemon per session** over a unix
socket, passthrough to the PTY, with **libghostty-vt as a shadow state machine**
used only to rehydrate a client on reattach. Deliberately **no windows, no tabs,
no splits** — persistence without the multiplexer wrapped around it.

**Why this does not move the thesis — and what it confirms.** Both share a
*screen* with the agent: cells, bytes, scrollback to scrape. spyc shares the
*working set* — cursor, picks, inventory, filter, branch, worktree — over MCP,
as state that already means something. A better multiplexer underneath spyc is
not a competitor to that; it is a better floor.

Where it does touch the roadmap is durability. zmx is the useful proof that the
persistence half is separable from the multiplexer half, which is the shape
spyc's 3.0 horizon takes: one headless process, one attached client, no
mirroring and no protocol ambitions (see ROADMAP.md). The funded, general-purpose
version of that category is Superlogical's fight, and ROADMAP.md's non-goals now
say so explicitly.

**Watch conditions.** Superlogical shipping anything at all — the first release
is the first evaluable fact. zmx growing a context or agent-integration surface,
which would move it out of the session layer and into this category.

---

## 2. What users love — validated

These features are actively praised and are now table stakes; spyc should match
or beat each.

- **Vertical / list-oriented tabs.** "Vertical tabs for a terminal emulator
  seems killer, I'll be trying this out for sure." — mikkupikku,
  [HN](https://news.ycombinator.com/item?id=47086714). *Many-pane work needs a
  list layout, not a horizontal tab strip — spyc's list-driven model is the
  right shape.*
- **Instant worktree creation.** "Since there's very little friction to spinning
  up a worktree (~2s), I open one for any small tasks." — hoakiet98,
  [HN](https://news.ycombinator.com/item?id=46368739). *Sub-2s creation changes
  behaviour; keep spyc's in-process gix create instant.*
- **Programmability / scriptability.** "Love the ideas here, specifically: the
  programmability … I haven't tried it yet, but had been considering learning
  tmux partly for this." — johnthedebs,
  [HN](https://news.ycombinator.com/item?id=47083596). *spyc's MCP socket + CLI
  control surface is exactly this.*
- **"Does one thing really well."** "All I wanted was a simple way to run a bunch
  of Claude codes in parallel. This feels like just a nice clean simple
  extension of how Claude code already works." — SOLAR_FIELDS / simonbw,
  [HN](https://news.ycombinator.com/item?id=44594584). *Stay a primitive; don't
  become a workflow engine.*

---

## 3. Unmet needs → roadmap, ranked

Ranked by **demand × how well spyc is already positioned to win it**. Each item
carries a verified quote.

### 1. Worktree bootstrap — bring the untracked files *(demand: highest · effort: medium)*

The single most recurring gripe in the category: a new worktree doesn't carry
`.env`, `node_modules`, or other untracked/ignored deps, so every worktree needs
manual setup before an agent can even run tests. Stateful deps (DBs, ports,
services) are unsolved across every tool. spyc's `clean_worktree`/graveyard
machinery is the starting point.

> "…they don't include things that are not tracked by git — e.g.
> .env.development.local … starting a new worktree requires additional setup and
> isn't as simple as just checking out a new branch." — _1tem / pjm331,
> [Conductor HN](https://news.ycombinator.com/item?id=44594584)

> "Most of these agents solutions are focusing on git branches and worktrees, but
> at least none of them mention databases." — 101008,
> [HN](https://news.ycombinator.com/item?id=46368739)

### 2. Agent attention & status — reliably *(demand: highest · effort: medium · asset: in-process pane parsing)*

"Which agent needs me?" is the founding pain of the category — and even the
leader does it unreliably (hook-vs-OSC races, blank sidebars when hooks stop).
spyc parses pane output in-process on one message channel: a single
deterministic path is a real reliability edge. Corroborate hooks with PID
liveness and PTY state; never trust hooks alone.

> "But Claude Code's notification body is always just 'Claude is waiting for your
> input' with no context, and with enough tabs open I couldn't even read the
> titles anymore." — lawrencechen (cmux creator),
> [HN](https://news.ycombinator.com/item?id=47079718)

> "Sometimes the notification fires immediately after Claude finishes its work;
> other times there's a noticeable delay (seconds to tens of seconds)." —
> austinywang (cmux), [#2322](https://github.com/manaflow-ai/cmux/issues/2322)

> cmux had to add a PID + process-state fallback because "when structured agent
> hooks stop updating set_status, the sidebar can go blank even though a live
> agent PID is still registered." —
> [cmux #3751](https://github.com/manaflow-ai/cmux/issues/3751)

**herdr update (now competitively urgent):** herdr *shipped* this — a live
blocked/working/done/idle sidebar — and it's their #1 loved feature. But they
build it by **screen-scraping** the agent's terminal UI, so it's their #1 bug
source (46+ detection issues; broke when Claude moved its spinner; invisible to
nix/Happy wrappers; was a no-op on Linux until v0.7.0). The differentiator is no
longer "spyc has agent-status" — it's **"spyc does it reliably."** spyc's win:
build it on the **MCP/hook channel** (cooperative agents *self-report* via the
MCP config spyc already writes) with a tunable scrape *fallback* — dodging
herdr's entire fragility class. This is the spine of `docs/archive/AGENT_AWARENESS_PLAN.md`.

### 3. The review loop — diff & merge across worktrees *(demand: high · effort: low, already built · asset: in-house gix diff/show/blame)*

The real ceiling isn't running agents — it's reviewing them. Users sit on 5–10
finished agents and serialize through merging. spyc's syntax-highlighted
side-by-side diff/show per worktree directly attacks this; make per-worktree
review a headline workflow.

> "I often have 5-10 agents with completed plans, and I'm just slogging through
> executing them one at a time." — senordevnyc,
> [HN](https://news.ycombinator.com/item?id=46368739)

> "The real bottleneck isn't human review per se, it's unstructured review.
> Parallel agents only make sense if each worktree has a tight contract." —
> amortka, [HN](https://news.ycombinator.com/item?id=46368739)

**herdr update (now uncontested):** herdr has **no** diff/review — its own
marketing concedes it ("Conductor, Emdash, Superset … review diffs; Herdr
orchestrates live terminals and agent state"), and Claude Squad doesn't either.
spyc's in-house side-by-side diff/show/blame (`gd`/`gD`/`gu`) is therefore the
**only** in-process review loop in the TUI lane. This should move to the *front*
of the wedge, not sit at #3. (Bonus: because spyc reviews in its own pager, it
sidesteps herdr's "`less`/`man`/git-pager unusable inside panes" OSC-leak bug,
[#816](https://github.com/ogulcancelik/herdr/issues/816) — a whole bug category
spyc structurally doesn't have.)

### 4. Session persistence that survives a hard kill *(demand: high · effort: medium · asset: existing session save / `-r`)*

cmux's "only reason I can't use it" blocker, open 2+ months: state lost on
non-graceful exit. Lesson for spyc — periodic autosaves must be
recovery-sufficient on their own, never a thin fallback to a quit-time flush that
`SIGKILL` truncates. Assume the process can die at any instant.

> "In the weekends I close work related apps, cmux included. And if my computer
> restarts, then I loose all current tabs and workspaces. … Kind of crazy this
> hasn't been prioritized. I'd like to recommend to people but it's hard to with
> this bug." — Seluj78 / clounie,
> [cmux #2823](https://github.com/manaflow-ai/cmux/issues/2823)

**herdr update (where herdr beats spyc — and why we don't chase it):** herdr
solves this *architecturally* with a detach/reattach daemon whose PTYs survive
client close and even a binary upgrade (live handoff via FD-passing). That's a
genuine advantage we can't claim parity on — but adopting a daemon would fight
spyc's single-process MVU/sync core, and herdr pays for it (macOS responsible-
process/TCC loss on handoff, [#808](https://github.com/ogulcancelik/herdr/issues/808);
a 7.8k-line headless server). **spyc's answer stays "resilient autosave, not a
daemon":** make periodic state saves recovery-sufficient against `SIGKILL`, and
capture richer layout + per-pane cwd so `-r` rebuilds faithfully. Win the 80%
(state survives) without the daemon's complexity tax. (Explicitly out of scope in §6.)

### 5. Resource awareness at scale *(demand: medium · effort: medium · asset: process-stat tracking)*

Many parallel agents OOM the machine with no backpressure — and Anthropic closed
the request as `NOT_PLANNED`, leaving it to tools. spyc already tracks
process/file state; a per-pane resource readout or a "too many agents" warning is
addressable and differentiated.

> "Spawn five parallel sessions on a 16 GB Mac Mini and you get OOM kills —
> there's no shared scheduler, no memory backpressure … Anthropic closed the
> maxParallelAgents feature request (#15487) as NOT_PLANNED." — xinhat
> (Centurion), [HN](https://news.ycombinator.com/item?id=47385034)

### 6. Many-pane layout without the squish *(demand: medium · effort: medium · asset: list-driven model)*

Tiled panes shrink to unusable past a handful of agents; users want
focus-grows / background-shrinks, or scrollable strips. And auto-reordering by
notification breaks keyboard muscle memory — a direct warning for spyc's
vi-navigation: **keep pane indices stable.**

> "When you keep appending vertical terminals to the right … They all get
> squished … I would love a horizontal scroll at the bottom." — snisarenko,
> [HN](https://news.ycombinator.com/item?id=47111737)

> "[Auto-reordering] makes the keyboard shortcut for a given conversation change
> all the time, which is a cognitive burden for me." — sltr,
> [HN](https://news.ycombinator.com/item?id=47091352)

---

## 4. Positioning — the wedge and the objections

### The one-line wedge

> The vi-driven **file manager** you already live in — that also spins up
> worktrees, watches what each agent is doing, and **reviews their diffs
> in-process** — without leaving your terminal or trusting a GUI with your repos.
> Terminal-native and cross-platform like the multiplexers, but a real file &
> review tool, not just a way to switch between agents.

> **vs the GUIs** (cmux/Conductor/supacode): cross-platform, local-first, no
> OAuth. **vs the Rust-TUI twin** (herdr) and Claude Squad: a *file manager* with
> an *in-process review loop* and an *MCP bridge* — none of which they have. Lead
> with file-manager + review; "terminal, cross-platform, Rust" is table stakes
> now, not the wedge.

Structural advantages back it — but note which competitor each one beats:

- **Cross-platform moat *(vs the GUI lane only)*.** The GUI leaders are
  macOS-only and can't easily cross it (Swift/AppKit); Linux/SSH/Windows users
  are locked out, and are spyc's natural early adopters. **Caveat: this does
  *not* differentiate from herdr or Claude Squad**, which are cross-platform too
  — against them the moat is file-manager + review, below. Demand vs the GUIs is
  so large the community shipped 4+ Linux forks of cmux:
  > "Since last week, I've fallen in love with cmux, however as a main linux
  > user the non support broke my heart. So I decided to build my own:
  > cmux-for-linux." — cai0baa,
  > [cmux #330](https://github.com/manaflow-ai/cmux/issues/330)
  >
  > "I don't deserve to own my Windows system." — countfeng,
  > [Conductor HN](https://news.ycombinator.com/item?id=44594584)
- **Local-first trust.** In-process gix, no GitHub OAuth, no argv rewriting of
  the agent. Both Conductor and cmux took trust hits spyc structurally avoids:
  > "Full read-write access required to all your Github account's repos. Not just
  > code. Settings, deploy keys. The works… you're asking for the keys for the
  > kingdom." — itsalotoffun,
  > [Conductor HN](https://news.ycombinator.com/item?id=44594584)
  >
  > "It feels like the latest update was compromised, and I don't feel safe
  > executing Claude code in this terminal." — BartInTheField, on cmux silently
  > injecting `--allow-dangerously-skip-permissions`,
  > [cmux #3547](https://github.com/manaflow-ai/cmux/issues/3547)
- **Not just a multiplexer *(vs herdr / Claude Squad)*.** This is *the* point
  against the Rust-TUI twins. Claude Squad / agent-deck switch sessions; herdr is
  the best-in-class multiplexer + agent-status sidebar — but all three are still
  *multiplexers*. spyc is a full vi **file manager** (git-status gutter, marks,
  harpoon, picks, frecency) with an **in-process review loop** (diff/show/blame,
  `gd`/`gD`/`gu`) and an **MCP bridge** that grounds the agent in your view —
  three things herdr's README confirms it has none of. The multiplexer is a
  feature of spyc; for herdr it is the whole product.

### Objections to pre-empt at launch

- **"tmux already does this."**
  > "You can already do that, in the terminal. Open your favourite terminal, use
  > splits or tmux and spin up as many claude code or codex instances as you
  > want." — submeta, [HN](https://news.ycombinator.com/item?id=46860355)

  *Answer:* tmux gives you panes; it doesn't give you worktree lifecycle, a
  git-status gutter, file navigation, or agent-status awareness in one model. The
  integration is the product.

- **"Why do you need 10 agents?" (scale skepticism).**
  > "Just why do you need 10 parallel agents … maybe a couple of things in
  > parallel could be useful, but more often the need is not for 'one more jira
  > ticket.'" — xmonkee / kaffekaka,
  > [HN](https://news.ycombinator.com/item?id=46368739)

  *Answer:* don't chase the "50 agents" vanity number. Nail the **2–4 agent**
  case with low friction and great review.

- **Tool fatigue.**
  > "It is really hard to justify tools like these, where you need CC + this tool
  > + some other tools." — maxdo,
  > [HN](https://news.ycombinator.com/item?id=46368739)

  *Answer:* spyc isn't another layer — it's the file/process manager you'd run
  anyway, that also manages agents. "One tool you already live in."

- **Scope discipline.**
  > "My two cents — don't do it. There's plenty of terminal editors … You will
  > end up reinventing an IDE." — danw1979 (re: adding an editor pane),
  > [HN](https://news.ycombinator.com/item?id=47109046)

  *Heed it:* the community punishes scope creep. cmux's embedded browser is one
  of its biggest stability liabilities. Stay a focused primitive.

---

## 5. Go-to-market

**The biggest GTM finding, three times over:** the HN breakouts that mattered
for lazygit, Atuin, and Helix were **organic re-posts by other people**, not the
authors' own Show HN.

> "I had taken a few stabs at publicising it in the weeks prior which fell on
> deaf ears. When I eventually posted to Hacker News I was so sure nothing would
> come of it that I had already forgotten about it by that afternoon." — Jesse
> Duffield, [lazygit, 5 Years On](https://jesseduffield.com/Lazygit-5-Years-On/)

> "The inflection points … were mostly Hacker News and Reddit. I tried posting on
> Hacker News myself a few times, and it just never really picked up. It's always
> been when other people post it." — Ellie Huxtable (Atuin),
> [Changelog #579](https://changelog.com/podcast/579)

> "I posted this on r/rust to get some feedback, definitely wasn't prepared to
> expose it to HN yet." — archseer (Helix), on the 657-point front-page thread,
> [HN](https://news.ycombinator.com/item?id=27358479)

**Implication:** seed early to a friendly niche (r/rust, r/commandline), make it
demoable enough that fans carry it to HN. Don't bank on your own submission.

**herdr is a fresh cautionary datapoint:** despite ~7.4k stars, herdr's own Show
HN scored **5 points, 0 comments** ([48247248](https://news.ycombinator.com/item?id=48247248)),
and it has effectively *no* organic Reddit/lobste.rs/HN-front-page discussion —
its momentum is GitHub-star + SEO-blog amplification, not community debate. Stars
are not the same as a discussion breakout; the organic-repost pattern above is
still the lever that matters.

### The repeatable levers a solo/small team controls

- **The README is the storefront.** "I don't have a standalone landing page or
  docs site. I keep everything in the repo, which means you're always one click
  away from starring." — Duffield. *(But stars ≠ usage — track Homebrew /
  crates.io install counts as the real adoption signal.)*
- **Crisp README + one-line install.** "A lot of the marketing is down to just
  having a readme that's very clear as to what it is … I tried to make the
  install very, very straightforward … the friction for people giving it a try
  is very low." — Huxtable.
- **HN tone.** "Don't write in a marketing, sales, or PR style. It doesn't work
  on HN. Talk to readers as peers … Be humble. Don't say nice things about
  yourselves; let the work speak." —
  [HN Launch guidelines](https://news.ycombinator.com/yli.html). Put a
  copy-paste install in the post and answer every comment personally on launch
  day.
- **Discoverability drives adoption.** Zellij's creator argues on-screen
  keybinding hints let new users succeed without learning lore upfront. spyc is
  vi-keyboard-driven with a help overlay — an always-visible hint bar is both an
  on-ramp and a thing worth showing in the demo.
  ([Why Zellij?](https://poor.dev/blog/why-zellij/))

### Channel plan

| Channel | When | Notes |
|---------|------|-------|
| **r/rust · r/commandline** | First | Warm-up venue, not launch. Post for feedback (the Helix/Atuin pattern); organic HN momentum starts here. |
| **awesome-ratatui PR** | First | One-line PR, `- [name](link) - desc.` format, one per suggestion → Development Tools. Near-zero effort, exact audience. ([contributing](https://github.com/ratatui/awesome-ratatui/blob/main/contributing.md)) |
| **awesome-tuis PR** | First | ~19.5k★ general TUI directory; bar is "actively maintained, not a wrapper." File Managers / Development. ([repo](https://github.com/rothgar/awesome-tuis)) |
| **Terminal Trove** | Soon | Self-serve form; **requires an image/GIF/MP4 preview** (we now have the demo GIF). Need a ~100-char tagline + 250–300-char description. ([post](https://terminaltrove.com/post/)) |
| **This Week in Rust** | Soon | PR to `drafts/`, one project/week. Bare links discouraged — frame as Rust-specific learnings. Also angle for Crate of the Week. ([README](https://github.com/rust-lang/this-week-in-rust)) |
| **Show HN** | When ready | Ignition, but don't rely on your own post. Demoable + seeded first; peer tone; frictionless try-it; answer everything. Low scores aren't fatal (Atuin: 48 pts → 30k★). ([yli.html](https://news.ycombinator.com/yli.html)) |
| **A conference talk** | Optional | A genuine talk (FOSDEM-style) gave Atuin a shareable, non-salesy bump. |

Channels with no signal worth pursuing for this category: Product Hunt, dev.to,
lobste.rs (niche, low reach).

---

## 6. Immediate next moves, ranked

Reordered after the herdr deep dive (§1a): the **review loop is now the single
uncontested wedge**, and **agent-status is competitively urgent but must be done
the reliable (MCP/hook) way**, not herdr's fragile scrape.

1. **Make per-worktree diff review a headline workflow** *(now THE wedge; mostly
   built).* No other TUI-lane tool has an in-process review loop — herdr concedes
   it outright. spyc has in-house diff/show/blame (`gd`/`gD`/`gu`); surface it as
   "review & merge across your agents," and lead the README with it.
2. **Reliable agent-status — the differentiated way** *(founding pain; herdr
   shipped it fragile).* Per-pane running/blocked/idle/done, but built on the
   **MCP/hook self-report channel** (cooperative agents report via the config
   spyc already writes) with a tunable scrape *fallback* — dodging herdr's 46+
   detection bugs. Stable pane indices for vi muscle memory. Plan:
   **`docs/archive/AGENT_AWARENESS_PLAN.md`**.
3. **Ship the worktree-bootstrap story** *(highest unmet demand; design spike
   first).* Make "new worktree carries your untracked files / runs your setup
   hook" first-class. Nobody owns it; spyc's graveyard machinery is the start.
4. **Resilient session autosave (NOT a daemon)** *(herdr beats us architecturally
   — pick the cheaper win).* Make periodic saves `SIGKILL`-recovery-sufficient
   and capture richer layout + per-pane cwd for `-r`. Explicitly **do not** build
   a detach/reattach daemon — it fights spyc's single-process MVU/sync core.
5. **Land the discovery PRs + Terminal Trove** *(low effort; demo GIF ready).*
   awesome-ratatui, awesome-tuis, Terminal Trove. Permanent, on-audience, before
   any HN attempt.
6. **Tighten the README around the sharpened wedge** *(low effort).* Lead with
   file-manager + in-process review (the uncontested ground), not
   "cross-platform Rust TUI" (herdr owns that too). Pre-empt the tmux objection.
   (v1 reframe + demo GIF landed in PR #507.)

---

## Sources

Primary sources, all fetched and verified:

- **HN threads:** cmux Show HN ([47079718](https://news.ycombinator.com/item?id=47079718)),
  cmux multiplexer ([45596024](https://news.ycombinator.com/item?id=45596024)),
  Superset ([46368739](https://news.ycombinator.com/item?id=46368739)),
  Conductor ([44594584](https://news.ycombinator.com/item?id=44594584)),
  Claude Squad ([44630194](https://news.ycombinator.com/item?id=44630194)),
  Helix ([27358479](https://news.ycombinator.com/item?id=27358479)), and many
  child comments.
- **GitHub issues:** manaflow-ai/cmux (#330, #1012, #2322, #2823, #3547, #3751,
  #4529, #6584, #6593, #6598, #6599) · supabitapp/supacode (#436, #441, #443,
  #444, #445, #448) · smtg-ai/claude-squad and asheshgoplani/agent-deck repos.
- **herdr (§1a):** [repo](https://github.com/ogulcancelik/herdr) + issues (open +
  closed, surveyed via `gh` 2026-06-26: detection cluster #671/#634/#673/#803/#773/#656/#655,
  terminal-emulation #816/#696/#722/#283, daemon/macOS #808/#774, features
  #222/#261/#303) · source deep-dive of v0.7.1 (detection manifests, the
  line-delimited-JSON socket API with `events.subscribe`/`pane.wait_for_output`,
  the two-tier hook integration, SCM_RIGHTS live handoff) — **AGPL-3.0; ideas
  only, no code reused.** Verified user voices: [coles.codes](https://coles.codes/posts/herding-agents-with-herdr/) ·
  [maxtokens.ai](https://maxtokens.ai/posts/herdr-agent-multiplexer/) · HN
  [48220211](https://news.ycombinator.com/item?id=48220211),
  [48424303](https://news.ycombinator.com/item?id=48424303),
  [48247248](https://news.ycombinator.com/item?id=48247248) (its flat Show HN).
- **GTM:** [lazygit "5 Years On"](https://jesseduffield.com/Lazygit-5-Years-On/) ·
  [Atuin / Changelog #579](https://changelog.com/podcast/579) ·
  [Why Zellij?](https://poor.dev/blog/why-zellij/) ·
  [HN Launch guidelines](https://news.ycombinator.com/yli.html) ·
  This Week in Rust + Terminal Trove + awesome-ratatui/awesome-tuis submission
  docs.
- **Review:** [rywalker.com/research/supacode](https://rywalker.com/research/supacode).
- **lf (§1e, added 2026-08-12):** [repo](https://github.com/gokcehan/lf) · README
  *Features* / *Non-Features*, `doc.md` (2,309 lines) and `CHANGELOG.md` read at
  master · counts, releases, contributors and per-author history via the GitHub API
  (`repos/gokcehan/lf`, `/releases`, `/contributors`, `/commits?author=gokcehan`;
  commit total from the `Link: rel="last"` page count) · issues
  [#170](https://github.com/gokcehan/lf/issues/170) (archive browsing, open since
  2019-05-22), #1903, #2076, #2563, #2581, #1620, #777 · sixel-on-by-default via
  CHANGELOG #2109 — **MIT; ideas only, no code reused.** Verified user voices, each
  re-fetched from HN's Firebase item API for author + timestamp rather than a
  search snippet: [49268693](https://news.ycombinator.com/item?id=49268693) (RMPR,
  2026-08-12 — the lead for this section, via the "shell exclamation mark" thread),
  [40298792](https://news.ycombinator.com/item?id=40298792) (thworp),
  [25127212](https://news.ycombinator.com/item?id=25127212) (29athrowaway),
  [41797442](https://news.ycombinator.com/item?id=41797442) (nvllsvm),
  [40323990](https://news.ycombinator.com/item?id=40323990) (monopoliessuck),
  [43419584](https://news.ycombinator.com/item?id=43419584) (ac29),
  [45272749](https://news.ycombinator.com/item?id=45272749) (yuvadam),
  [32436331](https://news.ycombinator.com/item?id=32436331) (rahen). Wider-lane
  star counts (superfile / nnn / ranger / broot / joshuto / walk) from the same API
  pass.
- **Yazi (§1d):** [repo](https://github.com/sxyazi/yazi) + [docs site](https://yazi-rs.github.io)
  (the canonical feature list; the README bullets are a subset) · PR #4005
  (OSC 72 drag-and-drop, merged 2026-05-28). Snapshot last refreshed 2026-07-01,
  spyc-side; Yazi still v26.5.6. Full standalone history archived at
  `docs/archive/YAZI_COMPETITIVE_REVIEW.md`.

**Verification caveats.** Six mined quotes were discarded because the text was
real but credited to the wrong author (e.g. a comment by the cmux dev attributed
to a different commenter) — they are *not* used here. One cmux issue (#4005, the
argv-denylist breakage) had its body verified but its comments unreachable, so
only the body claim is relied on. X/Twitter quotes were login-gated and verified
via indexed snippets rather than direct fetch. All counts/versions are mid-June
2026 and will drift.
