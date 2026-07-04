# Security

This document describes spyc's actual security posture — what we do,
what we don't, and why. It exists so reviewers and future maintainers
can see the threat model without inferring it from CI config.

If you find something wrong, contact derek.marshall@tripstack.com.

## Threat model

spyc is a single-binary terminal file manager. It runs locally as the
invoking user, has no network code of its own, and is distributed as
source from a public GitHub repo (engineers build it locally via
`make install`). The realistic threats are:

- **Supply-chain compromise of a Rust dependency** — a transitive
  crate is yanked + republished with malicious code, or an unmaintained
  dep develops a CVE.
- **Tampered local build** — someone modifies the source on a shared
  clone before `make install` runs.
- **MCP socket misuse** — the per-PID Unix socket exposes tool calls
  (`navigate_to`, `pick_files`, `get_file_content`) to whatever process
  reads `~/.local/state/spyc/mcp-<pid>.sock`. Filesystem permissions
  gate this; an attacker who's already running as your user can talk
  to any of your spyc instances.

We're not in scope for the kinds of threats that need professional
hardening: there's no remote attack surface, no privilege boundary
inside the binary, no secrets handling, and no untrusted-input parser
beyond TOML config files we already control.

## Supply-chain controls (what we do)

- **`Cargo.lock` is committed.** Every build resolves the same set
  of versions. `cargo build` will not silently bump deps.
- **`--locked` everywhere.** `make test` / `make lint` / all release
  builds pass `--locked` to cargo, so a CI-time `Cargo.lock` drift
  fails loudly. The Makefile and the GitHub Actions CI
  (`.github/workflows/ci.yml`) both enforce this.
- **`cargo deny check`** runs on every CI build (advisories,
  licenses, sources, bans). Replaces the older `cargo audit` step.
  Configuration is checked in at `deny.toml` with documented reasons
  for every advisory ignore — none are silent.
- **License allow-list.** Only the licenses present in our actual
  dep graph as of v1.37.1 are allowed. Adding a dep with a license
  outside that set fails CI; you read it, decide, and either add to
  the allow-list (with a reason) or pick a different dep.
- **Source allow-list.** Only crates from
  `https://github.com/rust-lang/crates.io-index` are accepted.
  No `git = "..."` deps, no patched forks.
- **MSRV pinned** via `rust-toolchain.toml`. A new stable release
  cannot tighten lints, change behavior, or drop features behind
  our back.
- **Pre-commit hook** (optional, install via `make install-hooks`)
  runs the same gate as CI before each commit so drift surfaces in
  seconds locally instead of ~10 min later in pipelines.

## Build and install

There is no prebuilt binary distribution. Engineers install spyc by
cloning the repo and running `make install` (default prefix
`~/.local/bin`). The chain of trust is: SSH-authenticated `git clone`
from GitHub → local Rust toolchain → local install.

This means:

- The integrity of an installation depends on the integrity of the
  local clone. If you clone from a corp-managed machine over an
  authenticated channel, the source is trusted.
- We don't sign release artifacts (no GPG, no Apple Developer ID
  cert) because there are no release artifacts to sign — nobody
  downloads a prebuilt `spyc` binary today. Adding signatures would
  be theater.
- `make install` invokes `codesign -s -` on macOS. This is **ad-hoc
  signing**, not Developer ID signing. It's enough for the binary
  to keep entitlements across rebuilds (and to silence some
  Gatekeeper-on-translocation noise), but it does **not** prove
  the binary came from a specific person and would not survive
  notarization. A real Developer ID requires Apple Developer
  enrollment and is out of scope.

If/when we start publishing prebuilt binaries (a release page, S3,
Homebrew tap, etc.), `Makefile` already has scaffolding for the
right thing: `make dist-checksums` writes SHA-256s, and
`make dist-sign` will produce a detached GPG signature on the
checksums file. The signing key fingerprint will be published here
when that happens.

## Known caveats (what we don't do)

- **No reproducible builds.** Two builds of the same source on two
  machines may differ in timestamps, paths, and rustc-version
  fingerprints. Bit-for-bit reproducibility is non-trivial for Rust
  binaries and we don't claim it.
- **No SBOM published.** `cargo deny check` and `Cargo.lock` together
  give us a full audit trail, but we don't emit a CycloneDX or SPDX
  SBOM artifact. If a consumer needs one, generating it from
  `Cargo.lock` is a one-shot script away.
- **No commit signing requirement.** The repo does not require signed
  commits (no GitHub branch-protection rule enforcing them). A compromised
  dev account could push unsigned commits indistinguishable from real ones,
  bounded by branch protection (PR-only merge into `main`, required
  status checks, restricted write access).
- **MCP socket permissions are filesystem-default.** Anyone running
  as your user on the same machine can read the per-PID socket and
  exercise the MCP tool surface. We rely on user-process isolation,
  not stricter ACLs.
- **No fuzzing.** The TOML and DSL parsers handle config we control;
  there's no untrusted-input parsing path in production code worth
  fuzzing today. If that changes (e.g., a remote-source feature),
  it should be revisited.

## Reporting a vulnerability

Email derek.marshall@tripstack.com. Internal contact, no formal SLA;
expect a same-day response during business hours.

If the issue is in a dependency we use rather than spyc itself,
please **also** report it upstream — we'll coordinate on a fix and
update `deny.toml` as needed.

## When to revisit this document

Update this document when any of the following change:

- A new dependency is added with a license outside `deny.toml`'s
  allow-list (and the allow-list is widened).
- A new advisory is ignored in `deny.toml` (the reason should also
  be expanded here under "known caveats" if it's load-bearing).
- spyc gains a network attack surface (HTTP client, RPC server,
  remote config source).
- spyc starts publishing prebuilt binaries (signing posture flips
  from "theater-avoided" to "actually meaningful").
- The MCP socket gains tools that mutate state outside spyc's own
  process (today: spyc-internal navigation; future: anything that
  writes outside `~/.local/state/spyc/`).
