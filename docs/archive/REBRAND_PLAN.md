# Rebrand Plan (ABANDONED — staying with spyc, 2026-07-02)

> **Status: NOT PURSUED.** The project keeps the name **spyc**; this
> rename-to-Cayenne plan is retained only as decision-history — the
> trademark/legal rationale (§1), the homage-line guidance (§6), and the
> risk register (§7) still inform the public 2.0 launch (see
> `docs/RELEASE_ENGINEERING.md` §13.6). Everything below is the original
> draft, verbatim; ignore its rename mechanics.

---

# Rebrand Plan: `spyc` → Cayenne (`cay`)

> **Status:** draft for review. This is a planning document — it describes
> intended work, not current behavior, so the usual "comments state what IS"
> rule doesn't apply here.

## 1. Decision & rationale

We are rebranding the project from **`spyc`** to **Cayenne**, distributed as the
binary **`cay`**.

- **Brand / product name:** **Cayenne** (carries the pronunciation, unambiguous).
- **Binary / command / crate:** **`cay`** (the fast 3-letter terminal form).
- **New home:** a **fresh public GitHub repository named `Cayenne`** — seeded as a
  **clean slate with no carried git history** (the private Bitbucket repo keeps the
  full record). Existing users are told to download from the new repo; the old
  Bitbucket repo gets an archival notice pointing at it.
- **Kept as-is:** the 🌶️ chili logo, the spice theme, and the spice-pair session
  names (`SAFFRON_CUMIN`, …). The rebrand *strengthens* these — Cayenne makes the
  chili literal rather than a pun.

**Why now (condensed from the legal deep-dive):** our Rust engine and vi-style
keybindings are not a legal risk — UI command layouts and keystrokes are
uncopyrightable "methods of operation" (*Lotus v. Borland*, *Cisco v. Arista*,
*Mitel v. Iqtel*). The *only* real exposure is (a) the name `spyc` being a
single-letter variant of SideFX's bundled `spy` utility, and (b) the README line
`spy (inspired by SideFX's in-house file manager) + claude = spyc`, which is a
written admission of derivation and a hybrid-mark construction that fails
nominative-fair-use. SideFX holds **no** registered trademark on "spy" for
software, so litigation risk is low — but a proactive rename is cheap insurance
that also removes the enterprise-legal-review friction and gives us a clean,
ownable identity on a public repo. Trademark risk: **Medium → Negligible**.
Copyright risk: **Low** (unchanged). This is a brand decision, executed early,
not a forced reaction.

## 2. Canonical naming scheme

Every occurrence falls into one of these buckets. This table is the source of
truth for the rename:

| Concern | Old | New |
|---|---|---|
| Brand / product name | spyc ("spicy") | **Cayenne** |
| Binary, command, crate name | `spyc` | `cay` |
| Cargo package / `[[bin]]` | `spyc` | `cay` |
| Rust crate path | `spyc::` | `cay::` |
| Config file | `.spycrc.toml` | `.cayrc.toml` |
| State dir | `~/.local/state/spyc/` | `~/.local/state/cay/` |
| Env var prefix | `SPYC_*` | `CAY_*` |
| Context marker file | `.spyc-context-<pid>.json` | `.cay-context-<pid>.json` |
| MCP server name (`.mcp.json` key) | `spyc` | `cay` |
| MCP resource URI | `spyc://context` | `cay://context` |
| MCP context tool | `get_spyc_context` | `get_cay_context` (alias old) |
| Debug/trace logs | `spyc-debug-*`, `spyc-key-trace-*` | `cay-debug-*`, `cay-key-trace-*` |
| Internal trap sigil | `SPYC-TRAP` | `CAY-TRAP` |
| Debug macro | `spyc_debug!` | `cay_debug!` |
| Context type | `SpycContext` | `CayContext` |
| Logo asset | `docs/spyc-logo.png` | `docs/cay-logo.png` |
| Repo | `bitbucket.org/tripstack/spyc` | new public `github.com/<org>/Cayenne` |
| 🌶️ emoji, spice session names | — | **unchanged** |

