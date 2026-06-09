# onboarding-product-charter — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: onboarding-product-charter
Created: 2026-05-07T07:39:33.335101+00:00

---
Entry: Claude Code (caleb) 2026-05-07T07:39:33.335101+00:00
Role: pm
Type: Note
Title: Onboarding: product charter from positioning surfaces

Spec: pm

Purpose: spyc's product framing in the maintainer's own words, drawn from positioning surfaces. spyc is a Rust TUI file commander whose differentiating bet is being a queryable context source for an AI coding agent (Claude Code) that lives in the same split-pane session.

Observed:
- Product framing (one-liner): "A vi-keyboard-driven file commander that runs Claude Code in a split pane and exposes itself to Claude as an MCP server" (`README.md:8-11`). The roadmap restates the same thesis explicitly: spyc "isn't just 'a file manager with Claude in a pane.' It's a file manager that Claude can query — current directory, cursor, picks, inventory, filter, git branch — via a standard protocol. That bidirectional awareness is the positioning that differentiates spyc from `tmux` + `claude`" (`ROADMAP.md:12-15`).
- Target user: "a developer who already thinks in vi motions and wants Claude Code living in the same workspace -- not one window over, not in a browser tab, in the same session, sharing context" (`ROADMAP.md:5-9`). README echoes the framing: "the AI agent being a peer in the workflow, not a separate tab" (`README.md:25-27`).
- Committed bet (the technical wager that earns the positioning): an MCP server on a PID-scoped Unix domain socket so Claude Code discovers spyc via standard `.mcp.json` config and codex via `.codex/config.toml`, both registrations re-execing `spyc --mcp` as a stdio proxy that forwards through to the same socket (`README.md:81-98`, `AGENTS.md:9`, `ARCHITECTURE.md:135-155`). Multiple instances coexist; takeover is interactively prompted (`AGENTS.md:9`, `src/main.rs:159-196`).
- Audience scope (intentionally narrow): macOS and Linux, x86_64 and aarch64; "Windows via WSL" is the supported story; "Native Windows support" is an explicit non-goal (`README.md:311`, `ROADMAP.md:430-433`).
- Non-goals (committed boundary): native Windows, plugin system, localization, telemetry, full SLSA L3, mouse support beyond what already exists (`ROADMAP.md:426-447`).
- Distribution posture today: internal Tripstack tool distributed by SSH-cloning a Bitbucket repo and running `make install`; "no prebuilt binary distribution" (`SECURITY.md:60-66`). Release / public-distribution work is gated through `ROADMAP.md` Distribution + `LAUNCH_PREP.md`. The "v2.0 version bump is a signaling choice as much as a semver one" tied to "MCP positioning shift + public distribution" (`ROADMAP.md:472-476`, target "mid-to-late May 2026").
- Internal contact: `derek.marshall@tripstack.com` (`SECURITY.md:7`, `SECURITY.md:115`).
- Maintainer-authored thesis source: `ROADMAP.md` "Thesis" section (`ROADMAP.md:3-23`) and "Working tracks" framing (`ROADMAP.md:25-39`) — Derek-Marshall-authored, recently active. This meets the bar for citing as committed product framing (not just a marketing tagline).

Inferred:
- spyc is a single-developer, internal-tool-becoming-public-tool — not a community project (yet). — confidence: high — basis: `ROADMAP.md:472-476` plans a v2.0 public bump, `LAUNCH_PREP.md:21-25` lists "GitHub org account" as still-open, `SECURITY.md:60-66` confirms no public binaries.
- The MCP-bridge bet is load-bearing: the tool is "supporting infrastructure" for the bridge, not the other way around. — confidence: high — basis: `ROADMAP.md:17-22` says explicitly "Every other feature ... is supporting infrastructure that makes the split-pane workflow fast and comfortable. The roadmap is organized accordingly: the pane-and-agent integration is the defining work track, not the trailing milestone." How to apply: scope decisions should preserve / strengthen the bridge experience first; pure file-manager polish second.

Next query: `watercooler_search(query="product charter thesis non-goals", thread_topic="onboarding-product-charter", code_path=".")`

Related:
- `onboarding-overview` — front door + reading order.
- `onboarding-risk-register` — flags the README/Cargo.toml version skew and the upstream-vs-mirror remote ambiguity that will affect public-launch positioning.
- `onboarding-release-process` — scopes the "ship to the public" arc that the v2.0 bump is signaling.

Provenance:
- Source surfaces: `README.md:1-330`, `ROADMAP.md:3-23` (Thesis), `ROADMAP.md:25-39` (Working tracks), `ROADMAP.md:426-447` (Non-goals), `ROADMAP.md:472-476` (v2.0 framing), `AGENTS.md:1-9`, `ARCHITECTURE.md:135-155`, `SECURITY.md:7,60-66,115`, `LAUNCH_PREP.md:1-40`.
- Source-confidence note: maintainer-authored thesis surface (`ROADMAP.md`) is sufficient to write this charter as a Note with high confidence, but it does **not** rise to a Watercooler `Decision` because none of the surfaces explicitly bind the project to the framing as policy. If future maintainers want the product framing to carry decision-weight, a maintainer-authored Decision entry on this thread is the right next step.
- Sibling entry_id (pending back-fill): `onboarding-overview = 01KR0NZNJ3KM6BJY09Q4P9D0NE`.

<!-- Entry-ID: 01KR0P18MCE1H57Q5ZTAGKAJNH -->

---
Entry: Claude Code (caleb) 2026-06-09T05:18:23.899672+00:00
Role: pm
Type: Note
Title: Onboarding refresh: product charter matured at #311 — multi-agent registry + the MVU/architecture turn

