# Reviewer F — test quality and guard integrity

**Subject:** `git diff v2.0.0..HEAD` (HEAD = `f8dedae`, version `2.1.0-CURRENT`), horizontal charter.
**Method:** diff-driven test-mass survey → full guard inventory with measured scan reach → fuzz build/run verification → revert-and-check-the-test-BITES on the high-risk clusters.
**Repo state:** read-only. Nothing in `/Users/derekmarshall/src/spyc` was modified; the only writes were gitignored build artifacts under `fuzz/target/` and a mutated *copy* of `src/` in the scratchpad used for one bite check.

---

## Summary verdict

The guard suite is in good structural health and the C2 splitter migration is real: all eleven source-scan guards pass on HEAD, every one reaches substantial production bytes (63–100% of its declared scope), and none scans zero bytes — the failure mode AGENTS.md warns about is not present anywhere in the tree. Much of the test mass added since 2.0 is genuinely honest: the pager-selection, pane-restart, journal/`plan_repack`, name-normalization, MCP-archive and OSC-52 clusters assert resolved state, carry paired negative cases, and bite under mutation. Two guard fail-opens exist, both proven empirically rather than argued — the `EnableMouseCapture` guard exempts `src/lib.rs` wholesale, the one file holding `setup_terminal` / `restore_terminal` / `resume_tui`, and the flashed-error guard sees only single-line call sites, with a live escaping site on HEAD. The concentrated *test-theatre* risk has one consistent shape across both hot areas: each safety rail's pure decision function is thoroughly tested while its wiring and failure path are not, so the rail reads as covered when only its decision is — `route.rs` has 68 tests while `handle_mouse` / `forward.rs` / `scroll.rs` have zero between them, and `scan::assess` / `journal` are well covered while the archive repack's `verify` step, its verify-before-rename ordering, and `assess`'s wiring can each be mutated to a no-op with all 2303 tests green. Finally, `archive_name`, the newest and best-justified fuzz target, builds and runs clean (300 k executions, no crash) but is absent from the weekly workflow matrix, so it has never executed in CI.

**Tag posture: no blockers.** The archive audit proposed blocker on three write-path items; I am downgrading all three to high (F1b–F1d) — nothing found says the shipped 2.1 code is *wrong*, only that the next edit to it is unprotected, and Reviewer A owns implementation correctness. The fuzz-matrix omission (F9) and the two guard fail-opens (F1, F5) are cheap, contained fixes worth taking before the tag; everything else is post-tag debt with a clear remediation each.

---

## Guard inventory

Every source-scan guard on HEAD. "Scanned" is measured by replicating each guard's own file-selection and splitter in Python against the working tree — the decisive answer to "does its scan actually reach production code".

| # | Guard | file:line | Scans | Target list resolves? | `production_half`? | Bites today? |
|---|---|---|---|---|---|---|
| 1 | `mod_rs_stays_decomposed` | `src/app/mod_tests.rs:25` | `include_str!("mod.rs")` line count vs CEILING 1500 | yes (compile-time) | n/a | **Yes**, but at 1448/1500 — 96.5% consumed |
| 2 | `flashed_errors_render_their_whole_chain` | `src/app/mod_tests.rs:65` | `src/**` via `scan_rs`; 217 files, 4289 KB (100%) | yes | no (conservative direction) | **Partly — fail-open, see F5** |
| 3 | `state_left_listing_dir_uses_are_allowlisted` | `src/app/mod_tests.rs:92` | `src/app/**` via `scan_rs`; 88 files, 1644/2075 KB (79%) | **stale entry — see F10** | **yes** | Yes |
| 4 | `every_app_module_is_in_the_agents_index` | `src/app/mod_tests.rs:123` | top-level `src/app/*.rs` names vs AGENTS.md | yes | n/a | **Partly — subdir hole, see F6/F15** |
| 5 | `traps_resolve_against_architecture_anchors` | `src/app/mod_tests.rs:168` | `src/**` via `scan_rs` (217 files) + ARCHITECTURE.md, both directions | yes | no (conservative) | Yes |
| 6 | `comments_carry_no_reasoning_leakage` | `src/app/mod_tests.rs:297` | `src/**` incl. tests via `scan_all_rs`; 239 files, 4804 KB (100%) | yes | n/a by design | Yes |
| 7 | `no_author_name_in_home_paths` | `src/app/mod_tests.rs:361` | same 239 files, 4804 KB (100%) | yes | n/a by design | Yes |
| 8 | `production_code_never_uses_crossterms_mouse_capture` | `src/lib.rs:1188` | `src/**` recursive, 239 files, 4797 KB — **`lib.rs` (52 KB) excluded** | yes | no | **NO — fail-open, see F1** |
| 9 | `persistent_state_is_written_atomically` | `src/state/mod.rs:385` (test at `:435`) | `src/state/**` minus test files; 22 files, 159/252 KB (63%) | yes; `ALLOWED` is empty (healthy) | **yes** | Yes |
| 10 | `production_code_never_spawns_git` | `src/git/mod.rs:100` | `src/**` minus test files; 211 files, 3090/4158 KB (74%) | yes | **yes** | Yes |
| 11 | `every_test_git_spawn_resists_an_ambient_git_dir` | `src/git/mod.rs:159` | `src/**` incl. tests, 240 files, 4849 KB (100%), ±window | yes | no by design | **Partly — 3-of-9 contract, see F12** |
| 12 | `pure_draw_modules_touch_no_os` | `src/app/render/mod.rs:1257` | 3 `include_str!` modules, 99 KB (100%) | yes (compile-time) | no (conservative) | Yes |

Non-scanning structural guards, also checked and healthy:

| Guard | file:line | Verdict |
|---|---|---|
| `leader_and_pane_namespaces_respect_tiers` | `src/keymap/resolver/tests/prefixes.rs:356` | Bites for `Space`, `Space w`, `^w`. Hardcodes the one submenu and drops `Sub` entries — see F16 |
| `command_table_is_sorted_and_unique` | `src/app/command_table.rs:176` | Bites |
| `command_table_dispatches_without_unknown` | `src/app/state/tests/dispatch.rs:317` | Bites — checks both dispatch and the declared `CmdLayer` |

**Empirical confirmation:** `cargo test --lib -- guard_tests flashed_errors production_code_never purity_guard state_writes` → **11 passed, 0 failed, 0 ignored** on HEAD. Full suite `cargo test` → **green, 0 ignored** (2303 lib + 15 integration).

### C2 (`guard_support::production_half`) adoption

