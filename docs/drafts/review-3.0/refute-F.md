# Refuter's verdicts: F1, F2, F3 and G1

- **SHA:** `444857a2a32b2f284d2b4fd19e57aef19700a5c5` (`git -C …/review/3.0 log -1`), unmoved.
- **Platform:** macOS (Apple silicon). Every verdict below rests on lines read at the pin, the F reviewer's logs (re-read, not re-run), and read-only `gh` queries. I built and ran nothing: no `cargo`, no `make`, no spyc or `spyc-jar` process, so no jar, socket or token was touched.

| Id | Verdict | Severity I'd give |
|---|---|---|
| F1 | `partly` | medium (a release precondition, not a latent defect) |
| F2 | `confirmed` | high, as filed |
| F3 | `severity` | medium |
| G1 | `partly` | low |

## F1. A build versioned `3.0.0` runs every program outside a jar

**Verdict: `partly`.** The code claim holds; two of its consequences do not.

**What survives.** `honoured_by` is `version.contains("-CURRENT") || version.contains("-rc.")` (`src/pane/jar/mod.rs:135-137`), read through `env!("CARGO_PKG_VERSION")` (`:131-133`) with no override (`rg -n honoured src crates tests` finds no other reader or switch). It gates `wanted` (`mod.rs:148`), `launch_tab_pane` (`src/app/pane_starts.rs:126`, read whole: its first statement returns `Pane::launch` when `!(jars && jar::honoured())`), `capture_host` (`src/app/capture_starts.rs:41-43`), the store's start (`src/app/bootstrap.rs:347`) and `jars_wanted` (`src/app/orphan_jars.rs:63-66`). That is five code sites plus the doc at `src/config/mod.rs:284-288`, not six. With no tab `jarred()`, `detach` (`src/app/detach.rs:83-92`) reaches `unjarred` (`:149-166`) and refuses. The F reviewer's logs agree: `…/scratchpad/F/v1-jar_start-at-3.0.0.log` ends `1 passed; 13 failed`, `…/v1-binary-tests-at-3.0.0.log` shows `jar_adopt` `1 passed; 18 failed` and, at its line 66, the screen text "can't detach: the tab sh runs outside a jar and would end with spyc".

**What does not.**

1. *"Every rc soaks the gated-on build, and the final tag's build is one no rc ran."* On every rc `honoured()` returns true, so every caller takes its jar branch. Removing the gate deletes a branch no rc ever takes. Whether the gate goes before `3.0.0-rc.1` or in the release PR, the final build runs the same jar paths as the last rc. The only build that differs from the rcs is the one that keeps the gate, which point 2 shows cannot merge green.
2. *"No release step checks for this."* The release PR is a PR to `main`, and `gh api repos/Tripstack-Corp/spyc/branches/main/protection` answers `{"contexts":["Tests (cargo test --all-targets)","Lint (fmt, clippy)"],"enforce_admins":true,"strict":true}`. At `3.0.0`, 31 tests in `make test` fail (above), and `make release-tag` refuses unless `HEAD` is `origin/main` with the version merged (`Makefile:361-379`). So nothing ships with the gate unless someone edits the tests to pass, which is not something a checklist would catch either. F1's own text concedes this.
3. It is planned work, not something that slipped. Plan S2 says "Until S6 only a `-CURRENT` or rc build honours the setting" (`docs/drafts/JAR_PRODUCTION_PLAN.md:75-77`), S6 (`:258-263`) and decision 1 (`:441-445`) say the first release with jars has them on, and S6 is open as #1141 (`gh issue view 1141`), with S2 to S5 (#1137 to #1140) also open. `main` is `2.5.0-CURRENT` and no `v3*` tag exists (`git tag -l 'v3*'` is empty).

**The narrower form.** S6's removal of `honoured()` (with `only_a_current_or_rc_build_reads_the_setting`, `src/pane/jar/tests.rs:561-567`, and the three "development builds" sentences of G1) must land before the `3.0.0` release PR. Neither #1141's text, nor S6's, nor `docs/RELEASE_ENGINEERING.md` §5 names `honoured()`. If it is forgotten, the release PR fails with "the tab's pane never landed" and "never: the session records the tab's jar", which point at a flake rather than at the gate. That is a checklist gap: medium. It "holds the tag" only in the sense that any unfinished slice does.

