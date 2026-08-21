# Archived plans

Shipped plan documents, kept as historical record of the designs and
decision logs. Nothing here describes future work — the living
roadmap is [`ROADMAP.md`](../../ROADMAP.md) at the repo root.

| Doc | Shipped as |
|---|---|
| `REFACTOR_PLAN.md` | The `app/mod.rs` decomposition, Phases 1–2 (PRs #180–#194); superseded further by the impl-extraction sweep (#248–#259) and the 800-LoC campaign. |
| `MVU_PLAN.md` | The full Model-View-Update migration, Phases 0–6 + D + E (PRs #196–#247). CLAUDE.md is the living statement of the resulting contract. |
| `V1_5_PLAN.md` | v1.50.0 — pager/task-viewer unification (`Mount`, scrollback view, visual block mode, task↔pane commands). |
| `CODE_REVIEW_2026-06.md` | The June-2026 deep-review remediation campaign (PRs #329–#424): ~112 findings across security, correctness, perf, and maintainability, all fixed or owner-accepted. The render-purity / off-thread / guard-test lessons live on in CLAUDE.md + AGENTS.md. |
| `LAUNCH_PLAN_2_0.md` | v2.0.0 — the public-distribution launch. Every done-criterion closed; the banner also names the cheap wins that never shipped. |
| `ARCHIVE_BROWSING_PLAN.md` | v2.1.0 — walking into a zip/tarball and editing what's inside ([#149](https://github.com/Tripstack-Corp/spyc/issues/149)). |
| `native_scroll_plan.md`, `mouse_selection_plan.md` | v2.1.0 — the mouse suite (capture, wheel, buttons, drag-select). Neither plan was amended while it was built; the banners name the divergences. |
| `pasted-image-preview-plan.md` | v2.1.0 — paste capture, image preview, and the `^a g` gallery ([#119](https://github.com/Tripstack-Corp/spyc/issues/119)). |
| `2.1-release-notes.md` | The human-facing v2.1 notes, as written at tag time. |
