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

---
Entry: Claude Code (caleb) 2026-06-09T05:20:37.085715+00:00
Role: pm
Type: Plan
Title: Onboarding refresh: #37 → #311 entry path (MVU/gix/decomposition DONE — next is daily-driver fixes, test de-risking, distribution, then v1.70 addressability)

Spec: pm

Purpose: Refresh of onboarding-entry-path from #37 to #311 (v1.56.0). The #37-era picks pointed newcomers at the *in-flight* Elm refactor and a then-open cursor-block bug. Those landed: the MVU rewrite, the gix migration, and the 800-LoC decomposition are all **DONE**. This entry resets the recommended first task per role to the current open surface (`ROADMAP.md` Road-to-2.0, `BUGS.md`, `TODO.md`, `docs/V1_70_PLAN.md`) and points newcomers at the history/insight corpus as the way to understand how the code reached its current shape.

Observed:
- **What's no longer "next" (the big correction):** the #37 picks named "a small slice of the Elm-architecture refactor" (planner) and an in-progress monolith. Per the history corpus, **MVU is complete** (`history-seg-refactor-mvu`, PR #166–#274; single `App::update` entry), **gix is complete** (`history-seg-gix-migration`, PR #283–#292, released 1.56.0), and the **800-LoC decomposition campaign is complete** (`history-seg-module-decomposition`, PR #248–#308). Do not hand a newcomer "land an MVU phase" — that work shipped.
- **Current open work surfaces (read before claiming a task):**
  - `ROADMAP.md` "Road to 2.0" (`:557-625`): lean refactor→launch. With items 1/1b (decomposition + MVU) now **done**, the live remaining tracks are (2) daily-driver fixes, (3) test de-risking, (4) thesis features gating 2.0, (5) distribution/launch hygiene. NOTE the section is itself slightly stale — `:567` still calls `app/mod.rs` "~12k lines" and the decomposition "the next track"; it's actually 1009 lines and done.
  - `BUGS.md` — SMALL/BIGGER/MAYBE/FIXED. The FIXED section now runs through v1.55.2 (`:118`); the #37 implementer pick (pane cursor-block on `DEC ?25l`) is FIXED (v1.41.26, `:175`). Fresh SMALL items below.
  - `TODO.md` — `[ ]` items under "Now — Foundations", "Next — v2.0 gate", "Later".
  - `docs/V1_70_PLAN.md` — "Mise en Place": the post-2.0 addressability plan (Stations/Plates/typed Order-rail/Bells; one protocol, three clients). This is the live next-big-thing; it **hard-depends on the decomposition** (now satisfied).
