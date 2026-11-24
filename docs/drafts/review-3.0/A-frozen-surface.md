# Unit A: what can't be fixed later (spyc 3.0 pre-release review)

**SHA:** `444857a2` (`main`, unchanged through the review).
**Platforms:** macOS 26 (Apple silicon) for reading, every probe and both fuzz targets; Linux in the review's Podman VM (Alpine, **aarch64 musl**, not CI's x86_64 glibc) for the exec-boundary and failed-`exec` probes, run against the debug `spyc-jar` and the jar built to differ that the Linux gate had built in the `spyc-review-home` volume. Probes are Python clients of the frozen wire in `docs/drafts/review-3.0/evidence/A/`; every probe jar ran under scratch `XDG_*`, `HOME`, `TMPDIR` and with `SPYC_*` and `SSH_AUTH_SOCK` unset, its socket in its own `0700` directory `/tmp/uaA-<probe>-*` (a socket path under the scratch directory is over macOS's 104-byte limit), and each probe ended the jar's session and program group it started and removed its directory.

## Verdict

Version 1's bytes are what the specification says: all 33 frame fixtures and the 487-byte sample blob in `JAR_PROTOCOL.md` are identical to `crates/spyc-jar/tests/wire_v1.rs`, and an independent decoder written from the prose alone consumed the blob to its last byte with every field where the prose puts it. The decoders bound every allocation by what has already arrived (or by `MAX_FRAME`/`MAX_HELLO` before reading) and both fuzz targets ran clean. The old image's upgrade, which is what freezes for good, is mostly sound: two upgrades in a row (stock to differ, then from that image to another) kept the pid, program, connection and lock on both platforms, and the new image's containment holds a program through a blob that never came and a panic at start. What I would not ship frozen as it stands: (A1) after any failed `exec` the old image runs with `SIGPIPE` at its default for the rest of its life, and on Linux a later upgrade whose holder closes early then kills the jar and its program, which I reproduced; (A2) the old image answers "rejected" (reason 6) where the protocol promises "busy" (reason 5) for running out of time, and also for transient `fork`/disk errors, and spyc stops all upgrades for the session on a 6; (A3) the copy step will open a FIFO or a device and wedge the jar's upgrade claim for life. None corrupts a user's work in ordinary use (each needs a failed upgrade or a hand-named path), so there is no STOP item. Outside the frozen surface, the new image still exits, losing the program, if it can't receive its descriptors (A5), and `tests/jar_compat.rs` cannot start any jar this crate builds, so the "every released jar stays a fixture" promise has no working harness for 3.0.0's jar (A8).

## Coverage ledger

| File | Status |
|---|---|
| `crates/spyc-jar/src/proto.rs` | read in full |
| `crates/spyc-jar/src/handover.rs` | read in full |
| `crates/spyc-jar/src/upgrade.rs` | read in full |
| `crates/spyc-jar/src/store.rs` | read in full |
| `crates/spyc-jar/src/held.rs` | read in full |
| `crates/spyc-jar/src/lock.rs` | read in full |
| `crates/spyc-jar/src/cli.rs` | read in full |
| `crates/spyc-jar/src/limits.rs` | read in full |
| `crates/spyc-jar/src/state.rs` | read in full |
| `crates/spyc-jar/src/lib.rs` | read in full |
| `crates/spyc-jar/tests/wire_v1.rs` | read in full; fixtures diffed against the protocol by script |
| `crates/spyc-jar/tests/fuzz_seeds.rs` | read in full |
| `tests/jar_differ.rs` | read in full |
| `tests/jar_compat.rs` | read in full; run against the pin's jar (A8) |
| `tests/jar_process/upgrade.rs` | read in full |
| `fuzz/fuzz_targets/jar_frame.rs`, `jar_handover.rs` | read in full; run (see A12) |
| `docs/drafts/JAR_UPGRADE_BRIEF.md` | read in full |
| `docs/drafts/JAR_PROTOCOL.md`, `JAR_PRODUCTION_PLAN.md` | read in full (Sharing, Keep-alive, Fuzzing, Who may connect sections skimmed) |
| `crates/spyc-jar/src/jar/conn.rs` | lines 100-268 read (the reader loop, `act`) |
| `crates/spyc-jar/src/jar/accept.rs` | lines 118-240 read (`no_sigpipe`, `serve`) |
| `crates/spyc-jar/src/jar/describe.rs` | read in full |
| `crates/spyc-jar/src/jar/mod.rs`, `life.rs` | the resume (`prepare`, `serve`, 340-400) and the child waiter (100-160) only |
| `src/bin/spyc-jar.rs` | not read (the entry point is `cli::main_built`, per `cli.rs:47-53`) |
| `tests/jar_process.rs` | lines 1650-1720 (the failed-`exec` test) and the uses of `/nonexistent/spyc-jar` only |
| `src/pane/jar/mod.rs`, `src/app/{jar_upgrades,pane_starts,capture_starts,jarless}.rs`, `src/app/pager_handler/mod.rs` | the parts that consume an upgrade's reason and that start a program with or without a jar (V4); not reviewed beyond that |
| Rust std 1.100 `library/std/src/sys/process/unix/unix.rs`, `os/unix/net/stream.rs` | the `exec` and `write` paths, for A1 |

## Findings

### A1. After a failed upgrade \`exec\`, a later upgrade can kill the jar with \`SIGPIPE\` on Linux

- **Severity:** medium. **Frozen:** yes (old image, parts 1 to 9). **Lens:** reliability.
- **Sites:** \`crates/spyc-jar/src/upgrade.rs:277-293\` (the \`exec\` and its failure path, which restores standard input and the lock's close-on-exec but not \`SIGPIPE\`); \`crates/spyc-jar/src/upgrade.rs:445-450\` and \`:250\` (\`rustix::net::sendmsg\` with \`SendFlags::empty()\`, no \`MSG_NOSIGNAL\`); \`crates/spyc-jar/src/jar/accept.rs:129-131\` (\`no_sigpipe\` is a no-op off macOS, on the premise "Linux's std writes to a socket with \`MSG_NOSIGNAL\` already", which holds for std's \`write\` and not for \`rustix::net::sendmsg\`); Rust std 1.100 \`library/std/src/sys/process/unix/unix.rs:360-373\` (\`do_exec\`, which \`Command::exec\` runs in the calling process, sets \`SIGPIPE\` to \`SIG_DFL\` before \`execvp\`).
- **Claim:** a failed \`exec\` leaves the old image with \`SIGPIPE\` at its default for life, and on Linux the holder handshake's \`sendmsg\` then raises \`SIGPIPE\` when the holder has already closed its end, ending the jar and, with its PTY master, the program.
- **Evidence (command + output).** \`docs/drafts/review-3.0/evidence/A/probe_linux.py\`, log \`docs/drafts/review-3.0/evidence/A/probe_linux.log\`, run in the \`localhost/spyc-review-linux\` container: \`SigIgn: 0000000000001000\` before; an upgrade to a copy that deletes itself in \`hold-handover\` (so the \`exec\` gets \`ENOENT\`) answers reason 7; \`SigIgn: 0000000000000000\` after. Then upgrades to a binary whose \`hold-handover\` is \`exec 0<&-; exit 0\`: "JAR DIED after 5 tries; status -13". On macOS (\`docs/drafts/review-3.0/evidence/A/probe_failedexec.py\`, \`docs/drafts/review-3.0/evidence/A/probe_lateproof.py\`, read with \`docs/drafts/review-3.0/evidence/A/sigs\`, a small \`sysctl(KERN_PROC_PID)\` reader of \`p_sigignore\`): the ignore bit for \`SIGPIPE\` goes from 1 to 0 across the failed \`exec\` too, but the jar survived 20 early-closing holders, because \`no_sigpipe\` sets \`SO_NOSIGPIPE\` on that socket there. The in-repo guard for review finding 11, \`a_failed_exec_leaves_a_jar_that_survives_the_next_refusal\` (\`tests/jar_process.rs:1666\`), covers a validator that refuses (std's \`write\`, which is safe), not a holder that closes.
- **Consequence:** rare (it needs one failed \`exec\`, then a new binary whose holder exits before reading its first byte), but when it happens a Linux user loses the agent in that tab outright, and once released the jar can't be fixed.
- **Status:** confirmed (Linux aarch64 musl; x86_64 glibc not run, same code path).
- **Fix direction:** send with \`MSG_NOSIGNAL\` on Linux at both \`sendmsg\` calls and set \`SIGPIPE\` back to ignored after a failed \`exec\`.

### A2. The old image answers "rejected" (6) where the protocol promises "busy" (5), and spyc then stops every upgrade for the session

- **Severity:** medium. **Frozen:** yes. **Lens:** correctness (a clock decides).
- **Sites:** \`crates/spyc-jar/src/upgrade.rs:726-733\` (the proof line is awaited with \`recv_timeout(left())\`; when the deadline has passed it reads nothing and answers \`UPGRADE_REJECTED\`); \`:652-660\` (any \`spawn\` error, including \`EAGAIN\` at the process limit and \`EMFILE\`, is \`rejected\`); \`:159-169\` with \`crates/spyc-jar/src/store.rs:111-151\` (any copy error, including \`ENOSPC\`, is \`rejected\`). Against \`docs/drafts/JAR_PROTOCOL.md:696-699\`: "Running out of time is not [a refusal]: the upgrade is busy (reason 5), since a loaded machine defers an upgrade and never fails a good binary". Consumer: \`src/pane/jar/mod.rs:298-305\` maps 6 to \`Refused\`, and \`src/app/jar_upgrades.rs:206-219\` then sets \`stopped\` and flashes "jar upgrade stopped: …; the other jars run on their build".
- **Claim:** the only branch of \`validate\` that can expire without becoming reason 5 is the wait for the proof, and it becomes reason 6; two transient OS failures also become 6.
- **Evidence (command + output, then lines read).** \`docs/drafts/review-3.0/evidence/A/probe_lateproof.py\`: a validator that runs the real validator, exits 0 at once and lets a child print the real proof line 40 s later: \`('refused', 6, '… exited without proving it took the handover (it printed "")', …), 30.03\` s. The load case is by reading: the validator prints its line and exits; the jar's \`try_wait\` loop (5 ms polls, \`:694-713\`) sees the exit; the line reaches \`line_rx\` only once the reader thread at \`:677-683\` has been scheduled; at the edge of the deadline \`left()\` is zero and \`recv_timeout(0)\` returns empty. Nothing in the branch distinguishes "no line" from "no line yet".
- **Consequence:** on a loaded machine, or one at its process or descriptor limit, a good build is reported as a bad one, spyc stops upgrading every jar for the session and shows an error, and the next start tries again; the jars keep running.
- **Status:** confirmed (the expiry branch by run; the load race by reading).
- **Fix direction:** an expired wait for the proof, and \`spawn\` or copy errors other than "missing", "not executable" and "not permitted", answer reason 5.

### A3. The copy step takes any path: a FIFO wedges the jar's upgrades for life, a device or huge file fills the disk, and rejected copies stay

- **Severity:** low. **Frozen:** yes (part 1). **Lens:** reliability.
- **Sites:** \`crates/spyc-jar/src/store.rs:111-151\` (\`File::open(src)\` and \`io::copy\` with no check that \`src\` is a regular file and no size bound; nothing removes the entry if the upgrade later fails); \`crates/spyc-jar/src/upgrade.rs:103\` and \`:113-139\` (the upgrade claim is held across the copy, so a copy that never ends answers every later \`upgrade\` with reason 5).
- **Claim:** an \`upgrade\` naming a FIFO blocks in \`open\` for as long as nothing writes to it, and the jar refuses every upgrade meanwhile; \`/dev/zero\` or a very large file is copied until the disk fills; each rejected copy is kept.
- **Evidence (command + output; the device case by reading).** \`docs/drafts/review-3.0/evidence/A/probe_failedexec.py\` / \`docs/drafts/review-3.0/evidence/A/probe_failedexec.log\`: after naming a FIFO, no answer in 2 s; a real upgrade then answers \`(5, 'an upgrade is already under way')\`, and again 40 s later; once the probe wrote to the FIFO the copy finished and was refused (6). \`docs/drafts/review-3.0/evidence/A/probe_boundary.log\`'s \`jar-bin\` listing keeps the copy of a refused downgrade.
- **Consequence:** only reachable by a token holder naming a path by hand (\`spyc-jar upgrade\`; spyc always names a store entry). A wrong path typed there leaves that jar unable to upgrade until its program ends, and spyc's later attempts end "busy" three times and stop.
- **Status:** confirmed (FIFO, leftovers); unverified for the disk-filling case (not run, to keep the owner's disk safe).
- **Fix direction:** open without blocking, refuse anything but a regular file, bound its size, and remove the copy when the upgrade fails.

### A4. A reader holding the token can make the jar run any file, with the jar's whole state on its standard input (lead V10)

- **Severity:** informational (addressed to the owner). **Frozen:** yes. **Lens:** security.
- **Sites:** \`crates/spyc-jar/src/jar/conn.rs:157-160\` (a reader may send everything but \`Detach\`) and \`:221\` (\`Upgrade\` goes to \`upgrade::begin\` for any role that got there); \`crates/spyc-jar/src/upgrade.rs:159-169\`, \`:193-198\`, \`:652-673\` (the named file is copied and run as \`validate-handover\` with the blob on standard input before anything parks); the blob carries the token (\`crates/spyc-jar/src/handover.rs:148\`), the log, the snapshots and the owner's pairs; \`docs/drafts/JAR_PROTOCOL.md:674\` freezes "The owner may ask, and so may a reader holding the token".
- **Claim:** confirmed as the brief states it.
- **Evidence (command + output).** \`docs/drafts/review-3.0/evidence/A/probe_failedexec.log\`: a role-2 connection asked for an upgrade to a script whose \`validate-handover\` wrote \`id -un\` to a file; the file holds the user's name.
- **Consequence:** inside the stated model this is by design: owner and reader present one token (R26), and an owner can already type commands into the program, so the reader gains nothing. What it fixes for good: no party meant to hold less than the owner (U6's watcher that can never type, U15's read-only share, any later role) can ever be given a version-1 jar's token. Such a party has to go through a gateway that holds the token itself, as "Sharing" plans, or every version-1 jar has to be upgraded to a version that knows roles before one is admitted. Nothing in the documents says this.
- **Status:** confirmed.
- **Fix direction:** decide before the tag: let only the owner ask for an upgrade in version 1 (a reader's `upgrade` refused with reason 4), or keep it and write the constraint beside U6 and "Sharing".

### A5. A new image that can't receive its descriptors exits, and the program goes with it

- **Severity:** medium. **Frozen:** no (new-image code; it is 3.0's own resume that every rc jar upgrades into). **Lens:** reliability.
- **Sites:** \`crates/spyc-jar/src/upgrade.rs:761-797\` (\`try_clone_to_owned()?\`, \`recvmsg(…)?\`, \`read_exact(…)?\`, \`bail!("no handover on standard input")\` all return from \`resume\` before the \`catch_unwind\` at \`:808-813\`).
- **Claim:** the containment ("hold what crossed") covers decoding and starting, not receiving; any error before it ends the process, closing the PTY master, which the holder doesn't hold.
- **Evidence (command + output).** \`docs/drafts/review-3.0/evidence/A/probe_resume.py\` / \`.log\`: a wrapper that starts the real new image with standard input on \`/dev/null\`: the owner's connection reads end-of-file with no \`refused\`, the jar and its program are gone, the socket refuses connections; the jar's standard error says "Socket operation on non-socket". The same probe with \`SPYC_JAR_RESUME_FAULT=blob\` and \`=panic\` shows the held mode working: reason 7, jar and program alive, \`held\` in the description.
- **Consequence:** an upgrade whose new image fails this early loses the program and tells the owner nothing; the protocol's "contain" paragraph reads as if it covered every failure after the \`exec\`.
- **Status:** confirmed.
- **Fix direction:** fall into the held mode with whatever descriptors did arrive (the master among them) on every failure after the \`exec\`.

### A6. The new image's resume has no deadline (lead H5)

- **Severity:** low. **Frozen:** no (new-image code). **Lens:** reliability.
- **Sites:** \`crates/spyc-jar/src/upgrade.rs:798-802\` (blocking read of the blob from the holder's pipe), \`:824-832\` (\`end_holder\`: \`SIGTERM\`, then a blocking \`waitpid\`), \`:566-576\` (the holder writes the blob from a thread and lives up to \`HOLD_BACKSTOP\`, 600 s); \`crates/spyc-jar/src/limits.rs:14-23\` (\`PARKED_AT_MOST\` stops at the holder's answer, so neither the \`exec\` nor the resume is in the bound spyc's waits are checked against).
- **Claim:** between the \`exec\` and serving, the new image waits on the holder with nothing but the holder to end the wait; a holder that is stopped never ends it, one that ignores \`SIGTERM\` ends it only at its 600 s backstop.
- **Evidence (command + output).** \`docs/drafts/review-3.0/evidence/A/probe_h5.py\` / \`.log\`: a holder whose shell traps \`SIGTERM\`: the upgrade got no answer in 10 s, the program didn't echo in a further 20 s, a lookup timed out; killing the holder by hand let the new image resume, and the program and a new reader carried on.
- **Consequence:** the real holder dies on \`SIGTERM\`, so this needs a holder stopped or wedged by something else; when it happens the program's PTY goes undrained (it blocks once the buffer fills) and spyc's owner connection reaches its 20 s write stall.
- **Status:** confirmed (by a misbehaving holder; a \`SIGSTOP\`ped one by reasoning: \`waitpid\` without \`WUNTRACED\` doesn't return for a stop).
- **Fix direction:** \`SIGKILL\` the holder once the blob is in (it holds nothing the new image needs by then) and bound the blob read by a poll on the pipe.

### A7. After an upgrade, the lock's descriptor is inherited by every child the jar starts

- **Severity:** informational. **Frozen:** no (documented at \`crates/spyc-jar/src/lock.rs:109-111\`). **Lens:** maintainability.
- **Sites:** \`crates/spyc-jar/src/lock.rs:64-67\`, \`:109-122\`; \`crates/spyc-jar/src/upgrade.rs:270\`.
- **Claim / evidence (command + output):** \`docs/drafts/review-3.0/evidence/A/probe_boundary.log\` (macOS \`lsof\`) and \`docs/drafts/review-3.0/evidence/A/probe_linux.log\` (\`/proc/PID/fdinfo\`): in a first image the lock's descriptor is close-on-exec (Linux \`flags=02400002\`) and validators and holders inherit only 0 to 2; after an upgrade it is not (\`flags=0400002\`), and every validator and holder of that image, and of later ones, has it as descriptor 3. Everything else that crossed is close-on-exec in the new image (\`resume\` sets it, \`:793-795\`); the new image's standard input stays the spent socketpair for life. Signal masks were empty throughout, and \`SIGPIPE\` stayed ignored across successful upgrades. A second upgrade, from an image that arrived by upgrade, kept the pid, program, connection and lock on both platforms.
- **Consequence:** none for the lock (\`fcntl\` locks aren't inherited); a holder keeps one more descriptor open while it lives.
- **Status:** confirmed.

### A8. \`tests/jar_compat.rs\` can't start any jar this crate builds, so no released jar can become the fixture the plan promises

- **Severity:** medium. **Frozen:** affects how the frozen surface is held to account. **Lens:** maintainability (tests).
- **Sites:** \`tests/jar_compat.rs:52-61\` (starts the old jar with \`--socket\` and \`-- PROGRAM\` on the command line and standard input on \`/dev/null\`), \`:66-71\` (polls for the socket with no deadline), \`:117\` (expects an old jar to answer \`upgrade\` with \`unsupported\`, which no version-1 jar does); \`crates/spyc-jar/src/cli.rs:215\` and \`:371-384\` (a version-1 jar refuses \`--\` and refuses to run without its start on standard input). Promises: \`docs/drafts/JAR_PRODUCTION_PLAN.md:164\`, \`docs/drafts/JAR_PROTOCOL.md:1052\`, \`docs/drafts/JAR_UPGRADE_BRIEF.md\` §5. No workflow or \`Makefile\` line sets \`SPYC_JAR_OLD_BIN\` (\`grep -rn SPYC_JAR_OLD_BIN .github Makefile\`: nothing).
- **Claim:** the compatibility test drives only the spike's command line; pointed at the pin's own jar it hangs.
- **Evidence (command + output).** \`docs/drafts/review-3.0/evidence/A/run_compat.py\` ran the built \`jar_compat\` test binary with \`--ignored\` and \`SPYC_JAR_OLD_BIN\` set to the pin's \`spyc-jar\`: "TIMED OUT after 30 s: killed" (the jar exits at once on \`unknown argument --socket\`; the test loops on \`connect\` for ever).
- **Consequence:** when 3.0.0 ships, its jar can't be kept as a fixture with this test, so the first spyc that changes the client side has nothing checking it against jars users are running.
- **Status:** confirmed.
- **Fix direction:** rewrite the test against the frozen start (\`--wire 1\` and \`cli::Start\` on standard input) with a deadline on every wait, and add the fixture step to CI before the tag.

### A9. A validator's children outlive every outcome but a timeout, and can hold a copy of the blob in the jar for life

- **Severity:** low. **Frozen:** yes. **Lens:** reliability (resources).
- **Sites:** \`crates/spyc-jar/src/upgrade.rs:704-711\` (the group is killed only on timeout), \`:714-734\` (success and non-zero exit return without it), \`:669-673\` (a detached thread writes the whole blob to the validator's standard input).
- **Claim / evidence (lines read):** a validator that exits while a child keeps its standard input open without reading leaves the writer thread blocked, holding a copy of the blob (the log, up to the ring's 4 MiB, plus both snapshots), for as long as that child lives; the child itself is never ended. \`a_validators_child_holding_its_stderr_cannot_hold_the_jar\` (\`tests/jar_process/upgrade.rs:199-219\`) checks the answer's timing, not the child.
- **Consequence:** only a misbehaving binary causes it; each attempt can leave a thread and several MB in a process meant to run for months.
- **Status:** unverified (reasoning).
- **Fix direction:** end the validator's process group on every outcome.

### A10. \`lock::keep\` makes the lock file before locking it, so a client can read a live jar as gone

- **Severity:** low. **Frozen:** no. **Lens:** correctness.
- **Sites:** \`crates/spyc-jar/src/lock.rs:101-107\` and \`:74-99\` (open with \`create(true)\`, then \`fcntl\` lock); \`:53-62\` (a client that takes a shared lock reads \`Free\`).
- **Claim / evidence (lines read):** when a cleaner has deleted the lock file, \`keep\` creates a new, unlocked file at the path and locks it a moment later; a client's \`state\` in that moment answers \`Free\`, which R32b says means "the jar is gone", the case that led spyc to delete a live jar's socket and token. The same 1 s \`READER_GRACE\` decides whether a starting jar treats a slow reader as another jar (\`:84-95\`).
- **Status:** unverified (a race of microseconds; not reproduced).
- **Fix direction:** make and lock the file under a private name, then rename it into place (the lock follows the inode).

### A11. Three texts contradict jar-or-nothing; the code doesn't (lead V4)

- **Severity:** low. **Frozen:** two of the three texts are in the frozen specification. **Lens:** docs.
- **Sites:** \`docs/drafts/JAR_PROTOCOL.md:62-63\` ("if the jar machinery fails, spyc hosts the pane itself … rather than prompting"), \`docs/drafts/JAR_PROTOCOL.md:759-761\` ("spyc runs the program without a jar rather than fail the tab"), \`crates/spyc-jar/src/cli.rs:15-19\` ("… and run the program another way"). Against \`docs/drafts/JAR_PRODUCTION_PLAN.md:455-466\` (decision 6) and SPYC-TRAP \`jar-or-nothing\` (\`src/pane/jar/mod.rs:425-427\`, \`ARCHITECTURE.md:1044\`).
- **Evidence (lines read):** every start without a jar I found is either jars off, a build that doesn't honour them, or the user's choice: \`src/app/pane_starts.rs:126\` (tabs: \`!(jars && honoured()) || jarless.chosen\`), \`src/app/capture_starts.rs:41-47\` (captures, the same test), \`src/pane/jar/mod.rs:680-686\` (exit 75 becomes \`NoUsableJar\`, which offers the choice, \`src/app/jarless.rs:56-67\`), \`src/app/pane_starts.rs:73-76\` (a retake with no spec is an error, not a plain start), \`src/app/jarless.rs:137-144\` (a restored tab whose jar may still run is never started without one). The search: \`grep -rn 'Pane::launch(\|launch_tab_pane(\|spawn_capture(' src\`. One start always runs without a jar and isn't a tab: the top overlay (\`src/app/pager_handler/mod.rs:339-360\`, used by \`src/app/commands.rs:48\`), whose program dies with spyc; whether decision 6 means it is for Units C and G.
- **Status:** confirmed (texts wrong, code right, for the paths listed).
- **Fix direction:** correct the three sentences; for the frozen document, mark them as superseded by decision 6.

### A12. The fuzz targets decode only; neither reaches the start, the proof line or a byte-for-byte round trip

- **Severity:** informational. **Frozen:** no. **Lens:** maintainability (tests).
- **Sites:** \`fuzz/fuzz_targets/jar_frame.rs\` (calls \`decode\` and \`read_msg\`, never \`encode\`); \`fuzz/fuzz_targets/jar_handover.rs\` (\`decode\`, then \`encode\`, then asserts only that the result decodes again).
- **Evidence (command + output):** macOS, `make fuzz` with `FUZZ_TOOLCHAIN=nightly-2026-11-10` (the default `nightly` on this machine is 1.98 and can't build the crate, which wants 1.100), 960 s each, one after the other in my worktree: `jar_frame` "Done 50055813 runs in 961 second(s)", coverage 554, no crash (`docs/drafts/review-3.0/evidence/A/fuzz2-jar_frame.log`); `jar_handover` "Done 71384166 runs in 961 second(s)", coverage 1005, no crash and no failed round trip (`docs/drafts/review-3.0/evidence/A/fuzz3-jar_handover.log`). Two `make fuzz` runs at once in one target directory rebuilt `spyc` in turn without either starting (an earlier attempt, stopped), worth knowing for whoever runs several targets.
- **What isn't reached:** \`proto::encode\` (and so any frame whose encoding doesn't read back as itself; only the 30 hand-made messages of \`proto.rs:665-755\` check that); \`cli::Start::parse\`, the start on standard input; the validator's proof line (\`upgrade.rs:739-744\`); \`resume\`'s 12-byte head. Every length-prefixed field I read is bounded before it allocates: \`proto::read_msg_max\` checks \`len\` against \`MAX_FRAME\` (16 MiB) or \`MAX_HELLO\` (64 KiB) before \`vec![0; len]\` (\`proto.rs:483-495\`), and that zeroed buffer is lazily mapped, so a peer that sends only a length costs address space, not memory; \`Cursor\` and \`handover::Reader\` check every count and length against what is left (\`proto.rs:579-585\`, \`:620-649\`; \`handover.rs:477-494\`), so a forged count never allocates; \`read_blob\` allocates up to \`MAX_BLOB\` (1 GiB) from a length the jar itself wrote (\`upgrade.rs:582-592\`); \`Start\` is capped at 1 MiB (\`cli.rs:235\`, \`:375-382\`).
- **Status:** confirmed.

### A13. Smaller documentation faults in the frozen texts

- **Severity:** informational. **Frozen:** the first two are in the frozen specification. **Lens:** docs.
- \`docs/drafts/JAR_PROTOCOL.md:736\`: the blob's snapshots are "each as in \`store_snapshot\`", but in the blob each snapshot's bytes carry a \`u32\` length (\`handover.rs:130\`), where the frame's run to its end; the fixture settles it, the prose doesn't.
- \`docs/drafts/JAR_PROTOCOL.md:966\` and \`crates/spyc-jar/src/limits.rs:19-23\`: \`PARKED_AT_MOST\` adds 1 s for the validator's output after its 5 s, but the code waits for that output inside the 5 s (\`upgrade.rs:717-726\`); the bound is 1 s generous, which is harmless, and leaves out the \`exec\` and the resume (A6).
- \`docs/drafts/JAR_UPGRADE_BRIEF.md\`: status "built", but its line numbers and \`HOLD_LIMIT\` (30 s, now \`HOLD_BACKSTOP\`, 600 s) are \`be018db6\`'s, and §5 is open: the \`=\` pin (\`Cargo.toml:139\` is still a caret requirement), a real upgrade among the property sequences (\`tests/jar_process.rs:2422\` still names \`/nonexistent/spyc-jar\`), and released jars as CI fixtures (A8).
- **Status:** confirmed.

## Seeded leads

- **V10 (owner): confirmed, and by design inside the stated model.** A reader with the token may upgrade (\`jar/conn.rs:157-160\`, \`:221\`), and the jar copies and runs any absolute path with the blob, token included, on standard input; shown by probe (A4). Owner and reader share one token, so it grants nothing an owner lacks. What it fixes for good: no party meant to hold less than the owner may ever hold a version-1 jar's token (U6, U15, later roles), which the documents don't say.
- **V4 (shared with G): partly.** The three sentences exist as the brief quotes them (\`JAR_PROTOCOL.md:62-63\`, \`:759-761\`, \`cli.rs:15-19\`), two inside the frozen specification. The code doesn't fall back unasked on any path I traced: tabs, captures, retake, restore and exit 75 all fail the tab or offer the user's choice (A11). The one program spyc always runs without a jar is the top overlay, which isn't a tab (left to C and G).
- **H5 (owner): confirmed, not frozen.** \`resume\` reads its descriptors and the blob with blocking reads and \`end_holder\` waits with a blocking \`waitpid\`; nothing but the holder bounds it: 600 s for a holder that ignores \`SIGTERM\`, nothing for a stopped one. Shown by probe (A6). It is new-image code, so a later release can fix it; it is also not counted in \`PARKED_AT_MOST\`. The earlier gap is worse: a new image that can't receive its descriptors exits and loses the program (A5).
- **V11, the validate part (owner): confirmed.** The success path waits only what is left of the deadline for the proof line and answers reason 6 when it isn't there, where the protocol says running out of time is reason 5; shown by probe at 30.03 s, and spyc then stops upgrades for the session (A2). Every other clock in the old image's upgrade (\`PARK_DEADLINE\`, \`FLUSH_DEADLINE\` for the owner, \`FIRST_VALIDATION\`, \`PARKED_VALIDATION\`, \`HOLDER_ANSWER\`) concludes "busy", which is the right conclusion; \`FLUSH_DEADLINE\` closes a reader that doesn't take its last frames in 1 s, a verdict a slow reader can't appeal (its next connection replays). Two more clocks in my files decide something: \`READER_GRACE\` (1 s) decides whether a starting jar takes a client reading its lock for another jar (\`lock.rs:69-95\`), and a validator's stderr is read for at most \`OUTPUT_GRACE\`, which only shortens the refusal's text.

## Manual checklist items

- **M-A1. A failed upgrade on the owner's Linux machine, if he has one.** Not needed on macOS (the probe shows macOS survives). On Linux: \`python3 -I docs/drafts/review-3.0/evidence/A/probe_linux.py\` with \`UA_S\`, \`UA_JAR\` and \`UA_DIFFER\` pointing at a scratch directory and the two binaries, as in the command in A1. Expect "JAR DIED … status -13" within 40 tries; it settles A1 on x86_64 glibc.
- No other item of this unit needs a terminal emulator, SSH, sleep or battery: every probe here drives the jar's socket directly.

## Not covered

- \`src/bin/spyc-jar.rs\` itself (only its call into \`cli::main_built\`), and most of \`crates/spyc-jar/src/jar/\` (Unit B's), beyond \`act\`, the hello and the resume.
- The store as spyc writes it (\`src/pane/jar/store.rs\`, the sweep): my probes ran jars outside a store, so \`build\` was empty and copies went beside the socket; the store-entry path of \`one_file\` was covered only by \`store.rs\`'s unit tests.
- A1 on x86_64 glibc; the exec-boundary probes on Linux used aarch64 musl.
- The disk-filling case of A3 (not run on purpose).
- A Rust red test for A1 and A5: the evidence is the Python probes; I left no code in my worktree beyond the fuzz corpus the \`make fuzz\` runs grew.
- The property tests (\`tests/jar_process.rs:2360\` onwards) and \`tests/jar_process/{contain,lock,ownership,timers}.rs\` were not read.
