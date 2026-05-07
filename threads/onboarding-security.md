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
