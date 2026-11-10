# The jar: requirements before the freeze

**Status:** requirements, proposed, revised after a design review. Every
verdict below is a proposal for the owner to change. Nothing here is a wire
format yet, and no byte fixtures exist.
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
is why the spike proves R33 early, and why several verdicts below are marked
**v1 or later**: they're v1 only if R33 fails.

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

The protocol spec, with exact byte fixtures, is written from what stages 2,
2b and 3 measure, not before.

---

## Principles

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
| U11 | **spyc on a newer engine** than the one that took the snapshot | The snapshot says what format it's in, and a portable form exists for a planned handover. |
| U12 | **Tasks and captures** survive too: a pane demoted to a task (`demote_pane_to_task`), a `!` capture still running | Everything `PtyHost` backs today runs in a jar ([below](#tasks-and-captures)). |
| U13 | **Move a project between spycs:** detach one project from a spyc and attach it to another, in another terminal or over SSH | Every jar of the project changes owner, and its agents' MCP connections follow to the new spyc ([below](#projects)). |
| U14 | **Fork a project:** a copy of its working set, each agent's conversation forked, perhaps into a new worktree | Nothing new: forking spawns new jars ([below](#projects)). |
| U15 | **Share with another person,** another user on this machine or someone elsewhere: read-only by default, write only on an explicit, warned, expiring grant | Nothing: the gateway does it, as an ordinary client of the jars ([Sharing](#sharing)). |
| U16 | **spyc in a browser,** from a laptop or a phone | Nothing beyond shape 2: the browser is one more attach client, through the gateway ([Sharing](#sharing)). |

## Requirements

**v1** means the first release. **Later** means a new capability, added
without a new version. **v1 or later** depends on R33 and on decision D.
**Never** means it stays out of the jar for good.

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
| R13 | Ring, spill and snapshot sizes set per jar, with a report of what each holds | v1 | Many jars run at once; memory has to be visible and bounded. | all |

### Input and the program's questions

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R14 | Take input as frames of whole keys, written whole and in order. A frame never splits an escape sequence; a bracketed paste is one frame. | v1 | With more than one writer, a split arrow key becomes garbage. Holds even with no keyboard lease. | U7 |
| R15 | Replies to the program's terminal queries are their own frame type, sent only by the owner, never held up by the keyboard lease | v1 | Programs ask the terminal for the cursor position (DSR), its identity (DA1), colours (OSC 10/11) and keyboard modes; spyc's engine answers (`ReplyWriter`). One answer per question, from the owner, even while someone else types. | U5, U7 |
| R15a | **A replayed tail never answers anything.** spyc parses replayed events with replies suppressed, and answers only queries that arrive live. | v1 | Replaying a tail makes the engine see old questions again. Answering them now would feed the program stale replies it reads as keystrokes. | U1, U2 |
| R15b | **Queries asked while no owner is connected go unanswered,** and the jar never answers on spyc's behalf, since it would have to parse to do so | v1 | What a program does with an unanswered query (time out, wait, misbehave) is measured in stage 2, not assumed. | U2, U3 |
| R16 | Refuse input from a connection that doesn't hold the keyboard, with a reason; never queue it | v1 or later | Only matters once there can be two writers (decision D). | U7 |

### Stored state

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R17 | A snapshot slot holding opaque bytes, a **format tag**, the offset it was taken at, and the **rows and columns at capture** | v1 | A snapshot has to say what it is and what size it describes before anything can load it. | U1, U2, U11 |
| R17a | Two formats: libghostty's **binary snapshot**, valid only for the same build and pin, and its **VT formatter emit**, portable across versions | v1 | The engine spike settled that snapshots are "transport-only — same binary, same pin — and never at-rest persistence across an upgrade". At 1.0.0 both are exact over the engine spike's 26-case corpus: 100% of rows, cell text, attributes and scrollback (`docs/archive/VT_ENGINE_SPIKE.md` → "Rehydration: both mechanisms are now exact"). The formatter's one-row offset and 99.9% scrollback belonged to the earlier `1f5bb576` pin. An exact portable form is what makes an upgrade across a pin bump lossless. | U1, U11 |
| R17b | On a **planned handover**, the outgoing spyc stores the formatter emit as well as its binary snapshot, and the incoming spyc loads the binary one only if its build matches | v1 | An upgrade is exactly when the pin moves. Without the portable form, a pin-bump release would fall back to a redraw and lose shell and inline-agent scrollback. | U1 |
| R18 | A small key/value store the owner writes: pane id, title, agent kind, session id, project, and whether it's a pane, a task or a capture | v1 | After a crash, a spyc learns what a jar is from the jar, even when the session manifest is stale. | U2, U10, U12 |
| R19 | Keep all of it in memory, never on disk, except in `spill` mode | v1 | [Where the jar keeps its state](#where-the-jar-keeps-its-state). | all |

### Connections

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R20 | Roles in the `hello`: **owner**, **keyboard**, **reader** ([below](#who-may-connect)) | the field v1; readers and keyboard v1 or later | The role field has to exist from the first byte. Whether more than the owner is implemented in v1 is decision D. | U5, U6, U7 |
| R21 | The owner's lease ends when its connection drops **or** it misses a liveness deadline | v1 | A dead process closes its socket, but a hung or `SIGSTOP`ped spyc doesn't, and neither does a forwarded socket whose far end vanished. | U2, U4 |
| R22 | The owner hands its lease to a named successor | v1 | An upgrade is a handover, not a race. | U1 |
| R23 | Taking over: a connection asks for the owner's lease; the current owner is told and drops to reader | v1 or later | Decision D. | U5 |
| R24 | The keyboard lease: taken by the first input frame when free, kept while input keeps coming, freed after an idle gap the owner sets | v1 or later | Decision D. | U7 |
| R25 | A label on every connection | v1 or later | Decision D. | U7 |
| R26 | The connecting peer is the same user (`SO_PEERCRED` / `getpeereid`) **and presents the jar's token** for owner or keyboard ([threat model](#threat-model)) | v1 | Same-uid alone lets any agent type into any other agent. | all |
| R27 | Ping and pong, which carry the liveness deadline | v1 | R21 needs a heartbeat. | U2, U4 |

### Events

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R28 | Tell every connection when the program exits, a gap opens, the keyboard or the owner changes, or a snapshot is stored | v1 | Readers have nothing else to go on. | U5, U6, U7 |
| R29 | Carry an agent's status (working, blocked, done) to readers | later | Hooks and MCP reach the owner's spyc. A status channel the owner writes and the jar only relays would fix that. | U5, U6 |

### Discovery and lifetime

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R30 | **An instance id** for every spyc, naming its MCP socket (`spyc/<instance>/mcp.sock`). **A jar id** for every jar, naming its socket in one directory per user (`spyc/jars/<jar-id>.sock`), not under the instance that spawned it. Both under `$XDG_RUNTIME_DIR` (`$TMPDIR` on macOS), `0700`, within the 104-byte path limit. | v1 | The MCP socket keyed by PID today is what strands agents on an upgrade. A jar can change owner, even to another spyc instance when its project moves (U13), so its name can't belong to the instance that spawned it. | U1, U2, U13 |
| R30a | **The jar records its owner's MCP socket,** and a pane's MCP proxy asks its own jar where to reconnect | v1 | The jar is the one rendezvous that outlives every spyc. A proxy that finds its owner through its jar follows an upgrade, a crash and a project move alike, without being told the new path. Which credential the proxy needs to ask is open ([threat model](#threat-model)). | U1, U2, U13 |
| R31 | A jar describes itself: id, instance, created time, jar version, child PID, overflow mode, linger policy | v1 | `spyc --jars` lists them, and a spyc after a crash finds them. | U2, U10 |
| R32 | A stale socket (its jar gone) is recognized and removed | v1 | A crash of the jar itself mustn't leave a path that looks alive. | U2 |
| R32a | **A linger policy per pane:** how long the jar keeps its program running with no owner: until an owner returns, for a set time, or not at all | v1 | With decision B, no owner means a crash. Each pane says whether its program should wait for a new spyc or end with the old one. | U2 |

### The jar itself

| | Requirement | Verdict | Why | For |
|---|---|---|---|---|
| R33 | **Upgrade in place:** the jar asks the new binary to validate a versioned handover blob, and only if it accepts does the jar `exec` it, keeping its PID, child, PTY and connections. The ring, snapshot and leases cross in a file descriptor. | v1 | The escape hatch for every change this document gets wrong. A validation that fails leaves the old jar running. | all |
| R34 | A `hello` that lists capabilities, and messages flagged critical or ancillary ([below](#how-the-protocol-grows)) | v1 | How every later requirement arrives without a new version. | all |

### Never in the jar

Parsing terminal output. Answering terminal queries. Knowing about agents,
hooks or MCP. Drawing anything. Reading a config file. Listening on the
network (SSH does that). Accepting a connection from another user. Logging
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

The sizes are bounded: a snapshot is a few MB at most (1.23 MB for the engine
spike's whole 26-case corpus), and while an owner is connected the ring only
holds what the owner hasn't acknowledged yet (R11).

## When the ring fills with no owner

With an owner connected, the ring never overflows: the owner's acknowledgements
pace the program (R11). The modes below apply only while no owner is
connected, which with decision B means after a crash, until a new spyc
arrives. A pane's mode is chosen when it's spawned; spyc picks it and the jar
only follows it. `drop` is v1; `block` and `spill` are later (R12b).

| Mode | When the ring is full | When an owner returns | For |
|---|---|---|---|
| **`drop`** (the default) | The program runs on at full speed. The jar discards the oldest events and marks the gap. | spyc loads the snapshot, sees the gap, and makes the program repaint by changing the PTY's size by a row and back. A `SIGWINCH` with no size change repaints nothing: neither Claude Code nor spyc redrew on one (stage 0b). A full-screen program such as Claude Code redraws itself on a real change. | Agents, and anything whose durable record isn't the screen. |
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

- **A pane** gets `drop` and lingers until an owner returns.
- **A task** keeps the jar it had as a pane when demoted; the owner rewrites its
  key/value entry from pane to task. Its mode and linger stay as they were.
- **A capture** lingers only briefly: a capture nobody comes back for is
  cleaned up. Its output is the point, so it should get `block` once R12b
  lands; until then a crash window can drop part of it.

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
jar of jars. The leaning is towards this shape, untested.

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
   audit that spyc never blocks waiting on a terminal's reply.
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

What shape 2 asks of the protocol, **open until stage 2b reports:**

- **An owner with no engine,** which stores no snapshot and relies on its
  program repainting on attach.
- **Raw input from a single writer:** a relay can't split bytes into whole keys
  (R14) or tell a terminal's replies from keystrokes (R15) without parsing, so a
  single writer may send unframed bytes.
- **An attach event the jar's own program can see** (point 2). A signal alone
  isn't enough.

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
- **The project's scope claims and its attention roll-up move with it.** Open:
  whether the handover is all or nothing, and what a half-moved project looks
  like when the receiving spyc dies partway.

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
memory and writes them, `0600`, into its instance directory so a spyc after a
crash can reclaim its jars. Readers need the token too, because another
agent's screen can hold secrets.

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

**Open: what the MCP proxy presents.** R30a has a pane's proxy, which runs
inside the agent's own process tree, ask its jar where its owner is. Anything
the proxy holds, the agent can read. So the lookup should need a credential
that answers only that one question, and reveals nothing an agent could use to
type or watch.

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

- **A. Replace the daemonized monolith in the decision log.** `ROADMAP.md` →
  "The 3.0 horizon: Slow Cooker (durable sessions)" describes one headless spyc,
  "One attached client at a time", and "no cross-machine protocols", and
  `docs/COMPETITIVE_REVIEW.md` keeps "resilient autosave, not a daemon" as
  spyc's answer to herdr. A jar per pane, with readers and SSH among its use
  cases, changes all three, and needs a decision-log entry before anything
  freezes.
- **B. Decided: spyc outlives its terminal.** Detaching never leaves a pane
  jar without an owner, so MCP, the hooks' target and the scope registry stay
  up while you're away, and a jar is ownerless only after a crash. Moved to
  later as a result: R10 (timestamps, the recap from the jar) and R12b
  (`block` and `spill`); R32a's linger now covers crash windows only. Still
  open: which shape ([How spyc outlives its terminal](#how-spyc-outlives-its-terminal)),
  decided by stage 2b. A separate MCP daemon was considered and rejected: the
  MCP tools are a view onto the Model, and splitting the Model in two would
  need a protocol between the halves.
- **C. Decided: no standalone tool.** dtach, abduco, shpool and zmx already
  keep a shell alive; this jar earns its place through spyc (snapshots, the
  owner role, MCP routing, project moves). It is built in spyc's repository as
  its own crate and binary and shipped with spyc. Its protocol stays private,
  so U10 (jars started with no spyc) is out of scope. The crate boundary keeps
  publishing it possible later.
- **D. Readers and the keyboard lease: built in v1, or reserved capabilities?**
  The role field is v1 either way.
- **E. `unsafe` in the jar.** The review asked whether `fork`, `setsid` and a
  controlling terminal need it. As far as this revision found, they don't:
  rustix 1.1.6 has a safe `setsid()` and `ioctl_tiocsctty()`, spyc can start
  the jar with `std::process::Command` (no `fork`), and the jar can spawn its
  program through `portable-pty` as `PtyHost` does today, whose `unsafe` sits
  in that dependency. The cost is the "`rustix` only" dependency target.
  Unverified until stage 2 builds it.

- **F. WebSocket for the browser, next to SSH.** The decision log says remote
  support rides SSH rather than a custom protocol. A WebSocket transport for
  the browser adds a transport rather than a protocol, but it is still a
  change to that entry.

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