**Scale:** ~1,774 occurrences of "spyc" (case-insensitive) across ~160 files,
but heavily weighted toward docs and the (immutable) CHANGELOG. The
code-critical surface is concentrated in `src/mcp/`, `src/paths.rs` /
`src/state/`, `src/context.rs`, `src/config/`, `build.rs`, and `src/lib.rs`.

## 3. Guiding principles

1. **No silent user-data loss.** Existing `spyc` users get a *new* `cay` binary
   that, by default, looks at *new* paths. State (sessions, graveyard,
   inventory, harpoon, marks, frecency) must be migrated, not abandoned —
   especially the **graveyard** (holds recoverable soft-deleted files) and
   **inventory** (persistent yank cache). Losing these is catastrophic.
2. **Backwards-compatible at every cross-process / user contract**, atomic where
   a contract spans processes:
   - Read both `CAY_*` and `SPYC_*` env (prefer `CAY_*`); write only `CAY_*`.
   - Config: read `.cayrc.toml`, fall back to `.spycrc.toml` for one release.
   - MCP context tool: accept both `get_cay_context` and `get_spyc_context`.
   - The MCP socket bridge (env var ↔ `.mcp.json` ↔ `--mcp` proxy ↔ socket
     path) must flip together — a half-rename hangs the agent silently.
3. **Historical records stay verbatim.** `CHANGELOG.md` entries that shipped as
   `spyc`, and everything under `docs/archive/`, are immutable history. Don't
   rewrite them; add new entries going forward.
4. **Line numbers in the appendix are as-of-recon** — re-grep by symbol at
   execution time, the code moves.

## 4. Cross-cutting decisions

Each decision below has a recommendation; the genuinely-your-call ones are also
collected in §8 (Open decisions).

### D1 — Version: a deliberate launch number (see §8.4)
The rename breaks env vars, the config filename, and the MCP server name — all
user/agent contracts. On the existing line that's a major bump. But the public
repo is a clean slate (D10), so the launch number is genuinely open: continue as
`2.0.0`, or reset to a fresh `1.0.0` for the new repo. Either way it's a single
deliberate number; migration shims make the break feel seamless.

### D2 — State migration: one-time copy on first run (**recommended**)
On `cay` startup, if `~/.local/state/cay/` does **not** exist but
`~/.local/state/spyc/` does, **recursively copy** old → new (preserve mtimes),
then flash `migrated spyc state → cay (sessions, graveyard, harpoon preserved)`.
- **Copy, not move/symlink** — a user may still run an old `spyc`; symlinks are
  fragile; copy is ownership-clean.
- Run `health::check()` on the new dir afterward to validate JSON↔payload pairs
  (graveyard/inventory) and clean corruption.
- If both dirs exist, prefer the new one (already migrated); do nothing.

### D3 — Config file: dual-read fallback for one release (**recommended**)
`cay` reads `.cayrc.toml` first (user `~/.cayrc.toml` + project `./.cayrc.toml`),
falling back to `.spycrc.toml` if absent. If only the old file is found, flash a
one-time nudge: `reading .spycrc.toml — rename to .cayrc.toml to adopt cay`. Drop
the fallback in the release after `2.0.0`. The project-config security sandbox
(executing bindings only from `$HOME`) is unchanged.

### D4 — Env vars: read both, write new (**recommended**)
All readers use `env::var("CAY_X").or_else(|| env::var("SPYC_X"))`. All writers
(pane injection, `.mcp.json`/codex env block) emit only `CAY_*`. Applies to
`CAY_MCP_SOCK`, `CAY_PANE_ID`, `CAY_CONTEXT`, `CAY_PANE_CMD`, `CAY_DEBUG`,
`CAY_KEY_TRACE`, `CAY_MCP_DEBUG`. Build-time `CAY_GIT_SHA` / `CAY_BUILD_TIME` /
`CAY_RUSTC_VERSION` are internal — rename outright (must move with their `env!`
sites in `src/lib.rs`).

### D5 — MCP context tool name: alias both (**recommended**)
Advertise `get_cay_context`; keep dispatch accepting `get_cay_context |
get_spyc_context` so any agent with the old name in context/muscle-memory still
works. Update `SERVER_INSTRUCTIONS`, AGENTS.md, README to the new name. Drop the
alias a release later. All other tools (`search_paths`, `git_status`,
`create_worktree`, `report_status`, …) are brand-neutral — no change.

