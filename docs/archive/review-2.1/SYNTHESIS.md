# Pre-2.1 review — synthesis

**Subject:** `git diff v2.0.0..HEAD`, HEAD = `f8dedae`, version `2.1.0-CURRENT`.
**Reviewers:** A (archive), B (app interaction), C (MCP/state/security), D (clipboard/pane/agent), E (docs/comments), F (tests/guards).
**Referee:** orchestrator. Date: 2026-08-10.

---

## 1. Verdict

> **UPDATED 2026-08-14 (eighth pass) — THE BLOCKER HAD A THIRD CONSEQUENCE.**
> `main` at `77d6826`. An adversarial read of #406–#415 — the seventh pass's own
> output — found a **tag-blocking escape in the containment code that pass had
> just verified**. `link_target_contained` canonicalized the link's *parent* and
> folded `..` in the *target* on paper, so `d/link1 -> ..` (contained) composed
> with `L -> d/link1/../ESCAPED` into a staged link resolving outside the mount.
> A file beside the mount was read through it. Fixed #417; four guards green on
> less than they claimed, #418.
>
> **B1 and HIGH-3 were both closed correctly.** B1 named the escape's *write*
> consequence, HIGH-3 its *MCP read*; both fixes hold. The third consequence —
> the **TUI** readers, three of which follow a staged link, one of them
> persisting the bytes to disk — was never filed, and the primitive survived in
> the direction nobody probed: the link *target*, not the member *destination*.
> §7 records "a finding naming N sites is closed when N are fixed" twice at HIGH.
> This is that shape applied to *consequences*, where no guard can enumerate for
> you.
>
> **No advisory.** Archive browsing has never shipped — absent from v2.0.3, #301
> in no tag — so the whole surface lived only on the unreleased stream. v2.1.0 is
> its first release and carries the fix.
>
> **Severity tiers remaining: 0 at every tier.** 49 PRs, #346–#418.
>
> ---
>
> **UPDATED 2026-08-12 (seventh pass) — THE RECORD WAS WRONG TWICE.**
> `main` at `76f2091`. Verifying §7's closure claims against the *code* rather
> than the record found **two findings marked closed that were still live**, both
> `high`, both because the finding named several sites and the fix reached only
> some:
>
> - **HIGH-1** named three allocation sites; `write.rs`'s `read_at` kept
>   `Vec::with_capacity(declared)` for the whole campaign. Reachable — a plain
>   `.tar` is seekable, so mounting one never consults the extract budget, and a
>   lying header lands there on the first `:archive write`, where the allocation
>   *aborts* rather than unwinding. Fixed #412, plus a guard requiring every
>   `with_capacity` in `src/archive/` to go through `reserve_for`.
> - **HIGH-2** named three consequences of trusting a zip's declared size; the
>   MCP 100 KB read cap still checked the *claim*. Measured on HEAD: a member
>   declaring 1 byte returned **300,000**. Fixed #414.
>
> Worse than the misses: **HIGH-1's own regression test never bit.** It asserts
> bytes read, which `take(size)` bounds either way — it passes with `reserve_for`
> removed from both sites. And the abort cannot be provoked from a tar fixture at
> all (octal sizes cap at ~64 GB; overcommit absorbs it), so no end-to-end test
> can pin that property. It had been giving false assurance since the first pass.
>
> The lesson is now in AGENTS.md (#415): *a test is evidence only once you've
> watched it fail; a finding naming N sites is closed when N are fixed; when the
> fix is a helper, pin the call site too.* That last clause is not hypothetical —
> the HIGH-2 fix itself briefly shipped a bounded function nothing called, caught
> only because its wiring had a separate test.
>
> **Severity tiers remaining: 0 at every tier, verified against code.**
> 47 PRs, #346–#415.
>
> ---
>
> **UPDATED 2026-08-12 (sixth pass) — THE INFORMATIONAL TAIL IS CLOSED TOO.**
> `main` at `20ad72a`. Six further PRs (#406–#411) close the eleven
> informational items, plus the one that had stopped being informational:
> **F18 was filed at 1448 of a 1500-line ceiling and was 1495 by the time it was
> worked** — five lines, i.e. the next feature meets a bump-or-extract decision
> under deadline. Fixed by extraction, twice: the pure predicates and loop-scoped
> types out (#406), then `Runtime` and `ViewState` into their own modules with
> their field docs (#407). **`app/mod.rs`: 1495 → 425**, ceiling 1500 → 600.
>
> That second step reversed a stated convention (AGENTS.md said mod.rs holds "the
> three struct defs"), so AGENTS.md changed with it — the docs-in-the-same-commit
> rule applied to a rule.
>
> **A third spliced doc block** turned up on the way: `Runtime`'s doc had been cut
> in half by `PendingScopeWait` inserted into the middle of it. With F11 (#395)
> and the two in #400, that is **three independent instances of one shape** in
> this window — enough that it is a pattern, not a coincidence.
>
> **Severity tiers remaining: 0 blockers, 0 highs, 0 mediums, 0 lows, 0
> informational.** 44 PRs, #346–#411.
>
> ---
>
> **UPDATED 2026-08-12 (fifth pass) — EVERY FINDING IS CLOSED.** `main` at
> `b11edca`. Eleven further PRs (#392–#401, #403) close the lows. Five of the 23
> filed were already closed as collateral by earlier passes (F12 by #383, F-E13
> by #357, and C-F5/C-F6/C-F8 by the H3 and SECURITY.md work) — verified on HEAD
> before any were worked, per the standing rule.
>
> **Three of the lows were live user-visible defects**, not the prose or coverage
> the tier suggests: a `0o000` member staged unreadable so the pager, `y` and
> copy-out all failed EACCES on a member the container held fine (A-LOW-2); every
> file put into an archive listing as size 0 / mtime epoch (A-LOW-3); and a vsplit
> whose column was inside a container losing **the entire split**, silently, on
> `spyc -r` (D7). A fourth had teeth of its own: a torn skill manifest read back
> as `NotInstalled`, which is overwritten unconditionally — so the file whose job
> was to protect the user's edits was the one that could discard them (C-F7).
>
> **One low was wrong on contact.** A-LOW-1 proposed demoting a mount containing
> an escaping symlink to read-only. Measured first: with `escaping_links = 1` the
> repack succeeds and the link returns byte-identical, rebuilt from the index that
> never lost it. `assess`'s rule is about *reproducing*, not extracting, so the
> exemption was correct — but undefended, and #392 had just shown this same file
> reads `symlink_metadata` for a staged member's mode. #397 pins the round trip
> instead of changing the behaviour.
>
> **Severity tiers remaining: 0 blockers, 0 highs, 0 mediums, 0 lows.**
> Full record in §7.
>
> ---
>
> **UPDATED 2026-08-12 (fourth pass) — EVERY MEDIUM IS CLOSED.** `main` at
> `07b69db`. Eleven further PRs (#378–#386, #388, #389) close the remaining
> sixteen: M9, M10, M21, M22 (the last of the live defects — a half-inverted
> wheel, a 30 ms loop stall from a wheel tick, an unbounded clipboard write, and
> a 150 ms hitch on every Linux yank), then M13, M3, M1, M2, M4, M11, M14, M15,
> M16, M20, M23, M24.
>
> **Severity tiers remaining: 0 blockers, 0 highs, 0 mediums, 23 lows.**
> Full record in §7.
>
> ---
>
> **UPDATED 2026-08-11 (third pass) — the first four mediums.** `main` at
> `6d5a0a2`:
>
> | PR | closes | what |
> |---|---|---|
> | #374 | MED-12 | the 2.1 release notes describe 2.1 — they were 75 commits and one whole campaign behind |
> | #375 | MED-7 | a chrome row's hit area has a right edge, so column `b` keeps its clicks |
> | #376 | MED-5 | a streamed member's bytes go where its own entry says |
> | #377 | MED-6 | a member can't squat on the staging escape namespace |
>
> **Two of the four were live defects.** In the default full-height vsplit a
> left-click on column `b`'s first row anchored a status-bar selection and a drag
> from there silently replaced the clipboard; and on macOS a case-colliding pair
> in a compressed tar served each other's bytes, with one of the two unreadable
> *and* unwritable. See §7.
>
> **Severity tiers remaining at the end of this pass: 16 mediums, 23 lows.**
>
> ---
>
> **UPDATED 2026-08-11 (second pass) — the blocker and ALL NINE highs are fixed
> and merged.** `main` at `9067366`. The four highs left open by the first pass
> closed in four further PRs:
>
> | PR | closes | what |
> |---|---|---|
> | #362 | HIGH-5 | the middle-click clipboard read runs off the loop, and is bounded |
> | #363 | HIGH-6 | a live codex tab saves the rollout it was pinned to |
> | #365 | HIGH-8 | the three archive write-back safety properties are pinned |
> | #368 | HIGH-9 | `handle_mouse`, `forward.rs`, `scroll.rs` covered |
>
> Verified on `main` after merge: `=== GATE: PASS ===`, **2,389 tests, 0 ignored**.
> Every mutation the F-report used as a bite check now fails. Two of these were
> live defects, not just coverage: a wedged clipboard helper froze spyc for as
> long as it took (30.21s measured against a stub), and a codex tab that was
> still working at quit restored as `resume --last`, which attaches to whichever
> rollout in the cwd was written last — including another spyc's.
>
> **Severity tiers remaining at the end of this pass: 20 mediums, 23 lows.**
>
> ---
>
> **UPDATED 2026-08-11 (first pass) — the blocker and six highs.** Eight PRs, `main` at `5ce862d`:
>
> | PR | closes | what |
> |---|---|---|
> | #346 | — | container-parser fuzz target, landed **first**, red on the PoC |
> | #347 | BLOCKER-1 | containment decided physically, per path component |
> | #348 | HIGH-1/2/3 | declared sizes stop driving allocations, the bomb gate, and MCP scope |
> | #349 | HIGH-7 | `EnableMouseCapture` guard sees `setup_terminal` |
> | #350 | HIGH-4 | every yank through one delivery seam |
> | #353 | MED-25 | flash guard reads the whole call (one live violation fixed) |
> | #357 | MED-17 | module-index guard sees subdirectories |
> | #359 | MED-8 | purity guard sees draw-pass mutation |
>
> Verified on `main` after merge: `=== GATE: PASS ===`, 2,331 tests, 0 ignored;
> all 12 fuzz seeds green; 915k-execution exploratory fuzz run found nothing new.
> HIGH-5, HIGH-6, HIGH-8 and HIGH-9 remained open at this point; all four
> closed in the second pass above.
>
> **Is the containment model now correct by construction?** Close, and honestly
> stated: extraction refuses to *traverse* a symlink at all, which is a property
> of each step rather than of the final path, so no ordering of members can
> arrange an escape — that part is structural, not PoC-negative. What is not
> structural: the walk is TOCTOU-exposed to a local attacker who can write into
> spyc's staging directory between the check and the write. Closing that needs
> per-component `openat`/`O_NOFOLLOW`, which was declined deliberately (it needs
> directory descriptors threaded through every write site plus a new dependency,
> and the race already requires local write access to spyc's private directory).
> Against the archive-controlled hazard — the one this review found — it is
> correct by construction. Against a local attacker racing it, it is not, and
> that is a known, bounded gap rather than an unexamined one.

**Original verdict (pre-fix): No — not releasable as 2.1 today. Yes after fixing one blocker.**

> **BLOCKER-1 — `src/archive/read/mod.rs:414` (`link_stays_inside`), consumed at `:252` and `:304`.**
> A chain of symlink members, each individually passing the containment check,
> composes into one that escapes the staging root; a later file member written
> *through* it lands anywhere on the filesystem. Triggered automatically by
> pressing `Enter` on a `.tar.gz` / `.tar.zst` — no prompt, no warning, and the
> mount is reported as `Capability::ReadWrite` with `escaping_links = 0` and
> `warnings = []`. Introduced by `3c61ac7` (#301), 2026-08-08.

That is the entire blocker list. It is one item because one item earns it.

**Not an emergency for shipped users.** `src/archive/` exists in no released tag
— 0 files at `v2.0.0`, 0 at `v2.0.3`, 10 at HEAD. No 2.0.x binary is affected.
This is a "don't cut the tag" problem, not a "recall the release" problem.

**Two viable paths**, owner's call:

1. **Fix it.** The defect is that containment is decided *lexically* while the
   write happens *physically*. The fix has to resolve the real path — verify the
   canonicalized parent of `dest` is still under `staging_root` immediately
   before each create, or refuse to traverse an existing symlink when writing a
   member. Small change, one file, and it also closes HIGH-4's escalation path.
2. **Don't ship archive browsing in 2.1.** Everything else in the delta is
   releasable. If the archive subsystem is gated off, the blocker and three of
   the six highs leave with it.

Recommendation: fix it, and fix HIGH-1/2/3 in the same batch — they are the same
file, the same untrusted-input surface, and the same afternoon's work. Shipping
"spyc opens archives from anywhere" with those four open makes a claim the code
does not support.

### Verification of the blocker (referee, independent of Reviewer A)

Reviewer A executed a proof-of-concept: a 424-byte `.tar.gz` that wrote
`/tmp/spyc-A-PWNED.txt` through `stream_mount`. I did not take that on trust. I
traced `link_stays_inside` myself and reproduced the mechanism against the real
filesystem, using the exact lexical depths the function computes:

| step | member path | `link_dir` depth | target | function says | reality |
|---|---|---|---|---|---|
| 1 | `d/link1` | 1 | `..` | depth 0 → **inside** | `staging/d/link1` → `staging/` ✓ |
| 2 | `d/link1/link2` | 2 | `..` | depth 1 → **inside** | resolves through link1, lands at `staging/link2` → **`staging/../`** |
| 3 | `d/link1/link2/ESCAPED.txt` | — | — | plain file member | written **outside** `staging/` |

Confirmed on disk: the file landed one directory above the staging root. Both
links pass the check; neither depth ever goes negative. The function assumes the
link sits at its lexical path, but step 1 has already redirected where step 2
physically lands. `AGENTS.md`'s claim that "zip-slip is structurally impossible"
is true of *names* — `normalize` is sound and Reviewer A failed to beat it — and
false of *links*.

---

## 2. Cross-validation: where reviewers overlapped or disagreed

Five overlaps. Three were genuine duplicates, two were real disagreements I ruled.

| # | Site | Reviewers | Ruling |
|---|---|---|---|
| 1 | `clipboard.rs:176` ← `effect.rs:722` — middle-click paste blocks the loop | D: `high`, B: `low` | **`high`.** I read the code: `Command::output()`, no timeout, on the loop thread. B's own framing decides it — mouse capture now defaults **on**, so an accidental middle-click on a wedged X selection owner freezes spyc with no escape key. B rated the defect low while describing why it is not. |
| 2 | `mcp/protocol.rs:634-643` — archive read skips containment | A: `high`, C: `low` | **`high`, with a stated dependency.** C is imprecise: `effective_root` *is* called first (`:622`), so a bogus `root` argument is rejected. What the archive branch skips is the *containment* check at `:646-657`, and the cwd fallback at `:635-638` re-resolves against the user's cwd even when `root` was passed. Alone that is bounded by "the user mounted this archive" — C's `low` is defensible in isolation. It is `high` because it composes with BLOCKER-1: a staging symlink pointing at `/etc/passwd` turns a bounded member read into arbitrary file read. Fixing BLOCKER-1 downgrades this to `medium`. |
| 3 | `KEYBINDINGS.md` has zero mouse content | B: `low`, E: `medium` | **`medium`.** The file bills itself as the complete keymap and capture ships on by default; a user who wants it off cannot find `:mouse off` there. |
| 4 | `tab_hit.rs` undocumented + index guard skips subdirectories | B: `low`, E: `low`, F: `medium` | **Split.** The missing `AGENTS.md` entry is `low`. The guard's structural blindness to every subdirectory under `src/app/` is `medium` — it is a guard that fails open, and it is why three reviewers found the same hole independently. |
| 5 | git-env sanitization checks 1 of 9 variables | C: `medium`, F: `low` | **`medium`.** This is the exact failure family as the 2026-08-07 split-brain-reset incident, and the guard is what is supposed to prevent its return. C also verified the two nine-variable lists do *not* diverge, which is the good news underneath. |

**Mutually reinforcing, not duplicate:** Reviewer A found the archive's safety
properties are wrong; Reviewer F independently found they are *untested* —
gutting `verify` to `Ok(())`, inverting verify-and-persist order, or hardcoding
`assess()` to `ReadWrite` each leaves all 2,303 tests green. Neither reviewer saw
the other's work. Together they explain how BLOCKER-1 shipped: the test suite
could not have caught it.

---

## 3. Findings, severity-ordered

Provenance in the last column. Severities are mine where §2 ruled; otherwise the
reviewer's.

### Blocker

| # | Site | Finding | Src |
|---|---|---|---|
| B1 | `archive/read/mod.rs:414` | Symlink-chain escape from the staging root → arbitrary file write on `Enter`. PoC executed by A; mechanism independently reproduced by referee. | A |

### High

| # | Site | Finding | Src |
|---|---|---|---|
| H1 | `archive/read/mod.rs:359`, `:351`, `write.rs:318` | `Vec::with_capacity(declared_size)` from an attacker-controlled header. A `.tar` declaring `1<<62` **aborts** the process (exit 6) — an abort, not an unwind, so the panic hook never restores the terminal. | A |
| H2 | `archive/read/mod.rs:347`, `budget.rs:110`, `mcp/readers.rs:244` | A zip's *declared* uncompressed size is trusted; it defeats the decompression-bomb gate, the MCP 100 KB read cap, and the read allocation. Patched to 1, `member_bytes` returned 1,000,000 bytes and `decide_mount` said `Proceed`. | A |
| H3 | `mcp/protocol.rs:634-643` | Archive branch returns before the containment check; cwd fallback ignores an explicitly passed `root`. F1 root validation does not hold through this path. | A, C |
| H4 | `effect.rs:610`, `quick_select.rs:145`, `pager_handler/image.rs:87` | `yp`/`ya`/`^a u`/image-`Y` call `clipboard::copy` directly, bypassing `deliver_clipboard`. Over SSH they write the **server's** clipboard and flash "yanked" — the exact outcome `delivery_tests:477` calls "worse than an error". `yf`/`yP`/mouse-drag in the same chord do it correctly. Verified by referee. | D |
| H5 | `clipboard.rs:176` ← `effect.rs:722` | `Effect::PasteFromClipboard` runs `Command::output()` inline on the loop thread, no deadline, no size bound. Reachable by accidental middle-click now that capture defaults on. | D, B |
| H6 | `session.rs:122-133`, `pane/tabs.rs:256` | A live codex tab saves `agent_session_id: None` and restores as `codex resume --last`, though `info.codex_session_id` holds the exact uuid — re-opens the #230 wrong-conversation class from the save side. | D |
| H7 | `lib.rs:1217` | The `EnableMouseCapture` guard `retain`s away every file named `lib.rs` — the only file holding `setup_terminal`/`restore_terminal`/`resume_tui`. **Bite-check executed:** inject the banned call into `setup_terminal` and the guard passes. The `?1003h`-storm trap is unguarded at the only site that could reintroduce it. Verified by referee. | F |
| H8 | `archive/write.rs:340`, `:106` vs `:120`, `archive_ops.rs:391` | Three archive write-back safety properties — repack `verify`, verify-before-persist ordering, and `assess()`'s capability demotion — can each be gutted with all 2,303 tests green. | F |
| H9 | `mouse/mod.rs:166`, `mouse/forward.rs`, `mouse/scroll.rs` | `handle_mouse` has zero tests; `forward.rs` and `scroll.rs` have zero tests. The documented press/release pairing obligation is entirely unasserted, and the regression test for the shipped `q`-into-composer bug re-implements the caller, so reverting the fix stays green. | F |

### Medium

| # | Site | Finding | Src |
|---|---|---|---|
| M1 | `SECURITY.md:50-54` + `mcp/readers.rs:78-88` | "An agent cannot point a read tool at an arbitrary directory" is false: `allowed_roots` admits the context file's `cwd`, and `navigate_to` (unvalidated, `state/navigation.rs:242`) sets it, then `write_context()` synchronously. Two auto-approved calls reach any directory. Verified by referee. | C |
| M2 | `SECURITY.md:70-73` | "`cargo deny check` runs on every CI build" — untrue since `ba874c2` (#273) moved it to weekly `audit.yml`. The PR-time gap `audit.yml` itself admits is absent from the caveats. | C |
| M3 | `git/mod.rs:123` | Git-env-hygiene guard greps for `env_remove("GIT_DIR")` alone while `GIT_REDIRECT_ENV` names nine; ten sites strip only three and all pass. Same failure family as the 2026-08-07 incident. | C, F |
| M4 | `Makefile:429-433` | No staleness story for the installed pre-commit hook — no stamp, no comparison, no test, nothing in CONTRIBUTING. The main checkout's installed hook already differs from the template. | C |
| M5 | `archive/read/mod.rs:168`, `:227` | Staging *writers* use `clean.inner` while every *reader* uses `staging_rel()`: on macOS a case-colliding member serves the wrong bytes and its twin is unreadable *and* unwritable. The comment at `:227` claims the opposite of what the code does. | A |
| M6 | `archive/index.rs:69-75` | A member literally named `.spyc-case-1/…` collides with a case-ranked member's staging path; reading one returns the other. Demonstrated. | A |
| M7 | `mouse/selection.rs:122-126` | `chrome_col_at` bounds `y` and a lower `x` but no width, so in a full-height vsplit a left-click on column `b`'s first row anchors a status-bar selection instead of focusing; a drag then silently overwrites the clipboard. `pager_slot_at` — the function `route.rs` cites as the model — bounds both axes. | B |
| M8 | `render/inner.rs:434-457` | `draw_chrome_line` is `&self` but writes `view.chrome_rows` through a `RefCell` every frame — the only interior-mutability write in the three `PURE_DRAW` modules, new since v2.0.0. The purity guard's `FORBIDDEN` list is OS tokens only, so it structurally cannot see mutation. | B |
| M9 | `mouse/forward.rs:158-160` | `invert_scroll` never reaches a mouse-aware child, so claude/vim scroll un-inverted while the list and pagers invert. `CONFIGURATION.md:253` says it flips the direction "everywhere". | B |
| M10 | `mouse/scroll.rs:129-132` → `pane_scroll.rs:273-277` | With `pane_scroll_view = "spyc_history"` a wheel tick reaches `3 × sleep(10ms)` on the main loop, re-paid per tick if scrollback is empty. `native_scroll_plan.md:504-508` rules this out by name; the plan was never amended. | B |
| M11 | `docs/drafts/native_scroll_plan.md:398-459,544` | Mouse-button DSL bindings — a titled subsection, an RCE warning marked "must be tested", half of PR 3's scope — shipped nowhere; the plan still reads as the spec. | B |
| M12 | `docs/drafts/2.1-release-notes.md:8,14` | Pinned to `2eef3a7`, 47 commits behind HEAD. "Two big things" omits the entire archive campaign (#301–#343) plus images, `^`/`$` anchors, `^z`, and a security fix. | E |
| M13 | `FEATURES.md:257-261` | States agy's hooks report working+done only; `mcp/hooks.rs:493` reports `blocked` via the `ask_question` hook. FEATURES now contradicts AGENT_ORCHESTRATION.md. | E |
| M14 | `FEATURES.md:1204-1214,1253` | Two-host skill documentation for a three-host feature (`skill/mod.rs:191` iterates Claude/Codex/Agy). | E |
| M15 | `docs/KEYBINDINGS.md` | Zero mouse/wheel/drag/click content, including `:mouse off`, in "the complete keymap" — while capture defaults on. | E, B |
| M16 | `FEATURES.md:678,693` | Tab-click-to-switch (#279) and prompt-row/HUD selection (#281) undocumented, inside enumerations that read as complete. These are also the only two `feat` commits that shipped no doc at all. | E |
| M17 | `mouse/tab_hit.rs` + `mod_tests.rs:131` | Added post-2.0 (#279), in no markdown doc; `every_app_module_is_in_the_agents_index` skips all subdirectories, so `AGENTS.md`'s per-file `mouse/` enumeration silently lost it. | F, B, E |
| M18 | `.github/workflows/fuzz.yml:62`, `Makefile:140` | `archive_name` is absent from the weekly matrix. It builds and runs clean (300k execs) but has never run in CI. | F |
| M19 | `fuzz/fuzz_targets/` | No fuzz target reaches any container parser — `archive_name` targets `index::normalize`, already covered by two proptests. Zero coverage of `detect`/`index_zip`/`index_tar`/`stream_mount`/`repack`. H1 is what a structural target finds first. | A, F |
| M20 | `archive/read/tests.rs:468` | `an_impossible_date_does_not_panic` tests the `zip` crate's constructor; `zip_mtime` is never called. | F |
| M21 | `clipboard.rs:474-477` | `stdin.write_all` runs *before* the deadline loop, so `HELPER_REAP_BUDGET` bounds the wait but not the write; a `[clipboard].command` that doesn't drain stdin blocks the loop forever. The adjacent exiting-helper case *is* tested, which is why the hole hides. | D |
| M22 | `clipboard.rs:452` | Every yank under xclip/xsel burns the full 150 ms budget on the loop. Self-documented, and the comment names the correct fix. | D |
| M23 | `CONFIGURATION.md:119`, `clipboard.rs:270`, `:281` | Three places promise an oversize-OSC-52 fallback to the local helper. `deliver_clipboard` has none, and under `auto`+SSH — the case OSC 52 exists for — `local` is false, so nothing is copied. | D |
| M24 | `render/mod.rs:450-469` | `prepare_frame`'s doc comment orphaned onto `frame_layout` by `6515ac3` (#219); `frame_layout` settles nothing and `prepare_frame:501` is undocumented. | E |
| M25 | `mod_tests.rs:70` | The flashed-error guard requires `flash_error(format!(` and `{e}` on one line; the live escaping site `effect.rs:896` splits them, so the guard passes on HEAD while the invariant is violated. | F |

### Low / informational

23 further items are itemized in the six reports and not reproduced here. The
recurring shapes: two mis-merged doc blocks carried verbatim through the mouse
split (`mouse/route.rs:202-227`, `forward.rs:88-101`); three comments in
`mouse/mod.rs` asserting things that are false on HEAD (spyc *does* emit 1002,
drags *are* forwarded); `escaping_links` warns but never demotes `Capability`;
`apply_mode` makes `0o444` writable and `0o000` unreadable while correctly
dropping setuid; two `Mutex::lock().unwrap()` without the invariant comment
`AGENTS.md` requires; a vsplit whose cwd is inside an archive is silently dropped
on restore; `CHANGELOG.md:3`'s header names the superseded one-step release.

---

## 4. Premises corrected

Six charter premises did not survive contact with the tree, and one reviewer
retracted a finding of its own. Recording them because the charter asked, and
because two of them change what the owner should conclude.

1. **Delta size.** 162 commits / 233 files / 39,627 insertions, not 145 / 231 /
   ~37k. Fifteen commits landed on 2026-08-10 after the prompt was measured.
2. **"Six fuzz targets."** Seven exist. The weekly workflow lists six — the
   missing one is `archive_name` (M18).
3. **"No archive fuzz target."** Half wrong. One exists; it targets name
   normalization, which two proptests already cover. The real gap is that
   nothing fuzzes the byte-eating parsers (M19).
4. **"`mouse.natural_scroll`."** No such key. It is `[mouse] invert_scroll`;
   `natural_scroll` was rejected **by name**, with the rationale recorded at
   `config/mod.rs:349-360` (which direction is "natural" depends on the OS
   trackpad *and* the terminal). The underlying question still had an answer:
   applied at exactly one point, guarded by a test — but excluding the forwarded
   pane (M9).
5. **"Audit that late changes did not erode F1 root validation."** Root
   validation did not exist at v2.0.0 — `effective_root` there only checked
   `is_dir()`. It is an *addition* inside this diff, not pre-existing hardening
   that survived.
6. **"`state_writes_are_atomic` coverage list."** There is no list; the guard
   recursively scans `src/state/` with an empty `ALLOWED` and passes. Every store
   there is atomic. The gap is *outside* `src/state/` (`skill/mod.rs:274,280`).
7. **"ABOUT.md bundled copy vs docs/ABOUT.md in sync."** There is only one file
   (`include_str!`), so drift is structurally impossible. The sentinel test
   exists but guards content integrity, not sync.
8. **Two open dogfood notes resolved:** `create_worktree` inside a mount is
   **fixed** on HEAD by `d979964` (#341) — close the note. Put rows listing
   `size=0/File` is **confirmed live** (`record_staged` never runs on a copy-in).

**A retracted finding.** Reviewer E filed "22 of 32 `feat` commits shipped with
zero doc updates" as a process failure. I could not reproduce it; my count was 2.
Challenged with the evidence, E found its own bug — it grepped `git show --stat`
output, whose column padding and trailing `| 12 ++--` histogram made both its
patterns miss real `.md` hits — and withdrew the finding. E had also run a
*correct* scan earlier in the same session and trusted the later contradicting
run without reconciling them.

The corrected number matters, because it inverts the answer to the owner's
question: the docs contract is honored in **29 of 32** `feat` commits, and the
three exceptions are exactly the individual doc defects already filed as M13 and
M16. There is no process problem behind them.

---

## 5. Standards trajectory

**The craft held; the verification did not — and it fell behind precisely where
the input is hostile.**

Read chronologically, the delta is severely back-loaded: 131 of 162 commits
landed in the six days from 2026-08-05, and the archive subsystem — 4.9k lines
that parse untrusted input — is **two days old**. Across that acceleration the
things the owner worried about held up well. Comments are the strongest part of
this diff: 7,467 added comment-bearing lines scanned against twelve banned-shape
patterns produced **11 hits and 0 true positives**, and of 70 hand-read hunks
stratified across four eras, **68 met the house standard** — the two misses being
stale placement, not slop. The diff even *deletes* a pre-existing slop comment.
All eight SPYC-TRAP anchors resolve. The docs contract is honored in 29 of 32
feature commits. MVU discipline survived four post-split feature PRs with no
cross-module reach-ins, `mod.rs`'s guarded ceiling was **not** bumped (1,500 at
both ends, sitting at 1,418), and the 0-dps-at-idle invariant survived the 1002
upgrade correctly. Issue #31's premise — that comments have gone verbose and
sloppy — is not supported by this delta.

What eroded is what *checks* the code. Two guards were proven to fail open by
executed bite-check (H7's `lib.rs` exemption; M25's one-line-form requirement),
one guard structurally cannot see the shape the new code took (M8's `RefCell`
write in a `PURE_DRAW` module), and one cannot see subdirectories at all (M17).
Reviewer F's summary is the sharpest description of the pattern: each safety
rail's *pure decision* is thoroughly tested while its *wiring and failure path*
are not, so the rail reads as covered when only its decision is. That is exactly
how BLOCKER-1 shipped — `link_stays_inside` has tests, and they cover the
single-hop shape it gets right. Three archive write-back safety properties can be
gutted with 2,303 tests green. The three newest mouse modules have zero tests.
Meanwhile file size drifted from 12 to 19 files over 800 production lines, the
worst new one being the two-day-old `app/archive.rs` at 1,443.

So: **not slop, and not carelessness in what was written — a widening gap between
what the code claims and what anything verifies.** The gate is green (2,303
tests, 0 ignored, clean fmt and clippy) and it was never going to catch any of
this. The single most valuable change after fixing the blocker is a fuzz target
on the container parsers (M19); it is what would have found H1 first, and it is
the pattern the repo already established six targets ago.

---

## 6. Reports

| File | Reviewer | Area |
|---|---|---|
| `A-archive.md` | A | `src/archive`, security-first |
| `B-app-interaction.md` | B | `src/app` mouse / selection / scroll |
| `C-mcp-state-security.md` | C | `src/mcp`, `src/state`, SECURITY.md |
| `D-clipboard-pane-agent.md` | D | clipboard, pane, agent plumbing |
| `E-docs-comments.md` | E | docs contract + comment standards |
| `F-tests-guards.md` | F | test honesty + guard integrity |

Gate status on HEAD at review time: `=== GATE: PASS ===`, 2,303 unit + 15
integration tests, 0 failed, 0 ignored.

---

## 7. Remediation record (2026-08-11)

Eight PRs, one finding per PR except HIGH-1/2/3 (one file, one mistake in three
places). Every fix carries red-before/green-after evidence; the fuzz target
landed first, deliberately, so it was written against the broken parsers rather
than alongside the fixes.

### What the fuzz-first ordering actually bought

It found both known crash shapes on its first run, and it caught a fix that
wasn't one. Mid-AB1 the target reported **every seed passing** — including the
OOM that was still unfixed. Cause: `contained_dest` canonicalized a staging root
that doesn't exist yet for seekable containers (the old code created it as a side
effect of `create_parent`), so `materialize` errored out before doing anything
and the whole zip/tar path silently did nothing. A green board produced by the
code under test not running. Nothing in the unit suite would have shown that.

### Near-misses worth keeping

- **Canonicalization leaking into a returned path.** AB1's first version returned
  the canonical destination; on macOS `/var` → `/private/var`, which broke nine
  app-layer tests that key staged members by the caller's path shape. Containment
  is decided canonically; the path is returned in the caller's namespace.
- **An assertion wrong rather than the code.** Two of AB1's new tests asserted a
  refused link's *name* wouldn't exist. It does — as a real directory, created for
  the member that named it as a parent. That is containment working, and the tests
  now assert the path's type.
- **A bite-check that didn't run.** MED-8's second injection referenced a field
  that doesn't exist, so the build failed and the test never executed — read as a
  pass until checked. Same false-green family as the one above.
- **An existing guard caught the fixer.** HIGH-4's new flashes were written with
  `{e}`; `flashed_errors_render_their_whole_chain` failed them. Worth recording
  because MED-25 is that same guard being *too narrow* elsewhere — narrow in one
  dimension, working in another.

### Standards trajectory, revisited

The pre-fix reading was "the craft held; the verification did not." That is now
materially better where it was worst. The archive parsers have a fuzz target in
the weekly matrix, seeded with the shapes that broke them. Four guards that
reported green while their invariant was violated now fail on an injected
violation, each with that injection kept as a test. Two of those (MED-25, MED-8)
found a live violation the moment they were widened — which is the argument for
fixing a guard rather than only the thing it missed.

What has not changed: the newest code is still the least covered. HIGH-9 (zero
tests on `mouse/mod.rs`, `forward.rs`, `scroll.rs`) and HIGH-8 (three archive
write-back safety properties gutable with the suite green) are open at the end of
the first pass, and they are the same shape as the blocker — a safety property
whose *pure decision* is tested while its wiring is not. Both closed in the
second pass; see below.

### The four highs the first pass left, and what closing them showed

| # | PR | What it turned out to be |
|---|---|---|
| H5 | #362 | A **live defect**, worse than filed. A clipboard read that never answers held the loop for as long as it took — 30.21s measured against a `sleep 30` stub, then returning `Ok("")`: a silent empty paste after a half-minute freeze. Fixed in two halves, because the hang had two: the read moved to a worker (`graveyard_ops` template), and `capture` gained a deadline that kills the helper, so off-threading doesn't just relocate the hang onto a growing pile of stuck threads. Not fixed: the paste size cap the finding also mentions. Once off-thread, what reaches the loop is the same `String` a terminal bracketed paste already delivers, and capping middle-click alone would be an inconsistency rather than a fix. |
| H6 | #363 | A **live defect**, and the interesting part is *why* it survived: `TabInfo::pinned_session_id()` already existed for exactly this, and three consumers used it — the transcript view, the image gallery, the status suffix. Save was the one that didn't. The judgement call is recorded in the PR: codex's pinned id is accepted without re-validation, because `codex_pin` read it out of a rollout file's name — the claim already *was* the observation. Claude still validates, because its id arrives from a hook payload that can name a conversation already deleted. |
| H8 | #365 | Coverage only; the production code was right. All four F-report bites re-confirmed on `089a830` first (2,365 tests green under each). The test that closes it needed a repack that *only* verify can refuse — a step whose stored name the index reads back differently (`./a.txt` → `a.txt`) — because every earlier guard lets it through. Also corrected a docstring that claimed verify coverage its test didn't have. |
| H9 | #368 | Coverage, plus a demonstration of the finding's real point. The existing route-level `one_flick_past_the_bottom_sends_the_close_key_once` re-implements its caller, so reverting the shipped fix leaves it **green**; the new caller-level test reports "the close key went out 3× on one flick". Both were run in the same pass to show it. Panes were made mouse-aware the way a real child does it — the DEC mode written to `cat`'s stdin and echoed back through the pty — rather than by poking the parser, so what the test exercises is the actual path. |

**Both remaining verification-shaped highs (H8, H9) are now closed, which
retires the "newest code is the least covered" observation above** for the two
subsystems it named. The general point stands as a habit worth keeping: in every
one of these four, the *pure decision* was tested and its wiring was not.

### Third pass — the first four mediums (2026-08-11)

| # | PR | What closing it showed |
|---|---|---|
| M12 | #374 | Worse than filed: 75 commits behind, not 47. The document's "two big things" predated the entire archive campaign, so the release's largest feature was missing from its own release notes. Rewritten to three, with new *Archives*, *Images*, *Mouse* and *Startup* sections and the review itself under *Under the hood*. Four claims in my own first draft were wrong on checking and corrected before commit — the four verbs in #350 (the pager's `y` was never one of them), `^z`'s widening being a fix and not an addition, the archive PR count, and the shape of the temp-dir fix. |
| M7 | #375 | A **live defect**. `chrome_col_at` bounded `y` and a lower `x` and nothing else, and `route_mouse` tests `over_chrome_row` *ahead of* the region — so in the default full-height vsplit, where the status row is width-clamped to column `a` while `b` spans the frame, a press on `b`'s first row anchored a status-bar selection and a drag from there replaced the clipboard. Bite-checked twice: once on the hit-test, then again with that assertion removed so the consequence had to report for itself. |
| M5 | #376 | A **live defect**, and a structural one: the rank a member stages under was assigned in `finish`, over the sorted table, but a compressed tar indexes and extracts in **one pass** — the writer could not have asked, whatever it wanted to do. Ranking moved to `push`, and `push` now returns the *path* rather than the rank, so the writer is handed the answer instead of re-deriving it. On macOS `a/README` had been reading back `a/readme`'s bytes, and `a/readme` was both unreadable and unwritable. |
| M6 | #377 | Same file, opposite direction: a member *named* into the escape namespace resolved to a ranked member's staging path, and `materialize` returns early on an existing destination. Fixed by making the namespace un-namable — one reserved first component (`.spyc`), refused in `normalize` where `..` and NUL already are. The reverse map (`mount_path_for_staged`) had to move with it, and its new round-trip test is what caught that; two older tests that hardcoded the prefix now ask the entry instead, which is what let the spelling drift in the first place. |

Two of the four were live defects rather than coverage gaps — the same ratio as
the second pass. The recurring shape is different this time: **two derivations of
one fact**, kept in agreement by nothing. `chrome_col_at` vs. what the renderer
drew; the staging writer vs. every staging reader; the escape prefix vs. the
reverse map that strips it. Each fix collapses the pair into one definition and
pins the round trip with a test.

### Fourth pass — every remaining medium (2026-08-11/12)

Eleven PRs, closing the other sixteen. Four more live defects, then the debt.

| # | PR | What closing it showed |
|---|---|---|
| M9 | #378 | A **live defect**. `invert_scroll` was applied inside `gesture_and_delta`, which the forwarding path never calls — so the list, the pagers and agy's synthesized keys inverted while claude/vim/htop did not. The knob's whole audience is the user whose wheel reads backwards, and what they got was half an inverted spyc. Fixed by correcting the *event* at `handle_mouse`'s boundary rather than adding a second config-reading site. The bite-check is the finding's own point: with the per-consumer flip restored, **106 of 107 mouse tests still pass**. |
| M10 | #379 | A **live defect**: `3 × sleep(10ms)` on the loop, reachable from a wheel tick, which `native_scroll_plan.md` had ruled out by name. The settle became a `Deadline` — same real 30 ms, spent on a loop that keeps running, and the loop's own per-iteration drain is a better flush than the sleep's thread yields ever were. Measured red-before at 35.48 ms; the test times the call rather than looking for a `sleep`, which would pass the moment the sleep moved one function along. Also revealed a case the finding didn't name: 30 ms is enough to switch tabs, so the pending snapshot carries the tab that asked and a switch abandons it. |
| M21 | #380 | **Live**, and the same family as H5 one direction over: `write_all` ran before the deadline loop, so a helper that never reads stdin *and stays alive* blocked the loop past any budget. It hid because the adjacent case — such a helper that **exits** — is tested, and errors promptly via EPIPE. Bite-check: 28 of 29 clipboard tests still pass with the defect restored. |
| M22 | #381 | The sibling: `xclip`/`xsel` never exit, so every yank spent the full budget on the loop (**156.77 ms** measured). Write moved off-thread on the `graveyard_ops` template; OSC 52 stays inline, being a stdout write rather than a spawn. The flash now precedes the outcome and a failure replaces it — the alternative traded a hitch for a silent wrong answer, which is what #350 closed. Both test seams had to go process-global: a thread-local cannot reach the worker, which is the property under test. |
| M13 | #382 | The one doc that stated the *opposite* of HEAD. FEATURES said agy's hooks cover `working` + `done` only; `hooks.rs:493` has reported `blocked` for `ask_question` since #287, which updated `AGENT_ORCHESTRATION.md` and not FEATURES. |
| M3 | #383 | The guard accepted a 1-of-9 strip and ten sites took it up. Fixed by requiring the *function that owns the list* rather than a spelling of one entry — naming the nine in the guard would have made a third copy of a list already kept in sync twice. **Nearly done wrong:** the first conversion attempt used a regex over Rust source and mangled two sites, one leaving a `git` spawn with no directory at all. Caught on the diff, redone by hand. |
| M1+M2 | #384 | Taken together: correcting one false claim in a security document while leaving another one paragraph away is not a defensible half-measure. M1 fixed in the doc, not the code — freezing the allowed set at launch would break the ordinary "navigate, then ask about it" case, and it grants no reach an agent with shell access lacks. A third stale "fails CI" turned up in the license bullet. |
| M4 | #385 | The drift was **live in this checkout** — the installed hook was ten lines behind, dated Aug 7. The check is a test rather than a line in the hook, because a staleness check inside the stale artifact can't detect its own absence. Two things the finding didn't name: `make install-hooks` didn't work from a worktree (`.git` is a *file* there), so the new failure message would have named a fix that fails where the reader is standing. |
| M14–M16, M23–M24 | #386 | The doc-contract cluster, one pass. `KEYBINDINGS.md` called itself "the complete keymap" with zero mouse content while capture defaults on; two shipped mouse features sat inside enumerations that read as complete; the skill docs listed two of three hosts. M23's promised OSC-52 fallback was asserted in **four** places and performed in none — the fourth turned up only because finding three was a reason to grep for the rest. |
| M11 | #389 | Amended rather than implemented: the plan's binding tokens were *dropped*, not deferred by accident, and the `Trust::Project` RCE warning marked "must be tested" read as a check someone had performed. Second amendment this plan has needed (#379 was the first) — both the same omission, written before the work and never revisited after. |
| M20 | #388 | The clearest "asserts the mock" in the diff — and the finding's proposed fix turned out to be unreachable. My first replacement **didn't bite**: `zip` validates on parse, so `zip_mtime` is never handed an invalid date at all, and reaching its guard needs `unsafe`. The test now pins the requirement (a corrupt date lists with no mtime, not an invented one) instead of the function, asserts its own premise, and bites on the regression that matters. |

**Four of the sixteen were live defects.** The fourth pass's recurring shape is
the second pass's, one layer out: **the adjacent case is tested and the one that
matters isn't** — a wedged helper that exits vs. one that stays alive; a delta
consumer vs. the forwarding path; a plan's prose vs. its "Files touched" table.
Where the third pass found two derivations of one fact, this one found one test
standing in for two behaviours.

**Two findings were wrong on contact and are recorded as such:** M20's suggested
fix isn't reachable without `unsafe`, and M12 was 75 commits stale rather than
47. Neither changes the finding; both change what closing it looks like.

### Fifth pass — every low (2026-08-12)

`main` `07b69db` → `b11edca`. Eleven PRs. Five findings were already closed as
collateral and verified so on HEAD before anything was worked.

| PR | closes | what closing it showed |
|---|---|---|
| #392 | A-LOW-2 | One mode field served two masters and got both wrong. A `0o000` member staged unreadable (EACCES from the pager, `y`, copy-out) and a `0o777` one staged world-writable, which `c` carried into the user's tree verbatim. **And in the other direction** — the repack read the mode back *off the staged copy*, so editing a `0o444` member published it as `0o644`. Split: staging carries spyc's permissions, the index carries the archive's. Bite: 257/258. |
| #393 | A-LOW-3 | Every put member listed as size 0, mtime epoch. `mount.staged` has exactly two fillers and a copy-in reaches neither — `current_staged_stats` walks the index (an addition isn't in it) and `record_staged` needs a materialize outcome (a copy produces none). Bite: 255/256, with `a_put_member_can_be_opened_yanked_and_deleted` green throughout — it exercises the same put and never looks at the row. |
| #394 | D7 | A `right_cwd` inside a container fails `is_dir()`, falls to the preview branch with no `preview_path`, and the blank-split guard drops the split. Asymmetric with the left column, which #317 handled *and tested*. Restored beside the container with a flash; full remount would need `Effect::ChangeDir` to name a column, which is its own change. |
| #395 | F10, F11 | `chrome.rs` sat in the per-column allowlist as "deliberately anchored to the primary column" — reversed by #322 **in this same window**, which removed the read and added a test pinning the opposite. So the one file just fixed for this bug was the one file free to reintroduce it. Bite both ways: HEAD's guard passes an injected regression, the fixed one names `render/chrome.rs`. F11 (a doc block spliced onto the wrong guard) is why the sentence naming `chrome.rs` was nowhere near the constant listing it. |
| #396 | A-LOW-4, A-LOW-5 | The cap bounded the walk, not the index: three members five deep produced **eighteen** entries against a cap of three, while the "capped at N" warning named 3. Bite: 259/260, with `the_entry_cap_truncates_and_stops_the_walk` green — it covers the other half of the same cap. |
| #397 | A-LOW-1 | The finding was wrong; the exemption was undefended. See §1. Bite: 259/260 with the symlink branch reading the target off disk. |
| #398 | F13, F14, F12e | Three assertions satisfied by the outcome their own names exclude. F13 never read `isError`, so a refusal and an empty answer were indistinguishable — bite both ways. F14's column half had no premise and `pane.x == 0` everywhere, so `report.col == 5` held whether the origin was subtracted or ignored: arithmetic, not subtlety. F12e's driver gave up after four hops silently. |
| #399 | F12d | The write-time free-space refusal had never once been true. Reached with a margin larger than any disk — no injection seam needed, and it pins the `saturating_add` too. Bite: the finding's own `&& false`, 261/262. |
| #400 | E comment audit | **Four** copies of "spyc asks the terminal only for 1000", one of them thirteen lines below the string that emits `?1002h`. `proc.rs` was the single copy updated when 1002 landed. Plus two doc blocks attached to the wrong function, the same splice as F11. |
| #401 | C-F7 | `read_manifest` → `None` → `NotInstalled` → overwritten unconditionally, so a torn manifest converted "never clobbered unprompted" into "clobbered". The guard scanned `src/state/` only and could see neither this nor the MCP sidecar; widened, and its one-literal limit written down rather than left to read as coverage. |
| #403 | C-F9, D8, D9, F-E8, F-E9, F-E10, F-E11, F-E12 | The doc-contract remainder. F-E12 resolved by strengthening where possible and narrowing where not: `"for now"` joins the guard (catching three sites that had been "for now" for over a year), while "until X lands" / "with the Y PR" stay convention — no needle separates them from legitimate prose about when a value settles. F-E9's header is now byte-identical to what `cliff.toml` renders, asserted by diffing the two. |

### What the low tier turned out to contain

The lows were not the leftovers the tier implies. **Three were live user-visible
defects and a fourth was a data-loss path** — the same ratio as the mediums, where
eight of twenty were live. The tier tracked how *hard the finding was to notice*,
not how much it cost once noticed: a `0o000` member is rare, a put row is a
glance, a vsplit inside an archive is a narrow habit, and a torn write needs a
crash. None of that made them cosmetic.

The recurring shapes held too. **Two derivations of one fact** (staging's mode vs
the index's) and **the adjacent case is tested** (the exiting helper, the walk's
half of the cap, the `Archived` path's mode, the row half of the coordinate
translation) both reappeared. And a new one: **a claim updated in one copy and
left in three** — `proc.rs` learned about 1002 while `mouse/mod.rs` and `lib.rs`
did not, which is the same failure as M23's four instances found by grepping after
fixing the filed three.

### One regression this campaign introduced

#381 (M22) moved the clipboard write off the loop and added
`Message::ClipboardCopyDone`. It was wired into two of the three matches that must
agree about a payloadless done-wake — the third had a catch-all and could only
fail open. Result: a select-and-copy on a **quiet** spyc panics the loop. Found by
hand-driving `main`, never released, fixed in #402.

Worth recording because of what it says about the fix pattern: that catch-all had
already failed open four times, and there was a regression test whose doc said
"Add every new payloadless done-wake to this list so the trap can't recur
silently." It recurred twice while that sentence sat there. **An instruction to a
human is not a guard** — #402 removes the catch-all so the compiler enforces it.

### Sixth pass — the informational tail (2026-08-12)

`main` `b11edca` → `20ad72a`. Six PRs.

| PR | closes | what closing it showed |
|---|---|---|
| #406 | F18 (part 1) | Filed at 1448/1500, found at **1495** — five lines. Extracted the five pure predicates to `util.rs` (whose own header already drew that line, and they were on the wrong side of it) and the loop-scoped `Draw`/`DispatchFlow`/`RunCtx` to `run.rs` (their own docs say they are `run()`-scoped). 1495 → 1278; ceiling → 1350. |
| #407 | F18 (part 2) | Measured before deciding: **52% of mod.rs was `///` field docs** on `Runtime`/`ViewState`. So the fix was not to move comments to a README — that would have satisfied the count while the module still held the same thing, and detached docs from the fields they explain, which is the exact drift F11/#400 kept finding. Moved the *types*, docs attached. 1127 → 425; ceiling → 600. Fields `pub(super)` = the reachability they already had, so **no call site changed**. |
| #408 | F-E15, F-E16, F16 | `^a g` was bound and absent from the popup FEATURES.md calls "the discovery surface"; `?` taught `Space` as "move right one column" when it has been the leader since the chord overhaul. Both are the two-derivations shape, so the fix is a guard: feed every key at every chord state and require anything resolving to an `Action` to be advertised. The tier guard now walks the same discovered tree instead of hardcoding `Space w`, which also brings `^s` into scope. |
| #409 | F15, F17 | The index guard's `contains(name)` let a **longer sibling** satisfy a shorter module: `route.rs` covered by `archive_route.rs`, and three more real pairs — failing open exactly where names are most similar. Bite-checked both ways. |
| #410 | INFO-1, INFO-2 | `verify` streamed the rewritten tar back out with `u64::MAX` — a second full copy of the expanded tree, ignoring the knob set to bound exactly that. Now the configured budget; refusal lands before the rename, so it fails safe. |
| #411 | C-F12, D10, D11 | Three correct-but-unwritten decisions: the trusted root is spyc's launch dir *and every descendant*; `via = "both"` over SSH discards the OSC-52 error that was the informative one; a suspended tab restores running. The last is now pinned with an exhaustive `SavedTab` literal, so adding the field is a compile error rather than a passing assertion. |

### What the informational tier turned out to contain

Same lesson as the lows, one tier down. **F18 had drifted from "worth naming" to
"five lines from blocking the next feature" in the two days between filing and
fixing** — the only finding in the review whose severity moved on its own. The
rest were correct decisions nobody had written down, which is the category that
gets "fixed" later by someone reading them as bugs.

### Seventh pass — verifying the record itself (2026-08-12)

Prompted by the owner asking what was left. Rather than re-read §7, every
finding was checked against the code. Twelve of the 35 table entries did not
appear in §7 under their table IDs; eleven were genuinely fixed under other
labels (B1's per-component containment, H7's guard now scanning `lib.rs`, M15's
mouse keybindings, M18's `archive_name` in the fuzz matrix, M19's
`archive_container` target, …). The twelfth was HIGH-1, and probing HIGH-2
found the second.

| PR | closes | what it showed |
|---|---|---|
| #412 | HIGH-1 (site 3/3) | The helper existed and was unit-tested; nothing tied it to its call sites. Replaced a non-biting end-to-end test with a structural guard, bite-checked against all three original sites. |
| #414 | HIGH-2 (consequence 3/3) | A zip understating its size defeats a cap that reads the declaration. Two tests — the function and the wiring — because the wiring edit silently failed to apply and only the second caught it. |
| #415 | — | The rule, in AGENTS.md. |

**Multi-site findings are the risk class.** Every finding naming more than one
site was re-verified: H4 (guard-enforced), H5, H6, H8 (five demotion tests), H9
(all three modules), M1, M3 (guard-enforced), M5, M23. Those hold.

### Eighth pass — an adversary on the seventh pass's own output (2026-08-14)

The seventh pass closed with "nothing still open, verified against the code."
An independent adversarial read of #406–#415 — the ten commits that pass
produced — found a **tag-blocking escape in the containment code the pass had
just verified**, plus four guards green on less than they claimed.

| PR | closes | what it showed |
|---|---|---|
| #417 | a staged link resolving outside the mount | `link_target_contained` canonicalized the link's *parent* and folded `..` in the *target* lexically. `d/link1 -> ..` is contained; `L -> d/link1/../ESCAPED` folds to a path inside staging while `open()` follows `link1` first and climbs out. Read a file outside the mount through a link inside it. |
| #418 | four guards | The module index was satisfied by a *prose* mention; the declared-size guard checked one spelling of "allocate"; the chord walk asserted a floor of 8 against 16 states; a popup alias sharing a row was never fed. |

**B1 and HIGH-3 were both closed correctly, and that is the finding.** The
review filed the escape's two *known* consequences separately — B1 as arbitrary
file **write**, HIGH-3 as the **MCP read**, the latter explicitly noting that
`is_file()` follows staging symlinks. Both fixes hold: `contained_dest` still
refuses to traverse a link component, and the MCP read still canonicalizes
against the mount. What no finding named was the third consequence — the **TUI**
readers — and the escape primitive itself survived in a direction nobody had
probed: the link *target* rather than the member *destination*.

Traced afterwards, three of the six effects routed through the staging tree
follow a staged link: inventory yank (`fs::copy`, which persists the target's
bytes into the on-disk cache), pager open (`fs::metadata`), and pipe
(`fs::read_to_string`). Copy-out, `:longlist` and `:filetype` use
`symlink_metadata` and do not.

**The recurring shape, one tier up.** §7 records it twice at HIGH: a finding
naming N *sites* is closed when N are fixed. B1 is the same shape applied to
*consequences* — and unlike sites, which a structural guard can enumerate for
you, the consequence list is the part no test can check. Nothing was wrong with
either fix; the enumeration was short by one, and "verified against the code"
answered only the questions the record already knew to ask.

**No advisory is needed.** Archive browsing has never shipped: `src/archive/` is
absent from v2.0.3 and #301 is contained in no tag. The whole mount surface —
the pre-#347 spelling check, the fold that replaced it, and every consumer above
— has only ever existed on the unreleased CURRENT stream. v2.1.0 is the first
release to carry the feature, and it carries the fix.

### Still open

**Nothing, at any tier.** All 58 original findings closed, plus the two the
eighth pass added, across eight passes and 49 PRs (#346–#418). The 2.1 tag is
unblocked on review grounds.

Stated once, plainly, because the seventh pass phrased it more strongly than it
had earned: this is a claim about every question asked so far, not about the
code. The eighth pass exists because the seventh forgot that distinction, and
the eighth is not exempt from it.