- **Implementer first task (small, well-bounded, hits the doc-sync rule):** pick a `BUGS.md` SMALL item the maintainer has already diagnosed. Concrete current options: (a) the `!` percent-expansion bug — `! awk '... printf "%d: %s"'` gets `%d`/`%s` greedily substituted with cursor/selection paths before exec; suggested fix in the entry is to only expand a documented token set with a non-alnum boundary or `%%` escape (`BUGS.md:18-26`). (b) "screen should flash if I'm doing something that hits a wall — e.g. `j` at the top of a directory" (`BUGS.md`, SMALL). Both are single-surface fixes. Walk-through: read `AGENTS.md:38-99`, find the module, add the handler in `src/app/actions.rs` (`apply_inner`) or the pure half in `AppState::apply` — **not** `mod.rs` — write a test, bump `Cargo.toml` patch, add a `CHANGELOG.md` Unreleased line, move the BUGS.md item to FIXED in the same commit.
- **Planner first task (architectural-direction, lots of reading):** the MVU slice is no longer available. The current planner-scale work is **scoping `docs/V1_70_PLAN.md` (Mise en Place)** — the typed addressability protocol + the `spyc-proto`/`spyc-pty`/`spyc-os` crate split it carries (`ROADMAP.md:622-640`). A good bounded first slice: read V1_70_PLAN + `docs/V1_60_PLAN.md`, then propose (as a Watercooler Decision thread) the `Station`/`Plate` type boundaries against the current `Runtime`/`PtyHost` registry. Lower-risk alternative still on the board: a daily-driver design from `ROADMAP.md:585-600` — `^a s` path-handoff anchoring (`docs/PATH_HANDOFF_PLAN.md`) or configurable startup pane tabs (`docs/PANE_STARTUP_TABS_PLAN.md`).
- **Tester first task (validation, no behavior change):** `TODO.md` "Testing" is the priority. The `App` workflow-test harness for high-bug-density flows (routing/focus, pane↔task transitions, session restore) — `docs/TEST_IMPROVEMENT_PLAN.md`, called out as ROADMAP Road-to-2.0 item 3 and the de-risking that gates the rest. Concrete: extend snapshot coverage on `list_view`/`pager` (`ROADMAP.md:126`, infra wired via `insta`+`TestBackend`; pager already has `src/ui/pager/tests.rs`), or add a `tests/`-level workflow test for a focus-routing flow. The `pane_roundtrip.rs` integration test (new since #37) is a model to follow.
- **Reviewer/critic first task:** read this refreshed seed set, then either (a) write the threat-model Note on the MCP socket surface (`src/mcp/server.rs`, `src/mcp/config.rs` enterprise policy + takeover) — the v1.41.24 project-scoped-discovery hardening is documented but `SO_PEERCRED`/`LOCAL_PEERPID` peer-auth remains defense-in-depth-only; or (b) chase the concrete doc-drift this refresh surfaced: `README.md:73` says "Rust 1.85+" but MSRV is `rust-version = 1.88` (`Cargo.toml:8`); `ROADMAP.md:567` still says `app/mod.rs` is ~12k lines (now 1009); `SECURITY.md` has no supported-versions block and pins deny.toml "as of v1.37.1" (`SECURITY.md:45`) vs shipped 1.56.0. Each is one-file-one-edit in the doc-sync surface.
- **PM/scribe first task:** the distribution/launch-hygiene backlog is now the sequencing bottleneck (the engine work is done). Turn `TODO.md` "Next — v2.0 gate › Distribution" `[ ]` items (release automation `:262`, macOS notarization `:268`, Homebrew tap `:287`, `SECURITY.md`/`CODE_OF_CONDUCT.md`/PR-templates `:305-312`) into Watercooler threads with explicit ball/ownership so the 2.0 cut has a tracked critical path.
- **Doc-sync gate still applies to every role's first task** (`AGENTS.md:88-99`): stage code → bump `Cargo.toml` if user-visible → `CHANGELOG.md` Unreleased → update `FEATURES.md`/`AGENTS.md`/`src/ui/help.rs` if behavior changed → `ARCHITECTURE.md` if a decision changed → move fixed bugs to `BUGS.md` FIXED → `:command` goes through `COMMAND_TABLE` (no hand-synced punt list anymore) → `make check`.

Inferred:
- The lowest-friction first task across roles is still one that exercises the doc-sync rule — but the *content* shifted from "help finish the refactor" to "fix a daily-driver papercut / harden tests / advance distribution," because the refactor is done. — confidence: high — basis: ROADMAP Road-to-2.0 items 1/1b complete per the three history segment threads; remaining items are 2–5.
- The right first move for a non-Derek contributor is still *propose, then implement* (open a thread before >~50 lines) — this is single-developer repo (no CODEOWNERS; `git log` shows one author). — confidence: high — basis: `onboarding-team-map`; unchanged at #311.

Next query: `watercooler_search(query="road to 2.0 remaining distribution V1_70 addressability", thread_topic="history-seg-docs-planning", code_path=".")`

Related:
- `onboarding-architecture` — context for the planner pick (the V1_70 crate split depends on the now-complete decomposition).
- `onboarding-working-map` — the decomposed tree the implementer pick lands in (handler goes in `app/actions.rs`/`AppState::apply`, not `mod.rs`).
- the history/insight corpus — **this is how a newcomer learns how the code got here**: `history-seg-refactor-mvu`, `history-seg-gix-migration`, `history-seg-module-decomposition` (the three completed engine campaigns), `insight-trajectory` (the plan-document arc / what's next), `history-seg-docs-planning` (the planning trail), `history-arc-00-genesis` (cspy→spyc origin).

Provenance:
- Files read: `ROADMAP.md:1-40,557-660` (Road to 2.0 + post-2.0 + plan-doc status), `BUGS.md:1-130` (SMALL/FIXED), `TODO.md:1-331` (open `[ ]` items), `docs/V1_70_PLAN.md:1-45` (Mise en Place thesis), `AGENTS.md:38-99` (module index + doc-sync gate), `README.md:73` (Rust 1.85 — stale vs MSRV 1.88), `Cargo.toml:3,8` (1.56.0 / rust-version 1.88), `SECURITY.md:35-45,113`.
- History entry_ids consulted: MVU decision 01KTMKVE85DEBMBWYCXY7YHP5E; gix release-1.56.0 01KTMMTF0MQ96P7BVN7QQPVBNS; decomposition close 01KTMMPARGNTSQB2Z67G6KBKQ0; PagerStream payoff 01KTMN2XSH67BNFQPAMTSFD81X.
- Prior #37-era entry: 01KR0PP11NEC9JF0PT9F7FZPG2 (left intact as point-in-time snapshot).

<!-- Entry-ID: 01KTND6JNSJVXVCXK1BCXVFDR0 -->
