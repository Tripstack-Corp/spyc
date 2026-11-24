# Unit G. Everything else in the diff, and the documents

**SHA:** `444857a2` (`main`, `2.5.0-CURRENT`), read in the worktree `~/src/spyc.worktrees/review/3.0`.
**Platform:** macOS (Apple silicon) for every read and command. Nothing in this unit was run on Linux; the Linux-only claims below (H7) are from reading and are marked so. No spyc or spyc-jar process was started by this unit, so no jar, socket or token was created or touched.
**Method:** the unit's code was split into five reading groups (crash report and `lib.rs`; #1316 column state; #1282 agy status line and the HUD; pane, keys and scrollback; the rest: environment, event log, worktree picker, loaders, image probe, config), each read whole by a sub-reviewer, and the SPYC-TRAP check was split in three. Every finding at medium or above, and every one marked `confirmed`, was re-read at its sites by me before it went in; the ones I did not re-read are marked `unverified`. The documents were read by me.

## Verdict

Outside the jar's own code, nothing in this unit corrupts or loses a user's work in ordinary use, and no STOP item was found. Two things should be settled before the tag. First, the documents and the code agree with each other and disagree with the plan: every user page says jars run "on development builds", the config template says the key is read only on `-CURRENT` and rc builds, and `pane::jar::honoured()` makes that true, so a build versioned `3.0.0` would ship with jars off and with `^d` refused whenever a tab is open (G1, the documents' side of lead V1). Second, the one path the code leaves open in the jar-or-nothing rule: a restore in a spyc that runs tabs without jars (`[pane] jars = false`, a build where `honoured()` is false, or the jarless offer taken) starts a second copy of a program whose jar still runs (G2); with V1 as it stands, that is every rc user's first `spyc -r` after a crash once they install 3.0.0. The rest is medium and low: a plain `:setenv` value written to disk through the event log (G3), program status reports that a frame's own drain can hide (G4), task and capture signals that still go to a pid rather than through the jar (G5), a crash report that tells the user it names none of their files while it does (G7), and a set of false or stale sentences in ARCHITECTURE.md's trap rationales and in the frozen protocol's text (G8, G9). Every SPYC-TRAP slug added since `v2.4.0` (21 of them) is anchored on both sides; their one-liners hold except one (`jar-or-nothing` at `src/pane/pty_host.rs:241`), and two of the invariants are broken outside the files their guards scan (G2, G5).

## Coverage ledger

"Read in full" means the whole file at the pin, plus its `git diff v2.4.0..HEAD`. "Diff" means the diff since `v2.4.0` read whole, with the surrounding code as needed. Files in `git diff --stat v2.4.0..HEAD` that Units A to F own are not listed.

### Code

| File | Status |
|---|---|
| `src/crash_report.rs` | read in full |
| `src/lib.rs` | read in full (CLI, `run`, `finish`, the panic hook) |
| `src/bin/spyc-jar.rs`, `src/mcp_cmd.rs`, `src/paths.rs`, `src/stderr_capture.rs`, `src/fs/atomic.rs` | read in full |
| `src/app/state/restore.rs`, `src/app/state/mod.rs`, `src/app/state/project.rs`, `src/app/state/flash.rs`, `src/app/harness_tests/column_state.rs` | read in full |
| `src/app/state/dispatch.rs` | diff; lines 1-186 and 396-425 read |
| `src/app/session.rs`, `src/app/session_restore.rs` | the column-state parts (`session.rs` 195-640, `session_restore.rs` 1-720); the rest is Unit D's |
| `src/state/sessions/mod.rs`, `src/state/sessions/manifest.rs` | lines 1-260 of each (the column-state parts); the rest is Unit D's |
| `src/agent/agy_statusline.rs`, `src/agent/agy_metrics/{mod,reducer}.rs`, `src/agent/claude_statusline.rs`, `src/ui/agent_hud/agy.rs`, `src/ui/agent_hud/resets.rs`, `src/app/agent_hud.rs`, `src/app/agent_hud/diagnostics.rs` | read in full |
| `src/agent/waiting.rs` | production half (1-258) read; tests skimmed |
| `src/agent/mod.rs`, `src/ui/agent_hud.rs`, `src/ui/agent_hud/claude.rs`, `src/app/agent_status.rs`, `src/agent/claude_metrics/{mod,records,reducer}.rs`, `src/agent/codex_command.rs` | diff |
| `src/pane/launch.rs`, `src/pane/kitty.rs`, `src/pane/tabs/program_status.rs`, `src/pane/engine.rs`, `src/pane/engine_ghostty/{keys,replies,jar_fidelity}.rs` | read in full |
| `src/pane/mod.rs`, `src/pane/tabs.rs`, `src/pane/engine_ghostty/mod.rs`, `src/pane/input.rs`, `src/pane/widget.rs`, `src/app/pane_input.rs`, `src/app/pane_wake.rs`, `src/app/pane_scroll.rs`, `src/ui/scrollback.rs`, `src/app/tasks.rs`, `src/app/run.rs`, `src/app/runtime.rs`, `src/app/bootstrap.rs`, `src/app/streaming.rs` (drain), `src/app/render/mod.rs` (`prepare_panes`) | diff, and the functions each finding depends on |
| `src/envset.rs`, `src/shell/assignments.rs`, `src/app/state/events.rs`, `src/app/events.rs`, `src/state/event_log.rs`, `src/app/worktree_picker.rs`, `src/git/last_updated.rs`, `src/app/git_view_session/loader.rs`, `src/ui/loader.rs`, `src/terminal/image_probe.rs`, `src/app/cooking.rs`, `src/app/mouse/project_drag.rs` | read in full |
| `src/shell/mod.rs`, `src/app/git_view_session.rs`, `src/ui/project_chips.rs`, `src/app/mouse/selection.rs`, `src/clipboard.rs`, `src/config/mod.rs`, `src/config/load.rs`, `src/config/default.spycrc.toml`, `src/keymap/action.rs`, `src/keymap/action/names.rs`, `src/keymap/resolver/{mod,hints}.rs`, `src/app/commands.rs`, `src/app/command_table.rs`, `src/ui/status.rs`, `src/app/activity_diagnostics.rs`, `src/ui/help.rs` | diff |
| `src/app/detach.rs` (Unit D's) | lines 60-170 read, for the documents' claims about `^d` and `:upgrade` |
| `src/app/pane_starts.rs` 100-240, `src/app/pane_tabs.rs` 320-365, `src/app/jarless.rs` 120-160, `src/pane/jar/mod.rs` 95-160 (Unit C's) | read, for the jar-or-nothing check and V4 |
| `src/app/git_state.rs`, `src/app/harness_tests/{kitty_keys,program_status,setenv,projects_order,project_tabs,jar_captures,jar_orphans}.rs`, `src/app/mouse/*_tests.rs`, `src/ui/project_chips.rs` tests | skimmed |
| the rest of the `src/` diff in Unit G's share (archive, git, fs, state, keymap tests, `src/ui/{diff_render,json,wrap,prompt,theme,status_segments}.rs`, `src/app/{actions,menus,matcher,vsplit,…}.rs` one- to twenty-line diffs) | skimmed: renames from the brand sweep, `{e:#}` changes, doc fixes |
| `crates/spyc-vt-sys/` (pin bump, `LIBC_SYMBOLS`), `scripts/`, `spikes/`, `.cargo/config.toml`, `rust-toolchain.toml`, `.aislop/` removal | `crates/spyc-vt-sys/vendor/LIBC_SYMBOLS` and `scripts/unexport-libc.py` read for the `vendored-libc-symbols` trap; the rest not read (build and release tooling is Unit F's) |

### Documents

| Document | Status |
|---|---|
| `docs/features/projects.md`, `docs/features/pane.md` (jar sections, 300-413), `docs/features/shell.md` (diff and its jar, environment and event-log sections) | read in full |
| `README.md`, `INSTALL.md`, `FEATURES.md`, `CONFIGURATION.md` | diff read; jar, install and uninstall sections read; the rest skimmed |
| `docs/features/{agents,browsing,mouse,pager}.md`, `docs/config/{agent-hud,keymap,lua,mouse}.md` | diff read; `docs/config/agent-hud.md` read in full |
| `docs/features/archives.md`, `docs/config/{notifications,projects}.md` | not read (unchanged or unrelated to 3.0) |
| `docs/KEYBINDINGS.md`, `src/ui/help.rs` | diff read; every jar, detach and attach row checked against the resolver and `COMMAND_TABLE` |
| `ARCHITECTURE.md` | every section holding a SPYC-TRAP anchor added since `v2.4.0` read in full (lines 341-378, 692-1231, 1336-1393), plus "spyc's stderr follows the screen"; the rest not read |
| `ROADMAP.md` | diff read; the 3.0 horizon, decisions log entry and doc map read |
| `docs/drafts/JAR_PRODUCTION_PLAN.md` | read in full |
| `docs/drafts/JAR_PROTOCOL.md` | "Principles", "Use cases", "When the ring fills", "Tasks and captures", "Meeting another version" and the decisions list read; the wire format is Unit A's |
| `crates/spyc-jar/src/cli.rs` (module doc), `crates/spyc-jar/src/state.rs` (1-80) | read, for V4 and the 4 MB capture claim |

## Findings

Ordered by severity within the code findings (G1-G8), then the documents (G9-G15). "Owner" names the unit whose files hold the code, where it isn't G's; those findings were found while checking G's documents and traps, and are reported here so they are not lost.

### G1. high. The release's user documents describe jars as a development feature, and as coded a `3.0.0` build refuses `^d` with any tab open

- **Lens:** docs, correctness. **Frozen:** no. **Owner:** the code is V1's (Unit F); the documents are G's.
- **Sites:** `src/pane/jar/mod.rs:128-137` (`honoured`, `honoured_by`); `src/app/pane_starts.rs:126`; `src/app/detach.rs:83-92` (`detach`) and `:148-166` (`unjarred`); `FEATURES.md:67` ("On development builds each tab's program runs in a jar"); `docs/features/pane.md:338` ("On development (`-CURRENT`) and release-candidate builds"); `src/config/default.spycrc.toml:110-117` ("Read on development (-CURRENT) and release-candidate builds only"); `CONFIGURATION.md:89-90`; against `FEATURES.md:103` ("`^d` detaches; `spyc attach` returns"), `docs/features/projects.md:248-266`, `docs/KEYBINDINGS.md:333`, `src/ui/help.rs:570-571`.
- **Claim:** every user page and the config template say jars are read only on `-CURRENT` and rc builds, which is what `honoured()` does, so the release that the plan's decision 1 says turns jars on would, as the tree stands, run every tab outside a jar, and `^d`, which the same pages present unconditionally, would then refuse with "can't detach: the tab … runs outside a jar and would end with spyc".
- **Evidence (lines read):** `honoured_by(version)` is `version.contains("-CURRENT") || version.contains("-rc.")` (`mod.rs:135-137`); `launch_tab_pane` returns `Pane::launch` when `!(jars && jar::honoured())` (`pane_starts.rs:126-128`), so every tab's `host.jarred()` is false; `detach` in a plain spyc calls `unjarred()` first, which returns the first open tab not `jarred()` (`detach.rs:88-92`, `:149-160`). Nothing in `session::start` or `detach.rs` reads `honoured()` (`rg -n 'honoured' src` finds only `bootstrap.rs:347`, `capture_starts.rs:42`, `pane_starts.rs:126`, `orphan_jars.rs:65` and `pane/jar/mod.rs`), so on such a build `^d` works only with no tab open. The doc comment on `honoured` (`mod.rs:128-130`) says "until jars are on by default", which `BY_DEFAULT` (`mod.rs:100`) already is.
- **Consequence:** a new user who installs 3.0.0 reads that jars are a development feature, and finds that the detach the front page promises refuses with their first tab open; or, if S6 lands without the documents, reads that the feature they are running is not in their build.
- **Status:** confirmed (lines read). The release version string is not in the tree, so the `3.0.0` case is `honoured_by("2.5.0")`'s, which `src/pane/jar/tests.rs:565` asserts false.
- **Direction:** remove the gate (S6) and the three "development builds" sentences in one change, and make `make release-prep` fail while `honoured` exists.

### G2. high. A restore in a spyc that runs tabs without jars starts a second copy of a program whose jar still runs

- **Lens:** correctness. **Frozen:** no. **Owner:** Unit C (`pane_starts.rs`); found checking SPYC-TRAP `jar-or-nothing`.
- **Sites:** `src/app/pane_starts.rs:126-128`; `src/app/pane_tabs.rs:344,359` (the restore's spawn passes `restoring` in); `src/app/jarless.rs:137-144` (the one path that checks); `src/app/orphan_jars.rs:64-66`; `ARCHITECTURE.md:1071-1072` ("A restored tab whose jar may still run is never started that way") and `:1121` ("A tab's program never runs twice").
- **Claim:** `launch_tab_pane` launches the program on spyc's own PTY whenever jars are off for this spyc, without looking at `restoring`, so a restored tab whose saved `jar_socket` names a live jar gets a fresh program beside the one still running there.
- **Evidence (lines read):**
  ```rust
  if !(jars && jar::honoured()) || self.runtime.jarless.chosen {
      return Pane::launch(launch, wake);
  }
  ```
  `restoring` is used only below this line (`Taking::Start(restoring…)`, `pane_starts.rs:143`). The only guard against a live socket is in `start_without_jar` (`jarless.rs:137-144`: `socket_state(s)` `Live` or `Unknown` returns false), which serves the tabs waiting on the offer, not later restores. `rg -n 'jar_socket' src/app/session_restore.rs` finds only line 810 (`jar_socket: None`, a new tab), so nothing on the restore side skips such a tab. The three ways in: `[pane] jars = false` in the user's file; a build where `honoured()` is false (V1, a release); the jarless offer taken earlier in the session. The paths that reach it: `spyc -r` (`session_restore.rs` → `spawn_pane_tab`), `:attach project` and `:attach tab` (`restore_out_of_view`, `restore_tabs_here`), and the orphan pop-up's `r` when the offer was taken after the look (`orphan_jars.rs:66` checks `chosen` only when it looks). On a build where `honoured()` is false the orphan look itself is off (`orphan_jars.rs:65`), so the old jars also run on unannounced.
- **Consequence:** a Claude Code tab is restored as a fresh `claude` that types `/resume <id>` into the conversation its jarred twin is still working in: two agents in one conversation, both editing the tree. With V1 unchanged this is the first `spyc -r` after a crash or a `^d` for every rc user who installs 3.0.0.
- **Status:** confirmed by reading; not run.
- **Direction:** decide in `launch_tab_pane`, before the native launch, what a `restoring` tab whose socket is `Live` or `Unknown` gets (left `[detached]`, as a refused adoption is), and test it through `spyc -r` with `jars = false`.

### G3. medium. A plain `:setenv` value is written to disk, in the session's event log

- **Lens:** security, docs. **Frozen:** no.
- **Sites:** `src/app/state/dispatch.rs:414-421`; `src/app/state/flash.rs:19-22`; `src/app/events.rs:66-92`; `src/state/event_log.rs:90-104`; `src/app/state/flash.rs` `note_tab_started`; `docs/features/shell.md:285-293`.
- **Claim:** `:setenv NAME=VALUE` flashes `setenv NAME=VALUE` through `flash_info`, which notes every flash in the event log, which is appended to `<state>/events/<SESSION>.log`; the page says the plain form "lasts until spyc exits" and steers secrets away from `-p` toward it.
- **Evidence (lines read):** `self.flash_info(… format!("setenv {name}={value}"))` (`dispatch.rs:420`; the `-p` form at :417 echoes the value too); `flash_info` does `self.events.note(EventKind::Info, &text)` (`flash.rs:19`); `settle_events` sends `Write::Append(…)` (`events.rs:92`). `note_tab_started` logs `tab started: {command}`, so a tab command written `FOO=token claude` (supported since #1236) lands there too. `spyc events` prints the file, and the log survives a crash, `^d` and `:upgrade` by design.
- **Consequence:** a token set with the form the documents recommend for tokens stays on disk, in a file the user is told is a history of status messages.
- **Status:** confirmed (lines read).
- **Direction:** flash `setenv NAME` without the value, and log a tab's command with its assignments' values elided.

### G4. medium. A program's status report can sit unread when the frame's drain sees its output first

- **Lens:** correctness. **Frozen:** no.
- **Sites:** `src/app/streaming.rs:390-397`; `src/pane/tabs.rs:802-809` (`drain_all`); `src/app/render/mod.rs:330-332, 404-405`; `src/pane/mod.rs:231-250` (`drain_output`); `src/app/run.rs:683-790` (loop order).
- **Claim:** OSC 7501 reports and prompt marks are taken only when the loop's drain finds a new output generation, but `prepare_panes` also calls `drain_output` on every tab of the project in view, which records the generation as seen without taking the events, so output that lands between the two drains leaves its events in the engine until the program prints again.
- **Evidence (lines read):** `if entry.pane.drain_output() { … for event in entry.pane.take_program_events() …}` (`streaming.rs:390-396`) is the only production caller of `take_program_events` (`rg -n take_program_events src`). `drain_all` calls `drain_output` and discards everything but `has_activity` (`tabs.rs:804-808`). `drain_output` sets `last_seen_gen = gen_now` (`pane/mod.rs:244-249`). The comment at `render/mod.rs:330-332` assumes the second drain "is a no-op when nothing changed", which no longer covers the events added in 3.0. The window is open on every dirty frame, and while any agent works the dot animation keeps frames dirty at 10 Hz.
- **Consequence:** a program that reports `blocked` or `done` and then goes quiet can leave its tab showing the old state; a later keystroke acknowledges first and the late report lands after.
- **Status:** confirmed (mechanism, lines read); how often it bites is unverified.
- **Direction:** take program events whatever `drain_output` returns, or give render's drain a seen-generation of its own.

### G5. medium. Task and capture signals go to the pid the jar reported, not through the jar

- **Lens:** correctness, reliability. **Frozen:** no. **Owner:** Unit C (the trap's guard); found checking SPYC-TRAP `jar-signals-through-the-jar`.
- **Sites:** `src/app/tasks.rs:285-298` (pause, `STOP`), `:321-331` (interrupt, `INT`), `:354-366` (resume, `CONT`), `:429-434` (to a pane, `CONT`), `:619-622` (`:fg`, `CONT`); `src/app/pager_handler/pager_action.rs:285-292` (`^C`) and `:314-337` (`^\`, `KILL` to the foreground group); `src/app/effect/executor.rs:159, 201`; `src/app/util.rs:283` (`kill_pg`); `src/pane/jar/link.rs:420-422`; `ARCHITECTURE.md:1207-1218`.
- **Claim:** 3.0 runs `!` captures, and the tasks made from them, in jars, but every task and capture signal is sent with `kill_pg(pid)` to `host.process_id()`, which for a jarred host is the pid the jar described, while the trap's rationale says a jarred program's signals go only as a frame to its jar.
- **Evidence (lines read):** `JarChild::process_id` returns `Some(self.0.pid)` (`link.rs:420-422`); `pause_task` builds `Effect::SignalGroup { pid, sig: STOP }` from it (`tasks.rs:285-298`) and the executor calls `kill_pg(pid, sig)` (`executor.rs:159`); `interrupt` calls `kill_pg(pid, INT)` directly (`tasks.rs:329`). The guard `every_signal_goes_through_the_jar` (`src/pane/jar/tests.rs:186`) drives only `PtyHost`. Search: `rg -n 'kill_pg|Signal::' src/app`.
- **Consequence:** while the program lives this works; once it has exited and spyc has not yet drained the exit (a jarred exit arrives after its output), `:pause`, `:resume` or `^C` reach whatever process group took the pid, the failure the trap names.
- **Status:** confirmed (lines read).
- **Direction:** route task and capture signals through `PtyHost`'s jar `Signals` as `^z` does, and extend the guard to `src/app`.

### G6. medium. A restored cursor can be moved again by a later rebuild that keeps the row number

- **Lens:** correctness. **Frozen:** no. PR #1316.
- **Sites:** `src/app/state/restore.rs:42-56`; `src/app/git_state.rs:141-145`; `src/app/config.rs:206-211`; `src/app/state/listing.rs:206-222` (the `=git` filter); `src/app/vsplit.rs:234`.
- **Claim:** `restore_column` places the cursor by file name against the rows that exist at restore time, and the rebuilds that follow on their own (a git status walk landing off-thread, a project config's `ignore_masks`) call `rebuild_rows_for(side)`, which does not place it by name again.
- **Evidence (lines read):** `apply_git_worker_result` ends in `self.state.rebuild_rows_for(side)` with no `focus_on_path` (`git_state.rs:141-145`, read). Column `b` always starts with an empty git cache (`vsplit.rs:234`), and under `=git` its rows are empty until the walk lands (`listing.rs:206-222`), so the name can't be found and the cursor stays at row 0. `App::test_app` runs git synchronously (`git_worker_available: false`), so `harness_tests/column_state.rs` cannot see this.
- **Consequence:** after `spyc -r` or a handover the cursor can sit on another file than the one saved, and the next `dd` acts on that file.
- **Status:** confirmed by reading: `rebuild_rows_for` rebuilds the rows and only clamps the cursor's index (`src/app/state/listing.rs:47-100`). The `=git` case for column `b` follows from the lines; the ghost-row and config cases were not probed.
- **Direction:** keep the restored name as a pending target until the column's first git result has landed.

### G7. low. The crash message says the report names none of the user's files; the report keeps them

- **Lens:** security, docs. **Frozen:** no. PR #1302.
- **Sites:** `src/crash_report.rs:311-314`; against `src/crash_report.rs:9-11` and the module's own test (`s.report().contains("/Users/alice smith/work/secret-plans.txt")`).
- **Claim:** the printed text "It names none of your files or what was on screen" describes the scrubbed link, but the file keeps the panic message unscrubbed, as the module doc says and the test asserts.
- **Evidence (lines read):** `report()` uses `self.message` as given (`crash_report.rs`, `text_with(location, &self.message, …)`); only `issue_url` calls `scrub`.
- **Consequence:** a user who trusts the message attaches the file, with paths and their user name, to a public issue.
- **Status:** confirmed.
- **Direction:** "The link names none of your files; the report may."

### G8. low. Smaller code findings

Each is low; status as marked. Sites are at the pin.

| id | claim | sites | evidence | status |
|---|---|---|---|---|
| G8a | `issue_url` can loop for ever inside the panic hook: once the actions are gone and the message is `…`, `truncate(len/2)` then `push('…')` leaves it unchanged, and the environment fields (`TERM`, `TERM_PROGRAM`, its version) are unbounded and appear twice | `src/crash_report.rs:200-212`, `:214-225` | lines read | confirmed; needs about 1.3 KB of terminal variables |
| G8b | a panic in `spyc attach` leaves the terminal raw: the client sets raw mode with rustix (`crates/spyc-jar/src/attach.rs:67-71`) and restores only on the normal return; the hook's `disable_raw_mode` is crossterm's, which acts only when crossterm enabled raw mode (crossterm 0.29 `terminal/sys/unix.rs:148-157`) | `src/lib.rs:378-381`, `src/terminal.rs:552`, `crates/spyc-jar/src/attach.rs:66-81` | lines read, crossterm source read | confirmed |
| G8c | panics off the main thread are only queued, and only the TUI's `finish` prints the queue, so in `spyc attach`, `spyc events`, `spyc shutdown` a worker panic is silent; a dead `relay_output` thread in `spyc attach` leaves the client waiting with no message | `src/lib.rs:379-401`, `:546`; `crates/spyc-jar/src/attach.rs:193, 222, 250` | lines read by a sub-reviewer; hook re-read by me | unverified for the attach hang |
| G8d | an input-reader panic ends the TUI without a crash report, though the module doc promises one for "a panic that ends the TUI" | `src/crash_report.rs:1`, `src/lib.rs:379-401`, `src/app/proc.rs:43-62` | lines read | confirmed |
| G8e | a second panic in the same second (a `Drop` panicking while unwinding) overwrites the first report: the name is the second's stamp and the pid, opened to truncate | `src/crash_report.rs:287-292` | lines read | confirmed mechanism |
| G8f | `:upgrade` in place truncates the earlier image's stderr log: the log is `<pid>.log`, opened with `TRUNC`, and the `exec` keeps the pid | `src/stderr_capture.rs:48-58`, `src/app/detach.rs` `upgrade_in_place` | lines read | confirmed |
| G8g | `^\` on a jarred capture blocks the loop up to `EXIT_WAIT` (2 s): `kill_capture` calls `capture.host.child.wait()` on the UI thread (SPYC-TRAP `jar-off-the-loop`'s gap) | `src/app/pager_handler/pager_action.rs:318-319`, `src/pane/jar/link.rs:170, 405-412` | lines read | confirmed |
| G8h | a pane taken back from its jar (restore, `:upgrade`, `:attach`) starts with `kitty_owner: None`, and the first drain makes the foreground group the owner, so a shell in front of a `^z`'d kitty program gets kitty-encoded keys (`^C` as `CSI 99;5u`), the case SPYC-TRAP `kitty-keys-follow-the-owner` exists for | `src/pane/mod.rs:194`, `src/pane/kitty.rs:16-22, 69-77`, `src/pane/launch.rs:124-128` | lines read | confirmed mechanism; rare |
| G8i | the image probe drops every byte before its DA1 reply, so keys typed while `App::new` runs are lost; and its 2 s backstop decides which probe a late reply ends, on start and on each attach | `src/terminal/image_probe.rs:47-61`, `src/lib.rs:511-522`, `src/terminal.rs:466-475, 499-500` | lines read by a sub-reviewer | unverified |
| G8j | event text keeps control characters (`note` replaces only `\n`) and `spyc events` writes it to stdout raw, so a directory name with escapes reaches the terminal | `src/app/state/events.rs:77`, `src/state/event_log.rs:223-233` | lines read by a sub-reviewer | unverified reachability |
| G8k | a `Working` record from OSC 7501 is cleared only by a prompt mark (OSC 133), which spyc doesn't inject; a reporter killed by `^C` leaves the tab working and keeps the 10 Hz animation armed, against "0 draws at idle" | `src/pane/tabs/program_status.rs:40-45, 71-78`, `src/app/agent_status.rs:654-674` | lines read by a sub-reviewer | unverified |
| G8l | the release of a pane-prefix chord's last key (`^a n`) reaches the newly active program when it asked for releases; only the focus chords are stamped | `src/app/pane_input.rs:68-92`, `src/app/key_dispatch/mod.rs:387-389` | lines read by a sub-reviewer | unverified |
| G8m | a `StopFailure` while the live report is `Waiting` and no turn is open is classed as outside a turn, so a woken background turn that fails first leaves the tab `waiting` | `src/agent/waiting.rs:88-101`, `src/app/mcp.rs:893-894` | lines read by a sub-reviewer; deliberate per its test (`waiting.rs:276-279`) | unverified (needs a live Claude probe) |
| G8n | agy's model label is checked after `display_name` is preferred, so a display name with a character outside the allow-list drops to no model and Gemini's quotas are shown; buckets are cut to 8 alphabetically, disabled ones counted | `src/agent/agy_statusline.rs:176-178, 197-217, 243-246, 296-298` | lines read by a sub-reviewer | unverified |
| G8o | restoring picks is quadratic (each pick scans the listing) on the UI thread, and the autosave fingerprint now builds a set of every pick on each pass | `src/app/state/restore.rs:42-46, 61-68`, `src/app/session.rs:578-582` | lines read by a sub-reviewer | unverified (cost not measured) |
| G8p | column `a` applies its saved state when the saved path equals the landing candidate (before canonicalizing), column `b` when the canonical `listing.dir` does | `src/app/session_restore.rs:366-371, 647-653` | lines read by a sub-reviewer | unverified |
| G8q | `W l` waits for every worktree's full walk and drops the result without a word if the column moved; repeated presses stack walks | `src/app/worktree_picker.rs:47-62, 133-168` | lines read by a sub-reviewer | unverified |
| G8r | the default config template still says `scrollback_line_numbers` defaults true (`config/mod.rs:463` is false since #1301), and the `jars` block now sits above the indented paragraph that describes `preview_pasted_images`, so that paragraph reads as `jars`' | `src/config/default.spycrc.toml:110-123, 166-171` | lines read | confirmed |
| G8s | closing the git loader stops only highlighting; blame and the diff models run to the end, against the module doc's "the worker stops at its next file" | `src/app/git_view_session/loader.rs:4-5`, `src/app/git_view_session.rs:365-368, 536-541` | lines read by a sub-reviewer | unverified |
| G8t | `split_assignments` splits on Unicode whitespace (NBSP), which shells treat as part of a word, so `FOO=a<NBSP>b claude` runs `b claude` | `src/shell/assignments.rs:23, 79` | lines read by a sub-reviewer | unverified |

### G9. medium, frozen (text). Three sentences, two inside the frozen specification, say spyc runs a program without a jar when the jar fails

- **Lens:** docs. **Frozen:** yes, as text: the sentences sit in `JAR_PROTOCOL.md`'s principles and its "Meeting another version", and in the module doc of the frozen crate's `cli.rs`; the behaviour they describe is not frozen and the code does not do it. Lead V4.
- **Sites:** `docs/drafts/JAR_PROTOCOL.md:62-63` ("**if the jar machinery fails,** spyc hosts the pane itself, as it does today, and notes it in `:stderr` rather than prompting"); `docs/drafts/JAR_PROTOCOL.md:759-761` ("a jar that doesn't speak it exits with 75 before anything starts, and spyc runs the program without a jar rather than fail the tab"); `crates/spyc-jar/src/cli.rs:15-19` ("so a spyc that finds a newer or older `spyc-jar` beside it can tell, and run the program another way"). Against: `JAR_PRODUCTION_PLAN.md` decision 6 ("spyc never launches it another way"), its S2a ("a jar that exits with 75 … fails the tab"), `ARCHITECTURE.md:1044` (SPYC-TRAP `jar-or-nothing`), `src/pane/jar/mod.rs:425-427`, and every user page (`docs/features/pane.md:342-345`: "A tab whose jar can't start says why and ends `[not started]`… offers, once a session, to run tabs without jars").
- **Claim:** these three sentences describe the fallback the owner rejected on 2026-11-15; the code is right and the text is wrong.
- **Evidence (lines read):** a failed start lands as an error and a `[not started]` tab, and a `NoUsableJar` raises the once-a-session prompt (`src/app/jarless.rs`), never a native launch: `launch_tab_pane` runs `Pane::launch` only when jars are off or the user chose the offer (`src/app/pane_starts.rs:126-128`). A path-by-path check (tabs, `^a c`, restart, startup tabs, fork, `-r`, retake, captures, `:make`, restored tasks, orphan `r`) found no unasked fallback; overlays and the foreground `$EDITOR`/`$`/`;` run outside jars by design. The one hole in the rule is the opposite direction, G2. A search for other such sentences (`rg -i "without a jar|another way|hosts the pane itself|fall ?back"` over the tree, archives excluded) found no fourth.
- **Consequence:** a later reader, or an agent, takes the frozen text as the contract and restores the fallback; and the published crate's doc tells `spyc-jar`'s readers something spyc does not do.
- **Status:** confirmed.
- **Direction:** amend the three sentences to the decision; text changes are not wire changes.

### G10. medium. No document says that a login manager can end a detached session

- **Lens:** docs. **Frozen:** no. Lead H7 (the documents' half).
- **Sites:** `docs/features/projects.md:248-253` ("leaves spyc running and frees your terminal… Your agents keep working"); `docs/features/pane.md:363` ("so it outlives the terminal it was started in"); `ROADMAP.md:266-268` ("Detach, close the laptop, let the agents keep working"); `INSTALL.md:13-14` ("WSL2… behaves like any other Linux install").
- **Claim:** the pages promise that a detached spyc survives the terminal and the login that started it, and nothing in the tree says where that does not hold: systemd-logind with `KillUserProcesses=yes` ends every process of a session at logout, whatever it did to leave its process group, and WSL ends a distro's processes when it shuts the distro down after its last terminal closes.
- **Evidence:** `rg -n -i 'KillUserProcesses|loginctl|enable-linger|systemd-run'` over the whole tree finds nothing; `rg -n -i wsl` over the documents finds only install and clipboard notes. macOS's own limit is documented (`projects.md:319-322`, local-network access), so the pattern of saying where it does not hold exists.
- **Consequence:** on such a Linux host, or on WSL, a user detaches, closes the terminal and logs out, and finds the session and every agent ended, with no word in the documents.
- **Status:** the absence is confirmed; whether a jar survives these on a given host was not probed (no logind session in the review container). See the manual checklist.
- **Direction:** a paragraph in "Detaching and attaching" naming `loginctl enable-linger` and WSL's idle shutdown.

### G11. low. ROADMAP.md dates the freeze to 2026-11-12, names a command that doesn't exist, and leaves the jar plans out of its doc map

- **Lens:** docs. **Frozen:** no. Lead V3's ROADMAP half.
- **Sites:** `ROADMAP.md:281` ("froze at version 1 on 2026-11-12"), `:778-779` ("Version 1 froze on this date"), `:811` (doc map: "Draft — … version 1, frozen 2026-11-12"); `:268` ("`spyc -a` restores the client"); `:29` ("Where we are (v2.1.1)"); the doc map (`:785-831`).
- **Claim:** the protocol reopened on 2026-11-12 and 2026-11-15 and froze again on 2026-11-24 (`JAR_PROTOCOL.md:3-12`, decision N), but the roadmap gives only the first date; `spyc -a` is not a flag (`Cli` has no `-a`; the command is `spyc attach`, `src/lib.rs:229-245`); the doc map lists `JAR_PROTOCOL.md` and `PANE_JAR_SPIKE.md` but not `JAR_PRODUCTION_PLAN.md` or `JAR_UPGRADE_BRIEF.md` (`grep -n 'JAR_PRODUCTION_PLAN\|JAR_UPGRADE_BRIEF' ROADMAP.md` finds nothing); the "Where we are" heading is four releases old.
- **Status:** confirmed.
- **Direction:** date the freeze 2026-11-24 with the history in one clause, and `spyc attach`.

### G12. low. Pages still say `^D` quits

- **Lens:** docs. **Frozen:** no.
- **Sites:** `docs/features/shell.md:179` ("writes the directory you were browsing … when you quit (`Q`, `^D`, `ZZ` or `:q`)"); `docs/features/shell.md:280` ("The quit confirmation (`Q`/`^D`) counts backgrounded running tasks").
- **Claim:** `^d`/`^D` detach (`src/keymap/resolver/mod.rs:378`), and a detach does not write the cwd file: `write_cwd_file` is called only from `quit_now` (`src/app/session.rs:742-747`), and the handover returns from `finish` before teardown (`src/lib.rs:539-543`).
- **Status:** confirmed.
- **Direction:** drop `^D` from both lists; say what `--cwd-file` does on a detach.

### G13. low. The front page and the install guide say nothing of jars

- **Lens:** docs. **Frozen:** no.
- **Sites:** `README.md` (`rg -n jar README.md` finds nothing; "Running multiple agents", lines 189-203, describes `spyc -r` and crashes, not detaching); `INSTALL.md:253-254` ("Before uninstalling spyc…").
- **Claim:** 3.0's headline, that tabs outlive spyc and spyc outlives its terminal, is not on the front page; and the uninstall note doesn't say that running jars outlive an uninstall (by design, plan S3) or that the jar store `jar-bin/` stays in the state directory, nor how to see them (`spyc --jars`) and end them (`spyc shutdown`, the orphan pop-up's `d`).
- **Status:** confirmed.

### G14. low. False or stale sentences in the trap rationales, a missing anchor, and guard gaps

Lens: docs, maintainability. Each sentence was checked against its code by reading; the table in "SPYC-TRAP" below has the per-slug verdict.

| ARCHITECTURE.md | sentence | what the code does |
|---|---|---|
| 352-355 | probes run "once at startup, before the input reader spawns, so nothing races it" | `terminal::reattach` probes again on every attach, with the reader parked (`src/terminal.rs:486-499`, `src/app/proc.rs:454`); the same "ONCE" wording is at `src/lib.rs:518-521`, `src/app/runtime.rs:362-364` (which also names the unused `from_query_stdio`) and ARCHITECTURE.md:393, 405 |
| 362 | "A timer ends it only for a terminal that answers nothing" | the 2 s `BACKSTOP` also ends it for a terminal that answers late (`src/terminal/image_probe.rs:47-51`), whose replies then reach the input reader (G8i) |
| 805-806 | `UPGRADE_AT_MOST` is "the jar's longest path to one" | `limits.rs:30-32` leaves out `one_file`'s copy and `fsync` and the new image's start to `Upgraded`; only the 10 s margin covers them |
| 854-855 | the guarded waits are "(`WRITE_STALL`, `ADOPT_WAIT`)" | the assertion also covers `LINK_WAIT` and `TAKEOVER_WAIT` (`src/pane/jar/mod.rs:414-420`); and two constants named `UPGRADE_WAIT` (`mod.rs:276`, `attach.rs:40`) collapse to one entry in the inventory test's name set (`src/pane/jar/tests.rs:1906-1922`) |
| 883-884 | the validator "runs in a process group of its own, which is ended with it" | the group is killed only on timeout or a `try_wait` error (`crates/spyc-jar/src/upgrade.rs:699, 705`); a validator that exits leaves its children (the test at `upgrade.rs` tests line 200 relies on it) |
| 886-888 | "Only what the binary did rejects it (reason 6)" | any spawn error, `EAGAIN` under load included, maps to reason 6 (`upgrade.rs:652-660`), which stops upgrades for the session (`src/app/jar_upgrades.rs:206-218`): a clock-free verdict that load can still get wrong |
| 889-890 | the tests "in `tests/jar_process.rs`" | they are in `tests/jar_process/upgrade.rs` |
| 924-926 | the tests fail "with any part of this undone" | `tests/jar_process/contain.rs` never sends `describe` on a crossed connection; failures before `crossed` exists (`upgrade.rs:763-784`) are not contained at all |
| 957-958 | `ps` shows `spyc-jar`, the program's name and `--wire` | after an upgrade it shows `<name> resume-handover <old image>` (`upgrade.rs:274-282`); a session jar's `argv[0]` is `spyc-jar <SESSION>` (`src/pane/jar/session.rs:247`) |
| 964-965 | "Only a connection's writer thread writes to its socket" | a lookup writes from its reader (`outbox.rs:33-38`), `serve` writes refusals (`accept.rs:162-165`), and the upgrade flushes every outbox from its own thread while holding `state` and `conns` (`upgrade.rs:208-210, 362-375`); all bounded, so the invariant holds in substance and the sentence overstates it |
| 1048 | "Only pane tabs: overlays, tasks and captures run on a PTY of spyc's own" | captures and tasks run in jars (`src/app/capture_starts.rs:42-52`, `src/app/jar_info.rs:39-45`); the same section's capture bullet says so |
| 1061-1063 | `with_jars_on_no_path_launches…` holds `PtyHost::spawn` to one launch "a tab with no jar asked for" | the test reads `pty_host.rs`'s text only; it cannot see the app layer pass `jar: None` (G2) |
| 1071-1072, 1121 | "A restored tab whose jar may still run is never started that way"; "A tab's program never runs twice" | false on the paths in G2 |
| 1163 | the store's check "up to 10 s" | `BUILD_INFO_WAIT` is 60 s (`src/pane/jar/store.rs:70`), and `store::binary()` waits on a condvar with no limit (`store.rs:364-371`) |
| 1207-1218 | "A jarred program's signals go through its jar" | true of `PtyHost` only (G5) |
| 1227 | the snapshot "sits below the jar's offset by the gap's size" | by the gap's size less one per gap, since the `CAN` byte counts on the pane side |
| 1385 | releases go "only while the pane keys go to holds a program that pushed flag 2" | `key_releases_wanted` checks the active tab and both overlays whatever has focus (`src/app/pane_input.rs:98-108`) |

Also: `release_session` (`src/app/project_move.rs:748-771`), the releasing half of `:attach`, saves durably then releases every jar but carries no `move-saved-before-release` anchor; the anchor guard scans only `src/` and `crates/spyc-jar/src` (`src/app/mod_tests.rs:590-591`), so `scripts/unexport-libc.py`, `scripts/vendor-ghostty.sh` and `crates/spyc-vt-sys/build.rs` name `vendored-libc-symbols` in plain text the guard can't check. **Status:** confirmed for every row I re-read (352-355, 1048, 1071-1072, 1121, 1163, 1207-1218); the rest by sub-reviewers' reading, unverified by me.

### G15. informational. Comments and user text that say what the code doesn't do

- `docs/config/agent-hud.md` (agy paragraph), `docs/features/agents.md` and `src/ui/agent_hud/agy.rs:5-6`: the `argon` bucket "when active or at least half-spent" is shown as `7d` with no `5h` cell when Argon is the active model (`src/agent/agy_statusline.rs:203-206`); "`:agent hud dump` lists every bucket" lists at most 8; "active custom `agent`" shows any reported name.
- `src/agent/agy_statusline.rs:16`: "a reset further out is ignored" holds for `reset_in_seconds` only, not an absolute `reset_time`.
- `src/pane/engine_ghostty/jar_fidelity.rs:149-151`: the portable form loses kitty flags below the top "(spyc never encodes keys with them)": since #1176 it does, once the top is popped.
- `src/pane/tabs.rs:822-829` "Returns `true`…" on a `Vec<String>`; `src/pane/engine.rs:109` names `ProgramStatus::kind`, which doesn't exist; `src/pane/engine.rs:146` "Set only for `Blocked`" (`decode_status` sets it whenever a kind is reported).
- `src/lib.rs:406-407`: `run`'s doc ("Binary entry point…") now heads `take_terminal`; `run` has none. `src/mcp_cmd.rs:242-256`: `is_notice` "only records something" includes `Upgrade` and `Shutdown`. `src/keymap/action.rs:745-758`: `describe_git_diff`'s doc heads `is_git_diff`.
- `src/app/activity_diagnostics.rs`: the `:dump-scrollback` doc names `/tmp/spyc-scrollback.txt`, as `FEATURES.md` does; the code writes to the state directory (true since 2.4).

### Checked and true

Claims in the user pages that I checked against the code and found to hold: `spyc attach [NAME]`, its refusal inside a pane and `--force` (`src/pane/jar/attach.rs:56-62, 98-110`); `spyc shutdown [NAME] [-y]` and `spyc events [NAME] [-n N] [-f]` (`src/lib.rs:229-262`); `spyc --jars` (`lib.rs:201-204, 302-305`); `^d` and `^D` both detach (`src/keymap/resolver/mod.rs:233, 378`), `Space d` (`hints.rs:63`); `:detach`, `:attach`, `:upgrade`, `:events`, `:log` registered (`src/app/command_table.rs:104-150`); `:upgrade` with no newer install says so (`src/app/detach.rs:111-118`); `:name` is refused in spyc's own jar (`src/app/state/dispatch.rs:426-427`); "Every tab must run in a jar to detach; spyc says which one doesn't" (`detach.rs:88-92`); a capture's jar holds 4 MB and blocks when full with no owner (`crates/spyc-jar/src/cli.rs:183`, `state.rs:53-55`); `[pane] jars` is read only from the user's own files (`src/config/load.rs:335-339`, test `config/mod.rs:2155`); the event log's 1 MB cap and path (`src/state/event_log.rs`); the KEYBINDINGS rows and help overlay rows for `^d`, `:attach`, `:detach` and `:events` match the resolver and the command table.

## SPYC-TRAP

Search: `git -C <review> diff v2.4.0..HEAD | grep SPYC-TRAP`, slugs taken from `SPYC-TRAP(<slug>)` and `SPYC-TRAP: <slug>`, each then located with `rg -n` over the tree. 21 slugs were added since `v2.4.0`; none was removed (the 23 anchors of `v2.4.0` are all still present). Every one is anchored on both sides.

| slug | ARCHITECTURE.md | code site(s) | one-liner | rationale | guarding test |
|---|---|---|---|---|---|
| `probe-reader-ends-with-probe` | 357 | `src/terminal.rs:363` | true | two sentences stale or loose (G14) | `src/terminal_probe_tests.rs:253, 311` |
| `vendored-libc-symbols` | 694 | `src/pane/engine_ghostty/mod.rs:40` | true | true as far as read; upstream citations unsettled | `tests/libsystem_binding.rs:18` (macOS developer builds only), `tests/libc_binding.rs:18`, `scripts/test_vendored_archives.py`; scripts unanchorable (G14) |
| `jar-upgrade-holder` | 770 | `crates/spyc-jar/src/upgrade.rs:234` | true | true | `tests/jar_process.rs:948`: fails only on macOS, where CI runs no tests (V2) |
| `jar-lock-crosses-exec` | 812 | `upgrade.rs:267` | true | true; "close-on-exec until the upgrade's `exec`" is true of the first image only | `tests/jar_process/lock.rs:87` |
| `jar-gone-on-evidence` | 836 | `src/pane/jar/place.rs:223` | true | true | `src/pane/jar/tests.rs:519, 1578, 1619` |
| `jar-outwait` | 847 | `src/pane/jar/mod.rs:412`, `crates/spyc-jar/src/attach.rs:44` | true | incomplete (G14); `describe()` and `ATTACH_WAIT` unchecked against a parked jar | compile-time `const _` (`mod.rs:414-420`, `attach.rs:46-49`); `tests.rs:1737` |
| `jar-upgrade-one-file` | 871 | `upgrade.rs:165` | true | partly false (G14) | `tests/jar_process/upgrade.rs:138` |
| `jar-resume-contained` | 910 | `upgrade.rs:808` | true from line 803 | test claim overstated; early failures uncontained (G14) | `tests/jar_process/contain.rs:146-181` |
| `detach-after-last-words` | 931 | `crates/spyc-jar/src/jar/conn.rs:28` | true | true | `tests/jar_process.rs:1535` |
| `jar-token-stdin` | 946 | `src/pane/jar/mod.rs:494`, `src/pane/jar/session.rs:264`, `crates/spyc-jar/src/cli.rs:373` | true | `ps` sentence stale (G14) | `src/pane/jar/tests.rs:669`; nothing checks the environment |
| `jar-one-writer` | 962 | `crates/spyc-jar/src/jar/outbox.rs:31` | true in substance, worded broadly | overstated (G14) | `tests/jar_process.rs:1854, 1879, 2041` |
| `jar-program-hears-each-owner-once` | 977 | `crates/spyc-jar/src/jar/accept.rs:136` | true | true | `tests/jar_process.rs:1501` (the double `attached` only; order untested) |
| `jar-frame-whole` | 989 | `crates/spyc-jar/src/client.rs:107` | true | true | `client.rs:448` |
| `mcp-follows-jar` | 1002 | `src/mcp/follow.rs:85` | true | true | `tests/jar_adopt.rs:548` |
| `own-jar-by-place` | 1020 | `src/pane/jar/session.rs:148` | true | true | `tests/session_jar.rs:787` |
| `move-saved-before-release` | 1032 | `src/app/project_move.rs:84, 322, 661, 710` | true at all four | true; `release_session` unanchored (G14) | `src/app/harness_tests/project_move.rs:98` (`:detach project` only) |
| `jar-or-nothing` | 1044 | `src/pane/pty_host.rs:241`, `src/pane/jar/mod.rs:425` | `mod.rs` true; `pty_host.rs:241` false as worded (overlays always, captures with jars off, run on spyc's PTY) | false in parts (G2, G14) | `src/pane/jar/tests.rs:439` (text only), `:385`, `:425` |
| `jar-off-the-loop` | 1161 | `src/app/capture_starts.rs:162, 174`, `src/app/pane_starts.rs:69` | true | one stale number (G14); `kill_capture` waits on the loop (G8g) | `src/app/pane_starts.rs:383` |
| `jar-signals-through-the-jar` | 1209 | `src/pane/pty_host.rs:593` | true of `PtyHost` | false app-wide (G5) | `src/pane/jar/tests.rs:186, 212` |
| `jar-snapshot-offset` | 1222 | `src/pane/jar/snapshots.rs:43` | true | off by one per gap (G14) | `src/pane/jar/tests.rs:273` |
| `kitty-keys-follow-the-owner` | 1362 | `src/pane/kitty.rs:55` | true | one sentence narrow (G14); broken across adoption (G8h) | `src/pane/tests.rs:537` (`kitty_route_tests`), `src/app/harness_tests/kitty_keys.rs` |

The trap rationales' table cells marked "true" for crate-side slugs (`jar-upgrade-holder` to `jar-frame-whole`) rest on the sub-reviewers' reading of `crates/spyc-jar`, which is Units A and B's ground; Unit A and B's reports are the authority for those.

## Seeded leads

- **V4 (the documents): confirmed.** The three sentences exist as the brief says (G9). No fourth was found. The code side: no path falls back unasked; the one breach of jar-or-nothing goes the other way, a second program started beside a live jar when jars are off for this spyc (G2).
- **H7 (whether the documents say it): confirmed.** Nothing in the tree mentions `KillUserProcesses`, lingering, or WSL's shutdown, while the pages promise the session outlives the terminal (G10). Whether a jar survives on such hosts is unprobed; it is on the manual checklist.
- **V3's ROADMAP date: confirmed, and wider.** `ROADMAP.md:281`, `:778-779` and `:811` date the freeze 2026-11-12; it froze again on 2026-11-24. The roadmap also names `spyc -a` and omits the plan and the upgrade brief from its doc map (G11).
- **V1, the documents' side (lead owned by F):** confirmed that every user page and the config template describe the gate as it is coded, so the documents will need to change with S6 (G1).

## Manual checklist items

1. **Detach survives logout on a logind host** (settles G10's consequence). On a Linux machine with systemd: `loginctl show-user $USER -p Linger` and `grep KillUserProcesses /etc/systemd/logind.conf`; set `KillUserProcesses=yes` (or use a distro that does), open a tab running `sleep 1000`, `^d`, log out of every session, log back in over SSH, `spyc attach`. Expected if G10 is right: no session; `spyc --jars` lists nothing. Then `loginctl enable-linger` and repeat: expected, the session attaches.
2. **WSL** (G10). In WSL2, `^d` a session with a tab, close every terminal on the distro, wait past WSL's idle timeout (or `wsl --shutdown` to see the worst case), open a terminal, `spyc attach`. Expected: no session.
3. **A program twice after a restore** (G2), on scratch XDG directories only. In a `-CURRENT` build, open a tab running `sh -c 'echo $$ >> /tmp/g2-pids; sleep 1000'`, `kill -9` that spyc, set `[pane] jars = false` in the scratch config, `spyc -r`. Expected if G2 holds: two pids in the file, both alive.
4. **`^d` on a release-versioned build** (G1). Build at the pin with the version set to `3.0.0` in a scratch worktree, open one tab, press `Space d`. Expected: "can't detach: the tab … runs outside a jar".
5. **Status report hidden by a frame** (G4). In a shell tab: `printf '\e]7501;blocked\a'; sleep 600` while another tab's agent is working (so frames are dirty). Expected if G4 bites: the tab's dot doesn't turn blocked until the program prints again; repeat a few times, since it is a race.

## Not covered

- Nothing was run: no probe of G2, G4 or G6, and no Linux check of G10. The findings marked "unverified" in G8 were read by a sub-reviewer and not re-read by me.
- `spikes/`, `scripts/` (other than the two the libc trap names), `crates/spyc-vt-sys/` beyond the trap, `docs/COMPETITIVE_REVIEW.md`, `docs/BRAND.md`, `docs/archive/` moves, `docs/drafts/PANE_JAR_SPIKE.md`, `docs/drafts/GIT_REVIEW_WORKSPACE_PLAN.md`: not read.
- The brand sweep was skimmed as diffs only; I found no behaviour change in it and did not check every renamed string.
- `ARCHITECTURE.md` outside the trap sections; `docs/HARNESS.md` and `docs/AGENT_ORCHESTRATION.md` diffs; `docs/features/archives.md`, `docs/config/{notifications,projects}.md`: not read.
- The Mac-only and Linux-only halves of the crash report (what a jarred spyc's panic shows on the next attach) were left to Units B and D.
