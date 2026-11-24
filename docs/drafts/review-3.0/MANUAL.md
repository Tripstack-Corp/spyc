# spyc 3.0 review: the owner's manual checklist

Probes that need a real terminal emulator, a Linux login host, WSL, a laptop lid or `sudo`, so no agent could run them. Each takes a few minutes. Run every one in a **scratch** spyc unless it says otherwise: `HOME`, `XDG_RUNTIME_DIR`, `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `TMPDIR` in a fresh temporary directory, with every `SPYC_*` variable unset, so nothing touches your live session. Keep `XDG_RUNTIME_DIR` short (e.g. a link at `/tmp/m1`), since macOS limits a socket path to 104 bytes.

Pin: `444857a2`. Ordered by the severity of the finding each settles.

## 1. (Dropped) A program run twice after a restore with jars off (C1)

Shown by Unit C's red test and its refuter's run; not needed by hand.

## 2. (Dropped) A stopped jar, and `^a R` on a `[detached]` tab (C2, C3)

Shown by Unit C's probes on both platforms; not needed by hand.

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

## 6. (Moved) A failed upgrade on x86_64 Linux (A1)

Run under emulation: see [`ADDENDUM.md`](ADDENDUM.md), P3-6.

## 7. (Moved) A stopped and continued spyc on Linux (B12)

Run in the container: see [`ADDENDUM.md`](ADDENDUM.md), P1.

## 8. A detached session and the login (G10 = D9, H7)

**systemd-logind:**
1. On a Linux host where `loginctl show-user $USER -p KillUserProcesses` says yes (or with `KillUserProcesses=yes` in `/etc/systemd/logind.conf`), `ssh` in and start spyc with a `sleep 1000` tab.
2. `^d`, log out of every session, `ssh` in again, and run `spyc attach`. **Expected by the code:** "no session runs in a jar of its own".
3. Run `loginctl enable-linger $USER` and repeat. **Expected:** the session attaches.

**WSL2:**
1. `^d` a session with a `sleep 1000` tab.
2. Close every Windows Terminal tab on the distribution, and wait two minutes. `wsl -l -v` in PowerShell shows whether it stopped.
3. Open a new terminal and run `spyc attach`.

## 9. (Moved) `/tmp/spyc-<uid>` planted by another user (E2)

Run in a container with two users: see [`ADDENDUM.md`](ADDENDUM.md), P3-9.

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
