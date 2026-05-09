# human-exploration-of-nonhuman-threads — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: human-exploration-of-nonhuman-threads
Created: 2026-05-09T00:38:24.943311+00:00

---
Entry: Claude Code (caleb) 2026-05-09T00:38:24.943311+00:00
Role: scribe
Type: Note
Title: First entry — what spyc is, drawn from the catalogue rather than the README

Spec: scribe

tags: #narrative #human-readable

# Frame for the thread

The previous fourteen threads on this orphan branch were written by an agent for an agent. The spine, the eight arc threads, the four insight threads — clinical, hedged, addressed to a future scribe. This thread inverts that: a human reader (Derek, the maintainer) asked for a plain-English read of the catalogue. The thread collects those reads as they are requested.

The first ask is what spyc *is*, framed for engineers who have not seen it. What follows is drawn from sitting with the catalogue, not from re-reading the source. The arc threads describe what spyc *did* over 22 days. The insight threads describe what spyc *is* given those choices. The product charter states the thesis. Compressed, the three layers produce the elevator pitch and one-pager below.

# Elevator pitch

spyc is a Rust TUI file commander where the AI agent in the side pane can query the file commander itself.

Most "AI in your terminal" tools give the agent a chat window and let you copy-paste paths back and forth. spyc puts a local MCP socket next to the file view, so the agent can ask spyc *what is the cursor on, what is staged, what is pinned, what is in this directory* — and answer questions about your working tree without you describing it. The file commander is the noun the agent operates on, not the chrome around it.

# One-pager

**What it is.** A two-pane terminal program. The top pane is a keyboard-driven, vim-flavoured file commander with git-aware listings. The bottom pane is a child process — by default Claude Code or codex (both first-class), but in practice any program. The two panes share focus through a chord prefix system, and the file commander exposes a local Unix-domain MCP socket the bottom-pane agent can connect to.

**Git surface.** Two-character per-file markers for staged-vs-unstaged state. `=g` filters to the git-changed set. `]g` / `[g` jump the cursor between changed entries. A 1 Hz poll backs FSEvents in case the OS misses a change. Markers do not leak between same-named files in different directories.

**Pager surface.** One programmable pager handles help, file preview, command output, and ad-hoc text. Visual-line mode for range yank. Substring search (not anchored prefix). `n`/`N` follows matches into the second column of multi-column views. An alt-screen scroll hint shows when scrollback has left the buffer. `D` opens the file under cursor in `$PAGER` as a top-pane overlay.

**MCP bridge.** A single project-scoped Unix socket per spyc instance, discovered by both Claude Code and codex through their respective config conventions (`.claude/`, `.codex/config.toml`). Project-scoped specifically — two spyc instances in two repos do not see each other's sockets. A small `.spyc-context-<pid>.json` marker file is the handshake artefact.

**Recoverability surface.** A local "graveyard" stores deleted files as per-entry zstd-compressed tarballs, with `R` as the undo verb. A system-trash cascade hands larger or external-volume cases to the OS. Directory listings cap at 50,000 entries to avoid hangs on pathological trees. The vt100 parser runs under panic recovery — a corrupt escape sequence from a child process cannot bring spyc down.

**Shape of the work.** Single-developer codebase. 36 PRs over a 22-day merge window, semver visible at every merge. The first 48 hours produced four minor cuts (zoom, harpoon, quickselect, graveyard); the next ~18 days were 24 consecutive patches. No plugin system, no telemetry, no localisation, no Windows-native build, no mouse, no SLSA L3 ambition — the charter says so, and the catalogue confirms each non-goal held across the window.

**What the catalogue noticed about the artefact.** Two findings show up in the code itself, not just in the description.

1. The codebase widens cheaply at the *substrate* — one socket serves both AI peers, one `git_files` cache feeds four features — and widens by *replication* at *registration* — one peer config file per AI client. A third MCP-aware peer would land as a third parallel registration with the substrate unchanged.
2. Across 22 days, exactly one PR shipped a documented command that did not work (`:undo`, PR #13) — and it was fixed 25 minutes later (PR #14). Sixteen description-layer drifts, one functional drift. The artefact reliably does what it says it does, even when the talk around it bundles or understates.

**Where to look first.** `ROADMAP.md:3-23` for the thesis. `ARCHITECTURE.md:135-155` for the MCP bridge surface. The eight `history-arc-NN-*` threads for what landed when. The four `insight-*` threads for what the artefact is, given that.

Provenance:
- `onboarding-product-charter` entry 0 = 01KR0P18MCE1H57Q5ZTAGKAJNH (thesis at `ROADMAP.md:3-23`; six non-goals at `ROADMAP.md:426-447`).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (current-state pane/pager/MCP surfaces).
- `history-overview` entry 1 = 01KR0TWHTC1MPK4KJ08Y9SPE6P (36 PRs across 22 days; arc segmentation).
- `insight-trajectory` Doc #2 (UX-skip 4-of-4) and Doc #5 (six non-goals 6-of-6).
- `insight-emergent-properties` Property 1 (description-vs-functional asymmetry, 16:1) and Property 4 (substrate-additive / registration-parallel).
- `insight-recurrence` Pattern 5 (front-loaded minors, sustained patch corridor) and Pattern 6 (`git_files` chain across PRs #1, #7, #15, #24, #27).
- `insight-drift` Pattern D (single functional drift = PR #13 → PR #14, 25 min).
- PR-level facts cited: #1 (1 Hz git poll), #7 (`=g` filter), #13 (graveyard + R-undo + trash cascade), #14 (`:undo` routing fix), #15 (basename leak fix), #17 (multi-col `n`/`N`), #20 (alt-screen scroll hint), #24 (`]g`/`[g`), #27 (two-char git markers), #28 (50k directory cap), #30 (vt100 panic recovery), #33 (pager visual-line mode), #35 (`D` opens cursor file in `$PAGER` top overlay), #36 (substring search), #37 (project-scoped MCP socket).
- Identity fallback: no `set_agent` tool surfaced this session; identity asserted via Role + Spec lines and `agent_func`.

<!-- Entry-ID: 01KR52QJD22HB8NEB01KYPY686 -->
