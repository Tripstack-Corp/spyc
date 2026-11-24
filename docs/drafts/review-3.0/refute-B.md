# Refuter pass: B1 and B13

**SHA:** `444857a2` (`main`), read in `~/src/spyc.worktrees/review/3.0` (HEAD checked: `444857a2a32b2f284d2b4fd19e57aef19700a5c5`).
**Platform:** macOS 26 for the reading; one experiment on Linux (aarch64, the `localhost/spyc-review-linux` image under Podman, `--network none`). No spyc or `spyc-jar` process was started; nothing of the owner's was touched.

| Finding | Verdict |
|---|---|
| B1 | `confirmed`, and wider than written: part of it is frozen |
| B13 | `refuted` as a defect of the upgrade path; what is left is a test that can fail under load (B3) |

## B1. A thread the jar can't start kills the program, or leaves a jar no owner can ever take

**Verdict: `confirmed`.** Severity `high` stands. The site list is incomplete, and the "Frozen: no" mark is wrong for the sites it missed in `upgrade.rs`.

**Evidence (lines read, at the pin):**

- Every thread in the jar starts with `std::thread::spawn`, which panics when the OS refuses a thread. Search: `grep -rn "thread::spawn\|Builder::new" crates/spyc-jar/src` finds no `thread::Builder` for a thread anywhere in the crate except `client.rs:192` (spyc's side of a connection).
- The acceptor (`crates/spyc-jar/src/jar/mod.rs:417-450`, `accept_loop`) runs on the process's main thread: `src/bin/spyc-jar.rs:5-7` → `cli::main` (`crates/spyc-jar/src/cli.rs:56-99`) → `run_jar` (`cli.rs:104-156`) → `jar::run` → `Ready::serve` (`mod.rs:372-413`) → `accept_loop`, with no `catch_unwind` on the way (the crate's only one is `upgrade.rs:811`, around `take_over`). The release profile unwinds (`Cargo.toml:274`, `panic = "unwind"`), so a panic at `mod.rs:448` leaves `main` and the process exits 101; the PTY master closes and the program is hung up. Every connection, lookups included, takes this spawn before any check (`mod.rs:446-448`), so a tokenless `describe` is enough.
- The owner case: `accept::serve` (`crates/spyc-jar/src/jar/accept.rs:227-233`) sets `st.owner = Some(id)` and `ownerless_since = None`, registers the connection in `conns` (`accept.rs:243`, `register` at `accept.rs:21-40`), drops the lock, then calls `run_conn` (`accept.rs:258`). `run_conn` spawns the writer first (`crates/spyc-jar/src/jar/conn.rs:102-110`); a panic there unwinds the serving thread before the reader loop (`conn.rs:113-172`) and before `release` (`conn.rs:174`, body at `conn.rs:355-375`). Nothing else clears `st.owner`: `release` is its only writer to `None`; the owner's deadline is checked only inside the reader loop (`conn.rs:120`); the reaper (`life.rs:153-197`) looks at `ownerless_since`, which stays `None`. Every later owner is refused with `OWNER_PRESENT` (`accept.rs:201-203`).
- I looked for a way out and found none. An upgrade can't carry the stuck connection: `wait_parked` (`crates/spyc-jar/src/upgrade.rs:595-630`) wants `ConnRead(id)` for every connection that isn't a lookup, and `ConnWrite(id)` while `writer_done` is false (it starts false for an owner, `accept.rs:36`), and neither thread exists, so every upgrade answers busy after `PARK_DEADLINE`. A reader can't signal the program (`conn.rs:256-261`). spyc's side has no answer to reason 2 beyond the attach client's (`crates/spyc-jar/src/attach.rs:299-302`). Only a hand-run `kill` of the jar's pid ends it.
- I did not re-run the Linux probe; the lines above are enough to reach both outcomes it reports.

**What the finding missed.** The same pattern is in the old image's upgrade path, which the brief counts as frozen ("everything a version-1 jar's own image does during an upgrade"):

