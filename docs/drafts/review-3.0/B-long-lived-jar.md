# Unit B: the jar as a long-lived process

**SHA:** `444857a2` (`main`, 2026-11-24).
**Platforms:** macOS 26 (Darwin 25.6.0), Apple silicon, 14 cores, the debug build at the pin built in my worktree; Linux 6.19 aarch64 (Alpine, musl) under Podman, the orchestrator's warm debug build at the pin. Linux here is aarch64 musl, not CI's x86_64 glibc.
**Worktree:** `~/src/spyc.worktrees/review/3.0-b` (branch `review/3.0-b`), uncommitted probe file `tests/review_b_probes.rs`.
**Scratch:** `docs/drafts/review-3.0/evidence/B/`.

## Verdict

The jar's design holds together at rest: one mutex with a clean lock order, no lock held across a write to a peer, every wait bounded, no descriptor or thread leaked over thousands of connections, and the flow-control rules the tests describe all pass unloaded on both platforms. As a process meant to run for months under load it falls short in four ways. A refused thread kills the program or leaves a jar no owner can ever take back, and a dead jar says nothing anywhere (B1, B2; shown on Linux). On Linux, every read the client makes fails with `EINTR` after a stop and continue, which spyc takes for a lost jar (B12). Every idle jar polls about 50 to 70 times a second for its whole life (B4). And the jar's own tests go red under CPU load on both platforms, nearly always on wall-clock bounds in the tests (B3), once in a way I could not reproduce in about 3,750 targeted rounds and which, if it is real, touches the frozen upgrade path (B13). Nothing here corrupts a user's files; nothing touched the owner's jars (every probe ran under scratch `XDG_*`, `TMPDIR` and `HOME` with every `SPYC_*` unset, sockets under fresh `/tmp/revb*` or `/tmp/jar*` directories, all gone afterwards). None of the findings is frozen except B13, if it is real. I would fix B1, B12 and B3 first.

## Coverage ledger

