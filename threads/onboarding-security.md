# onboarding-security — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: onboarding-security
Created: 2026-05-07T07:49:40.185895+00:00

---
Entry: Claude Code (caleb) 2026-05-07T07:49:40.185895+00:00
Role: critic
Type: Note
Title: Onboarding: security posture and supply-chain controls

Spec: security-audit

Purpose: Summarize spyc's actual security posture, supply-chain controls, and the parts of the threat model a new contributor must keep in mind. The maintainer's own framing in `SECURITY.md` is the source of truth — this entry indexes and cross-links it, doesn't paraphrase the policy.

Observed:
- **Threat model (`SECURITY.md:9-31`).** spyc is a single-binary terminal file manager that runs locally as the invoking user, has no network code of its own, and is distributed internally to Tripstack engineers. The maintainer enumerates exactly three realistic threats:
  1. Supply-chain compromise of a Rust dependency.
  2. Tampered local build (someone modifies the source on a shared clone before `make install`).
  3. MCP socket misuse — the per-PID Unix socket exposes tool calls to whatever process can read `~/.local/state/spyc/mcp-<pid>.sock`. FS perms gate it; an attacker who's already running as your user can talk to any of your spyc instances.
- **Out of scope by maintainer choice (`SECURITY.md:26-31`).** No remote attack surface, no privilege boundary inside the binary, no secrets handling, no untrusted-input parser beyond the TOML config files spyc itself controls.
- **Supply-chain controls (`SECURITY.md:33-57`):**
  - `Cargo.lock` committed; deterministic builds.
  - `--locked` flag everywhere (Makefile, CI, dev). `Makefile:42,46` test/lint; `bitbucket-pipelines.yml` matches.
  - `cargo-deny check` runs on every CI build (advisories + licenses + sources + bans). Replaces the older `cargo-audit` step. (Note: `TODO.md:99-104` still reads "cargo-audit" — flagged in `onboarding-risk-register` as docs drift.)
  - License allow-list in `deny.toml:104-124` reflects "the licenses present in our actual dep graph as of v1.37.1." Adding a dep with a license outside that set fails CI.
  - Source allow-list in `deny.toml:258`: only `crates.io` is accepted; no `git = "..."` deps, no patched forks.
  - MSRV pinned via `rust-toolchain.toml` (`channel = "stable"`) with the canonical 1.85 image enforced in CI (`bitbucket-pipelines.yml:8`).
  - Optional pre-commit hook (`make install-hooks`) runs the same gate locally.
- **Documented advisory ignores (`deny.toml:72-94`).** Five long-lived transitive issues, each with a `reason`:
  - `RUSTSEC-2026-0009` (time 0.3.45): transitive via syntect→plist→time; not exploitable; fix needs Rust 1.88, MSRV-blocked at 1.85.
  - `RUSTSEC-2024-0320` (yaml-rust 0.4.5): unmaintained transitive via syntect; build-time only.
  - `RUSTSEC-2025-0141` (bincode 1.3.3): unmaintained per maintainer's choice; transitive via syntect.
  - `RUSTSEC-2024-0436` (paste 1.0.15): unmaintained; transitive via ratatui; build-time proc-macro.
  - `RUSTSEC-2017-0008` (serial 0.4.0): unmaintained since 2017; transitive via portable-pty; no alternative without forking.
  Pattern: every ignore documents the route through the dep graph and the reason it's tolerable.
