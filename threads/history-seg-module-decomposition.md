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
