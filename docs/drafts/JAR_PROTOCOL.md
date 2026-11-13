# The jar: its requirements and version 1

**Status:** version 1 frozen on 2026-11-12, on the owner's word, after the
spike built every v1 verdict below, and **reopened** the same day for three
changes an external review found it needed (decision K). The changes are made
and tested; [Version 1 on the wire](#version-1-on-the-wire) is the format with
them, byte for byte, and it freezes again on the owner's word.
**Tracks:** [#1099](https://github.com/Tripstack-Corp/spyc/issues/1099).
**Tested by:** [`PANE_JAR_SPIKE.md`](PANE_JAR_SPIKE.md), which builds a minimal
jar, measures it, and reports which of these requirements survive.

A `spyc-jar` runs one pane's program, owns its PTY, and outlives spyc. A jar
started today may still be running under a spyc released a year from now, so
what it freezes has to be right from the first release. This document maps
everything a jar could be asked to do and proposes, item by item, whether
version 1 does it, leaves room for it, or never does it.

**What actually has to freeze is small, if R33 works.** A jar that can replace
its own binary without its program noticing only needs to freeze two things
for good: the `hello` that opens every connection, and the blob one jar hands
the next across that `exec`. Everything else can arrive as a capability. That
is why the spike proved R33 early (stage 3). With it working, the verdicts once
marked **v1 or later** became **later** (decision D).

The path to the freeze runs in stages, each ending in a stop for the owner:

0. **This document and the spike brief,** revised (this revision).
1. **The MCP groundwork** on `main`: an instance id, a stable socket path, a
   proxy that reconnects. Needed whatever the jar becomes.
2. **A minimal jar** on a throwaway branch, measured, ending in a go or no-go.
   - **2b. spyc outlives its terminal:** a headless handover against spyc in
     its own jar, measured on the same harness, ending in a recommendation.
3. **R33:** the jar upgrades itself, and an old jar runs under a new spyc.
4. **Overflow and lifetime:** the modes, the linger policy, the redraw after a
   gap.
5. **Roles,** if the owner keeps them in v1.
6. **Projects move between spycs:** a project's jars change owner together and
   its agents' MCP connections follow.

The protocol spec, with exact byte fixtures, was written from what the
stages measured, once they had all reported:
[Version 1 on the wire](#version-1-on-the-wire).

---

## Principles

- **A plain `spyc` stays plain** (decided by the owner). Someone who only
  wants a file commander types `spyc` and gets exactly what they get today,
  until they use a feature that needs more. All of the jar machinery is
  hidden from them:
  - **launching** brings no new prompt, no daemon and no measurable startup
    cost;
  - **jars exist only when there is a program to hold:** a session with no
    pane tabs has no jars, sockets or background processes;
  - **quitting** leaves no process, socket or file behind;
  - **if the jar machinery fails,** spyc hosts the pane itself, as it does
    today, and notes it in `:stderr` rather than prompting;
  - **the new verbs** (`spyc attach`, `:detach`, sharing) cost nothing until
    someone uses them.
- **Mechanism in the jar, policy in spyc.** The jar does what it's told: how
  big the ring is, what to do when it fills, how long it lingers alone. spyc
  decides all of them.
- **The jar never understands terminal content.** It doesn't parse VT, doesn't
  know what an agent is, and stores spyc's engine snapshot as opaque bytes.
  Everything that changes often stays in spyc.
- **The jar keeps the program alive, not its tools.** spyc's MCP server, the
  socket the status hooks report to, and the scope registry all live in spyc.
  While no spyc is the owner, an agent in a jar keeps running but has no spyc
  tools and reports no status. So **spyc outlives its terminal** (decision B,
  decided): detaching never leaves a jar without an owner, and a jar is
  ownerless only between a spyc dying and the next one arriving. How spyc
  outlives its terminal is open between two shapes
  ([below](#how-spyc-outlives-its-terminal)).
- **One program per jar.** A jar is born with its program and ends with it.
  Restarting a program, or running several, is spyc's business.
- **A jar is local and same-user, forever.** It holds a PTY, and writing to a
  PTY is running commands as its user, so it is the last thing that should
  face another user or a network. Sharing with anyone else goes through a
  separate gateway that is a client of the jars ([Sharing](#sharing)).
- **The jar ships inside spyc, not as a tool of its own** (decision C). Its
  protocol is private: versioned, with no stability promise to anything but
  spyc.

## Use cases

| | Use case | What it asks of the jar |
|---|---|---|
| U1 | **Upgrade spyc** while agents work | Outlive spyc; hand the owner's lease to the new spyc; replay what arrived in the gap, across an engine pin bump if the upgrade carries one. |
| U2 | **spyc crashes, or hangs,** and is launched again | Outlive spyc; notice the owner is gone even when its socket stays open; tell the new spyc what each jar is. |
| U3 | **Detach and attach** (3.0) | Nothing from a pane jar: a spyc stays the owner throughout (decision B). In the outer-jar shape, spyc's own jar carries the detach. |
| U4 | **Attach over SSH** | Work over a forwarded unix socket, so every byte is relayed (no fd passing) and a dead peer is noticed without the kernel's help. |
| U5 | **A second window** on the same agent | More than one connection; one owner, the rest watching. |
| U6 | **Watching from a phone** | A read-only connection that can never type. |
| U7 | **Pairing:** two people at two spycs take turns typing | A keyboard that moves between writers, and never splits a key. |
| U8 | **Recording and replay** | Output with timestamps, numbered so nothing is missed or doubled. |
| U9 | **The away recap** from what the program did while you were gone | Timestamps and byte counts, readable after the fact. |
| U10 | **A jar started with no spyc at all,** say an agent run from a script, attached to later | A jar can't assume a spyc spawned it or is listening (decision C). |
| U11 | **spyc on a newer engine** than the one that took the snapshot | The snapshot says what format it's in, and a portable form is always kept beside it (decision I). |
| U12 | **Tasks and captures** survive too: a pane demoted to a task (`demote_pane_to_task`), a `!` capture still running | Everything `PtyHost` backs today runs in a jar ([below](#tasks-and-captures)). |
| U13 | **Move a project between spycs:** detach one project from a spyc and attach it to another, in another terminal or over SSH | Every jar of the project changes owner, and its agents' MCP connections follow to the new spyc ([below](#projects)). |
| U14 | **Fork a project:** a copy of its working set, each agent's conversation forked, perhaps into a new worktree | Nothing new: forking spawns new jars ([below](#projects)). |
| U15 | **Share with another person,** another user on this machine or someone elsewhere: read-only by default, write only on an explicit, warned, expiring grant | Nothing: the gateway does it, as an ordinary client of the jars ([Sharing](#sharing)). |
| U16 | **spyc in a browser,** from a laptop or a phone | Nothing beyond shape 2: the browser is one more attach client, through the gateway ([Sharing](#sharing)). |

## Requirements

**v1** means the first release. **Later** means a new capability, added
without a new version, or by a jar upgrading itself (R33). **Never** means it
stays out of the jar for good.

### The program

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R1 | Spawn with argv, environment, working directory and starting size, in a new session with the PTY as controlling terminal | v1 | The minimum. Spawn options travel as an extensible map. | all |
| R2 | Report the exit: code or signal, and keep it until a client acknowledges it | v1 | A program that exits while nobody is connected must still be reported. | U2, U3, U10 |
| R3 | Deliver a signal to the program or its process group: `SIGINT`, `SIGSTOP`, `SIGCONT`, `SIGKILL` | v1 | `^z` sends `SIGSTOP` today, and closing a pane kills its group. | all |
| R4 | Answer "which process group is in the foreground" (`tcgetpgrp`) and "what are the terminal settings" | v1 | spyc reads `tcgetpgrp` on the PTY master today (`PtyHost::foreground_pgrp`), and only the master's holder can. | U1, U2 |
| R5 | Restart a program that exits | never | Policy. spyc spawns a new jar. | — |
| R6 | Several programs in one jar | never | A jar's life and its program's are the same thing. | — |

### The PTY

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R7 | Set the size in rows and columns **and pixels** | v1 | Kitty graphics and sixel need the cell size. spyc passes zero pixels today, but a field added later would break the freeze. | all |
| R8 | Relay every byte over the connection | v1, mandatory | A forwarded socket can't carry a file descriptor, so relaying is the one path that works everywhere. | all |
| R8a | Hand the PTY master's file descriptor to a local owner | never | Relaying costs about 10 µs a hop at the median and under 80 µs at p99 for two hops (`PANE_JAR_SPIKE.md`, stage 0b), against 16 ms for a frame. Not worth a second path. | U1 |

### The event log

The ring holds **events**, not just bytes: output chunks and resizes, each at
the byte offset where it happened. Bytes alone can't rebuild a screen, because
a resize changes how every byte after it lays out, and a resize never shows up
in the output.

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R9 | Number every output chunk by its **absolute byte offset since spawn** (64 bits) | v1 | Snapshots, gaps, readers catching up and recordings all count from it. | all |
| R9a | Record every resize in the log at the offset where it took effect | v1 | Replaying the tail at the wrong size draws the wrong screen. | U1, U2, U3 |
| R10 | Stamp every event with a monotonic time | later | Gives readers without hooks a working dot, the away recap, replay timing. Moved to later by decision B: an owner is there to keep its own times. | U6, U8, U9 |
| R11 | **While an owner is connected**, it acknowledges what it has parsed ("through offset N"), and the jar stops reading the PTY when the owner falls a ring behind | v1 | A slow owner must slow the program, as `PtyHost`'s bounded channel (`READER_CHANNEL_CAP`) does today. Dropping bytes under a live owner would corrupt a plain `cat` of a big file. | all |
| R12 | **While no owner is connected**, drop the oldest events when the ring fills (`drop`) | v1 | A crash window must never stall an agent. | U2 |
| R12b | The `block` and `spill` modes ([below](#when-the-ring-fills-with-no-owner)) | later | Moved to later by decision B: with no long ownerless stretches, they only matter if a crashed spyc isn't relaunched. | U2 |
| R12a | Mark a gap with the offsets it spans | v1 | A spyc that finds a gap forces a redraw rather than trusting a broken replay. | U2, U3 |
| R12c | **Across a gap,** spyc starts from the newest snapshot under it, applies the bytes after the gap, then makes the program repaint | v1 | Stage 4 (Q7): a repaint redraws the screen but sets no mode again. From an empty engine a full-screen Claude came back without the alternate screen, mouse reporting or bracketed paste; from the snapshot every mode matched a run that lost nothing, and the history before the gap stayed. Decided v1 by the owner. | U2 |
| R13 | Ring, spill and snapshot sizes set per jar, with a report of what each holds | v1 | Many jars run at once; memory has to be visible and bounded. | all |

### Input and the program's questions

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R14 | Take input as frames of whole keys, written whole and in order. A frame never splits an escape sequence; a bracketed paste is one frame. | v1 | With more than one writer, a split arrow key becomes garbage. Holds even with no keyboard lease. | U7 |
| R15 | Replies to the program's terminal queries are their own frame type, sent only by the owner, never held up by the keyboard lease | v1 | Programs ask the terminal for the cursor position (DSR), its identity (DA1), colours (OSC 10/11) and keyboard modes; spyc's engine answers (`ReplyWriter`). One answer per question, from the owner, even while someone else types. | U5, U7 |
| R15a | **A replayed tail never answers anything.** spyc parses replayed events with replies suppressed, and answers only queries that arrive live. | v1 | Replaying a tail makes the engine see old questions again. Answering them now would feed the program stale replies it reads as keystrokes. | U1, U2 |
| R15b | **Queries asked while no owner is connected go unanswered,** and the jar never answers on spyc's behalf, since it would have to parse to do so | v1 | What a program does with an unanswered query (time out, wait, misbehave) is measured in stage 2, not assumed. | U2, U3 |
| R16 | Refuse input from a connection that doesn't hold the keyboard, with a reason; never queue it | later | Only matters once there can be two writers (decision D). | U7 |

### Stored state

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R17 | A snapshot slot holding opaque bytes, a **format tag**, the offset it was taken at, and the **rows and columns at capture** | v1 | A snapshot has to say what it is and what size it describes before anything can load it. | U1, U2, U11 |
| R17a | Two formats: libghostty's **binary snapshot**, valid only for the same build and pin, and its **VT formatter emit**, portable across versions | v1 | The engine spike settled that snapshots are "transport-only — same binary, same pin — and never at-rest persistence across an upgrade". At 1.0.0 both are exact over the engine spike's 26-case corpus: 100% of rows, cell text, attributes and scrollback (`docs/archive/VT_ENGINE_SPIKE.md` → "Rehydration: both mechanisms are now exact"). The formatter's one-row offset and 99.9% scrollback belonged to the earlier `1f5bb576` pin. An exact portable form is what makes an upgrade across a pin bump lossless. The corpus didn't exercise everything: R17c lists what the raw emit misses. | U1, U11 |
| R17b | **Every snapshot stores both forms,** in every jar with an engine behind it: the binary one taken under the pane's parser lock, the portable one made from it in a scratch engine, off the lock. A restoring spyc loads the binary one when its build decodes it, else the portable one | v1 | Decision I. A crash followed by a build change otherwise leaves a pane empty, and the user did nothing wrong. Off the lock, the portable form costs the pane's own thread about 40 ms at a full scrollback, and the lock is held only for the binary encode, about 6 ms. | U1, U2, U11 |
| R17c | The portable form is the formatter's emit **plus what its extras don't carry:** the primary screen and its history under a full-screen program (the formatter writes only the screen showing), soft wraps (an unwrapped emit, so the restoring engine wraps again), the saved cursor (DECSC), a cursor shape other than the default, and the title. It is checked against the binary form as a control, case by case | v1 | Measured on the spike (`PANE_JAR_SPIKE.md` → "Decision I: what a portable snapshot costs and loses"): with these, 24 of 26 cases come back exact. The two that don't are kitty keyboard flags below the top of their stack and DECSCA protection, and spyc reads neither. | U1, U11 |
| R17d | Panes **take turns snapshotting:** a few at a time, not every pane at once | v1 | Thirty full panes snapshotting together held each pane's lock for 46 to 62 ms against 6 ms alone, enough to drop a frame. The spike lets two run at once. | all |
| R17e | **A snapshot every five seconds while output flows,** as well as after a second of quiet | v1 | Decided by the owner after stage 4. Through a long stream the snapshot under a gap (R12c) would otherwise predate the stream, losing its start and any mode it changed. Each costs the pane's lock about 6 ms; the spike showed none during a 14-second stream without it, and one every five seconds with it. | U2 |
| R18 | A small key/value store the owner writes: pane id, title, agent kind, session id, project, and whether it's a pane, a task or a capture | v1 | After a crash, a spyc learns what a jar is from the jar, even when the session manifest is stale. Stage 6 found it needed for a jar no session describes. Built before the freeze: spyc writes `is`, `label`, `agent`, `project` and `project_home` as it opens a tab, the jar keeps them when its owner goes, and after a `kill -9` of spyc, `spyc --jars` names an orphaned tab `zsh (other in work)` rather than by its launch command. | U2, U10, U12 |
| R19 | Keep all of it in memory, never on disk, except in `spill` mode. An upgrade's blob travels through the holder and a pipe, so it never touches a file | v1 | [Where the jar keeps its state](#where-the-jar-keeps-its-state). Until the review (decision K) the blob went through an unlinked file, sealed with AES-256-GCM because macOS has no `memfd`, so only ciphertext could reach a disk. The holder already lives across the `exec`, so it takes the blob and writes it into a pipe the new image reads: nothing touches a file on either platform, and no key or cipher is left in what freezes. | all |

### Connections

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R20 | Roles in the `hello`: **owner**, **keyboard**, **reader** ([below](#who-may-connect)) | the field, the owner and a read-only reader v1; the keyboard later | The role field has to exist from the first byte. A reader that only watches and describes is v1 because `spyc --jars` and an upgrade request already use it (decision D). | U5, U6, U7 |
| R21 | The owner's lease ends when its connection drops **or** it misses a liveness deadline | v1 | A dead process closes its socket, but a hung or `SIGSTOP`ped spyc doesn't, and neither does a forwarded socket whose far end vanished. | U2, U4 |
| R22 | The owner hands its lease to a named successor | v1 | An upgrade is a handover, not a race. Built in stage 6: `Release` ends the owner's connection, so nothing it sends afterwards acts (a closing tab's `Kill` among them). Before the freeze it became a change of token: the token in `Release` replaces the jar's own (R26), the old one opens nothing from then on, and every reader that came in on it is closed. The minute's hold that stage 6 built goes, and with it a gap: the hold didn't cross an upgrade, and the token does. | U1 |
| R23 | Taking over: a connection asks for the owner's lease; the current owner is told and drops to reader | later | Decision D. | U5 |
| R24 | The keyboard lease: taken by the first input frame when free, kept while input keeps coming, freed after an idle gap the owner sets | later | Decision D. | U7 |
| R25 | A label on every connection | later | Decision D. | U7 |
| R26 | The connecting peer is the same user (`SO_PEERCRED` / `getpeereid`) **and presents the jar's token** for every role but a lookup ([threat model](#threat-model)) | v1 | Same-uid alone lets any agent type into any other agent, or watch it. The spike had built neither: any process of the user could read any jar's screen, or ask it to `exec` a binary of its choosing. Built before the freeze: spyc makes 32 random bytes per jar, hands them over on the jar's standard input, and keeps them `0600` in its state directory; a jar with no token refuses to run. The `hello` carries the token length-prefixed. Its peer check came last: a jar refuses a peer of another user before reading its `hello`, by `SO_PEERCRED` on Linux and `getpeereid` on macOS (through `nix`, since rustix has it on Linux only), and on any other platform it accepts nobody. | all |
| R27 | Ping and pong, which carry the liveness deadline | v1 | R21 needs a heartbeat, and the same frames keep an idle forwarded connection alive ([Keep-alive](#keep-alive)). | U2, U4 |

### Events

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R28 | Tell every connection when the program exits, a gap opens, the keyboard or the owner changes, or a snapshot is stored | v1 | Readers have nothing else to go on. | U5, U6, U7 |
| R29 | Carry an agent's status (working, blocked, done) to readers | later | Hooks and MCP reach the owner's spyc. A status channel the owner writes and the jar only relays would fix that. | U5, U6 |

### Discovery and lifetime

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R30 | **An instance id** for every spyc, naming its MCP socket (`spyc/<instance>/mcp.sock`). **A jar id** for every jar, naming its socket in one directory per user (`spyc/jars/<jar-id>.sock`), not under the instance that spawned it. Both under `$XDG_RUNTIME_DIR` (`$TMPDIR` on macOS), `0700`, within the 104-byte path limit. | v1 | The MCP socket keyed by PID today is what strands agents on an upgrade. A jar can change owner, even to another spyc instance when its project moves (U13), so its name can't belong to the instance that spawned it. | U1, U2, U13 |
| R30a | **The jar records its owner's MCP socket,** and a pane's MCP proxy asks its own jar where to reconnect | v1 | The jar is the one rendezvous that outlives every spyc. A proxy that finds its owner through its jar follows an upgrade, a crash and a project move alike, without being told the new path. It asks as a **lookup**, which needs no token ([threat model](#threat-model)). Built in stage 6 for the MCP proxy and the status hooks: after a project moved, and after a `kill -9` of spyc and `spyc -r`, a Claude tab's spyc tools answered from the new spyc without the agent restarting. | U1, U2, U13 |
| R31 | A jar describes itself: id, instance, created time, jar version, child PID, overflow mode, linger policy | v1 | `spyc --jars` lists them, and a spyc after a crash finds them. Built in stage 4 as `key`, `value` pairs, so a newer jar adds keys an older spyc can still show; its instance and start time survive an upgrade. | U2, U10 |
| R32 | A stale socket (its jar gone) is recognized and removed, and a live jar whose socket has gone makes it again | v1 | A crash of the jar itself mustn't leave a path that looks alive. macOS removes what's left untouched in `$TMPDIR` for days, and nothing touches a listening socket's file. Built before the freeze: every two seconds a jar checks that its path is there, and binds a new socket at it, in a `0700` directory, when it isn't. | U2 |
| R32a | **A linger policy per pane:** how long the jar keeps its program running with no owner: until an owner returns, for a set time, or not at all | v1 | With decision B, no owner means a crash. Each pane says whether its program should wait for a new spyc or end with the old one. Built in stage 4: at the end of its linger the program gets `SIGHUP`, as from a closed terminal, then `SIGKILL` two seconds later. A jar with a finite linger gives its first owner ten seconds to connect. | U2 |

### The jar itself

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R33 | **Upgrade in place:** the jar asks the new binary to validate a versioned handover blob, and only if it accepts does the jar `exec` it, keeping its PID, child, PTY and connections. The blob, the listener, the PTY master and every connection cross as descriptors over a socketpair that becomes the new image's standard input, while the new binary, started as a **holder**, keeps copies of every socket until the new image has its own | v1, and only v1 | The escape hatch for every change this document gets wrong. A validation that fails leaves the old jar running. Stage 3 built it (`PANE_JAR_SPIKE.md` → "Stage 3: the jar upgrades itself"). The holder came before the freeze: on macOS an upgrade under load cut every connection, because the kernel flushes a unix socket whose every reference is in flight ("Before the freeze" there). The validator's and the holder's command lines are part of what freezes, since an old jar runs the new binary's. Since the review the holder also carries the blob, through a pipe, and a validator reads it from a socket, so one that refuses without reading can't raise `SIGPIPE` in the jar. It can't arrive later: a jar that doesn't know the request can never be upgraded, so the first released jar has to carry it. | all |
| R34 | A `hello` that lists capabilities, and messages flagged critical or ancillary ([below](#how-the-protocol-grows)) | v1 | How every later requirement arrives without a new version. Stage 3 found the first `hello` already reads the fields it knows and ignores what follows, so capabilities can follow them; that rule is part of what freezes. Since R26 the token comes first, length-prefixed, and capabilities after it. Since the review a `welcome`'s first field, `refused`'s reason and the start's `--wire` are in what freezes too ([Meeting another version](#meeting-another-version)). At the freeze the `hello`'s version became the highest its client speaks, and the `welcome`'s the one the connection speaks, so a later spyc meets an older jar without a refusal. | all |

### Never in the jar

Parsing terminal output. Answering terminal queries. Knowing about agents,
hooks or MCP. Drawing anything. Reading a config file. Listening on the
network (SSH does that). Accepting a connection from another user. Sending
a program anything a client didn't send it, keep-alive bytes included. Logging
output by default.

## Where the jar keeps its state

In its own memory, and nowhere else unless a pane asked for `spill`.

- **Disk can't outlive what it describes.** If the jar dies, the PTY closes and
  the program gets `SIGHUP`, so there is no live screen left to restore. A
  reboot kills the program too, and that case belongs to `spyc -r` and the
  agent's own resume.
- **An agent's durable record is already on disk, and it isn't the screen.**
  Claude's transcript, Codex's rollout and agy's session files are what `^a v`
  reads. The terminal is the disposable view of them.
- **Agent output carries secrets.** Keys, tokens and `.env` contents scroll
  past. Memory behind a `0700` directory exposes far less than a file left
  behind.

The sizes are bounded: at spyc's full 10,000-row scrollback, a pane's two
snapshots come to about 1.4 MB for plain text, 4.5 MB for a coloured shell
and 6.4 MB for an inline agent, and
while an owner is connected the ring only holds what the owner hasn't
acknowledged yet (R11).

## When the ring fills with no owner

With an owner connected, the ring never overflows: the owner's acknowledgements
pace the program (R11). The modes below apply only while no owner is
connected, which with decision B means after a crash, until a new spyc
arrives. A pane's mode is chosen when it's spawned; spyc picks it and the jar
only follows it. `drop` is v1; `block` and `spill` are later (R12b).

| Mode | When the ring is full | When an owner returns | For |
|---|---|---|---|
| **`drop`** (the default) | The program runs on at full speed. The jar discards the oldest events and marks the gap. | spyc starts from the newest snapshot under the gap, applies the bytes after it, and makes the program repaint by changing the PTY's size by a row and back (R12c). The repaint redraws the screen; the modes come back from the snapshot. A `SIGWINCH` with no size change repaints nothing: neither Claude Code nor spyc redrew on one (stage 0b). A full-screen program such as Claude Code redraws itself on a real change. | Agents, and anything whose durable record isn't the screen. |
| **`block`** | The jar stops reading the PTY; the program waits on its next write. Nothing is lost. | Everything replays; no redraw needed. | Panes where every byte matters more than progress while you're away. |
| **`spill`** | The ring overflows into a file in the jar's `0700` directory, removed when the jar ends. | Everything replays from the ring and the file. | A log you want in full, knowing the file holds whatever the program printed. |

The jar never pauses a program by itself. It can't tell a program is "about to"
fill the ring without understanding its output, and `SIGSTOP` would freeze an
agent's API stream along with its output. `block` gets the same effect from
terminal flow control, and only when the pane asked for it.

Because a spyc stays the owner through a detach, the ring needs to cover a
crash window, seconds to minutes, not hours.

## Tasks and captures

`PtyHost` backs three things today: pane tabs, background tasks, and `!`
captures (`spawn_capture`), and `demote_pane_to_task` moves a pane's host into
a task. The proposal is that all three run in jars, so there is one way to
host a program:

- **A pane** gets `drop` and lingers until an owner returns. So that one no
  spyc takes back doesn't run on unseen, a spyc's start notes the live jars
  with no owner, by label and project (built before the freeze).
- **A task** keeps the jar it had as a pane when demoted; the owner rewrites its
  key/value entry from pane to task. Its mode and linger stay as they were.
- **A capture** lingers a minute: a capture nobody comes back for is cleaned
  up, since a restored session doesn't reopen it. Its output is the point, so
  it should get `block` once R12b lands; until then a crash window can drop
  part of it.
- **spyc's outer jar** lingers until a terminal attaches again.

Decided by the owner after stage 4. Lingering matters only after a crash (or
a detach, for the outer jar), since quitting closes a spyc's jars. A pane that
waits for an owner however long can be forgotten, so a spyc that starts and
finds jars no spyc owns says how many, and offers to restore or discard them;
`spyc --jars` lists them.

## How spyc outlives its terminal

Decision B says spyc outlives its terminal; it leaves open how. Stage 2b of the
spike compares two shapes on the same harness and recommends one.

**Shape 1: a headless handover.** Detaching hands ownership of every pane jar
(R22) to a spyc started with no terminal, and attaching hands it back: the
same path as an upgrade. It needs a way for `App::run` to run without raw mode,
an input thread or drawing, and every attach rebuilds the Model from the
manifest, so anything the manifest doesn't carry resets daily.

**Shape 2: spyc in its own jar.** spyc's TUI runs as the program of an outer
jar. Detaching and attaching are a client disconnecting and reconnecting, as
with tmux or dtach. The Model never moves, MCP connections never drop, and an
SSH connection that dies kills only its relay. Panes are jars, and spyc is a
jar of jars. **Decided after stage 2b: this shape, entered lazily.** A bare
launch stays a plain spyc in its terminal; the first detach hands spyc over,
once, to a spyc running in an outer jar, through the manifest and the
pane-jar adoption stage 2 proved (`PANE_JAR_SPIKE.md` → "Stage 2b: how spyc
outlives its terminal" has the measurements).

What shape 2 has to get right:

1. **Upgrades still hand over the Model.** An outer jar ends when its program
   does, so spyc upgrades by `exec`ing its new binary inside its own jar: same
   PID, same PTY, the attached client untouched. The Model is rebuilt from the
   manifest and spyc reconnects to its pane jars. The manifest audit still
   matters, once per upgrade rather than once per attach.
2. **Telling spyc it was attached needs an explicit event.** dtach's `-r`
   redraw methods (`none`, `ctrl_l`, `winch`) looked like the minimum, but
   stage 0b measured otherwise: reattached at the same size, spyc redrew
   nothing on `SIGWINCH`, and at any size it set none of its modes again and
   sent no queries. Only a new terminal needs probing, and only spyc knows
   what to set, so the attach event tells spyc to set its modes, probe the
   terminal and repaint in full.
3. **spyc's own terminal queries while nobody is attached.** Nothing answers
   them. spyc has history here: a cursor-position query over SSH crashed it,
   which is why it never calls ratatui's `Terminal::clear()`. Shape 2 needs an
   audit that spyc never blocks waiting on a terminal's reply, and that no
   query's reader outlives its timeout. Stage 2b found one that does: the
   image-protocol probe at launch gives up after its timeout, but its thread
   stays blocked on standard input and takes the first keys a later terminal
   sends. Probes belong to the attach (point 2), each with a reader that ends
   when its probe does.
4. **The real terminal is restored on detach.** The attach client can't parse,
   so it can't know which modes spyc set. On detach it sends a fixed reset:
   leave the alternate screen, end mouse reporting and bracketed paste, pop the
   kitty keyboard flags (a stack spyc has leaked before, #696 and #710). On
   attach spyc sets its modes again, on a terminal that may be a different
   emulator, with a different colour depth, image protocol and keyboard
   protocol.
5. **The outer jar's token matters more than any pane's.** Attaching means
   control of spyc, and through it every shell and agent, so `spyc attach`
   presents the token, and a reader of the outer jar sees every pane.
6. **Two relays per keystroke.** Each local hop should cost microseconds, but
   latency is measured end to end through both.

What shape 2 asks of the protocol, as stage 2b built it:

- **An owner with no engine,** which stores no snapshot and relies on its
  program repainting on attach. The attach client is one, and spyc repainted
  every time.
- **Raw input from a single writer:** a relay can't split bytes into whole keys
  (R14) or tell a terminal's replies from keystrokes (R15) without parsing, so a
  single writer may send unframed bytes. The attach client sends what it reads,
  as it reads it.
- **An attach event the jar's own program can see** (point 2), and a way for
  it to detach. Until the review the spike's jar sent `SIGUSR1` and the
  client changed the size by a row and back, so the resize woke spyc's loop;
  on macOS a Node program never saw the size change at all. Since the review
  the program connects to its jar as role `program`, known by its pid, and
  hears `attached` and `detached`. Its `detach` ends the owner's connection
  once spyc's last bytes, its terminal restored, have gone out, so `^d` and
  `:detach` work from inside spyc and the fixed reset is only the fallback.

## Detaching, attaching and quitting

Decided by the owner: two ways to leave spyc, and verbs to take part of it
elsewhere and pick it back up.

| Key or command | What it does |
|---|---|
| `^d`, `:detach` | **Detach.** spyc leaves the terminal. Every jar keeps running, and spyc stays their owner (decision B). |
| `:detach project [name]` | Detach one project: the one in view, or the one named. Its jars keep running, ready to be attached elsewhere (U13). |
| `:detach tab [n]` | Detach one tab: the active one, or tab `n`. |
| `:attach` | List what is detached (whole spycs, projects and tabs) and pick one to attach. |
| `:attach project [name]`, `:attach tab [id]` | Attach a detached project, or a detached tab into the project in view, so this spyc owns it. |
| `spyc attach [name]`, from a shell | Reattach a terminal to a detached spyc. |
| `^D`, `Q`, `ZZ`, `:q` | **Quit.** End this spyc's jars, and spyc with them. Asks first while an agent or task is running, as quitting does today. Jars owned by another spyc are untouched. |

`^d` quits today, alongside `Q`, `ZZ` and `:q`; detaching takes it over, and
the other three stay a full quit. **`^d` detaches only when something is
running that's worth keeping:** an agent, a tab or a task. With nothing
running it quits exactly as it does today, so a file-commander user who
presses `^d` from habit never leaves a spyc running out of sight.

**`^D` needs the kitty keyboard protocol.** `^d` and `^D` are the same byte
(`0x04`) unless the terminal reports Shift with a control key, as Ghostty,
Kitty and WezTerm do through the kitty keyboard protocol (the same limit as
`Shift-Esc` for the `Esc` guard). Elsewhere `^D` arrives as `^d` and detaches.
The fallback is the harmless one, and `Q`, `ZZ` and `:q` quit on every
terminal.

**A detached project or tab still has an owner.** With decision B nothing is
left ownerless except by a crash, so detaching a project or a tab hands its
jars to a spyc started for it: in shape 2 a spyc in its own outer jar, in
shape 1 a headless one. `:attach` takes them from that spyc, which ends once
it owns nothing. Which spyc that is gets decided with the shape (stage 2b);
moving a project is stage 6.

## Projects

spyc holds several projects, each with its own columns, tabs and agents. Two
things a user may want of one:

**Moving a project between spycs (U13).** Detach a project from one spyc and
attach it to another, in another terminal or on another machine over SSH. For
the jars it is many owner handovers (R22), one per pane jar of the project,
together with the project's part of the manifest, which manifest v2 already
keeps per project. Three things follow:

- **A jar's name can't belong to the spyc that spawned it** (R30): it moves.
- **Its agents' MCP connections have to follow.** An agent's proxy learned its
  spyc's socket from its environment at spawn, and nothing can change that
  afterwards. A proxy that asks its own jar where its owner is now (R30a)
  follows a move with no new mechanism, and the same lookup serves an upgrade
  and a crash.
- **The project's scope claims and its attention roll-up move with it.** The
  claims do since before the freeze: they travel in the move file, keeping
  their ids unless the receiving spyc already uses one. So does each tab's
  live status, which the agent won't send again: a Claude blocked on a prompt
  reports nothing until it's answered. Without it the receiving spyc said
  nothing needed you. A shell tab's running agent goes too, since a shell tab
  learns it from output, and a blocked agent prints none.
- **Stage 6 settled how a move survives a receiving spyc that dies partway.**
  The handover is per jar, and the project is never described nowhere. A move
  file keeps the project and its jars' tokens until the receiving spyc's own
  session is saved. Whenever that spyc dies, either the move file or its
  session says where the jars belong.

**Forking a project (U14).** A copy of the project's working set (columns,
picks, inventory, tabs), with each agent tab's conversation forked as `^a F`
forks one today (`--fork-session` for Claude, `codex fork` for Codex), perhaps
into a new worktree to try a second approach side by side. A running process
can't be copied, so a fork spawns new jars and asks nothing new of them. It is
spyc's business entirely, which keeps the jar from learning what a project is.

## Sharing

Everything that reaches beyond one user on one machine goes through a
**gateway**: a separate process that is an ordinary client of the jars (of
spyc's own jar in shape 2, or of a single pane's jar) and terminates the other
side's authentication. A gateway that crashes or is compromised can do no
more than its grants allow, and it can't take the jars down with it.

**Grants.**

- **Read-only by default.** Watching an agent, or the whole spyc, is the common
  case (U6, U15).
- **Write takes an explicit confirmation that says what it means,** in words
  like: "They can type into this pane: run shell commands as you, and approve
  this agent's tool calls." It is never a default, never inherited, and never
  implied by read access.
- **Every write grant expires** and can be revoked at once. Every grant, and
  every write made under one, is logged.
- **A grant names its scope:** one pane, one project, or the whole spyc.

**Transports.**

- **A terminal user elsewhere: SSH,** as the decision log already records
  ("Remote support, when it comes, rides SSH rather than a custom protocol, so
  SSH authenticates the client"). Someone who connects over SSH as you is you;
  someone given a grant connects as themselves, through the gateway.
- **A browser: WebSocket over TLS,** carrying the jar protocol's own frames
  (decision F). It adds a transport, not a protocol.
- **gRPC was considered and set aside.** Browsers can't speak it without a
  grpc-web proxy; in Rust it means `tonic` on `tokio`, an async runtime the
  house style rules out; and it is a large, fast-moving stack under a protocol
  meant to change rarely.

**The web viewer comes almost free with shape 2.** In shape 2 spyc's whole UI is
the output of its own jar, so a browser is one more attach client rendering a
terminal stream, with no web UI to write. It can use the engine spyc already
uses: [ghostty-web](https://github.com/coder/ghostty-web) compiles Ghostty's VT
parser to WASM (MIT, about 400 KB, a drop-in for xterm.js), and Restty and
Vercel's wterm use libghostty-vt the same way. A read-only view on a phone is
the cheapest first version. Open, and on the browser's side, not the jar's:
inline images (kitty graphics), the clipboard through OSC 52, and latency over
a real network.

## Keep-alive

An idle network connection can be dropped by a NAT, a firewall or a proxy that
sees no traffic. Each link keeps itself alive with its own mechanism, and none
of them puts a byte in the terminal stream:

| Link | Keep-alive |
|---|---|
| A client and a jar, locally or over a forwarded socket | R27's ping and pong. They are protocol frames, so they never reach the program or the screen. |
| A laptop and the host running `spyc attach`, over SSH | SSH's own: `ServerAliveInterval` on the client, or `ClientAliveInterval` on the server. These are SSH messages, invisible to the terminal, and this is the link most likely to die idle. The attach docs will recommend `ServerAliveInterval 30`. |
| A browser and the gateway | WebSocket's ping and pong control frames (RFC 6455). |

**Why no keep-alive byte in the terminal stream:**

- **Towards the program there is no safe byte.** NUL is a keystroke,
  `Ctrl+@` (`Ctrl+Space` on most keyboards): readline and emacs bind it to set
  the mark, and an agent would read a keypress.
- **Towards the terminal NUL is only nearly safe.** ECMA-48 calls it time-fill
  that a terminal *may* ignore, and most do. A relay that doesn't parse can't
  tell where escape sequences begin and end, so a byte it injects can land
  inside one and corrupt it.

**The one in-band fallback is spyc's, not the jar's.** If a path ever needed
traffic in the terminal stream itself, spyc knows where its own frames start
and end, and at an idle frame boundary could send an empty synchronized update,
`ESC[?2026h` then `ESC[?2026l`. A terminal that supports mode 2026 draws an
empty frame, and one that doesn't ignores an unknown private mode. It isn't
built unless a real path needs it.

## Fuzzing

Agents are already a same-user adversary, through prompt injection, and a
gateway brings untrusted input closer still. Every parser is fuzzed before the
protocol freezes:

- **The frame parser,** on the jar's side and on spyc's.
- **The `hello`** and capability negotiation.
- **The handover blob a new jar binary validates before the jar `exec`s it**
  (R33). The most dangerous parser in the design: a malformed blob that gets
  through takes over a live PTY.
- **The gateway's grant and authentication handling,** once it exists.
- **Sequences across the roles,** as property tests: no sequence lets a reader
  type, takes a lease without the token, splits a key frame, or answers a
  replayed query.

At the freeze the frame and blob targets ran three minutes each over the
final parsers, 14.1 million and 18.3 million inputs, with no crash, and again
after the review reopened version 1: 16.2 million and 21.1 million. The
gateway's targets wait for the gateway.

The protocol lives in its own crate under `#![forbid(unsafe_code)]`, its fuzz
targets run in CI alongside spyc's existing ones, and spyc being public makes
OSS-Fuzz an option later.

## Who may connect

Three roles, named in the `hello`. Whether more than the owner is built in v1
is decision D; the field is v1 either way.

| Role | May | Lasts |
|---|---|---|
| **Owner** (at most one) | Set the size, store the snapshot and the key/value store, choose the overflow mode and linger policy, answer the program's terminal queries, close the pane, hand the lease on. The agent's MCP connection and hooks reach the owner's spyc. | Until its connection drops or it misses the liveness deadline. |
| **Keyboard** (at most one at a time) | Type. | From its first frame until an idle gap. |
| **Reader** (any number) | Watch: the snapshot and the events after it, then the live stream. | As long as its connection. |
| **Lookup** (any number) | Ask the jar to describe itself, and ping. Needs no token. | As long as its connection. |

In Rust terms the owner is ownership: it moves on an upgrade and drops on a
crash. The keyboard is a non-lexical borrow, a `&mut` that ends at its last
use. On spyc's side the roles are typestate (`Jar<Reader>`, `Jar<Writer>`), and
the jar enforces the same rules itself.

The owner's jobs don't move with the keyboard. Every resize sends `SIGWINCH`,
and Claude Code repaints in full on each, so the size stays with the owner.
Exactly one spyc answers queries (R15). A reader that falls behind is cut over
to a fresh snapshot and never slows the program.

## Threat model

**Agents run as the same user as spyc and its jars.** Same-uid checks and a
`0700` directory keep out other users, and nothing else. Without more, any
agent could connect to any jar and type into another agent, including at its
approval prompt. No MCP tool can type into a pane today, so jars would open a
new path.

**The proposed mitigation is a token per jar.** spyc creates a random token
when it spawns the jar and hands it over on an inherited pipe, never in argv
or the environment, which other processes of the same user can read. The jar
refuses the owner and keyboard roles without it. spyc keeps the tokens in
memory and writes them, `0600`, into its state directory (`jar-tokens/`, not
the runtime directory beside the sockets), so a spyc after a crash can reclaim
its jars. Readers need the token too, because another agent's screen can hold
secrets. A project move hands each jar a new token (R22), so the spyc it left
can't come back in.

**What it covers:** an agent that finds a socket and connects to it, whether by
accident, curiosity, or a prompt injection that asks it to.

**What it doesn't cover:** a process of the same user that reads the token
file, attaches a debugger to spyc, or edits the user's shell startup files. A
same-uid process that sets out to do harm can already do all of that, so the
token raises the bar for injection without pretending to be a boundary. Where
an agent runs in a sandbox (Claude Code's, Codex's seatbelt profile), denying
it the runtime directory is the stronger mitigation, and the spyc skill can say
so.

**Other users are outside the jar's model.** The jar refuses any peer that
isn't its own user (R26), whatever token it holds. Another person reaches a
jar only through a gateway grant ([Sharing](#sharing)).

**What the MCP proxy presents: nothing (decision J).** R30a has a pane's
proxy, which runs inside the agent's own process tree, ask its jar where its
owner is. Anything the proxy holds, the agent can read, so the question
should need nothing that could also type or watch. The lookup role answers
it without a token: it may ask the jar to describe itself and ping, and every
other request is refused. What a jar describes is what any process of the
user can find already: the owner's MCP socket, which lives in the same
directory as every other spyc's, the program's command line, which `ps`
shows, and the labels spyc wrote. The token is never among it.

## Version 1 on the wire

Frozen on 2026-11-12, and reopened the same day for three changes an external
review found it needed ("The review" in the spike's report); the changes are
made and tested, and it freezes again on the owner's word. The spike's
`spyc-jar` speaks it (`spike/jar-stage-2` at `670f5b04`, a branch that never
merges), and its `tests/wire_v1.rs` pins every frame and the blob below byte
for byte. The production crate starts from these fixtures and that test.

### What freezes for good

A spyc of any later release reaches a version-1 jar through these, and asks it
to upgrade to whatever came after (R33). Nothing else has to stay.

- **The start's first word:** every command line that starts a jar names
  `--wire N`, the version its starter speaks. A jar that doesn't speak `N`, or
  is given none, exits with status 75 before it binds its socket, reads its
  token or starts the program, whatever else the line says. The flags after it
  are that version's.
- **The frame:** `len` (u32), `kind` (u8), `flags` (u8), the payload. `len`
  counts `kind`, `flags` and the payload, and is at most 16 MiB. Integers are
  big-endian and text is UTF-8. Flag `0x80` marks a frame critical: a jar that
  doesn't know a critical kind answers `unsupported` and keeps the
  connection, and one it doesn't know that isn't critical it skips.
- **The `hello`,** first on every connection and at most 64 KiB: `version`
  (u16), `role` (u8: owner 1, reader 2, lookup 3, program 4), the token's
  length (u16), the token, then capabilities, which version 1 ignores.
  `version` is the highest the client speaks.
- **The `welcome`'s first field,** the version the connection speaks: the
  lower of the `hello`'s and the jar's. Version 0 is refused.
- **`refused`:** a reason (u16), then text for people. A client acts on the
  reason, never the text, and reads a reason it doesn't know as 0.

  | Reason | Meaning |
  |---|---|
  | 0 | unspecified |
  | 1 | wrong token |
  | 2 | the jar has an owner |
  | 3 | no version this jar speaks |
  | 4 | not for this connection's role |
  | 5 | an upgrade can't start now (one is under way, or no safe point was reached); worth asking again |
  | 6 | the new binary is missing or rejected the handover; the jar runs on |
  | 7 | the upgrade failed after the new binary accepted; the jar runs on |
  | 8 | invalid: no `hello` first, a value out of range, or a kind a client doesn't send |
  | 9 | a signal for a program that has ended |

- **`unsupported`** (the kind, u8), the answer to a critical kind a jar
  doesn't know.
- **`upgrade`, `upgraded`,** an upgrade's request and its answer, besides
  `refused`. The owner may ask, and so may a reader holding the token.
- **A lookup:** no token, one question, `describe` or `ping`, answered and
  then closed, and closed unasked after 5 seconds. Its `description` carries
  at least `pid` (the jar's) and `program_pid`, so a jar whose token is lost
  can still be found and ended.
- **`SPYC_JAR_SOCKET`:** a jar puts its socket's path in its program's
  environment, so the program, and whatever it starts, finds the jar without
  working the path out again.
- **The upgrade's command lines,** since a version-1 jar runs a later
  binary's:
  - `BINARY validate-handover`: the blob on standard input, a socket, until
    its end. Exiting 0 within 5 seconds accepts it.
  - `BINARY hold-handover`: on standard input, one byte, `h`, carrying as
    `SCM_RIGHTS` the write end of a pipe, the listener and every connection;
    then the blob's length (u64) and the blob. It answers `k` once it has
    both, writes the length and the blob into the pipe, and holds the sockets
    until `SIGTERM`, or for 30 seconds.
  - `BINARY resume-handover`, by `exec`: on standard input, one message of 12
    bytes, `SPYCJARF` and the holder's pid (u32), carrying the pipe's read
    end, the listener, the PTY master, and every connection in the blob's
    order. It reads the blob from the pipe, then sends the holder `SIGTERM`
    and reaps it.
- **The handover blob, version 1,** which every later binary accepts. A blob
  with bytes after its end is refused, so the blob changes only by its
  version, and a binary takes every version a released jar writes. In order:
  - `SPYCJARH` and the version (u16, 1);
  - the configuration: the socket's path, the program's arguments (a count,
    then each), its directory (a flag, then the path), rows and columns, the
    ring's size, the flow-control window, the owner's deadline and the linger
    after an exit (in milliseconds), the linger with no owner (an option), the
    instance, the created time, the program's pid;
  - the log: the next sequence number, its head and base, then its events (a
    count; each a sequence number, then `0`, an offset and bytes for output,
    or `1`, an offset, rows and columns for a resize);
  - the state: the owner (an option), what it acknowledged, the snapshots (a
    count, each as in `store_snapshot`), the exit (a flag, then the code and
    whether a signal ended it), whether the exit was collected, whether the
    PTY reached its end, rows and columns, the next connection id, how long
    the jar has had no owner (an option, in milliseconds), the foreground
    group, whether it ever had an owner, the token, and the owner's key/value
    pairs (a count, then pairs);
  - the connections (a count, at most 200): each one's id, role, cursor (a
    flag, then a sequence number and an offset), whether it was told of the
    exit, and the foreground group it was last told.

  Lengths and counts are u32, and an option or a flag is a byte, `0` or `1`,
  with an option's value after a `1`.

### Meeting another version

Each direction has one rule:

- **A later spyc, a version-1 jar.** Its `hello` names its own version; the
  welcome answers 1, and the connection speaks version 1. When it needs what
  version 1 lacks, it asks the jar to upgrade.
- **A spyc, a `spyc-jar` beside it of another release** (a package upgraded
  while spyc runs). The start names `--wire N`; a jar that doesn't speak it
  exits with 75 before anything starts, and spyc runs the program without a
  jar rather than fail the tab. A jar speaks every version from 1 to its own
  unless a release says it stopped.
- **An older spyc, a newer jar** (a downgrade). The jar welcomes the older
  version if it still speaks it, and otherwise refuses with reason 3; spyc
  then marks the tab detached and starts nothing. A jar can't be upgraded back
  down: the older binary's validator refuses the newer blob (reason 6), and
  the jar runs on.
- **Across an upgrade,** every connection keeps the version it was welcomed
  at: the new image speaks each the version it was welcomed with.

### The rest of version 1

Every other kind changes by capability, or after an upgrade. The version moves
only for a change no capability can express.

- **A payload grows only at its end.** A fixed payload is read field by field
  and whatever follows its last field is ignored: a later jar's `welcome` can
  list its capabilities there, and a version-1 jar's lists none. A payload
  whose last field runs to the end of the frame (`input`, `reply`, `release`,
  `set_info`, `upgrade`, `output`, `refused`, `upgraded`, a snapshot) can't
  grow, so a change to one is a new kind.
- **Who may send what.** A lookup: `describe` or `ping`, once. The program:
  `describe`, `ping` and `detach`. A reader: those but `detach`, and `replay`,
  `fetch_snapshot` and `upgrade`. The owner: everything but `detach`. Anything
  else is refused with reason 4.
- **The program** is the jar's own child, known by the pid the kernel records
  as it connects, and needs no token. It hears `attached` when an owner
  connects (at once, if one is there when it connects) and `detached` when the
  owner goes. Its `detach` ends the owner's connection once everything the
  program printed before has gone out on it, so its last words (its terminal
  restored) reach the client.
- **What a jar refuses at once** rather than store and fail on later: a
  `release` token under 16 bytes, a snapshot past what the program printed,
  and a signal once the program has ended.

| Kind | Name | Payload |
|---|---|---|
| 1 | `hello` | version (u16), role (u8), the token's length (u16), the token, capabilities |
| 2 | `input` | bytes for the program |
| 3 | `resize` | rows, columns, pixel width, pixel height (u16 each) |
| 4 | `ack` | offset (u64) |
| 5 | `replay` | from (u64) |
| 6 | `signal` | i32: 1 interrupt, 2 terminate, 3 kill, 4 stop, 5 continue, 6 hang up |
| 7 | `store_snapshot` | format (u16: 1 binary, 2 VT), offset (u64), rows and columns (u16), the snapshot |
| 8 | `fetch_snapshot` | none |
| 9 | `ping` | n (u64) |
| 10 | `ack_exit` | none |
| 11 | `reply` | bytes answering the program's query (R15) |
| 12 | `upgrade` | the new binary's path |
| 13 | `describe` | none |
| 14 | `release` | the jar's new token (R22), 16 bytes or more |
| 15 | `set_info` | the key's length (u32), the key, the value (R18) |
| 16 | `detach` | none; from the program only |
| 64 | `welcome` | version (u16), the program's pid (u32), head and base (u64), rows and columns (u16) |
| 65 | `output` | offset (u64), bytes |
| 66 | `resized` | offset (u64), rows and columns (u16) |
| 67 | `gap` | from and to (u64) |
| 68 | `snapshot` | `0`, or `1` and a snapshot as in `store_snapshot`: every one held, then `0` |
| 69 | `pong` | n (u64) |
| 70 | `exited` | code (i32), whether a signal ended it (u8) |
| 71 | `refused` | the reason (u16), then why, as text |
| 72 | `unsupported` | the kind (u8) |
| 73 | `foreground` | the foreground process group (u32) |
| 74 | `upgraded` | the new build, as text; the one kind that isn't critical |
| 75 | `description` | pairs, each a length (u32) and text: the key, then the value |
| 76 | `attached` | none; to the program |
| 77 | `detached` | none; to the program |

A version-1 jar describes itself with `id`, `instance`, `created`, `jar`,
`pid`, `program`, `program_pid`, `cwd`, `size`, `overflow`, `linger`, `owner`,
`owner_pid` (the owner's process, when there is one), `log`, `snapshots`, and
`exit` once its program has ended, then every pair its owner set (spyc's `is`,
`label`, `agent`, `project`, `project_home` and `owner_mcp`).

**Not frozen:** the start's flags after `--wire`, which belong to the version
it names (and the token of 16 bytes or more on standard input, which then
closes); where spyc keeps tokens; and the wording of a description's values
and of a refusal's text, which are for people to read.

### Fixtures

One frame of each kind, as the spike's test pins them:

```text
hello (owner)    000000170180000101001030313233343536373839616263646566
hello (lookup)   0000000701800001030000
input            0000000502806c730d
resize           0000000a03800028007800000000
ack              0000000a04800000000000001000
replay           0000000a05800000000000000000
signal           00000006068000000001
store_snapshot   00000012078000020000000000000006000200036869
fetch_snapshot   000000020880
ping             0000000a09800000000000000007
ack_exit         000000020a80
reply            000000060b801b5b306e
upgrade          000000080c802f62696e2f6a
describe         000000020d80
release          000000120e8066656463626139383736353433323130
set_info         0000000c0f8000000002697370616e65
welcome          0000001c40800001000010e1000000000000007d0000000000000064001e0064
output           0000000c418000000000000000646869
resized          0000000e42800000000000000066001e0064
gap              00000012438000000000000000000000000000000064
snapshot (none)  00000003448000
snapshot (some)  0000001344800100020000000000000006000200036869
pong             0000000a45800000000000000007
exited           0000000746800000000f01
refused          00000006478000016e6f
unsupported      00000003488063
foreground       00000006498000001092
upgraded         000000044a006232
description      000000104b800000000269730000000470616e65
hello (program)  0000000701800001040000
detach           000000021080
attached         000000024c80
detached         000000024d80
```

The sample blob (`handover::sample_blob`): a program `/bin/sh -c 'echo hi'` with
an owner and a reader connected, four events in its log, both snapshot forms,
and an exit not yet collected:

```text
535059434a41524800010000001a2f746d702f737079632f6a6172732f73616d
706c652e736f636b00000003000000072f62696e2f7368000000022d63000000
076563686f20686901000000042f746d70002800780000000000400000000000
00001000000000000000002710000000000000ea600100000000000493e01234
56789abcdef0000000006aee4480000010e10000000000000007000000000000
007d000000000000006400000004000000000000000300000000000000006400
00000668656c6c6f20000000000000000401000000000000006a001e00640000
00000000000500000000000000006a00000007776f726c640d0a000000000000
00060000000000000000710000000c1b5b316d626f6c641b5b306d0100000000
00000002000000000000006e000000020001000000000000006a001e00640000
00030102030002000000000000006a001e0064000000091b5b4868656c6c6f20
0100000000000001001e00640000000000000004000000109201000000103031
323334353637383961626364656600000001000000096f776e65725f6d637000
0000122f746d702f737079632f6d63702e736f636b0000000200000000000000
0201010000000000000007000000000000007d00000010920000000000000003
02000000000000
```

## How the protocol grows

- **Capabilities in the `hello`.** Each side lists what it supports. The version
  number moves only for a change no capability can express.
- **Critical and ancillary messages.** Every message carries a flag, as PNG
  chunk names do. A jar that doesn't know an ancillary message ignores it. One
  that doesn't know a critical message answers "unsupported" and keeps the
  connection open.
- **Messages are length-prefixed,** so an unknown one can always be skipped.
- **Every released jar stays a test fixture.** CI runs each against the current
  spyc, and a fuzzer runs over the parser.
- **R33 is the fallback.** When a change can't be additive, the jar upgrades
  itself.

## Decisions for the owner

Open unless marked decided:

- **A. Decided: the decision log replaces the daemonized monolith** with a
  jar per pane (2026-11-12). `ROADMAP.md` → "The 3.0 horizon: Slow Cooker
  (durable sessions)" described one headless spyc and "One attached client at
  a time", and `docs/COMPETITIVE_REVIEW.md` kept "resilient autosave, not a
  daemon" as spyc's answer to herdr. Both now describe the jar: one owner per
  pane with read-only readers, remote over SSH, and spyc still one process.
- **B. Decided: spyc outlives its terminal.** Detaching never leaves a pane
  jar without an owner, so MCP, the hooks' target and the scope registry stay
  up while you're away, and a jar is ownerless only after a crash. Moved to
  later as a result: R10 (timestamps, the recap from the jar) and R12b
  (`block` and `spill`); R32a's linger now covers crash windows only. The
  shape is decided too, after stage 2b: **shape 2, spyc in its own jar,
  entered lazily** ([How spyc outlives its terminal](#how-spyc-outlives-its-terminal)).
  A separate MCP daemon was considered and rejected: the
  MCP tools are a view onto the Model, and splitting the Model in two would
  need a protocol between the halves.
- **C. Decided: no standalone tool.** dtach, abduco, shpool and zmx already
  keep a shell alive; this jar earns its place through spyc (snapshots, the
  owner role, MCP routing, project moves). It is built in spyc's repository as
  its own crate and binary and shipped with spyc. Its protocol stays private,
  so U10 (jars started with no spyc) is out of scope. The crate boundary keeps
  publishing it possible later.
- **D. Decided: the owner and a read-only reader in v1; the rest later.** A
  reader watches and describes; `spyc --jars` and an upgrade request already
  use one. The keyboard lease, taking over, labels and refusing input (R16,
  R23 to R25) are later, since a jar gains them by upgrading itself (R33), so
  the spike's stage 5 isn't built.
- **E. Decided: no `unsafe` in the jar.** The review asked whether `fork`, `setsid` and a
  controlling terminal need it. **Stage 3 built the jar with none**, and
  without `portable-pty`: rustix opens the PTY, and the program starts through
  `spyc-jar pty-child`, which calls `setsid` and `ioctl_tiocsctty` and then
  `exec`s it, all safe calls. An upgrade's descriptors arrive as `SCM_RIGHTS`,
  which hands them over as owned descriptors; adopting inherited descriptor
  numbers instead would need `unsafe`. That is also why an upgrade's sockets
  are kept alive across the `exec` by a holder process (R33) rather than by
  inheriting them: inherited copies would then have to be closed by number.
  **Decided: no `unsafe` in the jar.** It depends on `anyhow`, `rustix` and
  `aes-gcm` (R19's seal), and on macOS `nix`, for `getpeereid` (R26).

- **F. WebSocket for the browser, next to SSH.** The decision log says remote
  support rides SSH rather than a custom protocol. A WebSocket transport for
  the browser adds a transport rather than a protocol, but it is still a
  change to that entry.

- **G. Decided: a spyc that loses its lease takes it back** when the jar is
  free, and marks the pane detached when another spyc holds it. The same
  reconnect serves a dropped connection (stage 2 found a resumed spyc showing
  a stale pane instead). Built before the freeze: a spyc stopped past the
  deadline takes its jar back on `SIGCONT` and replays what the program
  printed meanwhile; when another owner took the jar, its tab reads
  `[detached]`, and closing that tab leaves the program running.
- **H. Decided: a jar refuses to start when it can't leave its session**
  (`setsid` failing, as for a process group leader), rather than run in an
  indeterminate state that would die with its terminal.
- **I. Decided: every pane keeps both snapshot forms** (R17b to R17d). Stage 2
  found that a crash followed by a build change leaves a pane empty when the
  jar holds only a binary snapshot. A policy by kind of jar (binary only for
  agents, portable too for shells) was weighed and rejected: a tab's kind
  isn't fixed, since an agent can be started from a shell, and the owner wants
  no case where the way someone launched an agent costs them their history.
  The measurement made one rule affordable: the portable form costs the
  pane's own thread about 40 ms at a full scrollback, off the parser's lock,
  and is smaller than the binary form for real output (2.2 MB against 4.2 MB
  for an inline agent). spyc's outer jar is the exception: it holds no engine
  and stores no snapshot, and spyc repaints on attach.

- **J. Decided: a lookup needs no token.** R30a's proxy asks as a lookup,
  which may only describe and ping ([threat model](#threat-model)). The
  alternative, a second credential that opens only the lookup, gives the
  agent something to hold and leak for no gain: what it would protect is
  already readable. Since the review a lookup asks one thing and is closed,
  so tokenless connections can't pile up or hold an upgrade off.
- **K. Decided: version 1 reopens for the review** (2026-11-12). An external
  review found three things the frozen surface needed: a rule for each
  direction of version skew, with `welcome`, `unsupported`, a lookup's
  `describe` and the start's `--wire` frozen too; a reason on `refused`; and
  the blob carried by the holder through a pipe, not sealed in a file (the
  owner adopted this one). The attach and detach events for the program came
  with them, since the signal and size change they replace failed on macOS.
  Its other findings belong to production (`JAR_PRODUCTION_PLAN.md`
  is revised for them).

Smaller ones: the keyboard's idle gap (about a second is proposed); `spill`
stays opt-in; R29, status for readers.

## Open questions for the review

- Is there a use case missing from the table, and what does it need from the
  jar?
- Which v1 requirement could be dropped without a breaking change later, and
  which **later** one couldn't be added as a capability?
- Does anything here assume Linux or macOS in a way that breaks on the other?

## Sources

- [coder/ghostty-web](https://github.com/coder/ghostty-web), and its
  [0.4.0 README](https://cdn.jsdelivr.net/npm/ghostty-web@0.4.0/README.md)
- [Restty](https://gh.r-mj.com/wiedymi/restty)
- [Vercel's wterm](https://p.codekk.com/detail/typescript/vercel-labs/wterm)
- [dtach](https://dtach.sourceforge.net/), the stand-in for stage 0b