**What would make me wrong:** a path that builds or publishes a stable version without passing `main`'s required `Tests` check (for example, a tag pushed from a branch other than `main`, which `release.yml` itself would accept), or the owner choosing to merge the release PR over a red check.

## F2. `cargo install spyc` can build a jar from code other than the release's

**Verdict: `confirmed`.** Severity high, as filed. The mechanism is wider than F2 says.

**Evidence (lines read).** `release.yml`'s `crates` job runs on every `v*` tag, rc tags included (`.github/workflows/release.yml:22-26`, and the job's own comment at `:226-229`). `publish()` skips any crate version already on crates.io (`:263-273`) and publishes `spyc-jar` at whatever `crates/spyc-jar/Cargo.toml:3` says (`version = "0.1.0"`, `:279`). `make release-prep` changes only the root `version` line (`Makefile:348-349`). I found nothing that moves the jar crate's version or notices that its source changed since it was published: `rg -n "0\.1\.0|version_of|crates/spyc-jar/Cargo.toml" scripts src/*.rs Makefile tests .github` finds only the publish lines and a comment. `docs/RELEASE_ENGINEERING.md:236-238` describes the skip as intended ("a release that didn't move a library crate republishes nothing"), and gives `spyc-vt-sys` a versioning policy (`:281-282`) but `spyc-jar` none. The `spyc-jar` binary is `spyc_jar::cli::main_built(env!("SPYC_GIT_SHA"), env!("SPYC_BUILD_TIME"))` (`src/bin/spyc-jar.rs:5-7`), so its `build-info` names spyc's build, not the jar library's source. The package log (`…/scratchpad/F/package-check.log`, lines 42-44) shows spyc verified against `spyc-jar` from a temporary local registry built from the workspace, so `make package-check` cannot see a drift from crates.io either.

**What I tried, and why it does not refute.**

