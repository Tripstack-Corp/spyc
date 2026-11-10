# Pane jar spike: upgrade spyc without killing its agents

**Status:** scoped, not started
([#1099](https://github.com/Tripstack-Corp/spyc/issues/1099)). This is the
brief for a spike; its report replaces the "Findings" section once it has run.
**Gates:** `ROADMAP.md` → "The 3.0 horizon: Slow Cooker (durable sessions)".
That horizon's scope doesn't commit until this report and
[`VT_ENGINE_SPIKE.md`](VT_ENGINE_SPIKE.md) have both reported.
**Timebox:** about a week. Everything is built on a throwaway branch; the only
code expected to reach `main` from it is the MCP work in
[Before the jar](#before-the-jar), which is worth having whatever the
spike decides.

---

## The goal

A Claude tab in the middle of a turn survives two things, and neither the agent
nor its spyc tools notice:

1. **an upgrade:** the spyc binary is replaced and spyc restarts;
2. **a crash:** `kill -9` of spyc, then launching it again.

Today both kill every agent: spyc is the parent of every pane's program and
holds every PTY master, so its exit closes the PTYs and the children get
`SIGHUP`.

## The idea: a jar that doesn't understand terminals

Each pane's program runs under its own small process, a **jar**
(`spyc-jar`: where the spice is kept). The jar:

- owns the PTY and is the program's parent, so it can always `waitpid` it;
- relays bytes between the PTY and whichever spyc is connected;
- keeps a bounded ring of output while no spyc is connected;
- stores an **opaque snapshot**: spyc's terminal-engine state, pushed by spyc,
  with the byte offset it was taken at.

A new spyc, after an upgrade or a crash, loads the last snapshot and replays the
bytes that came after it. The pane comes back exactly as it was, and the jar
never parsed a byte of it.

That last property is the point. Everything that changes often stays in spyc:
VT parsing, libghostty pin bumps, agent detection, the HUD. The jar's job is
small enough to freeze, so a jar started by one spyc keeps working with the
next ten.

## Prior art

| Project | Shape | What it teaches |
|---|---|---|
| containerd shims | One shim per container owns its stdio and exit status; a restarted containerd reconnects through the shim's socket | The daemon restarts or upgrades without touching what runs under it. The closest analogue, proven at scale. |
| conmon (Podman) | A per-container monitor that receives the console PTY's fd over a unix socket (`recvfd`) | Handing a PTY master between processes is ordinary practice. |
| zmx (2025, Zig) | One daemon per session, about 1,000 lines, using libghostty-vt to redraw the screen on reattach | The nearest design, on our engine. Its session daemon parses the terminal, so it moves when the engine does; this spike keeps parsing out of the jar. |
| dtach, abduco | Tiny per-session detachers | dtach's latest release found is 0.9, from 2016: a process with a small enough job really does stop needing releases. |
| shpool (Rust) | One daemon for shell sessions; the client checks the protocol version and warns on a mismatch | Version the protocol in its first message. |
| tmux, WezTerm's mux | One server, and a client of another version is refused ("protocol version mismatch", "codec 42 vs 43") | The cautionary tale: with one monolithic server, upgrading means killing the server and its sessions. |
| systemd's fd store, nginx's `USR2`, Envoy's hot restart | File descriptors outlive a process image: parked with the service manager, inherited across `exec`, or passed over a socket | `exec`-in-place keeps the PID, so children stay children. It's this spike's fallback. |

Sources are listed at the end.

## Platform constraints

- **macOS has no `PR_SET_CHILD_SUBREAPER`.** When spyc spawns a program and
  then exits, the program is reparented to launchd, and a new spyc can't
  `waitpid` it. kqueue's `EVFILT_PROC` reports the exit, but the exit status
  isn't reliably available. A jar that stays the parent avoids the problem
  on both platforms.
- **A PTY master can be passed over a unix socket** (`SCM_RIGHTS`, as conmon
  does), and `rustix` covers it with no `unsafe`.

## What spyc does today that the spike must change

1. **The MCP socket is named after spyc's PID:** `mcp-<pid>.sock`, in
   `socket_path_in` ([`src/mcp/mod.rs`](../../src/mcp/mod.rs)). A new spyc
   process gets a new PID, so every agent's `spyc --mcp` proxy would point at a
   dead path. The path has to be stable across spyc processes.
2. **The MCP proxy never reconnects.** `proxy_io`
   ([`src/mcp/server.rs`](../../src/mcp/server.rs)) connects once, and the
   first failed send returns an error and exits it. The agent then has no spyc
   tools for the rest of its session. The proxy is a second long-lived process
   per agent, and after an upgrade it keeps running the old binary's code.
3. **Panes are spawned through `portable-pty`** inside `PtyHost`
   ([`src/pane/pty_host.rs`](../../src/pane/pty_host.rs)), which calls
   `setsid` and leaves spyc as the parent. The jar takes over spawning, and
   `PtyHost` becomes its client.

Two things already in place carry most of the rest:

- **The engine can write its state back out.** The VT engine spike measured
  libghostty-vt's emit reconstructing 99.5% of scrollback, with two narrow
  emit-ordering bugs in the pre-1.0 release. spyc now pins 1.0.0, so the spike
  re-checks both.
- **The Model handover exists.** Session manifest v2 (`spyc -r`) already
  serializes projects, tabs and session pins. An upgrade is `spyc -r` with the
  tabs still alive.

## Before the jar

These are needed whatever the spike recommends, `exec`-in-place included, and
can land on `main` as ordinary PRs:

- **A stable MCP socket path,** keyed by something that outlives a spyc
  process rather than by its PID.
- **A proxy that reconnects:** on a failed send or read it reconnects with a
  bounded backoff, sends `initialize` again with its pane id, and replays the
  request it was holding. A proxy from an older spyc must keep working against
  a newer server, so the proxy-to-server exchange gets a compatibility test.

## What the spike builds

1. **`spyc-jar`,** a separate small binary. Its whole job:
   - double-fork and `setsid`, so neither closing the terminal nor spyc's
     death reaches it;
   - spawn the program on a PTY and stay its parent;
   - listen on `$XDG_STATE_HOME/spyc/jars/<pane-id>.sock`, in a directory
     made `0700`, so who may attach is settled by file permissions;
   - relay bytes, apply resizes (`TIOCSWINSZ`), and deliver signals,
     including the `SIGSTOP` behind `^z`;
   - keep the output ring, the opaque snapshot and the snapshot's offset;
   - report the exit status, and leave once spyc has acknowledged it.

   Target: under 600 lines, with `rustix` its only dependency.
2. **A jar-backed `PtyHost`,** behind a flag, covering only spawn, read,
   write, resize, signal and exit.
3. **The snapshot loop.** spyc pushes an engine snapshot into the jar when a
   pane's output goes quiet, or when the ring is about to drop the oldest byte a
   rebuild would still need.
4. **The upgrade path.** Write the manifest, exit, start the new binary,
   reconnect to each jar, load its snapshot, replay its tail.
5. **The MCP work above,** so the agent's tools survive too.

## Questions it answers

Each becomes a decision in the 3.0 plan.

| | Question | How it's settled |
|---|---|---|
| Q1 | Does the jar relay bytes, or pass spyc the PTY master (conmon-style) and drain the PTY only while spyc is gone? | The keystroke-echo latency the relay adds at p99, against the handover's complexity: two readers on one master must never overlap. |
| Q2 | A separate `spyc-jar` binary, or the spyc binary run as `spyc --jar`? | Memory per jar (the release spyc binary is about 23.6 MB, and every jar of a superseded version keeps that version's file alive) against shipping one binary through brew, apt and crates.io. |
| Q3 | How faithful is a rebuild from a snapshot plus its tail? | A cell-by-cell comparison before and after, over the VT engine spike's 26 cases, an alternate-screen program, and a Claude tab streaming output. |
| Q4 | Can the protocol be frozen? | It's written down, versioned in its first message, and an old jar runs against a new spyc in a test. |
| Q5 | How long is the gap? | Time from the old spyc stopping to the new spyc's first frame, with a target under a second. The bytes the jar took in must equal the bytes spyc parsed, with none lost. |
| Q6 | How does a jar end? | Its program exits; spyc never comes back within a timeout; the user closes the pane. No jar is ever orphaned. |

## Passing

On both macOS and Linux, through an upgrade and through `kill -9`:

- the Claude tab keeps its PID and its turn finishes;
- no output byte is lost;
- the screen matches cell for cell;
- `get_spyc_context` works from the agent afterwards without restarting it.

## The fallback: `exec`-in-place

If the jar fails Q1, Q3 or Q5, the spike measures `exec`-in-place instead,
nginx-style: spyc writes its handover, clears `FD_CLOEXEC` on its PTY masters,
and `exec`s the new binary. The PID doesn't change, so `waitpid` and the MCP
socket path keep working. It's about a day of work. It survives an upgrade but
not a crash, and it brings detach no closer.

## Out of scope

Detaching and attaching, SSH, more than one client, and Windows. The design
mustn't rule any of them out: a jar per pane is most of what a detach needs,
and a jar's socket is something `ssh` can reach.

## Findings

Not run yet.

## Sources

- [zmx: session persistence for terminal processes](https://erock.prose.sh/zmx-session-persistence)
- [containerd: containers, tasks and shims](https://thecontainerdbook.pages.dev/chapters/part-4/12-containers-tasks-shims)
- [conmon's `ctrl.c` (the console socket and `recvfd`)](https://sources.debian.org/src/conmon/2.1.13%2Bds1-2/src/ctrl.c)
- [shpool's protocol version check](https://docs.rs/crate/libshpool/0.10.1/source/src/protocol.rs)
- [tmux: protocol version mismatch after an upgrade](https://wiki.neonvagabond.xyz/content/unix.stackexchange.com_en_all_2024-05/questions/122238/protocol-version-mismatch-client-8-server-6-when-trying-to-upgrade)
- [WezTerm: mux codec version mismatch (issue 5200)](https://github.com/wez/wezterm/issues/5200)
- [dtach(1)](https://www.mankier.com/1/dtach)
- [systemd: the file descriptor store](https://systemd.io/FILE_DESCRIPTOR_STORE)
- [nginx: upgrading the executable on the fly](https://docs.w3cub.com/nginx/control)
- [Darwin: exit status from kqueue's `EVFILT_PROC`](https://lists.apple.com/archives/Darwin-dev/2009/Aug/msg00210.html)
