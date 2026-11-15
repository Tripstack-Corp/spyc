# Brief: the jar's upgrade step, before version 1 freezes again

**For:** the session building the jar (`JAR_PRODUCTION_PLAN.md`).
**Why:** version 1 reopened on 2026-11-15 for the upgrade step
(`JAR_PROTOCOL.md`, decisions L and M). Read "Upgrading: where each check
runs" there and "The upgrade step" in the plan first; this brief says what to
build and in what order, not why.
**Checked against:** `main` at `be018db6`. Line numbers are that commit's;
the jar session's `jar/s2a-spawn` branch isn't on `main` and wasn't read
except where it says so.

Sort every change by where its code runs: the **old image** (fixed in the
first release, so all of it lands before that release), the **new image**
(ships with the first release an upgrade targets), or **spyc** (whenever).

No frame's bytes change and the blob's layout doesn't. `spyc-jar` keeps
`#![forbid(unsafe_code)]`. If any item below turns out to need a frame or a
blob change, stop and report.

## 1. Red tests first

Each in `tests/jar_process.rs`, beside
`a_failed_exec_leaves_a_jar_that_survives_the_next_refusal` (`:1412`), and
each watched failing for the reason it names before any fix lands:

1. **`fake`:** a binary whose `validate-handover` exits 0 and whose
   `hold-handover` is the real jar's, but whose `resume-handover` exits 1.
   Today the jar is lost, the program gets `SIGHUP`, and a reader sees
   end-of-file only after the holder's 30 seconds (`HOLD_LIMIT`,
   `crates/spyc-jar/src/upgrade.rs:58`). The test asserts the program
   survives.
2. **`swap`:** a binary that delegates to the real jar but, during
   `hold-handover`, replaces the file at its own path. Today the jar `exec`s a
   file it never validated: the path is looked up at `upgrade.rs:390`
   (validate), `:248` (hold) and `:202` (the `exec`). The test asserts the
   program survives and the jar runs the image it validated.
3. **A validator that exits 0 and prints nothing.** Today that is accepted
   (`validate`, `:387`, checks the exit status only, with standard output at
   `Stdio::null()`, `:393`). The test asserts reason 6 and the jar running on.
4. **A validator whose child holds its standard error.** The validator exits,
   and its child keeps standard error open: today `validate` reads it to the
   end after the exit (`:417`), outside the deadline, with every thread
   parked, so the jar's program stalls for as long as the child lives.
5. **A validator whose child outlives the timeout.** Today the deadline kills
   only the direct child (`:408`). The test asserts the validator's whole
   process group is gone.
6. **A reader that trickles during the flush.** `flushed` sets a write
   timeout of `FLUSH_DEADLINE` for each write (`:226`), so a reader taking a
   byte every 0.9 s keeps the jar parked indefinitely. The test asserts one
   deadline for the whole flush.

## 2. The old image's parts, and the store

The seven parts, as `JAR_PROTOCOL.md` → "Upgrading: where each check runs"
numbers them:

1. **One copy.** `upgrade` (`crates/spyc-jar/src/jar/conn.rs:161`, then
   `upgrade::begin`) copies the named binary into the jar's store once,
   then validates, holds and `exec`s that file. Refuse a relative path with
   reason 8.
2. **The proof line** on the validator's standard output, as "The upgrade's
   command lines" specifies; exiting 0 without it is reason 6. No new
   dependency for it.
3. **`resume-handover PREVIOUS`:** the old image passes the store entry it
   runs from. `cli.rs:36` dispatches on the first argument only, so a resume
   ignores it until part 3 of the new image uses it.
4. **The validator's standard error** read under the same deadline as its
   exit, and the validator started in its own process group (std's
   `CommandExt::process_group`, no `unsafe`) so a timeout kills the group.
