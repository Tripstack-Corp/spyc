## linux

Not covered: raw-gum (gum not installed).

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

### Rules on linux

| Rule | Scope | Position | False positives | False negatives | FP cases | FN cases |
|---|---|---|---|---|---|---|
| R1 echo off, canonical | 0 | ancestor | 0 of 60 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-fzf, go-read, ttin-job |
| R1 echo off, canonical | 0 | unrelated | 0 of 60 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-fzf, go-read, ttin-job |
| R1 echo off, canonical | 1 | ancestor | 0 of 60 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-fzf, go-read, ttin-job |
| R1 echo off, canonical | 1 | unrelated | 0 of 60 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-fzf, go-read, ttin-job |
| R1 echo off, canonical | 2 | ancestor | 0 of 60 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-fzf, go-read, ttin-job |
| R1 echo off, canonical | 2 | unrelated | 0 of 60 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-fzf, go-read, ttin-job |
| R4 S4 alone | 0 | ancestor | 17 of 60 | 15 of 51 | shell-prompt, vim, less, py-repl, cp-large, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, raw-fzf |
| R4 S4 alone | 0 | unrelated | 17 of 60 | 15 of 51 | shell-prompt, vim, less, py-repl, cp-large, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, raw-fzf |
| R4 S4 alone | 1 | ancestor | 16 of 60 | 15 of 51 | shell-prompt, vim, less, py-repl, cp-large, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, raw-fzf |
| R4 S4 alone | 1 | unrelated | 16 of 60 | 15 of 51 | shell-prompt, vim, less, py-repl, cp-large, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, raw-fzf |
| R4 S4 alone | 2 | ancestor | 17 of 60 | 15 of 51 | shell-prompt, vim, less, py-repl, cp-large, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, raw-fzf |
| R4 S4 alone | 2 | unrelated | 17 of 60 | 15 of 51 | shell-prompt, vim, less, py-repl, cp-large, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, raw-fzf |
| R4g S4, a job in the foreground, not vetoed | 0 | ancestor | 12 of 60 | 18 of 51 | vim, less, py-repl, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, raw-fzf, ttin-job |
| R4g S4, a job in the foreground, not vetoed | 0 | unrelated | 12 of 60 | 18 of 51 | vim, less, py-repl, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, raw-fzf, ttin-job |
| R4g S4, a job in the foreground, not vetoed | 1 | ancestor | 12 of 60 | 18 of 51 | vim, less, py-repl, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, raw-fzf, ttin-job |
| R4g S4, a job in the foreground, not vetoed | 1 | unrelated | 12 of 60 | 18 of 51 | vim, less, py-repl, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, raw-fzf, ttin-job |
| R4g S4, a job in the foreground, not vetoed | 2 | ancestor | 12 of 60 | 18 of 51 | vim, less, py-repl, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, raw-fzf, ttin-job |
| R4g S4, a job in the foreground, not vetoed | 2 | unrelated | 12 of 60 | 18 of 51 | vim, less, py-repl, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, raw-fzf, ttin-job |
| R6 R4g and canonical | 0 | ancestor | 3 of 60 | 21 of 51 | fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, node-question, raw-fzf, ttin-job |
| R6 R4g and canonical | 0 | unrelated | 3 of 60 | 21 of 51 | fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, node-question, raw-fzf, ttin-job |
| R6 R4g and canonical | 1 | ancestor | 3 of 60 | 21 of 51 | fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, node-question, raw-fzf, ttin-job |
| R6 R4g and canonical | 1 | unrelated | 3 of 60 | 21 of 51 | fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, node-question, raw-fzf, ttin-job |
| R6 R4g and canonical | 2 | ancestor | 3 of 60 | 21 of 51 | fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, node-question, raw-fzf, ttin-job |
| R6 R4g and canonical | 2 | unrelated | 3 of 60 | 21 of 51 | fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, node-question, raw-fzf, ttin-job |
| R2 member blocked reading the terminal | 0 | ancestor | 9 of 60 | 9 of 51 | less, fool-thread, devserver-idle | sudo, node-question, ttin-job |
| R2 member blocked reading the terminal | 0 | unrelated | 9 of 60 | 9 of 51 | less, fool-thread, devserver-idle | sudo, node-question, ttin-job |
| R2 member blocked reading the terminal | 1 | ancestor | 9 of 60 | 9 of 51 | less, fool-thread, devserver-idle | sudo, node-question, ttin-job |
| R2 member blocked reading the terminal | 1 | unrelated | 0 of 60 | 51 of 51 |  | sudo, ssh-hostkey, ssh-password, git-username, git-password, read-sh, read-bash, read-zsh, read-builtin, read-silent, rm-i, py-input, py-getpass, node-question, raw-fzf, go-read, ttin-job |
| R2 member blocked reading the terminal | 2 | ancestor | 0 of 60 | 51 of 51 |  | sudo, ssh-hostkey, ssh-password, git-username, git-password, read-sh, read-bash, read-zsh, read-builtin, read-silent, rm-i, py-input, py-getpass, node-question, raw-fzf, go-read, ttin-job |
| R2 member blocked reading the terminal | 2 | unrelated | 0 of 60 | 51 of 51 |  | sudo, ssh-hostkey, ssh-password, git-username, git-password, read-sh, read-bash, read-zsh, read-builtin, read-silent, rm-i, py-input, py-getpass, node-question, raw-fzf, go-read, ttin-job |
| R2b R2, or a wait set that is the terminal | 0 | ancestor | 24 of 60 | 3 of 51 | shell-prompt, vim, less, top, py-repl, cp-large, fool-thread, devserver-idle, fool-raw-poll | sudo |
| R2b R2, or a wait set that is the terminal | 0 | unrelated | 23 of 60 | 3 of 51 | shell-prompt, vim, less, top, py-repl, cp-large, fool-thread, devserver-idle | sudo |
| R2b R2, or a wait set that is the terminal | 1 | ancestor | 24 of 60 | 3 of 51 | shell-prompt, vim, less, top, py-repl, cp-large, fool-thread, devserver-idle, fool-raw-poll | sudo |
| R2b R2, or a wait set that is the terminal | 1 | unrelated | 0 of 60 | 51 of 51 |  | sudo, ssh-hostkey, ssh-password, git-username, git-password, read-sh, read-bash, read-zsh, read-builtin, read-silent, rm-i, py-input, py-getpass, node-question, raw-fzf, go-read, ttin-job |
| R2b R2, or a wait set that is the terminal | 2 | ancestor | 0 of 60 | 51 of 51 |  | sudo, ssh-hostkey, ssh-password, git-username, git-password, read-sh, read-bash, read-zsh, read-builtin, read-silent, rm-i, py-input, py-getpass, node-question, raw-fzf, go-read, ttin-job |
| R2b R2, or a wait set that is the terminal | 2 | unrelated | 0 of 60 | 51 of 51 |  | sudo, ssh-hostkey, ssh-password, git-username, git-password, read-sh, read-bash, read-zsh, read-builtin, read-silent, rm-i, py-input, py-getpass, node-question, raw-fzf, go-read, ttin-job |
| R3 R2b vetoed by S3 | 0 | ancestor | 20 of 60 | 3 of 51 | shell-prompt, vim, less, top, py-repl, cp-large, devserver-idle | sudo |
| R3 R2b vetoed by S3 | 0 | unrelated | 20 of 60 | 3 of 51 | shell-prompt, vim, less, top, py-repl, cp-large, devserver-idle | sudo |
| R3 R2b vetoed by S3 | 1 | ancestor | 19 of 60 | 3 of 51 | shell-prompt, vim, less, top, py-repl, cp-large, devserver-idle | sudo |
| R3 R2b vetoed by S3 | 1 | unrelated | 0 of 60 | 51 of 51 |  | sudo, ssh-hostkey, ssh-password, git-username, git-password, read-sh, read-bash, read-zsh, read-builtin, read-silent, rm-i, py-input, py-getpass, node-question, raw-fzf, go-read, ttin-job |
| R3 R2b vetoed by S3 | 2 | ancestor | 0 of 60 | 51 of 51 |  | sudo, ssh-hostkey, ssh-password, git-username, git-password, read-sh, read-bash, read-zsh, read-builtin, read-silent, rm-i, py-input, py-getpass, node-question, raw-fzf, go-read, ttin-job |
| R3 R2b vetoed by S3 | 2 | unrelated | 0 of 60 | 51 of 51 |  | sudo, ssh-hostkey, ssh-password, git-username, git-password, read-sh, read-bash, read-zsh, read-builtin, read-silent, rm-i, py-input, py-getpass, node-question, raw-fzf, go-read, ttin-job |
| R5 S4 and (R1 or R2b), not vetoed | 0 | ancestor | 14 of 60 | 15 of 51 | shell-prompt, vim, less, py-repl, cp-large | read-sh, read-bash, read-zsh, read-builtin, raw-fzf |
| R5 S4 and (R1 or R2b), not vetoed | 0 | unrelated | 14 of 60 | 15 of 51 | shell-prompt, vim, less, py-repl, cp-large | read-sh, read-bash, read-zsh, read-builtin, raw-fzf |
| R5 S4 and (R1 or R2b), not vetoed | 1 | ancestor | 13 of 60 | 15 of 51 | shell-prompt, vim, less, py-repl, cp-large | read-sh, read-bash, read-zsh, read-builtin, raw-fzf |
| R5 S4 and (R1 or R2b), not vetoed | 1 | unrelated | 0 of 60 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-fzf, go-read, ttin-job |
| R5 S4 and (R1 or R2b), not vetoed | 2 | ancestor | 0 of 60 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-fzf, go-read, ttin-job |
| R5 S4 and (R1 or R2b), not vetoed | 2 | unrelated | 0 of 60 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-fzf, go-read, ttin-job |
| R7 R2b and canonical, not vetoed | 0 | ancestor | 0 of 60 | 12 of 51 |  | sudo, node-question, raw-fzf, ttin-job |
| R7 R2b and canonical, not vetoed | 0 | unrelated | 0 of 60 | 12 of 51 |  | sudo, node-question, raw-fzf, ttin-job |
| R7 R2b and canonical, not vetoed | 1 | ancestor | 0 of 60 | 12 of 51 |  | sudo, node-question, raw-fzf, ttin-job |
| R7 R2b and canonical, not vetoed | 1 | unrelated | 0 of 60 | 51 of 51 |  | sudo, ssh-hostkey, ssh-password, git-username, git-password, read-sh, read-bash, read-zsh, read-builtin, read-silent, rm-i, py-input, py-getpass, node-question, raw-fzf, go-read, ttin-job |
| R7 R2b and canonical, not vetoed | 2 | ancestor | 0 of 60 | 51 of 51 |  | sudo, ssh-hostkey, ssh-password, git-username, git-password, read-sh, read-bash, read-zsh, read-builtin, read-silent, rm-i, py-input, py-getpass, node-question, raw-fzf, go-read, ttin-job |
| R7 R2b and canonical, not vetoed | 2 | unrelated | 0 of 60 | 51 of 51 |  | sudo, ssh-hostkey, ssh-password, git-username, git-password, read-sh, read-bash, read-zsh, read-builtin, read-silent, rm-i, py-input, py-getpass, node-question, raw-fzf, go-read, ttin-job |
| R2w a thread asleep in wait_woken | 0 | ancestor | 12 of 60 | 9 of 51 | less, recv-stall, fool-thread, devserver-idle | sudo, node-question, ttin-job |
| R2w a thread asleep in wait_woken | 0 | unrelated | 12 of 60 | 9 of 51 | less, recv-stall, fool-thread, devserver-idle | sudo, node-question, ttin-job |
| R2w a thread asleep in wait_woken | 1 | ancestor | 12 of 60 | 9 of 51 | less, recv-stall, fool-thread, devserver-idle | sudo, node-question, ttin-job |
| R2w a thread asleep in wait_woken | 1 | unrelated | 12 of 60 | 9 of 51 | less, recv-stall, fool-thread, devserver-idle | sudo, node-question, ttin-job |
| R2w a thread asleep in wait_woken | 2 | ancestor | 12 of 60 | 9 of 51 | less, recv-stall, fool-thread, devserver-idle | sudo, node-question, ttin-job |
| R2w a thread asleep in wait_woken | 2 | unrelated | 12 of 60 | 9 of 51 | less, recv-stall, fool-thread, devserver-idle | sudo, node-question, ttin-job |
| R7w R2w and canonical, not vetoed | 0 | ancestor | 3 of 60 | 12 of 51 | recv-stall | sudo, node-question, raw-fzf, ttin-job |
| R7w R2w and canonical, not vetoed | 0 | unrelated | 3 of 60 | 12 of 51 | recv-stall | sudo, node-question, raw-fzf, ttin-job |
| R7w R2w and canonical, not vetoed | 1 | ancestor | 3 of 60 | 12 of 51 | recv-stall | sudo, node-question, raw-fzf, ttin-job |
| R7w R2w and canonical, not vetoed | 1 | unrelated | 3 of 60 | 12 of 51 | recv-stall | sudo, node-question, raw-fzf, ttin-job |
| R7w R2w and canonical, not vetoed | 2 | ancestor | 3 of 60 | 12 of 51 | recv-stall | sudo, node-question, raw-fzf, ttin-job |
| R7w R2w and canonical, not vetoed | 2 | unrelated | 3 of 60 | 12 of 51 | recv-stall | sudo, node-question, raw-fzf, ttin-job |
| R8 R7w or R1 | 0 | ancestor | 3 of 60 | 9 of 51 | recv-stall | node-question, raw-fzf, ttin-job |
| R8 R7w or R1 | 0 | unrelated | 3 of 60 | 9 of 51 | recv-stall | node-question, raw-fzf, ttin-job |
| R8 R7w or R1 | 1 | ancestor | 3 of 60 | 9 of 51 | recv-stall | node-question, raw-fzf, ttin-job |
| R8 R7w or R1 | 1 | unrelated | 3 of 60 | 9 of 51 | recv-stall | node-question, raw-fzf, ttin-job |
| R8 R7w or R1 | 2 | ancestor | 3 of 60 | 9 of 51 | recv-stall | node-question, raw-fzf, ttin-job |
| R8 R7w or R1 | 2 | unrelated | 3 of 60 | 9 of 51 | recv-stall | node-question, raw-fzf, ttin-job |
| R9 R7 or R1 | 0 | ancestor | 0 of 60 | 9 of 51 |  | node-question, raw-fzf, ttin-job |
| R9 R7 or R1 | 0 | unrelated | 0 of 60 | 9 of 51 |  | node-question, raw-fzf, ttin-job |
| R9 R7 or R1 | 1 | ancestor | 0 of 60 | 9 of 51 |  | node-question, raw-fzf, ttin-job |
| R9 R7 or R1 | 1 | unrelated | 0 of 60 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-fzf, go-read, ttin-job |
| R9 R7 or R1 | 2 | ancestor | 0 of 60 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-fzf, go-read, ttin-job |
| R9 R7 or R1 | 2 | unrelated | 0 of 60 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-fzf, go-read, ttin-job |

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

