# spyc roadmap

The strategy layer — thesis, current state, the 2.0 gate, non-goals, and the
decisions log. The per-item **backlog lives in [GitHub Issues](https://github.com/Tripstack-Corp/spyc/issues)**
(organized on the [roadmap board](https://github.com/orgs/Tripstack-Corp/projects/1));
`CHANGELOG.md` is the shipped history. Detailed designs for not-yet-started work
live in `docs/drafts/*_PLAN.md`; shipped or parked plans are archived in
`docs/archive/`.

## Thesis

spyc is a vi-keyboard-driven file commander that exposes itself to an AI coding
agent as a queryable context source. The target user is a developer who already
thinks in vi motions and wants Claude Code living in the same workspace -- not
one window over, not in a browser tab, in the same session, sharing context.

The MCP server shifted the tool's nature: spyc isn't just "a file manager with
Claude in a pane." It's a file manager that Claude can query -- current
directory, cursor, picks, inventory, filter, git branch -- via a standard
protocol. That bidirectional awareness is the positioning that differentiates
spyc from `tmux` + `claude`.

Every other feature -- picks, inventory, pager, status bar, sessions -- is
supporting infrastructure that makes the split-pane workflow fast and
comfortable. The roadmap is organized accordingly: the pane-and-agent
integration is the defining work track, not the trailing milestone.

## Where we are (v1.97.2)

The structural foundation has been **done** for a while: the full MVU/Elm
migration (Model/Runtime/ViewState split, effects-as-data, single message
channel, pure render), the `app/mod.rs` decomposition (12.4k → ~1k lines,
ceiling-guard-enforced), the 800-LoC file rule, the complete git→gix migration
(100% in-process, guard-enforced, with in-house side-by-side diff/show/blame
views), off-thread PagerStream (grep / git-view / agent transcripts on one
seam), and unified input routing (`route_input`/`InputSink`, `Focus` as the
routing authority).

Since then the **thesis work has largely shipped** — the differentiators the
competitive review ([`docs/COMPETITIVE_REVIEW.md`](docs/COMPETITIVE_REVIEW.md))
named as spyc's wedge are now real, not planned:

- **Agent awareness (P0–P3, complete).** Per-pane activity dots driven by an
  MCP/hook *self-report* channel (working/blocked/done) with an output-timing
  fallback, desktop notifications + a branded visual-bell border pulse on the
  attention transition, and live Claude session-id pinning so `-r` resumes the
  *exact* conversation. The reliable, cooperative answer to "which agent needs
  me" that herdr does fragile-ly by screen-scraping. See
  [`docs/AGENT_ORCHESTRATION.md`](docs/AGENT_ORCHESTRATION.md).
- **Worktree MCP suite.** `list`/`create`/`open`/`remove`/`clean_worktrees` +
  leases, all in-process gix; safe-by-default teardown archives to the graveyard.
- **Merge / scope registry (P2).** `register_scope`/`list_scopes`/
  `wait_for_scope_clear`/`release_scope` for advisory multi-agent merge
  coordination, plus the `spyc-semver` merge driver that auto-resolves the
  version-line conflict every concurrent PR collides on.
- **In-process review loop.** Syntax-highlighted side-by-side diff/show/blame
  per worktree — the uncontested review wedge in the TUI lane.
- **Vertical split (Stage 1 + 2).** A live-reloading preview column, plus a full
  second file-commander with its own cwd/git/harpoon and worktree-scoped MCP.
- **Lua scripting.** `map KEY lua` + an `init.lua` platform (`spyc.map`/
  `spyc.command`/`spyc.on` events, full-`Action` `spyc.action`, live reads) on a
  sandboxed off-thread worker.
- **Chord + command overhaul.** which-key hint popup, `Space` leader, the
  `command`/`lua` DSL verbs, and a guard-enforced global/frame/pane tier
  taxonomy; low-frequency features demoted to `:`-commands.
- **Crash-sufficient autosave**, `^z` pane suspend/resume, and per-channel
  notification gating (`Blocked`-only bell/flash, quiet `Done` desktop ping).

AGENTS.md is the architectural contract + conventions; ARCHITECTURE.md holds
the deep design decisions. The feature set is **launch-ready** — what remains
before 2.0 is just the distribution / launch pass. The daily-driver papercuts
and the near-term thesis features (session forking, prompt templates) are
post-2.0 work — see "Road to 2.0."

## Working tracks

Work proceeds along three parallel tracks. They're not strictly sequential;
distribution work can land while thesis work is still in flight, and
foundations work continues throughout.

- **Foundations** -- testing, hardening, build hygiene. The minimum to not
  embarrass ourselves and to make every other change safer.
- **Thesis** -- deepening the agent integration until the split-pane workflow
  is measurably better than `tmux` + `claude` for the target
  audience. This is where the tool earns its reason for being.
- **Distribution** -- release automation, signing, packaging, docs.  Turns a
  repo into a tool people can install, trust, and find.

## Road to 2.0

2.0 is a public-distribution + signaling bump — **not** a feature gate. Where we
stand today is launch-ready: the structural foundation, the agent-awareness
wedge, the in-process review loop, worktrees + scope registry, vsplit, and Lua
all shipped (see "Where we are"), on top of the MVU rewrite and the
test-de-risking campaign (workflow harness, proptest/cargo-fuzz — see
[`docs/archive/TESTING_STRATEGY.md`](docs/archive/TESTING_STRATEGY.md)).

So the **only** thing between here and 2.0 is the **distribution / launch pass** —
the launch plan below, end to end. Everything else moves to post-2.0: the
daily-driver papercuts (path handoff, startup pane tabs, pane-recovery Phase 0,
cwd-export, keymap-DSL completeness, PgUp/PgDn) and the near-term thesis features
(session forking `^a f`, prompt templates) — all tracked in
[Issues](https://github.com/Tripstack-Corp/spyc/issues). Good, standalone work;
it just doesn't block the launch.

**v2.0 — public distribution launch.** Cut once the launch plan is done.
External announcement (TripStack engineering blog, optional Show HN). The major
bump is a signaling choice as much as a semver one — the MCP positioning shift +
public distribution mark the transition.

## Launch plan (2.0)

> **Execution manual:** this section holds the strategic gates and open
> decisions; the end-to-end release *mechanics* — release streams, CI
> workflows, signing/notarization, Homebrew, org setup — live in
> [`docs/RELEASE_ENGINEERING.md`](docs/RELEASE_ENGINEERING.md), the launch
> operating manual. Keep the two in sync when a gate moves.

Benchmarked against Yazi (github.com/sxyazi/yazi, ~39.9k stars) as the
gold-standard reputable TUI tool. The MCP / Claude-Code pairing
remains spyc's differentiator — Yazi has nothing like it; keep it
front and centre. Goal: a release that someone reading the repo cold
can trust enough to make their daily file manager. Not a promotion
blitz — just enough signal to feel "this is real, maintained, and
works for me."

### Open decisions

- [x] **Repo home: RESOLVED (2026-07-02) — full move to GitHub.**
  `github.com/Tripstack-Corp/spyc` is canonical; **all dev + CI move there**
  (not a mirror). The repo stays **private** until launch. `Cargo.toml
  repository =`, the clone URLs (README/INSTALL/CONTRIBUTING), and the CI moved
  in this pass: `.github/workflows/{ci,audit}.yml` port the retired
  `bitbucket-pipelines.yml` (archived under `docs/archive/`). Remaining
  GitHub-side setup (branch protection on `main`, the weekly-audit schedule,
  and the distribution workflows — release/snapshot/homebrew per
  RELEASE_ENGINEERING.md) is done on the repo before it goes public.
- [ ] **License footer.** Already BSD-3-Clause in `Cargo.toml`;
  confirm for public release and that LICENSE is at repo root.
- [ ] **Status statement wording.** Default proposal: *"Public beta,
  daily-driver-ready. macOS and Linux."*

### Required for 2.0

1. **Repo move/mirror execution** (per the decision above): public
   repo, history + tags pushed, `Cargo.toml` repository field,
   README/INSTALL link updates, branch protection on `main`.
2. **Demo capture at top of README.** 30–60s asciinema or MP4 of the
   full Claude pairing loop: launch → `F` fuzzy-find → `:grep` →
   `^\` to Claude → "what files am I picking?" answered via
   `get_spyc_context` → `gf` jump on a path Claude mentions → quit.
   Place as the first media element after the value prop.
3. **Release pipeline + binaries.** Tag push triggers cross-compile
   matrix — macOS arm64 + x86_64, Linux x86_64 + arm64 (musl,
   static) — with artifacts attached to Releases. Homebrew tap
   (`brew tap <org>/spyc && brew install spyc`) auto-bumped from the
   release workflow. crates.io publish (binary-only crate,
   acceptable). AUR `spyc-bin` deferred post-2.0 unless a volunteer
   emerges.

### Cheap wins — batch with the launch pass

- **README hygiene**: stale status line replaced with the agreed
  status statement; headline sells the Claude angle in one sentence;
  spot-check keybinding tables.
- **Repo scaffolding**: issue templates (bug: repro/version/OS/
  terminal; feature: what/why/would-you-use-it), PR template,
  CODE_OF_CONDUCT (Contributor Covenant, link only). SECURITY.md ✅
  exists.
- **`MIGRATION.md`**: three small keybind tables (ranger → spyc,
  lf → spyc, Yazi → spyc, ~10 binds each) plus one paragraph on what
  spyc has that they don't (the MCP integration). Unblocks the two
  remaining Yazi-review recommendations
  ([`docs/COMPETITIVE_REVIEW.md`](docs/COMPETITIVE_REVIEW.md) §1d).
- **Signing & supply chain**: macOS Developer ID signing +
  notarization (without it the first user report is "macOS says spyc
  is damaged"); Linux minisign signatures with the public key in the
  repo; SBOM via `cargo-sbom`/`cargo-auditable`; reproducible-build
  verification job (toolchain already pinned, `SOURCE_DATE_EPOCH`,
  rebuild-and-diff). Proportionate — no SLSA theater (see Non-goals).
- **Shell completions**: `spyc --generate-completion {bash,zsh,fish}`
  via clap derive; ship in release artifacts.
- **First-run hint flash**: on first launch (no
  `state_root()/first_run_done` marker), flash that (1) `^a`/`^w` are
  reserved chord prefixes (rebindable) and (2) `?` opens help.
  ~30 lines; saves every tmux/shell-heavy user the same surprise.
- **`:tutor` (vimtutor-style)**: interactive walkthrough on a
  pre-baked scratch directory — motions, marks, picks, `=` filter,
  pager, `^a` family, MCP context, sessions. Each lesson sets a goal,
  watches for the action, advances. The one-command demo for a
  Show-HN reader. Tutor content tracks bindings — add to the AGENTS.md
  doc-sync checklist when it lands.

### Explicitly deferred (not 2.0)

- Dedicated docs site (mdbook/Starlight). The Markdown reads fine on
  GitHub; revisit if docs outgrow single files.
- Blog/marketing posts beyond one Show HN at 2.0. CHANGELOG is enough.
- Windows support (see Non-goals — WSL is the story).
- Discord/Matrix/forum. GitHub Discussions post-launch if traffic
  warrants; a chat channel is a maintenance commitment.
- Sponsorship buttons, until traction warrants.

### Done-criteria for the 2.0 launch

A user landing on the repo cold should be able to:

1. Watch a 30-second demo in the README and understand what spyc does
   and why it's different.
2. Install via Homebrew *or* a pre-built Release binary — no Rust
   toolchain required.
3. Read FEATURES.md and INSTALL.md without broken links or stale
   version numbers.
4. File a bug or feature request via templated issues.
5. See a recent release (within ~30 days) and a current CHANGELOG.
6. Read a clear 2.0 CHANGELOG entry: what changed since 1.x, what
   stability we promise going forward.

Sequencing: repo decision first (blocks everything) → README hygiene
→ demo capture → scaffolding → release pipeline + first 2.0 binaries
→ Homebrew → migration page. The 2.0 CHANGELOG entry is written last,
once we've daily-driven our own builds for a few days.

## Backlog & roadmap

The live, actionable work — features, fixes, tooling, the speculative icebox, and
the post-2.0 arc — is tracked in **[GitHub Issues](https://github.com/Tripstack-Corp/spyc/issues)**,
labeled by `area:*` / `type:*` and organized on the **[roadmap board](https://github.com/orgs/Tripstack-Corp/projects/1)**. Signposts:

- **`2.0` milestone** — the launch-gating work (see "Launch plan" above).
- **`icebox`** — speculative / nice-to-have ideas.
- **`needs-design`** — items with a design doc in `docs/drafts/` or needing a spike.
- **`good first issue`** — small, self-contained entry points.

This file is the *strategy* layer — thesis, current state, the 2.0 gate,
non-goals, and the decisions log. The per-item backlog lives in Issues; detailed
designs for not-yet-started work are in `docs/drafts/*_PLAN.md`; shipped or
parked designs are archived under `docs/archive/`.

## Non-goals

These are things someone will inevitably ask for. The answer is no,
and the roadmap committing to that saves a lot of drift.

- **Native Windows support.** WSL is the supported story.
  `portable-pty` technically works on Windows but debugging the
  failure modes is a tax we're not paying. (A future crate split — the
  archived Mise en Place design — would isolate platform code so a
  volunteer *could*; that's the extent of the commitment.)
- **Plugin system.** A decade of maintenance debt for a feature 3% of
  users will touch. The `.spycrc` DSL and keymap extensibility are
  the customization surface.
- **Localization.** English only.
- **Telemetry.** Not even anonymized opt-in. The greybeard half of
  the audience will not forgive it and the vibe-coder half won't
  notice it's missing.
- **SLSA L3 / supply-chain theatre.** Minisign + SBOM + a
  reproducible-build job are proportionate. Full SLSA attestation is
  not.
- **Mouse support beyond the pane forward.** Keyboard-first by
  thesis; the pane forward (tracked as an issue) exists only because
  the pane hosts third-party mouse-aware tools.
- **tmux command compatibility.** We have our own bindings.
- **Persistent search index** (tantivy/ctags). Ripgrep on a 100K-file
  repo is sub-second cold; the maintenance burden isn't worth it.

## Decisions log

Condensed record of the choices that shaped current behavior — kept
so we don't re-litigate them. Full history in CHANGELOG.md.

- **Sync end-to-end, no tokio.** `std::thread` + one mpsc channel.
  Revisit never; async would be a regression for this workload.
- **MVU landed pre-2.0** (2026-05-30) so the launch ships on the
  clean foundation; strangler-fig, every phase behavior-equivalent
  behind green CI. Shipped.
- **`^Z` backgrounds tasks** despite overriding terminal-suspend
  muscle memory — consistent with spyc trapping most ctrl-combos.
  Backgrounded tasks don't survive `spyc -r` (children tied to the
  spyc PID; reattach is a rabbit hole; quit-time prompt covers it).
- **Task-viewer shape**: exited tasks auto-promote to buffer history
  on view-close instead of an explicit dismiss step.
- **No persistent search index** — see Non-goals.
- **Claude restore types `/resume <sid>`** into a fresh spawn (the
  `--resume` CLI flag has a mount-crash regression) with
  verify-and-retry on the Enter; codex restores via
  `codex resume <UUID>` directly; gemini recomputes its unstable
  resume index from the saved UUID at restore time; zot uses
  `--continue`.
- **OSC 72 DnD deferred** until a second terminal (beyond kitty)
  implements it.
- **Renovate auto-merges patch bumps** once public (May 2026); minors
  grouped weekly; majors labeled.
- **macOS CI deferred to post-launch**; PR template asks
  cross-platform contributors to run `make check` locally.
- **git is 100% in-process gix** in production, guard-enforced; no
  subprocess git, no gix repo open on the 1 Hz poll.
- **Crate-over-handroll**: prefer a small focused crate (features
  trimmed) over shelling out or reimplementing (libproc over
  ps/lsof). "Lightweight" means small runtime + few transitive deps,
  not "avoid crates."
- **No `unsafe` going forward** — DI / rustix / signal-hook over raw
  libc; unsafe is exceptional and isolated (a future crate split would
  give it a dedicated crate).

## Doc map

| Doc | Role |
|---|---|
| `ROADMAP.md` | This file — strategy, the 2.0 gate, non-goals, decisions log. |
| [GitHub Issues](https://github.com/Tripstack-Corp/spyc/issues) + [roadmap board](https://github.com/orgs/Tripstack-Corp/projects/1) | The live backlog — features, fixes, ideas, the post-2.0 arc; labeled + milestoned. |
| `docs/archive/BACKLOG_DRAFT_NOTES.md` | Archived raw intake backlog — open items migrated to Issues (2026-07); kept as history. |
| `CHANGELOG.md` | Shipped history (git-cliff, conventional commits). |
| `AGENTS.md` | The canonical agent guide: architectural contract (MVU invariants), module map, conventions. |
| `CLAUDE.md` | One-line `@AGENTS.md` import (Claude Code entrypoint). |
| `ARCHITECTURE.md` | Deep stable design decisions. |
| `DESIGN.md` | UI design language (theme, components, glyphs). |
| `FEATURES.md` | User-facing feature reference. |
| `CONFIGURATION.md` | Config reference (`.spycrc.toml`, notifications, keymap DSL, Lua). |
| `docs/RELEASE_ENGINEERING.md` | The launch operating manual — release streams, CI, signing, Homebrew, org setup. |
| `docs/BRAND.md` | Brand & identity — the name story, palette, voice. |
| `docs/AGENT_ORCHESTRATION.md` | How the agent activity-dots / notifications / session-resume / scope registry fit together (living reference). |
| `docs/drafts/AUTO_APPROVAL_PLAN.md` | Pending design (post-2.0). |
| `docs/drafts/PANE_STARTUP_TABS_PLAN.md` | Pending design (road-to-2.0). |
| `docs/drafts/PATH_HANDOFF_PLAN.md` | Pending design (Option A is road-to-2.0). |
| `docs/archive/TESTING_STRATEGY.md` | Testing strategy & guidelines (coverage, anti-"test theater", proptest/cargo-fuzz, AI-testing rules). Campaign complete (#426–#438); kept as the how-we-test reference. |
| `docs/archive/V1_60_PLAN.md` | Archived design — CounterTop multi-instance hub. Considered & parked (fights the single-process core); summarized in Post-2.0. |
| `docs/archive/V1_70_PLAN.md` | Archived design — Mise en Place typed addressability + crate split. Post-2.0 speculative; MCP already covers the basics. Summarized in Post-2.0. |
| `docs/COMPETITIVE_REVIEW.md` | Consolidated competitive review + GTM: the AI coding-agent-manager category (§1–§1c: herdr, psmux, claude-code-ide.el) plus the TUI file-manager lane (§1d: Yazi, folded 2026-07-02). Refresh on a competitor's next major. (Standalone Yazi original archived at `docs/archive/YAZI_COMPETITIVE_REVIEW.md`.) |
| `docs/archive/` | Shipped plans, kept as historical record. |

> **Note on pending plans:** the three feature plans predate the MVU
> decomposition — their designs hold, but `src/app/mod.rs:NNNN`-style
> file pointers are stale; re-resolve against the current module
> layout when picking one up.