| File | Status |
|---|---|
| `crates/spyc-jar/src/jar/mod.rs` | read in full |
| `crates/spyc-jar/src/jar/life.rs` | read in full |
| `crates/spyc-jar/src/jar/conn.rs` | read in full |
| `crates/spyc-jar/src/jar/accept.rs` | read in full |
| `crates/spyc-jar/src/jar/outbox.rs` | read in full |
| `crates/spyc-jar/src/jar/input.rs` | read in full |
| `crates/spyc-jar/src/jar/describe.rs` | read in full |
| `crates/spyc-jar/src/pty.rs` | read in full |
| `crates/spyc-jar/src/client.rs` | read in full |
| `crates/spyc-jar/src/attach.rs` | read in full |
| `tests/jar_process.rs` | read in full |
| `tests/jar_process/contain.rs` | read in full |
| `tests/jar_process/lock.rs` | read in full |
| `tests/jar_process/ownership.rs` | read in full |
| `tests/jar_process/timers.rs` | read in full (one middle section, lines 204 to 246, skimmed) |
| `tests/jar_process/upgrade.rs` (Unit A's) | skimmed: `upgrade_answer`, `parking`, and the test that failed under load |
| Outside the unit, read for context | `cli.rs` lines 50 to 200 and `Start`; `state.rs` `Log` and `State`; `lock.rs` 60 to 130; `store.rs` `store_of`, `build_of`; `src/pane/jar/mod.rs` `spawn`; `src/pane/jar/test_dir.rs`; `src/terminal.rs` `install_signal_handlers`; portable-pty 0.9.0 `spawn_command` |

## Findings


### B1. A thread the jar can't start kills the program, or leaves a jar no owner can ever take

- **Severity:** high. **Frozen:** no (the old image's behaviour during an upgrade is frozen, but nothing here is in the upgrade path). **Lens:** reliability.
- **Sites:** `crates/spyc-jar/src/jar/mod.rs:448` (the acceptor, on the jar's main thread, `thread::spawn` per connection); `crates/spyc-jar/src/jar/conn.rs:105` (each connection's writer, `thread::spawn` inside the connection's serving thread); `crates/spyc-jar/src/jar/conn.rs:32` (a program's detach); `crates/spyc-jar/src/jar/mod.rs:381,392,396,400,404,410` (start-up threads); `crates/spyc-jar/src/jar/accept.rs:227-233,243,258` (the owner is recorded, then `run_conn` starts the writer, and only `run_conn`'s reader loop ever checks the deadline or calls `release`).
- **Claim:** every jar thread starts with `std::thread::spawn`, which panics when the OS refuses a thread; a refusal in the acceptor ends the jar and its program, and a refusal of an owner's writer leaves the jar with an owner that never leaves.
- **Evidence (command and output, Linux):** `docs/drafts/review-3.0/evidence/B/linux/probe_nproc.py`, run as uid 2000 in its own container against the gate's `spyc-jar` at the pin; it lowers the jar's soft `RLIMIT_NPROC` with `prlimit` to just under or just over the user's task count, connects, then lifts the limit:
  ```
  V9a: user tasks 9; jar 2 soft RLIMIT_NPROC 8; jar threads 7
     lookup: ConnectionResetError(104, 'Connection reset by peer')
     after one lookup: jar alive=False exit status=101 program state=Z
  V9b: user tasks 8; jar 11 soft RLIMIT_NPROC 9; jar threads 5
     owner welcome: None
     limit lifted; jar alive: True ; jar threads 5 ; describe owner: connected
     13 s after that owner's socket closed (deadline 10 s): owner = connected
     a new owner gets: ('refused', 2, 'the jar already has an owner')
  ```
  With the limit one higher (both threads start) the same steps end `owner = none for 13 s` and a new owner is welcomed, the control. Exit status 101 is a Rust panic leaving `main`. In V9b the connection's serving thread panicked at `conn.rs:105` after `accept.rs:229` had made it the owner; `release` (`conn.rs:355`) and the deadline check (`conn.rs:120`) live in the loop that never ran, and nothing else clears `st.owner`.
- **Consequence:** a tokenless lookup (`spyc --jars`, an agent's MCP proxy asking where its owner is) that arrives while the user is at the thread or process limit, or short of memory for a stack, ends the jar: the program loses its terminal and dies. An owner (spyc reconnecting, `spyc attach`) that arrives at such a moment leaves a jar that refuses every later owner with `OWNER_PRESENT` for good, while its program runs on unreachable, since a pane's linger waits for an owner that is never "gone". Neither leaves a word anywhere (B2).
- **Status:** confirmed (Linux, run). macOS not probed: its per-process thread limit is high, but a failed stack allocation reaches the same lines.
- **Direction:** start threads with `thread::Builder::spawn` and treat a failure as that connection's failure (refuse and close it, releasing the lease), as spyc's side already does.

### B2. A jar that fails leaves no record of why

- **Severity:** medium. **Frozen:** no. **Lens:** reliability, maintainability.
- **Sites:** `src/pane/jar/mod.rs:478-479` (`stdout(Stdio::null())`, `stderr(Stdio::null())`), `src/pane/jar/mod.rs` `spawn`, the reaping thread `let _ = jar.wait();` after `answer`; `crates/spyc-jar/src/jar/mod.rs:381-411,448` (no thread is joined or watched; `life::reap` is the only thread that ends the process on purpose).
- **Claim:** spyc starts every jar with standard output and error on `/dev/null` and discards its exit status, and no thread of the jar watches another, so a panic message, a refused `exec`, or a thread's death is lost.
- **Evidence (lines read, plus B1's run):** the lines above; in B1 the jar's panic message went to `/dev/null` and its exit status 101 was available only because the probe was the parent. A thread that panics in a jar built with `panic = "unwind"` (`Cargo.toml:274`) ends alone: if it is the PTY pump (`life.rs:59`) the program blocks once the PTY buffer fills while the jar still answers pings and `describe`; if it is the input feeder (`input.rs:73`) input queues to `INBOX_CAP` and is then refused; if it is the reaper (`life.rs:153`) the jar never ends. I found no input that panics these loops in the code read, so this is a consequence, not an observed failure.
- **Consequence:** a user whose tab says `[jar lost]` or whose agent stops printing has nothing to read, and a bug report has nothing to quote. For a process meant to run for months this is the one place its failure could be recorded.
- **Status:** confirmed (lines read).
- **Direction:** give each jar a small append-only log file beside its token or socket (bounded, `0600`, no program output), or have spyc capture the jar's standard error into its own `:stderr` log.

### B3. Jar tests fail under CPU load: 5 of 7 loaded runs red on macOS, 4 of 31 on Linux, from ten tests

- **Severity:** medium. **Frozen:** no. **Lens:** reliability (of the gate), correctness of the tests.
- **Sites:** `tests/jar_process/upgrade.rs:563` (`a_reader_trickling_during_the_flush_cannot_keep_the_jar_parked`, asserts the whole upgrade took under 8 s); `tests/jar_process/upgrade.rs:214` (`a_validators_child_holding_its_stderr_cannot_hold_the_jar`, asserts the refusal came in under 7 s); `tests/jar_process.rs:753-757,829` (`an_upgrade_keeps_the_program_its_pty_and_every_connection`: the reader thread's `read_until` gives up after `WAIT`, 10 s); `tests/jar_process.rs:158` (`sequences::no_sequence_lets_a_reader_type`: `Jar::connect`, 10 s for the socket to appear). The first two are in Unit A's file; they are reported here because the load runs are this unit's.
- **Claim:** under CPU load the jar's own suite fails, nearly always on a wall-clock bound in the test, and in two cases in ways the evidence can't explain (the missing socket here, and B13).
- **Evidence (command and output, macOS):** `docs/drafts/review-3.0/evidence/B/mac_load.sh ITER BURNERS THREADS TAG` runs the pinned `jar_process` test binary and the crate's unit tests under `BURNERS` copies of `yes > /dev/null` (14 cores), every `XDG_*`, `TMPDIR` and `HOME` in scratch, every `SPYC_*` unset. Summary (`.../scratchpad/B/mac/summary`):
  ```
  load14 iter 1 burners 14 threads 32 jar_process=101 unit=0 secs=44
  load14 iter 2 burners 14 threads 32 jar_process=0 unit=0 secs=43
  load14 iter 3 burners 14 threads 32 jar_process=0 unit=0 secs=42
  load14t64 iter 1 burners 14 threads 64 jar_process=101 unit=0 secs=43
  load14t64 iter 2 burners 14 threads 64 jar_process=101 unit=0 secs=41
  load14t64 iter 3 burners 14 threads 64 jar_process=101 unit=0 secs=40
  load14t64 iter 4 burners 14 threads 64 jar_process=101 unit=0 secs=42
  ```
  The failures, iteration by iteration (logs `.../scratchpad/B/mac/load14-jp-1.log` and `.../scratchpad/B/mac/run2/load14t64-jp-{1,2,3,4}.log`):
  - `load14` 1: `the trickling reader held the jar parked for 8.184819291s`.
  - `load14t64` 1: the trickling test (8.758 s); the validator-child test (`the refusal waited on the validator's child: 8.046348292s`); `an_upgrade_keeps_the_program_its_pty_and_every_connection` (`timed out; saw 5387 bytes, frames [Foreground { pgid: 37405 }]` in the reader thread, then `the reader: Any { .. }` at line 829).
  - `load14t64` 2: trickling (8.720 s), validator child (8.379 s).
  - `load14t64` 3: trickling (9.832 s), validator child (8.035 s), `sequences::no_sequence_lets_a_reader_type`: `connect: connect /tmp/jarFCAi9z/j.sock: No such file or directory (os error 2)`.
  - `load14t64` 4: trickling (9.246 s), validator child (8.306 s).

  The crate's unit tests passed in all 7. Unloaded, the same binaries passed in the orchestrator's gate.

  Linux (aarch64 musl, the gate's binaries, `stress-ng --cpu 7 --io 2 --vm 2 --vm-bytes 512M`, 48 test threads; `.../scratchpad/B/linux/load.sh`, summary `.../scratchpad/B/linux/out/summary`): 25 iterations, 21 green; iterations 21 to 24 red, while load rose from about 14 to 21 (another unit's container started then). Then 6 more suite runs under the same stress beside B13's probe, all green (`.../scratchpad/B/linux/out/diag2-suite.txt`). The red ones (logs `.../scratchpad/B/linux/out/stress48-{jp,ut}-{21,22,23,24}.log`):
  - 21: `contain::a_new_image_that_panics_as_it_starts_holds_the_program` and `contain::a_new_image_that_never_reads_the_blob_holds_the_program`, both `timed out waiting for …: the program printing on` (`eventually`, 10 s, `tests/jar_process/contain.rs:107`); and the crate's unit test `client::tests::a_send_returns_while_the_jar_reads_nothing`: `sends waited on the socket: 2.009859554s` (`crates/spyc-jar/src/client.rs:436`, a 2 s bound on encoding 64 MiB).
  - 22: `a_reader_that_falls_behind_is_told_what_it_missed`: `timed out; saw 2827919 bytes` (10 s, `tests/jar_process.rs:260`).
  - 23: the same reader test (`saw 2982074 bytes`), and `contain::…never_reads_the_blob…` and `contain::…cant_decode_the_blob…` as in 21.
  - 24: `a_program_hears_one_attached_per_owner_however_the_connections_race`: `recv: Interrupted system call (os error 4)` (B12), and `an_upgrade_keeps_the_program_its_pty_and_every_connection`: `the count runs on unbroken; left: 35, right: 34` (B13).

  Earlier, 6 Linux iterations at 32 threads ran with no load at all (my first `stress-ng` line failed to start; the summary's load average of 0.4 to 1.9 shows it) and passed; they are not counted as loaded.
- **Reading:** the trickling and validator-child tests measure a whole upgrade, process starts of shell wrappers included, against a fixed number of seconds; under load the process starts alone exceed it, so these failures say nothing about the jar's deadlines either way, and the tests can't tell a jar that waited too long from a machine that was slow. The upgrade test's reader gives up after 10 s of wall clock. The socket that never appeared within 10 s is not explained by anything I could read: the jar's standard error goes to `/dev/null` in this harness too (`tests/jar_process.rs:132`), so whether it exited or was only slow is unknown (B2's problem in the test harness).
- **Consequence:** on a loaded machine (the owner's, during a `make check` with agents running, or a busy CI runner) `make test` goes red on tests that are green unloaded, which the owner's "No CI flakes" rule forbids; and a real regression in those deadlines would look the same as load.
- **Status:** confirmed (run, both platforms).
- **Direction:** have each test assert on the state the deadline is meant to bound (the jar parked at most its own published limits, measured by the jar or by a lock probe), not the wall clock of a whole upgrade, and keep the jar's standard error in a file the test prints on failure.

### B4. An idle jar wakes about 50 to 70 times a second, for its whole life

- **Severity:** medium. **Frozen:** no. **Lens:** performance (battery).
- **Sites:** `crates/spyc-jar/src/jar/mod.rs:207` (`TICK`, 100 ms) used by the acceptor `mod.rs:433`, the PTY pump `life.rs:78,82`, the input feeder `input.rs:82,90`, each connection's reader `conn.rs:119` and a detach `conn.rs:55,74`; `life.rs:19` (`WAITER_POLL`, 50 ms, `waitpid(NOHANG)` then `sleep`, `life.rs:126-129`); `life.rs:21` (`REAPER_TICK`, 200 ms, with `tcgetpgrp` each wake, `life.rs:160`); `conn.rs:19` (`WRITER_TICK`, 500 ms, `conn.rs:515-519`); `mod.rs:453` (`SOCKET_CHECK`, 2 s, two `stat`s); `attach.rs:52` (`SIZE_POLL`, 250 ms, the attach client).
- **Claim:** every thread of a jar polls on a timer, so a jar that holds a program doing nothing wakes continuously, against "0 wakes at idle" in `AGENTS.md`.
- **Evidence (command and output):** Linux, `.../scratchpad/B/linux/probe_v8.py` (ten jars of `sleep 100000`, five with an owner that pings every 2 s, five ownerless; `strace -f -c -p` on each for 10 s; run as uid 2000 in its own container). Per jar, the summary's own totals: owned 1,336 to 1,351 syscalls in 10 s (about 134 a second), ownerless 1,028 to 1,049 (about 103 a second). The timed sleeps among them, per owned jar per second: `ppoll` 30 (acceptor, pump, owner's reader at 10 each), `clock_nanosleep` 19 (the waiter), `futex` 18 (feeder 10, reaper 5, writer 2, plus wakes), `wait4` 19; ownerless: `ppoll` 19.5, `clock_nanosleep` 19, `futex` 15. That is about 67 timed wakeups a second with an owner and about 53 without, and ten idle jars about 600. (`clock_gettime` appears as a syscall on aarch64 musl; on glibc it is a vDSO call and the counts would be lower by that much, not the wakeups.) macOS, the probe `v8_idle_jars` in `tests/review_b_probes.rs` and `top -l 2 -s 20 -pid … -stats pid,csw,idlew,power,th`: over 20 s, owned jars 2,145 to 2,169 context switches (about 108 a second), ownerless 1,747 to 1,779 (about 88 a second); `top`'s `IDLEW` read 0 for all, which on a busy machine says nothing (it counts package idle exits); `powermetrics` needs root and is on the manual list.
- **Consequence:** a laptop with ten agent tabs open overnight, detached, carries about 600 timer wakeups a second from jars alone, which keeps the CPU out of its deepest idle states; macOS attributes them to `spyc-jar` in Activity Monitor's energy column. The jars outlive spyc by design, so quitting spyc's UI does not stop them.
- **Status:** confirmed (run, both platforms).
- **Direction:** block on the thing each thread waits for (a `poll` with no timeout on the socket and a self-pipe or `eventfd` for park and close, `waitpid` without `NOHANG` on a thread that parks by signal or by a pipe, condition variables without timeouts), keeping timers only where a deadline is the point (the owner's lease).

### B5. On Linux, a program whose descendant keeps the terminal is never reported ended, can't be signalled, and a capture's jar never ends

- **Severity:** low. **Frozen:** no. **Lens:** correctness.
- **Sites:** `crates/spyc-jar/src/jar/conn.rs:496-499` (`exit_due` needs `st.pty_eof`); `crates/spyc-jar/src/jar/conn.rs:307-312` (a signal after the exit is refused); `crates/spyc-jar/src/jar/life.rs:180-189` (the jar ends on an acknowledged exit or `linger_after_exit`, which for a capture is `UNTIL_READ`, `cli.rs:263`, about 584 million years).
- **Claim:** when the program exits while a process it started in another process group still holds the PTY's slave, the jar records the exit at once but never sends `exited`, refuses every later signal, and a capture's jar waits for that process for ever.
- **Evidence (command and output):** Linux, `.../scratchpad/B/linux/probe_h1.py` (the gate's `spyc-jar`, an owner that replays and acknowledges, 8 s watched):
  ```
  H1 linux [job-control shell, child in own group] exit_linger=default: describe exit=3 after 0.108, owner told 'exited' after None (watched 8 s), kill then: ('refused', 9, 'the program has ended')
  H1 linux [job-control shell, child in own group] exit_linger=until-read: describe exit=3 after 0.0012, owner told 'exited' after None (watched 8 s), kill then: ('refused', 9, 'the program has ended')
  ```
  The program is `sh -c 'set -m; sleep 30 & echo bg-started; exit 3'`. The three other shapes (a child in the program's own group that ignores `SIGHUP`, `nohup … >/dev/null &`, `nohup … &`) were told `exited` within 40 ms: Linux hangs up the foreground group when the session leader exits, and `nohup` gives up the terminal. On macOS the same probe (`h1_exit_with_a_descendant_holding_the_pty_reaches_the_owner` in `tests/review_b_probes.rs`) reported `exited` within 50 ms in both shapes it tried, the job-control one included: macOS revokes the terminal when its session leader exits.
- **Consequence:** an interactive shell tab where the user ran `server & disown` and then `exit` (or an agent's shell that left a job-controlled background process) shows the program running, on Linux, until the leftover process ends; spyc can't end it through the jar, since the jar refuses signals once its program has ended and would signal the program's group, not the leftover's. A tab without jars behaves the same way for the tab (it also waits for end of file on the master, `src/pane/pty_host.rs:469-482`), so this is parity for panes; what is new is that a capture's jar, kept "until read", and a pane's jar until its 60 s exit linger, now outlive spyc holding the leftover's terminal.
- **Status:** confirmed on Linux (run); refuted for macOS (run).
- **Direction:** send `exited` once the program is reaped and the PTY has been drained of what is readable, rather than waiting for end of file, and let an owner signal the PTY's foreground group after the exit.

### B6. Every jarred program inherits `SIGTTOU` ignored, and every jar keeps spyc's working directory of the moment

- **Severity:** low. **Frozen:** no. **Lens:** correctness.
- **Sites:** `src/terminal.rs:341` (spyc sets `SIGTTOU` to `SIG_IGN`, inherited across `exec`); `src/pane/jar/mod.rs:471-479` (the jar's `Command`: no `current_dir`, no change of disposition); `crates/spyc-jar/src/pty.rs:57-73` and `pty.rs:78-83` (the program's start: `current_dir` set to the tab's directory, dispositions untouched); `crates/spyc-jar/src/cli.rs:102-150` (the jar never changes directory); `src/app/process_cwd.rs:1-45` (spyc's own working directory follows the focused column).
- **Claim:** a program in a jar starts with `SIGTTOU` ignored, and the jar process holds whatever directory spyc's focused column was in when the tab opened, both for as long as the jar lives.
- **Evidence (command and output, Linux):** `.../scratchpad/B/linux/probe_h3.py` starts a jar from a parent that ignores `SIGTTOU` with umask `022`, as spyc does, and has the program print `/proc/self/status`:
  ```
  plain child:        SigIgn: 0000000000000004  SigBlk: 0000000000000000  Umask: 0022
  program in a jar:   SigIgn: 0000000000200004  SigBlk: 0000000000000000  Umask: 0022
  jar itself:         SigIgn: 0000000000201004
  jar cwd: /tmp   (the parent's at spawn; the program's was /home, as asked)
  program: sid 20 = pgrp 20 = tpgid 20; fds 0 and 2 on /dev/pts/0; no descriptor of the jar's
  ```
  Bit 22 is `SIGTTOU` (bit 3, `SIGQUIT`, came from the container's own shell for both). The jar's own `SIGPIPE` (bit 13) is Rust's and is reset for the program by `Command`. The mask is empty, and no descriptor of the jar (lock file, listener, master copies) reaches the program. A tab without jars inherits the same `SIGTTOU` disposition: portable-pty 0.9.0's `spawn_command` resets `SIGCHLD`, `SIGHUP`, `SIGINT`, `SIGQUIT`, `SIGTERM` and `SIGALRM` only.
- **Consequence:** `stty tostop` does nothing in a jarred shell's background jobs, and a background process's `tcsetpgrp` succeeds where it would stop; the comment at `src/terminal.rs:324-327` calls the inheritance harmless "for a foreground child" of spyc, and with jars the child is every program for its life, detached from spyc. The working directory: a disk image or USB volume that was open in a column when a tab was started can't be ejected while that tab's jar runs, long after the user left it (by reading: the jar's working directory is on it; not run on a removable volume).
- **Status:** confirmed (run, Linux; the cwd half by the lines read for macOS).
- **Direction:** have `pty-child` restore `SIGTTOU` (and the other job-control signals) to their defaults before its `exec`, and start the jar with its directory set to `/`.

### B7. The ring's limit counts payload bytes only; small writes cost up to about 55 times that in memory

- **Severity:** low. **Frozen:** no. **Lens:** performance, resource hygiene.
- **Sites:** `crates/spyc-jar/src/state.rs:87-120` (`Log::push`, `Log::trim`: `held` and `cap` count `bytes.len()`); `crates/spyc-jar/src/jar/life.rs:100` (one `Event::Out` with its own `Vec` for every read of the PTY); `crates/spyc-jar/src/cli.rs` `parse` (`ring_cap: 4 << 20`, which spyc does not override).
- **Claim:** the log holds up to its cap in payload bytes whatever the number of events, and each event costs a `VecDeque` slot and a heap allocation, so a program that writes a byte at a time makes the jar's memory many times the cap `describe` reports.
- **Evidence (command and output, macOS):** `.../scratchpad/B/ring_tiny.py` (a 256 KiB ring, the program printing 512 KiB):
  ```
  8 KiB writes:  262132 of 262144 bytes held ... jar RSS 2720 KiB at start, 4144 KiB now
  1-byte writes: 262144 of 262144 bytes held ... jar RSS 2688 KiB at start, 17168 KiB now
  ```
  About 56 bytes of resident memory per byte held at one byte per event. At the default 4 MiB ring that is about 230 MiB for one jar, and the `VecDeque`'s capacity never shrinks.
- **Consequence:** a long-running program that emits in tiny writes (a dot per test, a character-at-a-time progress display, keystroke echo over hours) can grow its jar to hundreds of megabytes while `spyc --jars` reports "4194304 of 4194304 bytes held". The common case (an agent repainting in kilobyte writes) is near the cap.
- **Status:** confirmed (run, debug build; the per-event sizes do not depend on the profile).
- **Direction:** count each event's overhead into `held`, or append small reads to the previous `Out` event when it has room.

### B8. The log under one lock: readers cost the owner throughput, a replay does not stall it

- **Severity:** informational. **Frozen:** no. **Lens:** performance.
- **Sites:** `crates/spyc-jar/src/jar/conn.rs:463-521` (`next_batch` copies the log from the cursor into frames under `state`); `life.rs:98-105` (the pump appends under `state` and wakes every waiter on the one `Condvar`); `life.rs:160` (`tcgetpgrp` under the lock on every reaper wake); `describe.rs:7-31` (`describe`, which any tokenless lookup can ask for, calls `current_exe()` twice and `store_of`'s `stat`s while holding `state`).
- **Claim:** every writer, the pump, the reaper and each detach share one mutex and one condition variable, so readers slow the owner, but in the runs below nothing waited long.
- **Evidence (command and output, macOS, debug build):** `h4_throughput_and_latency` in `tests/review_b_probes.rs`, `yes` into a jar, the owner acknowledging each frame and pinging every 50 ms, 4 s each:
  ```
  readers=0: owner 18.2 MB/s, ping rtt ms n=26713 p50=0.1 p99=1.3 max=18.9
  readers=0 + replaying reader: owner 14.6 MB/s, late reader got 62.6 MB, ping rtt p50=0.1 p99=1.7 max=125.8
  readers=5: owner 7.1 MB/s, readers total 35.7 MB/s, ping rtt p50=0.2 p99=1.7 max=36.8
  readers=5 + replaying reader: owner 7.9 MB/s, late reader got 35.7 MB, ping rtt p50=0.2 p99=1.2 max=31.9
  ```
  Five readers cut the owner's share by about 60 per cent; a reader replaying the whole 4 MiB ring cost about 20 per cent and one ping waited 126 ms. `replay_to_first_output_latency` (200 new owners, each replaying a short log) saw every first byte within 0.1 ms.
- **Consequence:** with today's single owner and rare readers, none a user would see; the `describe` path means `stat`s on the state directory under the jar's lock on every `spyc --jars` and every MCP proxy lookup, which on a hung network home directory would stop the pump.
- **Status:** confirmed (run) for the measurements; the network-filesystem consequence is by reading only.
- **Direction:** none needed now; compute `describe`'s fixed fields once at start.

### B9. A replay's wake can be lost, costing up to half a second

- **Severity:** low. **Frozen:** no. **Lens:** correctness.
- **Sites:** `crates/spyc-jar/src/jar/conn.rs:203-204` (`Replay` sets the cursor and calls `notify_all` without taking `state`); `conn.rs:468,480,515-519` (the writer reads the cursor under `state`, then waits on the condition with a 500 ms timeout).
- **Claim:** a `Replay` that lands between the writer's look at its cursor and its wait wakes nobody, and the writer starts streaming only at its next 500 ms tick.
- **Evidence (lines read and reasoning; one run that did not hit it):** `Conn::send` (`outbox.rs:40-46`) takes the state lock before notifying, its comment saying why; the `Replay` arm does not. The window is a few instructions wide; `replay_to_first_output_latency` (200 owners) never hit it.
- **Consequence:** an occasional half-second pause before a reattached tab or a new reader shows anything. Bounded, so not a hang.
- **Status:** confirmed by reading (not observed).
- **Direction:** take the state lock around the notify, as `Conn::send` does.

### B10. A jar test leaks a 300-second process on every run

- **Severity:** low. **Frozen:** no. **Lens:** maintainability (test hygiene).
- **Sites:** `tests/jar_process/timers.rs:457-497` (`a_holder_ends_when_the_jar_does`: the jar `exec`s a wrapper whose `resume-handover` case runs `sleep 300`, and the test kills the jar process only, `timers.rs:494`).
- **Claim:** each run of `jar_process` leaves one `sleep 300` orphaned, against the plan's "each jar test ends its program's process group".
- **Evidence (command and output, Linux):** the load runner records `pgrep -c -f 'sleep 300'` after each iteration: 1, 2, 3, … 8, then steady at 6 to 8 as the oldest end after 300 s (`.../scratchpad/B/linux/out/summary`). Of the four tests that start a `sleep 300` (`timers.rs:209,372,418,466`), the first three end the group that holds it (`end_group`, `gone_soon`); in the fourth the `sleep` is a child of the jar's own process image after its `exec`, and the test's `kill` reaches that process alone. Two orphaned `sleep 300` from someone's run were also present on the Mac (parent pid 1, not mine; left alone).
- **Consequence:** harmless for users; on a CI runner or the owner's Mac, stray processes that a later process-count assertion or leak check would trip over.
- **Status:** confirmed (run, with the test identified by reading).
- **Direction:** kill the jar's process group in that test, or have the wrapper `exec sleep`.

### B11. The attach client gives up on a jar that is only parked: `describe` waits 2 s, a park lasts up to 15 s

- **Severity:** low. **Frozen:** no. **Lens:** reliability.
- **Sites:** `crates/spyc-jar/src/client.rs:316` (`DESCRIBE_WAIT`, 2 s), `client.rs:324-334` (`describe`); `crates/spyc-jar/src/attach.rs:275-285` (`reconnect`: no answer reads as `None`, which returns "the connection closed"); `attach.rs:288-305` (then `Client::connect` with `ATTACH_WAIT`, 5 s); `attach.rs:44-49` (SPYC-TRAP `jar-outwait`, whose compile-time check covers `WRITE_STALL` and not these); `crates/spyc-jar/src/limits.rs` (`PARKED_AT_MOST`, 15 s).
- **Claim:** after the jar drops a silent attach client (a laptop that slept), the client decides whether to reattach from a `describe` it waits 2 s for, and a jar parked for an upgrade answers no lookup for up to 15 s, so the client ends instead of reattaching.
- **Evidence (command and output, macOS):** `.../scratchpad/B/park_describe.py` upgrades a jar to a wrapper whose second (parked) validation sleeps 4 s, and asks for a lookup's `describe` every 0.3 s with a 2 s timeout, as `client::describe` does:
  ```
  lookup at s, answered, took s: [(0.0, True, 0.0), (0.3, True, 0.0), (0.61, True, 0.0), (0.91, False, 2.0), (3.22, True, 1.72), (5.24, True, 0.0), …]
  owner heard: upgraded at 4.93s
  ```
  The lookup at 0.91 s got nothing in 2 s; the upgrade then succeeded. A park may last `PARKED_AT_MOST`.
- **Consequence:** `spyc attach` that comes back from sleep while spyc is upgrading a pane's jar, or while the session's jar is starved, prints `[the connection closed]` and leaves the user at a shell, the session still running; they attach again by hand. The same 2 s is spyc's `describe` in listing and sweeps (Unit C's ground).
- **Status:** confirmed (run).
- **Direction:** treat a `describe` that times out as "ask again" within a wait that outlasts `PARKED_AT_MOST`, and add these waits to the `jar-outwait` check.

### B12. On Linux, `Client::recv` reports an interrupted read as a lost connection; a stopped and continued spyc drops every jar link

- **Severity:** medium. **Frozen:** no (client side; the jar's own reads go through `poll`). **Lens:** reliability.
- **Sites:** `crates/spyc-jar/src/client.rs:114-131` (`recv`: `fill_buf`'s `WouldBlock` and `TimedOut` are "no frame yet", every other error, `Interrupted` included, is returned); `client.rs:156-162` (every `recv` sets `SO_RCVTIMEO`); callers that take any error for the end: `src/pane/jar/link.rs:227-235` (spyc's reader for each tab: `Lost::Held`, labelled `detached`), `crates/spyc-jar/src/attach.rs:211` (the attach client: `Ended::Lost`, then `reconnect`, which ends the client unless the jar says it dropped it for silence). Jar side, by the same mechanism: `crates/spyc-jar/src/jar/mod.rs:233-241` (`By::read` maps a `poll` that returns early to `TimedOut`, which for an owner mid-frame ends its lease as `silent`, `conn.rs:150-153`).
- **Claim:** Linux fails a socket read that has `SO_RCVTIMEO` set with `EINTR` when a caught signal interrupts it, whatever `SA_RESTART` says, and when the process is stopped and continued even with no handler (`signal(7)`); `recv` passes that up as an error, and spyc ends the tab's link.
- **Evidence (command and output):** `eintr_after_stop_and_continue` and `eintr_on_a_handled_signal` in `tests/review_b_probes.rs` (a thread blocked in `Client::recv` on an idle owner connection; the process stopped for 0.5 s and continued from outside, or the thread sent `SIGWINCH` under an `SA_RESTART` handler). Linux, built from my worktree in the container (`.../scratchpad/B/linux/eintr.sh`):
  ```
  EINTR-STOP: recv returned after 1.507948706s: Err(Interrupted system call (os error 4))
  EINTR-WINCH: recv returned after 505.036646ms: Err(Interrupted system call (os error 4))
  ```
  macOS, same probes: `Ok(None)` after the full 6 s and 4 s waits (BSD restarts the read). The load runs hit it unprovoked once: `a_program_hears_one_attached_per_owner_however_the_connections_race` failed at `tests/jar_process.rs:266` with `recv: Interrupted system call (os error 4)` (`.../scratchpad/B/linux/out/stress48-jp-24.log`).
- **Consequence:** on Linux, a spyc that is stopped and continued (job control, a debugger, a `kill -STOP` from a tool, the "spyc stopped past the deadline" case S2b exists for, but for any length of stop) finds every jarred tab's reader ended at once on `SIGCONT`, and every tab goes `[detached]` and through the retake, though no jar ended any lease. Whether the retake then succeeds depends on spyc closing the old connection first (Unit C's ground; I did not trace it). A caught signal delivered to a reader thread (Linux normally gives a process-directed signal to the main thread, so this is rare) does the same to one tab. The attach client, stopped and continued, ends with `[the connection closed]`.
- **Status:** confirmed (run, Linux; refuted for macOS).
- **Direction:** treat `Interrupted` as "try again" in `recv` (and `By::read`'s early `poll` return), as `read_exact` already does for the rest of a frame.

### B13. Once, under load on Linux, a count line went missing across an upgrade; not reproduced

- **Severity:** medium if real; **frozen** if real (the old image's behaviour during an upgrade). **Lens:** correctness.
- **Sites:** `tests/jar_process.rs:744-856` (`an_upgrade_keeps_the_program_its_pty_and_every_connection`), `tests/jar_process.rs:728-734` (`assert_unbroken`), `tests/jar_process.rs:237-248` (`Seen::take`, which already asserts every `output` frame's offset follows the last).
- **Claim:** in one of 31 loaded Linux runs the program's count, read by an owner across an upgrade with offsets that never jumped, went from 33 to 35.
- **Evidence:** `.../scratchpad/B/linux/out/stress48-jp-24.log`: `assertion \`left == right\` failed: the count runs on unbroken`, `left: 35`, `right: 34`. Since `Seen::take` would have panicked on a jump or a repeat in offsets, the bytes the owner received were the log's bytes, contiguous; the line `n=34` was absent or not a whole line in them. The test prints neither the text nor when the upgrade ran, so which is unknown. My diagnostic copy (`upgrade_count_diag` in `tests/review_b_probes.rs`: the same program, owner, reader and upgrade, dumping the text around any break with the upgrade's moments) found **0 breaks** in 64 rounds on macOS under 14 burners, 288 rounds on Linux under `stress-ng`, and about 3,400 rounds on Linux under `stress-ng` with the whole `jar_process` suite running beside it (`.../scratchpad/B/linux/diag2.sh`, output `.../scratchpad/B/linux/out/diag-linux2.txt` empty).
- **Reading:** two readings fit. Either a chunk of the program's output was lost across the park with the offsets kept consistent, which nothing in `jar/*.rs` I read allows (the pump never parks between a read and its append, `life.rs:63-106`; a writer parks only after moving its cursor, `conn.rs:389-417`), or the line was garbled at the terminal (the count and the shell's `while read` loop share the PTY, and the tty's echo of the typed `after` lands in the same stream), which is the program's business and the test's fragility.
- **Consequence:** if the first, output lost across an upgrade in the first jar released, for good; if the second, a test that can fail under load (B3).
- **Status:** unverified.
- **Direction:** make the test print the text around a break and the upgrade's moments, so the next occurrence settles it; run `stty -echo` in its program.

## Threads, locks and lifetimes (the map behind the findings)

### Each thread of a running jar

| Thread | Started at | Waits on | If it dies (panics, or never starts) |
|---|---|---|---|
| acceptor (the main thread) | `jar/mod.rs:412` | `poll` on the listener, 100 ms | the process exits (101): the program loses its PTY (B1, run) |
| PTY pump | `mod.rs:392` | `poll` on the master, 100 ms; the condition, 100 ms, when a window behind | the log stops; the program blocks once the PTY's buffer fills; `pty_eof` is never set, so no `exited` is ever sent; the jar answers pings and lookups as if well |
| input feeder | `mod.rs:396` | the inbox condition, 100 ms; `poll` for room, 100 ms | input queues to `INBOX_CAP`, then every `input` is refused |
| child waiter | `mod.rs:400` | `sleep` 50 ms between `waitpid(NOHANG)` | the exit is never recorded; the jar never ends |
| reaper | `mod.rs:404` | the condition, 200 ms | the jar never ends after an exit or a linger; `foreground` stops changing |
| first-owner hold | `mod.rs:410` (held starts only) | `sleep` for the hold | a held jar nobody claims never ends |
| a connection's reader (the thread `accept::serve` runs on) | `mod.rs:448`, and `mod.rs:381` after an upgrade | `poll`, 100 ms | an owner's lease is never released (B1, run) |
| a connection's writer | `conn.rs:105` | the condition, 500 ms; writes with a 100 ms timeout | a panic skips `writer_done` (`conn.rs:107`) and `close`: the client hears nothing more, pings included, while the reader carries on; an upgrade waits for a park that never comes (by reading) |
| a program's detach | `conn.rs:32` | the condition, 100 ms | the detach never happens |

Every one is `std::thread::spawn` (B1). None is joined except a connection's writer by its reader (`conn.rs:176-178`), and nothing notices a death (B2). In the code read I found no input that panics any of these loops: arithmetic on offsets keeps `acked <= head` (`conn.rs:303`, `accept.rs:232`) and `base <= head` (`state.rs:100-120`).

### Lock order

From every acquisition in `jar/*.rs` and the parts of `upgrade.rs` that touch `Shared`: `state` is taken first, then any of `conns` (`accept.rs:243` via `register`, `describe.rs:71`, `accept.rs:141`, `upgrade.rs:208-209`), a connection's `outbox` (`accept.rs:148`, `conn.rs:479`), its `cursor` (`conn.rs:480`), `drain` (`life.rs:48`, `conn.rs:46`), `listener` (`upgrade.rs:226-228`). Nothing takes `state` while holding any of those: `release` drops its `conns` guard at the end of its statement (`conn.rs:356`), `Conn::send` lets go of the outbox before it takes `state` (`outbox.rs:40-46`), `detach_owner` lets go of `drain` before `state` (`conn.rs:33-38`). `inbox`, `parked`, `tokenless` and `pending` are taken alone. I found no cycle.

What runs under `state`: `tcgetpgrp` on every reaper wake (`life.rs:160`), a zero-time `poll` (`life.rs:49`), the program's `fork` and `exec` for a held start (`accept.rs:215-216`, first owner only), `current_exe` and `stat`s in `describe` (B8), `remove_file` before an exit (`life.rs:39,186`), and copies of the log (B8). None blocks on a peer.

### Wakes

Every condition wait has a timeout (B4), so a lost wake costs at most the tick. One notify is made without the lock its waiter checks under: `Replay` (B9). `Conn::send` (`outbox.rs:40-46`), the pump's append (`life.rs:98-105`), `Ack` (`conn.rs:301-306`), `Resize` (`conn.rs:292-298`) and `release` (`conn.rs:357-373`) change state under the lock first.

### Lifetimes, by order of events (lines read; tests named where they cover it)

- **Exit with the owner there:** the waiter records it (`life.rs:140`); `exited` goes out once `pty_eof` is set and the log is sent (`conn.rs:496-499`), so a descendant that keeps the terminal holds it back on Linux (B5); `ack_exit` ends the jar within a reaper tick (`life.rs:185`). Tested: `the_exit_is_reported_and_collecting_it_ends_the_jar`.
- **Exit with nobody there:** kept for `linger_after_exit` (60 s; a capture's until read) from the later of the exit and the owner's going (`life.rs:181-184`). Tested: `an_exit_with_nobody_there_is_kept_from_the_exit`, `a_captures_exit_is_kept_until_an_owner_collects_it`.
- **Owner lost** (closed, or silent past `OWNER_DEADLINE`): `ownerless_since` set (`conn.rs:361`); with spyc's start (no `--linger-ownerless-ms` anywhere in `src/`), a pane waits for an owner for ever. The finite linger and its `FIRST_OWNER_GRACE` (`life.rs:30`) are reachable only by hand-run jars.
- **Upgrade:** the reaper's `exited_at` is local (`life.rs:155`), so an exited, ownerless jar that is upgraded keeps its exit another full linger; the lock and identity cross (tested by Unit A's file and `a_jar_describes_itself_and_keeps_its_identity_across_an_upgrade`).
- **Release:** the new token replaces the old, readers on the old one are closed, the releasing connection reads nothing more (`conn.rs:228-246`). Tested: `a_released_jar_opens_only_to_its_new_token`.

### Flow control

A live owner a window behind stops the pump (`life.rs:70-80`); with no owner a pane drops the oldest and a capture blocks (`life.rs:74-76`); a reader behind the ring gets a `gap` (`conn.rs:486-494`); a reader that takes nothing for the deadline (10 s) is closed (`conn.rs:445`), an owner never (`conn.rs:421-423`); a program that never reads its input fills `INBOX_CAP` (16 MiB) and further `input` is refused with reason `INVALID` (`conn.rs:272-277`), which spyc does not show (lead V7, Unit C). The tests cover each (`a_live_owner_paces_the_program_and_loses_nothing`, `an_ownerless_jar_drops_the_oldest_bytes_and_says_which`, `a_captures_jar_with_no_owner_holds_its_program_rather_than_drop_output`, `a_reader_that_falls_behind_is_told_what_it_missed`, `a_paste_the_program_never_reads_does_not_block_its_owner`), and they passed unloaded on both platforms.

### Resource hygiene over time

`.../scratchpad/B/linux/probe_leak.py` (Linux): a jar with an owner, put through three rounds of 300 `describe`s, 100 readers that replay and leave, its owner replaced, and 50 lookups that ask nothing: `fds=9 threads=7 rss=1496kB` at the start, `fds=9 threads=7 rss=1512kB` after the third round. No descriptor or thread leaked; memory moved by 16 kB. Memory that does grow is the ring's (B7) and the snapshots', both bounded by count, not by bytes in memory.

## Load runs

| Platform | Load | Test threads | Iterations | Red | Pass count |
|---|---|---|---|---|---|
| macOS, 14 cores | 14 `yes` burners | 32 | 3 | 1 | 2 |
| macOS, 14 cores | 14 `yes` burners | 64 | 4 | 4 | 0 |
| Linux aarch64, 7 CPUs | none (my `stress-ng` failed to start; not counted as loaded) | 32 | 6 | 0 | 6 |
| Linux aarch64, 7 CPUs | `stress-ng --cpu 7 --io 2 --vm 2`, load 9 to 21 | 48 | 25 | 4 (iterations 21 to 24) | 21 |
| Linux aarch64, 7 CPUs | the same, beside B13's probe | 48 | 6 | 0 | 6 |

Each iteration runs the whole `jar_process` binary (91 tests, Unit A's `upgrade.rs` included) and the `spyc-jar` crate's unit tests (21). Every failure is listed in B3 with its log. The macOS CPU stress totalled about 8 minutes, in runs of under 3 minutes each.

## Seeded leads

- **V8 (an idle jar is polling loops): confirmed.** B4: about 67 timed wakeups a second with an owner and 53 without on Linux (`strace -c`), about 108 and 88 context switches a second on macOS (`top`). The lead's 60 to 80 was right. The cost on battery needs `powermetrics` (manual item 1).
- **V9 (nothing supervises a jar's threads, and a jar has no log): confirmed, and worse than stated.** B1 and B2. A refused thread in the acceptor ends the process and the program (exit 101, run); a refused writer leaves a jar with an owner that can never leave (run). Every lock does recover from poison with `into_inner`; `catch_unwind` guards only `resume` (`upgrade.rs:811`). The thread table above says what each death does.
- **H1 (a program that exits while a descendant holds the PTY): partly confirmed.** B5. On Linux, when the descendant is in another process group (job control, `setsid`, `disown`), the owner is never told and signals are refused; a capture's jar waits for it for ever. Refuted on macOS (it revokes the terminal) and on Linux for a descendant in the program's own group or under `nohup`. A tab without jars also waits for end of file, so for panes it is parity.
- **H3 (what a jarred program inherits): confirmed in part.** B6. `SIGTTOU` is ignored for every jarred program's life (as it already was for a tab without jars, which portable-pty does not reset); the signal mask is empty; umask is spyc's (`022` in the probe; spyc's one `umask(0o077)`, `src/mcp/server.rs:590-592`, runs during `bootstrap.rs:244`, before the store and any tab start at `bootstrap.rs:347-355`, so no jar can inherit it); no jar descriptor reaches the program; the environment is spyc's at spawn plus `SPYC_JAR_SOCKET`; the jar holds spyc's working directory of the moment for its life (spyc's follows the focused column).
- **H4 (the log under one lock): partly confirmed.** B8. One mutex and one condition serve the pump, every writer, the reaper and each detach, and readers cost the owner about 60 per cent of its throughput with five of them; but a full-ring replay to a new reader cost the owner about 20 per cent and the worst ping waited 126 ms. Not a stall at today's one owner.

Also found while on these leads and the brief's Unit B questions: B3 and B13 (load failures), B7 (ring memory), B9 (a lost wake), B10 (a leaking test), B11 (the attach client's 2 s `describe` against a 15 s park), B12 (`EINTR` on Linux). Cross-references for other units: V7 (Unit C) is consistent with `client.rs:44` (`REST_OF_FRAME`, 3,600 s once a frame has begun) and with the refusal of input past `INBOX_CAP` reaching an owner that drops refusals; B12's consequence for the retake depends on Unit C's code; B11's 2 s `describe` is also spyc's.

## Manual checklist items

1. **Idle wakeups on battery (settles B4's cost).** With jars on, open ten tabs running `sleep 100000`, `^d` to detach, wait a minute, then `sudo powermetrics --samplers tasks --show-process-energy -i 10000 -n 1 | grep -E 'spyc-jar|Name'`. Expected: each `spyc-jar` at about 50 to 100 idle wakeups a second, a few per cent of one core's energy each. Repeat attached.
2. **Activity Monitor's view (B4).** Same ten tabs; Activity Monitor → Energy → add the "Idle Wake Ups" column. Expected: each `spyc-jar` in the tens per second, where `spyc` itself is near zero.
3. **A stopped and continued spyc on Linux (settles B12's consequence).** On a Linux machine with jars on, open two jarred agent tabs, then from another terminal `kill -STOP <spyc pid>; sleep 1; kill -CONT <spyc pid>`. Expected by B12: both tabs flash `[detached]` and are taken back, or stay `[detached]`. Expected if B12 is wrong: nothing visible. Repeat on macOS: nothing visible.
4. **A leftover in a jarred shell on Linux (B5).** In a jarred `bash` tab on Linux: `sleep 600 & disown; exit`. Expected: the tab keeps showing the program running for 10 minutes, and closing it leaves `sleep 600` and its `spyc-jar` (`ps -ef | grep -E 'spyc-jar|sleep 600'`) for the 60 s exit linger. On macOS the tab ends at once.
5. **Ejecting a volume (B6).** Mount a disk image, browse into it in a column, open a shell tab there, `cd ~` in the tab and move the column away, then eject the image in Finder. Expected by B6: "The disk wasn't ejected because one or more programs may be using it", and `lsof +D /Volumes/<name>` names `spyc-jar`.

## Not covered

- Release builds: every measurement and load run used debug builds (the pin's gate binaries and my worktree's). Wakeup counts don't depend on the profile; throughput (B8) would be higher.
- x86_64 and glibc Linux, which CI runs; WSL.
- Thread exhaustion on macOS (B1 was shown on Linux only).
- A months-long soak: the leak probe ran three rounds of churn, minutes, not days.
- `attach.rs`'s relay in a real terminal (`^\`, pastes, resizes): read in full, not run; lead H6 is Unit D's.
- `client.rs`'s `Outbox` stall behaviour against a real jar that reads a byte at a time.
- Lines 204 to 246 of `tests/jar_process/timers.rs` (two tests) were skimmed, not read.
- I did not trace what spyc does after B12's `Lost::Held` (Unit C's `retake`), nor whether spyc's own jar session shows B5.