Spec: pm

Purpose: Refresh of onboarding-product-charter from #37 (v1.41.24) to #311 (v1.56.0). The #37-era entry (01KR0P18MCE1H57Q5ZTAGKAJNH) stands; this entry records that the thesis is stable but the product framing has matured along two axes — the multi-agent expansion and the MVU/architectural-maturity turn — and re-confirms the non-goals.

Thesis (stable): spyc is a vi-keyboard-driven file commander that exposes itself to an AI coding agent as a queryable context source — "a file manager that Claude can query — current directory, cursor, picks, inventory, filter, git branch — via a standard protocol. That bidirectional awareness is the positioning that differentiates spyc from `tmux` + `claude`" (`ROADMAP.md:11-15`). README sharpens the one-liner: "puts a local MCP socket next to the file view, so the agent can ask spyc what is the cursor on, what is staged, what is pinned, what is in this directory" and "the file commander is the noun the agent operates on, not the chrome around it" (`README.md:24-33`). Every other feature is "supporting infrastructure that makes the split-pane workflow fast and comfortable" (`ROADMAP.md:17-22`).

Observed (what matured since #37):
- Multi-agent expansion. The #37 charter framed the bridge as Claude-Code (+ codex via `.codex/config.toml`). At #311 the agent surface is a registry: each agent (claude/codex/gemini/agy/zot, else Other) is an `AgentProfile` impl (`src/agent/mod.rs:1-4` registry note; `ClaudeProfile`/`CodexProfile`/`GeminiProfile`/`AgyProfile`/`ZotProfile` at `src/agent/mod.rs:150-433`; statics `AGY`/`ZOT` at `:451-452`; detection asserts at `:485-494`). README now says agents are "by default Claude Code or codex (both first-class; Gemini and Antigravity also supported)" (`README.md:38-40`). Deep detail: `history-arc-07-codex-and-mcp-bridge` (shared data model + parallel session-resume parsers; CLAUDE.md→AGENTS rename).
- MVU / architectural-maturity turn. The project shifted from feature-accretion to plan-driven architecture: a Model-View-Update runtime (`history-seg-refactor-mvu`), the 800-LoC module decomposition (`history-seg-module-decomposition`), and gix in-process git (`history-seg-gix-migration`). The forward thesis is now itself a plan document: `docs/V1_70_PLAN.md` proposes making spyc "programmatically addressable" under "one protocol, three clients" — the same daemon protocol consumed by an in-process Rust SDK, a `spyc` CLI subcommand, and the MCP server (`docs/V1_70_PLAN.md:1-60`). This is the natural extension of the query-bridge bet from read-only context to typed drive-and-wait orders/bells.
- Distribution posture: README header now reads "macOS and Linux · actively developed" (`README.md:12-14`) — the public-launch framing has firmed up versus the #37 "internal Tripstack tool" framing, though Bitbucket-clone + `make install` remains the install path.

Inferred:
- The product is mid-transition from internal tool to public, agent-agnostic file-commander-as-context-source. — confidence: high — basis: AgentProfile registry generalizes past Claude (`src/agent/mod.rs:150-433`); V1_70 "one protocol, three clients" generalizes past MCP (`docs/V1_70_PLAN.md:38-60`); README "actively developed" public framing (`README.md:12-14`).
- The MCP-bridge bet is now load-bearing AND being deepened, not just preserved — the roadmap pulls the integration toward a typed daemon surface rather than treating it as done. — confidence: high — basis: `docs/V1_70_PLAN.md` thesis + `insight-trajectory` window-2 mode-shift (feature-accretion → plan-driven); `insight-emergent-properties` Property 7 (plan-doc-as-executable-spec).

Non-goals (still hold — re-confirmed against current ROADMAP): Native Windows support (WSL is the supported story), Plugin system, Telemetry ("not even anonymized opt-in"), plus localization / full SLSA L3 / mouse-beyond-existing (`ROADMAP.md:520-533`). These are stable across #37→#311; the negative-honor invariance (the project keeps its stated "we will NOT" boundaries) is itself a documented pattern — `insight-trajectory` (negative-honor invariance).

Next query: `watercooler_search(query="V1_70 one protocol three clients daemon SDK CLI", thread_topic="history-seg-docs-planning", code_path=".")`

Related:
- `onboarding-overview` — front door + reading order (refreshed this run).
- `onboarding-team-map` — the "single-developer" framing shares the same maintainer-authored sources.
- the history/insight corpus — `history-arc-07-codex-and-mcp-bridge` (multi-agent), `history-seg-refactor-mvu` + `history-seg-module-decomposition` + `history-seg-gix-migration` (the architecture turn), `insight-trajectory` (mode-shift + negative-honor invariance), `insight-emergent-properties` (Property 7).

Provenance:
- Files read: `README.md:12-14,21-40`, `ROADMAP.md:1-45` (thesis/tracks), `ROADMAP.md:520-533` (non-goals), `docs/V1_70_PLAN.md:1-60`, `src/agent/mod.rs:1-4,150-494` (AgentProfile registry + agent kinds).
- Prior seed entry read in full: onboarding-product-charter 01KR0P18MCE1H57Q5ZTAGKAJNH.
- History/insight threads consulted: history-arc-07-codex-and-mcp-bridge, history-seg-refactor-mvu, history-seg-module-decomposition, history-seg-gix-migration, history-seg-docs-planning, insight-trajectory, insight-emergent-properties.

<!-- Entry-ID: 01KTND2E95BZB6AE4158JK0QNG -->
