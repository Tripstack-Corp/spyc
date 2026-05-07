# onboarding-entry-path — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: onboarding-entry-path
Created: 2026-05-07T07:50:53.717713+00:00

---
Entry: Claude Code (caleb) 2026-05-07T07:50:53.717713+00:00
Role: pm
Type: Plan
Title: Onboarding: entry path for future contributors

Spec: pm

Purpose: Recommend a first task to a contributor in each role, sized so they hit the doc-sync rule, the test surface, and the dispatch foot-gun on day one rather than month three. The picks below are taken from the maintainer's own open lists, not invented.

Observed:
- **Open work surfaces** (read these before claiming a task to avoid duplication):
  - `TODO.md` — operational checklist, sized `[S]`/`[M]`/`[L]`. Currently has open items in Foundations (snapshot tests, pty integration test, property tests, background directory loading) and Distribution (release automation, signing, Homebrew tap, etc.).
  - `BUGS.md` — maintainer's own bug log, split into SMALL / BIGGER / MAYBE / FIXED. The SMALL items are mostly self-contained.
  - `ROADMAP.md` "Remaining" sections under Foundations / Thesis / Distribution.
  - `LAUNCH_PREP.md` — pre-2.0 launch hygiene; some items (governance docs, repo hygiene, SECURITY/COC) are explicitly tagged `[ ]` open.

For an **implementer** (small, well-bounded, hits the action-dispatch loop and the doc-sync rule):
- Pick: `BUGS.md` SMALL — "pane widget always paints a reverse-video cursor block at `screen.cursor_position()` even when the child has set `DEC ?25l` (cursor hidden). `vt100::Screen` already exposes `hide_cursor()`; `src/pane/widget.rs:43-54` just doesn't read it" (`BUGS.md:9-15`). Why this one: explicitly diagnosed by the maintainer, single-file fix, includes the symptom (lazygit in pane), single guard added in `widget.rs`.
- Walk through: read `AGENTS.md:42-44` (pane subsystem index), open `src/pane/widget.rs` near line 43, add the guard, write a test (snapshot or unit), bump `Cargo.toml` patch version, add a `CHANGELOG.md` Unreleased entry, move the BUGS.md item into the FIXED section in the same commit. This is the maintainer's exact doc-sync rule applied end-to-end.

For a **planner** (architectural-direction work; light implementation, lots of reading):
- Pick: a small slice of the Elm-architecture refactor — specifically the View extraction (`ARCHITECTURE.md:50-55`, `REFACTOR_PLAN.md:1-15`, `ROADMAP.md:88-113`). Move one widget's render path from inline `app/mod.rs` rendering into a pure function in `src/ui/` that takes `&AppState`. Status bar is the natural first slice (it already has snapshot tests, `ROADMAP.md:53-58`).
- Why this one: marked explicitly as "Done incrementally alongside feature work — not a standalone rewrite" (`ARCHITECTURE.md:62`) and "The View extraction is the natural first slice (no behavior change, mechanical move)" (`ROADMAP.md:111-113`). The work is intentionally scoped for someone learning the codebase.

For a **tester** (validation surface, no behavior change):
- Pick: extend snapshot tests on the `pager` widget per `TODO.md:73-76` and `ROADMAP.md:114-118`. The infra is wired (`Cargo.toml:53` `insta` + `TestBackend`); only status bar (4) is covered today. Pager has well-defined sub-modes (ANSI, hex dump, line numbers, search highlight) — each becomes a snapshot.
- Alternative pick: the single PTY integration test (`TODO.md:78-81`, `ROADMAP.md:119-120`). "Spawn `cat` via `portable-pty`, write bytes, parse `vt100::Screen`, assert rendered output. `#[cfg(unix)]`. One test, not a suite." Higher information value — it would catch the entire pane I/O path that today is only covered by manual smoke testing.

For a **critic** (review-and-analysis surface):
- Pick: read this seed set, then write a follow-up Watercooler Note on whichever of the seed entries needs strengthening. The drift findings in `onboarding-risk-register` and `onboarding-docs-contracts` (README version banner, INSTALL.md MSRV, CONTRIBUTING.md project structure) are concrete starting points — each is one-file-one-edit, lands in the doc-sync surface rather than the code surface, and gives the critic an excuse to read the whole onboarding flow as a target user would.
- Alternative pick: an MCP socket security review focused on the v1.41.24 hardening (`CHANGELOG.md:9-31`, `src/mcp.rs`). The maintainer flagged `SO_PEERCRED`/`LOCAL_PEERPID` as defense-in-depth in `ROADMAP.md:312-318`; a critic could write the threat-model entry that decides whether that's load-bearing.

