# Unit F. Tests, CI and the release

- **SHA:** `444857a2a32b2f284d2b4fd19e57aef19700a5c5` (`main`, `2.5.0-CURRENT`), unmoved.
- **Platforms used:** macOS 26 (Darwin 25.6.0) on Apple silicon for every probe, the version-3.0.0 runs and the mutation runs; Linux only through the orchestrator's gate run (Podman, Fedora 43 kernel 6.19.7, **aarch64, musl**, not CI's x86_64 glibc). I ran no Linux probe of my own.
- **Worktree:** `~/src/spyc.worktrees/review/3.0-f` (branch `review/3.0-f`), left clean: every probe edit was reverted after its run. Build directory `~/.cache/spyc/target/3.0-f-6ad65ede`, a clone of the reference build. Scratch: `docs/drafts/review-3.0/evidence/F`.

## Verdict

At the pin, the gate tests what ships on Linux only. It does not test what ships on macOS: no jar test, or any other test, runs on a Mac in CI, and the owner's own `make check-ci` is the only macOS test run. Lead V1 is confirmed and holds the tag. `pane::jar::honoured()` gates jars on `-CURRENT` or `-rc.` in the version. Built as `3.0.0`, spyc starts no jar: 13 of 14 `jar_start` harness tests and 18 of 19 `jar_adopt` tests fail, and `^d` refuses with "can't detach: the tab sh runs outside a jar". No release step checks for this. The release PR's own `ci.yml` test job would go red, so a tag cannot be cut green with the gate in place. But every rc soaks the gated-on build, and the final tag's build is one no rc ran. Packaging carries `spyc-jar` in all three tarballs, the `.deb`s and the Homebrew formula, and `cargo install spyc` installs it. The crates.io channel has a defect of its own: rc tags publish `spyc-jar` `0.1.0`, which is not a prerelease version, and nothing makes the jar crate's version move when its code does. So `cargo install spyc` at 3.0.0 can build the jar from rc.1's source while the tarballs build it from the tag's. Of the plan's "CI does X" promises, the jar built to differ and the jar fuzz targets are in CI (Linux only; fuzz weekly). "Both CI platforms" and "every released jar kept as a fixture" are not: `tests/jar_compat.rs` is ignored, nothing sets `SPYC_JAR_OLD_BIN`, and the spike branch its doc points to no longer exists. "No timing in tests" does not hold as written. Of 143 sleep or timeout sites in the jar tests, most poll. About 20 are fixed waits that assume a state, or that assert something did not happen within a fixed time. The ratchets added no pin since `v2.4.0` and lowered two.

## Coverage ledger

| File | Status |
|---|---|
| `.github/workflows/ci.yml` | read in full |
| `.github/workflows/macos.yml` | read in full |
| `.github/workflows/release.yml` | read in full |
| `.github/workflows/fuzz.yml` | read in full |
| `.github/workflows/apt.yml` | read in full |
| `.github/workflows/homebrew.yml` | read in full |
| `.github/workflows/{audit,codeql,shipped-targets,vendored-archives,cache-cleanup}.yml` | not read (no jar or test-gate content expected; `vendored-archives.yml` is called by `release.yml`) |
| `.github/workflows/secrets.yml` | not read: the Read tool refused it under this session's permission settings |
| `Makefile` | read in full |
| `docs/RELEASE_ENGINEERING.md` | read in full |
| `build.rs` | read in full |
| `Cargo.toml`, `crates/spyc-jar/Cargo.toml`, `crates/spyc-vt-sys/Cargo.toml` | read in full |
| `src/size_guard.rs`, `src/fn_size_guard.rs`, `src/guard_support.rs` | read in full |
| `src/style_guard.rs`, `src/link_guard.rs` | skimmed (module docs, constants, test list) |
| `src/bin/spyc-jar.rs`, `scripts/test_release_ships_spyc_jar.py` | read in full |
| `src/app/harness_tests/jar_start.rs`, `tests/jar_compat.rs`, `src/pane/jar/test_dir.rs` | read in full |
| `tests/jar_process.rs`, `tests/jar_process/*.rs`, `tests/jar_adopt.rs`, `tests/jar_attach.rs`, `tests/jar_differ.rs`, `tests/session_jar.rs`, `src/pane/jar/tests.rs`, `crates/spyc-jar/tests/*.rs`, the `#[cfg(test)]` modules of `crates/spyc-jar/src/{cli,client,handover,store,proto,jar/accept}.rs` | skimmed: every `sleep` and timeout site read with its context (search below); exercised whole by the version-3.0.0 runs and the mutation runs; test bodies not read end to end |
| Production code read to place mutations and V1: `src/pane/jar/mod.rs` (lines 95 to 600), `crates/spyc-jar/src/jar/{conn,accept,life}.rs` (the parts named in the findings), `crates/spyc-jar/src/upgrade.rs:730-746`, `src/pane/jar/{link,snapshots}.rs` (the gate and the store), `src/app/{bootstrap,capture_starts,pane_starts,orphan_jars,detach}.rs` (the call sites) | read in part, as cited |

