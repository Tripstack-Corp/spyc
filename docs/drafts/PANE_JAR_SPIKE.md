# Pane jar spike: upgrade spyc without killing its agents

**Status:** scoped, not started
([#1099](https://github.com/Tripstack-Corp/spyc/issues/1099)), revised after a
design review. Its report replaces the "Findings" section stage by stage.
**Requirements:** [`JAR_PROTOCOL.md`](JAR_PROTOCOL.md). This spike builds a
minimal jar and measures it; it reports when a measurement contradicts a
verdict there, and never changes the verdict itself.
**Gates:** `ROADMAP.md` → "The 3.0 horizon: Slow Cooker (durable sessions)".
That horizon's scope doesn't commit until this report and
[`VT_ENGINE_SPIKE.md`](../archive/VT_ENGINE_SPIKE.md) have both reported.
**Shape:** stages 1 to 6, each ending in a stop for the owner, with a go or
no-go after the minimal jar. Only stage 1 reaches `main`; everything else is
built on a throwaway branch.

---

## The goal

A Claude tab in the middle of a turn survives an upgrade of spyc, a crash of
spyc, and a hung spyc, and neither the agent nor its spyc tools notice.

Today all three kill every agent: spyc is the parent of every pane's program
and holds every PTY master, so its exit closes the PTYs and the children get
`SIGHUP`.

## The idea: a jar that doesn't understand terminals

Each pane's program runs under its own small process, a **jar**
(`spyc-jar`: where the spice is kept). The jar owns the PTY, is the program's
parent, relays bytes to whichever spyc owns it, keeps a log of output and
resizes, and stores an opaque snapshot of spyc's engine. A new spyc loads the
snapshot and replays the log after it, and the jar never parsed a byte.

[`JAR_PROTOCOL.md`](JAR_PROTOCOL.md) has the rest: the use cases, each
requirement with its verdict, the overflow modes, roles, the threat model, and
the owner's open decisions.

## Prior art

| Project | Shape | What it teaches |
|---|---|---|
| containerd shims | One shim per container owns its stdio and exit status; a restarted containerd reconnects through the shim's socket | The daemon restarts or upgrades without touching what runs under it. The closest analogue, proven at scale. |
| conmon (Podman) | A per-container monitor that receives the console PTY's fd over a unix socket (`recvfd`) | Handing a PTY master between processes is ordinary practice. |
| zmx (2025, Zig) | One daemon per session, about 1,000 lines, using libghostty-vt to redraw the screen on reattach | The nearest design, on our engine. Its session daemon parses the terminal, so it moves when the engine does; the jar keeps parsing out. |
| herdr | A detach daemon whose PTYs survive a client closing and a binary upgrade, by live handoff over fd passing. The design review reports a `--remote` mode that makes the local terminal a client of a remote server (unverified here). | The server stays up through a detach, which is decision B's shape. Its live handoff is still labelled experimental, and lost its macOS responsible-process (TCC) identity on handoff ([herdr #808](https://github.com/ogulcancelik/herdr/issues/808)). Crash survival and tolerance of version skew are where a jar can lead. |
| dtach, abduco | Tiny per-session detachers | dtach's latest release found is 0.9, from 2016: a process with a small enough job stops needing releases. |
| shpool (Rust) | One daemon for shell sessions; the client checks the protocol version and warns on a mismatch | Version the protocol in its first message. |
| tmux, WezTerm's mux | One server, and a client of another version is refused ("protocol version mismatch", "codec 42 vs 43") | The cautionary tale: upgrading a monolithic server means killing its sessions. |
| systemd's fd store, nginx's `USR2`, Envoy's hot restart | File descriptors outlive a process image: parked with the service manager, inherited across `exec`, or passed over a socket | `exec`-in-place keeps the PID, so children stay children: how the jar upgrades itself (R33), and the measured alternative to a jar. |

Sources are listed at the end.

## Platform constraints

- **macOS has no `PR_SET_CHILD_SUBREAPER`.** When spyc spawns a program and
  then exits, the program is reparented to launchd, and a new spyc can't
  `waitpid` it. A jar that stays the parent avoids the problem on both
  platforms.
- **Starting a jar needs no `unsafe`,** as far as this brief found: spyc starts
  it with `std::process::Command`, the jar calls rustix's safe `setsid()`, and
  the program is spawned through `portable-pty` as today. Stage 2 confirms it
  (decision E).
- **macOS, unverified:** whether `$TMPDIR` cleanup removes a socket that lives
  for days; and whether a program in a jar keeps Keychain and pasteboard
  access after the terminal that launched spyc quits, the class of problem
  herdr #808 hit.

## What spyc does today that has to change

1. **The MCP socket is named after spyc's PID:** `mcp-<pid>.sock`, in
   `socket_path_in` ([`src/mcp/mod.rs`](../../src/mcp/mod.rs)). A new spyc
   process gets a new PID, so every agent's `spyc --mcp` proxy would point at
   a dead path.
2. **The MCP proxy never reconnects.** `proxy_io`
   ([`src/mcp/server.rs`](../../src/mcp/server.rs)) connects once, and the
   first failed send ends it. The agent then has no spyc tools for the rest of
   its session, and the proxy keeps running the old binary's code after an
   upgrade.
3. **`PtyHost` backs three things** ([`src/pane/pty_host.rs`](../../src/pane/pty_host.rs)):
   pane tabs, background tasks and `!` captures (`spawn_capture`), and
   `demote_pane_to_task` moves a pane's host into a task. All three move to
   jars (`JAR_PROTOCOL.md` → "Tasks and captures").

Two things already in place carry much of the rest:

- **The engine can write its state back out, two ways, both exact.** At
  libghostty-vt 1.0.0 the engine spike measured its binary snapshot and its VT
  formatter each rebuilding 100% of rows, cell text, attributes and scrollback,
  with cursor and alternate screen preserved in all 26 cases. The binary
  snapshot is valid only for the same build and pin; the formatter is the
  portable form (R17a, R17b).
- **The Model handover exists.** Session manifest v2 (`spyc -r`) already
  serializes projects, tabs and session pins.

## Stage 1: the MCP groundwork, on `main`

Ordinary PRs, needed whatever the jar becomes:

- **An instance id,** naming the MCP socket now and the jar directory later
  (R30), so the socket path survives a new spyc process.
- **A proxy that reconnects** with a bounded backoff and sends `initialize`
  again with its pane id.
- **Only read-only requests are replayed.** A request that was in flight when
  the connection dropped is sent again only if it can't change anything. A
  mutating tool (`create_worktree`, `remove_worktree`, `register_scope` and the
  rest) returns a retriable error instead, unless the server can deduplicate it
  by request id. Replaying blindly could create or remove a worktree twice.
- **A compatibility test** running the previous release's proxy against the
  current server.
- **Room for what comes later:** the reconnect logic is written so that it can
  later ask the pane's own jar where its owner is now (R30a), and start a spyc
  for its instance when reconnecting fails, behind a lock so two proxies
  racing start one.

Stop for review.

## Stage 2: a minimal jar

Single owner, relay only, the event-log ring, the snapshot slot, exit status, a
jar-backed `PtyHost` behind a flag, and the upgrade and crash paths.

Measured on macOS and Linux:

- keystroke-echo latency the relay adds, at p50 and p99;
- throughput against today's `PtyHost`, including a firehose (`cat` of a large
  file) with the owner attached, which must lose nothing (R11);
- the gap, from the old spyc stopping to the new spyc's first frame;
- fidelity cell by cell over the 26-case corpus, an alternate-screen program,
  and a Claude tab streaming output, comparing the primary screen and its
  scrollback after leaving the alternate screen;
- what a program does with a terminal query nobody answers (R15b);
- memory and snapshot CPU at 30 panes.

`exec`-in-place is measured on the same harness, as the alternative to a jar:
spyc `exec`s its new binary, keeping its PID, so `waitpid` and its PTY masters
carry over. It survives an upgrade but not a crash or a hang.

Stop with a go or no-go.

## Stage 2b: spyc outlives its terminal

Decision B is made: spyc outlives its terminal, and no separate MCP daemon or
client/server render protocol is built. This stage compares the two shapes in
`JAR_PROTOCOL.md` → "How spyc outlives its terminal", on the throwaway branch
and the same harness.

**Shape 1, a headless handover.** Measured:

1. whether `App::run` works with no terminal (no raw mode, no input thread, no
   drawing), and what in `run.rs` and `terminal.rs` has to become optional;
2. a TUI to headless to TUI handover under a streaming Claude tab: the gap, and
   that hooks and `get_spyc_context` work throughout;
3. a field-by-field audit of `AppState`: in the manifest, rebuilt, or lost on
   handover, with every lost field listed;
4. a crash while headless: the MCP proxy starts a headless spyc for its
   instance when reconnecting fails, behind a lock, and two proxies racing
   start one;
5. macOS: after the launching terminal quits, the headless spyc can still read
   a repo under `~/Documents` and deliver a notification;
6. what each tool that describes the user's view returns with no client
   attached.

A foreground job leads its process group, so `setsid` fails in it and spyc
can't daemonize in place. Shape 1 starts a new spyc and hands over instead.

**Shape 2, spyc in its own jar.** The minimum: `spyc attach` (with `^d` and
`:detach` to leave), a raw relay that
sets up and restores the real terminal, and an attach signal or event that has
spyc set its terminal modes again, probe capabilities again, and repaint.
Measured, beyond shape 1's items 3 to 6 where they apply:

- an upgrade by `exec` inside the outer jar, with the attached client
  untouched;
- an audit that spyc never blocks on a terminal reply while nobody is
  attached;
- the fixed reset on detach leaving the real terminal clean, kitty keyboard
  flags included.

**For both:** keystroke-echo latency end to end (two relays for shape 2), the
attach gap, a client killed mid-turn (an SSH drop), attaching from a different
terminal emulator, and what Model state survives.

**A plain `spyc` stays plain** (`JAR_PROTOCOL.md` → "Principles"), which is a
pass bar here: a launch with no pane tabs matches today's startup time,
process count and open sockets, and quitting leaves nothing behind. Shape 1
keeps a plain launch plain by construction, since spyc runs straight in the
terminal until it detaches. Shape 2 passes only if its outer jar starts
lazily, never on a bare launch, and that weighs on the recommendation.

Stop with a recommendation between the two shapes.

## Stage 3: the jar upgrades itself

R33: the jar has the new binary validate a versioned handover blob, `exec`s it
only if it accepts, and keeps its PID, child, PTY and connections. The blob's
validator gets a fuzz target before this stage closes: a malformed blob that
gets through takes over a live PTY. And an old
jar runs under a new spyc. If this works, most of `JAR_PROTOCOL.md`'s
"v1 or later" verdicts can become "later".

## Stage 4: overflow and lifetime

`drop` with no owner connected, the linger policy (R32a), and the redraw
after a gap. `block` and `spill` are later (R12b), so they're built only if
that verdict changes.

## Stage 5: roles

Readers, the keyboard lease and taking over, only if the owner keeps them in v1
(decision D).

## Stage 6: projects move between spycs

A project's jars change owner from one spyc to another together, with its part
of the manifest, its scope claims and its attention roll-up, and its agents'
MCP proxies find the new owner through their jars (R30a). Driven by the verbs
in `JAR_PROTOCOL.md` → "Detaching, attaching and quitting": `:detach project`,
`:attach project`, and the same for a single tab. Forking a project
asks nothing of the jar, so it isn't part of the spike.

## Questions it answers

| | Question | Stage | How it's settled |
|---|---|---|---|
| Q1 | Is relaying every byte fast enough? | 2 | The latency it adds at p99. If it isn't, fd passing to a local owner becomes a capability (R8a); relaying stays mandatory, because a forwarded socket can't carry an fd. |
| Q2 | A separate `spyc-jar` binary, or spyc run as `spyc --jar`? | 2 | Memory per jar (the release spyc binary is about 23.6 MB, and a jar of a superseded version keeps that file alive) against shipping one binary through brew, apt and crates.io. |
| Q3 | How faithful is a rebuild from a snapshot plus its log? | 2 | Cell by cell, over the corpus, an alternate-screen program and a streaming Claude tab, including the screen and scrollback after leaving the alternate screen. |
| Q4 | Can the protocol be frozen? | 3 | An old jar runs against a new spyc in a test. |
| Q5 | How long is the gap? | 2 | Target under a second, with the owner's acknowledged offset equal to the jar's and nothing lost. |
| Q6 | How does a jar end? | 4 | By its linger policy: its program exits, or no owner returns within the pane's linger time, or the user closes it. A jar that outlives every spyc does so because its pane asked to (R32a), and `spyc --jars` lists it. |
| Q7 | Is a redraw enough after `drop` loses events? | 4 | The lost bytes may have changed terminal modes the snapshot doesn't know about. Overflow the ring on purpose while a Claude tab streams, inline and full-screen, and compare the screen after the repaint with a run where nothing was dropped. |
| Q8 | Do the roles hold with two spycs on one jar? | 5 | A reader connects, takes the keyboard, hands it back, takes over as owner. No key is split, each terminal query gets exactly one reply, and a reader that falls behind is resynced without slowing the program. |
| Q9 | Can the jar replace its own binary? | 3 | R33 under a streaming Claude tab: the new binary rejects a malformed blob and the old jar keeps running; it accepts a good one and the program never notices. |
| Q10 | Which way should spyc outlive its terminal? | 2b | The headless handover against spyc in its own jar: latency end to end, the attach gap, an SSH drop mid-turn, a different emulator on attach, what Model state survives. |
| Q11 | Can a project move between spycs? | 6 | Move a project with a streaming Claude tab: every jar changes owner, the agent's MCP tools work against the new spyc without restarting it, and a receiving spyc killed partway leaves a state that is reported, not lost. |

## Passing

On both macOS and Linux:

- **an upgrade, including one across an engine pin bump:** the tab keeps its
  PID, its turn finishes, nothing is lost, and the screen and scrollback come
  back from the portable snapshot;
- **`kill -9` of spyc, and a `SIGSTOP`ped spyc:** the program keeps running,
  the liveness deadline frees the hung owner, and a new spyc reclaims the jar;
- **`kill -9` of the jar itself:** the program gets `SIGHUP`, as it would
  today, and spyc reports the pane as ended rather than hanging;
- **a resize during the gap** lands at its offset, and the rebuilt screen is
  drawn at the right size;
- **a terminal query during the gap** gets no stale reply on reattach;
- **a firehose with the owner attached** loses nothing;
- **memory and snapshot CPU at 30 panes** are reported;
- **macOS:** Keychain and pasteboard access after the launching terminal quits,
  and a socket surviving `$TMPDIR` cleanup;
- `get_spyc_context` works from the agent afterwards without restarting it.

Crash survival and tolerance of version skew are the two to prove hardest:
they're where a jar leads what herdr ships today.

## Rules for the report

- Every claim is a measurement with its command, platform and commit, or is
  marked unverified.
- A measurement that contradicts a verdict in `JAR_PROTOCOL.md` is reported,
  and the stage stops; the verdict is the owner's to change.
- Every parser a stage adds has a fuzz target before the stage closes
  (`JAR_PROTOCOL.md` → "Fuzzing").

## Out of scope

spyc's side of detaching and attaching, SSH, any UI for readers or
turn-taking, and Windows. The protocol carries what each needs, so none is
ruled out.

## Findings

Each stage adds, here: a results table per question, what failed, what wasn't
tested, and finally a recommended v1 requirement list split into freeze now,
capability later, and never.

### Stage 0b: dtach as a stand-in jar

[dtach](https://dtach.sourceforge.net/) 0.9 holds a program and its PTY and
relays bytes without parsing, which is the dumb half of a jar, so it tested the
riskiest assumptions before any jar code existed. It was a stand-in only:
spyc will neither depend on it nor borrow from it (it is GPL-2.0-or-later).

**Setup:** macOS, spyc 2.4.0 (`46146bf4`, the installed release), Claude Code
from Homebrew in full-screen mode, dtach 0.9 from Homebrew. A Python harness on
a pty played the terminal: it answered DA1, DSR, CPR, the kitty keyboard query,
OSC 10/11, XTVERSION, DECRQM and window-size queries (or deliberately didn't),
tracked the modes each program set, and rendered the screen with `pyte`. spyc
ran with an isolated `HOME`, and Claude with its own `XDG_STATE_HOME`, so
neither could reach the user's running spyc.

| | Experiment | Result | What it settles |
|---|---|---|---|
| 1 | Keystroke echo through relays: a raw echo program, 400 single-byte round trips | Direct 9.3 µs median, 45 µs p99. One dtach hop 19.9 / 49 µs. Two hops 31 / 76 µs. | Q1: relaying is effectively free against a 16 ms frame. R8a (fd passing) becomes "never". |
| 2 | Detach spyc | dtach prints `[detached]` and stops. The terminal is left on the alternate screen, with mouse reporting (`1000`, `1002`, `1006`) and bracketed paste on. | An attach client must send a fixed reset on detach (shape 2, point 4). |
| 3 | Reattach spyc at a new size (30×100, a terminal without kitty keyboard support) | spyc repainted at the new size, but set none of its modes on the new terminal and sent no queries. | Mouse and paste are dead after an attach, and spyc draws on the main screen, until spyc is told it was attached (shape 2, point 2). |
| 4 | Reattach spyc at the same size | `-r winch`: blank, nothing redrawn. `-r ctrl_l`: 7 rows drawn, possibly only ratatui's diff. | A `SIGWINCH` with no size change repaints nothing, so the attach signal must be an explicit event. |
| 5 | Start spyc with no terminal (`dtach -n`), attach 6 s later | No hang on its unanswered startup queries. It drew on the first attach and handled keys. It never set its modes on that terminal, and its probes went unanswered, so it ran on default capabilities. | spyc doesn't block on a terminal's reply (shape 2, point 3), but capabilities must be probed on every attach. |
| 6 | Claude in dtach, asked to print 1 to 400, detached at 63 | Claude kept running and finished the turn ("done") while nothing was attached. | An agent outlives its terminal, and its work goes on. |
| 7 | Reattach Claude | Same size: blank. A changed size: a full repaint (371 to 400 and the prompt) with its mouse modes set again, though not the alternate screen. `-r ctrl_l` repainted it too. | `drop`'s redraw has to change the PTY's size and change it back. |

**Not tested in stage 0b:**
- macOS identity: Keychain, pasteboard and `~/Documents` access after the
  launching terminal quits. It needs Terminal.app launched and quit, and may
  raise a permission dialog, so it waits for the owner.
- Claude in inline mode.
- Leaked kitty keyboard flags: neither program pushed flags against the harness.
- MCP after a spyc restart, already known from the code: the proxy keeps the old
  PID's socket.

### Stage 2: a minimal jar

**Recommendation: go.** A jar per pane works inside spyc, at a cost no user
would notice, and survives a crash of spyc with the pane intact.

**Built,** on the throwaway branch `spike/jar-stage-2` (commits `6764e999` to
`cc9ddbe0`, never merged):

- **A `crates/spyc-jar` crate.** The protocol has length-prefixed frames, a
  critical flag and its own signal numbers (macOS and Linux number signals
  differently). The jar has an event log of output and resizes at byte
  offsets, owner acknowledgements that pace the program, replay with gap
  markers, a snapshot slot, a liveness deadline and exit collection. 14 tests
  drive the real binary with no spyc involved.
- **`src/pane/jar_backend.rs`, behind `SPYC_JARS=1`.** It implements
  `portable-pty`'s `MasterPty` and `Child` traits over the jar's socket, so
  `PtyHost` runs a jarred program without other changes.
- **Adoption with no manifest change.** A jar's socket is named after its
  pane's `SPYC_PANE_ID`, which the session manifest already saves and a
  restored tab takes back. A restored tab whose jar still answers is adopted
  instead of spawned.
- **R15's `Reply` frame.** The input queue tags replies, and a jarred pane
  sends them on a writer of their own. They are dropped while an adopted
  jar's tail is replayed, and for 500 ms after.
- **R4, the foreground process group.** The jar reads `tcgetpgrp` about every
  200 ms and sends a `Foreground` frame when it changes; spyc caches it, so
  `foreground_pgrp` never waits on the socket.
- **Every jarred tab has a pane id,** so a tab running anything is adopted on
  restore, and an adopted agent skips restore's queued `/resume` text, which
  would otherwise queue a prompt in it.
- **R32, stale sockets.** A socket counts as stale only when nothing answers
  on it; the jar directory is swept on a spyc's first jar, so a plain spyc
  never looks. A live jar that refuses to be adopted (another spyc owns it) is
  left alone, where the first version deleted its socket and would have made
  it unreachable.
- **R17b, a portable snapshot.** The jar keeps the latest snapshot of each
  format. The engine also encodes the VT formatter's emit, scrollback
  included. On adoption spyc uses the binary snapshot when this build decodes
  it, else the portable one, else the full log.
- **The snapshot loop.** The pane's parser worker counts the bytes it parses
  and, after a second of quiet, stores libghostty's binary snapshot in the jar
  at that offset. On adoption spyc fetches it, checks this build decodes it,
  starts the pane's engine from it, and replays only what came after.
- **A Linux run,** `spikes/jar-stage-2/run-linux.sh`: the jar tests and the
  terminal harness in an Alpine Rust container under podman, using the musl
  target the vendored libghostty archives cover.

**Measured on macOS** (arm64): the jar figures with release builds and
`examples/measure.rs`, three runs each; the spyc figures with debug spyc in the
stage 0b harness (`spikes/jar-stage-2/` on the branch). **Linux** (arm64, in
podman's VM on the same Mac) passes the same tests and the same crash,
fidelity and R15a checks; its figures follow the table.

| | Result | Settles |
|---|---|---|
| Keystroke echo, 1000 round trips | Direct PTY 13–15 µs p50, 23–40 µs p99. Through a jar 40–49 µs p50, 90–137 µs p99. | Q1: relaying every byte costs under 0.15 ms against a 16 ms frame. Passing the fd stays "never" (R8a). |
| Throughput, 100 MB | Direct PTY 109–112 MB/s; through a jar 71–76 MB/s. spyc's engine parses about 490 MiB/s, so on a firehose the jar is the bottleneck: a `cat` of 100 MB takes about 1.35 s instead of 0.9 s. Batching writes, larger PTY reads, merged frames and buffered client reads each changed nothing; handoffs between the jar's threads are the likely cost. | An optimization to make before the freeze, not a blocker. |
| Replay | A full 4 MB log in 21–24 ms. | Q5, for the jar's part of the gap. |
| Memory | 2.0 MB resident per idle jar; 58 MB for 30. | Q2, with the binary size below. |
| Binary | Release `spyc-jar` 0.58 MB; release spyc 23.6 MB. | Q2: a separate small binary. |
| Crash and restore | `kill -9` of spyc, then `spyc -r`. A shell tab came back as the same process (a variable set before the crash still set), with its screen; the same jar, nothing respawned. | The spike's goal. |
| Fidelity and gap | `less` paged to line 79: pane content back 0.06 s after picking the session, all 27 pane rows identical. Without jars, a fresh prompt. | Q3 and Q5, with the full log; no snapshot yet. |
| R15a | A probe asked DA1 before the crash and read again after the restore. With the gate it read nothing stale; with the gate held open it read `ESC[?62;22c`, a replayed query's answer arriving as keystrokes. | R15a is a real hazard, and the gate closes it. |
| Snapshot loop | A tab printed about 17 KB against an 8 KB log, then spyc crashed and was restored. With snapshots the restored history starts at the original command; without, at line 1694 (1676 on Linux), the oldest the log still held. | The log can be small: history comes from the snapshot. |
| A streaming Claude tab | Claude (the owner's login, spyc's state isolated) printing 1 to 300 in a jarred tab; spyc killed at 44. After `spyc -r`: the same Claude process, its turn finished (279 to 300 and "done"), a clean input box, and it answered a new prompt. | The spike's goal, for an agent. |
| macOS identity | A jar started from Terminal.app, then Terminal.app killed. The program in it still read `~/Documents`, the pasteboard and a Keychain item, and delivered a notification; the jar was reparented to launchd. | herdr #808's loss doesn't happen here. |
| R17b | A restoring spyc playing a different build (it refuses the binary snapshot). From the portable snapshot the restored history is whole. With only a binary snapshot it is empty: the jar had trimmed its log to that snapshot, so nothing was left to replay. | The portable form is what makes a build change lossless, and it has to exist before the handover. Linux gives the same three results. |
| A `SIGSTOP`ped spyc | spyc frozen past the jar's 10 s deadline: the jar handed the lease to a new owner and the program kept running. Resumed, spyc showed a stale pane, not marked exited and never reconnecting, though the jar was free. | R21 holds; a resumed spyc must win its jar back. |
| R32 | A sweep removes a socket nothing answers on and keeps a live one; the test fails when a refused connection is taken as live. | — |
| Startup | Release spyc with a startup tab: first frame 25 ms without jars, 39 ms with; the pane's shell prompt 183 ms against 191 ms. With no tabs no jar starts, so a plain spyc pays nothing. | The plain-spyc principle holds. |
| R4 | An interactive shell in a jar: the jar reports the shell's process group, then a job's while it runs. The test fails when the jar stops tracking changes. | R4 works without spyc reaching the master. |

On **Linux**: echo through a jar 137 µs p50 and 290 µs p99 (direct 76 and
212 µs, both slower in the VM); throughput 130 MB/s through a jar against
166 MB/s direct, about 78%; a 4 MB replay in 9 ms; 0.67 MB resident per idle
jar, 19 MB for 30.

**Found along the way:**

- **Only shell and agent tabs have a pane id,** so only they are adopted on
  restore; a tab running `htop` or `tail -f` would be respawned. Every tab needs
  one.
- **On Linux spyc's "is that session still running" check counts a zombie as
  running.** The harness never reaped the spyc it killed, so the restore was
  treated as a copy and adopted nothing. A real crash's parent shell reaps it
  at once, but the check should still look past a zombie.
- **The window bounds the program, not delivery.** What a program printed
  before its owner connected is sent whatever the window, and the jar may run
  one PTY read past it; the Linux PTY's larger reads showed both. The test
  now checks that delivery stops growing with no acknowledgements.
- **The jar's `setsid` can fail and it carries on.** Started as a background
  job (`&`) from an interactive shell, the jar leads its process group, so
  `setsid` fails and it stays in the terminal's session. spyc never starts it
  that way, but the jar should fork first or refuse.
- **Four spike functions are over the 100-line limit** (the jar's `serve` is
  179 lines); production would split them. With jars off, the rest of the
  suite passes unchanged.
- **Two harness traps,** for whoever runs these again: answer `spyc -r`'s
  session picker only once it's drawn, and keep reading a killed spyc's
  terminal while reaping it, since a dying process can block closing a
  terminal whose output nobody reads.
- **A plain `cargo build` at the workspace root doesn't build `spyc-jar`.**
  Measuring with a stale jar briefly sent the R15a check the wrong way; use
  `cargo build --workspace`.
- **Two bugs the jar's own tests caught:** a gap marker that named the wrong
  start, and log trimming that could drop a resize at offset 0 before it was
  sent.

**`exec`-in-place, compared without building it.** Both ways a restore costs
spyc's own startup and restore, about 25 to 40 ms to a first frame. The jar
adds a socket connect and a replay, at most 25 ms for a 4 MB log and less
with a snapshot. For those milliseconds the jar also survives a crash and a
hang, and is what makes detaching possible, none of which `exec`-in-place
can do: its program dies with spyc. Building it would have measured the
small difference, not changed the choice.

**What stage 2 asks of the requirements,** for the owner (`JAR_PROTOCOL.md`'s
verdicts are unchanged here):

- **A spyc that loses its lease takes it back** when the jar is free, and marks
  the pane detached when another spyc holds it. The same reconnect serves a
  dropped connection.
- **A crash followed by a build change.** With only the binary snapshot kept,
  that restore is empty. Either a portable snapshot is kept alongside the
  binary one, at some cost per snapshot, or the jar keeps its log back to
  the last portable snapshot.
- **The jar handles its own `setsid` failing,** by forking first or refusing.

**Not done in stage 2:** memory and snapshot CPU at 30 panes inside spyc.
**Recommendation:** go, and on to stage 2b.

### Stage 2b: how spyc outlives its terminal

**Recommendation: shape 2, spyc in its own jar, entered lazily.** A bare
launch stays a plain spyc in its terminal (the principle). The first detach
hands spyc over, once, to a new spyc running in an outer jar, through the
session manifest and the pane-jar adoption stage 2 proved. From then on,
detaching and attaching are a client coming and going, and the Model never
moves again.

**Shape 2, built and measured** (`spike/jar-stage-2`, `3700ea8a`,
`bcd6f4ea` and `87763ae1`):

- `spyc-jar attach SOCKET`: a raw relay to a real terminal, `^\` to detach,
  and a fixed reset on the way out.
- With `--attach-signal`, a jar sends its program `SIGUSR1` when an owner
  connects. spyc takes it, at the resize the client's one-row nudge causes,
  as a new terminal: it sets its modes again and repaints, as returning from
  `^z` does.

| | Result (`spikes/jar-stage-2/shape2.py`, macOS and Linux alike) |
|---|---|
| Attach | spyc set its modes on the new terminal: alternate screen, mouse reporting, bracketed paste. |
| Typing through two relays | Reached the pane's shell and came back. |
| Detach | The real terminal was left clean (only the cursor shown, as the reset intends). |
| Reattach elsewhere | A terminal of another size, without the kitty keyboard protocol: modes set again, repainted at the new size, the pane's content intact. |
| A client killed (`kill -9`, an SSH drop) | spyc and the pane's program ran on; the next client found the loop four seconds further on. |
| Echo, end to end | Median through the outer jar and the pane's jar against spyc run directly: 6.5 against 6.4 ms on macOS, 9.7 against 9.3 ms on Linux. Inside spyc's render cadence. |

**Found on Linux: a probe that steals keys.** At launch spyc asks the
terminal which image protocol it speaks, through `ratatui-image`, whose reader
thread gives up after a timeout but stays blocked reading standard input. A
spyc started with nothing attached gets no answer, and that thread later takes
the first keys a terminal sends: there, the `^a j` that should have focused
the pane, so the command typed after it landed in the file view. Two readers
race for the bytes, so macOS passed by luck. The spike's outer jar marks its
program, and spyc skips the probe there; with that, every row above passes on
both platforms. The requirement (`JAR_PROTOCOL.md`, shape 2's point 3):
probes run at each attach, and no reader outlives its probe.

Not covered: spyc probes the kitty keyboard protocol only at launch, so a
spyc started with nothing attached never uses it; an attach should probe
again, along with the image protocol. An upgrade by `exec` inside the outer
jar wasn't built.

**Shape 1, assessed from the code rather than built.** A headless handover
on every detach and attach needs:

- **A run loop with no terminal.** `run.rs` and `terminal.rs` assume one:
  raw mode and the alternate screen at setup, `crossterm`'s input thread,
  resize events, the image-protocol probe, OSC 52 clipboard writes, the
  window title, and `suspend_tui`/`resume_tui` around foreground commands.
  Headless, `$EDITOR`, `;` and `$` can't run at all.
- **A handover through the manifest each time.** It saves each project's
  directories, split, pane size and focus, tabs and HUD mode. Everything else
  resets on every attach: cursors, picks, filters and sort, open pagers
  (`^a v` included), archive mounts, the list of background tasks, the last
  search, an open prompt or message, and the stderr log.

Shape 2 keeps all of that, costs nothing measurable per keystroke, and its
handover happens once, at the first detach, where shape 1 pays it every time.

### Decision I: what a portable snapshot costs and loses

The question was whether every pane can afford the portable form beside the
binary one, so a crash followed by a build change loses nothing. It can.
Measured on `spike/jar-stage-2` (`695c9b25`, `381b8276`): a release build,
120×40 panes, spyc's 10,000-row scrollback full.

| Output | Binary size | Binary encode | Portable size | Portable, made off the lock | Portable restore |
|---|---|---|---|---|---|
| Plain text | 0.70 MB | 0.9 ms | 0.69 MB | 6 ms | 0.9 ms |
| A coloured shell | 2.81 MB | 3.2 ms | 1.67 MB | 36 ms | 8 ms |
| An inline agent (truecolour, emoji, CJK) | 4.20 MB | 5.4 ms | 2.22 MB | 36 ms | 13 ms |
| Every cell its own colours | 3.90 MB | 7.6 ms | 6.06 MB | 40 ms | 32 ms |

- **Smaller for real output.** The portable form is larger only when every
  cell has its own colours.
- **Off the lock.** The pane's parser lock, which the draw pass also takes,
  covers only the binary encode. The portable form is made from the binary
  snapshot in a scratch engine on the pane's own thread, byte for byte the
  same as encoding it under the lock.
- **Thirty full agent panes at once** (a broadcast going quiet everywhere):
  every snapshot was done within about 125 ms, but contention stretched the
  binary encode under each lock to 46–62 ms. That's enough to drop a frame,
  with or without the portable form, so panes take turns (R17d).

**Fidelity.** `jar_fidelity` sets up 26 pieces of terminal state and compares
what spyc and the program can observe after a restore and after the same
continuation, with the binary form as the control, which came back exact in
every case. The formatter's emit on its own lost 9:

| Lost by the raw emit | Carried by spyc's conversion |
|---|---|
| The primary screen and its history under a full-screen program: the formatter writes only the screen showing | The primary screen first, then the alternate screen, whose emit opens by switching to it |
| Soft wraps: each row came back as its own line | An unwrapped emit, so the restoring engine wraps the lines again |
| The saved cursor (DECSC), in either screen | Asked of a scratch engine with DECRC and DECRQSS, saved again ahead of the screen's emit |
| A cursor shape such as vim's bar | Asked with DECRQSS, written when it isn't the default |
| The title | Read from the engine and set again |

With those, 24 of 26 come back exact on macOS and Linux. The two that don't are
pinned in the test: kitty keyboard flags below the top of their stack, and
DECSCA protection. spyc reads neither: it never encodes keys with the kitty
flags, and almost nothing uses DECSCA. A program-set palette survives in the
way that matters, because spyc draws palette indices with its own colours.

**The crash followed by a build change** (`snap.py refused`: spyc killed, then
a restore that refuses the binary snapshot): the restored history now starts
at line 1, where stage 2 left it empty.

### Stage 3: the jar upgrades itself

**Recommendation: go.** R33 works on macOS and Linux: a jar replaces its own
binary while its program runs on, keeps every connection, and runs on
unchanged when the new binary refuses. What has to freeze is what the plan
hoped: the `hello` and the handover blob. Built on `spike/jar-stage-2`
(`23c8f218`, `03f4fdcd`).

How an upgrade runs (`crates/spyc-jar/src/upgrade.rs`):

1. Every thread that may hold bytes in flight parks at a safe point: the PTY
   reader between reads, each connection's reader between frames and its
   writer between batches, the acceptor, and the child waiter.
2. The jar encodes itself as a versioned blob: its configuration, program,
   log, snapshots, leases and each connection's place in the log.
3. The new binary validates the blob (`spyc-jar validate-handover`). Any
   failure up to here unparks everything and refuses the upgrade.
4. The blob, the listener, the PTY master and every connection go as
   `SCM_RIGHTS` over a socketpair that becomes the new image's standard input,
   and the jar `exec`s `BINARY resume-handover`: same PID, same session, same
   program.

| | Result (macOS and Linux) |
|---|---|
| An upgrade under a program counting as fast as it can | The same jar process and program; the count runs on with nothing lost or repeated. Bite-checked: a PTY reader that doesn't park loses bytes, in 3 runs of 3. |
| Connections | The owner's and a reader's both cross, the listener takes new ones, and input and resizes reach the program afterwards. |
| Three upgrades in a row | Each one as clean as the first. |
| A binary that refuses the blob, or doesn't exist | Refused with its reason; the old jar runs on, with the count unbroken. |
| A streaming Claude tab in spyc (macOS, `claude_upgrade.py`) | Three upgrades mid-turn: the same jar process, now running the new binary, and the same Claude. The turn finished, and a new prompt got an answer. spyc noticed nothing. |
| Time, debug build | 344 ms to a binary never run before, which includes macOS's first-run scan of it; about 110 ms after that. Most of the 110 ms is threads noticing within their 100 ms poll; waking them directly would cut it. |

**Q4, an old jar under a new spyc.** The jar from before stage 3
(`381b8276`, built by `old_jar.sh`), driven by the new client
(`tests/compat.rs`):

- **A longer `hello` is welcomed.** Version 1 reads the fields it knows and
  ignores the rest, so capabilities (R34) can follow them. That rule is part
  of what freezes.
- **An unknown critical message** gets `Unsupported`, and the connection
  carries on. An unknown ancillary one is skipped. A new jar's own notice
  after an upgrade is ancillary, so an old client skips it.
- **An old jar can't be upgraded:** to it, the request is an unknown critical
  message. So R33 can't arrive later. The first released jar has to carry it.

**Decision E, answered by building it: no `unsafe`.** The jar no longer uses
`portable-pty`. rustix opens the PTY, and the program starts through
`spyc-jar pty-child`, which calls `setsid` and `ioctl_tiocsctty`, then `exec`s
it. Two other routes fail:
- **Adopting inherited descriptor numbers** after `exec` would need `unsafe`.
- **Reopening a PTY master through `/proc/self/fd`** allocates a new PTY on
  Linux.

`SCM_RIGHTS` avoids both. The jar depends on `anyhow` and `rustix` alone.

**Fuzzing.** `fuzz/fuzz_targets/jar_handover.rs` (the blob) and `jar_frame.rs`
(the frame parser, both sides) ran 3 minutes each: 23.8 million and 16.3
million inputs, with no crash. A first run of the frame target overran its
time limit and was stopped. A rerun with a 5-second limit on each input found
none slower. Unit tests add a round trip, every truncation, 50,000 mutations
and a forged count that must not allocate.

**Gaps:**

- **R19 on macOS.** On Linux the blob is a `memfd`, in memory only. macOS has
  no `memfd`, and its POSIX shared memory can only be mapped, which needs
  `unsafe`. There the blob is an unlinked file in the jar's `0700` directory,
  which may reach the disk through the page cache.
- **Nothing rolls back after `exec`.** A resume that fails ends the jar and
  its program. The new binary has just validated the same bytes, so only what
  validation can't see is left, such as a descriptor that didn't arrive.
- **A connection still in its `hello`** when the upgrade starts isn't carried.
  It closes, and its client reconnects, as decision G already requires.
- **Who may ask.** The spike takes an upgrade from any connection. The real
  jar takes it from the owner, with the token.

**What stage 3 asks of the requirements,** for the owner:

- R33 stays v1, for good: it can't arrive later.
- The verdicts marked **v1 or later** (R16, R20's readers and keyboard, R23,
  R24 and R25, all decision D's roles) can become **later**: an upgraded jar
  can add them.
- R19 needs a macOS answer, or an exception for the instant of a handover.

### Stage 4: overflow and lifetime

**Recommendation: go, with one change to restoring across a gap (R12c).**
Built on `spike/jar-stage-2` (`8e5828d1`, `80f0b3c6`).

**Q7: a repaint alone isn't enough after `drop`.** In `q7.py`:
- Claude prints 1 to 600 while spyc is dead, its jar's ring cut to 8 KB.
- A restored spyc then makes it repaint.
- That restore is compared with one from a ring big enough to lose nothing.

Inline, Claude printed about 89 KB for those 600 lines, and the jar lost
about 80 KB. Full-screen, it printed about 23 KB, and the jar lost about
12 KB.

| Claude | Restored | Screen after the repaint | Modes | History |
|---|---|---|---|---|
| Full-screen | Nothing lost | 600 shown, new prompt answered | Alternate screen, mouse reporting, bracketed paste | (Claude's own) |
| Full-screen | Lost, from an empty engine | The same | **None of the three** | (Claude's own) |
| Full-screen | Lost, from the snapshot under the gap | The same | All three | (Claude's own) |
| Inline | Nothing lost | 600 shown, new prompt answered | Bracketed paste | From the launch command on |
| Inline | Lost, from an empty engine | The same | **No bracketed paste** | From 557 on |
| Inline | Lost, from the snapshot under the gap | The same | Bracketed paste | From the launch command on, then the gap, then from 549 |

- **The repaint redraws the screen but sets no mode again.** Claude sets its
  modes once, at launch, in the oldest bytes, which overflow drops first.
- **From an empty engine,** spyc treats a full-screen Claude as one that
  wants no mouse, keeps its screen in the pane's scrollback, and sends a
  multi-line paste as separate lines.
- **From the newest snapshot under the gap,** every mode came back. The
  snapshot is older than the gap, so the bytes after the gap are applied on
  top of it before the repaint. That is now the default.
  `SPYC_JAR_GAP=blank` keeps the empty engine for comparison.
- **What can still go wrong:** a mode the program changes inside the lost
  bytes stays as the snapshot had it until the program sets it again. spyc
  takes snapshots after a second of quiet, so through a long stream the newest
  one predates the stream. A snapshot every few seconds even while output
  flows would narrow that, at about 6 ms of the pane's lock each.

**Q6: how a jar ends.** Three ways, each tested on macOS and Linux:

1. **Its program exits,** and an owner collects the exit, or nobody does
   within a minute.
2. **No owner within its linger (R32a).** `--linger-ownerless-ms`: the
   program gets `SIGHUP`, as from a closed terminal, then `SIGKILL` two
   seconds later. A program that ignores the hangup ends too. Without a
   linger, the jar waits for an owner however long; a new owner gets it. A
   jar with a finite linger gives its first owner ten seconds to connect.
3. **The user closes the tab:** spyc sends the jar `Kill`, as before.

**R31: a jar describes itself.** `Describe` answers `key`, `value` pairs:
- the id, the instance and its start time;
- the jar's version and binary;
- the program and its pid;
- its size, overflow mode, linger, owner, log and snapshots.

A newer jar can add keys that an older spyc still shows. The instance and the
start time survive an upgrade. `spyc-jar describe SOCKET` prints one jar, and
`spyc --jars` lists them all, a stale socket as stale without waiting on it.

`drop` with no owner has worked since stage 2. `block` and `spill` stay later
(R12b), unbuilt.

**What stage 4 asks of the requirements,** for the owner:

- R12c as v1: across a gap, the snapshot under it, then what follows, then the
  repaint.
- Whether spyc takes snapshots on a timer while output flows, not only after
  a quiet second.
- The linger each kind of tab gets. The draft gives a pane "until an owner
  returns" and a capture a short one; the spike's knob is
  `SPYC_JAR_LINGER_MS`.

## Sources

- [zmx: session persistence for terminal processes](https://erock.prose.sh/zmx-session-persistence)
- [containerd: containers, tasks and shims](https://thecontainerdbook.pages.dev/chapters/part-4/12-containers-tasks-shims)
- [conmon's `ctrl.c` (the console socket and `recvfd`)](https://sources.debian.org/src/conmon/2.1.13%2Bds1-2/src/ctrl.c)
- [herdr #808: responsible-process loss on handoff](https://github.com/ogulcancelik/herdr/issues/808)
- [shpool's protocol version check](https://docs.rs/crate/libshpool/0.10.1/source/src/protocol.rs)
- [tmux: protocol version mismatch after an upgrade](https://wiki.neonvagabond.xyz/content/unix.stackexchange.com_en_all_2024-05/questions/122238/protocol-version-mismatch-client-8-server-6-when-trying-to-upgrade)
- [WezTerm: mux codec version mismatch (issue 5200)](https://github.com/wez/wezterm/issues/5200)
- [dtach(1)](https://www.mankier.com/1/dtach)
- [systemd: the file descriptor store](https://systemd.io/FILE_DESCRIPTOR_STORE)
- [nginx: upgrading the executable on the fly](https://docs.w3cub.com/nginx/control)
- [Darwin: exit status from kqueue's `EVFILT_PROC`](https://lists.apple.com/archives/Darwin-dev/2009/Aug/msg00210.html)