- `upgrade.rs:114`: `begin` spawns the upgrade's thread from a connection's reader thread, after setting `upgrade_claimed` (`upgrade.rs:103`). A refusal there panics that reader thread: if it is the owner's, the same stuck lease as above, and `upgrade_claimed` stays true, so every later upgrade of that jar is refused with "already under way".
- `upgrade.rs:670, 678, 686`: `validate` spawns three threads. A refusal panics the upgrade thread, which skips the clean-up at `upgrade.rs:128-138`. In the first validation (`upgrade.rs:193-198`) that leaves `upgrade_claimed` set for good: the jar can never be upgraded again, and an upgrade is the only way a later release can fix a running 3.0 jar. In the parked validation (`upgrade.rs:220`), `upgrading` is already true (`upgrade.rs:199`) and is never cleared: every thread stays in `Shared::park` (`mod.rs:177-185`) for good, the pump stops reading the PTY, the program blocks once the terminal's buffer is full, and the owner hears nothing, pings included.
- Not frozen, same pattern: `held.rs:83, 87, 94, 201` (the new image's containment, which a later binary can change), `upgrade.rs:566` (the holder, which is the new binary), and on the client side `attach.rs:193, 222, 250`, `client.rs:458`, `cli.rs:443`.

**Severity.** The trigger is a thread refusal: on Linux, the user's task count at `RLIMIT_NPROC` (which counts threads; some distributions ship a default soft limit of a few thousand, which a large build beside a few agents can reach) or no memory for a stack; on macOS, mostly the latter. That is rare, but the outcome of a passing spike is permanent (a lost program, a jar nobody can own, or a jar parked for good), which fits the owner's bar of reliability under load. I'd keep `high` and add the `frozen` mark for the `upgrade.rs` sites.

**I'd be wrong if** `std::thread::spawn` returned an error instead of panicking on refusal, or the release build aborted on panic (then V9b becomes a process exit, still a lost program). Neither holds at the pin.

## B13. A count line went missing across an upgrade under load; not reproduced

**Verdict: `refuted`** as output loss in the upgrade path. No path in the code loses bytes read from the PTY between the park and the new image's resume, and the one-off has a cause outside the jar that I reproduced: the terminal's echo of the typed `after` lands inside a count line. What survives is a test that can fail under load, which belongs with B3 (`low`, not frozen).

**Evidence (lines read):**

1. **One reader of the PTY in a live jar, and it never parks holding bytes.** The only push of output into the log is `crates/spyc-jar/src/jar/life.rs:100` (search: `grep -rn "Event::Out\|Msg::Output" crates/spyc-jar/src/jar crates/spyc-jar/src/upgrade.rs crates/spyc-jar/src/held.rs`). `pump_pty` (`life.rs:59-110`) checks `upgrading()` only at the top of its loop (`life.rs:64-67`); a read (`life.rs:85`) is always followed by its append under the lock (`life.rs:98-104`) in the same pass. `wait_parked` (`upgrade.rs:595-630`) waits for `Who::Pty` before `try_run` takes `state` (`upgrade.rs:208`), so the pump can't be between a read and its append when the blob is made.
2. **The log crosses whole and is checked.** The blob is encoded under that lock (`upgrade.rs:219`) and the lock is held to the `exec` (`upgrade.rs:277-282`; it is released only on failure, `upgrade.rs:292`). `encode_parts` writes `next_seq`, `head`, `base` and every event with its sequence number (`crates/spyc-jar/src/handover.rs:96-118`); `Reader::log` refuses a log whose outputs don't tile it from base to head (`handover.rs:252-304`). Each connection's cursor crosses as `(seq, from)` (`handover.rs:162-178`, checked at `handover.rs:390-398`).
3. **A writer's cursor is what it sent.** A writer parks only at the top of `next_batch` (`conn.rs:473-478`), after `send_all` wrote a whole batch and the cursor moved (`conn.rs:402-415`). The outbox carries replies and notices, never output (`outbox.rs`; output frames come only from `collect`, `conn.rs:525-568`), and `flush_all` writes or puts back whole outboxes (`upgrade.rs:339-396`).
4. **Bytes not yet read wait in the kernel.** The master crosses as a descriptor (`upgrade.rs:238-240`) and the new image's pump reads it from where the old one stopped (`mod.rs:390-393` after `take_over`, `upgrade.rs:836-870`). Nothing flushes a terminal's queue in the jar: the only `tcflush` in the crate is the attach client's on its own standard input (`attach.rs:75`). The master is close-on-exec (`pty.rs:30-31`) and its copies use `try_clone` (`mod.rs:325-326`), so no child holds a reader of it.
5. **The one path that discards output can't produce this failure.** `held::drain` (`held.rs:106-125`) drops what the program prints, but only after a resume failed, and a held jar never sends `upgraded` (`held.rs:1-14`); the test would have failed at "no Upgraded" (`tests/jar_process.rs:774-778`), not at `assert_unbroken`.
6. **The failure's shape matches the echo.** The test types `after\n` into the program once, just after `upgraded` (`tests/jar_process.rs:801-805`), with the terminal's echo on and the counter writing to the same terminal (`tests/jar_process.rs:712-718`). On Linux, the line discipline's echo of input takes the terminal's output lock between the two parts of a writer's `n=34\n` (the plain characters, then the newline translated to `\r\n`), so the stream can read `n=34after\r\n\r\n`. `counts` (`tests/jar_process.rs:721-726`) doesn't parse `n=34after`, and the counts go 33, 35: `left: 35, right: 34`, the exact assertion in `stress48-jp-24.log`, with offsets unbroken, which is what B13 observed.

**Evidence (command and output, Linux).** `docs/drafts/review-3.0/evidence/refute-B/echo/echo_interleave.py` runs the test's own counter program on a PTY with Python's `pty`, no jar involved, types `after\n` every 1 to 10 ms for 20 s, and counts broken counts and count lines with something glued on. Run as `podman run --rm --network none -v …/refute-B/echo:/w:ro localhost/spyc-review-linux sh -c '[14 busy loops &] python3 /w/echo_interleave.py MODE 20'`:

```
echo   no load:  typed=3277 counts=3542 breaks=0 glued_lines=0 examples=[]
echo   loaded:   typed=3521 counts=3081 breaks=8 glued_lines=8 examples=['n=66after', 'n=150after', 'n=167after']
noecho loaded:   typed=3503 counts=3276 breaks=0 glued_lines=0 examples=[]
```

Every break is a glued line, and `stty -echo` removes them all. That is about 0.2% per typed line under this load. The test types one line a run, so a 1-in-31 failure is plausible on a bad run, and B13's 0 in about 3,400 diagnostic rounds (which also type one `after` after `upgraded`) is less likely at my rate but depends on how the load lands on the scheduler; neither number pins the rate down.

**Consequence.** No output is lost across an upgrade by anything I could find in `jar/`, `upgrade.rs` or `handover.rs`; the frozen mark should come off. The test is fragile under load for a reason its own program sets up; B13's direction (`stty -echo` in the counter, and print the text around a break) is the fix for that, and it belongs with B3.

**I'd be wrong if** the failing run's text showed `n=34` missing with no `after` echo next to it. The test doesn't print its text, so that one run can't be checked; a rerun of the test with `stty -echo` that still breaks a count under load would reopen B13.
