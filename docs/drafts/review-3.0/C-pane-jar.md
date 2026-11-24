# Unit C: spyc's side of a pane's jar

SHA: `444857a2` (main at the pin). Local `main` moved to `52ce15b7` (#1317) during the review; it touches `tests/jar_adopt.rs` and none of this unit's source files, and nothing here was re-read against it. Platform: macOS 25.6 on Apple silicon for every read, probe and red test; Linux (the review's Podman VM: Alpine, aarch64 musl, not CI's x86_64 glibc) for the stopped-jar probes of C2 and C3 only.

Worktree with the red tests (uncommitted, pinned to `444857a2`): `~/src/spyc.worktrees/review/3.0-c`, branch `review/3.0-c`. Probe scripts and logs: `docs/drafts/review-3.0/evidence/C/`.

## Verdict

spyc's side of a pane's jar does what the plan says on the paths its tests drive, and holds the MVU split: every jar wait found runs on a worker (the `jar-off-the-loop` guard holds, and tab closes and lands shut down off the loop). It fails the unit's first question, "can the program ever run twice?", on two ordinary paths, both shown by evidence: a restore with jars off (the owner's way back, and with lead V1 as coded, the first start of a plain `3.0.0` over a detached rc session) starts every restored tab again beside its still-running jar (C1, red test); and a jar silent for about 40 s (stopped, starved) leaves its tab `[detached]` for the session with a blank screen, after which `^a R` starts a second copy without asking (C2, probed on macOS and Linux). Lead V7 holds: a hung jar is invisible until socket buffers fill (C3), and input the jar refuses is dropped silently (C4); a confirmed pipe over 16 MiB kills a jarred tab's input (C5, red test). Tasks and captures are not retaken after a lapse (C6). The reply gate after a replay is a clock that starts too early (C7). None of this is in the frozen surface: all of it is spyc's and can be fixed in a patch release, but C1 and C2 should be fixed before jars are on by default, and C1's interaction with V1 decides what the rc-to-final upgrade does to running agents.

## Coverage ledger