### Cost on linux

| Group | Members | /proc entries | Full read µs (median, p90) | Instrumented calls | strace calls | Members only µs (median, p90) | Calls |
|---|---|---|---|---|---|---|---|
| 1 | 1 | 6 | 184, 235 | 69 | 87 | 88, 125 | 39 |
| 10 | 10 | 15 | 507, 559 | 456 | 494 | 418, 459 | 390 |
| 100 | 100 | 105 | 4231, 4365 | 4326 | 4556 | 3751, 3860 | 3900 |

## macos

Not covered: raw-fzf (fzf not installed); dd-direct (not this platform).

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

### Rules on macos

| Rule | Scope | Position | False positives | False negatives | FP cases | FN cases |
|---|---|---|---|---|---|---|
| R1 echo off, canonical | n/a | ancestor | 0 of 57 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-gum, go-read, ttin-job |
| R1 echo off, canonical | n/a | unrelated | 0 of 57 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-gum, go-read, ttin-job |
| R4 S4 alone | n/a | ancestor | 19 of 57 | 12 of 51 | shell-prompt, vim, less, top, py-repl, cp-large, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin |
| R4 S4 alone | n/a | unrelated | 19 of 57 | 12 of 51 | shell-prompt, vim, less, top, py-repl, cp-large, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin |
| R4g S4, a job in the foreground, not vetoed | n/a | ancestor | 15 of 57 | 15 of 51 | vim, less, top, py-repl, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, ttin-job |
| R4g S4, a job in the foreground, not vetoed | n/a | unrelated | 15 of 57 | 15 of 51 | vim, less, top, py-repl, fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, ttin-job |
| R6 R4g and canonical | n/a | ancestor | 3 of 57 | 21 of 51 | fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, node-question, raw-gum, ttin-job |
| R6 R4g and canonical | n/a | unrelated | 3 of 57 | 21 of 51 | fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, node-question, raw-gum, ttin-job |
| R5m S4 and R1, not vetoed | n/a | ancestor | 0 of 57 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-gum, go-read, ttin-job |
| R5m S4 and R1, not vetoed | n/a | unrelated | 0 of 57 | 36 of 51 |  | ssh-hostkey, git-username, read-sh, read-bash, read-zsh, read-builtin, rm-i, py-input, node-question, raw-gum, go-read, ttin-job |
| R8m R6 or R1 | n/a | ancestor | 3 of 57 | 21 of 51 | fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, node-question, raw-gum, ttin-job |
| R8m R6 or R1 | n/a | unrelated | 3 of 57 | 21 of 51 | fool-prompt-sleep | read-sh, read-bash, read-zsh, read-builtin, node-question, raw-gum, ttin-job |

### Access on macos

| Scope | Target | Position | Target uid/ppid | bsdinfo | listfds | listthreads | rusage | taskallinfo |
|---|---|---|---|---|---|---|---|---|
| n/a | same-user | ancestor | 501/60300 | ok | ok | ok | ok | ok |
| n/a | same-user | unrelated | 501/60300 | ok | ok | ok | ok | ok |
| n/a | root-owned | ancestor | None/None | EPERM | EPERM | EPERM | EPERM | EPERM |
| n/a | root-owned | unrelated | None/None | EPERM | EPERM | EPERM | EPERM | EPERM |
| n/a | reparented | ancestor | 501/1 | ok | ok | ok | ok | ok |
| n/a | reparented | unrelated | 501/1 | ok | ok | ok | ok | ok |

### Cost on macos

| Group | Members | /proc entries | Full read µs (median, p90) | Instrumented calls | strace calls | Members only µs (median, p90) | Calls |
|---|---|---|---|---|---|---|---|
| 1 | 1 | 1 | 17, 54 | 11 | — | 5, 16 | 8 |
| 10 | 10 | 10 | 140, 162 | 83 | — | 112, 131 | 80 |
| 100 | 100 | 100 | 728, 1023 | 803 | — | 693, 973 | 800 |

