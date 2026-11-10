# Pane jar spike: upgrade spyc without killing its agents

**Status:** scoped, not started
([#1099](https://github.com/Tripstack-Corp/spyc/issues/1099)), revised after a
design review. Its report replaces the "Findings" section stage by stage.
**Requirements:** [`JAR_PROTOCOL.md`](JAR_PROTOCOL.md). This spike builds a
minimal jar and measures it; it reports when a measurement contradicts a
verdict there, and never changes the verdict itself.
**Gates:** `ROADMAP.md` → "The 3.0 horizon: Slow Cooker (durable sessions)".
That horizon's scope doesn't commit until this report and
[`VT_ENGINE_SPIKE.md`](VT_ENGINE_SPIKE.md) have both reported.
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

**Shape 2, spyc in its own jar.** The minimum: `spyc attach`, a raw relay that
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

Stop with a recommendation between the two shapes.

## Stage 3: the jar upgrades itself

R33: the jar has the new binary validate a versioned handover blob, `exec`s it
only if it accepts, and keeps its PID, child, PTY and connections. And an old
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
MCP proxies find the new owner through their jars (R30a). Forking a project
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

## Out of scope

spyc's side of detaching and attaching, SSH, any UI for readers or
turn-taking, and Windows. The protocol carries what each needs, so none is
ruled out.

## Findings

Not run yet. Each stage adds, here: a results table per question, what failed,
what wasn't tested, and finally a recommended v1 requirement list split into
freeze now, capability later, and never.

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
