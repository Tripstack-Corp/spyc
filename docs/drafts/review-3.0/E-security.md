# Unit E. Security posture

**Subject:** `main` at `444857a2` (`444857a2a32b2f284d2b4fd19e57aef19700a5c5`), read in the reference worktree `~/src/spyc.worktrees/review/3.0`.
**Platform used:** macOS (Darwin 25.6.0, Apple silicon) for every read and probe. No Linux run: the findings that depend on Linux (`SO_PEERCRED`, `/tmp/spyc-<uid>` with no `XDG_RUNTIME_DIR`) are argued from the code and marked so.
**Probes:** the pinned debug binaries (`~/.cache/spyc/target/3.0-9f8124c5/debug/spyc`), each run under its own `HOME`, `XDG_*` and `TMPDIR` in `/tmp/rve1`, `/tmp/rve2` and `/tmp/rve3`, with `SPYC_CONTEXT`, `SPYC_JAR_SOCKET`, `SPYC_MCP_SOCK`, `SPYC_PANE_ID` and `SSH_AUTH_SOCK` removed (`env -u`). Scripts and logs are in `docs/drafts/review-3.0/evidence/E/`. Afterwards nothing new had appeared in the owner's `~/.local/state/spyc` or `$TMPDIR/spyc` (checked with `find -newer`), and no process of the probes was left running.

## Verdict

Nothing in this unit holds the tag on its own, and nothing found here touches the frozen surface beyond a consequence the owner should know about (E6). The jar's own side holds the model it states: a peer of another user is refused before its `hello` is read (`crates/spyc-jar/src/jar/accept.rs:158`), the token is compared in constant time and required for owner and reader, and a tokenless connection can only describe and ping. The weak points are on spyc's side and in the documents. First, spyc's MCP socket, which every hosted agent can reach, carries four control methods with no check of who asks: one unauthenticated request from any process of the user ends spyc and every program in every tab, with no confirmation (E1, shown on a scratch spyc). That is inside the stated model ("not a boundary"), but it is a larger reach than the jar token was added to deny, and no document says it. Second, the trust a client places in a jar is not checked on every path: `spyc attach`, `spyc shutdown`, `spyc --jars` and the orphan finder read a socket directory without the ownership check that spawn, adopt and the MCP proxy make, and `spyc attach` then sends the session's token and the user's keystrokes to that socket and `SIGTERM`s a pid the socket names (E2, shown with a stand-in socket). Third, five task and capture paths signal a pid the jar reported directly with `kill(-pid)`, which the SPYC-TRAP `jar-signals-through-the-jar` and review finding 10 say never happens (E3). Fourth, `SECURITY.md` does not describe 3.0 and says one thing that is now false (E4). The rest is low or informational.

## Coverage ledger

