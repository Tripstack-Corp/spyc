# The jar in production: a plan

**Status:** approved on 2026-11-12, after an external review revised it
("The review's findings, slice by slice", below); revised on 2026-11-15 for
the upgrade step ("The upgrade step", below). The owner's decisions are at
the end.
**Tracks:** [#1099](https://github.com/Tripstack-Corp/spyc/issues/1099), on
the `3.0` milestone.
**Builds:** [`JAR_PROTOCOL.md`](JAR_PROTOCOL.md), whose version 1 froze on
2026-11-12, froze again the same day with the review's changes, and reopened
on 2026-11-15 for the upgrade step (its decision L), from what
[`PANE_JAR_SPIKE.md`](PANE_JAR_SPIKE.md) proved.

The spike built every v1 verdict on `spike/jar-stage-2`, a branch that never
merges: about 8,000 lines over 48 files, measured on macOS and Linux. Production
rebuilds it on `main` in six slices, each ending where the others do: a build
the owner tests, then the merge, then the next slice. The spike's code is the
reference, ported a module at a time, not merged; its harness results are the
acceptance bar, carried into in-repo tests.

---

## The slices

### S1. The crate

`crates/spyc-jar`, ported from the spike at `670f5b04` (version 1 as it froze
again after the review): the frame and message codec, the handover blob, the jar process,
its PTY, the upgrade, the client and the attach client. Nothing in spyc uses it
yet.

- **Two PRs.** S1a: the parsers, with `tests/wire_v1.rs` (the frozen bytes) and
  the `jar_frame` and `jar_handover` fuzz targets registered with `make fuzz`
  and CI's fuzz job. S1b: the jar process and the upgrade, with the spike's
  integration and compatibility tests.
- **The guards the spike skipped.** Its `jar.rs` (1,094 lines) splits under
  the 1,000-line cap along its seams: accepting a connection and its `hello`,
  a connection's loop, the log and its ring, describing. Functions over 100
  lines split; every module opens with its doc; `#![forbid(unsafe_code)]`
  stays.
- **No lock across a blocking write** (review finding 4). The spike's jar
  wedges three ways: an owner gone silent while its program prints keeps the
  lease past its deadline, a reader that stops reading freezes a `release`,
  and a paste into a program that reads nothing blocks its connection. Each
  connection gets a bounded outbound queue and a write timeout, a socket is
  shut down through a cloned handle, and input is written to the PTY off the
  connection's reader thread.
- **Tokenless connections can't hold the jar** (finding 5): a deadline for a
  whole frame, not each read; lookups capped in number, never parked for an
  upgrade nor carried across one; one duplicated descriptor fewer for each
  connection.
- **Lifetimes from the right moment:** an uncollected exit is kept from the
  exit, not from when the owner left (16); a `release` grants a grace before a
  linger starts, so a pane that doesn't linger survives a handover (17); a
  window must be smaller than the ring (18). A signalled exit reports its
  signal; a reader that falls behind gets a `gap`; no input is ever traced to
  a file.
- **The review's probes become tests,** asserting, each watched failing
  before its fix lands.
- **No timing in tests.** The spike found two tests that waited a fixed time
  and failed under load; every wait polls for the state it needs. The property
  tests `JAR_PROTOCOL.md` promises under "Fuzzing" (no sequence lets a reader
  type, takes a lease without the token, or answers a replayed query) land
  here.
- **Packaging.** spyc's package gains a `spyc-jar` `[[bin]]` that calls the
  crate, so `cargo install spyc` installs both binaries (decision 2), and
  `make package-check` builds both from their tarballs.

**The owner tests:** nothing visible; the review is the test.

### S2. A pane in a jar, opt-in on `-CURRENT`

With `[pane] jars = true` (decision 1), every program spyc hosts runs in a
jar: pane tabs, tasks and `!` captures. Off, spyc runs exactly as today. Until
S6 only a `-CURRENT` or rc build honours the setting, so a release cut while
the slices land runs as today.

- **S2a: spawning and adopting.** `src/pane/jar_backend.rs` (1,019 lines on
  the spike) splits into spawning and tokens (R26: 32 random bytes on the
  jar's standard input, kept `0600`), the reader thread, snapshots, and listing
  and sweeping. The engine's snapshot code lands beside the engine, and every
  pane keeps both snapshot forms (decision I). After a `kill -9` of spyc,
  `spyc -r` adopts the jars and replays what each program printed meanwhile.
  From the review:
  - a jar that exits with 75 (`--wire` not spoken), isn't there, or doesn't
    answer fails the tab, which says why: with jars on, a program runs in a jar
    or not at all (finding 1; owner's decision 6);
  - a refused adoption marks the tab `[detached]` and starts nothing, where the
    spike started a second copy of the program (7);
  - a snapshot is stored at the jar offset of the last chunk parsed, so a gap
    in mid-session doesn't shift every later one (8);
  - every signal goes through the jar, never to a pid spyc was told (10);
  - an agent's proxy finds its jar by `SPYC_JAR_SOCKET`, never by working the
    path out again; with neither `XDG_RUNTIME_DIR` nor `TMPDIR`, jars live in
    `/tmp/spyc-<uid>`, and both sides check the directory is this user's,
    `0700` and not a link (14);
  - spyc's sender holds no lock across a socket write, and pings run on their
    own timer (4, spyc's side);
  - every jar starts from the store ("The jar binary store", below), never
    from the `spyc-jar` beside spyc, and a pane's jar starts with
    `--start-on-owner` (`JAR_PROTOCOL.md`, decision M).
- **S2b: losing and retaking the lease.** A spyc stopped past the deadline
  takes its jar back on resuming, or marks the tab `[detached]` when another
  spyc owns it (decision G). A killed jar marks its tab `[jar lost]`. On Linux
  that label waited for the next key on the spike: a reader that ends must
  wake the loop. The repaint after a gap holds the size a row off until
  output arrives or about 100 ms pass, then restores it: sent back to back,
  the two sizes reached a Node program on macOS in none of 40 trials
  (finding 9), and the fix is shown on macOS before it lands. Built: the
  retake (`app::pane_starts`), which takes a tab whose jar ended its lease
  back on a worker, the same program, from the jar's snapshot and replay,
  and wakes the loop when a reader ends. Not built: the repaint after a gap.
- **S2c: tasks, captures and orphans.** A demoted pane keeps its jar, its
  owner rewriting `is` to `task`; a capture lingers a minute. `spyc --jars`
  lists jars by label (R18). A spyc that starts and finds jars no spyc owns
  offers to restore or discard them, where the spike only flashed a notice.
- **The release gate.** The release that turns jars on (S6) ships `spyc-jar`
  in its archives, the Homebrew formula and the apt package, and **carries S3
  too** (finding 13): the format is in the field from the first release that
  ships a jar, so what consumes R33 is proven before it, not after. And jars
  aren't turned on, even as an opt-in, until a resumed spyc takes its tabs
  back (S2b; owner's decision 7): a spyc stopped past the owner's deadline
  otherwise leaves every jarred tab `[detached]` for the session, which is
  correct by the protocol and not acceptable in use. The retake is built,
  tested end to end by `a_spyc_stopped_past_the_deadline_takes_its_tabs_back`.
- **Tests,** in the repository, not the spike's harness: a `kill -9` of spyc
  and `spyc -r` adopting a running program, driven through the binary as
  `tests/mcp_proxy.rs` drives it, with a fake agent; the lease taken back and
  refused; a killed jar's label. Test jars live under a test's own state and
  runtime directories, never the user's.

**The owner tests:** `[pane] jars = true` day to day; `kill -9` spyc mid-turn,
then `spyc -r`, and the agent is still working.

### S3. Upgrades

A spyc upgrades a jar it owns when it needs something the jar's version lacks
(R33): same PID, same program, same connections. The `jar` key of a
description names the build. Not at every start (finding 13): an upgrade is the
design's one step with no rollback, so it runs only when it buys something.

- **One jar first.** spyc upgrades one idle jar, confirms it resumed, and only
  then the rest; never a jar whose agent is mid-turn, and never one another
  spyc owns.
- **A jar built to differ.** Before the first release with jars, CI builds a
  jar that differs in its blob version, its `hello` and its unfrozen frames,
  and runs upgrades and `hello`s against it in both directions, a downgrade
  included ("Meeting another version" in `JAR_PROTOCOL.md`).
- **Every released jar stays a test fixture.** CI keeps the jar of each
  release since the first that shipped one, and runs it against the current
  spyc.
- **A jar outlives the binary it came from.** An uninstall or an upgrade
  replaces the file, and on macOS a running jar can then name nothing that
  holds its own image (measured: "The upgrade step", below). It runs from its
  store entry, which no install touches, until its program ends or the next
  spyc upgrades it.

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
  full, on a terminal that may be another emulator. spyc hears it as the
  outer jar's program (role `program`), which version 1 carries;
  it hears `detached` too, and stops drawing for nobody.
- **spyc detaches itself.** `^d` and `:detach` write spyc's own restore
  sequence and then send `detach`, so the attach client's `^\` and its fixed
  reset are only the fallback, and `^\` can reach a program inside spyc.
- **A second attach takes over** (finding 12): `spyc attach` while a client
  is attached ends the old client, by the `owner_pid` the jar describes, a
  local process of the same user. Attached at a desk, then at home over SSH,
  works.
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
  partway loses nothing. It is written and `fsync`ed **before** any jar is
  released, and a detach that can't write it stops there; it keeps each old
  token until the receiving spyc confirms, which tries the new token, then the
  old (finding 6: the spike released first, so a sending spyc dying between
  the two stranded every jar).
- **An agent's MCP follows its jar** (R30a): its proxy asks the jar, as a
  lookup, where its owner is now (decision J).
- **Tests:** the spike's `scopes.py` and `attn.py` become in-repo tests: a
  claim and a blocked agent cross a move, and the receiving spyc's `Space !`
  goes to the agent.

**The owner tests:** detach a project with a blocked agent from one terminal,
attach it in another; answer the agent there.

### S6. On by default

Once S2–S5 have merged, the default flips on `-CURRENT`, and daily users run
it there, through rc builds, until the owner calls it ready. The first release
with jars has them on, and that release is 3.0. `[pane] jars = false` stays as
the way back (decision 1).

---

## The upgrade step

A review of vtmux's live upgrade led to probes of `main`'s jar (`be018db6`)
on macOS, which found two ways an upgrade ends the program it exists to keep
(`JAR_PROTOCOL.md`, decision L). The fixes are sorted by where their code
runs ("Upgrading: where each check runs" there): eight parts run in the old
image and ship in the first release that ships a jar; the new image's parts
ship with the first release an upgrade targets; spyc's ship whenever.
[`JAR_UPGRADE_BRIEF.md`](JAR_UPGRADE_BRIEF.md) is the brief for building
them.

### What was measured

On scratch sockets and directories, never a user's jars, with the release
`spyc-jar` built from `be018db6`:

- **macOS: a jar can't name its own image once it's replaced.** A jar started
  from a path, then the file replaced by `install` (a new inode): the jar
  ran on, and its `current_exe()` (as `describe`'s `jar` shows it), the
  kernel's `proc_pidpath` and `lsof`'s name all gave the path, which now held
  the new file. The jar's image was an inode with no name. Replaced in place
  by `cp` instead (the same inode, new contents): the jar ran on, and its
  image's file held the new contents. Either way a first upgrade has nothing
  to switch back to.
- **Linux: it can.** In a container on Linux 6.19 (Fedora 43, arm64, under
  podman on macOS), the same jar built for musl, its
  file replaced by `install`: `/proc/PID/exe` read `… (deleted)`, and copying
  it gave the original image byte for byte. A `cp` onto the running file was
  refused with `ETXTBSY`.
- **A new copy's first `exec`.** Three byte-identical copies of `spyc-jar` in
  fresh directories, each run three times: on macOS about 263 ms the first
  time (every new copy pays it) and about 6 ms after; on Linux about 1.6 ms,
  then 0.6 ms.
- **Where things live** (the socket directory as S2a's `src/pane/jar/place.rs`
  sets it, not yet on `main`; the state directory as `state::state_root`
  sets it):

  | | Sockets | State |
  |---|---|---|
  | macOS | `$TMPDIR/spyc/jars`, as `XDG_RUNTIME_DIR` is unset: `/var/folders/…/T/`, which the system cleans | `~/.local/state/spyc` (or `$XDG_STATE_HOME/spyc`) |
  | Linux | `$XDG_RUNTIME_DIR/spyc/jars`, `/run/user/UID`: a tmpfs, `nosuid,nodev`, not `noexec` on the host checked | the same as macOS |
  | either, with neither variable | `/tmp/spyc-UID/jars` | — |

### The jar binary store

spyc keeps the jar binaries it starts in `jar-bin/` under its state directory,
one entry per binary, named by its SHA-256: `jar-bin/<sha256>/spyc-jar`. Not
beside the sockets: on macOS that is `$TMPDIR`, which the system cleans. spyc
makes `jar-bin/` `0700`, with the marker file `.spyc-jar-store` in it, which
is how a jar knows it runs from a store.

- **spyc copies at its own start,** on a worker off the loop, before its first
  jar starts (which waits for it). It reads the `spyc-jar` beside its own
  binary once and writes every entry whole: hashing as it writes to a
  temporary name in `jar-bin/`, then `fsync`, `0555`, and a rename into the
  entry, so a crash mid-copy leaves no truncated binary under an entry's
  name. An entry that exists already is hashed again before it's used, never
  rewritten.
- **The copy is checked against spyc's own build** by running it once, as
  `spyc-jar build-info`, which prints the `SPYC_GIT_SHA` and `SPYC_BUILD_TIME`
  that `build.rs` gives both binaries of one build (they are one package). A
  copy that answers otherwise isn't used: a package upgrade landed while spyc
  started, and spyc runs this session without jars and says so, as it does for
  a jar that exits with 75. The run also pays the copy's first-run cost
  (about 263 ms on macOS) off the loop. Two development builds at one commit
  can pass this check as each other, since `build.rs` doesn't run again
  without a new commit; their entries, and so their `build` keys, still
  differ.
- **The jar never hashes.** A binary `upgrade` names that is already an entry
  of the jar's store is used as it is; anything else is copied in, written
  whole the same way, under a unique private name (decision L's part 1). S3
  always names an entry, so in practice no jar copies or pays the first-run
  cost; a hand-run `spyc-jar upgrade /some/path` is still protected. The jar
  keeps no dependency for it.
- **`build` is the entry's name,** read by the jar from the directory it runs
  from: the hash spyc computed, so two builds at one commit differ. `commit`
  gives the git commit beside it, for people.
- **A jar finds its store from its own path:** the directory two levels above
  the file it runs from, when that directory holds the marker and is the
  user's, `0700` and not a link. Neither the start's flags nor the blob carry
  it. The new image's `PREVIOUS` is the entry the old one ran from.
- **A jar not started from a store** (tests and `cargo run` start jars from
  scratch paths, where "two levels up" is an arbitrary directory) finds no
  marker and has no store. It keeps its upgrade copies, written the same way,
  in a `0700` directory beside its socket.
- **An entry is removed rarely.** An entry is about 0.75 MB, so deleting one
  wrongly is far worse than keeping it. spyc touches an entry's directory
  when it starts a jar from it, and its sweep, at spyc's start after the
  copy, touches every entry a live jar reports as its `build`. The sweep
  removes an entry only when no live jar reports it, it isn't spyc's own, and
  it has gone untouched for a day. If any socket in the jar directory fails
  to answer `describe`, which covers a jar mid-upgrade, the sweep removes
  nothing that time. Nothing waits on the sweep.
- **A state directory that can't execute** (a `noexec` mount): spyc finds out
  when it runs the copy, uses no store for that session, starts jars from the
  `spyc-jar` beside it as before, and says so in `spyc --jars`. Those jars
  find no marker and keep their copies beside their sockets. None of the
  macOS or Linux hosts checked mount either directory `noexec`.

What it closes, of the four it is meant to:

| | Closed? |
|---|---|
| A file swapped between validation and the `exec` | Yes, for every jar: it validates, holds and `exec`s one file in its store (or, with no store, its private copy) and never looks the named path up again. |
| No image to switch back to after a failed resume | Yes, for a jar started from the store: `PREVIOUS` is an entry no install touches. Not for a jar started outside one (a `noexec` state directory) on macOS, and not if the state directory is deleted. Linux has `/proc/self/exe` too. |
| A running spyc starting a newer jar from disk | Yes, while the store is in use: spyc starts jars from the entry it checked at its start. Not when the store is off. A store entry also keeps a program started late by `--start-on-owner` on the same image, since the jar starts it through its own binary (`spyc-jar pty-child`, `crates/spyc-jar/src/pty.rs:46`). |
| A binary overwritten in place under a running jar | The store removes the exposure: no install writes into it, and nothing spyc or a jar does writes an entry twice. It doesn't fix an observed failure: the in-place `cp` probe left the jar running. Another process of the same user can still change an entry (outside the threat model), and a jar outside the store keeps the exposure. |

### Refreezing

Version 1 freezes again when all three hold:

1. the store's design has merged;
2. `fake` and `swap` (decision L) pass: the program survives each, and the
   requester hears a refusal within the validator's deadline;
3. the jar built to differ (S3) passes upgrades in both directions.

## The review's findings, slice by slice

The external review of 2026-11-12 (`PANE_JAR_SPIKE.md` → "The review") found
18 things. The frozen surface's three, and the program role, were built on the
spike when version 1 reopened; the rest land here.

| Finding | Where |
|---|---|
| 1. Version skew in every direction | the format: built; a jar that exits with 75 fails the tab: S2a |
| 2. `refused` with a reason | built; spyc acts on it: S2a, S2b |
| 3. The blob through the holder | built |
| 4. Blocking writes under locks | S1b (the jar), S2a (spyc) |
| 5. Tokenless lookups deny upgrades | built in part (one question, then closed); the rest S1b |
| 6. A move strands its jars | S5 |
| 7. A refused adoption starts a second program | S2a |
| 8. Snapshot offsets drift after a gap | S2a |
| 9. The row-and-back repaint coalesces | S2b; for the outer jar, replaced by the program role |
| 10. Signals reach stale pids | built (none after the exit); every signal through the jar: S2a |
| 11. A failed `exec` leaves the jar killable | built |
| 12. Detach from inside spyc, and a second attach | the program role: built; the rest S4 |
| 13. Upgrades at every start, and S3 after S2 | S3, and S2's release gate |
| 14. Discovery from the environment | `SPYC_JAR_SOCKET`: built; the rest S2a |
| 15. Stored values the validator rejects | built |
| 16. An exit with nobody there is lost | S1b |
| 17. A pane with no linger dies in a handover | S1b |
| 18. A window wider than the ring | S1b |

The smaller items go with S1b (a signalled exit's code, a `gap` for a reader
that falls behind, no traced input, snapshot size against the frame limit),
S2a (the sweep and a socket not yet listening, `released()` keyed by pid, a
snapshot's build and pin, resynchronizing the parser after a gap) and the
property tests (S1b).

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

## The owner's decisions

1. **On by default from the first release that ships jars** (decided
   2026-11-12). The soak happens on `-CURRENT`, with daily users, before that
   release, not in a release with jars off. `[pane] jars` is the opt-in on
   `-CURRENT` while the slices land and stays as the way back. That release
   carries S2 and S3 together.
2. **One package, two binaries** (decided). spyc's package installs
   `spyc-jar` through a `[[bin]]` that calls the crate, so every install path
   (`cargo install`, Homebrew, apt) carries it without a second crate to
   install. The jar's crate is still published for spyc to depend on.
3. **One issue per slice** (decided), on the `3.0` milestone and linked from
   #1099, so each PR closes its slice.
4. **The spike branch is pushed** (decided) to `origin` as a reference branch,
   after a check that nothing of the owner's is in its harness, so a lost disk
   doesn't take the reference with it. It is deleted once S5 merges.
5. **The upgrade step reopens version 1** (decided 2026-11-15), sorted by
   where each part's code runs, with the store, and not refrozen until
   "Refreezing" holds (`JAR_PROTOCOL.md`, decisions L and M).
6. **With jars on, a pane's program runs in a jar or doesn't run** (decided
   2026-11-15). spyc never launches it another way: a tab that looks like
   every other and dies with spyc is worse than one that says it failed, and
   a fallback keeps two ways of hosting a pane alive for good. A jar slow to
   answer is waited for off the main thread, in a tab that opens at once.
   Not decided: what a session does when the store finds no usable jar at
   its start.
7. **S2b's retake gates turning jars on** (decided 2026-11-15), even as an
   opt-in: a resumed spyc takes its tabs back first.
