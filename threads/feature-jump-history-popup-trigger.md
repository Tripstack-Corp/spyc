# feature-jump-history-popup-trigger — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: feature-jump-history-popup-trigger
Created: 2026-05-15T06:21:35.169139+00:00

---
Entry: Claude Code (caleb) 2026-05-15T06:21:35.169139+00:00
Role: planner
Type: Plan
Title: Design + plan: bind `?` at empty J prompt to the jump-history popup

Spec: planner-architecture

tags: #design #jump #history #spy-parity #discoverability

# Thread intent

This thread documents the design and implementation plan for a small spy-parity feature: bind `?` at the empty `J` prompt to spyc's existing jump-history popup. The investigation that led here is in `caleb-initial-thoughts-and-findings` ("Jump (J) investigation + harpoon vs. marks vs. jump-history"). Read that first if context is missing — this thread starts assuming the conclusion: spyc already has the popup, the data store, and the key handlers; what's missing is the spy-style single-keystroke affordance.

# 1. Motivation

A long-time spy user (`J ?` to see the directory history) hits `J ?` in spyc and gets nothing. The history is there — there are 1000 entries deep across recent sessions on disk — but reaching it requires `J <Esc> <Space>`, a chord sequence with two prerequisites a spy user is unlikely to know:

1. that the `J` prompt is a vi-line-editor (per `src/app/mod.rs:3365-3368`, post-v1.33.0)
2. that `<Space>` while the editor is in Normal mode is overloaded as "open the bigger pager view"

Both are real and intentional spyc design choices (the vi-line-editor promotion bought J Up/Down history nav and inline editing; the Space overload reads better than double-Esc and frees Esc to keep its "cancel" semantic). Neither needs to change. What needs adding is a *second* trigger that matches the spy reflex.

There's already precedent: at the `!` (shell-captured) prompt, `?` on an empty buffer opens the shell history popup (`src/app/mod.rs:3719-3731`). We're proposing the same shape for `J`.

# 2. Scope

In scope:
- A new trigger: `?` typed into an empty `J` prompt opens the existing jump-history popup. No buffer-content change; just an immediate transition.
- Fix two stale code comments that describe the popup as "Esc-triggered" (`src/app/mod.rs:555-560`, `:7849-7853`).
- Update the in-app help overlay so the affordance is discoverable.

Out of scope (explicitly):
- Display ordering convention change (newest-as-1 vs. newest-as-highest-N). spyc's current convention is fine; leave it.
- Merging `jump_history` with `frecency`. They have different jobs (chronological vs. score-ranked) and the popup is the right surface for the chronological view.
- New keybindings inside the popup itself. j/k/Enter/x/q already match what spy users expect.
- Mouse-driven scroll affordances inside the popup. Out of scope for THIS feature; tracked separately in the sibling scrollback entry on `caleb-initial-thoughts-and-findings`.

# 3. Existing surfaces to reuse

Nothing new gets built. We're hooking a new key path into existing code.

| Element | Location | Used as-is |
|---|---|---|
| `State::jump_history` (the data) | `src/state/history.rs`, loaded via `History::load_file("jump_history")` (`src/app/mod.rs:763`) | yes |
| `show_jump_history_popup` (the renderer) | `src/app/mod.rs:7854-7880` | yes |
| Popup key handling (j/k/Enter/x/q/Esc) | `src/app/mod.rs:8398-8468` | yes |
| `pending_jump_history` state field | `src/app/mod.rs:560` | yes |
| Esc-on-empty cancel path (for the *default* `J` behavior — typing `?` will NOT change cancel semantics) | `src/app/mod.rs:3711-3714` (`^C`), editor's Esc-to-Normal | unchanged |

# 4. Design — the new trigger

At `src/app/mod.rs:3719-3731` there's already a `?`-on-empty handler scoped to `PromptKind::ShellCmdCaptured`. The pattern:

```rust
if key.code == KeyCode::Char('?') {
    if let Mode::Prompting(Prompt {
        kind: PromptKind::ShellCmdCaptured,
        ref buffer,
        ..
    }) = self.state.mode
    {
        if buffer.is_empty() {
            self.state.mode = Mode::Normal;
            self.show_history_popup();
            return PostAction::None;
        }
    }
}
```

