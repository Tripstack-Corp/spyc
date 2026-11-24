# Unit H. Cross-version: what one spyc writes and another reads

**SHA:** `origin/main` at `52ce15b7` (read from a `git archive` of it in scratch; the main checkout was not checked out or built). The older reader is `v2.4.0`, read with `git show v2.4.0:<path>`. The pin `444857a2` differs from `52ce15b7` only in #1317's files, none of which is a surface below. **Platform:** macOS (Apple silicon), reading only. No spyc or spyc-jar process was started, so no jar, socket, token, session or state file was made or touched. **Probe:** one scratch Cargo crate with no spyc code in it, holding copies of the manifest's serde types as `v2.4.0` and `52ce15b7` define them (attributes copied field by field), plus a copy of `prune_victims`; five tests, all passing. It is at `docs/drafts/review-3.0/evidence/H/serde-probe/` (`Cargo.toml`, `src/lib.rs`; its `target/` was deleted) with its run in `docs/drafts/review-3.0/evidence/H/serde-probe-run.log`.

## Verdict

The review read each 3.0 surface against a spyc of its own build. A detached session now outlives installs, so each surface below is also read by a spyc of another build, and the question is what that spyc does. The answer is mostly good: every JSON surface ignores fields it doesn't know, the manifest carries a version and skips one it doesn't know, `AgentKind` degrades instead of failing, snapshots are stamped with the build that can decode them and fall back to a portable form, tokens and SSH links are found by names that don't change, and `spyc/shutdown` and `spyc/release` are requests, so a spyc that predates them answers "Method not found" and the caller says so. Three things are wrong, and one of them belongs with C1 at high. **H1:** `v2.4.0`, the stable release every rc user can roll back to, reads a 3.0 manifest whole, because 3.0 still writes version 2 and the jar fields are optional additions. It doesn't know `jar_socket` or `tasks`, so `spyc -r` after a rollback starts every program whose jar still runs a second time and types `/resume` into each Claude, and its next save deletes the record of which jar each tab ran in. The fix costs one constant and has to be in the first rc, because 2.4.0's reader can't change. **H2** is C1's rc-to-final path re-read at `52ce15b7`: still open, closed by S6 if all five `honoured()` readers go, and no other switch on the version string exists. **H3–H5:** a 3.0.0 reader loses a whole session over one unknown enum value and drops unknown fields on its next save. `:upgrade` and jar upgrades treat any other build as newer, so a downgrade re-images every idle jar onto the older build. And the MCP proxy inside each jarred agent outlives every upgrade, carrying its own build's table of how long each tool may take. The rest are small. The manifest deserves a pinning test, but not a byte pin like `wire_v1.rs`: a frozen 3.0.0 fixture every later build must read with the same meaning, a test that every enum reachable from `Manifest` degrades, and a field-list check that forces a decision on `MANIFEST_VERSION` (see "Pinning", below).

## Coverage ledger