### D6 — Agent config (`.mcp.json` / `.codex/config.toml`) migration
On agent-pane launch, when writing the `cay` server entry, **remove any stale
`spyc` entry we previously wrote** in the same file (detect by our
`SPYC_MCP_SOCK`/socket-path signature), then write the `cay` entry. On exit,
`cleanup_written_mcp_configs` removes only the `cay` entry (leave a concurrent
old `spyc` instance's entry alone). The context-orphan sweep recognizes both
`.spyc-context-*` and `.cay-context-*` during the transition. **Enterprise:** the
Jamf `managed-mcp.json` / `managed-settings.json` allow/deny checks key on the
server name — update the detection to match `cay`, and document that org admins
must update their managed policy from `spyc` → `cay` (coordinate before launch).

### D7 — `SPYC-TRAP` sigil → `CAY-TRAP` (**recommended: rename**)
It's an internal, grep-unique, guard-enforced sigil (2 real slugs:
`cursor-read-ssh`, `iterm-osc1337`; 3 code anchors + 2 ARCHITECTURE.md markers).
Rename to `CAY-TRAP`: update the string-assembly in the guard
(`format!("{}{}", "CAY-", "TRAP")`), the guard's error-message literals, the 3
code anchors, the 2 doc markers, and the doc template. Keeping `SPYC-TRAP` would
leave a stale brand string in the one place future edits are *required* to touch.

### D8 — Repo & CI platform (your call — see §8)
New canonical home is public GitHub `Cayenne`. Decide whether **development**
moves to GitHub (→ port `bitbucket-pipelines.yml` to GitHub Actions, switch PR
tooling from `bkt` to `gh`, keep-branches rule becomes GitHub-native) or GitHub
is a **distribution mirror** while dev stays on Bitbucket. Recommendation: if the
public repo is canonical, move dev there too — one source of truth, and external
contributors expect GitHub PRs/Actions. This is the largest non-code workstream.

### D9 — License & IP: BSD-3 today, final form pending legal (launch blocker)
The project is **BSD-3-Clause** (`Cargo.toml`, `deny.toml`) — BRAND.md's earlier
MIT/Apache line was an error, now corrected to BSD-3. The **final public-release
license is pending legal review**: what Tripstack can publish, under which
license, and as whom. Before launch: confirm Tripstack owns the work and signs
off on a public OSS release under the company name, then reconcile a *single*
license answer across `Cargo.toml` `license`, a root `LICENSE` file, `deny.toml`,
and BRAND.md. This is a launch gate, not a code task — but the code can rename
everything else while it's pending.

### D10 — Public repo is a clean slate (decided)
The public GitHub `Cayenne` repo starts **fresh: a single initial commit, no
carried git history.** The private Bitbucket `spyc` repo keeps full history as the
internal provenance record — so the "built from scratch" trail isn't lost, just
not public (mild upside: the public record won't carry the old `spy + claude`
derivation commits the legal pass flagged). Only a curated set of files carries
over; internal planning/review docs and the owner's backlog are shed. BRAND.md
carries. See Phase 6 for the carry/shed manifest.

## 5. Workstreams (PR sequence)

Each phase is an independent PR (squash-merged, version-bumped). Order matters:
internal-first, then the breaking/user-facing layers, then docs, then infra.

### Phase 0 — Pre-flight (no code)
- [ ] Verify name availability: crates.io `cay` **and** `cayenne` (publish, or at
      least reserve); GitHub org/repo `Cayenne`; Homebrew formula name.
- [ ] Create the public GitHub `Cayenne` repo (license file in root — BSD-3, our
      current license; add `LICENSE` explicitly for a public OSS repo).
- [ ] Decide D8 (CI platform) and D1 (version `2.0.0`).
- [ ] Reserve the binary name on any package channel we'll publish to.

### Phase 1 — Internal identifiers (mechanical, low-risk, no behavior change)
- `Cargo.toml`: `name = "cay"`, `[[bin]] name = "cay"`, `description`,
  `repository` → GitHub URL; `cargo update -p cay` to rewrite `Cargo.lock`.