Exactly three guards consume it: `src/app/mod_tests.rs:101`, `src/state/mod.rs:429`, `src/git/mod.rs:86`. **No guard anywhere in the tree splits on the literal `"#[cfg(test)]"` string** — `grep -rn 'split("#\[cfg(test)\]")' src/ tests/` returns only the prose in `src/guard_support.rs:8` describing the retired heuristic. The `state_left_listing_dir_uses_are_allowlisted` migration to `production_half` happened *inside this window* (`src/app/mod_tests.rs`, `- let production = src.split("#[cfg(test)]").next().unwrap_or("");`). The remaining nine guards do not use it, and each is correct not to: they either scan tests deliberately (6, 7, 11), or scan whole files in the conservative false-positive direction (2, 5, 12), or do not scan file bodies at all (1, 4). **This charter item is clean.**

---

## Fuzz-target status

`fuzz/fuzz_targets/` holds **seven** targets, not six — the charter's count is stale (see Premises). All seven built against HEAD's APIs with `nightly-aarch64-apple-darwin` + `cargo-fuzz`, `cargo +nightly fuzz build` → **exit 0, 3m34s**, seven binaries in `fuzz/target/aarch64-apple-darwin/release/`.

| Target | `[[bin]]` in `fuzz/Cargo.toml` | Builds on HEAD | In `.github/workflows/fuzz.yml` matrix | In `Makefile:140` help |
|---|---|---|---|---|
| `dsl_parse` | yes | ✅ | ✅ | ✅ |
| `expand_path` | yes | ✅ | ✅ | ✅ |
| `expand_percent` | yes | ✅ | ✅ | ✅ |
| `highlight` | yes | ✅ | ✅ | ✅ |
| `render_markdown` | yes | ✅ | ✅ | ✅ |
| `word_wrap` | yes | ✅ | ✅ | ✅ |
| **`archive_name`** | yes (`fuzz/Cargo.toml:73-78`) | ✅ | ❌ **absent** | ❌ **absent** |

`archive_name` also *runs* clean: `cargo +nightly fuzz run archive_name -- -runs=300000 -max_total_time=40` → **300 000 runs in 28 s, cov 164, ft 554, corpus 125 inputs, no crash, exit 0**. So the target is healthy; it is only the CI wiring that is missing (F9).

The assertion depth is better than the target files suggest — the properties live in the `spyc::fuzz` facade, not the `fuzz_target!` bodies. `src/lib.rs:62-70` really does assert the escape property (`!starts_with('/')` and no `..` component) on every `Ok` normalization, and `src/lib.rs:96-109` really does assert char-boundary and sliceability on every wrap range. The doc comments on `fuzz_targets/archive_name.rs` and `word_wrap.rs` that claim these properties are **accurate**, not aspirational.

---

## Test-theatre findings, with bite-check evidence

### Mouse routing — the pure half is exemplary, the dispatch seam is unguarded

`src/app/mouse/` test counts by file: `route.rs` **68**, `selection.rs` **10**, `tab_hit.rs` **8**, `mod.rs` **6**, `scroll.rs` **0**, `forward.rs` **0`.

That distribution is the finding. `route_mouse`, `region_at`, `decide_agent_view_action`, `tab_at` and `clipboard_delivery` all have real precedence matrices with negative cases. But every one of them is a *decision*; nothing asserts what the decision is then wired to. Three shipped user-visible defects in this subsystem lived in the untested half.

- **`handle_mouse` has no test at all.** `grep -rn 'handle_mouse' src/` → one production caller (`src/app/run.rs:221`), one definition (`src/app/mouse/mod.rs:166`), two comments. Zero tests. **Bites:** (a) change `mod.rs:336` `MouseSink::Paste => vec![Effect::PasteFromClipboard]` to `self.forward_to_child(ev, &layout)` — `route.rs:1019` and `:1092` prove middle-click *yields* `Paste` from every region, but nothing binds `Paste` to the effect, so the suite stays green; (b) delete `self.focus_region(region);` from the `FocusAndForward` arm at `mod.rs:350` — green, even though `route.rs:74-77` documents that this combined variant exists *precisely because* "a sink that only forwarded was how the focus half came to be silently missing while a test asserting the sink still passed"; (c) delete the `if !crate::mouse_capture_is_on() { return Vec::new(); }` gate at `mod.rs:167` — green, despite `mod.rs:159-165` citing a shipped incident (unsolicited reports pasting the clipboard with the feature off).
- **`forward.rs` press/release pairing is unasserted.** `mouse_press_forwarded` appears only at `src/app/mod.rs:935`/`:1084` (decl/init) and `src/app/mouse/forward.rs:34`/`:58`/`:79` — never in a test. **Bite:** delete `forward.rs:33-35`, so releases stop reaching the child and a claude drag-select never completes a click — the exact failure `mod.rs:173-178` documents. Green.
- **`scroll.rs` has zero tests, and its one regression test models the caller instead of calling it.** `one_flick_past_the_bottom_sends_the_close_key_once` (`src/app/mouse/route.rs:1651-1688`) hand-rolls its own five-tick loop with its own `pending` variable. The defect it commemorates lived in `src/app/mouse/scroll.rs:121-125`. **Bite:** revert that block to `if pending.is_some() && is_open { self.view.pane_view_sent = None; }` — the test never touches `scroll.rs`, so it stays green while `q` again types into codex's composer once per wheel tick. To its credit the test's own docstring (`route.rs:1644-1649`) discloses that it *models* the caller; a model that can drift from its subject still pins nothing.
- **Pane-text selection coordinate translation is unpinned.** **Bite:** `src/app/mouse/selection.rs:213` `Some((row, col))` → `Some((col, row))` — highlight and copied text both go transposed, nothing fails. The pager has a dedicated test for the identical trap (`src/ui/pager/tests/selection.rs:527`); the pane side does not.
- **`tab_widths` — the click geometry — is never evaluated.** All eight `tab_hit` tests feed hand-written widths (`&[6, 6]`, `&[8, 6, 7]`, `&[4, 4]`). **Bite:** `src/app/mouse/tab_hit.rs:49` `1 // "─" separator` → `0` — all eight pass. The cross-check that would catch it is the `debug_assert_eq!` at `src/app/render/chrome.rs:164-174`, but no test both populates `runtime.pane_tabs` and draws a frame, so it never executes. The module's own doc (`tab_hit.rs:8-13`) names this failure: "clicks are off by one sometimes".

### Clipboard dispatch — intents pinned, execution not

`delivery_tests` (`src/app/clipboard.rs:476-503`) pins the `(local, osc52)` tuple exactly per mode × ssh, and the OSC-52 encoder is asserted *by content* (`src/clipboard.rs:813-889`: exact `"\x1b]52;c;aGk=\x07"`, tmux DCS wrap with doubled ESC, payload-alphabet injection resistance, refuse-not-truncate at the cap plus the just-under case). That is the good half.