| Surface | Files read at `52ce15b7` | How |
|---|---|---|
| Session manifest | `src/state/sessions/manifest.rs` (in full), `src/state/sessions/mod.rs` 1–360 (types, save, load, prune), `src/app/detach.rs` (in full), `src/app/project_move.rs` 180–260, 480–860, `src/app/session.rs` 520–645, `src/app/orphan_jars.rs` 1–240, `src/pane/jar/orphans.rs` (in full); `v2.4.0`: `manifest.rs` (in full), `sessions/mod.rs` types, `session_restore.rs` 355–400 | read; serde probe |
| `spyc/*` methods | `src/mcp/protocol.rs` 90–140, 420–510; `src/mcp/mod.rs` 60–240; `src/app/mcp.rs` 440–460, 740–760, 930–945; callers `src/pane/jar/attach.rs` (in full), `src/pane/jar/shutdown.rs` 1–130, `project_move.rs` `release` | read |
| Attach client against another build | `crates/spyc-jar/src/attach.rs` 1–140, `src/pane/jar/session.rs` (in full), `src/pane/ssh_agent.rs` 1–80 | read |
| MCP proxy in a jarred agent | `src/mcp/server.rs` 1–140, 285–560, `src/mcp/follow.rs` (in full), `src/mcp/deadline.rs` 1–110 | read |
| Jar store, build-info, jar upgrades | `src/pane/jar/store.rs` (in full), `src/pane/jar/place.rs` (in full), `src/app/jar_upgrades.rs` 1–60, 200–260, `crates/spyc-jar/src/jar/describe.rs` 1–70 | read |
| Tokens, sockets, locks | `src/pane/jar/tokens.rs` (in full), `place.rs` | read |
| Event log | `src/state/event_log.rs` 1–200 | read |
| Snapshots | `src/pane/jar/snapshots.rs` 1–60, 150–196 | read |
| Version-string switches | `rg -n -e '-rc\.|"-CURRENT"|CARGO_PKG_VERSION|crate::VERSION\b|PACKAGE_VERSION|proto::VERSION' src crates/spyc-jar/src` | search |
| Hooks | `src/mcp/hooks.rs` (the hook command lines) | search |
| Documents | `docs/drafts/JAR_PROTOCOL.md` 750–790, 1190–1210; `JAR_PRODUCTION_PLAN.md` 155–170; `docs/features/projects.md` 175–190 | read |
| Existing tests | `manifest.rs` tests, `src/state/sessions/tests.rs` (names and the legacy-format tests), `src/app/harness_tests/manifest_v2.rs` (names), `crates/spyc-jar/tests/wire_v1.rs` (names and messages) | read |

## Each surface, by direction

| Surface | How it is versioned | An older reader meets a field or value it doesn't know | A newer reader meets a missing field | A downgrade | Pinned? |
|---|---|---|---|---|---|
| Session manifest (`<state>/spyc/sessions/<id>.json`) | `version: 2` (`manifest.rs:20`), `2.4.0` also 2; no `deny_unknown_fields`; optional fields `#[serde(default)]` | an unknown field is ignored, then dropped by its next save (H3); an unknown `AgentKind` becomes `Other` (`mod.rs:42`); an unknown `AgentHudMode` or `ScopeIntent` fails the whole file, which `load_manifests_in` (`mod.rs:297`) then skips without a word (H3); an unknown `version` skips the file | defaults, except `version`, `id`, `epoch_secs`, `projects`, and per project `id`, `start_dir`, `pane_height_pct`; a `2.4.0` file reads (probe) | 3.0 → 2.4.0 restores without jars (H1); 3.0.x → 3.0.0 can lose a session or a field (H3) | version 1 and legacy aliases have tests; no 3.0 fixture |
| `spyc/shutdown`, `spyc/release` | requests; "Method not found" (-32601) read as "predates" (`mcp/mod.rs:157`, `:204`) | params are ignored, so E1's token can be added | n/a | the giver saves a manifest the taker may not read, and quits first (H6) | no |
| `spyc/behind` | request, `{"behind": bool}` | an old spyc gives -32601; the caller falls back to the start time (`attach.rs:133-165`) | n/a | "behind" means "another file", downgrade included (H4) | no |
| `spyc/upgrade` | notification, no reply (`protocol.rs:442`) | ignored silently; `spyc attach` waits 20 s (H7) | n/a | asks for an "upgrade" to any other file (H4) | no |
| Attach client ↔ session jar | jar protocol version 1 | describe keys: tolerant strings | `terminal`, `owner_left`: `None` when absent (`session.rs:51-70`) | the client's fixed reset may miss a mode a newer spyc sets (D8's ground) | frames by `wire_v1.rs`; keys not (H9) |
| MCP proxy ↔ spyc | none; a per-build table of reply budgets | a newer server's longer call ends an older proxy (H5) | a newer proxy waits longer: harmless | n/a | within one build only |
| Jar store | `<state>/jar-bin/<spyc-jar version>-<hash>/`; `build-info` | the sweep keeps an entry only by exact `build` name (`store.rs:302-327`) | n/a | the sweep removes another running spyc's entry after 24 h (C10, confirmed by reading here) | no |
| Jar upgrades | `build` compared for equality (`jar_upgrades.rs:238`) | n/a | n/a | re-images every idle jar onto an older build (H4) | the `differ` jar, across blob versions only |
| Token files | `<state>/jar-tokens/<socket stem>`, hex | n/a | n/a | n/a | no; stable by construction |
| Event log | plain lines, `<state>/events/<NAME>.log` | read as text | n/a | n/a | not needed |
| SSH agent links | `<runtime>/spyc/ssh-agents/<pane id>` and `<NAME>.session` | n/a | n/a | the path is in every jarred program's environment (H10) | no |
| Snapshots | stamped with version, commit and ghostty pin (`snapshots.rs:172-196`) | another build's binary form is refused and the portable form replays | same | same | design holds |
| Hook lines | `spyc --report-status <state>`, written once into the agent's configuration | an older spyc answers a newer state "unknown status" (`app/mcp.rs:759`), and the hook discards it (H11) | n/a | n/a | no |

