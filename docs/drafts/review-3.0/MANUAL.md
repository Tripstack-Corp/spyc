# spyc 3.0 review: the owner's manual checklist

Probes that need a real terminal emulator, a Linux login host, WSL, a laptop lid or `sudo`, so no agent could run them. Each takes a few minutes. Run every one in a **scratch** spyc unless it says otherwise: `HOME`, `XDG_RUNTIME_DIR`, `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `TMPDIR` in a fresh temporary directory, with every `SPYC_*` variable unset, so nothing touches your live session. Keep `XDG_RUNTIME_DIR` short (e.g. a link at `/tmp/m1`), since macOS limits a socket path to 104 bytes.

Pin: `444857a2`. Ordered by the severity of the finding each settles.

## 1. A program run twice after a restore with jars off (C1 = G2, high)

1. In a scratch spyc, open a tab running `sh -c 'echo $$ >> /tmp/c1-pids; exec cat'`.
2. `kill -9` that spyc from another terminal.
3. Put `[pane]` / `jars = false` in the scratch `.spycrc.toml`.
4. Run `spyc -r`.

**Expected at the pin:** two pids in `/tmp/c1-pids`, both alive (`ps -p`), one under a `spyc-jar`. Afterwards, end both jars with `kill`.

## 2. A stopped jar, and `^a R` on a `[detached]` tab (C2 high, C3 medium)

1. In a scratch spyc, `^a c` and run `sh`.
2. In another terminal, run `ps -o pid,ppid,command -A | grep 'spyc-jar sh'` and take the one whose parent is that spyc. `kill -STOP` it.
3. Type now and then for two minutes. **Expected:** nothing on screen says anything (C3).
4. Paste a few hundred kB of text. **Expected:** `[detached]` after about 40 s.
5. `kill -CONT` the jar. **Expected:** the tab stays detached (C2's medium half).
6. Press `^a R`. **Expected:** no question, and `ps` shows two `sh` programs in two jars (C2's high half).

End both jars afterwards.

## 3. `^d` with a long session name, on your real `TMPDIR` layout (D1, D2)

1. In a scratch spyc whose `TMPDIR` is left at macOS's default (`/var/folders/…`) but with `HOME` and the `XDG_*` variables scratch, open a `cat` tab.
2. `:name ABCDEFGHIJKLMNOPQRSTUVWXYZ12` (28 characters).
3. Press `^d`.

**Expected (D1):** a spyc under another name, offering to restore or discard jars no spyc owns. Answer **restore**, never discard. Then quit it and run `spyc -r`.

Repeat with a 40-character name. **Expected (D2):** `^d` fails, spyc exits to the shell, and the tab's program runs on with no owner until `spyc -r`.

## 4. `^\` and pastes in an attached session (D4)

1. `^d`, then `spyc attach` in Terminal.app.
2. Press `^\`. **Expected:** `[detached]`, where in spyc itself it would toggle the pane.
3. Repeat in Ghostty. **Expected:** the pane toggles.
4. In a shell, `printf 'a\x1cb\n'`, copy the line, and paste it into a Claude tab. **Expected:** `[detached]` and half a paste.

## 5. Idle wakeups on battery (B4)

1. With jars on, open ten tabs running `sleep 100000`.
2. `^d`, and wait a minute.
3. Run `sudo powermetrics --samplers tasks --show-process-energy -i 10000 -n 1 | grep -E 'spyc-jar|Name'`. **Expected:** each `spyc-jar` at about 50 to 100 idle wakeups a second.
4. Repeat attached.

Alternatively, add the "Idle Wake Ups" column under Activity Monitor → Energy. `spyc` itself should sit near zero beside them.

## 6. A failed upgrade on x86_64 Linux (A1, medium, frozen)

On an x86_64 glibc Linux machine, if you have one, run `python3 -I docs/drafts/review-3.0/evidence/A/probe_linux.py`, with `UA_S`, `UA_JAR` and `UA_DIFFER` set as in A1's command (a scratch directory, the pin's `spyc-jar`, and the jar built to differ). Copy the script out of the scratchpad first: it is pruned after a few days.

**Expected:** "JAR DIED … status -13" within 40 tries. It reproduced on aarch64 musl; macOS survives.

## 7. A stopped and continued spyc on Linux (B12)

On a Linux machine with jars on, open two jarred agent tabs. From another terminal, `kill -STOP <spyc pid>; sleep 1; kill -CONT <spyc pid>`.

**Expected by B12:** both tabs go `[detached]`, and are taken back or stay that way. Repeat on macOS: **expected,** nothing visible.

## 8. A detached session and the login (G10 = D9, H7)

**systemd-logind:**
1. On a Linux host where `loginctl show-user $USER -p KillUserProcesses` says yes (or with `KillUserProcesses=yes` in `/etc/systemd/logind.conf`), `ssh` in and start spyc with a `sleep 1000` tab.
2. `^d`, log out of every session, `ssh` in again, and run `spyc attach`. **Expected by the code:** "no session runs in a jar of its own".
3. Run `loginctl enable-linger $USER` and repeat. **Expected:** the session attaches.

**WSL2:**
1. `^d` a session with a `sleep 1000` tab.
2. Close every Windows Terminal tab on the distribution, and wait two minutes. `wsl -l -v` in PowerShell shows whether it stopped.
3. Open a new terminal and run `spyc attach`.

## 9. `/tmp/spyc-<uid>` planted by another user (E2)

On a Linux host with two logins:
1. As user B: `mkdir -m 777 /tmp/spyc-<uid of A>`.
2. As A, with neither `XDG_RUNTIME_DIR` nor `TMPDIR` set, start spyc, open a tab and press `^d`. **Expected:** the detach is refused.
3. As A, `env -u XDG_RUNTIME_DIR -u TMPDIR spyc attach`. **Expected at the pin:** it lists what B planted.

## 10. An agent ending spyc through its MCP socket (E1)

1. In a scratch spyc's Claude tab, ask the agent to run a `python3 -c` one-liner that sends `{"jsonrpc":"2.0","id":1,"method":"spyc/release"}`, framed with `Content-Length`, to `$SPYC_MCP_SOCK`.
2. **Expected:** Claude Code asks permission once. Once approved, spyc leaves the terminal and every tab runs on with no owner.

Do this in a scratch session only.

## 11. Ejecting a volume a jar sits in (B6)

1. Mount a disk image and browse into it in a column.
2. Open a shell tab there, `cd ~` in the tab, and move the column away.
3. Eject the image in Finder. **Expected:** "The disk wasn't ejected…", and `lsof +D /Volumes/<name>` names `spyc-jar`.

## 12. Release-side checks (F1, F2, F3; no build needed)

- **F1:** in GitHub settings, confirm "Tests (cargo test --all-targets)" stays required for admins on `main` (it was on 2026-11-24).
- **F2:** decide which commit publishes `spyc-jar` first, by hand (`RELEASE_ENGINEERING.md:263-265`), and whether an rc may publish it at all.
- **Homebrew:** after the first rc with jars, check `Formula/spyc.rb` in the tap installs both `spyc` and `spyc-jar`, and that `brew install --HEAD` does too.
- **F3:** until macOS runs in CI, run `make test` on the Mac at the exact tagged commit, and keep the log with the release. Run it on an idle machine and note the load: B3 found the jar tests red under load.

## Also worth a look (no finding)

- **Your S4 acceptance test:** attached at the desk, close the lid for ten minutes, then `ssh -t host spyc attach` from another machine. The desk's terminal should end, and show the shell as it was before spyc.
- **A leaked test process:** `ps -p 66123` is a `gated-jar` left by an `harness_tests::jar_start` run eight days ago (B10). It isn't one of your live jars; end it when convenient.
