# spyc 3.0 pre-release review: synthesis

**Subject:** `main` at `444857a2a32b2f284d2b4fd19e57aef19700a5c5` (2026-11-24, `2.5.0-CURRENT`). `main` moved once during the review, to `52ce15b7` (#1317, a stopped spyc taking its terminal back after `fg`): it touches `src/terminal.rs`, `src/app/proc.rs` and `tests/jar_adopt.rs` and nothing in the jar's frozen surface, so it was not a stop, and nothing here was re-read against it.

**Platforms:** macOS 26 on Apple silicon (14 cores) for the gate, every unit's reading and most probes; Linux 6.19 **aarch64 musl** (Alpine in Podman, 7 CPUs, non-root) for the Linux gate, Unit B's load runs, Unit A's exec-boundary probes and Unit C's stopped-jar probes. CI's platform, x86_64 glibc, was not run anywhere, and neither was WSL or a systemd-logind host.

**Gate:** `make check-ci` passed at the pin on both platforms, unpiped (`=== GATE: PASS ===`, exit 0; summaries in `evidence/`: `gate-macos-444857a2-summary.txt`, `gate-linux-aarch64-musl-444857a2-summary.txt`).

**How it ran:** seven unit reviewers (A to G) in parallel, each reading its files whole and keeping a coverage ledger; then an independent refuter for every blocker and high (`refute-B.md`, `refute-C.md`, `refute-D.md`, `refute-F.md`, `refute-G.md`); then this referee, who re-read the sites of every surviving high and of the one blocker candidate. The session was interrupted once (a login switch) and every reviewer resumed from its own transcript; nothing was lost.

**A note from the owner, mid-review:** the jar protocol is not frozen until 3.0 is tagged. Every `frozen` mark below therefore means "settle before the tag, because after it this can't change", and changing version 1 itself is a fair direction of fix.

## Verdict

**Yes, the tag should wait, and for a short list.** Nothing found ships broken by default once S6's planned work lands, and no probe touched the owner's jars. But the plan's central promise, "a tab's program never runs twice" (`ARCHITECTURE.md:1121`), is broken on two paths shown by a red test and a probe (C1, C2). One packaging decision can't be undone after the first rc publishes (F2). And the version-1 surface has a handful of faults that are cheap now and permanent after the tag (B1's upgrade half, A1, A2, A3, G9).

What holds the tag, in the order I'd do it:

1. **S6's removal of `honoured()`** (`src/pane/jar/mod.rs:131-137`; issue #1141, open). This is planned work, not a defect: a `3.0.0` build runs with jars off, and the required `Tests` check (strict, enforced for admins; checked with `gh api …/branches/main/protection`) goes red on the release PR, with 31 jar tests failing at `3.0.0` (F1). The brief's worry that the final tag ships the one configuration no rc ran is refuted. What survives is that no checklist names the gate (F1, G1).
2. **C1 and C2: a program run twice.** Fix before the tag. Two Claude Code tabs in one conversation, both editing the tree, is the outcome.
3. **F2: `spyc-jar`'s version.** Decide before the first rc tag, because `release.yml` publishes `spyc-jar` `0.1.0` (no prerelease suffix) on whichever tag comes first and skips it on every later one. crates.io versions can't be replaced. `spyc-jar` is not on crates.io yet (checked: `/api/v1/crates/spyc-jar` returns 404), so this costs nothing today.
4. **B1: unchecked `thread::spawn` in the jar**, the frozen half first (`crates/spyc-jar/src/upgrade.rs:114,566,670,678,686`): a refused thread there leaves a jar that can never be upgraded again, or one parked for good that stops reading its program's output.
5. **The smaller frozen faults:**
   - A1: `SIGPIPE` is no longer ignored after a failed `exec`.
   - A2: reason 6 is sent where the protocol promises 5.
   - A3: the copy step opens any path, a FIFO included.
   - G9: three sentences, two of them in the frozen specification, contradict jar-or-nothing.

Everything else is a patch-release matter, though several mediums (D1, D2, C3, B12, E1, E2) would each be a bad first week for a daily user.

## Findings by severity

Each line: id, `frozen` where it applies, status after refutation and referee, the claim. Every site, the evidence and the direction of fix are in the unit report named by the letter. Duplicates found by two units are merged under one id, with the other named.

### Blocker

None as a defect. F1 is listed under "What holds the tag" as S6's open work; as a defect it is medium (see below).

### High (4)

- **C1** (= G2), confirmed by a red test and its refuter.
  - **Claim:** with jars off for this spyc (`[pane] jars = false`, the jarless offer taken, or a build where `honoured()` is false), restoring a tab whose jar still runs starts its program a second time. `launch_tab_pane` returns `Pane::launch` at `src/app/pane_starts.rs:126-127` before it looks at `restoring`. A Claude tab also gets `/resume <id>` typed into it (`session_restore.rs:474`).
  - **Evidence:** red test `review_c_a_restored_tab_whose_jar_runs_is_not_started_again_with_jars_off` in `~/src/spyc.worktrees/review/3.0-c`. The refuter ran it twice and watched both `cat`s alive at once.
  - **More ways in (refute-G):** `:upgrade` from an rc into an installed release build, and `spyc attach` to a session an rc handed over.
- **C2**, confirmed in a narrower form.
  - **Claim:** `^a R` on any `[detached]` tab starts a second copy in a new jar, without asking. `needs_active_tab_confirm` (`src/app/pane_tabs.rs:41-54`) asks only when the pane isn't closed, and `restart_tab_now`'s `try_kill` (`src/pane/mod.rs:382`) reaches only the local child handle, not the jar's program. I re-read both.
  - **Wider than first reported:** this covers the `[detached]` that decision G intends (another spyc owns the jar), not only the jar silent for 40 s. A failed retake is also labelled `[jar lost]` while the jar runs (`pane_starts.rs:318-322`).
  - **Narrower:** the "tab stays `[detached]` after 40 s of silence" half is medium, since the jar's owner reader never waits on the program, so only a stopped or swapped-out jar falls silent that long (refute-C).
- **B1**, `frozen` (the upgrade half), confirmed on Linux and by the refuter's read.
  - **Claim:** every jar thread starts with `std::thread::spawn`, which panics when the OS refuses a thread. The referee counted the call sites with `grep -rn "thread::spawn" crates/spyc-jar/src`: 23 matches, of which `client.rs:458` is in a test module, so 22 in production. The crate's only `thread::Builder` is `client.rs:192`.
  - **Outcomes:**
    - On the acceptor: the jar exits 101 and the program dies.
    - On an owner's writer: `st.owner` is never cleared and every later owner is refused for good.
    - In the old image's upgrade path: `upgrade_claimed` or `upgrading` stays set for life.
- **F2**, decide before the first rc publishes; confirmed by the referee's read.
  - **Claim:** `crates/spyc-jar/Cargo.toml:3` is `0.1.0`, nothing in `release-prep` moves it, and `release.yml:263-279` publishes it on the first tag and skips it after. So `cargo install spyc` links whatever jar library the first tag carried, for every later release.
  - **Also wrong:** the workflow comment at `release.yml:227-229`, that rc publishes "can't poison the stable channel", is false for `spyc-jar`.

### Medium (31, plus C2's other half)

Frozen first:

- **A1**, `frozen`, confirmed (Linux probe, status -13 on the fifth try). After a failed upgrade `exec`, the jar no longer ignores `SIGPIPE`; a later upgrade whose holder exits early kills the jar and its program. macOS survives.
- **A2**, `frozen`, confirmed (probe: 30.03 s, reason 6). When the validator's proof line is late, and on `fork` or disk errors, the old image answers "rejected" (6) where the protocol promises "busy" (5). spyc then stops upgrades for the session (also C9).
- **G9** (= A11, which A rated low), `frozen` (text). `JAR_PROTOCOL.md:62-63`, `:759-761` and `crates/spyc-jar/src/cli.rs:15-19` say spyc runs a program without a jar when the jar fails. The code never does that unasked: A and G both traced every path. The texts are wrong.
- **F7**, `frozen` (Unit A's surface). Mutation M12, removing `SO_NOSIGPIPE` on macOS, failed no test.

Not frozen:

- **A5**, confirmed. A new image that can't receive its descriptors exits; the program dies and the owner hears nothing.
- **A8** (= F4), confirmed. `tests/jar_compat.rs` can't start any jar this crate builds (it hung 30 s against the pin's jar). Nothing sets `SPYC_JAR_OLD_BIN` and the spike branch its doc names is gone, so no released jar can become the fixture the plan promises.
- **B2**, confirmed. A jar's standard error goes to `/dev/null` and its exit status is discarded, so nothing records why a jar or a thread died.
- **B3** (with F5), confirmed. The jar tests go red under CPU load: 5 of 7 loaded runs on macOS and 4 of 31 on Linux, across 10 tests, mostly on wall-clock limits in the tests. F5 adds about 20 fixed waits that assume a state or assert an absence, against the plan's "no timing in tests". B13's one lost count line belongs here (see "Refuted").
- **B4**, confirmed by measurement. An idle jar wakes about 53 to 67 times a second on Linux (`strace -c`), and about 88 to 108 context switches a second on macOS, for its whole life. Battery cost is on the manual list.
- **B12**, confirmed on Linux, refuted on macOS. `Client::recv` treats `EINTR` as a lost connection, so stopping and continuing spyc on Linux drops every tab's link to its jar.
- **C2** (its 40 s half). A jar silent for about 40 s leaves its tab `[detached]` for the session, against decision G.
- **C3**, confirmed (probes). spyc does not notice a jar that holds its lock and answers nothing until socket buffers fill: 1193 s idle on macOS and 578 s on Linux, and nothing in 150 s of typing on macOS (lead V7).
- **C4**, confirmed. Input the jar refuses is dropped without a word, and a jarred tab loses the 8 MiB input bound.
- **C5**, confirmed (red test, with a passing control). A confirmed pipe of more than 16 MiB ends a jarred tab's input for good.
- **C6**, confirmed by reading. A task or capture whose lease lapses shows as exited while its program runs on, and is never taken back.
- **C7**, confirmed by reading. The reply gate is a 500 ms clock that starts when the tail is queued, not when it is parsed; with nothing to replay it stays shut until the program prints (lead V11, spyc's half).
- **D1**, confirmed (probe), high in its unit and medium after refutation.
  - **Claim:** `link_own` connects by the canonical socket path while the jar binds the given one (`src/pane/jar/session.rs:152-156` against `:231`). When only the given path fits `sun_path`, `^d` hands over to a spyc that can't link to its own jar. That spyc comes up under a new name and offers to discard tabs that are still running.
  - **Triggers:** a typed name of 28 to 35 characters on the default macOS `TMPDIR`; or, found by the refuter, any symlinked runtime directory whose real path is long, at any name length.
  - **Why medium:** `r` or `spyc -r` brings everything back. The risk is `d` at that prompt, which ends every agent with one key and no confirmation (`src/app/orphan_jars.rs:147-170`).
- **D2**, confirmed (probe). A longer name fails `^d` after `release_jars` has let every tab go: spyc exits to the shell, the tabs have no owner until `spyc -r`, and a lock file stays in `sessions/`.
- **D3**, confirmed (probe). `spyc attach` sends `SIGTERM` to the client pid it read before its prompt, without reading it again (lead V6). E's inventory found this is the only site that signals a pid from `describe`.
- **D4**, confirmed (probe). Any `0x1c` byte the attach client reads detaches it: a paste holding one is cut in half, and without the kitty protocol `^\` never reaches spyc (lead H6).
- **D5**, confirmed by reading. `^d` and `:upgrade` drop each tab's last status report (`save_released` skips `carry_live`), so an agent blocked on a prompt no longer shows as waiting.
- **E1**, confirmed (probe on a scratch spyc). One request with no token on the MCP socket, `spyc/shutdown` or `spyc/release`, ends spyc or leaves every tab ownerless, with no confirmation; `spyc/upgrade` and `spyc/statusline` are open the same way. The way in is `$SPYC_MCP_SOCK` from a shell, not the proxy. Inside the stated model (an injected agent running as the user can already `kill` spyc) this is medium, not high (lead V5).
- **E2**, confirmed (stand-in probe; the cross-user Linux case is argued from the code).
  - **Claim:** `spyc attach`, `spyc shutdown`, `spyc --jars` and the orphan finder never check the socket directory. `spyc attach` then sends the token and keystrokes to whatever socket is there, and `SIGTERM`s the `owner_pid` it names.
  - **Contradicts:** the plan's finding-14 claim that both sides check (lead H2).
- **E3** (= G5), confirmed by reading. Seven task and capture call sites send `kill(-pid)` straight to the pid the jar reported, against SPYC-TRAP `jar-signals-through-the-jar`.
- **E4**, confirmed. `SECURITY.md` says nothing about jars, tokens, the store, upgrades, attach or the control methods, and its line 37, "handles no secrets", is now false (lead V3).
- **F1**, confirmed as a mechanism; the release risk is refuted.
  - **Claim:** `honoured()` turns jars off in a `3.0.0` build: built as 3.0.0, 13 of 14 `jar_start` and 18 of 19 `jar_adopt` tests fail, and `^d` refuses. Six readers of the gate, plus a test that asserts the gated behaviour.
  - **Caught by:** the required `Tests` check, which blocks the release PR.
  - **The defect:** neither #1141 nor `RELEASE_ENGINEERING.md` names `honoured()`.
- **F3**, confirmed; high in its unit, medium after refutation. No test of any kind runs on macOS in CI. `macos.yml:1-10` documents the trade-off, and the owner's `make check-ci` is the only macOS test run. B3 shows that run is itself red under load 5 times in 7.
- **F6**, confirmed by mutation. Removing the jar's peer-user check failed no test.
- **G3**, confirmed. A plain `:setenv` value is written to the on-disk event log.
- **G4**, mechanism confirmed. Render's drain marks output as seen, so a program's OSC 7501 status report can sit unread until it prints again.
- **G6**, mechanism confirmed. A rebuild after restore that keeps the row number can move #1316's restored cursor to another file.
- **G10** (with D9), confirmed absence. No document says that logind's `KillUserProcesses` or WSL can end a detached session; whether they do is on the manual list (lead H7).

### Low (28)

`frozen` first:

- **A3**, `frozen`. The copy step opens any path: a FIFO makes every later upgrade "busy" for the jar's life, and refused copies are never removed. The disk-fill case is unverified.
- **A9**, `frozen`, unverified. A validator's children outlive every outcome but a timeout, and can keep a copy of the blob.

The rest:

- **A6**, confirmed. Resume has no deadline: a holder that ignores `SIGTERM` stalled the jar until it was killed by hand. This is new-image code, so a later release can fix it (lead H5).
- **A10**, unverified. `lock::keep` creates the lock file before locking it, so a client can briefly read a live jar as gone.
- **B5**, confirmed on Linux, refuted on macOS. A program whose descendant in another process group holds the PTY is never reported ended, and a capture's jar never ends (lead H1). Tabs without jars behave the same way for panes.
- **B6**, confirmed. Every jarred program inherits `SIGTTOU` ignored, and every jar keeps spyc's working directory from when the tab opened (lead H3; the signal mask, umask and descriptors are clean).
- **B7**, confirmed. The ring's limit counts output bytes only, so output written a byte at a time costs about 56 bytes of memory per byte held.
- **B9**, confirmed by reading. A `Replay` can miss its wake-up and delay first output by up to 500 ms.
- **B10**, confirmed. Jar tests leak processes past their scratch directory:
  - `a_holder_ends_when_the_jar_does` leaves a `sleep 300` on every run.
  - A `gated-jar` from an older `harness_tests::jar_start` run is still alive on the owner's Mac (pid 66123, parent 1, 8 days old; seen by Unit C and the referee, left untouched).
- **B11**, confirmed. `spyc attach` waits only 2 s for `describe`, but a parked jar can be silent for up to 15 s.
- **C8**, confirmed. The wait guard misdescribes some waits and misses jar waits in `src/app`.
- **C9**, confirmed by reading. One 2 s lookup after an upgrade can stop upgrades for the session.
- **C10**, unverified. The store sweep can delete an entry an older running spyc still uses.
- **C12**, confirmed. `[detached]` stands for four different states, on a blank screen.
- **D6**, confirmed by reading. `^d` and `:upgrade` let the jars go even when the session save failed.
- **D8**, confirmed. The attach client's reset misses the title pop and `>0p`, and no restore path writes `?2026l`.
- **D10**, unverified. A handover deletes running `!` captures' spill files.
- **E7**, confirmed by reading. A session's token file is never removed.
- **E8**, confirmed. Token files are written in place, and an existing `jar-tokens/` is trusted whatever its mode.
- **F8**, confirmed. `_deb` and `apt.yml` build a `.deb` without `spyc-jar` if the tarball lacks it.
- **F9**, confirmed. `RELEASE_ENGINEERING.md` is stale and has no 3.0 jar steps.
- **G1**, high in its unit, low after refutation. The user pages and config template call jars a development-build feature, which is true at the pin; they are four sentences for S6 to update.
- **G7**, confirmed. The crash message says the report names none of the user's files, but the report keeps them.
- **G8**, mixed. Twenty smaller code findings, among them a possible endless loop in `issue_url`, a raw terminal after a panic in `spyc attach`, and `^\` waiting up to 2 s on the loop.
- **G11**, confirmed. `ROADMAP.md` dates the freeze 2026-11-12, names a nonexistent `spyc -a`, and leaves the jar plans out of its doc map.
- **G12**, confirmed. `shell.md` still says `^D` quits.
- **G13**, confirmed. `README.md` and the uninstall note say nothing of jars.
- **G14**, partly confirmed. 17 false or stale sentences in trap rationales, and `release_session` lacks its anchor.

### Informational (15)

`frozen` first:

- **A4**, `frozen`, by design inside the model (lead V10). A reader holding the token can make the jar run any file, with the whole state on its standard input.
- **E6**, `frozen`. Owner and reader share one token and a reader may ask for an upgrade, so no version-1 jar can ever have a role that holds less than the owner (U6's read-only watcher included) without an upgrade first.
- **E9**, `frozen`. The inventory of what a tokenless `describe` returns, and what each caller does with it.
- **E10**, `frozen`. Any process holding a lock on a jar's `.lock` makes a gone jar look alive.
- **A13**, `frozen` in part. Smaller errors in the specification's prose, and `JAR_UPGRADE_BRIEF.md` is stale with its §5 still open.

The rest:

- **A7**. After an upgrade, the lock's descriptor is inherited by validators and holders; harmless to the lock.
- **A12**. Fuzzing: `jar_frame` 50,055,813 runs and `jar_handover` 71,384,166 runs, 960 s each, clean. Neither target reaches `encode`, the start on standard input or the proof line, and `jar_handover`'s round trip only checks that the output decodes again.
- **B8**. One lock for the whole log: five readers cut the owner's throughput by about 60 per cent; a full-ring replay to a new reader did not stall it (lead H4).
- **C11**. Smaller clock verdicts: `BUILD_INFO_WAIT`, the 5 s welcome wait during a start, `DISCARD_WAIT`.
- **C13**. Two key handlers do OS work directly instead of returning an `Effect`.
- **D7**. spyc never gives a jar a new token, though the protocol and plan say a move does (lead H8 is moot: there is no rotation to order).
- **E5**. `spyc attach` over SSH gives every tab the attaching terminal's SSH agent.
- **F10**. No size-ratchet pin was added since `v2.4.0`, and two were lowered.
- **F11**. The tag is built and published with no test run of its own.
- **G15**. Stale comments and HUD docs.

E11 (the crash report's security) found nothing.

### Refuted

- **B13** (one count line lost across an upgrade, under load). Nothing loses PTY bytes between the park and the resume. The refuter reproduced the failure with no jar at all: the terminal echoed typed input into the middle of a count line, 8 times in about 3,500 lines under load, and never with `stty -echo`. What's left is a test that fails under load, filed under B3.

## Where reviewers overlapped or disagreed

- **The same defect found twice:** C1 and G2; E3 and G5; A8 and F4; A11 and G9; D9 and G10; B3 and F5. In each case the ids agree on the mechanism, and the merge keeps the higher severity.
- **Severity, unit against refuter (the referee's call):**
  - F1: blocker → medium, with S6 as a tag prerequisite.
  - D1: high → medium.
  - F3: high → medium.
  - G1: high → low.
  - C2: split into a high and a medium half.
- **Wider than reported:**
  - B1 was found in the frozen upgrade path too (refute-B).
  - D1 holds at any name length under a long canonical runtime directory (refute-D).
  - C1 has two more ways in (refute-G).
- **Disagreement left standing:** A rated the three jar-or-nothing texts low; G rated them medium because two are in the specification that freezes. The referee takes medium.
- **B5 and H1:** B found the PTY outliving the program only on Linux and only with a descendant in another process group. That matches today's behaviour of tabs without jars for panes, but not for captures, whose jar never ends.

## Premises of the brief that were wrong

- **V1**, "the final tag ships the one configuration no rc ran": refuted. The required `Tests` check blocks a `3.0.0` with the gate in, and removing the gate changes no path an rc runs.
- **V2**, "`keep_socket` and the backlog refusal are macOS-specific": wrong. The only `target_os` code in the jar is in `accept.rs` (F). The lead's main claim, no macOS tests in CI, holds.
- **H8**, "check the new token is durable before the jar is told it": there is no token rotation at all (D7). The real fault is in the documents.
- **H4**, "the log under one lock": a throughput cost, but no stall (B8).
- **V4**: the texts are wrong, and the code never falls back unasked on any path traced.
- **H5**: confirmed, but it is new-image code, so it is not frozen.

## What was not reviewed

The units' "Not covered" sections have the detail. In summary:

- **Platforms:** nothing on x86_64 glibc (CI's platform), WSL or a systemd-logind host. Thread exhaustion (B1) was shown on Linux only. Every measurement used debug builds.
- **Tests:** the property tests and `tests/jar_process/{contain,lock,ownership,timers}.rs` (A); test bodies of `tests/jar_attach.rs`, `src/state/sessions/tests.rs` and `session_restore.rs` from line 560 (D); `jar_adopt.rs` and `src/pane/jar/attach.rs` were skimmed (C).
- **Sessions:** no kill -9 at a chosen instant of a handover was run. Unit D's crash-ordering claims, that a crash loses no jar and starts no program twice, rest on closed reasoning from the code.
- **Security:** `src/mcp/readers.rs`, `hooks.rs`, `config.rs` beyond their diffs, and `src/wiring/` unchanged bodies (E); `audit`, `codeql`, `shipped-targets`, `vendored-archives`, `cache-cleanup` and `secrets` workflows (F; `secrets.yml` was refused by permissions).
- **Guards:** `style_guard.rs` and `link_guard.rs` were skimmed, not checked for guards that check less than they claim (F).
- **Build:** `build.rs`'s `rerun-if-changed=.git/HEAD` names a path that doesn't exist in a linked worktree; whether that gives two builds at one commit different `SPYC_BUILD_TIME`s, which the store reads, was not measured (F).
- **Unit G's code:** `spikes/`, most of `scripts/`, `docs/COMPETITIVE_REVIEW.md` and `docs/BRAND.md`; the brand sweep was skimmed as diffs. G8's "unverified" items were read by a sub-reviewer only.
- **Mediums with no refuter:** only blockers and highs had one. The 32 mediums carry their own unit's evidence, and most were run, not only read.

## Evidence left behind (uncommitted)

- Red tests:
  - `~/src/spyc.worktrees/review/3.0-c`: C1, C5 and its control.
  - `~/src/spyc.worktrees/review/3.0-b/tests/review_b_probes.rs`: B's probes.
- Fuzz corpus grown by A's runs: `~/src/spyc.worktrees/review/3.0-a`.
- Unit F's worktree `~/src/spyc.worktrees/review/3.0-f` is clean (every mutation reverted); its logs are in the scratch `F/` directory.
- Probe scripts and logs: `<scratchpad>/`, in `unit-a/`, `B/`, `unit-c/`, `unit-d/`, `unit-e/`, `F/` and `refute-*/`. That directory is pruned after a few days: copy what you want to keep.
- Linux build volume: Podman `spyc-review-home` (image `localhost/spyc-review-linux`).

## Manual checklist

See `MANUAL.md`: twelve probes that need a real terminal, a Linux login host, WSL, a laptop lid or `sudo`, each naming the finding it settles.