## Findings

### H1. 2.4.0 restores a 3.0 session and starts every jarred program a second time

- **Severity:** high. **Frozen:** in effect. `2.4.0`'s reader can't change, so only what rc.1 and 3.0.0 write can; settle before rc.1. **Lens:** correctness (C1's invariant, "a tab's program never runs twice").
- **Sites:** `src/state/sessions/manifest.rs:20` (`MANIFEST_VERSION = 2`, the same as `v2.4.0:src/state/sessions/manifest.rs:20`); `src/state/sessions/mod.rs:123` (`jar_socket`), `:129` (`live`), `manifest.rs:61` (`tasks`), all optional additions to version 2; `v2.4.0:src/state/sessions/manifest.rs` `parse_manifest` (reads any file whose `version` is 2); `v2.4.0:src/app/session_restore.rs:361-400` (`restore_tab` spawns the command afresh and arms `/resume <sid>`); `v2.4.0` has no jar code (`git ls-tree -r v2.4.0 -- src/pane | grep -c jar` → 0) and no `attach` subcommand.
- **Claim:** a manifest written by 3.0 (rc or final) is a valid version 2 file to `2.4.0`, which ignores `jar_socket` and `tasks` and restores each tab by starting its program again, beside the jar that still runs it.
- **Evidence (probe and lines read):** serde probe `v24_reads_a_main_manifest_and_sees_no_jar`: `2.4.0`'s types read a manifest shaped as `save_released` writes it at `52ce15b7` (a claude tab and a shell tab with `jar_socket`, a jarred task, a carried `live` report, `left_column`), find both tabs, and re-save it with no `jar_socket`, `tasks` or `live` (`serde-probe-run.log`, "2.4.0 re-save"). The restore path was read, not run: no `2.4.0` binary was started.
- **Ways in:**
  - An rc user rolls back to stable `2.4.0` (`brew`, `apt`) after the rc misbehaves, with tabs still running in jars (after a crash, `^d` or `:upgrade`), and runs `spyc -r`. This is the likely one.
  - `^d` with `2.4.0` installed at spyc's path: the handover execs `spyc attach --force NAME` (`detach.rs:200`), which `2.4.0` doesn't have, so the user lands in a shell with the tabs ownerless and runs `spyc -r`.
  - `:upgrade` into `2.4.0` in the session's own jar (`detach.rs:208-222`): `2.4.0` starts as a plain spyc with nothing restored; its `spyc -r` is the case above. H4 is why `:upgrade` doesn't refuse this.
- **Consequence:** two copies of every agent, the second typed `/resume <sid>` into the conversation its jarred twin is still writing (or `codex resume <uuid>` into the same rollout), both editing the tree. `2.4.0`'s first autosave rewrites the file under the same id without `jar_socket` and `tasks`, so a return to 3.0 can't take the jars back with `-r`. Only the orphan prompt finds them, without layout and beside its `d`.
- **Status:** confirmed (probe and lines read).
- **Direction:** write `MANIFEST_VERSION` 3 from the first rc and read 2 and 3, so `2.4.0` skips every 3.0 file by its own rule ("A file from a newer spyc is skipped").

### H2. An rc's session restored by the final starts its programs again (= C1's rc-to-final path)

- **Severity:** high, as C1. **Frozen:** no. **Lens:** correctness.
- **Sites:** `src/app/pane_starts.rs:126` (`if !(jars && jar::honoured()) || …` → `Pane::launch`), `src/pane/jar/mod.rs:131-137` (`honoured_by`: true only for `-CURRENT` and `-rc.`), the other readers `mod.rs:148`, `src/app/bootstrap.rs:347`, `src/app/orphan_jars.rs:65`, `src/app/capture_starts.rs:42`; the ways in `detach.rs:175-203` (`hand_over` runs the binary installed now), `:208-222` (`upgrade_in_place`), `:340-353` (`restore_own_session`).
- **Claim:** refute-G's correction 1 holds at `52ce15b7`: #1317 touched none of these lines. A `3.0.0` build with `honoured()` in place restores an rc's session, taken over by `spyc attach` after an rc `^d` or exec'd by `:upgrade`, with every tab started afresh beside its live jar.
- **Evidence (lines read and a search):** the sites above. The search in the ledger finds no other switch on the version string a restore reads. `XTVERSION`, `--version` and the crash report only print it, and a snapshot's build stamp falls back to the portable form. Once S6 removes `honoured()`, an rc and the final differ in nothing a restore reads.
- **Consequence:** as C1.
- **Status:** confirmed by reading; open at `52ce15b7`.
- **Direction:** S6 (slice D) closes it only if `honoured()` leaves all five readers; C1's own fix covers what's left (`[pane] jars = false` in the one config file every build reads), and H1 covers the rollback to `2.4.0`.

### H3. One unknown enum value loses a whole session, and one unknown field is lost on the next save

- **Severity:** medium. **Frozen:** in effect. A downgrade from any 3.0.x meets `3.0.0`'s reader, so make it tolerant before the tag. **Lens:** reliability.
- **Sites:** `src/config/agent_hud.rs:7-14` (`AgentHudMode`, derived `Deserialize`, no catch-all), `src/state/scope_registry.rs:15-20` (`ScopeIntent`, the same), both reachable from `Manifest`; `manifest.rs:340-351` (`parse_manifest` turns any error into `None`); `mod.rs:297-300` (`load_manifests_in` drops it without a word); no `deny_unknown_fields` anywhere (`rg deny_unknown_fields src crates`: none), so unknown fields are dropped silently; `detach.rs:340-353` and `session.rs:614-640` (a restore adopts the file's id, and the autosave rewrites it whole).
- **Claim:** a `3.0.x` that adds a HUD mode or a scope intent writes manifests a `3.0.0` can't read at all. A `3.0.x` that adds a field has it dropped from the file by the first `3.0.0` save after a restore.
- **Evidence (probe):** `one_unknown_variant_loses_the_whole_session`: `"agent_hud": "minimal"` and `"intent": "reviewing"` each make `parse_manifest` return `None` ("unknown variant `minimal`, expected one of `off`, `compact`, `full`"); `"agent_kind": "gemini"` degrades and reads. `main_drops_a_field_it_does_not_know_on_its_next_save`: a tab field and a top-level field are both gone after a round trip.
- **Consequence:** after a downgrade, the session is missing from `spyc -r` and from `restore_own_session`. A spyc in its own jar then comes up empty, and its jars are offered as "no spyc owns", where `d` ends them (D1's damage, in slice E). The `AgentKind` comment (`mod.rs:36-41`) records the same loss once already, when `gemini` was dropped.
- **Status:** confirmed (probe).
- **Direction:** give every enum reachable from `Manifest` a tolerant `Deserialize`, as `AgentKind` has, with a test that walks them. Bump `MANIFEST_VERSION` for any new field whose loss changes what runs, as `jar_socket` and `tasks` did (H1).

### H4. `:upgrade`, `spyc attach`'s offer and jar upgrades take any other build for a newer one

- **Severity:** medium. **Frozen:** no. **Lens:** correctness, docs.
- **Sites:** `src/app/detach.rs:61-63` (`replaced_by`: another inode or modification time), `:139-146` (`newer_install`, used by `:upgrade` and by `spyc/behind`, `src/app/mcp.rs:454-458`); `src/pane/jar/attach.rs:133-169` (`behind`: another file, or one changed after the spyc started) and `:84` ("runs a spyc older than the one installed … Upgrade it?"); `src/app/jar_upgrades.rs:226-252` (`look`: any jar whose `build` isn't this spyc's is upgraded to this spyc's entry); `crates/spyc-jar/src/jar/describe.rs:19-27` (`jar` is `PACKAGE_VERSION`, `0.1.0` for every build until F2, so no reader can order two builds); `docs/drafts/JAR_PROTOCOL.md:763-767` ("A jar can't be upgraded back down").
- **Claim:** installing an older build makes `:upgrade` and `spyc attach` offer it as an upgrade. The spyc that then runs re-images every idle jar it owns onto its older build through the version-1 upgrade. The protocol's "can't be upgraded back down" holds only across blob versions, where the old validator refuses (`tests/jar_differ.rs`). Within version 1 a downgrade goes through, and no test runs that direction.
- **Evidence (lines read).** Ways in: a rollback from an rc, a package downgrade, or the owner's routine `make install` from an older worktree while `main`'s jars run.
- **Consequence:** every agent's jar moves onto an image the user didn't choose, bringing back what the newer one fixed (A1, A2 and B1's kind). With H1 or H3 the session itself may not come back. The prompt's text is false.
- **Status:** confirmed by reading.
- **Direction:** compare versions, not files: with F2's lockstep the `jar` field and `spyc --version` give an order. Ask before `:upgrade` to an older spyc, and never upgrade a jar to an older build; correct the protocol's sentence.

### H5. The MCP proxy in a jarred agent outlives every upgrade and keeps its own build's reply budgets

- **Severity:** medium. **Frozen:** in effect. A `3.0.0` proxy runs for as long as its agent, through every later `:upgrade`, so its assumptions bind every later server; settle before the tag. **Lens:** reliability.
- **Sites:** `src/mcp/deadline.rs:51-84` (`reply_budget` and `proxy_wait`, compiled into the proxy; SPYC-TRAP `proxy-outwaits-server-replies`), `src/mcp/mod.rs:51` (`PROXY_IO_TIMEOUT` 20 s, the wait for any tool the table doesn't name); `src/mcp/server.rs:454` (a silent server ends the proxy: `if e.silent { break; }`); `src/mcp/follow.rs:62` (one message read per request, taken as its reply), `:78-84`, `:90-110` (a follow replays only `initialize` and `initialized`, not `tools/list`).
- **Claim:** after `:upgrade` or `^d`, an agent reaches the new spyc through the proxy it started with. If a later spyc adds a tool, or lengthens a budget, past 20 s, the old proxy gives up first and ends, and the agent has no spyc tools for the rest of its life. A later spyc that sends anything unasked (`notifications/tools/list_changed`, say) puts the old proxy one reply out of step. Either way, the agent keeps the tool list of the spyc it first met.
- **Evidence (lines read).** The trap's test, `the_proxy_outwaits_every_tools_reply_budget` (`deadline.rs:95`), checks one build against itself.
- **Consequence:** an agent that survived an upgrade, the point of jars, silently loses spyc's tools on the first long call into a newer server.
- **Status:** confirmed by reading; latent until a server outgrows the `3.0.0` table, and no server-side change can then help an old proxy.
- **Direction:** before the tag, have the proxy wait on the server's word rather than its own table (the server's budgets in `initialize`'s result, or a ceiling no later budget exceeds). Let it pass unasked server messages through, and write the rule beside the trap.

### H6. No handover checks that the receiving spyc can read the session it is given

- **Severity:** low. **Frozen:** no (`spyc/release` isn't in any released spyc). **Lens:** reliability.
- **Sites:** `src/app/detach.rs:175-203`, `:208-222` (`^d` and `:upgrade` hand the session to whatever binary is installed), `src/app/project_move.rs:257` (`:detach project` starts the installed binary the same way); `detach.rs:340-353` (`restore_own_session` returns without a word when no file of its name parses); `project_move.rs:748-790` (`release_session` saves, releases the jars and quits) against `:615-623` ("the session it saved (`{id}`) isn't there", flashed by the taker afterwards); `src/mcp/mod.rs:152-165` (`spyc/release` carries no params).
- **Claim:** at `52ce15b7` every 3.0 build reads version 2, so nothing fails today. Once a release bumps `MANIFEST_VERSION` (H1's fix is one such bump), a handover into a binary from before the bump gives an empty spyc whose jars are offered as orphans. And `:attach project` from an older spyc has the giver quit before the taker learns it can't read the file.
- **Evidence (lines read).**
- **Consequence:** a session that doesn't come back, beside the orphan prompt's `d`. `spyc -r` by the newer binary recovers it.
- **Status:** confirmed by reading; latent.
- **Direction:** add to `spyc/release` the manifest versions the asker reads, and have the giver refuse what it can't write for them (free while no released spyc has the method). Have `restore_own_session` and the orphan prompt say "a session named X was saved in a form this spyc can't read".

### H7. `spyc/upgrade` is a notification, so a refusal is silent

- **Severity:** low. **Frozen:** no. **Lens:** correctness.
- **Sites:** `src/mcp/protocol.rs:114`, `:442-446`; `src/mcp/mod.rs:208-218` (`notify`); `src/pane/jar/attach.rs:176-199` (`upgrade_first` waits `UPGRADE_WAIT`, 20 s, then "hasn't come back upgraded yet; attaching anyway"); `detach.rs:111-118` (`:upgrade` can refuse, with a flash only the session's own screen shows).
- **Claim:** a session's spyc that refuses (E1's token rule once it lands, or no newer install by the time it looks) or doesn't know the method answers nothing. `spyc attach` waits 20 s and says the upgrade is late. For `shutdown` and `release` E1 is compatible both ways, because a token in params is ignored by a spyc before E1 and a refusal reaches an older caller as an error reply. Only `upgrade` has no reply to carry one.
- **Evidence (lines read).**
- **Status:** confirmed by reading.
- **Direction:** make it a request with a reply in E1's change, before rc.1, while no released spyc has it.

### H8. A running session's restore point is deleted by any other spyc's save

- **Severity:** medium. **Frozen:** no. **Lens:** reliability, correctness.
- **Sites:** `src/state/sessions/mod.rs:19` (`MAX_SESSIONS = 20`), `:311-345` (`prune_old` and `prune_victims`: the least recently written files go, whoever runs them); `src/app/session.rs:614-640` (the autosave writes only on a change, so an idle session's file ages); `manifest.rs:255-267` (`Held::by_running` sees only the files that exist).
- **Claim:** a detached session left idle is the least recently written file, so the 21st session file any spyc writes, of any build, deletes its manifest while its spyc runs. An older spyc also prunes newer files it can't read.
- **Evidence (probe and lines read):** `an_idle_running_sessions_file_is_pruned_by_another_spycs_save`, on a copy of `prune_victims`: 21 files, the running session's oldest → it is the one removed.
- **Consequence:** until the session changes, `:attach tab NAME` says it "hasn't saved its session yet". A crash or kill then leaves `spyc -r` nothing to restore, and the jars come back only through the orphan prompt. Meanwhile `Held::by_running` can't see that session's conversations, so restoring another file that holds one of them resumes into a conversation the running agent is in.
- **Status:** confirmed (probe on the copied function, and lines read); not run end to end.
- **Direction:** never prune a file whose spyc runs (`Manifest::running`), or have a running spyc rewrite its file at least as often as a prune could reach it.

### H9. The `describe` keys spyc reads are part of version 1, and no test pins them

- **Severity:** informational. **Frozen:** yes (the keys and the grammar of their values). **Lens:** tests.
- **Sites:** keys a spyc reads from a jar of another build: `owner` (`"connected"`, `"none for N s"`: `describe.rs:64`), `owner_pid`, `program`, `program_pid`, `exit`, `build` (matched against store entry names, `store.rs:302-327`), `owner_left` (`OwnerLeft::from_word`), `terminal`, `owner_mcp` (`src/pane/jar/mod.rs:259`, read by every proxy that follows a jar), and the keys spyc tells (`is`, `label`, `command`, `agent`, `project`, `project_home`, `pane_id`, `cwd`, read back by `orphans.rs:57-85`). `crates/spyc-jar/tests/wire_v1.rs` pins the `Description` frame's bytes, not these.
- **Claim:** a later spyc meets `3.0.0` jars for as long as they run, so a renamed key or a changed value makes its orphan finder, takeover, shutdown, store sweep or proxy follow misread a live jar. This is E9's inventory seen from a later reader.
- **Direction:** a test beside `wire_v1.rs` that pins each key spyc reads and the grammar of its value, from a jar of this build.

### H10. Paths in a jarred program's environment bind every later spyc

- **Severity:** informational. **Frozen:** yes in effect. **Lens:** docs.
- **Sites:** `src/pane/ssh_agent.rs:24-35` (`SSH_AUTH_SOCK` is `<runtime>/spyc/ssh-agents/<pane id>`), `SPYC_JAR_SOCKET` (the socket path, `place.rs` `socket`), `SPYC_PANE_ID`, and `SPYC_MCP_SOCK` (`mcp-<pid>.sock`).
- **Claim:** a program keeps the environment it started with across every upgrade. A later spyc that moves any of these leaves running agents with a dead `git push` agent, or a proxy that can't follow its jar (SPYC-TRAP `mcp-follows-jar`).
- **Direction:** list them in `JAR_PROTOCOL.md` (or ARCHITECTURE.md) as fixed for as long as a 3.0.0 program may run.

### H11. Hook lines name their state once, and an older spyc drops a state it doesn't know

- **Severity:** informational. **Frozen:** no. **Lens:** reliability.
- **Sites:** `src/mcp/hooks.rs:133`, `:305` (`spyc --report-status <state> 2>/dev/null || true`, written into the agent's configuration by `--install-agents`); `src/app/mcp.rs:756-760` ("unknown status").
- **Claim:** the hook runs the installed binary against whichever spyc owns the tab. A state added by a later install is refused by an older running spyc, and the hook discards the error, so the tab's dot doesn't move. This was already true in 2.x; detached sessions make the gap weeks long.
- **Direction:** a report of an unknown state counts as the nearest known one (`working`) rather than nothing.

## Pinning

`wire_v1.rs` pins bytes because a jar is a running process that can't be replaced. A manifest is a file read by builds that can't be replaced: `2.4.0` now, `3.0.0` after the tag. So pin readers, not bytes:

1. A fixture of a `3.0.0` manifest (every field set, jars and tasks included), checked in at the tag, which every later build must read with the same meaning: each `jar_socket` and task taken back, nothing started afresh. The existing `a_version_1_session_reads_as_one_project` is the model.
2. A test that every enum reachable from `Manifest` reads an unknown value (H3).
3. A list of `Manifest`'s and `SavedTab`'s fields, checked by a test, so adding one forces a decision on `MANIFEST_VERSION` (H1, H3).
4. The `describe` keys spyc reads (H9).

## Seen, and already someone else's

- **C10** (low, unverified in Unit C): confirmed by reading here. The sweep keeps only entries a live jar reports or this spyc's own (`store.rs:286-293`), and an entry is touched only at a spyc's start or when reported. A detached session's spyc whose tabs all closed has its entry removed by any other spyc after 24 hours, and its next tab then fails to start a jar. Long-lived sessions make this ordinary.
- **D1, D5:** H3 and H6 end at the orphan prompt's `d`, D1's damage. The fix in slice E (a confirmation, and no typed-ahead answer) covers these ways in too.
- **E1:** compatible both ways for `shutdown` and `release` if the token goes in params (H7). The brief's "no released spyc has these methods" stops being true once rc.1 is published, so E1 is cheapest before then, and H7 must be.

## Not covered

- Nothing was run end to end: no `2.4.0` binary restored a 3.0 session, and no `:upgrade` into an older build was run. H1's restore half and H4 rest on lines read.
- The jar protocol's own compatibility (frames, blob, the `differ` jar): Unit A's ground, taken as reported.
- `src/app/session_restore.rs` at `52ce15b7` beyond `restore_session`'s signature; Unit D and refute-G read it at the pin.
- Other state a later spyc reads but no session depends on: marks, picks, inventory, history, harpoon, graveyard, frecency, `pager_positions.json`, `tab_consent.json`, `wiring.json`, the skill's manifest, context files (`src/context.rs`).
- The agent wiring in each agent's own configuration (`src/wiring/`), beyond the hook line's state name.
- The `Lua` startup and config file across versions (`[pane] jars` aside, under H2).
- Linux: nothing here depends on the platform, and nothing was run there.
