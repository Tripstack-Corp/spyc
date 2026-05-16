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

---
Entry: Claude Code (caleb) 2026-05-16T10:23:41.772897+00:00
Role: planner
Type: Plan
Title: Execution plan: ]p / [p next/prev pick jump — complete and explicit

Spec: planner-architecture

This plan is written to be picked up cold by an implementer agent. Every step names the file, the anchor, and what to change. References to existing code use `path:line` to survive small line drifts.

Tracks **GitHub issue [Tripstack-Corp/spyc#1](https://github.com/Tripstack-Corp/spyc/issues/1)**.

---

## 0. Branch & PR conventions

- **Branch:** `feature/bracket-pick-jump` (cut from `main`).
- **PR title:** `feat(keymap): ]p / [p — cursor jumps to next / prev picked entry`.
- **PR body:** must include "Closes Tripstack-Corp/spyc#1".
- **Bump:** patch-level SemVer (additive, no breaking change). Touch the version in `Cargo.toml` *and* `CHANGELOG.md`'s [Unreleased] section per `CONTRIBUTING.md` versioning rules.
- **Commit footer convention** (per AGENTS.md / orphan branch protocol) — author identity will be set by Watercooler MCP; no manual footer required.

## 1. Action variants

**File:** `src/keymap/action.rs`

After the `JumpNextGitChange` / `JumpPrevGitChange` block (around action.rs:144-147), add a parallel pair:

```rust
// Cursor jumps to next / previous picked entry in the current
// listing. Wraps when there's no match in the desired direction
// so the user can keep pressing the chord without thinking about
// direction. No-op flash when the listing has no picks.
JumpNextPick, // ] p — cursor to next picked entry (wraps)
JumpPrevPick, // [ p — cursor to prev picked entry (wraps)
```

And in the matching `Action::describe` `impl` block (around action.rs:212-213), add:

```rust
Self::JumpNextPick => "jump to next picked entry",
Self::JumpPrevPick => "jump to prev picked entry",
```

Keep alphabetical / family ordering consistent with the existing file — picks pair appears immediately after the git-change pair.

## 2. Resolver — extend the bracket sub-command match

**File:** `src/keymap/resolver.rs`

The bracket pending-sequence state is already in place (resolver.rs:34-37 — `PendingSeq::NextBracket` / `PrevBracket`). The only change needed is in the `[` / `]` sub-command match (resolver.rs:242-257):

```rust
if matches!(
    self.pending,
    PendingSeq::PrevBracket | PendingSeq::NextBracket
) {
    let is_next = self.pending == PendingSeq::NextBracket;
    let out = match ev.code {
        KeyCode::Char('g') => ResolverOutcome::Action(if is_next {
            Action::JumpNextGitChange
        } else {
            Action::JumpPrevGitChange
        }),
        KeyCode::Char('p') => ResolverOutcome::Action(if is_next {
            Action::JumpNextPick
        } else {
            Action::JumpPrevPick
        }),
        _ => ResolverOutcome::Ignored,
    };
    self.reset();
    return out;
}
```

Also update the doc comments for `PendingSeq::NextBracket` / `PrevBracket` (resolver.rs:34-37) to note `p` as a sub-command, e.g. `"`g` = next git change, `p` = next pick"`.

## 3. State — the jump method

**File:** `src/app/state.rs`

Add a new method directly below `jump_to_git_change` (state.rs:245-275), so the two live as a family:

```rust
/// Move the cursor to the next (or previous) picked entry in the
/// current listing. Wraps; returns `false` when the listing has
/// no picks so the caller can flash an empty-search message.
pub fn jump_to_pick(&mut self, forward: bool) -> bool {
    let len = self.rows.len();
    if len == 0 || self.picks.is_empty() {
        return false;
    }
    let cur = self.cursor.index.min(len.saturating_sub(1));
    let is_picked = |idx: usize| -> bool {
        self.rows
            .get(idx)
            .is_some_and(|r| self.picks.contains(&r.path))
    };
    for n in 1..=len {
        let idx = if forward {
            (cur + n) % len
        } else {
            (cur + len - (n % len)) % len
        };
        if is_picked(idx) {
            self.cursor.index = idx;
            return true;
        }
    }
    false
}
```

Notes:
- Mirrors `jump_to_git_change` exactly so future maintainers can spot the family.
- `self.picks.is_empty()` is the cheap early-out (paralleling `self.git_files.is_empty()`).
- The predicate hits `self.picks.contains(&r.path)` — same `Row.path` field already used by the rebuild path and `selection()`.

## 4. Wire the actions in the handler

**File:** `src/app/state.rs`

Immediately after the `JumpPrevGitChange` arm (state.rs:768-772), add:

```rust
Action::JumpNextPick => {
    if !self.jump_to_pick(true) {
        self.flash_info("no picks in this directory");
    }
}
Action::JumpPrevPick => {
    if !self.jump_to_pick(false) {
        self.flash_info("no picks in this directory");
    }
}
```

Same flash idiom and same view-agnosticism as `JumpNext/PrevGitChange`. No need to gate on `view` — `self.picks.is_empty()` already short-circuits when there's nothing to traverse.

## 5. Tests

### 5a. State-level (`src/app/state.rs`)

Mirror the existing git-change tests (state.rs:2458-2492). Add a new `#[cfg(test)] mod tests` block (or extend the existing one) with the following cases:

1. `jump_to_pick_walks_forward_with_wrap` — three rows, pick rows 0 and 2, cursor on row 0, forward jump lands on 2, next forward jump wraps to 0.
2. `jump_to_pick_walks_backward_with_wrap` — same setup, backward jump from row 0 lands on 2 (wrap), backward from 2 lands on 0.
3. `jump_to_pick_returns_false_when_no_picks` — both directions return `false`, cursor unchanged.
4. `jump_to_pick_returns_false_when_rows_empty` — empty listing, both directions return `false`.
5. `jump_to_pick_skips_unpicked_rows` — five rows, pick only row 3, jump from row 0 lands directly on 3.

Use the same test scaffolding the existing `jump_to_git_change_*` tests use — load a few synthetic rows, set `state.picks.insert(&path)`, call the method, assert on `state.cursor.index`.

### 5b. Resolver-level (`src/keymap/resolver.rs`)

Mirror the existing bracket-chord tests (the `]g` test is around resolver.rs:1549). Add at minimum:

1. `bracket_next_p_resolves_to_jump_next_pick` — feed `]` then `p`, assert `ResolverOutcome::Action(Action::JumpNextPick)`.
2. `bracket_prev_p_resolves_to_jump_prev_pick` — feed `[` then `p`, assert `ResolverOutcome::Action(Action::JumpPrevPick)`.
3. `bracket_then_unknown_char_is_ignored_and_resets` — feed `]` then `x`, assert `ResolverOutcome::Ignored` and `!resolver.is_pending()`.

## 6. Help overlay

**File:** `src/ui/help.rs`

At help.rs:182-183, just after the two git-change rows, add:

```rust
("] p", "cursor to next picked entry (wraps)"),
("[ p", "cursor to prev picked entry (wraps)"),
```

Keep the family grouped — picks pair immediately follows the git-change pair, before the next section.

## 7. Config DSL aliases (optional but recommended)

**File:** `src/config/dsl.rs`

The DSL already exposes `pick` → `Action::TogglePick` (dsl.rs:150). Add the parallel aliases:

```rust
"next-pick" => Ok(BoundAction::Plain(Action::JumpNextPick)),
"prev-pick" => Ok(BoundAction::Plain(Action::JumpPrevPick)),
```

Add a corresponding `#[test]` that parses both aliases, mirroring the existing `parses_patternpick_arg` style (dsl.rs:253).

## 8. Documentation

### 8a. `FEATURES.md`

In the **Picks and inventory** section (around line 66), append a new bullet group after the existing pick toggle list:

```markdown
- **]p** jump cursor to next picked entry (wraps)
- **[p** jump cursor to previous picked entry (wraps)
```

Mention briefly that this complements `=!` ("focus on picks") — use the prose style of the surrounding bullets.

### 8b. `CHANGELOG.md`

Under `[Unreleased] → Added`, add a single line:

```markdown
- `]p` / `[p` chord jumps cursor to next / previous picked entry in the current listing (wraps; flash on empty). Mirrors `]g` / `[g`.
```

### 8c. `AGENTS.md`

No structural change required. If a future entry documents the bracket family explicitly, picks should appear alongside git-change.

### 8d. `BUGS.md`

No entry needed — this isn't a bug, it's a new feature.

## 9. Validation checklist (run before opening PR)

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test` — all green, including the five new state tests and three new resolver tests
- [ ] Manual smoke test in a directory with mixed picked / unpicked rows:
  - [ ] `t` on rows 1, 3, 7 → `]p` from row 0 lands on 1, then 3, then 7, then wraps to 1
  - [ ] `[p` from row 7 lands on 3, then 1, then wraps to 7
  - [ ] Clear all picks (`^T`) → `]p` flashes "no picks in this directory"
  - [ ] Empty directory → `]p` is a silent no-op (no flash, no error — same shape as `]g`)
  - [ ] Pick a file, then `]p` repeatedly — cursor stays put (single pick) but no flash, since there *is* a pick (matches `]g` behavior with a single change)
  - [ ] Apply `=!` filter, then `]p` — should still work (the filter narrows the listing; picks within the filtered set are traversed)

## 10. Out-of-scope follow-ups (do not bundle)

- `]P` / `[P` for harpooned entries within the current directory — separate issue / thread.
- Pager-side analog (jump-to-next-yanked-line, jump-to-next-search-match) — different state, different surface.
- Cross-directory pick traversal — picks are per-directory; cross-dir traversal would need a different state model.

## 11. Effort estimate

`[S]` — Half a day for an implementer with this plan in hand. The mechanical mirror of an existing, well-tested family makes this a low-risk additive change. Largest time cost is the doc sync (FEATURES + CHANGELOG + help overlay), not the code.

## 12. Done definition

- Issue Tripstack-Corp/spyc#1 closed via PR merge to `main`.
- A Closure entry on this thread referencing the merged PR (per the WC protocol in user CLAUDE.md).
- The next call to `[`/`]` followed by `p` lands the cursor on a picked entry, or flashes the empty-listing message, with no other side effects.

<!-- Entry-ID: 01KRR507PBGAN3G7X9MN91QZFD -->