`deliver_clipboard` (`src/app/clipboard.rs:435-465`), which consumes the tuple, has no test. **Bites, all green:** drop the early `return` at `:436-441` so a user command runs *and then* `via` also runs, violating the documented "exclusive top-priority tier, not one more mechanism to also try" (`:432-434`); swap the `osc52` and `local` blocks at `:445-456`, defeating the documented OSC-52-first rationale (`:428-430`). Separately, the platform helper cascade (`src/clipboard.rs:389-420` `copy_impl`, `:133-158` `paste_impl`) reads `WAYLAND_DISPLAY`/`DISPLAY` inline and is untested — reordering `xclip`/`xsel` at `:409-413`, or dropping `-n` from `wl-paste` at `:139` (re-adding the newline the comment says it exists to prevent), fails nothing on any platform. This is the one place in the area that departs from the repo's own pure-decision template; there is no `resolve_backends(is_wayland, has_display)` to test the way `clipboard_delivery` is.

### Archive — the pure layers are excellent, the write-path safety rails are structurally uncovered

This is the largest new test mass in the diff (`src/app/harness_tests/archive.rs` +3181, `tests/archive_roundtrip.rs` +163, `src/archive/read/tests.rs` new, plus inline blocks). It is **not** a theatre-heavy area: `journal::plan_repack`, `index::normalize`, `budget`, `scan::assess` and the mount registry are among the more honest test code in the tree, several tests are deliberately built to be un-fakeable, and I confirmed by mutation that they bite hard.

The dishonesty is concentrated in one place, and it has a consistent shape: **each safety rail's pure half is well tested, and its wiring and failure path are not — so the rail reads as covered when only its decision function is.** All bite checks below were *executed* against a clone in the scratchpad (2303 lib tests + 4 integration binaries per run), not reasoned about.