| File | Lines | Read |
|---|---|---|
| `src/pane/jar/mod.rs` | 774 | in full |
| `src/pane/jar/link.rs` | 573 | in full |
| `src/pane/jar/offsets.rs` | 76 | in full |
| `src/pane/jar/repaint.rs` | 103 | in full |
| `src/pane/jar/snapshots.rs` | 196 | in full |
| `src/pane/jar/place.rs` | 266 | in full |
| `src/pane/jar/tokens.rs` | 70 | in full |
| `src/pane/jar/store.rs` | 371 | in full |
| `src/pane/jar/orphans.rs` | 135 | in full |
| `src/pane/jar/listing.rs` | 85 | in full |
| `src/pane/jar/test_dir.rs` | 157 | in full |
| `src/pane/jar/shutdown.rs` | 216 | in full (Unit D's) |
| `src/pane/jar/session.rs` | 313 | in full (Unit D's) |
| `src/pane/jar/attach.rs` | 468 | skimmed: functions, constants and every signal (Unit D's) |
| `src/pane/jar/tests.rs` | 2295 | skimmed: every test's name and doc; read in full lines 1-184, 586-667, 918-937, 1210-1241, 1812-2176 |
| `src/pane/pty_host.rs` | 1148 | in full |
| `src/pane/pty_host/input_writer.rs` | 406 | in full |
| `src/pane/engine_ghostty/jar_snapshot.rs` | 507 | in full |
| `src/app/pane_starts.rs` | 423 | in full |
| `src/app/capture_starts.rs` | 226 | in full |
| `src/app/jarred_tasks.rs` | 84 | in full |
| `src/app/jar_info.rs` | 65 | in full |
| `src/app/jarless.rs` | 215 | in full |
| `src/app/orphan_jars.rs` | 279 | in full |
| `src/app/jar_upgrades.rs` | 312 | in full |
| `src/app/pane_tabs.rs` | 1075 | in full |
| `src/app/harness_tests/jar_start.rs` | 711 | in full |
| `tests/jar_adopt.rs` | 2189 | skimmed: every test's name and doc, lines 1-60 |
| Read to follow a call: `crates/spyc-jar/src/client.rs` 1-386, `jar/input.rs` 1-80, `jar/conn.rs` 240-360, `jar/life.rs` 55-115, `src/pane/mod.rs` 300-455, `src/app/pre_recv.rs` 190-230, `src/app/streaming.rs` 240-330, `src/app/file_ops.rs` 325-370 and 530-591, `src/app/clipboard.rs` 120-160, `src/app/session_restore.rs` 1-60, `src/app/bootstrap.rs` 335-365, `src/app/detach.rs` (outline) | | partial, as named |

## Findings

### C1. A restored tab whose jar still runs is started a second time when jars are off

- Severity: **high**. Not frozen. Lens: correctness.
- Sites: `src/app/pane_starts.rs:126-128` (the jars-off branch of `launch_tab_pane` launches natively and never looks at `restoring`); `src/pane/jar/mod.rs:143-152` (`wanted` returns `None` for "off or not honoured" without asking whether the saved jar runs); callers `src/app/session_restore.rs:18-30` (`open_restored_pane_tab`), `src/app/orphan_jars.rs:203-212`. Contrast `src/app/jarless.rs:137-144`, which does check `socket_state` before starting a restored tab without a jar.
- Claim: with `[pane] jars = false`, or on any build `pane::jar::honoured()` refuses (lead V1: a plain `3.0.0`), `spyc -r` or a handover restores a tab whose saved `jar_socket` names a live jar by starting the saved command again on spyc's own PTY, while the jar's program runs on, ownerless.
- Evidence (failing test, watched red): `review_c_a_restored_tab_whose_jar_runs_is_not_started_again_with_jars_off` in `~/src/spyc.worktrees/review/3.0-c/src/app/harness_tests/jar_start.rs`. It starts a jarred `sh -c 'echo $$ >> pid; exec cat'`, severs the connection as a crash does, sets `config.pane.jars = false`, and restores the tab with its `SavedTab.jar_socket`. Run: `cargo test --lib review_c_ -- --test-threads=1` with `CARGO_TARGET_DIR=$(make -s target-dir)` and scratch `HOME`/`XDG_*`. Output: `assertion left == right failed: its program ran twice: 825\n883  left: 2 right: 1` (log `unit-c/redtests-run2.log`).
- Consequence: a Claude tab is restored by a fresh `claude` plus the `/resume <sid>` injection, so two Claude processes run the same conversation in the same worktree, one invisible. The two ways this happens in ordinary use: the owner's documented way back (`jars = false`, decision 1) after a crash or a detach, and, with V1 as coded, the first `3.0.0` start after an rc session was detached, or `:upgrade`'s handover from an rc into the final build.
- Status: confirmed.
- Direction: the restore path asks `socket_state` of a saved `jar_socket` whatever the jar setting, and leaves a live one `[detached]`, as `jarless.rs` already does.

### C2. A jar that is silent for about 40 s leaves its tab `[detached]` for the session, and `^a R` then runs the program twice

- Severity: **high**. Not frozen. Lens: correctness, reliability.
- Sites: `src/pane/jar/link.rs:227-234` (a failed connection with the lock held is `Lost::Held`, "detached"); `crates/spyc-jar/src/client.rs:188`, `:283-289` with `src/pane/jar/mod.rs:404` (`WRITE_STALL` 20 s ends the connection); `src/app/pane_starts.rs:149-200` (one retake); `src/pane/jar/mod.rs:544-555` (`adopt`: a welcome not seen within `ADOPT_WAIT`, 20 s, gives `link::detached`); `src/pane/mod.rs:357-366` (`lease_lapsed` needs `jarred()`, which a detached stand-in is not, so no second retake); `src/app/pane_tabs.rs:34-36` (no confirm for a closed tab), `:796-842` (`restart_tab_now`: `try_kill` on the stand-in's `GoneChild` is a no-op, `link.rs:551-557`, and the new `TabInfo` has a new id, so a new socket and a new jar).
- Claim: a jar that reads nothing for 20 s while spyc has more to send than the socket buffers hold, and then for 20 s more while the retake waits, leaves its tab `[detached]` with nothing on screen to say its program still runs; nothing takes it back when the jar recovers, and `^a R` starts a second copy without asking while the first runs on in the ownerless jar.
- Evidence (command and output): `unit-c/drive_v7.py` in mode `pasteR` drives the pin's debug `spyc` in a pty with scratch `HOME`, `XDG_*`, `TMPDIR` and `XDG_RUNTIME_DIR` (`/tmp/sc7…`), `SPYC_*` unset; it opens a jarred tab running `sh -c 'echo READY_$$; while read l; do echo got_$l; done'`, `SIGSTOP`s that tab's jar (the pid read from `ps` as the probe spyc's child), pastes 200 kB, waits 60 s, `SIGCONT`s the jar, types, then presses `^a R`. Log `unit-c/pasteR1/events.log`:
  ```
   7.585 SIGSTOP jar 52950
  47.719 first detached 40.1 s after SIGSTOP
  67.682 SIGCONT jar
  78.686 typing after SIGCONT reaches the program False
  91.139 new programs started after ^a R [b'67253']
  91.587 processes holding files in my runtime dir [... '52950 52830 spyc-jar sh --wire 1', '67194 52830 spyc-jar sh --wire 1']
  91.607 old program alive 52951 sh -c echo READY_$$; ...
  ```
  The same in mode `paste` (`unit-c/paste1/events.log`): detached at 40.2 s, and after `SIGCONT` the tab stays detached and typing never reaches the program.
  Linux (Podman VM, Alpine aarch64 musl, the Linux gate's debug build at the pin), `unit-c/linux/pasteR120/events.log`: detached 46.5 s after the stop; `SIGCONT` at 76.6 s; `^a R` restarted without asking ("restarted sh -c …"), a new program (pid 72) started while the old one (pid 32) still ran in its jar (pid 24). With the jar continued at 60 s instead (`unit-c/linux/pasteR/events.log`), the retake still in its 20 s wait succeeded, and `^a R` asked "restart running tab 'sh'? [y/N]": the outcome turns on whether the jar is silent past `WRITE_STALL` plus `ADOPT_WAIT`.
- The same end by another road: `adopt` welcomed, then `client.recv(ADOPT_WAIT)?` or `connect(...)?` failing (`mod.rs:567-576`, `:596`) returns an error, which `land_pane_start` shows as `[not started]` with "^a R starts it again" (`pane_starts.rs:314-322`), though the jar welcomed spyc as owner and its program runs; `^a R` then starts a second copy. Read, not run.
- Consequence: a jar starved, stopped or slow for under a minute (the owner's stated bar: under load) costs the user the tab, and the natural recovery runs a second agent in the same worktree. The orphan look that could offer the jar back runs once a session, at start (`orphan_jars.rs:33-49`).
- Status: confirmed (probes on macOS and on Linux).
- Direction: a tab whose jar this spyc owned and still runs stays retakable (the retake retried on the jar's lock, not on one welcome's clock), and `^a R` on a tab with a live jar asks, or ends that jar first.

### C3. The owner's reader never notices a jar that holds its lock and answers nothing (V7)

- Severity: **medium**. Not frozen. Lens: reliability.
- Sites: `src/pane/jar/link.rs:164-166` (`NO_REPAINT_DUE`, 3600 s), `:181-237` (`read_frames`: no deadline on receiving, `Pong` ignored by `Ok(_) => {}`); `crates/spyc-jar/src/client.rs:263-290` (pings written, never matched to a pong; only a write that blocks for `WRITE_STALL` ends the connection); the guard's stated reason, `src/pane/jar/tests.rs:1825-1828` ("the connection's deadline ends it"), which no code backs.
- Claim: spyc learns that a jar has stopped answering only once its writes to that jar block for 20 s, which needs the 8 KiB send buffer plus the jar's 8 KiB receive buffer to fill (`sysctl net.local.stream.sendspace: 8192`, `recvspace: 8192` on this Mac); until then the tab looks live and shows nothing.
- Evidence (command and output): `drive_v7.py` mode `typing`, jar `SIGSTOP`ped for 150 s while the probe typed 740 keys (one `x` every 200 ms, `Enter` every tenth): `157.751 watch over; firsts {} keys sent 740`: no label, no message, no change on screen in 150 s (`unit-c/typing1/events.log`). After `SIGCONT` the queued keys reached the program (the screen shows `got_xxxxxxxxxafter` on row 39 of `typing1/raw.out`; the probe's own check for `got_after` was wrong, so its "False" there is the probe's error). On Linux the same typing probe was noticed at 85.5 s (`unit-c/linux/typing/events.log`: "first detached 85.5 s after SIGSTOP"), since a Linux unix socket charges each small frame far more than its bytes against the buffer; the tab then showed `[detached]`, and the jar, continued 12 s later while the retake still waited, was taken back. Mode `idle` (no keys, only spyc's pings): the stopped jar was first noticed, as `[detached]`, after **1193 s on macOS** (`unit-c/idle1/events.log`: "first detached 1193.1 s after SIGSTOP") and **578 s on Linux** (`unit-c/linux/idle900/events.log`), and in both the tab stayed detached after `SIGCONT` (C2).
- Consequence: a hung jar is a frozen tab with no explanation for about 20 minutes on macOS (8 KiB of 14-byte pings every 2 s is 1170 s, plus the 20 s stall: the measured 1193 s) and about 10 minutes on Linux, unless the user pastes; and what ends the wait is C2's permanent `[detached]`.
- Status: confirmed (probes on macOS and Linux, idle and typing).
- Direction: the reader holds a receive deadline fed by the pongs to its own pings, and a jar past it is shown as not answering (distinct from detached and lost), with the retake of C2 waiting on its lock.

### C4. Input the jar refuses is dropped without a word, and a jarred tab loses the input queue's bound

- Severity: **medium**. Not frozen. Lens: correctness.
- Sites: `src/pane/jar/link.rs:221-225` (a `Refused` that isn't an upgrade's is ignored), `:293-306` (`JarWriter::write` queues on the `Outbox` and returns at once); `crates/spyc-jar/src/client.rs:189` (the `Outbox` queue is an unbounded `mpsc::channel`), `:232-246`; `crates/spyc-jar/src/jar/input.rs:29-48`, `jar/conn.rs:271-277` (past `INBOX_CAP`, 16 MiB unread, the jar answers `refused` reason 8, "the program isn't reading its input", and drops the frame); `src/pane/pty_host/input_writer.rs:10-11`, `:112-147` (the 8 MiB bound the native path keeps, and its "input was not sent" error).
- Claim: for a jarred tab the input worker's 8 MiB bound measures only bytes not yet handed to the `Outbox`, which takes them at once, so a program that stops reading lets input pile up in spyc's unbounded queue and the jar's 16 MiB inbox, after which the jar refuses each frame and spyc drops the refusal; the user's paste or keys are lost with no error, where a tab without a jar says "child input queue full; input was not sent".
- Evidence: lines read, reasoning closed.
- Consequence: a paste into an agent that is busy and not reading is silently cut, and the user can't tell which part reached it.
- Status: confirmed (by reading).
- Direction: `read_frames` turns a refusal of input into a note on the tab (or the flash the native path gives), and the `Outbox` reports queued input bytes back to the input worker's bound.

### C5. A confirmed file pipe over 16 MiB ends a jarred tab's input for good

- Severity: **medium**. Not frozen. Lens: correctness.
- Sites: `src/app/file_ops.rs:328-370` (the pipe payload has no upper bound), `:561-583` (over 8 MiB asks `[y/N]`); `src/pane/pty_host/input_writer.rs:100-104`, `:122-133` (a confirmed pipe may be any size); `src/pane/jar/link.rs:296-302` (one `Input` frame for the whole buffer); `crates/spyc-jar/src/client.rs:233-239` (`InvalidInput` over `MAX_FRAME`); `src/pane/pty_host/input_writer.rs:69-72` (the worker returns on the first write error, and every later batch fails "child input writer closed").
- Claim: piping more than 16 MiB of text into a jarred tab and confirming it ends that tab's input worker, so nothing typed into the tab reaches its program again, while the program runs on.
- Evidence (failing test, watched red, with a passing control): `review_c_a_pipe_over_the_frame_limit_leaves_the_tabs_input_working` (red: `input after a large pipe is still accepted: child input writer closed`) and `review_c_control_a_pipe_over_the_frame_limit_without_a_jar` (green: the same 17 MB pipe and key with no jar), in `~/src/spyc.worktrees/review/3.0-c/src/pane/jar/tests.rs`; log `unit-c/redtests-run2.log`.
- Consequence: the tab is dead to the keyboard until closed; `^a R` restarts the agent.
- Status: confirmed.
- Direction: `JarWriter` splits input into frames under `MAX_FRAME` (a confirmed pipe is bracketed paste, so the split doesn't change what the program reads), and the worker survives one failed batch.


### C6. A task or capture whose jar ends spyc's lease is shown as ended while its program runs, and is never taken back

- Severity: **medium**. Not frozen. Lens: correctness.
- Sites: `src/app/pane_starts.rs:149-200` (`retake_lapsed_tabs` walks `every_tab_set()` only); `src/app/streaming.rs:253-262` (a capture's EOF is reaped and finalized), `:299-313` (a task's); `src/pane/jar/link.rs:227-234` (the lapse arrives as an exit with the signal label "detached"); `src/app/jarred_tasks.rs:18-35` (a finalized task isn't saved); `src/app/orphan_jars.rs:33-49` (the look for ownerless jars runs once a session).
- Claim: when a jar ends this spyc's lease (spyc stopped past `OWNER_DEADLINE`, or a `WRITE_STALL` as in C2), a tab is taken back, but a background task (a demoted tab) or a `!` capture is finalized as exited, its program still running in the jar with no owner; nothing takes it back this session, and a capture's jar, in `block` mode, stops its program once the ring fills.
- Evidence: lines read, reasoning closed. No test covers a task's or capture's lapse: `tests/jar_adopt.rs:1887` (`a_spyc_stopped_past_the_deadline_takes_its_tabs_back`) covers tabs, `src/app/harness_tests/jar_start.rs:416` too.
- Consequence: a long build sent to the background (`^Z` on a capture) shows "exit 1" after spyc is stopped for ten seconds, keeps running unseen, and its output after that point is never shown; the next spyc start offers it as an orphan.
- Status: confirmed (by reading); a red test was not written.
- Direction: the retake walks tasks and the pending capture as it walks tabs (`retake_task_host` exists for the restore).

### C7. A clock decides whether a terminal reply reaches the program after an adoption (V11, spyc's half)

- Severity: **medium**. Not frozen. Lens: correctness.
- Sites: `src/pane/jar/link.rs:80-130` (`REPLY_GRACE`, `ReplayGate`), `:202-205` (`handed_over` is called once the bytes are in the 64-deep channel to `PtyHost`'s reader, not once parsed), `:311-319` (a closed gate drops the reply and reports success); `src/pane/jar/mod.rs:590-595` (`ReplayGate::until(head)`); `src/pane/pty_host.rs:55` (a further 512-chunk channel to the parser).
- Claim: the gate opens 500 ms after the last replayed byte is queued for the parser, not after the parser has read it, so under load a replayed query still queued (up to 64 jar frames plus 512 chunks of 8 KiB) is answered a second time (R15a broken), while on an idle machine a query the program asks within 500 ms of the hand-over gets no answer; and when the jar's head equals the snapshot's offset (nothing to replay), `handed_over` is never called until the program prints, so every reply is dropped until its first output plus 500 ms.
- Evidence: lines read, reasoning closed. The test `replies_wait_until_the_replayed_tail_is_parsed` (`src/pane/jar/tests.rs:921`) is named for the parse but the gate never consults the parser.
- Consequence: after `spyc -r` or a retake, a full-screen program that queries on redraw (cursor position, colours) can read a stale answer as typed input, or wait on an answer that was dropped.
- Status: confirmed (by reading).
- Direction: the parser worker reports the pane-byte count it has parsed (it already keeps it for `Snapshotter`, `snapshots.rs:95-99`), and the gate opens when that passes the hand-over point; no clock.

### C8. The wait guard's reasons and reach are narrower than it says

- Severity: **low**. Not frozen. Lens: maintainability.
- Sites: `src/pane/jar/tests.rs:1812-1945` (`every_wait_on_a_jar_outlasts_it_or_says_why_not`): it scans `src/pane/jar` only, and only lines of the form `const NAME: Duration =`; it keys on the name, so `UPGRADE_WAIT` at `src/pane/jar/mod.rs:276` (spyc's wait for a jar's upgrade answer) is classified by the reason written for `attach.rs:40`'s `UPGRADE_WAIT` ("a session's spyc restarting, not a wait on a jar"); `NO_REPAINT_DUE`'s reason ("the connection's deadline ends it", `:1825-1828`) is untrue (C3); `REPLY_GRACE`'s ("the engine's parser, not a wait on a jar", `:1888`) excuses a clock that decides correctness (C7). Waits on jars outside the scan: `END_WAIT` (`src/app/pane_starts.rs:353`, `src/app/capture_starts.rs:30`), `BUSY_RETRY` (`src/app/jar_upgrades.rs:29-33`), and the client crate's `DESCRIBE_WAIT` (2 s, `crates/spyc-jar/src/client.rs:316`), which the other guard classifies as a command's wait though `owner_mcp`, `orphans::find`, `store::reported_builds` and `jar_upgrades::look` all use it.
- Claim: the guard passes while a wait it lists is misdescribed and others aren't in its reach.
- Evidence: lines read; `rg -n "const (UPGRADE_WAIT|END_WAIT)" src/pane/jar src/app` lists the duplicate and the waits outside.
- Status: confirmed.
- Direction: key the guard by file and name, extend it to `src/app`'s jar modules, and drop "not a wait" reasons that decide a correctness question.

### C9. A spyc's look at a jar after its upgrade gives up after 2 s and stops upgrades for the session

- Severity: **low**. Not frozen. Lens: reliability.
- Sites: `src/app/jar_upgrades.rs:226-251` (`look`: after `Upgraded`, `describe` must answer, else "it resumed as nothing it would name"), `:206-219` (any such answer sets `stopped` and flashes an error); `crates/spyc-jar/src/client.rs:316`, `:324-334` (`DESCRIBE_WAIT`, 2 s to connect and 2 s for the answer).
- Claim: a jar that upgraded and is slow to answer one lookup under load is reported as a failed upgrade and ends upgrades for every other jar this session.
- Evidence: lines read.
- Consequence: an error flash and jars left on the old build until the next start; nothing is lost.
- Status: confirmed (by reading).
- Direction: the confirmation rides the `upgraded` frame's build (which the jar sends) rather than a second, clocked lookup.

### C10. The store's sweep can remove the entry a running spyc of another build still starts its jars from

- Severity: **low**. Not frozen. Lens: reliability.
- Sites: `src/pane/jar/store.rs:270-298` (`sweep` spares only entries a live jar reports, the sweeper's own, and those touched in 24 h), `:249-264` (`touch` only at a spyc's start and at each jar's start, `src/pane/jar/mod.rs:464-468`), `:327-371` (each spyc caches its entry's path for its life); `src/pane/jar/mod.rs:480-492` (a missing binary is `NoUsableJar`).
- Claim: a spyc of build X that has run more than a day and has no live jar at the moment a spyc of build Y starts loses its store entry, and its next tab fails "no usable spyc-jar … check spyc's installation" with the offer to run tabs without jars.
- Evidence: lines read, reasoning closed; not run.
- Consequence: a confusing failure and a prompt that leads a user to run unjarred, in a long-lived detached session after an upgrade.
- Status: unverified (narrow preconditions; reasoning closed but not reproduced).
- Direction: each running spyc touches its own entry on a timer it already has, or the sweep spares entries a running spyc names (its pid in the entry).

### C11. Smaller clock verdicts on spyc's side

- Severity: **informational**. Not frozen. Lens: reliability.
- `BUILD_INFO_WAIT` (60 s, `src/pane/jar/store.rs:70`, `:222-233`): a store copy whose first run takes longer under load falls back to the `spyc-jar` beside spyc (the store's protection off for the session, said only on `:stderr`), and if that also takes longer, no jar this session and the offer to run without jars.
- `WELCOME_WAIT` (5 s, `crates/spyc-jar/src/client.rs:47`) inside `answer` (`src/pane/jar/mod.rs:668-701`): a jar that accepted the `hello` but welcomes late is hung up on and dialled again; the first owner's departure is harmless to the program, but each try costs the jar an owner change. When the hold (`place::HOLD`, 30 s) runs out between a hello the jar accepted and its welcome, `jar.kill()` (`:517`) ends a program that did start, and the tab says it ran nothing.
- `ENDING_WAIT` (2 s, `src/pane/jar/mod.rs:377`, `:448-456`): a restart whose last jar takes longer to end fails `[not started]`; documented and recoverable.
- `DISCARD_WAIT` (5 s, `src/pane/jar/orphans.rs:88`, `:102`): a discard of a jar that is parked or slow fails, the error dropped by `let _ =` (`src/app/orphan_jars.rs:163-167`) after the flash "jars: ending N no spyc owned"; the jar and its program run on.
- Status: confirmed (by reading).

### C12. `[detached]` stands for four different states, with nothing on the tab to tell them apart

- Severity: **low**. Not frozen. Lens: correctness (what the user is told).
- Sites: `src/pane/jar/link.rs:442-447` (a detached tab's stand-in has an empty screen), `:258-265` (one label); `src/pane/jar/mod.rs:544-555` (the reason goes to standard error only, `eprintln!`); `src/app/pane_starts.rs:9-13`.
- Claim: a tab reads `[detached]` with a blank screen when another spyc owns its jar, when this spyc has no token for it, when the jar didn't welcome spyc within 20 s (stopped, starved, parked past its limit), and when a lease lapsed and its retake failed; only `:stderr` says which, and the first is the only one in which the program is somebody else's. `spyc --jars` (`src/pane/jar/listing.rs:44-85`) shows `owner none` for the last three and "no answer" for a stopped jar, and shows the program's pid but not the jar's, which is the one a user would need to end a hung jar.
- Evidence: lines read; the probe in C2 showed the blank `[detached]` tab.
- Consequence: the user can't tell "another terminal has this agent" from "this agent is ownerless and waiting for you", and C2's `^a R` follows.
- Status: confirmed.
- Direction: the stand-in's note says why (the reason `adopt` already has), and `--jars` names the jar's pid.

### C13. Two key handlers reach the OS directly

- Severity: **informational**. Not frozen. Lens: maintainability (MVU).
- Sites: `src/app/orphan_jars.rs:159-170` (the `d` answer spawns a thread that connects to and signals jars, from `handle_orphan_jars_key`); `src/app/jarless.rs:105-121` with `:137-144` (the `y` answer calls `socket_state`, an `fcntl` lock probe and a non-blocking connect, from `handle_jarless_fallback_key`).
- Claim: both do OS work inside a key handler rather than returning an `Effect`, against `AGENTS.md`'s "Effects are data; `run_effects` is the only executor"; neither blocks the loop.
- Evidence: lines read.
- Status: confirmed.
- Direction: an `Effect` each, as the other jar work goes through workers.

### Notes for other units

- Unit D: tokens are filed by the socket's stem (`src/pane/jar/tokens.rs:29-35`), pane jars and session jars alike, so a session whose name is 16 lowercase hex digits would share a token file name with a pane jar's; whether session names can take that form is D's question.
- Unit E: `listing::print` (`src/pane/jar/listing.rs:10-29`) and `orphans::find` (`src/pane/jar/orphans.rs:36-53`) read the socket directory without `place::private_dir`; `find` connects (as a lookup) to every socket there.

## Seeded leads

**V7 (owned): confirmed, with one premise refined.** `read_frames` waits up to `NO_REPAINT_DUE` (3600 s, `src/pane/jar/link.rs:166`) with no receive deadline, ignores `Pong` and every `Refused` but an upgrade's (`:221-226`). Probed on macOS (C3): a `SIGSTOP`ped jar is invisible while the user types (150 s, 740 keys, nothing shown), and the keys typed meanwhile are delivered once it continues. Refined: what ends the connection is a write blocked for `WRITE_STALL` (20 s), which with a paste comes 20 s after the stop, and then the tab goes `[detached]` at about 40 s and never comes back (C2), which is worse than the lead says. The dropped `refused` for input past `INBOX_CAP`: confirmed by reading (C4); the user sees nothing at all, where a tab without a jar says "input was not sent". Idle, measured: 1193 s on macOS, 578 s on Linux before anything shows.

**V11 (owned, spyc's half): partly confirmed.** `ReplayGate` (`link.rs:80-130`) is clock-decided as the lead says, and worse in one way: the clock starts when the tail is queued, not parsed, and with nothing to replay the gate stays shut until the program next prints (C7). `REPAINT_HOLD` (100 ms, `repaint.rs:16`): a clock verdict, confirmed by reading; expiry concludes the program has seen the size a row off, which a program not scheduled within 100 ms (a loaded machine) hasn't, and then it reads only its unchanged size and doesn't repaint, leaving the snapshot plus the bytes after the gap on screen until something else redraws; there is no state on spyc's side that would decide it, short of the program's own output, which the module doc explains can't be trusted either. `ENDING_WAIT` and `EXIT_WAIT` (2 s): clock verdicts whose expiry concludes nothing that is lost (a restart fails `[not started]`; a shutdown leaves the jar to end its program), confirmed by reading; not findings. `WELCOME_WAIT`: in `answer` and `adopt` (C11). `FRAME_DEADLINE`, `LOOKUP_DEADLINE`, `FLUSH_DEADLINE`, `PARK_DEADLINE`, `HOLDER_ANSWER`: the jar's (Units A and B). `RELEASE_WAIT`: Unit D's moves. Reason 6 against 5: Unit A's. Further clock verdicts found on spyc's side: `DESCRIBE_WAIT` after an upgrade (C9), `BUILD_INFO_WAIT` (C11), `ADOPT_WAIT` deciding `[detached]` for good (C2).

**V1 (Unit F's), the call sites from the code side: confirmed.** `pane::jar::honoured()` (`src/pane/jar/mod.rs:131-137`) is read at five sites: `src/pane/jar/mod.rs:148` (`wanted`), `src/app/pane_starts.rs:126` (`launch_tab_pane`), `src/app/capture_starts.rs:42` (`capture_host`), `src/app/orphan_jars.rs:65` (`jars_wanted`, which also gates `jar_upgrades` through `startup_settled_for_jars`), `src/app/bootstrap.rs:347` (the store's start); `src/config/mod.rs:286` mentions it in a doc. Search: `rg -n "honoured\(\)|honoured_by|jar::honoured" src tests crates`. On a build `honoured()` refuses, two consequences in this unit go beyond "jars off": every restored tab whose jar still runs is started a second time (C1), which is exactly what the first `3.0.0` start does to a detached rc session, by `spyc -r` or by the upgrade `spyc attach` offers; and a restored task's or capture's retake fails with "this build doesn't run programs in jars" (`capture_starts.rs:176-177`) while its jar's program runs on. `^d` is refused while any tab runs outside a jar (`src/app/detach.rs:83-90`, `:149-165`), so on such a build it never detaches with tabs open (Unit D/F to confirm what the user is told).

## Manual checklist items

1. **A stopped jar, in the owner's terminal** (settles C2, C3; macOS and, if there is a Linux box, there too). In a scratch spyc (`HOME`, `XDG_*`, `TMPDIR` and `XDG_RUNTIME_DIR` in a temporary directory, `SPYC_*` unset), `^a c` and run `sh`. In another terminal, `ps -o pid,ppid,command -A | grep 'spyc-jar sh'` and take the one whose parent is that spyc; `kill -STOP` it. Wait two minutes typing now and then: expected, nothing on screen says anything. Paste a few hundred kB of text: expected, `[detached]` after about 40 s. `kill -CONT` the jar: expected, the tab stays detached. `^a R`: expected, no question, and `ps` shows two `sh` programs, one in each of two jars. End both jars with `kill` afterwards.
2. **Restore with jars off** (settles C1 in the real binary): in a scratch spyc with an agent tab, `^d`; set `[pane] jars = false` in the scratch `.spycrc.toml`; `spyc -r` (or `spyc attach`). Expected at the pin: the tab starts afresh, and `ps` shows the old program still running under its `spyc-jar`.
3. **Pipe over 16 MiB into a jarred agent tab** (settles C5 from the keyboard): select files totalling more than 16 MiB of text, `^W p`, answer `y`; then type into the tab. Expected at the pin: nothing typed arrives.

## Not covered

- **Linux** was used only for the stopped-jar probes (C2, C3), with the Linux gate's debug build at the pin. The red tests ran on macOS only.
- `src/pane/jar/attach.rs` and `tests/jar_adopt.rs` were skimmed, not read in full; `src/pane/jar/tests.rs` was read in the parts named in the ledger.
- No red test for C6 (task and capture lapse), C7 (the reply gate under load: making the parser slow on purpose was not attempted), C9 or C10.
- `capture_starts.rs`'s retake of a restored capture under load, and `jar_info.rs`'s `tell` across a retake (a fresh `Link` starts with an empty `told`, so it re-tells every key, which is correct), were read but not exercised.
- The snapshot fidelity path (`jar_snapshot.rs`, `engine_from`) was read, not tested beyond the in-repo tests; no snapshot from another engine pin was built.
- Observed while checking for my own leftovers, not mine and not touched: a `/bin/sh /tmp/jsx0Kz2H/gated-jar … --socket … --start-on-owner 30000` process (pid 66123, parent 1) running a pre-pin jar command line, apparently left by an earlier run of `harness_tests::jar_start`'s `gated_jar`; Unit F may want it (a test that leaks a process past `TestDir`).

## Recap

Findings: 2 high (C1, C2), 5 medium (C3, C4, C5, C6, C7), 4 low (C8, C9, C10, C12), 2 informational (C11, C13). None touches the frozen surface. The three to fix first: C1 (a restore never starts a tab whose jar runs, whatever the jar setting), C2 (a tab this spyc owned stays retakable, and `^a R` never runs a second copy beside a live jar), C3 (a receive deadline on the owner's link, so a hung jar is said, not silent).