Add a parallel arm for `PromptKind::Jump` that calls `show_jump_history_popup`. Two implementation choices:

**(a)** Extend the existing guard to a match on both kinds:

```rust
if key.code == KeyCode::Char('?') {
    if let Mode::Prompting(Prompt { ref kind, ref buffer, .. }) = self.state.mode {
        if buffer.is_empty() {
            match kind {
                PromptKind::ShellCmdCaptured => {
                    self.state.mode = Mode::Normal;
                    self.show_history_popup();
                    return PostAction::None;
                }
                PromptKind::Jump => {
                    self.state.mode = Mode::Normal;
                    self.show_jump_history_popup();
                    return PostAction::None;
                }
                _ => {}
            }
        }
    }
}
```

**(b)** Keep the two arms separate (one block per prompt kind). Slightly more code; easier to read in isolation.

Preference: **(a)** — single block, single match, single contract ("`?` on empty buffer of a prompt that supports a history popup opens that popup"). Makes future additions (e.g. `:` command history popup, currently noted as a KNOWN LIMITATION at `src/app/mod.rs:3745-3747`) trivial.

# 5. Edge cases — go through them all

For each, the question is: what's the right behavior, and does the proposed code handle it?

1. **`?` with a non-empty buffer.** Should be a literal `?` character in the path (e.g. `cd /tmp/?` is a valid glob to expand at chdir time, though we don't currently glob-expand jump paths). Behavior: fall through to the editor's normal feed. ✓ The `buffer.is_empty()` guard handles this.

2. **`?` while the editor is in Normal mode and buffer is empty.** The Normal-mode-`?` keybinding in many vi editors is "search backward"; spyc's line editor doesn't define it (per `src/ui/line_edit.rs` — would need to verify; if it's bound to anything else we need to swap order). Decision: the wrapping `?`-on-empty handler runs *before* the editor feed (it's at `:3719` which is above the `editor.feed(key)` at `:3776`), so we get first dibs. If the editor was using `?` for something, we'd be hijacking it on empty-buffer — but empty buffer = no text to search anyway. Safe.

3. **`?` after `<Esc>` (i.e. user is in Normal mode with empty buffer).** Same as case 2 — handler runs before feed, opens the popup. The user got there by `J <Esc> ?` instead of `J ?`. Both should work.

4. **`?` after the user has typed and then deleted back to empty.** Buffer empty, editor presumably in Insert. Handler matches; popup opens. Behavior identical to fresh prompt. ✓

5. **Empty jump_history.** `show_jump_history_popup` already handles this: flashes "jump history is empty" and bails (`src/app/mod.rs:7856-7859`). ✓

6. **Race with another spyc instance writing to the on-disk history.** The snapshot at `:7863` captures `entries().iter().rev().cloned()` into `pending_jump_history`, so subsequent file mutations don't desync the popup. ✓

7. **Path expansion at chdir.** Popup `<Enter>` already calls `crate::paths::expand(path_str)` at `:8414` before `chdir`. Tildes/$VAR survive a roundtrip (since `push` stores the path as-typed and `expand` resolves at jump time). ✓

8. **User wants to *type* `?` as the start of a literal jump path.** Unusual but possible (`cd ?weird-name-with-leading-question`). They'd have to start with anything else (e.g. `./?...`) or use Tab-cycle. Acceptable for a corner case so rare it's never come up in spy either; spy made the same trade.

9. **`?` at the `!` prompt vs. the `J` prompt vs. some future prompt.** The match-on-kind structure makes each prompt's policy explicit. ✓

10. **Stale on-disk history pointing to a directory that no longer exists.** Popup `<Enter>` runs `state.chdir`, which returns `Err` for non-existent dirs; the error is flashed (`src/app/mod.rs:8422`). The entry stays in history (so the user can `x` to remove it). Reasonable — not auto-pruning matches the harpoon module's "the user may have just reverted a deletion" rationale.

11. **Concurrent popup mounted.** Pre-existing pager? Need to check the gate: `show_jump_history_popup` blindly sets `self.pager = Some(view)` at `:7878`. If there's already a pager open (e.g. a captured shell output) it gets dropped silently. Looking at the call site — we'd be invoking from `handle_vi_prompt_key`, which only runs while a prompt is active, and a prompt being active *should* preclude a pager. The `!?` shell path has the same shape (drop pager, mount popup). Safe by symmetry. ✓

# 6. The stale-comment fixes

Two locations to bring into sync with reality. Both currently describe the popup as Esc-triggered; the actual trigger is `<Space>` in Normal mode (and, after this change, `?` on empty too).

**`src/app/mod.rs:555-560`** — current text:

> /// Snapshot of jump-history entries (newest first) for the popup
> /// opened by `Esc` on an empty `J` prompt. While `Some`, an
> /// `Enter` on the active pager chdirs to the entry at the
> /// cursor; `^D` deletes the entry from history and the snapshot.
> /// `None` when no jump-history popup is active.

Proposed replacement:

> /// Snapshot of jump-history entries (newest first) for the popup
> /// opened from the `J` prompt — by `?` on an empty buffer, or by
> /// `<Space>` in the editor's Normal mode. While `Some`, an `Enter`
> /// on the active pager chdirs to the entry at the cursor; `x`
> /// deletes the entry from history and the snapshot. `None` when
> /// no jump-history popup is active.

(Note also: existing comment says `^D` deletes but the actual binding at `:8427` is `x` — that's a separate stale-comment fix even before this feature lands.)

**`src/app/mod.rs:7849-7853`** — current text:

> /// Open a popup listing every entry in `jump_history`, newest at
> /// the top. j/k navigate, Enter chdirs to the cursored path,
> /// ^D deletes the entry from history, q/Esc closes. Triggered by
> /// hitting Esc on an empty `J` prompt -- since there's nothing to
> /// throw away, the cancel turns into "show me my jumps."

Proposed replacement:

> /// Open a popup listing every entry in `jump_history`, newest at
> /// the top. j/k navigate, Enter chdirs to the cursored path,
> /// x deletes the entry from history, q/Esc closes. Triggered
> /// from the `J` prompt by either `?` on an empty buffer (spy
> /// parity — `J?`) or `<Space>` while the line-editor is in
> /// Normal mode (the post-v1.33.0 "bigger pager view" pattern,
> /// shared with `!?`).

# 7. Help-overlay update

`src/ui/help.rs` — find the `J` row in the keymap section and add a parenthetical. Pseudo-diff:

```
- ("J",   "jump to path (prompt)"),
+ ("J",   "jump to path (prompt) — `J?` opens history popup"),
```

And in the "history popup" section (if there is one — verify; if not, add a one-liner under prompts):

```
+ ("? in empty prompt", "open history popup (J: jumps, !: shell)"),
```

# 8. Test plan

This is small and the existing test suite has popup-related coverage; we should add focused tests around the new trigger and prove the stale-comment-driven assumptions of the old behavior didn't have a stray code path.

**Unit / integration tests to add** (test file: existing `src/app/mod.rs` `#[cfg(test)]` block or wherever popup tests live; check `tests/` dir for an integration-level harness):

1. `question_mark_on_empty_jump_prompt_opens_popup` — drive the state machine: `apply(Action::JumpPrompt)`, then send `?` key event. Assert `pending_jump_history.is_some()` and `state.mode == Mode::Normal`.
2. `question_mark_on_non_empty_jump_prompt_is_literal` — same setup, but type `foo` first then `?`. Assert no popup (`pending_jump_history.is_none()`); assert prompt buffer is `foo?`.
3. `question_mark_on_empty_jump_when_history_empty_flashes` — empty `jump_history`; trigger; assert popup NOT opened, status flash contains "empty."
4. `space_in_normal_mode_still_works` — regression guard. Existing path: `J`, `<Esc>`, `<Space>`. Assert popup opens. (Probably already exists; if so, link it.)
5. `question_mark_on_empty_shell_prompt_still_opens_shell_history` — regression guard for the pre-existing `!?` path. We don't want the refactor (option (a)) to break this.

**Manual smoke** (run on the local checkout):
- `cargo build && ./target/debug/spyc` in a populated test dir
- Visit 3-4 directories with `e`/`u`/`J` to seed history
- `J ?` — popup opens
- `j`, `j`, `<Enter>` — chdir to third-newest entry
- `J ?` again — see that previous entry is now top (move-to-end)
- `J ?` then `x` — entry deleted; popup updates
- `J ?` then `q` — popup closes; J prompt re-opens? (verify expected: based on current code at `:8411` it cancels both pager and prompt, returning to Normal mode. The "reopen J after popup close" UX isn't worth adding — too clever; matches `!?` behavior.)

# 9. Risks

- **Tiny risk of typed-`?` regression** at shell prompts that aren't ShellCmdCaptured (e.g. `!` non-captured `ShellCmd`). The current code only matches `ShellCmdCaptured`, so option (a)'s match doesn't accidentally add ShellCmd to the list. Verify when implementing.
- **Risk of breaking the `<Space>` trigger** if the refactor reorders the handler. Mitigated by keeping the new `?` arm at the same location as the old (above the Space block, above editor feed) and adding the regression test (#4 above).
- **Risk of competing with the editor's own `?` handling.** Need to grep `src/ui/line_edit.rs` for any `KeyCode::Char('?')` handler. If there isn't one (likely — spyc's line editor is small), no conflict.

# 10. Implementation plan

Single PR, small. Estimate: 30-60 min including tests.

1. **(read-only)** `grep -rn "KeyCode::Char('?')" src/` to confirm no surprise handlers in `line_edit.rs` or elsewhere.
2. **(read-only)** confirm popup gate at `src/app/mod.rs:7878` (`self.pager = Some(view)`) is safe to invoke during an active prompt — same as `show_history_popup`'s safety story.
3. Edit `src/app/mod.rs:3717-3731` per design option (a) — match on both `Jump` and `ShellCmdCaptured` kinds.
4. Edit `src/app/mod.rs:555-560` — fix stale doc on `pending_jump_history` (Esc → `?` / `<Space>`; `^D` → `x`).
5. Edit `src/app/mod.rs:7849-7853` — fix stale docstring on `show_jump_history_popup` (Esc → `?` / `<Space>`; `^D` → `x`).
6. Edit `src/ui/help.rs` — add `?`-on-empty mention to the `J` row and (optionally) a new row under prompts documenting the empty-prompt-`?` pattern.
7. Add tests per §8.
8. `just check && just test` — confirm green.
9. Manual smoke per §8.
10. Commit with a Decision-style footer pointing to this thread.

# 11. Open questions

- Should the popup's selection of a path *re-open the J prompt with that path pre-filled* (one-stop edit-before-jump) instead of chdir'ing immediately? Spy didn't; spyc currently doesn't. Probably no — `<Enter>` should jump as expected. But noting for future-Caleb if he ever wants in-popup-edit semantics. Could be `e` while cursored on an entry.
- Should `?` also pull frecency-only entries (paths in frecency but not in jump_history) into the popup? Two arguments against: (1) frecency's surface is Tab completion at the J prompt, which is the right place for "score-ranked completion"; (2) mixing two different ranking models in one popup gets confusing. Argument for: it'd be a one-popup view of "all known paths" rather than "places I've typed at J recently." Suggest defer; current jump_history is sufficient for spy parity.
- Should the popup's display ordering be made configurable (spy-style "newest=highest-N" vs. spyc-style "newest=1")? Treating that as a cosmetic Caleb-preference question, not a design decision. Defer unless asked.

# 12. Provenance

- Investigation entry: `caleb-initial-thoughts-and-findings` → "Jump (J) investigation + harpoon vs. marks vs. jump-history" (Entry-ID prior in this branch's thread)
- Existing `!?` precedent: `src/app/mod.rs:3717-3731`
- Popup renderer: `src/app/mod.rs:7849-7880`
- Popup key handling: `src/app/mod.rs:8398-8468`
- Data store: `src/state/history.rs`, instantiated at `src/app/mod.rs:763`

Identity fallback: no `set_agent` tool surfaced in this session; identity asserted via `agent_func`, Role and visible Spec line.

Ball intentionally not flipped to a specific implementer agent — this plan is for Caleb to review/redirect first.

<!-- Entry-ID: 01KRN4R84GPSXW1TMPA6K9GTKF -->