- **The repack verify step is never exercised.** `src/archive/write.rs:340` (body), called once at `:106`. **Bite (run):** replace the body of `verify` with `if true { return Ok(()); }` → **all 2303 lib tests + every integration binary pass.** The docstring at `:334-339` — *"This is what makes the write trustworthy rather than hopeful"* — is unbacked. Aggravating: `a_plan_referencing_an_unknown_member_is_refused` (`src/archive/write.rs:953-969`) carries the docstring *"Verification is what catches a plan the writer couldn't honour"*, which is false — its step names a member that `write_zip` rejects at `:147-149` (`index.get(inner).with_context(…)?`) long before `verify` runs, proven by the fact that it still passes with `verify` gutted. The one test claiming this coverage is testing a different guard.
- **The verify → snapshot → rename ordering is unenforced.** `src/archive/write.rs:106` vs `:120`. **Bite (run):** move `verify(...)` to *after* `tmp.persist(archive)`, so a doomed archive replaces the original and is checked afterwards → **all 2303 tests pass.** Both the module docstring (`:5-7`) and `repack`'s (`:46-49`) state this ordering as *the* safety property (*"A failure anywhere leaves the original byte-identical, because nothing touches it until the rename"*). The same mutation also defeats the graveyard-ordering claim at `:47-49`.
- **`a_read_only_mount_refuses_to_be_written` asserts its own setup.** `src/app/harness_tests/archive.rs:943-964` hand-writes `mount.capability = Capability::ReadOnly("2 duplicate member name(s)".to_string())` and asserts the write is refused. The gate it hits (`src/app/archive.rs:1136`) is real, so the test is not worthless — but the demotion string is a fiction, and **no test anywhere builds an archive that genuinely earns one.** **Bite (run):** replace `let capability = assess(&indexed.facts, format);` at `src/app/archive_ops.rs:391` with an unconditional `Capability::ReadWrite` → **all 2303 tests pass.** A real duplicate-name zip, or a tar with hardlinks/device nodes, would mount ReadWrite and be repacked lossily with nothing failing. `src/archive/scan.rs:198-275` tests `assess` thoroughly as a pure function, which is exactly why the wiring gap is invisible. Same root: **bite (run)** blanking the read-only indicator at `src/app/render/chrome.rs:462` (`" ro"`) and `src/app/archive.rs:866` (`" (ro)"`) also passes everything.
- **`the_status_suffix_names_the_container` asserts what the fixture filename already guarantees.** `src/app/harness_tests/archive.rs:467-488` renders a frame and asserts `rendered.contains("zip")` — but inside the mount the column's path is `…/pkg.zip`, painted in the path line, so `"zip"` is on screen whether or not the archive tag exists. **Bite (run):** blank the format label at `src/app/render/chrome.rs:469` → all tests pass. Same weakness at `:2251`. This is the recorded *my-test-asserted-its-own-narrowness* trap exactly: the requirement is "the bar names the container", the assertion is "the word zip is somewhere on the screen". (The sibling `after.contains("~1")` at `:2270` is strong and does bite — only the container half is weak.)
- **`an_impossible_date_does_not_panic` never calls spyc code.** `src/archive/read/tests.rs:468-473` — the entire body is `assert!(zip::DateTime::from_date_and_time(2026, 0, 0, 0, 0, 0).is_err());`. That is a test of the third-party `zip` crate's constructor. The name and comment promise that `zip_mtime` (`src/archive/read/mod.rs:480`) survives a corrupt DOS field on the indexing worker; `zip_mtime` is never called. Verified by direct read — no mutation needed. This is the single clearest "asserts the mock" instance in the diff.
- **The write-time free-space precheck is untested.** `src/archive/write.rs:64-71`. **Bite (run):** insert `&& false` into the condition → nothing fails. Every write test uses `free_space_margin: 0` (`:395-400`) against a roomy tempdir, so the branch is never taken. The *mount-time* equivalent is well covered (`src/archive/budget.rs:240-252`), which is what makes the write-time one look covered.
- **Minor:** `a_mounted_column_sits_somewhere_that_is_not_a_directory` (`archive.rs:124-140`) restates its fixture — `assert!(here.is_file())` re-asserts that `build_zip` wrote a file, and the sibling at `:104-119` already asserts `listing.dir == archive` more precisely. `eviction_hands_back_the_staging_tree_to_clean` (`:493-531`) never constructs an `App` and duplicates `src/archive/mount.rs:483-507`; its one added claim (`roots[0].exists()` — `Mounts::insert` doesn't unlink) is real and does bite. The `settle()` driver (`:537-585`) runs `for _ in 0..4` with no assertion that the queue drained, so a longer effect chain would silently run later assertions against half-applied state instead of failing loudly.
- **Possible real bug, untested either way** (flagging for Reviewer A, not claiming it): `src/archive/read/mod.rs:225-229` — the comment says *"`staging_rel` is what a reader looks under, so a case-colliding member is restored where its own reader expects it"*, but the code checks and writes `clean.inner`, not `entry.staging_rel()`. Both refill tests (`src/archive/write.rs:788`, `:842`) use non-colliding names.

**Answers to the three targeted questions:**
1. *Does anything verify the repack verify-by-reading-back step?* **No.** Gutting `verify` passes the entire suite, and the one test whose docstring claims that coverage is actually caught by `write_zip`'s index lookup.
2. *Does anything verify a FAILED write leaves the original byte-identical?* **Partially.** `a_failed_repack_leaves_the_original_untouched` (`src/archive/write.rs:925-949`) covers a failure in the *writer* stage, compares the archive's actual bytes before/after, and genuinely bites. Nothing covers a failure at the *verify* stage (impossible today, since verify is never made to fail) or after `persist`, and the ordering is unenforced.
3. *Does anything verify zip-slip rejection end to end?* **Yes for streamed tar** — `src/archive/read/tests.rs:325-356`, whose fixture forges a raw tar header and recomputes the checksum *because* `tar::Builder` refuses the name; it asserts both that the escape file is absent and that the safe sibling landed, so it cannot pass by extracting nothing. There is no zip-format equivalent, but both formats funnel through the same `IndexBuilder::push`/`normalize`, so I read that as informational rather than a gap.

### Over-narrow assertion (MCP)

`searching_inside_a_mount_is_refused_rather_than_answered_emptily` (`src/mcp/tests/mod.rs:1405`) asserts `msg.contains("archive") || msg.contains("not a directory")` at `:1439-1441`. A response of the shape `"0 matches under /tmp/x/archive.zip"` — an *empty answer* that merely names the archive path — satisfies it. That is exactly the outcome the test name promises to exclude. The test does bite for the total-silence case (`unwrap_or_default()` → `""` fails both `contains`), so it is not vacuous, just weaker than its name.

---

## Flake posture

**`#[ignore]`: zero occurrences in `src/`, `tests/`, `fuzz/`.** The only hit for the string is prose at `src/ui/line_edit.rs:870` describing tests that *used to be* ignored. Full-suite run confirms `0 ignored` everywhere. This is a clean posture and matches the owner's "no CI flakes allowed — fix, don't re-run" directive.

Sleeps and retries in test code, each checked against its stated justification:

| Site | Shape | Justification | Verdict |
|---|---|---|---|
| `src/git/test_support.rs:88-98` | `RUN_GIT_MAX_ATTEMPTS = 6`, 50 ms × attempt backoff, panics with the last error | Documented as riding out transient temp-volume/cwd churn | **Accept.** Bounded, loud on exhaustion. Note the doc itself now says the 2026-07-02 lock failure "was almost certainly the same [GIT_DIR] leak … papered over by widening the retry budget" — the retry is retained as belt-and-braces after the root cause was fixed, which is defensible but worth revisiting once a few clean months pass |
| `src/git/test_support.rs:44-91` (new) | `git_command` strips 9 `GIT_REDIRECT_ENV` vars | The 2026-08-07 split-brain-reset incident | **Accept — this is the model fix.** Root-caused, guarded, documented |
| `src/clipboard.rs:554-575` (new) | `retry_text_busy`, 50 × 10 ms, **only** on `ETXTBSY` | rust-lang/rust#74253: a parallel test's fork inherits the writable fd across exec | **Accept.** Narrowest possible: every other outcome returns untouched on the first attempt, so tests asserting a specific error still see it |
| `src/app/state/git.rs:375` (new) | unconditional `sleep(1100 ms)` | fs mtime granularity, so an immediate write lands in a new tick | **Accept but costly.** Correct reasoning; 1.1 s of pure wall-clock on every suite run. A settable clock or a forced `set_file_mtime` would remove it. Low priority |
| `src/app/harness_tests/mcp.rs:440-446`, `per_column.rs:23-29`/`:57-63` (new) | bounded poll loop (200–500 × 10 ms) then a hard assert | worker lands off-thread with no wake tx in tests | **Accept.** The correct shape — deadline + assertion, not sleep-and-hope |
| `tests/pane_roundtrip.rs:68-80` | `Instant` deadline + 20 ms poll, then `kill` | bound a hung `cat` | **Accept** |
| `src/mcp/protocol.rs:1175` | 2 s sleep *inside* the closure under test | proves `call_with_timeout` returns `Err` while the work outlives the deadline | **Accept.** The sleep is the fixture, the 20 ms deadline is the subject; the test returns in ~20 ms |

No new `#[ignore]`, no unbounded waits, no bare `sleep`-then-assert introduced since 2.0. **This charter item is clean.**

---

## Findings, severity-ordered

Severity is for the *tag*: blocker holds it, high should be scheduled, medium/low are debt.

### F1 — HIGH — `src/lib.rs:1214-1216`
**The `EnableMouseCapture` guard exempts the one file that can violate it.**
`offenders.retain(|p| p.file_name().is_none_or(|n| n != "lib.rs"));` drops `lib.rs` unconditionally, because `lib.rs` names `EnableMouseCapture` in prose at `:520` and `:1013`. But `setup_terminal` (`src/lib.rs:790`), `restore_terminal` (`:845`) and `resume_tui` (`:921`) all live in `lib.rs` — and the guard's own doc at `:1180-1184` names exactly those as the risk: *"one convenient-looking call anywhere (`setup_terminal`, `resume_tui`, a future feature) reintroduces the redraw storm while every existing test still passes."*
**Bite check (performed):** copied `src/` to the scratchpad, injected `execute!(io::stdout(), crossterm::event::EnableMouseCapture)?;` as the first statement of `setup_terminal`, replayed the guard's exact scan-and-retain logic. Offenders before the retain: `['lib.rs']`. After: `[]`. **Guard verdict on the mutated tree: PASS.** The guard cannot fire on the only file where the bug can be written.
Introduced pre-2.0 (`c8c820e` / `d3b2e13`), in scope because the mouse surface roughly doubled in this window (`src/app/mouse/` split into six files, `tab_hit.rs` added).
**A fix would need to:** stop excluding a whole file — e.g. strip `//`-comment text before matching, or match a call-shaped needle (`event::EnableMouseCapture` / `EnableMouseCapture)`) that prose cannot satisfy, then delete the `retain`.

### F1b — HIGH — `src/archive/write.rs:340` (body), `:106` (call)
**The repack verify step — the stated basis for trusting the write — is never exercised.**
**Bite check (executed):** replacing `verify`'s body with `if true { return Ok(()); }` leaves all 2303 lib tests and every integration binary green. The one test whose docstring claims this coverage (`a_plan_referencing_an_unknown_member_is_refused`, `:953-969`) is actually caught by `write_zip`'s index lookup at `:147-149`, proven by the same mutation.
**Severity note:** the subagent proposed *blocker*. I am **downgrading to high**. Nothing here says the shipped 2.1 code is wrong — `verify` is present, and the archive path has been extensively hand-driven (15 post-#149 PRs). What is missing is regression protection for the next edit. That is serious debt, not a reason to hold the tag. Reviewer A owns whether the implementation itself is correct.
**A fix would need to:** make `verify` fail on purpose — a `RepackStep` whose `out` name the zip writer mangles, or an injected truncation of the temp file — and assert both the error *and* that the original is byte-identical afterwards.

### F1c — HIGH — `src/archive/write.rs:106` vs `:120`
**The verify → snapshot → rename ordering, which both docstrings call *the* safety property, is unenforced.**
**Bite check (executed):** moving `verify(...)` to after `tmp.persist(archive)` — so a doomed archive replaces the original and is checked afterwards — leaves all 2303 tests green. Same mutation defeats the graveyard-ordering claim at `:47-49`.
**A fix would need to:** with a deliberately-failing verify in place (F1b), assert the archive on disk is unchanged and no graveyard entry was written.

### F1d — HIGH — `src/app/archive_ops.rs:391` + `src/app/harness_tests/archive.rs:943-964`
**`assess()`'s wiring is untested, and the test that looks like it covers it asserts its own setup.** The read-only demotion string is hand-written into the mount; no test builds an archive that genuinely earns one.
**Bite check (executed):** replacing `assess(&indexed.facts, format)` with an unconditional `Capability::ReadWrite` leaves all 2303 tests green — a real duplicate-name zip would mount ReadWrite and be repacked lossily. Blanking the `" ro"` / `" (ro)"` indicators (`src/app/render/chrome.rs:462`, `src/app/archive.rs:866`) is likewise green.
**A fix would need to:** mount a real duplicate-name zip (or a tar with hardlinks) and assert `capability` is `ReadOnly` end to end, plus one render assertion on the indicator.

### F2 — HIGH — `src/app/mouse/mod.rs:166-366`
**`handle_mouse`, the single mouse dispatch entry, has no test.** Middle-click → `Effect::PasteFromClipboard`, the focus half of `FocusAndForward`, the `mouse_capture_is_on` gate, and the scroll sign are each individually mutable to a wrong value with the suite green. Evidence and three bites above.
**A fix would need to:** add App-level tests that call `handle_mouse` with a synthetic `MouseEvent` against a real `compute_layout`, asserting the returned `Vec<Effect>` *and* the focus side effect — not the `MouseSink`.

### F3 — HIGH — `src/app/mouse/forward.rs` (whole file, 0 tests)
**The documented press/release pairing obligation is unasserted.** `mouse_press_forwarded` never appears in a test. Deleting the set at `:33-35` or the `std::mem::take` at `:79` leaves the suite green while breaking the invariant `forward.rs:72` and `mod.rs:173-178` describe.
**A fix would need to:** assert that a `Down` followed by an `Up` reaches the *same* child, and that an unclaimed `Up` is not forwarded.

### F4 — HIGH — `src/app/mouse/scroll.rs` (whole file, 0 tests) + `src/app/mouse/route.rs:1651`
**The regression test for a shipped user-visible bug models the caller instead of calling it.** Reverting `scroll.rs:121-125` to the buggy form leaves `one_flick_past_the_bottom_sends_the_close_key_once` green. `send_scroll_keys` (`:32`), `send_agent_view_scroll_keys` (`:74`), the fast-vs-normal key choice (`:152-167`) and the DECCKM `app_cursor` threading (`:89`) are all unpinned.
**A fix would need to:** drive `send_agent_view_scroll_keys` itself across five ticks and count the emitted close keys.

### F5 — MEDIUM — `src/app/mod_tests.rs:70-82`
**The flashed-error guard only sees single-line call sites, and a site escapes it on HEAD.**
The detection is a same-line conjunction: `line.contains("flash_error(format!(") ... && line.contains(&needle)`. A wrapped call puts the two on different lines and is invisible.
**Bite check (performed, empirical):** `src/app/effect.rs:896-898` is
```
self.state.flash_error(format!(
    "couldn't read back edits ({e}); preserved at {}",
    path.display()
));
```
and `cargo test --lib -- flashed_errors` **passes** on HEAD. The guard is blind to it. Scanning the tree with the guard's own file-selection but a multi-line-aware matcher finds exactly this one site — so the *live* exposure is small, and this particular `e` is an `io::Error` from `std::fs::read_to_string`, whose `Display` ignores the `#` flag, meaning nothing is actually lost to the user today. The defect is the guard, not the site.
**A fix would need to:** match across the `format!(` argument list (scan forward to the balancing paren) rather than one line, then re-run and fix whatever it surfaces.

### F6 — MEDIUM — `src/app/mouse/tab_hit.rs` (file) + `src/app/mod_tests.rs:127`
**A module added in this window is absent from the guard-checked AGENTS.md index, and the guard structurally cannot see it.**
`every_app_module_is_in_the_agents_index` iterates `read_dir(root.join("src/app"))` and `continue`s on any non-`.rs` entry — so directories are skipped entirely. `src/app/mouse/tab_hit.rs` was added by `309f838` *feat(mouse): click a pane tab to switch to it (#279)*, post-2.0. `grep -rn 'tab_hit' --include='*.md' .` returns **nothing** — it is in no doc at all, and the AGENTS.md `mouse/` bullet (`AGENTS.md:64`) enumerates exactly five of the six files. AGENTS.md advertises this index as guard-checked; for `src/app/*/` it is not.
**A fix would need to:** either recurse one level (checking `src/app/<dir>/*.rs` against the same index) or document the limit; and add the missing `tab_hit.rs` bullet.

### F7 — MEDIUM — `src/app/mouse/tab_hit.rs:34-56`
**The click geometry itself is never evaluated; the renderer cross-check that would catch it never runs.** All eight tests feed hand-written widths. `chrome.rs:164-174`'s `debug_assert_eq!` is the intended backstop, but no test both populates `runtime.pane_tabs` and draws a frame.
**A fix would need to:** one test that renders a divider with 2–3 tabs (one suspended) through `TestBackend` and asserts a click at a computed column selects the expected tab.

### F8 — MEDIUM — `src/app/clipboard.rs:435-465`
**`deliver_clipboard` is untested; only the precedence tuple it consumes is pinned.** Bites at `:436-441` (drop the exclusive early return) and `:445-456` (swap OSC-52/local order) are both green. Related: `src/clipboard.rs:389-420` / `:133-158` platform cascade has no pure-decision extraction and no test.
**A fix would need to:** extract `resolve_backends(is_wayland, has_display) -> Vec<(prog, args)>` in the `clipboard_delivery` style and test it; and assert `deliver_clipboard`'s ordering with stub helpers.

### F9 — MEDIUM — `.github/workflows/fuzz.yml:56-62` and `Makefile:140`
**`archive_name` is not in the weekly fuzz matrix or the Makefile help.** The workflow matrix lists six targets; `fuzz/Cargo.toml` declares seven. `archive_name` covers attacker-controlled archive member names — the target with the clearest threat-model justification in the set (`SECURITY.md` threat model, and `src/lib.rs:59-61` calls it "the boundary that makes zip-slip impossible"). It builds and runs clean (300 k runs, cov 164, no crash), so nothing is broken — it has simply never run in CI, accrues no corpus, and a future API break in it would not be noticed by any scheduled job. The `[[bin]]` block was also appended *after* the `[workspace]` table in `fuzz/Cargo.toml:70-78`, orphaning the "Own workspace so the parent crate's build/gate ignores this entirely" comment from the table it explains (valid TOML; cosmetic).
**A fix would need to:** add `- archive_name` to the matrix at `fuzz.yml:62` and to the `TARGET=` list in the `Makefile:140` help line.

### F10 — LOW — `src/app/mod_tests.rs:93`
**A stale allowlist entry re-opens the exact hole that was just closed.** `const ALLOW: &[&str] = &["run.rs", "bootstrap.rs", "chrome.rs"];` — but `chrome.rs` no longer contains `state.left.listing.dir`. It did at v2.0.0 (`git show v2.0.0:src/app/render/chrome.rs` → line 392); `b1478f1` *fix(status): the status bar describes the focused column, not always column a (#322)* removed it in this window. The exemption's stated rationale (`mod_tests.rs:49-51`: "the status-bar header (`chrome.rs`, deliberately anchored to the primary column)") is now contradicted by shipped behaviour — the status bar deliberately follows focus, and `the_status_bar_describes_the_focused_column` (`src/app/render/mod.rs:1022`) asserts it. So the guard silently pre-approves a regression in the one file that just got fixed for it. Secondary: the allowlist matches on `file_name()` only, not path, so *any* file so named anywhere under `src/app` is exempt.
**A fix would need to:** drop `chrome.rs` from `ALLOW`, correct the rationale comment, and match on the path relative to `src/app` rather than the bare file name.

### F11 — LOW — `src/app/mod_tests.rs:38-64`
**Doc-comment misattachment introduced in this window.** `flashed_errors_render_their_whole_chain` was inserted *into the middle of* the existing doc block for `state_left_listing_dir_uses_are_allowlisted`. The result: lines 38-51 (the per-column rationale + the allowlist explanation) and lines 52-63 (the anyhow-cause rationale) are one `///` block attached to `flashed_errors_...` at `:65`, while `state_left_listing_dir_uses_are_allowlisted` at `:92` has no doc comment at all. The diff hunk (`@@ -49,6 +49,45 @@`) shows the splice.
**A fix would need to:** move lines 38-51 back above `fn state_left_listing_dir_uses_are_allowlisted`.

### F12 — LOW — `src/git/mod.rs:159-183`
**The new git-env guard enforces a 3-var subset of the 9-var contract it cites.** `spawn_sites_missing_env_hygiene` accepts a window containing the literal `env_remove("GIT_DIR")`. `src/git/test_support.rs:35-43` declares `GIT_REDIRECT_ENV` with nine variables and `:46-48` says *"Every test-side git spawn must go through this — `-C` alone is not enough."* Ten sites hand-roll a partial strip (3 of 9) and pass the guard: `src/app/harness_tests/per_column.rs:208`, `:281`, `:372`; `src/app/state/tests/mod.rs:684`, `:759`, `:847`, `:926`, `:1119`; `src/mcp/config.rs:1083`; `src/merge_driver.rs:286` (test half — confirmed, `#[cfg(test)]` opens at `:178`, so `production_code_never_spawns_git` is correctly silent). Practical exposure is limited because git only exports a subset of these into hooks (`GIT_DIR`, `GIT_INDEX_FILE`, `GIT_PREFIX`, and `GIT_WORK_TREE` for some hooks) and the three most dangerous are covered everywhere — so this is contract drift, not a live corruption path.
**A fix would need to:** require the window to contain `git_command(` (the canonical helper) or the full `GIT_REDIRECT_ENV` loop, and convert the ten sites.

### F12b — MEDIUM — `src/archive/read/tests.rs:468-473`
**`an_impossible_date_does_not_panic` tests the `zip` crate, not spyc.** The whole body is `assert!(zip::DateTime::from_date_and_time(2026, 0, 0, 0, 0, 0).is_err());`. The name and comment promise that `zip_mtime` (`src/archive/read/mod.rs:480`) survives a corrupt DOS field on the indexing worker; `zip_mtime` is never called. This is the clearest single "asserts the mock" instance in the diff, and it is new in this window.
**A fix would need to:** call `zip_mtime` with the impossible `DateTime` (or the raw field) and assert it yields "no timestamp" rather than panicking.

### F12c — MEDIUM — `src/app/harness_tests/archive.rs:487` (and `:2251`)
**`the_status_suffix_names_the_container` asserts what the fixture filename already puts on screen.** `rendered.contains("zip")` over a full-frame render, where the column path `…/pkg.zip` is painted regardless. **Bite (executed):** blanking the format label at `src/app/render/chrome.rs:469` leaves it green.
**A fix would need to:** assert the tag's actual shape (the rendered suffix substring, e.g. the badge as `chrome.rs` composes it), not the substring `"zip"` anywhere in the buffer.

### F12d — LOW — `src/archive/write.rs:64-71`
**The write-time free-space precheck is untested** (`&& false` in the condition fails nothing); every write test runs `free_space_margin: 0` against a roomy tempdir. The mount-time equivalent is well covered (`src/archive/budget.rs:240-252`), which is what makes this one look covered.

### F12e — LOW — `src/app/harness_tests/archive.rs:537-585`
**The `settle()` effect driver gives up silently.** `for _ in 0..4 { … }` with no assertion that the queue drained, so a change lengthening an effect chain past four hops runs later assertions against half-applied state instead of failing loudly.
**A fix would need to:** `assert!(queue.is_empty(), …)` after the loop.

### F13 — LOW — `src/mcp/tests/mod.rs:1439-1441`
**Assertion is weaker than the test's own name.** `msg.contains("archive") || msg.contains("not a directory")` is satisfied by an empty-result message that merely names the archive path — the precise outcome `searching_inside_a_mount_is_refused_rather_than_answered_emptily` exists to exclude.
**A fix would need to:** assert the response is an *error* (or carries an explicit refusal marker), not merely that its text mentions "archive".

### F14 — LOW — `src/app/mouse/route.rs:1250`
**The column half of the coordinate-translation test cannot discriminate.** `assert_eq!(report.col, 5)` with input `column: pane.x + 5`, where `compute_layout` gives the pane `x: area.x == 0` in every branch. **Bite:** `forward.rs:186` `col: ev.column.saturating_sub(origin.x)` → `col: ev.column` — green. The row half is properly pinned and even guards `assert!(pane.y > 0, "pane must be offset for this to prove anything")` at `:1235`; the column half has no equivalent. Low because non-zero `pane.x` is currently unreachable in production — but it becomes reachable the moment a `FullHeight` vsplit hosts the pane in the right column.
**A fix would need to:** construct a layout with a non-zero pane origin (or assert the guard-clause explicitly), mirroring the row half.

### F15 — INFORMATIONAL — `src/app/mod_tests.rs:138`
**The index guard's substring match can be satisfied by a longer sibling.** `if !agents.contains(name)` — so a hypothetical undocumented `src/app/route.rs` is satisfied by the string `archive_route.rs` in AGENTS.md; likewise `tabs.rs` by `pane_tabs.rs`, `scroll.rs` by `pane_scroll.rs`, `ops.rs` by `file_ops.rs`. Latent only: I checked all 64 top-level `src/app/*.rs` and every one is named with its own backticked mention, so there is no current false pass.
**A fix would need to:** match a delimited form (`` `<name>` `` or `/<name>`).

### F16 — INFORMATIONAL — `src/keymap/resolver/tests/prefixes.rs:359-392`
**The tier guard walks depth-1 plus one hardcoded submenu.** `actions_after` filters out `ChordEntry::Sub(..)`, and the only submenu descended into is `Space w` (hardcoded at `:377`). Today that is complete — `PendingSeq::Leader` has exactly one `Sub` (`resolver/mod.rs:158`) and `PendingSeq::W` has exactly one (`Sub("Space", …)` at `:205`, which loops back to Leader). But a new submenu under either prefix is silently unchecked. Separately, `PendingSeq::CtrlS` (`^s n` / `^s x`, `OpenSecondCommander`/`CloseSecondCommander`, `resolver/mod.rs:207-210`) is not checked at all; AGENTS.md places vsplit in the PANE tier, so it arguably belongs in the guard's scope.
**A fix would need to:** recurse into `Sub` entries instead of hardcoding, and decide explicitly whether `^s` is in the pane namespace.

### F17 — INFORMATIONAL — `src/app/render/snapshots/spyc__app__render__render_tests__snapshot_frame_second_commander.snap:3`
**A snapshot carries an `assertion_line: 1010` header.** Current insta strips this on accept; its presence means the file will churn (and produce a spurious diff) on any edit that shifts line numbers above 1010. The sibling snapshot updated in the same window does not have it.
**A fix would need to:** re-accept the snapshot with a current `cargo insta`.

### F18 — INFORMATIONAL — `src/app/mod_tests.rs:26`
**The anti-monolith ceiling is 96.5% consumed.** `CEILING: usize = 1_500`; `src/app/mod.rs` is **1448** lines. The guard's own doc says "extract a module, don't bump the ceiling" and records the last ratchet (4000 → 1500). Worth naming now so the next feature lands as an extraction rather than as a ceiling bump under deadline.

---

## Premises checked

1. **"Fuzz targets: all six…" — WRONG on HEAD, as the orchestrator suspected.** There are seven (`archive_name` added in this window). Corrected throughout; the seventh is the subject of F9.
2. **"…the weekly workflow's target list matches `fuzz/fuzz_targets/`" — FALSE.** `.github/workflows/fuzz.yml:56-62` lists six; `fuzz/fuzz_targets/` and `fuzz/Cargo.toml` both hold seven. `Makefile:140`'s help string has the same six. Confirmed mismatch → F9.
3. **"Confirm each guard consumes the shared splitter (C2)" — the premise is too strong.** Only three of twelve guards consume `production_half`, and that is correct: the other nine either scan test code deliberately, or scan whole files in the conservative (false-positive) direction where `production_half` would add nothing. The thing AGENTS.md actually forbids — splitting on the literal `"#[cfg(test)]"` — occurs **nowhere** in the tree. I report the charter item as clean, with the nuance that "uses `production_half`" is not the right pass/fail test; "does not truncate on the literal" is.
4. **"A guard scanning a file that moved in the mouse split is silently scanning nothing" — NOT observed.** No guard names a moved file. `pure_draw_modules_touch_no_os` uses `include_str!`, so a moved target is a compile error, not a silent pass. Measured scan reach for every guard is 63–100% of its declared scope; none is near zero. The *related* problem I did find is different in kind: a guard whose target list is stale in the permissive direction (F10, `chrome.rs` still allowlisted after its offending line was removed) and a guard that structurally cannot see the mouse split's new directory at all (F6).
5. **Fuzz-target doc claims vs bodies.** I initially flagged `archive_name.rs` and `word_wrap.rs` for promising assertions their `fuzz_target!` bodies do not contain. **That was wrong** — the assertions live in the `spyc::fuzz` facade (`src/lib.rs:62-70`, `:96-109`), which is where AGENTS.md says fuzz entry points belong. Both docs are accurate. Withdrawn.
6. **`#[ignore]` / retry / sleep audit.** Charter expected these to need justification-checking. Zero `#[ignore]` in the tree; every new sleep is either a bounded poll with a hard assert, a narrowly-scoped `ETXTBSY` retry, or an mtime-granularity nudge. Nothing to escalate.
7. **"Sample archive, mouse routing, clipboard hardest" — the charter's instinct was right, but the *shape* of the problem is the same in all three**, and worth naming as one pattern rather than three findings: the pure decision function is thoroughly tested, the code that consumes it is not. `route_mouse` (68 tests) → `handle_mouse` (0). `clipboard_delivery` (exact tuple per mode × ssh) → `deliver_clipboard` (0). `scan::assess` (`src/archive/scan.rs:198-275`) → its call site (`src/app/archive_ops.rs:391`, mutable to a constant with the suite green). `journal::plan_repack` (proptest) → `repack`'s verify-and-ordering (mutable to a no-op, green). The repo's own "pure decisions get extracted + tested" convention is being honoured at the extraction step and dropped at the wiring step. That is a single remediation theme, not four.
8. **Severity calibration.** I downgraded three proposed blockers to high (F1b–F1d) and did not promote anything to blocker. The test I applied: does this mean 2.1 ships something *wrong or unknowably risky*? In every case the implementation is present and correct-as-far-as-tested; what is missing is protection for future edits. Reviewer A owns the correctness half; if A finds the archive write path actually broken, F1b/F1c become the evidence that no test would have caught it.
9. **The archive fuzz gap (Reviewer A's item).** I own the build-and-workflow half only: the target builds, runs, and finds no crash in 300 k executions, but is absent from CI. Whether `archive_name` reaches the archive *parser* entry points (as opposed to name normalization alone) is Reviewer A's call — from my side I note it exercises only `crate::archive::index::normalize`, one function, and nothing in `archive/read.rs` or `archive/write.rs`.

---

## Verified correct

Spot-checks that came back clean, so they are not re-litigated downstream:

- **The C2 splitter itself** (`src/guard_support.rs:36-85`) is sound and its own four unit tests are real regression tests, each naming the file and line number of the shape that broke the old heuristic (`render/mod.rs:23` prose; `git/worktree.rs:128` mid-file `mod` decl). The brace-matching handles nesting; the unrecognized-item path is documented as biasing to false positives, which is the correct direction.
- **`src/ui/pager/tests/selection_render.rs`** (whole file, new) — reads real `Cell::bg` off a `TestBackend` draw rather than glyphs, with the module doc explaining exactly why a glyph snapshot is blind to this bug class (#120). Carries the negative cases that stop the fix being over-applied: charwise must not fill the row tail (`:114`), block selection stays a rectangle (`:143`), rows outside the range stay `Color::Reset` (`:81`). This is the best new test file in the diff.
- **Pane restart cluster** (`src/app/harness_tests/pane.rs:729-897`, new) — four tests pinning confirm-before-restart, same-slot respawn (asserting neighbours' ids are unchanged, not just the count), which identity survives (command/cwd/rename) and which does not (pid), and that an `[exited N]` suffix does not ride onto the live replacement. Real assertions on resolved state throughout.
- **The two changed insta snapshots** are both legitimate behaviour changes, each accompanied by a named non-snapshot test: `:_` cursor glyph ← `1b343f8` (#295), covered by `normal_cursor_never_draws_an_underscore` (`src/ui/prompt.rs:277`) which pins the *negative* case; `/projects/demo` → `/projects/other` ← `b1478f1` (#322), covered by `the_status_bar_describes_the_focused_column` (`src/app/render/mod.rs:1022`). Neither is a tautological re-blessing.
- **OSC-52** (`src/clipboard.rs:813-889`) — asserted by exact byte sequence, including the tmux DCS wrap with doubled ESC, unicode base64 round-trip, injection resistance (payload alphabet + exactly one terminator), and refuse-not-truncate at the cap *with* the just-under case.
- **`mouse_mode.rs:73`/`:111`/`:138`** — drives the real process-global through `set_mouse_capture_for_test` and asserts the exact `Effect::SetMouseMode { capture }` payload *and the silence between transitions*. The "never emit `SetMouseMode`" bite is caught cleanly.
- **`route.rs:1277` `every_button_encodes_as_itself_not_as_a_wheel_tick`** — the left-click-sends-wheel-down defect properly pinned: table-driven over all 11 `MouseEventKind`s, plus the cross-check that bit 64 marks a wheel tick and nothing else (`:1311-1315`).
- **`command_table_dispatches_without_unknown`** (`src/app/state/tests/dispatch.rs:317`) — iterates the registry and checks both "does not flash unknown" and "the declared `CmdLayer` matches how it actually routes". Turns the old four-list-desync footgun into a build/test failure.
- **`persistent_state_is_written_atomically`** — `ALLOWED` is empty, which is the healthy state, and the scan reaches 63% of `src/state/` after `production_half` (the remainder is genuine test bodies).
- **`a_failed_repack_leaves_the_original_untouched`** (`src/archive/write.rs:925-949`) — reads the archive's *bytes* before and after and compares, rather than "the file still exists". Bite: pointing `write_zip`/`write_tar` at `archive` instead of `tmp.path()` (`:102-103`) fails it plus 19 others.
- **`a_traversal_member_is_skipped_not_written_outside_staging`** (`src/archive/read/tests.rs:325-356`) — the best fixture in the diff: `hostile_tar_gz_at` (`:50-66`) forges a raw tar header and recomputes the checksum *because* `tar::Builder` refuses the name, i.e. it builds the attacker's archive rather than testing the writer's validation. Asserts both that the escape file is absent and that the safe sibling landed, so it cannot pass by extracting nothing. Bite: making `IndexBuilder::push` (`src/archive/index.rs:227-233`) index rejected names fails it.
- **`an_unsupported_action_is_refused_inside_a_mount`** (`src/app/harness_tests/archive.rs:256-271`) paired with **`the_gate_does_not_leak_outside_a_mount`** (`:274-289`) — bite: disabling the gate at `src/app/actions.rs:146` fails exactly the first; the paired negative stops it degenerating into "refuse everything".
- **`a_write_re_reads_the_archive_rather_than_trusting_the_old_index`** (`src/app/harness_tests/archive.rs:1935-1985`) — deletes the *first* stored member, shifting every later central-directory position, then reads **every** surviving member's bytes through the post-write index and compares content. Its own comment explains why checking all of them is what stops it passing by luck.
- **`a_plan_never_loses_or_duplicates_a_member`** (`src/archive/journal.rs:741-783`) and **`a_normalized_name_can_never_escape_its_mount`** (`src/archive/index.rs:737-751`) — proptest over generated inputs, with the step count derived independently of `plan_repack`, and the security invariant asserted over arbitrary strings rather than a hand-picked list. This is exactly what AGENTS.md's "reach for `proptest` on wide input spaces" asks for.
- **The staging-refill pair** (`src/archive/write.rs:788-838`, `:842-889`) — one asserts the refill happens, the other that it does not clobber the pending edit, and *both* assert the unrelated delete still applied, so a "refill everything" implementation cannot pass either.
- **Full suite green on HEAD**, 0 ignored, and all 11 source-scan guards pass.
