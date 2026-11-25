# Shell attention: can the jar tell that a command waits on the terminal?

A spike: from the position of a pane's jar (the holder of the PTY master and
the ancestor of the command), with no tracing and no timer, can spyc tell a
shell command that waits on the user at the terminal from one that works, and
from one that waits on something else, on Linux and on macOS? It builds a probe
and this report, and nothing in spyc. Written against `ee035a3`
(`2.5.0-CURRENT`); where the brief and the tree disagree is listed near the
end.

## The answer

- **The terminal settings (S1) read from the master on both platforms, and
  match what the program set on the slave.** `stty -echo -icanon min 3 time 7`
  on the slave reads back exactly from the master
  (`linux/1/s1-confirm/2s/ancestor`, `macos/n/a/s1-confirm/2s/ancestor`), and
  the ancestor's read equals the unrelated reader's in every row pair on both
  platforms. Echo off in canonical mode flags no false positive anywhere and
  catches every password prompt (`sudo`, `ssh`, `git`, `getpass`, `read -s`),
  but only those: it misses 36 of 51 rows, every one an echo-on prompt.
- **On Linux, the kernel names the wait.** `/proc/<pid>/task/<tid>/syscall`
  shows a `read` on the slave or `/dev/tty`, and `fdinfo` shows an epoll set
  holding the terminal. `ptrace_scope` 1 admits the ancestor and refuses an
  unrelated process, as the brief supposed; scope 2 refuses both. But
  `/proc/<pid>/task/<tid>/wchan`, which Yama doesn't gate, names `wait_woken`
  for every terminal read in the corpus; of the other waits, only a blocking
  socket read sleeps there too (`linux/2/recv-stall/2s/unrelated`).
- **On macOS, nothing names the wait.** Every waiting thread reads `waiting`,
  whatever it waits on. A disk wait reads `uninterruptible`, and a kqueue's
  knotes show an event loop's interest in the terminal; nothing more.
- **CPU and disk (S3) work as a veto**, given a threshold: macOS's nanosecond
  counters see an idle event loop's timer wakeups.
- **Quiet output with an unterminated last line (S4) alone flags 16 of 60
  negative rows on Linux and 19 of 57 on macOS**: every full-screen program and
  the shell's own prompt look like that.
- **The best rule on Linux, from the jar** at scope 0 or 1: a member waiting on
  the terminal (S2's syscall, or an epoll or `select` set that is the
  terminal) in canonical mode, not vetoed, or echo off in canonical mode. It
  has no false positive and misses 9 of 51: the two raw-mode prompts and the
  stopped job. **From spyc**, at any scope, the same rule on `wchan` misses the
  same 9 and also flags the blocking socket read (3 of 60). **On macOS** the
  scrape stays: the best rule found, S4 with a job in the foreground in
  canonical mode, not vetoed, or echo off, makes 3 false positives of 57 and
  misses 21 of 51.

## Method

The probe (`probe/`, Rust) opens a PTY and starts a shell on it as
`crates/spyc-jar/src/pty.rs` does: the master `O_NOCTTY` and close-on-exec
(`crates/spyc-jar/src/pty.rs:30-31`), the slave `O_NOCTTY` (`pty.rs:37-42`), and the shell
through the probe's own `pty-child`, which calls `setsid`, `TIOCSCTTY` and
`exec` (`pty.rs:80-82`, copied line for line in `probe/src/spawn.rs`). The
probe is then the shell's parent, as the jar is: in every Yama row the target's
shell is the prober's child (`target_info.shell`, `prober` in
`access-*.json`). The tab's shell is `bash --noprofile --norc`, with `HOME`
set to a scratch directory, so no user configuration, history, known-hosts file
or credential helper is read or written.

Each case is typed into that shell, as a user would. After the command's echo,
the probe reads the master until output has been quiet for 250 ms, 2 s and 10 s
(new output before the 10 s point restarts the clock, up to four times in the
first 15 s; a point not reached by 35 s is read then, with `reached: false`).
At each point it reads every signal twice over, from two positions:

- **ancestor**: the probe itself, holding the master: `tcgetpgrp` and
  `tcgetattr` on the master, then the foreground group's members by pid.
- **unrelated**: a child of the probe that first calls `setsid`, so it is not
  an ancestor of the program, and reads by pid only: the foreground group from
  the shell's `/proc/<pid>/stat` (`tpgid`) or `proc_bsdinfo.e_tpgid`, and the
  settings by opening the slave by path. That is spyc's position.

S3 is read as a pair 100 ms apart at each point (`s3_pair_delta`), and also
against the previous point (`s3_since_prev`). S4 is the bytes after the last
newline on the master, escape sequences dropped: `unterminated` when something
visible is left. No row's verdict uses anything but these.

Linux ran in a rootless podman container on the podman machine's VM (Fedora
CoreOS, kernel `6.19.7`, `aarch64`), as an ordinary user (`uid 1000`) with no
capabilities, once under each of `ptrace_scope` 0, 1 and 2, which `run.sh` sets
on the VM and puts back. macOS ran on the owner's Mac (macOS `26.7.2`, Apple
silicon) as the owner's user. Programs and versions are in each file's `meta`.

```sh
./run.sh            # both platforms; ./run.sh macos or ./run.sh linux
python3 -I analyze.py > analysis.md
```

Every row has an `id`, `platform/scope/case/point/position`
(`linux/1/sudo/2s/ancestor`), and the report cites rows by it. `analysis.md`
is `analyze.py`'s output over the committed files: every table below comes
from it.

## 1. Each signal, per program and platform

The tables give the `2s` point from the ancestor; every point and position is
in `analysis.md` and the observation files. Linux is shown at scope 1; S1, S3 and S4
don't depend on the scope.

