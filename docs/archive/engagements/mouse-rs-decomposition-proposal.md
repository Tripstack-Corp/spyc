# `src/app/mouse.rs` decomposition — proposal (F4 Part B)

**Status:** proposal awaiting approval. No code moved.
**Measured at:** `72d3ae7`.

## Why this file and not the others

The review said "twenty-plus production files exceed 800 lines." That count —
and the 35 I first reproduced — comes from `wc -l`, which includes inline test
modules. The house convention keeps tests at the bottom of the same file, so a
1,250-line file can hold 830 production lines and be perfectly well factored.

Counting only lines outside `#[cfg(test)]` modules (brace-matched, not
split-on-first-occurrence — that naive version is fooled by a *comment*
mentioning the attribute, which is exactly how `render/mod.rs` reads as 22
production lines when it has 827):

| File | production | total |
|---|---|---|
| **`src/app/mouse.rs`** | **1,529** | 3,038 |
| `src/app/mod.rs` | 1,438 | 1,443 |
| `src/mcp/protocol.rs` | 1,076 | 1,147 |
| `src/app/state/mod.rs` | 1,028 | 1,029 |
| `src/app/pane_tabs.rs` | 1,005 | 1,004 |
| `src/app/pager_handler/mod.rs` | 992 | 991 |

Sixteen files exceed 800 production lines, so the rule has genuinely drifted —
but far less than a raw `wc -l` suggests, and `app/mod.rs` is deliberately
ceiling-guarded at 1,500 and legitimately holds the module's type definitions
(AGENTS.md allows exactly that).

`mouse.rs` is the outlier: largest, and ~450 production lines clear of the next
non-guarded file. It also grew ~1,000 lines in #233/#234, so the seams are
recent and still legible rather than fossilised.

## The seams

The file is two things sharing a filename: a pure decision layer and an impure
dispatch layer. That boundary is already explicit — it's the `route.rs` /
`focus.rs` template the codebase applies elsewhere.

| Lines | Cluster | Content |
|---|---|---|
| 50–615 | **pure decisions** | `MouseDragTarget`, `ListSelection`, `ChromeSelection`, `PaneScrollStreak`, `ChromeRow`, `MouseSnapshot`, timing constants, `scroll_streak_step`, `AgentViewAction` / `AgentViewInputs` / `decide_agent_view_action`, `route_mouse`, `hit`, `region_at`, `clamp_to_area` |
| 629–808 | **dispatch** | `handle_mouse` — the one entry point |
| 809–980 | **scroll** | `active_pane_wheel_scroll`, `send_scroll_keys`, `send_agent_view_scroll_keys`, `repeat_key_effect` |
| 981–1357 | **selection** | pager / chrome / pane / list-row `begin`+`extend`+`finish` triples, plus `chrome_col_at`, `pane_cell_at`, `list_row_at` |
| 1358–1483 | **forwarding** | `forward_to_child`, `forward_drag`, `forward_release`, `focus_pager_slot`, `focus_region` |
| 1484–1528 | | `mouse_report` |
| 1529–3038 | tests | move with their subjects |

Four near-identical `begin`/`extend`/`finish` triples in one contiguous block is
the strongest signal — that's a cohesive subsystem, not an arbitrary cut.

## Proposed shape

```
src/app/mouse/
  mod.rs        ~250   type defs + `handle_mouse` dispatch + re-exports
  route.rs      ~400   MouseSnapshot, route_mouse, region_at, hit,
                       scroll_streak_step, decide_agent_view_action, constants
  selection.rs  ~375   the four begin/extend/finish clusters
  scroll.rs     ~170   wheel + agent-view scroll, repeat_key_effect
  forward.rs    ~170   forward_to_child/drag/release, focus_*, mouse_report
```

Every file lands well under 800 production lines. Tests move beside their
subjects, which also splits the ~1,500-line test block along the same seams.

`mod.rs` keeping the type definitions matches AGENTS.md ("a module root holding
its own type defs is a legit reason") and mirrors `app/mod.rs`.

## What would make this a bad idea

State me wrong before doing it:

- **If the impure clusters share private `App` state heavily**, the split forces
  cross-module reaching and is worse than a long file. The descendant-module
  rule means child modules *can* read `App`'s private fields, so this is
  probably fine — but verify per cluster rather than assuming.
- **`handle_mouse` is a dispatcher.** The existing `too_many_lines` clippy-allow
  rationale about dispatch functions applies; don't shred it to hit a number.
- **Relocations must be verbatim.** Behaviour-preserving, no test-assertion
  edits — the `project_mod_extraction_sweep` playbook. If a move needs a
  behaviour change, that's a separate PR first.

## Sequencing

One PR per module, verbatim moves, gate green between each. `route.rs` first:
it's pure, has the cleanest boundary, and carries its own tests — so it proves
the seam before anything impure moves.

## Not recommended

Splitting the other fifteen. `app/mod.rs` is guarded and type-defs;
`protocol.rs`, `state/mod.rs`, and `pane_tabs.rs` are 1,000–1,076 production
lines with no comparably obvious seam. Chasing those is churn. Revisit
individually if one crosses ~1,200.
