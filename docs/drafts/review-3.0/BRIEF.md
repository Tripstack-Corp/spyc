# spyc 3.0: pre-release code review (the brief, as given)

You are reviewing spyc (\`Tripstack-Corp/spyc\`) before its 3.0 release. This is
a review, not a remediation: you find, prove and rank; you fix nothing.

3.0 is the release that turns jars on. A \`spyc-jar\` is a small process that
holds one pane's program and its PTY and outlives spyc; spyc itself can run in
a jar of its own, so it outlives its terminal (\`^d\`, \`spyc attach\`), and a
project's jars can move between spycs. Read \`docs/drafts/JAR_PRODUCTION_PLAN.md\`
and \`docs/drafts/JAR_PROTOCOL.md\` before any code: they hold the design, the
slices S1 to S6, the owner's decisions, and the threat model.

Three facts set the stakes, and should set your priorities:

1. **Part of this can never be fixed after the tag.** A jar started by 3.0 may
   still be running under a spyc released a year later. Protocol version 1 is
   frozen, and so is everything a version-1 jar's own image does during an
   upgrade ("What freezes for good" and parts 1 to 9 of "Upgrading: where each
   check runs" in \`JAR_PROTOCOL.md\`). A defect there ships for good. A defect
   anywhere else is a patch release.
2. **It was written fast.** \`v2.4.0\` was tagged on 2026-11-09 (\`46146bf4\`). The
   first jar commit landed on 2026-11-14 (\`3a8f6532\`). \`main\` at the pin below
   is 195 commits later: +42,434 lines across 351 files of \`src/\`, \`crates/\`
   and \`tests/\`, ten days of work. Every line of jar code is inside
   \`git diff v2.4.0..HEAD\`.
3. **The owner's bar is reliability under load.** He runs spyc all day with
   agents in every tab. His stated position on the bugs found so far: a wait
   that is too short under load is not fixed by a longer wait. Treat every
   place where a clock decides a correctness question as a finding candidate,
   whatever its current value.

## Setup

1. **Pin.** The review subject is \`main\` at
   \`444857a2a32b2f284d2b4fd19e57aef19700a5c5\` (2026-11-24, \`2.5.0-CURRENT\`).
   \`git pull --ff-only\`, then record what \`HEAD\` is. If it has moved, review
   \`HEAD\`, name its SHA in every report, and re-verify each seeded lead below
   against it, since their line numbers are from the pin. Trust a function's
   name over its line number.
2. **Read \`AGENTS.md\` first** and follow it: worktree-first, spyc's MCP tools
   over shell equivalents, never pipe the gate. Then \`CONTRIBUTING.md\`,
   \`SECURITY.md\`, and the two jar documents above.
3. **Establish green before you look for red.** Run \`make check-ci\` unpiped at
   the pin. If it fails, stop and report: nothing after it means anything.
4. **Two platforms.** The owner works on an Apple silicon Mac; CI runs tests on
   Linux only (lead V2). Run the gate and your probes on macOS, and the Linux
   half (\`make lint-linux\`, \`make test\`, the Linux-only probes) in a container.
   If you have one platform, say which in every report, and list what went
   unchecked for want of the other.
5. **You are probably running inside the thing under review.** The owner
   dogfoods: your pane is likely a jar, and his other tabs are live agents in
   live jars. Never signal, upgrade, attach to, sweep or delete a jar, socket,
   token or store entry you did not create. Run every probe against a spyc and
   jars started under scratch \`XDG_RUNTIME_DIR\`, \`XDG_STATE_HOME\` and \`TMPDIR\`,
   as the in-repo tests do (\`src/pane/jar/test_dir.rs\`), and end each probe's
   process groups when it finishes. A probe that needs a real terminal
   emulator, a second SSH login, or a laptop that sleeps goes on the owner's
   manual checklist (below), written so he can run it in a minute; it is never
   silently skipped and never faked.

## Ground rules

- **Review only.** No fix, no PR, no commit, no version bump. Red tests and
  probe scripts are evidence and are welcome: write them in a worktree, leave
  them uncommitted, and name their paths.
- **Evidence, by the house standard.** \`AGENTS.md\`: "a test is evidence only
  once you've watched it fail", and "a finding naming N sites is closed when N
  are fixed". For you that means: a finding cites \`path:line\` at the pinned SHA
  and carries either a command with its output, a failing test, or the exact
  lines read with the reasoning from them. A finding about a pattern
  enumerates every site, with the search that found them, so its closure can
  be checked. Say which of the three kinds of evidence each finding has.
- **Verify against the tree, never against a summary**, your own subagents'
  included. Earlier engagements here caught agents overclaiming counts and
  coverage. The referee re-derives every blocker and high from the code.
- **Refute before you rank.** Each blocker and high gets an independent
  refuter whose only job is to disprove it against the tree. A finding that
  survives is \`confirmed\`; one that doesn't is \`refuted\` and stays in the
  report with the reason; one nobody got to is \`unverified\` and is labelled
  so. Nothing is dropped and nothing is promoted without a check.
- **Decided is decided.** The plan's "The owner's decisions" and the
  protocol's "Decisions for the owner" are settled: a jar per pane and not a
  daemon; one program per jar; the token raises the bar for a same-user
  process and is not a boundary; \`#![forbid(unsafe_code)]\` in the jar; one
  package with two binaries; a program runs in a jar or not at all, with a
  jarless session only when the user chooses it; \`^d\` always detaches; jars on
  by default in 3.0. Do not argue them. Do report code that contradicts one,
  text that misstates one, and a consequence of one that its text never
  mentions; the last is informational and addressed to the owner.
- **House style is not a finding.** Sync-only threads and \`mpsc\`, \`anyhow\`, the
  deliberate hand-rolls (\`format_size\`, \`format_age\`, \`write_atomic\`, hex
  colour parsing, the XDG resolvers, clipboard shell-outs, the keymap DSL, the
  markdown renderer, the scrollback adapter), the 1,000-line and 100-line
  ratchets, Canadian English. Whatever a guard already enforces, leave to the
  guard; a guard that checks less than it claims is a finding.
- **Where the code and a document disagree, say which is wrong**, and why you
  think so. Do not average them.

## Stop and report if

- \`make check-ci\` is red at the pin before you change anything.
- \`main\` moves under you in a way that rewrites the jar's frozen surface
  (\`crates/spyc-jar/src/proto.rs\`, \`handover.rs\`, \`upgrade.rs\`, \`store.rs\`,
  \`tests/wire_v1.rs\`).
- You find a way for a probe to have touched the owner's live jars, or that
  one did.
- You find something that corrupts or loses a user's work in ordinary use.
  Report it at once with its reproduction; don't wait for the synthesis.
- Your budget will not cover the units. Stop fanning out, write what you have,
  and mark coverage honestly. A June review here lost its verification pass to
  a spend limit twice and left 117 findings unverified; the units below are
  ordered so that the first ones matter most if that happens again.

A seeded lead whose premise turns out false is not a stop: record it as
refuted and go on.

## How to run it

One context will not hold this. Run it as an orchestrator with one reviewer
per unit, then a refuter pass, then a referee who writes the synthesis; the
pre-2.1 review (\`docs/archive/review-2.1/\`) is the shape to follow and its
\`SYNTHESIS.md\` shows what the owner found useful. If you cannot fan out, run
the units as sequential sessions in the order given, each ending in its
report file, which is the only hand-off the next one gets.

Each reviewer reads its files whole, not by grep, and keeps a coverage ledger:
every file in its unit marked read in full, skimmed, or not read.

### Unit A. What can't be fixed later

\`crates/spyc-jar/src/{proto,handover,upgrade,store,held,lock,cli,limits,state}.rs\`,
\`tests/wire_v1.rs\`, \`tests/jar_differ.rs\`, \`tests/jar_compat.rs\`,
\`tests/jar_process/upgrade.rs\`, \`fuzz/fuzz_targets/jar_{frame,handover}.rs\`,
\`docs/drafts/JAR_UPGRADE_BRIEF.md\`.

The question: is version 1, as the bytes and as the old image's behaviour,
something a spyc two years from now can live with? Check every frozen item
against its implementation and its fixture, byte for byte. Walk the upgrade
as a state machine and try to break it at every edge: a failure before the
park, while parked, between the holder's answer and the \`exec\`, a failed
\`exec\`, a new image that can't read its descriptors, can't read the blob,
can't decode it, panics starting. For each, what does the program see, what
does the owner hear, and within what bound? Check what crosses the \`exec\` and
what does not: every descriptor's close-on-exec flag before and after, signal
dispositions and mask (\`Command::exec\` resets some in the calling process
before it fails), the lock, standard input. Check a second upgrade from an
image that itself arrived by upgrade. Run both fuzz targets long enough to
mean something and report inputs per target; read the targets to see whether
they reach \`encode\` and round trips or only \`decode\`. Ask of every
length-prefixed field: who bounds the allocation, before or after the read?

### Unit B. The jar as a long-lived process

\`crates/spyc-jar/src/jar/*.rs\`, \`pty.rs\`, \`client.rs\`, \`attach.rs\`,
\`tests/jar_process.rs\` and \`tests/jar_process/\`.

Threads, locks and condition variables: draw the lock order from the code and
look for any path that holds \`state\` across a syscall that can block, any
wake that can be lost, any thread whose death the rest don't notice. Lifetimes:
linger, exit linger, first-owner hold, the reaper, in every order of exit,
owner loss and upgrade. Flow control: window, ring, ack, gap, a reader far
behind, a program that prints faster than anyone reads, one that never reads
its input. The PTY and the program: session, controlling terminal, process
group, what the program inherits (environment, descriptors, signal
dispositions, working directory, umask), and what happens to the program's
descendants. Resource hygiene of a process meant to run for months: threads,
descriptors, memory in the log and snapshots, timed wakeups at idle.
Behaviour under load: run the jar's own tests under \`stress-ng\` or equivalent
CPU and I/O pressure, with the test threads oversubscribed, repeatedly, and
report every failure with its seed or log; a test that fails only under load
is the finding this unit exists for.

### Unit C. spyc's side of a pane's jar

\`src/pane/jar/\` (all of it), \`src/pane/pty_host*\`, \`src/pane/engine_ghostty/jar_snapshot.rs\`,
\`src/app/{pane_starts,capture_starts,jarred_tasks,jar_info,jarless,orphan_jars,jar_upgrades,pane_tabs}.rs\`,
\`src/app/harness_tests/jar_start.rs\`, \`tests/jar_adopt.rs\`.

Spawn, adopt, retake, lose, restart, close, for tabs, tasks and \`!\` captures:
for each, can the program ever run twice, run outside a jar unasked, or be
lost while its tab says it lives? Does every blocking wait stay off the loop
(SPYC-TRAP \`jar-off-the-loop\`), and does the MVU split hold: no handle in the
Model, no OS in a handler or the draw pass? Snapshots and replay: offsets
across a gap, a snapshot from another engine pin, the parser resynchronized
mid-sequence, terminal replies during and after replay. What does the user
see for each way a jar can be unwell: gone, busy, parked, stopped, hung,
refused, another spyc's? Is each state distinguishable on screen and in
\`spyc --jars\`, and does spyc ever wait on a jar with no bound or give up on
one that is only slow?

### Unit D. Sessions: detach, attach, handover, moves

\`src/pane/jar/{session,attach,shutdown}.rs\`, \`src/pane/ssh_agent*\`,
\`src/app/{detach,project_move,session,session_restore,projects,events}.rs\`,
\`src/state/sessions/\`, \`src/state/event_log.rs\`, \`src/terminal.rs\`,
\`src/terminal_probe_tests.rs\`, \`tests/{session_jar,jar_attach}.rs\`,
\`docs/features/\` pages for detaching, attaching and projects.

The lazy entry on the first \`^d\`; \`spyc attach\` from another emulator, over
SSH, while attached elsewhere, after sleep; \`:upgrade\` and the handover by
\`exec\`; \`:detach project\`, \`:detach tab\`, \`:attach project\`, \`:attach tab\`.
For each handover and move, write down the order of durable writes and
releases and find the instant at which a crash of either spyc loses a jar,
strands a token, or starts a program twice (review finding 6 is the model).
Terminal state: every mode spyc sets is restored on every way out, and no
reader outlives its probe. What a session loses across a handover against
what the docs say it loses. Session names: what characters and lengths are
accepted, and what they become as a socket path and a token file name.

### Unit E. Security posture

\`SECURITY.md\`, the threat model in \`JAR_PROTOCOL.md\`, \`src/mcp/\` (the server,
\`protocol.rs\`, the proxy), \`src/pane/jar/{place,tokens,store,orphans,listing}.rs\`,
\`crates/spyc-jar/src/jar/accept.rs\`, \`src/crash_report.rs\`, \`src/wiring/\`.

Hold the code to the model it states: the adversary is a hosted agent under
prompt injection, running as the user. For every socket, file and directory
the jar work adds, write the table the pre-2.1 review wrote for MCP tools:
who may reach it, what proves who they are, what they can do with no token,
with the token, and as each role. Check trust in both directions: the jar
checks its peer's user, but what does a client check about the jar it
connects to, on each path that connects? Check every path that sends a signal
to a pid it was told. Check what \`describe\` gives a tokenless caller against
what each caller then does with it. Then \`SECURITY.md\` itself: does it
describe 3.0?

### Unit F. Tests, CI and the release

\`.github/workflows/\`, \`Makefile\`, \`docs/RELEASE_ENGINEERING.md\`, \`build.rs\`,
\`Cargo.toml\` (both crates), \`src/{size_guard,fn_size_guard,guard_support,style_guard,link_guard}.rs\`,
every jar test file.

Does the gate test what ships, on the platforms it ships to? Is the build the
rc soaks the build the tag produces? Packaging: \`spyc-jar\` beside \`spyc\` in
every artefact (tarballs, \`.deb\`, Homebrew, \`cargo install\`), the publish
order of the two crates, \`make package-check\`. The plan promises "no timing in
tests": enumerate every \`sleep\` and fixed wait in the jar tests and classify
each as polling for a state or assuming one. Pick the ten jar tests that guard
the most, break what each covers, and report which ones failed to notice. For
each plan promise of the form "CI does X" (both platforms, a jar built to
differ, every released jar kept as a fixture, fuzz in CI), show the workflow
line that does it or say it is not there. New pins in the size ratchets since
\`v2.4.0\`.

### Unit G. Everything else in the diff, and the documents

\`git diff v2.4.0..HEAD\` minus what A to F own: the crash report (#1302), the
session's per-column cursor, picks and filter (#1316), the agy status line
(#1282), the event log, the scrollback and pane changes, the brand sweep.
Then the documents as documents: \`README.md\`, \`INSTALL.md\`, \`FEATURES.md\`,
\`CONFIGURATION.md\`, \`docs/features/\`, \`docs/config/\`, \`ARCHITECTURE.md\`'s jar
sections and trap anchors, \`ROADMAP.md\`. A new user installs 3.0 and reads
these: is anything said that the code doesn't do, or done that nothing says?
Is every \`SPYC-TRAP\` the jar work added anchored on both sides and true?

## Seeded leads

These come from a static read of the pin: all of \`crates/spyc-jar/src/jar/\`,
\`upgrade.rs\`, \`cli.rs\`, \`client.rs\`, \`attach.rs\`, \`pty.rs\`, \`lock.rs\`,
\`limits.rs\`; \`src/pane/jar/{mod,link,place,tokens,attach}.rs\`; parts of
\`src/mcp/protocol.rs\`, \`src/app/mcp.rs\`, \`src/app/project_move.rs\`,
\`src/terminal.rs\`; the workflows; and the two jar documents. Nothing was
built or run. Not read: \`handover.rs\`, \`held.rs\`, \`store.rs\`, \`proto.rs\`
beyond its message table, \`src/pane/jar/{session,shutdown,snapshots,store,orphans}.rs\`,
\`src/app/detach.rs\`, the session code, and every test body. Treat that list
as the least-examined ground, not as cleared.

The first group is what the code says, read directly; confirm it, then work
out what it costs. The second group is inference and needs a probe. Each
names the unit that owns it. They are a floor: a unit that only confirms its
leads has not done its job.

### Read in the code

**V1 (F, blocker candidate). A build versioned \`3.0.0\` runs with jars off.**
\`pane::jar::honoured()\` (\`src/pane/jar/mod.rs\`) is true only when
\`CARGO_PKG_VERSION\` contains \`-CURRENT\` or \`-rc.\`, and gates jars at
\`src/app/pane_starts.rs:126\`, \`bootstrap.rs:347\`, \`capture_starts.rs:42\` and
\`orphan_jars.rs:65\`. The plan's S6 removes it; at the pin it is still there.
As coded, every rc exercises jars and the final tag does not, so the release
ships the one configuration no rc ran. Confirm, find every call site, and
check whether anything in \`make release-prep\` or the release checklist would
catch it.

**V2 (F, high candidate). No jar test runs on macOS in CI.** \`ci.yml\` runs
\`make test\` on \`ubuntu-latest\` only. \`macos.yml\` runs fmt, clippy and a link
check, on path filters and tags. The plan says each jar test "runs on both CI
platforms". The mechanisms with no CI are the macOS-specific ones the design
leans on: the holder process that keeps sockets alive across the \`exec\`
(SPYC-TRAP \`jar-upgrade-holder\`), \`getpeereid\` and \`LOCAL_PEERPID\`, the
\`$TMPDIR\` cleaner and \`keep_socket\`, a full backlog refused like a dead
socket, the first-run scan of a new binary. The fuzz workflow is Linux and
scheduled. \`tests/jar_compat.rs\` is ignored without \`SPYC_JAR_OLD_BIN\`; find
what will pin 3.0.0's own jar as the first fixture.

**V3 (E). \`SECURITY.md\` does not describe 3.0.** It mentions jars once, in
passing (line 325). The jar's threat model, the token, the binary store, the
self-\`exec\` upgrade and \`spyc attach\` over SSH are described only in
\`docs/drafts/\`, which the published crate excludes. The frozen protocol
itself lives in a directory named drafts, and \`ROADMAP.md\`'s index still
dates the freeze 2026-11-12.

**V4 (G, A). Three texts contradict the jar-or-nothing decision.**
\`JAR_PROTOCOL.md\` "Principles" ("if the jar machinery fails, spyc hosts the
pane itself"), its "Meeting another version" ("spyc runs the program without
a jar rather than fail the tab"), and the module doc of
\`crates/spyc-jar/src/cli.rs\` ("and run the program another way"), against the
plan's decision 6 and SPYC-TRAP \`jar-or-nothing\`. Two of the three are inside
the frozen specification. Confirm the code never falls back unasked on any
path (tabs, tasks, captures, restore, retake), and list every sentence that
says otherwise.

**V5 (E). Control-plane methods on the MCP socket take any caller.**
\`spyc/shutdown\`, \`spyc/release\`, \`spyc/upgrade\` and \`spyc/disconnected\`
dispatch in \`src/mcp/protocol.rs\` (about lines 436 to 510) with no check of
who asks; \`release_session\` (\`src/app/project_move.rs:748\`) saves the
session, lets every jar go and quits, and \`Shutdown\` (\`src/app/mcp.rs:522\`)
ends the session, neither asking the user. Every hosted agent holds a
connection to that socket. Enumerate every non-tool method, say what each
lets an injected agent do that the token was meant to make harder, and
classify each against the stated model. Is the proxy a filter, or can an
agent reach the socket directly?

**V6 (D, E). \`spyc attach\` signals a pid it read earlier and never checks.**
\`run\` in \`src/pane/jar/attach.rs\` takes \`owner_pid\` from a tokenless
\`describe\` inside \`running()\`, may then wait at the "Attach to which?"
prompt, and \`take_over\` sends that pid \`SIGTERM\` without reading it again or
checking what it is. A client that exited meanwhile leaves a number another
process may hold. Find every other place a pid from \`describe\` or a saved
file is signalled or asked (\`behind\`, \`ask_upgrade\`, \`shutdown::program_pid\`,
the orphan sweep).

**V7 (C). The owner's reader cannot see a hung jar, and drops refusals.**
\`read_frames\` in \`src/pane/jar/link.rs\` waits up to \`NO_REPAINT_DUE\` (3600 s)
for a frame, never reads a \`Pong\`, and ignores every \`Refused\` that isn't an
upgrade's. Pings are written and never checked, so a jar that holds its lock
but answers nothing is noticed only when the socket's buffer fills and
\`WRITE_STALL\` passes. Probe: \`SIGSTOP\` a scratch jar and time how long the
tab takes to say anything, idle and while typing, on both platforms. And the
jar's \`refused\` for input past \`INBOX_CAP\` reaches nobody: what does the user
see when a paste is dropped?

**V8 (B). An idle jar is a set of polling loops.** By its constants: the
acceptor, the PTY pump, the input feeder and each connection's reader poll at
\`TICK\` (100 ms), the child waiter at 50 ms, the reaper at 200 ms, each writer
at 500 ms; the attach client polls its size at 250 ms. That is on the order
of 60 to 80 timed wakeups a second for each idle jar with one owner, for the
jar's whole life, against \`AGENTS.md\`'s "0 wakes at idle" for spyc itself.
Measure it with ten idle jars, attached and detached (\`powermetrics\` or
Activity Monitor's idle wake-ups on macOS, \`perf stat\` on Linux), and state
the cost on battery.

**V9 (B). Nothing supervises a jar's threads, and a jar has no log.** The
release profile unwinds, every lock recovers from poison with \`into_inner\`,
and \`catch_unwind\` guards only \`resume\`. A panic in the PTY pump, the input
feeder, the waiter or the reaper leaves a jar that holds its lock and answers
pings while part of it is dead. Most of the crate's threads start with
\`thread::spawn\`, which panics when the OS refuses a thread, where spyc's side
uses \`Builder::spawn\` and \`?\`; the acceptor spawns one for each connection
from the main thread, where a panic ends the process and the program with it.
spyc starts every jar with standard output and error on \`/dev/null\`
(\`spawn\` in \`src/pane/jar/mod.rs\`), so whatever a jar says as it fails is
lost. Enumerate each thread, what happens to the program if it dies, and
what a user could learn afterwards.

**V10 (A, frozen). A reader with the token can make the jar run any file.**
\`act\` in \`jar/conn.rs\` passes \`upgrade\` from a reader to \`upgrade::begin\`;
\`one_file\` accepts any absolute path and copies it into the store or the
copies directory; the validator then runs it with the blob, the token in it,
on standard input. "A reader holding the token may ask" is in the list that
freezes for good. Inside the stated model the token is already full trust, so
this may be by design; say so or not, and say what it does to U6 (a read-only
watcher that can never type) and to any later role that is meant to hold less
than the owner.

**V11 (C). A clock decides whether a terminal reply reaches the program.**
\`ReplayGate\` in \`link.rs\` drops replies until the replayed tail is handed
over and for \`REPLY_GRACE\` (500 ms) after. Under load the parser may still be
in the tail after 500 ms, and a second answer reaches the program; on a fast
machine a query the program asks in that half second gets none. Find every
other verdict a clock gives. Candidates from the read: \`REPAINT_HOLD\`
(100 ms, measured at 40 of 40 on one machine), \`WELCOME_WAIT\` (5 s, after
which an owner whose \`hello\` was accepted hangs up and tries again),
\`FRAME_DEADLINE\` and \`LOOKUP_DEADLINE\` (5 s), \`ENDING_WAIT\` and \`EXIT_WAIT\`
(2 s), \`FLUSH_DEADLINE\` (1 s), \`PARK_DEADLINE\` (3 s), \`HOLDER_ANSWER\` (5 s),
\`RELEASE_WAIT\` (20 s), and the success path of \`validate\`, which waits only
what is left of the deadline for the proof line and then answers reason 6
(rejected), where the protocol says running out of time is always reason 5.
For each: what does expiry conclude, is that conclusion ever wrong on a slow
machine, and what state would decide it instead?

### Inferred, needs a probe

**H1 (B). A program that exits while a descendant holds the PTY.** A
connection is sent \`exited\` only once \`pty_eof\` is set (\`next_batch\` in
\`conn.rs\`), and the reaper ends the jar only on an acknowledged exit or an
ownerless linger. \`sh -c 'sleep 600 & exit'\`, a \`nohup\` server an agent
started, a \`!\` capture that backgrounds something: does the tab ever learn
its program ended, and does a capture's jar ever end? Compare with a tab
without jars.

**H2 (E). A client trusts a socket by its directory, and not on every path.**
The jar refuses a peer of another user; \`Client::connect\` checks nothing
about the jar. \`owner_mcp\` checks \`private_dir\` first; \`attach::running\` and
\`socket_state\` appear not to. \`keep_socket\` remakes the directory and binds
again with no ownership check. With neither \`XDG_RUNTIME_DIR\` nor \`TMPDIR\`,
sockets live under \`/tmp/spyc-<uid>\`, which another user can create first.
Trace every connecting path (adopt, retake, attach, listing, orphans, the MCP
proxy by \`SPYC_JAR_SOCKET\`) and say for each what stops it sending a token,
keystrokes, or an agent's MCP traffic to a socket someone else bound.

**H3 (B). What a jarred program inherits.** spyc sets \`SIGTTOU\` to \`SIG_IGN\`
(\`src/terminal.rs\`, whose comment argues the inheritance is harmless for a
foreground child); a jar and its program now inherit it for life. Compare
\`SigIgn\`, \`SigBlk\`, the open descriptors (\`lsof -p\`), the environment and the
umask of a shell in a jarred tab, a tab with jars off, and a plain terminal.
The jar never changes directory: it holds spyc's working directory at spawn
until its program ends.

**H4 (B). The log under one lock.** \`next_batch\` copies everything from a
connection's cursor to the head into frames while it holds \`state\`, which
the PTY pump needs to append. One \`Condvar\` wakes the pump, every writer, the
reaper and each detach on every read, and the reaper calls \`tcgetpgrp\` under
the lock on each wake. Measure throughput and latency for \`yes\`, for a
4 MiB replay to a new reader while the program prints, and with five readers.

**H5 (A). A resume with no deadline.** \`resume\` reads its descriptors and the
blob with blocking reads, and \`end_holder\` waits for the holder with a
blocking \`waitpid\`. A holder that is stopped, not dead, never closes the pipe.
What bounds the time the program's PTY goes undrained?

**H6 (D). \`^\\\` and the attach client.** \`relay_input\` detaches on any \`0x1c\`
byte in what it reads, a paste included, and the plan says \`^\\\` can reach a
program inside spyc. In a terminal without the kitty keyboard protocol, type
\`^\\\` at a program in a jarred tab of an attached session, and paste text that
contains the byte.

**H7 (D, G). Where the session ends with the login.** On Linux with
\`KillUserProcesses=yes\`, and under WSL when the last terminal closes, a
process that left its session is still ended. Nothing in the tree mentions
either. The owner's acceptance test for S4 is "close the terminal, open
another, \`spyc attach\`"; find where it does not hold and whether the
documents say so.

**H8 (D). Durability of what a move depends on.** \`tokens::save\` truncates
and writes in place, with no sync and not through the tree's atomic write.
For \`release\` (a new token) and for the move file, check that the new token
is durable before the jar is told it, on every path, and what a crash between
the two leaves.

## Deliverable

Under \`docs/drafts/review-3.0/\` in your worktree, uncommitted:

- \`A-frozen-surface.md\` through \`G-diff-and-docs.md\`, one per unit. Each opens
  with the SHA, the platform, and a verdict in a paragraph; then its coverage
  ledger; then findings.
- \`SYNTHESIS.md\`, by the referee: the verdict (does anything hold the tag, and
  what), findings ordered by severity with the frozen ones first, where
  reviewers overlapped or disagreed, premises of this brief that were wrong,
  what was not reviewed, and the manual checklist.
- \`MANUAL.md\`: the probes that need the owner's hands, each with its steps,
  what to expect, and which finding it settles.

A finding has: an id (unit letter and number); severity (\`blocker\` holds the
tag, \`high\`, \`medium\`, \`low\`, \`informational\`); a \`frozen\` mark if it touches
what can't change after release; a lens (correctness, reliability, security,
performance, maintainability, docs); every site as \`path:line\`; the claim in
one sentence; the evidence and its kind; the consequence for a user; status
(\`confirmed\`, \`refuted\`, \`unverified\`); and at most one sentence on the
direction of a fix. Reports are prose under the tree's own rules: Canadian
English, identifiers in backticks, no adjective that carries no information.

End your hand-off with the path of the worktree and a recap short enough to
read between two other things: the number of findings at each severity, the
frozen ones by id, and the three you would want fixed first.

## What not to do

- Don't fix, commit, push, open a PR or file an issue.
- Don't touch a jar, socket, token, store entry or session you didn't create.
- Don't report a seeded lead as confirmed because this brief says it; re-read
  the code at your SHA.
- Don't report a count you didn't produce with a command you can show.
- Don't propose async, a second way to host a pane, \`unsafe\` in the jar, or a
  new protocol version as the fix for anything.
- Don't lengthen a wait as the direction of a fix for a clock-decided verdict.
- Don't re-run a failed gate or test hoping for green: a failure is evidence,
  and goes in the report with its log.
- Don't stop at the jar. Units F and G exist because a release is more than
  its headline feature.
