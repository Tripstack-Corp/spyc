# Remediation ledger

One row per finding the remediation brief's section 3 closes, plus what its section 4 adds. A row is closed when its red test was seen failing and then passing on both platforms, a refuter who didn't write the fix failed to break it, and its PR is merged. Each PR that changes a row updates it here.

Findings, sites and evidence are in [`SYNTHESIS.md`](SYNTHESIS.md), [`ADDENDUM.md`](ADDENDUM.md) and [`H-cross-version.md`](H-cross-version.md). What section 5 files instead of fixing is listed at the end.

| Finding | Slice | Red test | PR | Refuter's verdict | Status |
|---|---|---|---|---|---|
| B3 (jar tests red under load) | A | n/a (test-only slice; 25+ mutations caught, logs in the PR) | #1329 | confirmed after two rounds | merged; the loaded runs (20 green in a row on each platform) are not yet recorded: see F3 |
| F5 (fixed waits that assume a state) | A | n/a (test-only slice; 25+ mutations caught, logs in the PR) | #1329 | confirmed after two rounds | merged; the loaded runs (20 green in a row on each platform) are not yet recorded: see F3 |
| B13's remainder (the counter's echo) | A | n/a (test-only slice; 25+ mutations caught, logs in the PR) | #1329 | confirmed after two rounds | merged; the loaded runs (20 green in a row on each platform) are not yet recorded: see F3 |
| B10 (tests leak processes) | A | n/a (test-only slice; 25+ mutations caught, logs in the PR) | #1329 | confirmed after two rounds | merged; the loaded runs (20 green in a row on each platform) are not yet recorded: see F3 |
| F3 (macOS in CI), after the loaded runs are green | A | | | | open: blocked by #1368 (a timers test fails on a loaded Mac) |
| C1 = G2 = H2 (restore with jars off runs a program twice) | B | `review_c_a_restored_tab_whose_jar_runs_is_not_started_again_with_jars_off`, `a_restore_with_jars_off_leaves_the_tab_detached_saying_why`, `a_restored_tab_whose_id_a_live_tab_holds_is_not_started_again`, `a_restored_tab_whose_jar_lost_its_socket_file_is_not_started_again` | #1344, #1345 | cleared after round 2 | closed |
| C2, both halves, with P1 (`^a R` and the retake) | B | `a_tab_whose_jar_is_silent_past_one_welcome_is_taken_back_once_it_answers`, `restarting_a_tab_whose_jar_runs_ends_its_program_there_first`, `restarting_a_tab_another_spyc_owns_is_refused_and_says_who`, `a_retake_that_meets_this_spycs_old_connection_waits_for_it_to_go`, `restarting_with_jars_off_ends_the_program_in_its_jar_first`, `restarting_a_tab_whose_other_spyc_let_go_asks`, `a_live_tabs_restart_whose_token_went_after_the_question_ends_then_starts`, `a_short_stop_and_continue_leaves_every_jarred_tab_its_link` (Linux) | #1344, #1345, #1396, #1397 | cleared after round 2; #1397 refuted on its own | closed; #1398 and #1399 filed from #1397 |
| B12 (`EINTR` ends a link on Linux) | B | `tests/jar_interrupted.rs` (Linux), `a_read_whose_poll_is_interrupted_reads_what_comes` (`spyc-jar`) | #1344, #1345 | cleared after round 2 | closed |
| C12 (`[detached]` says why) | B | `a_tab_detached_for_another_spyc_says_so_on_its_screen`, `a_tab_whose_jar_has_no_token_here_says_so_and_restart_refuses`, `a_tab_whose_jar_cant_be_told_about_says_so_and_restart_refuses`, `a_detached_tabs_restart_whose_token_went_after_the_question_starts_nothing_and_says_why`, `a_connect_to_a_jar_that_has_gone_gives_up_at_once` (`spyc-jar`) | #1344, #1345 | cleared after round 2 | closed |
| An ending jar leaves its socket behind, so its tab's restart is refused for good (found through a flake in `a_tab_whose_jar_still_runs_never_starts_its_program_twice`) | B | `an_ending_jar_leaves_no_socket_for_its_tab_to_be_refused_on` (`test/jar-end-race-red`) | #1346, #1347 | cleared | closed; added to slice B at the owner's word, 2026-11-25 |
| B1 (unchecked `thread::spawn`, upgrade path first) | C | `tests/jar_process/threads.rs`: three Linux tests (`RLIMIT_NPROC` at 0), red at `origin/main` 1dfaac86 | #1370, #1371 | cleared | closed |
| A1 (`SIGPIPE` after a failed `exec`) | C | `a_holder_gone_before_its_handshake_does_not_end_a_jar_whose_exec_failed` (Linux, ptrace orders the holder's exit first), red at `origin/main` 1dfaac86 | #1374, #1375 | cleared | closed |
| A2 (reason 6 where 5 is promised) | C | `tests/jar_process/answers.rs`: a late proof (both platforms), two Linux tests short of descriptors; red at `origin/main` 1dfaac86 | #1372, #1373 | cleared | closed |
| A3 (the copy step opens any path) | C | `an_upgrade_naming_a_fifo_is_refused_and_the_next_one_runs`, `a_refused_upgrade_leaves_no_copy_behind`; red at `origin/main` 1dfaac86 | #1372, #1373 | cleared | closed |
| A9 (a validator's process group) | C | `a_validators_children_end_with_the_validation`; red at `origin/main` 1dfaac86 | #1372, #1373 | cleared | closed |
| G9 (three jar-or-nothing sentences) | C | none (texts) | #1377 | cleared | closed |
| A13 (the specification's prose) | C | none (texts) | #1377 | cleared | closed |
| D7 (token rotation no code performs) | C | none (texts) | #1377 | cleared | closed |
| E6's sentence under "Who may connect" | C | none (texts) | #1377 | cleared | closed |
| F6 (no test of the peer-user check) | C | `a_connection_from_another_user_is_closed_unheard` (unit, `accept.rs`), seen failing with M3 | #1376 | cleared | closed |
| F7 (no test of `SO_NOSIGPIPE`) | C | `a_validator_gone_mid_blob_does_not_end_a_jar_whose_exec_failed`, seen failing on macOS with M12 | #1374, #1375 | cleared | closed |
| S6: `honoured()` removed, with F1 and G1 | D | `a_release_build_reads_the_setting` (retired with the gate it tested) and the guard `no_build_tells_a_release_from_its_rc`, `src/version_guard.rs` | #1355, #1356 | cleared | closed |
| F2 (`spyc-jar` in lockstep with spyc) | D | `scripts/test_jar_version_lockstep.py` | #1357, #1358 | cleared | closed |
| A8 = F4 (`jar_compat` and the first fixture) | D | n/a (a test rewrite: the old test hangs on this crate's jar; mutations caught) | #1361 | cleared | closed |
| F8 (a `.deb` without `spyc-jar`) | D | `test_a_deb_of_a_release_that_ships_spyc_jar_needs_it`, `scripts/test_release_ships_spyc_jar.py` | #1359, #1360 | cleared | closed |
| F9 (the release document) | D | n/a (documentation) | #1362 | cleared | closed |
| D1 (connect at the given path; refuse a `:name` that can't fit) | E | `ctrl_d_hands_over_when_the_runtime_directory_is_a_link_to_a_long_path` (`tests/jar_adopt.rs`), `a_name_whose_session_socket_would_not_fit_is_refused` (`src/app/detach.rs`), `a_name_too_long_for_a_socket_is_refused_in_the_sessions_own_jar` (`tests/session_jar.rs`, after the refuter's MD1b) | #1379, #1380 | cleared after the recheck | closed |
| D2, D6, and P3-9's low (start and save before release) | E | `ctrl_d_with_no_place_for_the_sessions_jar_keeps_spyc_and_its_tabs`, `ctrl_d_whose_save_fails_keeps_spyc_and_its_tabs`, `upgrade_whose_save_fails_keeps_the_session_in_its_jar`; after the refuter's findings `ctrl_d_with_a_slow_session_jar_keeps_spyc_drawing_and_answering`; after #1337 and the owner's option (a), `ctrl_d_leaves_no_tab_detached_and_runs_no_program_twice` (`tests/jar_adopt.rs`); after the recheck, `an_autosave_due_while_the_session_jar_starts_leaves_the_session_to_it` (`tests/jar_adopt.rs`) `nothing_saves_or_ends_the_session_while_its_jar_starts`, `no_quit_ends_spyc_while_its_session_jar_starts` and `mcp_calls_that_change_the_session_are_refused_while_its_jar_starts` (`src/app/detach.rs`), `a_handover_whose_jar_fails_to_start_leaves_each_tab_linked` (`src/app/harness_tests/handover.rs`) | #1381, #1382, #1387, #1388, #1393, #1394 | cleared after three rounds (option (a) for `^d`, after #1337) | closed; the lows left are #1395 |
| The orphan prompt (confirm a discard; no type-ahead) | E | `a_key_before_the_offer_is_drawn_doesnt_answer_it` (`src/app/harness_tests/jar_orphans.rs`), `discarding_a_jar_no_spyc_owns_asks_first` (`tests/jar_adopt.rs`); after the refuter's findings `typing_a_word_at_the_offer_ends_nothing` and `a_pop_up_counts_keys_from_after_its_frame_is_written` (the same harness file) | #1383, #1384, #1389, #1390 | cleared after the recheck | closed |
| D5 (`save_released` keeps the last report) | E | `ctrl_d_keeps_a_tabs_last_status_report` (`tests/jar_adopt.rs`) | #1381, #1382 | cleared | closed |
| D3 (`take_over` re-reads before it signals) | E | `a_takeover_after_the_question_ends_the_client_attached_then` (`tests/session_jar.rs`); after the refuter's low `a_takeover_of_a_session_that_ended_meanwhile_says_it_ended` (`src/pane/jar/attach.rs`) | #1385, #1386, #1391, #1392 | cleared after the recheck | closed |
| E1 (the control methods need the session's token) | F | `tests/session_jar.rs` (`a_shutdown_without_the_sessions_token_is_refused`, `a_release_without_…`, `an_upgrade_without_…`, `a_spyc_outside_a_session_jar_takes_no_control_request`, `a_token_written_into_the_token_file_is_refused`, `an_empty_or_partial_token_is_refused`), `src/mcp/tests/control.rs` (the loop never hears a refused request), on `test/control-token-red` | #1406, #1407, #1410, #1411, #1412, #1413 | cleared after two rounds | closed |
| H7 (a refused `spyc/upgrade` is silent; the owner's word, with E1) | F | `src/mcp/tests/control.rs` (`an_upgrade_request_…` ×3), `harness_tests::upgrade_answers`, `pane::jar::attach` tests (`a_refused_upgrade_is_heard_from_the_session_spyc`, `an_upgrade_a_spyc_doesnt_know_is_reported_as_such`), on `test/upgrade-answers-red` | #1408, #1409 | cleared | closed |
| E2, high since P3-9 (one ownership check on every path) | F | `tests/session_jar.rs` (`spyc_attach_refuses_a_sessions_directory_open_to_others`, `spyc_shutdown_refuses_…`, `spyc_jars_refuses_…`), `pane::jar::place::tests` (orphan finder, MCP proxy, a link as a session, the guard `every_jar_socket_is_reached_through_the_ownership_check`), `tests/jar_process/socket_dir.rs` (`keep_socket`), on `test/socket-trust-red`; the SSH-agent links' directory, `pane::ssh_agent::tests::a_links_directory_under_one_open_to_others_is_refused` on `test/ssh-agents-trust-red` | #1404, #1405, #1414, #1415 | cleared after the final round (a missing socket file waits, it isn't refused) | closed |
| E3 = G5 (tasks and captures signal through the jar) | F | `app::harness_tests::jar_signals` (seven cases and the guard `no_number_is_signalled_outside_the_listed_sites`), on `test/task-signals-red` | #1400, #1401, #1416, #1417 | cleared | closed |
| E4 (`SECURITY.md` describes 3.0) | F | n/a (docs; the link guard and the prose guards) | #1420 | cleared | closed |
| E7 (a session's token is forgotten with its jar) | F | `tests/session_jar.rs` (`a_session_shut_down_forgets_its_token`, `a_session_quit_from_its_terminal_forgets_its_token`, `a_session_whose_spyc_died_forgets_its_token_through_its_terminal`, `a_session_quit_by_its_own_spyc_forgets_its_token`), `pane::jar::place::tests::a_gone_sessions_socket_and_token_are_swept`, on `test/session-token-forgotten-red` | #1402, #1403, #1418, #1419 | cleared | closed |
| H1 (2.4.0 restores a 3.0 session and runs every program twice) | B | `a_saved_session_is_one_2_4_0_skips` (`test/manifest-v3-red`), `every_manifest_written_is_this_version` | #1348, #1349 | cleared after round 2 | closed; added to slice B at the owner's word, 2026-11-24 |
| A jar's start on a pipe another child holds open (slice A's finding, macOS) | B | `a_jars_start_reaches_it_whatever_else_holds_this_spycs_pipes`, `a_sessions_jar_start_reaches_it_whatever_else_holds_this_spycs_pipes` (`test/jar-start-pipe-red`) | #1350, #1351 | cleared after round 2 | closed (the clipboard sites are #1323, the PTY's `openpty` #1334); added to slice B at the owner's word, 2026-11-24 |
| Handover fault hooks (Unit D's crash ordering, by test) | §4 | `tests/handover_faults.rs` (32 cases over 21 points, a background task's jar among them, and 5 handovers run to their end as controls; each case checks every jar the crashed spyc and its peers ran, those that move and those that stay: no jar lost, no program started again anywhere, and every jar back by `spyc -r` or the session jar's spyc), guards `handover_fault::tests::every_handover_write_and_release_is_followed_by_a_fault_point` and `…::every_fault_point_is_placed_and_has_a_case`, on `test/handover-faults` | #1451 | cleared after round 2 (round 1: staying jars unchecked, M4 misread, no task case; all answered) | no point found a lost jar, a second start or a session that doesn't come back; #1423, #1424 and #1452 filed from it |

## Filed, not fixed (section 5)

Issues on the `3.0.x` milestone. There is one per remaining medium, one per unit for its lows and informationals, and B4 with its design. Every finding id of the review is in one of the rows above or one of these issues.

### The review's remaining findings

- #1425: B4: An idle jar wakes 50 to 70 times a second for its whole life
- #1426: A5: A new jar image that can't receive its descriptors exits and takes the program with it
- #1427: B2: A jar that fails leaves no record of why
- #1428: C3: spyc never notices a jar that holds its lock and answers nothing
- #1429: C4: Input a jar refuses is dropped without a word, and a jarred tab has no input bound
- #1430: C5: A confirmed file pipe over 16 MiB ends a jarred tab's input for good
- #1431: C6: A task or capture whose jar ends the lease shows as ended while its program runs on
- #1432: C7: A clock decides whether a terminal reply reaches the program after an adoption
- #1433: D4: ^\ in spyc attach cuts a paste in half, and never reaches spyc without the kitty protocol
- #1434: G3: A plain :setenv value is written to disk in the session's event log
- #1435: G4: A program's status report can sit unread when the frame's drain sees its output first
- #1436: G6: A restored cursor can be moved to another file by a later rebuild
- #1437: G10, D9: No document says that a login manager or WSL can end a detached session
- #1438: H3: One unknown enum value loses a whole saved session, and an unknown field is dropped on the next save
- #1439: H4: :upgrade, spyc attach's offer and jar upgrades take any other build for a newer one
- #1440: H5: The MCP proxy in a jarred agent outlives every upgrade and keeps its own build's reply budgets
- #1441: H8: A running session's restore point is deleted by any other spyc's save
- #1442: A6, A10, A7, A12: Unit A's remaining lows and informationals (the jar's upgrade and lock)
- #1443: B5, B6, B7, B9, B11, B8: Unit B's remaining lows and informationals (the jar as a long-lived process)
- #1444: C8, C9, C10, C11, C13: Unit C's remaining lows and informationals (spyc's side of a pane's jar)
- #1445: D8, D10: Unit D's remaining lows (sessions: detach, attach, handover)
- #1446: E8, E9, E10: Unit E's remaining lows and informationals (security)
- #1447: F10, F11: Unit F's remaining informationals (tests, CI and the release)
- #1448: G7, G8, G11, G12, G13, G14, G15: Unit G's remaining lows and informationals (the rest of the diff, and the documents)
- #1449: H6, H9, H10, H11: Unit H's remaining lows and informationals (cross-version), and the manifest's pinning tests
- #1450: A13's remainder: the jar's property sequences never upgrade to a real image

### Found during the remediation

- #1323: Clipboard copy and paste can hang or fail silently when another child inherits their pipe (macOS)
- #1325: Concurrent PTY starts with SHELL unset can segfault on musl (getpwuid is not thread-safe there)
- #1332: session_jar: the attach-and-draw test can record its replay offset before spyc sees the first terminal leave
- #1333: agent HUD double-click test decides from a 400 ms wall-clock window and fails under load
- #1334: macOS: a child started while spyc opens a PTY or pipe can inherit it and hold it open (openpty, upgrade pipe)
- #1338: jar_process: tokenless_connections_cannot_exhaust_the_jars_descriptors failed once under load
- #1342: disk_tree test matches b/ and c/ against text that includes the random temp path
- #1343: jar: a signal for the program sent before it has its own process group is lost
- #1352: Manifest::running() reads an unreadable start time (0) as a real one
- #1363: Cargo.lock merge=ours drops one side's lock changes on a rebase without a conflict
- #1364: jars_no_spyc_owns_are_found_by_what_they_hold failed once under load
- #1365: agent_hud_mouse_rows_never_reach_child_and_geometry_changes_wait_for_owned_drags failed once under heavy load on Linux
- #1366: a_tab_whose_jar_dies_is_marked_jar_lost_with_no_key_typed failed once with "Connection refused"
- #1367: a_failed_exec_leaves_a_jar_that_survives_the_next_refusal saw its connection end at high load on macOS
- #1368: timers test an_owner_that_cant_take_its_last_frames_keeps_its_connection fails on a loaded Mac, so F3 (macOS make test in CI) is not reached
- #1369: make ignored-tests misses Python tests landed red with @unittest.skip
- #1378: the exit guard misses an aliased end::end call, and the pipe guard a new jar::pair() call
- #1395: low gaps left in the ^d handover while the session jar starts (Lua actions, report_status, TOOK_OVER_FROM, log rename)
- #1398: release drops the owner's connection before clearing the owner, so a query can see an owner with no pid
- #1399: the retake asks a jar again in a tight loop while its owner has no pid it can name
- #1421: The orphan-jars pop-up reads ^d as d and ^r as r
- #1422: an_owner_paused_mid_frame_makes_an_upgrade_busy races the jar under load
- #1423: The handover fault guard knows writes and releases only by name
- #1424: ^d leaves a jarred capture's fate undefined
- #1452: A handover fault case can miss a restore that starts its startup tabs again
