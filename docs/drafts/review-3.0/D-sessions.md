# Unit D. Sessions: detach, attach, handover, moves

**SHA:** `444857a2` (main, unmoved). **Platform used:** macOS (Apple silicon, Darwin 25.6), the pin's debug binaries at `~/.cache/spyc/target/3.0-9f8124c5/debug/`. Linux not used for this unit (no probe here needs it; H7 needs systemd-logind or WSL, which the Alpine container lacks: see the manual checklist).

**Probes:** `docs/drafts/review-3.0/evidence/D/` (`ud.py` helper, `p_attach.py`, `p_detach.py`, `p_live.py`). Each ran with `HOME`, `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `TMPDIR` in that directory, `XDG_RUNTIME_DIR=/tmp/udD` (a short link to `unit-d/rt`), and `SPYC_CONTEXT`, `SPYC_JAR_SOCKET`, `SPYC_MCP_SOCK`, `SPYC_PANE_ID`, `SSH_AUTH_SOCK` unset. The owner's socket directories (`$TMPDIR/spyc/sessions`: 2 entries, `jars`: 28) were counted before and after each probe and did not change. No worktree was made; no code was written.

## Verdict

No blocker, and nothing in this unit is frozen: every finding is in spyc or in the attach client that spyc links, so each is a patch release. Read against kill -9, the orders of writes and releases hold up: on every path I traced, a crash leaves the jars running and a session file that names them, and a second spyc that tries to take a jar another holds is refused and starts nothing, so I found no instant where a program starts twice. The worst a crash leaves is a session listed twice, or tabs with no owner until \`spyc -r\`. The defects are elsewhere. A session name of 28 to 35 characters on the owner's Mac makes \`^d\` hand the session to a spyc that doesn't recognize its own jar: the session isn't restored, the agents run with no owner, and the new spyc offers to discard them (D1, high, shown by a probe). A longer name makes \`^d\` drop the user to the shell with every tab detached (D2). \`spyc attach\` sends SIGTERM to a pid it read before its prompt (V6, D3, shown). \`^\` in the attach client cuts a paste in half and can't reach spyc on a terminal without the kitty protocol (H6, D4, shown). \`^d\` and \`:upgrade\` lose each tab's last status report, the thing S5 carries across moves (D5). H8's premise is false: spyc never gives a jar a new token, though the protocol says a move does (D7).

## Coverage ledger

| File | Read |
|---|---|
| \`src/pane/jar/session.rs\` | in full |
| \`src/pane/jar/attach.rs\` | in full |
| \`src/pane/jar/shutdown.rs\` | in full |
| \`src/pane/ssh_agent.rs\` | in full |
| \`src/app/detach.rs\` | in full |
| \`src/app/project_move.rs\` | in full |
| \`src/app/session.rs\` | production half (1–926) in full except about 50 lines of \`saved_tabs\` (325–375) skimmed; tests (927–1882) not read |
| \`src/app/session_restore.rs\` | 1–560 read (293–365 skimmed); 560–814 and the tests not read |
| \`src/app/projects.rs\` | skimmed (function list, \`forget_project\`, \`reopen_project\`) |
| \`src/app/events.rs\` | production half in full; tests not read |
| \`src/state/sessions/mod.rs\` | save, load, prune and \`SavedTab\` in full; the agent-session finders (about 400–640) skimmed |
| \`src/state/sessions/manifest.rs\` | in full |
| \`src/state/sessions/tests.rs\` | not read |
| \`src/state/event_log.rs\` | production half in full; tests not read |
| \`src/terminal.rs\` | production half (1–730) in full; tests not read |
| \`src/terminal/image_probe.rs\` | in full |
| \`src/terminal_probe_tests.rs\` | skimmed (structure and test names) |
| \`tests/session_jar.rs\` | in full |
| \`tests/jar_attach.rs\` | skimmed (helpers and test names) |
| \`tests/jar_adopt.rs\` | 780–960 (the \`:detach project\` test) read; the rest searched for \`^d\` and \`:upgrade\` cases |
| \`docs/features/projects.md\` → "Detaching and attaching" | in full |
| \`docs/features/pane.md\` 360–372, \`docs/features/shell.md\` 170–185, 276–282, \`docs/KEYBINDINGS.md\` 333 | read |
| Read for context, outside the unit | \`crates/spyc-jar/src/attach.rs\` (in full), \`crates/spyc-jar/src/cli.rs\` 1–370, \`src/pane/jar/place.rs\`, \`tokens.rs\` (in full), \`src/pane/jar/mod.rs\` 150–380, \`src/lib.rs\` 380–560, \`src/app/proc.rs\` 396–482, \`src/fs/atomic.rs\`, \`src/state/session_names.rs\`, \`src/app/run.rs\` 445–560 |

## The order of each handover and move

Each list is what the code does, in order; "crash" means kill -9 of the spyc named. "Durable" means \`save_session_durably\` (\`write_durable\`: fsync of the file and its directory); "atomic" means \`save_session\` (\`write_atomic\`, no fsync).

**\`^d\` in a plain spyc, the lazy entry** (\`src/app/detach.rs:83-107\`, \`src/lib.rs:532-543\`, \`src/app/detach.rs:175-203\`, \`src/pane/jar/session.rs:220-282\`).
1. The loop ends (\`should_quit\`); \`finish\` removes the MCP socket (\`lib.rs:533\`) and restores the terminal.
2. \`close_event_log\`, then \`save_released\`: the manifest with \`pid: None\`, **atomic**, its error printed and ignored (\`detach.rs:227-234\`).
3. \`session::start\`: make \`sessions/\`, check the socket's state, new token, \`tokens::save\` (truncate and write, no fsync), spawn \`spyc-jar\` with \`hold\` 30 s (\`Begin::OnAttach\`), write its start on stdin, poll every 20 ms until the lock is held.
4. \`release_jars\`: sever every tab's and task's connection, remove context files, staging, the capture spill directory (\`detach.rs:238-252\`). **This runs whether step 3 worked or not** (\`detach.rs:193\` before \`started.with_context(...)?\` at 194).
5. \`exec spyc attach --force NAME\`; the client connects; the jar starts spyc; that spyc links to its own jar (\`session.rs:146-163\`), restores the manifest of its name with no running spyc (\`detach.rs:340-353\`) and adopts each tab's jar.

Crash windows: before 2, the jars run with no owner and the last autosave names them (\`spyc -r\`). Between 2 and 4, the same with a fresh manifest. Between 3 and 5, the session jar holds an unstarted spyc for 30 s and then ends; the tabs wait for \`spyc -r\`. During 5, a crash of the new spyc before it autosaves leaves the \`pid: None\` file for \`spyc -r\`. Nothing is started twice; nothing loses a token (one token per jar, never rotated: D7).

**\`:upgrade\` in spyc's own jar** (\`detach.rs:109-135\`, \`208-222\`): MCP socket removed, terminal restored (\`lib.rs:533-538\`), \`save_released\` (atomic, error ignored), \`release_jars\`, \`exec\` of the installed binary with the same arguments. A failed \`exec\` returns an error and spyc exits, so the session jar's program has exited and every tab waits for \`spyc -r\`. A new binary that crashes at start does the same. Nothing durable is needed for kill -9 (the page cache survives the process); a failed save is D6.

**\`spyc attach\` taking over** (\`src/pane/jar/attach.rs:56-96\`, \`335-364\`): list sessions (describes each, reading \`owner_pid\`), maybe prompt, load the token, point \`<NAME>.session\` at this terminal's SSH agent, SIGTERM the old client's pid, poll until the jar's owner is no longer that pid (20 s), maybe offer \`:upgrade\`, install the signal restore, attach. No durable writes. The pid is not read again after the prompt: D3.

**\`:detach project\` and \`:detach tab\`** (\`src/app/project_move.rs:72-100\`, \`302-338\`): pick a name, build the moved manifest (claims and last reports carried), **durable** save of the new session (stop if it fails: SPYC-TRAP \`move-saved-before-release\`), switch away, sever the project's (or tab's) jars, \`panes_moved\`, drop the project or close the tab, **atomic** autosave of this session without it, then a worker starts the session jar with \`Begin::Now\`; its spyc restores the file by name and adopts. Crash of the sender between the durable save and the autosave: both files name the same jars; restoring both gives one owner and one \`[detached]\` tab (a duplicate, not a second program). Crash after the autosave and before the worker starts the jar: a session file nobody runs; \`spyc attach NAME\` finds nothing; \`spyc -r\` lists it. Crash of the receiver mid-restore: its file still says \`pid: None\`; \`spyc -r\`.

**\`:attach project\` and \`:attach tab\`** (\`project_move.rs:509-592\`, \`609-729\`, \`748-772\`): a worker asks the giver's spyc, by the pid its jar describes, to release; the giver checks nothing would end, **durable** save of its whole session with \`pid: None\` and last reports, severs every jar, marks itself released and quits (its teardown signals nothing: \`run.rs:540-545\`). The receiver loads the file by id, restores each project out of view (adopting the jars), takes claims and tasks, **durable** save of its own session, then removes the giver's file (only if every project opened, for \`:attach project\`). Crash of the giver after its save, before the reply: the receiver reports an error, the giver's file says \`pid: None\`, the jars wait for \`spyc -r\`. Crash of the receiver before its own save: the giver's file still names everything. Crash between the receiver's save and the remove: the session appears twice, as above.

## Findings


### D1. A session name of 28 to 35 characters turns \`^d\` into a session that doesn't come back

- **Severity:** high. **Frozen:** no. **Lens:** correctness.
- **Sites:** \`src/pane/jar/session.rs:152-155\` (\`link_own\` canonicalizes the socket's directory and keeps the canonical path), \`session.rs:156\` and \`169\` (connects at that canonical path), \`session.rs:231\` (\`start\` binds at the path as given), \`src/app/state/dispatch.rs:440-447\` and \`src/state/session_names.rs\` \`normalize\` (\`:name\` takes any length), \`src/app/project_move.rs:812-818\` (the fallback name, up to 29 characters).
- **Claim:** spyc binds a session's jar at \`$TMPDIR/spyc/sessions/NAME.sock\` but the spyc inside connects to it by the canonical path, which on macOS is 8 bytes longer (\`/var\` is \`/private/var\`), so for names whose canonical path passes the 103-byte limit and whose given path doesn't, the spyc in the session jar fails to link, runs as a plain spyc under a new name, restores nothing, and offers to discard the session's still-running tabs.
- **Evidence (command and output):** \`unit-d/p_len.py\`, run twice under \`XDG_RUNTIME_DIR=/tmp/udF\` (a real directory whose canonical path is \`/private/tmp/udF\`, the same 8-byte growth as macOS's \`TMPDIR\`): a spyc with a \`cat\` tab, \`:name\` set, then \`^a Space d\`.
  - name of 60: \`given path len 88 canonical len 96\` → \`session restored\`, \`sh: taken back from its jar, still running\`, the tab's jar owned by the new spyc.
  - name of 70: \`given path len 98 canonical len 106\` → the session jar runs spyc pid 90198, whose event log is \`JUNIPER_NUTMEG.log\` (a fresh name), the tab's jar \`owner_pid: ''\` with its program alive, and the screen shows the offer about jars no spyc owns (\`restore\`, \`discard\`). An earlier run of the same shape (\`p_live.py\`, by the symlink \`/tmp/udD\`) had its next keystrokes, \`:activity dump\`, answer that offer, and its log reads \`jars: ending 1 no spyc owned\`: the tab's program was ended.
  - On the owner's Mac (\`TMPDIR=/var/folders/69/…/T/\`) the given prefix \`…/T/spyc/sessions/\` is 63 bytes and the canonical 71, so names of 28 to 35 characters land in the gap (computed in the REPL; 36 and up is D2).
- **Consequence:** a user who renamed the session to, say, \`SPYC_REVIEW_3_0_RELEASE_PREP\` presses \`^d\` and, on \`spyc attach\`, finds a different, empty spyc that offers to end "jars no spyc owned"; answering discard ends every agent. Choosing restore or quitting and running \`spyc -r\` recovers, since \`save_released\` left the manifest. \`spyc attach NAME\` reaches the impostor too.
- **Status:** confirmed. The in-repo \`^d\` tests (\`tests/jar_adopt.rs\`) use short names under \`/tmp\`, so they never reach the gap.
- **Fix direction:** connect at the path as given and canonicalize only for the comparison, and refuse a \`:name\` whose session socket would not fit.

### D2. A longer name fails \`^d\` after spyc has let every jar go

- **Severity:** medium. **Frozen:** no. **Lens:** reliability.
- **Sites:** \`src/app/detach.rs:186-196\` (\`release_jars\` at 193 runs before \`started\`'s error is returned at 194), \`src/pane/jar/session.rs:231\` and \`271-280\` (bind fails, the token is forgotten, the lock file stays), \`src/lib.rs:539-543\` (the loop has already ended).
- **Claim:** with a name over 35 characters on macOS (69 on a typical Linux \`/run/user/1000\`), the session jar can't bind, and \`hand_over\` severs every tab anyway and exits to the shell with an error.
- **Evidence (command and output):** \`unit-d/p_detach.py\` with an 80-character name: spyc printed \`Error: AAAA…'s tabs run on in their jars: \`spyc -r\` takes them back / Caused by: AAAA…'s jar didn't start\` and exited (status 256); the tab's jar then described \`owner: none for 0 s\` with its program alive; \`sessions/\` held only \`AAAA….sock.lock\`.
- **Consequence:** \`^d\` ends the visible spyc; agents keep running but lose spyc's tools until \`spyc -r\`. A stale lock file is left in \`sessions/\`. \`:upgrade\` from a plain spyc takes the same path (\`detach.rs:133\`).
- **Status:** confirmed.
- **Fix direction:** check that the session's socket fits, and start its jar, before the loop ends; stay in the loop when it can't start.

### D3. \`spyc attach\` signals a pid it read before its prompt (V6)

- **Severity:** medium. **Frozen:** no. **Lens:** correctness, security.
- **Sites:** \`src/pane/jar/attach.rs:64\` (\`running\` reads each session's \`owner_pid\` through \`client_of\`, 261-272), \`67-73\` (the "Attach to which?" prompt, unbounded), \`78-79\` and \`335-349\` (\`take_over\` sends SIGTERM to that pid, no second look), \`350-363\` (any other owner counts as success).
- **Claim:** the takeover kills whatever process holds the number the session's client had when the list was read, and then fails when another client has attached meanwhile.
- **Evidence (command and output):** \`unit-d/p_attach.py\`: client C1 (pid 42059) attached to \`ALPHA\`; \`spyc attach\` listed \`1  ALPHA  (attached elsewhere)\`; C1 left and C2 (pid 42069) attached; answering \`1\` printed \`spyc: ALPHA is attached elsewhere (pid 42059); taking it over\`, then the fixed reset, then \`Error: refused: the jar already has an owner\`. The SIGTERM went to 42059, by then gone (\`ESRCH\` is taken as success at 343). Had the number been reused, a process of the user's would have been sent SIGTERM; C2 kept the session.
- **Other places a pid from \`describe\` is used later** (\`rg -n 'program_pid|owner_pid' src/pane/jar src/app\`): \`src/pane/jar/shutdown.rs:39\`→\`53\` (\`ask_shutdown(pid)\` after a \`[y/N]\` prompt: a reused pid that is another spyc's would shut down that spyc's session through \`mcp-<pid>.sock\`); \`attach.rs:141\`→\`148\`, \`186\` (\`ask_behind\`, \`ask_upgrade\`); \`src/app/project_move.rs:793-796\` (\`ask_release\`, no prompt). These ask an MCP socket named by the pid rather than signal; the pid is the jar's own child, so it goes stale only when the session's spyc exits in between. Pid signalling elsewhere (the orphan sweep) is unit E's.
- **Consequence:** rare, but the wrong process gets SIGTERM, and a takeover raced by another attach fails after resetting the terminal.
- **Status:** confirmed.
- **Fix direction:** describe the jar again after the prompt and check the pid's start time, as \`Manifest::running\` does, before signalling.

### D4. \`^\` in the attach client cuts a paste and never reaches spyc without the kitty protocol (H6)

- **Severity:** medium. **Frozen:** no (the client runs in the \`spyc\` process, \`src/pane/jar/attach.rs:95\`). **Lens:** correctness, docs.
- **Sites:** \`crates/spyc-jar/src/attach.rs:58-59\`, \`218-245\` (\`relay_input\` sends the bytes before the first \`0x1c\`, drops the rest, detaches); \`docs/drafts/JAR_PRODUCTION_PLAN.md\` S4 ("so the attach client's \`^\` and its fixed reset are only the fallback, and \`^\` can reach a program inside spyc"); spyc's own \`^\` binding (\`docs/features/pane.md:19\`).
- **Claim:** any \`0x1c\` in a read, a pasted one included, detaches, and on a terminal that sends \`^\` as \`0x1c\` (no kitty keyboard protocol) spyc and its programs can never receive \`^\` through \`spyc attach\`.
- **Evidence (command and output):** \`unit-d/p_attach.py\`, a jar running \`stty raw -echo; exec cat > alpha.out\`: writing \`hello\x1cworld\` in one write gave the program \`hello\` and the client \`[detached]\`; a bracketed paste \`\x1b[200~paste-a\x1cpaste-b\x1b[201~\` gave the program \`\x1b[200~paste-a\` (no end marker) and detached.
- **Consequence:** a paste holding the byte (rare in text, possible in binary or copied terminal output) leaves the program in bracketed-paste mode with half the paste; in Terminal.app, \`^\` (spyc's pane toggle) detaches. The plan's sentence is true only on a terminal with the kitty protocol, where spyc's push makes \`^\` a CSI-u sequence. \`docs/features/pane.md:372\` does say \`^\` ends an attach.
- **Status:** confirmed (the paste); the pane-toggle reading is from the code and needs a real terminal (manual item 3).
- **Fix direction:** detach only on a read that is \`^\` alone, and say in the docs which terminals pass \`^\` through.

### D5. \`^d\` and \`:upgrade\` drop each tab's last status report

- **Severity:** medium. **Frozen:** no. **Lens:** correctness.
- **Sites:** \`src/app/detach.rs:227-234\` (\`save_released\` saves \`build_manifest\` without \`carry_live\`), \`src/app/session.rs:428\` (\`build_manifest\` writes \`live: None\`), \`src/app/project_move.rs:193-204\` (\`carry_live\`, called by \`session_of\`, \`release_session\` and \`take_*\`, never by \`save_released\`), \`project_move.rs:836-850\` and \`src/app/session_restore.rs:464\` (a restored tab's status comes only from \`SavedTab::live\`).
- **Claim:** after the first \`^d\` and after every \`:upgrade\`, a tab whose agent is blocked on a prompt comes back with no status, and the attention roll-up says nothing needs the user.
- **Evidence (lines read and reasoning):** \`live\` is set only by \`carry_live\`; \`save_released\` doesn't call it; \`land_carried_status\` returns at once with no \`carried_live\`. S5 carries the report across moves because "a Claude blocked on a prompt reports nothing until it's answered" (\`project_move.rs:12-14\`), and \`^d\` and \`:upgrade\` go through the same manifest. A probe (\`p_live.py\`) to show it end to end didn't establish the "before" state (a scratch \`--report-status blocked\` didn't show as \`dot=blocked\`), so the evidence is the code.
- **Consequence:** the owner's S4 test ("\`^d\` with agents running … its agents mid-turn") passes while a waiting agent's dot goes blank; \`docs/features/projects.md:262-264\` names pagers and \`/\` as what a handover loses, not this.
- **Status:** confirmed (lines read).
- **Fix direction:** call \`carry_live\` in \`save_released\`.

### D6. \`^d\` and \`:upgrade\` let the jars go after a save that may have failed

- **Severity:** low. **Frozen:** no. **Lens:** reliability.
- **Sites:** \`src/app/detach.rs:227-234\` (atomic save, error printed and ignored), \`182-193\`, \`211-212\` (\`release_jars\` follows regardless), \`340-350\` (the new spyc finds no manifest and restores nothing).
- **Claim:** unlike the moves (SPYC-TRAP \`move-saved-before-release\`), the handover and the upgrade go on when the save fails, so the next spyc starts empty beside ownerless jars.
- **Evidence (lines read):** as above. A kill -9 doesn't hit this (the page cache survives); a full disk or unwritable state directory does.
- **Consequence:** the session doesn't come back; the new spyc offers the jars as ones no spyc owns.
- **Status:** confirmed (lines read).
- **Fix direction:** save durably and stay when it fails, as the move paths do.

### D7. No jar is ever given a new token; the protocol says a move gives one (H8)

- **Severity:** informational. **Frozen:** no (the \`release\` message exists in version 1; spyc just doesn't send it). **Lens:** docs.
- **Sites:** \`docs/drafts/JAR_PROTOCOL.md\` "Threat model" ("A project move hands each jar a new token (R22), so the spyc it left can't come back in"), \`docs/drafts/JAR_PRODUCTION_PLAN.md\` S5 ("it keeps each old token until the receiving spyc confirms, which tries the new token, then the old"); code: \`rg -n 'Msg::Release' src crates\` finds it only in \`crates/spyc-jar/src/proto.rs\` and \`jar/conn.rs\`; \`tokens::save\` is called only at a jar's start (\`src/pane/jar/mod.rs:469-470\`, \`session.rs:244-245\`).
- **Claim:** moves and handovers keep each jar's first token, read from the one \`jar-tokens/\` directory both spycs share, so the sentences above describe nothing the code does.
- **Evidence (command and lines read):** the search above.
- **Consequence:** none for safety under the stated model (same user, same token directory); the documents overstate it. For H8: \`tokens::save\` truncates and writes without a sync (\`tokens.rs:37-54\`), but it only ever writes a jar's first token before the jar is spawned, so a kill -9 between the write and the jar can't strand a jar; a power cut ends the jars as well.
- **Status:** confirmed.
- **Fix direction:** correct the two documents.

### D8. The attach client's reset leaves spyc's title and pointer mode, and no reset ends a synchronized update

- **Severity:** low. **Frozen:** no. **Lens:** correctness.
- **Sites:** \`crates/spyc-jar/src/attach.rs:61-64\` (\`RESET\`: no \`\x1b[23;0t\` title pop, no \`\x1b[>0p\`, and \`\x1b[<10u\` pops ten kitty entries where spyc pushed one); \`src/terminal.rs:227-241\`, \`507-528\`, \`542-569\` (no \`\x1b[?2026l\` in any restore) against \`src/app/run.rs:457\` (every frame opens one).
- **Claim:** a terminal that leaves without spyc detaching it (spyc exited or crashed in its jar, or \`^\`) keeps spyc's window title on the title stack and the hide-pointer-while-typing mode; a spyc killed between a frame's start and end leaves the terminal holding output until its own timeout.
- **Evidence (lines read).** Mode inventory: \`setup_terminal\` and \`reattach\` set raw mode, \`?1049h\`, \`?2004h\`, \`?1007h\`, \`>1p\`, a kitty push (when supported), a title push and a hidden cursor; at run time \`?1000h ?1002h ?1006h\` (\`set_mouse_capture\`), \`=N;1u\` (\`set_key_event_types\`), \`?2026h/l\` around each frame and OSC titles. Restored: on quit, \`restore_terminal\`, all but \`?2026\`; on a panic, \`restore_on_panic\`, the same; on SIGTERM and SIGHUP, \`signal_terminate\`, the same plus termios; on \`^d\` in the own jar, \`detach_reset\`, the same escapes (termios is the client's); on a takeover the old \`spyc attach\` restores through \`signal_terminate\` (\`attach.rs:94\`, \`terminal.rs:461-464\`); on \`^\` or the program's exit, the client's \`RESET\` as above. No focus reporting (\`?1004\`) or cursor shape is set anywhere (\`rg '1004h|SetCursorStyle' src\`). The image probe's reader ends with its probe (\`terminal/image_probe.rs:36-63\`) and \`reattach\` runs with the input reader parked (\`src/app/proc.rs:454\`); each attach blocks the loop for at most the probe's 2 s backstop plus crossterm's keyboard query.
- **Consequence:** cosmetic: a stale window title and pointer mode after a crash or \`^\`.
- **Status:** confirmed (lines read); not shown on a real terminal.
- **Fix direction:** add the title pop, \`>0p\` and \`?2026l\` to the fixed reset and to spyc's restore string.

### D9. Nothing says where a detached session ends with the login (H7)

- **Severity:** informational. **Frozen:** no. **Lens:** docs.
- **Sites:** \`crates/spyc-jar/src/cli.rs:121-135\` (the jar's only escape is \`setsid\`); \`docs/features/projects.md:246-317\`, \`INSTALL.md:13-14\` (WSL is a supported platform).
- **Claim:** \`setsid\` leaves the terminal's session but not systemd-logind's login scope, so with \`KillUserProcesses=yes\` a logout ends every jar, and WSL may end the distribution once its last terminal closes; no document mentions either (\`rg -n 'KillUserProcesses|enable-linger|loginctl|systemd-run' docs README.md INSTALL.md FEATURES.md src\` finds nothing).
- **Evidence:** the search; the behaviour is unverified here (the Linux container has no logind, and there is no WSL).
- **Consequence:** the owner's S4 test, "close the terminal, open another, \`spyc attach\`", fails on such hosts with no hint why.
- **Status:** partly confirmed (the documents are silent); behaviour unverified: manual items 1 and 2.
- **Fix direction:** say so on the projects page, with \`loginctl enable-linger\` as the way round.

### D10. What else a handover drops

- **Severity:** low. **Frozen:** no. **Lens:** docs, correctness.
- **Sites:** \`src/app/detach.rs:251\` (\`release_jars\` drops \`capture_spill_dir\`), \`src/app/capture.rs:40-71\`, \`src/lib.rs:533\` (\`mcp::cleanup_socket\` before the handover).
- **Claim:** a \`!\` capture running across \`^d\` or \`:upgrade\` loses the spill file that holds its whole output, so the new spyc has only what its jar still holds; and every agent's MCP connection drops at the handover and comes back through its jar (R30a).
- **Evidence (lines read):** the spill directory is a \`TempDir\` removed on drop; whether the jar's replay still holds a capture's start depends on its ring (unit C's ground). Not probed.
- **Status:** unverified.
- **Fix direction:** keep the spill directory across a handover, or say what a capture loses.

## Seeded leads

- **V6 (attach flow): confirmed.** \`take_over\` signals the pid read before the prompt, with no second look (D3, shown by \`p_attach.py\`). Other later uses of a described pid are asks, not signals: \`shutdown::program_pid\` → \`ask_shutdown\` after a prompt, \`behind\` → \`ask_behind\`/\`ask_upgrade\`, \`release\` → \`ask_release\` (D3). The orphan sweep is unit E's.
- **H6: confirmed.** Any \`0x1c\` in a read detaches, a paste is cut at it, and without the kitty protocol \`^\` can't reach spyc (D4).
- **H7: partly confirmed.** No document mentions logind's \`KillUserProcesses\` or WSL's shutdown; the jar only calls \`setsid\`. The behaviour needs a host with either (D9; manual 1, 2).
- **H8: refuted as stated.** spyc never sends \`release\` with a new token, so there is no "new token before the jar is told it" to order; \`tokens::save\` is not atomic or synced, but only writes a new jar's first token before that jar exists. The documents that promise a new token per move are wrong (D7).

## Manual checklist items

1. **H7, Linux with logind** (settles D9). On a Linux host with \`KillUserProcesses=yes\` in \`/etc/systemd/logind.conf\` (or \`loginctl show-user $USER -p KillUserProcesses\` says yes), \`ssh\` in, start spyc with a tab running \`sleep 1000\`, \`^d\`, log out of every session of that user, \`ssh\` in again, \`spyc attach\`. Design: the session is there. Expected by the code: "no session runs in a jar of its own". Then \`loginctl enable-linger $USER\` and repeat.
2. **H7, WSL** (settles D9). In WSL2, spyc with a \`sleep 1000\` tab, \`^d\`, close every Windows Terminal tab, wait two minutes (\`wsl -l -v\` in PowerShell shows whether the distribution stopped), open a new one, \`spyc attach\`.
3. **H6 on real terminals** (settles D4). In an attached session (\`^d\`, then \`spyc attach\`), press \`^\` in Terminal.app: expected, \`[detached]\` instead of the pane toggling. Repeat in Ghostty: expected, the pane toggles. Copy a line holding a \`0x1c\` (\`printf 'a\x1cb\n'\` in a shell, then copy it) and paste it into a Claude tab: expected, \`[detached]\` and half a paste.
4. **D1 on the real \`TMPDIR\`** (settles D1 on the owner's paths). With a \`cat\` tab, \`:name ABCDEFGHIJKLMNOPQRSTUVWXYZ12\` (28 characters), \`^d\`. Expected: a spyc with another name and an offer about jars no spyc owns. Answer restore, never discard; then quit it and \`spyc -r\`.
5. **Takeover after sleep and over SSH** (the S4 owner test, no finding): attached at the desk, close the lid ten minutes, then \`ssh -t host spyc attach\` from another machine; the desk's terminal should end and show the shell as before spyc.

## Not covered

- \`src/state/sessions/tests.rs\`, the tests in \`src/app/session.rs\`, \`session_restore.rs\` 560 onward, \`src/app/projects.rs\` beyond a skim, \`tests/jar_attach.rs\` test bodies.
- No kill -9 probe at a chosen instant of a handover: the orders above are from the code, with closed reasoning, not from an injected pause.
- \`spyc attach\` over a real SSH connection, after sleep, and across two emulators (manual 5).
- \`spyc shutdown\` and \`spyc events -f\` were read, not run.
- Linux: nothing of this unit was run there.