5. **One flush deadline** for the whole flush.
6. **`build` in `describe`** (`crates/spyc-jar/src/jar/describe.rs:19-25`
   has `jar` as the version and `current_exe()`): the SHA-256 of the image the
   jar started from. `upgraded` carries the path today
   (`crates/spyc-jar/src/jar/mod.rs:268-270`) where the protocol's table says
   "the new build": send the `build`.
7. **Validate twice:** once unparked, on a blob taken then, to pay a new
   binary's first-run cost (about 263 ms on macOS) and refuse a non-jar while
   the program is still served; then again, parked, on the real blob.

**The store** ("The jar binary store" in the plan) is spyc's side and the
jar's together:

- spyc: copy and check at start (`spyc-jar build-info`, a new subcommand of
  `src/bin/spyc-jar.rs`, printing `SPYC_GIT_SHA` and `SPYC_BUILD_TIME`), the
  sweep, and the fallback when the state directory can't execute.
- The jar: its store is the directory two levels above the entry it runs
  from; a jar outside a store keeps upgrade copies in a `0700` directory
  beside its socket.
- **S2a starts every jar from the store.** On `jar/s2a-spawn`,
  `src/pane/jar/place.rs` sets `Place::binary` to the `spyc-jar` beside
  spyc's own binary: that is what changes.
- The SHA-256: `sha2` (0.10, already in the lockfile) would be a new
  dependency of `spyc-jar`, which decision E lists. Ask the owner before
  adding it.

## 3. `--start-on-owner`

A pane's jar starts its program when its first owner's `hello` is accepted
(decision M); without that owner within the owner's deadline
(`--deadline-ms`, 10 s by default) it exits having started nothing. Today the
program is spawned as `jar::run` begins (`crates/spyc-jar/src/jar/mod.rs:197`).
It is a flag after `--wire`, which isn't frozen; spyc's outer jar never passes
it. Refuse an upgrade (reason 5) until the program has started, so the blob
always has a program pid. The program still starts through the jar's own
image (`spyc-jar pty-child`, `crates/spyc-jar/src/pty.rs:46`), which the
store keeps unchanged for the wait.

## 4. The new image's parts

In order, prevent, contain, recover:

- **Prevent:** the store and the proof line (section 2).
- **Contain:** `catch_unwind` around decoding the blob and starting the jar
  (`upgrade::resume`, `crates/spyc-jar/src/upgrade.rs:439`, and
  `jar::start`); a held mode that keeps the PTY open and drained and its
  connections served when the jar can't resume; ending the holder at once
  rather than after `HOLD_LIMIT`.
- **Recover, last:** the switch back to `PREVIOUS`. Only its argument ships
  now (section 2, part 3).

## 5. Tests that make it stay fixed

- **A real upgrade among the property sequences.** Today the sequences' only
  upgrade names `/nonexistent/spyc-jar` (`tests/jar_process.rs:2121`).
- **An upgrade after every N steps** of the `Jar` harness's scenarios, with a
  skip list that gives each skip's reason.
- **Released jars as CI fixtures** for `tests/jar_compat.rs`, whose one test
  is `#[ignore]`d (`:43`) and so never runs; a missing fixture is reported as
  a skip, never a silent pass.
- **`=` pins:** `Cargo.toml:140` has `spyc-jar = { version = "0.1.0", … }`, a
  caret requirement, so `cargo install spyc` can build a jar from a newer
  `spyc-jar` than the release was tested with.
- **A guard for the user's real jars:** no test may start, reach or remove a
  jar in the user's socket or state directories; each test sets its own.
- **The jar tests on macOS in CI.** The macOS job runs lint only
  (`.github/workflows/ci.yml`, "Lint macOS"); the tests run on Ubuntu, while
  the upgrade's riskiest behaviour (the holder, `exec` of `/dev/fd/N` refused,
  the image with no name) is macOS's.

## Report back

What you verified and how, what on `main` differed from this brief, what you
changed from it and why, and what is still open.