## CI promises, line by line

| Promise (where) | In CI? | Workflow line |
|---|---|---|
| Each jar test "runs on both CI platforms" (`JAR_PRODUCTION_PLAN.md:430`) | **No.** Linux only | `ci.yml:95-104` (`runs-on: ubuntu-latest`, `make test`). `macos.yml:55-94` runs `make fmt-check lint` and a link of `--bin spyc`, never a test, and only on PRs touching `macos.yml:28-41`'s paths, on `v*` tags, or by hand. See F3. |
| "A jar built to differ" upgraded both ways (`JAR_PRODUCTION_PLAN.md` S3, `JAR_PROTOCOL.md:771-779`) | Yes, Linux only | `ci.yml:104` → `Makefile:126-128` → `Makefile:133-136` (`--features spyc-jar/differ`, `--include-ignored`). Both gate logs show `tests/jar_differ.rs` 3 passed. |
| "Every released jar stays a test fixture. CI keeps the jar of each release" (`JAR_PRODUCTION_PLAN.md:164`, `JAR_PROTOCOL.md:1052-1053`) | **No** | Nothing sets `SPYC_JAR_OLD_BIN` (search: `rg -n SPYC_JAR_OLD_BIN --hidden` finds only `tests/jar_compat.rs` and the brief). See F4. |
| Jar fuzz targets "run in CI alongside spyc's existing ones" (`JAR_PROTOCOL.md:546-547`, plan `:35`) | Yes, scheduled, Linux, not a gate | `fuzz.yml:21-24` (Mondays 07:00 UTC, and by hand), `fuzz.yml:64-65` (`jar_frame`, `jar_handover`), 300 s per target (`fuzz.yml:108`). True as worded; it gates nothing. |
| "`make package-check` builds both from their tarballs" (plan S1) | Yes | `ci.yml:108-121` → `Makefile:237-239`. Passed locally (below). |
| The release ships `spyc-jar` in archives, the formula and apt (plan S2 "The release gate") | Yes | `release.yml:63-65`, `:106-111`; `homebrew.yml:109-110,122`; `Makefile:557-561`, `apt.yml:168-177`; checked by `scripts/test_release_ships_spyc_jar.py` (run by `make test-scripts`, `ci.yml:105-106`). |
| Release-time tests | **None** | `release.yml` has no test step on either platform. It relies on `main`'s CI, which branch protection makes required (`gh api …/protection/required_status_checks`: `strict: true`, contexts `Tests (cargo test --all-targets)` and `Lint (fmt, clippy)`). |

