# feature-bracket-pick-jump — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: feature-bracket-pick-jump
Created: 2026-05-16T10:22:27.942498+00:00

---
Entry: Claude Code (caleb) 2026-05-16T10:22:27.942498+00:00
Role: planner
Type: Note
Title: Gap: no keyboard chord to jump between picked files

Spec: planner-architecture

## The gap

spyc reserves the `[` / `]` family for **"next / prev *thing*"** cursor jumps. Today there is exactly one member: `]g` / `[g` (next / prev git-changed entry, wraps, flash on empty).

There is **no chord to jump between picked (tagged) entries** in the current listing — even though picks are the most-used multi-select primitive in the tool (`t`, `T`, `^T`, `=!`, `yf`, `:limit !`, `p`, `take`, MCP `pick_files` / `clear_picks` / `search_picks`, etc.).

The de-facto workaround is the `=!` limit filter: hide everything that isn't picked, walk with `j`/`k`, then `=` to clear. That's three keystrokes (plus a mode swap and a re-render) for a motion that wants to be one chord, and it *destroys context* — non-picked rows vanish while you traverse.

## Why now, and why as a chord

- The bracket family is *the* idiom for cross-row jumps and is currently underpopulated (one member). Adding `]p` / `[p` makes the family read as a family rather than a one-off.
- Picks already carry every property the jump needs:
  - Persistent per-directory state (`Picks: HashSet<PathBuf>` in `src/state/picks.rs`).
  - A cheap `contains(&Path) -> bool` predicate.
  - Reliable join to the listing (`Row.path`, used identically by `jump_to_git_change`).
- The implementation is a near-mechanical mirror of `jump_to_git_change` (src/app/state.rs:245-275) — same wrap logic, same flash-on-empty, same wiring.
- It complements rather than replaces `=!`: `=!` is for **focusing** on picks; `]p` / `[p` is for **traversing** picks while keeping the surrounding listing visible. The two compose.

## Scope of this thread

- **In scope:** `]p` / `[p` for picks in the current listing.
- **Out of scope (separate follow-up):** `]P` / `[P` for harpooned entries within the current directory. Mentioned only so the family's namespace stays consistent.

## Cross-link

Tracked in **GitHub issue [Tripstack-Corp/spyc#1](https://github.com/Tripstack-Corp/spyc/issues/1)** — *"Add ]p / [p chord to jump cursor to next / prev picked file"*.

The next entry in this thread is the full execution plan, ready for an implementer agent to pick up cold.

---

**Pointers**

- `src/keymap/action.rs:144-147` — `JumpNextGitChange` / `JumpPrevGitChange` (template for the new action variants)
- `src/keymap/resolver.rs:34-37,240-257` — `PendingSeq::NextBracket` / `PrevBracket` and the `g`-only sub-command match (the matched sub-command list is what grows)
- `src/app/state.rs:245-275` — `jump_to_git_change` (the algorithm to mirror)
- `src/app/state.rs:763-772` — action handler + flash fallback
- `src/state/picks.rs` — `Picks::contains(&Path) -> bool`, the predicate
- `src/ui/help.rs:182-183` — the help-overlay row to extend
- `src/config/dsl.rs:150` — config DSL alias precedent (`pick` → `Action::TogglePick`)

<!-- Entry-ID: 01KRR4XYSASG86Q1X7WE61AFMM -->