**S1 (terminal settings).** The master reflects the slave on both platforms:
the stop condition doesn't apply. `s1-confirm`'s distinctive `VMIN` 3 and `VTIME` 7, set by
`stty` on the slave, read back from the master and from the slave opened by
path alike. Across the corpus, prompts sit in canonical mode with echo
on (`[y/N]`, a username, a host key, `rm -i`, `input()`) or off (passwords);
raw-mode prompts (node's `readline`, `fzf`, `gum`) and `vim` turn off
canonical mode, echo and `ISIG`; `less`, `top`, the Python REPL and bash's own
prompt turn off canonical mode and echo and keep `ISIG`.

**S2 (Linux).** For each thread of each foreground member, the syscall number
and first two arguments, the `stat` state and `wchan`. A `read` names its
descriptor, which `/proc/<pid>/fd/<n>` resolves: `ssh` reads `/dev/tty` on
descriptor 4 at both its prompts (`linux/1/ssh-hostkey/2s/ancestor`), `getpass`
`/dev/tty` on 3, and `git`'s prompt runs in `git-remote-http`, a second
member, reading `/dev/tty` while `git` waits on a pipe and a child
(`linux/1/git-username/2s/ancestor`). A static Go binary reads descriptor 0 on
one thread while four park in `futex` (`linux/1/go-read/2s/ancestor`). What
can be learnt without memory for the multiplexing calls:

- `epoll_pwait`: the set, from `/proc/<pid>/fdinfo/<epfd>`, each target
  resolved: node's loop and `fzf` both watch the terminal
  (`linux/1/node-question/2s/ancestor`, `linux/1/raw-fzf/2s/ancestor`).
- `pselect6` and `select`: `nfds`, the highest descriptor plus one, so at most
  which descriptors could be in the set; every `pselect6` in the corpus had
  `nfds` 1, descriptor 0, the terminal (bash's prompt, `vim`, `top`, the REPL).
- `ppoll` and `poll`: only the count (`curl`'s 2, `linux/1/curl-stall/2s/ancestor`);
  the set itself is in the process's memory.

`wchan` names the kernel function each thread sleeps in, and is the finding:
`wait_woken` for every terminal read and for a blocking TCP `recv`
(`recv-stall`, whose syscall reads `recvfrom` where it can be read), and for
the other waits `hrtimer_nanosleep`, `inet_csk_accept`, `anon_pipe_read`, `wait_for_partner`
(a FIFO's `open`), `do_wait`, `ep_poll`, `poll_schedule_timeout` and
`futex_do_wait` (every `linux/2/*/2s/unrelated` row). `/proc/<pid>/syscall`
needs `PTRACE_MODE_ATTACH` and `wchan` only `PTRACE_MODE_READ`, which Yama
doesn't restrict (Yama matrix). For a member the reader may not inspect at all,
`wchan` reads `0` rather than failing (`sudo`, `linux/1/sudo/2s/ancestor`).

**S2m (macOS).** Confirmed: nothing a same-user, unprivileged, untraced
process can call names what a thread waits on. What was tried:

- `proc_pidinfo(PROC_PIDTHREADINFO)` for every thread: `pth_run_state` is
  `waiting` for a terminal read (`macos/n/a/py-getpass/2s/ancestor`), a timer
  (`…/sleep/2s/…`), a socket (`…/nc-listen/2s/…`, `…/recv-stall/2s/…`), a
  FIFO's open (`…/fifo/2s/…`) and a child (`…/make-child/2s/…`) alike;
  `running` only while on CPU (`…/cpu-loop/2s/…`), and `uninterruptible` in a
  disk wait (`…/cp-large/2s/…`), the one wait it does name. `pth_sleep_time`
  was 0 throughout, and `pth_flags` set only `TH_FLAGS_SWAPPED`, on idle
  threads of every kind of wait (`sleep`, `curl-stall` and `go-read` alike).
- `kinfo_proc`'s wait channel, through `ps -o wchan`: `-` for every member
  (`ps_wchan_state` in each `2s` `ancestor` row).
- `proc_pidinfo(PROC_PIDTASKALLINFO)`: counters (`syscalls_unix`, `csw`,
  `numrunning`), no wait. `proc_pidfdinfo(PROC_PIDFDVNODEPATHINFO)` names each
  descriptor's file, so which members hold the terminal, not which wait on it.
- `proc_pidfdinfo(PROC_PIDFDKQUEUE_EXTINFO)` lists a kqueue's knotes, and is the
  one partial exception: node's loop has an `EVFILT_READ` knote on its
  terminal descriptor (`macos/n/a/node-question/2s/ancestor`), and `gum`'s on
  descriptor 0 (`…/raw-gum/2s/…`). That is registered interest, the macOS
  counterpart of Linux's `fdinfo` on an epoll descriptor, not a blocked call,
  and programs that `read` or `select` (`python3`, `vim`, `less`, `goread`)
  have no kqueue to read.
- Ruled out by decision 1 or by privilege: `task_for_pid` and
  `thread_get_state`, dtrace, `stackshot` and `spindump` (root), `sample`
  (uses `task_for_pid`).

A root-owned member is opaque: every `proc_info` call on `sudo` and on the
setuid `top` answers `EPERM`, its name included (`access-macos.json`,
`root-owned`; `macos/n/a/sudo/2s/ancestor`). The terminal settings still
read, being the terminal's.

**S3 (CPU and disk).** Linux: `utime`, `stime`, `cutime` and `cstime` from
`/proc/<pid>/stat` count clock ticks of 10 ms (`clk_tck` 100 in `meta`);
`/proc/<pid>/schedstat` counts nanoseconds, but for the thread group leader
only, so `fool-thread`'s busy second thread shows in the ticks and not there
(`linux/1/fool-thread/2s/ancestor`); `read_bytes` and `write_bytes` from
`/proc/<pid>/io` count storage traffic (`linux/1/dd-direct/250ms/ancestor`).
`cutime` and `cstime` include children once reaped and waited for. macOS:
`proc_pid_rusage(RUSAGE_INFO_V4)` gives user, system and children's time in
Mach ticks, 125/3 ns each on Apple silicon (the probe converts), disk bytes, and
instruction and cycle counts. Unreadable: a root-owned member's `io` (Linux,
`EACCES`) and everything of it on macOS (`EPERM`); `stat` stays readable on
Linux. Read over 100 ms, a working program moves by 14 to 120 ms of CPU or
megabytes of disk; a waiting one by nothing on Linux, and on macOS by tens to
hundreds of microseconds for an event loop's timers (curl, the Python
REPL, `gum`). The rules count a member as working at 2 ms or more per 100 ms,
or any disk bytes.

**S4 (the baseline).** Quiet output whose last line holds something visible.
It is true for every prompt that prints one, false for silent `read`s, and true
for every full-screen program, the Python REPL, the shell's own prompt and the
`Continue? [y/N]` that only sleeps.

### Signals on linux, 2s, ancestor, scope 1

S1: E/e echo on/off, C/c canonical on/off, I/i `ISIG`. S3: CPU (clock ticks) / scheduler run time over 100 ms, disk bytes.

| Case | Expect | S1 | S2 | S3 | S4 | Row |
|---|---|---|---|---|---|---|
| shell-prompt | — | ecI | bash/S:pselect6(nfds=1) | 0/0 ms, 0 KiB | yes | `linux/1/shell-prompt/2s/ancestor` |
| s1-confirm | — | ecI | sleep/S:clock_nanosleep | 0/0 ms, 0 KiB | no | `linux/1/s1-confirm/2s/ancestor` |
| sudo | blocked | eCI | sudo:EACCES | 0/0 ms, 0 KiB | yes | `linux/1/sudo/2s/ancestor` |
| ssh-hostkey | blocked | ECI | ssh/S:read(4=tty) | 0/0 ms, 0 KiB | yes | `linux/1/ssh-hostkey/2s/ancestor` |
| ssh-password | blocked | eCI | ssh/S:read(4=tty) | 0/0 ms, 0 KiB | yes | `linux/1/ssh-password/2s/ancestor` |
| git-username | blocked | ECI | git/S:read(6=pipe:[15308147]) git/S:wait4 git-remote-http/S:read(5=tty) | 0/0 ms, 0 KiB | yes | `linux/1/git-username/2s/ancestor` |
| git-password | blocked | eCI | git/S:read(6=pipe:[15307464]) git/S:wait4 git-remote-http/S:read(5=tty) | 0/0 ms, 0 KiB | yes | `linux/1/git-password/2s/ancestor` |
| read-sh | blocked | ECI | sh/S:read(0=tty) | 0/0 ms, 0 KiB | no | `linux/1/read-sh/2s/ancestor` |
| read-bash | blocked | ECI | bash/S:read(0=tty) | 0/0 ms, 0 KiB | no | `linux/1/read-bash/2s/ancestor` |
| read-zsh | blocked | ECI | zsh/S:read(0=tty) | 0/0 ms, 0 KiB | no | `linux/1/read-zsh/2s/ancestor` |
| read-builtin | blocked | ECI | bash/S:read(0=tty) | 0/0 ms, 0 KiB | no | `linux/1/read-builtin/2s/ancestor` |
| read-silent | blocked | eCI | bash/S:read(0=tty) | 0/0 ms, 0 KiB | yes | `linux/1/read-silent/2s/ancestor` |
| rm-i | blocked | ECI | rm/S:read(0=tty) | 0/0 ms, 0 KiB | yes | `linux/1/rm-i/2s/ancestor` |
| py-input | blocked | ECI | python3/S:read(0=tty) | 0/0 ms, 0 KiB | yes | `linux/1/py-input/2s/ancestor` |
| py-getpass | blocked | eCI | python3/S:read(3=tty) | 0/0 ms, 0 KiB | yes | `linux/1/py-getpass/2s/ancestor` |
| node-question | blocked | eci | node/S:epoll_pwait(tty) node/S:epoll_pwait(2 fds) node/S:futex×5 | 0/0 ms, 0 KiB | yes | `linux/1/node-question/2s/ancestor` |
| raw-fzf | blocked | eci | fzf/S:futex×7 fzf/S:read(3=tty) fzf/S:epoll_pwait(tty) | 0/0 ms, 0 KiB | no | `linux/1/raw-fzf/2s/ancestor` |
| go-read | blocked | ECI | goread/S:read(0=tty) goread/S:futex×4 | 0/0 ms, 0 KiB | yes | `linux/1/go-read/2s/ancestor` |
| ttin-job | blocked | ecI | bash/S:pselect6(nfds=1) | 0/0 ms, 0 KiB | yes | `linux/1/ttin-job/2s/ancestor` |
| vim | — | eci | vim/S:pselect6(nfds=1) vim/S:rt_sigtimedwait | 0/0 ms, 0 KiB | yes | `linux/1/vim/2s/ancestor` |
| less | — | ecI | less/S:read(3=tty) | 0/0 ms, 0 KiB | yes | `linux/1/less/2s/ancestor` |
| top | — | ecI | top/S:pselect6(nfds=1) | 0/0 ms, 0 KiB | no | `linux/1/top/2s/ancestor` |
| py-repl | — | ecI | python3/S:pselect6(nfds=1) | 0/0 ms, 0 KiB | yes | `linux/1/py-repl/2s/ancestor` |
| cpu-loop | — | ECI | sh/R:running | 110/107 ms, 0 KiB | no | `linux/1/cpu-loop/2s/ancestor` |
| compile | — | ECI | cc/S:wait4 cc1/R:running | 100/103 ms, 0 KiB | no | `linux/1/compile/2s/ancestor` |
| dd-direct | — | ECI | dd/D:write | 20/29 ms, 6936 KiB | no | `linux/1/dd-direct/2s/ancestor` |
| cp-large | — | ECI | cp/D:read(3=/work/home/big.bin) | 0/0 ms, 0 KiB | no | `linux/1/cp-large/2s/ancestor` |
| sleep | — | ECI | sleep/S:clock_nanosleep | 0/0 ms, 0 KiB | no | `linux/1/sleep/2s/ancestor` |
| nc-listen | — | ECI | nc/S:accept4 | 0/0 ms, 0 KiB | no | `linux/1/nc-listen/2s/ancestor` |
| curl-stall | — | ECI | curl/S:ppoll(nfds=2) | 0/0 ms, 0 KiB | no | `linux/1/curl-stall/2s/ancestor` |
| recv-stall | — | ECI | python3/S:recvfrom | 0/0 ms, 0 KiB | no | `linux/1/recv-stall/2s/ancestor` |
| fifo | — | ECI | cat/S:openat | 0/0 ms, 0 KiB | no | `linux/1/fifo/2s/ancestor` |
| make-child | — | ECI | make/S:wait4 sleep/S:clock_nanosleep | 0/0 ms, 0 KiB | no | `linux/1/make-child/2s/ancestor` |
| fool-prompt-sleep | — | ECI | sh/S:wait4 sleep/S:clock_nanosleep | 0/0 ms, 0 KiB | yes | `linux/1/fool-prompt-sleep/2s/ancestor` |
| fool-thread | — | ecI | python3/S:read(0=tty) python3/R:running | 110/0 ms, 0 KiB | no | `linux/1/fool-thread/2s/ancestor` |
| devserver-idle | — | ecI | python3/S:read(0=tty) python3/S:accept4 | 0/0 ms, 0 KiB | no | `linux/1/devserver-idle/2s/ancestor` |
| fool-raw-poll | — | eci | python3/S:pselect6(nfds=1) | 50/49 ms, 0 KiB | no | `linux/1/fool-raw-poll/2s/ancestor` |

Below the S3 threshold (CPU moved by under 2 ms per 100 ms, µs per row): curl-stall [24].

Below the S3 threshold (CPU moved by under 2 ms per 100 ms, µs per row): curl-stall [24].

### Signals on macos, 2s, ancestor

S1: E/e echo on/off, C/c canonical on/off, I/i `ISIG`. S3: CPU (clock ticks) / scheduler run time over 100 ms, disk bytes.

| Case | Expect | S1 | S2 | S3 | S4 | Row |
|---|---|---|---|---|---|---|
| shell-prompt | — | ecI | bash:waiting | 0/0 ms, 0 KiB | yes | `macos/n/a/shell-prompt/2s/ancestor` |
| s1-confirm | — | ecI | sleep:waiting | 0/0 ms, 0 KiB | no | `macos/n/a/s1-confirm/2s/ancestor` |
| sudo | blocked | eCI | None:EPERM | 0/0 ms, 0 KiB | yes | `macos/n/a/sudo/2s/ancestor` |
| ssh-hostkey | blocked | ECI | ssh:waiting | 0/0 ms, 0 KiB | yes | `macos/n/a/ssh-hostkey/2s/ancestor` |
| ssh-password | blocked | eCI | ssh:waiting | 0/0 ms, 0 KiB | yes | `macos/n/a/ssh-password/2s/ancestor` |
| git-username | blocked | ECI | git-remote-http:waiting×3 git:waiting×2 | 0/0 ms, 0 KiB | yes | `macos/n/a/git-username/2s/ancestor` |
| git-password | blocked | eCI | git-remote-http:waiting×3 git:waiting×2 | 0/0 ms, 0 KiB | yes | `macos/n/a/git-password/2s/ancestor` |
| read-sh | blocked | ECI | bash:waiting | 0/0 ms, 0 KiB | no | `macos/n/a/read-sh/2s/ancestor` |
| read-bash | blocked | ECI | bash:waiting | 0/0 ms, 0 KiB | no | `macos/n/a/read-bash/2s/ancestor` |
| read-zsh | blocked | ECI | zsh:waiting | 0/0 ms, 0 KiB | no | `macos/n/a/read-zsh/2s/ancestor` |
| read-builtin | blocked | ECI | bash:waiting | 0/0 ms, 0 KiB | no | `macos/n/a/read-builtin/2s/ancestor` |
| read-silent | blocked | eCI | bash:waiting | 0/0 ms, 0 KiB | yes | `macos/n/a/read-silent/2s/ancestor` |
| rm-i | blocked | ECI | rm:waiting | 0/0 ms, 0 KiB | yes | `macos/n/a/rm-i/2s/ancestor` |
| py-input | blocked | ECI | Python:waiting | 0/0 ms, 0 KiB | yes | `macos/n/a/py-input/2s/ancestor` |
| py-getpass | blocked | eCI | Python:waiting | 0/0 ms, 0 KiB | yes | `macos/n/a/py-getpass/2s/ancestor` |
| node-question | blocked | eci | node:waiting×7 | 0/0 ms, 0 KiB | yes | `macos/n/a/node-question/2s/ancestor` |
| raw-gum | blocked | eci | gum:waiting×15 | 1/1 ms, 0 KiB | yes | `macos/n/a/raw-gum/2s/ancestor` |
| go-read | blocked | ECI | goread:waiting×5 | 0/0 ms, 0 KiB | yes | `macos/n/a/go-read/2s/ancestor` |
| ttin-job | blocked | ecI | bash:waiting | 0/0 ms, 0 KiB | yes | `macos/n/a/ttin-job/2s/ancestor` |
| vim | — | eci | vim:waiting×2 | 0/0 ms, 0 KiB | yes | `macos/n/a/vim/2s/ancestor` |
| less | — | ecI | less:waiting | 0/0 ms, 0 KiB | yes | `macos/n/a/less/2s/ancestor` |
| top | — | ecI | None:EPERM | 0/0 ms, 0 KiB | yes | `macos/n/a/top/2s/ancestor` |
| py-repl | — | ecI | Python:waiting | 0/0 ms, 0 KiB | yes | `macos/n/a/py-repl/2s/ancestor` |
| cpu-loop | — | ECI | bash:running | 106/106 ms, 0 KiB | no | `macos/n/a/cpu-loop/2s/ancestor` |
| compile | — | ECI | clang:running clang:waiting | 113/113 ms, 80 KiB | no | `macos/n/a/compile/2s/ancestor` |
| cp-large | — | ECI | cp:uninterruptible | 14/14 ms, 242356 KiB | no | `macos/n/a/cp-large/2s/ancestor` |
| sleep | — | ECI | sleep:waiting | 0/0 ms, 0 KiB | no | `macos/n/a/sleep/2s/ancestor` |
| nc-listen | — | ECI | nc:waiting | 0/0 ms, 0 KiB | no | `macos/n/a/nc-listen/2s/ancestor` |
| curl-stall | — | ECI | curl:waiting×3 | 0/0 ms, 0 KiB | no | `macos/n/a/curl-stall/2s/ancestor` |
| recv-stall | — | ECI | Python:waiting | 0/0 ms, 0 KiB | no | `macos/n/a/recv-stall/2s/ancestor` |
| fifo | — | ECI | cat:waiting | 0/0 ms, 0 KiB | no | `macos/n/a/fifo/2s/ancestor` |
| make-child | — | ECI | sleep:waiting gnumake:waiting | 0/0 ms, 0 KiB | no | `macos/n/a/make-child/2s/ancestor` |
| fool-prompt-sleep | — | ECI | sleep:waiting bash:waiting | 0/0 ms, 0 KiB | yes | `macos/n/a/fool-prompt-sleep/2s/ancestor` |
| fool-thread | — | ecI | Python:waiting Python:running | 108/108 ms, 0 KiB | no | `macos/n/a/fool-thread/2s/ancestor` |
| devserver-idle | — | ecI | Python:waiting×2 | 0/0 ms, 0 KiB | no | `macos/n/a/devserver-idle/2s/ancestor` |
| fool-raw-poll | — | eci | Python:waiting | 40/40 ms, 0 KiB | no | `macos/n/a/fool-raw-poll/2s/ancestor` |

Below the S3 threshold (CPU moved by under 2 ms per 100 ms, µs per row): curl-stall [21, 34]; py-repl [35, 42, 42, 44, 65, 65]; raw-gum [356, 429, 461, 498, 611, 769].

Below the S3 threshold (CPU moved by under 2 ms per 100 ms, µs per row): curl-stall [21, 34]; py-repl [35, 42, 42, 44, 65, 65]; raw-gum [356, 429, 461, 498, 611, 769].

## 2. Rules: false positives and false negatives

Each cell counts rows: a program at a time point. The 17 terminal cases make
51 positive rows on each platform; the others (controls, interaction, working,
other, fools) make 60 negative rows on Linux and 57 on macOS, which has no
`dd-direct`. A row whose prompt never came would be
dropped; none was. `analysis.md` has every scope and position, and the cases
behind each count.

- **R1** echo off in canonical mode (S1).
- **R2** a foreground member's thread blocked in `read` on the terminal (S2's
  syscall). **R2b** adds an epoll set holding the terminal, or a `select`
  whose possible descriptors are all the terminal.
- **R3** R2b, vetoed when S3 moves.
- **R4** S4 alone. **R4g** S4 while a job, not the shell, holds the
  foreground, not vetoed. **R6** R4g in canonical mode.
- **R5** S4 and (R1 or R2b), not vetoed: S4 corroborated by the others.
- **R7** R2b in canonical mode, not vetoed. **R2w** a thread asleep in
  `wait_woken` (S2's `wchan`). **R7w** R2w in canonical mode, not vetoed.
- **R8** R7w or R1, and **R9** R7 or R1 (Linux); **R8m** R6 or R1 (macOS).

| Rule | linux scope 1, ancestor | linux scope 1, unrelated | linux scope 2, unrelated | macos ancestor |
|---|---|---|---|---|
| R1 echo off, canonical | FP 0/60, FN 36/51 | FP 0/60, FN 36/51 | FP 0/60, FN 36/51 | FP 0/57, FN 36/51 |
| R4 S4 alone | FP 16/60, FN 15/51 | FP 16/60, FN 15/51 | FP 17/60, FN 15/51 | FP 19/57, FN 12/51 |
| R4g S4, a job in the foreground, not vetoed | FP 12/60, FN 18/51 | FP 12/60, FN 18/51 | FP 12/60, FN 18/51 | FP 15/57, FN 15/51 |
| R6 R4g and canonical | FP 3/60, FN 21/51 | FP 3/60, FN 21/51 | FP 3/60, FN 21/51 | FP 3/57, FN 21/51 |
| R2 member blocked reading the terminal | FP 9/60, FN 9/51 | FP 0/60, FN 51/51 | FP 0/60, FN 51/51 | n/a |
| R2b R2, or a wait set that is the terminal | FP 24/60, FN 3/51 | FP 0/60, FN 51/51 | FP 0/60, FN 51/51 | n/a |
| R3 R2b vetoed by S3 | FP 19/60, FN 3/51 | FP 0/60, FN 51/51 | FP 0/60, FN 51/51 | n/a |
| R5 S4 and (R1 or R2b), not vetoed | FP 13/60, FN 15/51 | FP 0/60, FN 36/51 | FP 0/60, FN 36/51 | n/a |
| R7 R2b and canonical, not vetoed | FP 0/60, FN 12/51 | FP 0/60, FN 51/51 | FP 0/60, FN 51/51 | n/a |
| R2w a thread asleep in wait_woken | FP 12/60, FN 9/51 | FP 12/60, FN 9/51 | FP 12/60, FN 9/51 | n/a |
| R7w R2w and canonical, not vetoed | FP 3/60, FN 12/51 | FP 3/60, FN 12/51 | FP 3/60, FN 12/51 | n/a |
| R8 R7w or R1 | FP 3/60, FN 9/51 | FP 3/60, FN 9/51 | FP 3/60, FN 9/51 | n/a |
| R9 R7 or R1 | FP 0/60, FN 9/51 | FP 0/60, FN 36/51 | FP 0/60, FN 36/51 | n/a |
| R5m S4 and R1, not vetoed | n/a | n/a | n/a | FP 0/57, FN 36/51 |
| R8m R6 or R1 | n/a | n/a | n/a | FP 3/57, FN 21/51 |

What the counts say:

- **S1** (R1) is precise and narrow: it never fires wrongly, and catches the
  password prompts only.
- **S2** (R2) flags `less`, which reads `/dev/tty`, and the two programs
  built with a thread reading the terminal; it misses `sudo` (unreadable),
  node (an epoll wait) and the stopped job. Widening it to wait sets (R2b)
  catches node and `fzf` but flags every raw-mode program and the shell's own
  prompt, including the one that follows `cp-large`. S3's veto
  (R3) removes only what works (`fool-thread`, `fool-raw-poll`); the idle dev
  server's thread reading the terminal stays a false positive. Canonical mode (R7) is what separates a prompt from an
  interactive program: every one of R2b's false positives turns canonical mode
  off.
- **The position matters only for the syscall file.** At scope 1 the unrelated
  reader gets `EPERM` for every `syscall` read, so R2, R2b, R3 and R7 never
  fire for it; at scope 2 neither position can read it. The `wchan` rules
  score the same from every position at every scope, and pay for it with the
  socket read: `wait_woken` doesn't say which kind of file the read is on.
- **S4** alone (R4) is the weakest rule here, and corroborating it (R5) only
  helps where S2's syscall reads. Gating it on a job in the foreground (R4g)
  removes the shell's prompt; canonical mode (R6) removes the full-screen
  programs and leaves the `Continue? [y/N]` that only sleeps, which no signal
  here can tell from a prompt.

## 3. The Yama matrix

`access-linux.json` has one row per scope, target and position, each cell the
file's answer: `ok` or the `errno`. The targets: a same-user job in the
prober's PTY (`sleep 300`), `sudo` at its password prompt (setuid root), an
orphan whose parent exited (reparented to the container's `init`), and the
same orphan with the prober set as child subreaper (`PR_SET_CHILD_SUBREAPER`).
The unrelated reader is the prober's child in a session of its own: not an
ancestor of any target.

### Access on linux

| Scope | Target | Position | Target uid/ppid | fd0_link | fd_list | fdinfo0 | io | schedstat | stat | status | syscall | task_syscall | wchan |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 0 | same-user | ancestor | 1000/752 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 0 | same-user | unrelated | 1000/752 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 0 | root-owned | ancestor | 0/756 | EACCES | EACCES | EACCES | EACCES | ok | ok | ok | EACCES | EACCES | ok |
| 0 | root-owned | unrelated | 0/756 | EACCES | EACCES | EACCES | EACCES | ok | ok | ok | EACCES | EACCES | ok |
| 0 | reparented | ancestor | 1000/1 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 0 | reparented | unrelated | 1000/1 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 0 | reparented-to-subreaper | ancestor | 1000/751 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 0 | reparented-to-subreaper | unrelated | 1000/751 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 1 | same-user | ancestor | 1000/1169 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 1 | same-user | unrelated | 1000/1169 | ok | ok | ok | ok | ok | ok | ok | EPERM | EPERM | ok |
| 1 | root-owned | ancestor | 0/1173 | EACCES | EACCES | EACCES | EACCES | ok | ok | ok | EACCES | EACCES | ok |
| 1 | root-owned | unrelated | 0/1173 | EACCES | EACCES | EACCES | EACCES | ok | ok | ok | EACCES | EACCES | ok |
| 1 | reparented | ancestor | 1000/1 | ok | ok | ok | ok | ok | ok | ok | EPERM | EPERM | ok |
| 1 | reparented | unrelated | 1000/1 | ok | ok | ok | ok | ok | ok | ok | EPERM | EPERM | ok |
| 1 | reparented-to-subreaper | ancestor | 1000/1168 | ok | ok | ok | ok | ok | ok | ok | ok | ok | ok |
| 1 | reparented-to-subreaper | unrelated | 1000/1168 | ok | ok | ok | ok | ok | ok | ok | EPERM | EPERM | ok |
| 2 | same-user | ancestor | 1000/1585 | ok | ok | ok | ok | ok | ok | ok | EPERM | EPERM | ok |
| 2 | same-user | unrelated | 1000/1585 | ok | ok | ok | ok | ok | ok | ok | EPERM | EPERM | ok |
| 2 | root-owned | ancestor | 0/1589 | EACCES | EACCES | EACCES | EACCES | ok | ok | ok | EACCES | EACCES | ok |
| 2 | root-owned | unrelated | 0/1589 | EACCES | EACCES | EACCES | EACCES | ok | ok | ok | EACCES | EACCES | ok |
| 2 | reparented | ancestor | 1000/1 | ok | ok | ok | ok | ok | ok | ok | EPERM | EPERM | ok |
| 2 | reparented | unrelated | 1000/1 | ok | ok | ok | ok | ok | ok | ok | EPERM | EPERM | ok |
| 2 | reparented-to-subreaper | ancestor | 1000/1584 | ok | ok | ok | ok | ok | ok | ok | EPERM | EPERM | ok |
| 2 | reparented-to-subreaper | unrelated | 1000/1584 | ok | ok | ok | ok | ok | ok | ok | EPERM | EPERM | ok |

Read across:

- **The premise holds for `syscall` only.** Scope 0 admits both positions;
  scope 1 admits the ancestor and refuses the unrelated reader (`EPERM`);
  scope 2 refuses both. Every other file the signals use (`wchan`, `stat`,
  `status`, `fd/`, `fdinfo/`, `io`, `schedstat`) reads at every scope from
  both positions: they need `PTRACE_MODE_READ`, and Yama gates only attaching.
- **An orphan loses the ancestor.** At scope 1 a reparented process refuses the
  prober too: it is no longer the prober's descendant. With the prober as its
  subreaper it is again, and reads.
- **Root-owned is refused at every scope and from both positions** (`EACCES`
  for `syscall`, `fd/`, `fdinfo/`, `io`), and `wchan` reads `0`; `stat`,
  `status` and `schedstat` stay readable.
- The root-owned target is root in the container's user namespace, which the
  kernel's checks treat as root against the probe's user; it is not root on the
  VM.

### Access on macos

| Scope | Target | Position | Target uid/ppid | bsdinfo | listfds | listthreads | rusage | taskallinfo |
|---|---|---|---|---|---|---|---|---|
| n/a | same-user | ancestor | 501/60300 | ok | ok | ok | ok | ok |
| n/a | same-user | unrelated | 501/60300 | ok | ok | ok | ok | ok |
| n/a | root-owned | ancestor | None/None | EPERM | EPERM | EPERM | EPERM | EPERM |
| n/a | root-owned | unrelated | None/None | EPERM | EPERM | EPERM | EPERM | EPERM |
| n/a | reparented | ancestor | 501/1 | ok | ok | ok | ok | ok |
| n/a | reparented | unrelated | 501/1 | ok | ok | ok | ok | ok |

macOS has no Yama: both positions read a same-user process, an orphan included,
and neither reads a root-owned one.

## 4. Cost per read

One observation of a foreground group of 1, 10 and 100 processes (`sleep`s in
one group), 50 times after 3 to warm up: the full read (`tcgetpgrp`,
`tcgetattr`, finding the group's members, then every member's S2 and S3), and
the members alone with the group already known. "Instrumented calls" counts the
probe's own syscalls (`probe/src/count.rs`); on Linux `strace` counted an
unrelated reader's first read between two marks, as a check.

### Cost on linux

| Group | Members | /proc entries | Full read µs (median, p90) | Instrumented calls | strace calls | Members only µs (median, p90) | Calls |
|---|---|---|---|---|---|---|---|
| 1 | 1 | 6 | 184, 235 | 69 | 87 | 88, 125 | 39 |
| 10 | 10 | 15 | 507, 559 | 456 | 494 | 418, 459 | 390 |
| 100 | 100 | 105 | 4231, 4365 | 4326 | 4556 | 3751, 3860 | 3900 |

### Cost on macos

| Group | Members | /proc entries | Full read µs (median, p90) | Instrumented calls | strace calls | Members only µs (median, p90) | Calls |
|---|---|---|---|---|---|---|---|
| 1 | 1 | 1 | 17, 54 | 11 | — | 5, 16 | 8 |
| 10 | 10 | 10 | 140, 162 | 83 | — | 112, 131 | 80 |
| 100 | 100 | 100 | 728, 1023 | 803 | — | 693, 973 | 800 |

- **Linux costs about 39 syscalls and 38 µs per member**, as the probe reads
  it: every thread's `stat`, `wchan` and `syscall`, and every descriptor's
  `readlink`. Finding the group walks `/proc` and reads each process's
  `stat`: the container had 6 to 105 entries, and each cost four syscalls and
  about 5 µs, so on a desktop with a thousand processes the walk alone is
  about 4,000 syscalls and 5 ms.
  The jar's own share, the `syscall` file per thread, is one read each.
- **macOS costs about 8 calls and 7 µs per member**, and one call finds the
  group (`proc_listpgrppids`).
- **`strace` counted 87, 494 and 4,556 against the instrumented 69, 456 and
  4,326**: the probe's estimate of two `getdents` per directory listing runs
  low.

## 5. What the jar would add

**Where each read belongs.** Only two of the reads need the jar. `tcgetattr`
needs the master or an open slave, and the jar holds the master; that is R4's
unbuilt half (item 6). `/proc/<pid>/task/<tid>/syscall` needs the reader to be
the target's ancestor at `ptrace_scope` 1 (Yama matrix). Everything else in
this report (`stat`, `status`, `wchan`, `fd/`, `fdinfo/`, `io` on Linux;
every `proc_info` call on macOS) an unrelated same-user process reads as well
as the jar does, so spyc can read them itself, by pid, when it has a decision
pending.

**The calls.**

- Both platforms: `rustix::termios::tcgetattr(&master)`, beside the
  `tcgetpgrp` the reaper already makes (`crates/spyc-jar/src/jar/life.rs:160`).
  No new dependency or feature.
- Linux, for the syscall: `std::fs` reads of `/proc/<pid>/task/<tid>/syscall`
  for each thread of each member, plus `readlink` of `/proc/<pid>/fd/<arg0>`
  for the read family and `/proc/<pid>/fdinfo/<epfd>` for `epoll_pwait`. No new
  dependency. Group membership needs a walk of `/proc` (cost table) or of
  `/proc/<pid>/task/<tid>/children` below the program, which this spike didn't
  measure.
- Linux, optional: `rustix::process::set_child_subreaper`, already in rustix's
  `process` feature. With it, a member whose parent exits is reparented to the
  jar, and stays readable at scope 1 (`reparented-to-subreaper` rows). It
  changes what the jar reaps, so it is a decision of its own.
- macOS: nothing beyond `tcgetattr`. If spyc rather than the jar reads S2m and
  S3, the `libproc` crate it already depends on (`Cargo.toml:246`) covers
  `proc_pidinfo` (`TaskAllInfo`, `ListThreads`, `ThreadInfo`, `ListFDs`) and
  `proc_pid_rusage`. `libproc` 0.15 implements `pidfdinfo` only for sockets
  (`libproc-0.15.0/src/libproc/net_info.rs:38`), so the vnode path of a
  descriptor and a kqueue's knotes would need a `#[repr(C)]` type implementing
  its `PIDFDInfo` trait, or `unsafe` FFI as the probe uses. The jar itself
  can't take the FFI route: it forbids `unsafe`.

**The raw facts a reply carries.** Named, so spyc needn't know the jar's
platform, and classified by nothing:

```text
terminal:  echo, icanon, isig, iexten, echonl, icrnl, ixon, opost (booleans),
           vmin, vtime (u8), foreground pgid (u32)
waits:     Linux only, one per thread of each foreground member:
           pid, tid, state (the stat letter), syscall number, arg0, arg1,
           arg0_is_pty (bool: arg0 names a descriptor whose target is this
           jar's slave or /dev/tty), epoll_has_pty (bool), or the errno
```

`arg0_is_pty` compares a `readlink` result with the slave's path: a fact about
identity, not a judgement. The jar parses no output to produce any of it.

**How it arrives.** As a capability in the `hello` ("How the protocol grows",
`docs/drafts/JAR_PROTOCOL.md:1043-1055`), say `terminal-state`, with an
ancillary request and its answer, so a jar or spyc that doesn't know them skips
them. spyc asks when it has a decision pending (output quiet, a scrape
matched); the jar keeps no history and starts no timer for it. Today's reaper
already wakes every 200 ms to read `tcgetpgrp`
(`crates/spyc-jar/src/jar/life.rs:21, 158-166, 193`); the request adds no
second loop.

**What it costs the jar.** One `ioctl` for the settings; on Linux about 39
syscalls and 38 µs per foreground member for the waits as the probe reads them
(threads, `fd/`, `fdinfo`), most of which spyc can read instead. The jar's
part, the syscall files alone, is one read per thread.

## 6. R4's terminal settings: unbuilt

R4 (`docs/drafts/JAR_PROTOCOL.md:124`) lists both "which process group is in
the foreground" and "what are the terminal settings" as v1. Only the first is
built: the reaper reads `tcgetpgrp` on the master (`crates/spyc-jar/src/jar/life.rs:158-166`),
`end` reads it to hang up the group (`crates/spyc-jar/src/held.rs:316`), and
the protocol carries it as `Foreground { pgid }` (`crates/spyc-jar/src/proto.rs:203-206`).
There is no `tcgetattr` on the master anywhere in `crates/spyc-jar/src`. The one
`tcgetattr` there, `crates/spyc-jar/src/attach.rs:69`, saves the attaching
terminal's own settings before putting it in raw mode; it never sees the PTY.
spyc's side reads none either: its only `tcgetattr` is its own standard input
(`src/terminal.rs:438`).

R4's "Why" also says the foreground group can be read only by the master's
holder. The measurements disagree: an unrelated same-user process read the same
group in every row, from the shell's `/proc/<pid>/stat` (`tpgid`, field 8) on
Linux and `proc_bsdinfo.e_tpgid` on macOS (the `fg` of each `unrelated` row
equals its `ancestor` row's, 333 row pairs on Linux, 108 on macOS). What it can't do is `tcgetpgrp` on the
slave, which answers `ENOTTY` to a process whose controlling terminal it isn't
(`obs.extra.tcgetpgrp_on_slave` in every `unrelated` row). Not fixed, as asked.

## 7. Go or no-go

| Signal | Linux | macOS |
|---|---|---|
| S1 `tcgetattr` | **Go**, in the jar: R4's unbuilt half. Echo off in canonical mode never fired wrongly. | **Go**, the same. |
| S2 syscall | **Go for the jar** at scope 0 and 1 (and for orphans only as subreaper); **no-go for spyc** at scope 1; **no-go** for anyone at scope 2. | No equivalent. |
| S2 `wchan` | **Go, from spyc**, at any scope, as "a thread asleep in a read on a terminal or a socket": telling the two apart takes the syscall, so the jar. | No equivalent. |
| S2m | — | **No-go.** Nothing names a wait; a kqueue's knotes show an event loop's interest, which this spike didn't build a rule on. |
| S3 | **Go as a veto**, at 2 ms per 100 ms. Root-owned `io` unreadable. | **Go as a veto**, with the threshold needed for idle timers. Root-owned unreadable. |
| S4 | Corroboration only. | Corroboration, and the only signal for an echo-on prompt. |

**What is left to the scrape on macOS:** every prompt that echoes, in
canonical mode (`[y/N]`, a username, a host key, `rm -i`, `input()`, a Go
`ReadString`), which R6 catches from S4 with a job in the foreground and only a
printed-then-sleeping `Continue? [y/N]` against it; every silent `read`, which
no signal here catches; and every raw-mode prompt (node's `readline`, `gum`),
which reads exactly like `vim`, `less` and `top` by every signal on macOS. On
Linux the raw-mode prompts are the scrape's too: their wait looks like any
full-screen program's.

## Calling `tcgetattr`

Use `rustix::termios::tcgetattr(&master)`. It needs nothing new: the jar
already depends on rustix with the `termios` feature
(`crates/spyc-jar/Cargo.toml:27`) and holds the master as an `OwnedFd`. It is
safe, takes `AsFd`, so no raw descriptor outlives its owner, and returns a
`Termios` whose flags are typed (`LocalModes::ECHO`, `ICANON`, `ISIG`) and
whose control characters index by `SpecialCodeIndex::VMIN`. On Linux it issues
`TCGETS2` and falls back to `TCGETS` where that is refused (rustix
`src/termios/tc.rs:10-30`); on macOS it is the C library's `tcgetattr`. The
probe reads every row's settings through it and again through libc's
`tcgetattr` on the same descriptor: `c_lflag` agreed in every row on both
platforms (`s1.libc_lflag_agrees`).

The alternatives buy nothing here:

- **nix** (`nix::sys::termios::tcgetattr`) is also safe and takes `AsFd`, but
  the jar has nix only on macOS and only with `socket` and `user`
  (`crates/spyc-jar/Cargo.toml:30-31`); it would need the `term` feature on
  both platforms to do what rustix already does.
- **termios** (`termios::tcgetattr(RawFd, &mut Termios)`) is a new dependency
  over libc, and takes a `RawFd`, giving up I/O safety.
- **libc** (`libc::tcgetattr`) is `unsafe`, which the jar forbids
  (`crates/spyc-jar/Cargo.toml:33-36`, `crates/spyc-jar/src/lib.rs:24`).

Three things about how, rather than with what:

- **Read the master, never a held slave.** The master reflects the slave's
  settings on both platforms (item 1, S1). The jar drops its slave once the
  program has it (`crates/spyc-jar/src/pty.rs:60-62`, where `start` consumes it); keeping one open
  would keep the slave alive past the program's exit and change what the pump
  sees at the end.
- **Send named flags, not raw words.** The bit values differ: `ICANON` is
  `0x2` on Linux and `0x100` on macOS (compare `s1.lflag` in
  `linux/1/s1-confirm/2s/ancestor` and `macos/n/a/s1-confirm/2s/ancestor`).
  A reply that carries `echo`, `icanon`, `isig`, `iexten`, `VMIN` and `VTIME`
  by name is still a raw fact, and spyc needn't know the jar's platform.
- **It costs one `ioctl`.** The cost table's single-member row includes it.

## Other unprivileged signals

- **The foreground group, by pid.** Both platforms publish each process's
  terminal foreground group: `tpgid` in `/proc/<pid>/stat`, `e_tpgid` in
  `proc_bsdinfo`. Every unrelated row's `fg` equals its ancestor row's (item 6).
- **The settings, by path.** A same-user process can open the slave by its
  path (`O_NOCTTY`, `O_NONBLOCK`) and `tcgetattr` it; every unrelated row's S1
  equals its ancestor row's on both platforms. spyc could read S1 without the
  jar, at the cost of briefly holding the slave open.
- **Stopped jobs, by controlling terminal.** A job stopped by `SIGTTIN` leaves
  the foreground to the shell, so no foreground signal sees it, but a scan for
  processes whose controlling terminal is the slave finds it in state `T`
  (`linux/1/ttin-job/2s/ancestor`, `macos/n/a/ttin-job/2s/ancestor`,
  `obs.session`). Nothing unprivileged says which signal stopped it, so a job
  stopped with `^z` reads the same.
- **Not measured:** the bytes typed and not yet read (`TIOCOUTQ` on the
  master), which would say whether the program is reading at all; the slave's
  access and modification times, which Linux moves on terminal reads and
  writes; and walking `/proc/<pid>/task/<tid>/children` below the program in
  place of the whole of `/proc`.

## Where the brief and the tree disagree, and the stop conditions

- **R4's "only the master's holder can"** read the foreground group
  (`docs/drafts/JAR_PROTOCOL.md:124`, `crates/spyc-jar/src/proto.rs:203-204`,
  `crates/spyc-jar/src/jar/life.rs:158-159`): the group reads by pid on both
  platforms (item 6).
- **The jar already has a timer on R4's other half.** The reaper reads
  `tcgetpgrp` every 200 ms (`crates/spyc-jar/src/jar/life.rs:21, 158-166, 193`).
  Decision 3 concerns the new signals, and the design adds none.
- **The Yama premise** holds for `/proc/<pid>/syscall` and for nothing else the
  signals use (item 3).
- **Stop conditions.** None applied: the master's settings reflect the slave's
  on both platforms (S1); the program descends from the prober as `pty.rs`
  arranges, the shell the prober's child and each job the shell's (`ppid` in
  every member, `target_info` in the Yama rows); nothing in the tree exposes
  terminal settings to spyc (item 6); and no unprivileged way to observe
  syscalls exists on macOS, so decision 1 stands.

## Not covered, and limits

- **Not covered:** `raw-gum` on Linux (gum not installed); `raw-fzf` on macOS (fzf not installed); `dd-direct` on macOS (not this platform).
- **Linux ran in a container**, rootless podman on a Fedora CoreOS VM, not a
  dedicated VM. `ptrace_scope` is the VM kernel's own setting, so the Yama rows
  are real; the root-owned target is root only inside the container's user
  namespace.
- **One run per case.** The three time points of a case are not independent
  trials, and counts are of rows.
- **The tab's shell is bash** (`3.2` on macOS, `5.2` on Linux), not the user's
  zsh; `read-zsh` runs zsh as a job. Versions are in each file's `meta`.
- **macOS's Python is `3.15`** and Linux's `3.11`, so the REPLs differ (PyREPL
  against readline); both turn canonical mode and echo off.

## Files

- `probe/`: the probe. `run` runs the corpus, `yama` the access matrix,
  `cost` the cost table; `observe`, `access` and `pty-child` are steps they
  spawn. Its dependencies: rustix with the jar's features, `anyhow`,
  `serde_json` for its own output, and `libc`, for the `tcgetattr` cross-check
  and macOS's `proc_info` calls, which the jar couldn't make (item 5).
- `fixtures/`: the local servers, the programs built to fool a rule, the Go
  reader and the `make` file.
- `run.sh`, `linux.sh`, `merge.py`, `Containerfile`: the rerun.
- `analyze.py` and its output `analysis.md`.
- `observation-linux.json`, `observation-macos.json`: the rows.
  `access-*.json`, `cost-*.json`: items 3 and 4.