- `build.rs`: `SPYC_GIT_SHA`/`SPYC_BUILD_TIME`/`SPYC_RUSTC_VERSION` → `CAY_*`,
  and their `env!()` sites in `src/lib.rs`.
- `src/main.rs` / `src/lib.rs`: `spyc::run()` path, `#[command(name = "cay")]`.
- `spyc_debug!` macro → `cay_debug!` (40 call sites).
- `SpycContext` type → `CayContext` (~21 refs).
- `Makefile`: `BINARY := cay`, install paths → `~/.local/bin/cay`.
- `fuzz/`: `name = "cay-fuzz"`, `[dependencies.cay]`, `pub mod fuzz` facade refs.
- **Gate:** `make check` green; binary builds as `cay`.

### Phase 2 — Paths, persistence & migration shim (highest data-risk)
- `src/paths.rs` / `src/state/mod.rs`: state/config/cache dir name `spyc` → `cay`.
- `src/ui/syntax.rs`: syntax cache subdir.
- `src/context.rs`: `.cay-context-<pid>.json`, `CONTEXT_ENV_VAR = "CAY_CONTEXT"`.
- `src/mcp/mod.rs`: socket dir + `mcp-<pid>.sock`/`.root` naming, `mcp.log`.
- `src/debug_log.rs` / `src/key_trace.rs`: log filename patterns + env reads.
- **Migration shim (D2):** one-time copy of `~/.local/state/spyc/` → `cay/` with
  health-check + flash.
- **Config dual-read (D3)** in `src/config/`; rename `default.spycrc.toml` →
  `default.cayrc.toml` (and `--print-config`).
- **Gate:** start `cay` with an existing `~/.local/state/spyc/`; confirm
  sessions/graveyard/harpoon all survive; confirm `.cayrc.toml` and `.spycrc.toml`
  both load.

### Phase 3 — Env, MCP bridge & agent config (atomic cross-process layer)
- `src/mcp/mod.rs`: `SERVER_NAME = "cay"`, `CONTEXT_URI = "cay://context"`,
  `SERVER_INSTRUCTIONS` reworded ("cay's tools", new tool name).
- `src/mcp/config.rs`: server key `spyc`→`cay` in `.mcp.json` / `.codex` writers,
  env key `SPYC_MCP_SOCK`→`CAY_MCP_SOCK`, binary fallback `PathBuf::from("cay")`,
  stale-`spyc`-entry cleanup (D6), enterprise policy name match.
- `src/mcp/protocol.rs`: advertise `get_cay_context`, dispatch alias (D5).
- `src/app/pane_tabs.rs` / `src/pane/mod.rs`: inject `CAY_*` env into panes.
- Env compat readers everywhere (D4).
- Takeover prompt text → `🌶️ cay: PID N already owns MCP here…`.
- **Gate:** launch an agent pane; confirm the socket bridge connects,
  `get_cay_context` (and the old alias) returns context, `report_status` works.

### Phase 4 — Tests, snapshots & the trap sigil
- `SPYC-TRAP` → `CAY-TRAP` (D7): guard string-assembly + error literals + 3 code
  anchors + 2 ARCHITECTURE.md markers + template.
- Update test assertions hard-coding `"spyc"` (MCP server-name tests, flash-text
  test, config-cleanup tests, `serverInfo.name`).
- `cargo insta accept` — ~23 `.snap` files, ~12 with real content changes
  (status bar `🌶️`, the one `spyc` literal in a status snapshot); **spot-check the
  12**. Inline snapshots (~22) likewise.
- Test fixtures/helpers building `spyc` paths/env (`test_harness.rs`,
  `src/mcp/tests/`, config tests).
- **Gate:** `make check` green including the trap guard.

### Phase 5 — Docs & user-facing strings
- **Homage-line rewrite (the legal fix)** — see §6.
- Live docs (rewrite throughout): `README.md`, `AGENTS.md`, `CLAUDE.md`,
  `ARCHITECTURE.md`, `DESIGN.md`, `FEATURES.md`, `INSTALL.md`, `ROADMAP.md`,
  `CONTRIBUTING.md`, `BACKLOG_DRAFT_NOTES.md`, and live `docs/*.md`.
