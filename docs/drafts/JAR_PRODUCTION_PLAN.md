# The jar in production: a plan

**Status:** proposed, for the owner to change.
**Tracks:** [#1099](https://github.com/Tripstack-Corp/spyc/issues/1099), on
the `3.0` milestone.
**Builds:** [`JAR_PROTOCOL.md`](JAR_PROTOCOL.md), whose version 1 froze on
2026-11-12, from what [`PANE_JAR_SPIKE.md`](PANE_JAR_SPIKE.md) proved.

The spike built every v1 verdict on `spike/jar-stage-2`, a branch that never
merges: about 8,000 lines over 48 files, measured on macOS and Linux. Production
rebuilds it on `main` in six slices, each ending where the others do: a build
the owner tests, then the merge, then the next slice. The spike's code is the
reference, ported a module at a time, not merged; its harness results are the
acceptance bar, carried into in-repo tests.

---

## The slices

### S1. The crate

`crates/spyc-jar`, ported from the spike at `9bec55f2`: the frame and message
codec, the handover blob, the jar process, its PTY, the upgrade, the client and
the attach client. Nothing in spyc uses it yet.

- **Two PRs.** S1a: the parsers, with `tests/wire_v1.rs` (the frozen bytes) and
  the `jar_frame` and `jar_handover` fuzz targets registered with `make fuzz`
  and CI's fuzz job. S1b: the jar process and the upgrade, with the spike's
  integration and compatibility tests.
- **The guards the spike skipped.** Its `jar.rs` (1,094 lines) splits under
  the 1,000-line cap along its seams: accepting a connection and its `hello`,
  a connection's loop, the log and its ring, describing. Functions over 100
  lines split; every module opens with its doc; `#![forbid(unsafe_code)]`
  stays.
- **No timing in tests.** The spike found two tests that waited a fixed time
  and failed under load; every wait polls for the state it needs.
- **Packaging.** spyc's package gains a `spyc-jar` `[[bin]]` that calls the
  crate, so `cargo install spyc` installs both binaries (decision 2), and
  `make package-check` builds both from their tarballs.

**The owner tests:** nothing visible; the review is the test.

### S2. A pane in a jar, opt-in

With `[pane] jars = true` (decision 1), every program spyc hosts runs in a
jar: pane tabs, tasks and `!` captures. Off, spyc runs exactly as today.

- **S2a: spawning and adopting.** `src/pane/jar_backend.rs` (1,019 lines on
  the spike) splits into spawning and tokens (R26: 32 random bytes on the
  jar's standard input, kept `0600`), the reader thread, snapshots, and listing
  and sweeping. The engine's snapshot code lands beside the engine, and every
  pane keeps both snapshot forms (decision I). After a `kill -9` of spyc,
  `spyc -r` adopts the jars and replays what each program printed meanwhile.
- **S2b: losing and retaking the lease.** A spyc stopped past the deadline
  takes its jar back on resuming, or marks the tab `[detached]` when another
  spyc owns it (decision G). A killed jar marks its tab `[jar lost]`. On Linux
  that label waited for the next key on the spike: a reader that ends must
  wake the loop.
- **S2c: tasks, captures and orphans.** A demoted pane keeps its jar, its
  owner rewriting `is` to `task`; a capture lingers a minute. `spyc --jars`
  lists jars by label (R18). A spyc that starts and finds jars no spyc owns
  offers to restore or discard them, where the spike only flashed a notice.
- **The release gate.** A release with `[pane] jars` in it ships `spyc-jar` in
  its archives, the Homebrew formula and the apt package.
- **Tests,** in the repository, not the spike's harness: a `kill -9` of spyc
  and `spyc -r` adopting a running program, driven through the binary as
  `tests/mcp_proxy.rs` drives it, with a fake agent; the lease taken back and
  refused; a killed jar's label. Test jars live under a test's own state and
  runtime directories, never the user's.

**The owner tests:** `[pane] jars = true` day to day; `kill -9` spyc mid-turn,
then `spyc -r`, and the agent is still working.

### S3. Upgrades

A spyc that starts and finds its jars on an older build asks each to upgrade
to the `spyc-jar` beside it (R33): same PID, same program, same connections.
The `jar` key of a description names the build.

- **Every released jar stays a test fixture.** CI keeps the jar of each
  release since the first that shipped one, and runs it against the current
  spyc. S3 seeds that set with the first.
- **A jar outlives the binary it came from.** An uninstall or an upgrade
  replaces the file; a running jar keeps its own copy until its program ends,
  or until the next spyc upgrades it.

**The owner tests:** start agents on build A, install build B, start spyc: the
agents run on, and `spyc --jars` shows B.

### S4. spyc outlives its terminal

Shape 2, entered lazily (decision B): a bare launch stays a plain spyc, and the
first `^d` with something running hands spyc to a spyc in an outer jar,
through the manifest and pane-jar adoption. The spike proved the shape with
spyc started in an outer jar by its harness; the lazy entry is new.

- **Detaching and attaching.** `^d` and `:detach` detach only when an agent, a
  tab or a task is running; with nothing running, `^d` quits as it does today.
  `spyc attach [name]` reattaches. `^D`, `Q`, `ZZ` and `:q` quit and end this
  spyc's jars.
- **The attach event** sets spyc's modes, probes the terminal and repaints in
  full, on a terminal that may be another emulator. Version 1 carries it as
  the spike did: the jar signals its program, and the client changes the size
  by a row and back.
- **The detach reset** leaves the real terminal as it found it: the alternate
  screen, mouse reporting, bracketed paste, the kitty keyboard flags (#696,
  #710).
- **No reader outlives its probe.** Stage 2b found the image-protocol probe's
  reader still blocked on standard input after its timeout, taking the first
  keys a later terminal sent. Probes move to the attach, each reader ending
  with its probe, and an audit confirms spyc never waits on a terminal's
  reply.
- **spyc upgrades inside its outer jar** by `exec`ing its new binary: the
  same PTY, the client untouched, the Model rebuilt from the manifest. An
  audit of the manifest confirms every field serializes or is rebuilt.
- **Docs:** a feature page for detaching and attaching, `docs/KEYBINDINGS.md`
  and the help overlay for `^d`, and `:detach` and `:attach` for users.

**The owner tests:** `^d` with agents running, close the terminal, open
another, `spyc attach`: the same spyc, its agents mid-turn, its modes set
again. Then an SSH connection killed while attached.

### S5. Projects and tabs move

`:detach project [name]`, `:detach tab [n]`, `:attach` with a list of what is
detached, `:attach project` and `:attach tab`. A detached project's jars go to
a spyc in an outer jar, which ends once it owns nothing.

- **The move file** carries the project, its jars' tokens, its scope claims,
  each tab's live status and a shell tab's running agent, and is kept until
  the receiving spyc's session is saved (stage 6), so a spyc that dies
  partway loses nothing.
- **An agent's MCP follows its jar** (R30a): its proxy asks the jar, as a
  lookup, where its owner is now (decision J).
- **Tests:** the spike's `scopes.py` and `attn.py` become in-repo tests: a
  claim and a blocked agent cross a move, and the receiving spyc's `Space !`
  goes to the agent.

**The owner tests:** detach a project with a blocked agent from one terminal,
attach it in another; answer the agent there.

### S6. On by default

After a release of soak with `[pane] jars = true` among daily users, the
default flips, and that release is 3.0. `[pane] jars = false` stays as the way
back (decision 1).

---

## Across every slice

- **ARCHITECTURE.md** takes the decisions as they land, and a `SPYC-TRAP` for
  each silent failure the spike found: the holder process that keeps an
  upgrade's sockets alive on macOS, the token never in argv or the
  environment, a probe's reader outliving its timeout.
- **AGENTS.md** takes an index line for each new module and the crate.
- **`make lint-linux`** for the platform code (the peer check, the `memfd`).
- **No CI flakes:** each jar test ends its program's process group, polls
  rather than sleeps, and runs on both CI platforms.
- **The spike branch stays** until S5 merges, as the reference, and is deleted
  then.

**Not in this plan:** sharing and the gateway (U15), the keyboard lease and
taking over (decision D), `block` and `spill` (R12b), and spyc not repainting
after an outside `SIGCONT` until a key arrives, which happens without jars too
and is its own issue.

## Decisions for the owner

1. **Opt-in, then on by default.** Proposed: `[pane] jars`, off until a release
   of soak, on in 3.0, kept as the way back. The alternative, on from the first
   release that ships it, gives daily users no way around a bug.
2. **One package, two binaries.** Proposed: spyc's package installs
   `spyc-jar` through a `[[bin]]` that calls the crate, so every install path
   (`cargo install`, Homebrew, apt) carries it without a second crate to
   install. The jar's crate is still published for spyc to depend on.
3. **One issue per slice,** on the `3.0` milestone and linked from #1099, so
   each PR closes its slice.
4. **The spike branch is only on this machine.** Proposed: push it to
   `origin` as a reference branch, after a check that nothing of the owner's
   is in its harness, so a lost disk doesn't take the reference with it.