For a **PM / scribe** (sequencing and durable record):
- Pick: take the open items in `LAUNCH_PREP.md` "Open decisions" section (`LAUNCH_PREP.md:21-33`) and turn each into a Watercooler thread with a clear handoff target. Today they live as `[ ]` checkboxes; surfacing them as threads with explicit ownership and "ball" state is exactly the coordination work this role is for. The GitHub-org-account decision (`LAUNCH_PREP.md:21-25`) is the load-bearing one — it gates `Cargo.toml` `repository`, `.github/` workflows, and the Homebrew tap namespace.

Doc-sync gate (applies to every role's first task):
The maintainer's keep-in-sync rule (`AGENTS.md:65-77`, `CONTRIBUTING.md:104-115`) is treated as part of the merge gate. Concrete checklist before opening a PR:
1. Code change is staged.
2. `Cargo.toml` version bumped if user-visible (`AGENTS.md:77`).
3. `CHANGELOG.md` entry added under `Unreleased`.
4. If keybinding / command / behavior changed: `FEATURES.md`, `AGENTS.md`, `src/ui/help.rs` updated.
5. If architectural decision changed: `ARCHITECTURE.md` updated.
6. If UI design language changed: `DESIGN.md` updated.
7. If install / build steps changed: `INSTALL.md` (and `Makefile`/`Justfile` if relevant) updated.
8. If a bug was fixed: moved to `BUGS.md` FIXED section.
9. If the new code is a `:` command: punt list updated in `AppState::dispatch_command` per `AGENTS.md:61` (search for `"bprev"` to find it).
10. `make check` passes locally.

Inferred:
- The lowest-friction first task across roles is one that exercises the doc-sync rule. — confidence: high — basis: every role's pick above lands in 3–5 surfaces by design; the maintainer's own framing in `AGENTS.md:65-77` is that doc updates *are* part of the work, not follow-up.
- The "single-developer project" reality (see `onboarding-team-map`) means the right first move for a non-Derek contributor is *propose, then implement* — open a Watercooler thread with the proposed change before coding more than ~50 lines, so the work doesn't collide with the maintainer's in-flight refactor seams. — confidence: medium — basis: 321/321 commits in the last 6 months by a single author; the in-flight Elm refactor (`onboarding-architecture`) is described as touching many handlers incrementally.

Drift findings: not required for this topic.

Next query: `watercooler_search(query="entry path first task contributor", thread_topic="onboarding-entry-path", code_path=".")`

Related:
- `onboarding-overview` — front door.
- `onboarding-architecture` — context for the planner pick.
- `onboarding-test-surface` — the validation gate every pick must pass.
- `onboarding-docs-contracts` — the doc-sync surfaces every pick must touch.
- `onboarding-team-map` — the "single-developer project" reality that shapes the "propose, then implement" guidance.

Provenance:
- Files read: `TODO.md:1-225` (full), `BUGS.md:1-120` (SMALL section), `ROADMAP.md:88-123,367-401`, `LAUNCH_PREP.md:1-40`, `AGENTS.md:60-77,38-56`, `CONTRIBUTING.md:104-115`, `ARCHITECTURE.md:42-62`, `REFACTOR_PLAN.md:1-15`, `Cargo.toml:53`, `CHANGELOG.md:1-100`, `src/pane/widget.rs` (referenced for the implementer pick).
- Sibling entry_ids: `onboarding-overview = 01KR0NZNJ3KM6BJY09Q4P9D0NE`, `onboarding-architecture = 01KR0P4W3ED1QZ8F44PFB2WPDZ`, `onboarding-test-surface = 01KR0PBH6T1AK4VA0JSE75390F`, `onboarding-docs-contracts = 01KR0PDR0APW4N1QC03C8JWSQX`, `onboarding-team-map = 01KR0P2PC2GBFJ9Z0GPGXZ48Y4`.

<!-- Entry-ID: 01KR0PP11NEC9JF0PT9F7FZPG2 -->
