# spyc-testing — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: spyc-testing
Created: 2026-05-11T21:41:22.655452+00:00

---
Entry: Claude Code (caleb) 2026-05-11T21:41:22.655452+00:00
Role: tester
Type: Note
Title: First entry — local build + full gate at v1.41.24 on 2026-05-11

Spec: tester

tags: #testing #build #gate

# Frame for the thread

This thread is the running ledger of test/build/gate runs against the spyc codebase, separate from the eight history arcs (which describe what landed) and the four insight threads (which describe what the artefact is). The arcs and insights are mostly retrospective; this thread is operational — *what happened when I tried to verify a build*. Each entry stamps a date, a commit, and a result, so future readers can see whether the gate was green when a given change was reasoned about.

# This entry: full local gate at v1.41.24 on 2026-05-11

**Commit under test:** `a303251` (PR #37 `fix/mcp-socket-project-scoped-discovery`, the head of `main` at session start).
**Host:** Linux 6.8.0-110-generic, x86_64.
**Toolchain:** `rust-toolchain.toml` pins 1.85+ per `README.md:53`; system `cargo` resolved the pin.

## Sequence

1. **`make build` (debug profile).** Cold incremental build; `dev` profile completed in **32.10s** with **exit 0**. Output binary at `target/debug/spyc` (~119 MB; unstripped + debuginfo, normal for a debug build). No warnings surfaced in the tail.

2. **`make check`.** This is the canonical CI gate: `fmt-check`, `lint` (clippy with pedantic + nursery per `Makefile`), `test`, `deny`. Outcome:
   - `fmt-check` — ✓
   - `lint` — ✓
   - `test` — ✓; **577 tests** across three binaries, **0 failed / 0 ignored**:
     - unit (`target/debug/deps/spyc-…`): 566 tests, 0.16s
     - integration `tests/filesystem.rs`: 6 tests, 0.00s
     - integration `tests/keymap_roundtrip.rs`: 5 tests, 0.00s
   - `deny` — ✗; **`cargo-deny not found — install with: cargo install cargo-deny --locked`**. The Makefile's own remediation message was exact.

3. **`cargo install cargo-deny --locked`.** Exit 0 (background task, completed without surfacing warnings). Installed into `~/.cargo/bin`.

4. **`make deny` (rerun, post-install).** Exit 0. Final lines:
   ```
   advisories ok, bans ok, licenses ok, sources ok
   ```
   One warning surfaced:
   ```
   warning[advisory-not-detected]: advisory was not encountered
   ┌─ deny.toml:89:13
   │
   89 │     { id = "RUSTSEC-2024-0436", reason = "unmaintained transitive via ratatui;
   │       build-time proc-macro" },
   │       ━━━━━━━━━━━━━━━━━ no crate matched advisory criteria
   ```
   `RUSTSEC-2024-0436` no longer matches anything in the resolved dependency tree. `insight-trajectory` Doc #8 catalogued 5-of-5 advisory ignores surviving PR #31's trio bump (vt100 0.15→0.16, ratatui 0.29→0.30, plus a third); since then the upstream chain has shifted enough that this specific ignore is now stale. Not a failure — `cargo-deny` exits 0 — but the ignore can be deleted from `deny.toml` whenever convenient.

## Net result

Green gate. The codebase passes `make check` on Linux x86_64 with no source modifications, given a working `cargo-deny` install. One pre-existing minor cleanup observable: stale advisory ignore at `deny.toml:89`.

## Side observations

- **`README.md:14`** says `v1.21.1`. `Cargo.toml` reports `1.41.24`. Almost certainly a typo (`2` for `4`); the binary's `gV`/`:version` output is authoritative. Worth a one-character fix on a future docs PR. Consistent in shape with `insight-drift` Pattern A (description-layer understatement; functional surface unaffected).
- The build did **not** require any of the optional extras flagged in `INSTALL.md`/`README.md` Quick Start (Nerd Font, Claude Code npm install). Those affect runtime experience, not the gate.
- The `make check` target name resolves to `fmt-check lint test deny` in `Makefile` — same four-step composition `insight-trajectory` Doc #4 read as part of the charter's quality posture.

Provenance:
- Commit under test: `a303251` (PR #37 `fix/mcp-socket-project-scoped-discovery`, 2026-05-07).
- `Makefile:58` — `deny:` target (the line that emitted the install hint).
- `deny.toml:89` — stale advisory-not-detected ignore for `RUSTSEC-2024-0436`.
- `README.md:14` — version typo (`v1.21.1` vs `Cargo.toml = 1.41.24`).
- `insight-trajectory` Doc #8 (advisory ignores trajectory; 5-of-5 at PR #31).
- `insight-drift` Pattern A (description-layer understatement; the `v1.21.1` typo fits the pattern).
- `insight-drift` Pattern D (single-functional-drift baseline that frames why a green gate is the expected outcome).
- Identity fallback: no `set_agent` tool surfaced this session; identity asserted via Role + Spec lines and `agent_func`.

<!-- Entry-ID: 01KRCFSJ6ZQAWN1XVJ7F3V6G9H -->
