# Remediation ledger

One row per finding the remediation brief's section 3 closes, plus what its section 4 adds. A row is closed when its red test was seen failing and then passing on both platforms, a refuter who didn't write the fix failed to break it, and its PR is merged. Each PR that changes a row updates it here.

Findings, sites and evidence are in [`SYNTHESIS.md`](SYNTHESIS.md), [`ADDENDUM.md`](ADDENDUM.md) and [`H-cross-version.md`](H-cross-version.md). What section 5 files instead of fixing is listed at the end.

| Finding | Slice | Red test | PR | Refuter's verdict | Status |
|---|---|---|---|---|---|
| B3 (jar tests red under load) | A | n/a (test-only slice; 25+ mutations caught, logs in the PR) | #1329 | confirmed after two rounds | in PR, Linux 20-in-a-row waits on slice B's B12 fix |
| F5 (fixed waits that assume a state) | A | n/a (test-only slice; 25+ mutations caught, logs in the PR) | #1329 | confirmed after two rounds | in PR, Linux 20-in-a-row waits on slice B's B12 fix |
| B13's remainder (the counter's echo) | A | n/a (test-only slice; 25+ mutations caught, logs in the PR) | #1329 | confirmed after two rounds | in PR, Linux 20-in-a-row waits on slice B's B12 fix |
| B10 (tests leak processes) | A | n/a (test-only slice; 25+ mutations caught, logs in the PR) | #1329 | confirmed after two rounds | in PR, Linux 20-in-a-row waits on slice B's B12 fix |
| F3 (macOS in CI), after the loaded runs are green | A | | | | waits on the four above |
| C1 = G2 = H2 (restore with jars off runs a program twice) | B | `review_c_a_restored_tab_whose_jar_runs_is_not_started_again_with_jars_off`, `a_restore_with_jars_off_leaves_the_tab_detached_saying_why`, `a_restored_tab_whose_id_a_live_tab_holds_is_not_started_again`, `a_restored_tab_whose_jar_lost_its_socket_file_is_not_started_again` | | | fixed on `fix/never-run-twice`; refuter round 2 answered, awaiting its re-check |
| C2, both halves, with P1 (`^a R` and the retake) | B | `a_tab_whose_jar_is_silent_past_one_welcome_is_taken_back_once_it_answers`, `restarting_a_tab_whose_jar_runs_ends_its_program_there_first`, `restarting_a_tab_another_spyc_owns_is_refused_and_says_who`, `a_retake_that_meets_this_spycs_old_connection_waits_for_it_to_go`, `restarting_with_jars_off_ends_the_program_in_its_jar_first`, `restarting_a_tab_whose_other_spyc_let_go_asks`, `a_live_tabs_restart_whose_token_went_after_the_question_ends_then_starts`, `a_short_stop_and_continue_leaves_every_jarred_tab_its_link` (Linux) | | | fixed on `fix/never-run-twice`; refuter round 2 answered, awaiting its re-check |
| B12 (`EINTR` ends a link on Linux) | B | `tests/jar_interrupted.rs` (Linux), `a_read_whose_poll_is_interrupted_reads_what_comes` (`spyc-jar`) | | | fixed on `fix/never-run-twice`; refuter round 2 answered, awaiting its re-check |
| C12 (`[detached]` says why) | B | `a_tab_detached_for_another_spyc_says_so_on_its_screen`, `a_tab_whose_jar_has_no_token_here_says_so_and_restart_refuses`, `a_tab_whose_jar_cant_be_told_about_says_so_and_restart_refuses`, `a_detached_tabs_restart_whose_token_went_after_the_question_starts_nothing_and_says_why`, `a_connect_to_a_jar_that_has_gone_gives_up_at_once` (`spyc-jar`) | | | fixed on `fix/never-run-twice`; refuter round 2 answered, awaiting its re-check |
| An ending jar leaves its socket behind, so its tab's restart is refused for good (found through a flake in `a_tab_whose_jar_still_runs_never_starts_its_program_twice`) | B | `an_ending_jar_leaves_no_socket_for_its_tab_to_be_refused_on` (`test/jar-end-race-red`) | | | fixed on `fix/jar-end-race` (every end and every socket check take the jar's address), awaiting its refuter; added to slice B at the owner's word, 2026-11-25 |
| B1 (unchecked `thread::spawn`, upgrade path first) | C | `tests/jar_process/threads.rs`: three Linux tests (`RLIMIT_NPROC` at 0), red at `origin/main` 1dfaac86 | `test/jar-thread-refusal-red`, `fix/jar-thread-refusal` | pending | fixed on the branch, awaiting the refuter |
| A1 (`SIGPIPE` after a failed `exec`) | C | `a_holder_gone_before_its_handshake_does_not_end_a_jar_whose_exec_failed` (Linux, ptrace orders the holder's exit first), red at `origin/main` 1dfaac86 | `test/jar-sigpipe-red`, `fix/jar-sigpipe` | pending | fixed on the branch, awaiting the refuter |
| A2 (reason 6 where 5 is promised) | C | `tests/jar_process/answers.rs`: a late proof (both platforms), two Linux tests short of descriptors; red at `origin/main` 1dfaac86 | `test/jar-upgrade-answers-red`, `fix/jar-upgrade-answers` | pending | fixed on the branch, awaiting the refuter |
| A3 (the copy step opens any path) | C | `an_upgrade_naming_a_fifo_is_refused_and_the_next_one_runs`, `a_refused_upgrade_leaves_no_copy_behind`; red at `origin/main` 1dfaac86 | `test/jar-upgrade-answers-red`, `fix/jar-upgrade-answers` | pending | fixed on the branch, awaiting the refuter |
| A9 (a validator's process group) | C | `a_validators_children_end_with_the_validation`; red at `origin/main` 1dfaac86 | `test/jar-upgrade-answers-red`, `fix/jar-upgrade-answers` | pending | fixed on the branch, awaiting the refuter |
| G9 (three jar-or-nothing sentences) | C | none (texts) | `docs/jar-protocol-texts` | pending | fixed on the branch, awaiting the refuter |
| A13 (the specification's prose) | C | none (texts) | `docs/jar-protocol-texts` | pending | fixed on the branch, awaiting the refuter |
| D7 (token rotation no code performs) | C | none (texts) | `docs/jar-protocol-texts` | pending | fixed on the branch, awaiting the refuter |
| E6's sentence under "Who may connect" | C | none (texts) | `docs/jar-protocol-texts` | pending | fixed on the branch, awaiting the refuter |
| F6 (no test of the peer-user check) | C | `a_connection_from_another_user_is_closed_unheard` (unit, `accept.rs`), seen failing with M3 | `test/jar-peer-user` | pending | fixed on the branch, awaiting the refuter |
| F7 (no test of `SO_NOSIGPIPE`) | C | `a_validator_gone_mid_blob_does_not_end_a_jar_whose_exec_failed`, seen failing on macOS with M12 | `fix/jar-sigpipe` | pending | fixed on the branch, awaiting the refuter |
| S6: `honoured()` removed, with F1 and G1 | D | | | | open |
| F2 (`spyc-jar` in lockstep with spyc) | D | | | | open |
| A8 = F4 (`jar_compat` and the first fixture) | D | | | | open |
| F8 (a `.deb` without `spyc-jar`) | D | | | | open |
| F9 (the release document) | D | | | | open |
| D1 (connect at the given path; refuse a `:name` that can't fit) | E | `ctrl_d_hands_over_when_the_runtime_directory_is_a_link_to_a_long_path` (`tests/jar_adopt.rs`), `a_name_whose_session_socket_would_not_fit_is_refused` (`src/app/detach.rs`) | | refuter pending | fixed on the branch `fix/session-own-socket` |
| D2, D6, and P3-9's low (start and save before release) | E | `ctrl_d_with_no_place_for_the_sessions_jar_keeps_spyc_and_its_tabs`, `ctrl_d_whose_save_fails_keeps_spyc_and_its_tabs`, `upgrade_whose_save_fails_keeps_the_session_in_its_jar` (`tests/jar_adopt.rs`) | | refuter pending | fixed on the branch `fix/handover-before-release` |
| The orphan prompt (confirm a discard; no type-ahead) | E | `a_key_before_the_offer_is_drawn_doesnt_answer_it` (`src/app/harness_tests/jar_orphans.rs`), `discarding_a_jar_no_spyc_owns_asks_first` (`tests/jar_adopt.rs`) | | refuter pending | fixed on the branch `fix/orphan-discard-confirm` |
| D5 (`save_released` keeps the last report) | E | `ctrl_d_keeps_a_tabs_last_status_report` (`tests/jar_adopt.rs`) | | refuter pending | fixed on the branch `fix/handover-before-release`, with D2 and D6 (the same function) |
| D3 (`take_over` re-reads before it signals) | E | | | | open |
| E1 (the control methods need the session's token) | F | | | | open |
| E2, high since P3-9 (one ownership check on every path) | F | | | | open |
| E3 = G5 (tasks and captures signal through the jar) | F | | | | open |
| E4 (`SECURITY.md` describes 3.0) | F | | | | open |
| E7 (a session's token is forgotten with its jar) | F | | | | open |
| H1 (2.4.0 restores a 3.0 session and runs every program twice) | B | `a_saved_session_is_one_2_4_0_skips` (`test/manifest-v3-red`), `every_manifest_written_is_this_version` | | | fixed on `fix/manifest-v3` (manifest version 3; 2 and 3 read); refuter round 2 answered, awaiting its re-check; added to slice B at the owner's word, 2026-11-24 |
| A jar's start on a pipe another child holds open (slice A's finding, macOS) | B | `a_jars_start_reaches_it_whatever_else_holds_this_spycs_pipes`, `a_sessions_jar_start_reaches_it_whatever_else_holds_this_spycs_pipes` (`test/jar-start-pipe-red`) | | | fixed on `fix/jar-start-pipe` (the start, and every child end a reader waits on, as a file; guard `no_reader_waits_for_the_end_of_a_pipe_another_spawn_can_hold`, which parses the code for every way to make a pipe; the clipboard sites filed as #1323; the pane PTY's `openpty`, the same race, written up, not fixed); refuter round 2 answered, awaiting its re-check; added to slice B at the owner's word, 2026-11-24 |
| Handover fault hooks (Unit D's crash ordering, by test) | §4 | | | | open |

## Filed, not fixed (section 5)

Issues on the `3.0.x` milestone, one per remaining medium, one per unit for its lows and informationals, and B4 with its design. Listed here as they are filed.