**Linux gate (orchestrator's run).** `make check-ci` at the pin in Podman, aarch64 musl, 7 CPUs: `=== GATE: PASS ===`, `exit=0`. It ran `jar_adopt` 19 passed, `jar_attach` 4, `jar_process` 91, `session_jar` 8, `jar_differ` 1 + 3 (with the differ jar), `wire_v1` 3, the `spyc-jar` unit tests 21, the `spyc` lib 4,687, and the scripts 56 (1 skipped). Log: `<scratchpad>/linux/gate-linux.log`. The macOS gate ran 4,689 lib tests. Comparing the lists of passing tests (`comm` of the two logs): the macOS-only tests are `fs::listing::tests::a_privacy_denial_points_at_system_settings`, `fs::listing::tests::only_eperm_is_treated_as_a_privacy_denial` and `spyc_takes_its_memory_functions_from_libsystem`. None is a jar test, and no test runs on Linux only.

**Package check.** `cargo package --locked --workspace` (what `make package-check` runs; I ran cargo directly against my own build directory so that `make`'s `target-cap` would not prune other checkouts' build directories): exit 0. It packaged `spyc-jar` 0.1.0 (72.8 KiB), `spyc-vt-sys` 0.2.0 (4.4 MiB compressed) and `spyc`, then built each from its tarball. The packaged `spyc` manifest has `[[bin]] name = "spyc-jar"`, with `src/bin/spyc-jar.rs` in the crate and `[dependencies.spyc-jar] version = "0.1.0"` with no path. So `cargo install spyc` builds both binaries. Log: `…/scratchpad/F/package-check.log`.

## Findings

### F1. A build versioned `3.0.0` runs every program outside a jar (lead V1)

- **Severity:** blocker. **Lens:** correctness. **Status:** confirmed (failing tests at version 3.0.0, and lines read).
- **Sites** (searched with `rg -n "honoured\b|fn honoured|-CURRENT|-rc\." src crates tests --type rust` and `rg -n "pane\.jars|\.jars\b|BY_DEFAULT|by_default\(|jar::wanted|wanted\(|spec_for\(" src --type rust`):
  - the gate: `src/pane/jar/mod.rs:128-137` (`honoured()`, `honoured_by`: `version.contains("-CURRENT") || version.contains("-rc.")`);
  - `src/pane/jar/mod.rs:148` (`wanted`), reached by `spec_for` at `src/pane/jar/mod.rs:163` when no place is set;
  - `src/app/pane_starts.rs:126` (every tab, through `launch_tab_pane`, called with `config.pane.jars` from `src/app/pane_tabs.rs:344` and `:465`);
  - `src/app/capture_starts.rs:41-43` (`!` captures);
  - `src/app/bootstrap.rs:347` (the jar binary store is never started);
  - `src/app/orphan_jars.rs:63-66` (no look for orphaned jars);
  - the doc that states it, `src/config/mod.rs:284-288`;
  - a test that pins the gated behaviour: `src/pane/jar/tests.rs:561-567` asserts `!honoured_by("2.5.0")`, so removing the gate means changing this test.
- **Claim:** with the version at `3.0.0`, `[pane] jars` is ignored, no tab, task or capture starts in a jar, and `^d` refuses to detach. Every rc runs with jars, so the build the tag produces is one no rc soaked.
- **Evidence (failing tests).** In `~/src/spyc.worktrees/review/3.0-f` I set `version = "3.0.0"` in `Cargo.toml` and `Cargo.lock` (uncommitted, since reverted) and ran, under scratch `XDG_*`, `TMPDIR` and `HOME`:
  - `cargo test --locked --lib harness_tests::jar_start`: **1 passed, 13 failed**, every failure "the tab's pane never landed" (`src/app/harness_tests/jar_start.rs:120`). At the pin all 14 pass (macOS gate log). Log: `…/scratchpad/F/v1-jar_start-at-3.0.0.log`.
  - `cargo test --locked --test jar_adopt`: **1 passed, 18 failed**, mostly "never: the session records the tab's jar" (`tests/jar_adopt.rs:215`). spyc's own screen in `spyc_shutdown_ends_a_detached_session_and_its_tabs` reads "can't detach: the tab sh runs outside a jar and would end with spyc" (`src/app/detach.rs:88-92`). Log: `…/scratchpad/F/v1-binary-tests-at-3.0.0.log`. The run stopped before `session_jar`.
- **What would catch it.** Not `make release-prep` (`Makefile:339-359`: it rewrites the version line, runs `cargo update -p spyc`, prepends the changelog and commits; it runs no test). Not `make release-tag` (`Makefile:361-379`: version, changelog section and `origin/main` only). Not `release.yml` (no test step). Not `docs/RELEASE_ENGINEERING.md` §5 (no step about jars or S6). What does catch it is the release PR's own `ci.yml` `test` job: `make test` at `3.0.0` fails at least the 31 tests above, and branch protection requires that context with `strict: true`. So the tag cannot be cut green with the gate in place. The failure is loud, but nothing names its cause, and the obvious way through ("jar tests fail on the release PR") is to look for a flake.
- **Consequence for a user:** if the gate survives to the tag (merged over a red check, or "fixed" by changing the tests), 3.0.0 is 2.x with dead code: no agent outlives spyc, `^d` refuses whenever a tab is open, and `spyc -r` cannot take back jars an rc left running. If it is removed only in the release PR, the tag is a build no rc soaked.
- **Direction:** remove `honoured()` and its call sites in a PR before `3.0.0-rc.1`, so that every rc is the code the tag ships.

### F2. `cargo install spyc` can build a jar from code other than the release's

- **Severity:** high. **Frozen:** yes (a published crates.io version is immutable, and the jar it builds is the old image of every later upgrade from it). **Lens:** correctness, reliability. **Status:** confirmed (lines read, packaged manifest inspected).
- **Sites:** `release.yml:263-280` (`publish()` skips a version already on crates.io; `publish spyc-jar "$(version_of spyc-jar)"`), `release.yml:226-229` (rc tags publish too), `crates/spyc-jar/Cargo.toml:3` (`version = "0.1.0"`, not a prerelease), `Cargo.toml:139` (`spyc-jar = { version = "0.1.0", path = … }`, a caret requirement), `Makefile:348-349` (`release-prep` changes only the root `version` line), `docs/RELEASE_ENGINEERING.md:236-238` and `:281-282` (the skip is described as the design).
- **Claim:** the jar crate's version moves only by hand, and `release.yml` publishes it from whichever tag first carries an unpublished version. So spyc's crates.io package resolves the jar's library from that earlier tag's code, while the tarballs, the `.deb`s and Homebrew build it from this tag's.
- **Evidence (lines read and command output).** `curl https://crates.io/api/v1/crates/spyc-jar` answers 404 (never published). The `spyc` crate's version list on crates.io includes `2.4.0-rc.1` to `rc.3`, so rc tags do publish. The packaged `spyc` manifest from `cargo package` has `[dependencies.spyc-jar] version = "0.1.0"` and no path. Walk the 3.0 cycle: `v3.0.0-rc.1` publishes `spyc-jar` 0.1.0, a normal (non-prerelease) version, from rc.1's source, after the manual first publish `RELEASE_ENGINEERING.md:263-265` requires. A jar fix lands during the soak, and nothing bumps `crates/spyc-jar/Cargo.toml`. At `v3.0.0` the job prints "spyc-jar 0.1.0 is already on crates.io, skipping" and publishes spyc 3.0.0, whose `spyc-jar` binary (`src/bin/spyc-jar.rs`) links rc.1's library. `build-info` cannot tell: it prints `SPYC_GIT_SHA` and `SPYC_BUILD_TIME`, which come from spyc's `build.rs`, and those are the same for both binaries of one `cargo install`. `docs/drafts/JAR_UPGRADE_BRIEF.md:147-149` already lists the other direction (the caret lets `cargo install` take a *newer* 0.1.x); it is not done at the pin.
- **Consequence for a user:** a `cargo install` user runs jars whose protocol and upgrade code differ from the tested release. Once such a jar is running, its image is the old image of every later upgrade from it.
- **Direction:** tie `spyc-jar`'s version to spyc's in `release-prep` (or have a guard fail when `crates/spyc-jar` changed since its last published version), and pin it with `=`.

### F3. No test of any kind runs on macOS in CI (lead V2)

- **Severity:** high. **Lens:** reliability. **Status:** confirmed (workflow lines read, gate logs compared, and a macOS-only mutation that no test notices: F7).
- **Sites:** `.github/workflows/ci.yml:95-106` (the only test job, `runs-on: ubuntu-latest`); `.github/workflows/macos.yml:26-44` (path filter, tags, dispatch) and `:55-94` (`make fmt-check lint`, `cargo build --bin spyc`; no test); `.github/workflows/release.yml:43-70` (macOS build, no test); against `docs/drafts/JAR_PRODUCTION_PLAN.md:430` ("polls rather than sleeps, and runs on both CI platforms").
- **Claim:** every jar test runs in CI on x86_64 Linux only. The macOS mechanisms the design depends on are tested only by the owner's local `make check-ci`: the holder that keeps sockets alive across the `exec` (SPYC-TRAP `jar-upgrade-holder`), `getpeereid` and `LOCAL_PEERPID` (`crates/spyc-jar/src/jar/accept.rs:80-83`, `:108-112`), `SO_NOSIGPIPE` (`accept.rs:124-127`), `$TMPDIR` socket placement, and the first-run scan of a new binary.
- **Evidence:** the workflow lines above. Neither `macos.yml` job runs `cargo test` or `make test`. `macos.yml`'s path filter covers no `src/**` or `crates/spyc-jar/src/**` path, so even the macOS clippy run skips a PR that changes only macOS-gated jar code. The gate logs show no jar test is platform-gated, so the same 227 jar tests ran on both machines (`jar_process` 91, `pane::jar` 64, `spyc-jar` unit 21, `jar_adopt` 19, `jar_start` 14, `session_jar` 8, `jar_attach` 4, `jar_differ` 3, `wire_v1` 3) (macOS gate log, and the Linux log above). In CI they run on Linux alone. Mutation M12 (F7) removes the macOS-only `SO_NOSIGPIPE` call, and no test fails on macOS either.
- **Consequence for a user:** most users (the owner and his daily users) run macOS, where a regression in a macOS-only path can merge green. The owner's pre-merge `make check-ci` is the one barrier. It is documented as such (`macos.yml:9-10`), but it is local and unrecorded.
- **Direction:** a `macos-latest` job running at least `cargo test --test jar_process --test jar_attach --test jar_adopt --test session_jar -p spyc-jar` and the differ test on every PR that touches `crates/spyc-jar` or `src/pane/jar`, and on every tag.

### F4. No released jar is kept or run as a fixture, and the one compatibility test cannot run

- **Severity:** medium. **Lens:** maintainability, docs. **Status:** confirmed (search and lines read).
- **Sites:** `tests/jar_compat.rs:1-6` (its doc says the old jar comes from "the `spike/jar-stage-2` branch's `spikes/jar-stage-2/old_jar.sh`") and `:43` (`#[ignore]`); `docs/drafts/JAR_PRODUCTION_PLAN.md:164-166` and `docs/drafts/JAR_PROTOCOL.md:1052-1053` (both say CI keeps each released jar and runs it).
- **Claim:** nothing in CI or the `Makefile` sets `SPYC_JAR_OLD_BIN`, nothing keeps a released `spyc-jar`, and the branch the test's doc points to no longer exists. The documents state a mechanism that isn't there: the documents are wrong, not the code.
- **Evidence:** `rg -n "SPYC_JAR_OLD_BIN" --hidden` finds only `tests/jar_compat.rs` and the review brief. `git branch -a | grep -i spike` and `git ls-remote origin 'refs/heads/spike/*'` both return nothing. In both gate logs `jar_compat` shows "0 passed; 1 ignored". `release.yml:203-220` keeps only the three newest rc releases, so an rc's jar is gone from GitHub after three more rcs. Stable releases' tarballs stay.
- **Consequence for a user:** nothing checks that a later spyc still drives a 3.0.0 jar. That is the property that makes freezing version 1 worth anything, and it is the one most likely to regress quietly.
- **Direction:** a CI job that downloads each stable release's `spyc-jar` from its GitHub release (starting with 3.0.0) and runs `jar_compat` with it, failing, not skipping, when a fixture is missing.

### F5. "No timing in tests" does not hold: about 20 fixed waits assume a state or assert an absence

- **Severity:** medium. **Lens:** reliability. **Status:** confirmed (every site read with its context).
- **Search:** `rg -n "thread::sleep|\bsleep\(|sleep [0-9]|sleep\s+[0-9.]+|usleep|recv_timeout|wait_timeout|set_read_timeout|park_timeout"` over `tests/jar_*.rs`, `tests/jar_process/*.rs`, `tests/session_jar.rs`, `crates/spyc-jar/tests/*.rs`, `src/pane/jar/tests.rs` and `src/app/harness_tests/jar_start.rs` (138 hits), and `rg -n "thread::sleep|\bsleep\(|sleep [0-9]|recv_timeout|wait_timeout|park_timeout"` over the six `spyc-jar` sources that hold a `#[cfg(test)]` module (5 hits, 3 of them in `client.rs`'s tests). Every hit with its context: `…/scratchpad/F/sleep-sites.json`.
- **Against:** `docs/drafts/JAR_PRODUCTION_PLAN.md:61-62` ("every wait polls for the state it needs") and `:430`.
- **Classification:**
  - **Polling for a state** (a short sleep inside a loop or `eventually` that checks the state and has a bound): 82 sites, plus about 7 bounded reads with `WAIT`. These meet the plan.
  - **`sleep N` inside a program's text** (a long-lived program, or a fake validator or resumer that is slow on purpose): about 22, such as `sh("sleep 1000")` and `timers.rs:441` `sleep 35`. Not a test wait.
  - **Deliberately past a production clock** (the test is about the deadline): `tests/jar_process/ownership.rs:21` (13 s), `:102` (7 s), `:137` (12 s); `tests/jar_attach.rs:353` (6 s); `src/pane/jar/tests.rs:745,762` (1.1 s, the snapshot interval), `:925,929` (600 ms against `REPLY_GRACE`), `:1715` (6 s of `SIGSTOP`), `:1757` (`PARKED_AT_MOST`); `tests/jar_process.rs:1014` (500 ms against a 300 ms linger), `:1545` (2 s slow owner), `:2025` (3 s CPU sample), `:2161` (500 ms before a successor); `tests/jar_adopt.rs:1468` (2 s, whole-second start times); `crates/spyc-jar/src/client.rs:459` (200 ms against a 50 ms `recv`). These test clocks the brief already questions (lead V11). Each passes only while the machine keeps up, and `replies_wait_until_the_replayed_tail_is_parsed` is the only test that notices mutation M9 (below).
  - **Asserting that nothing happened within a fixed wait** (can't go red from load, but can go green without testing anything, when the thing it rules out is merely slow): `src/pane/jar/tests.rs:413` (300 ms, "the program ran"), `:764` (100 ms), `:1161` (300 ms, "started twice"), `:1229` (1.5 s, "a replayed query was answered again"), `:1676` (2 s, "ran once"), `:2275` (1 s second chance); `tests/jar_process/upgrade.rs:602` (300 ms, "no program before an owner"); `tests/jar_process.rs:1039` (2 s), `:1380` (200 ms), `:1717` (300 ms); `tests/jar_adopt.rs:458` (5 s, "typed /resume").
  - **Assuming a state was reached** (can flake, or skip the scenario, under load): `tests/jar_process/upgrade.rs:571-580`: `parking()` decides "the jar is parked" from a lookup unanswered for 300 ms, so a jar slow to answer reads as parked, and the two tests that use it (`timers.rs:308`, `upgrade.rs:522`) proceed on that verdict. `tests/jar_process/timers.rs:300` and `upgrade.rs:509`: 300 ms after writing 20,000 pings, taken as enough for the jar to have queued their answers. `tests/jar_process.rs:403` and `:458`: "the log settled" means unchanged across 200 ms and 500 ms. `tests/jar_adopt.rs:1261`: 300 ms between `^a k` and `yf`, taken as enough for the chord to land.
- **Consequence for a user:** none directly. For the owner: the gate's jar tests can go red under load, which his flake policy forbids, or green without testing what they say. The plan says the spike's two flaky tests were exactly this kind.
- **Direction:** for each "assuming" and "absence" site, find the state that decides it (a `describe` field, a marker the program writes, a frame) and poll for that; where a jar exposes no such state, that is a gap in what it reports, not a reason for a longer wait.

### F6. No test notices the jar accepting another user's connection

- **Severity:** medium. **Lens:** security. **Status:** confirmed (mutation M3 survived).
- **Sites:** `crates/spyc-jar/src/jar/accept.rs:156-160` (`serve`'s `peer_is` check); `accept.rs:298-302` (`a_peer_is_known_by_its_user` tests `peer_is` alone).
- **Claim:** removing the R26 check that refuses a peer of another user leaves the jar suites green: nothing proves `serve` calls `peer_is`, or calls it with the jar's own uid.
- **Evidence:** M3 above: `spyc-jar` units, `jar_process` (91) and `jar_attach` (4) all pass with `if false && !peer_is(…)`.
- **Consequence for a user:** a refactor that drops or inverts the first fence of a jar's socket can merge green. The `0700` directory would still stand, but it is the second fence.
- **Direction:** route the uid through `serve` (or the acceptor's config), so that a test can run a jar that expects a uid it is not, and see a same-user connection refused.

### F7. `SO_NOSIGPIPE` on macOS has no test

- **Severity:** medium. **Frozen:** touches the old image's behaviour after a failed `exec` (unit A owns that surface). **Lens:** reliability. **Status:** confirmed (mutation M12 survived on macOS).
- **Sites:** `crates/spyc-jar/src/jar/accept.rs:121-127`.
- **Claim:** the call exists for the case its comment names: after a failed `exec`, `SIGPIPE` is back at its default, and a write to a peer that has gone would kill the jar and its program. No macOS test writes to a gone peer after a failed `exec`, so removing the call is unnoticed. CI would not run such a test anyway (F3).
- **Evidence:** M12: `spyc-jar` units, `jar_process` and `jar_attach` all pass on macOS with the call removed. On Linux the code is a no-op (`accept.rs:129-131`), because std writes with `MSG_NOSIGNAL` (checked in the 1.100 std source: `library/std/src/sys/net/connection/socket/mod.rs:80-83,446`; on macOS `MSG_NOSIGNAL` is `0`).
- **Consequence for a user:** a regression here ends a jar, and the agent in it, the first time an owner disconnects after a failed upgrade, on macOS only. A 3.0.0 jar would carry that regression for its whole life.
- **Direction:** a macOS test that fails an upgrade's `exec`, then drops a reader mid-write and checks the program runs on.

### F8. A `.deb` without `spyc-jar` is built silently

- **Severity:** low. **Lens:** reliability. **Status:** confirmed (lines read).
- **Sites:** `Makefile:557-561` (`_deb` installs `spyc-jar` only `if [ -f … ]`), `.github/workflows/apt.yml:168-177` (moves each binary only if the tarball had it).
- **Claim:** both skip a missing `spyc-jar` without a word, by design for releases from before the jar. Nothing distinguishes a 3.x release whose tarball lacks it.
- **Evidence:** the lines. Today `release.yml:106-111` always packs `spyc-jar`, and `scripts/test_release_ships_spyc_jar.py` checks that step, so the exposure is a future change to the release packaging.
- **Consequence for a user:** an apt user of such a release gets a spyc whose every tab fails to start ("check spyc's installation") until they accept jarless tabs.
- **Direction:** require `spyc-jar` in `_deb` for a version at or above 3.0.0.

### F9. `RELEASE_ENGINEERING.md` is out of step with the pipeline in places

- **Severity:** low. **Lens:** docs. **Status:** confirmed (lines read against workflows and `cargo package` output).
- **Sites and claims:** `docs/RELEASE_ENGINEERING.md:383` ("attach … ×4") and `:617-618` ("all four artifacts"): `release.yml` attaches three tarballs. `:275-279`: `spyc-vt-sys` "measures 3.91 MiB as a `.crate`", but `cargo package` at the pin reports 4.4 MiB compressed (13.1 MiB unpacked). §5 and §12 say nothing about jars: no step for S6 (F1), the first manual publish of `spyc-jar` beyond `:263-265`, or bumping the jar crate's version (F2).
- **Consequence for a user:** none directly. The release manager follows a checklist that misses the 3.0-specific steps.
- **Direction:** add the 3.0 steps (gate removed before rc.1, `spyc-jar` first publish and version policy, the first released jar kept as a fixture) to §5.

### F10. Size ratchets since `v2.4.0`: no pin added, two lowered

- **Severity:** informational. **Status:** confirmed (command output).
- **Evidence:** `git diff v2.4.0..HEAD -- src/size_guard.rs src/fn_size_guard.rs` changes only two lines of `src/fn_size_guard.rs`: `src/app/mcp.rs::App::execute_mcp_command` from 236 to 214, and `src/app/state/dispatch.rs::AppState::dispatch_command` from 185 to 174. `src/size_guard.rs` is unchanged, and both its pin lists are empty (`:66`, `:76`). The guards' roots include `crates` and `tests` (`src/size_guard.rs:33`), so the jar crate and its tests are held to the limits. Test files count as test modules (`is_test_module`, `:119-128`), so a test file's limit is 3,000 lines: `tests/jar_process.rs` (2,700) and `src/pane/jar/tests.rs` (2,295) are the nearest.

### F11. The tag is built and published with no test run of its own

- **Severity:** informational. **Lens:** reliability. **Status:** confirmed (lines read).
- **Sites:** `.github/workflows/release.yml:43-125` (build jobs; `publish` needs `macos`, `linux` and `vendored-archives` only), `.github/workflows/macos.yml:42-43` (runs on tags, but nothing waits for it).
- **Claim:** a tag publishes binaries, the tap, apt and crates.io from a commit whose tests ran only in `main`'s Linux CI (required and `strict`), and the macOS lint that runs on the tag can fail after the release is published. Given F1's protection this is acceptable on Linux. It is the macOS half of F3 again.


## Mutation runs

One mutation at a time in `~/src/spyc.worktrees/review/3.0-f`, reverted with `git checkout` after each (the worktree is clean now). Jar-side mutations ran `cargo test --no-fail-fast -p spyc-jar` and `--test jar_process --test jar_attach`. spyc-side mutations ran `--lib pane::jar`, `--lib app::harness_tests::jar_start` and `--test jar_adopt`. All ran on macOS, under scratch `XDG_*`, `TMPDIR` and `HOME`. Baselines: all green (spyc-jar 21+2+3, `jar_attach` 4, `jar_process` 91, `pane::jar` 64, `jar_start` 14, `jar_adopt` 19). Every mutation compiled. Results and logs: `…/scratchpad/F/mutation-results.json`, `…/scratchpad/F/mut-*.log`.

| # | What it breaks | Tests that failed |
|---|---|---|
| M1 | a reader's Input, Resize, Signal and other owner-only messages act as the owner's: `crates/spyc-jar/src/jar/conn.rs:256` `_ if !owner \|\| program` → `_ if lookup \|\| program` | `sequences::no_sequence_lets_a_reader_type`, `a_refusal_says_why`, `one_owner_at_a_time_and_a_reader_cannot_type` |
| M2 | the hello's token is never checked: `crates/spyc-jar/src/jar/accept.rs:198` token check disabled (`if false && !same_token(…)`) | `a_refusal_says_why`, `a_released_jar_opens_only_to_its_new_token`, `a_released_jars_new_token_crosses_an_upgrade`, `sequences::no_sequence_takes_the_lease_without_the_token`, `without_the_token_a_client_can_only_ask_what_the_jar_is` |
| M3 | serve skips the peer-user check: `crates/spyc-jar/src/jar/accept.rs:158` peer-user check disabled (`if false && !peer_is(…)`) | **none: survived** |
| M4 | any line from the validator counts as the proof: `crates/spyc-jar/src/upgrade.rs:739-744` `proves` always true | `timers::a_binary_that_refuses_is_rejected_for_what_it_did`, `upgrade::a_proof_for_another_blob_is_refused`, `upgrade::a_validator_that_prints_no_proof_is_refused`, `upgrade::an_upgrade_to_a_binary_that_cant_resume_leaves_the_program_running` |
| M5 | release keeps the old token: `crates/spyc-jar/src/jar/conn.rs:230` `st.token = token` removed | `a_pane_that_does_not_linger_survives_a_handover`, `a_released_jar_opens_only_to_its_new_token`, `a_stalled_reader_does_not_freeze_a_release`, `a_released_jars_new_token_crosses_an_upgrade` |
| M6 | a replay from before the ring's base gets no gap: `crates/spyc-jar/src/jar/conn.rs:200-202` the `Gap` frame never sent | `an_ownerless_jar_drops_the_oldest_bytes_and_says_which`, `upgrade::a_validators_child_holding_its_stderr_cannot_hold_the_jar`, `upgrade::a_reader_trickling_during_the_flush_cannot_keep_the_jar_parked` |
| M10 | the jar ends at its program's exit, collected or not: `crates/spyc-jar/src/jar/life.rs:185` `if true \|\| st.exit_acked \|\| alone_too_long` | `a_paste_the_program_never_reads_does_not_block_its_owner`, `a_signal_after_the_exit_is_refused`, `a_signalled_exit_reports_its_signal`, `an_exit_with_nobody_there_is_kept_from_the_exit`, `ownership::a_jar_sits_idle_in_every_idle_state`, `a_captures_exit_is_kept_until_an_owner_collects_it`, `upgrade::a_captures_jar_still_loses_no_output_after_an_upgrade` |
| M11 | a signal after the exit is sent to the program's old pid: `crates/spyc-jar/src/jar/conn.rs:307` the after-exit `Signal` refusal disabled | `a_signal_after_the_exit_is_refused` |
| M13 | any local process of the user may connect as the program role: `crates/spyc-jar/src/jar/accept.rs:190-191` program role accepts any peer pid | `a_refusal_says_why`, `sequences::no_sequence_takes_the_lease_without_the_token` |
| M12 | macOS: a connection's socket keeps SIGPIPE (SO_NOSIGPIPE never set): `crates/spyc-jar/src/jar/accept.rs:126` `set_socket_nosigpipe` removed (macOS only) | **none: survived** |
| M7 | an adopted tab answers terminal queries in the replay: `src/pane/jar/mod.rs:594` adopted tab gets `ReplayGate::open()` | `pane::jar::tests::a_replayed_query_is_not_answered_again` |
| M8 | a snapshot is stored at the parser's count, not the jar's offset: `src/pane/jar/snapshots.rs:45-50` snapshot stored at `parsed`, not the jar's offset | `pane::jar::tests::a_snapshot_after_a_gap_is_stored_at_the_jars_offset` |
| M9 | the reply gate opens once any of the replay is handed over: `src/pane/jar/link.rs:117` reply gate opens once any replay is handed over | `pane::jar::tests::replies_wait_until_the_replayed_tail_is_parsed` |

Eleven of thirteen were noticed. M3 and M12 survived (F6, F7). M7, M8, M9 and M11 are each noticed by exactly one test, and M9's only guard is a test that waits 600 ms against `REPLY_GRACE` (F5).

## Seeded leads

- **V1 (owner F): confirmed.** `honoured()` is at `src/pane/jar/mod.rs:131` with six call sites and a pinning test (F1). The brief's sites are right (`pane_starts.rs:126`, `bootstrap.rs:347`, `capture_starts.rs:42`, `orphan_jars.rs:65`); added are `wanted`/`spec_for` in `mod.rs`, the config doc and the test at `src/pane/jar/tests.rs:561-567`. Built as `3.0.0`, 31 tests fail and `^d` refuses. Nothing in `release-prep`, `release-tag`, `release.yml` or the release checklist catches it. The required `Tests` check on the release PR does, so it fails loudly before a tag. But rc builds and the final build differ unless the gate goes before rc.1.
- **V2 (owner F): confirmed.** No jar test, and no test of any kind, runs on macOS in CI (F3). The fuzz job is Linux and weekly (true as the protocol words it). `jar_compat` is ignored, and nothing will pin 3.0.0's jar as the first fixture: no workflow keeps or fetches a released jar, and the test's doc points to a branch that no longer exists (F4). One correction to the lead: the brief lists "a full backlog refused like a dead socket" and "`keep_socket`" as macOS-specific. Those code paths are not `cfg`-gated: the only `target_os` code in the jar is in `crates/spyc-jar/src/jar/accept.rs` (`rg -n 'target_os' crates/spyc-jar/src`). Their behaviour differs by platform, which only a test on macOS can show.

## Manual checklist items

1. **Branch protection holds the release PR.** Before `release-prep` for 3.0.0, confirm in GitHub settings that "Tests (cargo test --all-targets)" is required for admins too. Expected: a merge button that stays disabled on a red test job. Settles F1's "loud, not silent".
2. **Homebrew formula.** After the first rc with jars, open `Tripstack-Corp/homebrew-tap`'s `Formula/spyc.rb` and check that `bin.install "spyc", "spyc-jar"` is there and that the `--HEAD` build installs both bins (`brew install --HEAD Tripstack-Corp/tap/spyc && ls $(brew --prefix)/bin/spyc-jar`). Expected: both present. Settles the packaging row for Homebrew, which I could only check in `homebrew.yml`.
3. **The first `spyc-jar` publish.** Decide which commit publishes `spyc-jar` 0.1.0 by hand (`RELEASE_ENGINEERING.md:263-265`), and whether rc tags may publish it at all. Settles F2.
4. **A Mac run of the jar suites before each tag.** Until F3 is fixed: `make test` on the Apple silicon Mac at the exact tagged commit, with its log kept beside the release. Expected: `jar_*`, `session_jar` and the differ test green.

## Not covered

- Linux probes of my own: I relied on the orchestrator's gate run (aarch64 musl). Nothing was run on x86_64 glibc, CI's platform.
- `.github/workflows/{audit,codeql,shipped-targets,vendored-archives,cache-cleanup,secrets}.yml`: not read (`secrets.yml` was refused by permission settings).
- `src/style_guard.rs` and `src/link_guard.rs` only skimmed: I did not check them for guards that check less than they claim.
- The jar test bodies were not read end to end. They were read at every wait site and exercised by the runs above.
- The `version = 3.0.0` run did not reach `session_jar` or `jar_attach` (cargo stopped after `jar_adopt` failed).
- `build.rs`: its `rerun-if-changed=.git/HEAD` names a path that does not exist in a linked worktree (`.git` is a file there). I did not measure what cargo does with it, or whether two builds at one commit then get different `SPYC_BUILD_TIME`s, which the store check reads.
- Mutation coverage is thirteen mutations chosen by hand, not a tool's sweep. The jar-side runs did not include `jar_adopt`, `session_jar` or the `spyc` lib.
- Load: no test was run under CPU or I/O pressure here (unit B owns that), so the "assuming a state" sites in F5 are classified from their code, not seen failing.