- **Build / install posture (`SECURITY.md:59-87`).** No prebuilt binaries; install is `make install` from a local SSH-cloned Bitbucket repo. macOS gets ad-hoc `codesign -s -` (entitlements only, not Developer ID). Distribution scaffolding (`make dist-checksums`, `make dist-sign`) is pre-staged for the future public release flow but not yet wired into CI.
- **Known caveats (`SECURITY.md:89-111`):**
  - No reproducible builds (timestamps, paths, rustc fingerprints differ build-to-build).
  - No SBOM published (Cargo.lock + cargo-deny give the audit trail; emitting CycloneDX/SPDX is one-shot scriptable).
  - No commit signing requirement (Bitbucket doesn't enforce `enforced_signed_commits`).
  - MCP socket permissions are filesystem-default (relying on user-process isolation, not stricter ACLs).
  - No fuzzing — explicitly judged out of scope today.
- **Recently-strengthened MCP socket invariant (v1.41.24, `CHANGELOG.md:9-31`).** Cross-project MCP attachment was a real bug: a claude in project A could silently attach to a spyc running in project B (or even another user's spyc when `$HOME` was unset on a shared host). The fix made socket discovery project-scoped via the `.spyc-context-<pid>.json` ancestor walk. With no project match, the stdio proxy falls back to read-only direct mode rather than attaching to a stranger's spyc. Stale-socket cleanup was tightened to delete only on `ConnectionRefused` / `NotFound` (not every connect error) so transient `EAGAIN`/`EMFILE` doesn't race-delete a healthy peer's socket. Preserve this invariant when touching `src/mcp.rs`.
- **Reporting channel.** `derek.marshall@tripstack.com`, "Internal contact, no formal SLA; expect a same-day response during business hours" (`SECURITY.md:115-116`). For dependency issues: also report upstream and update `deny.toml` as needed (`SECURITY.md:118-120`).
- **Signing future-state (`SECURITY.md:81-87`, `Makefile:140-146`).** GPG-signed checksums are pre-staged via `make dist-sign`; the maintainer has committed to publishing the signing key fingerprint in `SECURITY.md` once public binaries ship. Today: not load-bearing; tomorrow: a hard contract.

Inferred:
- The threat model is unusually disciplined — the maintainer wrote *what is and isn't in scope*, not just controls. — confidence: high — basis: `SECURITY.md:9-31` explicitly enumerates and out-of-scopes; `SECURITY.md:122-136` lists the exact change-trigger conditions ("When to revisit this document"). How to apply: any PR that adds a network surface, a privileged operation, or an external input parser must update `SECURITY.md` in the same commit — the rule is named explicitly.
- "MCP socket misuse" is the threat that's *most likely to grow* as the MCP tool surface expands. — confidence: high — basis: `SECURITY.md:21-25` flags the FS-perms-only gate; `SECURITY.md:104-107` repeats it as a known caveat; v1.41.24 already had to harden cross-project discovery (`CHANGELOG.md:9-31`); roadmap entries like "MCP peer credential checking" via `SO_PEERCRED`/`LOCAL_PEERPID` (`ROADMAP.md:312-318`) are pending. How to apply: any new MCP tool that *mutates* state outside spyc's own process needs `SECURITY.md` updated in the same commit, per `SECURITY.md:131-136`.

Next query: `watercooler_search(query="security threat-model supply-chain mcp socket", thread_topic="onboarding-security", code_path=".")`

Related:
- `onboarding-overview` — front door.
- `onboarding-architecture` — the MCP-server-on-Unix-socket section of architecture is the surface this threat model bounds.
- `onboarding-risk-register` — sibling that flags `cargo-deny` ignores and the v1.41.24 socket-discovery hardening as load-bearing seams.
- `onboarding-release-process` — the signing posture flips from "theater-avoided" to "load-bearing" the moment public artifacts ship.

Provenance:
- Files read: `SECURITY.md:1-137` (full), `deny.toml:72-94,104-124,258`, `Makefile:42,46,140-146,170-176`, `bitbucket-pipelines.yml:1-77`, `CHANGELOG.md:1-100` (Unreleased section), `ROADMAP.md:312-318` (MCP peer credential roadmap), `src/main.rs:1-403` (signal handlers + panic hook + setup_terminal — the "no privilege boundary inside the binary" claim is verifiable here).
- Sibling entry_ids: `onboarding-overview = 01KR0NZNJ3KM6BJY09Q4P9D0NE`, `onboarding-architecture = 01KR0P4W3ED1QZ8F44PFB2WPDZ`, `onboarding-risk-register = 01KR0P9JC8Z3DF6FQ1GJPF3VKA`, `onboarding-release-process = 01KR0PHNA4XW7CWPQ2D93K24HC`.

<!-- Entry-ID: 01KR0PKS884SXRAKZ8A790Q438 -->

---
Entry: Claude Code (caleb) 2026-06-09T05:20:11.246259+00:00
Role: critic
Type: Note
Title: Onboarding refresh: security posture from #37 to #311 (v1.56.0)

Spec: security-audit

Purpose: Refresh of the security posture from #37 (v1.41.24) to #311 (v1.56.0). The #37-era entry (`01KR0PKS884SXRAKZ8A790Q438`) stays as a snapshot; this entry re-verifies the unsafe count, re-assesses the attack surface after the gix migration, and re-checks SECURITY.md against the shipped tree.

Observed (current posture):
- **Unsafe reduction — verified, and now LOWER than the arc-08 thread records.** `history-arc-08` PR #83 (`01KTMMRWF583`, refactor/unsafe-reduction) took unsafe sites 36→2 via DI + rustix, with a signal-hook revert that "fixed the count at 2." PR #154 (`01KTMN1PMZRV`, fix/remove-unsafe-setenv) then removed the `:s` `unsafe { set_var }` site for a thread-safe `crate::envset` store — "removes one of the two residual unsafe sites." Current verification (`grep -rnE '\bunsafe\b' src --include='*.rs'`, filtering comments/docstrings): exactly **1** live `unsafe {}` block in production code, at `src/main.rs:314` — the startup signal-handler install (`libc::signal` for SIGINT/SIGQUIT/SIG_IGN-on-SIGTTOU; `src/main.rs:309-323`), which the function doc explains is not safely exposable through rustix/signal-hook for the SIGTTOU-inherit need. The other 4 grep hits are doc comments (`src/envset.rs:3`, `src/state/mod.rs:50`, `src/state/harpoon.rs:211`, `src/main.rs:310`). So the seed's "36→2" arc is now effectively **36→1** in shipped code — a staleness correction worth recording.
- **gix migration and attack surface (in-process vs subprocess git).** `gix = 0.84` (`history-seg-gix-migration`, #283–#292) moved status/diff/blame/worktree in-process — removing fork+exec of `git` from those hot paths reduces PATH-hijack / argument-injection surface there. BUT 8 `Command::new("git")` sites remain (`grep -rn 'Command::new("git")' src`): production at `src/git/blame.rs:113`, `src/git/status.rs:484`+`:506`, `src/git/worktree.rs:370`, `src/git/discovery.rs:52`, `src/git/diff_model/mod.rs:219`; test-only at `src/app/state/tests/mod.rs:302`+`:373`. Each production site still fork+execs a `git` binary resolved off PATH — the subprocess-git attack surface is REDUCED, not eliminated. Anyone hardening this should target the 6 production sites (esp. worktree create/remove, which the gix thread explicitly left subprocess-hand-rolled).
- **SECURITY.md vs current major (also cross-check #2).** `SECURITY.md` (137 lines, read in full) is a posture document — threat model (`:9-31`), supply-chain controls (`:32-57`), build/install (`:58-87`), known caveats (`:89-111`), revisit triggers (`:122-136`). It has **NO supported-versions matrix**, so the "supported claim vs current major 1.x" check is **[n/a — no supported-versions block]** (unchanged disposition from #37). Its substantive claims still hold against the shipped tree: "no network code of its own," "no secrets handling," "no untrusted-input parser beyond TOML config we control" (`SECURITY.md:27-30`). The gix migration did NOT add a network surface, so the threat model is still accurate. The MCP-socket-misuse threat (`SECURITY.md:21-25`) is still the most-likely-to-grow surface.
- **External-input surfaces.**
  - **vt100 parsing of pane output:** child-process bytes are parsed by the per-pane `vt100` emulator (`src/ui/scrollback.rs` adapts `vt100::Screen`; vt100 now at 0.16, `scrollback.rs:14`). This is the one untrusted-input-ish path (a child app's escape sequences). It is defended by `catch_unwind` + `panic = "unwind"` (`Cargo.toml:97-107`) so a vt100 unwrap-panic recovers instead of killing spyc — `history-arc-08` PR #30 vt100-panic-recovery (`01KR393P15VT`). SECURITY.md frames this as in-scope-but-bounded (child runs as the same user already).
  - **MCP socket surface:** PID-scoped `~/.local/state/spyc/mcp-<pid>.sock` (`src/mcp/mod.rs:50-54`), filesystem-permission-gated only (`SECURITY.md:104-107` known caveat). Stale-socket cleanup is gated to no-peer errors (ConnectionRefused/NotFound) so transient EAGAIN/EMFILE can't race-delete a healthy peer (`src/mcp/server.rs:58-62`). The 10-tool surface (`src/mcp/protocol.rs:130-285`) includes mutating tools (`navigate_to`, `set_filter`, `pick_files`, `clear_picks`) but they mutate only spyc's OWN TUI state, never the filesystem outside it — `SECURITY.md:131-136` names the revisit trigger if that ever changes. Peer-credential checks (SO_PEERCRED/LOCAL_PEERPID) remain roadmap, not shipped.
  - **TOML config:** `.spycrc.toml` / keymap DSL parsed from user-controlled files (`src/config/`); SECURITY.md explicitly scopes this as trusted-input.
- **Secrets handling:** none. `SECURITY.md:28-29` states "no secrets handling"; spyc stores XDG state (inventory/marks/history/sessions) under `~/.local/state/spyc/`, no credentials, no tokens. The MCP socket carries TUI context (cwd/cursor/picks/filter/git-branch), not secrets.
- **Supply-chain controls unchanged and current:** `Cargo.lock` committed, `--locked` everywhere, `cargo deny check` on every CI build, license + source allow-lists in `deny.toml`, MSRV pinned via `rust-toolchain.toml` (now 1.88). `SECURITY.md:32-57`.

Drift findings (security required-section optional; relevant items recorded):
- **Unsafe count is stale in the arc record: thread says "fixed at 2," shipped tree is 1** (PR #154 removed the setenv site). Verified `src/main.rs:314` is the sole remaining production `unsafe` block.
- **MSRV string drift** (also flagged in risk-register/docs-contracts): `README.md:73` "Rust 1.85+" vs `Cargo.toml` rust-version 1.88. Security-relevant because the time/RUSTSEC-2026-0009 advisory-ignore in `deny.toml` was MSRV-gated — verify the deny.toml ignore reasons still cite the right MSRV after the 1.85→1.88 bump.

Inferred:
- The single most-likely-to-grow surface is still MCP socket misuse, now with a 10-tool surface (vs the ~6 at #37). — confidence: high — basis: `SECURITY.md:21-25,104-107`; tool count grew but all tools remain spyc-internal-mutation-only. How to apply: any new MCP tool that mutates state OUTSIDE spyc's process must update SECURITY.md in the same commit per `:131-136`.
- The gix migration is a net attack-surface reduction but the worktree subprocess path is the residual exposure worth closing. — confidence: medium — basis: 6 production `Command::new("git")` sites remain, worktree being the explicitly-deferred one. How to apply: a future PATH-hardening pass should target `src/git/worktree.rs:370` and `src/git/discovery.rs:52` first.

Next query: `watercooler_search(query="security unsafe gix subprocess mcp socket vt100", thread_topic="onboarding-security", code_path=".")`

Related:
- `onboarding-risk-register` — sibling re-scoring the 8 subprocess-git sites and `app/mod.rs` as load-bearing seams.
- `onboarding-docs-contracts` — SECURITY.md currency / the doc-sync rule.
- `history-arc-08-recoverability-and-deps` — unsafe-reduction (#83), remove-unsafe-setenv (#154), vt100-panic-recovery (#30).
- `history-seg-gix-migration` — the in-process git campaign and its scoped boundary.

Provenance:
- Commands: `grep -rnE '\bunsafe\b' src --include='*.rs'` (1 live block at main.rs:314, 4 comment hits); `grep -rn 'Command::new("git")' src` (8 sites listed inline); `grep -n 'panic' Cargo.toml` (`:97-107` unwind); `grep -E 'gix|rust-version' Cargo.toml` (gix 0.84, rust-version 1.88).
- Files: `SECURITY.md:1-137` (full, esp. `:9-31,32-57,89-111,122-136`), `src/main.rs:309-323`, `src/mcp/mod.rs:50-54`, `src/mcp/server.rs:58-62`, `src/mcp/protocol.rs:130-285`, `src/ui/scrollback.rs:14`, `Cargo.toml:97-107`, `README.md:73`, `deny.toml` (advisory-ignore MSRV reasons).
- History/insight entry_ids: `history-arc-08` `01KTMMRWF583`(#83 unsafe 36→2), `01KTMN1PMZRV`(#154 remove-unsafe-setenv), `01KR393P15VT`(#30 vt100-panic-recovery); #37-era snapshot `01KR0PKS884SXRAKZ8A790Q438`.

<!-- Entry-ID: 01KTND5SE2H5MA681GD9DC1P98 -->
