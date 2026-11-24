# Addendum to the pre-3.0 review

**SHA:** `origin/main` at `52ce15b7` (#1317 landed after the review's pin, `444857a2`). `git diff --stat 444857a2 52ce15b7 -- crates src/pane` is empty, so the jar and spyc's jar code are the pin's; #1317 changed `src/terminal.rs`, `src/app/{proc,run,runtime,message}.rs` and two tests.

**What this is:** the probes the remediation brief's section 1 asked for before any fix (P1, P2, P3), run without the owner's hands. Each settles a finding or a lead the review left open. The cross-version unit the brief's section 4 asked for is its own report, [`H-cross-version.md`](H-cross-version.md). Evidence: `evidence/P1-P2/`, `evidence/P3/`, `evidence/H/`.

**Platforms:** Linux arm64 (Alpine, musl) in the review's Podman VM for P1 and P3-9; x86_64 glibc under qemu-user emulation, with a cross-compiled jar, for P3-6; macOS on Apple silicon for P1's control.

## What changes in the review

| Entry | Settles | Result | Change to the review |
|---|---|---|---|
| P1 | B12 into C2 | Confirmed on Linux. Under `stress-ng`, a 1 to 3 s stop and continue of spyc left 5 tabs `[detached]` in 4 of 50 rounds, and `^a R` on one started the program a second time | Part of **C2, high**. B12 is the way in, so its fix belongs with slice B |
| P2 | #1317 against B12, C2 and Unit D | Fixes none of them. It makes C2 easier to reach, since a stopped spyc now takes keys again after `fg` | Informational |
| P3-6 | A1 on x86_64 glibc | Confirmed once the race is forced: status -13 on the first try, 3 of 3 rounds | A1 unchanged (medium, frozen). Slice C's red test must force the holder's exit before the send |
| P3-9 | E2 across users | Confirmed: another user's `/tmp/spyc-<uid>` gets the session token and every typed key, and `spyc attach` `SIGTERM`s a pid that user names | **E2 raised to high** (referee, below). A new low joins D2/D6 |
| CI run 45232716535 | B12 on CI's own platform | On 2026-11-23, `a_client_stopped_past_the_deadline_attaches_again` (`tests/jar_attach.rs:290`) failed on x86_64 glibc: the attach client exited with `Interrupted system call (os error 4)` from `Client::recv` (`crates/spyc-jar/src/client.rs:130`) | B12 is not a lab result: it already fails CI now and then |
| H1 | rollback to 2.4.0 | A 3.0 session restored by 2.4.0 starts every jarred program a second time | **New high**, in `H-cross-version.md`. Needs to be fixed before rc.1 |

### Severity, the referee's calls

- **C2 stays high and now covers P1.** The everyday ways in are a stop from outside followed by `fg` (now usable since #1317), a debugger, and a cgroup freeze. The run shows a freeze ends every reader; its race was not hit idle.
- **E2: medium → high.** The review ranked E2 inside the stated threat model, a hosted agent running as the user. P3-9 shows it crosses that model's boundary: **another local user** receives the token and the keystrokes, and picks a pid of this user's for `spyc attach` to signal.
  - Its preconditions narrow it: Linux with neither `XDG_RUNTIME_DIR` nor `TMPDIR` set (a login without systemd-logind, `su`, some containers and CI), and a token file for the planted name, which E7 leaves behind and the word-list names make cheap to guess.
  - On macOS `TMPDIR` is always set per user, so E2's cross-user case does not apply there.
- **H1: high, new.** See `H-cross-version.md`. It is fixed by one constant, and only before the first rc: 2.4.0's reader can't change.

## P1. On Linux, stopping and continuing spyc for any length of time ends every tab's link, and the retake races the old connection's close; when it loses, the tab stays `[detached]` and `^a R` runs the program twice

- **Severity:** high, as part of C2. Not frozen. **Lens:** correctness, reliability.
- **Sites (52ce15b7):**
  - The read: `crates/spyc-jar/src/client.rs:119-130` returns `Interrupted` as an error.
  - `Lost::Held` and the lease: `src/pane/jar/link.rs:186`, `:227-235`; `src/pane/mod.rs:357-366` (`lease_lapsed`).
  - The retake's order: `src/app/pre_recv.rs:209` (the call); `src/app/pane_starts.rs:184` (the worker, `spawn_worker` at `:239-258`) runs before `:195` (`sever_jar`).
  - The close: `src/pane/pty_host.rs:316-320`, `src/pane/jar/mod.rs:347-349`, `client.rs:254-258`, `:281`, `:289`.
  - The jar letting go: `crates/spyc-jar/src/jar/conn.rs:154`, `:173-174`, `:355-368`.
  - The refusal and the stand-in: `src/pane/jar/mod.rs:439-443`, `:552-554`; `client.rs:83` (a refusal is never retried); `accept.rs:201-202` (`OWNER_PRESENT`); `link.rs:476` (`signals: None`).
  - `^a R`: `src/app/pane_tabs.rs:34-36`, `:796-842`.
- **Claim:** the brief's reading holds at every step.
  1. A stop and continue ends every reader with `EINTR`, while the connection is still alive: the outbox writer holds its own copy of the socket (`client.rs:182-187`) and keeps pinging, and the stop is shorter than `OWNER_DEADLINE` (10 s, `limits.rs:43`).
  2. The jar's lock is held, so the reader sets `Lost::Held`, and `lease_lapsed` is true.
  3. The retake's worker starts before `sever_jar`.
  4. If the old connection is still the owner, `adopt` is refused with reason 2 and returns `link::detached`, with no second try.
  5. The stand-in isn't `jarred()`, so the tab is never taken back, and `^a R` doesn't ask and starts a new jar.
  6. A stop longer than 10 s is safe, because the jar has let go before spyc runs again. That is the only case `tests/jar_adopt.rs:1888-1950` covers: it waits for "owner none" before `SIGCONT`.
- **Evidence (run; `evidence/P1-P2/`: `drive_p1.py`, `run-linux.sh`, `run-mac.sh`, and per run `events.log` and `rounds.jsonl`):**

  | Run (4 tabs per session) | Stop | Tab-rounds | Readers ended (`EINTR`) | Tabs left `[detached]` (refused, reason 2) |
  |---|---|---|---|---|
  | Linux, idle | 1–3 s | 200 | 200 | 0 |
  | Linux, `stress-ng --cpu 14 --io 4` | 1–3 s | 200 | 200 | **5, in 4 of 50 rounds** |
  | Linux, same stress | 12–13 s | 32 | 32 | 0 |
  | Linux, `podman pause` and `unpause` (cgroup freeze) | 2 s | 40 | 40 | 0 (idle) |
  | Linux, a `SIGCONT` alone | – | 120 | 0 | 0 |
  | macOS, installed 52ce15b7 release build | 1–3 s | 40 | 0 | 0 |

  - Each lost tab's standard error reads `spyc: a restored tab's jar runs on without it (refused: the jar already has an owner); the tab is detached`.
  - `^a R` on the first lost tab drew no `[y/N]`: a new program (pid 4373) started in a new jar (4365) while the old one (4036) still ran in its jar (4028).
  - Field evidence of B12 on x86_64 glibc: CI run 45232716535, above.
- **How a user gets here:** not from the keyboard. spyc's raw mode makes `^z` a key, nothing in spyc sends `SIGTSTP`, and a `$` or foreground child gets its own process group (`proc.rs:251-258`, `:304-315`). The ways in:
  - a stop from outside, then `fg` (#1309's case, usable since #1317);
  - a debugger;
  - a cgroup freeze, shown here. systemd 256 and later freezes `user.slice` across suspend; that is from its release notes, and was not run on hardware.
  - The message calls a retake "a restored tab's jar", which bears on C12.
- **Status:** confirmed. Run on Linux; the macOS control shows no reader ending.
- **Direction:** `Client::recv` retries `Interrupted`, and the retake closes its old connection and waits on the jar's lock before its `hello`.

## P2. #1317 against B12, C2 and Unit D: it fixes none of them, makes B12 and C2 easier to reach, and its test can't see them

- **Severity:** informational. **Lens:** correctness, tests.
- **Sites:** `src/app/proc.rs:473-500` (a `SIGCONT` handler through signal-hook) and `:502-535`; `src/app/run.rs:634`, `:663`; `src/terminal.rs:482-535`; `tests/continued.rs:159-199`; `tests/jar_adopt.rs:1956-1982`.
- **B12:** unchanged. `client.rs` is untouched, and 400 of 400 readers still end.
  - The new handler makes `SIGCONT` a caught signal, but a `SIGCONT` alone ended 0 of 120 readers, because Linux delivers it to the main thread.
  - macOS still restarts the read: 0 of 40 readers ended.
- **C2:** the retake's order (`pane_starts.rs:184`, then `:195`) and the labels at `:318-322` are unchanged.
  - Before #1317, a Linux spyc stopped and continued by `fg` never took keys again. Now it does, and the new wake shows a lost tab without a key, so the user sees `[detached]` and can press `^a R`: #1317 makes C2's double run easier to reach.
  - `continued.rs` opens no jarred tab, so CI's Linux job runs exactly the sequence that triggers P1 without seeing it.
- **Unit D (D1 to D7, D9, D10):** no interaction. The handler is set only in `App::run`, not in the attach client, and `exec` resets it.
- **D8:** `terminal::continued` is a fourth place that sets terminal modes. Like `resume_tui`, it pushes no title and sets no `>1p`. D8 is unchanged.
- **Status:** confirmed by lines read and by runs.
- **Direction:** P1's red test extends `continued.rs` with jarred tabs on Linux.

## P3. MANUAL.md items 6 and 9, run without the owner

Remediation brief section 1, P3. Both items ran in containers on the Podman VM (arm64, 7 CPUs, 8 GiB), with nobody at a terminal. Scripts and logs are under `docs/drafts/review-3.0/evidence/P3/` (`A1/` for item 6, `E2/` for item 9, `build/` for the x86_64 build).

| id | settles | at 444857a2 | at 52ce15b7 (origin/main) | what changes |
|---|---|---|---|---|
| P3-6 | A1 | x86_64 glibc jar killed by `SIGPIPE` (status -13), on the first try once the race is forced; 0 deaths in 640 unforced tries under emulation | same (`crates/spyc-jar` is unchanged) | Status: confirmed on x86_64 glibc too. The try count can't be measured under emulation |
| P3-9 | E2 | not run (`src/pane/jar` unchanged since the pin) | across users: `spyc attach` lists uid 1001's planted sessions, sends the real token and the typed keys to one, and `SIGTERM`s a uid 1000 process it names. `^d` hands nothing over, but spyc exits instead of refusing | Status: confirmed across users on Linux. New: `^d`'s failure path quits spyc with a false message, and a planted directory stops every jarred tab from starting |

---

### P3-6. A1 on x86_64 glibc: the same `SIGPIPE` death, once the holder is gone before the send

- **Settles:** A1 ("x86_64 glibc not run, same code path"). **Severity:** unchanged (medium, frozen). **Lens:** reliability.
- **Setup.**
  - The VM runs amd64 images: binfmt has `qemu-x86_64` (`/usr/bin/qemu-x86_64-static`, QEMU 10.1.4, flags `POCF`), and `docker.io/amd64/rust:1.100.0-bookworm` reports `x86_64` and glibc 2.36. That tag was pulled, not `library/rust`, so the arm64 tag other agents use stayed where it was.
  - Building under emulation was not possible: `rustc --version` segfaults under qemu-user every time, 5 of 5, and `QEMU_CPU=max` doesn't help either. So the jar was **cross-compiled** for `x86_64-unknown-linux-gnu` in the arm64 `rust:1.100.0-bookworm` image, with `gcc-x86-64-linux-gnu` (`build/build.sh`, log `xbuild.log`), and run in the amd64 Debian container under emulation, as uid 1000.
  - **Only the jar crate was built.** `crates/spyc-jar` has no binary of its own. `cargo build -p spyc-jar --bin spyc-jar` names nothing, and the Makefile's `--bin spyc-jar` builds spyc's package, which means all of spyc and libghostty-vt. So a wrapper crate (`build/{pin,main}/Cargo.toml`, `main.rs`) depends on the commit's `crates/spyc-jar` by path. Its `main` is the one line in `src/bin/spyc-jar.rs`, `spyc_jar::cli::main_built(..)`. The differ is the same wrapper with `--features differ` (`spyc-jar/differ`) in its own target directory.
  - The lock versions match the repo's `Cargo.lock`: rustix 1.1.6, anyhow 1.0.104, linux-raw-sys 0.12.1, bitflags 2.13.2.
  - `git diff --stat 444857a2 origin/main -- crates/spyc-jar` is empty. Both commits were built and run all the same. The binaries are in `build/bin/`.
- **Evidence.**
  1. **`probe_linux.py` as A1 runs it** (`A1/run.sh`, `A1/run.log`, `A1/out/{pin,main}-r{1,2,3}/`). Three rounds at each commit.
     - Upgrading to the differ and back works.
     - Across the failed `exec` the ignore bit goes `SigIgn: 0000000000001000   SIGPIPE ignored=1` → `SigIgn: 0000000000000000   SIGPIPE ignored=0`, as on aarch64 musl.
     - Then the jar is **alive after 40 tries in all 6 rounds** (240 tries). Every refusal reads `didn't hold the handover: no word from the holder: Connection reset by peer (os error 104)`: the jar's `sendmsg` went out before the holder exited.
  2. **A holder that is gone at once** (`A1/probe_linux_quick.py`, `A1/holder/quick_holder.rs`, `A1/run_quick.log`). The shell wrapper is replaced by a native static aarch64 program that returns from `main` on `hold-handover` and otherwise `exec`s the jar under test. Five rounds at each commit: **alive after 40 tries in all 10 rounds** (400 tries), with the same reset every time.
     - The result fits qemu-user turning the vfork-style `clone` that glibc's `posix_spawn` makes into a plain `fork`. `spawn` then returns before the holder has run, so under emulation the holder never wins the race. This explanation is inferred, not traced.
  3. **The race forced** (`A1/probe_linux_inject.py`, `A1/inject_sidecar.sh`, `A1/run_inject.log`, `A1/out/inj-*/strace.log`).
     - After the failed `exec`, an arm64 sidecar sharing the container's pid namespace runs `strace -f -p <jar> -e inject=sendmsg:delay_enter=300000`. Each of the jar's `sendmsg` calls now starts 300 ms late, and the holder has exited by then.
     - **The jar died with status -13 on the first try, in 3 of 3 rounds at each commit.**
     - From `inj-main-fail-1/strace.log` (the pin's run, `inj-pin-fail-1`, is the same):
       ```text
       sendmsg(13, {... msg_iov=[{iov_base="h", iov_len=1}] ... cmsg_type=SCM_RIGHTS, cmsg_data=[10, 4, 8]}], ...}, 0 <unfinished ...>
       <... sendmsg resumed>) = -1 EPIPE (Broken pipe) (DELAYED)
       --- SIGPIPE {si_signo=SIGPIPE, si_code=SI_USER, ...} ---
       +++ killed by SIGPIPE +++
       ```
       That `sendmsg` is the holder handshake at `upgrade.rs:445-450`, the one-byte `h` with the held descriptors.
     - **Control:** the same injection with no failed `exec` first, so `SIGPIPE` stays ignored. Two rounds at each commit, 10 tries each: every try is refused with reason 7, `didn't hold the handover: Broken pipe (os error 32)`, and the jar stays alive. So the death comes from the disposition the failed `exec` left, not from emulation or from the tracer.
- **What it changes about A1:**
  - The status becomes "confirmed (Linux aarch64 musl natively; x86_64 glibc under emulation, with the holder's exit put before the send)". The mechanism is the same on both libcs: rustix's `sendmsg` on the `linux_raw` backend, no `MSG_NOSIGNAL`, and std's `do_exec` resetting `SIGPIPE` to default.
  - How often the race happens without help on real x86_64 hardware is still unmeasured. The native aarch64 run hit it after 5 tries, and emulation can't answer the question either way.
  - The fix direction stands. #1317 didn't touch the jar.
  - For slice C's red test: a holder that exits on its own loses this race on most runs. The test needs the holder's exit to come before the send by construction, not by chance.

### P3-9. E2 across users: another user's `/tmp/spyc-<uid>` takes the token, the keys and a `SIGTERM`, and `^d` quits rather than refusing

- **Settles:** E2's cross-user precondition, which was read from the code and not run on Linux. **Severity:** E2 unchanged (medium). One new low below. **Lens:** security.
- **Setup** (`E2/e2_drive.py`, `E2/stand_in.py`, `E2/run.log`, `E2/out/<phase>/{events.log,raw.out,stand_in.log}`).
  - spyc was built at 52ce15b7 (`spyc 2.5.0-CURRENT (52ce15b7)`, debug, aarch64 musl) in a copy of the shared clone (`E2/build.sh`). The copy was removed afterwards.
  - Each phase ran in a fresh `localhost/spyc-review-linux` container, so `/tmp` was clean every time. The driver runs as root and adds `mallory` (uid 1001). Before uid 1000 starts anything, `mallory` runs `umask 0; mkdir -m 777 /tmp/spyc-1000 /tmp/spyc-1000/sessions`.
  - uid 1000 (`rev`) runs spyc in a PTY with the technique from Unit C's `drive_v7.py`. Its `HOME` and the `XDG_STATE`/`CONFIG`/`DATA`/`CACHE` directories are scratch, and **neither `XDG_RUNTIME_DIR` nor `TMPDIR` is set**.
- **Evidence.**
  - **Control, nothing planted.**
    - The tab starts in a jar at `/tmp/spyc-1000/jars/<id>.sock`, and the directory is `drwx------ rev`.
    - `^a k` then `^d` hands over: `ps` shows `spyc attach --force GARAM_HARISSA`, `spyc-jar GARAM_HARISSA --wire 1` and the new spyc under it, with the session's socket in `/tmp/spyc-1000/sessions` (`drwx------ rev`).
  - **Planted, a tab open, then `^d`.**
    - The tab never starts. After the 30 s hold it shows `[not started]` and `spyc: no place for the tab's jar: /tmp/spyc-1000 belongs to uid 1001, not this user's 1000; no jar is trusted there ^a R starts it again`.
    - `^d` doesn't refuse inside spyc: **spyc leaves the terminal and exits** (the PTY closes 0.16 s after `^d`), printing:
      ```text
      Error: ZAATAR_ROSEMARY's tabs run on in their jars: `spyc -r` takes them back
      Caused by: /tmp/spyc-1000 belongs to uid 1001, not this user's 1000; no jar is trusted there
      ```
    - Nothing was written into the planted directory, and no process of uid 1000 is left.
  - **Planted, no tab, then `^d`.** The same exit and the same two lines (`DUKKAH_SUMAC`).
  - **Planted, then `spyc attach`.**
    - `mallory` runs Unit E's stand-in, adapted, at `/tmp/spyc-1000/sessions/{VICTIM,PLANTED2}.sock`: sockets at `0777`, each lock file `0644` with `mallory` holding the lock, so spyc reads the session as Live.
    - Its `describe` names `owner_pid` = a `sleep 300` of uid 1000.
    - uid 1000 has a token file for `VICTIM`, kept from an earlier session of that name (E7).
    - `env -u XDG_RUNTIME_DIR -u TMPDIR spyc attach` prints:
      ```text
        1  PLANTED2  (attached elsewhere)
        2  VICTIM  (attached elsewhere)
      Attach to which? VICTIM
      spyc: VICTIM is attached elsewhere (pid 14); taking it over
      [a stand-in jar of uid 1001 paints this terminal] password: [detached]
      ```
    - The `sleep` ends with returncode -15 (`SIGTERM`).
    - The stand-in, running as uid 1001, logs:
      ```text
      VICTIM hello: role 1 token 0123456789abcdef0123456789abcdef-REAL-SESSION-TOKEN from peer uid 1000 pid 20
      VICTIM input frame: b'hunter2\r'
      ```
- **What it changes about E2:**
  - The status becomes "confirmed across users (Linux, origin/main 52ce15b7)". `git diff --stat 444857a2 origin/main -- src/pane crates` is empty, so the pin behaves the same.
  - The attach client checks neither the directory nor the socket's owner. A session that another user's socket serves gets the token, every typed key and a `SIGTERM` for a pid of that user's choosing.
  - Its one precondition is the token file for the planted name: `run` loads the token before `take_over`, so without one nothing is sent and nothing is signalled. That is an inference from the code; this run always had a token file.
  - Session names come from a word list (`TARRAGON_CUMIN`, `GARAM_HARISSA`), and token files are never removed (E7). Planting many names is cheap. That was not run.
  - `spyc shutdown` was not run.
- **New, low (with D2/D6, slice E):**
  - When the session's jar can't be started, `^d` quits spyc instead of staying in the loop.
  - Its message says the tabs "run on in their jars" when none ran, here because none could start.
  - The handover had already saved the session as released (`hand_over` calls `save_released` before `session::start`). This is the ground of D2/D6's decision, "when either fails, spyc stays in its loop and says so", and it is reachable by another user, with no long name needed.
- **Also seen (informational, for E4's SECURITY.md):** while another user owns `/tmp/spyc-<uid>`, none of this user's tabs starts in a jar. This fails closed, as intended, and the tab says why, but it is a denial of service any local user can cause where `XDG_RUNTIME_DIR` and `TMPDIR` are unset.


## MANUAL.md

- Items 6 (A1 on x86_64), 7 (a stopped and continued spyc on Linux) and 9 (E2 across users) moved here, as P3-6, P1 and P3-9.
- Items 1 and 2 (C1, C2) were dropped at the owner's word: the red test and the probes already show them.
- The owner runs items 4, 5 and 8.