- **Leave verbatim:** `docs/archive/**`, and `CHANGELOG.md` ≤ v1.56.0 frozen
  history + all entries that shipped as `spyc` (they're the record). Add a
  `2.0.0` "Rebranded to Cayenne" entry; git-cliff handles future entries.
- Runtime strings: `src/ui/help.rs` (self-referential text), status/title
  fallbacks (`src/ui/status.rs`, `src/term_title.rs` `"spyc"` defaults → `"cay"`),
  flash messages. **Keep `🌶️`.**
- Logo: rename `docs/spyc-logo.{png,svg}` → `docs/cay-logo.{png,svg}` (or
  `cayenne-logo`); update README `<img>` + `docs/presentation.html`.
- Install snippets: `git clone <new GitHub URL>`, `make install` → `cay`,
  `.mcp.json` example with the `cay` key.

### Phase 6 — Public repo (clean slate) & distribution (infra)
The public `Cayenne` repo is seeded fresh — single initial commit, no history
(D10). Curate the manifest:

- **Carries (rewritten for `cay`):** `src/`, `build.rs`, `Cargo.toml` /
  `Cargo.lock`, `Makefile`, `fuzz/`, `rust-toolchain.toml`, `deny.toml`,
  `cliff.toml`, `.gitignore`; user/contributor docs `README.md`, `BRAND.md`,
  `FEATURES.md`, `INSTALL.md`, `ARCHITECTURE.md`, `DESIGN.md`, `AGENTS.md`,
  `CONTRIBUTING.md`, `RELEASE_ENGINEERING.md`; community-health files `SECURITY.md`
  (already exists — update), `CODE_OF_CONDUCT.md` (add), `.github/` templates +
  `CODEOWNERS`; a fresh `LICENSE` (D9); a fresh `CHANGELOG.md` starting at the
  public launch; GitHub Actions CI under `.github/workflows/`.
- **Release process, pipelines, signing, and the `Tripstack-Corp` org/repo setup
  are specified in `RELEASE_ENGINEERING.md`** (FreeBSD-style streams; carries
  public). Phase 6 here is the *seeding*; that doc is the *operating manual*.
- **Shed (internal-only — stay on private Bitbucket):** all git history; every
  `docs/*_PLAN.md` (including this REBRAND_PLAN) and `docs/archive/**`; the
  competitive / code-review docs (`docs/*REVIEW*.md`); `BACKLOG_DRAFT_NOTES.md`
  (owner's private backlog); `docs/presentation.html` (unless wanted); any other
  internal artifact.
- CI: author GitHub Actions for the quality gate (`make check`) + coverage floor
  (`cargo llvm-cov --fail-under-lines 35`); the public repo has no
  `bitbucket-pipelines.yml`. PR tooling → `gh` (D8).
- Release artifacts: rename release binary/targets, cross-compile matrix, any
  `.sha256` naming, Homebrew tap/formula.
- Old Bitbucket repo: README notice pointing at the public repo; keep it private
  as the history-of-record (or archive).
- `.gitignore`: add `.cayrc.toml`; keep `.mcp.json` / `.codex/` ignored.

### Phase 7 — Verification & cutover
- Full `make check` + `make lint-linux` on the merged result.
- **Manual dogfood:** build `cay`, run it inside itself, launch a Claude pane,
  confirm MCP tools resolve, `gf`/`gF`, worktree tools, `report_status` dot.
- **Migration test:** copy a real `~/.local/state/spyc/` to a scratch HOME, run
  `cay`, verify state carried over and nothing was lost.
- Tag `v2.0.0`, publish, flip the docs/README to point everyone at `Cayenne`.

## 6. The homage-line rewrite (legal fix)

**Current** (`README.md`, the "Why" section name line) — the liability:

> The name: **spy** (inspired by SideFX's in-house file manager) +
> **c**laude = **spyc**.

**Replace with** (descriptive, no hybrid mark, no implied endorsement):

> **Cayenne** — a fast, fiery spice for a fast, keyboard-driven tool. `cay` for
> short on the command line. Its vi-style workflow draws on the lineage of
> classic Unix terminal file managers, rebuilt from scratch in Rust for the
> AI-agent era.

**Recommendation:** drop the explicit SideFX/`spy` derivation entirely — we don't
need it, and naming it re-introduces the exact "admission of derivation" the
research flagged. If you *want* to credit the inspiration, use a strict
nominative-fair-use form (plain text, no hybrid, plus a disclaimer that the
project is not affiliated with or endorsed by Side Effects Software Inc. or
Anthropic). Cleaner to omit. Everywhere else, "spy + claude" framing is removed.

## 7. Risk register

| Area | Severity if mishandled | Mitigation |
|---|---|---|
| Graveyard / inventory not migrated | **Catastrophic** (lost recoverable files) | D2 copy + health-check |
| Sessions not migrated | **Critical** (lost workspaces/agent IDs) | D2 |
| MCP socket bridge half-renamed | **Critical** (agent hangs silently) | Phase 3 atomic; bridge gate test |
| MCP server/tool name break | **High** (agents lose tools) | D5 alias, D6 config migration |
| Config not found | **Medium** (lost keybinds/theme) | D3 dual-read |
| Enterprise managed-mcp `spyc` name | **Medium** | Coordinate w/ admins pre-launch |
| Trap guard fails on next anchor | **Medium** | D7 rename now |
| Snapshot drift | **Low** | `cargo insta accept` + spot-check |
| Debug/trace log names | **Low** | Cosmetic rename |
| Copyright (engine/keys) | **Low** (unchanged) | Already clean-room |

## 8. Open decisions (your call)

*Resolved since first draft:* **D10** clean-slate repo — history shed; **license =
BSD-3-Clause today** (the MIT/Apache suggestion was an error), final public form
pending legal (**D9**); **SideFX credit** — omitted, lineage framed descriptively
per BRAND.md.

1. **License — public-release form (D9):** confirm with legal what Tripstack can
   publish and under which license. **Launch blocker.**
2. **Maintainer / IP (D9):** confirm Tripstack owns the work and signs off on a
   public OSS release under the company name. (Owner is updating BRAND.md's
   maintainer line.)
3. **CI / dev platform (D8):** strongly indicated to move to GitHub (Actions +
   `gh`) now that the public repo is canonical — confirm the team's dev process
   formally moves vs. publishing snapshots from Bitbucket.
4. **Version number:** continue spyc's line (next major, e.g. `2.0.0`) or reset to
   a fresh **`1.0.0`** for the new repo? Clean-slate history argues for `1.0.0`.
   *(Recommend: `1.0.0` — new repo, new start.)*
5. **crates.io:** publish `cay` / `cayenne` (reserve the name now) or stay
   source-install only?
6. **Logo filename:** `cay-logo` vs `cayenne-logo` (cosmetic).
7. **Repo owner/org** for the public `Cayenne` (personal vs Tripstack org — ties
   to D9 IP).

## 9. Appendix — concrete inventory (as-of-recon; re-grep by symbol)

Produced by a parallel recon pass over six dimensions. Line numbers are hints.

### A. Build identity & code identifiers
- `Cargo.toml`: `name`/`[[bin]]`/`repository` (L2,L11,L7); `Cargo.lock` L~3945.
- `build.rs` L7–11: `SPYC_GIT_SHA`/`SPYC_BUILD_TIME`/`SPYC_RUSTC_VERSION`;
  consumed `src/lib.rs` L37,L219–221.
- `src/lib.rs` L127 `#[command(name="spyc")]`; `src/main.rs` `spyc::run()`.
- `src/debug_log.rs` L82 `macro_rules! spyc_debug`; L40 `spyc-debug-{ts}.log`.
- `src/context.rs` L14 `struct SpycContext`, L51 `CONTEXT_ENV_VAR`, L57
  `.spyc-context-{pid}.json`.
- `src/mcp/mod.rs` L23 `SERVER_NAME`, L74 `CONTEXT_URI = "spyc://context"`,
  socket dir L~110.
- `Makefile` L1/L17 `BINARY := spyc`, install targets L257–288.
- `fuzz/Cargo.toml` L12 `name="spyc-fuzz"`, L23 `[dependencies.spyc]`.
- CI/build prose only (skip): `bitbucket-pipelines.yml`, `deny.toml` L107,
  `cliff.toml` L12, `rust-toolchain.toml` (clean).

### B. Filesystem & persistence (highest data-risk)
- State root `src/state/mod.rs` L52–60 → all stores below live under it.
- Stores: sessions `state/sessions/mod.rs`; harpoon `state/harpoon.rs` L20;
  graveyard `state/graveyard.rs` L24 (`.json`+`.tar.zst`, **+ legacy `.dat`**);
  inventory `state/inventory.rs`; frecency `frecency.rs`; pager_positions
  `pager_positions.rs`; marks `marks.rs`; history `history.rs`; health `health.rs`.
- Config `src/config/mod.rs` L4–5, `src/app/config.rs` L58,L64 (`.spycrc.toml`);
  template `src/config/default.spycrc.toml`.
- Sockets/markers (ephemeral, auto-handled) `src/mcp/mod.rs` L110,L114–127.
- Context file `src/context.rs` L55–58; worktree temp `worktree_clean.rs` L148
  (`.spyc-wt-remove-*`); bootstrap orphan sweep `src/app/bootstrap.rs` L184,
  `src/app/config.rs` L90–91.
- Logs `src/debug_log.rs` L38–41, `src/key_trace.rs` L41, `src/mcp/mod.rs` L87–92.
- Logo `docs/spyc-logo.{png,svg}`.

### C. Env vars & process interface
- Atomic socket contract: `CAY_MCP_SOCK` + `CAY_PANE_ID` injected
  `src/app/pane_tabs.rs` L127–128, read `src/mcp/mod.rs` L175,L181; written into
  `.mcp.json`/codex `src/mcp/config.rs` L217,L330; binary fallback
  `PathBuf::from("spyc")` L194,L294.
- `SPYC_CONTEXT` `src/context.rs` L51, injected `src/pane/mod.rs` L132.
- `SPYC_PANE_CMD` `pane_tabs.rs` L274; `SPYC_DEBUG` `debug_log.rs` L53;
  `SPYC_KEY_TRACE` `key_trace.rs` L32; `SPYC_MCP_DEBUG` `mcp/mod.rs` L99.
- Self re-exec uses `current_exe()` (good) — verify no hardcoded `"spyc"` on PATH.

### D. MCP server identity & agent config
- `SERVER_NAME` / `CONTEXT_URI` / `SERVER_INSTRUCTIONS` `src/mcp/mod.rs`.
- Config writers/keys (~18) `src/mcp/config.rs` (server key, env key, `/mcpServers/spyc/`).
- Enterprise policy match `src/mcp/config.rs` L124–173.
- Tool name `get_spyc_context` `src/mcp/protocol.rs` L134–141,L502–504 (only
  brand-bearing tool; ~20 others neutral).
- Takeover prompt `src/lib.rs` (`run()`).

### E. Tests & snapshots
- Guard `traps_resolve_against_architecture_anchors` `src/app/mod_tests.rs`
  L129–198; sigil assembly L132. Anchors: `src/lib.rs` L149,L158, `src/app/run.rs`
  L446. Markers: `ARCHITECTURE.md` L137,L172 (+ template L424).
- `.snap`: 23 files (`src/ui/snapshots/`, `src/app/render/snapshots/`,
  `src/ui/pager/tests/snapshots/`); ~12 with brand content.
- Assertions on `"spyc"`: `src/mcp/tests/mod.rs` L48,L737,L773,L860,L947–1006;
  flash `src/app/state/tests/apply.rs` L225; title `src/ui/status.rs`
  L346,L376,L393, `src/term_title.rs` L78.

### F. Docs & user-facing strings
- Live (rewrite): README, AGENTS, CLAUDE, ARCHITECTURE, DESIGN, FEATURES,
  INSTALL, ROADMAP, CONTRIBUTING, BACKLOG_DRAFT_NOTES, live `docs/*.md`,
  `docs/presentation.html`.
- Verbatim: `docs/archive/**`; `CHANGELOG.md` historical/shipped-as-spyc entries.
- Homage line: README "Why" section (see §6).
- Runtime: `src/ui/help.rs`, `src/mcp/mod.rs` SERVER_INSTRUCTIONS, title/status
  fallbacks, takeover prompt. **Keep 🌶️ + spice session names.**