| File | Status |
|---|---|
| `SECURITY.md` | read in full |
| `docs/drafts/JAR_PROTOCOL.md` | read in full |
| `docs/drafts/JAR_PRODUCTION_PLAN.md` | read in full |
| `docs/archive/review-2.1/SYNTHESIS.md` | first 9,000 characters (the shape) |
| `docs/archive/review-2.1/C-mcp-state-security.md` | first 16,000 characters (the shape and its MCP table) |
| `src/mcp/protocol.rs` | read in full |
| `src/mcp/server.rs` | read in full |
| `src/mcp/mod.rs` | read in full |
| `src/mcp/follow.rs`, `moved.rs`, `framing.rs`, `statusline.rs`, `deadline.rs` | read in full |
| `src/mcp/readers.rs` | its diff since `v2.4.0` read; the rest skimmed (reviewed in 2.1, unchanged in substance) |
| `src/mcp/hooks.rs`, `hooks/codex*`, `config.rs` | diff since `v2.4.0` read (13 lines in `hooks.rs`, none in `config.rs`); not otherwise read |
| `src/mcp/tool_schemas.rs` | diff since `v2.4.0` read (one description) |
| `src/mcp/test_support.rs`, `src/mcp/tests/*` | not read |
| `src/mcp_cmd.rs` | read in full |
| `src/app/mcp.rs` | read in full |
| `src/app/mcp_routing.rs` | read in full |
| `src/pane/jar/place.rs`, `tokens.rs`, `orphans.rs`, `listing.rs`, `store.rs` | read in full |
| `src/pane/jar/mod.rs`, `attach.rs`, `session.rs`, `shutdown.rs`, `test_dir.rs` | read in full (outside the unit, for the connecting paths) |
| `src/pane/jar/link.rs` | lines 300 to 573 read (the stand-ins and `process_group_leader`) |
| `crates/spyc-jar/src/jar/accept.rs` | read in full |
| `crates/spyc-jar/src/jar/describe.rs` | read in full |
| `crates/spyc-jar/src/jar/conn.rs` | lines 1 to 300 read |
| `crates/spyc-jar/src/client.rs`, `lock.rs`, `store.rs` | read in full |
| `crates/spyc-jar/src/upgrade.rs` | lines 95 to 200 read (`begin`, `one_file`) |
| `crates/spyc-jar/src/cli.rs` | lines 120 to 180 read (start, `private_dir`) |
| `crates/spyc-jar/src/jar/mod.rs` | `keep_socket` (lines 459 to 481) read |
| `src/crash_report.rs` | read in full |
| `src/wiring/mod.rs`, `file.rs`, `cli.rs` | read in full |
| `src/wiring/statusline.rs` | lines 1 to 250 read (all production code); tests skimmed |
| `src/wiring/agy.rs`, `claude.rs`, `codex.rs` | diffs since `v2.4.0` read; the rest not read (unchanged since the 2.1 and #619 reviews) |
| `src/wiring/tests.rs` | not read |
| `src/app/detach.rs` | lines 1 to 330 read |
| `src/app/project_move.rs` | lines 700 to 860 read (`release_session`, `release`) |
| `src/app/session.rs` | lines 720 to 800 read (`end_session`, quit) |
| `src/app/tasks.rs`, `src/app/effect/executor.rs`, `src/app/pager_handler/pager_action.rs`, `src/pane/pty_host.rs`, `src/app/util.rs` | the signal sites only (ranges cited in E3) |
| `src/pane/ssh_agent.rs` | lines 1 to 95 read |
| `src/state/mod.rs` | `state_root` and `open_state_file` read |
| `src/context.rs` | `write_context_file` read |

## The access table

Every socket, file and directory the jar work adds, with the MCP socket beside them because every agent reaches it. "The user" is any process running as the user's uid, a prompt-injected agent included. `<rt>` is `$XDG_RUNTIME_DIR/spyc`, else `$TMPDIR/spyc`, else `/tmp/spyc-<uid>` (`src/pane/jar/place.rs:159-171`); `<state>` is `$XDG_STATE_HOME/spyc`, else `~/.local/state/spyc` (`src/state/mod.rs:64-77`).

| Object | Mode, and who checks it | Who can reach it | What proves identity | No token | With the token | By role |
|---|---|---|---|---|---|---|
| Pane jar socket `<rt>/jars/<16 hex>.sock` | socket `0600`; its directory checked `0700`, the user's, not a link, by the jar at start (`crates/spyc-jar/src/cli.rs:137-143`) and by spyc before spawn or adopt (`src/pane/jar/mod.rs:437`, `place.rs:103-114`) | the user only: a peer of another uid is closed before its `hello` is read (`accept.rs:158`) | peer uid, then the token in the `hello`, compared in constant time (`accept.rs:198`, `288-291`); the program role by the kernel's peer pid (`accept.rs:189-197`) | a lookup: `describe` or `ping`, once, closed after 5 s; at most 16 tokenless at once (`accept.rs:19`, `conn.rs:157-160`). `describe` gives the pids, argv, cwd, `owner_pid`, `owner_mcp`, `pane_id`, labels (`describe.rs:7-104`) | owner (when there is none) or reader: the same token opens both | **owner:** input, replies, resize, signals, snapshots, `set_info` (and so `owner_mcp`), `release` (a new token), `upgrade`. **reader:** replay, snapshots, describe, ping, **`upgrade`** (E6). **program** (its own child's pid, no token): describe, ping, `detach`, which ends the owner's connection (`conn.rs:212`, `30-79`) |
| Pane jar lock `<sock>.lock` | `0600` (`lock.rs:76-83`) | the user | none | read whether a jar runs (`lock.rs:53-62`); a process holding an exclusive `fcntl` lock on it makes a gone jar read as live (E10) | n/a | n/a |
| Session jar socket `<rt>/sessions/<NAME>.sock` | as a pane jar's; spyc checks the directory when it starts a session (`place.rs:86-99`), **not** when it attaches or shuts one down (E2) | the user | as a pane jar's | as a pane jar's | owner = the attached terminal: every key reaches spyc, and through it every tab | as a pane jar's; spyc itself is the program |
| Upgrade copies `<rt>/jars/jar-bin/` | `0700`, checked (`crates/spyc-jar/src/store.rs:86-97`) | the user | none | n/a | an owner or reader can have the jar copy any absolute path in and run it (E6) | n/a |
| MCP socket `<state>/mcp-<pid>.sock` | bound under `umask 077` (`src/mcp/server.rs:590-593`); the state directory is not checked | the user: peer uid checked (`server.rs:660-669`) | nothing; `SPYC_PANE_ID` in `initialize` binds attribution, forgeable (SECURITY.md says so) | **everything**: every tool, and the control methods `spyc/shutdown`, `spyc/release`, `spyc/upgrade`, `spyc/statusline` (E1) | there is no token | no roles |
| Root sidecar `<state>/mcp-<pid>.root` | `0600`, atomic (`server.rs:144-164`) | the user | none | read and rewrite (discovery) | n/a | n/a |
| Context file `<state>/.spyc-context-<pid>-<project>.json` | `std::fs::write`, so `0644` under the usual umask (`src/context.rs:103-111`); unchanged since `v2.4.0` | the user; other users when the state directory is open to them | none | read the cwd, picks, inventory, mounts | n/a | n/a |
| Token files `<state>/jar-tokens/<socket stem>` | `0600` on create, directory `0700` on create; neither checked when they exist; written in place, unsynced (`src/pane/jar/tokens.rs:37-54`) | the user | none | read: that is full control of the jar (the threat model accepts it) | n/a | a session's file is never removed (E7) |
| Binary store `<state>/jar-bin/` | `0700`, marker, the user's, not a link, checked by spyc (`src/pane/jar/store.rs:112-122`) and by a jar (`crates/spyc-jar/src/store.rs:47-57`); entries `0555` in `0700` directories | the user | none | replace an entry's file (out of model; an entry is hashed again only at spyc's start, `store.rs:150-160`) | n/a | n/a |
| SSH agent links `<rt>/ssh-agents/` | `0700`, checked (`src/pane/ssh_agent.rs:69-90`); a link only to the user's socket in a directory others can't change (`ssh_agent.rs:43-66`) | the user | none | use the agent the link names (E5) | n/a | n/a |
| Crash reports `<state>/crash/*.txt` | directory `0700`, file `0600`, `O_NOFOLLOW` (`src/crash_report.rs:278-295`, `src/state/mod.rs:121-133`) | the user | none | read: the panic message, which may name a path; the issue link scrubs paths (`crash_report.rs:228-259`) | n/a | n/a |
| `mcp.log` | `0600`, append, `O_NOFOLLOW` (`src/mcp/mod.rs:56-61`) | the user | none | read connection events; bodies only with `SPYC_MCP_DEBUG` | n/a | n/a |
| Session and move files | not reviewed here (unit D) | | | | | |

### Trust from the client's side

The jar checks its peer. What a client checks about the jar it reaches:

| Path | Code | Checks the directory? | What it sends or does |
|---|---|---|---|
| Spawn a tab, task or capture | `src/pane/jar/mod.rs:428-540` | yes, `place.prepare()` at line 437 | the new token on the jar's standard input; owner `hello` with it |
| Adopt a restored tab, retake a tab | `mod.rs:439-443`, `544-609` | yes, the same `prepare()` runs first | the saved token |
| A session's own jar, started | `src/pane/jar/session.rs:220-282` | yes, `prepare_sessions()` | the new token |
| spyc linking to its own jar | `session.rs:146-197` | no (the path is canonicalized and compared with the sessions directory) | a program `hello`, no token |
| `spyc attach` | `src/pane/jar/attach.rs:56-96`, `224-244` | **no** | the session's token, every key typed, `SIGTERM` to the `owner_pid` it describes, `spyc/behind` and `spyc/upgrade` to the `program_pid` it describes (E2) |
| `spyc shutdown` | `src/pane/jar/shutdown.rs:27-65`, `109-125` | **no** | `spyc/shutdown` to the MCP socket of the `program_pid` it describes |
| `:attach project`'s release | `src/app/project_move.rs:792-797` | no | `spyc/release` to the MCP socket of the `program_pid` the jar describes |
| `spyc --jars` | `src/pane/jar/listing.rs:10-29` | no | tokenless `describe` only |
| Orphans, found and discarded | `src/pane/jar/orphans.rs:36-53`, `95-135` | no (`Place::for_this_user` makes no `prepare()`) | the saved token, then hangup and kill through the jar |
| The MCP proxy and status hooks by `SPYC_JAR_SOCKET` | `src/pane/jar/mod.rs:265-272` (`owner_mcp`), `src/mcp/follow.rs:90-111`, `src/mcp/mod.rs:369-395` | yes, `private_dir` on the jar's directory; the `owner_mcp` path the jar names is connected to as it is | the agent's MCP traffic, replayed `initialize` |
| The MCP proxy by `SPYC_MCP_SOCK` and discovery | `src/mcp/mod.rs:626-645`, `server.rs:35-85` | no; the paths come from the pane's environment or the state directory | the agent's MCP traffic |

### Non-tool methods on the MCP socket

Enumerated from `dispatch_for` (`src/mcp/protocol.rs:421-511`). Every one is served to any connection that passes the peer-uid check; none looks at the caller's binding.

| Method | Kind | Who is meant to call it | What it does | Site |
|---|---|---|---|---|
| `initialize`, `notifications/initialized` | MCP | an agent's proxy | binds the connection to the pane id or cwd its `_meta` names | `protocol.rs:483`, `515-556` |
| `resources/list`, `resources/read` | MCP | an agent | the context file | `protocol.rs:484-486` |
| `tools/list`, `tools/call`, `ping` | MCP | an agent | the tools | `protocol.rs:487-490` |
| `spyc/statusline` | request | `spyc --statusline` in a pane | records the rate-limit, context and vim-mode projection for **any** `pane_id` it names | `protocol.rs:491`, `statusline.rs:102-128` |
| `spyc/behind` | request | `spyc attach` | says whether a newer spyc is installed | `protocol.rs:492-495`, `app/mcp.rs:454-458` |
| `spyc/release` | request | `:attach project` in another spyc | saves the session as no spyc's, lets every jar go, quits | `protocol.rs:496-502`, `app/project_move.rs:748-772` |
| `spyc/shutdown` | request | `spyc shutdown`, after its own `[y/N]` | saves the session and quits, ending every tab | `protocol.rs:503-508`, `app/mcp.rs:522-529`, `app/session.rs:735-739` |
| `spyc/upgrade` | notification | `spyc attach` from a newer install | sets `upgrade_pending`: with a newer binary installed, spyc hands itself over (in place in its own jar, else by `^d`'s handover); otherwise a flash | `protocol.rs:442-446`, `app/mcp.rs:516-521`, `app/detach.rs:111-135`, `257-262` |
| `spyc/disconnected` | notification | an older spyc | flashes a message naming the pid it was sent | `protocol.rs:436-441`, `app/mcp.rs:530-538` |

## Findings

### E1. One unauthenticated request on the MCP socket ends spyc and every tab, with no confirmation

- **Severity:** medium. **Frozen:** no. **Lens:** security.
- **Sites:** `src/mcp/protocol.rs:503-508` (`spyc/shutdown`), `src/mcp/protocol.rs:496-502` (`spyc/release`), `src/mcp/protocol.rs:442-446` (`spyc/upgrade`), `src/mcp/protocol.rs:491` with `src/mcp/statusline.rs:102-128` (`spyc/statusline`); the handlers `src/app/mcp.rs:522-529` → `src/app/session.rs:735-739`, `src/app/project_move.rs:748-772`, `src/app/mcp.rs:516-521` → `src/app/detach.rs:111-135`.
- **Claim:** the four control methods dispatch for any connection that passes the peer-uid check, without looking at its binding or asking the user, and `spyc/shutdown` saves the session and quits, which ends every program in every tab.
- **Evidence (command and output).** `probe_mcp_control.py` starts a scratch spyc in a PTY under `/tmp/rve1`, opens a jarred tab running `sleep 9999`, then connects to `mcp-<pid>.sock` as an unrelated process (no `initialize`) and sends one framed request. Log `probe_shutdown.log`:

  ```text
  describe .../jars/9937a28abce2826c.sock {... "program_pid": "40858", "owner": "connected", "owner_pid": "40691", "owner_mcp": "/tmp/rve1/state/spyc/mcp-40691.sock", ...}
  program pid (sleep) 40858 alive True
  reply to spyc/shutdown : {"jsonrpc":"2.0","id":7,"result":{}}
  spyc exited after 0.1s 0
  program (sleep) alive after: False
  jar sockets after: []
  ```

  The same with `spyc/release` under `/tmp/rve2` (`probe_release.log`): the reply was `{"result":{"session":1795531581455}}`, spyc exited at once, and the jar ran on with `owner none for 3 s`: every agent in the session loses its spyc (tools, status, scopes) until the user runs `spyc -r` or another spyc takes the session.
  `spyc/upgrade` and `spyc/statusline` were read, not run: the first sets `runtime.upgrade_pending`, and `settle_detach` (`detach.rs:257-262`) then hands spyc over to the newly installed binary whenever one is there, which on the owner's machine is after every `make install`; the second sets the statusline figures of whichever `pane_id` it names, and any jar's tokenless `describe` gives the pane ids.
- **Against the stated model.** The adversary is a hosted agent under prompt injection, running as the user. Every hosted agent has `SPYC_MCP_SOCK` in its environment. These methods are not reachable through the agent's MCP client, which sends only MCP methods, so the agent needs a shell command; that is the same bar a jar's socket had before the token. The token was added (R26) so that "an agent that finds a socket and connects to it, whether by accident, curiosity, or a prompt injection" can't type into or watch another pane. A process that finds the MCP socket can, with no token and no file to read, end all of them, or detach the whole session from the user. `SECURITY.md` says the MCP surface isn't a boundary, but describes it as the tool surface; it names none of these methods. `spyc shutdown` asks `[y/N]` on the client side (`src/pane/jar/shutdown.rs:42-52`), which an agent skips by sending the request itself, and spyc's own quit asks while anything runs (`app/session.rs:753-796`).
- **Consequence:** an injected agent with a shell can, in one line, end every shell, task, capture and agent in the user's spyc, including unsaved work in an editor in a tab, or make spyc vanish from the terminal leaving every agent without tools.
- **Status:** confirmed (run on a scratch spyc, macOS).
- **Direction:** have spyc ask the user in the TUI before it acts on `spyc/shutdown`, `spyc/release` and `spyc/upgrade` (or require the session jar's token with them), and record the decision in `SECURITY.md`.

### E2. `spyc attach`, `spyc shutdown`, `spyc --jars` and the orphan finder trust a socket directory they never check

- **Severity:** medium (narrow exposure: see below). **Frozen:** no. **Lens:** security.
- **Sites:** `src/pane/jar/place.rs:125-131` (`this_users_sessions`, no check), `src/pane/jar/attach.rs:64`, `224-244` (`running`), `attach.rs:75-80` and `95` (token loaded and sent, `take_over`), `attach.rs:133-150` (`behind`), `src/pane/jar/shutdown.rs:28`, `39-53`, `109-125`, `src/pane/jar/listing.rs:10-29`, `src/pane/jar/orphans.rs:36-53`, `95-102`, `src/pane/jar/session.rs:146-163`; the jar's own `keep_socket`, `crates/spyc-jar/src/jar/mod.rs:459-481`, which makes the directory again and binds with no ownership check. Against them: `docs/drafts/JAR_PRODUCTION_PLAN.md:94-97` ("both sides check the directory is this user's, `0700` and not a link (14)").
- **Claim:** these paths read the sessions or jars directory without `private_dir`, and `spyc attach` then sends the session's token and every key the user types to whatever socket it finds, and `SIGTERM`s whatever pid that socket's `describe` names as `owner_pid`.
- **Evidence (command and output).** `probe_attach_trust.py` makes a scratch tree under `/tmp/rve3` whose `run/spyc` and `run/spyc/sessions` are mode `0777` (what `private_dir` refuses), keeps a token file for the session name `VICTIM` as a real session would leave, and binds a stand-in socket `VICTIM.sock` that answers a lookup with `owner: connected` and `owner_pid` of a decoy `sleep 300` it started, then answers an owner `hello` with a welcome and a fake prompt. It then runs the pinned `spyc attach VICTIM` in a PTY and types `hunter2\r`. Log `probe_attach_trust.log`:

  ```text
  sessions dir mode 0o777
  hello: role 3 token
  hello: role 1 token 0123456789abcdef0123456789abcdef-REAL-SESSION-TOKEN
  input frame: b'hunter2\r'
  decoy after attach: returncode -15 (-15 = SIGTERM)
  what the attach client printed: 'spyc: VICTIM is attached elsewhere (pid 43364); taking it over\r\n\r\n[a stand-in jar paints this terminal] password: ...'
  ```

  Spawn, adopt and retake are protected by `place.prepare()` (`src/pane/jar/mod.rs:437`), starting a session by `prepare_sessions()` (`place.rs:86-99`), and the MCP proxy by `owner_mcp`'s check (`mod.rs:266`). The others aren't.
- **Who can exploit it.** Another user only where the directory is shared: with neither `XDG_RUNTIME_DIR` nor `TMPDIR` set, `<rt>` is `/tmp/spyc-<uid>` (`place.rs:165-168`), which another user can create first (a Linux login without `pam_systemd`, `su`, cron, many containers). spyc then refuses to start sessions there, so the victim has none, but `spyc attach` lists and, with one entry, picks the planted socket; it needs a token file with the planted name, and session token files are never removed (E7). `spyc shutdown` needs no token: it asks the victim's own spyc of whatever pid the planted socket names to shut down (after its `[y/N]`). On macOS `TMPDIR` is per-user and private, so this needs a misconfiguration. A same-user agent gains nothing it lacks already.
- **Consequence:** in the shared-`/tmp` case another user can collect a session token and the keys a user types into what looks like their session, and can have the user's own `spyc attach` send `SIGTERM` to any of the user's processes. The plan's statement that both sides check is untrue for these paths.
- **Status:** confirmed (the trust gap shown on macOS with a stand-in; the cross-user precondition is read from the code and not run on Linux).
- **Direction:** route every client that reads a socket directory through the one `private_dir` check (or `Place::prepare`), so a finding naming N paths is closed at N.

### E3. Tasks and captures signal the pid the jar reported, not through the jar

- **Severity:** medium. **Frozen:** no. **Lens:** correctness, security.
- **Sites (seven production call sites):** `src/app/tasks.rs:285-298` (pause, `SIGSTOP`) and `tasks.rs:354-365` (resume, `SIGCONT`), both executed at `src/app/effect/executor.rs:154-175` by `kill_pg(pid, sig)`; `tasks.rs:321-329` (interrupt, `SIGINT`, `kill_pg` directly); `tasks.rs:429-434` (promote to a pane, `SIGCONT`); `tasks.rs:618-622` (`:fg`, `SIGCONT`); `src/app/pager_handler/pager_action.rs:285-294` (a capture's `^C`, `SIGINT` to the foreground group the jar reported, else the pid) and `pager_action.rs:314-336` (a capture's `^\`, `SIGKILL` to that group), both executed at `executor.rs:200-202`. `kill_pg` is `src/app/util.rs:283-288`.
- **Claim:** with jars on, a task's and a capture's host is a jarred `PtyHost` (`src/app/capture_starts.rs:36-51`; `tasks.rs:54`), whose `process_id()` is the pid the jar's `welcome` named (`src/pane/jar/link.rs:420-422`) and whose `foreground_pgrp()` is the group the jar's `foreground` frame named (`link.rs:363-368`); these sites pass those numbers to `kill(-pid)` themselves.
- **Evidence (lines read and reasoning).** `ARCHITECTURE.md:1209-1218` (SPYC-TRAP `jar-signals-through-the-jar`) says every signal for a jarred program goes as a frame to its jar, "and never to the pid (finding 10)", because after the exit the kernel may give the pid to anyone; the plan lists finding 10 as closed in S2a (`JAR_PRODUCTION_PLAN.md:93`). `PtyHost::signal_group` and `suspend` honour it (`src/pane/pty_host.rs:591-634`), and `toggle_pane_suspend` goes through them (`src/app/pane_tabs.rs:592-625`). The task and capture paths call `kill_pg` on the number instead. The jar is the program's parent and reaps it, so spyc can't hold the pid. Their only guard is spyc's own record (`TaskStatus::Running`, a pending capture), which lags the jar's reap. The test `pane::jar::tests::every_signal_goes_through_the_jar` covers `PtyHost`'s methods, not these. Search used for the inventory below.
- **Consequence:** pausing, resuming, interrupting or killing a task or capture whose program has just ended can stop, continue, interrupt or `SIGKILL` an unrelated process group of the user's that took the pid; the jar's own refusal of a signal after the exit (reason 9) is bypassed. The window is short, but it is the case the trap exists for.
- **Status:** confirmed (lines read; not run).
- **Direction:** send these through `PtyHost::signal_group` (or a jar frame), and extend the trap's test to tasks and captures.

### E4. `SECURITY.md` does not describe 3.0, and says spyc keeps no secrets

- **Severity:** medium. **Frozen:** no. **Lens:** docs.
- **Sites:** `SECURITY.md:36-37` ("handles no secrets of its own"), `SECURITY.md:325` (the only mention of `spyc-jar`), `SECURITY.md:349-363` ("When to revisit"); `Cargo.toml:31-46` (`/docs/**` excluded from the published crate); `ROADMAP.md:811`.
- **Claim:** the security document a user reads names none of what 3.0 adds, and the design it relies on lives only in `docs/drafts/`.
- **Evidence (lines read).** `grep -n -i "jar\|token\|attach\|session\|crash\|upgrade\|ssh" SECURITY.md` finds only line 325 for jars. Missing: the jar's threat model (same-uid check, the per-jar token and what it does and doesn't cover, tokens kept `0600` in the state directory), the session jar's token as control of all of spyc, `spyc attach` over SSH and that it points every tab's `SSH_AUTH_SOCK` at the attaching terminal's agent (E5), the binary store and the self-`exec` upgrade (a same-user process can replace a store entry), the MCP control methods (E1), crash reports. Line 37 is now false: spyc generates and keeps jar tokens, which are secrets. Line 357's trigger ("spyc gains ... RPC server") has fired: the jar protocol is one. The frozen protocol and its threat model are in `docs/drafts/JAR_PROTOCOL.md`, which the published crate excludes, and `ROADMAP.md:811` still calls it "Draft ... frozen 2026-11-12" where the document itself says it froze again on 2026-11-24 (`JAR_PROTOCOL.md:3`).
- **Consequence:** a user or a reviewer assessing 3.0 from its security document learns nothing about the new sockets, the token or the upgrade path, and is told spyc holds no secrets.
- **Status:** confirmed. Seeded lead V3.
- **Direction:** add a jar section to `SECURITY.md` (model, token, store, upgrade, attach, control methods), correct line 37, and move the frozen protocol out of `docs/drafts/`.

### E5. `spyc attach` over SSH gives every tab the attaching terminal's SSH agent

- **Severity:** informational. **Frozen:** no. **Lens:** security, docs.
- **Sites:** `src/pane/jar/attach.rs:117-128` (`follow_this_agent`), `src/app/detach.rs:296-328` (`follow_attached_agent`), `src/pane/ssh_agent.rs`.
- **Claim:** when a terminal with `SSH_AUTH_SOCK` attaches, every tab's link is pointed at that agent, so every agent in every tab can ask it to sign while attached.
- **Evidence (lines read).** `follow_attached_agent` repoints every tab's link once the session link changes; the links are checked to name the user's own socket in a safe directory (`ssh_agent.rs:43-90`). `docs/features/projects.md:256` and `:316` tell users their tabs reach the agent, as a feature.
- **Consequence:** an injected agent in any tab can use keys forwarded from the laptop (`ssh -A`) for as long as the user is attached. That is ordinary agent forwarding, but it extends to every program spyc hosts, which `SECURITY.md` should say.
- **Status:** confirmed (read).
- **Direction:** say it in `SECURITY.md` beside E4's section.

### E6. A version-1 jar has one token for owner and reader, and a reader may ask for an upgrade: no read-only role is possible without upgrading first

- **Severity:** informational, addressed to the owner. **Frozen:** yes. **Lens:** security.
- **Sites:** `crates/spyc-jar/src/jar/accept.rs:198-204`, `crates/spyc-jar/src/jar/conn.rs:157-161`, `221`, `crates/spyc-jar/src/upgrade.rs:102-169`, `crates/spyc-jar/src/store.rs:77-99`; `JAR_PROTOCOL.md:673-674`, `888-891`.
- **Claim:** inside the stated model this is by design, but it fixes what "reader" can mean in every version-1 jar.
- **Evidence (lines read).** A reader's `hello` must carry the jar's one token, the same that admits the owner (`accept.rs:198`), so a reader can connect as owner whenever there is none. `upgrade` is permitted to owner and reader (`conn.rs:160`, `221`), `one_file` accepts any absolute path and copies it into the store or the copies directory, and the validator runs it with the blob, which holds the token, on standard input. "A reader holding the token may ask" is in "What freezes for good" (`JAR_PROTOCOL.md:673-674`).
- **Consequence for roles:** U6 (a watcher that can never type) and any later role meant to hold less than the owner cannot be given to a version-1 jar's role 2 or its token: it needs a credential and a role number that version 1 doesn't know, so the jar must be upgraded first, and the gateway's read-only grants (Sharing) must never hand out the jar's token. The protocol text doesn't say this; V10's premise that the token is already full trust holds.
- **Status:** confirmed (read). Seeded lead V10, the roles part.
- **Direction:** a sentence in `JAR_PROTOCOL.md` under "Who may connect".

### E7. A session's token file is never removed

- **Severity:** low. **Frozen:** no. **Lens:** security (hygiene).
- **Sites:** `src/pane/jar/session.rs:244-245` (saved), `session.rs:276` (the only `forget` for a session, on a failed start), `src/pane/jar/place.rs:246-266` (the sweep walks only the jars directory), `src/pane/jar/tokens.rs:29-35` (the file is named by the socket's stem, the session's name).
- **Claim:** every session's token stays in `<state>/jar-tokens/<NAME>` after its jar has gone.
- **Evidence (lines read).** `rg -n "tokens::forget|forget\(" src/pane/jar/session.rs src/app/detach.rs src/app/project_move.rs src/app/session_restore.rs` finds only `session.rs:276`; the pane-jar sweep removes tokens for `place.sockets()` only.
- **Consequence:** secrets accumulate in the state directory, against "quitting leaves no ... file behind" (`JAR_PROTOCOL.md:61`), and a stale token for a name is what E2's attack needs.
- **Status:** confirmed (read; not run).
- **Direction:** forget a session's token when its jar ends, and sweep the sessions directory as the jars directory is.

### E8. Tokens are written in place, and existing directories and files are trusted as they are

- **Severity:** low. **Frozen:** no. **Lens:** security.
- **Sites:** `src/pane/jar/tokens.rs:37-54`.
- **Claim:** `save` creates `jar-tokens/` `0700` and the file `0600` only when it makes them; an existing directory or file keeps its mode, and the write truncates in place without a sync (durability is H8, unit D).
- **Evidence (lines read).** `DirBuilder::mode` and `OpenOptions::mode` apply on creation only; nothing checks the directory as `private_dir` does elsewhere.
- **Consequence:** a `jar-tokens/` made open by hand or by a restore from a backup stays open; on Linux with a `0755` home and state directory another user could read the tokens. Their use is limited, since a jar refuses another user's peer.
- **Status:** confirmed (read).
- **Direction:** check `jar-tokens/` with `private_dir` and write tokens atomically, `0600`.

### E9. What a tokenless `describe` gives, and what each consumer does with it

- **Severity:** informational. **Frozen:** yes (the lookup and its keys are in the frozen surface). **Lens:** security.
- **Sites:** `crates/spyc-jar/src/jar/describe.rs:7-104`; consumers `src/pane/jar/attach.rs:133-150`, `261-272`, `335-364`, `shutdown.rs:101-125`, `project_move.rs:792-797`, `mod.rs:265-272`, `orphans.rs:56-82`, `store.rs:302-324`, `session.rs:57-72`.
- **Claim:** `describe` gives any process of the user the jar's and program's pids, argv, cwd, `owner_pid`, `owner_mcp`, `pane_id`, `project_home` and labels; spyc acts on `owner_pid` (a `SIGTERM`), `program_pid` (requests on that pid's MCP socket), `owner_mcp` (where an agent's MCP traffic goes) and `build` (the store sweep).
- **Evidence (lines read).** All of it is what `ps` and the state directory show already, as decision J argues, except `pane_id`, which lets a process bind an MCP connection as another tab; `SECURITY.md:92-133` already treats pane ids as forgeable. Every consumer trusts the values once it has reached the socket, which is why E2 matters.
- **Status:** confirmed (read).

### E10. A held lock on a jar's `.lock` file makes a gone jar look alive

- **Severity:** informational. **Frozen:** yes (R32b's meaning is frozen). **Lens:** reliability.
- **Sites:** `crates/spyc-jar/src/lock.rs:53-62`, `src/pane/jar/place.rs:227-238`, `src/pane/jar/mod.rs:448-456`.
- **Claim:** any process of the user that holds an exclusive `fcntl` lock on `<sock>.lock` makes `socket_state` say `Live`, so spyc refuses to restart that tab's program ("its program wasn't started twice"), never sweeps it, and a new jar at that path refuses to start.
- **Evidence (lines read).** `state` reads `AGAIN`/`ACCESS` as `Held` whoever holds it. Within the model (a same-user process can do worse), and only a denial of service.
- **Status:** confirmed (read).

### E11. The crash report

- **Severity:** none found. `write_report` writes `0600` in a `0700` directory with `O_NOFOLLOW` (`src/crash_report.rs:278-295`); nothing is sent; the link scrubs the home, the working directory and every path- or file-shaped word (`crash_report.rs:228-259`), tested in `the_link_names_no_home_working_directory_or_file`. The file keeps the message unscrubbed, as its doc says. Recorded so the coverage is visible.

## The pid-signalling inventory (V6, the part this unit owns)

Search, run in the reference worktree over production and test code, then sorted by hand:

```sh
rg -n --no-heading "kill\(|signal::|Signal::|SIGTERM|SIGKILL|killpg|process::kill|kill_process|send_signal|\.kill\(\)|Signal::Term|Signal::Kill|nix::sys::signal|libc::kill" src crates -g '*.rs' \
  | rg -v "^[^:]*tests?\.rs:|/tests/|_tests\.rs:|harness_tests"
```

then `rg -n "foreground_pgrp|process_id\(\)" src/app -g '*.rs'` for the callers that hand a pid on to an effect. Comments and test files are left out of the table.

| Site | Signal | Where the pid comes from | Through the jar? | Verdict |
|---|---|---|---|---|
| `src/pane/jar/attach.rs:335-349` (`take_over`) | `SIGTERM` | `owner_pid` from a tokenless `describe`, read in `running()` (`attach.rs:236`, `261-272`) before the "Attach to which?" prompt and the token load, never read again, never checked to be a `spyc attach` | no | stale-pid and wrong-socket risk: V6 (unit D) and E2 |
| `src/app/tasks.rs:285-298` → `effect/executor.rs:154-175` | `SIGSTOP` to the group | the jar's `welcome` pid | no | E3 |
| `src/app/tasks.rs:321-329` | `SIGINT` | the same | no | E3 |
| `src/app/tasks.rs:354-365` → `executor.rs:154-175` | `SIGCONT` | the same | no | E3 |
| `src/app/tasks.rs:429-434` | `SIGCONT` | the same | no | E3 |
| `src/app/tasks.rs:618-622` | `SIGCONT` | the same | no | E3 |
| `src/app/pager_handler/pager_action.rs:285-294` → `executor.rs:200-202` | `SIGINT` to the group | the jar's `foreground` frame, else its pid | no | E3 |
| `src/app/pager_handler/pager_action.rs:314-336` → `executor.rs:200-202` | `SIGKILL` to the group | the jar's `foreground` frame | no (the `child.kill()` beside it is) | E3 |
| `src/pane/pty_host.rs:591-608` (`signal_group`), `614-634` (`suspend`), and its callers at `675`, `681`, `706`, `728` | all | jarred: a jar frame; unjarred: spyc's own child | yes | correct |
| `src/pane/jar/link.rs:387-440` (`JarChild::kill`, `JarKiller::kill`) | `KILL` | a jar frame | yes | correct |
| `src/pane/jar/orphans.rs:110-124` | hangup, then kill | a jar frame, as owner with the token | yes | correct |
| `src/pane/jar/mod.rs:517`, `session.rs:274`, `store.rs:228`, `src/clipboard.rs:292`, `670`, `src/proc_cwd.rs:74`, `src/wiring/claude.rs:151` | `Child::kill` | a `std::process::Child` spyc spawned and hasn't reaped | n/a | correct: an unreaped child's pid can't be reused |
| `crates/spyc-jar/src/jar/life.rs:214-250`, `held.rs:312-331` | to the program's group | the jar's own child, which it reaps | n/a | correct; after the exit a signal is refused (reason 9) |
| `crates/spyc-jar/src/upgrade.rs:523`, `666` | `SIGKILL` to a validator's or holder's group | children the jar spawned | n/a | correct |
| `crates/spyc-jar/src/upgrade.rs:829` | `SIGTERM` to the holder | the pid in the `SPYCJARF` message on standard input | n/a | the holder is the jar process's own child across the `exec`, so unreaped; unit A owns it |
| `src/pane/jar/test_dir.rs:29-49`, `src/app/pane_input.rs:361`, `src/clipboard.rs:1122` | `KILL`, `CONT` | `describe` in a test's own directory, or a test's own child | n/a | test code only |

Pids that are **asked**, not signalled, by mapping them to `<state>/mcp-<pid>.sock`: `spyc shutdown` (`shutdown.rs:39-53`, `program_pid` from `describe`), `:attach project`'s release (`project_move.rs:792-797`), `spyc attach`'s `spyc/behind` and `spyc/upgrade` (`attach.rs:141-150`, `186`). A pid that another spyc has taken since would send that spyc the request; the window is the time between the `describe` and the connect, short except in `spyc shutdown`, which waits at its `[y/N]` between them.

## Seeded leads

- **V3 (`SECURITY.md` does not describe 3.0): confirmed**, and wider: line 37 says spyc handles no secrets, and the "When to revisit" trigger for an RPC server has fired. E4.
- **V5 (control-plane methods take any caller): confirmed by probe.** Nine non-tool methods enumerated above; `spyc/shutdown` ends spyc and every tab, `spyc/release` detaches the session from its spyc, `spyc/upgrade` forces a handover after any install, `spyc/statusline` writes any tab's figures, `spyc/behind` and `spyc/disconnected` are harmless. Classified against the model: inside "not a boundary", outside what the jar token was introduced to deny, and undocumented. The proxy is not a filter (`annotate_initialize` touches only `initialize`, `src/mcp/server.rs:295-327`), but it isn't the way in either: the agent's MCP client sends only MCP methods, so the route is a shell command to `$SPYC_MCP_SOCK`. E1.
- **H2 (a client trusts a socket by its directory, not on every path): confirmed on four paths**, refuted on the others: spawn, adopt, retake, a session's start and the proxy by `SPYC_JAR_SOCKET` all check; `spyc attach`, `spyc shutdown`, `spyc --jars` and the orphan finder don't, and `keep_socket` rebinds unchecked. `Client::connect` itself checks nothing, as the lead says. Shown with a stand-in socket; the cross-user precondition (`/tmp/spyc-<uid>`) is argued from the code. E2.
- **V6 (every place a pid from `describe` or a saved file is signalled), the inventory: confirmed and extended.** One production site signals a `describe` pid (`take_over`); seven more signal a pid the jar reported, outside the jar, which the lead didn't name (E3); four ask a `describe` pid by its MCP socket. No saved file's pid is signalled. The table above.
- **V10 (a reader with the token can make the jar run any file), the consequence for roles: confirmed as by design inside the model**, with the consequence that no version-1 jar can have a read-only role without an upgrade, which the protocol doesn't say. E6.

## Manual checklist items

1. **E2 on Linux, two logins** (settles E2's cross-user precondition). On a Linux host, as user B: `mkdir -m 777 /tmp/spyc-<uid of A>` (A's uid from `id -u A`). As A, in a login with no `XDG_RUNTIME_DIR` (`env -u XDG_RUNTIME_DIR -u TMPDIR spyc`), open a tab and press `^d`. Expected: the detach is refused (spyc can't prepare the sessions directory). Then, as A, `env -u XDG_RUNTIME_DIR -u TMPDIR spyc attach`. Expected today: it lists what B planted in `/tmp/spyc-<A>/sessions`; with a fix, it refuses the directory.
2. **E1 from an agent tab** (settles whether a harness would let it through). In a scratch spyc, in a Claude tab, ask the agent to run `python3 -c` that sends `{"jsonrpc":"2.0","id":1,"method":"spyc/release"}` framed with `Content-Length` to `$SPYC_MCP_SOCK`. Expected today: Claude Code asks for permission for the command once; approved, spyc leaves the terminal and every tab runs on ownerless. Do this in a scratch session only.

## Not covered

- Linux: nothing in this unit was run on Linux, so `SO_PEERCRED`, the `/tmp/spyc-<uid>` fallback and `keep_socket` under `systemd-tmpfiles` are argued, not shown.
- `src/mcp/readers.rs` beyond its diff, `hooks.rs` and `config.rs` beyond their diffs, `src/mcp/tests/`, `src/wiring/tests.rs` and the unchanged bodies of `src/wiring/{claude,codex,agy}.rs`: all were reviewed in 2.1 or for #619, and changed only as recorded in the ledger.
- The session and move files' modes and contents (unit D's files).
- Whether a pane jar's program sending `detach` (allowed to any program role, `conn.rs:159`, `212`) does more than make spyc retake the tab; not traced.
- The `spyc/upgrade` and `spyc/statusline` effects were read, not run.
- No refuter pass on E1 to E3 within this unit.