- The rationale at `release.yml:226-229` ("crates.io keeps prereleases out of the default `cargo install` resolution") is true of `spyc`'s `3.0.0-rc.N` but not of `spyc-jar` `0.1.0`, which is an ordinary version. It is a misstatement, not a guard.
- `cargo publish -p spyc` verifies the package against the registry's `spyc-jar`. If spyc's code needs a `spyc_jar` item added after the first publish (spyc's lib uses `spyc_jar::{client,proto,state,attach}` in 21 files), that verify fails loudly. So the silent case is a change inside the jar that keeps the API spyc calls: exactly the shape of a bug fix in the jar's behaviour, its upgrade code included.
- Without `--locked`, `cargo install` resolves `spyc-jar` afresh, so a later `0.1.x` would reach a `3.0.0` install. That is a partial repair after the fact, not a defence; with `--locked` the published `Cargo.lock` pins `0.1.0`. `INSTALL.md:91` and `README.md:132` say plain `cargo install spyc`.
- The timing depends on the owner: if the manual first publish (`RELEASE_ENGINEERING.md:263-265`, required before Trusted Publishing works) is done at `3.0.0` rather than rc.1, the 3.0.0 crate is right. It does not help after that: every later spyc release builds its jar from `0.1.0`'s code until someone bumps it by hand. So the drift is not only rc.1 to final but from the first publish onward, which makes F2 broader, not narrower.

On the `frozen` mark: rc users running jars from rc tarballs also carry rc code as their old image, so the frozen exposure is not new in kind. It is new in that a *final* release's channel carries it, with no way to tell from `build-info`. I keep the mark.

**What would make me wrong:** a release step outside the tree (an owner's habit, a crates.io setting) that bumps `crates/spyc-jar/Cargo.toml` whenever `crates/spyc-jar/src` changes, or a decision that `cargo install` is not a supported channel for 3.0.

## F3. No test of any kind runs on macOS in CI

**Verdict: `severity`.** True as stated; I'd rank it medium, not high.

**What survives.** The only test job is `ci.yml:95-106`, `runs-on: ubuntu-latest`. `rg -n "runs-on|make test|cargo test" .github/workflows` shows every `macos-latest` runner is in `macos.yml` (`:63`, `:75`, lint and link) or `release.yml:45` (build, `check-libsystem-binding.sh`, size). The files F's ledger left unread (`shipped-targets`, `vendored-archives`, `audit`, `codeql`, `cache-cleanup`, `secrets`) all run on `ubuntu-latest`, so none of them is a hidden macOS test. `macos.yml`'s path filter (`:28-41`) names no `src/**` or `crates/spyc-jar/src/**`. The plan's "runs on both CI platforms" (`JAR_PRODUCTION_PLAN.md:430`) is not met, and `JAR_UPGRADE_BRIEF.md` §5 already lists it as owed.

**Why medium.**

- The gap is a documented choice, not an oversight: `macos.yml:1-10` says the macOS jobs run only when a build or link can change because "macOS runners are often short", and that "every change is also … built on an Apple silicon Mac before it merges: the developer's own `make check-ci`", which runs the whole test suite. `CONTRIBUTING.md:333` (`SHIP_LOCAL_MACOS_LINT`) records the same trade-off for lint. So the macOS tests run before each merge; what is missing is a record and an enforcement, which matters most for a merge made by an agent or without the owner's machine.
- F3's strongest evidence does not support it. Mutation M12 (`SO_NOSIGPIPE` removed) survived on macOS in the F reviewer's own local run, so a macOS CI job running the same tests would have missed it too. M12 shows a missing test (F7), not a missing platform.
- What the gap costs is real but bounded: a macOS-only regression that the owner's local run would show, merged without that run. The brief's points about load and flakes favour CI runs on macOS (many recorded runs find what one local run does not), which keeps it above low.

**What would make me wrong:** evidence that merges to `main` happen without a local macOS `make check-ci` (agents merging, or the owner merging from Linux), or a macOS-only test failure that reached `main` since the jar work began.

## G1. The documents call jars a development feature, and `3.0.0` refuses `^d`

**Verdict: `partly`.** Severity low, not high.

**What survives.** The sentences are there: `FEATURES.md:67` ("On development builds each tab's program runs in a jar"), `docs/features/pane.md:338-339` ("On development (`-CURRENT`) and release-candidate builds"), `src/config/default.spycrc.toml:110-112` and `CONFIGURATION.md:89-90` ("`-CURRENT` and rc builds only"). The doc comment on `honoured` (`src/pane/jar/mod.rs:128-130`) says "until jars are on by default" while `BY_DEFAULT` (`:100`) is already `!cfg!(test)`. And the `^d` refusal follows from the code exactly as G1 traces it (`detach.rs:83-92`, `:149-166`).

**What does not.**

1. At the pin the documents are right: they say what `honoured()` does. They become wrong only when S6 removes the gate without them, which `AGENTS.md`'s same-commit docs rule already requires of that PR, or if `3.0.0` ships with the gate, which F1's point 2 shows cannot pass `main`'s required checks.
2. *"`^d`, which the same pages present unconditionally."* The full reference states the condition: `docs/features/projects.md:265-266`, "Every tab must run in a jar to detach; spyc says which one doesn't." Only the summaries (`FEATURES.md:103`, `docs/KEYBINDINGS.md:333`, `src/ui/help.rs:570-571`) leave it out, as summaries do.
3. G1's consequence ("a new user who installs 3.0.0 reads that jars are a development feature") needs a `3.0.0` built with the gate, the same premise F1 rests on.

**The narrower form.** S6's PR owes four sentences (the three pages and the template) plus the stale comment on `honoured`, and `FEATURES.md:67` already understates the rc case ("development builds" where the code also honours `-rc.`). That is a docs item for S6: low.

**What would make me wrong:** a release that can ship with the gate in place (see F1), or a reading of `projects.md` that a user would not reach from the `^d` row in `docs/KEYBINDINGS.md` and the `?` overlay before trying it.
