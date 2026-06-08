# history-seg-module-decomposition — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-seg-module-decomposition
Created: 2026-06-08T22:09:24.180864+00:00

---
Entry: Claude Code (caleb) 2026-06-08T22:09:24.180864+00:00
Role: scribe
Type: Note
Title: PR #248–259: mod-extract pr1–12 — twelve verbatim cuts off app/mod.rs

Spec: scribe

tags: #history #module-decomposition

Moment: module-decomposition — Reconstructed: a 12-PR wave (one cut per PR) relocates self-contained concerns out of `src/app/mod.rs` into sibling `src/app/*.rs` files; pure verbatim moves, line-for-line conserved.   [kind: refactor]
When: 2026-06-02→03 · PR #248–259 (refactor/mod-extract-pr1..pr12) · commits 5bfe7950, 504ebdd7, 3ba07bd0, dae370d6, 487615a5, ad7a54c8, 1451bf0c, d8f824cc, 56b7e85d, 7560da1e, 92c30e10, 488ff7e9
Recorded rationale: "extract graveyard / harpoon / config from mod.rs" — pre-squash subject (commit 7b2b131, refactor/mod-extract-pr1-leaves). The campaign goal is stated in `REFACTOR_PLAN.md`: "No file in `src/app/` over ~1500 lines … Each was a verbatim move + a `mod ...; use ...;` import — no behavior change, no API change. Each was one PR." (REFACTOR_PLAN.md, Phase 1).
Inferred intent: this wave continues the `REFACTOR_PLAN.md` decomposition track *after* the MVU rewrite (see history-seg-refactor-mvu) — PR #248 lands directly after the MVU Phase E6/E7 render-teardown PR (#247, commit 7034c2d). Each diff is a near-perfect insertion/deletion balance (e.g. #248: +541/-502; #257 pane-tabs: +431/-429; #258 prompt: +514/-501), the verbatim-move signature. confidence: high
Supersedes: extends the `REFACTOR_PLAN.md` Phase 1–2 extractions (PRs #180–#196, history-seg-refactor-mvu) — same one-way-dependency / child-module pattern, applied to the concerns MVU left in `mod.rs`.

Reconstructed: the wave sets the campaign's working convention — one cohesive concern per PR, relocated verbatim with a `mod x; use x::…;` line, insertion count ≈ deletion count, no test edits. The twelve cuts, folded:
+ #248 leaves → `config.rs` / `graveyard.rs` / `harpoon.rs` (mod.rs -502)
+ #249 clipboard MCP routing → `clipboard.rs` (+266/-252)
+ #250 pager MCP → `mcp.rs` (+208/-202)
+ #251 pager hub → `pager_handler.rs` (+396/-381)
+ #252 git display state → `git_state.rs` (+156/-158)
+ #253 grep/find → `find_picker.rs` / `grep_session.rs` (+328/-313)
+ #254 background tasks → `tasks.rs` (+643/-633)
+ #255 nav/qs scroll → `pane_scroll.rs` (+226/-215)
+ #256 navigate/quick-select → `navigate.rs` / `quick_select.rs` (+507/-471)
+ #257 pane tabs → `pane_tabs.rs` (+430/-429)
+ #258 prompt → `prompt.rs` (+513/-501)
+ #259 session/render → `session.rs` / `render.rs` / `route.rs` (+ a 4-line `keymap/action.rs` touch — the only non-mechanical edit in the wave).

Provenance:
- 5bfe7950 (PR #248 refactor/mod-extract-pr1-leaves, 2026-06-02) — `src/app/{config,graveyard,harpoon}.rs` created; `src/app/mod.rs` -505 lines
- 7b2b131 (pre-squash on bitbucket/refactor/mod-extract-pr1-leaves) — subject "extract graveyard / harpoon / config from mod.rs"; lands after MVU Phase E6/E7 (7034c2d)
- 488ff7e9 (PR #259 refactor/mod-extract-pr12-session-render, 2026-06-03) — `src/app/{session,route}.rs` created, `render.rs`/`keymap/action.rs` touched
- REFACTOR_PLAN.md (Phase 1, lines 81–106) — "verbatim move + a `mod ...; use ...;` import — no behavior change … Each was one PR"; goal "No file in `src/app/` over ~1500 lines"

<!-- Entry-ID: 01KTMMGZWER9304EZM82KTEZMQ -->

---
Entry: Claude Code (caleb) 2026-06-08T22:10:01.580197+00:00
Role: scribe
Type: Note
Title: PR #275–281: decompose-mod-* — mod.rs split into run/bootstrap/proc/tests/util siblings

Spec: scribe

tags: #history #module-decomposition

Moment: module-decomposition — Reconstructed: the campaign's "800-LoC" phase carves the event-loop machinery and test scaffolding out of `src/app/mod.rs` into named siblings (`run.rs`, `bootstrap.rs`, `proc.rs`, `util.rs`) and pulls the in-file test modules into their own files; one `fs/ops.rs` formatter split rides along first.   [kind: refactor]
When: 2026-06-03→04 · PR #275–281 (refactor/decompose-fs-ops, -mod-runloop, -bootstrap, -proc, -tests, -harness, -util) · commits cf48899a, 5fa1b404, 0681d216, 009b1ad0, 50b92967, 4ee4ceae, da42bb16
Recorded rationale: "extract the long-listing formatter into fs/long_listing.rs (800-LoC campaign)" — pre-squash subject (commit 19df2a1, refactor/decompose-fs-ops). The "800-LoC campaign" tag is the recorded name of this phase; AGENTS.md later records its effect: mod.rs cut "to ~1k … the constructor, event loop, process I/O, and leaf helpers are sibling modules" (AGENTS.md, edited PR #282).
Inferred intent: where mod-extract (#248–259) moved feature concerns, this phase targets the structural core REFACTOR_PLAN.md had deliberately left in mod.rs ("the App struct, App::new, and the ~920-line run event loop" — REFACTOR_PLAN.md lines 156–161). The diffs are large balanced cuts: #276 run.rs +670/-657, #277 bootstrap.rs +318/-308, #278 proc.rs +363/-351, #281 util.rs +260/-264. confidence: high
Supersedes: REFACTOR_PLAN.md Phase 2 done-criteria (lines 154–161) explicitly named these as *optional follow-on* and listed run/`App::new` as "what remains is largely what the criterion intends to stay"; this phase moves them anyway, taking mod.rs below the Phase-2 floor.

Reconstructed: the wave establishes the test-extraction convention later codified in PR #308's no-subprocess guard — test modules go to `*_tests.rs` / `tests.rs` files (the no-subprocess git scan special-cases exactly these names). The seven cuts, folded:
+ #275 long-listing formatter → `fs/long_listing.rs` (+394/-384; the one non-app cut, lands first)
+ #276 the ~660-line `run` event loop → `run.rs`
+ #277 `App::new` → `bootstrap.rs`
+ #278 process I/O (input reader, foreground exec) → `proc.rs`
+ #279 unit tests → `mod_tests.rs` (+515/-513; file header names "(800-LoC campaign)")
+ #280 test harness → `harness_tests.rs` + `test_harness.rs`
+ #281 leaf helpers (time/byte/text format) → `util.rs`
The guard test that polices the result lives in the file #279 created — `app::guard_tests::mod_rs_stays_decomposed`, ceiling 4000 lines, "If you hit this: extract a module, don't bump the ceiling" (src/app/mod_tests.rs:14–32).

Provenance:
- cf48899a (PR #275 refactor/decompose-fs-ops, 2026-06-03) — `src/fs/long_listing.rs` created, `fs/ops.rs` -384
- 19df2a1 (pre-squash, bitbucket/refactor/decompose-fs-ops) — subject tags "(800-LoC campaign)"
- 5fa1b404 (PR #276, 2026-06-04) — `src/app/run.rs` +670; 0681d216 (#277) bootstrap.rs; 009b1ad0 (#278) proc.rs; da42bb16 (#281) util.rs
- 50b92967 (PR #279, 2026-06-04) — `src/app/mod_tests.rs` created (+515); src/app/mod_tests.rs:1 "Unit tests relocated from app/mod.rs (800-LoC campaign)"
- REFACTOR_PLAN.md (lines 154–161, 286–297) — Phase-2 criteria naming run/App::new as intended-to-stay and the follow-on tidy-up as optional

<!-- Entry-ID: 01KTMMHZCN03GA638EHKKCY50E -->

---
Entry: Claude Code (caleb) 2026-06-08T22:10:26.637857+00:00
Role: scribe
Type: Decision
Title: PR #282: CLAUDE.md architectural contract — the ~800-line ceiling codified

Spec: scribe

tags: #history #module-decomposition

Moment: module-decomposition — Reconstructed: a new `CLAUDE.md` "architectural contract" writes the decomposition rule down as a standing invariant ("No `.rs` over ~800 lines without a solid reason"), and `AGENTS.md`/`ARCHITECTURE.md` are refreshed to record the campaign's end-state (mod.rs ~1k, the module index).   [kind: convention]
When: 2026-06-04 · PR #282 (docs/claude-md-architecture) · commit f5c2b4aa
Recorded rationale: "No `.rs` over ~800 lines without a solid reason. Oversized files make diffs impossible to reason about. When a file grows, extract a cohesive child/sibling module (verbatim relocation, behavior-identical) rather than letting it sprawl. A module root holding its own core *type definitions* is a legitimate 'solid reason'; a pile of helpers is not." — CLAUDE.md (added PR #282).
Inferred intent: this is the rationale source for the whole campaign — it states *why* files are being split (diff reviewability) and the rule that distinguishes a legitimate module root from a junk drawer. confidence: high
Supersedes: the earlier ~1500-line target in REFACTOR_PLAN.md ("No file in `src/app/` over ~1500 lines", line 30). The new ceiling is ~800 and applies repo-wide, not just `src/app/` — which is why the per-subsystem wave (#297–#308) reaches into `ui/`, `mcp/`, `keymap/`, `state/`, `git/`.

Reconstructed: CLAUDE.md also fixes the decomposition mechanics it expects: "`app/mod.rs` is the module root, not a junk drawer" — it holds the core type defs (App/Runtime/ViewState/Message) and a little glue; `run` → run.rs, `App::new` → bootstrap.rs, process I/O → proc.rs, leaf helpers → util.rs (exactly the #276–281 split). The AGENTS.md edit records the campaign by name: mod.rs "carved down to ~1k … the 800-LoC campaign" and updates the module index to the post-split layout. ARCHITECTURE.md flips the MVU narrative from "purity pass in progress" to "done", tying the decomposition to the completed MVU foundation (see history-seg-refactor-mvu).

This entry is typed Decision: it carries a quoted, scoped rule (the ~800-line ceiling + the "module root vs junk drawer" distinction) that governs every subsequent decompose PR in the slice.

Provenance:
- f5c2b4aa (PR #282 docs/claude-md-architecture, 2026-06-04) — adds CLAUDE.md (82 lines, new file); edits AGENTS.md (+module index), ARCHITECTURE.md, CHANGELOG.md
- CLAUDE.md (added PR #282, lines 35–47) — the ~800-line ceiling + extract-don't-sprawl rule
- AGENTS.md (edited PR #282) — "the 800-LoC campaign carved it down to ~1k"; module index for the post-split `src/app/`
- REFACTOR_PLAN.md (line 30) — the superseded ~1500-line target

<!-- Entry-ID: 01KTMMJVZMX3SJCBSK8YF896YP -->

---
Entry: Claude Code (caleb) 2026-06-08T22:11:06.889223+00:00
Role: scribe
Type: Note
Title: PR #297,298,303,305,306: UI/render subsystems split into directory modules

Spec: scribe

tags: #history #module-decomposition

Moment: module-decomposition — Reconstructed: the rendering subsystems are converted from single oversized `.rs` files into directory modules (`foo.rs` → `foo/{mod.rs, …}`), splitting renderer logic from tests and from per-area concerns; the two largest files in the slice (`ui/pager.rs` ~2954 lines, `app/pager_handler.rs` ~1500, `app/render.rs` ~1406) are the headline cuts.   [kind: refactor]
When: 2026-06-06→07 · PR #297, #298, #303, #305, #306 (refactor/decompose-markdown, -diff-render, -render, -pager-handler, -pager) · commits 02f22c98, 1d05b331, 71817e4e, c325b815, e077c047
Recorded rationale: "decompose pager.rs into a directory module" / "decompose handle_pager_key into per-context sub-handlers" — pre-squash subjects (commits 5dc68c3, ad34202, on the -pager / -pager-handler branches). The governing rule is CLAUDE.md's ~800-line ceiling (PR #282): each of these files was far over it.
Inferred intent: same verbatim-relocation discipline as the mod.rs phases, now applied to `src/ui/` and `src/app/render`+`pager_handler`. Diffs are insertion/deletion-balanced directory conversions (e.g. #306: pager.rs -2954, six new pager/*.rs +2954; #303: render.rs -1406, four render/*.rs +1456). Snapshot `.snap` fixtures are `git mv`'d into the new `snapshots/` subdirs (0-line moves in the stat), so the `insta`/`TestBackend` net stays intact. confidence: high
Supersedes: `src/app/render.rs` and `src/app/pager_handler.rs` were themselves created by the REFACTOR_PLAN.md Phase-2 extraction (render.rs at commit 71bc573, pager_handler.rs at 13a1f2d, both 2026-05-30; see history-seg-refactor-mvu) and re-created in the mod-extract wave above — this moment splits those single files into directory modules.

Reconstructed: the five cuts, grouped by area, folded:
+ #297 markdown → `ui/markdown/{mod,renderer,wrap,tests}.rs` (renderer.rs -595, mod+wrap+tests +604)
+ #298 diff-render → `ui/diff_render/{mod,tests}.rs` (pure test split, +379/-379)
+ #303 render → `app/render/{mod,chrome,inner,overlays}.rs` (+1456/-1408, snapshots moved)
+ #305 pager-handler → `app/pager_handler/{mod,modes,motion,pickers}.rs` (+1656/-1501; pre-squash shows a two-step: first split `handle_pager_key` into per-context sub-handlers, then relocate into the directory)
+ #306 pager → `ui/pager/{mod,construct,layout,render,scroll_search,selection,tests}.rs` (+3021/-2954; the single biggest file in the campaign)
Each `decompose-*` PR also bumps the one-line module-index reference in AGENTS.md (and ARCHITECTURE.md for #303) in the same commit — the "update affected docs in the same commit" rule from CLAUDE.md.

Provenance:
- e077c047 (PR #306 refactor/decompose-pager, 2026-06-07) — `src/ui/pager.rs` -2954; six pager/*.rs created; 4 snapshot files `git mv`'d
- 71817e4e (PR #303 refactor/decompose-render, 2026-06-07) — `src/app/render.rs` -1406; render/{mod,chrome,inner,overlays}.rs; 5 snapshots moved
- c325b815 (PR #305, 2026-06-07) — `src/app/pager_handler.rs` -1500; four sub-files
- 02f22c98 (PR #297, 2026-06-06) markdown; 1d05b331 (PR #298) diff_render
- ad34202 / 5dc68c3 (pre-squash, bitbucket/refactor/decompose-pager*) — subjects naming the directory-module conversion
- render.rs origin: 71bc573 (2026-05-30); pager_handler.rs origin: 13a1f2d (2026-05-30) — the superseded Phase-2 single files

<!-- Entry-ID: 01KTMMM1322W32NGAH5H7MWYEP -->
