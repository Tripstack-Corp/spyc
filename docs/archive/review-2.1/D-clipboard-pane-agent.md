# Reviewer D — clipboard, pane, and agent plumbing

Subject: `git diff v2.0.0..HEAD` (HEAD `f8dedae`, `2.1.0-CURRENT`) restricted to
`src/clipboard.rs`, `src/app/clipboard.rs`, `src/pane/`, `src/agent/`,
`src/app/codex_pin.rs`, `src/state/codex_transcript.rs`, `src/app/session.rs`,
`src/state/sessions/`. Read-only review; no repo changes were made.

## Verdict

The DECCKM fix (#259) and the codex pin-window fix (#250) are both correct, well
tested, and land exactly where the commit messages claim — the encoder and the key
trace genuinely read one value, and the 32-row byte table behind
`normal_mode_encoding_is_unchanged` is a literal-expectation regression table, not a
tautology. The new clipboard work is where the problems are: `deliver_clipboard` /
`copy_osc52` / `[clipboard] via` all arrived in one commit (#234) and were wired into
only *some* of the yank verbs, so `yp` / `ya` / `^a u` / the image-overlay `Y` still
write the SSH server's clipboard and still ignore `[clipboard].command` — inside the
same `y` chord where `yf` and `yP` do the right thing. Two blocking-on-the-loop holes
remain in that same file (an unbounded `Command::output()` on the paste path, and a
`write_all` that the 150 ms reap budget does not cover). On the session side the round
trip is honest for everything it names, but a **live codex tab throws away the exact
rollout uuid spyc already pinned** and restores via `codex resume --last`, which
re-opens the #230 wrong-conversation class in the save→restore direction. Two charter
premises are wrong on HEAD and are corrected below.

---

## Clipboard dispatch-order trace

The charter states the order as `override → Wayland gated on $WAYLAND_DISPLAY →
xclip/xsel → pbcopy → OSC 52`. That is not the shape on HEAD. Actual, from
`App::deliver_clipboard` (`src/app/clipboard.rs:435`):

```
Effect::CopyToClipboard / CopyToPagerClipboard   (src/app/effect.rs:531, :543)
  └─ App::deliver_clipboard                      (src/app/clipboard.rs:435)
      │
      ├─1. clipboard::resolve_override(config.clipboard.command)   src/clipboard.rs:335
      │      $SPYC_CLIPBOARD (non-empty)  →  else  [clipboard].command (non-empty)
      │      hit ⇒ copy_via_user_command  (src/clipboard.rs:349)  ── EXCLUSIVE, returns
      │
      ├─2. clipboard_delivery(via, is_ssh) -> (local, osc52)       src/app/clipboard.rs:399
      │      Auto + ssh  = (false, true)     Auto + !ssh = (true, false)
      │      System      = (true, false)     Osc52       = (false, true)
      │      Both        = (true, true)
      │
      ├─3. if osc52:  clipboard::copy_osc52     ── FIRST, before the local helper
      │      src/clipboard.rs:288 → osc52_sequence (base64, ≤74_994, tmux DCS wrap)
      │      → io::stdout().write_all + flush
      │
      ├─4. if local:  clipboard::copy           src/clipboard.rs:362
      │      4a. #[cfg(test)] CLIPBOARD_OVERRIDE → /bin/sh <stub>
      │      4b. resolve_override(None)  ── $SPYC_CLIPBOARD only, NOT the config key
      │            (dead when reached via deliver_clipboard; LIVE for the direct
      │             callers in finding D1)
      │      4c. copy_impl, cfg-selected — not a cascade across platforms:
      │            macOS   → spawn_and_pipe("pbcopy")
      │            Linux   → if $WAYLAND_DISPLAY  → wl-copy
      │                      if $DISPLAY          → xclip -selection clipboard
      │                                           → xsel -ib
      │                      else ErrorKind::NotFound "install xclip, xsel, or wl-copy"
      │            other   → ErrorKind::Unsupported
      │
      └─5. ok if ANY enabled mechanism returned Ok; else errs.join("; ")
```

`spawn_and_pipe` (`src/clipboard.rs:459`) is the shared exec leaf:
`spawn` → `stdin.write_all(text)` (**result captured, never `?`-returned** — a bare `?`
would drop the child unreaped) → poll `try_wait()` every 10 ms until
`HELPER_REAP_BUDGET` (150 ms, `src/clipboard.rs:452`) → if still running, treat as
success and `thread::spawn(move || child.wait())` to detach a reaper → else a non-zero
exit becomes `io::Error::other` (deliberately **not** `NotFound`, so the Linux cascade
stops on a present-but-broken helper) → else surface the deferred write error.

Read direction (`clipboard::paste`, `src/clipboard.rs:118`), used only by
`Effect::PasteFromClipboard` (middle-click):

```
macOS  → capture("pbpaste")
Linux  → if $WAYLAND_DISPLAY → wl-paste -n
         → xclip -selection clipboard -o
         → xsel -ob                         (NotFound falls through; any other error returns)
other  → ErrorKind::Unsupported
capture() = Command::output()  ── no budget, no timeout, blocking (finding D2)
```

Notable asymmetry: the write path gates X11 helpers on `$DISPLAY`; the read path does
not. Harmless in practice (a display-less `xclip -o` exits fast with a real message),
but the module header (`src/clipboard.rs:7-10`) documents neither the `$DISPLAY` gate
nor OSC 52 (finding D8).

---

## Session save vs. restore — field table

`Session` (`src/state/sessions/mod.rs:100`), written by
`App::build_session_snapshot` (`src/app/session.rs:85`), read by
`App::restore_session` (`src/app/session.rs:420`).

| field | written from | read on restore | verdict |
|---|---|---|---|
| `id` | `state.session_id` (stable per process) | `state.session_id = Some(id)` (`:453`) | round-trips |
| `saved_at` | `sysinfo::format_now()` | picker display only | display-only, intentional |
| `epoch_secs` | `sysinfo::epoch_secs()` | picker sort/age only | display-only, intentional |
| `cwd` | `project_home` ?? `start_dir` | `chdir` (`:425`), or `Effect::ChangeDir` when it is an archive path (`:435`), else error | round-trips, incl. archive mounts (tested, `harness_tests/archive.rs:1626`) |
| `tabs` | live `pane_tabs` | respawned in order (`:459-519`) | see `SavedTab` below |
| `active_tab` | `PaneTabs::active_index` | `tabs.switch_to` (clamps) (`:523`) | round-trips |
| `pane_height_pct` | `state.pane.pane_height_pct` | `:456` | round-trips |
| `pane_focused` | `state.pane_focused()` | `state.focus` (`:525`), re-applied in `restore_vsplit` (`:581`) | round-trips |
| `name` | `state.session_name` | `:448` (kept if saved value empty) | round-trips |
| `project_home` | `state.project_home` | `:454`, filtered `is_dir()` | round-trips; a project_home inside an archive is silently dropped |
| `vsplit` | `state.vsplit` + `view.right_pager` / `state.right` | `restore_vsplit` (`:552`) | mostly; see finding D7 |
| `scope_claims` | `state.scope_registry` | `:541`, verbatim | round-trips |

`SavedTab` (`src/state/sessions/mod.rs:51`):

| field | written from | read on restore | verdict |
|---|---|---|---|
| `command` | `profile.command_without_resume(info.command)` | `reconstruct_restore` (`:477`) | round-trips |
| `label` | `strip_exit_suffix(info.label)` | `entry.info.label` (`:498`) | round-trips |
| `cwd` | `info.cwd` | spawn cwd, falls back to `session.cwd` if not a dir (`:460`) | round-trips |
| `agent_kind` | `profile.kind()` | selects the profile for `reconstruct_restore` (`:477`) | round-trips; unknown names degrade to `Other` by the hand-written `Deserialize` (`:34`) |
| `agent_session_id` | `validate_live_session_id(live_session_id)` else `resolve_resume_target` (`:122-133`) | baked into the codex/agy command, or armed as `ClaudeStdin` (`:506`) | round-trips for claude/agy; **not populated for a live codex tab** — finding D5 |
| `agent_session_name` | resolver / `find_claude_session_name` | **never read by `restore_session`** — picker label + exit summary only | saved-not-restored; benign (there is no runtime field for it), undocumented as such |
| `claim_owner` | `info.claim_owner` | `:503`, only when non-empty | round-trips |

`SavedVsplit` (`src/state/sessions/mod.rs:133`): `width_pct` (clamped 20..80),
`full_height`, `focus_right`, `preview_path` (re-loaded if it `exists()`), `right_cwd`
(reopened if `is_dir()`) — all read in `restore_vsplit`.

**Live pane state deliberately NOT persisted** (all of it new or reworked since 2.0):
`TabInfo.id` (documented "not persisted"), `codex_session_id`, `live_session_id`,
`activity` / `notified` / `reported` / `scrape_status` / `scrape_dirty`,
`suspended`, `last_output_at`, `pane.pending_images` (paste-capture ring),
`pane.last_pane_prompt`, `pane.pane_prompt_buf`, per-column `picks` / `cursor` /
`temp_filter`. All of these are process-lifetime facts and dropping them is right;
only `codex_session_id` (D5) and `suspended` (D11) are worth a note.

**Clipboard config is not session state.** `[clipboard] via` / `command` live in
`.spycrc.toml` and are re-read from config on every launch — there is nothing to
round-trip. The charter's inclusion of "clipboard config" under session restore is a
category error, not a gap.

---

## Findings (severity-ordered)

### D1 — `high` — `src/app/effect.rs:610`, `src/app/quick_select.rs:145`, `src/app/pager_handler/image.rs:87`

**Three yank verbs bypass `deliver_clipboard`, so OSC 52 and `[clipboard].command`
do not apply to them.**

`deliver_clipboard`, `clipboard_delivery`, `copy_osc52` and the `[clipboard]` config
block were all introduced by `84d2f28` (#234) inside this diff window
(`git log -S copy_osc52` returns exactly that commit). That commit routed
`Effect::CopyToClipboard` / `Effect::CopyToPagerClipboard` through the new delivery but
left three pre-existing call sites on the raw `crate::clipboard::copy` leaf:

- `src/app/effect.rs:610` — `PaneTextSink::Clipboard`, i.e. **`yp`** (yank visible pane
  output, `src/app/actions.rs:171`) and **`ya`** (yank pane scrollback,
  `src/app/actions.rs:185`).
- `src/app/quick_select.rs:145` — `yank_quick_select`, the lowercase **`^a u`** Quick
  Select yank.
- `src/app/pager_handler/image.rs:87` — the image overlay's **`Y`** (yank a diagram's
  mermaid source / a file's path).

Consequences, all silent:

1. **Over SSH these four verbs write the server's clipboard and flash "yanked".** That
   is verbatim the failure `clipboard_delivery`'s own doc comment
   (`src/app/clipboard.rs:396-398`) exists to prevent: "`pbcopy`/`xclip` set the
   clipboard of the machine spyc runs on, which over SSH is the *server* — text the
   user can never paste." `yf` and `yP` (same `y` chord) do the right thing; `yp` and
   `ya` do not.
2. **`[clipboard].command` is not honored** by them. `copy()` calls
   `resolve_override(None)` (`src/clipboard.rs:378`), which reads only
   `$SPYC_CLIPBOARD` — by design, since `clipboard.rs` is a leaf with no config access
   (`src/clipboard.rs:325-334`). So `$SPYC_CLIPBOARD` works on all six verbs but the
   config key works on only two, directly contradicting `CONFIGURATION.md:127`
   ("`command` overrides everything above: when set, spyc runs this exact command").
3. The mouse drag-select copies (`src/app/mouse/selection.rs:194`, `:268`, `:390`) —
   new in the same PR — emit `Effect::CopyToClipboard` and *are* covered, so a user
   drag-selecting pane text gets OSC 52 while `^a y` on the same pane does not.

No test asserts which call sites use `deliver_clipboard`; `delivery_tests`
(`src/app/clipboard.rs:468`) only covers the pure `clipboard_delivery` table.

A fix needs every clipboard write to reach `deliver_clipboard` — most cleanly by
converting the three sites to `Effect::CopyToClipboard` / `Effect::CopyToPagerClipboard`
(the image overlay already needs its flash in `iv.flash`, so it may want a third
`ClipMsg` sink rather than the status bar), plus a test that pins "no production caller
of `crate::clipboard::copy` outside `deliver_clipboard`" — a source-scan guard in the
`no_subprocess_git_in_production` style would hold it.

### D2 — `high` — `src/clipboard.rs:172-178`, reached from `src/app/effect.rs:722`

**`Effect::PasteFromClipboard` blocks the event loop with no bound.**

`capture()` uses `Command::output()`, which waits for the helper to exit. It runs
inline in `run_effects` on the loop thread. There is no deadline, no `try_wait` poll,
and no off-thread hop — in explicit contrast to the write direction, which grew
`HELPER_REAP_BUDGET` for exactly this reason and documents it at length
(`src/clipboard.rs:430-451`). A wedged X selection owner (`xclip -o` waits for the
selection transfer), a stalled Wayland compositor, or a `pbpaste` behind a hung
pasteboard server freezes spyc completely — the input thread keeps reading, but nothing
is dispatched and no frame is drawn, with no key that can break out.

Middle-click paste is new in this window (`dbffd29`, #214), so this is in-scope code,
not inherited.

There is also no size bound: `capture` reads the whole clipboard into a `String` and
`handle_paste` (`src/app/key_dispatch/mod.rs:547`) does not cap it, so a large X11
selection is an unbounded allocation on the loop.

A fix needs the read moved to the `graveyard_ops` template (Effect → detached worker →
`Runtime` slot → payloadless `Message` → pre-recv drain), the way `read_image` already
is (`src/clipboard.rs:232`, `src/app/image_ops.rs:250`) — the doc comment on
`read_image` already argues the case; `paste()` just never got it.

### D3 — `medium` — `src/clipboard.rs:474-477`

**The reap budget bounds the wait, not the write — `spawn_and_pipe` can still block
forever.**

```rust
let write_result = match child.stdin.take() {
    Some(mut stdin) => stdin.write_all(text.as_bytes()),
    None => Ok(()),
};
```

This runs *before* the deadline loop. If the helper starts, never reads stdin, and
stays alive, `write_all` blocks on a full pipe indefinitely — on the event-loop thread.
The existing test `copy_reaps_child_and_errors_when_helper_ignores_large_stdin`
(`src/clipboard.rs:638`) covers the adjacent case where such a helper *exits* (the
reader disappears → EPIPE → error), which is why the hole is easy to miss: the
never-reads-**and**-stays-alive shape is untested.

The realistic trigger is the new user-command tier (`ae2789d`, #261): `[clipboard]
command` / `$SPYC_CLIPBOARD` runs an arbitrary binary verbatim, and nothing constrains
it to drain stdin. spyc's own helpers all read stdin first, so this is a
user-configuration hazard rather than a default-path one — but the whole point of the
budget is that a clipboard write cannot hang the UI, and this path escapes it.

A fix needs the write to be bounded too (non-blocking write with the same deadline, or
— better, and what the code's own comment at `src/clipboard.rs:450-451` recommends —
the whole `spawn_and_pipe` moved off-thread, where no budget is needed at all).

### D4 — `medium` — `src/clipboard.rs:452`, `:483-491`

**Every yank under `xclip`/`xsel` stalls the loop for the full 150 ms budget.**

`xclip`/`xsel` legitimately persist after a successful copy to keep serving the X11
selection, so `try_wait()` never returns `Some` and the poll loop runs its whole
`HELPER_REAP_BUDGET`. The file's own comment states this plainly ("this budget is paid
on the event-loop thread **every** time an X11 helper persists — i.e. on every single
yank under `xclip`/`xsel`, which is the common case on that platform, not an edge
one"). 150 ms of a fully blocked loop on every `yf` is a perceptible hitch on the
platform where most non-macOS users are.

I am recording this rather than accepting the comment as a waiver because the comment
also names the correct fix ("moving the write off-thread entirely, where no budget is
needed") and AGENTS.md's own rule is that known blocking IO on the loop gets moved
off-thread during foundation work, not deferred behind a constant. Same fix as D2/D3.

Second, quieter cost of the short budget, also self-documented: a helper that launches
and then fails *slower* than 150 ms is reported as success, so the user sees "yanked"
with nothing on the clipboard.

### D5 — `high` — `src/app/session.rs:122-133`; `src/pane/tabs.rs:256`

**Save discards the exact codex rollout uuid spyc already pinned, so restore falls back
to `codex resume --last`.**

`build_session_snapshot` resolves the id to persist from `info.live_session_id` +
`AgentProfile::validate_live_session_id`, else `resolve_resume_target`. For codex:

- `live_session_id` is never set (`src/pane/tabs.rs:186-188` says so explicitly: "Codex
  pins to `codex_session_id` instead").
- `CodexProfile` does not override `validate_live_session_id`, so the default `None`
  (`src/agent/mod.rs:179`) applies.
- `CodexProfile::resolve_resume_target` (`src/agent/mod.rs:564`) reads only the **exit
  banner** out of `pane.recent_lines(200)`. A tab that is still running at quit has
  never printed one.

So a live codex tab saves `agent_session_id: None`, and
`CodexProfile::reconstruct_restore` (`src/agent/mod.rs:583`) spawns
`codex resume --last`. Meanwhile `info.codex_session_id` holds the exact uuid that
`app::codex_pin` claimed for that tab, and `TabInfo::pinned_session_id()`
(`src/pane/tabs.rs:256`) exists precisely to read "the codex rollout claim or a
reported/resumed id" — it is used by the transcript view
(`src/app/pane_scroll.rs:173`), the image gallery (`src/app/image_gallery.rs:261`) and
the status suffix (`src/app/agent_status.rs:264`), but **not** by save.

Downstream, a `codex resume --last` tab is `is_resume_without_id == true`
(`src/state/codex_transcript.rs:227`), which puts both resolvers on their
mtime-ranked branches — `assign_codex_sessions`'s `.max_by_key(mtime_secs)`
(`src/app/codex_pin.rs:91`) and `pick_best_rollout`'s `resuming` arm
(`src/state/codex_transcript.rs:174`). Their own test
`the_same_candidates_invert_when_the_pane_is_resuming`
(`src/state/codex_transcript.rs:621`) shows the consequence: the busiest rollout in the
cwd wins, including another spyc instance's. That is the #230 failure mode, reached
from the save side.

The refactor that introduced `live_session_id` in this window
(`7bce4c4` #201 / `e2d8c5e` #202) named the gap in a doc comment and did not close it.

A fix needs the codex save path to prefer `info.codex_session_id` (either via a
`CodexProfile::validate_live_session_id` that checks the rollout file exists, or by
reading `pinned_session_id()` at the `src/app/session.rs:122` match) so a restored
codex tab spawns `codex resume <uuid>` — which `open_pane_tab_in` then re-pins exactly
at launch (`src/app/pane_tabs.rs:173`).

### D6 — `medium` — `CONFIGURATION.md:119-120`; `src/clipboard.rs:264-271`, `:281`

**The documented OSC-52 oversize fallback does not exist on the path that matters.**

`CONFIGURATION.md:119` promises "A selection too large for the escape falls back to the
helper rather than risk a terminal truncating it silently." Two code comments repeat
it: `OSC52_MAX_BASE64`'s ("past this we fall back to the local helper rather than
gamble", `src/clipboard.rs:270`) and `copy_osc52`'s ("`Err` when the payload is too
large to send safely; the caller falls back", `src/clipboard.rs:281`).

`deliver_clipboard` has no fallback. Under `via = "auto"` **over SSH** — the whole
reason OSC 52 exists — `clipboard_delivery` returns `(local: false, osc52: true)`, so
an oversized selection produces `Err("selection too large for OSC 52 …")` and nothing
is copied anywhere. Same under `via = "osc52"`. The claim is true only under
`via = "both"`, which is not the default.

The behavior is arguably right (a helper writing the server's clipboard is not a
useful fallback over SSH, and the user *is* told — see "verified correct" below). The
defect is that three places state a fallback that no code performs, which is exactly
what AGENTS.md's "comments state what IS" rule and the `comments_carry_no_reasoning_leakage`
guard exist to prevent.

A fix needs either the comments and `CONFIGURATION.md` corrected to describe the real
behavior ("refused with a message; no fallback unless `via = "both"`"), or a genuine
fallback added under `auto`.

### D7 — `low` — `src/app/session.rs:568`; `src/app/session.rs:211`

**A second commander whose cwd is inside an archive silently loses the whole split on
restore.**

`build_session_snapshot` saves `right_cwd: state.right.listing.dir` unconditionally
(`:211`), so a column browsing `…/pkg.zip/src` is persisted as that path.
`restore_vsplit` filters it with `.filter(|p| p.is_dir())` (`:568`), which a mount path
fails. Control falls to the preview branch, `preview_path` is `None`, `right_pager`
stays `None`, and the guard at `:600` sets `state.vsplit = None` — the split vanishes
with no message.

This is asymmetric with the left column, which got exactly this case handled in the
same window (`7f0b268`, #317): `restore_session:430` detects `archive_ancestor_of` and
emits an `Effect::ChangeDir` for the effect screen to mount, and it is tested
(`src/app/harness_tests/archive.rs:1626`). `restore_vsplit`'s tests
(`src/app/harness_tests/second_commander.rs:193`) cover the missing-preview-file and
width-clamp cases only.

A fix needs `restore_vsplit` to route an archive-ancestor `right_cwd` the way
`restore_session` routes the session cwd, or — if that is out of scope for a split —
to at least flash why the split was dropped.

### D8 — `low` — `src/clipboard.rs:1-14`

**Module header is stale in three ways.**

It documents only `copy`/`paste`, omits the `$DISPLAY` gate that `copy_impl` actually
applies to `xclip`/`xsel` (`:408`), omits OSC 52 entirely, and asserts "No external
crate dependency — mirrors spyc's in-tree fork-exec pattern" while the file now depends
on `arboard` (`:201`, `:233`), `image` (`:197`, `:246`) and `base64` (`:306`). The file
grew from 249 lines at v2.0.0 to 891; the header did not move.

### D9 — `low` — `src/app/codex_pin.rs:173`, `:187`

`*pending.lock().unwrap()` and `self.runtime.codex_pin_pending.lock().unwrap()` carry no
invariant comment. AGENTS.md permits `.unwrap()` in production **with a comment stating
the invariant**; `src/app/` outside `state/` and `render/` is not under
`deny(clippy::unwrap_used)`, so this compiles, but it is the convention the house style
names explicitly. (A poisoned mutex here is only reachable if the scan worker panicked
between `lock()` and the store, which is worth one line either way.)

### D10 — `informational` — `src/app/clipboard.rs:445-459`

Under `via = "both"`, an oversized selection has its OSC-52 error discarded when the
local helper succeeds, and the user sees a plain "yanked". Over SSH with `both` that
means the text reached the server only, reported as unqualified success. Documented as
deliberate ("Succeeds if ANY enabled mechanism succeeded") and defensible; recording it
because `both` + SSH is the configuration where the swallowed error is the one the user
needed.

### D11 — `informational` — `src/pane/tabs.rs:133`; `src/state/sessions/mod.rs:51`

`TabInfo.suspended` (💤, `1eaf185` #306) is not persisted, so a `^z`-suspended tab
restores as a freshly spawned, running child. This is almost certainly correct — a
`SIGSTOP` is a property of a process that no longer exists — but neither the field doc
nor `SavedTab` says so, and no test pins it. Same shape as
`a_restored_pane_starts_in_normal_cursor_mode` (`src/pane/input.rs:717`), which *does*
pin the equivalent DECCKM decision; one sentence on the field would match.

---

## Premises checked

| premise (from charter / orchestrator notes) | verdict on HEAD |
|---|---|
| "`clipboard.rs` (new)" | **Wrong.** The file existed at v2.0.0 at 249 lines (`git log --diff-filter=A` → `f2b1f5b`, a pre-2.0 PR). It grew to 891. What is genuinely new in the window is `paste`/`capture` (#214), `copy_osc52`/`osc52_sequence` (#234), `read_image` (#304), `resolve_override`/`copy_via_user_command`/`HELPER_REAP_BUDGET` (#261). `copy_image` predates v2.0.0 (`97aae3d`). |
| "dispatch order: override → Wayland → xclip/xsel → pbcopy → OSC 52" | **Wrong order.** OSC 52 runs **before** the local helper (`src/app/clipboard.rs:445`), deliberately, so a knowable size failure beats the helper's silent success-on-the-wrong-machine. `pbcopy` and the Linux helpers are `cfg(target_os)`-exclusive, not sequential steps. There is also an undocumented `$DISPLAY` gate on `xclip`/`xsel`. Full trace above. |
| "the xclip child detached, never waited on" | **Wrong.** HEAD does a bounded wait — `try_wait()` polled at 10 ms up to 150 ms (`src/clipboard.rs:483-491`) — and only *then* detaches a reaper thread. The detach is real (test `spawn_and_pipe_detaches_a_persisting_child_instead_of_blocking`, `:778`), but the 150 ms is paid on the loop first (finding D4). |
| "OSC 52 escape written through the owned terminal seam rather than raw stdout" | **Not the house pattern, and not a defect.** `copy_osc52` writes to `io::stdout()` (`src/clipboard.rs:290`), identically to `term_title::emit` (`src/term_title.rs:34`) and `notifications`' OSC-9/BEL writers (`src/notifications.rs:44`, `:55`). Ordering is safe: the ratatui backend is `CrosstermBackend<io::Stdout>` (`src/lib.rs:404`), so both writers share the one global buffered handle, and `run_effects` runs between draws on the same thread. No finding. |
| "OSC 52 size limit enforced with a user-visible warning" | **True where it counts.** Refusal is in `osc52_sequence` (`:307`) and surfaces as `yank failed: selection too large for OSC 52 (N bytes base64, limit 74994)` whenever OSC 52 is the only enabled mechanism. Swallowed under `via = "both"` when the helper succeeds (D10). The *fallback* half of the claim is false (D6). |
| "DECCKM: encoder consults the same state the trace logs — one read, not two" | **True.** `Pane::send_key` (`src/pane/mod.rs:319-324`) takes `let app_cursor = self.application_cursor();` once and feeds both `encode_key` and the `app_cursor=` trace field, with a comment saying why. |
| "modified arrows unaffected; default-mode bytes unchanged; the snapshot guard is meaningful, not a tautology" | **All true.** `push_cursor_key` (`src/pane/input.rs:137`) takes the SS3 branch only when `modifier_param` is `None`. `modified_cursor_keys_ignore_decckm` (`:601`) checks 6 keys × 5 modifier sets. `normal_mode_encoding_is_unchanged` (`:663`) is a 32-row table of **literal expected byte strings**, not a self-comparison — it would catch any encoder drift. |
| "C3: mtime still strictly a liveness filter, never a ranking key" | **False on HEAD, deliberately.** mtime is the **ranking key** on both `resuming` branches: `assign_codex_sessions` uses `.max_by_key(\|r\| r.mtime_secs)` (`src/app/codex_pin.rs:91`) and `pick_best_rollout` orders by `c.mtime > *m` when `resuming` (`src/state/codex_transcript.rs:174`). It is a *filter* only on the fresh-pane branch (`r.mtime_secs >= tab.spawn`, `:88`; `c.mtime + MTIME_SKEW_SECS < spawn` → skip, `:165`). The design is documented at length (`src/state/codex_transcript.rs:137-154`) and tested from both directions (`the_same_candidates_invert_when_the_pane_is_resuming`, `a_fresh_tab_never_adopts_a_live_old_rollout`), because a resumed rollout's `session_meta` is frozen and mtime is the only live signal. So: correct as designed, but the charter's invariant does not hold, and D5 is the case where it bites. |
| "`tests/pane_roundtrip.rs` may be the relevant session test" | **Wrong file.** It is a 110-line pty/vt100 echo test (`cat_roundtrip_renders_input_in_vt100_screen`), unrelated to session restore. Session-restore coverage lives in `src/app/harness_tests/archive.rs:1626`/`:1655` and `src/app/harness_tests/second_commander.rs:174-222`; `restore_session` itself has no direct unit test outside the archive cases. |
| "`^z` suspend semantics and `shell::pane_invocation` (SPYC-TRAP `pane-shell-rc-double-source`) still hold" | **True.** `pane_suspend_key_action` (`src/app/key_dispatch/mod.rs:82`) still gates on `shell::command_is_shell`, the trap anchor resolves (`src/shell/mod.rs:52` ↔ `ARCHITECTURE.md:366`), and `pty_host.rs` now delegates the whole invocation policy to the one pure fn (`src/pane/pty_host.rs:202`) instead of building `exec …` inline. |

---

## Verified correct

Spot-checks that found nothing to report:

- **DECCKM encoding (#259).** SS3 (`ESC O X`) only for the six unmodified cursor keys;
  parameterized CSI (`ESC [ 1 ; N X`) for every modified one, in both modes; tilde keys
  and F1–F4 untouched (`src/pane/input.rs:137-172`, tests `:569`, `:601`, `:630`,
  `:663`). Every non-test `encode_key` caller threads a real mode or documents why it
  passes `false`: `Pane::send_key` reads the child's (`src/pane/mod.rs:319`),
  `mouse/scroll.rs::repeat_key_effect` takes it from `tabs.active().application_cursor()`
  (`src/app/mouse/scroll.rs:45`, `:89`, `:179-186`), and the `!` capture path states the
  invariant ("a capture is a bare `PtyHost` with no vt100 emulator, so no DECCKM state
  exists to honor", `src/app/key_dispatch/mod.rs:454-457`).

- **`osc52_sequence` (#234).** Encoding is the escaping — the payload is base64, whose
  alphabet excludes ESC and BEL, so no sanitizer is needed and none is written; the test
  `control_chars_in_the_text_cannot_terminate_the_sequence` (`src/clipboard.rs:838`)
  feeds it a hostile string containing both terminators and asserts exactly one `\x07`
  survives. tmux DCS passthrough doubles inner ESCs (`:319`, test `:823`); the
  just-under-cap boundary is tested from both sides (`:874`, `:884`).

- **`spawn_and_pipe`'s zombie contract.** The deferred `write_result` (`:474-477`,
  `:498`, `:521`) is the fix for the bug a bare `?` would reintroduce, and it is stated
  in a comment at the site; a non-zero exit deliberately becomes `ErrorKind::Other` so
  the Linux cascade stops on a present-but-broken helper, asserted negatively
  (`assert_ne!(err.kind(), NotFound)`, `:629`). The `retry_text_busy` ETXTBSY harness
  (`:565`, added by `78d1681` #325) fixes a genuine fixture race, not the code under
  test, and says so.

- **`AgentKind`'s hand-written `Deserialize`** (`src/state/sessions/mod.rs:34-48`).
  An unrecognized name degrades to `Other` rather than failing the parse, because
  `load_sessions` drops any file it cannot deserialize — so retiring the `gemini`
  variant in this window would otherwise have silently deleted every restore point that
  mentioned it. Correct, and the reasoning is recorded where it is needed.

- **`src/agent/detect_rules.rs`.** `Matcher::All` is a conjunction, and the agy rule
  requires both of the dialog's own lines (`src/agent/mod.rs:634-639`) precisely because
  either alone is prose an agent writes readily — asserted negatively at
  `src/agent/mod.rs:901-902`. The vacuous `All(&[])` case is documented *and* tested
  (`src/agent/detect_rules.rs:136`), which is the right way to handle a degenerate
  total function.

- **`PaneTabs::replace_at`** (`src/pane/tabs.rs:576`) now `mem::replace`s and calls
  `shutdown_detached` on the old entry — dropping the `TabEntry` alone would have left
  `PtyHost::Drop`'s `SIGKILL` to reach only the immediate child, orphaning grandchildren.
  Correct fix for `^a R` restart-in-place (#269).

- **Archive browsing position round-trips for the focused column** (#317). The saved
  cwd inside a mount is detected by `archive_ancestor_of` and handed to the effect
  screen as an `Effect::ChangeDir`, which mounts on the way
  (`src/app/session.rs:430-440`); tested end-to-end
  (`src/app/harness_tests/archive.rs:1626`), with the negative case (directory genuinely
  gone) tested too (`:1655`).

- **`codex_pin_window_open`** (`src/app/codex_pin.rs:107`) — the actual #230 fix. The
  window runs from `last_output_at` with a fallback to `spawn_at`, so a codex tab you
  read for a minute before typing still gets pinned, and it still quiesces on two
  independent conditions. The test (`:433`) exercises all three transitions and uses
  `checked_sub` against monotonic-clock origin rather than assuming `Instant` arithmetic
  is safe.
